//! Actual reset errors must flow through the existing HTTP receive mapping.
use super::*;

#[test]
fn live_http_resets_preserve_disconnected_mapping_and_retirement() {
    let root = std::env::temp_dir().join(format!("tellur-http-reset-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let example = repo.join("tests/fixtures/http_sync");
    let mut manifest = crate::manifest::Manifest::from_json_at(
        &fs::read_to_string(example.join("manifest.json")).unwrap(),
        &example.join("manifest.json"),
    )
    .unwrap();
    let rust = manifest.rust.as_mut().unwrap();
    rust.dependencies.insert(
        "mio".into(),
        crate::rust_host::Dependency {
            version: "=1.2.4".into(),
            features: vec!["os-poll".into(), "net".into()],
            default_features: true,
        },
    );
    rust.dependencies.insert(
        "libc".into(),
        crate::rust_host::Dependency {
            version: "=0.2.191".into(),
            features: vec![],
            default_features: true,
        },
    );
    // Preserve the pinned package set; only the fixture root gains libc directly.
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
    let p = crate::modules::compile_with_loader(example.join("main.tellur"), manifest, |path| {
        fs::read_to_string(path)
    })
    .unwrap();
    let host = include_str!("../../library/host/loopback_http.rs");
    let constants =
        &host[host.find("const HEADER_LIMIT:").unwrap()..host.find("pub fn http_listen(").unwrap()];
    let failure =
        &host[host.find("fn io_failure(").unwrap()..host.find("fn read_before(").unwrap()];
    // Reuse the live driver without rerunning its already validated matrix.
    let driver = include_str!("../../tests/fixtures/http_live_receive_driver.rs");
    let mut driver = driver[..driver.find("pub fn run() {").unwrap()].to_string();
    for (from, to) in [
        (
            "    blocked: Cell<usize>,",
            "    blocked: Cell<usize>,\n    resets: Cell<usize>,",
        ),
        (
            "                blocked: Cell::new(0),",
            "                blocked: Cell::new(0),\n                resets: Cell::new(0),",
        ),
        (
            "            _ => {}",
            "            Err(e) if e.kind() == io::ErrorKind::ConnectionReset => {\n                assert!(e.raw_os_error().is_some());\n                self.0.resets.set(self.0.resets.get()+1);\n            }\n            _ => {}",
        ),
    ] {
        assert_eq!(driver.matches(from).count(), 1);
        driver = driver.replace(from, to);
    }
    driver.push_str(include_str!("../../tests/fixtures/http_reset_driver.rs"));
    for opt in [0, 2] {
        let binary = root.join(format!("http-reset-{opt}"));
        let project = p.native_project("main", &binary).unwrap();
        let mut code = fs::read_to_string(&project.2).unwrap();
        let main = "fn main() -> std::process::ExitCode {";
        assert_eq!(code.matches(main).count(), 1);
        code = code.replacen(main, "fn original_main() -> std::process::ExitCode {", 1);
        code.push_str(&format!(
            r#"
mod receive_test {{
{}
{constants}
{failure}
mod operation {{ {} }}
}}
fn main()->std::process::ExitCode {{receive_test::run();std::process::ExitCode::SUCCESS}}
"#,
            driver,
            include_str!("../../tests/fixtures/http_receive_operation.rs")
        ));
        fs::write(&project.2, code).unwrap();
        p.finish_native_build(project, opt).unwrap();
        let output = Command::new(&binary).output().unwrap();
        assert!(
            output.status.success(),
            "O{opt}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let report = String::from_utf8(output.stdout).unwrap();
        assert!(report.contains("live HTTP reset cases: 3"), "{report}");
        eprintln!("O{opt}: {}", report.trim());
    }
    fs::remove_dir_all(root).unwrap();
}
