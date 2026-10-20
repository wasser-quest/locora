// The offline app. On install, every file of the app is stored in a cache, so the app opens and
// reloads without a network. Data is not in this cache: it lives in IndexedDB (see file-storage.mjs
// and boot.mjs), and nothing is sent anywhere.
//
// build.mjs replaces the two placeholders: the cache version (a hash of every file of the build,
// so a new build gets a new cache) and the list of files to store.

const VERSION = '__VERSION__';
const PRECACHE = __PRECACHE__;
const CACHE = `locora-${VERSION}`;

self.addEventListener('install', (event) => {
  event.waitUntil(
    caches
      .open(CACHE)
      .then((cache) => cache.addAll(PRECACHE.map((path) => new Request(path, { cache: 'reload' }))))
      .then(() => self.skipWaiting()),
  );
});

self.addEventListener('activate', (event) => {
  event.waitUntil(
    caches
      .keys()
      .then((keys) => Promise.all(keys.filter((key) => key.startsWith('locora-') && key !== CACHE).map((key) => caches.delete(key))))
      .then(() => self.clients.claim()),
  );
});

self.addEventListener('fetch', (event) => {
  const request = event.request;
  if (request.method !== 'GET') return;
  const url = new URL(request.url);
  if (url.origin !== self.location.origin) return;
  event.respondWith(answer(request, url));
});

/** The cache key of a request: its path below the app's folder (the query is not part of it). */
function keyOf(url) {
  const scope = new URL('./', self.registration.scope).pathname;
  const path = url.pathname.startsWith(scope) ? url.pathname.slice(scope.length) : url.pathname;
  return path === '' ? 'index.html' : path;
}

// Network first: online, every file comes from the server, so a new deploy is never mixed with the
// files of an older one. The cache answers when the network does not (offline).
async function answer(request, url) {
  const cache = await caches.open(CACHE);
  const href = new URL(keyOf(url), self.registration.scope).href;
  try {
    const response = await fetch(request);
    if (response.ok && response.type === 'basic') cache.put(href, response.clone());
    return response;
  } catch (error) {
    const cached = await cache.match(href);
    if (cached) return cached;
    // Offline and not stored: the app's start page answers, so the app still opens.
    if (request.mode === 'navigate') {
      const start = await cache.match(new URL('index.html', self.registration.scope).href);
      if (start) return start;
    }
    throw error;
  }
}
