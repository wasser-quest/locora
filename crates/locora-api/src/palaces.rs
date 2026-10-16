//! Host-owned palace service. Snapshots contain route data, not workbook or media bytes.
use super::*;

pub struct Palaces;
impl Service for Palaces {
    const NAME: &'static str = "core.palaces@1";
    type Request = Request;
    type Response = Response;
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Request {
    List,
    /// Exact id, case-insensitive name or unique id prefix (at least four characters).
    Get {
        query: String,
    },
    Create {
        name: String,
    },
    Rename {
        id: String,
        name: String,
    },
    Delete {
        id: String,
    },
    AddRoom {
        palace_id: String,
        name: String,
        kind: RoomKind,
        media: Option<String>,
    },
    AddLocus {
        room_id: String,
        locus: NewLocus,
    },
    /// Returns Done, not the stored locus (write permission does not grant read access).
    UpdateLocus {
        id: String,
        patch: LocusPatch,
    },
    DeleteLocus {
        id: String,
    },
}

impl Request {
    /// Write permission intentionally does not imply read permission.
    pub fn permission(&self) -> &'static str {
        match self {
            Self::List | Self::Get { .. } => permission::PALACES_READ,
            _ => permission::PALACES_WRITE,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Response {
    List(Vec<Summary>),
    Palace(Palace),
    Created {
        id: String,
        name: String,
    },
    Room(Room),
    Locus(Locus),
    Done,
    /// False when the palace or locus did not exist.
    Deleted(bool),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Summary {
    pub id: String,
    pub name: String,
    pub rooms: u32,
    pub loci: u32,
    pub updated_at: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Palace {
    pub id: String,
    pub name: String,
    pub rooms: Vec<Room>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RoomKind {
    Blank,
    Image,
    Video,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Room {
    pub id: String,
    pub palace_id: String,
    pub name: String,
    pub kind: RoomKind,
    /// Existing media hash, never a path or URL.
    pub media: Option<String>,
    pub loci: Vec<Locus>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Locus {
    pub id: String,
    pub room_id: String,
    pub place: String,
    pub item: String,
    pub x: f64,
    pub y: f64,
    pub time: Option<f64>,
    #[serde(default)]
    pub answer: crate::journey::Answer,
    /// Memory aid shown after the answer.
    #[serde(default)]
    pub note: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewLocus {
    pub place: String,
    pub item: String,
    pub x: f64,
    pub y: f64,
    pub time: Option<f64>,
    #[serde(default)]
    pub answer: crate::journey::Answer,
    #[serde(default)]
    pub note: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LocusPatch {
    pub place: Option<String>,
    pub item: Option<String>,
    pub x: Option<f64>,
    pub y: Option<f64>,
    /// None keeps the timestamp, Some(None) clears it.
    pub time: Option<Option<f64>>,
    pub answer: Option<crate::journey::Answer>,
    #[serde(default)]
    pub note: Option<String>,
}
