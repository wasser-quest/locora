use super::*;

const FULL: &str = r#"
name = "Planeten"

[[room]]
name = "Wohnung"
image = "wohnung.png"
items = ["Merkur", "Venus", "Erde"]

[[room]]
name = "Garten"

[[room.locus]]
place = "Baum"
item = "Mars"
at = [0.4, 0.6]
answer = { type = "guess", accept = ["Roter Planet"] }
note = "Ein Marsriegel hängt im Baum"

[[room.locus]]
item = "Jupiter"
answer = { type = "quiz", correct = ["Jupiter"], wrong = ["Saturn", "Venus"] }
bindings = { item = 'Major["4"]', active = "true" }

[[room.locus]]
item = "Uranus"
answer = { type = "quiz", options = [{ text = "Nein", correct = false }, { text = "Uranus", correct = true }] }

[[room]]
name = "Vortrag"
video = "talk.mp4"

[[room.locus]]
item = "These"
time = "1:23"

[[room.locus]]
item = "Fazit"
time = 250.5

[[table]]
table = "Major"
key = "4"
value = "Reh"

[[table]]
table = "Inputs"
key = "n"
value = "3"
type = "number"

[[table]]
table = "Data"
key = "p"
cells = [{ column = "name", type = "text", value = "Ada" }, { column = "age", type = "number", value = "36" }]
"#;

fn files(dir: &Path) {
    fs::write(dir.join("wohnung.png"), b"\x89PNG fake").unwrap();
    fs::write(dir.join("talk.mp4"), b"fake mp4").unwrap();
}

fn plan_of(text: &str, dir: &Path) -> Result<Plan> {
    check(&parse(text)?, dir)
}

fn err_of(text: &str, dir: &Path) -> String {
    match plan_of(text, dir) {
        Ok(_) => panic!("unexpectedly valid"),
        Err(e) => e.to_string(),
    }
}

#[test]
fn builds_all_forms_into_the_store() {
    let dir = tempfile::tempdir().unwrap();
    files(dir.path());
    let plan = plan_of(FULL, dir.path()).unwrap();
    assert_eq!(plan.loci(), 8);
    let store = Store::open(&dir.path().join("profile")).unwrap();
    let tree = build(&store, &plan).unwrap();
    assert_eq!(tree.rooms.len(), 3);
    assert_eq!(tree.rooms[0].room.kind, RoomKind::Image);
    assert_eq!(tree.rooms[2].room.kind, RoomKind::Video);
    let route: Vec<_> = tree
        .route()
        .map(|l| (l.place.as_str(), l.item.as_str()))
        .collect();
    assert_eq!(route[0], ("Ort 1", "Merkur"));
    assert_eq!(route[3], ("Baum", "Mars"));
    assert_eq!(
        route[4],
        ("Ort 5", "Jupiter"),
        "default places count across the palace"
    );
    let locus = |i: usize| tree.route().nth(i).unwrap().clone();
    assert_eq!(locus(3).answer.kind, AnswerKind::Guess);
    assert_eq!((locus(3).x, locus(3).y), (0.4, 0.6));
    let quiz = locus(4).answer;
    assert_eq!(
        quiz.options
            .iter()
            .map(|o| (o.text.as_str(), o.correct))
            .collect::<Vec<_>>(),
        [("Jupiter", true), ("Saturn", false), ("Venus", false)]
    );
    assert_eq!(locus(4).bindings.len(), 2);
    assert_eq!(locus(6).time, Some(83.0));
    assert_eq!(locus(7).time, Some(250.5));
    assert_eq!(tree.tables.len(), 3);
    assert_eq!(tree.tables[1].cells[0].kind, CellType::Number);

    // Names stay unique.
    let again = build(&store, &plan).unwrap_err().to_string();
    assert!(again.contains("schon einen Palast"), "{again}");
}

#[test]
fn dump_and_rebuild_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    files(dir.path());
    let store = Store::open(&dir.path().join("a")).unwrap();
    let original = build(&store, &plan_of(FULL, dir.path()).unwrap()).unwrap();

    let out = dir.path().join("out");
    let file = dump(&store, "Planeten", &out).unwrap();
    assert!(file.ends_with("planeten.locora.toml"));
    assert!(out.join("wohnung.png").is_file() && out.join("vortrag.mp4").is_file());
    let text = fs::read_to_string(&file).unwrap();
    assert!(
        text.contains("time = \"1:23\"") && text.contains("time = 250.5"),
        "{text}"
    );
    // Dumping twice never overwrites.
    assert!(
        dump(&store, "Planeten", &out)
            .unwrap_err()
            .to_string()
            .contains("gibt es schon")
    );

    let other = Store::open(&dir.path().join("b")).unwrap();
    let copy = build(&other, &check(&parse(&text).unwrap(), &out).unwrap()).unwrap();
    let view = |t: &PalaceTree| {
        t.route()
            .map(|l| {
                (
                    l.place.clone(),
                    l.item.clone(),
                    l.x,
                    l.y,
                    l.time,
                    l.answer.clone(),
                    l.bindings.clone(),
                    l.note.clone(),
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(view(&copy), view(&original));
    let baum = original.route().find(|l| l.place == "Baum").unwrap();
    assert_eq!(baum.answer.accept, ["Roter Planet"]);
    assert_eq!(baum.note, "Ein Marsriegel hängt im Baum");
    assert_eq!(copy.tables, original.tables);
    let rooms = |t: &PalaceTree| {
        t.rooms
            .iter()
            .map(|r| (r.room.name.clone(), r.room.kind, r.room.media.clone()))
            .collect::<Vec<_>>()
    };
    assert_eq!(rooms(&copy), rooms(&original));
}

#[test]
fn mistakes_are_reported_with_their_place() {
    let dir = tempfile::tempdir().unwrap();
    files(dir.path());
    let cases = [
        (
            "name = \" \"\n[[room]]\nname = \"a\"",
            "`name` darf nicht leer sein",
        ),
        ("name = \"P\"", "mindestens einen"),
        (
            "name = \"P\"\nsurprise = 1\n[[room]]\nname = \"a\"",
            "unknown field",
        ),
        (
            "name = \"P\"\n[[room]]\nname = \"a\"\nimage = \"wohnung.png\"\nvideo = \"talk.mp4\"",
            "entweder `image` oder `video`",
        ),
        (
            "name = \"P\"\n[[room]]\nname = \"a\"\nimage = \"fehlt.png\"",
            "fehlt",
        ),
        (
            "name = \"P\"\n[[room]]\nname = \"a\"\nimage = \"talk.mp4\"",
            "kein Bild",
        ),
        (
            "name = \"P\"\n[[room]]\nname = \"a\"\nitems = [\"x\"]\n[[room.locus]]\nitem = \"y\"",
            "entweder `items`",
        ),
        (
            "name = \"P\"\n[[room]]\nname = \"a\"\n[[room.locus]]\nitem = \"y\"\nat = [1.5, 0]",
            "`at`",
        ),
        (
            "name = \"P\"\n[[room]]\nname = \"a\"\n[[room.locus]]\nitem = \"y\"\ntime = 3",
            "nur in Video-Räumen",
        ),
        (
            "name = \"P\"\n[[room]]\nname = \"a\"\nvideo = \"talk.mp4\"\n[[room.locus]]\nitem = \"y\"",
            "braucht jeder Locus `time`",
        ),
        (
            "name = \"P\"\n[[room]]\nname = \"a\"\nvideo = \"talk.mp4\"\n[[room.locus]]\nitem = \"y\"\ntime = \"1:x\"",
            "Zeit „1:x“",
        ),
        (
            "name = \"P\"\n[[room]]\nname = \"a\"\n[[room.locus]]\nitem = \"y\"\nanswer = \"essay\"",
            "unbekannte Antwortart",
        ),
        (
            "name = \"P\"\n[[room]]\nname = \"a\"\n[[room.locus]]\nitem = \"y\"\nanswer = { type = \"quiz\", correct = [\"a\"] }",
            "Quiz braucht",
        ),
        (
            "name = \"P\"\n[[room]]\nname = \"a\"\n[[room.locus]]\nitem = \"y\"\nbindings = { colour = \"1\" }",
            "Unknown binding field",
        ),
        (
            "name = \"P\"\n[[room]]\nname = \"a\"\n[[room.locus]]\nitem = \"y\"\nbindings = { item = \"1 +\" }",
            "Formel für item",
        ),
        (
            "name = \"P\"\n[[room]]\nname = \"a\"\n[[table]]\ntable = \"T\"\nkey = \"k\"\nvalue = \"1\"\n[[table]]\ntable = \"T\"\nkey = \"k\"\nvalue = \"2\"",
            "doppelt",
        ),
        (
            "name = \"P\"\n[[room]]\nname = \"a\"\n[[table]]\ntable = \"T\"\nkey = \"k\"\nvalue = \"x\"\ntype = \"number\"",
            "Tabelle T.k",
        ),
    ];
    for (text, expected) in cases {
        let err = err_of(text, dir.path());
        assert!(err.contains(expected), "{text}\n→ {err}");
    }
    let err = err_of(
        "name = \"P\"\n[[room]]\nname = \"Küche\"\n[[room.locus]]\nitem = \"\"",
        dir.path(),
    );
    assert!(err.contains("Raum 1 („Küche“), Locus 1"), "{err}");
}

#[test]
fn times_parse() {
    assert_eq!(parse_time("83"), Some(83.0));
    assert_eq!(parse_time("1:23"), Some(83.0));
    assert_eq!(parse_time("1:00:05"), Some(3605.0));
    assert_eq!(parse_time("a:1"), None);
    assert_eq!(parse_time("1::2"), None);
    assert!(matches!(clock(83.0), TimeSource::Clock(ref c) if c == "1:23"));
    assert!(matches!(clock(3605.0), TimeSource::Clock(ref c) if c == "1:00:05"));
}

#[test]
fn splinely_pictures_and_backdrops() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    fs::write(d.join("szene.sply"), b"\x89SPLY\r\n\x1a scene").unwrap();
    fs::write(d.join("simo.sply"), b"\x89SPLY\r\n\x1a simo").unwrap();
    fs::write(d.join("falsch.sply"), b"<svg/>").unwrap();
    fs::write(d.join("talk.mp4"), b"fake mp4").unwrap();
    let text = r#"
name = "Tutorial"
[[room]]
name = "Eingang"
image = "szene.sply"
[[room.locus]]
place = "Tor"
item = "Ein Palast ist eine Route"
image = "simo.sply"
"#;
    let plan = plan_of(text, d).unwrap();
    let store = Store::open(&d.join("profil")).unwrap();
    let tree = build(&store, &plan).unwrap();
    assert_eq!(tree.rooms[0].room.kind, RoomKind::Image);
    let picture = tree.route().next().unwrap().image.clone().unwrap();
    assert_eq!(
        store.media(&picture).unwrap().unwrap().mime,
        locora_store::SPLINELY
    );

    let out = d.join("out");
    let file = dump(&store, "Tutorial", &out).unwrap();
    let dumped = fs::read_to_string(&file).unwrap();
    assert!(
        dumped.contains("image = \"tor.sply\"") && dumped.contains("image = \"eingang.sply\""),
        "{dumped}"
    );
    assert_eq!(
        fs::read(out.join("tor.sply")).unwrap(),
        b"\x89SPLY\r\n\x1a simo"
    );

    let err = err_of(&text.replace("simo.sply", "falsch.sply"), d);
    assert!(err.contains("keine binäre .sply-Datei"), "{err}");
    let err = err_of(
        &text.replace("image = \"simo.sply\"", "image = \"talk.mp4\""),
        d,
    );
    assert!(err.contains("kann kein Video sein"), "{err}");
}

#[test]
fn custom_marker_in_source() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    fs::write(d.join("stern.sply"), b"\x89SPLY\r\n\x1a star").unwrap();
    fs::write(d.join("stern.png"), b"\x89PNG").unwrap();
    let text = "name = \"P\"\nmarker = \"stern.sply\"\n[[room]]\nname = \"a\"\nitems = [\"x\"]\n";
    let store = Store::open(&d.join("profil")).unwrap();
    let tree = build(&store, &plan_of(text, d).unwrap()).unwrap();
    assert!(tree.palace.marker.is_some());
    let file = dump(&store, "P", &d.join("out")).unwrap();
    assert!(
        fs::read_to_string(file)
            .unwrap()
            .contains("marker = \"marker.sply\"")
    );
    let err = err_of(&text.replace("stern.sply", "stern.png"), d);
    assert!(err.contains("muss eine .sply-Datei sein"), "{err}");
}

#[test]
fn guess_tables_only_take_accept() {
    let dir = tempfile::tempdir().unwrap();
    let base = "name = \"P\"\n[[room]]\nname = \"a\"\n[[room.locus]]\nitem = \"x\"\n";
    for (answer, error) in [
        (r#"{ type = "guess", correct = ["x"] }"#, "nur `accept`"),
        (
            r#"{ type = "quiz", correct = ["x"], wrong = ["y"], accept = ["z"] }"#,
            "nur bei Guess",
        ),
        (r#"{ type = "guess", accept = ["X!", "x"] }"#, "eindeutig"),
    ] {
        let text = format!("{base}answer = {answer}\n");
        assert!(err_of(&text, dir.path()).contains(error), "{answer}");
    }
}

#[test]
fn image_size_validates_before_build_and_survives_dump() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    let text = "name = \"P\"\n[[room]]\nname = \"R\"\n[[room.locus]]\nitem = \"X\"\n";
    let store = Store::open(&d.join("profil")).unwrap();
    let tree = build(&store, &plan_of(text, d).unwrap()).unwrap();
    assert_eq!(tree.route().next().unwrap().image_size, 0.25);
    for value in ["0.0", "-0.1", "0.049", "1.01", "nan", "inf", "-inf"] {
        let err = err_of(&format!("{text}image_size = {value}\n"), d);
        assert!(err.contains("Bildgröße"), "{err}");
    }
    store.delete_palace(&tree.palace.id).unwrap();
    let custom = format!("{text}image_size = 0.4\n");
    let tree = build(&store, &plan_of(&custom, d).unwrap()).unwrap();
    assert_eq!(tree.route().next().unwrap().image_size, 0.4);
    let file = dump(&store, "P", &d.join("out")).unwrap();
    let dumped = fs::read_to_string(&file).unwrap();
    assert!(dumped.contains("image_size = 0.4"), "{dumped}");
    store.delete_palace(&tree.palace.id).unwrap();
    let tree = build(&store, &plan_of(&dumped, file.parent().unwrap()).unwrap()).unwrap();
    assert_eq!(tree.route().next().unwrap().image_size, 0.4);
}

#[test]
fn zoom_validates_and_survives_dump() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    let text = "name = \"P\"\n[[room]]\nname = \"R\"\n[[room.locus]]\nitem = \"X\"\n";
    for value in ["0.5", "4.5", "nan"] {
        let err = err_of(&format!("{text}zoom = {value}\n"), d);
        assert!(err.contains("Zoom"), "{err}");
    }
    let store = Store::open(&d.join("profil")).unwrap();
    let tree = build(&store, &plan_of(&format!("{text}zoom = 2\n"), d).unwrap()).unwrap();
    assert_eq!(tree.route().next().unwrap().zoom, 2.0);
    let file = dump(&store, "P", &d.join("out")).unwrap();
    let dumped = fs::read_to_string(&file).unwrap();
    assert!(dumped.contains("zoom = 2"), "{dumped}");
    store.delete_palace(&tree.palace.id).unwrap();
    let tree = build(&store, &plan_of(&dumped, file.parent().unwrap()).unwrap()).unwrap();
    assert_eq!(tree.route().next().unwrap().zoom, 2.0);
    let whole = build(
        &store,
        &plan_of(&text.replace("\"P\"", "\"Q\""), d).unwrap(),
    )
    .unwrap();
    assert_eq!(whole.route().next().unwrap().zoom, 1.0);
    let plain = fs::read_to_string(dump(&store, "Q", &d.join("out2")).unwrap()).unwrap();
    assert!(
        !plain.contains("zoom"),
        "the default is not written: {plain}"
    );
}
