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
fn rejects(s: &str, code: &str) {
    let e = compile(s, m()).unwrap_err();
    assert_eq!(e.code, code, "{e}");
}
const SHARED: &str = r#"
struct text_pair['a,'b] {ref string left <- 'a; ref string right <- 'b;}
control take_left(text_pair input) exits found {ref string value from input['a]} {
    exit found {input.left};
}
control relay(text_pair input) exits found... {forward take_left(input);}
control main() exits done {string a,string b} {
    mutable string a="a";mutable string b="b";
    text_pair p={a};
    delta relay(p) {found {auto first}=>ready}
    [ready]: b="released b";emit(first);a="released a";exit done {a,b};
}
"#;
fn shared() -> String {
    SHARED.replace(
        "text_pair p={a};",
        "text_pair p=text_pair {borrow a,borrow b};",
    )
}
#[test]
fn shared_record_inputs_propagate_selected_origins() {
    let s = shared();
    let p = compile(&s, m()).unwrap();
    assert_eq!(
        p.run(&scenario()).unwrap().fields,
        vec![
            Value::String("released a".into()),
            Value::String("released b".into())
        ]
    );
    assert_eq!(
        p.run(&scenario()).unwrap(),
        p.expanded("main").unwrap().run(&scenario()).unwrap()
    );
    rejects(
        &s.replace("emit(first);a=", "a=\"bad\";emit(first);a="),
        "E_LOAN",
    );
    rejects(
        &s.replace("exit found {input.left}", "exit found {input.right}"),
        "E_LIFETIME",
    );
    rejects(
        &s.replace("from input['a]", "from input['unknown]"),
        "E_LIFETIME",
    );
    rejects(&s.replace("from input['a]", "from input"), "E_LIFETIME");
}
#[test]
fn moved_exclusive_record_returns_transfer_permissions() {
    let s = r#"
    struct editors['a,'b] {ref mutable string left <- 'a;ref mutable string right <- 'b;}
    control relay(editors input) exits ready {editors[input['a],input['b]] value} {exit ready {input};}
    control main() exits done... {
        mutable string a="a";mutable string b="b";
        editors p=editors {borrow mutable a,borrow mutable b};
        delta relay(p) {ready {auto q}=>ready}
        [ready]: deref q.left="changed a";deref q.right="changed b";exit done {a,b};
    }"#;
    let p = compile(s, m()).unwrap();
    assert_eq!(
        p.run(&scenario()).unwrap(),
        p.expanded("main").unwrap().run(&scenario()).unwrap()
    );
    assert_eq!(
        p.run(&scenario()).unwrap().fields,
        vec![
            Value::String("changed a".into()),
            Value::String("changed b".into())
        ]
    );
    rejects(
        &s.replace("deref q.left=", "emit(p.left);deref q.left="),
        "E_MOVED",
    );
    rejects(
        &s.replace("deref q.left=", "a=\"bad\";deref q.left="),
        "E_LOAN",
    );
}
#[test]
fn owned_carrier_storage_never_escapes() {
    rejects(
        r#"struct view['a] {string owned;ref string value <- 'a;}
    control bad(view input) exits found... {exit found {borrow input.owned};}"#,
        "E_LIFETIME",
    );
}
#[test]
fn call_aliases_include_contained_loans_in_both_argument_orders() {
    for (params, args) in [
        ("view input,ref mutable string owner", "p,borrow mutable a"),
        ("ref mutable string owner,view input", "borrow mutable a,p"),
    ] {
        let s = format!(
            "struct view['a] {{ref string value <- 'a;}} control use({params}) exits done {{emit(input.value);deref owner=\"bad\";exit done;}} control main() exits done {{mutable string a=\"a\";view p=view {{borrow a}};forward use({args});}}"
        );
        let e = compile(&s, m()).unwrap_err();
        assert!(matches!(e.code, "E_ALIAS" | "E_LOAN"), "{e}");
    }
}
#[test]
fn borrowed_carrier_shared_contents_outlive_the_carrier() {
    let s = r#"struct view['a] {string caption;ref string value <- 'a;}
    control peek(ref view input) exits found {ref string value from input['a]} {exit found {input.value};}
    control main() exits done... {
        mutable string a="outside";
        view p=view {"carrier",borrow a};
        delta peek(borrow p) {found {auto value}=>ready}
        [ready]: drop(p);emit(value);a="released";exit done {a};
    }"#;
    let p = compile(s, m()).unwrap();
    assert_eq!(
        p.run(&scenario()).unwrap(),
        p.expanded("main").unwrap().run(&scenario()).unwrap()
    );
    rejects(
        &s.replace("emit(value);a=", "a=\"bad\";emit(value);a="),
        "E_LOAN",
    );
    rejects(
        &s.replace("from input['a]", "from input")
            .replace("input.value", "borrow input.caption"),
        "E_LOAN",
    );
}
#[test]
fn carrier_and_contained_origins_remain_distinct() {
    let s = r#"struct view['a] {string caption;ref string value <- 'a;}
    control inspect(ref view input) exits found {ref string caption from input,ref string value from input['a]} {
        exit found {borrow input.caption,input.value};
    }
    control main() exits done {
        string a="outside";view p=view {"carrier",borrow a};
        delta inspect(borrow p) {found {auto caption,auto value}=>ready}
        [ready]: emit(caption);drop(p);emit(value);exit done;
    }"#;
    let p = compile(s, m()).unwrap();
    assert_eq!(
        p.run(&scenario()).unwrap(),
        p.expanded("main").unwrap().run(&scenario()).unwrap()
    );
    rejects(
        &s.replace("emit(caption);drop(p);", "drop(p);emit(caption);"),
        "E_LOAN",
    );
    rejects(
        &s.replace(
            "borrow input.caption,input.value",
            "input.value,borrow input.caption",
        ),
        "E_LIFETIME",
    );
}
#[test]
fn local_carrier_borrows_keep_contained_loans_live() {
    let s = r#"struct view['a] {ref string value <- 'a;}
    control main() exits done {
        mutable string a="a";view p=view {borrow a};
        ref view alias=borrow p;emit(alias.value);a="released";exit done;
    }"#;
    let p = compile(s, m()).unwrap();
    assert_eq!(
        p.run(&scenario()).unwrap(),
        p.expanded("main").unwrap().run(&scenario()).unwrap()
    );
    rejects(
        &s.replace("emit(alias.value);a=", "a=\"bad\";emit(alias.value);a="),
        "E_LOAN",
    );
}
#[test]
fn exclusive_carrier_borrow_can_mutate_owned_fields() {
    let s = r#"struct view['a] {string caption;ref string value <- 'a;}
    control edit(ref mutable view input) exits found {ref string value from input['a]} {
        input.caption="changed";exit found {input.value};
    }
    control main() exits done... {
        string a="outside";mutable view p=view {"carrier",borrow a};
        delta edit(borrow mutable p) {found {auto value}=>ready}
        [ready]: emit(value);exit done {p.caption};
    }"#;
    let p = compile(s, m()).unwrap();
    assert_eq!(
        p.run(&scenario()).unwrap().fields,
        vec![Value::String("changed".into())]
    );
    assert_eq!(
        p.run(&scenario()).unwrap(),
        p.expanded("main").unwrap().run(&scenario()).unwrap()
    );
}
#[test]
fn inline_record_arguments_and_inferred_groups_compose() {
    let s = shared()
        .replace("text_pair p=text_pair {borrow a,borrow b};", "")
        .replace(
            "relay(p)",
            "relay(text_pair {right:borrow b,left:borrow a})",
        )
        .replace(
            "exits found {ref string value from input['a]}",
            "exits found...",
        );
    let p = compile(&s, m()).unwrap();
    assert_eq!(
        p.run(&scenario()).unwrap(),
        p.expanded("main").unwrap().run(&scenario()).unwrap()
    );
}
#[test]
fn nested_generic_inputs_are_checked_before_instantiation() {
    let s = r#"struct view<t>['a] {ref t value <- 'a;}
    struct wrapper<t>['a] {view<t>['a] inner;}
    control peek<t>(wrapper<t> input) exits found {ref t value from input['a]} {exit found {input.inner.value};}
    control main() exits done... {string a="a";wrapper<string> p=wrapper<string> {view<string> {borrow a}};
        exit delta peek<string>(p) {found {auto result}=>done {clone(result)}};
    }"#;
    let p = compile(s, m()).unwrap();
    assert_eq!(
        p.run(&scenario()).unwrap(),
        p.expanded("main").unwrap().run(&scenario()).unwrap()
    );
    rejects(
        r#"struct view<t>['a] {ref t value <- 'a;} control bad<t>(view<t> input,ref t other) exits found {ref t value from input['a]} {exit found {other};}"#,
        "E_LIFETIME",
    );
}
#[test]
fn certificate_rejects_retargeted_contained_origin() {
    let p = compile(&shared(), m()).unwrap();
    let mut artifact = p.expansion_artifact("relay").unwrap();
    p.accept_expansion(&artifact).unwrap();
    artifact.region.borrow_returns.values_mut().next().unwrap()[0]
        .contained
        .as_mut()
        .unwrap()
        .fields = vec![1];
    assert_eq!(
        p.accept_expansion(&artifact).unwrap_err().code,
        "E_EXPANSION_CERTIFICATE"
    );
}
#[test]
fn exclusive_carriers_forward_through_inline_adapters() {
    let s = r#"struct view['a] {ref mutable string value <- 'a;}
    control relay(view input) exits ready {view[input['a]] value} {exit ready {input};}
    control again(view input) exits ready... {forward relay(input);}
    control main() exits done... {mutable string a="a";view p=view {borrow mutable a};
        delta again(p) {ready {auto result}=>ready}
        [ready]: deref result.value="changed";exit done {a};
    }"#;
    let p = compile(s, m()).unwrap();
    assert_eq!(
        p.run(&scenario()).unwrap(),
        p.expanded("main").unwrap().run(&scenario()).unwrap()
    );
}
#[test]
fn native_inputs_match_interpreter_and_expansion_at_both_optimization_levels() {
    let exclusive = r#"struct view['a] {ref mutable string value <- 'a;}
    control relay(view input) exits ready {view[input['a]] value} {exit ready {input};}
    control main() exits done... {mutable string a="a";view p=view {borrow mutable a};
        delta relay(p) {ready {auto result}=>ready}
        [ready]: deref result.value="changed";exit done {a};}"#;
    for (case, source) in [include_str!("fixtures/record_inputs.tellur"), exclusive]
        .iter()
        .enumerate()
    {
        let p = compile(source, m()).unwrap();
        let expected = p.run(&scenario()).unwrap();
        let root =
            std::env::temp_dir().join(format!("path-record-inputs-{}-{case}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        for expanded in [false, true] {
            let p = if expanded {
                p.expanded("main").unwrap()
            } else {
                p.clone()
            };
            for opt in [0, 2] {
                let binary = root.join(format!("inputs-{expanded}-{opt}"));
                support::scenario_executable(&p)
                    .build_native("main", &binary, opt)
                    .unwrap();
                let output = std::process::Command::new(binary)
                    .arg("--json")
                    .output()
                    .unwrap();
                assert!(
                    output.status.success(),
                    "{}",
                    String::from_utf8_lossy(&output.stderr)
                );
                assert_eq!(
                    serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap(),
                    serde_json::to_value(&expected).unwrap()
                );
            }
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}
#[test]
fn exclusive_carrier_inputs_are_accepted() {
    for parameter in ["ref view input", "ref mutable view input"] {
        compile(&format!("struct view['a] {{ref mutable string value <- 'a;}} control peek({parameter}) exits done {{exit done;}}"), m()).unwrap();
    }
}

#[test]
fn released_carrier_alias_is_unavailable_even_when_external_owner_survives() {
    rejects(
        r#"struct view['a] {ref string value <- 'a;} control main() exits done {
        string a="a";view p=view {borrow a};ref view alias=borrow p;drop(alias);emit(alias.value);exit done;
    }"#,
        "E_MOVED",
    );
    compile(r#"struct view['a] {ref string value <- 'a;} control lend(ref view input) exits found... {exit found {input};}"#, m()).unwrap();
}
#[test]
fn projected_access_checks_carrier_but_copied_references_release_it() {
    let s = r#"struct person {string name;} struct view['a] {ref person value <- 'a;}
    control main() exits done {person a=person {"a"};view p=view {borrow a};ref view alias=borrow p;
        emit(borrow alias.value.name);exit done;
    }"#;
    compile(s, m()).unwrap();
    rejects(
        &s.replace(
            "emit(borrow alias.value.name)",
            "drop(alias);emit(borrow alias.value.name)",
        ),
        "E_MOVED",
    );
    let independent = s.replace(
        "emit(borrow alias.value.name)",
        "ref string name=borrow alias.value.name;drop(alias);drop(p);emit(name)",
    );
    let p = compile(&independent, m()).unwrap();
    assert_eq!(
        p.run(&scenario()).unwrap(),
        p.expanded("main").unwrap().run(&scenario()).unwrap()
    );
}
#[test]
fn cloned_carriers_and_multi_field_groups_retain_their_public_dependencies() {
    let cloned = shared().replace("relay(p)", "relay(clone(borrow p))");
    let p = compile(&cloned, m()).unwrap();
    assert_eq!(
        p.run(&scenario()).unwrap(),
        p.expanded("main").unwrap().run(&scenario()).unwrap()
    );
    let grouped = shared()
        .replace("['a,'b]", "['a]")
        .replace("right <- 'b", "right <- 'a");
    rejects(&grouped, "E_LOAN");
    let allowed = grouped.replace(
        "b=\"released b\";emit(first);",
        "emit(first);b=\"released b\";",
    );
    let p = compile(&allowed, m()).unwrap();
    assert_eq!(
        p.run(&scenario()).unwrap(),
        p.expanded("main").unwrap().run(&scenario()).unwrap()
    );
}
