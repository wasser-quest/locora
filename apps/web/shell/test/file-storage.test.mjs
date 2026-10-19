import { test } from 'node:test';
import assert from 'node:assert/strict';
import { indexedDB } from '../../plugin-host/node_modules/fake-indexeddb/build/esm/index.js';
import { openFileStorage, loadFiles, saveChanges } from '../src/file-storage.mjs';
import { MemFs } from '../src/wasi-memfs.mjs';

test('saved changes come back after a restart: writes replace, removes delete', async () => {
  const db = await openFileStorage(indexedDB, `files-${Math.random()}`);
  await saveChanges(db, {
    writes: [['locora.db', new Uint8Array([1, 2, 3])], ['media/ab', new Uint8Array([9])]],
    removes: [],
  });
  await saveChanges(db, { writes: [['locora.db', new Uint8Array([4])]], removes: ['media/ab'] });
  const files = (await loadFiles(db)).map(([path, bytes]) => [path, [...bytes]]);
  assert.deepEqual(files, [['locora.db', [4]]]);
});

test('a file the core writes through WASI is saved and comes back in a new file system', async () => {
  const db = await openFileStorage(indexedDB, `files-${Math.random()}`);
  const memory = { buffer: new ArrayBuffer(4096) };
  const first = new MemFs();
  first.memory = memory;
  const wasi = first.imports();
  const bytes = new Uint8Array(memory.buffer);
  const view = new DataView(memory.buffer);
  const put = (offset, text) => bytes.set(new TextEncoder().encode(text), offset);

  put(0, 'notes');
  assert.equal(wasi.path_create_directory(3, 0, 5), 0);
  put(100, 'notes/a.txt');
  assert.equal(wasi.path_open(3, 0, 100, 11, 1, 0n, 0n, 0, 200), 0, 'creates the file');
  const fd = view.getUint32(200, true);
  bytes[400] = 7;
  view.setUint32(300, 400, true);
  view.setUint32(304, 1, true);
  assert.equal(wasi.fd_write(fd, 300, 1, 308), 0);
  assert.equal(wasi.fd_close(fd), 0);

  await saveChanges(db, first.takeChanges());
  const second = new MemFs();
  second.load(await loadFiles(db));
  assert.deepEqual(second.files().map(([path, data]) => [path, [...data]]), [['notes/a.txt', [7]]]);
});

test('scratch files move in and out without being saved, and a removed export leaves storage', async () => {
  const db = await openFileStorage(indexedDB, `files-${Math.random()}`);
  const fs = new MemFs();
  fs.putScratch('inbox/photo.png', new Uint8Array([1, 2]));
  assert.deepEqual([...fs.readFile('inbox/photo.png')], [1, 2]);
  assert.deepEqual(fs.takeChanges().writes, [], 'a scratch file is not a change');
  fs.removeScratch('inbox/photo.png');
  assert.equal(fs.readFile('inbox/photo.png'), undefined);

  const memory = { buffer: new ArrayBuffer(4096) };
  fs.memory = memory;
  const wasi = fs.imports();
  const bytes = new Uint8Array(memory.buffer);
  const view = new DataView(memory.buffer);
  bytes.set(new TextEncoder().encode('outbox'), 0);
  wasi.path_create_directory(3, 0, 6);
  bytes.set(new TextEncoder().encode('outbox/x.locora'), 100);
  wasi.path_open(3, 0, 100, 15, 1, 0n, 0n, 0, 200);
  const fd = view.getUint32(200, true);
  bytes[400] = 5;
  view.setUint32(300, 400, true);
  view.setUint32(304, 1, true);
  wasi.fd_write(fd, 300, 1, 308);
  wasi.fd_close(fd);
  await saveChanges(db, fs.takeChanges());
  fs.removeScratch('outbox/x.locora');
  await saveChanges(db, fs.takeChanges());
  assert.deepEqual(await loadFiles(db), [], 'the export is gone from storage once the page has it');
});
