"""Real WebKitGTK probe for the marker layer, no app/Tauri required.

Export via sply::tests::export_marker_layer_fixture (ignored test; set
LOCORA_SPLY_INPUT and LOCORA_SPLY_OUTPUT), then:
    xvfb-run -a /usr/bin/python3 marker_layer_probe.py /tmp/marker-layer.svg
Requires the system python GI Gtk 3 / WebKit2 4.1 packages.
The fixture is enriched with custom artwork/references before execution.
"""
import base64
import html
import json
import re
import sys

import gi

gi.require_version("Gtk", "3.0")
gi.require_version("WebKit2", "4.1")
from gi.repository import Gtk, WebKit2, GLib

svg = open(sys.argv[1]).read()
match = re.search(r'<metadata id="splinely-project">(.*?)</metadata>', svg, re.S)
project = json.loads(html.unescape(match[1]))
project["customJs"] = "window.customJsExecuted=true"
project["variables"]["probe"] = "probe"  # Must NOT be rewritten as an ID.
project["groups"] = [{"id": "probeGroup"}]
project["nodes"].append({
    "id": "probe", "group": "probeGroup", "visible": True,
    "kind": {"type": "rect", "x": 0, "y": 0, "width": 50, "height": 50, "rx": 0},
    "gradient": {"radial": False, "angle": 45, "stops": [
        {"offset": 0, "color": {"r": 1, "g": 0, "b": 0, "a": 1}},
        {"offset": 1, "color": {"r": 0, "g": 0, "b": 1, "a": 1}}]},
    "transform": {"x": 0, "y": 0, "rotation": 0, "scaleX": 1, "scaleY": 1},
    "style": {"opacity": 1},
})
project["clips"].append({"id": "probeClip", "duration": 1, "tracks": [
    {"nodeId": "probe", "property": "width", "keyframes": [
        {"time": 0, "value": 10}, {"time": 1, "value": 90}]}]})
project.setdefault("followers", []).append({"id": "probeFollower", "target": "probe", "mode": "offset", "max": 25, "strength": 1})
project["stateMachines"] = [{"id": "status", "initial": "neu", "states": [
    {"id": name, "overrides": [{"nodeId": "probe", "property": "opacity", "value": opacity}],
     "onEnter": [{"type": "setVariable", "name": "probe", "value": "probe"},
                 {"type": "setProperty", "nodeId": "probe", "property": "strokeWidth", "value": 2}]}
    for name, opacity in [("neu", 1), ("faellig", .5), ("gewusst", .25)]
], "transitions": [{"from": "*", "to": name, "event": {"name": name}}
                    for name in ["neu", "faellig", "gewusst"]]}]
project["stateMachines"].append({"id": "probeMachine", "initial": "idle", "states": [
    {"id": "idle"}, {"id": "active"}], "transitions": [
    {"from": "idle", "to": "active", "event": {"name": "poke", "target": "probe"}}]})
svg = svg[:match.start(1)] + html.escape(json.dumps(project)) + svg[match.end(1):]
custom = '''<defs>
<linearGradient id="probePaint"><stop offset="0" stop-color="red"/><stop offset="1" stop-color="blue"/></linearGradient>
<clipPath id="probeClipPath"><rect width="100" height="100"/></clipPath>
</defs><title id="probeTitle">probe</title>
<g id="probeGroup"><rect id="probe" width="50" height="50" fill="url(#probePaint)" clip-path="url('#probeClipPath')" aria-labelledby="probeTitle"/></g>
<use id="probeUse" href="#probe"/>
<animate id="probeAnimate" href="#probe" attributeName="opacity" begin="probe.click; probeUse.end+1s" dur="1s"/>
'''
svg = svg.replace('</svg>', custom + '</svg>', 1)
probe = r"""
(async function () {
  function assert(ok, message) { if (!ok) throw new Error(message); }
  const wait = ms => new Promise(resolve => setTimeout(resolve, ms));
  function message(data, source = window.parent) {
    window.dispatchEvent(new MessageEvent('message', {source, data: {locora: 1, ...data}}));
  }
  const roots = () => [...document.documentElement.children].filter(el => el.tagName === 'svg');
  function snapshot(count) { return Array.from({length: count}, (_, i) => ({id: 'locus'+i, x: (i%10)/10, y: Math.floor(i/10)/10, status: ['neu','faellig','gewusst'][i%3]})); }
  try {
    message({loci: snapshot(100)}, window); // Source authentication rejects child/self.
    assert(roots().length === 0, 'untrusted message accepted');
    message({loci: snapshot(100)});
    assert(roots().length === 100, '100 markers were not created');
    assert(window.rafCalls === 1, 'per-instance RAF schedulers were created');
    let sandboxed = false;
    try { void window.parent.document; } catch (_) { sandboxed = true; }
    assert(sandboxed, 'frame can access parent DOM');
    const ids = [...document.querySelectorAll('[id]')].map(el => el.id);
    assert(new Set(ids).size === ids.length, 'duplicate document IDs');
    const entries = roots();
    for (const [i, svg] of entries.entries()) {
      const p = svg.splinely.sourceProject, id = p.nodes.find(n => n.id.endsWith('-probe')).id;
      const prefix = id.slice(0, -5), node = svg.querySelector('[id="'+id+'"]');
      assert(p.variables.probe === 'probe', 'variable payload renamed');
      assert(p.nodes.find(n => n.id === id).group === prefix+'probeGroup', 'group reference not renamed');
      assert(p.clips.find(c => c.id === 'probeClip').tracks[0].nodeId === id, 'clip target not renamed');
      assert(p.followers.find(f => f.id === 'probeFollower').target === id, 'follower target not renamed');
      assert(p.stateMachines[0].states[0].overrides[0].nodeId === id, 'override target not renamed');
      assert(p.stateMachines[0].states[0].onEnter[1].nodeId === id, 'action target not renamed');
      assert(p.stateMachines[1].transitions[0].event.target === id, 'event target not renamed');
      assert(svg.splinely.machine('status').state === ['neu','faellig','gewusst'][i%3], 'independent state missing');
      assert(node.getAttribute('fill') === 'url(#'+prefix+'probePaint)', 'gradient reference lost');
      assert(node.getAttribute('clip-path') === 'url(#'+prefix+'probeClipPath)', 'clip reference lost');
      assert(node.getAttribute('aria-labelledby') === prefix+'probeTitle', 'ARIA reference lost');
      assert(svg.querySelector('[id="'+prefix+'probeUse"]').getAttribute('href') === '#'+id, 'href lost');
      assert(svg.querySelector('[id="'+prefix+'probeAnimate"]').getAttribute('begin') === id+'.click; '+prefix+'probeUse.end+1s', 'SMIL reference lost');
      assert(svg.querySelector('[id="'+prefix+'probePaint"]').getAttribute('x1') !== null, 'gradient runtime stopped updating');
      assert(svg.getAttribute('width') === '56' && svg.getAttribute('height') === '56', 'wrong marker size');
      assert(svg.splinely.snapshot().diagnostics.length === 0, 'runtime diagnostics');
    }
    const a = entries[0], b = entries[1];
    const node = a.querySelector('[id$="-probe"]');
    a.splinely.emit('poke', {}, node.id);
    assert(a.splinely.machine('probeMachine').state === 'active' && b.splinely.machine('probeMachine').state === 'idle', 'targeted event leaked or failed');
    a.splinely.clip('probeClip').seek(.25);
    assert(Math.abs(Number(node.getAttribute('width')) - 30) < .01, 'clip did not animate');
    assert(b.querySelector('[id$="-probe"]').getAttribute('width') === '50', 'clip leaked to another marker');
    a.splinely.clip('probeClip').play(); await wait(750);
    assert(Number(node.getAttribute('width')) > 30, 'shared scheduler did not advance clip: width='+node.getAttribute('width')+' RAF='+window.rafCalls+' time='+a.splinely.clip('probeClip').time);
    message({pointer: {x: -1000, y: 100}}); await wait(100);
    const left = node.getAttribute('transform');
    message({pointer: {x: 2000, y: 100}}); await wait(100);
    assert(left !== node.getAttribute('transform'), 'pointer follower did not move');
    message({pointerLeave: true});
    message({loci: [{id: 'locus0', x: .75, y: .25, status: 'gewusst'}]});
    assert(roots().length === 1 && roots()[0] === a, 'snapshot did not reuse/remove instances');
    assert(a.splinely.machine('status').state === 'gewusst', 'status update missing');
    assert(!b.splinely, 'removed runtime not destroyed');
    assert(Math.abs(Number(a.getAttribute('x')) - (document.documentElement.clientWidth*.75-28)) < .01, 'coordinate layout mismatch');
    window.parent.postMessage({probeResize: 400}, '*'); await wait(500);
    assert(Math.abs(Number(a.getAttribute('x')) - 272) < .01, 'resize did not realign marker');
    assert(!window.customJsExecuted, 'customJs executed');
    message({loci: []}); assert(roots().length === 0 && !a.splinely, 'empty snapshot not cleaned');
    window.parent.postMessage({probe: 'PASS: 100 markers, namespacing, custom references, independent states/clips, gradients, pointer, cleanup, sandbox'}, '*');
  } catch (error) { window.parent.postMessage({probe: 'FAIL: '+error.message+' '+String(error.stack).replace(/data:image\/svg\+xml;base64,[A-Za-z0-9+/=]+/g, 'fixture')}, '*'); }
})();
"""
svg = svg.replace('document.documentElement.splinely = {};', 'window.rafCalls=0; const nativeRAF=window.requestAnimationFrame; window.requestAnimationFrame=function(fn){window.rafCalls++;return nativeRAF.call(window,fn);}; document.documentElement.splinely = {};', 1)
svg = svg.rsplit('</svg>', 1)[0] + '<script><![CDATA[' + probe + ']]></script></svg>'
uri = 'data:image/svg+xml;base64,' + base64.b64encode(svg.encode()).decode()
# Keep the production sandbox and CSP, and exercise the actual SVG document type.
csp = "default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; img-src data: blob:; media-src data: blob:; frame-src data:"
page = '<meta http-equiv="Content-Security-Policy" content="' + csp + '"><script>window.addEventListener("message",e=>{if(e.data.probe)window.probe=e.data.probe;if(e.data.probeResize)document.querySelector("iframe").style.width=e.data.probeResize+"px";});</script><iframe sandbox="allow-scripts" width="800" height="600" src="' + uri + '"></iframe>'
view = WebKit2.WebView()
window = Gtk.Window()
window.set_default_size(1000, 800)
window.add(view)
window.show_all()
results = []


def finished(view, result, *args):
    value = view.run_javascript_finish(result).get_js_value().to_string()
    if value != 'undefined':
        results.append(value)
        print(value)
        Gtk.main_quit()


def poll():
    view.run_javascript('window.probe', None, finished, None)
    return not results


view.load_html(page, 'file:///tmp/')
GLib.timeout_add(300, poll)
GLib.timeout_add(15000, lambda: (Gtk.main_quit(), False)[1])
Gtk.main()
assert results and results[0].startswith('PASS:'), results
