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
outcome read_result<item> {received {item value},end}
struct text_source {string text;}
trait reader {
    type item;
    control read(&self input) exits read_result<item>;
}
impl reader for text_source {
    type item=string;
    control read(&self input) exits read_result<item> {
        emit("reading");exit received {clone(&(*input).text)};
    }
}
control main() exits auto {
    text_source source=text_source{text:"Ada"};
    reader<text_source>.item expected="Ada";
    exit delta reader<text_source>.read(&source) {
        received {auto text}=>done {text==expected},end=>done {false}
    };
}
"#;
#[test]
fn concrete_traits_normalize_associated_types_and_dispatch_to_checked_controls() {
    let p = compile(SOURCE, m()).unwrap();
    let r = p.run(&scenario()).unwrap();
    assert_eq!(r.fields, vec![Value::Bool(true)]);
    assert_eq!(r, p.expanded("main").unwrap().run(&scenario()).unwrap());
    assert!(p.region("reader<text_source>.read").is_some());
    assert!(
        !p.manifest()
            .types
            .iter()
            .any(|t| t.name().starts_with("reader<"))
    );
}
#[test]
fn missing_extra_duplicate_members_and_duplicate_implementations_fail() {
    for change in [
        SOURCE.replace("type item=string;", ""),
        SOURCE.replace("type item=string;", "type item=string;type extra=u32;"),
        SOURCE.replace("type item=string;", "type item=string;type item=u32;"),
        SOURCE.replace(
            "control read(&self input) exits read_result<item>;",
            "control read(&self input) exits read_result<item>;control extra() exits done;",
        ),
    ] {
        assert_eq!(compile(&change, m()).unwrap_err().code, "E_TRAIT");
    }
    let source = "trait marker {} impl marker for string {} impl marker for string {}";
    assert_eq!(compile(source, m()).unwrap_err().code, "E_COHERENCE");
}
#[test]
fn explicit_ownership_and_complete_exit_contracts_are_required_even_unused() {
    for (declaration, implementation) in [
        (
            "control use(&self input) exits done;",
            "control use(self input) exits done {exit done;}",
        ),
        (
            "control use(&mutable self input) exits done;",
            "control use(&self input) exits done {exit done;}",
        ),
        (
            "control use() exits ready {u32 value};",
            "control use() exits ready {string value} {exit ready {\"x\"};}",
        ),
        (
            "control use() exits ready,empty;",
            "control use() exits ready {exit ready;}",
        ),
        (
            "control use() exits ready;",
            "control use() exits ready,extra {exit ready;}",
        ),
    ] {
        let source =
            format!("trait test {{{declaration}}} impl test for string {{{implementation}}}");
        let e = compile(&source, m()).unwrap_err();
        assert_eq!(e.code, "E_TRAIT_SIGNATURE", "{e}");
    }
    let source = "trait test {control use() exits ready,empty;} impl test for string {control use() exits empty,ready {exit ready;}}";
    compile(source, m()).unwrap();
}
#[test]
fn associated_aliases_nested_projections_and_cycles_are_checked() {
    let source = "trait source {type item;} impl source for u32 {type item=string;} impl source for string {type item=vector<source<u32>.item>;} control main() exits auto {source<string>.item values=[\"Ada\"];exit done {values};}";
    let p = compile(source, m()).unwrap();
    assert_eq!(
        p.run(&scenario()).unwrap(),
        p.expanded("main").unwrap().run(&scenario()).unwrap()
    );
    for source in [
        "trait t {type a;type b;} impl t for u32 {type a=b;type b=a;}",
        "trait t {type a;} impl t for u32 {type a=t<string>.a;} impl t for string {type a=t<u32>.a;}",
        "trait t {type a;} impl t for u32 {type a=missing;}",
        "trait t {type a;} impl t for u32 {type a=t<string>.a;}",
    ] {
        assert!(compile(source, m()).is_err(), "{source}");
    }
}
#[test]
fn unused_trait_declarations_are_validated_without_concrete_witnesses() {
    for source in [
        "trait t {control x(missing value) exits done;}",
        "trait t {type self;}",
        "trait t {type a;control a() exits done;}",
        "trait t {control x() exits auto;}",
        "trait t {region x() exits done;}",
        "trait t {control x<a>(a value) exits done;}",
        "trait t {control x(vector<string,u32> value) exits done;}",
    ] {
        assert!(compile(source, m()).is_err(), "{source}");
    }
    compile(
        "trait t {type item;control x(&self input) exits ready {item value};}",
        m(),
    )
    .unwrap();
}
#[test]
fn borrowed_results_and_private_associated_types_are_rejected() {
    for source in [
        "trait t {control get(&self value) exits ready {&self value};}",
        "trait t {type item;} impl t for u32 {type item=&string;}",
    ] {
        assert_eq!(compile(source, m()).unwrap_err().code, "E_LIFETIME");
    }
    let source =
        "struct secret {u32 value;} pub trait t {type item;} impl t for u32 {type item=secret;}";
    assert_eq!(compile(source, m()).unwrap_err().code, "E_VISIBILITY");
    let source =
        "struct secret {u32 value;} pub trait t {control get() exits ready {secret value};}";
    assert_eq!(compile(source, m()).unwrap_err().code, "E_VISIBILITY");
}
#[test]
fn ordinary_generic_helpers_and_capture_work_with_static_trait_calls() {
    let source = "trait t {control give(self value) exits ready {self value};} impl t for string {control give(self value) exits ready {self value} {forward id<self>(value);}} control id<item>(item value) exits auto {exit ready {value};} control main() exits auto {outcome<t<string>.give> saved=capture t<string>.give(\"Ada\");exit delta saved {ready {auto value}=>done {value}};}";
    let p = compile(source, m()).unwrap();
    assert_eq!(
        p.run(&scenario()).unwrap().fields,
        vec![Value::String("Ada".into())]
    );
}
#[test]
fn native_trait_calls_match_reference_and_certified_expansion() {
    use std::{fs, process::Command};
    let p = compile(SOURCE, m()).unwrap();
    let expected = serde_json::to_value(p.run(&scenario()).unwrap()).unwrap();
    let root = std::env::temp_dir().join(format!("path-traits-{}", std::process::id()));
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
fn associated_construction_and_nominal_fields_remain_ordinary_data() {
    let source = "struct box<t>{t value;} variant choice {yes,no} trait factory {type item;type result;} impl factory for u32 {type item=box<string>;type result=choice;} control main() exits auto {auto value=factory<u32>.item{value:\"Ada\"};auto choice=factory<u32>.result.yes{};delta choice {yes=>yes,no=>no} [yes]:exit done {value.value};[no]:exit done {\"no\"};}";
    let p = compile(source, m()).unwrap();
    assert_eq!(
        p.run(&scenario()).unwrap().fields,
        vec![Value::String("Ada".into())]
    );
    for source in [
        "trait t {control x() exits missing<u32>;}",
        "trait t {type result;control x() exits result;}",
    ] {
        assert!(compile(source, m()).is_err(), "{source}");
    }
}

#[test]
fn associated_selection_respects_shadowing_and_never_specializes_a_generic_definition_early() {
    let prefix =
        "struct concrete {u32 value;} trait t {type item;} impl t for concrete {type item=string;}";
    for suffix in [
        "struct wrapper<concrete> {t<concrete>.item value;}",
        "control bad<concrete>(t<concrete>.item value) exits ready {string value} {exit ready {value};}",
    ] {
        assert!(
            compile(&format!("{prefix}{suffix}"), m()).is_err(),
            "{suffix}"
        );
    }
    let e = compile(
        "struct item<t>{t value;} trait t {type item;control x(item<u32> value) exits done;}",
        m(),
    )
    .unwrap_err();
    assert_eq!(e.code, "E_TRAIT");
    let source = "trait t {control call() exits ready {string value};} impl t for string {control call() exits ready {string value} {exit ready {\"Ada\"};}} control pass<item>(item value) exits ready {item value} {delta t<string>.call() {ready {auto text}=>called} [called]:drop(text);exit ready {value};} control main() exits auto {forward pass<u32>(7);}";
    let p = compile(source, m()).unwrap();
    assert_eq!(p.run(&scenario()).unwrap().fields, vec![Value::U32(7)]);
}
#[test]
fn traits_preserve_outcome_and_resource_completion_obligations() {
    let source = "trait completer {control finish(self value) exits done;} impl completer for handle {control finish(self value) exits done {exit done;}}";
    assert_eq!(compile(source, m()).unwrap_err().code, "E_RESOURCE");
    let source = "outcome answer {yes,no} trait completer {control finish(self value) exits done;} impl completer for answer {control finish(self value) exits done {exit done;}}";
    assert_eq!(compile(source, m()).unwrap_err().code, "E_OUTCOME");
    let p = compile(include_str!("../playground/traits.tellur"), m()).unwrap();
    p.run(&scenario()).unwrap();
}

#[test]
fn phantom_contract_arguments_are_validated_and_cannot_leak_private_types() {
    let source = "outcome result<t>{done} trait bad {control run() exits result<missing>;}";
    assert_eq!(compile(source, m()).unwrap_err().code, "E_TRAIT");
    let source = "pub outcome result<t>{done} struct secret {u32 value;} pub trait bad {control run() exits result<secret>;}";
    assert_eq!(compile(source, m()).unwrap_err().code, "E_VISIBILITY");
    let source = "trait reader {control run() exits done;} impl reader for u32 {control run() exits done {exit done;}} control main() exits done {\n forward reader<u32>.run();\n}";
    let e = tellur::extract::extract_tail(source, m(), "main", 2, "reader").unwrap_err();
    assert_eq!(e.code, "E_EXTRACT");
}
