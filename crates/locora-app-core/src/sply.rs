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
