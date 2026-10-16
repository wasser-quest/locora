//! End-to-end workbook editing and run-local practice/review state.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

fn run(data: &Path, args: &[&str], input: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_locora"))
        .arg("--data")
        .arg(data)
        .args(args)
        .env("LOCORA_NOW", "1800000000")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    feed(child.stdin.take().unwrap(), input.as_bytes());
    child.wait_with_output().unwrap()
}

fn ok(data: &Path, args: &[&str], input: &str) -> String {
    let out = run(data, args, input);
    assert!(
        out.status.success(),
        "{args:?}: {}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        out.stderr.is_empty(),
        "{args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

fn set(data: &Path, palace: &str, address: &str, value: &str, kind: &str) {
    ok(
        data,
        &[
            "table", "set", "--palace", palace, address, value, "-t", kind,
        ],
        "",
    );
}

fn bind(data: &Path, palace: &str, n: &str, field: &str, expression: &str) {
    ok(data, &["bind", palace, n, field, expression], "");
}

#[test]
fn tables_scopes_columns_types_and_calculation() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    ok(d, &["new", "A", "--", "Basis"], "");
    ok(
        d,
        &["table", "set", "--shared", "Major.7.object", "Kuh"],
        "",
    );
    ok(
        d,
        &["table", "set", "--shared", "Major.7.image", "Bild"],
        "",
    );
    assert_eq!(ok(d, &["calc", "Major[\"7\"].object"], ""), "Kuh\n");
    set(d, "A", "Major.7.object", "Kaffee", "text");
    set(d, "A", "Major.7.image", "Foto", "text");
    assert_eq!(ok(d, &["calc", "A", "Major[\"7\"].object"], ""), "Kaffee\n");
    let shown = ok(d, &["table", "show", "A"], "");
    assert!(
        shown.contains("[text] = Kaffee") && shown.contains("[text] = Foto"),
        "{shown}"
    );
    assert!(!shown.contains("Kuh"));
    assert!(ok(d, &["table", "show", "--shared"], "").contains("Kuh"));
    assert_eq!(
        ok(d, &["table", "show"], ""),
        ok(d, &["table", "show", "--shared"], "")
    );
    ok(d, &["table", "rm", "--palace", "A", "Major.7.image"], "");
    assert_eq!(ok(d, &["calc", "A", "Major[\"7\"].object"], ""), "Kaffee\n");
    assert!(
        !run(d, &["calc", "A", "Major[\"7\"].image"], "")
            .status
            .success()
    );
    ok(d, &["table", "rm", "--palace", "A", "Major.7"], "");
    assert_eq!(ok(d, &["calc", "A", "Major[\"7\"].image"], ""), "Bild\n");
    assert_eq!(ok(d, &["calc", "-1"], ""), "-1\n");
    assert_eq!(ok(d, &["calc", "A", "-1"], ""), "-1\n");
    set(d, "A", "Data.n", "-2", "number");
    set(d, "A", "Data.flag", "true", "boolean");
    set(d, "A", "Data.list", "[1,2]", "json");
    set(d, "A", "Data.result", "Data.n + 3", "formula");
    assert_eq!(
        ok(d, &["calc", "A", "[Data.result, Data.flag, Data.list]"], ""),
        "[1,true,[1,2]]\n"
    );
    for args in [
        vec!["table", "set", "Data.x", "v"],
        vec!["table", "set", "--shared", "--palace", "A", "Data.x", "v"],
        vec!["table", "show", "A", "--shared"],
        vec!["table", "set", "--shared", "Data.x.y.z", "v"],
        vec!["table", "set", "--shared", "Data.x", "v", "-t", "unknown"],
        vec!["table", "set", "--shared", "Data.x", "NaN", "-t", "number"],
        vec!["table", "set", "--shared", "Data.x", "yes", "-t", "boolean"],
        vec!["table", "set", "--shared", "Data.x", "{", "-t", "json"],
        vec!["table", "rm", "--shared", "Missing.x"],
        vec!["calc", "Missing", "1"],
    ] {
        assert!(!run(d, &args, "").status.success(), "{args:?}");
    }
    assert_eq!(ok(d, &["calc", "A", "Data.n"], ""), "-2\n");
}

#[test]
fn binding_replacement_unbinding_and_validation() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    ok(d, &["new", "A", "--", "Basis"], "");
    bind(d, "A", "1", "place", "\"Tür\"");
    bind(d, "A", "1", "item", "-1");
    assert!(ok(d, &["practice", "A"], "\n3\n").contains("→ -1"));
    bind(d, "A", "1", "item", "\"Alt\"");
    bind(d, "A", "1", "item", "\"Neu\"");
    let out = ok(d, &["practice", "A"], "\n3\n");
    assert!(
        out.contains("Tür") && out.contains("→ Neu") && !out.contains("→ Alt"),
        "{out}"
    );
    ok(d, &["unbind", "A", "1", "item"], "");
    let out = ok(d, &["practice", "A"], "\n3\n");
    assert!(out.contains("Tür") && out.contains("→ Basis"), "{out}");
    for args in [
        vec!["bind", "A", "0", "item", "1"],
        vec!["bind", "A", "2", "item", "1"],
        vec!["bind", "A", "1", "unknown", "1"],
        vec!["bind", "A", "1", "item", "("],
        vec!["unbind", "A", "1", "unknown"],
    ] {
        assert!(!run(d, &args, "").status.success(), "{args:?}");
    }
}

#[test]
fn inputs_active_reveal_and_new_limit_in_review() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    ok(
        d,
        &[
            "new",
            "A",
            "--",
            "Inaktiv",
            "Antwort",
            "Kaputt",
            "Verborgen",
        ],
        "",
    );
    set(d, "A", "Inputs.n", "1", "number");
    set(d, "A", "Inputs.word", "Standard", "text");
    bind(d, "A", "1", "active", "$n > 2");
    bind(d, "A", "2", "active", "View.reveal");
    bind(d, "A", "2", "visible", "false");
    bind(d, "A", "2", "place", "IF(View.reveal, \"FALSCH\", $word)");
    bind(
        d,
        "A",
        "2",
        "item",
        "IF(View.reveal, \"GEHEIM\", Missing.cell)",
    );
    bind(d, "A", "3", "active", "Missing.cell");
    bind(d, "A", "3", "item", "[1,2]");
    bind(d, "A", "4", "active", "false");
    let out = ok(
        d,
        &["review", "A", "--new", "2"],
        "falsch\n2\nMiXeD\n\n3\n\n3\n",
    );
    assert!(
        out.contains("Ungültige Eingabe") && out.contains("MiXeD"),
        "{out}"
    );
    assert!(
        out.contains("2 Loci") && out.contains("active-Formel fehlerhaft"),
        "{out}"
    );
    assert!(
        out.contains("→ GEHEIM") && out.contains("Formel für item fehlerhaft"),
        "{out}"
    );
    assert!(
        !out.contains("FALSCH") && !out.contains("→ Inaktiv") && !out.contains("→ Verborgen"),
        "{out}"
    );
    assert!(out.contains("2 von 2 geübt"), "{out}");
    // Only the two selected loci were scheduled; inactive fresh loci remain new.
    assert!(ok(d, &["due"], "").contains("2 neu"));
    assert_eq!(ok(d, &["calc", "A", "$n"], ""), "1\n");
    let out = ok(d, &["practice", "A", "--due", "--new", "0"], "\n\n");
    assert!(out.contains("Keine aktiven Loci ausgewählt"), "{out}");
    let out = ok(d, &["practice", "A"], "");
    assert!(
        out.contains("Übung beendet") && !out.contains("[1/"),
        "{out}"
    );
}

#[test]
fn actions_are_case_sensitive_run_local_and_do_not_grade() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    ok(d, &["new", "A", "--", "Erster", "Zweiter"], "");
    set(d, "A", "Inputs.word", "Standard", "text");
    set(d, "A", "RAM.n", "0", "number");
    for (key, kind) in [
        ("Next", "set"),
        ("Clear", "reset"),
        ("Show", "reveal"),
        ("Bad", "set"),
    ] {
        set(d, "A", &format!("Actions.{key}.kind"), kind, "text");
    }
    set(d, "A", "Actions.Next.target", "RAM.n", "text");
    set(d, "A", "Actions.Next.expression", "RAM.n + 1", "text");
    set(d, "A", "Actions.Bad.target", "RAM.n, Inputs.word", "text");
    set(d, "A", "Actions.Bad.expression", "[99, 123]", "text");
    bind(d, "A", "1", "place", "$word");
    bind(d, "A", "1", "item", "RAM.n");
    bind(d, "A", "2", "active", "RAM.n = 0");
    bind(d, "A", "2", "item", "RAM.n");
    let out = ok(
        d,
        &["practice", "A"],
        "MiXeD\n?\n!next\n!Next\n!Show\n!Bad\n!Next\n!Clear\n!Next\n3\n\n3\n",
    );
    assert!(
        out.contains("!Next") && out.contains("Aktion fehlgeschlagen"),
        "{out}"
    );
    assert!(
        out.contains("→ 1") && out.contains("→ 2") && out.contains("→ 0"),
        "{out}"
    );
    assert!(!out.contains("→ 99"), "{out}");
    assert!(out.contains("2 von 2 geübt"), "{out}");
    assert_eq!(ok(d, &["calc", "A", "RAM.n"], ""), "0\n");
    assert_eq!(ok(d, &["calc", "A", "$word"], ""), "Standard\n");
    let out = ok(d, &["practice", "A"], "\n!Show\nq\n");
    assert!(
        out.contains("Standard") && out.contains("→ 0") && out.contains("0 von 2 geübt"),
        "{out}"
    );
}

#[test]
fn typed_inputs_defaults_and_quitting_before_reveal() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    ok(d, &["new", "A", "--", "Basis"], "");
    set(d, "A", "Inputs.flag", "true", "boolean");
    set(d, "A", "Inputs.list", "[1,2]", "json");
    bind(d, "A", "1", "place", "Text($flag)");
    bind(d, "A", "1", "item", "Text($list)");
    // Neither commands nor quitting before reveal may schedule the locus.
    let out = ok(
        d,
        &["practice", "A"],
        "yes\nfalse\n{\n[3,4]\n?\n!Missing\nq\n",
    );
    assert!(
        out.contains("Ungültige Eingabe") && out.contains("Keine Aktionen vorhanden"),
        "{out}"
    );
    assert!(
        out.contains("false") && !out.contains("→ [3,4]") && out.contains("0 von 1 geübt"),
        "{out}"
    );
    assert!(ok(d, &["due"], "").contains("1 neu"));
    let out = ok(d, &["practice", "A"], "\n\n\n3\n");
    assert!(
        out.contains("true") && out.contains("→ [1,2]") && out.contains("1 von 1 geübt"),
        "{out}"
    );
    assert_eq!(
        ok(d, &["calc", "A", "[$flag, $list]"], ""),
        "[true,[1,2]]\n"
    );
}

#[test]
fn review_keeps_palace_runs_isolated_and_empty_routes_are_safe() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    for name in ["A", "B"] {
        ok(d, &["new", name, "--", "Basis"], "");
        set(d, name, "Inputs.word", name, "text");
        bind(d, name, "1", "item", "$word");
    }
    let out = ok(d, &["review"], "Alpha\nBeta\n\n3\n\n3\n");
    assert!(
        out.contains("→ Alpha") && out.contains("→ Beta") && out.contains("2 von 2 geübt"),
        "{out}"
    );
    for name in ["A", "B"] {
        bind(d, name, "1", "active", "false");
    }
    let out = ok(d, &["practice", "A"], "\n");
    assert!(
        out.contains("Keine aktiven Loci ausgewählt") && !out.contains("[1/"),
        "{out}"
    );
}

/// Write test input; a process that exits before reading it is fine.
fn feed(mut stdin: std::process::ChildStdin, input: &[u8]) {
    if let Err(err) = stdin.write_all(input) {
        assert_eq!(err.kind(), std::io::ErrorKind::BrokenPipe, "{err}");
    }
}
