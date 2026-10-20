//! New templates compile as real components and their services are callable.
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn cli(cwd: &Path, data: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_locora"))
        .current_dir(cwd)
        .arg("--data")
        .arg(data)
        .args(args)
        .output()
        .unwrap()
}

fn ok(cwd: &Path, data: &Path, args: &[&str]) -> String {
    let output = cli(cwd, data, args);
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn component(dir: &Path, id: &str) -> PathBuf {
    let status = Command::new(env!("CARGO"))
        .args([
            "build",
            "--release",
            "--offline",
            "--target",
            "wasm32-wasip2",
        ])
        .current_dir(dir)
        .status()
        .unwrap();
    assert!(status.success());
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| dir.join("target"));
    target.join(format!("wasm32-wasip2/release/{id}.wasm"))
}

#[test]
fn command_locus_and_importer_templates_build_and_run() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let data = root.join("profile");
    let cases = [
        (
            "Befehlprobe",
            "befehl",
            "befehlprobe",
            "command",
            " Hallo ",
            "Hallo",
        ),
        (
            "Locusprobe",
            "locus-typ",
            "locusprobe",
            "locus",
            "Erde|erde",
            "richtig",
        ),
        (
            "Importprobe",
            "importer",
            "importprobe",
            "import",
            "Testpalast\nOrt | Erde",
            "1 Loci",
        ),
    ];
    for (name, template, id, kind, input, expected) in cases {
        ok(
            root,
            &data,
            &["plugin", "new", name, "--template", template],
        );
        let source = fs::read_to_string(root.join(id).join("src/lib.rs")).unwrap();
        assert!(!source.contains("{{"));
        let readme = fs::read_to_string(root.join(id).join("README.md")).unwrap();
        assert!(readme.contains(&format!("plugin invoke {id}")));
        let wasm = component(&root.join(id), id);
        ok(
            root,
            &data,
            &["plugin", "install", wasm.to_str().unwrap(), "--yes"],
        );
        let result = if kind == "import" {
            let file = root.join("input.txt");
            fs::write(&file, input).unwrap();
            ok(
                root,
                &data,
                &[
                    "plugin",
                    "invoke",
                    id,
                    kind,
                    "--file",
                    file.to_str().unwrap(),
                ],
            )
        } else {
            ok(root, &data, &["plugin", "invoke", id, kind, input])
        };
        assert!(result.contains(expected), "{result}");
    }
    let shown = ok(root, &data, &["show", "Testpalast"]);
    assert!(shown.contains("Erde"));
}

#[test]
fn generated_names_are_escaped_and_invoke_rejects_missing_provider() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let data = root.join("profile");
    ok(
        root,
        &data,
        &["plugin", "new", "Mein \"Befehl\"", "--template", "befehl"],
    );
    let source = fs::read_to_string(root.join("mein-befehl/src/lib.rs")).unwrap();
    assert!(source.contains("Mein \\\"Befehl\\\""));
    let output = cli(
        root,
        &data,
        &["plugin", "invoke", "absent", "command", "hello"],
    );
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("bietet"));
}
