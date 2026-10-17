//! Compiles the official plugins to WebAssembly and embeds them in
//! locora-runtime, so the CLI and the app work out of the box. If the `wasm32-wasip2` target is missing
//! it still builds, just without the built-in plugins (with a warning).
//! Set `LOCORA_SKIP_PLUGINS=1` to skip this step.

use std::path::PathBuf;
use std::process::Command;
use std::{env, fs};

const OFFICIAL: &[&str] = &["core", "notify", "review"];

fn main() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let root = manifest.join("../..").canonicalize().unwrap();
    let plugins = root.join("plugins");
    let out = PathBuf::from(env::var("OUT_DIR").unwrap()).join("embedded_plugins.rs");
    println!("cargo:rerun-if-env-changed=LOCORA_SKIP_PLUGINS");
    for dir in [
        "plugins/Cargo.toml",
        "crates/locora-sdk/src",
        "crates/locora-sdk/wit",
        "crates/locora-sdk-macros/src",
        "crates/locora-api/src",
        "crates/locora-fsrs/src",
    ] {
        println!("cargo:rerun-if-changed={}", root.join(dir).display());
    }
    for name in OFFICIAL {
        println!(
            "cargo:rerun-if-changed={}",
            plugins.join(name).join("src").display()
        );
    }

    let mut embedded = Vec::new();
    if env::var_os("LOCORA_SKIP_PLUGINS").is_none() {
        let cargo = env::var("CARGO").unwrap_or_else(|_| "cargo".into());
        let mut cmd = Command::new(cargo);
        cmd.current_dir(&plugins)
            .args(["build", "--release", "--target", "wasm32-wasip2"]);
        for name in OFFICIAL {
            cmd.args(["-p", name]);
        }
        // Settings of the outer build must not leak into the plugin build.
        for var in [
            "CARGO_TARGET_DIR",
            "CARGO_BUILD_TARGET",
            "CARGO_ENCODED_RUSTFLAGS",
            "RUSTFLAGS",
            "CARGO_MAKEFLAGS",
            "CARGO_PROFILE_RELEASE_LTO",
        ] {
            cmd.env_remove(var);
        }
        match cmd.status() {
            Ok(status) if status.success() => {
                for name in OFFICIAL {
                    let wasm = plugins
                        .join("target/wasm32-wasip2/release")
                        .join(format!("{name}.wasm"));
                    if wasm.exists() {
                        embedded.push((name.to_string(), wasm));
                    }
                }
            }
            _ => println!(
                "cargo:warning=Offizielle Plugins konnten nicht gebaut werden (fehlt `rustup target add wasm32-wasip2`?). Locora läuft ohne sie."
            ),
        }
    }
    let mut code = String::from(
        "/// Official plugins compiled into this binary.\npub const EMBEDDED: &[(&str, &[u8])] = &[\n",
    );
    for (name, path) in embedded {
        code.push_str(&format!(
            "    ({name:?}, include_bytes!({:?})),\n",
            path.display().to_string()
        ));
    }
    code.push_str("];\n");
    fs::write(out, code).unwrap();
}
