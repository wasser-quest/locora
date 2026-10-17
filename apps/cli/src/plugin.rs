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
use locora_host::{ApprovalReceipt, HostEvent, Level, Manifest, PluginHost};
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
    /// Plugin-Datei ohne Aktivierung prüfen und Manifest anzeigen
    Check { file: PathBuf },
    /// Lokale WASM-Komponente nach Rechteprüfung installieren (kein Update)
    Install {
        file: PathBuf,
        /// Warnung und alle angezeigten Rechte ausdrücklich akzeptieren
        #[arg(long, short)]
        yes: bool,
    },
    /// Installierte Datei entfernen; Plugin-Daten bleiben erhalten
    Remove {
        /// Plugin-ID oder Dateiname aus `plugin ls` (auch defekte Dateien)
        plugin: String,
    },
    /// Eingebaute und installierte Plugins ohne Aktivierung auflisten
    Ls,
    /// Signierschlüssel für Plugins anlegen (Datei, Modus 0600)
    Keygen { path: PathBuf },
    /// Plugin-Datei signieren; schreibt `<datei>.sig` daneben
    Sign {
        file: PathBuf,
        /// Schlüsseldatei aus `plugin keygen`
        #[arg(long)]
        key: PathBuf,
    },
    /// Einem Herausgeber (öffentlicher Schlüssel) vertrauen
    Trust {
        /// Öffentlicher Schlüssel des Herausgebers (base64)
        key: String,
        #[arg(long)]
        name: String,
    },
    /// Vertrauenswürdige Herausgeber anzeigen
    Publishers,
    /// Installiertes Plugin durch eine neuere, vom selben Herausgeber signierte Version ersetzen
    Update {
        file: PathBuf,
        /// Änderungen (Rechte, Version) ausdrücklich akzeptieren
        #[arg(long, short)]
        yes: bool,
    },
}

#[derive(Clone, Copy, ValueEnum)]
pub enum Template {
    /// Reagiert auf Ereignisse (z. B. beendete Journeys)
    Ereignis,
    /// Zeigt eine eigene Kachel auf dem Startbildschirm
    Kachel,
}

pub fn run(cmd: PluginCommand, data: Option<&Path>) -> Result<()> {
    match cmd {
        PluginCommand::New { name, template } => new(&name, template),
        PluginCommand::Dev { dir, demo, once } => dev(&dir, demo, once),
        PluginCommand::Run { files } => run_files(&files),
        PluginCommand::Check { file } => check(&file),
        PluginCommand::Install { file, yes } => install(&file, &crate::app::data_dir(data)?, yes),
        PluginCommand::Remove { plugin } => remove(&plugin, &crate::app::data_dir(data)?),
        PluginCommand::Ls => list(&crate::app::data_dir(data)?),
        PluginCommand::Keygen { path } => plugin_sign::keygen(&path),
        PluginCommand::Sign { file, key } => plugin_sign::sign(&file, &key),
        PluginCommand::Trust { key, name } => {
            plugin_sign::trust(&crate::app::data_dir(data)?, &key, &name)
        }
        PluginCommand::Publishers => plugin_sign::list_publishers(&crate::app::data_dir(data)?),
        PluginCommand::Update { file, yes } => {
            plugin_sign::update(&file, &crate::app::data_dir(data)?, yes)
        }
    }
}

fn host() -> Result<PluginHost> {
    // Development and check commands must never mutate the user's profile.
    PluginHost::with_palaces(
        dirs::cache_dir().map(|d| d.join("locora").join("plugins")),
        std::sync::Arc::new(locora_host::MemoryKv::default()),
        std::sync::Arc::new(locora_store::Store::in_memory()?),
    )
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
    let host = PluginHost::new(None)?;
    let manifest = host.inspect_bytes(&read_regular(file)?)?;
    validate_install(&manifest)?;
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

// These identities remain reserved even in builds without embedded components.
pub(crate) const BUILT_INS: &[&str] = &["core", "review", "notify", "tutorial", "gamification"];

pub(crate) fn validate_install(manifest: &Manifest) -> Result<()> {
    let id = &manifest.id;
    let device_name = matches!(id.as_str(), "con" | "prn" | "aux" | "nul")
        || (id.len() == 4
            && (id.starts_with("com") || id.starts_with("lpt"))
            && matches!(id.as_bytes()[3], b'1'..=b'9'));
    if device_name
        || id.is_empty()
        || id.len() > 128
        || !id.as_bytes()[0].is_ascii_alphanumeric()
        || !id
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-' || c == b'_')
    {
        bail!(
            "Unsichere Plugin-ID {id:?}: erwartet 1–128 Zeichen a-z, 0-9, - oder _, beginnend mit a-z/0-9"
        );
    }
    for permission in &manifest.permissions {
        if !matches!(
            permission.as_str(),
            "storage" | "palaces:read" | "palaces:write" | "ui:highlight"
        ) {
            bail!(
                "Nicht unterstützte Berechtigung {permission:?}; unterstützt: storage, palaces:read, palaces:write, ui:highlight"
            );
        }
    }
    Ok(())
}

fn installed_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(e).with_context(|| format!("Plugin-Ordner {} lesen", dir.display())),
    };
    let mut files = Vec::new();
    for entry in entries {
        let path = entry?.path();
        if path.extension().is_some_and(|x| x == "wasm") {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

use crate::plugin_sign;
use locora_runtime::read_regular;

fn inspect_file(host: &PluginHost, file: &Path) -> Result<Manifest> {
    host.inspect_bytes(&read_regular(file)?)
}

fn install(file: &Path, data: &Path, yes: bool) -> Result<()> {
    // Read once: the exact reviewed bytes are persisted, not a later source copy.
    let bytes = read_regular(file).with_context(|| format!("Plugin {} lesen", file.display()))?;
    let host = PluginHost::new(None)?;
    let manifest = host.inspect_bytes(&bytes)?;
    validate_install(&manifest)?;
    if BUILT_INS.contains(&manifest.id.as_str()) {
        bail!("Eingebautes Plugin {:?} ist geschützt", manifest.id);
    }
    let dir = crate::app::plugin_dir(data)?;
    let destination = dir.join(format!("{}.wasm", manifest.id));
    let approval = destination.with_extension("wasm.json");
    for path in [&destination, &approval] {
        if fs::symlink_metadata(path).is_ok() {
            bail!(
                "Zieldatei {} existiert bereits; kein Überschreiben (ggf. zuerst plugin remove)",
                path.display()
            );
        }
    }
    for installed in installed_files(&dir)? {
        if let Ok(other) = inspect_file(&host, &installed)
            && other.id == manifest.id
        {
            bail!(
                "Plugin-ID {:?} ist bereits installiert ({})",
                manifest.id,
                installed.display()
            );
        }
    }
    let signer = plugin_sign::signer_of(file, &bytes)?;
    match &signer {
        Some(key) => match locora_runtime::publishers::trusted_name(data, key)? {
            Some(name) => eprintln!("✓ Signiert von {name} (vertrauenswürdig)."),
            None => eprintln!(
                "⚠ Signiert von einem unbekannten Herausgeber: {key}\n  Vertrauen: locora plugin trust {key} --name <NAME>"
            ),
        },
        None => eprintln!("⚠ Nicht signiert."),
    }
    eprintln!("⚠ Nicht von Locora geprüft – Installation auf eigene Verantwortung.");
    eprintln!(
        "  Plugin: {:?} ({:?}), Version {:?}, API {:?}",
        manifest.name, manifest.id, manifest.version, manifest.api
    );
    eprintln!(
        "  Rechte: {}",
        if manifest.permissions.is_empty() {
            "keine".into()
        } else {
            manifest.permissions.join(", ")
        }
    );
    plugin_sign::consent(yes, "Trotzdem installieren?")?;
    fs::create_dir_all(&dir)?;
    // Publish approval first and the component last. A crash can leave an
    // orphan approval, but never a loadable component without consent.
    let mut receipt = ApprovalReceipt::new(manifest.clone(), &bytes);
    if let Some(key) = signer {
        receipt = receipt.signed_by(key);
    }
    persist_new(&approval, &serde_json::to_vec(&receipt)?)?;
    if let Err(err) = persist_new(&destination, &bytes) {
        let _ = fs::remove_file(&approval);
        return Err(err);
    }
    println!(
        "✓ {:?} installiert; wird beim nächsten Profilbefehl aktiviert.",
        manifest.id
    );
    Ok(())
}

fn persist_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut temp = tempfile::NamedTempFile::new_in(path.parent().context("Kein Plugin-Ordner")?)?;
    temp.write_all(bytes)?;
    temp.as_file().sync_all()?;
    temp.persist_noclobber(path)
        .map_err(|e| e.error)
        .with_context(|| format!("{} ohne Überschreiben speichern", path.display()))?;
    Ok(())
}

fn remove(plugin: &str, data: &Path) -> Result<()> {
    // Explicit filenames require no guest execution. Bare IDs are resolved
    // through isolated metadata probes, never activation or profile backends.
    if plugin.is_empty() || plugin.contains(['/', '\\', ':']) || plugin == "." || plugin == ".." {
        bail!("Erwartet Plugin-ID oder einzelnen .wasm-Dateinamen, keinen Pfad");
    }
    let id = plugin.strip_suffix(".wasm").unwrap_or(plugin);
    let dir = crate::app::plugin_dir(data)?;
    let path = if plugin.ends_with(".wasm") {
        dir.join(plugin)
    } else {
        let host = PluginHost::new(None)?;
        let mut matches = Vec::new();
        let canonical = dir.join(format!("{plugin}.wasm"));
        let mut canonical_has_other_id = false;
        for file in installed_files(&dir)? {
            if let Ok(manifest) = inspect_file(&host, &file) {
                if manifest.id == plugin {
                    matches.push(file);
                } else if file == canonical {
                    canonical_has_other_id = true;
                }
            }
        }
        match matches.len() {
            0 if canonical_has_other_id => bail!(
                "Dateiname {} gehört zu einer anderen Plugin-ID; zum Entfernen ausdrücklich den .wasm-Dateinamen angeben",
                canonical.display()
            ),
            0 => canonical, // Broken components and orphan approvals remain removable.
            1 => matches.pop().unwrap(),
            _ => bail!(
                "Mehrere Dateien haben Plugin-ID {plugin:?}; bitte genauen .wasm-Dateinamen angeben"
            ),
        }
    };
    let approval = path.with_extension("wasm.json");
    let has_file = fs::symlink_metadata(&path).is_ok();
    let has_approval = fs::symlink_metadata(&approval).is_ok();
    if !has_file && !has_approval && BUILT_INS.contains(&id) {
        bail!("Eingebautes Plugin {id:?} ist geschützt");
    }
    if has_file || !has_approval {
        fs::remove_file(&path)
            .with_context(|| format!("Plugin-Datei {} entfernen", path.display()))?;
    }
    if has_approval {
        fs::remove_file(&approval).context("Installationszustimmung entfernen")?;
    }
    println!("✓ {plugin:?} entfernt; gespeicherte Plugin-Daten bleiben erhalten.");
    Ok(())
}

fn list(data: &Path) -> Result<()> {
    let dir = crate::app::plugin_dir(data)?;
    let host = PluginHost::new(None)?;
    for id in BUILT_INS {
        println!(
            "{id} [eingebaut, geschützt{}]",
            if crate::app::embedded::EMBEDDED
                .iter()
                .any(|(name, _)| name == id)
            {
                ""
            } else {
                ", in diesem Build nicht enthalten"
            }
        );
    }
    for file in installed_files(&dir)? {
        let name = file.file_name().unwrap_or_default();
        match inspect_file(&host, &file) {
            Ok(manifest) => {
                println!(
                    "{name:?}: {:?} {:?}, API {:?}, Rechte: {:?} [installiert, nicht aktiviert]",
                    manifest.id, manifest.version, manifest.api, manifest.permissions
                );
                if let Err(err) = validate_install(&manifest) {
                    println!("  ⚠ {err:#}");
                }
                let approved = read_regular(&file.with_extension("wasm.json"))
                    .ok()
                    .and_then(|bytes| serde_json::from_slice::<ApprovalReceipt>(&bytes).ok());
                let component = read_regular(&file).ok();
                if !matches!((&approved, &component), (Some(receipt), Some(bytes)) if receipt.matches(&manifest, bytes))
                {
                    println!(
                        "  ⚠ Keine passende Installationszustimmung für diese Plugin-Datei; wird nicht aktiviert"
                    );
                }
                if BUILT_INS.contains(&manifest.id.as_str()) {
                    println!("  ⚠ Eingebaute ID geschützt; wird nicht ersetzt");
                }
            }
            Err(err) => println!("{name:?}: [defekt/nicht lesbar] {err:#}"),
        }
    }
    Ok(())
}

fn print_events(host: &mut PluginHost) {
    for event in host.drain_events() {
        match event {
            HostEvent::Highlighted { plugin, step } => {
                println!(
                    "[{plugin}] App-Tour: {} ({}/{})",
                    step.title, step.step, step.total
                );
            }
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
    fn development_hosts_have_separate_ephemeral_palaces() {
        use locora_api::palaces::{Palaces, Request, Response};
        let mut first = super::host().unwrap();
        let mut second = super::host().unwrap();
        first
            .call::<Palaces>(&Request::Create {
                name: "Test".into(),
            })
            .unwrap();
        assert!(
            matches!(first.call::<Palaces>(&Request::List).unwrap(), Response::List(v) if v.len() == 1)
        );
        assert_eq!(
            second.call::<Palaces>(&Request::List).unwrap(),
            Response::List(vec![])
        );
    }

    #[test]
    fn install_ids_and_permissions_are_strict() {
        let mut manifest = locora_host::Manifest {
            id: "mein-plugin_2".into(),
            name: "Test".into(),
            version: "0.1.0".into(),
            api: locora_api::API_VERSION.into(),
            permissions: vec![],
        };
        for permission in ["storage", "palaces:read", "palaces:write"] {
            manifest.permissions = vec![permission.into()];
            super::validate_install(&manifest).unwrap();
        }
        for permission in ["notify", "network", "palaces_read", "", "future:right"] {
            manifest.permissions = vec![permission.into()];
            assert!(super::validate_install(&manifest).is_err());
        }
        manifest.permissions.clear();
        for id in [
            "", ".", "..", "../bad", "a/b", "a\\b", "a:b", "Bad", "übel", "-bad", "con", "nul",
            "prn", "aux", "com1", "com9", "lpt1", "lpt9",
        ] {
            manifest.id = id.into();
            assert!(super::validate_install(&manifest).is_err(), "{id}");
        }
        manifest.id = "a".repeat(129);
        assert!(super::validate_install(&manifest).is_err());
    }

    #[test]
    fn atomic_publish_never_clobbers_and_cleans_temporary_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("plugin.wasm");
        super::persist_new(&path, b"approved").unwrap();
        assert!(super::persist_new(&path, b"replacement").is_err());
        assert_eq!(std::fs::read(path).unwrap(), b"approved");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn ids_from_display_names() {
        assert_eq!(plugin_id("Mein Zähler"), "mein-zaehler");
        assert_eq!(plugin_id("  Große Übung!! "), "grosse-uebung");
        assert_eq!(plugin_id("3D Paläste"), "plugin-3d-palaeste");
        assert_eq!(plugin_id("!!"), "");
    }
}
