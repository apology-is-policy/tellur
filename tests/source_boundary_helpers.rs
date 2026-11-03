use tellur::{
    compile,
    manifest::Manifest,
    runtime::{Event, ExecutionProgress, RunCompletion, Scenario},
};

fn manifest() -> Manifest {
    Manifest::from_json(include_str!("../playground/manifest.json")).unwrap()
}
fn scenario() -> Scenario {
    Scenario {
        entry: "main".into(),
        arguments: vec![],
        calls: vec![],
    }
}
const NORMAL: &str = include_str!("../playground/boundary_helpers.tellur");
const FAULT: &str = include_str!("fixtures/source_boundary_helper_fault.tellur");

fn observed(text: &str, messages: &[&str], status: Option<u8>) {
    let p = compile(text, manifest()).unwrap();
    for p in [p.clone(), p.expanded("main").unwrap()] {
        let s = scenario();
        let expected = p.run_completion(&s, 1000).unwrap();
        let events = match &expected {
            RunCompletion::Outcome(r) => {
                assert_eq!(status, None);
                &r.events
            }
            RunCompletion::Process(r) => {
                assert_eq!(status, Some(r.status));
                &r.events
            }
        };
        let actual: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                Event::Emit { message } => Some(message.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(actual, messages);
        for quota in [1, 3, usize::MAX] {
            let mut session = p.start_execution(&s, 1000);
            let mut polls = 0;
            loop {
                polls += 1;
                assert!(polls < 1000);
                assert!(matches!(session.advance(0), ExecutionProgress::Paused));
                match session.advance(quota) {
                    ExecutionProgress::Paused => {}
                    ExecutionProgress::Complete(result) => {
                        assert_eq!(
                            serde_json::to_value(result.unwrap()).unwrap(),
                            serde_json::to_value(&expected).unwrap()
                        );
                        break;
                    }
                    other => panic!("{other:?}"),
                }
            }
        }
    }
}

#[test]
fn uninterrupted_boundaries_preserve_queued_suspended_and_completed_children() {
    observed(
        NORMAL,
        &[
            "named recovery",
            "named work",
            "lexical recovery",
            "lexical work",
            "parent result",
            "child started",
            "named recovery",
            "named work",
            "lexical recovery",
            "lexical work",
            "child completed",
            "named recovery",
            "named work",
            "lexical recovery",
            "lexical work",
            "child result",
        ],
        None,
    );
}

#[test]
fn escaping_helper_fault_settles_original_owner_and_suppresses_sibling_contingency() {
    observed(
        FAULT,
        &[
            "result completed",
            "sibling started",
            "helper work",
            "helper contingency",
            "owner contingency",
        ],
        Some(23),
    );
    observed(
        &FAULT.replace("    yield;\n    helper(true);", "    helper(true);"),
        &["helper work", "helper contingency", "owner contingency"],
        Some(23),
    );
}

#[test]
fn helper_boundaries_keep_existing_child_capture_and_result_loans() {
    for (before, after) in [
        (
            "    auto child = spawn worker(borrow text);",
            "    auto child = spawn worker(borrow text);drop(text);",
        ),
        (
            "    auto moved = move child;",
            "    drop(text);auto moved = move child;",
        ),
        (
            "    [joined]: emit(result);",
            "    [joined]: drop(text);emit(result);",
        ),
    ] {
        let e = compile(&NORMAL.replace(before, after), manifest()).unwrap_err();
        assert_eq!(e.code, "E_LOAN", "{e}");
    }
}

#[test]
fn direct_boundaries_and_handoff_helpers_remain_gated() {
    let prefix = "async control worker() exits done {yield;exit done;}";
    for (decl, call) in [
        ("", "fault boundary {} contingency [] {escalate;}"),
        (
            "boundary b() {terminate;} contingency(string reason) {terminate;}",
            "supervise b();",
        ),
        (
            "control h() exits done {fault boundary {run worker();} contingency [] {escalate;}exit done;}",
            "h();",
        ),
        (
            "boundary b() {run worker();terminate;} contingency(string reason) {terminate;} control h() exits done {supervise b();exit done;}",
            "h();",
        ),
        (
            "control inner() exits done {run worker();exit done;} control h<t>() exits done {fault boundary {} contingency [] {escalate;}inner();exit done;}",
            "h<u32>();",
        ),
        (
            "control h() exits done {fault boundary {} contingency [] {escalate;}delta false {true=>yes,false=>no} [yes]:run worker();[yes|no]:exit done;}",
            "h();",
        ),
    ] {
        let s = format!(
            "{prefix}{decl} async control startup() exits done {{auto child=spawn worker();{call}await child;exit done;}}"
        );
        let e = compile(&s, manifest()).unwrap_err();
        assert_eq!(e.code, "E_ASYNC", "{s}\n{e}");
    }
}

#[test]
fn constructors_and_capture_use_checked_uninterrupted_bodies() {
    let source = r#"
    control request() exits ready {string text} {
        fault boundary {emit("request");} contingency [] {escalate;}
        exit ready {"value"};
    }
    struct box {string text;constructor() {
        delta request() {ready {string text}=>ready}
        [ready]:self.text=text;
    }}
    async control worker() exits done {emit("worker");exit done;}
    async control startup() exits done {
        auto child=spawn worker();auto value=box();emit(borrow value.text);drop(value);
        auto saved=capture request();delta saved {ready {string text}=>ready}
        [ready]:emit(text);await child;exit done;
    }
    control main() exits done {forward run startup();}
    "#;
    observed(
        source,
        &["request", "value", "request", "value", "worker"],
        None,
    );
}

#[test]
fn invocation_owned_wrappers_survive_local_recovery_without_entering_boundary() {
    observed(
        &NORMAL
            .replace(
                "worker(ref string text) exits ready { ref string value from text }",
                "worker(string text) exits ready { string value }",
            )
            .replace(
                "auto child = spawn worker(borrow text);",
                "auto child = spawn worker(clone(borrow text));auto wrapped=some {move child};",
            )
            .replace(
                "auto moved = move child;",
                "delta wrapped {some {auto moved}=>present,none=>exit done}",
            )
            .replace("delta await moved", "[present]:delta await moved"),
        &[
            "named recovery",
            "named work",
            "lexical recovery",
            "lexical work",
            "parent result",
            "child started",
            "named recovery",
            "named work",
            "lexical recovery",
            "lexical work",
            "child completed",
            "named recovery",
            "named work",
            "lexical recovery",
            "lexical work",
            "child result",
        ],
        None,
    );
}
