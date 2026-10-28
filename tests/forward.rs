mod support;
use tellur::{
    compile,
    ir::{Action, Value},
    manifest::Manifest,
    runtime::Scenario,
};
fn m() -> Manifest {
    Manifest::from_json(include_str!("../playground/manifest.json")).unwrap()
}
fn rejects(s: &str, code: &str) {
    let e = compile(s, m()).unwrap_err();
    assert_eq!(e.code, code, "{e}");
}
fn run(s: &str, args: Vec<Value>) -> tellur::runtime::Execution {
    let p = compile(s, m()).unwrap();
    let scenario = Scenario {
        entry: "main".into(),
        arguments: args,
        calls: vec![],
    };
    let r = p.run(&scenario).unwrap();
    assert_eq!(r, p.expanded("main").unwrap().run(&scenario).unwrap());
    r
}
const PROGRAM: &str = r#"control main() exits done {
 display("first"); forward display("second");
}
control display(string text) exits done {emit(text);exit done;}"#;
#[test]
fn single_outcome_calls_and_tail_forward_match_explicit_wiring() {
    let compact = run(PROGRAM, vec![]);
    let explicit=PROGRAM.replace("display(\"first\"); forward display(\"second\");","exit delta display(\"first\") {done=>[first_displayed]:delta display(\"second\") {done=>done {}}};");
    assert_eq!(compact, run(&explicit, vec![]));
    assert_eq!(compact.events.len(), 2);
    let p = compile(PROGRAM, m()).unwrap().expanded("main").unwrap();
    let emits = p
        .region("main")
        .unwrap()
        .code
        .iter()
        .filter(|i| matches!(i.action, Action::Emit(_)))
        .collect::<Vec<_>>();
    assert_eq!(emits.len(), 2);
    assert_eq!(
        emits[0].provenance.definition,
        emits[1].provenance.definition
    );
    assert_ne!(emits[0].provenance.calls, emits[1].provenance.calls);
}
#[test]
fn calls_never_discard_additional_outcomes_or_payloads() {
    rejects(
        &PROGRAM.replace("exits done {emit", "exits done,failed {emit"),
        "E_DISPOSITION",
    );
    rejects(
        "control main() exits auto {u32_text(3);exit done;}",
        "E_DISPOSITION",
    );
    rejects(
        "control main() exits done {missing();exit done;}",
        "E_DISPOSITION",
    );
    rejects("control main() exits auto {forward 7;}", "E_FORWARD");
    rejects(
        "control main() exits auto {forward unknown();}",
        "E_FORWARD",
    );
}
#[test]
fn forward_checks_closed_target_labels_arity_and_exact_types() {
    let s = "control choose() exits ready {u32 n} {exit ready {7};} control main() exits ready {u32 n},empty {forward choose();}";
    assert_eq!(run(s, vec![]).fields, vec![Value::U32(7)]);
    rejects(
        &s.replace("exits ready {u32 n},empty", "exits renamed {u32 n},empty"),
        "E_OUTCOME",
    );
    rejects(
        &s.replace("exits ready {u32 n},empty", "exits ready,empty"),
        "E_ARITY",
    );
    rejects(
        &s.replace("exits ready {u32 n},empty", "exits ready {string n},empty"),
        "E_TYPE",
    );
    rejects(
        &s.replace(
            "forward choose();",
            "auto saved=capture choose();forward choose();",
        ),
        "E_OUTCOME",
    );
}
#[test]
fn arm_identity_forwards_hidden_payloads_into_tail_or_stored_adapters() {
    let s = r#"outcome message {ready {string text},empty}
 control main(bool flag) exits auto {
 message input=delta flag {true=>ready {"hello"},false=>empty {}};
 auto saved=delta input {message.ready=>[accepted]:forward,message.empty=>missing {}};
 exit delta saved {ready=>forward,missing=>missing {}};
 }"#;
    assert_eq!(
        run(s, vec![Value::Bool(true)]).fields,
        vec![Value::String("hello".into())]
    );
    assert_eq!(run(s, vec![Value::Bool(false)]).outcome, "missing");
    rejects(
        &s.replace("message.ready=>", "message.ready {u32 wrong}=>"),
        "E_TYPE",
    );
    rejects(
        &s.replace("ready=>forward,missing=>missing {}", "ready=>forward"),
        "E_ARMS",
    );
}
#[test]
fn forwarded_references_keep_origins_and_cannot_escape_local_owners() {
    let s = r#"control first(&vector<string> words) exits auto {forward vector_get(words,0);}
 control main() exits auto {vector<string> words=["hello"];exit delta first(&words) {found {auto word}=>done {clone(word)},out_of_bounds=>empty {}};}"#;
    assert_eq!(run(s, vec![]).fields, vec![Value::String("hello".into())]);
    rejects(
        &s.replace("found {auto word}=>done {clone(word)}", "found=>forward"),
        "E_LIFETIME",
    );
    let p=compile("control borrow_text(&string text) exits auto {exit found {text};} control proxy(&string owner) exits auto {exit delta borrow_text(owner) {found=>forward};}",m()).unwrap();
    assert_eq!(
        p.region("proxy").unwrap().borrow_returns[&("found".into(), 0)][0].parameter,
        0
    );
}
#[test]
fn forwarding_resources_preserves_payload_and_unrelated_obligations() {
    let s = "control transfer(handle file) exits ready {handle file} {exit ready {file};} control main(handle file) exits auto {forward transfer(file);}";
    let h = Value::Opaque {
        ty: "handle".into(),
        token: "f".into(),
    };
    assert_eq!(run(s, vec![h.clone()]).fields, vec![h]);
    rejects(
        &s.replace("main(handle file)", "main(handle file,handle other)"),
        "E_RESOURCE",
    );
    compile(
        &s.replace(
            "forward transfer(file);",
            "exit delta transfer(file) {ready=>forward};",
        ),
        m(),
    )
    .unwrap();
}
#[test]
fn statement_calls_keep_outer_selection_and_expose_no_path_bindings() {
    let s = r#"control say() exits done {emit("yes");exit done;}
 control main(bool flag) exits done {delta flag {true=>yes,false=>no} [yes]:say(); [entry]:exit done;}"#;
    assert_eq!(run(s, vec![Value::Bool(false)]).events.len(), 0);
    assert_eq!(run(s, vec![Value::Bool(true)]).events.len(), 1);
    rejects(
        &s.replace("[entry]:exit done;", "[done]:exit done;"),
        "E_PATH",
    );
    run(
        "control main() exits done {mutable string text=\"before\";change(&mutable text);emit(text);exit done;} control change(&mutable string text) exits done {*text=\"after\";exit done;}",
        vec![],
    );
}
#[test]
fn forward_function_names_and_forward_exit_labels_remain_valid() {
    run(
        "control forward() exits done {exit done;} control main() exits auto {forward forward();}",
        vec![],
    );
    run(
        "control main() exits auto {exit delta true {true=>forward {},false=>other {}};}",
        vec![],
    );
    run(
        "control main() exits auto {exit delta (true) {true=>forward,false=>forward};}",
        vec![],
    );
}
#[test]
fn native_forwarding_preserves_effects_at_o0_o2_and_after_expansion() {
    let p = compile(PROGRAM, m()).unwrap();
    let root = std::env::temp_dir().join(format!("path-forward-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    for (index, program) in [p.clone(), p.expanded("main").unwrap()].iter().enumerate() {
        for opt in [0, 2] {
            let binary = root.join(format!("forward-{index}-{opt}"));
            support::scenario_executable(program)
                .build_native("main", &binary, opt)
                .unwrap();
            let actual = std::process::Command::new(&binary)
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
                serde_json::to_value(run(PROGRAM, vec![])).unwrap()
            );
        }
    }
    std::fs::remove_dir_all(root).unwrap();
}
