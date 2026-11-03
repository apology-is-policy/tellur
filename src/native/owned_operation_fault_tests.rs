//! Typed state and unadopted outcomes survive rejected registration, then settle.
use super::*;
fn replace(s: &mut String, from: &str, to: &str) {
    assert_eq!(s.matches(from).count(), 1, "{from}");
    *s = s.replacen(from, to, 1);
}
fn owned_fault_source(p: &CheckedProgram) -> String {
    let mut code = fault_source(p);
    code.push_str(&format!(
        "\nmod owned_operation_contract {{\n{}\n}}\n",
        include_str!("owned_operation_contract.rs")
    ));
    replace(
        &mut code,
        "pub struct Operation {",
        "pub struct Operation {\nowned: crate::owned_operation_contract::Operation<crate::rust_host::StoppedRead>,",
    );
    replace(
        &mut code,
        "                request,\n            },",
        "                request: request.clone(),\n            },\n            owned: crate::owned_operation_contract::Operation::start(probe,request,57),",
    );
    replace(
        &mut code,
        "fn prepare_fault_operation(",
        &format!(
            "{}\nfn prepare_fault_operation(",
            include_str!("owned_operation_fault_driver.rs")
        ),
    );
    replace(
        &mut code,
        "        operation(&mut c, &t)?.poll(\"partial\")?;",
        "        assert!(!poll_pending_owned(&mut c,&t,b\"partial\")?);",
    );
    replace(
        &mut code,
        "            operation(&mut c, &t)?\n                .complete(\"ready\", vec![V::String(\"discarded candidate\".into())])?;",
        "            assert!(!poll_pending_owned(&mut c,&t,b\"A\")?);\n            assert!(poll_pending_owned(&mut c,&t,b\"B\")?);",
    );
    replace(
        &mut code,
        "        wakes.push(c.park_for_readiness(&t.context)?);",
        "        registration_failure(&mut c,&t)?;\n        wakes.push(c.park_for_readiness(&t.context)?);",
    );
    // Other private drivers also have an operation-ticket assertion, so change the
    // exact fault-driver loop rather than matching every occurrence.
    replace(
        &mut code,
        "    for t in &tickets {\n        assert!(operation(&mut c, t).is_err());\n    }",
        "    for t in &tickets {\n        assert!(operation(&mut c,t).is_err());\n        assert!(poll_pending_owned(&mut c,t,b\"late\").is_err());\n    }",
    );
    code
}
fn owned_fault_manifest(root: &Path) -> Manifest {
    let manifest = fault_manifest(root);
    let host = root.join("host.rs");
    let mut wrapper = fs::read_to_string(&host).unwrap();
    let begin = wrapper.find("pub fn sample(").unwrap();
    let end = wrapper[begin..].find("pub fn inspect(").unwrap() + begin;
    wrapper.replace_range(begin..end,"pub fn sample(_: &Probe,_:String)->rust_api::sample_outcome {panic!(\"synchronous sample replayed\")}\n");
    let mut adapter = include_str!("owned_operation_adapter.rs").to_string();
    replace(
        &mut adapter,
        "assert_eq!(state.partial, b\"AB\");",
        "assert_eq!(state.partial, b\"partialAB\");",
    );
    replace(
        &mut adapter,
        ".push(format!(\"sample {}\", state.request));",
        ".push(format!(\"candidate {}\", state.request));",
    );
    wrapper.push_str(&adapter);
    wrapper.push_str(r#"
pub struct StoppedRead;
pub struct Candidate {
    value:rust_api::sample_outcome,
    request:String,
    alive:Rc<Cell<bool>>,
    trace:Rc<RefCell<Vec<String>>>,
}
impl Drop for Candidate {
    fn drop(&mut self) {
        assert!(self.alive.get(),"candidate outlived supplier");
        let rust_api::sample_outcome::ready(text)=&self.value;
        assert!(text.ends_with(":partialAB"));
        self.trace.borrow_mut().push(format!("candidate drop {}",self.request));
    }
}
impl Adapter for StoppedRead {
    type Resource=Probe;
    type State=ReadState;
    type Outcome=Candidate;
    fn prepare(p:&Probe,request:String,deadline:u64)->ReadState {ReadSample::prepare(p,request,deadline)}
    fn poll(s:&mut ReadState,p:&Probe,bytes:&[u8])->Poll<Candidate> {
        match ReadSample::poll(s,p,bytes) {
            Poll::Wait(interest)=>Poll::Wait(interest),
            Poll::Complete(value)=>Poll::Complete(Candidate{value,request:s.request.clone(),alive:p.alive.clone(),trace:p.trace.clone()}),
        }
    }
}
"#);
    replace(
        &mut wrapper,
        "        trace.push(\"checkpoint\".into());",
        r#"
        for request in ["left","right"] {
            assert!(trace.iter().any(|s|s==&format!("state drop {request}")));
            let ready=trace.iter().any(|s|s==&format!("candidate {request}"));
            assert_eq!(trace.iter().filter(|s|*s==&format!("candidate drop {request}")).count(),usize::from(ready));
        }
        trace.push("checkpoint".into());"#,
    );
    fs::write(host, wrapper).unwrap();
    manifest
}
fn state_trace(root: &Path, mode: u8) {
    let log = fs::read_to_string(root.join("trace.log")).unwrap();
    let lines = log.lines().collect::<Vec<_>>();
    let position = |s: &str| {
        assert_eq!(
            lines.iter().filter(|line| **line == s).count(),
            1,
            "{s}: {log}"
        );
        lines.iter().position(|line| *line == s).unwrap()
    };
    for (name, id) in [("left", 1), ("right", 3)] {
        assert!(position(&format!("state start {name}")) < position(&format!("state drop {name}")));
        assert!(position(&format!("state drop {name}")) < position(&format!("resource drop {id}")));
        let count = lines
            .iter()
            .filter(|s| s.starts_with(&format!("state poll {name} ")))
            .count();
        assert_eq!(count, if mode == 2 { 3 } else { 1 });
        if mode == 2 {
            assert!(
                position(&format!("candidate {name}")) < position(&format!("state drop {name}"))
            );
            assert!(
                position(&format!("state drop {name}"))
                    < position(&format!("candidate drop {name}"))
            );
            assert!(
                position(&format!("candidate drop {name}"))
                    < position(&format!("resource drop {id}"))
            );
        } else {
            assert!(!lines.iter().any(|s| s.starts_with("candidate ")));
        }
    }
}
#[test]
fn owned_rust_state_and_unadopted_completion_retire_after_registration_failure() {
    let root = std::env::temp_dir().join(format!("tellur-owned-fault-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let p = crate::compile(
        include_str!("../../tests/fixtures/native_pending_fault.tellur"),
        owned_fault_manifest(&root),
    )
    .unwrap()
    .with_entry_exit_codes("main", BTreeMap::from([("done".into(), 0)]))
    .unwrap();
    let mut expanded = p.clone();
    for region in p.regions.keys() {
        expanded = expanded.expanded(region).unwrap();
    }
    for (expanded, p) in [(false, p), (true, expanded)] {
        for opt in [0, 2] {
            let binary = root.join(format!("typed-fault-{expanded}-{opt}"));
            let project = p.native_project("main", &binary).unwrap();
            fs::write(&project.2, owned_fault_source(&p)).unwrap();
            p.finish_native_build(project, opt).unwrap();
            for mode in 0..3 {
                for quotas in ["1", "0,2,0,7", "3,1,9"] {
                    let mut finished = false;
                    for fuel in 0..64 {
                        finished = compare_fault(&p, &binary, &root, fuel, mode, quotas, false);
                        state_trace(&root, mode);
                        if finished {
                            break;
                        }
                    }
                    assert!(finished);
                    assert!(compare_fault(&p, &binary, &root, 64, mode, quotas, true));
                    state_trace(&root, mode);
                }
            }
        }
    }
    fs::remove_dir_all(root).unwrap();
}
