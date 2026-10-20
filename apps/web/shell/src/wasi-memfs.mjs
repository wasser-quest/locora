// A WASI preview 1 file system that lives in memory. The browser build keeps the core's folder
// here and saves what changed to IndexedDB (see storage.mjs). It runs in the browser and in Node,
// so the core's tests use the same code as the page.
//
// Paths are relative to the one preopened folder. Changes are collected in `takeChanges()`: files
// written or created since the last call, and files removed.

export const ERRNO = {
  SUCCESS: 0, ACCES: 2, BADF: 8, EXIST: 20, INVAL: 28, ISDIR: 31, NOENT: 44, NOSYS: 52,
  NOTDIR: 54, NOTEMPTY: 55, NOTSUP: 58, NOTCAPABLE: 76,
};
const FILETYPE = { CHARACTER_DEVICE: 2, DIRECTORY: 3, REGULAR: 4 };
const OFLAGS = { CREAT: 1, DIRECTORY: 2, EXCL: 4, TRUNC: 8 };
const FDFLAGS_APPEND = 1;
const WHENCE = { SET: 0, CUR: 1, END: 2 };
const PRESTAT_DIR = 0;

class WasiError extends Error {
  constructor(errno) {
    super(`WASI errno ${errno}`);
    this.errno = errno;
  }
}

class FileNode {
  constructor() {
    this.buf = new Uint8Array(0);
    this.size = 0;
  }

  reserve(capacity) {
    if (capacity <= this.buf.length) return;
    const next = new Uint8Array(Math.max(capacity, this.buf.length * 2, 4096));
    next.set(this.buf.subarray(0, this.size));
    this.buf = next;
  }

  write(position, bytes) {
    this.reserve(position + bytes.length);
    if (position > this.size) this.buf.fill(0, this.size, position);
    this.buf.set(bytes, position);
    this.size = Math.max(this.size, position + bytes.length);
  }

  read(position, length) {
    if (position >= this.size) return new Uint8Array(0);
    return this.buf.subarray(position, Math.min(this.size, position + length));
  }

  truncate(size) {
    this.reserve(size);
    if (size > this.size) this.buf.fill(0, this.size, size);
    this.size = size;
  }

  bytes() {
    return this.buf.slice(0, this.size);
  }
}

class DirNode {
  constructor() {
    this.entries = new Map();
  }
}

/** The file system: one tree below the preopened folder, plus the open file descriptors. */
export class MemFs {
  constructor(folderName = '/locora') {
    this.folderName = folderName;
    this.root = new DirNode();
    this.fds = new Map([[3, { node: this.root, path: '', position: 0, append: false }]]);
    this.nextFd = 4;
    this.dirty = new Set();
    this.removed = new Set();
    this.memory = null;
    this.stdout = (text) => console.log(text);
    this.stderr = (text) => console.error(text);
  }

  /** Puts files that were saved earlier into the tree (they are not changes). */
  load(entries) {
    for (const [path, bytes] of entries) {
      const parts = path.split('/').filter(Boolean);
      const name = parts.pop();
      let dir = this.root;
      for (const part of parts) {
        if (!dir.entries.has(part)) dir.entries.set(part, new DirNode());
        dir = dir.entries.get(part);
      }
      const file = new FileNode();
      file.write(0, bytes);
      dir.entries.set(name, file);
    }
  }

  /** Every file as [path, bytes], e.g. to inspect the folder in tests. */
  files() {
    const out = [];
    const walk = (dir, prefix) => {
      for (const [name, node] of dir.entries) {
        const path = prefix ? `${prefix}/${name}` : name;
        if (node instanceof DirNode) walk(node, path);
        else out.push([path, node.bytes()]);
      }
    };
    walk(this.root, '');
    return out;
  }

  /** Files written or created, and files removed, since the last call. */
  takeChanges() {
    const writes = [];
    for (const path of this.dirty) {
      const node = this.lookupPath(path);
      if (node instanceof FileNode) writes.push([path, node.bytes()]);
    }
    const removes = [...this.removed];
    this.dirty.clear();
    this.removed.clear();
    return { writes, removes };
  }

  /** The bytes of a file by its path from the preopened folder, or undefined. */
  readFile(path) {
    const node = this.lookupPath(path);
    return node instanceof FileNode ? node.bytes() : undefined;
  }

  /**
   * Puts a file in the folder without recording a change: the page moves imported files in and
   * exported ones out with this, and the core never sees them as its own.
   */
  putScratch(path, bytes) {
    const parts = path.split('/').filter(Boolean);
    const name = parts.pop();
    let dir = this.root;
    for (const part of parts) {
      if (!dir.entries.has(part)) dir.entries.set(part, new DirNode());
      dir = dir.entries.get(part);
    }
    const file = new FileNode();
    file.write(0, bytes);
    dir.entries.set(name, file);
  }

  /** Removes a file the page put there, and records the removal so storage forgets it too. */
  removeScratch(path) {
    const parts = path.split('/').filter(Boolean);
    const name = parts.pop();
    let dir = this.root;
    for (const part of parts) {
      dir = dir.entries.get(part);
      if (!(dir instanceof DirNode)) return;
    }
    dir.entries.delete(name);
    this.dirty.delete(path);
    this.removed.add(path);
  }

  lookupPath(path) {
    let node = this.root;
    for (const part of path.split('/').filter(Boolean)) {
      if (!(node instanceof DirNode)) return undefined;
      node = node.entries.get(part);
      if (node === undefined) return undefined;
    }
    return node;
  }

  /**
   * A relative path as the parts below the preopened folder. `..` goes up within it and nothing
   * above it is reachable; without links, that is all the path resolution there is.
   */
  normalize(base, relative) {
    const parts = base.split('/').filter(Boolean);
    for (const part of relative.split('/')) {
      if (part === '' || part === '.') continue;
      if (part === '..') {
        if (parts.length === 0) throw new WasiError(ERRNO.NOTCAPABLE);
        parts.pop();
      } else {
        parts.push(part);
      }
    }
    return parts;
  }

  /** The node at the given parts below the preopened folder. */
  walk(parts) {
    let node = this.root;
    for (const part of parts) {
      if (!(node instanceof DirNode)) throw new WasiError(ERRNO.NOTDIR);
      node = node.entries.get(part);
      if (node === undefined) throw new WasiError(ERRNO.NOENT);
    }
    return node;
  }

  /**
   * The folder that holds a path, its name, and the path from the preopened folder. `base` is the
   * open folder ({ node, path }) the relative path starts from. Creates nothing.
   */
  parentOf(base, relative) {
    const parts = this.normalize(base.path, relative);
    if (parts.length === 0) throw new WasiError(ERRNO.INVAL);
    const name = parts.pop();
    const dir = this.walk(parts);
    if (!(dir instanceof DirNode)) throw new WasiError(ERRNO.NOTDIR);
    return { dir, name, path: [...parts, name].join('/') };
  }

  dirOf(fd) {
    const entry = this.fds.get(fd);
    if (entry === undefined) throw new WasiError(ERRNO.BADF);
    if (!(entry.node instanceof DirNode)) throw new WasiError(ERRNO.NOTDIR);
    return entry;
  }

  fileOf(fd) {
    const entry = this.fds.get(fd);
    if (entry === undefined) throw new WasiError(ERRNO.BADF);
    return entry;
  }

  /** The WASI functions, as the core's `wasi_snapshot_preview1` imports. */
  imports() {
    const fs = this;
    const view = () => new DataView(fs.memory.buffer);
    const bytes = () => new Uint8Array(fs.memory.buffer);
    const text = (ptr, len) => new TextDecoder().decode(bytes().subarray(ptr, ptr + len));
    const guard = (run) => {
      try {
        return run();
      } catch (error) {
        if (error instanceof WasiError) return error.errno;
        throw error;
      }
    };
    const writeFilestat = (ptr, node) => {
      const v = view();
      v.setBigUint64(ptr, 0n, true);
      v.setBigUint64(ptr + 8, 0n, true);
      v.setUint8(ptr + 16, node instanceof DirNode ? FILETYPE.DIRECTORY : FILETYPE.REGULAR);
      v.setBigUint64(ptr + 24, 1n, true);
      v.setBigUint64(ptr + 32, BigInt(node instanceof FileNode ? node.size : 0), true);
      v.setBigUint64(ptr + 40, 0n, true);
      v.setBigUint64(ptr + 48, 0n, true);
      v.setBigUint64(ptr + 56, 0n, true);
    };
    // Reads the iovec list: [buf, len] pairs of u32.
    const iovecs = (ptr, count) => {
      const v = view();
      const list = [];
      for (let i = 0; i < count; i++) {
        list.push([v.getUint32(ptr + i * 8, true), v.getUint32(ptr + i * 8 + 4, true)]);
      }
      return list;
    };
    const markWrite = (path) => {
      fs.dirty.add(path);
      fs.removed.delete(path);
    };
    const markRemove = (path) => {
      fs.dirty.delete(path);
      fs.removed.add(path);
    };
    // A file's descendants, for moving or deleting a folder.
    const filesUnder = (node, prefix, out = []) => {
      for (const [name, child] of node.entries) {
        const path = prefix ? `${prefix}/${name}` : name;
        if (child instanceof DirNode) filesUnder(child, path, out);
        else out.push(path);
      }
      return out;
    };

    return {
      args_sizes_get(countPtr, sizePtr) {
        view().setUint32(countPtr, 0, true);
        view().setUint32(sizePtr, 0, true);
        return ERRNO.SUCCESS;
      },
      args_get() {
        return ERRNO.SUCCESS;
      },
      environ_sizes_get(countPtr, sizePtr) {
        view().setUint32(countPtr, 0, true);
        view().setUint32(sizePtr, 0, true);
        return ERRNO.SUCCESS;
      },
      environ_get() {
        return ERRNO.SUCCESS;
      },
      clock_time_get(clockId, precision, out) {
        const nanos = clockId === 0 ? BigInt(Date.now()) * 1000000n : BigInt(Math.round(performance.now() * 1e6));
        view().setBigUint64(out, nanos, true);
        return ERRNO.SUCCESS;
      },
      random_get(buf, len) {
        const target = bytes().subarray(buf, buf + len);
        for (let offset = 0; offset < target.length; offset += 65536) {
          crypto.getRandomValues(target.subarray(offset, Math.min(offset + 65536, target.length)));
        }
        return ERRNO.SUCCESS;
      },
      proc_exit(code) {
        throw new Error(`the core exited with code ${code}`);
      },
      sched_yield() {
        return ERRNO.SUCCESS;
      },
      poll_oneoff(_in, _out, _count, eventsPtr) {
        view().setUint32(eventsPtr, 0, true);
        return ERRNO.SUCCESS;
      },
      fd_prestat_get(fd, out) {
        if (fd !== 3) return ERRNO.BADF;
        const name = new TextEncoder().encode(fs.folderName);
        view().setUint8(out, PRESTAT_DIR);
        view().setUint32(out + 4, name.length, true);
        return ERRNO.SUCCESS;
      },
      fd_prestat_dir_name(fd, out, len) {
        if (fd !== 3) return ERRNO.BADF;
        const name = new TextEncoder().encode(fs.folderName);
        bytes().set(name.subarray(0, len), out);
        return ERRNO.SUCCESS;
      },
      fd_fdstat_get(fd, out) {
        const entry = fs.fds.get(fd);
        const v = view();
        for (let i = 0; i < 24; i++) v.setUint8(out + i, 0);
        if (entry === undefined) {
          if (fd === 0 || fd === 1 || fd === 2) {
            v.setUint8(out, FILETYPE.CHARACTER_DEVICE);
            v.setBigUint64(out + 8, ~0n, true);
            v.setBigUint64(out + 16, ~0n, true);
            return ERRNO.SUCCESS;
          }
          return ERRNO.BADF;
        }
        v.setUint8(out, entry.node instanceof DirNode ? FILETYPE.DIRECTORY : FILETYPE.REGULAR);
        v.setUint16(out + 2, entry.append ? FDFLAGS_APPEND : 0, true);
        v.setBigUint64(out + 8, ~0n, true);
        v.setBigUint64(out + 16, ~0n, true);
        return ERRNO.SUCCESS;
      },
      fd_fdstat_set_flags(fd, flags) {
        return guard(() => {
          const entry = fs.fileOf(fd);
          entry.append = (flags & FDFLAGS_APPEND) !== 0;
          return ERRNO.SUCCESS;
        });
      },
      fd_filestat_get(fd, out) {
        return guard(() => {
          if (fd >= 0 && fd <= 2) {
            view().setUint8(out + 16, FILETYPE.CHARACTER_DEVICE);
            return ERRNO.SUCCESS;
          }
          writeFilestat(out, fs.fileOf(fd).node);
          return ERRNO.SUCCESS;
        });
      },
      fd_filestat_set_size(fd, size) {
        return guard(() => {
          const entry = fs.fileOf(fd);
          if (!(entry.node instanceof FileNode)) throw new WasiError(ERRNO.ISDIR);
          entry.node.truncate(Number(size));
          markWrite(entry.path);
          return ERRNO.SUCCESS;
        });
      },
      fd_filestat_set_times() {
        return ERRNO.SUCCESS;
      },
      fd_allocate(fd, offset, length) {
        return guard(() => {
          const entry = fs.fileOf(fd);
          const end = Number(offset + length);
          if (entry.node instanceof FileNode && end > entry.node.size) {
            entry.node.truncate(end);
            markWrite(entry.path);
          }
          return ERRNO.SUCCESS;
        });
      },
      fd_advise() {
        return ERRNO.SUCCESS;
      },
      fd_seek(fd, offset, whence, out) {
        return guard(() => {
          const entry = fs.fileOf(fd);
          const size = entry.node instanceof FileNode ? entry.node.size : 0;
          const base = [0, entry.position, size][Number(whence)];
          if (base === undefined) throw new WasiError(ERRNO.INVAL);
          const next = base + Number(offset);
          if (next < 0) throw new WasiError(ERRNO.INVAL);
          entry.position = next;
          view().setBigUint64(out, BigInt(next), true);
          return ERRNO.SUCCESS;
        });
      },
      fd_tell(fd, out) {
        return guard(() => {
          view().setBigUint64(out, BigInt(fs.fileOf(fd).position), true);
          return ERRNO.SUCCESS;
        });
      },
      fd_read(fd, iovs, count, out) {
        return guard(() => {
          if (fd === 0) {
            view().setUint32(out, 0, true);
            return ERRNO.SUCCESS;
          }
          const entry = fs.fileOf(fd);
          if (!(entry.node instanceof FileNode)) throw new WasiError(ERRNO.ISDIR);
          let total = 0;
          for (const [buf, len] of iovecs(iovs, count)) {
            const chunk = entry.node.read(entry.position, len);
            bytes().set(chunk, buf);
            entry.position += chunk.length;
            total += chunk.length;
            if (chunk.length < len) break;
          }
          view().setUint32(out, total, true);
          return ERRNO.SUCCESS;
        });
      },
      fd_pread(fd, iovs, count, offset, out) {
        return guard(() => {
          const entry = fs.fileOf(fd);
          if (!(entry.node instanceof FileNode)) throw new WasiError(ERRNO.ISDIR);
          let position = Number(offset);
          let total = 0;
          for (const [buf, len] of iovecs(iovs, count)) {
            const chunk = entry.node.read(position, len);
            bytes().set(chunk, buf);
            position += chunk.length;
            total += chunk.length;
            if (chunk.length < len) break;
          }
          view().setUint32(out, total, true);
          return ERRNO.SUCCESS;
        });
      },
      fd_write(fd, iovs, count, out) {
        return guard(() => {
          if (fd === 1 || fd === 2) {
            const sink = fd === 1 ? fs.stdout : fs.stderr;
            const pieces = iovecs(iovs, count).map(([buf, len]) => bytes().slice(buf, buf + len));
            const total = pieces.reduce((sum, piece) => sum + piece.length, 0);
            const joined = new Uint8Array(total);
            let offset = 0;
            for (const piece of pieces) {
              joined.set(piece, offset);
              offset += piece.length;
            }
            sink(new TextDecoder().decode(joined).trimEnd());
            view().setUint32(out, total, true);
            return ERRNO.SUCCESS;
          }
          const entry = fs.fileOf(fd);
          if (!(entry.node instanceof FileNode)) throw new WasiError(ERRNO.ISDIR);
          let total = 0;
          if (entry.append) entry.position = entry.node.size;
          for (const [buf, len] of iovecs(iovs, count)) {
            entry.node.write(entry.position, bytes().slice(buf, buf + len));
            entry.position += len;
            total += len;
          }
          markWrite(entry.path);
          view().setUint32(out, total, true);
          return ERRNO.SUCCESS;
        });
      },
      fd_pwrite(fd, iovs, count, offset, out) {
        return guard(() => {
          const entry = fs.fileOf(fd);
          if (!(entry.node instanceof FileNode)) throw new WasiError(ERRNO.ISDIR);
          let position = Number(offset);
          let total = 0;
          for (const [buf, len] of iovecs(iovs, count)) {
            entry.node.write(position, bytes().slice(buf, buf + len));
            position += len;
            total += len;
          }
          markWrite(entry.path);
          view().setUint32(out, total, true);
          return ERRNO.SUCCESS;
        });
      },
      fd_sync() {
        return ERRNO.SUCCESS;
      },
      fd_datasync() {
        return ERRNO.SUCCESS;
      },
      fd_close(fd) {
        if (fd <= 3) return ERRNO.SUCCESS;
        return fs.fds.delete(fd) ? ERRNO.SUCCESS : ERRNO.BADF;
      },
      fd_readdir(fd, buf, bufLen, cookie, out) {
        return guard(() => {
          const entry = fs.dirOf(fd);
          const names = ['.', '..', ...[...entry.node.entries.keys()].sort()];
          const v = view();
          const mem = bytes();
          let used = 0;
          const start = Number(cookie);
          for (let index = start; index < names.length; index++) {
            const name = new TextEncoder().encode(names[index]);
            const child = names[index] === '.' || names[index] === '..' ? entry.node : entry.node.entries.get(names[index]);
            const type = child instanceof DirNode ? FILETYPE.DIRECTORY : FILETYPE.REGULAR;
            const header = new Uint8Array(24);
            const hv = new DataView(header.buffer);
            hv.setBigUint64(0, BigInt(index + 1), true);
            hv.setBigUint64(8, 0n, true);
            hv.setUint32(16, name.length, true);
            hv.setUint8(20, type);
            const room = bufLen - used;
            const piece = new Uint8Array(24 + name.length);
            piece.set(header, 0);
            piece.set(name, 24);
            mem.set(piece.subarray(0, Math.max(0, Math.min(piece.length, room))), buf + used);
            used += Math.min(piece.length, Math.max(0, room));
            if (used >= bufLen) break;
          }
          v.setUint32(out, used, true);
          return ERRNO.SUCCESS;
        });
      },
      path_open(dirfd, _dirflags, pathPtr, pathLen, oflags, _rightsBase, _rightsInheriting, fdflags, outFd) {
        return guard(() => {
          const dir = fs.dirOf(dirfd);
          const relative = text(pathPtr, pathLen);
          const found = fs.openPath(dir, relative, oflags);
          const fd = fs.nextFd++;
          fs.fds.set(fd, {
            node: found.node,
            path: found.path,
            position: 0,
            append: (fdflags & FDFLAGS_APPEND) !== 0,
          });
          view().setUint32(outFd, fd, true);
          return ERRNO.SUCCESS;
        });
      },
      path_filestat_get(dirfd, _flags, pathPtr, pathLen, out) {
        return guard(() => {
          const dir = fs.dirOf(dirfd);
          const node = fs.lookupIn(dir, text(pathPtr, pathLen));
          writeFilestat(out, node);
          return ERRNO.SUCCESS;
        });
      },
      path_filestat_set_times() {
        return ERRNO.SUCCESS;
      },
      path_create_directory(dirfd, pathPtr, pathLen) {
        return guard(() => {
          const dir = fs.dirOf(dirfd);
          const relative = text(pathPtr, pathLen);
          // The folder that is open already exists (create_dir_all reaches it this way).
          if (fs.normalize(dir.path, relative).length === 0) throw new WasiError(ERRNO.EXIST);
          const { dir: parent, name } = fs.parentOf(dir, relative);
          if (parent.entries.has(name)) throw new WasiError(ERRNO.EXIST);
          parent.entries.set(name, new DirNode());
          return ERRNO.SUCCESS;
        });
      },
      path_remove_directory(dirfd, pathPtr, pathLen) {
        return guard(() => {
          const dir = fs.dirOf(dirfd);
          const { dir: parent, name } = fs.parentOf(dir, text(pathPtr, pathLen));
          const node = parent.entries.get(name);
          if (node === undefined) throw new WasiError(ERRNO.NOENT);
          if (!(node instanceof DirNode)) throw new WasiError(ERRNO.NOTDIR);
          if (node.entries.size > 0) throw new WasiError(ERRNO.NOTEMPTY);
          parent.entries.delete(name);
          return ERRNO.SUCCESS;
        });
      },
      path_unlink_file(dirfd, pathPtr, pathLen) {
        return guard(() => {
          const dir = fs.dirOf(dirfd);
          const { dir: parent, name, path } = fs.parentOf(dir, text(pathPtr, pathLen));
          const node = parent.entries.get(name);
          if (node === undefined) throw new WasiError(ERRNO.NOENT);
          if (node instanceof DirNode) throw new WasiError(ERRNO.ISDIR);
          parent.entries.delete(name);
          markRemove(path);
          return ERRNO.SUCCESS;
        });
      },
      path_rename(oldfd, oldPtr, oldLen, newfd, newPtr, newLen) {
        return guard(() => {
          const from = fs.dirOf(oldfd);
          const to = fs.dirOf(newfd);
          const source = fs.parentOf(from, text(oldPtr, oldLen));
          const node = source.dir.entries.get(source.name);
          if (node === undefined) throw new WasiError(ERRNO.NOENT);
          const target = fs.parentOf(to, text(newPtr, newLen));
          const replaced = target.dir.entries.get(target.name);
          if (replaced instanceof DirNode && node instanceof FileNode) throw new WasiError(ERRNO.ISDIR);
          if (replaced instanceof DirNode && replaced.entries.size > 0) throw new WasiError(ERRNO.NOTEMPTY);
          const oldPrefix = source.path;
          const movedFiles = node instanceof DirNode ? filesUnder(node, oldPrefix) : [oldPrefix];
          source.dir.entries.delete(source.name);
          target.dir.entries.set(target.name, node);
          for (const path of movedFiles) markRemove(path);
          const newPrefix = target.path;
          const newFiles = node instanceof DirNode ? filesUnder(node, newPrefix) : [newPrefix];
          for (const path of newFiles) markWrite(path);
          return ERRNO.SUCCESS;
        });
      },
      path_link() {
        return ERRNO.NOTSUP;
      },
      path_symlink() {
        return ERRNO.NOTSUP;
      },
      path_readlink(dirfd, pathPtr, pathLen) {
        return guard(() => {
          // There are no links here: a path that exists is simply not a link.
          fs.lookupIn(fs.dirOf(dirfd), text(pathPtr, pathLen));
          throw new WasiError(ERRNO.INVAL);
        });
      },
    };
  }

  /** Opens or creates a file below `dir` for path_open, honouring the open flags. */
  openPath(dir, relative, oflags) {
    if (this.normalize(dir.path, relative).length === 0) {
      return { node: this.root, path: '' };
    }
    const { dir: parent, name, path: full } = this.parentOf(dir, relative);
    let node = parent.entries.get(name);
    if (node !== undefined) {
      if (oflags & OFLAGS.CREAT && oflags & OFLAGS.EXCL) throw new WasiError(ERRNO.EXIST);
      if (oflags & OFLAGS.DIRECTORY && !(node instanceof DirNode)) throw new WasiError(ERRNO.NOTDIR);
      if (node instanceof FileNode && oflags & OFLAGS.TRUNC) {
        node.truncate(0);
        this.dirty.add(full);
        this.removed.delete(full);
      }
    } else {
      if (!(oflags & OFLAGS.CREAT)) throw new WasiError(ERRNO.NOENT);
      if (oflags & OFLAGS.DIRECTORY) {
        node = new DirNode();
      } else {
        node = new FileNode();
        this.dirty.add(full);
        this.removed.delete(full);
      }
      parent.entries.set(name, node);
    }
    return { node, path: full };
  }

  /** Looks a path up below `dir` (path_filestat_get). */
  lookupIn(dir, relative) {
    return this.walk(this.normalize(dir.path, relative));
  }
}
