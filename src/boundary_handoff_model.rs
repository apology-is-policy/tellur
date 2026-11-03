//! Checked source models for inline helper boundary handoffs and owning fault routing.
use crate::{CheckedProgram, manifest::Manifest};

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
            auto child=spawn fail();
            helper();
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
    let p = crate::compile(&source, manifest).unwrap_or_else(|e| panic!("{e}\n{source}"));
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

/// Checked source resource model: an independent child runs beside helper-owned
/// holders and inline, awaited or spawned borrowers.
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
            auto child=spawn fail(stop);
            helper(path);
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
    let p = crate::compile(&source, manifest).unwrap_or_else(|e| panic!("{e}\n{source}"));
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
