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
    fn nested_calls_preserve_named_paths_and_multiple_scalar_payloads() {
        let f = Fixture::new(
            r#"
            control main(bool pick, string text, bytes data) exits result {
                noop();
                delta relay(text, data) {ready {auto label, auto raw} => prepared}
                [prepared]: delta choose(pick, label, raw);
                [accepted]: emit("accepted"); exit accepted {word, payload};
                [declined]: emit("declined"); exit declined {word, payload};
            }
            control noop() exits done {exit done;}
            control relay(string text, bytes data) exits ready {string word, bytes payload} {
                delta echo(text, data);
                [ready]: exit ready {word, payload};
            }
            control echo(string text, bytes data) exits ready {string word, bytes payload} {
                exit ready {text, data};
            }
            control choose(bool pick, string text, bytes data)
                exits accepted {string word, bytes payload}, declined {string word, bytes payload} {
                delta pick {true=>yes,false=>no}
                [yes]: exit accepted {text, data};
                [no]: exit declined {text, data};
            }
            outcome result {@exit 0 accepted {string text, bytes data}, @exit 7 declined {string text, bytes data}}
        "#,
        );
        let checked = compile_file(
            &f.source,
            Manifest::from_json(&fs::read_to_string(&f.manifest).unwrap()).unwrap(),
        )
        .unwrap();
        for expanded in [false, true] {
            for opt in [0, 2] {
                let mut outputs = vec![];
                for backend in ["llvm", "rust"] {
                    let mut command = f.command(backend, opt);
                    if expanded {
                        command.arg("--expanded");
                    }
                    let built = command.output().unwrap();
                    assert!(
                        built.status.success(),
                        "{}",
                        String::from_utf8_lossy(&built.stderr)
                    );
                    outputs.push(f.root.join(format!("{backend}-{opt}")));
                }
                for pick in [true, false] {
                    let scenario = json!({"entry":"main","arguments":[{"kind":"bool","value":pick},{"kind":"string","value":"😀\u{0}text"},{"kind":"bytes","value":[0,255,128]}],"calls":[]});
                    let path = f.root.join("scenario.json");
                    fs::write(&path, scenario.to_string()).unwrap();
                    let a = execute(&outputs[0], &path, None);
                    let b = execute(&outputs[1], &path, None);
                    assert_eq!(a.status.code(), if pick { Some(0) } else { Some(7) });
                    assert_eq!(a.status.code(), b.status.code());
                    assert_eq!(a.stdout, b.stdout);
                    assert_eq!(a.stderr, b.stderr);
                    let expected = checked
                        .run(&serde_json::from_value(scenario).unwrap())
                        .unwrap();
                    assert_eq!(report(&a), serde_json::to_value(expected).unwrap());
                }
            }
        }
    }

    #[test]
    fn imported_private_calls_keep_activation_ids_locations_and_every_fuel_cut() {
        let f = Fixture::new(
            r#"
            import "helpers.tellur" as helpers;
            control main(bool fail) exits result faults {
                helpers.run(false);
                helpers.run(fail);
                emit("after both calls");
                exit done;
            }
            outcome result {@exit 0 done}
        "#,
        );
        fs::write(
            f.root.join("helpers.tellur"),
            r#"
            pub control run(bool fail) exits done faults {
                delta hidden(fail);
                [done]: exit done;
            }
            control hidden(bool fail) exits done faults {
                emit("child before");
                delta fail {true=>bad,false=>good}
                [bad]: fault "nested stopped 😀";
                [good]: emit("child after"); exit done;
            }
        "#,
        )
        .unwrap();
        for expanded in [false, true] {
            for opt in [0, 2] {
                for backend in ["llvm", "rust"] {
                    let mut command = f.command(backend, opt);
                    if expanded {
                        command.arg("--expanded");
                    }
                    let built = command.output().unwrap();
                    assert!(
                        built.status.success(),
                        "{}",
                        String::from_utf8_lossy(&built.stderr)
                    );
                }
                for fail in [false, true] {
                    let path = f.root.join("scenario.json");
                    fs::write(&path,json!({"entry":"main","arguments":[{"kind":"bool","value":fail}],"calls":[]}).to_string()).unwrap();
                    for fuel in 0..=48 {
                        let a = execute(&f.root.join(format!("llvm-{opt}")), &path, Some(fuel));
                        let b = execute(&f.root.join(format!("rust-{opt}")), &path, Some(fuel));
                        assert_eq!(
                            a.status.code(),
                            b.status.code(),
                            "{expanded}/{opt}/{fail}/{fuel}"
                        );
                        assert_eq!(a.stdout, b.stdout, "{expanded}/{opt}/{fail}/{fuel}");
                        assert_eq!(a.stderr, b.stderr, "{expanded}/{opt}/{fail}/{fuel}");
                    }
                }
            }
        }
    }
    #[test]
    fn owned_records_preserve_partial_moves_named_order_and_every_fuel_cut() {
        let f = Fixture::new(include_str!("../playground/llvm_records.tellur"));
        let checked = compile_file(
            &f.source,
            Manifest::from_json(&fs::read_to_string(&f.manifest).unwrap()).unwrap(),
        )
        .unwrap();
        for expanded in [false, true] {
            for opt in [0, 2] {
                for backend in ["llvm", "rust"] {
                    let mut cmd = f.command(backend, opt);
                    if expanded {
                        cmd.arg("--expanded");
                    }
                    let output = cmd.output().unwrap();
                    assert!(
                        output.status.success(),
                        "{}",
                        String::from_utf8_lossy(&output.stderr)
                    );
                }
                for fail in [false, true] {
                    let scenario = json!({"entry":"main","arguments":[{"kind":"bool","value":fail},{"kind":"string","value":"Ada 😀\u{0}"}],"calls":[]});
                    let path = f.root.join("scenario.json");
                    fs::write(&path, scenario.to_string()).unwrap();
                    for fuel in (0..=64).map(Some).chain([None]) {
                        let a = execute(&f.root.join(format!("llvm-{opt}")), &path, fuel);
                        let b = execute(&f.root.join(format!("rust-{opt}")), &path, fuel);
                        assert_eq!(
                            a.status.code(),
                            b.status.code(),
                            "{expanded}/{opt}/{fail}/{fuel:?}"
                        );
                        assert_eq!(a.stdout, b.stdout, "{expanded}/{opt}/{fail}/{fuel:?}");
                        assert_eq!(a.stderr, b.stderr, "{expanded}/{opt}/{fail}/{fuel:?}");
                        if !fail && fuel.is_none() {
                            let expected = checked
                                .run(&serde_json::from_value(scenario.clone()).unwrap())
                                .unwrap();
                            assert_eq!(report(&a), serde_json::to_value(expected).unwrap());
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn imported_generic_records_and_record_entry_arguments_match_both_executors() {
        let f = Fixture::new(
            r#"
            import "helpers.tellur" as helpers;
            struct packet {bytes data;unit marker;bool flag;}
            control main(packet value) exits result {
                delta helpers.carry(value.data) {ready {auto data}=>ready}
                [ready]: exit done {packet {flag: value.flag, data: data, marker: value.marker}};
            }
            outcome result {@exit 0 done {packet value}}
        "#,
        );
        fs::write(
            f.root.join("helpers.tellur"),
            r#"
            struct box<t> {t value;}
            struct pair<t> {box<t> first;u32 second;}
            pub control carry(bytes input) exits ready {bytes value} {
                auto held=pair<bytes> {second: 3, first: box<bytes> {input}};
                delta hidden(held) {ready {auto result}=>ready}
                [ready]: exit ready {result.first.value};
            }
            control hidden(pair<bytes> input) exits ready {pair<bytes> value} {exit ready {input};}
        "#,
        )
        .unwrap();
        let checked = compile_file(
            &f.source,
            Manifest::from_json(&fs::read_to_string(&f.manifest).unwrap()).unwrap(),
        )
        .unwrap();
        for expanded in [false, true] {
            for opt in [0, 2] {
                for backend in ["llvm", "rust"] {
                    let mut cmd = f.command(backend, opt);
                    if expanded {
                        cmd.arg("--expanded");
                    }
                    let output = cmd.output().unwrap();
                    assert!(
                        output.status.success(),
                        "{}",
                        String::from_utf8_lossy(&output.stderr)
                    );
                }
                let valid = json!({"entry":"main","arguments":[{"kind":"construct","value":{"ty":"packet","name":"packet","fields":[{"kind":"bytes","value":[0,255,128]},{"kind":"unit"},{"kind":"bool","value":true}]}}],"calls":[]});
                let mut invalid = valid.clone();
                invalid["arguments"][0]["value"]["fields"][0] = json!({"kind":"u32","value":9});
                for (scenario, valid) in [(valid, true), (invalid, false)] {
                    let path = f.root.join("scenario.json");
                    fs::write(&path, scenario.to_string()).unwrap();
                    let a = execute(&f.root.join(format!("llvm-{opt}")), &path, None);
                    let b = execute(&f.root.join(format!("rust-{opt}")), &path, None);
                    assert_eq!(a.status.code(), b.status.code());
                    assert_eq!(a.stdout, b.stdout);
                    assert_eq!(a.stderr, b.stderr);
                    assert_eq!(
                        a.status.success(),
                        valid,
                        "{}",
                        String::from_utf8_lossy(&a.stderr)
                    );
                    if valid {
                        let expected = checked
                            .run(&serde_json::from_value(scenario).unwrap())
                            .unwrap();
                        assert_eq!(report(&a), serde_json::to_value(expected).unwrap());
                    }
                }
            }
        }
    }

    #[test]
    fn unsupported_checked_operations_create_no_build_outputs() {
        for source in [
            "struct box {option<u32> value;} control main() exits result {drop(box {some {1}});exit done;} outcome result {@exit 0 done}",
            "variant choice {one,two} control main() exits result {drop(choice.one {});exit done;} outcome result {@exit 0 done}",
            "struct view['a] {ref string value <- 'a;} control main() exits result {string s=\"text\";auto held=view {borrow s};drop(held);exit done;} outcome result {@exit 0 done}",
            "struct box {vector<u32> value;} control main() exits result {drop(box {[1,2]});exit done;} outcome result {@exit 0 done}",
            "struct box {read_error value;} control main(read_error e) exits result {drop(box {e});exit done;} outcome result {@exit 0 done}",
            "control main() exits result {exit done;} async control helper() exits done {exit done;} outcome result {@exit 0 done}",
            "control main() exits result {delta checked_add(1,2) {sum {auto n}=>ok,overflow=>bad} [ok]:exit done; [bad]:exit done;} outcome result {@exit 0 done}",
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
