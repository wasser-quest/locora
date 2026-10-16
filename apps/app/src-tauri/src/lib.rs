//! Locora app backend: opens the same profile as the CLI, answers the UI's
//! commands and serves media and `.sply` animations over `locora://`.

mod editor;
mod journey;
mod settings;
mod sply;
mod transfer;
mod views;

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Mutex;

use locora_app_api::{
    ActionDone, Feedback, Home, ImportResult, InputField, JourneyStart, PalaceView, Question,
    Reply, Revealed, SettingsChange, SettingsView, Step, Summary,
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
    /// `.sply` hash → SVG, converted once per run.
    svg: HashMap<String, String>,
}

type Shared = Mutex<Backend>;

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
    let (tree, selected) = editor::apply(&b.profile.store, &change).map_err(err)?;
    let palace = views::palace_view(&b.profile.store, &tree, &cards, &b.default_marker);
    Ok(locora_app_api::EditorUpdated { palace, selected })
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
    let selected = tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
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
    let path = selected
        .into_path()
        .map_err(|_| "Diese Auswahl ist keine lokale Datei".to_owned())?;
    let mut b = lock(&state)?;
    let cards = b.profile.schedule(Some(&target.palace_id)).map_err(err)?;
    let (tree, selected) = editor::import_picture(&b.profile.store, &target, &path).map_err(err)?;
    let palace = views::palace_view(&b.profile.store, &tree, &cards, &b.default_marker);
    Ok(Some(locora_app_api::EditorUpdated { palace, selected }))
}

/// Only the native dialog can authorize a local path; the WebView sends no path.
#[tauri::command]
async fn home_pick_media(
    app: tauri::AppHandle,
    state: State<'_, Shared>,
) -> Result<Option<PalaceView>, String> {
    use tauri_plugin_dialog::DialogExt;
    let selected = tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
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
    let path = selected
        .map(|s| {
            s.into_path()
                .map_err(|_| "Diese Auswahl ist keine lokale Datei".to_owned())
        })
        .transpose()?;
    let b = lock(&state)?;
    let tree = editor::import_palace(&b.profile.store, path.as_deref()).map_err(err)?;
    // New rooms contain no loci: no fallible plugin call after committing.
    Ok(tree.map(|tree| {
        views::palace_view(&b.profile.store, &tree, &HashMap::new(), &b.default_marker)
    }))
}

/// Pick a `.locora` file and import it; `None` when the dialog was cancelled.
#[tauri::command]
async fn import_pick(
    app: tauri::AppHandle,
    state: State<'_, Shared>,
) -> Result<Option<ImportResult>, String> {
    use tauri_plugin_dialog::DialogExt;
    let selected = tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .set_title("Paläste importieren")
            .add_filter("Locora-Paläste", &["locora"])
            .blocking_pick_file()
    })
    .await
    .map_err(|e| format!("Dateiauswahl fehlgeschlagen: {e}"))?;
    let Some(selected) = selected else {
        return Ok(None);
    };
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
    let selected = tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
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
    let path = selected
        .into_path()
        .map_err(|_| "Dieses Ziel ist keine lokale Datei".to_owned())?;
    let mut b = lock(&state)?;
    transfer::export_path(&mut b.profile, &id, &transfer::with_extension(path))
        .map(Some)
        .map_err(err)
}

/// Build (or rebuild) the tutorial palace; returns its id.
#[tauri::command]
async fn tutorial(state: State<'_, Shared>) -> Result<String, String> {
    let mut b = lock(&state)?;
    let (tree, _) = locora_runtime::tutorial::install(&mut b.profile).map_err(err)?;
    Ok(tree.palace.id)
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
async fn settings(state: State<'_, Shared>) -> Result<SettingsView, String> {
    let mut b = lock(&state)?;
    settings::view(&mut b.profile).map_err(err)
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

/// `locora://localhost/media/<sha256>` and `…/sply/<sha256>`.
fn serve(backend: &mut Backend, path: &str) -> Response<Cow<'static, [u8]>> {
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
        "media" => match std::fs::read(&file) {
            Ok(bytes) => respond(StatusCode::OK, &found.mime, bytes),
            Err(_) => not_found(),
        },
        "sply" if found.mime == SPLINELY => {
            if let Some(svg) = backend.svg.get(hash) {
                return respond(StatusCode::OK, "image/svg+xml", svg.clone().into_bytes());
            }
            let converted = std::fs::read(&file)
                .map_err(anyhow::Error::from)
                .and_then(|bytes| sply::to_svg(&bytes));
            match converted {
                Ok(svg) => {
                    backend.svg.insert(hash.to_owned(), svg.clone());
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
    let profile = Profile::open(data.as_deref())?;
    let default_marker = profile
        .store
        .put_media(locora_runtime::tutorial::DEFAULT_MARKER, SPLINELY)?
        .hash;
    let simo = profile
        .store
        .put_media(locora_runtime::tutorial::SIMO, SPLINELY)?
        .hash;
    Ok(Backend {
        profile,
        journey: None,
        default_marker,
        simo,
        svg: HashMap::new(),
    })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let backend = open(app).map_err(|e| format!("Profil öffnen: {e:#}"))?;
            app.manage::<Shared>(Mutex::new(backend));
            Ok(())
        })
        .register_uri_scheme_protocol(locora_app_api::SCHEME, |ctx, request| {
            let state = ctx.app_handle().state::<Shared>();
            let Ok(mut backend) = state.lock() else {
                return not_found();
            };
            serve(&mut backend, request.uri().path())
        })
        .invoke_handler(tauri::generate_handler![
            home,
            palace,
            editor_change,
            editor_pick_picture,
            home_pick_media,
            tutorial,
            journey_inputs,
            settings,
            import_pick,
            export_pick,
            settings_set,
            journey_start,
            journey_action,
            journey_reveal,
            journey_answer,
            journey_stop
        ])
        .run(tauri::generate_context!())
        .expect("Locora konnte nicht starten");
}
