//! `locora`: memory palaces from the command line, and plugin development.

mod app;
mod palace;
mod places;
mod plugin;
mod practice;

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use locora_store::LocusPatch;

#[derive(Parser)]
#[command(
    name = "locora",
    version,
    about = "Gedächtnispaläste und Plugins für Locora",
    after_help = "Beispiele:\n  locora new \"Planeten\" -- Merkur Venus Erde Mars\n  locora practice Planeten\n  locora due\n  locora review"
)]
struct Cli {
    /// Datenordner (Standard: $LOCORA_DATA oder der Datenordner des Systems)
    #[arg(long, global = true, value_name = "ORDNER")]
    data: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Neuen Palast anlegen, optional mit Bild/Video und Loci
    New {
        /// Name des Palasts
        name: String,
        /// Bild als Hintergrund (png, jpg, webp, gif, avif)
        #[arg(long, conflicts_with = "video")]
        image: Option<PathBuf>,
        /// Video als Hintergrund (mp4, webm, mov); Loci brauchen dann Zeiten: 1:23=These
        #[arg(long)]
        video: Option<PathBuf>,
        /// Loci aus Datei: je Zeile `Erinnerung`, `Ort | Erinnerung` oder `Ort | Erinnerung | x,y`
        #[arg(long, value_name = "DATEI")]
        from: Option<PathBuf>,
        /// Erinnerungen, je eine pro Locus, in Routen-Reihenfolge
        #[arg(trailing_var_arg = true)]
        items: Vec<String>,
    },
    /// Locus an einen Palast anhängen
    Add {
        palace: String,
        /// Was man sich an diesem Ort merkt
        memory: String,
        /// Name des Orts (Standard: „Ort N“)
        #[arg(long)]
        label: Option<String>,
        /// Position auf der Stage, x,y zwischen 0 und 1
        #[arg(long)]
        at: Option<String>,
        /// Zeit im Video (mm:ss oder Sekunden)
        #[arg(long)]
        time: Option<String>,
    },
    /// Locus ändern (Nummer wie in `locora show`)
    Edit {
        palace: String,
        locus: usize,
        #[arg(long)]
        memory: Option<String>,
        #[arg(long)]
        label: Option<String>,
        #[arg(long)]
        at: Option<String>,
    },
    /// Alle Paläste mit fälligen und neuen Loci
    #[command(alias = "list")]
    Ls,
    /// Einen Palast mit allen Loci zeigen
    Show { palace: String },
    /// Palast löschen, oder nur einen Locus (Nummer wie in `locora show`)
    Rm {
        palace: String,
        locus: Option<usize>,
        /// Ohne Rückfrage
        #[arg(long, short)]
        yes: bool,
    },
    /// Einen Palast üben (Route ablaufen, erinnern, bewerten)
    Practice {
        palace: String,
        /// Nur fällige Loci
        #[arg(long)]
        due: bool,
        /// Höchstens so viele neue Loci (mit --due)
        #[arg(long, default_value_t = 10)]
        new: usize,
    },
    /// Fällige Loci wiederholen (alle Paläste oder einer)
    Review {
        palace: Option<String>,
        /// Höchstens so viele neue Loci
        #[arg(long, default_value_t = 10)]
        new: usize,
    },
    /// Was ist heute fällig?
    Due,
    /// Plugins erstellen, entwickeln und testen
    #[command(subcommand)]
    Plugin(plugin::PluginCommand),
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let data = cli.data.as_deref();
    match cli.command {
        Command::Plugin(cmd) => plugin::run(cmd),
        Command::New {
            name,
            image,
            video,
            from,
            items,
        } => palace::new(
            &mut app::App::open(data)?,
            palace::NewArgs {
                name,
                media: image.or(video),
                from,
                items,
            },
        ),
        Command::Add {
            palace,
            memory,
            label,
            at,
            time,
        } => palace::add(
            &mut app::App::open(data)?,
            palace::AddArgs {
                palace,
                memory,
                label,
                at,
                time,
            },
        ),
        Command::Edit {
            palace,
            locus,
            memory,
            label,
            at,
        } => {
            let point = match at {
                Some(text) => {
                    let (x, y) = text
                        .split_once(',')
                        .and_then(|(x, y)| Some((x.trim().parse().ok()?, y.trim().parse().ok()?)))
                        .context("--at erwartet x,y zwischen 0 und 1")?;
                    (Some(x), Some(y))
                }
                None => (None, None),
            };
            palace::edit(
                &mut app::App::open(data)?,
                &palace,
                locus,
                LocusPatch {
                    label,
                    memory,
                    x: point.0,
                    y: point.1,
                    at: None,
                },
            )
        }
        Command::Ls => palace::list(&mut app::App::open(data)?),
        Command::Show { palace } => palace::show(&mut app::App::open(data)?, &palace),
        Command::Rm { palace, locus, yes } => {
            palace::remove(&mut app::App::open(data)?, &palace, locus, yes)
        }
        Command::Practice { palace, due, new } => practice::run(
            &mut app::App::open(data)?,
            Some(&palace),
            practice::Options { due_only: due, new },
        ),
        Command::Review { palace, new } => practice::run(
            &mut app::App::open(data)?,
            palace.as_deref(),
            practice::Options {
                due_only: true,
                new,
            },
        ),
        Command::Due => palace::due(&mut app::App::open(data)?),
    }
}
