//! End-to-end tests with real plugins compiled to WebAssembly components.

use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;

use locora_api::core::{JourneyCompleted, PalaceCreated};
use locora_api::ui::Tile;
use locora_host::{HostEvent, PluginHost};

/// Build the plugin workspace once and return the directory with the `.wasm` files.
fn wasm_dir() -> &'static PathBuf {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let plugins = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins");
        let status = Command::new(env!("CARGO"))
            .args(["build", "--release", "--target", "wasm32-wasip2"])
            .current_dir(&plugins)
            .status()
            .expect("cargo build for plugins");
        assert!(status.success(), "building test plugins failed");
        plugins.join("target/wasm32-wasip2/release")
    })
}

fn load(host: &mut PluginHost, name: &str) {
    host.load_file(&wasm_dir().join(format!("{name}.wasm")))
        .unwrap_or_else(|e| panic!("load {name}: {e:#}"));
}

fn journey(secs: u64) -> JourneyCompleted {
    JourneyCompleted {
        palace_id: "p".into(),
        correct: 1,
        total: 1,
        duration_secs: secs,
    }
}

fn palace(name: &str) -> PalaceCreated {
    PalaceCreated {
        palace_id: "p".into(),
        name: name.into(),
    }
}

fn tiles(events: &[HostEvent]) -> Vec<String> {
    events
        .iter()
        .filter_map(|e| match e {
            HostEvent::Published { topic, payload, .. } if topic == "ui.tile" => {
                Some(locora_api::decode::<Tile>(payload).unwrap().text)
            }
            _ => None,
        })
        .collect()
}

#[test]
fn hot_reload_keeps_storage() {
    let mut host = PluginHost::new(None).unwrap();
    load(&mut host, "lernzeit");
    host.publish(&journey(90)).unwrap();
    assert_eq!(tiles(&host.drain_events()), ["1 min"]);

    load(&mut host, "lernzeit");
    let events = host.drain_events();
    assert!(matches!(events[0], HostEvent::Reloaded(ref m) if m.id == "lernzeit"));
    host.publish(&journey(60)).unwrap();
    assert_eq!(tiles(&host.drain_events()), ["2 min"]);

    assert!(host.unload("lernzeit"));
    load(&mut host, "lernzeit");
    host.publish(&journey(60)).unwrap();
    assert_eq!(tiles(&host.drain_events()), ["3 min"]);
}

#[test]
fn panicking_plugin_is_disabled_without_affecting_others() {
    let mut host = PluginHost::new(None).unwrap();
    load(&mut host, "lernzeit");
    load(&mut host, "fixture");
    host.drain_events();

    host.publish(&palace("panic")).unwrap();
    let events = host.drain_events();
    assert!(
        events
            .iter()
            .any(|e| matches!(e, HostEvent::Disabled { plugin, .. } if plugin == "fixture")),
        "{events:?}"
    );
    assert_eq!(
        host.plugins().map(|m| m.id.as_str()).collect::<Vec<_>>(),
        ["lernzeit"]
    );

    host.publish(&journey(60)).unwrap();
    assert_eq!(tiles(&host.drain_events()), ["1 min"]);
}

#[test]
fn endless_loop_runs_out_of_fuel() {
    let mut host = PluginHost::new(None).unwrap();
    load(&mut host, "fixture");
    host.publish(&palace("loop")).unwrap();
    assert!(
        host.drain_events()
            .iter()
            .any(|e| matches!(e, HostEvent::Disabled { reason, .. } if reason.contains("fuel")))
    );
}

#[test]
fn storage_requires_permission() {
    let mut host = PluginHost::new(None).unwrap();
    load(&mut host, "fixture");
    host.publish(&palace("storage")).unwrap();
    let events = host.drain_events();
    assert!(
        events.iter().any(|e| matches!(e,
            HostEvent::HandlerError { message, .. } if message.contains("`storage`"))),
        "{events:?}"
    );
    // A handler error does not disable the plugin.
    assert_eq!(host.plugins().count(), 1);
}

#[test]
fn plugins_do_not_receive_their_own_events() {
    let mut host = PluginHost::new(None).unwrap();
    load(&mut host, "fixture");
    host.drain_events();
    host.publish(&palace("echo")).unwrap();
    let published: Vec<_> = host
        .drain_events()
        .into_iter()
        .filter(|e| matches!(e, HostEvent::Published { .. }))
        .collect();
    assert_eq!(published.len(), 1, "{published:?}");
}

#[test]
fn compiled_plugins_are_cached() {
    let cache = std::env::temp_dir().join(format!("locora-cache-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&cache);
    let mut host = PluginHost::new(Some(cache.clone())).unwrap();
    load(&mut host, "lernzeit");
    let cached = std::fs::read_dir(&cache).unwrap().count();
    assert_eq!(cached, 1);

    let mut host = PluginHost::new(Some(cache.clone())).unwrap();
    load(&mut host, "lernzeit");
    host.publish(&journey(60)).unwrap();
    assert_eq!(tiles(&host.drain_events()), ["1 min"]);
    let _ = std::fs::remove_dir_all(&cache);
}
