mod support;
use tellur::{compile, manifest::Manifest, runtime::Scenario};
fn manifest() -> Manifest {
    Manifest::from_json(include_str!("../playground/manifest.json")).unwrap()
}
fn same(source: &str) -> tellur::CheckedProgram {
    let p = compile(source, manifest()).unwrap();
    let s = Scenario {
        entry: "main".into(),
        arguments: vec![],
        calls: vec![],
    };
    assert_eq!(
        p.run(&s).unwrap(),
        p.expanded("main").unwrap().run(&s).unwrap()
    );
    p
}
const NAMED: &str = r#"
outcome reply['a, 'b] { left {ref string text <- 'a}, right {ref string text <- 'b}, empty }
control choose(ref string a, ref string b, bool flag) exits reply[a, b] {
    exit delta flag {true=>left {a}, false=>right {b}};
}
control relay(reply input) exits ready {reply[input['a], input['b]] value} {exit ready {input};}
control main() exits done {
    mutable string a="a"; mutable string b="b";
    reply saved=capture choose(borrow a,borrow b,true);
    delta relay(saved) {ready {auto value}=>received}
    [received]:delta value {left {auto left_text}=>left,right {auto right_text}=>right,empty=>empty}
    [left]:b="released";emit(left_text);
    [right]:a="released";emit(right_text);
    [entry]:a="finished";b="finished";exit done;
}
"#;
#[test]
fn named_completion_capture_and_owned_boundaries_preserve_groups() {
    let p = same(NAMED);
    same(&NAMED.replace("exits reply[a, b]", "exits reply"));
    assert_eq!(p.region("choose").unwrap().borrow_returns.len(), 2);
    let bad = NAMED.replace(
        "b=\"released\";emit(left_text)",
        "a=\"too early\";emit(left_text)",
    );
    assert_eq!(compile(&bad, manifest()).unwrap_err().code, "E_LOAN");
    assert_eq!(
        compile(&NAMED.replace("reply[a, b]", "reply[b, a]"), manifest())
            .unwrap_err()
            .code,
        "E_LIFETIME"
    );
}
const OPTIONAL: &str = r#"
control choose(ref mutable string input, bool present) exits ready {option<ref mutable string>[input] value} {
    exit delta present {true=>ready {some {input}},false=>ready {none {}}};
}
control relay(option<ref mutable string> input) exits ready {option<ref mutable string>[input['source]] value} {exit ready {input};}
control main() exits done {
    mutable string owner="before";
    delta choose(borrow mutable owner,true) {ready {auto value}=>chosen}
    [chosen]:delta relay(value) {ready {auto second}=>received}
    [received]:delta second {some {auto text}=>some,none=>none}
    [some]:deref text="after";emit(text);
    [entry]:owner="released";emit(borrow owner);exit done;
}
"#;
#[test]
fn optional_exclusive_references_transfer_or_release_on_absence() {
    same(OPTIONAL);
    same(&OPTIONAL.replace("owner,true)", "owner,false)"));
    assert_eq!(
        compile(
            &OPTIONAL.replace("deref text=", "owner=\"too early\";deref text="),
            manifest()
        )
        .unwrap_err()
        .code,
        "E_LOAN"
    );
}
#[test]
fn inferred_option_results_and_nested_optional_schemas() {
    same(
        r#"control produce(ref string input) exits auto {auto value=some {some {input}};exit ready {value};}
control relay(option<option<ref string>> input) exits ready... {exit ready {input};}
control main() exits done {string owner="hello";delta produce(borrow owner) {ready {auto value}=>produced}
[produced]:delta relay(value) {ready {auto second}=>received}
[received]:delta second {some {auto inner}=>some,none=>none}
[some]:delta inner {some {auto text}=>text,none=>empty}
[text]:emit(text);[entry]:exit done;}"#,
    );
}
#[test]
fn named_schemas_validate_declarations_and_contract_bindings() {
    for source in [
        "outcome bad['a] {found {ref string text},empty}",
        "outcome bad['a] {found {ref string text <- 'missing},empty}",
        "outcome bad['a,'a] {found {ref string text <- 'a}}",
        "outcome bad {found {ref string text <- 'a}}",
        "outcome bad['a] {found {string text <- 'a}}",
        "outcome bad<t>['a] {found {ref t text <- 'unknown}}",
    ] {
        assert_eq!(
            compile(source, manifest()).unwrap_err().code,
            "E_ORIGIN",
            "{source}"
        );
    }
    assert_eq!(
        compile(&NAMED.replace("reply[a, b]", "reply[a]"), manifest())
            .unwrap_err()
            .code,
        "E_ORIGIN"
    );
    assert_eq!(
        compile(
            "control bad(ref string input) exits done[input] {exit done;}",
            manifest()
        )
        .unwrap_err()
        .code,
        "E_ORIGIN"
    );
    assert_eq!(
        compile(
            "outcome empty {done} control bad(ref string input) exits empty[input] {exit done;}",
            manifest()
        )
        .unwrap_err()
        .code,
        "E_ORIGIN"
    );
}
#[test]
fn absent_option_needs_explicit_origins_and_escaping_locals_are_rejected() {
    let s = "control absent(ref string input) exits ready... {option<ref string> value=none {};exit ready {value};}";
    assert_eq!(compile(s, manifest()).unwrap_err().code, "E_ORIGIN");
    compile(
        &s.replace("ready...", "ready {option<ref string>[input] value}"),
        manifest(),
    )
    .unwrap();
    let s = "control bad() exits ready... {string local=\"x\";auto value=some {borrow local};exit ready {value};}";
    assert_eq!(compile(s, manifest()).unwrap_err().code, "E_LIFETIME");
}

#[test]
fn options_inherit_named_payload_groups_and_keep_independent_last_uses() {
    let s = include_str!("fixtures/outcome_origins.tellur");
    same(s);
    same(&s.replace("body, true)", "body, false)"));
    let bad = s.replace(
        "emit(fields.title);",
        "title=\"too early\";emit(fields.title);",
    );
    assert_eq!(compile(&bad, manifest()).unwrap_err().code, "E_LOAN");
    let bad = s.replace(
        "title = \"title released independently\"",
        "body = \"too early\"",
    );
    assert_eq!(compile(&bad, manifest()).unwrap_err().code, "E_LOAN");
    let bad = s.replace(
        "[available]: delta result { found { auto fields } => found, missing => missing }",
        "[available]: drop(result); path found=available; path missing=absent;",
    );
    assert!(compile(&bad, manifest()).is_err());
}

#[test]
fn generic_named_outcomes_compose_with_grouped_records_and_options() {
    let s = r#"outcome reply_view<t>['a] {found {ref t text <- 'a},empty}
struct envelope<t>['a] {option<reply_view<t>>['a] value;}
control produce<t>(ref t input) exits reply_view<t>[input] {exit found {input};}
control wrap<t>(reply_view<t> input) exits ready {envelope<t>[input['a]] value} {exit ready {envelope<t> {some {input}}};}
control main() exits done {string owner="hello";reply_view<string> saved=capture produce<string>(borrow owner);
delta wrap<string>(saved) {ready {auto box}=>wrapped}
[wrapped]:delta box.value {some {auto result}=>some,none=>none}
[some]:delta result {found {auto text}=>found,empty=>empty}
[found]:emit(text);[entry]:exit done;}"#;
    same(s);
}

#[test]
fn named_outcome_obligations_and_exclusive_aliases_survive_options() {
    let s = "outcome message['a] {found {ref string text <- 'a},empty} control main() exits done {string owner=\"x\";option<message> saved=some {message.found {borrow owner}};drop(saved);exit done;}";
    assert_eq!(compile(s, manifest()).unwrap_err().code, "E_RESOURCE");
    let s = "outcome edits['a,'b] {ready {ref mutable string left <- 'a,ref mutable string right <- 'b}} control invalid(ref mutable string input) exits edits[input,input] {exit ready {input,input};}";
    assert!(compile(s, manifest()).is_err());
    let s = "outcome message['a] {found {ref string text <- 'a}} trait reader {control peek(ref self input) exits message[input];}";
    compile(s, manifest()).unwrap();
    assert_eq!(
        compile(&s.replace("message[input]", "message"), manifest())
            .unwrap_err()
            .code,
        "E_LIFETIME",
    );
}

#[test]
fn optional_input_selectors_return_only_the_contained_source() {
    let source = r#"control read_optional(option<ref string> input) exits found {ref string text from input['source]},empty {
delta input {some {auto text}=>some,none=>none}[some]:exit found {text};[none]:exit empty;}
control main() exits done {mutable string owner="hello";option<ref string> saved=some {borrow owner};
delta read_optional(saved) {found {auto text}=>found,empty=>empty}[found]:emit(text);[entry]:owner="released";exit done;}"#;
    same(source);
    same(&source.replace("saved=some {borrow owner}", "saved=none {}"));
}

#[test]
fn native_named_outcome_option_boundaries_match_both_branches_and_expansion() {
    let root = std::env::temp_dir().join(format!("tellur-outcome-origins-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    for present in [true, false] {
        let source = include_str!("fixtures/outcome_origins.tellur")
            .replace("body, true)", &format!("body, {present})"));
        let p = same(&source);
        let scenario = Scenario {
            entry: "main".into(),
            arguments: vec![],
            calls: vec![],
        };
        let expected = serde_json::to_value(p.run(&scenario).unwrap()).unwrap();
        for expanded in [false, true] {
            let p = if expanded {
                p.expanded("main").unwrap()
            } else {
                p.clone()
            };
            for opt in [0, 2] {
                let binary = root.join(format!("outcome-{present}-{expanded}-{opt}"));
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
                    expected
                );
            }
        }
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_surviving_optional_copy_keeps_the_owner_borrowed_after_a_call() {
    let source = r#"control keep(option<ref string> input) exits ready... {exit ready {input};}
control main() exits done {mutable string owner="hello";option<ref string> first=some {borrow owner};
option<ref string> retained=first;delta keep(first) {ready {auto returned}=>returned}
[returned]:drop(returned);owner="too early";delta retained {some {auto text}=>some,none=>none}
[some]:emit(text);[entry]:exit done;}"#;
    assert_eq!(compile(source, manifest()).unwrap_err().code, "E_LOAN");
    same(&source.replace("owner=\"too early\";", ""));
}
