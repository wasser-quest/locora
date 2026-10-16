// Executed only inside the opaque, scripts-only marker frame.
(function () {
  'use strict';
  const root = document.documentElement;
  delete root.splinely; // Suppress the vendored runtime's single-root bootstrap.
  const template = document.getElementById('locora-marker-template').firstElementChild;
  const source = window.SplinelyRuntime.readProject(template);
  const instances = new Map();
  let serial = 0;

  // Only DOM identities are renamed. Machine/state/clip/variable IDs and user
  // strings stay unchanged, even if they happen to equal a node ID.
  function namespace(svg, project, prefix) {
    const ids = new Map();
    for (const el of [svg, ...svg.querySelectorAll('[id]')]) {
      if (el.id) ids.set(el.id, prefix + el.id);
    }
    const ref = value => ids.get(value) || value;
    const urls = value => value.replace(/url\(\s*(['"]?)#([^\s)'"]+)\1\s*\)/g,
      (match, quote, id) => 'url(#' + ref(id) + ')');
    for (const el of [svg, ...svg.querySelectorAll('*')]) {
      for (const attr of [...el.attributes]) {
        let value = attr.value;
        if (attr.localName === 'id' || attr.localName === 'data-splinely-node' || attr.localName === 'data-splinely-group') value = ref(value);
        else if (attr.localName === 'href' && value[0] === '#') value = '#' + ref(value.slice(1));
        else if (['aria-labelledby', 'aria-describedby', 'aria-owns', 'aria-controls', 'aria-flowto', 'aria-activedescendant'].includes(attr.localName)) value = value.split(/\s+/).map(ref).join(' ');
        else if (attr.localName === 'begin' || attr.localName === 'end') value = value.split(';').map(part => {
          const trimmed = part.trim();
          for (const [id, renamed] of ids) if (trimmed.startsWith(id + '.')) return part.replace(id + '.', renamed + '.');
          return part;
        }).join(';');
        else value = urls(value);
        if (value !== attr.value) el.setAttributeNS(attr.namespaceURI, attr.name, value);
      }
    }
    const actions = list => { for (const a of list || []) {
      if (a.type === 'setProperty') a.nodeId = ref(a.nodeId);
      if (a.type === 'emit' && a.event) a.event.target = ref(a.event.target);
    }};
    for (const n of project.nodes || []) {
      n.id = ref(n.id); n.group = ref(n.group);
      if (n.motionPath) n.motionPath.path = ref(n.motionPath.path);
    }
    for (const g of project.groups || []) { g.id = ref(g.id); g.parentId = ref(g.parentId); }
    for (const c of project.clips || []) for (const t of c.tracks || []) {
      if (t.nodeId != null) t.nodeId = ref(t.nodeId);
      if (t.target != null) t.target = ref(t.target);
    }
    for (const m of project.stateMachines || project.machines || []) {
      for (const s of m.states || []) {
        for (const o of s.overrides || []) {
          if (o.nodeId != null) o.nodeId = ref(o.nodeId);
          if (o.target != null) o.target = ref(o.target);
        }
        actions(s.onEnter); actions(s.onExit);
      }
      for (const t of m.transitions || []) {
        if (t.event) t.event.target = ref(t.event.target);
        actions(t.actions);
      }
    }
    for (const f of project.followers || []) f.target = ref(f.target);
    for (const b of project.interactions || []) { b.target = ref(b.target); actions(b.actions); }
    return project;
  }

  function create() {
    const svg = template.cloneNode(true);
    const project = namespace(svg, JSON.parse(JSON.stringify(source)), 'marker' + (++serial) + '-');
    // Project data is passed explicitly; do not retain per-instance metadata.
    for (const el of svg.querySelectorAll('metadata, script')) el.remove();
    svg.setAttribute('width', '56'); svg.setAttribute('height', '56');
    root.appendChild(svg);
    try {
      return { svg, runtime: window.SplinelyRuntime.createRuntime(svg, project, {autoTick: false}), status: null };
    } catch (error) { svg.remove(); throw error; }
  }
  function layout() {
    for (const entry of instances.values()) {
      entry.svg.setAttribute('x', entry.x * root.clientWidth - 28);
      entry.svg.setAttribute('y', entry.y * root.clientHeight - 28);
    }
  }
  function snapshot(loci) {
    if (!Array.isArray(loci)) throw new Error('Invalid marker snapshot');
    const seen = new Set();
    for (const item of loci) {
      if (!item || typeof item.id !== 'string' || !item.id || seen.has(item.id) ||
          !Number.isFinite(item.x) || !Number.isFinite(item.y) ||
          !['neu', 'faellig', 'gewusst'].includes(item.status)) throw new Error('Invalid marker');
      seen.add(item.id);
    }
    for (const [id, entry] of instances) if (!seen.has(id)) {
      entry.runtime.destroy(); entry.svg.remove(); instances.delete(id);
    }
    for (const item of loci) {
      let entry = instances.get(item.id);
      if (!entry) { entry = create(); instances.set(item.id, entry); }
      entry.x = item.x; entry.y = item.y;
      if (entry.status !== item.status) {
        // A custom marker without a status machine still plays its own clips.
        if ((source.stateMachines || source.machines || []).some(m => m.id === 'status')) entry.runtime.machine('status').send(item.status);
        entry.status = item.status;
      }
    }
    layout();
  }
  const ready = value => window.parent.postMessage({locora: 1, markerLayerReady: value}, '*');
  window.addEventListener('message', function (event) {
    const d = event.data;
    if (event.source !== window.parent || !d || d.locora !== 1) return;
    try {
      if (d.checkRuntime) ready(true);
      if (d.loci !== undefined) snapshot(d.loci);
      if (d.pointer && Number.isFinite(d.pointer.x) && Number.isFinite(d.pointer.y)) {
        for (const entry of instances.values()) entry.svg.dispatchEvent(new PointerEvent('pointermove', {
          clientX: d.pointer.x, clientY: d.pointer.y, pointerType: 'mouse', isPrimary: true
        }));
      }
      if (d.pointerLeave) for (const entry of instances.values()) entry.svg.dispatchEvent(new PointerEvent('pointerleave', {pointerType: 'mouse'}));
    } catch (error) { console.error('Marker layer:', error); ready(false); }
  });
  const resize = new ResizeObserver(layout); resize.observe(root);
  let last = null, raf;
  function tick(now) {
    const delta = last === null ? 0 : Math.max(0, (now - last) / 1000); last = now;
    for (const entry of instances.values()) {
      try { entry.runtime.tick(delta); } catch (error) { console.error('Marker tick:', error); ready(false); }
    }
    raf = requestAnimationFrame(tick);
  }
  raf = requestAnimationFrame(tick);
  window.addEventListener('pagehide', () => {
    cancelAnimationFrame(raf); resize.disconnect();
    for (const entry of instances.values()) entry.runtime.destroy();
    instances.clear();
  });
  ready(true);
})();
