//! `.sply` → interactive SVG with the Splinely runtime, plus a tiny bridge so
//! the sandboxed frame can be steered from the app with `postMessage`:
//! `{ locora: 1, machine: "status", send: "faellig" }`.

use anyhow::{Result, anyhow};
use splinely_document::Project;

const RUNTIME: &str = include_str!("../vendor/splinely-0.5.4.js");

/// The frame side of the bridge. `attach` (sply_bridge.js) listens for state machine events and
/// timed commands from the app and answers with `reply`. The same file runs inline in the app page
/// (see the web shell), so the messages are the same there.
fn bridge() -> String {
    format!(
        r#"<style>:root{{width:100% !important;height:100% !important;background:transparent !important}}</style>
<script><![CDATA[
(function () {{
{ATTACH}
  attach(document.documentElement, {{
    reply: function (message) {{ window.parent.postMessage(message, '*'); }},
    onMessage: function (listener) {{
      window.addEventListener('message', function (e) {{ if (e.source === window.parent) listener(e.data); }});
    }}
  }});
}})();
]]></script>"#,
        ATTACH = include_str!("sply_bridge.js")
    )
}

/// Content-Security-Policy for a served `.sply`: inline scripts only, nothing
/// fetched from anywhere.
pub const CSP: &str = "default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; img-src data: blob:; media-src data: blob:";

pub fn to_svg(bytes: &[u8]) -> Result<String> {
    let project = Project::from_bytes(bytes).map_err(|e| anyhow!("{e}"))?;
    let mut svg = project.to_svg(RUNTIME).map_err(|e| anyhow!("{e}"))?;
    let end = svg
        .rfind("</svg>")
        .ok_or_else(|| anyhow!("SVG ohne schließendes </svg>"))?;
    svg.insert_str(end, &bridge());
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

    #[test]
    fn timed_bridge_uses_only_splinely_timeline() {
        let svg = super::to_svg(locora_runtime::tutorial::DEFAULT_MARKER).unwrap();
        assert!(svg.contains("runtime.timeline.seek(target)"));
        assert!(svg.contains("runtime.timeline.at(target"));
        assert!(svg.contains("svg.splinely.timeline.pause()"));
        assert!(svg.contains("timedError"));
    }
}
