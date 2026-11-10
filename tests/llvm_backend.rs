//! The new backend is opt-in. Production source is compared with both existing
//! executors, and unsupported checked programs must fail before output creation.
use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};

struct Fixture {
    root: PathBuf,
    source: PathBuf,
    manifest: PathBuf,
}
impl Fixture {
    fn new(source: &str) -> Self {
        static ID: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "tellur-llvm-{}-{}",
            std::process::id(),
            ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("main.tellur");
        fs::write(&path, source).unwrap();
        let manifest = root.join("manifest.json");
        fs::write(&manifest, include_str!("../playground/manifest.json")).unwrap();
        Self {
            root,
            source: path,
            manifest,
        }
    }
    fn command(&self, backend: &str, opt: u8) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_tellurc"));
        c.arg("build")
            .arg(&self.source)
            .arg(&self.manifest)
            .args(["--backend", backend, "--out"])
            .arg(self.root.join(format!("{backend}-{opt}")));
        if opt == 2 {
            c.arg("--release");
        }
        c
    }
    #[cfg(all(feature = "llvm", target_os = "macos", target_arch = "aarch64"))]
    fn build(&self, backend: &str, opt: u8) -> PathBuf {
        let output = self.command(backend, opt).output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        self.root.join(format!("{backend}-{opt}"))
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn backend_selection_is_explicit_and_unknown_names_are_rejected() {
    let f = Fixture::new(include_str!("../playground/hello.tellur"));
    let bad = f.command("unknown", 0).output().unwrap();
    assert!(!bad.status.success());
    assert!(String::from_utf8_lossy(&bad.stderr).contains("unknown backend"));
    assert!(!f.root.join("unknown-0.tellur-build").exists());
    #[cfg(not(feature = "llvm"))]
    {
        let missing = f.command("llvm", 0).output().unwrap();
        assert!(!missing.status.success());
        assert!(
            String::from_utf8_lossy(&missing.stderr)
                .contains("requires a compiler built with the llvm feature")
        );
        assert!(!f.root.join("llvm-0.tellur-build").exists());
    }
}

#[cfg(all(feature = "llvm", target_os = "macos", target_arch = "aarch64"))]
mod enabled {
    use super::*;
    use serde_json::{Value as Json, json};
    use std::{path::Path, process::Output};
    use tellur::{compile_file, manifest::Manifest, runtime::Scenario};
    fn execute(binary: &Path, scenario: &Path, fuel: Option<usize>) -> Output {
        let mut c = Command::new(binary);
        c.arg("--scenario").arg(scenario);
        if let Some(n) = fuel {
            c.arg("--fuel").arg(n.to_string());
        }
        c.output().unwrap()
    }
    fn report(out: &Output) -> Json {
        serde_json::from_slice(&out.stdout).unwrap_or_else(|_| {
            panic!(
                "{}\n{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            )
        })
    }
    #[test]
    fn scalar_branches_moves_and_multiple_entries_match_both_executors() {
        let f = Fixture::new(
            r#"
control main(bool pick, u32 number, string text) exits result {
    mutable u32 current=number;
    current=9;
    string original=text;
    delta pick && number < 10 {true=>yes,false=>no}
    [yes]: emit(concat(original, u32_text(current))); exit accepted {current};
    [no]: emit("other 😀"); exit declined {number};
}
control other(bytes data, unit nothing) exits bytes_result {exit done {data, nothing};}
control logic(bool a, bool b) exits logic_result {exit done {!a || a == b};}
outcome result {@exit 0 accepted {u32 value}, @exit 7 declined {u32 value}}
outcome bytes_result {@exit 0 done {bytes data, unit nothing}}
outcome logic_result {@exit 0 done {bool value}}
"#,
        );
        let p = compile_file(
            &f.source,
            Manifest::from_json(&fs::read_to_string(&f.manifest).unwrap()).unwrap(),
        )
        .unwrap();
        let mut scenarios = vec![];
        for pick in [true, false] {
            for number in [0, 9, 10, u32::MAX] {
                scenarios.push(json!({"entry":"main","arguments":[{"kind":"bool","value":pick},{"kind":"u32","value":number},{"kind":"string","value":"Unicode 😀 and \u{0} NUL"}],"calls":[]}));
            }
        }
        scenarios.push(json!({"entry":"other","arguments":[{"kind":"bytes","value":[0,255,128]},{"kind":"unit"}],"calls":[]}));
        for a in [false, true] {
            for b in [false, true] {
                scenarios.push(json!({"entry":"logic","arguments":[{"kind":"bool","value":a},{"kind":"bool","value":b}],"calls":[]}));
            }
        }
        for opt in [0, 2] {
            let llvm = f.build("llvm", opt);
            let rust = f.build("rust", opt);
            let ir = fs::read_to_string(llvm.with_extension("ll")).unwrap();
            assert!(ir.contains("define void @control_"));
            assert!(ir.contains("br i1"));
            assert!(!ir.contains("switch i64"));
            assert!(
                !f.root
                    .join(format!("llvm-{opt}.tellur-build/main.rs"))
                    .exists()
            );
            for scenario in &scenarios {
                let file = f.root.join("scenario.json");
                fs::write(&file, scenario.to_string()).unwrap();
                let a = execute(&llvm, &file, None);
                let b = execute(&rust, &file, None);
                assert_eq!(a.status.code(), b.status.code());
                assert_eq!(report(&a), report(&b));
                let s: Scenario = serde_json::from_value(scenario.clone()).unwrap();
                assert_eq!(
                    report(&a),
                    serde_json::to_value(p.run(&s).unwrap()).unwrap()
                );
            }
        }
    }
    #[test]
    fn faults_and_every_fuel_cut_keep_existing_reports_and_effects() {
        let f = Fixture::new(
            r#"control main(bool fail) exits result faults {emit("before");delta fail {true=>bad,false=>good} [bad]: fault "stopped 😀"; [good]: emit("after");exit done;} outcome result {@exit 0 done}"#,
        );
        for opt in [0, 2] {
            let llvm = f.build("llvm", opt);
            let rust = f.build("rust", opt);
            for fail in [true, false] {
                for fuel in 0..=14 {
                    let scenario = f.root.join("scenario.json");
                    fs::write(&scenario,json!({"entry":"main","arguments":[{"kind":"bool","value":fail}],"calls":[]}).to_string()).unwrap();
                    let a = execute(&llvm, &scenario, Some(fuel));
                    let b = execute(&rust, &scenario, Some(fuel));
                    assert_eq!(a.status.code(), b.status.code(), "{opt}/{fail}/{fuel}");
                    assert_eq!(a.stdout, b.stdout, "{opt}/{fail}/{fuel}");
                    assert_eq!(a.stderr, b.stderr, "{opt}/{fail}/{fuel}");
                }
            }
        }
    }
    #[test]
    fn expanded_scalar_calls_preserve_completion_bindings_and_path_guards() {
        let f = Fixture::new(
            r#"control main(bool pick, u32 number) exits result {
                delta helper(pick, number) { done {auto value} => chosen }
                [chosen]: emit(u32_text(value)); exit done {value};
            }
            control helper(bool pick, u32 number) exits done {u32 value} {
                delta pick {true=>yes,false=>no}
                [yes]: exit done {number};
                [no]: exit done {42};
            }
            outcome result {@exit 0 done {u32 value}}"#,
        );
        for opt in [0, 2] {
            let out = f.command("llvm", opt).arg("--expanded").output().unwrap();
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
            let rust = f.build("rust", opt);
            for pick in [true, false] {
                let file = f.root.join("scenario.json");
                fs::write(&file, json!({"entry":"main","arguments":[{"kind":"bool","value":pick},{"kind":"u32","value":23}],"calls":[]}).to_string()).unwrap();
                let llvm = execute(&f.root.join(format!("llvm-{opt}")), &file, None);
                let rust = execute(&rust, &file, None);
                assert_eq!(llvm.status.code(), rust.status.code());
                assert_eq!(report(&llvm), report(&rust));
            }
        }
    }
    #[test]
    fn unsupported_checked_operations_create_no_build_outputs() {
        for source in [
            "control main() exits result {exit done;} async control helper() exits done {exit done;} outcome result {@exit 0 done}",
            "control main() exits result {helper();exit done;} control helper() exits done {exit done;} outcome result {@exit 0 done}",
            "control main() exits result {loop L(){again L();}} outcome result {@exit 0 done}",
            "control main() exits result {fault boundary {exit done;} contingency [] {terminate process 1;}} outcome result {@exit 0 done}",
        ] {
            let f = Fixture::new(source);
            let out = f.command("llvm", 0).output().unwrap();
            assert!(!out.status.success());
            assert!(
                String::from_utf8_lossy(&out.stderr).contains("E_BACKEND"),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
            assert!(!f.root.join("llvm-0.tellur-build").exists());
            assert!(!f.root.join("llvm-0.ll").exists());
        }
        let f = Fixture::new(include_str!("../playground/hello.tellur"));
        let mut manifest: Json =
            serde_json::from_str(&fs::read_to_string(&f.manifest).unwrap()).unwrap();
        manifest["launch"] = json!({"main": {"signals": "cancel"}});
        fs::write(&f.manifest, manifest.to_string()).unwrap();
        let out = f.command("llvm", 0).output().unwrap();
        assert!(!out.status.success());
        assert!(
            String::from_utf8_lossy(&out.stderr)
                .contains("does not yet support launch signal policies")
        );
        assert!(!f.root.join("llvm-0.tellur-build").exists());
    }
}
