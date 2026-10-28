mod support;
use tellur::{compile, ir::Value, manifest::Manifest, runtime::Scenario};
fn m() -> Manifest {
    Manifest::from_json(include_str!("../playground/manifest.json")).unwrap()
}
fn scenario() -> Scenario {
    Scenario {
        entry: "main".into(),
        arguments: vec![],
        calls: vec![],
    }
}
const SOURCE: &str = r#"
struct box<t> {t value;}
struct pair<a,b> {a first;b second;}
variant choice<t> {some {t value},none}
outcome result<t,e> {ready {t value},failed {e error}}
control make() exits result<box<string>,u32> {
    exit ready {box<string> {value:"Ada"}};
}
control main() exits auto {
    auto copy=box<u32> {1};
    auto pair=pair<u32,box<string>> {second:box<string> {"nested"},first:7};
    auto choice=choice<u32>.some {pair.first};
    delta choice {some {auto number}=>present,none=>absent}
    [present]:emit(u32_text(number));
    [entry]: exit delta make() {ready {auto value}=>done {value.value,pair.second.value},failed {auto error}=>bad {error}};
}
"#;
#[test]
fn generic_struct_variant_outcome_and_nested_layouts_execute() {
    let p = compile(SOURCE, m()).unwrap();
    let result = p.run(&scenario()).unwrap();
    assert_eq!(
        result.fields,
        vec![Value::String("Ada".into()), Value::String("nested".into())]
    );
    assert_eq!(
        result,
        p.expanded("main").unwrap().run(&scenario()).unwrap()
    );
    assert!(p.manifest().is_copy("box<U32>"));
    assert!(!p.manifest().is_copy("box<String>"));
}
#[test]
fn generic_data_obligations_are_derived_from_actual_fields() {
    for (source, code) in [
        (
            "struct box<t>{t value;} control main(handle file) exits done {auto box=box<handle>{file};exit done;}",
            "E_RESOURCE",
        ),
        (
            "struct box<t>{t value;} outcome answer {yes,no} control main() exits done {auto box=box<answer>{answer.yes {}};exit done;}",
            "E_OUTCOME",
        ),
        (
            "struct box<t>{t value;} control main() exits done {auto box=box<string>{\"x\"};auto first=box;auto second=box;exit done;}",
            "E_MOVED",
        ),
        (
            "struct box<t>{t value;} control main() exits done {box<u32> box=box<string>{\"x\"};exit done;}",
            "E_TYPE",
        ),
    ] {
        let e = compile(source, m()).unwrap_err();
        assert_eq!(e.code, code, "{e}");
    }
    compile("struct box<t>{t value;} control main(handle file) exits ready {box<handle> value} {exit ready {box<handle>{file}};}",m()).unwrap();
    compile("struct box<t>{t value;} control main() exits done {auto box=box<u32>{1};auto a=box;auto b=box;exit done;}",m()).unwrap();
}
#[test]
fn malformed_unused_templates_and_invalid_arguments_are_diagnosed() {
    for source in [
        "struct box<t,t>{t value;}",
        "struct box<string>{string value;}",
        "struct box<t>{missing value;}",
        "struct box<t>{t value;t value;}",
        "variant box<t>{a,a}",
        "outcome box<t>{}",
        "struct box<t>{box<t> next;}",
        "struct box<t>{vector<box<t>> next;}",
        "struct a<t>{b<t> value;}struct b<t>{a<t> value;}",
        "struct box<t>{t value;} struct bad {box value;}",
        "struct box<t>{t value;} struct bad {box<u32,string> value;}",
        "struct box<t>{t value;} struct bad {box<missing> value;}",
        "struct box<t>{t value;} control main() exits done {auto x=box<&string>{\"x\"};exit done;}",
        "struct bad<t>{map<bytes,u32> value;}",
    ] {
        let e = compile(source, m()).unwrap_err();
        assert_eq!(e.code, "E_GENERIC", "{source}: {e}");
    }
    compile("struct view<t>{ref t value;}", m()).unwrap();
}
#[test]
fn instantiation_order_and_alias_spelling_do_not_change_identity() {
    let source = "struct box<t>{t value;} control main() exits auto {box<U32> a=box<u32>{1};box<u32> b=a;exit done {b.value};}";
    assert_eq!(
        compile(source, m())
            .unwrap()
            .run(&scenario())
            .unwrap()
            .fields,
        vec![Value::U32(1)]
    );
    compile("struct unused<t>{t value;}", m()).unwrap();
    compile("struct box<t>{t value;} struct outer {box<u32> value;} control main() exits auto {auto value=box<box<u32>>{box<u32>{1}};exit done {value.value.value};}",m()).unwrap();
}
#[test]
fn expected_generic_outcomes_can_be_stored_and_forwarded() {
    let source = "outcome answer<t>{ready {t value},empty} control make() exits answer<string> {exit ready {\"x\"};} control main() exits answer<string> {answer<string> saved=capture make();exit saved;}";
    let p = compile(source, m()).unwrap();
    assert_eq!(
        p.run(&scenario()).unwrap(),
        p.expanded("main").unwrap().run(&scenario()).unwrap()
    );
}
#[test]
fn constructor_angles_do_not_change_ordinary_comparisons() {
    compile("struct box<t>{t value;} control main(u32 a,u32 b) exits done {bool less=a<b;bool more=a>b;exit done;}",m()).unwrap();
}
#[test]
fn native_generic_layouts_match_reference_at_o0_o2() {
    use std::{fs, process::Command};
    let p = compile(SOURCE, m()).unwrap();
    let expected = serde_json::to_value(p.run(&scenario()).unwrap()).unwrap();
    let root = std::env::temp_dir().join(format!("path-generic-data-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let file = root.join("scenario.json");
    fs::write(&file, serde_json::to_vec(&scenario()).unwrap()).unwrap();
    for (name, program) in [
        ("original", p.clone()),
        ("expanded", p.expanded("main").unwrap()),
    ] {
        for opt in [0, 2] {
            let bin = root.join(format!("{name}-{opt}"));
            support::scenario_executable(&program)
                .build_native("main", &bin, opt)
                .unwrap();
            let out = Command::new(bin)
                .arg("--scenario")
                .arg(&file)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&out.stdout).unwrap(),
                expected
            );
        }
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn specialization_budgets_fail_instead_of_truncating_the_program() {
    let deep = format!("{}u32{}", "box<".repeat(70), ">".repeat(70));
    let e = compile(
        &format!("struct box<t>{{t value;}} control main({deep} value) exits done {{exit done;}}"),
        m(),
    )
    .unwrap_err();
    assert_eq!(e.code, "E_GENERIC");
    assert!(e.message.contains("budget"), "{e}");
    let params = (0..1025)
        .map(|n| format!("box<array<u32,{n}>> x{n}"))
        .collect::<Vec<_>>()
        .join(",");
    let e = compile(
        &format!("struct box<t>{{t value;}} control main({params}) exits done {{exit done;}}"),
        m(),
    )
    .unwrap_err();
    assert_eq!(e.code, "E_GENERIC");
    assert!(e.message.contains("1024"), "{e}");
}
#[test]
fn unused_templates_validate_names_and_concrete_collection_constraints_at_use() {
    compile("struct dictionary<k,v>{map<k,v> values;}", m()).unwrap();
    let e=compile("struct dictionary<k,v>{map<k,v> values;} control main() exits done {auto value=dictionary<bytes,string>{map {}};exit done;}",m()).unwrap_err();
    assert_eq!(e.code, "E_MANIFEST");
    let source = "struct box<t>{t value;} control main() exits auto {auto value=box<array<u32,2>>{[1,2]};exit done {value};}";
    compile(source, m()).unwrap();
}

#[test]
fn type_parameters_are_lexical_even_when_an_outer_private_type_has_that_name() {
    let source = "struct item {} pub struct box<item>{pub item value;} control main() exits auto {auto value=box<u32>{1};exit done {value.value};}";
    compile(source, m()).unwrap();
    let bad = "struct box<t>{t value;}struct bad<box>{box<u32> value;}";
    assert_eq!(compile(bad, m()).unwrap_err().code, "E_GENERIC");
}

#[test]
fn generic_playground_and_extracted_tails_preserve_specialized_contracts() {
    let source = include_str!("../playground/generic_data.tellur");
    let p = compile(source, m()).unwrap();
    assert_eq!(
        p.run(&scenario()).unwrap(),
        p.expanded("main").unwrap().run(&scenario()).unwrap()
    );
    let source = "struct box<t>{t value;}\ncontrol main() exits auto {\n auto value=box<string>{\"Ada\"};\n exit ready {value};\n}";
    let extracted = tellur::extract::extract_tail(source, m(), "main", 4, "finish").unwrap();
    assert_eq!(
        compile(source, m()).unwrap().run(&scenario()).unwrap(),
        compile(&extracted.source, m())
            .unwrap()
            .run(&scenario())
            .unwrap()
    );
}

#[test]
fn specializations_never_inject_unqualified_variant_constructors() {
    let source = "variant generic<t>{some {t value},none,plain {t value}} variant ordinary{plain {u32 value}} control main() exits auto {auto a=generic<u32>.some {1};auto b=generic<string>.some {\"x\"};auto c=plain {7};option<u32> d=some {9};exit done {c,d};}";
    let p = compile(source, m()).unwrap();
    assert_eq!(
        p.run(&scenario()).unwrap(),
        p.expanded("main").unwrap().run(&scenario()).unwrap()
    );
    let bad = "variant generic<t>{tag {t value}} control main() exits done {auto a=generic<u32>.tag {1};auto b=tag {2};exit done;}";
    assert_eq!(compile(bad, m()).unwrap_err().code, "E_NAME");
}
