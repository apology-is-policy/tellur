//! Compile the private owned receiver against the current generated Rust outcomes.
use super::*;
#[test]
fn owned_receive_state_matches_synchronous_outcomes_and_fixed_deadlines() {
    let root = std::env::temp_dir().join(format!("tellur-owned-receive-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let example = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/http_hello");
    let manifest = crate::manifest::Manifest::from_json_at(
        &fs::read_to_string(example.join("manifest.json")).unwrap(),
        &example.join("manifest.json"),
    )
    .unwrap();
    let p = crate::modules::compile_with_loader(example.join("main.tellur"), manifest, |path| {
        fs::read_to_string(path)
    })
    .unwrap();
    let host = include_str!("../../library/host/loopback_http.rs");
    let constants =
        &host[host.find("const HEADER_LIMIT:").unwrap()..host.find("pub fn http_listen(").unwrap()];
    let failure = host[host.find("fn io_failure(").unwrap()..host.find("fn read_before(").unwrap()]
        .to_string();
    let mut baseline = host
        [host.find("fn read_before(").unwrap()..host.find("pub fn http_send(").unwrap()]
        .to_string();
    assert_eq!(baseline.matches("&std::net::TcpStream").count(), 1);
    baseline = baseline.replace("&std::net::TcpStream", "&Stream");
    assert_eq!(baseline.matches("std::time::Instant::now()").count(), 2);
    let before = baseline.find("pub fn http_receive(").unwrap();
    let read = baseline[..before].replace("std::time::Instant::now()", "stream.now()");
    let receive =
        baseline[before..].replace("std::time::Instant::now()", "connection.stream.now()");
    baseline = format!("{read}\n{receive}");
    for opt in [0, 2] {
        let binary = root.join(format!("owned-receive-{opt}"));
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
mod baseline {{use super::*;{baseline}}}
mod operation {{ {} }}
}}
fn main()->std::process::ExitCode {{receive_test::run();std::process::ExitCode::SUCCESS}}
"#,
            include_str!("../../tests/fixtures/http_receive_driver.rs"),
            include_str!("../../tests/fixtures/http_receive_operation.rs")
        ));
        fs::write(&project.2, code).unwrap();
        p.finish_native_build(project, opt).unwrap();
        let output = Command::new(&binary).output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let count = String::from_utf8(output.stdout).unwrap();
        let comparisons = count
            .trim()
            .strip_prefix("receive comparisons: ")
            .unwrap()
            .parse::<usize>()
            .unwrap();
        assert!(comparisons > 1500, "{count}");
        eprintln!("O{opt}: {}", count.trim());
    }
    fs::remove_dir_all(root).unwrap();
}
