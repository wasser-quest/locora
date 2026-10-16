//! `locora plugin …`: scaffold, build, hot-reload and inspect plugins.

use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};
use std::{env, fs};

use anyhow::{Context, Result, bail};
use clap::{Subcommand, ValueEnum};
use locora_api::core::JourneyCompleted;
use locora_api::host::Started;
use locora_host::{HostEvent, Level, PluginHost};
use notify::{RecursiveMode, Watcher};

const WASM_TARGET: &str = "wasm32-wasip2";

#[derive(Subcommand)]
pub enum PluginCommand {
    /// Neues Plugin-Projekt aus einer Vorlage anlegen
    New {
        /// Name des Plugins (wird auch der Ordnername)
        name: String,
        #[arg(long, short, value_enum, default_value_t = Template::Ereignis)]
        template: Template,
    },
    /// Plugin bauen, laden und bei jeder Änderung live neu laden
    Dev {
        /// Plugin-Ordner (Standard: aktueller Ordner)
        #[arg(default_value = ".")]
        dir: PathBuf,
        /// Nach jedem Laden eine Beispiel-Journey senden
        #[arg(long)]
        demo: bool,
        /// Einmal bauen und laden, nicht beobachten
        #[arg(long)]
        once: bool,
    },
    /// Fertige Plugin-Dateien (.wasm) laden und Beispiel-Events senden
    Run {
        #[arg(required = true)]
        files: Vec<PathBuf>,
    },
    /// Plugin-Datei prüfen und Manifest anzeigen
    Check { file: PathBuf },
}

#[derive(Clone, Copy, ValueEnum)]
pub enum Template {
    /// Reagiert auf Ereignisse (z. B. beendete Journeys)
    Ereignis,
    /// Zeigt eine eigene Kachel auf dem Startbildschirm
    Kachel,
}

pub fn run(cmd: PluginCommand) -> Result<()> {
    match cmd {
        PluginCommand::New { name, template } => new(&name, template),
        PluginCommand::Dev { dir, demo, once } => dev(&dir, demo, once),
        PluginCommand::Run { files } => run_files(&files),
        PluginCommand::Check { file } => check(&file),
    }
}

fn host() -> Result<PluginHost> {
    PluginHost::new(dirs::cache_dir().map(|d| d.join("locora").join("plugins")))
}

fn demo_journey() -> JourneyCompleted {
    JourneyCompleted {
        palace_id: "planeten".into(),
        correct: 8,
        total: 9,
        duration_secs: 95,
    }
}

fn new(name: &str, template: Template) -> Result<()> {
    let id = plugin_id(name);
    if id.is_empty() {
        bail!("Der Name braucht mindestens einen Buchstaben oder eine Ziffer");
    }
    let dir = PathBuf::from(&id);
    if dir.exists() {
        bail!("Ordner `{id}` existiert bereits");
    }
    let type_name: String = id
        .split('-')
        .map(|part| {
            let mut chars = part.chars();
            chars
                .next()
                .map(|c| c.to_ascii_uppercase().to_string() + chars.as_str())
                .unwrap_or_default()
        })
        .collect();
    let source = match template {
        Template::Ereignis => include_str!("../templates/ereignis.rs"),
        Template::Kachel => include_str!("../templates/kachel.rs"),
    }
    .replace("{{name}}", name.trim())
    .replace("{{type}}", &type_name);

    fs::create_dir_all(dir.join("src"))?;
    fs::create_dir_all(dir.join(".cargo"))?;
    fs::write(
        dir.join("Cargo.toml"),
        format!(
            r#"[package]
name = "{id}"
version = "0.1.0"
edition = "2024"

[lib]
crate-type = ["cdylib"]

[dependencies]
locora-sdk = {sdk}

[profile.release]
lto = true
opt-level = "s"
strip = true
"#,
            sdk = sdk_dependency()
        ),
    )?;
    fs::write(
        dir.join(".cargo/config.toml"),
        format!("[build]\ntarget = \"{WASM_TARGET}\"\n"),
    )?;
    fs::write(dir.join(".gitignore"), "/target\n")?;
    fs::write(dir.join("src/lib.rs"), source)?;

    println!("✓ Plugin `{name}` angelegt in ./{id}");
    println!();
    println!("  cd {id}");
    println!("  locora plugin dev --demo");
    Ok(())
}

/// Turn a display name like „Mein Zähler“ into a crate/plugin id like `mein-zaehler`.
fn plugin_id(name: &str) -> String {
    let mut id = String::new();
    for c in name.trim().to_lowercase().chars() {
        match c {
            'ä' => id.push_str("ae"),
            'ö' => id.push_str("oe"),
            'ü' => id.push_str("ue"),
            'ß' => id.push_str("ss"),
            c if c.is_ascii_alphanumeric() => id.push(c),
            _ if !id.is_empty() && !id.ends_with('-') => id.push('-'),
            _ => {}
        }
    }
    let id = id.trim_end_matches('-');
    // Crate names must not start with a digit.
    if id.starts_with(|c: char| c.is_ascii_digit()) {
        format!("plugin-{id}")
    } else {
        id.to_owned()
    }
}

/// Use the local SDK when running from a checkout, otherwise the git repo.
fn sdk_dependency() -> String {
    let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/locora-sdk");
    match local.canonicalize() {
        Ok(path) => format!("{{ path = {:?} }}", path.display().to_string()),
        Err(_) => r#"{ git = "https://github.com/wasser-quest/locora" }"#.to_owned(),
    }
}

fn dev(dir: &Path, demo: bool, once: bool) -> Result<()> {
    let dir = dir
        .canonicalize()
        .with_context(|| format!("Ordner {} nicht gefunden", dir.display()))?;
    if !dir.join("Cargo.toml").exists() {
        bail!(
            "{} ist kein Plugin-Ordner (keine Cargo.toml). Neues Plugin: `locora plugin new <name>`",
            dir.display()
        );
    }
    ensure_wasm_target()?;

    let mut host = host()?;
    let rebuild = |host: &mut PluginHost| -> Result<()> {
        let started = Instant::now();
        let files = build(&dir)?;
        for file in &files {
            host.load_file(file)?;
        }
        host.publish(&Started)?;
        if demo {
            host.publish(&demo_journey())?;
        }
        print_events(host);
        println!(
            "  gebaut und geladen in {:.1} s",
            started.elapsed().as_secs_f32()
        );
        Ok(())
    };

    if let Err(err) = rebuild(&mut host) {
        if once {
            return Err(err);
        }
        eprintln!("✗ {err:#}");
    }
    if once {
        return Ok(());
    }

    let (tx, rx) = mpsc::channel();
    let mut watcher = notify::recommended_watcher(tx)?;
    watcher.watch(&dir.join("src"), RecursiveMode::Recursive)?;
    watcher.watch(&dir.join("Cargo.toml"), RecursiveMode::NonRecursive)?;
    println!(
        "👀 Beobachte {} … (Strg+C beendet)",
        dir.join("src").display()
    );

    while let Ok(event) = rx.recv() {
        if !matches!(event, Ok(ref e) if e.kind.is_modify() || e.kind.is_create() || e.kind.is_remove())
        {
            continue;
        }
        // Editors write several events per save; wait until it is quiet.
        while rx.recv_timeout(Duration::from_millis(150)).is_ok() {}
        println!("\n↻ Änderung erkannt");
        if let Err(err) = rebuild(&mut host) {
            eprintln!("✗ {err:#}");
        }
    }
    Ok(())
}

/// Build a plugin crate for WebAssembly and return the produced components.
fn build(dir: &Path) -> Result<Vec<PathBuf>> {
    let output = Command::new(env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .args([
            "build",
            "--release",
            "--target",
            WASM_TARGET,
            "--message-format=json-render-diagnostics",
        ])
        .current_dir(dir)
        .stderr(Stdio::inherit())
        .output()
        .context("cargo konnte nicht gestartet werden; ist Rust installiert? https://rustup.rs")?;
    if !output.status.success() {
        bail!("Bauen fehlgeschlagen (siehe Fehler oben)");
    }
    let mut files = Vec::new();
    for line in output.stdout.lines() {
        let line = line?;
        let Ok(msg) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        if msg["reason"] != "compiler-artifact" {
            continue;
        }
        if let Some(names) = msg["filenames"].as_array() {
            files.extend(
                names
                    .iter()
                    .filter_map(|f| f.as_str())
                    .filter(|f| f.ends_with(".wasm"))
                    .map(PathBuf::from),
            );
        }
    }
    if files.is_empty() {
        bail!("Keine .wasm-Datei gebaut. Steht `crate-type = [\"cdylib\"]` in der Cargo.toml?");
    }
    Ok(files)
}

fn ensure_wasm_target() -> Result<()> {
    let installed = Command::new("rustup")
        .args(["target", "list", "--installed"])
        .output()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .any(|l| l == WASM_TARGET)
        })
        .unwrap_or(true); // without rustup, let cargo report the problem
    if installed {
        return Ok(());
    }
    print!("Das Rust-Ziel `{WASM_TARGET}` fehlt. Jetzt installieren? [J/n] ");
    io::stdout().flush()?;
    let mut answer = String::new();
    io::stdin().read_line(&mut answer)?;
    if answer.trim().to_lowercase().starts_with('n') {
        bail!("Ohne `{WASM_TARGET}` können keine Plugins gebaut werden");
    }
    let status = Command::new("rustup")
        .args(["target", "add", WASM_TARGET])
        .status()?;
    if !status.success() {
        bail!("`rustup target add {WASM_TARGET}` fehlgeschlagen");
    }
    Ok(())
}

fn run_files(files: &[PathBuf]) -> Result<()> {
    let mut host = host()?;
    for file in files {
        host.load_file(file)?;
    }
    host.publish(&Started)?;
    host.publish(&demo_journey())?;
    print_events(&mut host);
    Ok(())
}

fn check(file: &Path) -> Result<()> {
    let mut host = host()?;
    let manifest = host.load_file(file)?;
    println!("✓ {} ({})", manifest.name, manifest.id);
    println!("  Version:       {}", manifest.version);
    println!("  API:           {}", manifest.api);
    let permissions = if manifest.permissions.is_empty() {
        "keine".to_owned()
    } else {
        manifest.permissions.join(", ")
    };
    println!("  Berechtigungen: {permissions}");
    Ok(())
}

fn print_events(host: &mut PluginHost) {
    for event in host.drain_events() {
        match event {
            HostEvent::Loaded(m) => println!("✓ geladen: {} {} ({})", m.name, m.version, m.id),
            HostEvent::Reloaded(m) => {
                println!("↻ neu geladen: {} {} ({})", m.name, m.version, m.id)
            }
            HostEvent::Unloaded(id) => println!("– entladen: {id}"),
            HostEvent::Log {
                plugin,
                level,
                message,
            } => {
                let tag = match level {
                    Level::Trace | Level::Debug => "debug",
                    Level::Info => "info",
                    Level::Warn => "warn",
                    Level::Error => "error",
                };
                println!("  [{plugin}] {tag}: {message}");
            }
            HostEvent::Published {
                plugin,
                topic,
                payload,
            } => {
                if topic == <locora_api::ui::Tile as locora_api::Event>::TOPIC
                    && let Ok(tile) = locora_api::decode::<locora_api::ui::Tile>(&payload)
                {
                    println!("  [{plugin}] ▣ Kachel „{}“: {}", tile.title, tile.text);
                    continue;
                }
                println!("  [{plugin}] → {topic}");
            }
            HostEvent::HandlerError {
                plugin,
                topic,
                message,
            } => {
                println!("  [{plugin}] ✗ Fehler bei {topic}: {message}");
            }
            HostEvent::Disabled { plugin, reason } => {
                println!("  [{plugin}] ✗ deaktiviert: {reason}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::plugin_id;

    #[test]
    fn ids_from_display_names() {
        assert_eq!(plugin_id("Mein Zähler"), "mein-zaehler");
        assert_eq!(plugin_id("  Große Übung!! "), "grosse-uebung");
        assert_eq!(plugin_id("3D Paläste"), "plugin-3d-palaeste");
        assert_eq!(plugin_id("!!"), "");
    }
}
