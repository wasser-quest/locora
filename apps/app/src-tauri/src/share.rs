//! Sharing a card: the app's own medal art as a still PNG, and the finished picture saved where
//! the person chooses (the UI has copied its text by then). The apps have no share sheet yet; the
//! browser build uses the system's through the Web Share API.

use locora_app_api::{ShareDone, ShareWay, ShareWays};
use locora_app_core::share;
use tauri::State;

use crate::{Shared, err, lock};

/// One of the app's graphics as a PNG for a share card (base64). Checked under the backend's
/// lock, drawn without it.
#[tauri::command]
pub async fn share_art(state: State<'_, Shared>, hash: String) -> Result<String, String> {
    let store = {
        let backend = lock(&state)?;
        if !backend.assets.knows(&hash) {
            return Err("Grafik nicht gefunden".into());
        }
        backend.profile.store.clone()
    };
    tauri::async_runtime::spawn_blocking(move || share::draw(&store, &hash).map_err(err))
        .await
        .map_err(|e| e.to_string())?
}

/// How sharing works here: the picture is saved and its text copied.
#[tauri::command]
pub async fn share_ways() -> Result<ShareWays, String> {
    Ok(ShareWays {
        card: ShareWay::Save,
        text_sheet: false,
    })
}

/// Saves a picture (`png`, base64) under a name the person may change. A text alone has no
/// sheet here: the UI copies it (`Copy`).
#[tauri::command]
pub async fn share_send(
    app: tauri::AppHandle,
    name: String,
    png: Option<String>,
) -> Result<ShareDone, String> {
    use tauri_plugin_dialog::DialogExt;
    let Some(png) = png else {
        return Ok(ShareDone::Copy);
    };
    let bytes = share::png_bytes(&png).map_err(err)?;
    let file_name = share::file_name(&name);
    let picker_app = app.clone();
    let selected = tauri::async_runtime::spawn_blocking(move || {
        picker_app
            .dialog()
            .file()
            .set_file_name(file_name)
            .add_filter("PNG", &["png"])
            .blocking_save_file()
    })
    .await
    .map_err(|e| format!("Dateiauswahl fehlgeschlagen: {e}"))?;
    let Some(selected) = selected else {
        return Ok(ShareDone::Cancelled);
    };
    #[cfg(mobile)]
    {
        let mut staged = tempfile::Builder::new()
            .suffix(".png")
            .tempfile()
            .map_err(|e| format!("Datei vorbereiten: {e}"))?;
        std::io::Write::write_all(&mut staged, &bytes)
            .map_err(|e| format!("Datei schreiben: {e}"))?;
        crate::mobile_files::save(&app, selected, staged.path())?;
    }
    #[cfg(desktop)]
    {
        let path = selected
            .into_path()
            .map_err(|_| "Dieses Ziel ist keine lokale Datei".to_owned())?;
        std::fs::write(&path, bytes).map_err(|e| format!("Datei schreiben: {e}"))?;
    }
    Ok(ShareDone::Saved)
}
