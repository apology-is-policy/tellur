//! Persistent OS registration and one-use runtime waits in the source-pump model.
use super::*;
fn live_source(p: &CheckedProgram) -> String {
    let mut code = backend_source(p);
    replace(
        &mut code,
        include_str!("readiness_backend_model.rs"),
        include_str!("live_readiness_backend_model.rs"),
    );
    replace(
        &mut code,
        "registration:Option<backend_model::Subscription>,",
        "registration:Option<backend_model::Subscription>,\nphysical:Option<crate::rust_host::LiveRegistration>,",
    );
    replace(
        &mut code,
        "            owned: crate::owned_operation_contract::Operation::start(probe,request,57),",
        "            physical:Some(crate::rust_host::LiveRegistration::new(probe,&request)?),\n            owned: crate::owned_operation_contract::Operation::start(probe,request,57),",
    );
    replace(
        &mut code,
        "        None => {\n            assert!(op.owned.poll",
        "        None => {\n            drop(op.physical.take().expect(\"completed registration\"));\n            assert!(op.owned.poll",
    );
    replace(
        &mut code,
        "        let poll = contexts.advance_retained(&key, quantum)?;",
        "        let reads=crate::rust_host::live_read_count();\n        let poll=contexts.advance_retained(&key, quantum)?;\n        if quantum==0 {assert_eq!(reads,crate::rust_host::live_read_count(),\"zero quota read from socket\");}",
    );
    code
}
fn live_manifest(root: &Path) -> Manifest {
    let mut manifest = backend_manifest(root);
    let rust = manifest.rust.as_mut().unwrap();
    rust.lockfile =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/http_receive_mio.lock");
    // Reuse the exact private live-receive lock, including its existing parser pin.
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
        "        let actual=resource.transport.drain();\n        assert_eq!(actual,bytes,\"controlled peer byte mismatch\");\n        state.partial.extend_from_slice(&actual);",
    );
    replace(
        &mut wrapper,
        "        trace.push(\"checkpoint\".into());",
        "        for request in [\"left\",\"right\"] {assert!(trace.iter().any(|s|s==&format!(\"unregister {request}\")));}\n        for id in [3,5] {assert!(trace.iter().any(|s|s==&format!(\"socket drop {id}\")));}\n        trace.push(\"checkpoint\".into());",
    );
    wrapper.push_str(include_str!("live_readiness_transport.rs"));
    fs::write(host, wrapper).unwrap();
    manifest
}
fn live_trace(root: &Path) {
    backend_trace(root);
    let text = fs::read_to_string(root.join("trace.log")).unwrap();
    let lines = text.lines().collect::<Vec<_>>();
    let count = |s: &str| lines.iter().filter(|line| **line == s).count();
    let pos = |s: &str| lines.iter().position(|line| *line == s).unwrap();
    for (request, id) in [("other", 1), ("left", 3), ("right", 5)] {
        let started = count(&format!("start {request}"));
        assert_eq!(count(&format!("register {request}")), started, "{text}");
        assert_eq!(count(&format!("unregister {request}")), started, "{text}");
        assert_eq!(count(&format!("socket drop {id}")), started, "{text}");
        if started > 0 {
            let removed = pos(&format!("unregister {request}"));
            assert!(pos(&format!("register {request}")) < removed);
            assert!(removed < pos(&format!("resource drop {id}")));
            assert!(pos(&format!("state drop {request}")) < pos(&format!("socket drop {id}")));
            if count(&format!("deliver {request}")) > 0 {
                assert!(removed < pos(&format!("deliver {request}")));
            } else {
                assert!(removed < pos(&format!("state drop {request}")));
            }
            for (at, line) in lines.iter().enumerate() {
                if line.starts_with(&format!("unsubscribe {request} ")) {
                    assert!(at < removed);
                }
            }
        }
        if count(&format!("sample {request}")) > 0 {
            assert_eq!(count(&format!("empty hint {id}")), 1);
            assert_eq!(
                lines
                    .iter()
                    .filter(|s| s.starts_with(&format!("os notice {id} ")))
                    .count(),
                2
            );
            assert!(count(&format!("io read {id}")) >= 5);
        }
    }
}
#[test]
fn live_socket_registration_preserves_source_waits_quotas_and_fault_settlement() {
    let root = std::env::temp_dir().join(format!("tellur-live-backend-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let p = crate::compile(
        include_str!("../../tests/fixtures/native_source_pending_operation.tellur"),
        live_manifest(&root),
    )
    .unwrap()
    .with_entry_exit_codes("main", BTreeMap::from([("done".into(), 0)]))
    .unwrap();
    let mut expanded = p.clone();
    for name in p.regions.keys() {
        expanded = expanded.expanded(name).unwrap();
    }
    let mut comparisons = 0;
    for (expanded, p) in [(false, p), (true, expanded)] {
        for opt in [0, 2] {
            let binary = root.join(format!("live-backend-{expanded}-{opt}"));
            let project = p.native_project("main", &binary).unwrap();
            fs::write(&project.2, live_source(&p)).unwrap();
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
                                live_trace(&root);
                                comparisons += 1;
                            }
                            if finished {
                                break;
                            }
                        }
                        assert!(finished);
                        assert!(compare_run(
                            &p, &binary, &root, stop, 1000, reverse, early, "0,2,0,7"
                        ));
                        live_trace(&root);
                        comparisons += 1;
                    }
                }
            }
            eprintln!("live source matrix: expanded={expanded}, O{opt}, comparisons={comparisons}");
        }
    }
    fs::remove_dir_all(root).unwrap();
}
