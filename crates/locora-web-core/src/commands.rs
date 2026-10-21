//! The UI's commands, as the browser build answers them. Names and argument envelopes are the
//! ones the Tauri backend (apps/app/src-tauri) uses, so the same UI runs in both. Files travel
//! through the folder: the page writes an imported file into `/locora/inbox/` and exports arrive in
//! `/locora/outbox/`, so the existing path-based import and export code runs unchanged.

use std::path::Path;

use anyhow::{Result, bail};
use locora_app_api::{
    CloudStatus, EditorChange, EditorUpdated, JourneyStart, Media, MediaKind, NotificationView,
    PictureTarget, Reply, SettingsChange, Step, Summary, TourAction, UtcOffset,
};
use locora_app_core::journey::Journey;
use locora_app_core::{editor, settings, sply, transfer, views, workbook};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::Backend;

/// Days in the activity overview: one year.
const ACTIVITY_DAYS: usize = 365;

/// Where the page's imported files arrive and where exports are written (in the core's folder).
const INBOX: &str = "/locora/inbox";
const OUTBOX: &str = "/locora/outbox";

/// Shown where the browser build would need Locora Cloud (connect, sync, keys). Nothing is sent.
const CLOUD_UNAVAILABLE: &str =
    "Locora Cloud ist im Browser-Build noch nicht verfügbar. Alle Daten bleiben auf diesem Gerät.";

fn take<T: DeserializeOwned>(args: Value) -> Result<T> {
    Ok(serde_json::from_value(args)?)
}

fn out<T: serde::Serialize>(value: T) -> Result<Value> {
    Ok(serde_json::to_value(value)?)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct IdArgs {
    id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PalaceArgs {
    palace_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PalaceRenameArgs {
    palace_id: String,
    name: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RoomRenameArgs {
    palace_id: String,
    room_id: String,
    name: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RoomArgs {
    palace_id: String,
    room_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RoomMoveArgs {
    palace_id: String,
    room_id: String,
    step: i32,
}

#[derive(Deserialize)]
struct ChangeArgs {
    change: EditorChange,
}

#[derive(Deserialize)]
struct LanguageArgs {
    lang: String,
}

#[derive(Deserialize)]
struct TourArgs {
    action: TourAction,
}

#[derive(Deserialize)]
struct StartArgs {
    start: JourneyStart,
}

#[derive(Deserialize)]
struct ReplyArgs {
    reply: Reply,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ActionArgs {
    key: String,
    revealed: bool,
    after: Option<bool>,
}

#[derive(Deserialize)]
struct SettingsArgs {
    change: SettingsChange,
}

#[derive(Deserialize)]
struct OffsetArgs {
    offset: UtcOffset,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ImportArgs {
    /// File name in the inbox.
    name: String,
    /// `archive`: a `.locora` file or TOML source; `palace`: a picture or video as a new palace;
    /// `picture`: a picture for the locus in `target`.
    kind: String,
    target: Option<PictureTarget>,
}

#[derive(Deserialize)]
struct PluginsArgs {
    plugins: Vec<locora_host::Manifest>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct NotificationArgs {
    notification: NotificationView,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExportArgs {
    palace_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MediaArgs {
    hash: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SplyArgs {
    hash: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkbookArgs {
    palace_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkbookChangeArgs {
    palace_id: String,
    change: locora_app_api::WorkbookChange,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkbookCalcArgs {
    palace_id: String,
    expression: String,
}

/// The running journey; a finished one only takes actions on its last answer.
fn running(journey: Option<&mut Journey>) -> Result<&mut Journey> {
    match journey {
        Some(j) if !j.done() => Ok(j),
        Some(_) => bail!("Die Journey ist beendet"),
        None => bail!("Keine Journey aktiv"),
    }
}

/// Every command name the browser build answers (the cloud ones are answered with a message).
pub const NAMES: &[&str] = &[
    "home",
    "palace",
    "palace_rename",
    "room_rename",
    "room_delete",
    "room_move",
    "palace_delete",
    "editor_change",
    "workbook",
    "workbook_change",
    "workbook_calc",
    "tutorial",
    "formula_guide",
    "tour",
    "ui_assets",
    "achievements",
    "activity",
    "gamification_status",
    "settings",
    "settings_set",
    "settings_optimize",
    "set_utc_offset",
    "journey_start",
    "journey_inputs",
    "journey_action",
    "journey_reveal",
    "journey_answer",
    "journey_stop",
    "notifications",
    "notification_shown",
    "import_file",
    "export_file",
    "media_file",
    "sply_svg",
    "dump_file",
    "set_plugins",
];

/// Whether a command name is known, before any backend is needed to answer it.
pub fn is_known(command: &str) -> bool {
    NAMES.contains(&command) || command.starts_with("cloud_")
}

/// The browser host learns which plugins the page runs; native hosts load their own.
#[cfg(target_arch = "wasm32")]
fn tell_host_plugins(profile: &mut locora_runtime::Profile, plugins: Vec<locora_host::Manifest>) {
    profile.host.set_plugins(plugins);
}

#[cfg(not(target_arch = "wasm32"))]
fn tell_host_plugins(_profile: &mut locora_runtime::Profile, _plugins: Vec<locora_host::Manifest>) {
}

/// Writes every file below `folder` into a ZIP archive, with names relative to `folder`.
fn zip_folder(folder: &Path, archive: &Path) -> Result<()> {
    use std::io::Write;
    let mut zip = zip::ZipWriter::new(std::fs::File::create(archive)?);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    let mut pending = vec![folder.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(&dir)? {
            let path = entry?.path();
            if path.is_dir() {
                pending.push(path);
                continue;
            }
            let name = path
                .strip_prefix(folder)?
                .to_string_lossy()
                .replace('\\', "/");
            zip.start_file(name, options)?;
            zip.write_all(&std::fs::read(&path)?)?;
        }
    }
    zip.finish()?;
    Ok(())
}

/// Runs one command against the open backend.
pub fn run(b: &mut Backend, command: &str, args: Value) -> Result<Value> {
    let Backend {
        profile,
        journey,
        default_marker,
        simo,
        assets,
    } = b;
    match command {
        "home" => {
            let mut home = views::home(profile)?;
            for tile in &mut home.tiles {
                tile.icon = Some(assets.tile_icon(&tile.plugin));
            }
            if home.palaces.is_empty() {
                home.welcome = Some(Media {
                    hash: simo.clone(),
                    kind: MediaKind::Sply,
                });
            }
            out(home)
        }
        "palace" => {
            let a: IdArgs = take(args)?;
            out(views::palace(profile, &a.id, default_marker)?)
        }
        "workbook" => {
            let a: WorkbookArgs = take(args)?;
            out(workbook::view(&profile.store, &a.palace_id)?)
        }
        "workbook_change" => {
            let a: WorkbookChangeArgs = take(args)?;
            out(workbook::change(&profile.store, &a.palace_id, a.change)?)
        }
        "workbook_calc" => {
            let a: WorkbookCalcArgs = take(args)?;
            out(workbook::calc(&profile.store, &a.palace_id, &a.expression)?)
        }
        "palace_rename" => {
            let a: PalaceRenameArgs = take(args)?;
            profile.store.rename_palace(&a.palace_id, &a.name)?;
            out(views::palace(profile, &a.palace_id, default_marker)?)
        }
        "room_rename" => {
            let a: RoomRenameArgs = take(args)?;
            profile.store.rename_room(&a.room_id, &a.name)?;
            out(views::palace(profile, &a.palace_id, default_marker)?)
        }
        "room_delete" => {
            let a: RoomArgs = take(args)?;
            if !profile.store.delete_room(&a.room_id)? {
                bail!("Der Raum ist nicht mehr vorhanden");
            }
            out(views::palace(profile, &a.palace_id, default_marker)?)
        }
        "room_move" => {
            let a: RoomMoveArgs = take(args)?;
            profile.store.move_room(&a.room_id, a.step)?;
            out(views::palace(profile, &a.palace_id, default_marker)?)
        }
        "palace_delete" => {
            let a: PalaceArgs = take(args)?;
            if !profile.store.delete_palace(&a.palace_id)? {
                bail!("Der Palast ist nicht mehr vorhanden");
            }
            out(())
        }
        "editor_change" => {
            let a: ChangeArgs = take(args)?;
            // Fallible plugin reads come before the write, so a successful write never looks failed.
            let cards = profile.schedule(Some(&a.change.palace_id))?;
            let (tree, selected, receipt) = editor::apply_with_history(&profile.store, &a.change)?;
            let palace = views::palace_view(&profile.store, &tree, &cards, default_marker);
            out(EditorUpdated {
                palace,
                selected,
                receipt,
            })
        }
        "tutorial" => {
            let a: LanguageArgs = take(args)?;
            out(locora_runtime::tutorial::ensure_in(profile, &a.lang)?)
        }
        "formula_guide" => {
            let a: LanguageArgs = take(args)?;
            out(locora_runtime::formula_guide::ensure_in(profile, &a.lang)?)
        }
        "tour" => {
            let a: TourArgs = take(args)?;
            out(profile.tour(a.action)?)
        }
        "ui_assets" => out(assets.ui()),
        "achievements" => out(locora_app_core::journey::achievements(profile)?),
        "activity" => {
            let days = profile.activity(ACTIVITY_DAYS)?;
            out(days
                .into_iter()
                .map(|(day, answers)| locora_app_api::ActivityDay { day, answers })
                .collect::<Vec<_>>())
        }
        "gamification_status" => out(profile.gamification_status()?),
        "settings" => out(settings::view(profile)?),
        "settings_set" => {
            let a: SettingsArgs = take(args)?;
            out(settings::change(profile, a.change)?)
        }
        "settings_optimize" => out(settings::optimize(profile)?),
        "set_utc_offset" => {
            let a: OffsetArgs = take(args)?;
            profile.set_utc_offset(a.offset.minutes)?;
            out(())
        }
        "journey_start" => {
            let a: StartArgs = take(args)?;
            if let Some(mut old) = journey.take() {
                old.finish(profile)?;
            }
            match Journey::start(profile, a.start)? {
                None => out(Step::Done(Summary::default())),
                Some(started) => {
                    let question = started.question(profile, default_marker);
                    *journey = Some(started);
                    out(Step::Question(question))
                }
            }
        }
        "journey_inputs" => {
            let a: StartArgs = take(args)?;
            out(locora_app_core::journey::inputs(profile, &a.start)?)
        }
        "journey_action" => {
            let a: ActionArgs = take(args)?;
            let done = if a.after.unwrap_or(false) {
                let running = journey
                    .as_mut()
                    .ok_or_else(|| anyhow::anyhow!("Keine Journey aktiv"))?;
                running.action_after(profile, &a.key, default_marker)?
            } else {
                running(journey.as_mut())?.action(profile, &a.key, a.revealed, default_marker)?
            };
            out(done)
        }
        "journey_reveal" => out(running(journey.as_mut())?.reveal(profile)),
        "journey_answer" => {
            let a: ReplyArgs = take(args)?;
            out(running(journey.as_mut())?.answer(profile, a.reply, default_marker)?)
        }
        "journey_stop" => match journey.take() {
            Some(mut finished) => out(finished.finish(profile)?),
            None => out(Summary::default()),
        },
        // The due messages leave for the system's notifications (the page shows them); a message
        // stays queued until `notification_shown` confirms it.
        "notifications" => {
            let messages = profile.notifications()?;
            out(messages
                .into_iter()
                .map(|m| NotificationView {
                    channel: m.channel,
                    priority: m.priority,
                    title: m.title,
                    body: m.body,
                    action: m.action,
                })
                .collect::<Vec<_>>())
        }
        "notification_shown" => {
            let a: NotificationArgs = take(args)?;
            let n = a.notification;
            profile.shown_notification(&locora_api::notify::Message {
                plugin: "notify".into(),
                channel: n.channel,
                priority: n.priority,
                title: n.title,
                body: n.body,
                action: n.action,
                shown_at: locora_runtime::now(),
            })?;
            out(())
        }
        // Imports and exports: the page stages the file in the inbox or fetches it from the outbox.
        "import_file" => {
            let a: ImportArgs = take(args)?;
            let path = format!("{INBOX}/{}", a.name);
            match a.kind.as_str() {
                "archive" => out(transfer::import_path(profile, Path::new(&path))?),
                "palace" => {
                    let tree = editor::import_palace(&profile.store, Some(Path::new(&path)))?;
                    out(tree.map(|tree| {
                        views::palace_view(
                            &profile.store,
                            &tree,
                            &Default::default(),
                            default_marker,
                        )
                    }))
                }
                "picture" => {
                    let target = a
                        .target
                        .ok_or_else(|| anyhow::anyhow!("Kein Ort für das Bild"))?;
                    editor::picture_target(&profile.store, &target)?;
                    let cards = profile.schedule(Some(&target.palace_id))?;
                    let (tree, selected) =
                        editor::import_picture(&profile.store, &target, Path::new(&path))?;
                    out(EditorUpdated {
                        palace: views::palace_view(&profile.store, &tree, &cards, default_marker),
                        selected,
                        receipt: None,
                    })
                }
                other => bail!("Unbekannte Dateiart: {other}"),
            }
        }
        "export_file" => {
            let a: ExportArgs = take(args)?;
            std::fs::create_dir_all(OUTBOX)?;
            let name = transfer::suggested_name(profile, &a.palace_id)?;
            let path = transfer::with_extension(Path::new(OUTBOX).join(name));
            let message = transfer::export_path(profile, &a.palace_id, &path)?;
            out(serde_json::json!({ "message": message, "file": path }))
        }
        // The stored bytes behind a media hash, for the page to show or play.
        "media_file" => {
            let a: MediaArgs = take(args)?;
            let Some(found) = profile.store.media(&a.hash)? else {
                bail!("Medium nicht gefunden");
            };
            let Some(file) = profile.store.media_file(&a.hash) else {
                bail!("Medium nicht gefunden");
            };
            out(serde_json::json!({ "path": file, "mime": found.mime }))
        }
        // An animated `.sply` as the SVG document the page embeds.
        "sply_svg" => {
            let a: SplyArgs = take(args)?;
            let Some(file) = profile.store.media_file(&a.hash) else {
                bail!("Medium nicht gefunden");
            };
            let bytes = std::fs::read(file)?;
            let svg = sply::to_svg(&bytes)?;
            out(svg)
        }
        // The folder export: the palace's source and media, as one ZIP file for the page to save.
        // The plugins the page started: their tiles and declared blocks are shown.
        "set_plugins" => {
            let a: PluginsArgs = take(args)?;
            tell_host_plugins(profile, a.plugins);
            out(())
        }
        "dump_file" => {
            let a: ExportArgs = take(args)?;
            std::fs::create_dir_all(OUTBOX)?;
            let name = transfer::suggested_name(profile, &a.palace_id)?;
            let folder = Path::new(OUTBOX).join(&name);
            let message = transfer::dump_path(profile, &a.palace_id, &folder)?;
            let archive = Path::new(OUTBOX).join(format!("{name}.zip"));
            zip_folder(&folder, &archive)?;
            std::fs::remove_dir_all(&folder)?;
            out(serde_json::json!({ "message": message, "file": archive }))
        }
        // Without Locora Cloud the account is signed out: the settings page shows that, and the
        // actions that need the server explain why they cannot run here.
        "cloud_status" | "cloud_refresh" => out(CloudStatus {
            account: None,
            server: None,
            access: false,
        }),
        "cloud_keys_status" => out(false),
        "cloud_logout" => out(()),
        cloud if cloud.starts_with("cloud_") => bail!(CLOUD_UNAVAILABLE),
        other => bail!("unknown command {other}"),
    }
}
