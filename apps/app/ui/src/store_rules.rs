//! The native apps never steer to a purchase: Locora Cloud is paid for only on the web, and the
//! app stores forbid prices, buy buttons and links to an outside checkout in an app. These tests
//! read the texts the apps can show (every UI source, the translation catalog in all languages,
//! the Tauri shell and the cloud messages of the shared crates) and fail on purchase wording or a
//! link to anything but the few pages without a purchase.

use std::fs;
use std::path::{Path, PathBuf};

/// Words and phrases that sell (matched in lower case anywhere in a file).
const PHRASES: &[&str] = &[
    "abo abschließen",
    "abonnieren",
    "kaufen",
    "upgrade",
    "checkout",
    "paddle",
    "€",
    "pricing",
    "purchase",
    "mit einem abo",
    "wähle ein abo",
    "a subscription gives",
    "choose a plan",
    "acheter",
    "s'abonner",
    "s’abonner",
    "un abonnement donne",
    "購入",
    "料金",
    "アップグレード",
    "购买",
    "价格",
];

/// Short words that sell, matched only as whole words (so "plan" or "buyer" elsewhere stays fine).
const WORDS: &[&str] = &["buy", "price", "prices", "preis", "preise", "prix"];

/// Hosts the apps may link to, each with the paths allowed there (empty: any path).
const HOSTS: &[(&str, &[&str])] = &[
    ("api.locora.cloud", &[]),
    ("web.locora.cloud", &["/forgot"]),
    ("github.com", &["/wasser-quest/locora/issues/new"]),
    ("example.com", &[]),
    ("localhost", &[]),
    ("127.0.0.1", &[]),
    // The app's own origins and XML namespaces.
    ("ipc.localhost", &[]),
    ("locora.localhost", &[]),
    ("www.w3.org", &[]),
    ("schema.tauri.app", &[]),
];

/// Every file whose texts the native apps show; this file is left out (it names the words).
fn sources() -> Vec<PathBuf> {
    let app = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let crates = app.join("../../crates");
    let mut files = vec![
        app.join("ui/index.html"),
        app.join("src-tauri/tauri.conf.json"),
    ];
    for dir in [
        app.join("ui/src"),
        app.join("shared/src"),
        app.join("src-tauri/src"),
        crates.join("locora-app-core/src"),
    ] {
        let mut found: Vec<PathBuf> = fs::read_dir(&dir)
            .unwrap_or_else(|error| panic!("{}: {error}", dir.display()))
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "rs"))
            .filter(|path| !path.ends_with("store_rules.rs"))
            .collect();
        found.sort();
        files.extend(found);
    }
    for name in [
        "cloud.rs",
        "cloud_http.rs",
        "cloud_key.rs",
        "cloud_sync.rs",
        "sync_flow.rs",
    ] {
        files.push(crates.join("locora-runtime/src").join(name));
    }
    files
}

/// The purchase wording in `text`, one entry per finding.
fn selling(text: &str) -> Vec<String> {
    let lower = text.to_lowercase();
    let mut found: Vec<String> = PHRASES
        .iter()
        .filter(|phrase| lower.contains(*phrase))
        .map(|phrase| format!("{phrase:?}"))
        .collect();
    found.extend(
        lower
            .split(|c: char| !c.is_alphanumeric())
            .filter(|word| WORDS.contains(word))
            .map(|word| format!("the word {word:?}")),
    );
    found
}

/// Links in `text` to a host or path that is not allowed.
fn foreign_links(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    for scheme in ["https://", "http://"] {
        for (at, _) in text.match_indices(scheme) {
            let rest = &text[at + scheme.len()..];
            let end = rest
                .find(|c: char| !(c.is_ascii_alphanumeric() || "-._~:/?#@!$&*+=%".contains(c)))
                .unwrap_or(rest.len());
            let link = &rest[..end];
            let (host, path) = link.split_at(link.find('/').unwrap_or(link.len()));
            let host = host.split(':').next().unwrap_or(host);
            // A bare scheme (a check of user input) or a format string is not a link.
            if host.is_empty() || host.contains('{') {
                continue;
            }
            let allowed = HOSTS.iter().any(|(name, paths)| {
                *name == host
                    && (paths.is_empty() || paths.iter().any(|allowed| path.starts_with(allowed)))
            });
            if !allowed {
                found.push(format!("{scheme}{link}"));
            }
        }
    }
    found
}

#[test]
fn the_apps_show_no_purchase_wording_and_no_checkout_links() {
    let mut problems = Vec::new();
    for file in sources() {
        let text =
            fs::read_to_string(&file).unwrap_or_else(|error| panic!("{}: {error}", file.display()));
        // Unit tests at the end of a source show nothing.
        let shown = text.split("\n#[cfg(test)]").next().unwrap_or(&text);
        for (index, line) in shown.lines().enumerate() {
            for finding in selling(line).into_iter().chain(foreign_links(line)) {
                problems.push(format!("{}:{}: {finding}", file.display(), index + 1));
            }
        }
    }
    assert!(
        problems.is_empty(),
        "purchase wording or links in the native apps:\n{}",
        problems.join("\n")
    );
}

#[test]
fn the_check_finds_selling_and_leaves_neutral_texts_alone() {
    assert!(!selling("Mit einem Abo gibt es mehr Platz.").is_empty());
    assert!(!selling("Jetzt Abo abschließen").is_empty());
    assert!(!selling("Upgrade to Pro for 5 €").is_empty());
    assert!(!selling("Buy more space").is_empty());
    assert!(!selling("より多くの容量を購入").is_empty());
    assert!(selling("Abo: aktiv. Testzeitraum endet am 1. Mai").is_empty());
    assert!(selling("Die Daten bleiben, solange du synchronisierst").is_empty());
    assert!(selling("No places in the plan yet; the buyer").is_empty());
    assert_eq!(
        foreign_links("see https://web.locora.cloud/checkout?plan=year"),
        ["https://web.locora.cloud/checkout?plan=year"]
    );
    assert_eq!(
        foreign_links("pay at https://buy.paddle.com/x"),
        ["https://buy.paddle.com/x"]
    );
    assert!(foreign_links(r#"const URL: &str = "https://web.locora.cloud/forgot";"#).is_empty());
    assert!(foreign_links("https://api.locora.cloud/v1/sync").is_empty());
    assert!(foreign_links(r#"starts_with("https://") || format!("http://{}", host)"#).is_empty());
}
