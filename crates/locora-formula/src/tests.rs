//! Ported from mnemodim's `formulas.spec.ts`, plus Rust-specific checks.
use super::*;

fn row(table: &str, key: &str, value: &str, kind: CellType) -> Row {
    Row::typed(table, key, kind, value)
}

fn cell(column: &str, kind: CellType, value: &str) -> Cell {
    Cell {
        column: column.into(),
        kind,
        value: value.into(),
    }
}

fn base() -> Vec<Row> {
    vec![
        row("Inputs", "input1", "487", CellType::Text),
        row("RAM", "carry", "0", CellType::Number),
        Row {
            table: "Major".into(),
            key: "48".into(),
            cells: vec![
                cell("object", CellType::Text, "Reif"),
                cell("image", CellType::Asset, "reif.png"),
            ],
        },
        Row {
            table: "Major".into(),
            key: "7".into(),
            cells: vec![cell("object", CellType::Text, "Kuh")],
        },
        Row {
            table: "Major".into(),
            key: "00".into(),
            cells: vec![cell("object", CellType::Text, "Sauce")],
        },
        Row {
            table: "Functions".into(),
            key: "T".into(),
            cells: vec![
                cell("params", CellType::Text, "d1,d2,c"),
                cell(
                    "body",
                    CellType::Formula,
                    "[(d1+d2+c) mod 10, floor((d1+d2+c)/10)]",
                ),
            ],
        },
    ]
}

fn eval(rows: &[Row], source: &str) -> Result<Value> {
    Engine::new(rows)?.evaluate(source)
}

#[test]
fn evaluates_expressions() {
    let cases: Vec<(&str, Value)> = vec![
        ("487+256", 743.0.into()),
        ("floor(13/10)", 1.0.into()),
        ("13 mod 10", 3.0.into()),
        ("2+3*4", 14.0.into()),
        ("2^3^2", 512.0.into()),
        ("-2^2", (-4.0).into()),
        ("$input1[0]", "4".into()),
        ("$input1[2]", "7".into()),
        ("Integer($input1)+1", 488.0.into()),
        ("Reverse($input1)", "784".into()),
        ("-1 mod 10", 9.0.into()),
        ("false and Missing.cell", false.into()),
        ("true or Missing.cell", true.into()),
        ("IF(false, Missing.cell, \"ok\")", "ok".into()),
        ("WENN(1=1; \"yes\"; \"no\")", "yes".into()),
        ("Major(2,$input1)[0].object", "Reif".into()),
        ("Major(2,$input1)[1].zahl", "7".into()),
        ("Major(2,\"0048\")[0].number", "00".into()),
        ("T(7,6,0)[0]", 3.0.into()),
        ("T(8,5,1)", vec![4.0, 1.0].into()),
        ("T(4,2,1)", vec![7.0, 0.0].into()),
    ];
    for (source, expected) in cases {
        assert_eq!(
            eval(&base(), source).unwrap_or_else(|e| panic!("{source}: {e}")),
            expected,
            "{source}"
        );
    }
}

#[test]
fn user_function_handles_all_200_column_additions() {
    let mut engine = Engine::new(&base()).unwrap();
    for a in 0..=9 {
        for b in 0..=9 {
            for carry in 0..=1 {
                let expected = Value::from(vec![
                    ((a + b + carry) % 10) as f64,
                    ((a + b + carry) / 10) as f64,
                ]);
                assert_eq!(
                    engine.evaluate(&format!("T({a},{b},{carry})")).unwrap(),
                    expected
                );
            }
        }
    }
}

#[test]
fn chained_cells_and_overrides_do_not_mutate_rows() {
    let mut rows = base();
    rows.push(row("Values", "next", "RAM.carry + 1", CellType::Formula));
    let overrides = HashMap::from([("RAM.carry.value".to_owned(), Value::Number(5.0))]);
    assert_eq!(
        Engine::with_overrides(&rows, overrides)
            .unwrap()
            .evaluate("Values.next")
            .unwrap(),
        6.0.into()
    );
    assert_eq!(eval(&rows, "Values.next").unwrap(), 1.0.into());
}

#[test]
fn rejects_invalid_or_unsafe_input() {
    for source in [
        "eval(\"1\")",
        "window.location",
        "Math.random()",
        "RAM.constructor",
        "$input1[99]",
        "1/0",
        "1e999",
        "9^999",
        "Major(2,\"49\")",
        "Major(0,\"487\")",
        "IF(1,2,3)",
        "\"1\" + 2",
        "Number(\"\")",
    ] {
        assert!(eval(&base(), source).is_err(), "{source} should fail");
    }
}

#[test]
fn structured_data_and_immutable_list_operations() {
    let rows = vec![row("RAM", "items", "[\"Buch\",\"Stift\"]", CellType::Json)];
    let mut engine = Engine::new(&rows).unwrap();
    assert_eq!(
        engine.evaluate("Concat(RAM.items,[\"Pinsel\"])").unwrap(),
        vec!["Buch", "Stift", "Pinsel"].into()
    );
    assert_eq!(
        engine.evaluate("SetAt(RAM.items,0,\"Heft\")").unwrap(),
        vec!["Heft", "Stift"].into()
    );
    assert_eq!(
        engine.evaluate("RAM.items").unwrap(),
        vec!["Buch", "Stift"].into()
    );
}

#[test]
fn exponentially_expanded_results_are_rejected() {
    let mut rows = vec![row("Data", "r0", "0", CellType::Number)];
    for i in 1..30 {
        rows.push(row(
            "Data",
            &format!("r{i}"),
            &format!("[Data.r{p},Data.r{p}]", p = i - 1),
            CellType::Formula,
        ));
    }
    let err = eval(&rows, "Text(Data.r29)").unwrap_err();
    assert!(err.0.contains("too large"), "{err}");
}

#[test]
fn cycles_recursion_and_huge_syntax_fail_instead_of_hanging() {
    let cyclic = vec![
        row("A", "one", "A.two", CellType::Formula),
        row("A", "two", "A.one", CellType::Formula),
    ];
    assert!(eval(&cyclic, "A.one").unwrap_err().0.contains("Circular"));
    let recursive = vec![Row {
        table: "Functions".into(),
        key: "Loop".into(),
        cells: vec![
            cell("params", CellType::Text, "x"),
            cell("body", CellType::Formula, "Loop(x)"),
        ],
    }];
    assert!(eval(&recursive, "Loop(1)").unwrap_err().0.contains("depth"));
    let nested = format!("{}1{}", "(".repeat(100), ")".repeat(100));
    assert!(parse(&nested).unwrap_err().0.contains("nesting"));
    assert!(parse(&format!("{}1", "1+".repeat(2000))).is_err());
}

#[test]
fn text_numbers_and_records_print_naturally() {
    let rows = vec![Row {
        table: "Data".into(),
        key: "p".into(),
        cells: vec![
            cell("name", CellType::Text, "Ada"),
            cell("age", CellType::Number, "36"),
        ],
    }];
    assert_eq!(eval(&rows, "Text(1.5)").unwrap(), "1.5".into());
    assert_eq!(eval(&rows, "Text(3)").unwrap(), "3".into());
    assert_eq!(
        eval(&rows, "Text(Data.p)").unwrap(),
        "{\"age\":36,\"name\":\"Ada\"}".into()
    );
    assert_eq!(eval(&rows, "Data.p.name + \"!\"").unwrap(), "Ada!".into());
    // Like the original, the bare row shortcut only works without a column.
    assert!(eval(&rows, "p.name").is_err());
    assert_eq!(eval(&rows, "Length(\"Größe\")").unwrap(), 5.0.into());
    assert_eq!(
        eval(&rows, "=Max(1, 7, 3) - Min(4, 2)").unwrap(),
        5.0.into()
    );
    assert_eq!(
        eval(&rows, "Digits(907)").unwrap(),
        vec!["9", "0", "7"].into()
    );
}

#[test]
fn rows_are_validated() {
    assert!(Engine::new(&[row("bad table", "k", "1", CellType::Text)]).is_err());
    assert!(Engine::new(&[row("S", "k", "abc", CellType::Number)]).is_err());
    assert!(Engine::new(&[row("S", "k", "yes", CellType::Boolean)]).is_err());
    assert!(Engine::new(&[row("S", "k", "{\"constructor\":1}", CellType::Json)]).is_err());
    assert!(
        Engine::new(&[
            row("S", "k", "1", CellType::Text),
            row("S", "k", "2", CellType::Text)
        ])
        .is_err()
    );
}
