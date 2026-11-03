//! Routed owning-fault retirement with actual sockets and unadopted typed outcomes.
use super::*;
fn live_fault_source(p: &CheckedProgram) -> String {
    let mut code = owned_fault_source(p);
    replace(
        &mut code,
        "pub struct Operation {",
        "pub struct Operation {\nregistration:Option<backend_model::Subscription>,\nphysical:Option<crate::rust_host::LiveRegistration>,\ninterest:crate::owned_operation_contract::ReadInterest,",
    );
    replace(
        &mut code,
        "            owned: crate::owned_operation_contract::Operation::start(probe,request,57),",
        "            registration:None,\n            physical:Some(crate::rust_host::LiveRegistration::new(probe,&request)?),\n            interest:crate::owned_operation_contract::ReadInterest{deadline:57},\n            owned: crate::owned_operation_contract::Operation::start(probe,request,57),",
    );
    let mut backend = include_str!("live_readiness_backend_model.rs").to_string();
    replace(
        &mut backend,
        "pub fn signal(&self, expected: &Readiness)",
        "pub fn signal(&self, expected: &Readiness, offer: bool)",
    );
    replace(
        &mut backend,
        "        self.0.borrow().entries[&event.id]\n            .transport\n            .offer(event.id)?;",
        "        if offer {self.0.borrow().entries[&event.id].transport.offer(event.id)?;}",
    );
    replace(
        &mut code,
        "fn poll_pending_owned(",
        &format!(
            "mod backend_model {{\n{backend}\n}}\n{}\n{}\nfn poll_pending_owned(",
            include_str!("readiness_backend_driver.rs"),
            include_str!("live_candidate_fault_driver.rs")
        ),
    );
    replace(
        &mut code,
        "    let mut c = Contexts::new(host);\n    let root = c.start_entry(owner, vec![])?;",
        "    let mut c = Contexts::new(host);\n    let backend=backend_model::Backend::new();\n    let root = c.start_entry(owner, vec![])?;",
    );
    for bytes in ["partial", "A", "B"] {
        replace(
            &mut code,
            &format!("poll_pending_owned(&mut c,&t,b\"{bytes}\")?"),
            &format!("live_poll(&backend,&mut c,&t,b\"{bytes}\")?"),
        );
    }
    replace(
        &mut code,
        "        wakes.push(c.park_for_readiness(&t.context)?);",
        "        wakes.push(final_live_wait(&backend,&mut c,&t)?);",
    );
    replace(
        &mut code,
        "            c.signal_readiness(wake)?;",
        "            let notice=backend.signal(wake,false)?;\n            c.signal_readiness(&notice)?;",
    );
    replace(
        &mut code,
        "    for t in &tickets {\n        assert!(operation(&mut c,t).is_err());",
        "    backend.finish();\n    for t in &tickets {\n        assert!(operation(&mut c,t).is_err());",
    );
    code
}
fn live_fault_manifest(root: &Path) -> Manifest {
    let mut manifest = owned_fault_manifest(root);
    let rust = manifest.rust.as_mut().unwrap();
    rust.lockfile =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/http_receive_mio.lock");
    for (name, version, features) in [
        ("mio", "=1.2.4", vec!["os-poll".into(), "net".into()]),
        ("httparse", "=1.10.1", vec![]),
    ] {
        rust.dependencies.insert(
            name.into(),
            crate::rust_host::Dependency {
                version: version.into(),
                features,
                default_features: true,
            },
        );
    }
    let host = root.join("host.rs");
    let mut wrapper = fs::read_to_string(&host).unwrap();
    replace(
        &mut wrapper,
        "pub struct Probe {",
        "pub struct Probe { pub transport:Rc<LiveTransport>,",
    );
    replace(
        &mut wrapper,
        "Probe {id,alive:",
        "Probe {transport:LiveTransport::new(id,trace.clone()),id,alive:",
    );
    replace(
        &mut wrapper,
        "        assert!(self.alive.replace(false));",
        "        self.transport.require_unregistered();\n        assert!(self.alive.replace(false));",
    );
    replace(
        &mut wrapper,
        "        state.partial.extend_from_slice(bytes);",
        "        let actual=resource.transport.drain();\n        assert_eq!(actual,bytes);\n        state.partial.extend_from_slice(&actual);",
    );
    replace(
        &mut wrapper,
        "        trace.push(\"checkpoint\".into());",
        "        for request in [\"left\",\"right\"] {assert!(trace.iter().any(|s|s==&format!(\"unregister {request}\")));}\n        for id in [1,3] {assert!(trace.iter().any(|s|s==&format!(\"socket drop {id}\")));}\n        trace.push(\"checkpoint\".into());",
    );
    let mut transport = include_str!("live_readiness_transport.rs").to_string();
    let begin = transport.find("        if phase == 1 {").unwrap();
    let end = transport[begin..]
        .find("        let mut wire = self.wire.borrow_mut();")
        .unwrap()
        + begin;
    transport.replace_range(begin..end,"        let bytes: &[u8] = match phase {0=>b\"partial\",1=>b\"A\",2=>b\"B\",_=>unreachable!()};\n");
    replace(
        &mut transport,
        "let mut peek = [0; 1];",
        "let mut peek = [0; 8];",
    );
    replace(
        &mut transport,
        "Ok(1) if observed => {\n                    assert_eq!(&peek, bytes);",
        "Ok(n) if observed && n >= bytes.len() => {\n                    assert_eq!(&peek[..bytes.len()], bytes);",
    );
    replace(
        &mut transport,
        "assert!(result.len() <= 1);",
        "assert!(result.len() <= 7);",
    );
    wrapper.push_str(&transport);
    fs::write(host, wrapper).unwrap();
    manifest
}
fn live_fault_trace(root: &Path, mode: u8) {
    state_trace(root, mode);
    let text = fs::read_to_string(root.join("trace.log")).unwrap();
    let lines = text.lines().collect::<Vec<_>>();
    let count = |s: &str| lines.iter().filter(|line| **line == s).count();
    let pos = |s: &str| {
        assert_eq!(count(s), 1, "{s}: {text}");
        lines.iter().position(|line| *line == s).unwrap()
    };
    for (request, id) in [("left", 1), ("right", 3)] {
        let remove = pos(&format!("unregister {request}"));
        assert!(pos(&format!("register {request}")) < remove);
        assert!(remove < pos(&format!("resource drop {id}")));
        assert!(pos(&format!("state drop {request}")) < pos(&format!("socket drop {id}")));
        if mode == 2 {
            assert!(remove < pos(&format!("candidate drop {request}")));
        } else {
            assert!(remove < pos(&format!("state drop {request}")));
        }
        assert_eq!(
            lines
                .iter()
                .filter(|s| s.starts_with(&format!("os notice {id} ")))
                .count(),
            if mode == 2 { 3 } else { 1 }
        );
        for (i, line) in lines.iter().enumerate() {
            if let Some(suffix) = line.strip_prefix(&format!("subscribe {request} ")) {
                let removed = pos(&format!("unsubscribe {request} {suffix}"));
                assert!(i < removed && removed < remove);
            }
        }
        if count("checkpoint") > 0 {
            assert!(pos(&format!("socket drop {id}")) < pos("checkpoint"));
        }
    }
}
#[test]
fn live_socket_candidates_retire_before_routed_owning_contingency() {
    let root = std::env::temp_dir().join(format!(
        "tellur-live-candidate-fault-{}",
        std::process::id()
    ));
    fs::create_dir_all(&root).unwrap();
    let p = crate::compile(
        include_str!("../../tests/fixtures/native_pending_fault.tellur"),
        live_fault_manifest(&root),
    )
    .unwrap()
    .with_entry_exit_codes("main", BTreeMap::from([("done".into(), 0)]))
    .unwrap();
    let mut expanded = p.clone();
    for region in p.regions.keys() {
        expanded = expanded.expanded(region).unwrap();
    }
    let mut comparisons = 0;
    for (expanded, p) in [(false, p), (true, expanded)] {
        for opt in [0, 2] {
            let binary = root.join(format!("live-candidate-{expanded}-{opt}"));
            let project = p.native_project("main", &binary).unwrap();
            fs::write(&project.2, live_fault_source(&p)).unwrap();
            p.finish_native_build(project, opt).unwrap();
            for mode in 0..3 {
                for quotas in ["1", "0,2,0,7", "3,1,9"] {
                    let mut finished = false;
                    for fuel in 0..64 {
                        finished = compare_fault(&p, &binary, &root, fuel, mode, quotas, false);
                        live_fault_trace(&root, mode);
                        comparisons += 1;
                        if finished {
                            break;
                        }
                    }
                    assert!(finished);
                    assert!(compare_fault(&p, &binary, &root, 64, mode, quotas, true));
                    live_fault_trace(&root, mode);
                    comparisons += 1;
                }
            }
            eprintln!(
                "live candidate fault matrix: expanded={expanded}, O{opt}, comparisons={comparisons}"
            );
        }
    }
    fs::remove_dir_all(root).unwrap();
}
