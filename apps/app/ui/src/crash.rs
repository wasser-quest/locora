//! Crash reports for bug reports. A panic saves only the error text, where it happened and the
//! app version. Quoted text is removed from the error text, so palace names and answers stay out.
//! Nothing is sent: the next start shows the report, and the link opens a prefilled GitHub issue
//! that the person reads and submits themselves.

use serde::{Deserialize, Serialize};

const KEY: &str = "locora-last-crash-v1";
const REPO: &str = "https://github.com/wasser-quest/locora/issues/new";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Report {
    pub message: String,
    pub location: String,
    pub version: String,
}

/// Replace quoted text (which may hold palace names or answers) and keep the message short.
pub fn sanitize(message: &str) -> String {
    let mut out = String::new();
    let mut quoted = false;
    for ch in message.chars() {
        match ch {
            '"' | '“' | '”' => {
                quoted = !quoted;
                if !quoted {
                    out.push('…');
                }
            }
            _ if !quoted => out.push(ch),
            _ => {}
        }
    }
    out.chars().take(200).collect()
}

/// Installed once at start-up: every panic is saved before the default handling runs.
pub fn install() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .map(|text| text.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "panic".to_string());
        let location = info
            .location()
            .map(|at| format!("{}:{}", at.file(), at.line()))
            .unwrap_or_else(|| "unknown".to_string());
        let report = Report {
            message: sanitize(&message),
            location,
            version: env!("CARGO_PKG_VERSION").to_string(),
        };
        if let (Some(storage), Ok(text)) = (
            web_sys::window().and_then(|window| window.local_storage().ok().flatten()),
            serde_json::to_string(&report),
        ) {
            let _ = storage.set_item(KEY, &text);
        }
        previous(info);
    }));
}

/// The report of the last crash, removed so it is shown once.
pub fn take() -> Option<Report> {
    let storage = web_sys::window().and_then(|window| window.local_storage().ok().flatten())?;
    let text = storage.get_item(KEY).ok().flatten()?;
    let _ = storage.remove_item(KEY);
    serde_json::from_str(&text).ok()
}

/// A GitHub issue with the error, where it happened, the version and the steps to fill in.
pub fn issue_url(report: &Report) -> String {
    let title = format!("Crash: {}", report.message.chars().take(70).collect::<String>());
    let body = format!(
        "**Error:** {}\n**Where:** {}\n**Version:** {}\n\n**What I did before the crash (without personal content):**\n1. \n2. \n3. \n\n**Expected:**\n\n**Device / system:**\n",
        report.message, report.location, report.version
    );
    format!("{REPO}?title={}&body={}", encode(&title), encode(&body))
}

/// Percent-encoding for a query value (unreserved characters stay).
fn encode(text: &str) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(byte as char),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoted_text_is_removed() {
        assert_eq!(sanitize("index \"Mein Palast\" out of range"), "index … out of range");
    }

    #[test]
    fn issue_url_encodes_the_report() {
        let url = issue_url(&Report {
            message: "bad value".into(),
            location: "journey.rs:12".into(),
            version: "0.1.0".into(),
        });
        assert!(url.starts_with(REPO));
        assert!(url.contains("title=Crash%3A%20bad%20value"));
    }
}
