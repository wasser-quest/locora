// Shows one animated graphic. The page answers with the graphic's SVG; this document then navigates
// to that SVG as its own document. A blob document inherits the policy of the document that made it
// (this one), not the page's, and the SVG is parsed as XML, so its scripts run as they do on the desktop.
(function () {
  const params = new URLSearchParams(location.search);
  // A graphic is either stored on the device (`hash`) or a static file of the app (`file`, e.g. the
  // splash). The page answers either request; it sends the SVG text.
  const request = params.has('file') ? { file: params.get('file') } : { hash: params.get('hash') ?? '' };
  window.addEventListener('message', (event) => {
    if (event.source !== window.parent || event.data?.locoraGraphic !== 'svg') return;
    if (typeof event.data.svg !== 'string') return;
    clearInterval(asking);
    const url = URL.createObjectURL(new Blob([event.data.svg], { type: 'image/svg+xml' }));
    location.replace(url);
  });
  // The page may not be ready yet (the splash runs first), so the request is repeated until it is answered.
  const ask = () => window.parent.postMessage({ locoraGraphic: 'ready', ...request }, '*');
  const asking = setInterval(ask, 150);
  ask();
})();
