//! Readiness registration ownership around the existing typed source-pump matrix.
use super::*;
fn backend_source(p: &CheckedProgram) -> String {
    let mut code = owned_source(p);
    replace(
        &mut code,
        "pub struct Operation {",
        "pub struct Operation {\nregistration:Option<backend_model::Subscription>,\ninterest:crate::owned_operation_contract::ReadInterest,",
    );
    replace(
        &mut code,
        "            owned: crate::owned_operation_contract::Operation::start(probe,request,57),",
        "            registration:None,\n            interest:crate::owned_operation_contract::ReadInterest{deadline:57},\n            owned: crate::owned_operation_contract::Operation::start(probe,request,57),",
    );
    replace(
        &mut code,
        "pub struct SourcePump {",
        &format!(
            "mod backend_model {{\n{}\n}}\n{}\npub struct SourcePump {{\nbackend:backend_model::Backend,",
            include_str!("readiness_backend_model.rs"),
            include_str!("readiness_backend_driver.rs")
        ),
    );
    replace(
        &mut code,
        "            tickets: BTreeMap::new(),",
        "            backend:backend_model::Backend::new(),\n            tickets: BTreeMap::new(),",
    );
    replace(
        &mut code,
        "        let token = c.park_for_readiness(key)?;",
        r#"
        let count=self.backend.active();
        let before=(c.host.fuel,c.host.events.clone(),c.host.cursor,c.waits.len());
        for failure in [backend_model::Failure::BeforeInsert,backend_model::Failure::AfterInsert] {
            assert!(register_backend(&self.backend,c,key,failure).is_err());
            assert_eq!(self.backend.active(),count);
            assert_eq!(before,(c.host.fuel,c.host.events.clone(),c.host.cursor,c.waits.len()));
        }
        let generation=c.next_readiness;
        c.next_readiness=u64::MAX;
        assert!(register_backend(&self.backend,c,key,backend_model::Failure::None).is_err());
        c.next_readiness=generation;
        assert_eq!(self.backend.active(),count);
        assert_eq!(before,(c.host.fuel,c.host.events.clone(),c.host.cursor,c.waits.len()));
        let token=register_backend(&self.backend,c,key,backend_model::Failure::None)?;
"#,
    );
    replace(
        &mut code,
        "                c.signal_readiness(&token)?;",
        "                let event=self.backend.signal(&token)?;\n                assert_eq!(event.context,token.context);assert_eq!(event.generation,token.generation);\n                c.signal_readiness(&event)?;",
    );
    replace(
        &mut code,
        "    let resource = operation(c, ticket)?.resource.clone();",
        "    drop(operation(c,ticket)?.registration.take().expect(\"poll needs a subscription\"));\n    let resource = operation(c, ticket)?.resource.clone();",
    );
    replace(
        &mut code,
        "            assert_eq!(interest.deadline, 57);",
        "            assert_eq!(interest.deadline, 57);\n            op.interest=interest;",
    );
    replace(
        &mut code,
        "        self.tickets.clear();",
        "        self.backend.finish();\n        self.tickets.clear();",
    );
    code
}
fn backend_manifest(root: &Path) -> Manifest {
    let manifest = resource_manifest(root);
    let host = root.join("host.rs");
    let mut wrapper = fs::read_to_string(&host).unwrap();
    let begin = wrapper.find("pub fn sample(").unwrap();
    let end = wrapper[begin..].find("pub fn inspect(").unwrap() + begin;
    wrapper.replace_range(begin..end,"pub fn sample(_: &Probe,_:String)->rust_api::sample_outcome {panic!(\"synchronous sample replayed\")}\n");
    let mut adapter = include_str!("owned_operation_adapter.rs").to_string();
    replace(
        &mut adapter,
        "        assert!(self.alive.get(), \"owned state outlived its resource\");",
        r#"
        assert!(self.alive.get(),"owned state outlived its resource");
        let trace=self.trace.borrow();
        let subscribed=trace.iter().filter(|s|s.starts_with(&format!("subscribe {} ",self.request))).count();
        let removed=trace.iter().filter(|s|s.starts_with(&format!("unsubscribe {} ",self.request))).count();
        assert_eq!(subscribed,removed,"state retired with backend subscription");
        drop(trace);
"#,
    );
    wrapper.push_str(&adapter);
    replace(
        &mut wrapper,
        "        trace.push(\"checkpoint\".into());",
        "        for request in [\"left\",\"right\"] {assert!(trace.iter().any(|s|s==&format!(\"state drop {request}\")));}\n        trace.push(\"checkpoint\".into());",
    );
    fs::write(host, wrapper).unwrap();
    manifest
}
fn backend_trace(root: &Path) {
    check_state_trace(root);
    let text = fs::read_to_string(root.join("trace.log")).unwrap();
    let lines = text.lines().collect::<Vec<_>>();
    for (index, line) in lines.iter().enumerate() {
        if let Some(suffix) = line.strip_prefix("subscribe ") {
            let removed = format!("unsubscribe {suffix}");
            let positions = lines
                .iter()
                .enumerate()
                .filter(|(_, line)| **line == removed)
                .map(|(i, _)| i)
                .collect::<Vec<_>>();
            assert_eq!(positions.len(), 1);
            assert!(index < positions[0]);
            let request = suffix.split_whitespace().next().unwrap();
            let drop = lines
                .iter()
                .position(|line| *line == format!("state drop {request}"))
                .unwrap();
            assert!(positions[0] < drop);
        }
    }
}
#[test]
fn backend_subscriptions_rollback_and_retire_before_owned_operations() {
    let root =
        std::env::temp_dir().join(format!("tellur-readiness-backend-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let p = crate::compile(
        include_str!("../../tests/fixtures/native_source_pending_operation.tellur"),
        backend_manifest(&root),
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
            let binary = root.join(format!("backend-{expanded}-{opt}"));
            let project = p.native_project("main", &binary).unwrap();
            fs::write(&project.2, backend_source(&p)).unwrap();
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
                                backend_trace(&root);
                            }
                            if finished {
                                break;
                            }
                        }
                        assert!(finished);
                        assert!(compare_run(
                            &p, &binary, &root, stop, 1000, reverse, early, "0,2,0,7"
                        ));
                        backend_trace(&root);
                    }
                }
            }
        }
    }
    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[path = "live_readiness_backend_tests.rs"]
mod live_backend_tests;
