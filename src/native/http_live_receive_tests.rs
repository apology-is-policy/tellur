//! Private real-socket transport test; the public HTTP adapter stays synchronous.
use super::*;

#[test]
fn live_owned_receive_readiness_deadlines_and_descriptor_retirement() {
    let root = std::env::temp_dir().join(format!("tellur-live-receive-{}", std::process::id()));
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
    rust.lockfile = repo.join("tests/fixtures/http_receive_mio.lock");
    let p = crate::modules::compile_with_loader(example.join("main.tellur"), manifest, |path| {
        fs::read_to_string(path)
    })
    .unwrap();
    let host = include_str!("../../library/host/loopback_http.rs");
    let constants =
        &host[host.find("const HEADER_LIMIT:").unwrap()..host.find("pub fn http_listen(").unwrap()];
    let failure =
        &host[host.find("fn io_failure(").unwrap()..host.find("fn read_before(").unwrap()];
    for opt in [0, 2] {
        let binary = root.join(format!("live-receive-{opt}"));
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
            include_str!("../../tests/fixtures/http_live_receive_driver.rs"),
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
        assert!(report.contains("live receive cases: 12"), "{report}");
        eprintln!("O{opt}: {}", report.trim());
    }
    fs::remove_dir_all(root).unwrap();
}

#[path = "http_reset_tests.rs"]
mod reset_tests;
