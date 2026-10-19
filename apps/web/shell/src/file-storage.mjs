// The core's files on the device: one IndexedDB database, one object store, keyed by the path inside
// the core's folder (e.g. "locora.db", "media/3f…"). Nothing leaves the browser. The page loads every
// file at start (MemFs.load) and writes the changes after each command (MemFs.takeChanges).

const STORE = 'files';

export function openFileStorage(indexedDB, name = 'locora-files') {
  return new Promise((resolve, reject) => {
    const request = indexedDB.open(name, 1);
    request.onupgradeneeded = () => request.result.createObjectStore(STORE);
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });
}

/** Every saved file as [path, bytes] pairs (what MemFs.load takes). */
export function loadFiles(db) {
  return new Promise((resolve, reject) => {
    const result = [];
    const request = db.transaction(STORE, 'readonly').objectStore(STORE).openCursor();
    request.onsuccess = () => {
      const cursor = request.result;
      if (!cursor) return resolve(result);
      result.push([String(cursor.key), new Uint8Array(cursor.value)]);
      cursor.continue();
    };
    request.onerror = () => reject(request.error);
  });
}

/** Writes the files that changed and deletes the ones removed (MemFs.takeChanges). */
export function saveChanges(db, { writes, removes }) {
  return new Promise((resolve, reject) => {
    if (writes.length === 0 && removes.length === 0) return resolve();
    const tx = db.transaction(STORE, 'readwrite');
    const store = tx.objectStore(STORE);
    for (const [path, bytes] of writes) store.put(bytes, path);
    for (const path of removes) store.delete(path);
    tx.oncomplete = () => resolve();
    tx.onerror = () => reject(tx.error);
    tx.onabort = () => reject(tx.error);
  });
}
