//! Gated inline helper schedules with local recovery and helper-owned children.
use crate::{CheckedProgram, ir::*, manifest::Manifest};

pub(crate) fn program(named: bool, spawned: bool, stop: bool) -> CheckedProgram {
    let child = if spawned {
        "auto child=spawn inner(broken);await child;"
    } else {
        "await inner(broken);"
    };
    let outside_end = if stop {
        "fault \"outside failed\";"
    } else {
        "emit(\"outside finished\");exit done;"
    };
    let helper = if named {
        r#"boundary request(bool broken) {run work(broken);terminate;}
        contingency(string reason) {emit("local recovery");terminate;}
        control helper() exits done {
            supervise request(true);supervise request(false);
            emit("helper returned");exit done;
        }"#
    } else {
        r#"control helper() exits done {
            supervised loop retry(mutable bool first=true) {
                bool broken=first;first=false;
                fault boundary {run work(broken);emit("helper returned");exit done;}
                contingency [] {emit("local recovery");recover again retry;}
            }
        }"#
    };
    let source = format!(
        r#"
        async control inner(bool broken) exits done faults {{
            emit("inner active");yield;
            delta broken {{true=>failed,false=>ok}}
            [failed]:fault "local failed";
            [ok]:emit("inner finished");exit done;
        }}
        async control work(bool broken) exits done faults {{{child}exit done;}}
        async control outside() exits done faults {{
            emit("outside active");yield;yield;yield;{outside_end}
        }}
        {helper}
        async control startup() exits done faults {{
            helper();
            auto child=spawn outside();
            await child;exit done;
        }}
        control main() exits done {{
            fault boundary {{run startup();}}
            contingency [] {{emit("owner contingency");terminate process 23;}}
            exit done;
        }}
    "#
    );
    let manifest = Manifest::from_json(include_str!("../playground/manifest.json")).unwrap();
    let gated = source.replace(
        "helper();\n            auto child=spawn outside();",
        "auto child=spawn outside();\n            helper();",
    );
    assert_ne!(source, gated);
    let error = crate::compile(&gated, manifest.clone()).unwrap_err();
    assert_eq!(error.code, "E_ASYNC", "{error}");
    let mut p = crate::compile(&source, manifest).unwrap();
    let code = &mut p.regions.get_mut("startup").unwrap().code;
    assert!(
        matches!(&code[0].action,Action::Decide {operation:Operation::Region(name),args,arms,..} if name=="helper" && args.is_empty() && arms.len()==1 && arms[0].slots.is_empty())
    );
    assert!(
        matches!(&code[1].action,Action::Decide {operation:Operation::Spawn(name),args,..} if name=="outside" && args.is_empty())
    );
    assert_eq!(code[0].guard, Guard::True);
    assert_eq!(code[1].guard, Guard::True);
    code.swap(0, 1);
    p.verify().unwrap();
    p
}

#[test]
fn helper_recovery_preserves_outer_children_and_settles_its_own_children() {
    use crate::runtime::{Event, RunCompletion, Scenario};
    for named in [false, true] {
        for spawned in [false, true] {
            for stop in [false, true] {
                let p = program(named, spawned, stop);
                for p in [p.clone(), p.expanded("main").unwrap()] {
                    let s = Scenario {
                        entry: "main".into(),
                        arguments: vec![],
                        calls: vec![],
                    };
                    let result = crate::runtime::census_model(&p, &s);
                    let events = match result {
                        RunCompletion::Outcome(r) => {
                            assert!(!stop);
                            assert_eq!(r.outcome, "done");
                            r.events
                        }
                        RunCompletion::Process(r) => {
                            assert!(stop);
                            assert_eq!(r.status, 23);
                            r.events
                        }
                    };
                    let count = |text: &str| {
                        events
                            .iter()
                            .filter(|e| matches!(e,Event::Emit {message} if message==text))
                            .count()
                    };
                    assert_eq!(
                        count("local recovery"),
                        1,
                        "{named}/{spawned}/{stop}: {events:?}"
                    );
                    assert_eq!(count("inner active"), 2);
                    assert_eq!(count("outside active"), 1);
                    assert_eq!(count("helper returned"), usize::from(!stop));
                    assert_eq!(count("inner finished"), usize::from(!stop));
                    assert_eq!(count("outside finished"), usize::from(!stop));
                    assert_eq!(count("owner contingency"), usize::from(stop));
                    let faults: Vec<_> = events
                        .iter()
                        .filter_map(|e| match e {
                            Event::Fault { report } => Some(report),
                            _ => None,
                        })
                        .collect();
                    assert_eq!(faults.len(), 1 + usize::from(stop));
                    if stop {
                        let mut cause = faults.last().unwrap().as_ref();
                        while let Some(inner) = &cause.cause {
                            cause = inner;
                        }
                        assert_eq!(cause.boundary, "outside");
                        assert_eq!(cause.activations.last().unwrap().control, "outside");
                        assert!(cause.message.contains("outside failed"));
                    }
                }
            }
        }
    }
}
