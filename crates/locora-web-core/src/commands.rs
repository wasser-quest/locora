//! The UI's commands, as the browser build answers them. Names and argument envelopes are the
//! ones the Tauri backend (apps/app/src-tauri) uses, so the same UI runs in both. Files travel
//! through the folder: the page writes an imported file into `/locora/inbox/` and exports arrive in
//! `/locora/outbox/`, so the existing path-based import and export code runs unchanged.

use std::path::Path;
use std::time::Instant;

use anyhow::{Result, bail};
use locora_app_api::{
    CloudStatus, EditorChange, EditorUpdated, JourneyStart, Media, MediaKind, NotificationView,
    PictureTarget, Reply, SettingsChange, Step, Summary, TourAction, UtcOffset,
};
use locora_app_core::journey::Journey;
use locora_app_core::{competition, editor, settings, sply, transfer, views, workbook};
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
    /// `picture`: a picture for the locus in `target`; `audio`: a sound clip for it; `room`: a
    /// picture or video as one more room of the palace `palace_id`.
    kind: String,
    target: Option<PictureTarget>,
    #[serde(default)]
    palace_id: Option<String>,
}

#[derive(Deserialize)]
struct PluginsArgs {
    plugins: Vec<locora_host::Manifest>,
}

#[derive(Deserialize)]
struct OwnProgressArgs {
    progress: Vec<u8>,
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

#[derive(Deserialize)]
struct CompeteStartArgs {
    start: locora_app_api::CompeteStart,
}

#[derive(Deserialize)]
struct HandInArgs {
    finish: locora_app_api::CompeteHandIn,
}

#[derive(Deserialize)]
struct EveningArgs {
    evening: locora_app_api::CompeteEvening,
}

#[derive(Deserialize, Default)]
struct HistoryArgs {
    #[serde(default)]
    query: locora_app_api::CompeteHistoryQuery,
}

#[derive(Deserialize)]
struct ChoiceArgs {
    choice: locora_app_api::CompeteChoice,
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
    "tour_demo",
    "formula_guide",
    "tour",
    "ui_assets",
    "achievements",
    "achievement_unlock",
    "activity",
    "gamification_status",
    "game_week_goal",
    "game_seen",
    "game_reset",
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
    "share_art",
    "dump_file",
    "set_plugins",
    "game_keep_own",
    "compete_today",
    "compete_start",
    "compete_recall",
    "compete_finish",
    "compete_abandon",
    "compete_evening_start",
    "compete_evening_finish",
    "compete_records",
    "compete_history",
    "compete_code",
    "compete_routes",
    "compete_route_walk",
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

/// A picture (`picture`) or sound clip (`audio`) from the inbox for the locus in `target`.
fn import_locus_file(
    profile: &mut locora_runtime::Profile,
    kind: &str,
    target: &PictureTarget,
    path: &Path,
    default_marker: &str,
) -> Result<EditorUpdated> {
    editor::picture_target(&profile.store, target)?;
    let cards = profile.schedule(Some(&target.palace_id))?;
    let (tree, selected) = if kind == "audio" {
        locora_app_core::locus_audio::import_audio(&profile.store, target, path)?
    } else {
        editor::import_picture(&profile.store, target, path)?
    };
    Ok(EditorUpdated {
        palace: views::palace_view(&profile.store, &tree, &cards, default_marker),
        selected,
        receipt: None,
    })
}

/// Runs one command against the open backend.
pub fn run(b: &mut Backend, command: &str, args: Value) -> Result<Value> {
    let Backend {
        profile,
        journey,
        compete,
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
        "tour_demo" => {
            let a: LanguageArgs = take(args)?;
            out(locora_runtime::tour_demo::ensure_in(profile, &a.lang)?)
        }
        "tour" => {
            let a: TourArgs = take(args)?;
            out(profile.tour(a.action)?)
        }
        "ui_assets" => out(assets.ui()),
        "achievements" => out(locora_app_core::journey::achievements(profile)?),
        "achievement_unlock" => {
            let a: locora_app_api::AchievementArgs = take(args)?;
            out(locora_app_core::journey::unlock(profile, &a.id)?)
        }
        "activity" => {
            let days = profile.activity(ACTIVITY_DAYS)?;
            out(days
                .into_iter()
                .map(|(day, answers)| locora_app_api::ActivityDay { day, answers })
                .collect::<Vec<_>>())
        }
        "gamification_status" => {
            let a: locora_app_api::GameStatusArgs = take(args).unwrap_or_default();
            out(profile.gamification_status(a.full)?)
        }
        "game_week_goal" => {
            let a: locora_app_api::WeekGoalArgs = take(args)?;
            out(profile.set_week_goal(a.days)?)
        }
        "game_seen" => {
            let a: locora_app_api::GameSeenArgs = take(args)?;
            profile.game_seen(&a.hint)?;
            out(())
        }
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
                kind @ ("picture" | "audio") => {
                    let target = a
                        .target
                        .ok_or_else(|| anyhow::anyhow!("Kein Ort für die Datei"))?;
                    out(import_locus_file(
                        profile,
                        kind,
                        &target,
                        Path::new(&path),
                        default_marker,
                    )?)
                }
                "room" => {
                    let palace_id = a
                        .palace_id
                        .ok_or_else(|| anyhow::anyhow!("Kein Palast für den Raum"))?;
                    let tree = editor::import_room_named(
                        &profile.store,
                        &palace_id,
                        Path::new(&path),
                        None,
                    )?;
                    let cards = profile.schedule(Some(&palace_id))?;
                    out(views::palace_view(
                        &profile.store,
                        &tree,
                        &cards,
                        default_marker,
                    ))
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
        // One of the app's graphics as a still PNG for a share card (never a palace's media).
        "share_art" => {
            let a: SplyArgs = take(args)?;
            out(locora_app_core::share::art_png(
                assets,
                &profile.store,
                &a.hash,
            )?)
        }
        // The folder export: the palace's source and media, as one ZIP file for the page to save.
        // The plugins the page started: their tiles and declared blocks are shown.
        // The game progress this browser kept for itself before the sync (the plugin's old
        // `progress` from IndexedDB): taken over by the migration on top of the synced progress,
        // never written over it.
        "game_keep_own" => {
            let a: OwnProgressArgs = take(args)?;
            out(profile.keep_own_game_progress(&a.progress)?)
        }
        "set_plugins" => {
            let a: PluginsArgs = take(args)?;
            tell_host_plugins(profile, a.plugins);
            // The plugins run now: the progress from before the sync can be taken over (once).
            if let Err(error) = profile.ensure_game_migrated() {
                profile
                    .notes
                    .push(format!("Spielfortschritt nicht übernommen: {error:#}"));
            }
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
        // The memory contest: one round at a time, held in the core; the plugin keeps the results.
        "compete_today" => out(competition::today(profile)?),
        "compete_start" => {
            let a: CompeteStartArgs = take(args)?;
            out(competition::start(
                compete,
                profile,
                a.start,
                Instant::now(),
            )?)
        }
        "compete_recall" => out(competition::recall(compete, Instant::now())?),
        "compete_finish" => {
            let a: HandInArgs = take(args)?;
            out(competition::finish(
                compete,
                profile,
                a.finish,
                Instant::now(),
            )?)
        }
        "compete_abandon" => {
            competition::abandon(compete);
            out(())
        }
        "compete_evening_start" => {
            let a: EveningArgs = take(args)?;
            out(competition::evening_start(
                compete,
                profile,
                a.evening,
                Instant::now(),
            )?)
        }
        "compete_evening_finish" => {
            let a: HandInArgs = take(args)?;
            out(competition::evening_finish(compete, profile, a.finish)?)
        }
        "compete_records" => out(competition::records(profile)?),
        "compete_history" => {
            let a: HistoryArgs = take(args).unwrap_or_default();
            out(competition::history(profile, a.query)?)
        }
        "compete_code" => {
            let a: ChoiceArgs = take(args)?;
            out(competition::new_code(a.choice))
        }
        "compete_routes" => {
            let a: ChoiceArgs = take(args)?;
            out(competition::route_choices(&profile.store, a.choice)?)
        }
        "compete_route_walk" => {
            let a: PalaceArgs = take(args)?;
            out(competition::route_places(&profile.store, &a.palace_id)?)
        }
        // Without Locora Cloud the account is signed out: the settings page shows that, and the
        // actions that need the server explain why they cannot run here.
        "cloud_status" | "cloud_refresh" => out(CloudStatus::signed_out()),
        "cloud_keys_status" => out(false),
        "cloud_logout" => out(()),
        cloud if cloud.starts_with("cloud_") => bail!(CLOUD_UNAVAILABLE),
        other => bail!("unknown command {other}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sound_clip_from_the_inbox_is_set_on_the_locus() {
        let dir = tempfile::tempdir().unwrap();
        let mut profile = locora_runtime::Profile::open(Some(&dir.path().join("profil"))).unwrap();
        let store = &profile.store;
        let p = store.create_palace("Klänge").unwrap();
        let r = store
            .add_room(&p.id, "Raum", locora_store::RoomKind::Blank, None)
            .unwrap();
        let l = store
            .add_locus(
                &r.id,
                locora_store::NewLocus {
                    audio: None,
                    image_size: locora_store::default_image_size(),
                    zoom: locora_store::default_zoom(),
                    place: "Tür".into(),
                    item: "Merkur".into(),
                    x: 0.5,
                    y: 0.5,
                    time: None,
                    answer: Default::default(),
                    image: None,
                    note: String::new(),
                },
            )
            .unwrap();
        let target = PictureTarget {
            palace_id: p.id,
            room_id: r.id,
            locus_id: l.id.clone(),
        };
        let file = dir.path().join("klang.wav");
        std::fs::write(&file, locora_media::fixtures::wav()).unwrap();
        let updated = import_locus_file(&mut profile, "audio", &target, &file, "marker").unwrap();
        assert_eq!(updated.selected.as_deref(), Some(l.id.as_str()));
        let locus = &updated.palace.rooms[0].loci[0];
        let hash = locus.audio.clone().unwrap();
        assert_eq!(
            profile.store.media(&hash).unwrap().unwrap().mime,
            "audio/wav"
        );
        // A picture file is not taken as a sound clip.
        let png = dir.path().join("bild.png");
        std::fs::write(&png, locora_media::fixtures::png()).unwrap();
        assert!(import_locus_file(&mut profile, "audio", &target, &png, "marker").is_err());
    }

    #[test]
    fn a_contest_round_runs_through_the_commands_of_the_page() {
        let dir = tempfile::tempdir().unwrap();
        let profile = locora_runtime::Profile::open(Some(&dir.path().join("profil"))).unwrap();
        let assets = locora_app_core::assets::Assets::register(&profile.store).unwrap();
        let mut b = Backend {
            profile,
            journey: None,
            compete: Default::default(),
            default_marker: String::new(),
            simo: String::new(),
            assets,
        };
        let today = run(&mut b, "compete_today", serde_json::json!({})).unwrap();
        assert_eq!(today["today"]["levels"].as_array().unwrap().len(), 3);
        assert_eq!(today["routes_needed"].as_array().unwrap().len(), 3);
        let start = serde_json::json!({
            "start": { "kind": "code", "code": "c2-28t5-cy4t-qkff-k", "lang": "en" }
        });
        let started = run(&mut b, "compete_start", start).unwrap();
        let deck = started["task"]["cards"].clone();
        assert_eq!(deck[0], 36, "the golden deck");
        assert_eq!(started["places"].as_array().unwrap().len(), 26);
        run(&mut b, "compete_recall", serde_json::json!({})).unwrap();
        let finish = serde_json::json!({ "finish": { "recall": { "cards": deck }, "lang": "en" } });
        let result = run(&mut b, "compete_finish", finish).unwrap();
        assert_eq!(result["finished"]["score"]["points"], 26);
        assert_eq!(result["code"], "C2-28T5-CY4T-QKFF-K");
        assert_eq!(result["correct"][0], "jack of diamonds");
        let history = run(&mut b, "compete_history", serde_json::json!({})).unwrap();
        assert_eq!(history.as_array().unwrap().len(), 1);
        let records = run(&mut b, "compete_records", serde_json::json!({})).unwrap();
        assert_eq!(records["trophies"][0]["id"], "compete-first");
        let choice = serde_json::json!({ "choice": { "discipline": "list", "level": 3 } });
        let code = run(&mut b, "compete_code", choice.clone()).unwrap();
        assert!(code.as_str().unwrap().starts_with("S3-"), "{code}");
        assert_eq!(
            run(&mut b, "compete_routes", choice).unwrap(),
            serde_json::json!([])
        );
        let contest = NAMES.iter().filter(|name| name.starts_with("compete_"));
        assert_eq!(contest.count(), 12);
        assert!(run(&mut b, "compete_finish", serde_json::json!({})).is_err());
    }
}
