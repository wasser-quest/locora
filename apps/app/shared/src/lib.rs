//! What the Leptos UI and the Tauri backend exchange (JSON over Tauri IPC).
//! Command names are the `CMD_*` constants; arguments and results are the
//! types below. Text meant for people is German and ready to show.

use serde::{Deserialize, Serialize};

pub use locora_api::journey::{Answer as EditorAnswer, AnswerKind as EditorAnswerKind, QuizOption};

pub const CMD_EDITOR_CHANGE: &str = "editor_change";
pub const CMD_EDITOR_PICK_PICTURE: &str = "editor_pick_picture";

/// Exact scope only; file paths never come from the WebView.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PictureTarget {
    pub palace_id: String,
    pub room_id: String,
    pub locus_id: String,
}
pub const CMD_HOME: &str = "home";
pub const CMD_HOME_PICK_MEDIA: &str = "home_pick_media";
pub const CMD_PALACE: &str = "palace";
pub const CMD_TUTORIAL: &str = "tutorial";
pub const CMD_FORMULA_GUIDE: &str = "formula_guide";
pub const CMD_NOTIFICATIONS: &str = "notifications";
pub const CMD_NOTIFICATION_SHOWN: &str = "notification_shown";
pub const CMD_JOURNEY_START: &str = "journey_start";
pub const CMD_JOURNEY_REVEAL: &str = "journey_reveal";
pub const CMD_JOURNEY_ANSWER: &str = "journey_answer";
pub const CMD_JOURNEY_INPUTS: &str = "journey_inputs";
pub const CMD_UI_ASSETS: &str = "ui_assets";
pub const CMD_IMPORT_PICK: &str = "import_pick";

/// The app's own animations (assets/ui).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UiAssets {
    /// Journey feedback: right, again, journey done.
    pub right: Media,
    pub again: Media,
    pub done: Media,
    /// Room kinds (tabs): no picture, picture, video.
    pub room_blank: Media,
    pub room_image: Media,
    pub room_video: Media,
    /// The Locora logo (loading) and Simo (resting, nothing due).
    pub logo: Media,
    pub simo: Media,
}
pub const CMD_EXPORT_PICK: &str = "export_pick";
pub const CMD_DUMP_PICK: &str = "dump_pick";

/// After importing a `.locora` file.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ImportResult {
    pub palaces: Vec<ImportedPalace>,
    /// Loci whose review schedule came along; `None` when the file had none.
    pub restored: Option<u32>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImportedPalace {
    pub id: String,
    pub name: String,
    /// Differs from `name` when the name was taken.
    pub original_name: String,
}

pub const CMD_SETTINGS: &str = "settings";
pub const CMD_SETTINGS_SET: &str = "settings_set";
pub const CMD_SETTINGS_OPTIMIZE: &str = "settings_optimize";
/// The system's offset from UTC in minutes, sent by the UI at every start.
pub const CMD_SET_UTC_OFFSET: &str = "set_utc_offset";

/// Argument of `CMD_SET_UTC_OFFSET` (sent as `{ "offset": … }`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct UtcOffset {
    /// Minutes east of UTC, e.g. 120 for summer time in Germany.
    pub minutes: i32,
}

/// Settings and learning statistics.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SettingsView {
    pub new_per_day: u32,
    pub max_new_per_day: u32,
    /// Target retention 0.7–0.99; `None` without the review plugin.
    pub retention: Option<f64>,
    /// Personal FSRS weights (from `locora optimize`) are in use.
    pub personal_weights: bool,
    pub stats: Option<StatsView>,
    /// Reviews at least a day apart (what fitting weights learns from) and
    /// how many it needs at least.
    #[serde(default)]
    pub usable_reviews: u32,
    #[serde(default)]
    pub needed_reviews: u32,
    /// Fitting (again) is worth it now.
    #[serde(default)]
    pub optimize_hint: bool,
    /// Learning days begin at this full hour, local time.
    #[serde(default)]
    pub day_start_hour: u8,
    /// „4:00 (UTC+2)“
    #[serde(default)]
    pub day_start: String,
}

/// After fitting personal FSRS weights in the app.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OptimizeDone {
    pub message: String,
    pub view: SettingsView,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct StatsView {
    pub answers: u32,
    pub answered_today: u32,
    pub new_today: u32,
    /// Answers of the last 30 days, and how many were not „Nochmal“.
    pub recent: u32,
    pub recent_known: u32,
    pub streak: u32,
    pub scheduled: u32,
    pub due: u32,
}

/// Change settings; `None` keeps a value.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SettingsChange {
    pub new_per_day: Option<u32>,
    /// As typed: `0,9`, `0.85`, `90 %`.
    pub retention: Option<String>,
    #[serde(default)]
    pub day_start_hour: Option<u8>,
}
pub const CMD_JOURNEY_ACTION: &str = "journey_action";

/// Custom URI scheme for media: `<scheme>://localhost/<kind>/<sha256>`
/// (`http://<scheme>.localhost/…` on Windows and Android; the UI asks Tauri's
/// `convertFileSrc` for the right form). Kinds: `media` (bytes as stored) and
/// `sply` (a `.sply` as interactive SVG with the Splinely runtime).
pub const SCHEME: &str = "locora";

/// The start screen.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Home {
    pub palaces: Vec<PalaceCard>,
    /// Due loci across all palaces.
    pub due: u32,
    /// New loci that today's limit still allows.
    pub fresh: u32,
    /// Is the tutorial palace there?
    pub tutorial: bool,
    /// Is the separate formula guide palace there?
    #[serde(default)]
    pub formula_guide: bool,
    /// Problems while opening the profile (plugins that did not load).
    pub notes: Vec<String>,
    /// The latest tile of every loaded plugin (`ui.tile`).
    #[serde(default)]
    pub tiles: Vec<TileView>,
    /// No palaces yet: Simo welcomes and points to the tutorial.
    #[serde(default)]
    pub welcome: Option<Media>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TileView {
    pub plugin: String,
    pub title: String,
    pub text: String,
    #[serde(default)]
    pub icon: Option<Media>,
}

/// A notification to show now. `action` names what a tap runs,
/// e.g. `review.start@Planeten` (start a journey through that palace).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotificationView {
    pub channel: String,
    /// `0` = informational, `1` = time-sensitive, `2` = urgent.
    pub priority: u8,
    pub title: String,
    pub body: String,
    pub action: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PalaceCard {
    pub id: String,
    pub name: String,
    pub rooms: u32,
    pub loci: u32,
    pub due: u32,
    pub fresh: u32,
    /// The first room's backdrop, as a thumbnail.
    pub cover: Option<Media>,
}

/// A stored file the UI can show.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Media {
    pub hash: String,
    pub kind: MediaKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MediaKind {
    Image,
    Video,
    /// A Splinely animation, shown through the `sply` URL kind.
    Sply,
}

impl Media {
    /// Path below the custom scheme, e.g. `sply/<hash>`.
    pub fn path(&self) -> String {
        match self.kind {
            MediaKind::Sply => format!("sply/{}", self.hash),
            MediaKind::Image | MediaKind::Video => format!("media/{}", self.hash),
        }
    }
}

/// A palace with its rooms, for the room view.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PalaceView {
    pub id: String,
    pub name: String,
    /// Locus marker (`.sply`, see assets/STIL.md 8b); the default when none.
    pub marker: Media,
    pub rooms: Vec<RoomView>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoomView {
    pub video: bool,
    pub id: String,
    pub name: String,
    pub backdrop: Option<Media>,
    pub loci: Vec<LocusView>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LocusView {
    pub answer: EditorAnswer,
    /// Keep the raw reference even if media cannot currently be displayed.
    pub image: Option<String>,
    pub bound: bool,
    /// Formula-bound fields (`place`, `item`, `x`, `y`, `visible`, `active`);
    /// only these stay read-only in the editor.
    #[serde(default)]
    pub bound_fields: Vec<String>,
    /// Picture width as a fraction of the room width.
    pub image_size: f64,
    /// Zoom on the locus while asking, 1 (whole room) to 4.
    #[serde(default = "whole_room")]
    pub zoom: f64,
    pub id: String,
    /// Video position in seconds; absent in non-video rooms.
    pub time: Option<f64>,
    /// Route number across the palace, from 1.
    pub number: u32,
    pub place: String,
    pub item: String,
    /// Position 0..1 from the left / top.
    pub x: f64,
    pub y: f64,
    pub status: Status,
    /// Belongs to the answer: shown only once the locus is revealed.
    pub picture: Option<Media>,
    pub note: String,
}

impl LocusView {
    pub fn is_bound(&self, field: &str) -> bool {
        self.bound_fields.iter().any(|f| f == field)
    }
}

/// Full content replacement; geometry is deliberately patched separately.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LocusContent {
    pub place: String,
    pub item: String,
    pub note: String,
    pub image: Option<String>,
    pub image_size: f64,
    #[serde(default = "whole_room")]
    pub zoom: f64,
    pub answer: EditorAnswer,
}

/// Zoom when none is given: the whole room.
pub fn whole_room() -> f64 {
    1.0
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EditorChange {
    pub palace_id: String,
    pub room_id: String,
    pub action: EditorAction,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum EditorAction {
    Create {
        x: f64,
        y: f64,
    },
    CreateVideo {
        time: f64,
    },
    Paste {
        items: Vec<String>,
    },
    Save {
        id: String,
        content: LocusContent,
    },
    Move {
        id: String,
        x: f64,
        y: f64,
    },
    Resize {
        id: String,
        image_size: f64,
    },
    Delete {
        id: String,
    },
    /// Backend-issued, scoped, single-use structural undo/redo capability.
    Restore {
        receipt: String,
    },
}

pub const MAX_PASTE_ITEMS: usize = 100;
pub const MAX_PASTE_ITEM_BYTES: usize = 500;
pub const MAX_PASTE_BYTES: usize = 64 * 1024;

/// Validate again at the IPC boundary; the UI is not trusted.
pub fn validate_paste_items(items: &[String]) -> Result<(), &'static str> {
    if items.is_empty() || items.len() > MAX_PASTE_ITEMS {
        return Err("Bitte 1 bis 100 Begriffe einfügen");
    }
    if items.iter().any(|item| {
        item.trim().is_empty() || item.len() > MAX_PASTE_ITEM_BYTES || item.contains(['\n', '\r'])
    }) {
        return Err("Jeder Begriff braucht eine nichtleere Zeile mit höchstens 500 Bytes");
    }
    Ok(())
}

/// Serpentine route, inset from the edges. Call with index < count, count > 0.
pub fn paste_route_position(index: usize, count: usize) -> (f64, f64) {
    let columns = (count as f64).sqrt().ceil() as usize;
    let rows = count.div_ceil(columns);
    let row = index / columns;
    let column = if row.is_multiple_of(2) {
        index % columns
    } else {
        columns - 1 - index % columns
    };
    let axis = |n: usize, total: usize| {
        if total == 1 {
            0.5
        } else {
            0.1 + 0.8 * n as f64 / (total - 1) as f64
        }
    };
    (axis(column, columns), axis(row, rows))
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EditorUpdated {
    pub palace: PalaceView,
    pub selected: Option<String>,
    /// Reciprocal receipt after a structural mutation or restore.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt: Option<String>,
}

/// Marker states; the names are the marker's state machine events.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Neu,
    Faellig,
    Gewusst,
}

impl Status {
    pub fn event(self) -> &'static str {
        match self {
            Status::Neu => "neu",
            Status::Faellig => "faellig",
            Status::Gewusst => "gewusst",
        }
    }
}

/// Start a journey through one palace or (without `palace_id`) everything due.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct JourneyStart {
    pub palace_id: Option<String>,
    pub due_only: bool,
    pub reverse: bool,
    pub numbers: bool,
    pub shuffle: bool,
    /// Values for workbook inputs (`Inputs` table), asked before the start.
    #[serde(default)]
    pub inputs: Vec<InputValue>,
}

/// A workbook input a journey asks for before it starts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InputField {
    pub palace_id: String,
    pub palace: String,
    pub key: String,
    /// `text`, `number`, `boolean`, …
    pub kind: String,
    /// The default as text.
    pub default: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InputValue {
    pub palace_id: String,
    pub key: String,
    pub value: String,
}

/// After running a workbook action (`!name`) on the current question.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActionDone {
    /// The action asks to show the answer.
    pub reveal: bool,
    /// The question again, with fields recomputed.
    pub question: Question,
    /// The answer side, when it is (or becomes) visible.
    pub revealed: Option<Revealed>,
    /// After grading: the next question, recomputed (actions change the run
    /// values later stops use). `None` at the end of the journey.
    #[serde(default)]
    pub next: Option<Question>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AnswerKind {
    Recall,
    Guess,
    Quiz,
}

/// One question of a journey.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Question {
    /// 1-based position and length of the journey.
    pub n: u32,
    pub total: u32,
    pub palace: String,
    pub room: String,
    /// Video position for this locus, in seconds.
    pub time: Option<f64>,
    pub backdrop: Option<Media>,
    pub marker: Media,
    pub x: f64,
    pub y: f64,
    /// The stage zooms in on the locus by this factor (1 = whole room).
    #[serde(default = "whole_room")]
    pub zoom: f64,
    pub prompt: String,
    pub kind: AnswerKind,
    /// Quiz: the options shown, in order; answer with their positions.
    pub options: Vec<String>,
    /// Reverse journeys ask with the picture, which then is the question.
    pub picture: Option<Media>,
    /// Workbook actions of this palace, run with `journey_action`.
    #[serde(default)]
    pub actions: Vec<String>,
}

/// What the learner did.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "lowercase")]
pub enum Reply {
    /// Self-rating 1 (Nochmal) … 4 (Leicht), after revealing.
    Recall(u8),
    Guess(String),
    /// Positions in `Question::options`.
    Quiz(Vec<u32>),
    /// Revealed without answering (counts as Nochmal).
    Forfeit,
}

/// The answer side of a question: shown on reveal and after answering.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Revealed {
    /// Picture width as a fraction of the room width.
    pub image_size: f64,
    pub solution: String,
    pub note: String,
    /// The locus picture; part of the answer (assets/STIL.md 8a).
    pub picture: Option<Media>,
    /// Quiz: positions of the right options among those shown.
    pub right: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Feedback {
    pub correct: bool,
    pub points: u64,
    pub revealed: Revealed,
    pub next: Step,
}

/// What follows an answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
// Sent once per question as JSON; boxing would only complicate the UI code.
#[allow(clippy::large_enum_variant)]
pub enum Step {
    Question(Question),
    Done(Summary),
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Summary {
    pub asked: u32,
    pub correct: u32,
    pub points: u64,
    /// Messages from plugins (e.g. learning time).
    pub notes: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_paths() {
        let sply = Media {
            hash: "ab".into(),
            kind: MediaKind::Sply,
        };
        assert_eq!(sply.path(), "sply/ab");
        let png = Media {
            hash: "cd".into(),
            kind: MediaKind::Image,
        };
        assert_eq!(png.path(), "media/cd");
    }
}
