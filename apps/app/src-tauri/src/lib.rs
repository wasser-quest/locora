//! Locora app backend: opens the same profile as the CLI, answers the UI's
//! commands and serves media and `.sply` animations over `locora://`.

mod cloud_auto;
mod cloud_setup;
mod cloud_social;
mod drops;
mod keystore;
pub use locora_app_core::journey;
use locora_app_core::{assets, competition, editor, settings, sply, transfer, workbook};
mod media;
#[cfg(any(mobile, test))]
#[cfg_attr(not(mobile), allow(dead_code))]
mod mobile_files;
mod share;
pub use locora_app_core::views;

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

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
    /// The memory contest's running round.
    compete: competition::Contest,
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

/// The profile opens in the background while the window is already on screen. Commands wait for
/// it (see `lock`); the media protocol answers "not found" until then.
pub struct Shared {
    opening: Mutex<Opening>,
    ready: std::sync::Condvar,
}

enum Opening {
    Pending,
    Ready(Box<Backend>),
    Failed(String),
}

impl Shared {
    fn new() -> Self {
        Self {
            opening: Mutex::new(Opening::Pending),
            ready: std::sync::Condvar::new(),
        }
    }

    fn finish(&self, result: Result<Backend, String>) {
        let mut opening = self
            .opening
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *opening = match result {
            Ok(backend) => Opening::Ready(Box::new(backend)),
            Err(error) => Opening::Failed(error),
        };
        self.ready.notify_all();
    }
}

/// The backend, once it is open.
pub struct BackendGuard<'a>(std::sync::MutexGuard<'a, Opening>);

impl std::ops::Deref for BackendGuard<'_> {
    type Target = Backend;
    fn deref(&self) -> &Backend {
        match &*self.0 {
            Opening::Ready(backend) => backend,
            _ => unreachable!("handed out only once the backend is open"),
        }
    }
}

impl std::ops::DerefMut for BackendGuard<'_> {
    fn deref_mut(&mut self) -> &mut Backend {
        match &mut *self.0 {
            Opening::Ready(backend) => backend,
            _ => unreachable!("handed out only once the backend is open"),
        }
    }
}
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

fn lock<'a>(state: &'a State<'_, Shared>) -> Result<BackendGuard<'a>, String> {
    let poisoned = || "Interner Fehler: Zustand blockiert".to_owned();
    let mut opening = state.opening.lock().map_err(|_| poisoned())?;
    loop {
        if let Opening::Ready(_) = *opening {
            return Ok(BackendGuard(opening));
        }
        if let Opening::Failed(error) = &*opening {
            return Err(error.clone());
        }
        opening = state.ready.wait(opening).map_err(|_| poisoned())?;
    }
}

/// The backend if it is open already; never waits.
fn ready<'a>(state: &'a State<'_, Shared>) -> Option<BackendGuard<'a>> {
    let opening = state.opening.lock().ok()?;
    matches!(*opening, Opening::Ready(_)).then_some(BackendGuard(opening))
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

/// The formulas of a palace: its tables, the shared tables and the bindings of its loci.
#[tauri::command]
async fn workbook(
    state: State<'_, Shared>,
    palace_id: String,
) -> Result<locora_app_api::WorkbookView, String> {
    let b = lock(&state)?;
    workbook::view(&b.profile.store, &palace_id).map_err(err)
}

#[tauri::command]
async fn workbook_change(
    state: State<'_, Shared>,
    palace_id: String,
    change: locora_app_api::WorkbookChange,
) -> Result<locora_app_api::WorkbookView, String> {
    let b = lock(&state)?;
    workbook::change(&b.profile.store, &palace_id, change).map_err(err)
}

#[tauri::command]
async fn workbook_calc(
    state: State<'_, Shared>,
    palace_id: String,
    expression: String,
) -> Result<String, String> {
    let b = lock(&state)?;
    workbook::calc(&b.profile.store, &palace_id, &expression).map_err(err)
}

/// Rename a palace; the reply is the palace as it now reads.
#[tauri::command]
async fn palace_rename(
    state: State<'_, Shared>,
    palace_id: String,
    name: String,
) -> Result<PalaceView, String> {
    let mut b = lock(&state)?;
    b.profile
        .store
        .rename_palace(&palace_id, &name)
        .map_err(|e| e.to_string())?;
    let marker = b.default_marker.clone();
    views::palace(&mut b.profile, &palace_id, &marker).map_err(err)
}

/// Rename a room of a palace.
#[tauri::command]
async fn room_rename(
    state: State<'_, Shared>,
    palace_id: String,
    room_id: String,
    name: String,
) -> Result<PalaceView, String> {
    let mut b = lock(&state)?;
    b.profile
        .store
        .rename_room(&room_id, &name)
        .map_err(|e| e.to_string())?;
    let marker = b.default_marker.clone();
    views::palace(&mut b.profile, &palace_id, &marker).map_err(err)
}

/// Delete a room with its loci; the reply is the palace without it.
#[tauri::command]
async fn room_delete(
    state: State<'_, Shared>,
    palace_id: String,
    room_id: String,
) -> Result<PalaceView, String> {
    let mut b = lock(&state)?;
    if !b
        .profile
        .store
        .delete_room(&room_id)
        .map_err(|e| e.to_string())?
    {
        return Err("Der Raum ist nicht mehr vorhanden".into());
    }
    let marker = b.default_marker.clone();
    views::palace(&mut b.profile, &palace_id, &marker).map_err(err)
}

/// Delete a palace with its rooms and loci.
#[tauri::command]
async fn palace_delete(state: State<'_, Shared>, palace_id: String) -> Result<(), String> {
    let b = lock(&state)?;
    if !b
        .profile
        .store
        .delete_palace(&palace_id)
        .map_err(|e| e.to_string())?
    {
        return Err("Der Palast ist nicht mehr vorhanden".into());
    }
    Ok(())
}

/// Move a room one place left (`-1`) or right (`1`) in its palace.
#[tauri::command]
async fn room_move(
    state: State<'_, Shared>,
    palace_id: String,
    room_id: String,
    step: i32,
) -> Result<PalaceView, String> {
    let mut b = lock(&state)?;
    b.profile
        .store
        .move_room(&room_id, step)
        .map_err(|e| e.to_string())?;
    let marker = b.default_marker.clone();
    views::palace(&mut b.profile, &palace_id, &marker).map_err(err)
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

/// A sound clip for a locus, like `editor_pick_picture`: the WebView supplies only scope.
#[tauri::command]
async fn editor_pick_audio(
    app: tauri::AppHandle,
    state: State<'_, Shared>,
    target: locora_app_api::PictureTarget,
) -> Result<Option<locora_app_api::EditorUpdated>, String> {
    use tauri_plugin_dialog::DialogExt;
    {
        let b = lock(&state)?;
        editor::picture_target(&b.profile.store, &target).map_err(err)?;
    }
    let picker_app = app.clone();
    let selected = tauri::async_runtime::spawn_blocking(move || {
        picker_app
            .dialog()
            .file()
            .set_title("Ton am Ort wählen")
            .add_filter("Töne", &locora_app_core::locus_audio::EXTENSIONS)
            .blocking_pick_file()
    })
    .await
    .map_err(|e| e.to_string())?;
    let Some(selected) = selected else {
        return Ok(None);
    };
    #[cfg(mobile)]
    let staged = mobile_files::stage(&app, selected, mobile_files::Kind::Audio)?;
    #[cfg(mobile)]
    let path = staged.path().to_path_buf();
    #[cfg(desktop)]
    let path = selected
        .into_path()
        .map_err(|_| "Diese Auswahl ist keine lokale Datei".to_owned())?;
    let mut b = lock(&state)?;
    let cards = b.profile.schedule(Some(&target.palace_id)).map_err(err)?;
    let (tree, selected) =
        locora_app_core::locus_audio::import_audio(&b.profile.store, &target, &path)
            .map_err(err)?;
    let palace = views::palace_view(&b.profile.store, &tree, &cards, &b.default_marker);
    Ok(Some(locora_app_api::EditorUpdated {
        palace,
        selected,
        receipt: None,
    }))
}

/// A picture, animation or video chosen in the native dialog: the dialog's own path on the
/// desktop, a staged copy on mobile. The WebView never names a path.
struct PickedMedia {
    #[cfg(mobile)]
    staged: mobile_files::Staged,
    #[cfg(desktop)]
    path: std::path::PathBuf,
}

impl PickedMedia {
    fn path(&self) -> &std::path::Path {
        #[cfg(mobile)]
        return self.staged.path();
        #[cfg(desktop)]
        return &self.path;
    }

    /// The chosen file's own name where a staged copy hides it; `None` uses the file name.
    fn name(&self) -> Option<&str> {
        #[cfg(mobile)]
        return Some(self.staged.name().unwrap_or(""));
        #[cfg(desktop)]
        return None;
    }
}

async fn pick_media(
    app: &tauri::AppHandle,
    title: &'static str,
) -> Result<Option<PickedMedia>, String> {
    use tauri_plugin_dialog::DialogExt;
    let picker_app = app.clone();
    let selected = tauri::async_runtime::spawn_blocking(move || {
        picker_app
            .dialog()
            .file()
            .set_title(title)
            .add_filter(
                "Bilder, Animationen und Videos",
                &[
                    "png", "jpg", "jpeg", "webp", "gif", "avif", "sply", "mp4", "m4v", "mov",
                    "webm", "mkv", "3gp",
                ],
            )
            .blocking_pick_file()
    })
    .await
    .map_err(|e| format!("Dateiauswahl fehlgeschlagen: {e}"))?;
    let Some(selected) = selected else {
        return Ok(None);
    };
    #[cfg(mobile)]
    let staged = mobile_files::stage(app, selected, mobile_files::Kind::Media)?;
    // Only a video in a codec devices do not play is converted (by the device; it can take a
    // while). iOS needs no converter: iPhone videos (HEVC or H.264) are kept as they are.
    #[cfg(target_os = "android")]
    let staged = {
        let converter = app.clone();
        tauri::async_runtime::spawn_blocking(move || {
            mobile_files::converted_if_needed(&converter, staged)
        })
        .await
        .map_err(|e| format!("Umwandlung fehlgeschlagen: {e}"))??
    };
    #[cfg(mobile)]
    return Ok(Some(PickedMedia { staged }));
    #[cfg(desktop)]
    return Ok(Some(PickedMedia {
        path: selected
            .into_path()
            .map_err(|_| "Diese Auswahl ist keine lokale Datei".to_owned())?,
    }));
}

/// Only the native dialog can authorize a local path; the WebView sends no path.
#[tauri::command]
async fn home_pick_media(
    app: tauri::AppHandle,
    state: State<'_, Shared>,
) -> Result<Option<PalaceView>, String> {
    let picked = pick_media(&app, "Neuen Palast aus Bild oder Video anlegen").await?;
    let b = lock(&state)?;
    let tree = editor::import_palace_named(
        &b.profile.store,
        picked.as_ref().map(PickedMedia::path),
        picked.as_ref().and_then(PickedMedia::name),
    )
    .map_err(err)?;
    // New rooms contain no loci: no fallible plugin call after committing.
    Ok(tree.map(|tree| {
        views::palace_view(&b.profile.store, &tree, &HashMap::new(), &b.default_marker)
    }))
}

/// One more room for a palace, from a picture, animation or video chosen in the native dialog.
#[tauri::command]
async fn editor_pick_room(
    app: tauri::AppHandle,
    state: State<'_, Shared>,
    palace_id: String,
) -> Result<Option<PalaceView>, String> {
    let Some(picked) = pick_media(&app, "Raum aus Bild oder Video hinzufügen").await? else {
        return Ok(None);
    };
    let mut b = lock(&state)?;
    let tree =
        editor::import_room_named(&b.profile.store, &palace_id, picked.path(), picked.name())
            .map_err(err)?;
    let cards = b.profile.schedule(Some(&tree.palace.id)).map_err(err)?;
    Ok(Some(views::palace_view(
        &b.profile.store,
        &tree,
        &cards,
        &b.default_marker,
    )))
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
            .add_filter(
                "Locora-Paläste",
                if cfg!(mobile) {
                    &["locora"]
                } else {
                    &["locora", "toml"]
                },
            )
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
    if staged.path().extension().is_none_or(|ext| ext != "locora") {
        return Err("Auf dem Handy bitte eine .locora-Datei importieren; TOML mit separaten Medien benötigt einen gemeinsamen Ordner".into());
    }
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

/// Save a short text, such as the cloud recovery key, as a `.txt` file; `None` when cancelled.
#[tauri::command]
async fn save_text_pick(
    app: tauri::AppHandle,
    name: String,
    text: String,
) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    let picker_app = app.clone();
    let selected = tauri::async_runtime::spawn_blocking(move || {
        picker_app
            .dialog()
            .file()
            .set_file_name(name)
            .add_filter("Text", &["txt"])
            .blocking_save_file()
    })
    .await
    .map_err(|e| format!("Dateiauswahl fehlgeschlagen: {e}"))?;
    let Some(selected) = selected else {
        return Ok(None);
    };
    #[cfg(mobile)]
    {
        let mut staged = tempfile::Builder::new()
            .suffix(".txt")
            .tempfile()
            .map_err(|e| format!("Datei vorbereiten: {e}"))?;
        std::io::Write::write_all(&mut staged, text.as_bytes())
            .map_err(|e| format!("Datei schreiben: {e}"))?;
        mobile_files::save(&app, selected, staged.path())?;
    }
    #[cfg(desktop)]
    {
        let path = selected
            .into_path()
            .map_err(|_| "Dieses Ziel ist keine lokale Datei".to_owned())?;
        std::fs::write(&path, text).map_err(|e| format!("Datei schreiben: {e}"))?;
    }
    Ok(Some("Gespeichert.".to_owned()))
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
async fn tutorial(state: State<'_, Shared>, lang: String) -> Result<String, String> {
    let mut b = lock(&state)?;
    locora_runtime::tutorial::ensure_in(&mut b.profile, &lang).map_err(err)
}

/// Simo's example palace for the app tour, made once.
#[tauri::command]
async fn tour_demo(state: State<'_, Shared>, lang: String) -> Result<String, String> {
    let mut b = lock(&state)?;
    locora_runtime::tour_demo::ensure_in(&mut b.profile, &lang).map_err(err)
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
async fn formula_guide(state: State<'_, Shared>, lang: String) -> Result<String, String> {
    let mut b = lock(&state)?;
    locora_runtime::formula_guide::ensure_in(&mut b.profile, &lang).map_err(err)
}

/// Everything that may be shown now (quiet hours, muting and the daily
/// per-channel limit were already applied by the plugin).
/// Due notifications leave as system notifications (Android or desktop), not as banners in the app.
/// A message counts as shown once the system accepted it, so the queue drops it.
#[tauri::command]
async fn notifications(
    app: tauri::AppHandle,
    state: State<'_, Shared>,
) -> Result<Vec<NotificationView>, String> {
    use tauri_plugin_notification::{NotificationExt, PermissionState};
    // The backend lock covers only reading and writing the profile. The system calls below can
    // wait for the OS; holding the lock meanwhile would block every other command, the palace
    // delete included.
    let messages = {
        let mut b = lock(&state)?;
        b.profile.notifications().map_err(err)?
    };
    if messages.is_empty() {
        return Ok(Vec::new());
    }
    let system = app.notification();
    if system
        .permission_state()
        .map_or(true, |state| state != PermissionState::Granted)
    {
        let _ = system.request_permission();
    }
    let mut sent = Vec::new();
    for message in messages {
        let shown = system
            .builder()
            .title(message.title.clone())
            .body(message.body.clone())
            .show()
            .is_ok();
        if shown {
            sent.push(message);
        }
    }
    let mut b = lock(&state)?;
    for message in sent {
        b.profile
            .shown_notification(&locora_api::notify::Message {
                plugin: "notify".into(),
                channel: message.channel,
                priority: message.priority,
                title: message.title,
                body: message.body,
                action: message.action,
                shown_at: locora_runtime::now(),
            })
            .map_err(err)?;
    }
    Ok(Vec::new())
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

/// Built-in achievements and whether each is unlocked.
#[tauri::command]
async fn achievements(
    state: State<'_, Shared>,
) -> Result<Vec<locora_app_api::AchievementView>, String> {
    journey::achievements(&mut lock(&state)?.profile).map_err(err)
}

/// Unlock an achievement the UI earned (a lesson or the finished tour); whether it was new.
#[tauri::command]
async fn achievement_unlock(state: State<'_, Shared>, id: String) -> Result<bool, String> {
    journey::unlock(&lock(&state)?.profile, &id).map_err(err)
}

/// Days shown in the activity overview: one year.
const ACTIVITY_DAYS: usize = 365;

/// Answers per learning day for the last year (days without answers included).
#[tauri::command]
async fn activity(state: State<'_, Shared>) -> Result<Vec<locora_app_api::ActivityDay>, String> {
    let days = lock(&state)?.profile.activity(ACTIVITY_DAYS).map_err(err)?;
    Ok(days
        .into_iter()
        .map(|(day, answers)| locora_app_api::ActivityDay { day, answers })
        .collect())
}

#[tauri::command]
async fn gamification_status(
    state: State<'_, Shared>,
    full: Option<bool>,
) -> Result<Option<locora_app_api::GamificationStatus>, String> {
    lock(&state)?
        .profile
        .gamification_status(full.unwrap_or(false))
        .map_err(err)
}

/// Learning days per week from this week on; returns the goal.
#[tauri::command]
async fn game_week_goal(state: State<'_, Shared>, days: u8) -> Result<u8, String> {
    lock(&state)?.profile.set_week_goal(days).map_err(err)
}

/// A one-time hint was dismissed, on every device.
#[tauri::command]
async fn game_seen(state: State<'_, Shared>, hint: String) -> Result<(), String> {
    lock(&state)?.profile.game_seen(&hint).map_err(err)
}

/// Deletes the game progress here and, through the sync, on every device; then asks the server to
/// delete the rankings when an account is signed in (off the UI thread). The local deletion never
/// waits for or fails because of the server.
#[tauri::command]
async fn game_reset(app: tauri::AppHandle) -> Result<locora_app_api::GameResetReport, String> {
    let state = app.state::<Shared>();
    let data = {
        let mut backend = lock(&state)?;
        backend.profile.reset_game().map_err(err)?;
        backend.data.clone()
    };
    let outcome = tauri::async_runtime::spawn_blocking(move || {
        locora_runtime::cloud::delete_rankings(&data)
            .unwrap_or(locora_runtime::cloud::RankingsDeletion::Pending)
    })
    .await
    .unwrap_or(locora_runtime::cloud::RankingsDeletion::Pending);
    Ok(locora_app_api::GameResetReport {
        rankings: locora_app_core::cloud_view::rankings(outcome),
    })
}

// The memory contest (see `locora_app_core::competition`): one round at a time, its task and
// hand-in; the plugin keeps the results.

#[tauri::command]
async fn compete_today(state: State<'_, Shared>) -> Result<locora_app_api::CompeteToday, String> {
    competition::today(&mut lock(&state)?.profile).map_err(err)
}

#[tauri::command]
async fn compete_start(
    state: State<'_, Shared>,
    start: locora_app_api::CompeteStart,
) -> Result<locora_app_api::CompeteStarted, String> {
    let mut b = lock(&state)?;
    let b = &mut *b;
    competition::start(&mut b.compete, &mut b.profile, start, Instant::now()).map_err(err)
}

#[tauri::command]
async fn compete_recall(state: State<'_, Shared>) -> Result<locora_app_api::CompeteRecall, String> {
    competition::recall(&mut lock(&state)?.compete, Instant::now()).map_err(err)
}

#[tauri::command]
async fn compete_finish(
    state: State<'_, Shared>,
    finish: locora_app_api::CompeteHandIn,
) -> Result<locora_app_api::CompeteResult, String> {
    let mut b = lock(&state)?;
    let b = &mut *b;
    competition::finish(&mut b.compete, &mut b.profile, finish, Instant::now()).map_err(err)
}

#[tauri::command]
async fn compete_abandon(state: State<'_, Shared>) -> Result<(), String> {
    competition::abandon(&mut lock(&state)?.compete);
    Ok(())
}

#[tauri::command]
async fn compete_evening_start(
    state: State<'_, Shared>,
    evening: locora_app_api::CompeteEvening,
) -> Result<locora_app_api::CompeteEveningStarted, String> {
    let mut b = lock(&state)?;
    let b = &mut *b;
    competition::evening_start(&mut b.compete, &mut b.profile, evening, Instant::now()).map_err(err)
}

#[tauri::command]
async fn compete_evening_finish(
    state: State<'_, Shared>,
    finish: locora_app_api::CompeteHandIn,
) -> Result<locora_app_api::CompeteEveningResult, String> {
    let mut b = lock(&state)?;
    let b = &mut *b;
    competition::evening_finish(&mut b.compete, &mut b.profile, finish).map_err(err)
}

#[tauri::command]
async fn compete_records(
    state: State<'_, Shared>,
) -> Result<locora_app_api::CompeteRecords, String> {
    competition::records(&mut lock(&state)?.profile).map_err(err)
}

#[tauri::command]
async fn compete_history(
    state: State<'_, Shared>,
    query: Option<locora_app_api::CompeteHistoryQuery>,
) -> Result<Vec<locora_app_api::CompeteHistoryEntry>, String> {
    competition::history(&mut lock(&state)?.profile, query.unwrap_or_default()).map_err(err)
}

#[tauri::command]
async fn compete_code(choice: locora_app_api::CompeteChoice) -> Result<String, String> {
    Ok(competition::new_code(choice))
}

#[tauri::command]
async fn compete_routes(
    state: State<'_, Shared>,
    choice: locora_app_api::CompeteChoice,
) -> Result<Vec<locora_app_api::RouteChoice>, String> {
    competition::route_choices(&lock(&state)?.profile.store, choice).map_err(err)
}

#[tauri::command]
async fn compete_route_walk(
    state: State<'_, Shared>,
    palace_id: String,
) -> Result<Vec<String>, String> {
    competition::route_places(&lock(&state)?.profile.store, &palace_id).map_err(err)
}

#[tauri::command]
async fn settings(state: State<'_, Shared>) -> Result<SettingsView, String> {
    let mut b = lock(&state)?;
    settings::view(&mut b.profile).map_err(err)
}

/// The app reads the status at the start, after signing in and on the settings: a deletion of
/// the game scores that still waits for the server is asked again then, in the background.
#[tauri::command]
async fn cloud_status(state: State<'_, Shared>) -> Result<locora_app_api::CloudStatus, String> {
    let data = lock(&state)?.data.clone();
    let saved = locora_runtime::cloud::load(&data).map_err(err)?;
    if saved.is_some() {
        retry_rankings_later(data.clone());
    }
    Ok(locora_app_core::cloud_view::status(saved.as_ref()))
}

/// Asks the server again to delete the game scores when that waits (never twice at once).
fn retry_rankings_later(data: std::path::PathBuf) {
    static RUNNING: AtomicBool = AtomicBool::new(false);
    if RUNNING.swap(true, Ordering::AcqRel) {
        return;
    }
    tauri::async_runtime::spawn_blocking(move || {
        let _ = locora_runtime::cloud::retry_rankings(&data);
        RUNNING.store(false, Ordering::Release);
    });
}

/// Argon2 and network I/O run off the UI thread. Only one sync may mutate the
/// profile's cursors at once; other app actions keep access to the backend.
#[tauri::command]
async fn cloud_sync(
    app: tauri::AppHandle,
    passphrase: String,
    confirmed: Option<Vec<String>>,
) -> Result<locora_app_api::CloudSyncResult, String> {
    if !(12..=1024).contains(&passphrase.chars().count()) {
        return Err("Verschlüsselungs-Passwort braucht 12 bis 1024 Zeichen".into());
    }
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
        let passphrase = zeroize::Zeroizing::new(passphrase);
        use locora_runtime::cloud_sync::{self, SyncOptions, Unlock};
        // A palace deletion is sent only once the person confirmed it (the UI asks first).
        let options = SyncOptions {
            confirmed: Some(confirmed.unwrap_or_default()),
            automatic: false,
        };
        let unlock = Unlock::Passphrase(passphrase);
        let report = cloud_sync::sync_with(&data, &store, unlock, options).map_err(err)?;
        Ok(locora_app_core::cloud_view::sync_result(&report))
    })
    .await
    .map_err(|error| format!("Cloud-Sync konnte nicht abgeschlossen werden: {error}"))?
}

/// Deletes the synced data on the server. This device starts over with the new generation, so its
/// next sync sends everything it has.
#[tauri::command]
async fn cloud_erase(app: tauri::AppHandle) -> Result<(), String> {
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
        return Err("Ein Cloud-Sync läuft gerade".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let result = (|| {
            use locora_runtime::cloud;
            let saved = cloud::load(&data)?
                .ok_or_else(|| anyhow::anyhow!("Kein Cloud-Konto angemeldet"))?;
            let remote = cloud::erase(&saved)?;
            // The kept key goes with the cloud data; auto-sync asks for the password again.
            let _ = locora_runtime::cloud_key::forget(&data, &keystore::SystemKeys);
            let mut saved = cloud::load(&data)?.unwrap_or(saved);
            cloud::adopt_generation(&mut saved, &store, remote.generation)?;
            cloud::save(&data, &saved)
        })();
        busy.store(false, Ordering::Release);
        result.map_err(err)
    })
    .await
    .map_err(|e| e.to_string())?
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

/// Content-addressed media and standalone `.sply` graphics.
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
        "sply" if found.mime == SPLINELY => {
            let key = format!("{kind}/{hash}");
            if let Some(svg) = backend.svg.get(&key) {
                return respond(StatusCode::OK, "image/svg+xml", svg.clone().into_bytes());
            }
            let converted = std::fs::read(&file)
                .map_err(anyhow::Error::from)
                .and_then(|bytes| sply::to_svg(&bytes));
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

fn open(app: &tauri::AppHandle) -> anyhow::Result<Backend> {
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
        compete: competition::Contest::default(),
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
    // LOCORA_LANGUAGE (en, de, fr, ja, zh) sets the interface language before the page starts, as
    // if it had been chosen in the settings: for tests and fixed installations.
    let language = std::env::var("LOCORA_LANGUAGE")
        .ok()
        .filter(|code| matches!(code.as_str(), "en" | "de" | "fr" | "ja" | "zh"))
        .map(|code| {
            format!(
                "try {{ localStorage.setItem('locora-language-v1', '{code}'); }} catch (_) {{}}"
            )
        })
        .unwrap_or_default();
    tauri::Builder::default()
        .append_invoke_initialization_script(language)
        .manage(DropInbox::default())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_locora_video::init())
        .setup(|app| {
            // The window comes first; the profile opens behind it and commands wait for it.
            app.manage::<Shared>(Shared::new());
            let window = app
                .config()
                .app
                .windows
                .iter()
                .find(|window| window.label == "main")
                .ok_or("Fensterkonfiguration 'main' fehlt")?
                .clone();
            tauri::WebviewWindowBuilder::from_config(app, &window)?.build()?;
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                // A panic must still end the wait of the commands, so it becomes an error.
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| open(&handle)))
                    .map_err(|_| "Profil öffnen: Abbruch beim Start".to_owned())
                    .and_then(|opened| opened.map_err(|e| format!("Profil öffnen: {e:#}")));
                handle.state::<Shared>().finish(result);
            });
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
            let Some(mut backend) = ready(&state) else {
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
            palace_rename,
            workbook,
            workbook_change,
            workbook_calc,
            room_rename,
            room_move,
            room_delete,
            palace_delete,
            editor_pick_picture,
            editor_pick_audio,
            home_pick_media,
            editor_pick_room,
            home_import_drop,
            editor_import_drop,
            tutorial,
            tour,
            tour_demo,
            formula_guide,
            journey_inputs,
            settings,
            cloud_status,
            cloud_sync,
            cloud_setup::cloud_connect,
            cloud_setup::cloud_keys_status,
            cloud_setup::cloud_keys_setup,
            cloud_setup::cloud_logout,
            cloud_setup::cloud_refresh,
            cloud_setup::cloud_delete_account,
            cloud_erase,
            cloud_auto::cloud_pending,
            cloud_auto::cloud_auto_state,
            cloud_auto::cloud_auto_enable,
            cloud_auto::cloud_auto_disable,
            cloud_auto::cloud_auto_sync,
            cloud_auto::cloud_email_resend,
            cloud_social::cloud_friends,
            cloud_social::cloud_friend_ask,
            cloud_social::cloud_friend_remove,
            cloud_social::cloud_ranking,
            cloud_social::cloud_report,
            cloud_social::cloud_invite_create,
            cloud_social::cloud_invite_revoke,
            cloud_social::cloud_invite_preview,
            cloud_social::cloud_invite_accept,
            share::share_art,
            share::share_ways,
            share::share_send,
            ui_assets,
            gamification_status,
            game_week_goal,
            game_seen,
            game_reset,
            activity,
            achievements,
            achievement_unlock,
            import_pick,
            export_pick,
            dump_pick,
            save_text_pick,
            settings_set,
            settings_optimize,
            set_utc_offset,
            journey_start,
            journey_action,
            journey_reveal,
            journey_answer,
            journey_stop,
            compete_today,
            compete_start,
            compete_recall,
            compete_finish,
            compete_abandon,
            compete_evening_start,
            compete_evening_finish,
            compete_records,
            compete_history,
            compete_code,
            compete_routes,
            compete_route_walk
        ])
        .run(context)
        .expect("Locora konnte nicht starten");
}
