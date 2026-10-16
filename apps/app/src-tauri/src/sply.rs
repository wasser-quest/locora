//! `.sply` → interactive SVG with the Splinely runtime, plus a tiny bridge so
//! the sandboxed frame can be steered from the app with `postMessage`:
//! `{ locora: 1, machine: "status", send: "faellig" }`.

use anyhow::{Result, anyhow};
use splinely_document::Project;

const RUNTIME: &str = include_str!("../vendor/splinely-0.5.2.js");

/// Fill the frame; listen for state machine events from the app.
const BRIDGE: &str = r#"<style>:root{width:100% !important;height:100% !important}</style>
<script><![CDATA[
(function () {
  var svg = document.documentElement;
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
    if (d.pointer && Number.isFinite(d.pointer.x) && Number.isFinite(d.pointer.y)) {
      svg.dispatchEvent(new PointerEvent('pointermove', {
        clientX: d.pointer.x, clientY: d.pointer.y, bubbles: true,
        pointerType: 'mouse', isPrimary: true
      }));
      return;
    }
    if (d.click && Number.isFinite(d.click.x) && Number.isFinite(d.click.y)) {
      var target = document.elementFromPoint(d.click.x, d.click.y);
      if (target && svg.contains(target)) {
        target.dispatchEvent(new MouseEvent('click', {
          clientX: d.click.x, clientY: d.click.y, bubbles: true, button: 0
        }));
      }
      return;
    }
    if (d.pointerLeave) {
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
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"100%\" height=\"100%\" style=\"width:100%;height:100%;overflow:hidden\"><defs id=\"locora-marker-template\">{template}</defs><script><![CDATA[{script}]]></script></svg>"
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
}
