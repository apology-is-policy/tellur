mod support;
const EXCLUSIVE: &str = r#"struct editors['a,'b] {ref mutable string left <- 'a;ref mutable string right <- 'b;}
    control make(ref mutable string a,ref mutable string b) exits ready {editors[a,b] value} {exit ready {editors {a,b}};}
    control main() exits done {string a,string b} {
        mutable string a="a";mutable string b="b";
        delta make(borrow mutable a,borrow mutable b) {ready {auto p}=>ready}
        [ready]: ref mutable string first=p.left;deref first="changed a";deref p.right="changed b";exit done {a,b};
    }"#;
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
fn rejects(s: &str, code: &str) {
    let e = compile(s, m()).unwrap_err();
    assert_eq!(e.code, code, "{e}");
}
const SOURCE: &str = r#"
struct text_pair['a, 'b] {
    ref string left <- 'a;
    ref string right <- 'b;
}
control pair(ref string a, ref string b) exits ready { text_pair[a, b] value } {
    exit ready {text_pair {left: a, right: b}};
}
control relay(ref string a, ref string b) exits ready... {forward pair(a,b);}
control main() exits done {string first, string second} {
    mutable string a="left"; mutable string b="right";
    delta relay(borrow a,borrow b) {ready {text_pair p}=>paired}
    [paired]: emit(p.left); a="left released"; emit(p.right); b="right released"; exit done {a,b};
}
"#;
#[test]
fn independent_groups_return_forward_and_end_at_individual_last_uses() {
    let p = compile(SOURCE, m()).unwrap();
    assert_eq!(p.region("pair").unwrap().stored_returns.len(), 2);
    assert_eq!(
        p.run(&scenario()).unwrap().fields,
        vec![
            Value::String("left released".into()),
            Value::String("right released".into())
        ]
    );
    assert_eq!(
        p.run(&scenario()).unwrap(),
        p.expanded("main").unwrap().run(&scenario()).unwrap()
    );
    rejects(
        &SOURCE.replace("emit(p.left); a=", "a=\"too early\"; emit(p.left); a="),
        "E_LOAN",
    );
    rejects(
        &SOURCE.replace("emit(p.right); b=", "b=\"too early\"; emit(p.right); b="),
        "E_LOAN",
    );
}
#[test]
fn invalid_schema_and_boundary_maps_are_rejected() {
    rejects(&SOURCE.replace("['a, 'b]", "['a, 'a]"), "E_ORIGIN");
    rejects(
        &SOURCE.replace("left <- 'a", "left <- 'unknown"),
        "E_ORIGIN",
    );
    rejects(&SOURCE.replace("right <- 'b", "right"), "E_ORIGIN");
    rejects(
        &SOURCE.replace("text_pair[a, b] value", "text_pair[a] value"),
        "E_ORIGIN",
    );
    rejects(
        &SOURCE.replace("text_pair[a, b] value", "text_pair[a, unknown] value"),
        "E_ORIGIN",
    );
    rejects(
        &SOURCE.replace("text_pair[a, b] value", "text_pair[b, a] value"),
        "E_LIFETIME",
    );
    rejects(
        &SOURCE.replace("left: a, right: b", "left: b, right: a"),
        "E_LIFETIME",
    );
    rejects(
        &SOURCE.replace(
            "ref string a, ref string b) exits ready {",
            "string a, ref string b) exits ready {",
        ),
        "E_ORIGIN",
    );
    rejects(
        "struct bad['a] { string text <- 'a; } control main() exits done {exit done;}",
        "E_ORIGIN",
    );
    rejects(
        "struct bad<t>['a] {ref t text <- 'bad;} control main() exits done {exit done;}",
        "E_ORIGIN",
    );
}
#[test]
fn inferred_groups_cannot_hide_local_owner_escape() {
    rejects(
        &SOURCE
            .replace("exits ready { text_pair[a, b] value }", "exits ready...")
            .replace(
                "exit ready {text_pair",
                "string local=\"bad\"; exit ready {text_pair",
            )
            .replace("left: a, right: b", "left: borrow local, right: b"),
        "E_LIFETIME",
    );
}
#[test]
fn one_group_intentionally_unions_dependencies() {
    let s = SOURCE
        .replace("['a, 'b]", "['a]")
        .replace("right <- 'b", "right <- 'a")
        .replace("exits ready { text_pair[a, b] value }", "exits ready...");
    rejects(&s, "E_LOAN");
    compile(
        &s.replace(
            "a=\"left released\"; emit(p.right);",
            "emit(p.right); a=\"left released\";",
        ),
        m(),
    )
    .unwrap();
}
#[test]
fn nested_generic_schema_composition_and_copied_reference_retain_origins() {
    let s = r#"struct view<t>['a] {ref t value <- 'a;}
    struct pair['a,'b] {view<string>['a] left;view<string>['b] right;}
    control make(ref string a,ref string b) exits ready {pair[a,b] value} {
        exit ready {pair {view<string> {a},view<string> {b}}};
    }
    control main() exits done {
        mutable string a="a";string b="b";
        delta make(borrow a,borrow b) {ready {auto p}=>ready}
        [ready]: ref string copied=p.left.value;emit(p.left.value);emit(copied);a="changed";emit(p.right.value);exit done;
    }"#;
    let p = compile(s, m()).unwrap();
    assert_eq!(
        p.run(&scenario()).unwrap(),
        p.expanded("main").unwrap().run(&scenario()).unwrap()
    );
    rejects(
        &s.replace("emit(copied);a=", "a=\"bad\";emit(copied);a="),
        "E_LOAN",
    );
}
#[test]
fn exclusive_results_transfer_without_duplicating_permissions() {
    let s = EXCLUSIVE;
    let p = compile(s, m()).unwrap();
    assert_eq!(
        p.run(&scenario()).unwrap().fields,
        vec![
            Value::String("changed a".into()),
            Value::String("changed b".into())
        ]
    );
    assert_eq!(
        p.run(&scenario()).unwrap(),
        p.expanded("main").unwrap().run(&scenario()).unwrap()
    );
    rejects(
        &s.replace("deref first=", "deref p.left=\"bad\";deref first="),
        "E_MOVED",
    );
    rejects(
        &s.replace("borrow mutable b)", "borrow mutable a)"),
        "E_ALIAS",
    );
}
#[test]
fn native_record_origins_match_interpreter_and_certified_expansion() {
    for (case, source) in [SOURCE, EXCLUSIVE].iter().enumerate() {
        let p = compile(source, m()).unwrap();
        let expected = p.run(&scenario()).unwrap();
        let root =
            std::env::temp_dir().join(format!("path-origin-records-{}-{case}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        for expanded in [false, true] {
            let p = if expanded {
                p.expanded("main").unwrap()
            } else {
                p.clone()
            };
            for opt in [0, 2] {
                let binary = root.join(format!("origins-{expanded}-{opt}"));
                support::scenario_executable(&p)
                    .build_native("main", &binary, opt)
                    .unwrap();
                let actual = std::process::Command::new(binary)
                    .arg("--json")
                    .output()
                    .unwrap();
                assert!(
                    actual.status.success(),
                    "{}",
                    String::from_utf8_lossy(&actual.stderr)
                );
                assert_eq!(
                    serde_json::from_slice::<serde_json::Value>(&actual.stdout).unwrap(),
                    serde_json::to_value(&expected).unwrap()
                );
            }
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn symbolic_generic_bodies_validate_group_contracts_without_a_caller() {
    let s = r#"struct view<t>['a] {ref t value <- 'a;}
    control view_of<t>(ref t input) exits ready {view<t>[input] value} {exit ready {view<t> {input}};}
    control main() exits done... {string a="a";exit delta view_of<string>(borrow a) {ready {auto v}=>done {clone(v.value)}};}"#;
    let p = compile(s, m()).unwrap();
    assert_eq!(
        p.run(&scenario()).unwrap(),
        p.expanded("main").unwrap().run(&scenario()).unwrap()
    );
    rejects(
        r#"struct view<t>['a] {ref t value <- 'a;} control bad<t>(ref t a,ref t b) exits ready {view<t>[a] value} {exit ready {view<t> {b}};}"#,
        "E_LIFETIME",
    );
}
#[test]
fn unsupported_carrier_boundaries_and_missing_inference_are_diagnosed() {
    compile("struct view['a] {ref string value <- 'a;} control okay(view input) exits done {exit done;}", m()).unwrap();
    rejects(
        "struct view['a] {ref string value <- 'a;} control bad(ref string input) exits ready {view value},missing {exit missing;}",
        "E_ORIGIN",
    );
    compile("struct view['a] {ref string value <- 'a;} control okay(ref string input) exits ready {view[input] value},missing {exit missing;}",m()).unwrap();
    rejects(
        "struct view['a] {ref string value <- 'a;} control make(ref string input) exits ready... {exit ready {view {input}};} control main() exits done {string x=\"x\";auto result=capture make(borrow x);exit done;}",
        "E_OUTCOME",
    );
}
#[test]
fn expansion_certificates_protect_contained_origin_metadata() {
    let p = compile(SOURCE, m()).unwrap();
    let original = p.expansion_artifact("relay").unwrap();
    p.accept_expansion(&original).unwrap();
    let mut corrupted = original.clone();
    corrupted.region.stored_returns[0].sources[0].parameter = 1;
    assert_eq!(
        p.accept_expansion(&corrupted).unwrap_err().code,
        "E_EXPANSION_CERTIFICATE"
    );
    let mut corrupted = original;
    corrupted.region.stored_returns.clear();
    assert_eq!(
        p.accept_expansion(&corrupted).unwrap_err().code,
        "E_EXPANSION_CERTIFICATE"
    );
}

#[test]
fn repeated_invocations_and_record_copies_keep_each_retainer_live() {
    let source = SOURCE.replace(
        "emit(p.left); a=",
        "text_pair copy=p; emit(copy.left); emit(p.left); a=",
    );
    let p = compile(&source, m()).unwrap();
    assert_eq!(
        p.run(&scenario()).unwrap(),
        p.expanded("main").unwrap().run(&scenario()).unwrap()
    );
    rejects(
        &SOURCE
            .replace("emit(p.left); a=", "text_pair copy=p; emit(p.left); a=")
            .replace("emit(p.right);", "emit(copy.left);emit(p.right);"),
        "E_LOAN",
    );
    let source=SOURCE.replace("[paired]: emit(p.left);", "[paired]: delta pair(borrow b,borrow a) {ready {auto reversed}=>again} [paired & again]: emit(reversed.right);emit(reversed.left);emit(p.left);");
    let p = compile(&source, m()).unwrap();
    assert_eq!(
        p.run(&scenario()).unwrap(),
        p.expanded("main").unwrap().run(&scenario()).unwrap()
    );
}
#[test]
fn opaque_exclusive_groups_require_checked_body_permissions() {
    let s = r#"struct owner {string left;string right;}
    struct refs['a,'b] {ref mutable string left <- 'a;ref mutable string right <- 'b;}
    control split(ref mutable owner input) exits ready {refs[input,input] value} {
        exit ready {refs {borrow mutable input.left,borrow mutable input.right}};
    }"#;
    compile(s, m()).unwrap();
    rejects(
        &s.replace("borrow mutable input.right", "borrow mutable input.left"),
        "E_ALIAS",
    );
    // Without an opaque anchor, the checked concrete projections establish disjointness.
    compile(
        &s.replace("exits ready {refs[input,input] value}", "exits ready..."),
        m(),
    )
    .unwrap();
}

#[test]
fn grouped_schema_expansion_has_an_explicit_budget() {
    let mut source = "struct leaf['a] {ref string value <- 'a;}\n".to_owned();
    let mut previous = "leaf".to_owned();
    for i in 0..13 {
        let name = format!("layer_{i}");
        source += &format!("struct {name}['a] {{{previous}['a] left;{previous}['a] right;}}\n");
        previous = name;
    }
    let error = compile(&source, m()).unwrap_err();
    assert_eq!(error.code, "E_ORIGIN");
    assert!(error.message.contains("4096"));
}
