//! Video conversion on Android: a video becomes MP4 with H.264 and AAC, encoded by the device
//! (Media3 Transformer, see `android/`). Other platforms have no such plugin: the desktop uses
//! ffmpeg and the browser WebCodecs (see `locora_media`).

use std::path::Path;

use serde::Serialize;
use tauri::plugin::{Builder, TauriPlugin};
use tauri::{Manager, Runtime};

#[cfg(target_os = "android")]
struct Video<R: Runtime>(tauri::plugin::PluginHandle<R>);

pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("locora-video")
        .setup(|_app, _api| {
            #[cfg(target_os = "android")]
            _app.manage(Video(
                _api.register_android_plugin("cloud.locora.video", "VideoPlugin")?,
            ));
            Ok(())
        })
        .build()
}

#[derive(Serialize)]
struct Convert<'a> {
    input: &'a str,
    output: &'a str,
}

/// Converts the video at `input` into an MP4 at `output`. Blocks until the device is done, so it
/// runs off the UI thread. `Err` with a message for the person; on other platforms always `Err`.
pub fn convert<R: Runtime, M: Manager<R>>(
    app: &M,
    input: &Path,
    output: &Path,
) -> Result<(), String> {
    let (Some(input), Some(output)) = (input.to_str(), output.to_str()) else {
        return Err("Ungültiger Dateipfad".into());
    };
    #[cfg(target_os = "android")]
    {
        let video = app.state::<Video<R>>();
        video
            .0
            .run_mobile_plugin::<serde_json::Value>("convert", Convert { input, output })
            .map(|_| ())
            .map_err(|error| error.to_string())
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = (app, Convert { input, output });
        Err("Videos werden hier nicht umgewandelt".into())
    }
}
