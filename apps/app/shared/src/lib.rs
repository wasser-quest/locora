//! What the Leptos UI and the Tauri backend exchange (JSON over Tauri IPC).
//! Command names are the `CMD_*` constants; arguments and results are the
//! types below. Text meant for people is German and ready to show.

use serde::{Deserialize, Serialize};

pub const CMD_HOME: &str = "home";
pub const CMD_PALACE: &str = "palace";
pub const CMD_TUTORIAL: &str = "tutorial";
pub const CMD_JOURNEY_START: &str = "journey_start";
pub const CMD_JOURNEY_REVEAL: &str = "journey_reveal";
pub const CMD_JOURNEY_ANSWER: &str = "journey_answer";

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
    /// Problems while opening the profile (plugins that did not load).
    pub notes: Vec<String>,
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
    pub id: String,
    pub name: String,
    pub backdrop: Option<Media>,
    pub loci: Vec<LocusView>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LocusView {
    pub id: String,
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
    pub backdrop: Option<Media>,
    pub marker: Media,
    pub x: f64,
    pub y: f64,
    pub prompt: String,
    pub kind: AnswerKind,
    /// Quiz: the options shown, in order; answer with their positions.
    pub options: Vec<String>,
    /// Reverse journeys ask with the picture, which then is the question.
    pub picture: Option<Media>,
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
