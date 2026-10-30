mod support;
use tellur::{
    compile,
    ir::Value,
    manifest::Manifest,
    runtime::{Event, Scenario},
};
fn manifest() -> Manifest {
    Manifest::from_json(include_str!("../playground/manifest.json")).unwrap()
}
fn checked(source: &str) -> tellur::CheckedProgram {
    compile(source, manifest()).unwrap()
}
fn scenario() -> Scenario {
    Scenario {
        entry: "main".into(),
        arguments: vec![],
        calls: vec![],
    }
}
fn rejects(source: &str, code: &str) {
    assert_eq!(
        compile(source, manifest()).unwrap_err().code,
        code,
        "{source}"
    );
}
const REQUEST: &str = r#"
control work(bool fail) exits done {
    delta fail { true => bad, false => good }
    [bad]: fault "broken request";
    [good]: exit done;
}
@contingent
control report(ref string context, string reason) exits done {
    emit(clone(context)); emit(reason); exit done;
}
boundary request(bool fail, string context) retains context {
    work(fail);
    emit("normal request");
    terminate;
}
contingency(string reason) {
    report(borrow context, reason);
    terminate;
}
control main() exits done {
    supervise request(true, "retained context");
    supervise request(false, "second context");
    emit("supervisor continued");
    exit done;
}
"#;
#[test]
fn failure_retires_request_and_only_supervisor_continues() {
    let p = checked(REQUEST);
    assert!(!p.region("main").unwrap().faults);
    let run = p.run(&scenario()).unwrap();
    assert_eq!(run.outcome, "done");
    let messages: Vec<_> = run
        .events
        .iter()
        .filter_map(|e| {
            if let Event::Emit { message } = e {
                Some(message.as_str())
            } else {
                None
            }
        })
        .collect();
    assert_eq!(
        messages,
        [
            "retained context",
            "broken request",
            "normal request",
            "supervisor continued"
        ]
    );
    assert_eq!(
        run.events
            .iter()
            .filter(|e| matches!(e, Event::Fault { .. }))
            .count(),
        1
    );
    p.verify().unwrap();
}
#[test]
fn contingency_permissions_are_transitive_and_public_contractual() {
    checked(
        "control helper() exits done {exit done;} @contingent control main() exits done {helper();exit done;}",
    );
    rejects(
        "pub control helper() exits done {exit done;} @contingent control main() exits done {helper();exit done;}",
        "E_CONTINGENT",
    );
    checked(
        "@contingent pub control helper() exits done {exit done;} @contingent control main() exits done {helper();exit done;}",
    );
    rejects("control main() exits done {terminate;}", "E_BOUNDARY");
    rejects(
        "control main() exits done {supervise other();} control other() exits done {exit done;}",
        "E_BOUNDARY",
    );
    rejects(
        &REQUEST.replace("supervise request(true", "request(true"),
        "E_BOUNDARY",
    );
    rejects(
        &REQUEST.replace("retains context {", "retains context { drop(context);"),
        "E_BOUNDARY_RETAIN",
    );
    rejects(
        &REQUEST.replace(
            "boundary request(bool fail, string context)",
            "boundary request(bool fail, ref string context)",
        ),
        "E_BOUNDARY",
    );
}
#[test]
fn escalation_preserves_original_cause_and_abandons_supervisor() {
    let p = checked(&REQUEST.replace(
        "report(borrow context, reason);\n    terminate;",
        "report(borrow context, reason);\n    escalate;",
    ));
    assert!(p.region("main").unwrap().faults);
    let failure = p.run_detailed(&scenario(), 1000).unwrap_err();
    assert_eq!(failure.diagnostic.message, "broken request");
    assert_eq!(failure.report.cause.as_ref().unwrap().boundary, "request");
    assert!(
        !failure
            .events
            .iter()
            .any(|e| matches!(e,Event::Emit{message} if message=="supervisor continued"))
    );
}
#[test]
fn native_boundary_matches_reference_and_storage_survives_repeated_calls() {
    use std::{fs, process::Command};
    let p = checked(REQUEST);
    let root = std::env::temp_dir().join(format!("tellur-boundaries-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let input = root.join("scenario.json");
    fs::write(&input, serde_json::to_vec(&scenario()).unwrap()).unwrap();
    let binary = root.join("run");
    support::scenario_executable(&p)
        .build_native("main", &binary, 0)
        .unwrap();
    let output = Command::new(binary)
        .args(["--scenario", input.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let actual: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        actual,
        serde_json::to_value(p.run(&scenario()).unwrap()).unwrap()
    );
    fs::remove_dir_all(root).unwrap();
}
#[test]
fn caller_borrows_remain_valid_across_activation_storage() {
    let p = checked(
        "control peek(ref string value) exits found {ref string value from value} {exit found {value};} control main() exits done {string value=\"kept\";delta peek(borrow value){found {auto reference}=>read}[read]:emit(clone(reference));exit done;}",
    );
    assert_eq!(
        p.run(&scenario()).unwrap().events,
        vec![Event::Emit {
            message: "kept".into()
        }]
    );
}

#[test]
fn retained_must_complete_resource_requires_handover_and_contingent_cleanup() {
    let source = "boundary request(handle owned) retains owned { fault \"broken\"; } contingency(string reason) { delta close(owned) {closed=>closed,failed{close_error error}=>failed} [closed|failed]:terminate; } control main(handle owned) exits done {supervise request(owned);exit done;}";
    assert_eq!(
        compile(source, manifest()).unwrap_err().code,
        "E_CONTINGENT"
    );
    let mut m = manifest();
    m.primitives
        .iter_mut()
        .find(|p| p.name == "close")
        .unwrap()
        .contingent = true;
    let p = compile(source, m.clone()).unwrap();
    let s = Scenario {
        entry: "main".into(),
        arguments: vec![Value::Opaque {
            ty: "handle".into(),
            token: "owned".into(),
        }],
        calls: vec![tellur::runtime::ScriptCall {
            operation: "close".into(),
            outcome: "closed".into(),
            fields: vec![],
        }],
    };
    assert_eq!(p.run(&s).unwrap().outcome, "done");
    assert_eq!(
        compile(&source.replace("retains owned", ""), m.clone())
            .unwrap_err()
            .code,
        "E_NAME"
    );
    assert_eq!(compile(&source.replace("delta close(owned) {closed=>closed,failed{close_error error}=>failed} [closed|failed]:", ""),m).unwrap_err().code,"E_RESOURCE");
}

#[test]
fn trait_subset_and_generic_witnesses_enforce_contingent_permission() {
    let source = "trait cleanup {@contingent control finish(self value) exits done;} implement cleanup for string {control finish(self value) exits done {emit(value);exit done;}} @contingent control finish<t:cleanup>(t value) exits done {cleanup<t>.finish(value);exit done;} control main() exits done {finish<string>(\"finished\");exit done;}";
    checked(source);
    rejects(
        &source.replace("trait cleanup {@contingent", "trait cleanup {"),
        "E_CONTINGENT",
    );
    rejects(
        "trait cleanup {control finish(self value) exits done;} implement cleanup for string {@contingent control finish(self value) exits done {emit(value);exit done;}}",
        "E_CONTINGENT",
    );
}

#[test]
fn secondary_fault_keeps_original_and_does_not_reenter_same_handler() {
    let p = checked(&REQUEST.replace(
        "report(borrow context, reason);\n    terminate;",
        "fault \"cleanup broke\";",
    ));
    let failure = p.run_detailed(&scenario(), 1000).unwrap_err();
    assert_eq!(failure.diagnostic.message, "cleanup broke");
    assert!(
        failure
            .report
            .cause
            .unwrap()
            .cause
            .unwrap()
            .message
            .contains("broken request")
    );
    assert_eq!(
        failure
            .events
            .iter()
            .filter(|e| matches!(e, Event::Fault { .. }))
            .count(),
        1
    );
}

#[test]
fn nested_escalation_reaches_only_outer_contingency() {
    let p = checked(
        "boundary inner() {fault \"first\";} contingency(string reason){escalate;} boundary outer(){supervise inner();emit(\"unreachable\");terminate;} contingency(string reason){emit(reason);terminate;} control main() exits done {supervise outer();exit done;}",
    );
    let run = p.run(&scenario()).unwrap();
    assert_eq!(
        run.events
            .iter()
            .filter(|e| matches!(e, Event::Fault { .. }))
            .count(),
        2
    );
    assert!(
        run.events
            .iter()
            .any(|e| matches!(e,Event::Emit{message} if message=="first"))
    );
    assert!(
        !run.events
            .iter()
            .any(|e| matches!(e,Event::Emit{message} if message=="unreachable"))
    );
}

#[test]
fn expansion_preserves_supervision_and_protocol_analysis_does_not_erase_it() {
    let p = checked(REQUEST);
    let expanded = p.expanded("main").unwrap();
    assert_eq!(
        p.run(&scenario()).unwrap(),
        expanded.run(&scenario()).unwrap()
    );
    assert!(
        p.protocol("main", 1000)
            .unwrap_err()
            .message
            .contains("supervision")
    );
    let mut artifact = p.expansion_artifact("main").unwrap();
    artifact.region.contingent = true;
    assert!(p.accept_expansion(&artifact).is_err());
}

#[test]
fn host_limits_are_not_recoverable_source_faults() {
    let failure = checked(REQUEST).run_detailed(&scenario(), 2).unwrap_err();
    assert_eq!(failure.diagnostic.code, "E_FUEL");
    assert_eq!(failure.report.kind, tellur::fault::Kind::ReturnedError);
    assert!(
        !failure
            .events
            .iter()
            .any(|e| matches!(e, Event::Fault { .. }))
    );
    rejects(
        "boundary request(handle owned) {fault \"broken\";} contingency(string reason) {emit(reason);terminate;}",
        "E_FAULT_RESOURCE",
    );
    rejects(
        "pub boundary request() {fault \"broken\";} contingency(string reason) {escalate;}",
        "E_FAULT_CONTRACT",
    );
}

#[test]
fn imported_boundary_contracts_keep_retention_and_contingency_permissions() {
    use std::fs;
    let root = std::env::temp_dir().join(format!("tellur-boundary-import-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("library.tellur"),"@contingent pub control report(string reason) exits done {emit(reason);exit done;} pub boundary request(string context) retains context {fault \"broken\";} contingency(string reason) {report(reason);emit(context);terminate;}").unwrap();
    let main = root.join("main.tellur");
    fs::write(&main,"import \"library.tellur\" as library; pub control main() exits done {supervise library.request(\"retained\");exit done;}").unwrap();
    let p = tellur::compile_file(&main, manifest()).unwrap();
    assert_eq!(p.run(&scenario()).unwrap().outcome, "done");
    let snapshot =
        serde_json::to_value(tellur::modules::contract_snapshot(&main, manifest()).unwrap())
            .unwrap();
    let declarations = snapshot["modules"]
        .as_object()
        .unwrap()
        .values()
        .find_map(|m| m["declarations"].get("request").map(|_| &m["declarations"]))
        .unwrap();
    assert_eq!(
        declarations["request"]["retains"],
        serde_json::json!(["context"])
    );
    assert_eq!(declarations["report"]["contingent"], true);
    assert!(
        !declarations
            .as_object()
            .unwrap()
            .keys()
            .any(|k| k.contains('$'))
    );
    fs::remove_dir_all(root).unwrap();
}
