//! `.sply` → interactive SVG with the Splinely runtime, plus a tiny bridge so
//! the sandboxed frame can be steered from the app with `postMessage`:
//! `{ locora: 1, machine: "status", send: "faellig" }`.

use anyhow::{Result, anyhow};
use splinely_document::Project;

const RUNTIME: &str = include_str!("../vendor/splinely-0.5.2.js");

/// Fill the frame; listen for state machine events from the app.
const BRIDGE: &str = r#"<style>:root{width:100% !important;height:100% !important;background:transparent !important}</style>
<script><![CDATA[
(function () {
  var svg = document.documentElement;
  var timedClips = [], timedCancel = null, timedTicker = null, timed = false;
  function animationDuration() {
    var clips = svg.splinely && svg.splinely.sourceProject.clips;
    return clips && clips.length ? Math.max.apply(null, clips.map(function (clip) { return clip.duration; })) : 0;
  }
  function timedReply(field, value) {
    var reply = { locora: 1 };
    reply[field] = value;
    window.parent.postMessage(reply, '*');
  }
  function pauseTimed() {
    if (timedCancel) { timedCancel(); timedCancel = null; }
    if (timedTicker) { clearInterval(timedTicker); timedTicker = null; }
    if (svg.splinely) svg.splinely.timeline.pause();
  }
  // The app forwards the pointer as coordinates. These helpers rebuild the DOM sequence
  // a browser would produce: enter/leave along the ancestor chain, a press on one element,
  // moves to that element while pressed, and a click on the nearest common ancestor.
  var hovered = null, pressed = null;
  function chainOf(node) {
    var out = [];
    for (var n = node; n && n !== svg; n = n.parentNode) out.push(n);
    return out;
  }
  function targetAt(x, y) {
    var el = document.elementFromPoint(x, y);
    return el && el !== svg && svg.contains(el) ? el : null;
  }
  function pointerEvent(type, x, y, bubbles) {
    return new PointerEvent(type, {
      clientX: x, clientY: y, bubbles: bubbles, cancelable: true,
      pointerType: 'mouse', isPrimary: true, button: 0, buttons: pressed ? 1 : 0
    });
  }
  function setHover(next, x, y) {
    if (next === hovered) return;
    var before = hovered ? chainOf(hovered) : [], after = next ? chainOf(next) : [];
    if (hovered) {
      before.forEach(function (n) {
        if (after.indexOf(n) < 0) n.dispatchEvent(pointerEvent('pointerleave', x, y, false));
      });
      hovered.dispatchEvent(pointerEvent('pointerout', x, y, true));
    }
    if (next) {
      next.dispatchEvent(pointerEvent('pointerover', x, y, true));
      after.slice().reverse().forEach(function (n) {
        if (before.indexOf(n) < 0) n.dispatchEvent(pointerEvent('pointerenter', x, y, false));
      });
    }
    hovered = next;
  }
  function moveTo(x, y) {
    var el = targetAt(x, y);
    setHover(el, x, y);
    var receiver = pressed || el;
    if (receiver) receiver.dispatchEvent(pointerEvent('pointermove', x, y, true));
  }
  function pressAt(x, y) {
    var el = targetAt(x, y);
    setHover(el, x, y);
    if (!el) return;
    pressed = el;
    el.dispatchEvent(pointerEvent('pointerdown', x, y, true));
  }
  function releaseAt(x, y) {
    var start = pressed, el = targetAt(x, y);
    pressed = null;
    setHover(el, x, y);
    if (!start) return;
    start.dispatchEvent(pointerEvent('pointerup', x, y, true));
    var shared = null, others = el ? chainOf(el) : [];
    chainOf(start).some(function (n) { if (others.indexOf(n) >= 0) { shared = n; return true; } return false; });
    if (shared) shared.dispatchEvent(new MouseEvent('click', { clientX: x, clientY: y, bubbles: true, cancelable: true, button: 0 }));
  }
  // document.currentScript can be null for SVG scripts in WebViews. The
  // bundled runtime may therefore be loaded without its auto-bootstrap.
  if (!svg.splinely && window.SplinelyRuntime) {
    window.SplinelyRuntime.createRuntime(svg);
  }
  window.parent.postMessage({ locora: 1, runtimeReady: !!svg.splinely }, '*');
  window.addEventListener('message', function (e) {
    var d = e.data;
    if (e.source !== window.parent || !d || d.locora !== 1) return;
    if (d.checkRuntime) {
      window.parent.postMessage({ locora: 1, runtimeReady: !!svg.splinely }, '*');
      return;
    }
    if (!svg.splinely) return;
    if (d.timed && typeof d.timed.action === 'string') {
      try {
        var runtime = svg.splinely, duration = animationDuration();
        if (!Number.isFinite(duration) || duration <= 0) {
          timedReply('timedError', 'Diese Splinely-Animation hat keine Zeitleiste');
          return;
        }
        if (d.timed.action === 'init') {
          timedClips = runtime.sourceProject.clips.filter(function (clip) {
            return clip.autoplay || runtime.clip(clip.id).playing;
          }).map(function (clip) { return clip.id; });
          if (!timedClips.length) {
            timedClips = runtime.sourceProject.clips.map(function (clip) { return clip.id; });
          }
          if (!timedClips.length) {
            timedReply('timedError', 'Keine Splinely-Animation vorhanden');
            return;
          }
          timed = true;
          pauseTimed();
          timedClips.forEach(function (id) { runtime.clip(id).seek(0); });
          runtime.timeline.seek(0);
          timedReply('timedReady', duration);
        } else if (timed && (d.timed.action === 'seek' || d.timed.action === 'playTo')) {
          var target = d.timed.time;
          if (!Number.isFinite(target) || target < 0 || target > duration) {
            timedReply('timedError', 'Zeit liegt außerhalb der Animation');
            return;
          }
          pauseTimed();
          if (d.timed.action === 'playTo' && Number.isFinite(d.timed.start)
              && d.timed.start >= 0 && d.timed.start <= target) {
            runtime.timeline.seek(d.timed.start);
          }
          if (d.timed.action === 'seek' || target <= runtime.timeline.time) {
            runtime.timeline.seek(target);
            timedReply('timedAt', target);
          } else {
            timedCancel = runtime.timeline.at(target, function () {
              pauseTimed();
              runtime.timeline.seek(target);
              timedReply('timedAt', target);
            });
            timedClips.forEach(function (id) { runtime.clip(id).play(); });
            timedTicker = setInterval(function () {
              timedReply('timedPosition', runtime.timeline.time);
            }, 100);
            runtime.timeline.play();
          }
        }
      } catch (err) {
        timedReply('timedError', String(err));
      }
      return;
    }
    if (d.pointer && Number.isFinite(d.pointer.x) && Number.isFinite(d.pointer.y)) {
      // Pointer followers read the root; hover and drags use the element underneath.
      svg.dispatchEvent(new PointerEvent('pointermove', {
        clientX: d.pointer.x, clientY: d.pointer.y, bubbles: true,
        pointerType: 'mouse', isPrimary: true
      }));
      moveTo(d.pointer.x, d.pointer.y);
      return;
    }
    if (d.pointerDown && Number.isFinite(d.pointerDown.x) && Number.isFinite(d.pointerDown.y)) {
      pressAt(d.pointerDown.x, d.pointerDown.y);
      return;
    }
    if (d.pointerUp && Number.isFinite(d.pointerUp.x) && Number.isFinite(d.pointerUp.y)) {
      releaseAt(d.pointerUp.x, d.pointerUp.y);
      return;
    }
    if (d.pointerLeave) {
      pressed = null;
      setHover(null, 0, 0);
      svg.dispatchEvent(new PointerEvent('pointerleave', { pointerType: 'mouse' }));
      return;
    }
    try {
      if (typeof d.machine === 'string' && typeof d.send === 'string') {
        svg.splinely.machine(d.machine).send(d.send);
      }
    } catch (err) {
      svg.splinely.reportError && svg.splinely.reportError(err);
    }
  });
})();
]]></script>"#;

/// Content-Security-Policy for a served `.sply`: inline scripts only, nothing
/// fetched from anywhere.
pub const CSP: &str = "default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; img-src data: blob:; media-src data: blob:";

pub fn to_svg(bytes: &[u8]) -> Result<String> {
    let project = Project::from_bytes(bytes).map_err(|e| anyhow!("{e}"))?;
    let mut svg = project.to_svg(RUNTIME).map_err(|e| anyhow!("{e}"))?;
    let end = svg
        .rfind("</svg>")
        .ok_or_else(|| anyhow!("SVG ohne schließendes </svg>"))?;
    svg.insert_str(end, BRIDGE);
    Ok(svg)
}

/// Immutable, room-independent batch template. The host sends complete locus
/// snapshots; artwork, clips and state machines remain those of the supplied file.
/// Serve with `CSP` and the same scripts-only iframe sandbox as `to_svg`.
pub fn to_marker_layer(bytes: &[u8]) -> Result<String> {
    let project = Project::from_bytes(bytes).map_err(|e| anyhow!("{e}"))?;
    let template = project
        .to_svg_embed("", splinely_document::EmbedFormat::Json)
        .map_err(|e| anyhow!("{e}"))?;
    // Keep the exporter-generated SVG (including gradients and accessibility).
    // Its empty script is inert inside <defs> and removed from every clone.
    let script = format!(
        "document.documentElement.splinely = {{}};\n{RUNTIME}\n{}",
        include_str!("marker_layer.js")
    )
    .replace("]]>", "]]]]><![CDATA[>");
    Ok(format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"100%\" height=\"100%\" style=\"width:100%;height:100%;overflow:hidden;background:transparent\"><defs id=\"locora-marker-template\">{template}</defs><script><![CDATA[{script}]]></script></svg>"
    ))
}

#[cfg(test)]
mod tests {
    /// Generate the exact app document for an external browser smoke test.
    #[test]
    #[ignore = "set LOCORA_SPLY_INPUT and LOCORA_SPLY_OUTPUT"]
    fn export_browser_fixture() {
        let bytes = std::fs::read(std::env::var_os("LOCORA_SPLY_INPUT").unwrap()).unwrap();
        let svg = super::to_svg(&bytes).unwrap();
        std::fs::write(std::env::var_os("LOCORA_SPLY_OUTPUT").unwrap(), svg).unwrap();
    }

    #[test]
    #[ignore = "set LOCORA_SPLY_INPUT and LOCORA_SPLY_OUTPUT"]
    fn export_marker_layer_fixture() {
        let bytes = std::fs::read(std::env::var_os("LOCORA_SPLY_INPUT").unwrap()).unwrap();
        let svg = super::to_marker_layer(&bytes).unwrap();
        std::fs::write(std::env::var_os("LOCORA_SPLY_OUTPUT").unwrap(), svg).unwrap();
    }

    #[test]
    fn marker_layer_keeps_the_custom_project_and_one_runtime() {
        let bytes = locora_runtime::tutorial::DEFAULT_MARKER;
        let svg = super::to_marker_layer(bytes).unwrap();
        assert_eq!(svg.matches(super::RUNTIME).count(), 1);
        assert!(svg.contains("locora-marker-template"));
        assert!(svg.contains("autoTick: false"));
        assert!(svg.contains("event.source !== window.parent"));
        assert!(svg.contains("markerLayerReady"));
        assert!(super::to_marker_layer(b"not sply").is_err());
        let project = splinely_document::Project::from_bytes(bytes).unwrap();
        let template = project
            .to_svg_embed("", splinely_document::EmbedFormat::Json)
            .unwrap();
        assert!(svg.contains(&template));
    }

    #[test]
    fn marker_layer_preserves_custom_gradient_and_animations() {
        use splinely_document::{Color, Gradient, Project};
        let mut project = Project::from_bytes(locora_runtime::tutorial::DEFAULT_MARKER).unwrap();
        project.name = "Custom marker <&>".into();
        project.custom_js = "window.untrusted = true;".into();
        project.nodes[0].gradient = Some(Gradient::linear(
            45.0,
            Color {
                r: 1.0,
                g: 0.0,
                b: 0.0,
                a: 1.0,
            },
            Color {
                r: 0.0,
                g: 0.0,
                b: 1.0,
                a: 1.0,
            },
        ));
        let svg = super::to_marker_layer(&project.to_binary().unwrap()).unwrap();
        let template = project
            .to_svg_embed("", splinely_document::EmbedFormat::Json)
            .unwrap();
        assert!(svg.contains(&template));
        assert!(svg.contains("<linearGradient"));
        assert!(svg.contains(&format!(
            "url(#{})",
            project.gradient_id(&project.nodes[0].id)
        )));
        assert!(svg.contains("Custom marker &lt;&amp;&gt;"));
        assert!(svg.contains("stateMachines"));
        assert!(svg.contains("clips"));
        assert!(!svg.contains("<script>window.untrusted"));
    }

    #[test]
    fn marker_becomes_svg_with_runtime_and_bridge() {
        let svg = super::to_svg(locora_runtime::tutorial::DEFAULT_MARKER).unwrap();
        assert!(svg.starts_with("<svg") || svg.starts_with("<?xml"));
        assert!(svg.contains("splinely-project"));
        assert!(svg.contains("locora !== 1"));
        assert!(svg.contains("window.SplinelyRuntime.createRuntime(svg)"));
        assert!(svg.trim_end().ends_with("</svg>"));
        assert!(super::to_svg(b"kein sply").is_err());
    }

    #[test]
    fn timed_bridge_uses_only_splinely_timeline() {
        let svg = super::to_svg(locora_runtime::tutorial::DEFAULT_MARKER).unwrap();
        assert!(svg.contains("runtime.timeline.seek(target)"));
        assert!(svg.contains("runtime.timeline.at(target"));
        assert!(svg.contains("svg.splinely.timeline.pause()"));
        assert!(svg.contains("timedError"));
    }
}
