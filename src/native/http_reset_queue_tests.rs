//! Actual reset errors through the existing checked HTTP source queue.
use super::*;
fn reset_manifest(root: &Path) -> Manifest {
    let mut manifest = live_manifest(root);
    let rust = manifest.rust.as_mut().unwrap();
    rust.dependencies.insert(
        "libc".into(),
        crate::rust_host::Dependency {
            version: "=0.2.191".into(),
            features: vec![],
            default_features: true,
        },
    );
    let lock = include_str!("../../tests/fixtures/http_receive_mio.lock");
    let root_dependencies =
        "name = \"tellur_adapter_lock\"\nversion = \"0.1.0\"\ndependencies = [\n \"httparse\",";
    assert_eq!(lock.matches(root_dependencies).count(), 1);
    rust.lockfile = root.join("reset.lock");
    fs::write(
        &rust.lockfile,
        lock.replace(
            root_dependencies,
            &format!("{root_dependencies}\n \"libc\","),
        ),
    )
    .unwrap();
    let mut host = fs::read_to_string(root.join("host.rs")).unwrap();
    for (from, to) in [
        (
            "    client: std::net::TcpStream,",
            "    client: Option<std::net::TcpStream>,",
        ),
        (
            "        wire.client.write_all(chunk)?;",
            "        wire.client.as_mut().expect(\"peer already closed\").write_all(chunk)?;",
        ),
        ("            client,", "            client:Some(client),"),
        (
            "        if self.socket.timeout && self.socket.chunks.borrow().is_empty() {",
            "        if self.socket.name==\"left\" && self.socket.chunks.borrow().is_empty() {\n            self.socket.reset_peer(self.token,until)?;\n        } else if self.socket.timeout && self.socket.chunks.borrow().is_empty() {",
        ),
        (
            "        if let Ok(n) = result {",
            "        if result.as_ref().is_err_and(|e|e.kind()==io::ErrorKind::ConnectionReset) {\n            assert!(result.as_ref().unwrap_err().raw_os_error().is_some());\n            s.trace.borrow_mut().push(format!(\"reset error {}\",s.name));\n        }\n        if let Ok(n) = result {",
        ),
    ] {
        replace(&mut host, from, to);
    }
    let reset = include_str!("../../tests/fixtures/http_reset_driver.rs");
    host.push_str(
        &reset[..reset.find("fn reset_after(").unwrap()]
            .replace("peer: TcpStream", "peer: std::net::TcpStream"),
    );
    host.push_str(include_str!("http_reset_queue_host.rs"));
    fs::write(root.join("host.rs"), host).unwrap();
    manifest
}
fn reset_trace(root: &Path, success: bool, stop: bool, early: bool) {
    let text = fs::read_to_string(root.join("trace.log")).unwrap();
    let count = |s: &str| text.lines().filter(|l| *l == s).count();
    let resets = count("reset error left");
    let closes = count("abort peer left");
    assert!(resets <= closes && closes <= 1, "{text}");
    if success {
        assert_eq!(resets, usize::from(!stop), "{text}");
    }
    if stop {
        assert_eq!(closes, 0, "fault precedes planned peer reset: {text}");
        if success {
            let body = "left:🙂";
            let packet = format!(
                "HEAD /left HTTP/1.1\r\nHost: local\r\nX-Name: left\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            );
            assert!(
                text.contains(&format!(
                    "resource drop left head={early} bytes={}",
                    if early { packet.len() - 2 } else { 0 }
                )),
                "{text}"
            );
        }
    }
    if resets > 0 {
        let error = text.find("reset error left").unwrap();
        assert!(text.find("abort peer left").unwrap() < error);
        assert!(text.rfind("logical retire left ").unwrap() < error);
        assert!(error < text.find("unregister left").unwrap());
        let body = "left:🙂";
        let packet = format!(
            "HEAD /left HTTP/1.1\r\nHost: local\r\nX-Name: left\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        assert!(text.contains(&format!(
            "resource drop left head=true bytes={}",
            packet.len() - 2
        )));
    }
    assert_eq!(count("reset error right"), 0);
    assert_eq!(count("reset error other"), 0);
}
#[test]
fn live_http_resets_preserve_source_events_faults_and_retirement() {
    let root = std::env::temp_dir().join(format!("tellur-http-reset-queue-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let p = crate::compile(
        include_str!("../../tests/fixtures/native_source_repeated_http.tellur"),
        reset_manifest(&root),
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
            let binary = root.join(format!("http-reset-queue-{expanded}-{opt}"));
            let project = p.native_project("main", &binary).unwrap();
            fs::write(&project.2, live_source(&p)).unwrap();
            p.finish_native_build(project, opt).unwrap();
            for (instant, stop) in [(false, false), (false, true)] {
                for reverse in [false, true] {
                    for early in [false, true] {
                        assert!(compare_repeated(
                            &p,
                            &binary,
                            &root,
                            "disconnected",
                            instant,
                            stop,
                            1000,
                            "0,2,0,7",
                            reverse,
                            early
                        ));
                        live_trace(&root, instant, stop, true);
                        reset_trace(&root, true, stop, early);
                        comparisons += 1;
                    }
                }
            }
            for (instant, stop, reverse, early) in [
                (false, false, false, false),
                (false, true, false, false),
                (false, true, true, true),
            ] {
                let mut finished = false;
                for fuel in 0..250 {
                    for quotas in ["1", "0,2,0,7", "3,1,9"] {
                        finished = compare_repeated(
                            &p,
                            &binary,
                            &root,
                            "disconnected",
                            instant,
                            stop,
                            fuel,
                            quotas,
                            reverse,
                            early,
                        );
                        live_trace(&root, instant, stop, finished);
                        reset_trace(&root, finished, stop, early);
                        comparisons += 1;
                    }
                    if finished {
                        break;
                    }
                }
                assert!(finished);
            }
            eprintln!(
                "live HTTP reset queue: expanded={expanded}, O{opt}, comparisons={comparisons}"
            );
        }
    }
    fs::remove_dir_all(root).unwrap();
}
