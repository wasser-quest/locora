//! Auto-sync: once the person turned it on (the key is then kept on this device), the app syncs
//! by itself when it starts or comes back, about 30 seconds after the last change, and every ten
//! minutes while it is open. One sync at a time; after an error it waits longer each time (much
//! longer when the server asks to). It never sends a palace deletion on its own: it shows the
//! question and waits, and it pauses when a person has to act (sign in again, sync by hand).

use leptos::prelude::*;
use leptos::task::spawn_local;
use locora_app_api::{
    CMD_CLOUD_AUTO_STATE, CMD_CLOUD_AUTO_SYNC, CloudAutoOutcome, CloudAutoState, CloudPalaceChange,
    CloudSyncResult,
};
use serde::Serialize;

use crate::i18n::{t, tf};
use crate::tauri::{NoArgs, call};

/// Seconds without a new change before pending changes are sent.
const QUIET_SECS: f64 = 30.0;
/// Seconds between syncs while the app is open.
const EVERY_SECS: f64 = 600.0;
const FIRST_RETRY_SECS: f64 = 60.0;
const MAX_RETRY_SECS: f64 = 1800.0;
/// After "too many requests" the server is left alone for at least this long.
const RATE_LIMIT_SECS: f64 = 900.0;

/// What the cloud settings and the notice show about auto-sync.
#[derive(Clone, Default, PartialEq)]
pub struct AutoView {
    pub enabled: bool,
    pub running: bool,
    pub last_sync_at: Option<i64>,
    /// The last error, shown small; the driver retries by itself.
    pub error: Option<String>,
    /// A person has to act first (sign in again, sync by hand once).
    pub paused: Option<String>,
    /// Palace deletions waiting for a yes.
    pub confirm: Vec<CloudPalaceChange>,
    pub result: Option<CloudSyncResult>,
}

/// The driver's state and its controls, for every screen.
#[derive(Clone, Copy)]
pub struct AutoSync {
    pub view: RwSignal<AutoView>,
    /// Palace deletions the person agreed to.
    confirmed: StoredValue<Vec<String>>,
    /// Set to sync at the next tick, whatever the timers say.
    due: StoredValue<bool>,
}

impl AutoSync {
    /// Sync soon: after turning auto-sync on, after a confirmation, or when the app comes back.
    pub fn nudge(&self) {
        self.due.set_value(true);
        self.view.update(|view| view.paused = None);
    }

    /// The person agreed to these deletions: the next sync sends them.
    pub fn confirm(&self, palaces: &[CloudPalaceChange]) {
        self.confirmed
            .update_value(|ids| ids.extend(palaces.iter().map(|p| p.id.clone())));
        self.view.update(|view| view.confirm.clear());
        self.nudge();
    }

    /// Auto-sync was turned on or off in the settings.
    pub fn set_enabled(&self, enabled: bool) {
        self.view.update(|view| {
            view.enabled = enabled;
            view.error = None;
            view.paused = None;
        });
        if enabled {
            self.nudge();
        }
    }

    /// A sync by hand finished: the driver counts it as its own.
    pub fn synced(&self, at: i64) {
        self.view.update(|view| {
            view.last_sync_at = Some(at);
            view.paused = None;
            view.error = None;
        });
    }
}

pub fn use_auto_sync() -> Option<AutoSync> {
    use_context::<AutoSync>()
}

fn now() -> f64 {
    js_sys::Date::now() / 1000.0
}

#[derive(Serialize)]
struct SyncArgs {
    confirmed: Vec<String>,
}

/// Starts the driver; `changed` is bumped when a sync brought changes, so the start page reloads.
pub fn start(changed: RwSignal<u64>) -> AutoSync {
    let auto = AutoSync {
        view: RwSignal::new(AutoView::default()),
        confirmed: StoredValue::new(Vec::new()),
        due: StoredValue::new(true),
    };
    provide_context(auto);
    let running = StoredValue::new(false);
    let last_attempt = StoredValue::new(0.0_f64);
    let retry_at = StoredValue::new(0.0_f64);
    let failures = StoredValue::new(0_i32);
    // The newest local change and when this device first saw it.
    let seen_seq = StoredValue::new(None::<i64>);
    let seq_changed_at = StoredValue::new(0.0_f64);

    let tick = move || {
        if running.get_value() {
            return;
        }
        running.set_value(true);
        spawn_local(async move {
            let Ok(state) = call::<CloudAutoState>(CMD_CLOUD_AUTO_STATE, NoArgs {}).await else {
                running.set_value(false);
                return;
            };
            auto.view.update(|view| {
                view.enabled = state.enabled;
                if state.last_sync_at > view.last_sync_at {
                    view.last_sync_at = state.last_sync_at;
                }
            });
            let at = now();
            if seen_seq.get_value() != Some(state.local_seq) {
                seen_seq.set_value(Some(state.local_seq));
                seq_changed_at.set_value(at);
            }
            let view = auto.view.get_untracked();
            let waiting = !view.confirm.is_empty() || view.paused.is_some();
            let quiet = state.pending && at - seq_changed_at.get_value() >= QUIET_SECS;
            let due = auto.due.get_value() || quiet || at - last_attempt.get_value() >= EVERY_SECS;
            if !state.enabled || waiting || !due || at < retry_at.get_value() {
                running.set_value(false);
                return;
            }
            auto.due.set_value(false);
            last_attempt.set_value(at);
            auto.view.update(|view| view.running = true);
            let args = SyncArgs {
                confirmed: auto.confirmed.get_value(),
            };
            let outcome = call::<CloudAutoOutcome>(CMD_CLOUD_AUTO_SYNC, args)
                .await
                .unwrap_or_else(|error| CloudAutoOutcome {
                    enabled: true,
                    error: Some(error),
                    ..CloudAutoOutcome::default()
                });
            apply(auto, outcome, failures, retry_at, changed);
            auto.view.update(|view| view.running = false);
            running.set_value(false);
        });
    };
    tick();
    let every = set_interval_with_handle(tick, std::time::Duration::from_secs(10)).ok();
    // Back in the app (a window focused again, a phone unlocked): sync soon.
    let back = move || {
        auto.due.set_value(true);
        tick();
    };
    let focus = window_event_listener(leptos::ev::focus, move |_| back());
    let visible = window_event_listener(leptos::ev::visibilitychange, move |_| {
        let shown = web_sys::window()
            .and_then(|window| window.document())
            .is_some_and(|document| !document.hidden());
        if shown {
            back();
        }
    });
    on_cleanup(move || {
        focus.remove();
        visible.remove();
        if let Some(handle) = every {
            handle.clear();
        }
    });
    auto
}

fn apply(
    auto: AutoSync,
    outcome: CloudAutoOutcome,
    failures: StoredValue<i32>,
    retry_at: StoredValue<f64>,
    changed: RwSignal<u64>,
) {
    if !outcome.enabled {
        auto.view.update(|view| view.enabled = false);
        return;
    }
    if let Some(result) = outcome.result {
        failures.set_value(0);
        retry_at.set_value(0.0);
        auto.confirmed.set_value(Vec::new());
        if result.applied > 0 {
            changed.update(|revision| *revision += 1);
        }
        auto.view.update(|view| {
            view.last_sync_at = Some(now() as i64);
            view.error = None;
            view.paused = None;
            view.result = Some(result);
        });
    } else if !outcome.confirm.is_empty() {
        auto.view.update(|view| view.confirm = outcome.confirm);
    } else if let Some(paused) = outcome.paused {
        auto.view.update(|view| view.paused = Some(paused));
    } else if let Some(error) = outcome.error {
        let count = failures.get_value() + 1;
        failures.set_value(count);
        let mut wait = (FIRST_RETRY_SECS * 2f64.powi(count - 1)).min(MAX_RETRY_SECS);
        if outcome.rate_limited {
            wait = wait.max(RATE_LIMIT_SECS);
        }
        retry_at.set_value(now() + wait);
        auto.view.update(|view| view.error = Some(error));
    }
}

/// The question for palace deletions that wait, shown on every screen while auto-sync waits.
#[component]
pub fn DeletionNotice() -> impl IntoView {
    let Some(auto) = use_auto_sync() else {
        return ().into_any();
    };
    view! {
        {move || {
            let waiting = auto.view.get().confirm;
            (!waiting.is_empty()).then(|| {
                let palaces = StoredValue::new(waiting.clone());
                view! {
                    <aside class="card notes cloud-confirm" role="alert" style="margin:.6rem;">
                        <ConfirmText palaces=waiting />
                        <p>
                            <button class="danger" on:click=move |_| auto.confirm(&palaces.get_value())>
                                {t("Löschen bestätigen")}
                            </button>
                            " "
                            <button on:click=move |_| auto.view.update(|view| {
                                view.confirm.clear();
                                view.paused = Some(t("Auto-Sync wartet, bis du das Löschen bestätigst.").into());
                            })>{t("Später")}</button>
                        </p>
                    </aside>
                }
            })
        }}
    }
    .into_any()
}

/// One sentence per palace that a sync would delete everywhere.
#[component]
pub fn ConfirmText(palaces: Vec<CloudPalaceChange>) -> impl IntoView {
    palaces
        .into_iter()
        .map(|palace| {
            let name = palace.name.unwrap_or_else(|| t("(ohne Namen)").into());
            view! {
                <p><strong>{tf(
                    "Palast „{0}“ wird auch in der Cloud und auf deinen anderen Geräten gelöscht.",
                    &[&name],
                )}</strong></p>
            }
        })
        .collect_view()
}
