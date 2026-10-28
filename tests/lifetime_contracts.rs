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
fn rejects(source: &str, code: &str) {
    let e = compile(source, m()).unwrap_err();
    assert_eq!(e.code, code, "{e}");
    assert!(!e.message.contains("_path_poly_"), "{e}");
}
const SOURCE: &str = r#"
struct pair { string left; string right; }
struct wrapper { u32 tag; pair contents; }
trait reader {
    type item;
    control peek(&self input) exits found { &item value from input };
}
impl reader for wrapper {
    type item = pair;
    control peek(&self owner) exits found { &item value from owner } {
        exit found { &(*owner).contents };
    }
}
control peek<r: reader>(&r input) exits found { &reader<r>.item value from input } {
    forward reader<r>.peek(input);
}
control left(&wrapper input) exits found { &string value from input } {
    exit delta peek<wrapper>(input) { found { auto pair } => found { &(*pair).left } };
}
control relay(&wrapper input) exits auto { forward left(input); }
control main() exits done { string text } {
    wrapper owner = wrapper { 9, pair { "left", "right" } };
    exit delta relay(&owner) { found { auto text } => done { clone(text) } };
}
"#;
#[test]
fn opaque_origins_survive_generic_calls_projection_and_inferred_forwarding() {
    let p = compile(SOURCE, m()).unwrap();
    let result = p.run(&scenario()).unwrap();
    assert_eq!(result.fields, vec![Value::String("left".into())]);
    assert_eq!(
        result,
        p.expanded("main").unwrap().run(&scenario()).unwrap()
    );
    for name in ["reader<wrapper>.peek", "peek<wrapper>", "left", "relay"] {
        let source = &p.region(name).unwrap().borrow_returns[&("found".into(), 0)][0];
        assert_eq!(source.parameter, 0);
        assert!(source.opaque);
        assert!(source.fields.is_empty());
    }
}
#[test]
fn declarations_validate_anchor_names_permissions_and_payloads_even_when_unused() {
    for source in [
        "control bad(&string x) exits found { &string value from missing } {exit found {x};}",
        "control bad(string x) exits found { &string value from x } {exit found {&x};}",
        "control bad(&string x) exits found { string value from x } {exit found {clone(x)};}",
        "control bad(&string x) exits found { &mutable string value from x } {exit found {x};}",
        "trait t {control bad(&self x) exits found {&self value};}",
        "trait t {control bad(&self x) exits found {&self value from unknown};}",
    ] {
        rejects(source, "E_LIFETIME");
    }
}
#[test]
fn bodies_must_establish_declared_origins_and_cannot_escape_local_storage() {
    for body in [
        "exit found {other};",
        "string local=\"bad\";exit found {&local};",
    ] {
        rejects(
            &format!(
                "control bad(&string input,&string other) exits found {{&string value from input}} {{{body}}}"
            ),
            "E_LIFETIME",
        );
    }
    rejects(
        "trait t {control get(&self input,&self other) exits found {&self value from input};} impl t for string {control get(&self a,&self b) exits found {&self value from b} {exit found {b};}}",
        "E_LIFETIME",
    );
    rejects(
        "trait t {control get(&self input,&self other) exits found {&self value from input};} impl t for string {control get(&self a,&self b) exits found {&self value from a} {exit found {b};}}",
        "E_LIFETIME",
    );
    rejects(
        "trait t {control get(&self input) exits found {&self value from input};} control bad<r:t>(&r input,&r other) exits found {&r value from input} {forward t<r>.get(other);}",
        "E_LIFETIME",
    );
}
#[test]
fn opaque_origins_freeze_the_owner_including_after_pointee_projection() {
    let source=SOURCE.replace("exit delta relay(&owner) { found { auto text } => done { clone(text) } };", "delta relay(&owner) { found { auto text } => available } [available]: auto moved=owner; exit done {clone(text)};");
    rejects(&source, "E_LOAN");
    let source=SOURCE.replace("exit delta relay(&owner) { found { auto text } => done { clone(text) } };", "delta relay(&owner) { found { auto text } => available } [available]: string copied=clone(text); auto moved=owner; exit done {copied};");
    compile(&source, m()).unwrap();
}
#[test]
fn exclusive_trait_return_reborrows_and_restores_parent_at_last_use() {
    let source = r#"
    struct box {string text;}
    trait edit {control view(&mutable self input) exits found {&mutable string text from input};}
    impl edit for box {control view(&mutable self input) exits found {&mutable string text from input} {exit found {&mutable (*input).text};}}
    control view<r:edit>(&mutable r input) exits found {&mutable string text from input} {forward edit<r>.view(input);}
    control main() exits done {string text} {
        mutable box owner=box {"before"}; auto parent=&mutable owner;
        delta view<box>(parent) {found {auto child}=>available}
        [available]: *child="after"; emit(child); (*parent).text="parent"; exit done {owner.text};
    }"#;
    let p = compile(source, m()).unwrap();
    let result = p.run(&scenario()).unwrap();
    assert_eq!(result.fields, vec![Value::String("parent".into())]);
    assert_eq!(
        result,
        p.expanded("main").unwrap().run(&scenario()).unwrap()
    );
    rejects(
        &source.replace(
            "*child=\"after\";",
            "(*parent).text=\"conflict\"; *child=\"after\";",
        ),
        "E_LOAN",
    );
}
#[test]
fn declared_shared_outputs_and_separate_outcome_origins() {
    let source = r#"control choose(bool flag,&string a,&string b) exits first {&string x from a,&string y from a},second {&string z from b} {
        delta flag {true=>yes,false=>no}
        [yes]:exit first {a,a}; [no]:exit second {b};
    } control main() exits done {string text} {
        string a="a";string b="b";
        exit delta choose(true,&a,&b) {first {auto x,auto y}=>done {clone(x)},second {auto z}=>done {clone(z)}};
    }"#;
    let p = compile(source, m()).unwrap();
    assert_eq!(
        p.run(&scenario()).unwrap().fields,
        vec![Value::String("a".into())]
    );
}
#[test]
fn native_declared_trait_borrows_match_reference_before_and_after_expansion() {
    let p = compile(SOURCE, m()).unwrap();
    let expected = p.run(&scenario()).unwrap();
    let root = std::env::temp_dir().join(format!("path-lifetime-contracts-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    for expanded in [false, true] {
        let p = if expanded {
            p.expanded("main").unwrap()
        } else {
            p.clone()
        };
        for opt in [0, 2] {
            let binary = root.join(format!("borrow-{expanded}-{opt}"));
            support::scenario_executable(&p)
                .build_native("main", &binary, opt)
                .unwrap();
            let result = std::process::Command::new(binary)
                .arg("--json")
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&result.stdout).unwrap(),
                serde_json::to_value(&expected).unwrap()
            );
        }
    }
    std::fs::remove_dir_all(root).unwrap();
}
