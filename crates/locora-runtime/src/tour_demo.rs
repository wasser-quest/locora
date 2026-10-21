//! Simo's example palace for the app tour: one room, the tutorial's entrance hall, and no loci
//! yet. The tour then builds it in front of the user, through the editor like a person would:
//! it places a locus and fills in its fields. It is listed with the person's own palaces.

use anyhow::Result;
use locora_store::{RoomKind, SPLINELY};

use crate::Profile;
use crate::tutorial::language;

/// The example palace has the same stable id on every installation.
pub const NAMESPACE: &str = "locora-tour-demo";

/// The room's picture: the tutorial's entrance hall, with a door in the middle.
pub const ROOM: &[u8] = include_bytes!("../../../assets/tutorial/halle.sply");

/// The palace's name in a language.
pub fn name_for(code: &str) -> &'static str {
    match language(code) {
        "de" => "Simos Beispiel",
        "fr" => "L'exemple de Simo",
        "ja" => "シモの例",
        "zh" => "西莫的示例",
        _ => "Simo's example",
    }
}

/// The room's name in a language.
pub fn room_for(code: &str) -> &'static str {
    match language(code) {
        "de" => "Eingangshalle",
        "fr" => "Hall d'entrée",
        "ja" => "玄関ホール",
        "zh" => "门厅",
        _ => "Entrance hall",
    }
}

/// The id of the example palace: the existing one, or a new one with its one empty room.
pub fn ensure_in(profile: &mut Profile, code: &str) -> Result<String> {
    let id = locora_source::bundled_palace_id(NAMESPACE);
    if let Some(existing) = profile.store.find_palace(&id)?
        && existing.id == id
    {
        return Ok(id);
    }
    // A palace of the person's own may carry the name already.
    let base = name_for(code);
    let mut name = base.to_owned();
    let mut suffix = 2;
    while profile.store.find_palace(&name)?.is_some() {
        name = format!("{base} ({suffix})");
        suffix += 1;
    }
    let palace = profile.store.create_palace_with_id(&name, &id)?;
    let media = profile.store.put_media(ROOM, SPLINELY)?;
    profile.store.add_room(
        &palace.id,
        room_for(code),
        RoomKind::Image,
        Some(&media.hash),
    )?;
    Ok(palace.id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_example_palace_has_one_empty_room_and_is_made_once() {
        let dir = tempfile::tempdir().unwrap();
        let mut profile = Profile::open(Some(dir.path())).unwrap();
        let id = ensure_in(&mut profile, "de").unwrap();
        assert_eq!(ensure_in(&mut profile, "de").unwrap(), id);
        let tree = profile.store.palace(&id).unwrap();
        assert_eq!(tree.palace.name, "Simos Beispiel");
        assert_eq!(tree.rooms.len(), 1);
        assert_eq!(tree.rooms[0].room.name, "Eingangshalle");
        assert!(tree.rooms[0].loci.is_empty());
        // It is listed with the person's own palaces, to keep or delete.
        assert_eq!(crate::built_in_kind(&id), None);
    }
}
