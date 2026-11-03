//! Test-only model for a source combination that remains gated. Relocate one
//! checked zero-argument helper call across one independent spawn. This is not
//! source admission or an ownership-preserving transformation exposed to users.
use crate::{CheckedProgram, ir::*, manifest::Manifest};

pub(crate) fn program(named: bool, awaited: bool) -> CheckedProgram {
    build(named, awaited, false)
}

pub(crate) fn recovery_program(named: bool, awaited: bool) -> CheckedProgram {
    build(named, awaited, true)
}

fn build(named: bool, awaited: bool, recovery: bool) -> CheckedProgram {
    let wait = if awaited {
        "await pause();"
    } else {
        "yield;yield;"
    };
    let helper = if named && recovery {
        r#"boundary request(string retained) retains retained {run work();terminate;}
        contingency(string reason) {emit(retained);terminate;}
        control helper() exits done {supervise request("must not handle parent");exit done;}"#
    } else if named {
        r#"boundary request() {run work();terminate;}
        contingency(string reason) {emit("must not handle parent");terminate;}
        control helper() exits done {supervise request();exit done;}"#
    } else {
        r#"control helper() exits done {
            fault boundary {run work();}
            contingency [] {emit("must not handle parent");escalate;}
            exit done;
        }"#
    };
    let main = if recovery {
        r#"control main() exits done {
            supervised loop retry(mutable bool first=true) {
                bool run_child=first;first=false;
                fault boundary {
                    delta run_child {true=>attempt,false=>recovered}
                    [attempt]:run startup();exit done;
                    [recovered]:fault boundary {fault "after recovery";}
                    contingency [] {emit("fresh contingency");escalate;}
                } contingency [run_child] {
                    emit("owner contingency");
                    delta run_child {true=>retry_path,false=>exit_path}
                    [retry_path]:recover again retry;
                    [exit_path]:terminate process 23;
                }
            }
        }"#
    } else {
        r#"control main() exits done {
            fault boundary {run startup();}
            contingency [] {emit("owner contingency");terminate process 23;}
            exit done;
        }"#
    };
    let source = format!(
        r#"
        async control fail() faults {{yield;fault "child failed";}}
        async control pause() exits done {{yield;yield;exit done;}}
        async control work() exits done {{emit("helper active");{wait}exit done;}}
        {helper}
        async control startup() exits done faults {{
            helper();
            auto child=spawn fail();
            await child;
        }}
        {main}
    "#
    );
    let manifest = Manifest::from_json(include_str!("../playground/manifest.json")).unwrap();
    let gated = source.replace(
        "helper();\n            auto child=spawn fail();",
        "auto child=spawn fail();\n            helper();",
    );
    assert_ne!(source, gated);
    let error = crate::compile(&gated, manifest.clone()).unwrap_err();
    assert_eq!(error.code, "E_ASYNC", "{error}");
    let mut p = crate::compile(&source, manifest).unwrap();
    let code = &mut p.regions.get_mut("startup").unwrap().code;
    let call = code.iter().position(|i| matches!(&i.action, Action::Decide { operation: Operation::Region(name), .. } if name=="helper")).unwrap();
    let spawn = code.iter().position(|i| matches!(&i.action, Action::Decide { operation: Operation::Spawn(name), .. } if name=="fail")).unwrap();
    assert_eq!(spawn, call + 1);
    for i in [&code[call], &code[spawn]] {
        assert_eq!(i.guard, Guard::True);
        let Action::Decide { args, .. } = &i.action else {
            unreachable!()
        };
        assert!(args.is_empty());
    }
    let Action::Decide { arms, .. } = &code[call].action else {
        unreachable!()
    };
    assert_eq!(arms.len(), 1);
    assert!(arms[0].slots.is_empty());
    code.swap(call, spawn);
    p.verify().unwrap();
    p
}

#[test]
fn model_routes_sibling_fault_past_parent_inline_boundaries() {
    check(program, &["helper active", "owner contingency"]);
}

#[test]
fn owning_recovery_clears_routing_target_and_skips_inner_retained_handover() {
    check(
        recovery_program,
        &[
            "helper active",
            "owner contingency",
            "fresh contingency",
            "owner contingency",
        ],
    );
}

fn check(build: fn(bool, bool) -> CheckedProgram, expected: &[&str]) {
    use crate::runtime::{Event, RunCompletion, Scenario};
    for named in [false, true] {
        for awaited in [false, true] {
            let p = build(named, awaited);
            for p in [p.clone(), p.expanded("main").unwrap()] {
                let s = Scenario {
                    entry: "main".into(),
                    arguments: vec![],
                    calls: vec![],
                };
                let result = p.run_completion(&s, 1000).unwrap();
                let RunCompletion::Process(result) = result else {
                    panic!("{result:?}")
                };
                assert_eq!(result.status, 23);
                let messages: Vec<_> = result
                    .events
                    .iter()
                    .filter_map(|e| match e {
                        Event::Emit { message } => Some(message.as_str()),
                        _ => None,
                    })
                    .collect();
                assert_eq!(messages, expected, "named={named}, awaited={awaited}");
            }
        }
    }
}
