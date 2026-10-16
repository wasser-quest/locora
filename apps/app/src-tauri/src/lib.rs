//! Locora app backend: opens the same profile as the CLI, answers the UI's
//! commands and serves media and `.sply` animations over `locora://`.

mod journey;
mod sply;
mod views;

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Mutex;

use locora_app_api::{
    Feedback, Home, JourneyStart, PalaceView, Question, Reply, Revealed, Step, Summary,
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
    /// `.sply` hash → SVG, converted once per run.
    svg: HashMap<String, String>,
}

type Shared = Mutex<Backend>;

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
    views::home(&mut b.profile).map_err(err)
}

#[tauri::command]
async fn palace(state: State<'_, Shared>, id: String) -> Result<PalaceView, String> {
    let mut b = lock(&state)?;
    let marker = b.default_marker.clone();
    views::palace(&mut b.profile, &id, &marker).map_err(err)
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
async fn journey_reveal(state: State<'_, Shared>) -> Result<Revealed, String> {
    let b = lock(&state)?;
    let journey = b.journey.as_ref().ok_or("Keine Journey aktiv")?;
    Ok(journey.reveal(&b.profile))
}

#[tauri::command]
async fn journey_answer(state: State<'_, Shared>, reply: Reply) -> Result<Feedback, String> {
    let mut b = lock(&state)?;
    let b = &mut *b;
    let journey = b.journey.as_mut().ok_or("Keine Journey aktiv")?;
    let feedback = journey
        .answer(&mut b.profile, reply, &b.default_marker)
        .map_err(err)?;
    if matches!(feedback.next, Step::Done(_)) {
        b.journey = None;
    }
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
    Ok(Backend {
        profile,
        journey: None,
        default_marker,
        svg: HashMap::new(),
    })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
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
            tutorial,
            journey_start,
            journey_reveal,
            journey_answer,
            journey_stop
        ])
        .run(tauri::generate_context!())
        .expect("Locora konnte nicht starten");
}
