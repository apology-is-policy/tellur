mod support;
use tellur::{compile, ir::Value, manifest::Manifest, runtime::Scenario};
fn manifest() -> Manifest {
    Manifest::from_json(include_str!("../playground/manifest.json")).unwrap()
}
fn rejects(source: &str, code: &str) {
    let e = compile(source, manifest()).unwrap_err();
    assert_eq!(e.code, code, "{e}");
}
fn run(source: &str, flag: bool) -> tellur::runtime::Execution {
    let p = compile(source, manifest()).unwrap();
    let s = Scenario {
        entry: "main".into(),
        arguments: vec![Value::Bool(flag)],
        calls: vec![],
    };
    let r = p.run(&s).unwrap();
    assert_eq!(r, p.expanded("main").unwrap().run(&s).unwrap());
    r
}
const PROGRAM: &str = r#"
control main(bool flag) exits message {
    auto first=capture choose(flag);
    message second=capture fallback();
    vector<message> queue=[first,second];
    delta vector_take(queue) {item {auto result,auto rest}=>available,empty=>absent}
    [available]: delta discard(rest) {done=>drained}
    [drained]: exit delta forward(result) {ready {auto text}=>ready {text},empty=>empty {}};
    [absent]: exit empty;
}
control forward(message result) exits message {exit result;}
control discard(vector<message> queue) exits done {
    loop drain(vector<message> pending=queue) {
        delta vector_take(pending) {item {auto value,auto rest}=>item,empty=>empty}
        [item]: delta value {ready {auto text}=>ready,empty=>missing}
        [ready|missing]: again drain(rest);
        [empty]: exit done;
    }
}
control choose(bool flag) exits message {emit("choose");exit delta flag {true=>ready {"hello"},false=>empty {}};}
control fallback() exits message {emit("fallback");exit empty;}
outcome message {ready {string text},empty}
"#;
#[test]
fn producers_share_one_nominal_contract_and_container_obligations() {
    for flag in [true, false] {
        let r = run(PROGRAM, flag);
        assert_eq!(r.outcome, if flag { "ready" } else { "empty" });
        assert_eq!(r.events.len(), 2);
    }
    let p = compile(PROGRAM, manifest()).unwrap();
    assert!(
        p.region("main")
            .unwrap()
            .slots
            .values()
            .any(|s| s.name == "first" && s.ty == "message")
    );
    assert!(p.manifest().linear("message"));
    assert!(!p.manifest().is_copy("message"));
    rejects(&PROGRAM.replace("exit result;", "exit empty;"), "E_OUTCOME");
}
#[test]
fn named_adapters_contextual_constructors_and_qualified_constructors() {
    let source = r#"outcome numbers {ready {vector<u32> values},empty}
    control main(bool flag) exits numbers {
        numbers selected=delta flag {true=>ready {[1,2]},false=>empty {}};
        numbers direct=ready {[3]};
        auto qualified=numbers.empty {};
        delta direct {ready {auto values}=>ready,empty=>empty}
        [entry]:delta qualified {ready {auto unused}=>unused,empty=>absent}
        [entry]:exit selected;
    }"#;
    assert_eq!(run(source, true).outcome, "ready");
    assert_eq!(run(source, false).outcome, "empty");
    rejects(
        &source.replace("numbers direct=ready", "auto direct=ready"),
        "E_NAME",
    );
}
#[test]
fn direct_constructors_carry_handling_obligation_even_when_nullary() {
    let prefix = "outcome signal {ready,empty} ";
    for body in [
        "signal value=ready {};exit done;",
        "signal value=ready {};drop(value);exit done;",
        "signal value=ready {};auto copy=clone(&value);exit value;",
    ] {
        rejects(
            &format!("{prefix}control main() exits auto {{{body}}}"),
            "E_OUTCOME",
        );
    }
    compile(
        &format!("{prefix}control main() exits auto {{auto value=signal.ready {{}};exit value;}}"),
        manifest(),
    )
    .unwrap();
}
#[test]
fn different_named_types_are_not_interchangeable_by_shape() {
    let source = "outcome first {ready} outcome second {ready} control main() exits auto {first value=ready {};second wrong=value;exit wrong;}";
    rejects(source, "E_TYPE");
    compile(
        &source.replace(
            "second wrong=value;",
            "second wrong=delta value {ready=>ready {}};",
        ),
        manifest(),
    )
    .unwrap();
}
#[test]
fn capture_into_named_contract_allows_explicit_widening_but_no_narrowing() {
    let source = r#"outcome broad {ready {u32 value},empty,failed}
    control main(bool flag) exits broad {broad result=capture choose(flag);exit result;}
    control choose(bool flag) exits auto {exit delta flag {true=>ready {7},false=>empty {}};}"#;
    assert_eq!(run(source, true).fields, vec![Value::U32(7)]);
    rejects(&source.replace(",empty,failed", ",failed"), "E_OUTCOME");
    rejects(&source.replace("u32 value", "string value"), "E_TYPE");
    rejects(&source.replace("ready {u32 value}", "ready"), "E_ARITY");
    rejects(
        &source.replace(
            "exit result;",
            "exit delta result {ready {auto n}=>ready {n},empty=>empty {}};",
        ),
        "E_ARMS",
    );
}
#[test]
fn resource_payloads_still_need_completion_after_handling() {
    let source = "outcome opened {ready {handle file},empty} control main(handle file) exits auto {opened result=ready {file};exit delta result {ready {auto owned}=>done {owned},empty=>empty {}};}";
    compile(source, manifest()).unwrap();
    rejects(&source.replace("done {owned}", "done {}"), "E_RESOURCE");
    rejects(
        &source.replace("ready {file};", "ready {file};auto twice=file;"),
        "E_MOVED",
    );
}
#[test]
fn invalid_named_layouts_are_rejected_before_ownership_analysis() {
    for declaration in [
        "outcome message {}",
        "outcome message {ready,ready}",
        "outcome message {ready {u32 n,u32 n}}",
        "outcome message {ready {missing n}}",
        "outcome message {ready {message next}}",
        "outcome message {ready {vector<message> next}}",
        "outcome message {ready {other next}} outcome other {ready {message next}}",
        "outcome string {ready}",
        "outcome message {ready} outcome message {empty}",
    ] {
        rejects(
            &format!("{declaration} control main() exits done {{exit done;}}"),
            "E_MANIFEST",
        );
    }
    compile(
        "outcome outer {wrapped {inner value}} outcome inner {ready}",
        manifest(),
    )
    .unwrap();
}
#[test]
fn named_contracts_do_not_steal_explicit_inline_exits_or_option_constructors() {
    compile(
        "outcome done {ready} control main() exits {done} {exit done;}",
        manifest(),
    )
    .unwrap();
    compile(
        "outcome done {ready} control main() exits done {} {exit done;}",
        manifest(),
    )
    .unwrap();
    compile(
        "outcome done {ready} control main() exits done() {exit done;}",
        manifest(),
    )
    .unwrap();
    rejects(
        "outcome done {ready} control main() exits done {exit done;}",
        "E_EXIT",
    );
    compile("outcome maybe {some {u32 n},none} control main() exits auto {maybe named=some {1};auto option=some {2};delta named {some {auto n}=>present,none=>absent} [entry]:exit done {option};}",manifest()).unwrap();
}
#[test]
fn legacy_producer_type_can_still_be_selected_explicitly() {
    compile("outcome message {ready} control choose() exits message {exit ready;} control main() exits auto {outcome<choose> result=capture choose();exit result;}",manifest()).unwrap();
}
#[test]
fn native_named_contracts_match_original_and_expanded_graphs() {
    let p = compile(PROGRAM, manifest()).unwrap();
    let root = std::env::temp_dir().join(format!("path-named-outcomes-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    for (index, program) in [p.clone(), p.expanded("main").unwrap()].iter().enumerate() {
        for opt in [0, 2] {
            let binary = root.join(format!("named-{index}-{opt}"));
            support::scenario_executable(program)
                .build_native("main", &binary, opt)
                .unwrap();
            for flag in [true, false] {
                let actual = std::process::Command::new(&binary)
                    .args(["--json", "--", if flag { "true" } else { "false" }])
                    .output()
                    .unwrap();
                assert!(
                    actual.status.success(),
                    "{}",
                    String::from_utf8_lossy(&actual.stderr)
                );
                assert_eq!(
                    serde_json::from_slice::<serde_json::Value>(&actual.stdout).unwrap(),
                    serde_json::to_value(run(PROGRAM, flag)).unwrap()
                );
            }
        }
    }
    std::fs::remove_dir_all(root).unwrap();
}
