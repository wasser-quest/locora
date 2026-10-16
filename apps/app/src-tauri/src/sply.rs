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
    if (!d || d.locora !== 1) return;
    if (d.checkRuntime) {
      window.parent.postMessage({ locora: 1, runtimeReady: !!svg.splinely }, '*');
      return;
    }
    if (!svg.splinely) return;
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
