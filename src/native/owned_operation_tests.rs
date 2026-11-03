//! An owned typed Rust adapter, driven through existing generated source machinery.
use super::*;

const CONTRACT: &str = include_str!("owned_operation_contract.rs");

fn replace(s: &mut String, from: &str, to: &str) {
    assert_eq!(s.matches(from).count(), 1, "{from}");
    *s = s.replacen(from, to, 1);
}
fn owned_source(p: &CheckedProgram) -> String {
    let mut code = pump_source(p);
    code.push_str(&format!(
        "\nmod owned_operation_contract {{\n{CONTRACT}\n}}\n"
    ));
    replace(
        &mut code,
        "pub struct Operation {",
        "pub struct Operation {\nowned: crate::owned_operation_contract::Operation<crate::rust_host::ReadSample>,",
    );
    replace(
        &mut code,
        "                request,\n            },",
        "                request: request.clone(),\n            },\n            owned: crate::owned_operation_contract::Operation::start(probe,request,57),",
    );
    let driver = include_str!("owned_operation_driver.rs");
    replace(
        &mut code,
        "pub struct SourcePump {",
        &format!("{driver}\npub struct SourcePump {{"),
    );
    let pump = include_str!("source_operation_pump.rs");
    let start = pump
        .find("        operation(c, &t)?.poll(chunk)?;")
        .unwrap();
    let end = pump[start..]
        .find("        assert!(\n            operation(c, &t)?")
        .unwrap()
        + start;
    replace(
        &mut code,
        &pump[start..end],
        r#"
        let result=poll_owned(c,&t,chunk.as_bytes())?;
        assert_eq!(before,(c.host.fuel,c.host.events.clone(),c.host.cursor));
        if let Some(result)=result {
            assert_eq!(*phase,3);
            let op=operation(c,&t)?;
            let args=vec![op.resource.clone(),V::String(op.lease.request.clone())];
            let (label,fields)=c.host.finish_owned_model("sample",args,result)?;
            operation(c,&t)?.complete(&label,fields)?;
        } else {
            assert!(*phase<3);
            self.park(c,key)?;
            return Ok(true);
        }
"#,
    );
    // Reuse the existing host argument/result validation and trace tail, without
    // invoking the synchronous Rust wrapper again after typed poll completion.
    let begin = SUPPORT.find("    pub fn primitive(").unwrap();
    let end = SUPPORT[begin..].find("    fn check_contract(").unwrap() + begin;
    let mut finish = SUPPORT[begin..end].to_string();
    replace(
        &mut finish,
        "pub fn primitive(&mut self, name: &str, args: Vec<V>)",
        "pub fn finish_owned_model(&mut self, name: &str, args: Vec<V>, result: Outcome)",
    );
    let begin = finish.find("        let (outcome, values) = if").unwrap();
    let end = finish[begin..]
        .find("        let o = p[\"outcomes\"]")
        .unwrap()
        + begin;
    finish.replace_range(begin..end, "        let (outcome, values) = result;\n");
    replace(
        &mut code,
        "    fn check_contract(",
        &format!("{finish}\n    fn check_contract("),
    );
    code
}

#[test]
fn owned_rust_operation_matches_source_waits_and_resource_settlement() {
    let root = std::env::temp_dir().join(format!("tellur-owned-operation-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let manifest = resource_manifest(&root);
    let host = root.join("host.rs");
    let mut wrapper = fs::read_to_string(&host).unwrap();
    // Any accidental replay through synchronous dispatch must fail this experiment.
    replace(
        &mut wrapper,
        "    assert!(value.alive.get());\n    value.trace.borrow_mut().push(format!(\"sample {request}\"));",
        "    panic!(\"synchronous sample replayed\");\n    value.trace.borrow_mut().push(format!(\"sample {request}\"));",
    );
    wrapper.push_str(include_str!("owned_operation_adapter.rs"));
    replace(
        &mut wrapper,
        "        trace.push(\"checkpoint\".into());",
        "        for request in [\"left\",\"right\"] {assert!(trace.iter().any(|s|s==&format!(\"state drop {request}\")));}\n        trace.push(\"checkpoint\".into());",
    );
    fs::write(&host, wrapper).unwrap();
    let p = crate::compile(
        include_str!("../../tests/fixtures/native_source_pending_operation.tellur"),
        manifest,
    )
    .unwrap()
    .with_entry_exit_codes("main", BTreeMap::from([("done".into(), 0)]))
    .unwrap();
    let mut expanded = p.clone();
    for name in p.regions.keys() {
        expanded = expanded.expanded(name).unwrap();
    }
    for (expanded, p) in [(false, p), (true, expanded)] {
        for opt in [0, 2] {
            let binary = root.join(format!("owned-{expanded}-{opt}"));
            let project = p.native_project("main", &binary).unwrap();
            fs::write(&project.2, owned_source(&p)).unwrap();
            p.finish_native_build(project, opt).unwrap();
            for stop in [false, true] {
                for reverse in [false, true] {
                    for early in [false, true] {
                        let mut finished = false;
                        for fuel in 0..200 {
                            for quotas in ["1", "0,2,0,7", "3,1,9"] {
                                finished = compare_run(
                                    &p, &binary, &root, stop, fuel, reverse, early, quotas,
                                );
                                check_state_trace(&root);
                            }
                            if finished {
                                break;
                            }
                        }
                        assert!(finished);
                        assert!(compare_run(
                            &p, &binary, &root, stop, 1000, reverse, early, "0,2,0,7"
                        ));
                        check_state_trace(&root);
                    }
                }
            }
        }
    }
    fs::remove_dir_all(root).unwrap();
}
fn check_state_trace(root: &Path) {
    let log = fs::read_to_string(root.join("trace.log")).unwrap();
    let lines = log.lines().collect::<Vec<_>>();
    let count = |s: &str| lines.iter().filter(|line| **line == s).count();
    for (request, supplier) in [("other", 1), ("left", 3), ("right", 5)] {
        let started = count(&format!("start {request}"));
        assert_eq!(count(&format!("state start {request}")), started);
        assert_eq!(count(&format!("state drop {request}")), started);
        if started > 0 {
            let pos = |s: &str| lines.iter().position(|line| *line == s).unwrap();
            assert!(
                pos(&format!("state drop {request}")) < pos(&format!("resource drop {supplier}"))
            );
            if count(&format!("sample {request}")) > 0 {
                assert!(pos(&format!("state drop {request}")) < pos(&format!("deliver {request}")));
                for n in 1..=3 {
                    assert_eq!(count(&format!("state poll {request} {n} @57")), 1);
                }
            }
        }
    }
}

#[path = "owned_operation_type_tests.rs"]
mod type_tests;
