mod support;
use tellur::{
    compile,
    ir::Value,
    manifest::{Manifest, TypeDef},
    runtime::Scenario,
};
fn m() -> Manifest {
    Manifest::from_json(include_str!("../playground/manifest.json")).unwrap()
}
fn run(source: &str, args: Vec<Value>) -> tellur::runtime::Execution {
    let p = compile(source, m()).unwrap();
    let s = Scenario {
        entry: "main".into(),
        arguments: args,
        calls: vec![],
    };
    let r = p.run(&s).unwrap();
    assert_eq!(r, p.expanded("main").unwrap().run(&s).unwrap());
    r
}
fn rejects(source: &str, code: &str) {
    let e = compile(source, m()).unwrap_err();
    assert_eq!(e.code, code, "{e}");
}
#[test]
fn source_records_variants_and_wrapper_example_execute() {
    let p = compile(include_str!("../playground/data_types.tellur"), m()).unwrap();
    assert!(matches!(
        p.manifest().type_def("person"),
        Some(TypeDef::Record { .. })
    ));
    assert!(matches!(
        p.manifest().type_def("contact"),
        Some(TypeDef::Variant { .. })
    ));
    assert!(p.manifest().is_copy("user_id"));
    assert!(!p.manifest().is_copy("person"));
    assert!(!p.manifest().linear("contact"));
    run(include_str!("../playground/data_types.tellur"), vec![]);
}
#[test]
fn nominal_wrappers_and_records_never_convert_by_shape() {
    let prefix = "struct user_id {u32 value;} struct invoice_id {u32 value;}";
    rejects(
        &format!(
            "{prefix} control main() exits done {{user_id id=user_id {{1}};invoice_id other=id;exit done;}}"
        ),
        "E_TYPE",
    );
    run(
        &format!(
            "{prefix} control main() exits ready {{invoice_id id}} {{user_id id=user_id {{1}};invoice_id other=invoice_id {{id.value}};exit ready {{other}};}}"
        ),
        vec![],
    );
}
#[test]
fn ordinary_variants_drop_and_clone_but_outcomes_must_be_handled() {
    let source = "variant choice {text {string value},empty} control main() exits done {choice first=choice.text {\"hello\"};choice second=clone(&first);drop(first);exit done;}";
    run(source, vec![]);
    rejects(
        &source.replace("variant choice", "outcome choice"),
        "E_OUTCOME",
    );
    let source = "outcome answer {yes,no} struct envelope {answer result;} control main() exits done {envelope saved=envelope {answer.yes {}};exit done;}";
    rejects(source, "E_OUTCOME");
}
#[test]
fn source_types_keep_partial_moves_mutation_and_borrowed_field_origins() {
    let source = r#"struct pair {string left;string right;}
control right(&pair value) exits found {&string text} {exit found {&(*value).right};}
control main() exits auto {
 mutable pair value=pair {"left","right"};
 string left=value.left;
 value.left="replacement";
 exit delta right(&value) {found {auto text}=>done {clone(text),value.left}};
}"#;
    run(source, vec![]);
    rejects(
        &source.replace("value.left=\"replacement\";", ""),
        "E_PARTIAL_MOVE",
    );
}
#[test]
fn resource_variants_and_nested_outcomes_retain_obligations() {
    let source = "struct file_box {handle file;} variant maybe_file {present {file_box value},empty} control main(handle file) exits ready {maybe_file saved} {exit ready {maybe_file.present {file_box {file}}};}";
    run(
        source,
        vec![Value::Opaque {
            ty: "handle".into(),
            token: "file".into(),
        }],
    );
    rejects(
        "variant maybe_file {present {handle file},empty} control main() exits done {maybe_file saved=maybe_file.empty {};exit done;}",
        "E_RESOURCE",
    );
}
#[test]
fn declarations_are_order_independent_and_can_form_data_only_units() {
    compile("struct envelope {answer value;} outcome answer {yes {pair value},no} struct pair {string text;} variant data {item {envelope value},empty}",m()).unwrap();
    run(
        "control main() exits ready {marker result} {exit ready {marker {}};} struct marker {}",
        vec![],
    );
}
#[test]
fn malformed_and_unsupported_layouts_are_rejected_at_their_declarations() {
    for bad in [
        "struct bad {unknown field;}",
        "struct bad {u32 field;bool field;}",
        "variant bad {}",
        "variant bad {yes,yes}",
        "variant bad {yes {u32 x,bool x}}",
        "struct string {}",
        "variant bad {found {vector<ref string> value}}",
        "struct bad {vector<bad> children;}",
        "struct bad {other next;} struct other {bad next;}",
    ] {
        let e = compile(&format!("struct good {{}}\n{bad}"), m()).unwrap_err();
        assert_eq!(e.code, "E_MANIFEST", "{e}");
        assert_eq!(e.span.line, 2, "{e}");
    }
    compile("variant borrowed {found {ref string value},empty}", m()).unwrap();
    rejects("struct bad {u32 value}", "E_PARSE");
    compile("struct box<t> {t value;}", m()).unwrap();
    rejects("struct thing {} variant thing {empty}", "E_MANIFEST");
    rejects("struct handle {}", "E_MANIFEST");
}
#[test]
fn distinct_variants_require_exhaustive_matching_and_nominal_assignment() {
    rejects(
        "variant first {yes,no} variant second {yes,no} control main() exits done {first value=first.yes {};second other=value;exit done;}",
        "E_TYPE",
    );
    rejects(
        "variant choice {yes,no} control main(choice value) exits done {delta value {yes=>selected}[selected]:exit done;}",
        "E_ARMS",
    );
}
#[test]
fn extraction_and_protocol_analysis_accept_source_data_types() {
    let source = "struct text_box {string value;}\ncontrol main() exits auto {\n text_box box=text_box {\"hello\"};\n exit ready {box};\n}\n";
    let extracted = tellur::extract::extract_tail(source, m(), "main", 4, "finish").unwrap();
    assert_eq!(run(source, vec![]), run(&extracted.source, vec![]));
    compile(&extracted.source, m())
        .unwrap()
        .protocol("main", 100)
        .unwrap();
}

#[test]
fn native_source_types_match_reference_and_expansion_at_o0_o2() {
    use std::{fs, process::Command};
    let source = format!(
        "{}\n{}",
        include_str!("../playground/data_types.tellur"),
        r#"
control sample(bool flag) exits auto {
    exit delta flag {
        true=>ready {contact.known {person {user_id {9},"Grace"}}},
        false=>ready {contact.anonymous {}}
    };
}
control borrow_text(&person value) exits found {&string text} {exit found {&(*value).name};}
control borrowed() exits auto {
    person value=person {user_id {11},"borrowed"};
    exit delta borrow_text(&value) {found {auto text}=>done {clone(text)}};
}
"#
    );
    let original = compile(&source, m()).unwrap();
    let mut expanded = original.clone();
    for r in original.regions() {
        expanded = expanded.expanded(&r.name).unwrap();
    }
    let scenarios = [
        Scenario {
            entry: "main".into(),
            arguments: vec![],
            calls: vec![],
        },
        Scenario {
            entry: "sample".into(),
            arguments: vec![Value::Bool(false)],
            calls: vec![],
        },
        Scenario {
            entry: "sample".into(),
            arguments: vec![Value::Bool(true)],
            calls: vec![],
        },
        Scenario {
            entry: "borrowed".into(),
            arguments: vec![],
            calls: vec![],
        },
    ];
    let expected = scenarios
        .iter()
        .map(|s| serde_json::to_value(original.run(s).unwrap()).unwrap())
        .collect::<Vec<_>>();
    let root = std::env::temp_dir().join(format!("path-data-native-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    for (name, program) in [("original", original), ("expanded", expanded)] {
        for opt in [0, 2] {
            let bin = root.join(format!("{name}-{opt}"));
            support::scenario_executable(&program)
                .build_native("main", &bin, opt)
                .unwrap();
            for (s, expected) in scenarios.iter().zip(&expected) {
                let file = root.join("scenario.json");
                fs::write(&file, serde_json::to_vec(s).unwrap()).unwrap();
                let out = Command::new(&bin)
                    .arg("--scenario")
                    .arg(file)
                    .output()
                    .unwrap();
                assert!(
                    out.status.success(),
                    "{}",
                    String::from_utf8_lossy(&out.stderr)
                );
                assert_eq!(
                    serde_json::from_slice::<serde_json::Value>(&out.stdout).unwrap(),
                    *expected,
                    "{name} O{opt} {}",
                    s.entry
                );
                assert_eq!(
                    serde_json::to_value(program.run(s).unwrap()).unwrap(),
                    *expected
                );
            }
        }
    }
    fs::remove_dir_all(root).unwrap();
}
