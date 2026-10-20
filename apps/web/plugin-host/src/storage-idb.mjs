// Plugin storage on the device: one IndexedDB database, one object store, keys "<plugin>\u0000<key>".
// Nothing leaves the browser. Plugins read their keys when the app starts (see loadPluginStorage)
// and their changes are written after each call (see flushPluginStorage).

const STORE = 'plugin-storage';
const SEPARATOR = '\u0000';

export function openStorage(indexedDB, name = 'locora-plugins') {
  return new Promise((resolve, reject) => {
    const request = indexedDB.open(name, 1);
    request.onupgradeneeded = () => request.result.createObjectStore(STORE);
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });
}

/** Every stored key of one plugin, as a Map (what PluginHost.register takes). */
export function loadPluginStorage(db, pluginId) {
  return new Promise((resolve, reject) => {
    const prefix = pluginId + SEPARATOR;
    const result = new Map();
    const request = db.transaction(STORE, 'readonly').objectStore(STORE).openCursor();
    request.onsuccess = () => {
      const cursor = request.result;
      if (!cursor) return resolve(result);
      const key = String(cursor.key);
      if (key.startsWith(prefix)) result.set(key.slice(prefix.length), new Uint8Array(cursor.value));
      cursor.continue();
    };
    request.onerror = () => reject(request.error);
  });
}

/** Writes changed keys: `changes` is [key, value] pairs from PluginHost.takeDirty. A missing value deletes. */
export function flushPluginStorage(db, pluginId, changes) {
  return new Promise((resolve, reject) => {
    if (changes.length === 0) return resolve();
    const tx = db.transaction(STORE, 'readwrite');
    const store = tx.objectStore(STORE);
    for (const [key, value] of changes) {
      const id = pluginId + SEPARATOR + key;
      if (value === undefined) store.delete(id);
      else store.put(value, id);
    }
    tx.oncomplete = () => resolve();
    tx.onerror = () => reject(tx.error);
    tx.onabort = () => reject(tx.error);
  });
}
