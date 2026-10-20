// Switched off. A worker installed by an earlier version of the app is replaced by this file: it
// deletes the app's caches, removes itself, and reloads the open pages. From then on the browser
// loads the app from the network, as an ordinary web page. It answers no requests itself.

self.addEventListener('install', () => self.skipWaiting());

self.addEventListener('activate', (event) => {
  event.waitUntil(
    caches
      .keys()
      .then((keys) => Promise.all(keys.filter((key) => key.startsWith('locora-')).map((key) => caches.delete(key))))
      .then(() => self.registration.unregister())
      .then(() => self.clients.matchAll({ type: 'window' }))
      .then((clients) => Promise.all(clients.map((client) => client.navigate(client.url).catch(() => {})))),
  );
});
