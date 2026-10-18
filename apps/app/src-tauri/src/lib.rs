//! Locora app backend: opens the same profile as the CLI, answers the UI's
//! commands and serves media and `.sply` animations over `locora://`.

mod assets;
mod drops;
mod editor;
mod journey;
mod media;
#[cfg(any(mobile, test))]
#[cfg_attr(not(mobile), allow(dead_code))]
mod mobile_files;
mod settings;
mod sply;
mod transfer;
mod views;

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use locora_app_api::{
    ActionDone, Feedback, Home, ImportResult, InputField, JourneyStart, NotificationView,
    PalaceView, Question, Reply, Revealed, SettingsChange, SettingsView, Step, Summary,
};
use locora_runtime::Profile;
use locora_store::SPLINELY;
use tauri::http::{Response, StatusCode, header};
use tauri::{Manager, State};

use journey::Journey;

pub struct Backend {
    profile: Profile,
    journey: Option<Journey>,
    /// Hash of the built-in locus marker in the store.
    default_marker: String,
    /// Hash of Simo (welcome and empty states) in the store.
    simo: String,
    /// The app's own graphics.
    assets: assets::Assets,
    /// Protocol kind + `.sply` hash → immutable SVG template, converted once per run.
    svg: HashMap<String, String>,
    data: std::path::PathBuf,
    cloud_sync_busy: Arc<AtomicBool>,
}

type Shared = Mutex<Backend>;
// Native UI callbacks must never wait for profile/import operations.
type DropInbox = Mutex<drops::Inbox>;

/// The running journey; a finished one only takes actions on its last answer.
fn running(journey: Option<&mut Journey>) -> Result<&mut Journey, String> {
    match journey {
        Some(j) if !j.done() => Ok(j),
        Some(_) => Err("Die Journey ist beendet".into()),
        None => Err("Keine Journey aktiv".into()),
    }
}

fn err(e: anyhow::Error) -> String {
    format!("{e:#}")
}

fn lock<'a>(state: &'a State<'_, Shared>) -> Result<std::sync::MutexGuard<'a, Backend>, String> {
    state
        .lock()
        .map_err(|_| "Interner Fehler: Zustand blockiert".to_owned())
}

#[tauri::command]
async fn home(state: State<'_, Shared>) -> Result<Home, String> {
    let mut b = lock(&state)?;
    let b = &mut *b;
    let mut home = views::home(&mut b.profile).map_err(err)?;
    for tile in &mut home.tiles {
        tile.icon = Some(b.assets.tile_icon(&tile.plugin));
    }
    if home.palaces.is_empty() {
        home.welcome = Some(locora_app_api::Media {
            hash: b.simo.clone(),
            kind: locora_app_api::MediaKind::Sply,
        });
    }
    Ok(home)
}

#[tauri::command]
async fn palace(state: State<'_, Shared>, id: String) -> Result<PalaceView, String> {
    let mut b = lock(&state)?;
    let marker = b.default_marker.clone();
    views::palace(&mut b.profile, &id, &marker).map_err(err)
}

#[tauri::command]
async fn editor_change(
    state: State<'_, Shared>,
    change: locora_app_api::EditorChange,
) -> Result<locora_app_api::EditorUpdated, String> {
    let mut b = lock(&state)?;
    // Do all fallible plugin reads before the write. A successful write must not
    // look like a failed command just because a subsequent schedule read failed.
    let cards = b.profile.schedule(Some(&change.palace_id)).map_err(err)?;
    let (tree, selected, receipt) =
        editor::apply_with_history(&b.profile.store, &change).map_err(err)?;
    let palace = views::palace_view(&b.profile.store, &tree, &cards, &b.default_marker);
    Ok(locora_app_api::EditorUpdated {
        palace,
        selected,
        receipt,
    })
}

/// The WebView supplies only scope; the native picker is the sole path authority.
#[tauri::command]
async fn editor_pick_picture(
    app: tauri::AppHandle,
    state: State<'_, Shared>,
    target: locora_app_api::PictureTarget,
) -> Result<Option<locora_app_api::EditorUpdated>, String> {
    use tauri_plugin_dialog::DialogExt;
    {
        let b = lock(&state)?;
        editor::picture_target(&b.profile.store, &target).map_err(err)?;
    }
    // Blocking native dialog off the main thread, without holding the profile lock.
    let picker_app = app.clone();
    let selected = tauri::async_runtime::spawn_blocking(move || {
        picker_app
            .dialog()
            .file()
            .set_title("Bild am Ort wählen")
            .add_filter(
                "Bilder und Animationen",
                &["png", "jpg", "jpeg", "webp", "gif", "avif", "sply"],
            )
            .blocking_pick_file()
    })
    .await
    .map_err(|e| e.to_string())?;
    let Some(selected) = selected else {
        return Ok(None);
    };
    #[cfg(mobile)]
    let staged = mobile_files::stage(&app, selected, mobile_files::Kind::Picture)?;
    #[cfg(mobile)]
    let path = staged.path().to_path_buf();
    #[cfg(desktop)]
    let path = selected
        .into_path()
        .map_err(|_| "Diese Auswahl ist keine lokale Datei".to_owned())?;
    let mut b = lock(&state)?;
    let cards = b.profile.schedule(Some(&target.palace_id)).map_err(err)?;
    let (tree, selected) = editor::import_picture(&b.profile.store, &target, &path).map_err(err)?;
    let palace = views::palace_view(&b.profile.store, &tree, &cards, &b.default_marker);
    Ok(Some(locora_app_api::EditorUpdated {
        palace,
        selected,
        receipt: None,
    }))
}

/// Only the native dialog can authorize a local path; the WebView sends no path.
#[tauri::command]
async fn home_pick_media(
    app: tauri::AppHandle,
    state: State<'_, Shared>,
) -> Result<Option<PalaceView>, String> {
    use tauri_plugin_dialog::DialogExt;
    let picker_app = app.clone();
    let selected = tauri::async_runtime::spawn_blocking(move || {
        picker_app
            .dialog()
            .file()
            .set_title("Neuen Palast aus Bild oder Video anlegen")
            .add_filter(
                "Bilder, Animationen und Videos",
                &[
                    "png", "jpg", "jpeg", "webp", "gif", "avif", "sply", "mp4", "m4v", "webm",
                    "mov",
                ],
            )
            .blocking_pick_file()
    })
    .await
    .map_err(|e| format!("Dateiauswahl fehlgeschlagen: {e}"))?;
    #[cfg(mobile)]
    let staged = selected
        .map(|s| mobile_files::stage(&app, s, mobile_files::Kind::Media))
        .transpose()?;
    #[cfg(mobile)]
    let path = staged.as_ref().map(|s| s.path());
    #[cfg(desktop)]
    let path = selected
        .map(|s| {
            s.into_path()
                .map_err(|_| "Diese Auswahl ist keine lokale Datei".to_owned())
        })
        .transpose()?;
    let b = lock(&state)?;
    #[cfg(mobile)]
    let tree = editor::import_palace(&b.profile.store, path).map_err(err)?;
    #[cfg(desktop)]
    let tree = editor::import_palace(&b.profile.store, path.as_deref()).map_err(err)?;
    // New rooms contain no loci: no fallible plugin call after committing.
    Ok(tree.map(|tree| {
        views::palace_view(&b.profile.store, &tree, &HashMap::new(), &b.default_marker)
    }))
}

/// Import only the path authorized by a recent native drop, never a UI path.
#[tauri::command]
async fn home_import_drop(
    state: State<'_, Shared>,
    drops: State<'_, DropInbox>,
    ticket: String,
) -> Result<PalaceView, String> {
    let path = drops
        .lock()
        .map_err(|_| "Dateifreigabe blockiert".to_owned())?
        .take(&ticket)
        .map_err(str::to_owned)?;
    let b = lock(&state)?;
    let tree = editor::import_palace(&b.profile.store, Some(&path))
        .map_err(err)?
        .ok_or_else(|| "Keine Datei verfügbar".to_owned())?;
    Ok(views::palace_view(
        &b.profile.store,
        &tree,
        &HashMap::new(),
        &b.default_marker,
    ))
}

#[tauri::command]
async fn editor_import_drop(
    state: State<'_, Shared>,
    drops: State<'_, DropInbox>,
    ticket: String,
    target: locora_app_api::PictureTarget,
) -> Result<locora_app_api::EditorUpdated, String> {
    let mut b = lock(&state)?;
    editor::picture_target(&b.profile.store, &target).map_err(err)?;
    let cards = b.profile.schedule(Some(&target.palace_id)).map_err(err)?;
    let path = drops
        .lock()
        .map_err(|_| "Dateifreigabe blockiert".to_owned())?
        .take(&ticket)
        .map_err(str::to_owned)?;
    let (tree, selected) = editor::import_picture(&b.profile.store, &target, &path).map_err(err)?;
    Ok(locora_app_api::EditorUpdated {
        palace: views::palace_view(&b.profile.store, &tree, &cards, &b.default_marker),
        selected,
        receipt: None,
    })
}

/// Pick a `.locora` archive or TOML source; `None` when the dialog was cancelled.
#[tauri::command]
async fn import_pick(
    app: tauri::AppHandle,
    state: State<'_, Shared>,
) -> Result<Option<ImportResult>, String> {
    use tauri_plugin_dialog::DialogExt;
    let picker_app = app.clone();
    let selected = tauri::async_runtime::spawn_blocking(move || {
        picker_app
            .dialog()
            .file()
            .set_title("Paläste importieren")
            .add_filter("Locora-Paläste", &["locora", "toml"])
            .blocking_pick_file()
    })
    .await
    .map_err(|e| format!("Dateiauswahl fehlgeschlagen: {e}"))?;
    let Some(selected) = selected else {
        return Ok(None);
    };
    #[cfg(mobile)]
    let staged = mobile_files::stage(&app, selected, mobile_files::Kind::Archive)?;
    #[cfg(mobile)]
    let path = staged.path().to_path_buf();
    #[cfg(desktop)]
    let path = selected
        .into_path()
        .map_err(|_| "Diese Auswahl ist keine lokale Datei".to_owned())?;
    let mut b = lock(&state)?;
    transfer::import_path(&mut b.profile, &path)
        .map(Some)
        .map_err(err)
}

/// Save one palace (with its review schedule) as `.locora`; `None` when cancelled.
#[tauri::command]
async fn export_pick(
    app: tauri::AppHandle,
    state: State<'_, Shared>,
    id: String,
) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    let suggested = {
        let b = lock(&state)?;
        transfer::suggested_name(&b.profile, &id).map_err(err)?
    };
    let picker_app = app.clone();
    let selected = tauri::async_runtime::spawn_blocking(move || {
        picker_app
            .dialog()
            .file()
            .set_title("Palast exportieren")
            .set_file_name(suggested)
            .add_filter("Locora-Paläste", &["locora"])
            .blocking_save_file()
    })
    .await
    .map_err(|e| format!("Dateiauswahl fehlgeschlagen: {e}"))?;
    let Some(selected) = selected else {
        return Ok(None);
    };
    #[cfg(mobile)]
    {
        let staged = tempfile::Builder::new()
            .suffix(".locora")
            .tempfile()
            .map_err(|e| format!("Export vorbereiten: {e}"))?;
        {
            let mut b = lock(&state)?;
            transfer::export_path(&mut b.profile, &id, staged.path()).map_err(err)?;
        }
        mobile_files::save(&app, selected, staged.path())?;
        return Ok(Some("Palast als .locora exportiert".to_owned()));
    }
    #[cfg(desktop)]
    let path = selected
        .into_path()
        .map_err(|_| "Dieses Ziel ist keine lokale Datei".to_owned())?;
    #[cfg(desktop)]
    let mut b = lock(&state)?;
    #[cfg(desktop)]
    transfer::export_path(&mut b.profile, &id, &transfer::with_extension(path))
        .map(Some)
        .map_err(err)
}

/// Export a TOML source and its media to a chosen local directory.
#[cfg(desktop)]
#[tauri::command]
async fn dump_pick(
    app: tauri::AppHandle,
    state: State<'_, Shared>,
    id: String,
) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    let selected = tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .set_title("Ordner für TOML und Medien wählen")
            .blocking_pick_folder()
    })
    .await
    .map_err(|e| format!("Ordnerauswahl fehlgeschlagen: {e}"))?;
    let Some(selected) = selected else {
        return Ok(None);
    };
    let path = selected
        .into_path()
        .map_err(|_| "Dieses Ziel ist kein lokaler Ordner".to_owned())?;
    let b = lock(&state)?;
    transfer::dump_path(&b.profile, &id, &path)
        .map(Some)
        .map_err(err)
}

// The dialog plugin has no mobile directory picker. Keep the build working and
// fail explicitly instead of silently choosing an app-private destination.
#[cfg(mobile)]
#[tauri::command]
async fn dump_pick(
    app: tauri::AppHandle,
    state: State<'_, Shared>,
    id: String,
) -> Result<Option<String>, String> {
    let _ = (app, state, id);
    Err("TOML-Ordnerexport ist nur auf dem Desktop verfügbar; bitte .locora exportieren".into())
}

/// Build (or rebuild) the tutorial palace; returns its id.
#[tauri::command]
async fn tutorial(state: State<'_, Shared>) -> Result<String, String> {
    let mut b = lock(&state)?;
    let (tree, _) = locora_runtime::tutorial::install(&mut b.profile).map_err(err)?;
    Ok(tree.palace.id)
}

#[tauri::command]
async fn tour(
    state: State<'_, Shared>,
    action: locora_app_api::TourAction,
) -> Result<Option<locora_app_api::HighlightRequest>, String> {
    let mut b = lock(&state)?;
    b.profile.tour(action).map_err(err)
}

/// Open the existing formula guide or build it once.
#[tauri::command]
async fn formula_guide(state: State<'_, Shared>) -> Result<String, String> {
    let mut b = lock(&state)?;
    if let Some(existing) = locora_runtime::formula_guide::find(&b.profile).map_err(err)? {
        return Ok(existing.id);
    }
    let (tree, _) = locora_runtime::formula_guide::install(&mut b.profile).map_err(err)?;
    Ok(tree.palace.id)
}

/// Everything that may be shown now (quiet hours, muting and the daily
/// per-channel limit were already applied by the plugin).
#[tauri::command]
async fn notifications(state: State<'_, Shared>) -> Result<Vec<NotificationView>, String> {
    let mut b = lock(&state)?;
    let messages = b.profile.notifications().map_err(err)?;
    Ok(messages
        .into_iter()
        .map(|message| NotificationView {
            channel: message.channel,
            priority: message.priority,
            title: message.title,
            body: message.body,
            action: message.action,
        })
        .collect())
}

/// Confirm that a notification was shown, so the queue drops it.
#[tauri::command]
async fn notification_shown(
    state: State<'_, Shared>,
    notification: NotificationView,
) -> Result<(), String> {
    let mut b = lock(&state)?;
    b.profile
        .shown_notification(&locora_api::notify::Message {
            plugin: "notify".into(),
            channel: notification.channel,
            priority: notification.priority,
            title: notification.title,
            body: notification.body,
            action: notification.action,
            // The plugin reads only channel/priority/title/body, but the
            // full message travels for the history.
            shown_at: locora_runtime::now(),
        })
        .map_err(err)?;
    Ok(())
}

#[tauri::command]
async fn journey_start(state: State<'_, Shared>, start: JourneyStart) -> Result<Step, String> {
    let mut b = lock(&state)?;
    let b = &mut *b;
    if let Some(mut old) = b.journey.take() {
        old.finish(&mut b.profile).map_err(err)?;
    }
    match Journey::start(&mut b.profile, start).map_err(err)? {
        None => Ok(Step::Done(Summary::default())),
        Some(journey) => {
            let question: Question = journey.question(&b.profile, &b.default_marker);
            b.journey = Some(journey);
            Ok(Step::Question(question))
        }
    }
}

#[tauri::command]
async fn ui_assets(state: State<'_, Shared>) -> Result<locora_app_api::UiAssets, String> {
    Ok(lock(&state)?.assets.ui())
}

#[tauri::command]
async fn gamification_status(
    state: State<'_, Shared>,
) -> Result<Option<locora_app_api::GamificationStatus>, String> {
    lock(&state)?.profile.gamification_status().map_err(err)
}

#[tauri::command]
async fn settings(state: State<'_, Shared>) -> Result<SettingsView, String> {
    let mut b = lock(&state)?;
    settings::view(&mut b.profile).map_err(err)
}

#[tauri::command]
async fn cloud_status(state: State<'_, Shared>) -> Result<locora_app_api::CloudStatus, String> {
    let data = lock(&state)?.data.clone();
    let saved = locora_runtime::cloud::load(&data).map_err(err)?;
    Ok(locora_app_api::CloudStatus {
        account: saved.as_ref().map(|state| state.account.email.clone()),
        access: saved.as_ref().is_some_and(|state| {
            locora_runtime::cloud::access(state, locora_runtime::cloud::now_secs()).is_some()
        }),
    })
}

/// Argon2 and network I/O run off the UI thread. Only one sync may mutate the
/// profile's cursors at once; other app actions keep access to the backend.
#[tauri::command]
async fn cloud_sync(
    app: tauri::AppHandle,
    passphrase: String,
) -> Result<locora_app_api::CloudSyncResult, String> {
    let state = app.state::<Shared>();
    let (data, store, busy) = {
        let backend = lock(&state)?;
        (
            backend.data.clone(),
            backend.profile.store.clone(),
            backend.cloud_sync_busy.clone(),
        )
    };
    if busy.swap(true, Ordering::AcqRel) {
        return Err("Ein Cloud-Sync läuft bereits".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        struct Reset(Arc<AtomicBool>);
        impl Drop for Reset {
            fn drop(&mut self) {
                self.0.store(false, Ordering::Release);
            }
        }
        let _reset = Reset(busy);
        let report = locora_runtime::cloud_sync::sync_with_passphrase(&data, &store, &passphrase)
            .map_err(err)?;
        Ok(locora_app_api::CloudSyncResult {
            pushed: report.pushed,
            media_uploaded: report.media_uploaded,
            pulled: report.pulled,
            media_downloaded: report.media_downloaded,
            applied: report.applied.applied,
            stale: report.applied.stale,
            missing: report.applied.missing,
            failed: report.applied.failed.len(),
            quarantined: report.quarantined.len(),
            cloud_expires_at: report.cloud_expires_at,
        })
    })
    .await
    .map_err(|error| format!("Cloud-Sync konnte nicht abgeschlossen werden: {error}"))?
}

/// The UI sends the system's UTC offset at every start (summer time included).
#[tauri::command]
async fn set_utc_offset(
    state: State<'_, Shared>,
    offset: locora_app_api::UtcOffset,
) -> Result<(), String> {
    let b = lock(&state)?;
    b.profile.set_utc_offset(offset.minutes).map_err(err)
}

#[tauri::command]
async fn settings_optimize(
    state: State<'_, Shared>,
) -> Result<locora_app_api::OptimizeDone, String> {
    let mut b = lock(&state)?;
    settings::optimize(&mut b.profile).map_err(err)
}

#[tauri::command]
async fn settings_set(
    state: State<'_, Shared>,
    change: SettingsChange,
) -> Result<SettingsView, String> {
    let mut b = lock(&state)?;
    settings::change(&mut b.profile, change).map_err(err)
}

/// Workbook inputs to ask before `journey_start`.
#[tauri::command]
async fn journey_inputs(
    state: State<'_, Shared>,
    start: JourneyStart,
) -> Result<Vec<InputField>, String> {
    let b = lock(&state)?;
    journey::inputs(&b.profile, &start).map_err(err)
}

/// Run a workbook action (`!name`) on the current question.
#[tauri::command]
async fn journey_action(
    state: State<'_, Shared>,
    key: String,
    revealed: bool,
    after: Option<bool>,
) -> Result<ActionDone, String> {
    let mut b = lock(&state)?;
    let b = &mut *b;
    if after.unwrap_or(false) {
        let journey = b.journey.as_mut().ok_or("Keine Journey aktiv")?;
        journey.action_after(&b.profile, &key, &b.default_marker)
    } else {
        running(b.journey.as_mut())?.action(&b.profile, &key, revealed, &b.default_marker)
    }
    .map_err(err)
}

#[tauri::command]
async fn journey_reveal(state: State<'_, Shared>) -> Result<Revealed, String> {
    let mut b = lock(&state)?;
    let b = &mut *b;
    let journey = running(b.journey.as_mut())?;
    Ok(journey.reveal(&b.profile))
}

#[tauri::command]
async fn journey_answer(state: State<'_, Shared>, reply: Reply) -> Result<Feedback, String> {
    let mut b = lock(&state)?;
    let b = &mut *b;
    let journey = running(b.journey.as_mut())?;
    let feedback = journey
        .answer(&mut b.profile, reply, &b.default_marker)
        .map_err(err)?;
    // A finished journey stays for actions on its last answer; the next
    // start replaces it (`finish` reports completed palaces only once).
    Ok(feedback)
}

/// Stop the current journey early; completed palaces still count.
#[tauri::command]
async fn journey_stop(state: State<'_, Shared>) -> Result<Summary, String> {
    let mut b = lock(&state)?;
    let b = &mut *b;
    match b.journey.take() {
        Some(mut journey) => journey.finish(&mut b.profile).map_err(err),
        None => Ok(Summary::default()),
    }
}

fn respond(status: StatusCode, mime: &str, body: Vec<u8>) -> Response<Cow<'static, [u8]>> {
    let mut response = Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, mime)
        .header(header::CACHE_CONTROL, "max-age=31536000, immutable")
        .header("X-Content-Type-Options", "nosniff");
    if mime == "image/svg+xml" {
        response = response.header("Content-Security-Policy", sply::CSP);
    }
    response
        .body(Cow::Owned(body))
        .expect("valid response parts")
}

fn not_found() -> Response<Cow<'static, [u8]>> {
    respond(
        StatusCode::NOT_FOUND,
        "text/plain",
        b"nicht gefunden".to_vec(),
    )
}

/// Content-addressed media, standalone `.sply` and shared room-marker layers.
fn serve(
    backend: &mut Backend,
    path: &str,
    range: Option<&str>,
    head: bool,
) -> Response<Cow<'static, [u8]>> {
    // `convertFileSrc` percent-encodes the slash between kind and hash.
    let path = path.replace("%2F", "/").replace("%2f", "/");
    let mut parts = path.trim_start_matches('/').splitn(2, '/');
    let (Some(kind), Some(hash)) = (parts.next(), parts.next()) else {
        return not_found();
    };
    if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        return not_found();
    }
    let store = &backend.profile.store;
    let Ok(Some(found)) = store.media(hash) else {
        return not_found();
    };
    let Some(file) = store.media_file(hash) else {
        return not_found();
    };
    match kind {
        "media" => match media::serve(&file, &found.mime, range, head) {
            Ok(response) => response,
            Err(_) => not_found(),
        },
        "sply" | "markers" if found.mime == SPLINELY => {
            let key = format!("{kind}/{hash}");
            if let Some(svg) = backend.svg.get(&key) {
                return respond(StatusCode::OK, "image/svg+xml", svg.clone().into_bytes());
            }
            let converted = std::fs::read(&file)
                .map_err(anyhow::Error::from)
                .and_then(|bytes| {
                    if kind == "markers" {
                        sply::to_marker_layer(&bytes)
                    } else {
                        sply::to_svg(&bytes)
                    }
                });
            match converted {
                Ok(svg) => {
                    backend.svg.insert(key, svg.clone());
                    respond(StatusCode::OK, "image/svg+xml", svg.into_bytes())
                }
                Err(e) => respond(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "text/plain; charset=utf-8",
                    format!("{e:#}").into_bytes(),
                ),
            }
        }
        _ => not_found(),
    }
}

fn open(app: &tauri::App) -> anyhow::Result<Backend> {
    // Desktop shares the CLI's profile; mobile keeps it in the app's sandbox.
    #[cfg(mobile)]
    let data = Some(app.path().app_data_dir()?);
    #[cfg(not(mobile))]
    let data: Option<std::path::PathBuf> = {
        let _ = app;
        None
    };
    let data = locora_runtime::data_dir(data.as_deref())?;
    let profile = Profile::open(Some(&data))?;
    let default_marker = profile
        .store
        .put_media(locora_runtime::tutorial::DEFAULT_MARKER, SPLINELY)?
        .hash;
    let simo = profile
        .store
        .put_media(locora_runtime::tutorial::SIMO, SPLINELY)?
        .hash;
    let assets = assets::Assets::register(&profile.store)?;
    Ok(Backend {
        profile,
        journey: None,
        default_marker,
        simo,
        assets,
        svg: HashMap::new(),
        data,
        cloud_sync_busy: Arc::new(AtomicBool::new(false)),
    })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let mut context = tauri::generate_context!();
    // A test or explicit alternate profile must also own the WebKit storage.
    // Otherwise localStorage preferences leak between otherwise isolated DBs.
    if let Some(data_dir) = std::env::var_os("LOCORA_DATA") {
        let path = std::path::PathBuf::from(data_dir);
        let path = if path.is_absolute() {
            path
        } else {
            std::env::current_dir()
                .expect("Arbeitsverzeichnis bestimmen")
                .join(path)
        };
        context.config_mut().app.app_directories_override =
            Some(tauri::utils::config::AppDirectoriesOverride::Root(path));
    }
    tauri::Builder::default()
        .manage(DropInbox::default())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .setup(|app| {
            let backend = open(app).map_err(|e| format!("Profil öffnen: {e:#}"))?;
            app.manage::<Shared>(Mutex::new(backend));
            Ok(())
        })
        // A window's own content webview reports drops as a window event
        // (`WebviewEvent::DragDrop` is only for additional child webviews).
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::DragDrop(tauri::DragDropEvent::Drop { paths, .. }) = event {
                let state = window.app_handle().state::<DropInbox>();
                let result = state
                    .lock()
                    .map_err(|_| "Interner Fehler: Zustand blockiert")
                    .and_then(|mut inbox| inbox.offer(paths));
                // Only JSON-encoded tickets/errors enter the renderer. Native paths stay here.
                let detail = match result {
                    Ok(ticket) => serde_json::json!({"ticket": ticket}),
                    Err(error) => serde_json::json!({"error": error}),
                };
                if let Some(webview) = window.app_handle().get_webview_window(window.label()) {
                    let _ = webview.eval(format!(
                        "window.dispatchEvent(new CustomEvent('locora-file-drop', {{detail:{detail}}}))"
                    ));
                }
            }
        })
        .register_uri_scheme_protocol(locora_app_api::SCHEME, |ctx, request| {
            let state = ctx.app_handle().state::<Shared>();
            let Ok(mut backend) = state.lock() else {
                return not_found();
            };
            serve(
                &mut backend,
                request.uri().path(),
                request
                    .headers()
                    .get(header::RANGE)
                    .and_then(|v| v.to_str().ok()),
                request.method() == tauri::http::Method::HEAD,
            )
        })
        .invoke_handler(tauri::generate_handler![
            home,
            palace,
            notifications,
            notification_shown,
            editor_change,
            editor_pick_picture,
            home_pick_media,
            home_import_drop,
            editor_import_drop,
            tutorial,
            tour,
            formula_guide,
            journey_inputs,
            settings,
            cloud_status,
            cloud_sync,
            ui_assets,
            gamification_status,
            import_pick,
            export_pick,
            dump_pick,
            settings_set,
            settings_optimize,
            set_utc_offset,
            journey_start,
            journey_action,
            journey_reveal,
            journey_answer,
            journey_stop
        ])
        .run(context)
        .expect("Locora konnte nicht starten");
}
