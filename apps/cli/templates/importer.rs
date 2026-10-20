//! Text importer: `locora plugin invoke {{id}} import --file daten.txt`.
//! Input: first line is a new palace name; remaining lines are `Ort | Inhalt`.
//! The host grants only palaces:write, and does not give this plugin file access.
use locora_sdk::prelude::*;

#[locora_plugin(name = {{name_literal}}, permissions = [palaces_write])]
struct {{type}};

struct Import;
impl Service for Import {
    const NAME: &'static str = "{{id}}.import@1";
    type Request = String;
    type Response = String;
}

impl Plugin for {{type}} {
    fn activate(ctx: &mut Ctx) -> Result {
        ctx.provide::<Import>(|ctx, input| {
            let mut lines = input.lines().map(str::trim).filter(|line| !line.is_empty());
            let name = lines.next().ok_or_else(|| Error::msg("Palastname fehlt"))?;
            if name.len() > 200 {
                return Err(Error::msg("Palastname ist zu lang"));
            }
            let loci: Vec<_> = lines
                .map(|line| {
                    let (place, item) = line
                        .split_once('|')
                        .ok_or_else(|| Error::msg("Erwartet Ort | Inhalt"))?;
                    let (place, item) = (place.trim(), item.trim());
                    if place.is_empty() || item.is_empty() {
                        return Err(Error::msg("Ort und Inhalt dürfen nicht leer sein"));
                    }
                    Ok((place.to_owned(), item.to_owned()))
                })
                .collect::<Result<_>>()?;
            if loci.is_empty() || loci.len() > 100 {
                return Err(Error::msg("Erwartet 1 bis 100 Loci"));
            }
            let palaces::Response::Created { id, .. } =
                ctx.palaces(&palaces::Request::Create { name: name.into() })?
            else {
                return Err(Error::msg("Palast konnte nicht angelegt werden"));
            };
            let palaces::Response::Room(room) = ctx.palaces(&palaces::Request::AddRoom {
                palace_id: id,
                name: "Import".into(),
                kind: palaces::RoomKind::Blank,
                media: None,
            })? else {
                return Err(Error::msg("Raum konnte nicht angelegt werden"));
            };
            let count = loci.len();
            for (place, item) in loci {
                ctx.palaces(&palaces::Request::AddLocus {
                    room_id: room.id.clone(),
                    locus: palaces::NewLocus {
                        place, item, x: 0.5, y: 0.5, time: None,
                        answer: Answer::default(), note: String::new(), image: None,
                        image_size: palaces::default_image_size(),
                        zoom: palaces::default_zoom(),
                    },
                })?;
            }
            Ok(format!("{count} Loci importiert"))
        })
    }
}
