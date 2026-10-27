use std::{fs, process::Command};

#[test]
fn canonical_and_legacy_commands_report_tellur_and_check_both_extensions() {
    let root = std::env::temp_dir().join(format!("tellur-branding-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("manifest.json"),
        r#"{"version":"0.1","types":[],"primitives":[]}"#,
    )
    .unwrap();
    fs::write(
        root.join("helper.path"),
        "pub control finish() exits done {exit done;}",
    )
    .unwrap();
    let source =
        "import \"helper.path\" as helper; control main() exits done {forward helper.finish();}";
    for extension in ["tellur", "path"] {
        fs::write(root.join(format!("main.{extension}")), source).unwrap();
    }
    for compiler in [env!("CARGO_BIN_EXE_tellurc"), env!("CARGO_BIN_EXE_pathc")] {
        let version = Command::new(compiler).arg("--version").output().unwrap();
        assert!(version.status.success());
        assert!(
            String::from_utf8_lossy(&version.stdout)
                .starts_with(concat!("tellurc ", env!("CARGO_PKG_VERSION")))
        );
        let help = Command::new(compiler).arg("--help").output().unwrap();
        assert!(help.status.success());
        assert!(String::from_utf8_lossy(&help.stdout).contains("Tellur compiler"));
        for extension in ["tellur", "path"] {
            let checked = Command::new(compiler)
                .arg("check")
                .arg(root.join(format!("main.{extension}")))
                .output()
                .unwrap();
            assert!(
                checked.status.success(),
                "{}",
                String::from_utf8_lossy(&checked.stderr)
            );
        }
    }
    fs::remove_dir_all(root).unwrap();
}
