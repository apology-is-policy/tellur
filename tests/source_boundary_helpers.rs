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
fn direct_boundaries_remain_gated_while_helper_handoffs_are_checked() {
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
        if call.starts_with("fault boundary") || call.starts_with("supervise") {
            let e = compile(&s, manifest()).unwrap_err();
            assert_eq!(e.code, "E_ASYNC", "{s}\n{e}");
        } else {
            compile(&s, manifest()).unwrap_or_else(|e| panic!("{s}\n{e}"));
        }
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

#[test]
fn contingencies_keep_async_invocation_and_transitive_handoffs_unavailable() {
    let prefix = "async control pause() exits done {yield;exit done;}";
    for body in [
        "run pause();",
        "await pause();",
        "auto child=defer pause();",
        "auto child=spawn pause();",
        "yield;",
    ] {
        for named in [false, true] {
            let source = if named {
                format!(
                    "{prefix} boundary request() {{fault \"local\";}} contingency(string reason) {{{body}terminate;}}"
                )
            } else {
                format!(
                    "{prefix} async control work() exits done {{fault boundary {{fault \"local\";}} contingency [] {{{body}escalate;}}exit done;}}"
                )
            };
            let error = compile(&source, manifest()).unwrap_err();
            assert_eq!(error.code, "E_ASYNC", "{source}\n{error}");
        }
    }
    for helper in [
        "control helper() exits done {run pause();exit done;}",
        "control inner() exits done {run pause();exit done;} control helper() exits done {inner();exit done;}",
    ] {
        for named in [false, true] {
            let boundary = if named {
                "boundary request() {fault \"local\";} contingency(string reason) {helper();terminate;}"
            } else {
                "control work() exits done {fault boundary {fault \"local\";} contingency [] {helper();escalate;}exit done;}"
            };
            let source = format!("{prefix}{helper}{boundary}");
            let error = compile(&source, manifest()).unwrap_err();
            assert_eq!(error.code, "E_CONTINGENT", "{source}\n{error}");
        }
    }
}

#[test]
fn awaited_local_recovery_finishes_contingency_before_an_unrelated_sibling() {
    observed(
        include_str!("../playground/local_recovery_tasks.tellur"),
        &[
            "outside active",
            "inner active",
            "local recovery",
            "outside finished",
            "inner active",
            "inner finished",
            "helper returned",
        ],
        None,
    );
}

const HANDOFF: &str = include_str!("../playground/boundary_handoff_tasks.tellur");
#[test]
fn boundary_handoffs_preserve_local_recovery_child_ownership_and_result_origins() {
    observed(
        HANDOFF,
        &[
            "named handoff",
            "child started",
            "named recovery",
            "lexical handoff",
            "child finished",
            "parent result",
            "child result",
        ],
        None,
    );
    for (before, after) in [
        (
            "delta helper<string>(borrow parent)",
            "drop(text);delta helper<string>(borrow parent)",
        ),
        (
            "[checked]: emit(value);",
            "[checked]: drop(parent);emit(value);",
        ),
        (
            "[checked]: emit(value);",
            "[checked]: drop(text);emit(value);",
        ),
        (
            "[joined]: emit(result);",
            "[joined]: drop(text);emit(result);",
        ),
    ] {
        let source = HANDOFF.replace(before, after);
        assert_ne!(source, HANDOFF);
        let error = compile(&source, manifest()).unwrap_err();
        assert_eq!(error.code, "E_LOAN", "{source}\n{error}");
    }
}

#[test]
fn boundary_helpers_with_handoffs_preserve_exclusive_parent_results() {
    let source = HANDOFF
        .replace("ref t input", "ref mutable t input")
        .replace("ref t value from input", "ref mutable t value from input")
        .replace("string parent =", "mutable string parent =")
        .replace("borrow parent", "borrow mutable parent")
        .replace(
            "[checked]: emit(value);",
            "[checked]: deref value=\"edited parent\";emit(value);",
        );
    observed(
        &source,
        &[
            "named handoff",
            "child started",
            "named recovery",
            "lexical handoff",
            "child finished",
            "edited parent",
            "child result",
        ],
        None,
    );
    let error = compile(
        &source.replace("deref value=", "emit(borrow parent);deref value="),
        manifest(),
    )
    .unwrap_err();
    assert_eq!(error.code, "E_LOAN", "{error}");
}

#[test]
fn boundary_helpers_with_handoffs_do_not_discard_captured_outcomes() {
    let source = "async control pause() exits done {yield;exit done;} control helper() exits ready {string text} {fault boundary {run pause();} contingency [] {escalate;}exit ready {\"value\"};} async control startup() exits done {auto child=spawn pause();auto result=capture helper();drop(result);await child;exit done;}";
    let error = compile(source, manifest()).unwrap_err();
    assert_eq!(error.code, "E_OUTCOME", "{error}");
}

#[test]
fn boundary_helpers_with_handoffs_require_fault_dispositions_for_retained_owners() {
    let mut m = manifest();
    m.primitives
        .iter_mut()
        .find(|p| p.name == "close")
        .unwrap()
        .contingent = true;
    let close =
        "delta close(h) {closed=>closed,failed {close_error e}=>failed} [closed|failed]:terminate;";
    for named in [false, true] {
        let helper = if named {
            format!(
                "boundary request(handle h) retains h {{run pause();{close}}} contingency(string reason) {{{close}}} control helper(handle h) exits done {{supervise request(h);exit done;}}"
            )
        } else {
            format!(
                "control helper(handle h) exits done {{fault boundary {{run pause();{}}} contingency [h] {{{}}} exit done;}}",
                close.replace("terminate;", "emit(\"closed\");"),
                close.replace("terminate;", "escalate;")
            )
        };
        let caller = "async control startup(handle h) exits done {auto child=spawn pause();helper(h);await child;exit done;}";
        let prefix = "async control pause() exits done {yield;exit done;}";
        let source = format!("{prefix}{helper}{caller}");
        let error = compile(&source, m.clone()).unwrap_err();
        assert_eq!(error.code, "E_ASYNC", "{named}: {error}\n{source}");
        assert!(error.message.contains("fault dispositions"), "{error}");
        compile(
            &source.replace("helper(h);await child;", "await child;helper(h);"),
            m.clone(),
        )
        .unwrap();
    }
}

#[test]
fn trait_and_generic_boundary_helpers_preserve_returned_loans() {
    let start = HANDOFF.find("control helper<t>").unwrap();
    let end = HANDOFF[start..].find("async control worker").unwrap() + start;
    let replacement = r#"
        trait reader {control read(ref self input) exits ready {ref self value from input} faults;}
        implement reader for string {
            control read(ref self input) exits ready {ref self value from input} faults {
                supervise request();
                fault boundary {run step("lexical handoff");} contingency [] {escalate;}
                exit ready {input};
            }
        }
        control helper<t:reader>(ref t input) exits ready {ref t value from input} {
            forward reader<t>.read(input);
        }
    "#;
    let source = format!("{}{}{}", &HANDOFF[..start], replacement, &HANDOFF[end..]);
    observed(
        &source,
        &[
            "named handoff",
            "child started",
            "named recovery",
            "lexical handoff",
            "child finished",
            "parent result",
            "child result",
        ],
        None,
    );
    let error = compile(
        &source.replace(
            "[checked]: emit(value);",
            "[checked]: drop(parent);emit(value);",
        ),
        manifest(),
    )
    .unwrap_err();
    assert_eq!(error.code, "E_LOAN", "{error}");
}

#[test]
fn owned_child_wrappers_survive_helper_boundary_handoffs_but_not_direct_boundaries() {
    let source=HANDOFF.replace("worker(ref string text) exits ready { ref string value from text }", "worker(string text) exits ready { string value }")
        .replace("spawn worker(borrow text);", "spawn worker(text);auto wrapped=some {move child};")
        .replace("delta await child", "delta wrapped {some {auto moved}=>present,none=>exit done} [present]:delta await moved");
    observed(
        &source,
        &[
            "named handoff",
            "child started",
            "named recovery",
            "lexical handoff",
            "child finished",
            "parent result",
            "child result",
        ],
        None,
    );
    let error = compile(
        &source.replace(
            "delta helper<string>",
            "fault boundary {} contingency [] {escalate;}delta helper<string>",
        ),
        manifest(),
    )
    .unwrap_err();
    assert_eq!(error.code, "E_ASYNC", "{error}");
}
