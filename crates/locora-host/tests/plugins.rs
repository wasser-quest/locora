//! End-to-end tests with real plugins compiled to WebAssembly components.

#[path = "support/host_services.rs"]
mod host_services;
#[path = "support/journey.rs"]
mod journey_tests;

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

#[test]
fn metadata_inspection_and_approval_mismatch_never_activate_or_touch_backends() {
    use locora_host::{KvStore, MemoryKv};
    use std::sync::Arc;
    let store = Arc::new(locora_store::Store::in_memory().unwrap());
    let kv = Arc::new(MemoryKv::default());
    let cache = tempfile::tempdir().unwrap();
    let mut host =
        PluginHost::with_palaces(Some(cache.path().into()), kv.clone(), store.clone()).unwrap();
    let path = wasm_dir().join("install_probe.wasm");
    let mut manifest = host.inspect_bytes(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(manifest.id, "install-probe");
    assert_eq!(host.plugins().count(), 0);
    assert!(host.drain_events().is_empty());
    assert_eq!(std::fs::read_dir(cache.path()).unwrap().count(), 0);
    assert!(store.palaces().unwrap().is_empty());
    assert!(kv.get("install-probe", "activated").unwrap().is_none());
    manifest.permissions.clear();
    let err = host.load_approved_file(&path, &manifest).unwrap_err();
    assert!(err.to_string().contains("bestätigten Installation"));
    assert!(kv.get("install-probe", "activated").unwrap().is_none());
    assert!(store.palaces().unwrap().is_empty());
}

#[test]
fn approved_profile_loading_rejects_duplicates_and_reserved_ids_before_activation() {
    let mut host = PluginHost::new(None).unwrap();
    load(&mut host, "fixture");
    let path = wasm_dir().join("fixture.wasm");
    let manifest = host.inspect_bytes(&std::fs::read(&path).unwrap()).unwrap();
    let err = host.load_approved_file(&path, &manifest).unwrap_err();
    assert!(err.to_string().contains("bereits geladen"));
    assert_eq!(host.plugins().count(), 1);
    for name in ["core", "review"] {
        let path = wasm_dir().join(format!("{name}.wasm"));
        let manifest = host.inspect_bytes(&std::fs::read(&path).unwrap()).unwrap();
        assert!(
            host.load_approved_file(&path, &manifest)
                .unwrap_err()
                .to_string()
                .contains("geschützt")
        );
        assert!(host.load_additional_file(&path).is_err());
    }
    assert_eq!(host.plugins().count(), 1);
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

#[test]
fn review_schedules_answers_and_forgets_deleted_loci() {
    use locora_api::core::{Grade, LocusAnswered, LocusDeleted, PalaceDeleted};
    use locora_api::review::{Schedule, ScheduleRequest};

    let mut host = PluginHost::new(None).unwrap();
    load(&mut host, "review");
    assert!(host.provides("review.schedule@1"));
    let t0 = 1_800_000_000;
    for (locus, grade) in [("l1", Grade::Good), ("l2", Grade::Again)] {
        host.publish(&LocusAnswered {
            palace_id: "p".into(),
            locus_id: locus.into(),
            correct: grade != Grade::Again,
            grade,
            elapsed_ms: 1200,
            answered_at: t0,
        })
        .unwrap();
    }
    let all = |host: &mut PluginHost| {
        host.call::<Schedule>(&ScheduleRequest { palace_id: None })
            .unwrap()
    };
    let scheduled = all(&mut host);
    assert_eq!(scheduled.len(), 2, "{:?}", host.drain_events());
    let good = scheduled.iter().find(|s| s.locus_id == "l1").unwrap();
    assert_eq!(good.card.due, t0 + 3 * 86_400);
    let again = scheduled.iter().find(|s| s.locus_id == "l2").unwrap();
    assert_eq!(again.card.due, t0 + 600);

    // Storage survives a reload of the plugin.
    load(&mut host, "review");
    assert_eq!(all(&mut host).len(), 2);

    host.publish(&LocusDeleted {
        palace_id: "p".into(),
        locus_id: "l1".into(),
    })
    .unwrap();
    assert_eq!(all(&mut host).len(), 1);
    host.publish(&PalaceDeleted {
        palace_id: "p".into(),
    })
    .unwrap();
    assert!(all(&mut host).is_empty());
}

#[test]
fn calling_a_missing_service_is_an_error() {
    let mut host = PluginHost::new(None).unwrap();
    load(&mut host, "lernzeit");
    let err = host.call_raw("nobody.home@1", &[]).unwrap_err();
    assert!(err.to_string().contains("nobody.home@1"));
}

#[test]
fn review_restores_imported_cards_and_rejects_bad_ones() {
    use locora_api::review::{CardState, Restore, RestoreRequest, Schedule, ScheduleRequest};

    let mut host = PluginHost::new(None).unwrap();
    load(&mut host, "review");
    let card = CardState {
        stability: 3.2,
        difficulty: 5.0,
        last_review: 1_800_000_000,
        due: 1_800_259_200,
        reps: 1,
        lapses: 0,
    };
    let restored = host
        .call::<Restore>(&RestoreRequest {
            palace_id: "p".into(),
            cards: vec![("l1".into(), card), ("l2".into(), card)],
        })
        .unwrap();
    assert_eq!(restored, 2);
    let scheduled = host
        .call::<Schedule>(&ScheduleRequest {
            palace_id: Some("p".into()),
        })
        .unwrap();
    assert_eq!(scheduled.len(), 2);
    assert_eq!(scheduled[0].card, card);

    let bad = CardState {
        difficulty: 42.0,
        ..card
    };
    let err = host
        .call::<Restore>(&RestoreRequest {
            palace_id: "p".into(),
            cards: vec![("l3".into(), bad)],
        })
        .unwrap_err();
    assert!(
        err.to_string().contains("Ungültiger Wiederholungsstand"),
        "{err}"
    );
    assert_eq!(
        host.call::<Schedule>(&ScheduleRequest {
            palace_id: Some("p".into())
        })
        .unwrap()
        .len(),
        2,
        "a rejected restore changes nothing"
    );
}
