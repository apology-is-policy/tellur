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
outcome result<item> {ready {item value},empty}
trait reader {type item: clone;control read(&self input) exits result<item>;}
impl reader for string {type item=string;control read(&self input) exits result<item> {emit("read");exit ready {clone(input)};}}
control read_one<r: reader>(&r input) exits result<reader<r>.item> {forward reader<r>.read(input);}
control duplicate<t: clone>(t value) exits ready {t first,t second} {auto second=clone(&value);exit ready {value,second};}
control main() exits auto {
 string value="Ada";
 exit delta read_one<string>(&value) {ready {auto text}=>delta duplicate<reader<string>.item>(text) {ready {auto first,auto second}=>done {first,second}},empty=>done {"empty","empty"}};
}
"#;
#[test]
fn trait_constrained_generics_are_checked_with_abstract_witnesses_then_specialized() {
    let p = compile(SOURCE, m()).unwrap();
    let r = p.run(&scenario()).unwrap();
    assert_eq!(
        r.fields,
        vec![Value::String("Ada".into()), Value::String("Ada".into())]
    );
    assert_eq!(r, p.expanded("main").unwrap().run(&scenario()).unwrap());
    assert!(!p.regions().any(|r| r.name.contains("_path_poly_")));
    assert!(
        !p.manifest()
            .types
            .iter()
            .any(|t| t.name().contains("_path_poly_"))
    );
    let generic = p
        .generic_controls()
        .iter()
        .find(|g| g.name == "read_one")
        .unwrap();
    assert_eq!(generic.exits[0].fields[0].ty, "reader<r>.item");
}
#[test]
fn unused_generic_bodies_have_only_declared_traits_and_associated_permissions() {
    for source in [
        "trait t {control act(&self input) exits done;} control bad<r>(&r input) exits done {forward t<r>.act(input);}",
        "trait t {type item;control get(&self input) exits ready {item value};} control bad<r:t>(&r input) exits auto {exit delta t<r>.get(input) {ready {auto value}=>done {clone(&value),value}};}",
        "trait t {type item:drop;control get(&self input) exits ready {item value};} control bad<r:t>(&r input) exits auto {exit delta t<r>.get(input) {ready {auto value}=>done {clone(&value),value}};}",
        "trait t {type item:clone;control get(&self input) exits ready {item value};} control bad<r:t>(&r input) exits auto {exit delta t<r>.get(input) {ready {auto value}=>done {value,value}};}",
    ] {
        let e = compile(source, m()).unwrap_err();
        assert!(e.message.contains("generic definition check"), "{e}");
    }
    compile("trait t {type item:copy;control get(&self input) exits ready {item value};} control good<r:t>(&r input) exits auto {exit delta t<r>.get(input) {ready {auto value}=>done {value,value}};}",m()).unwrap();
}
#[test]
fn concrete_and_generic_requirements_are_checked_even_when_phantom() {
    let source = "trait marker {} control need<t: marker>() exits done {exit done;} control main() exits done {forward need<u32>();}";
    assert_eq!(compile(source, m()).unwrap_err().code, "E_BOUND");
    compile(
        &source.replace("trait marker {}", "trait marker {} impl marker for u32 {}"),
        m(),
    )
    .unwrap();
    let source = "trait marker {} control need<t:marker>(t value) exits ready {t value} {exit ready {value};} control bad<t>(t value) exits auto {forward need<t>(value);}";
    assert_eq!(compile(source, m()).unwrap_err().code, "E_BOUND");
    let source = source.replace("bad<t>", "bad<t:marker>");
    compile(&source, m()).unwrap();
    let source = "trait marker {} control bad<t:marker>(t value) exits done {exit done;}";
    assert_eq!(compile(source, m()).unwrap_err().code, "E_RESOURCE");
}
#[test]
fn associated_bounds_cannot_be_promised_by_an_invalid_concrete_implementation() {
    for (bound, ty) in [("copy", "string"), ("clone", "handle"), ("drop", "answer")] {
        let source = format!(
            "outcome answer{{yes,no}} trait t{{type item:{bound};}} impl t for u32{{type item={ty};}}"
        );
        let e = compile(&source, m()).unwrap_err();
        assert_eq!(e.code, "E_BOUND", "{e}");
    }
    let source = "trait t {type item:clone+drop;} impl t for u32 {type item=string;}";
    compile(source, m()).unwrap();
}
#[test]
fn multiple_traits_keep_same_spelled_associated_types_distinct() {
    let source = "trait a {type item;control get(&self input) exits ready {item value};} trait b {type item;control get(&self input) exits ready {item value};} control bad<t:a+b>(&t input) exits ready {a<t>.item value} {forward b<t>.get(input);}";
    assert_eq!(compile(source, m()).unwrap_err().code, "E_TYPE");
    let source = source.replace(
        "exits ready {a<t>.item value}",
        "exits ready {b<t>.item value}",
    );
    compile(&source, m()).unwrap();
}
#[test]
fn generic_dispatch_preserves_resources_outcomes_and_mutable_borrows() {
    let source = "trait consume {control finish(self value) exits done;} impl consume for answer {control finish(self value) exits done {exit delta value {yes=>done {},no=>done {}};}} outcome answer{yes,no} control finish<t:consume>(t value) exits done {forward consume<t>.finish(value);} control main() exits done {forward finish<answer>(answer.yes{});}";
    compile(source, m()).unwrap().run(&scenario()).unwrap();
    let source = "trait rewrite {control change(&mutable self value) exits done;} impl rewrite for string {control change(&mutable self value) exits done {*value=\"after\";exit done;}} control update<t:rewrite>(&mutable t value) exits done {forward rewrite<t>.change(value);} control main() exits auto {mutable string value=\"before\";update<string>(&mutable value);exit done {value};}";
    assert_eq!(
        compile(source, m())
            .unwrap()
            .run(&scenario())
            .unwrap()
            .fields,
        vec![Value::String("after".into())]
    );
}
#[test]
fn native_trait_bound_calls_match_reference_and_certified_expansion() {
    use std::{fs, process::Command};
    let p = compile(SOURCE, m()).unwrap();
    let expected = serde_json::to_value(p.run(&scenario()).unwrap()).unwrap();
    let root = std::env::temp_dir().join(format!("path-trait-bounds-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let file = root.join("scenario.json");
    fs::write(&file, serde_json::to_vec(&scenario()).unwrap()).unwrap();
    for (name, p) in [
        ("original", p.clone()),
        ("expanded", p.expanded("main").unwrap()),
    ] {
        for opt in [0, 2] {
            let bin = root.join(format!("{name}-{opt}"));
            support::scenario_executable(&p)
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
fn generic_capture_and_associated_arguments_keep_named_contracts_and_permissions() {
    let source=SOURCE.replace("{forward reader<r>.read(input);}","{read_result_placeholder}").replace("{read_result_placeholder}","{result<reader<r>.item> saved=capture reader<r>.read(input);exit delta saved {ready {auto value}=>ready {value},empty=>empty {}};}");
    let p = compile(&source, m()).unwrap();
    assert_eq!(
        p.run(&scenario()).unwrap(),
        p.expanded("main").unwrap().run(&scenario()).unwrap()
    );
    let source = "trait t {type item:clone;control get(&self input) exits ready {item value};} control dup<a:clone>(a value) exits ready {a first,a second} {auto second=clone(&value);exit ready {value,second};} control use_t<r:t>(&r input) exits auto {exit delta t<r>.get(input) {ready {auto value}=>delta dup<t<r>.item>(value) {ready {auto a,auto b}=>done {a,b}}};}";
    compile(source, m()).unwrap();
    assert_eq!(
        compile(&source.replace("type item:clone;", "type item:drop;"), m())
            .unwrap_err()
            .code,
        "E_BOUND"
    );
}
#[test]
fn missing_bounds_in_abstract_expansion_never_expose_internal_type_identifiers() {
    let source = "trait t {control go(&self input) exits done;} control bad<r>(&r input) exits done {forward t<r>.go(input);}";
    let e = compile(source, m()).unwrap_err();
    assert_eq!(e.code, "E_BOUND");
    assert!(e.message.contains("generic definition check"));
    assert!(!e.message.contains("_path_poly_"));
    let source =
        "trait t {type item;} control bad<r>(t<r>.item value) exits auto {exit done {value};}";
    let e = compile(source, m()).unwrap_err();
    assert!(!e.message.contains("_path_poly_"), "{e}");
}

#[test]
fn trait_and_sealed_bounds_compose_and_associated_borrows_keep_parameter_origins() {
    let source = "trait reader {type item;} impl reader for string {type item=string;} control view<r:reader>(&reader<r>.item value) exits found {&reader<r>.item value} {exit found {value};} control main() exits auto {string value=\"Ada\";exit delta view<string>(&value) {found {auto reference}=>done {clone(reference)}};}";
    let p = compile(source, m()).unwrap();
    assert_eq!(
        p.run(&scenario()).unwrap().fields,
        vec![Value::String("Ada".into())]
    );
    let source = "trait marker{} impl marker for string{} control twice<t:marker+clone>(t value) exits auto {auto copy=clone(&value);exit ready {value,copy};} control main() exits auto {forward twice<string>(\"Ada\");}";
    compile(source, m()).unwrap().run(&scenario()).unwrap();
    assert_eq!(
        compile(&source.replace("marker+clone", "marker+copy"), m())
            .unwrap_err()
            .code,
        "E_BOUND"
    );
}
#[test]
fn abstract_witness_budgets_fail_explicitly() {
    let methods = (0..4097)
        .map(|i| format!("control method_{i}() exits done;"))
        .collect::<String>();
    let source = format!(
        "trait large {{{methods}}} control use_trait<t:large>(t value) exits ready {{t value}} {{exit ready {{value}};}}"
    );
    let e = compile(&source, m()).unwrap_err();
    assert!(e.message.contains("abstract-operation budget"), "{e}");
}
