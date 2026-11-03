//! Test-only models for source combinations that remain gated. Each relocates
//! a checked helper call across an independent spawn under its stated fixture
//! constraints. This is not source admission or a user-facing transformation.
use crate::{CheckedProgram, ir::*, manifest::Manifest};

pub(crate) fn program(named: bool, awaited: bool) -> CheckedProgram {
    build(named, awaited, false, false)
}

pub(crate) fn recovery_program(named: bool, awaited: bool) -> CheckedProgram {
    build(named, awaited, true, false)
}

pub(crate) fn nested_program(named: bool, awaited: bool) -> CheckedProgram {
    build(named, awaited, false, true)
}

fn build(named: bool, awaited: bool, recovery: bool, nested: bool) -> CheckedProgram {
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
    let source = if nested {
        let source = source.replace("run startup();", "run root();");
        format!(
            "{source} async control root() exits done faults {{auto parent=spawn startup();await parent;exit done;}}"
        )
    } else {
        source
    };
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

/// A separate gated model with real resource contracts. Only the boolean child
/// capture and independently moved string path are reordered; the child never observes either
/// helper storage or the path. Resource capture admission is not bypassed.
pub(crate) fn resource_program(manifest: Manifest, named: bool, awaited: bool) -> CheckedProgram {
    resource_model(manifest, named, awaited, false)
}

pub(crate) fn spawned_resource_program(manifest: Manifest, named: bool) -> CheckedProgram {
    resource_model(manifest, named, true, true)
}

fn resource_model(manifest: Manifest, named: bool, awaited: bool, spawned: bool) -> CheckedProgram {
    let work = if spawned {
        "auto child=spawn observe(path,parent);await child;"
    } else {
        "await observe(path,parent);"
    };
    let call = if awaited {
        "run work(path,borrow local);"
    } else {
        "run observe(path,borrow local);"
    };
    let helper = if named {
        format!(
            r#"
        boundary request(string path,counter local) retains local {{
            {call}close(local);terminate;
        }} contingency(string reason) {{close(local);terminate;}}
        control helper(string path) exits done {{
            delta make(borrow path,4) {{made {{counter local}}=>made}}
            [made]:supervise request(path,local);exit done;
        }}"#
        )
    } else {
        format!(
            r#"
        control helper(string path) exits done {{
            fault boundary {{
                delta make(borrow path,4) {{made {{counter local}}=>made}}
                [made]:{call}close(local);exit done;
            }} contingency [] {{escalate;}}
        }}"#
        )
    };
    let source = format!(
        r#"
        async control observe(string path,ref counter parent) exits done {{
            delta make(borrow path,5) {{made {{counter local}}=>made}}
            [made]:inspect(parent);yield;yield;inspect(parent);close(local);exit done;
        }}
        async control work(string path,ref counter parent) exits done {{
            {work}exit done;
        }}
        async control fail(bool stop) exits done faults {{
            yield;delta stop {{true=>failed,false=>ok}}
            [failed]:fault "child failed";[ok]:exit done;
        }}
        {helper}
        async control startup(string path,bool stop) exits done faults {{
            helper(path);
            auto child=spawn fail(stop);
            await child;exit done;
        }}
        control main(string path,bool stop) exits done {{
            delta make(borrow path,3) {{made {{counter owner}}=>made}}
            [made]:fault boundary {{run startup(path,stop);}}
            contingency [owner] {{close(owner);terminate process 23;}}
            [made]:close(owner);exit done;
        }}
    "#
    );
    let gated = source.replace(
        "helper(path);\n            auto child=spawn fail(stop);",
        "auto child=spawn fail(stop);\n            helper(path);",
    );
    assert_ne!(source, gated);
    let e = crate::compile(&gated, manifest.clone()).unwrap_err();
    assert_eq!(e.code, "E_ASYNC", "{e}\n{gated}");
    let mut p = crate::compile(&source, manifest).unwrap_or_else(|e| panic!("{e}\n{source}"));
    let code = &mut p.regions.get_mut("startup").unwrap().code;
    assert!(
        matches!(&code[0].action,Action::Decide {operation:Operation::Region(name),args,arms,..} if name=="helper" && args.len()==1 && arms.len()==1 && arms[0].slots.is_empty())
    );
    assert!(
        matches!(&code[1].action,Action::Decide {operation:Operation::Spawn(name),args,..} if name=="fail" && args.len()==1)
    );
    assert_eq!(code[0].guard, Guard::True);
    assert_eq!(code[1].guard, Guard::True);
    code.swap(0, 1);
    p.verify().unwrap();
    p
}

#[test]
fn nested_source_owners_keep_the_original_child_fault_cause() {
    check(nested_program, &["helper active", "owner contingency"]);
    for named in [false, true] {
        for awaited in [false, true] {
            let p = nested_program(named, awaited);
            for p in [p.clone(), p.expanded("main").unwrap()] {
                let scenario = crate::runtime::Scenario {
                    entry: "main".into(),
                    arguments: vec![],
                    calls: vec![],
                };
                let crate::runtime::RunCompletion::Process(result) =
                    crate::runtime::census_model(&p, &scenario)
                else {
                    panic!("owning process disposition expected")
                };
                let faults: Vec<_> = result
                    .events
                    .iter()
                    .filter_map(|event| match event {
                        crate::runtime::Event::Fault { report } => Some(report),
                        _ => None,
                    })
                    .collect();
                assert_eq!(faults.len(), 1);
                let cause = faults[0].cause.as_ref().expect("original child cause");
                assert_eq!(cause.boundary, "fail");
                assert_eq!(cause.kind, crate::fault::Kind::SourceFault);
                assert_eq!(cause.activations.last().unwrap().control, "fail");
            }
        }
    }
}
