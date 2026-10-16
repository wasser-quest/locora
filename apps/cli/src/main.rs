//! `locora`: memory palaces from the command line, and plugin development.

mod app;
mod palace;
mod plugin;
mod practice;
mod settings;
mod source;
mod transfer;
mod tutorial;
mod workbook;

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
        /// Loci aus Datei: je Zeile `Inhalt`, `Ort | Inhalt` oder `Ort | Inhalt | x,y`
        #[arg(long, value_name = "DATEI")]
        from: Option<PathBuf>,
        /// Inhalte, je einer pro Locus, in Routen-Reihenfolge
        #[arg(trailing_var_arg = true)]
        items: Vec<String>,
    },
    /// Locus an einen Palast anhängen
    Add {
        palace: String,
        /// Was man an diesem Ort ablegt
        item: String,
        /// Name des Orts (Standard: „Ort N“)
        #[arg(long)]
        place: Option<String>,
        /// Position im Raum, x,y zwischen 0 und 1
        #[arg(long)]
        at: Option<String>,
        /// Zeit im Video (mm:ss oder Sekunden)
        #[arg(long)]
        time: Option<String>,
        /// Bild für den Ort (png, jpg, webp, gif, avif oder eine .sply-Animation)
        #[arg(long, value_name = "DATEI")]
        image: Option<PathBuf>,
        /// Picture width as a fraction of the room width.
        #[arg(long, value_parser = palace::parse_image_size, help = "Bildbreite als Anteil der Raumbreite (0.05 bis 1, Standard: 0.25)")]
        image_size: Option<f64>,
        #[arg(long, value_parser = palace::parse_zoom, help = "Zoom auf den Ort beim Abfragen (1 = ganzer Raum bis 4)")]
        zoom: Option<f64>,
        /// Merkhilfe: das lebhafte Bild in Worten, gezeigt nach dem Aufdecken
        #[arg(long)]
        note: Option<String>,
    },
    /// Locus ändern (Nummer wie in `locora show`)
    Edit {
        palace: String,
        locus: usize,
        /// Antwortkonfiguration als JSON: {"type":"guess"} oder Quiz mit options
        #[arg(long, conflicts_with = "accept")]
        answer: Option<String>,
        /// Als Guess abfragen; jede weitere gültige Antwort einzeln (mehrfach angeben)
        #[arg(long, value_name = "TEXT")]
        accept: Vec<String>,
        /// Neue Merkhilfe ("" entfernt sie)
        #[arg(long)]
        note: Option<String>,
        /// Neuer Inhalt
        #[arg(long)]
        item: Option<String>,
        /// Neuer Ortsname
        #[arg(long)]
        place: Option<String>,
        #[arg(long)]
        at: Option<String>,
        /// Neues Bild für den Ort (Bild oder .sply-Animation)
        #[arg(long, value_name = "DATEI", conflicts_with = "no_image")]
        image: Option<PathBuf>,
        /// Bild des Orts entfernen
        #[arg(long)]
        no_image: bool,
        /// Picture width as a fraction of the room width.
        #[arg(long, value_parser = palace::parse_image_size, help = "Bildbreite als Anteil der Raumbreite (0.05 bis 1)")]
        image_size: Option<f64>,
        #[arg(long, value_parser = palace::parse_zoom, help = "Zoom auf den Ort beim Abfragen (1 = ganzer Raum bis 4)")]
        zoom: Option<f64>,
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
        /// Höchstens so viele neue Loci (mit --due; Standard: was vom Tageslimit übrig ist)
        #[arg(long)]
        new: Option<usize>,
        #[command(flatten)]
        modes: practice::Modes,
    },
    /// Fällige Loci wiederholen (alle Paläste oder einer)
    Review {
        palace: Option<String>,
        /// Höchstens so viele neue Loci (Standard: was vom Tageslimit übrig ist)
        #[arg(long)]
        new: Option<usize>,
        #[command(flatten)]
        modes: practice::Modes,
    },
    /// Was ist heute fällig?
    Due,
    /// Einstellungen anzeigen oder ändern
    Settings {
        #[command(subcommand)]
        command: Option<settings::SettingsCommand>,
    },
    /// FSRS-Gewichte aus dem eigenen Wiederholungsverlauf berechnen
    Optimize {
        /// Nur berechnen und anzeigen
        #[arg(long, conflicts_with = "reset")]
        dry_run: bool,
        /// Zurück zu den Standardgewichten
        #[arg(long)]
        reset: bool,
    },
    /// Lernstatistik: Antworten, Trefferquote, Serie
    Stats,
    /// Marker der Orte eines Palasts zeigen oder ändern (eigene .sply-Animation)
    Marker {
        palace: String,
        /// Eigene Marker-Animation (.sply mit Zuständen neu/faellig/gewusst)
        #[arg(conflicts_with = "reset")]
        file: Option<PathBuf>,
        /// Zurück zum Standard-Marker
        #[arg(long)]
        reset: bool,
    },
    /// Tutorial-Palast „Locora lernen“ anlegen (Locora mit der Loci-Methode lernen)
    Tutorial {
        /// Vorhandenes Tutorial neu anlegen (sein Wiederholungsplan geht verloren)
        #[arg(long)]
        reset: bool,
    },
    /// Paläste als .locora-Datei speichern (mit Bildern und Videos)
    Export {
        /// Paläste (Name oder Id); leer mit --all
        palaces: Vec<String>,
        /// Alle Paläste
        #[arg(long, conflicts_with = "palaces")]
        all: bool,
        /// Zieldatei (Standard: <Palast>.locora)
        #[arg(long, short, value_name = "DATEI")]
        output: Option<PathBuf>,
        /// Wiederholungsstand (Plugin `review`) mitnehmen
        #[arg(long)]
        with_progress: bool,
    },
    /// .locora-Datei importieren (immer als neue Kopie)
    Import {
        file: PathBuf,
        /// Nur prüfen und anzeigen, nichts importieren
        #[arg(long)]
        dry_run: bool,
        /// Mitgelieferten Wiederholungsstand nicht übernehmen
        #[arg(long)]
        without_progress: bool,
    },
    /// Palast aus einer locora.toml-Quelltextdatei bauen
    Build {
        /// Quelltextdatei, z. B. planeten.locora.toml
        file: PathBuf,
        /// Nur prüfen, nichts anlegen
        #[arg(long)]
        check: bool,
        /// Vorhandenen Palast gleichen Namens ersetzen (sein Wiederholungsplan geht verloren)
        #[arg(long)]
        replace: bool,
        /// Ohne Rückfrage ersetzen
        #[arg(long, short)]
        yes: bool,
    },
    /// Palast als locora.toml plus Mediendateien in einen Ordner schreiben
    Dump {
        palace: String,
        /// Zielordner (Standard: aktueller Ordner)
        #[arg(long, short, value_name = "ORDNER", default_value = ".")]
        output: PathBuf,
    },
    /// Workbook-Tabellen anzeigen und bearbeiten
    #[command(subcommand)]
    Table(workbook::TableCommand),
    /// Locus-Feld an eine Formel binden
    Bind {
        palace: String,
        locus: usize,
        field: String,
        #[arg(allow_hyphen_values = true)]
        expression: String,
    },
    /// Formelbindung eines Locus-Felds entfernen
    Unbind {
        palace: String,
        locus: usize,
        field: String,
    },
    /// Formel berechnen: [PALAST] FORMEL (ohne Palast: gemeinsame Tabellen)
    Calc {
        #[arg(allow_hyphen_values = true)]
        first: String,
        #[arg(allow_hyphen_values = true)]
        expression: Option<String>,
    },
    /// Plugins erstellen, entwickeln und testen
    #[command(subcommand)]
    Plugin(plugin::PluginCommand),
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let data = cli.data.as_deref();
    match cli.command {
        Command::Plugin(cmd) => plugin::run(cmd, data),
        Command::Table(cmd) => workbook::table(&mut app::App::open(data)?, cmd),
        Command::Bind {
            palace,
            locus,
            field,
            expression,
        } => workbook::bind(
            &mut app::App::open(data)?,
            &palace,
            locus,
            &field,
            Some(expression),
        ),
        Command::Unbind {
            palace,
            locus,
            field,
        } => workbook::bind(&mut app::App::open(data)?, &palace, locus, &field, None),
        Command::Calc { first, expression } => {
            workbook::calc(&mut app::App::open(data)?, &first, expression.as_deref())
        }
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
            item,
            place,
            at,
            time,
            image,
            image_size,
            zoom,
            note,
        } => palace::add(
            &mut app::App::open(data)?,
            palace::AddArgs {
                palace,
                item,
                place,
                at,
                time,
                image,
                image_size,
                zoom,
                note,
            },
        ),
        Command::Edit {
            palace,
            locus,
            answer,
            accept,
            note,
            item,
            place,
            at,
            image,
            no_image,
            image_size,
            zoom,
        } => {
            let mut app = app::App::open(data)?;
            let image = match (image, no_image) {
                (Some(path), _) => Some(Some(palace::store_picture(&app, &path)?)),
                (None, true) => Some(None),
                (None, false) => None,
            };
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
                &mut app,
                &palace,
                locus,
                LocusPatch {
                    place,
                    item,
                    x: point.0,
                    y: point.1,
                    time: None,
                    answer: if accept.is_empty() {
                        answer
                            .map(|text| serde_json::from_str(&text))
                            .transpose()
                            .context("Ungültige Antwortkonfiguration")?
                    } else {
                        Some(locora_api::journey::Answer {
                            kind: locora_api::journey::AnswerKind::Guess,
                            accept,
                            ..Default::default()
                        })
                    },
                    image,
                    image_size,
                    zoom,
                    note,
                },
            )
        }
        Command::Ls => palace::list(&mut app::App::open(data)?),
        Command::Show { palace } => palace::show(&mut app::App::open(data)?, &palace),
        Command::Rm { palace, locus, yes } => {
            palace::remove(&mut app::App::open(data)?, &palace, locus, yes)
        }
        Command::Practice {
            palace,
            due,
            new,
            modes,
        } => {
            let mut app = app::App::open(data)?;
            let daily_limit = new.is_none() && due;
            let new = match new {
                Some(n) => n,
                None if due => settings::new_budget(&mut app)?,
                None => 0,
            };
            practice::run(
                &mut app,
                Some(&palace),
                practice::Options {
                    due_only: due,
                    new,
                    daily_limit,
                    modes,
                },
            )
        }
        Command::Review { palace, new, modes } => {
            let mut app = app::App::open(data)?;
            let daily_limit = new.is_none();
            let new = match new {
                Some(n) => n,
                None => settings::new_budget(&mut app)?,
            };
            practice::run(
                &mut app,
                palace.as_deref(),
                practice::Options {
                    due_only: true,
                    new,
                    daily_limit,
                    modes,
                },
            )
        }
        Command::Settings { command } => settings::settings(&mut app::App::open(data)?, command),
        Command::Optimize { dry_run, reset } => {
            settings::optimize_command(&mut app::App::open(data)?, dry_run, reset)
        }
        Command::Stats => settings::stats(&mut app::App::open(data)?),
        Command::Marker {
            palace,
            file,
            reset,
        } => palace::marker(&mut app::App::open(data)?, &palace, file.as_deref(), reset),
        Command::Tutorial { reset } => tutorial::run(&mut app::App::open(data)?, reset),
        Command::Due => palace::due(&mut app::App::open(data)?),
        Command::Export {
            palaces,
            all,
            output,
            with_progress,
        } => transfer::export(
            &mut app::App::open(data)?,
            palaces,
            all,
            output,
            with_progress,
        ),
        Command::Import {
            file,
            dry_run,
            without_progress,
        } => transfer::import(&mut app::App::open(data)?, &file, dry_run, without_progress),
        Command::Build {
            file,
            check,
            replace,
            yes,
        } => source::build(data, &file, check, replace, yes),
        Command::Dump { palace, output } => {
            source::dump(&mut app::App::open(data)?, &palace, &output)
        }
    }
}
