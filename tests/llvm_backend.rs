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
            assert!(ir.contains("call void @tellur_rt_v1_frame"));
            assert!(ir.contains("call i64 @tellur_rt_v1_local"));
            assert!(ir.contains("call void @tellur_rt_v1_store"));
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
    fn tagged_values_and_captured_outcomes_preserve_branches_faults_and_fuel() {
        let f = Fixture::new(include_str!("../playground/llvm_tagged.tellur"));
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
                for mode in 0..=2 {
                    for keep in [true, false] {
                        for fail in [false, true] {
                            let scenario = json!({"entry":"main","arguments":[{"kind":"u32","value":mode},{"kind":"bool","value":keep},{"kind":"bool","value":fail},{"kind":"string","value":"tag 😀\u{0}"}],"calls":[]});
                            let path = f.root.join("scenario.json");
                            fs::write(&path, scenario.to_string()).unwrap();
                            let fuels: Vec<_> = if mode == 0 && keep {
                                (0..=80).map(Some).chain([None]).collect()
                            } else {
                                vec![None]
                            };
                            for fuel in fuels {
                                let a = execute(&f.root.join(format!("llvm-{opt}")), &path, fuel);
                                let b = execute(&f.root.join(format!("rust-{opt}")), &path, fuel);
                                let case =
                                    format!("{expanded}/{opt}/{mode}/{keep}/{fail}/{fuel:?}");
                                assert_eq!(a.status.code(), b.status.code(), "{case}");
                                assert_eq!(a.stdout, b.stdout, "{case}");
                                assert_eq!(a.stderr, b.stderr, "{case}");
                                if fuel.is_none() && !(mode == 0 && keep && fail) {
                                    assert_eq!(
                                        a.status.code(),
                                        Some(if mode == 0 && keep { 0 } else { 7 })
                                    );
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
        }
    }

    #[test]
    fn imported_generic_tags_and_copy_decisions_match_reference() {
        let f = Fixture::new(
            r#"
            import "helpers.tellur" as helpers;
            variant bit {on,off}
            outcome result {@exit 0 done {option<bytes> data}}
            control main(bool pick, bytes data) exits result {
                bit flag=bit.on {};
                delta flag {on=>first,off=>other}
                [first|other]: delta flag {on=>second,off=>unused}
                [second|unused]: delta helpers.run(pick,data) {ready {auto value}=>ready}
                [ready]: exit done {value};
            }
        "#,
        );
        fs::write(
            f.root.join("helpers.tellur"),
            r#"
            struct wrapper<t> {option<t> value;}
            variant maybe<t> {yes {wrapper<t> value},no}
            outcome response<t> {value {maybe<t> data},missing}
            pub control run(bool pick, bytes input) exits ready {option<bytes> value} {
                response<bytes> captured=capture hidden(pick,input);
                delta captured {value {auto data}=>value,missing=>missing}
                [value]: delta data {yes {auto box}=>yes,no=>no}
                [yes]: exit ready {box.value};
                [no|missing]: exit ready {none};
            }
            control hidden(bool pick, bytes input) exits response<bytes> {
                delta pick {true=>yes,false=>no}
                [yes]: exit value {maybe<bytes>.yes {wrapper<bytes> {some {input}}}};
                [no]: exit missing;
            }
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
                for pick in [true, false] {
                    let scenario = json!({"entry":"main","arguments":[{"kind":"bool","value":pick},{"kind":"bytes","value":[0,255,128]}],"calls":[]});
                    let path = f.root.join("scenario.json");
                    fs::write(&path, scenario.to_string()).unwrap();
                    let a = execute(&f.root.join(format!("llvm-{opt}")), &path, None);
                    let b = execute(&f.root.join(format!("rust-{opt}")), &path, None);
                    assert!(a.status.success(), "{}", String::from_utf8_lossy(&a.stderr));
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

    fn build_pair(f: &Fixture, expanded: bool, opt: u8) {
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
    }

    #[test]
    fn direct_references_preserve_returned_loans_mutation_faults_and_fuel() {
        let f = Fixture::new(include_str!("../playground/llvm_references.tellur"));
        let checked = compile_file(
            &f.source,
            Manifest::from_json(&fs::read_to_string(&f.manifest).unwrap()).unwrap(),
        )
        .unwrap();
        for expanded in [false, true] {
            for opt in [0, 2] {
                build_pair(&f, expanded, opt);
                let ir = fs::read_to_string(f.root.join(format!("llvm-{opt}.ll"))).unwrap();
                assert!(ir.contains("call i64 @tellur_rt_v1_evidence"));
                for fail in [false, true] {
                    let scenario = json!({"entry":"main","arguments":[{"kind":"bool","value":fail}],"calls":[]});
                    let path = f.root.join("scenario.json");
                    fs::write(&path, scenario.to_string()).unwrap();
                    for fuel in (0..=55).map(Some).chain([None]) {
                        let a = execute(&f.root.join(format!("llvm-{opt}")), &path, fuel);
                        let b = execute(&f.root.join(format!("rust-{opt}")), &path, fuel);
                        assert_eq!(
                            a.status.code(),
                            b.status.code(),
                            "{expanded}/{opt}/{fail}/{fuel:?}: {a:?}; {b:?}"
                        );
                        assert_eq!(a.stdout, b.stdout, "{expanded}/{opt}/{fail}/{fuel:?}");
                        assert_eq!(a.stderr, b.stderr, "{expanded}/{opt}/{fail}/{fuel:?}");
                        if !fail && fuel.is_none() {
                            assert_eq!(
                                report(&a),
                                serde_json::to_value(
                                    checked
                                        .run(&serde_json::from_value(scenario.clone()).unwrap())
                                        .unwrap()
                                )
                                .unwrap()
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn borrowed_decisions_conditional_origins_and_temporary_arguments_match_both_executors() {
        let f = Fixture::new(
            r#"
control choose(bool pick, ref string left, ref string right) exits auto {
    exit delta pick {true=>chosen {left},false=>chosen {right}};
}
control display(ref string input) exits done {emit(input);exit done;}
control adjust(ref mutable option<string> input) exits done {
    delta input {some {auto text}=>present,none=>absent}
    [present]:deref text="changed payload";
    [entry]:exit done;
}
control main(bool pick, option<string> input) exits result {
    string left="left";string right="right";
    ref string first=borrow left;ref string second=first;emit(second);
    delta choose(pick,borrow left,borrow right) {chosen {auto selected}=>selected}
    [selected]:
        emit(selected);
        display(borrow concat("temporary ","argument"));
        mutable option<string> owned=input;
        adjust(borrow mutable owned);
        exit done {owned};
}
outcome result {@exit 0 done {option<string> value}}
"#,
        );
        let checked = compile_file(
            &f.source,
            Manifest::from_json(&fs::read_to_string(&f.manifest).unwrap()).unwrap(),
        )
        .unwrap();
        for expanded in [false, true] {
            for opt in [0, 2] {
                build_pair(&f, expanded, opt);
                for pick in [false, true] {
                    for present in [false, true] {
                        let option = json!({"kind":"construct","value":{"ty":"Option<String>","name":if present {"Some"} else {"None"},"fields":if present {vec![json!({"kind":"string","value":"original 😀"})]} else {vec![]}}});
                        let scenario = json!({"entry":"main","arguments":[{"kind":"bool","value":pick},option],"calls":[]});
                        let path = f.root.join("scenario.json");
                        fs::write(&path, scenario.to_string()).unwrap();
                        let a = execute(&f.root.join(format!("llvm-{opt}")), &path, None);
                        let b = execute(&f.root.join(format!("rust-{opt}")), &path, None);
                        assert!(a.status.success(), "{a:?}");
                        assert_eq!(a.status.code(), b.status.code(), "{a:?}; {b:?}");
                        assert_eq!(a.stdout, b.stdout);
                        assert_eq!(a.stderr, b.stderr);
                        assert_eq!(
                            report(&a),
                            serde_json::to_value(
                                checked
                                    .run(&serde_json::from_value(scenario).unwrap())
                                    .unwrap()
                            )
                            .unwrap()
                        );
                    }
                }
            }
        }
    }

    fn carrier_matrix(source: &str, scenarios: &[Json], fuels: &[Option<usize>]) {
        let f = Fixture::new(source);
        // These reused language fixtures declare outcomes without CLI policy.
        let mut manifest: Json =
            serde_json::from_str(&fs::read_to_string(&f.manifest).unwrap()).unwrap();
        manifest["launch"]["main"]["exit_codes"] = json!({"done":0});
        fs::write(&f.manifest, manifest.to_string()).unwrap();
        let checked = compile_file(
            &f.source,
            Manifest::from_json(&fs::read_to_string(&f.manifest).unwrap()).unwrap(),
        )
        .unwrap();
        manifest["launch"]["main"]["exit_codes"] = checked
            .region("main")
            .unwrap()
            .exits
            .iter()
            .map(|exit| (exit.name.clone(), json!(0)))
            .collect::<serde_json::Map<String, Json>>()
            .into();
        fs::write(&f.manifest, manifest.to_string()).unwrap();
        for expanded in [false, true] {
            for opt in [0, 2] {
                build_pair(&f, expanded, opt);
                for scenario in scenarios {
                    let path = f.root.join("scenario.json");
                    fs::write(&path, scenario.to_string()).unwrap();
                    for &fuel in fuels {
                        let a = execute(&f.root.join(format!("llvm-{opt}")), &path, fuel);
                        let b = execute(&f.root.join(format!("rust-{opt}")), &path, fuel);
                        assert_eq!(
                            a.status.code(),
                            b.status.code(),
                            "{expanded}/{opt}/{scenario}/{fuel:?}: {a:?}; {b:?}"
                        );
                        assert_eq!(a.stdout, b.stdout, "{expanded}/{opt}/{scenario}/{fuel:?}");
                        assert_eq!(a.stderr, b.stderr, "{expanded}/{opt}/{scenario}/{fuel:?}");
                        if fuel.is_none() && a.status.success() {
                            assert_eq!(
                                report(&a),
                                serde_json::to_value(
                                    checked
                                        .run(&serde_json::from_value(scenario.clone()).unwrap())
                                        .unwrap()
                                )
                                .unwrap()
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn reference_carriers_preserve_guarded_leaves_whole_results_faults_and_fuel() {
        let scenarios = [false, true].map(
            |fail| json!({"entry":"main","arguments":[{"kind":"bool","value":fail}],"calls":[]}),
        );
        carrier_matrix(
            include_str!("../playground/llvm_carriers.tellur"),
            &scenarios,
            &(0..=85).map(Some).chain([None]).collect::<Vec<_>>(),
        );
        carrier_matrix(
            include_str!("fixtures/carrier_reborrows.tellur"),
            &[json!({"entry":"main","arguments":[],"calls":[]})],
            &[None],
        );
    }

    // Reuse the accepted whole-carrier source examples, replacing only task
    // creation/join/yield with their synchronous call equivalents.
    fn synchronous_carriers(source: &str) -> String {
        let source = source
            .replace("async control", "control")
            .replace("yield;", "")
            .replace("forward run ", "forward ")
            .replace("await ", "");
        let mut calls = std::collections::BTreeMap::new();
        let mut result = String::new();
        for line in source.lines() {
            let text = line.trim();
            if let Some((name, call)) = text
                .strip_prefix("auto ")
                .and_then(|s| s.split_once(" = spawn "))
            {
                calls.insert(name.to_owned(), call.trim_end_matches(';').to_owned());
                continue;
            }
            if let Some((name, original)) = text
                .strip_prefix("auto ")
                .and_then(|s| s.split_once(" = move "))
            {
                if let Some(call) = calls.get(original.trim_end_matches(';')).cloned() {
                    calls.insert(name.to_owned(), call);
                    continue;
                }
            }
            let mut line = line.to_owned();
            for (name, call) in &calls {
                line = line.replace(&format!("delta {name} {{"), &format!("delta {call} {{"));
            }
            result.push_str(&line);
            result.push('\n');
        }
        result
    }

    #[test]
    fn whole_record_and_outcome_results_preserve_selected_origins_and_empty_alternatives() {
        for source in [
            include_str!("../playground/borrowed_record_spawn_results.tellur"),
            include_str!("../playground/borrowed_outcome_spawn_results.tellur"),
            include_str!("fixtures/source_borrowed_outcome_result_guards.tellur"),
        ] {
            carrier_matrix(
                &synchronous_carriers(source),
                &[json!({"entry":"main","arguments":[],"calls":[]})],
                &[None],
            );
        }
    }

    #[test]
    fn nested_options_variants_and_shared_reference_copies_match_both_executors() {
        let source = r#"
struct view<t>['a] {ref t value <- 'a;}
variant choice['a] {held {view<string>['a] value},empty}
control make(ref string input, bool present) exits ready {option<choice>[input] value} {
    exit delta present {true=>ready {some {choice.held {view<string> {input}}}},false=>ready {some {choice.empty {}}}};
}
control main(bool present) exits result {
    string owner="nested shared";
    delta make(borrow owner,present) {ready {auto value}=>ready}
    [ready]:delta value {some {auto selected}=>some,none=>none}
    [some]:delta selected {held {auto leaf}=>held,empty=>empty}
    [held]:ref string copy=leaf.value;emit(leaf.value);emit(copy);
    [entry]:exit done;
}
outcome result {@exit 0 done}
"#;
        carrier_matrix(source, &[false,true].map(|present| json!({"entry":"main","arguments":[{"kind":"bool","value":present}],"calls":[]})), &[None]);
    }

    #[test]
    fn live_owned_primitives_match_reference_scripts_and_rust() {
        let f = Fixture::new(include_str!("../playground/llvm_primitives.tellur"));
        let checked = compile_file(
            &f.source,
            Manifest::from_json(&fs::read_to_string(&f.manifest).unwrap()).unwrap(),
        )
        .unwrap();
        let n = |n: u32| json!({"kind":"u32","value":n});
        let call = |name: &str, outcome: &str, fields: Vec<Json>| json!({"operation":name,"outcome":outcome,"fields":fields});
        let cases = [
            (
                "12",
                3,
                2,
                0,
                vec![
                    call("parse_u32", "number", vec![n(12)]),
                    call("checked_add", "sum", vec![n(15)]),
                    call("checked_mul", "product", vec![n(30)]),
                ],
            ),
            ("bad", 0, 1, 7, vec![call("parse_u32", "malformed", vec![])]),
            (
                "4294967296",
                0,
                1,
                7,
                vec![call("parse_u32", "overflow", vec![])],
            ),
            (
                "4294967295",
                1,
                1,
                7,
                vec![
                    call("parse_u32", "number", vec![n(u32::MAX)]),
                    call("checked_add", "overflow", vec![]),
                ],
            ),
            (
                "2147483648",
                0,
                2,
                7,
                vec![
                    call("parse_u32", "number", vec![n(2147483648)]),
                    call("checked_add", "sum", vec![n(2147483648)]),
                    call("checked_mul", "overflow", vec![]),
                ],
            ),
        ];
        for expanded in [false, true] {
            for opt in [0, 2] {
                build_pair(&f, expanded, opt);
                for (input, add, mul, status, calls) in &cases {
                    let scenario = json!({"entry":"main","arguments":[{"kind":"string","value":input},n(*add),n(*mul)],"calls":calls});
                    let a = Command::new(f.root.join(format!("llvm-{opt}")))
                        .args(["--json", input, &add.to_string(), &mul.to_string()])
                        .output()
                        .unwrap();
                    let b = Command::new(f.root.join(format!("rust-{opt}")))
                        .args(["--json", input, &add.to_string(), &mul.to_string()])
                        .output()
                        .unwrap();
                    assert_eq!(a.status.code(), Some(*status), "LLVM {a:?}");
                    assert_eq!(
                        a.status.code(),
                        b.status.code(),
                        "{expanded}/{opt}/{input}/{add}/{mul}: LLVM {a:?}; Rust {b:?}"
                    );
                    assert_eq!(a.stdout, b.stdout);
                    assert_eq!(a.stderr, b.stderr);
                    assert_eq!(
                        report(&a),
                        serde_json::to_value(
                            checked
                                .run(&serde_json::from_value(scenario).unwrap())
                                .unwrap()
                        )
                        .unwrap()
                    );
                }
                for (bytes, text) in [
                    (vec![65, 0, 240, 159, 152, 128], Some("A\0😀")),
                    (vec![255], None),
                ] {
                    let value = json!({"kind":"bytes","value":bytes});
                    let calls = vec![call(
                        "decode_utf8",
                        if text.is_some() { "text" } else { "invalid" },
                        text.map(|text| vec![json!({"kind":"string","value":text})])
                            .unwrap_or_default(),
                    )];
                    let scenario =
                        json!({"entry":"decode","arguments":[value.clone()],"calls":calls});
                    let a = Command::new(f.root.join(format!("llvm-{opt}")))
                        .args(["--json", "--entry", "decode", &value.to_string()])
                        .output()
                        .unwrap();
                    let b = Command::new(f.root.join(format!("rust-{opt}")))
                        .args(["--json", "--entry", "decode", &value.to_string()])
                        .output()
                        .unwrap();
                    assert_eq!(a.status.code(), Some(if text.is_some() { 0 } else { 7 }));
                    assert_eq!(a.stdout, b.stdout);
                    assert_eq!(a.stderr, b.stderr);
                    assert_eq!(
                        report(&a),
                        serde_json::to_value(
                            checked
                                .run(&serde_json::from_value(scenario).unwrap())
                                .unwrap()
                        )
                        .unwrap()
                    );
                }
            }
        }
    }

    #[test]
    fn scripted_owned_primitive_payloads_errors_and_fuel_match_both_executors() {
        let f = Fixture::new(
            r#"
            control main(string text) exits result {
                delta helper(packet {text,9}) {got {auto reply}=>got}
                [got]: delta reply {some {auto value}=>some,none=>none}
                [some]: exit ready {value};
                [none]: exit empty;
            }
            control helper(packet input) exits got {option<packet> reply} {
                delta transfer(input) {ready {auto reply}=>ready,rejected=>rejected}
                [ready]: exit got {reply};
                [rejected]: exit got {none};
            }
            outcome result {@exit 0 ready {packet value},@exit 7 empty}
        "#,
        );
        let mut manifest: Json =
            serde_json::from_str(&fs::read_to_string(&f.manifest).unwrap()).unwrap();
        manifest["types"].as_array_mut().unwrap().push(json!({"kind":"record","name":"packet","fields":[{"name":"text","type":"String"},{"name":"number","type":"U32"}]}));
        manifest["primitives"].as_array_mut().unwrap().push(json!({"name":"transfer","parameters":[{"name":"input","type":"packet","mode":"consume"}],"outcomes":[{"name":"ready","fields":[{"name":"reply","type":"Option<packet>"}]},{"name":"rejected","fields":[]}],"effects":["transfer"],"returns":true}));
        fs::write(&f.manifest, manifest.to_string()).unwrap();
        let checked = compile_file(
            &f.source,
            Manifest::from_json(&manifest.to_string()).unwrap(),
        )
        .unwrap();
        let packet = json!({"kind":"construct","value":{"ty":"packet","name":"packet","fields":[{"kind":"string","value":"returned 😀"},{"kind":"u32","value":42}]}});
        let some = json!({"kind":"construct","value":{"ty":"Option<packet>","name":"Some","fields":[packet]}});
        let none =
            json!({"kind":"construct","value":{"ty":"Option<packet>","name":"None","fields":[]}});
        let good = json!({"operation":"transfer","outcome":"ready","fields":[some]});
        let mut waits = good.clone();
        waits["waits"] = json!(1);
        let mut wrong_operation = good.clone();
        wrong_operation["operation"] = json!("different");
        let scripts = [
            (vec![good], Some(0)),
            (
                vec![json!({"operation":"transfer","outcome":"ready","fields":[none]})],
                Some(7),
            ),
            (
                vec![json!({"operation":"transfer","outcome":"rejected","fields":[]})],
                Some(7),
            ),
            (vec![], None),
            (vec![waits], None),
            (vec![wrong_operation], None),
            (
                vec![json!({"operation":"transfer","outcome":"unknown","fields":[]})],
                None,
            ),
            (
                vec![
                    json!({"operation":"transfer","outcome":"ready","fields":[{"kind":"u32","value":5}]}),
                ],
                None,
            ),
        ];
        for expanded in [false, true] {
            for opt in [0, 2] {
                build_pair(&f, expanded, opt);
                for (index, (calls, status)) in scripts.iter().enumerate() {
                    let scenario = json!({"entry":"main","arguments":[{"kind":"string","value":"sent"}],"calls":calls});
                    let path = f.root.join("scenario.json");
                    fs::write(&path, scenario.to_string()).unwrap();
                    let fuels: Vec<_> = if index == 0 {
                        (0..=36).map(Some).chain([None]).collect()
                    } else {
                        vec![None]
                    };
                    for fuel in fuels {
                        let a = execute(&f.root.join(format!("llvm-{opt}")), &path, fuel);
                        let b = execute(&f.root.join(format!("rust-{opt}")), &path, fuel);
                        assert_eq!(
                            a.status.code(),
                            b.status.code(),
                            "{expanded}/{opt}/{index}/{fuel:?}"
                        );
                        assert_eq!(a.stdout, b.stdout);
                        assert_eq!(a.stderr, b.stderr);
                        if fuel.is_none() {
                            if let Some(status) = status {
                                assert_eq!(a.status.code(), Some(*status));
                                assert_eq!(
                                    report(&a),
                                    serde_json::to_value(
                                        checked
                                            .run(&serde_json::from_value(scenario.clone()).unwrap())
                                            .unwrap()
                                    )
                                    .unwrap()
                                );
                            } else {
                                assert!(!a.status.success());
                            }
                        }
                    }
                }
                // An unknown live adapter still fails through the existing Host diagnostic.
                let a = Command::new(f.root.join(format!("llvm-{opt}")))
                    .args(["--json", "sent"])
                    .output()
                    .unwrap();
                let b = Command::new(f.root.join(format!("rust-{opt}")))
                    .args(["--json", "sent"])
                    .output()
                    .unwrap();
                assert!(!a.status.success());
                assert_eq!(a.status.code(), b.status.code());
                assert_eq!(a.stdout, b.stdout);
                assert_eq!(a.stderr, b.stderr);
            }
        }
    }

    #[test]
    fn live_resources_borrow_read_and_consume_close_through_owned_records() {
        let f = Fixture::new(include_str!("../playground/llvm_resources.tellur"));
        let file = f.root.join("input.dat");
        let bytes = vec![0, 65, 255, 10];
        fs::write(&file, &bytes).unwrap();
        let checked = compile_file(
            &f.source,
            Manifest::from_json(&fs::read_to_string(&f.manifest).unwrap()).unwrap(),
        )
        .unwrap();
        let handle = json!({"kind":"opaque","value":{"ty":"handle","token":"file-0"}});
        for expanded in [false, true] {
            for opt in [0, 2] {
                build_pair(&f, expanded, opt);
                for (filename, status, calls) in [
                    (
                        file.clone(),
                        0,
                        vec![
                            json!({"operation":"open","outcome":"opened","fields":[handle.clone()]}),
                            json!({"operation":"read","outcome":"data","fields":[{"kind":"bytes","value":bytes}]}),
                            json!({"operation":"close","outcome":"closed","fields":[]}),
                        ],
                    ),
                    (
                        f.root.join("absent"),
                        3,
                        vec![json!({"operation":"open","outcome":"not_found","fields":[]})],
                    ),
                ] {
                    let args = [filename.to_str().unwrap(), "false"];
                    let a = Command::new(f.root.join(format!("llvm-{opt}")))
                        .arg("--json")
                        .args(args)
                        .output()
                        .unwrap();
                    let b = Command::new(f.root.join(format!("rust-{opt}")))
                        .arg("--json")
                        .args(args)
                        .output()
                        .unwrap();
                    assert_eq!(a.status.code(), Some(status), "{a:?}");
                    assert_eq!(a.status.code(), b.status.code(), "{a:?}; {b:?}");
                    assert_eq!(a.stdout, b.stdout);
                    assert_eq!(a.stderr, b.stderr);
                    let scenario = json!({"entry":"main","arguments":[{"kind":"string","value":filename},{"kind":"bool","value":false}],"calls":calls});
                    assert_eq!(
                        report(&a),
                        serde_json::to_value(
                            checked
                                .run(&serde_json::from_value(scenario).unwrap())
                                .unwrap()
                        )
                        .unwrap()
                    );
                }
            }
        }
    }

    #[test]
    fn scripted_resources_errors_faults_and_fuel_match_both_executors() {
        let f = Fixture::new(include_str!("../playground/llvm_resources.tellur"));
        let checked = compile_file(
            &f.source,
            Manifest::from_json(&fs::read_to_string(&f.manifest).unwrap()).unwrap(),
        )
        .unwrap();
        let handle = json!({"kind":"opaque","value":{"ty":"handle","token":"owned-token"}});
        let mut scenarios = vec![];
        for read_ok in [false, true] {
            for close_ok in [false, true] {
                for fail in [false, true] {
                    let read = if read_ok {
                        json!({"operation":"read","outcome":"data","fields":[{"kind":"bytes","value":[42]}]})
                    } else {
                        json!({"operation":"read","outcome":"failed","fields":[{"kind":"opaque","value":{"ty":"read_error","token":"read failed"}}]})
                    };
                    let close = if close_ok {
                        json!({"operation":"close","outcome":"closed","fields":[]})
                    } else {
                        json!({"operation":"close","outcome":"failed","fields":[{"kind":"opaque","value":{"ty":"close_error","token":"close failed"}}]})
                    };
                    scenarios.push(json!({"entry":"main","arguments":[{"kind":"string","value":"scripted"},{"kind":"bool","value":fail}],"calls":[{"operation":"open","outcome":"opened","fields":[handle.clone()]},read,close]}));
                }
            }
        }
        for outcome in ["not_found", "denied"] {
            scenarios.push(json!({"entry":"main","arguments":[{"kind":"string","value":"scripted"},{"kind":"bool","value":false}],"calls":[{"operation":"open","outcome":outcome,"fields":[]}]}));
        }
        let mut malformed = scenarios[0].clone();
        malformed["calls"][1]["fields"][0]["value"]["ty"] = json!("close_error");
        scenarios.push(malformed);
        for expanded in [false, true] {
            for opt in [0, 2] {
                build_pair(&f, expanded, opt);
                for (index, scenario) in scenarios.iter().enumerate() {
                    let path = f.root.join("scenario.json");
                    fs::write(&path, scenario.to_string()).unwrap();
                    for fuel in (0..=35).map(Some).chain([None]) {
                        let a = execute(&f.root.join(format!("llvm-{opt}")), &path, fuel);
                        let b = execute(&f.root.join(format!("rust-{opt}")), &path, fuel);
                        assert_eq!(
                            a.status.code(),
                            b.status.code(),
                            "{expanded}/{opt}/{index}/{fuel:?}: {a:?}; {b:?}"
                        );
                        assert_eq!(a.stdout, b.stdout);
                        assert_eq!(a.stderr, b.stderr);
                        if fuel.is_none() && index < 10 && !(index == 7) {
                            assert_eq!(
                                report(&a),
                                serde_json::to_value(
                                    checked
                                        .run(&serde_json::from_value(scenario.clone()).unwrap())
                                        .unwrap()
                                )
                                .unwrap()
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn resource_options_and_references_retain_owner_until_consuming_call() {
        let source = r#"
control maybe(handle input,bool keep) exits ready {option<handle> value} {
    [entry]:delta keep {true=>keep,false=>discard}
    [keep]:exit ready {some {input}};
    [discard]:delta close(input) {closed=>closed,failed {auto error}=>failed}
    [closed|failed]:exit ready {none};
}
control read_ref(ref handle input) exits done {
    delta read(input) {data {auto bytes}=>data,failed {auto error}=>failed}
    [entry]:exit done;
}
control main(bool keep) exits result {
    delta open("scripted") {opened {auto owner}=>opened,not_found=>absent,denied=>denied}
    [opened]:delta maybe(owner,keep) {ready {auto value}=>ready}
    [ready]:delta value {some {auto file}=>some,none=>none}
    [some]:read_ref(borrow file);delta close(file) {closed=>closed,failed {auto error}=>failed}
    [entry]:exit done;
}
outcome result {@exit 0 done}
"#;
        let mut scenarios = vec![];
        for keep in [false, true] {
            let mut calls = vec![
                json!({"operation":"open","outcome":"opened","fields":[{"kind":"opaque","value":{"ty":"handle","token":"owner"}}]}),
            ];
            if keep {
                calls.push(json!({"operation":"read","outcome":"data","fields":[{"kind":"bytes","value":[65]}]}));
            }
            calls.push(json!({"operation":"close","outcome":"closed","fields":[]}));
            scenarios.push(
                json!({"entry":"main","arguments":[{"kind":"bool","value":keep}],"calls":calls}),
            );
        }
        carrier_matrix(source, &scenarios, &[None]);
        for bad in [
            source.replace(
                "read_ref(borrow file);delta close(file)",
                "read_ref(borrow file);drop(file);delta close(file)",
            ),
            source.replace(
                "read_ref(borrow file);delta close(file)",
                "auto duplicate=clone(borrow file);read_ref(borrow file);delta close(file)",
            ),
        ] {
            assert!(
                tellur::compile(
                    &bad,
                    Manifest::from_json(include_str!("../playground/manifest.json")).unwrap()
                )
                .is_err()
            );
        }
    }

    #[test]
    fn collection_primitive_payloads_keep_script_validation_and_selected_outcomes() {
        let f = Fixture::new(
            "control main() exits result {delta custom() {done=>done,bad {auto value}=>bad} [bad]:emit(u32_text(vector_length(borrow value))); [done|bad]:exit done;} outcome result {@exit 0 done}",
        );
        let mut manifest: Json =
            serde_json::from_str(&fs::read_to_string(&f.manifest).unwrap()).unwrap();
        manifest["primitives"].as_array_mut().unwrap().push(json!({"name":"custom","parameters":[],"outcomes":[{"name":"done","fields":[]},{"name":"bad","fields":[{"name":"values","type":"vector<U32>"}]}],"effects":[],"returns":true}));
        fs::write(&f.manifest, manifest.to_string()).unwrap();
        let checked = compile_file(
            &f.source,
            Manifest::from_json(&manifest.to_string()).unwrap(),
        )
        .unwrap();
        for expanded in [false, true] {
            for opt in [0, 2] {
                build_pair(&f, expanded, opt);
                for (index,call) in [
                json!({"operation":"custom","outcome":"done","fields":[]}),
                json!({"operation":"custom","outcome":"bad","fields":[{"kind":"construct","value":{"ty":"vector<U32>","name":"vector","fields":[{"kind":"u32","value":7}]}}]}),
                json!({"operation":"custom","outcome":"bad","fields":[{"kind":"construct","value":{"ty":"vector<U32>","name":"vector","fields":[{"kind":"bool","value":true}]}}]})
            ].into_iter().enumerate() {
                let scenario=json!({"entry":"main","arguments":[],"calls":[call]});
                let path=f.root.join("scenario.json");fs::write(&path,scenario.to_string()).unwrap();
                let a=execute(&f.root.join(format!("llvm-{opt}")),&path,None);
                let b=execute(&f.root.join(format!("rust-{opt}")),&path,None);
                assert_eq!(a.status.code(),b.status.code(),"{a:?}; {b:?}");
                assert_eq!(a.stdout,b.stdout);assert_eq!(a.stderr,b.stderr);
                if index<2 {assert!(a.status.success());assert_eq!(report(&a),serde_json::to_value(checked.run(&serde_json::from_value(scenario).unwrap()).unwrap()).unwrap());}
                else {assert!(!a.status.success());}
            }
            }
        }
    }

    #[test]
    fn collections_preserve_mutation_element_borrows_faults_and_fuel() {
        let scenarios=[0,1,99].into_iter().flat_map(|index| [false,true].map(move |fail|json!({"entry":"main","arguments":[{"kind":"u32","value":index},{"kind":"bool","value":fail}],"calls":[]}))).collect::<Vec<_>>();
        carrier_matrix(
            include_str!("../playground/llvm_collections.tellur"),
            &scenarios,
            &(0..=55).map(Some).chain([None]).collect::<Vec<_>>(),
        );
        carrier_matrix(r#"
control first(&vector<string> values) exits auto {
    [entry]: delta vector_get(values,0) {found {&string value}=>yes,out_of_bounds=>no}
    [yes]: exit found {value}; [no]: exit empty;
}
control main(u32 index) exits auto {
    [entry]: mutable vector<string> words=["one","two"];
        delta vector_push(&mutable words,"three") {pushed=>added}
    [added]: delta first(&words) {found {auto view}=>read,empty=>unexpected}
    [unexpected]: exit failed;
    [read]: string original=clone(view);
        delta vector_replace(&mutable words,index,"new") {replaced {string previous}=>changed,out_of_bounds {string rejected}=>outside}
    [changed]: emit(previous);
    [outside]: emit(rejected);
    [changed|outside]: delta vector_get_mut(&mutable words,0) {found {auto edit}=>editing,out_of_bounds=>bad}
    [bad]: exit failed;
    [editing]: *edit="edited";
        mutable map<string,u32> counts=map {};
        delta map_insert(&mutable counts,"b",2) {inserted=>b_added,replaced {u32 old_b}=>b_replaced}
    [b_added|b_replaced]: delta map_insert(&mutable counts,"a",1) {inserted=>a_added,replaced {u32 old_a}=>a_replaced}
    [a_added|a_replaced]: delta map_insert(&mutable counts,"b",20) {inserted=>new_b,replaced {u32 old}=>old_b}
    [new_b|old_b]: delta map_get_mut(&mutable counts,&"a") {found {auto number}=>number_found,missing=>number_missing}
    [number_missing]: exit failed;
    [number_found]: *number=10;
        array<u32,3> fixed=[4,5,6];
        u32 length=vector_length(&words);
        u32 map_size=map_length(&counts); exit done {original,words,counts,fixed,length,array_length(&fixed),map_size};
}
"#, &[0,1,99].map(|index|json!({"entry":"main","arguments":[{"kind":"u32","value":index}],"calls":[]})), &[None]);
        carrier_matrix(
            r#"control main() exits auto { [entry]:
        mutable array<u32,1> a=[7];
        delta array_get_mut(&mutable a,0) {found {auto p}=>yes,out_of_bounds=>no}
        [no]: exit failed;
        [yes]: *p=8; delta array_get(&a,9) {found {auto q}=>bad,out_of_bounds=>outside}
        [bad]: exit failed;
        [outside]: mutable vector<string> v=["a","b"];
        delta vector_remove(&mutable v,0) {item {string removed}=>removed,out_of_bounds=>bad_remove}
        [bad_remove]: exit failed;
        [removed]: delta vector_pop(&mutable v) {item {string popped}=>popped,empty=>bad_pop}
        [bad_pop]: exit failed;
        [popped]: delta vector_pop(&mutable v) {item {auto bad_item}=>bad_empty,empty=>empty}
        [bad_empty]: exit failed;
        [empty]: mutable map<u32,string> m=map {};
        delta map_insert(&mutable m,7,"seven") {inserted=>inserted,replaced {auto old}=>replaced}
        [inserted|replaced]: delta map_get(&m,&8) {found {auto unexpected}=>bad_get,missing=>missing}
        [bad_get]: exit failed;
        [missing]: delta map_remove(&mutable m,&7) {removed {string seven}=>seven,missing=>bad_missing}
        [bad_missing]: exit failed;
        [seven]: delta map_take(m) {item {auto k,auto val,auto rest}=>bad_take,empty=>end}
        [bad_take]: exit failed;
        [end]: exit done {a,removed,popped,seven};
    }"#,
            &[json!({"entry":"main","arguments":[],"calls":[]})],
            &[None],
        );
        carrier_matrix(
            r#"control main() exits auto { [entry]: array<string,2> a=["a","b"];
        delta array_into_vector(a) {converted {vector<string> v}=>converted}
        [converted]: mutable vector<string> work=v;
        delta vector_remove(&mutable work,5) {item {auto unexpected}=>bad,out_of_bounds=>outside}
        [bad]: exit failed;
        [outside]: delta vector_pop(&mutable work) {item {string b}=>popped,empty=>bad_empty}
        [bad_empty]: exit failed;
        [popped]: exit done {b,work}; }"#,
            &[json!({"entry":"main","arguments":[],"calls":[]})],
            &[None],
        );
        carrier_matrix(
            r#"control main() exits auto { [entry]: mutable vector<vector<string>> rows=[["a"]];
        delta vector_get_mut(&mutable rows,0) {found {auto row}=>row,out_of_bounds=>no}
        [no]: exit failed;
        [row]: delta vector_get_mut(row,0) {found {auto cell}=>cell,out_of_bounds=>empty}
        [empty]: exit failed;
        [cell]: *cell="changed"; exit done {rows};
    }"#,
            &[json!({"entry":"main","arguments":[],"calls":[]})],
            &[None],
        );
    }

    #[test]
    fn collections_of_shared_references_keep_external_origins_after_transfer() {
        carrier_matrix(
            include_str!("fixtures/borrowed_collections.tellur"),
            &[json!({"entry":"main","arguments":[],"calls":[]})],
            &[None],
        );
        carrier_matrix(
            r#"control main() exits done {string first="a";string second="b";
mutable array<ref string,2> values=[borrow first,borrow second];auto copied=values;
delta array_get_mut(borrow mutable values,1) {found {auto text}=>found,out_of_bounds=>missing}
[found]:emit(text);[entry]:delta array_replace(borrow mutable values,0,borrow second) {replaced {auto old}=>replaced,out_of_bounds {auto rejected}=>rejected}
[replaced]:emit(old);[rejected]:emit(rejected);[entry]:delta array_into_vector(copied) {converted {auto vector}=>converted}
[converted]:delta vector_take(vector) {item {auto head,auto tail}=>item,empty=>empty}
[item]:emit(head);drop(tail);[entry]:exit done;}"#,
            &[json!({"entry":"main","arguments":[],"calls":[]})],
            &[None],
        );
        carrier_matrix(
            r#"control main() exits done {string first="a";string second="b";mutable map<string,ref string> values=map {};
delta map_insert(borrow mutable values,"key",borrow first) {inserted=>inserted,replaced {auto old}=>replaced}
[replaced]:emit(old);[entry]:delta map_insert(borrow mutable values,"key",borrow second) {inserted=>inserted2,replaced {auto previous}=>replaced2}
[replaced2]:emit(previous);[entry]:delta map_get_mut(borrow mutable values,borrow "key") {found {auto text}=>found,missing=>missing}
[found]:emit(text);[entry]:delta map_remove(borrow mutable values,borrow "key") {removed {auto removed}=>removed,missing=>missing2}
[removed]:emit(removed);[entry]:drop(values);exit done;}"#,
            &[json!({"entry":"main","arguments":[],"calls":[]})],
            &[None],
        );
        carrier_matrix(
            r#"control first(ref string input) exits found {ref string value from input},missing {vector<ref string> values=[input];exit delta vector_get(borrow values,0) {found {auto text}=>found {text},out_of_bounds=>missing {}};}
control main() exits done {string text="ok";delta first(borrow text) {found {auto value}=>found,missing=>missing}[found]:emit(value);[entry]:exit done;}"#,
            &[json!({"entry":"main","arguments":[],"calls":[]})],
            &[None],
        );
        let bad = include_str!("fixtures/borrowed_collections.tellur")
            .replace("emit(word)", "first=\"early\";emit(word)");
        assert_eq!(
            tellur::compile(
                &bad,
                Manifest::from_json(include_str!("../playground/manifest.json")).unwrap()
            )
            .unwrap_err()
            .code,
            "E_LOAN"
        );
    }

    #[test]
    fn collection_literals_and_resource_transfers_preserve_nested_owners() {
        carrier_matrix(
            r#"control main(handle first,handle second) exits auto { [entry]:
        mutable map<string,handle> m=map {};
        delta map_insert(&mutable m,"key",first) {inserted=>inserted,replaced {handle old}=>replaced}
        [replaced]: exit unexpected {m,old,second};
        [inserted]: delta map_insert(&mutable m,"key",second) {inserted=>new,replaced {handle previous}=>old}
        [new]: exit unexpected_new {m};
        [old]: delta map_take(m) {item {string key,handle current,map<string,handle> rest}=>item,empty=>empty}
        [empty]: exit unexpected_empty {previous};
        [item]: delta map_take(rest) {item {auto key2,auto value2,auto rest2}=>extra,empty=>end}
        [extra]: exit extra {previous,current,value2,rest2};
        [end]: exit done {previous,current};
    }"#,
            &[
                json!({"entry":"main","arguments":[{"kind":"opaque","value":{"ty":"handle","token":"first"}},{"kind":"opaque","value":{"ty":"handle","token":"second"}}],"calls":[]}),
            ],
            &[None],
        );
        carrier_matrix(
            r#"struct packet {option<vector<array<u32,2>>> values;}
variant choice {one {vector<u32> values},two}
control main() exits auto {
    packet p=packet {some {[[1,2],[3,4]]}};
    choice c=choice.one {[5,6]};
    array<string,0> empty=[];
    delta array_into_vector(empty) {converted {auto v}=>ready}
    [ready]:exit done {p,c,v};
}"#,
            &[json!({"entry":"main","arguments":[],"calls":[]})],
            &[None],
        );
        carrier_matrix(
            r#"control main(handle input) exits auto {
    array<handle,1> owner=[input];
    delta array_into_vector(owner) {converted {auto vector}=>converted}
    [converted]:delta vector_take(vector) {item {auto value,auto rest}=>item,empty=>empty}
    [empty]:exit absent;
    [item]:delta vector_take(rest) {item {auto extra,auto tail}=>extra,empty=>done}
    [extra]:exit extra {value,extra,tail};
    [done]:exit done {value};
}"#,
            &[
                json!({"entry":"main","arguments":[{"kind":"opaque","value":{"ty":"handle","token":"one"}}],"calls":[]}),
            ],
            &[None],
        );
        for bad in [
            "control main(handle h) exits done {vector<handle> owners=[h];drop(owners);exit done;}",
            "control main() exits done {mutable string s=\"x\";vector<ref mutable string> refs=[borrow mutable s];exit done;}",
        ] {
            assert!(
                tellur::compile(
                    bad,
                    Manifest::from_json(include_str!("../playground/manifest.json")).unwrap()
                )
                .is_err()
            );
        }
    }

    #[test]
    fn loops_preserve_simultaneous_owned_parameters_outer_updates_faults_and_fuel() {
        let scenarios = [(0, false), (1, false), (4, false), (4, true)]
            .into_iter()
            .map(|(limit, fail)| json!({"entry":"main","arguments":[{"kind":"u32","value":limit},{"kind":"bool","value":fail}],"calls":[]}))
            .collect::<Vec<_>>();
        let fuels = std::iter::once(None)
            .chain((0..=100).map(Some))
            .collect::<Vec<_>>();
        carrier_matrix(
            include_str!("../playground/llvm_loops.tellur"),
            &scenarios,
            &fuels,
        );
    }

    #[test]
    fn loops_reset_inlined_facts_and_resume_nested_expansion_targets() {
        for helper in [
            "control identity(bool x) exits yes,no {delta x {true=>yes,false=>no}[yes]:exit yes;[no]:exit no;}",
            "control identity(bool x) exits yes,no {loop inner(){delta x {true=>yes,false=>no}[yes]:exit yes;[no]:exit no;}}",
        ] {
            let source = format!(
                r#"{helper}
control main() exits done {{
    loop outer(bool first=true) {{
        delta identity(first) {{yes=>first,no=>later}}
        [first]: emit("first"); again outer(false);
        [later]: emit("second"); exit done;
    }}
}}"#
            );
            let fuels = std::iter::once(None)
                .chain((0..=35).map(Some))
                .collect::<Vec<_>>();
            carrier_matrix(
                &source,
                &[json!({"entry":"main","arguments":[],"calls":[]})],
                &fuels,
            );
        }
    }

    #[test]
    fn loops_enter_only_selected_histories_and_reset_iteration_bindings() {
        carrier_matrix(
            r#"control main(bool pick) exits auto {
                delta pick {true=>selected,false=>skipped}
                [selected] over &: {
                    loop pass(bool first=true) {
                        delta first {true=>first,false=>later}
                        [first]: u32 value=1; emit(u32_text(value)); again pass(false);
                        [later]: u32 value=2; exit done {value};
                    }
                }
                [skipped]: exit done {0};
            }"#,
            &[
                json!({"entry":"main","arguments":[{"kind":"bool","value":false}],"calls":[]}),
                json!({"entry":"main","arguments":[{"kind":"bool","value":true}],"calls":[]}),
            ],
            &std::iter::once(None)
                .chain((0..=25).map(Some))
                .collect::<Vec<_>>(),
        );
    }

    #[test]
    fn loops_drain_resources_and_end_element_borrows_before_owner_transfer() {
        carrier_matrix(
            r#"control main(handle input) exits done {
                vector<handle> owners=[input];
                loop drain(vector<handle> pending=owners) {
                    delta vector_take(pending) {item {auto item,auto rest}=>next,empty=>end}
                    [next]:delta close(item) {closed=>closed,failed {auto error}=>failed}
                    [closed|failed]:again drain(rest);
                    [end]:exit done;
                }
            }"#,
            &[
                json!({"entry":"main","arguments":[{"kind":"opaque","value":{"ty":"handle","token":"one"}}],"calls":[{"operation":"close","outcome":"closed","fields":[]}]}),
                json!({"entry":"main","arguments":[{"kind":"opaque","value":{"ty":"handle","token":"one"}}],"calls":[{"operation":"close","outcome":"failed","fields":[{"kind":"opaque","value":{"ty":"close_error","token":"error"}}]}]}),
            ],
            &std::iter::once(None)
                .chain((0..=30).map(Some))
                .collect::<Vec<_>>(),
        );
        carrier_matrix(
            r#"control first(ref vector<string> words) exits auto {forward vector_get(words,0);}
            control main() exits done {
                loop drain(vector<string> words=["a","b"]) {
                    delta first(borrow words) {found {auto word}=>found,out_of_bounds=>empty}
                    [found]:emit(word);
                    [entry]:delta vector_take(words) {item {auto item,auto rest}=>next,empty=>end}
                    [next]:drop(item);again drain(rest);
                    [end]:exit done;
                }
            }"#,
            &[json!({"entry":"main","arguments":[],"calls":[]})],
            &std::iter::once(None)
                .chain((0..=35).map(Some))
                .collect::<Vec<_>>(),
        );
    }

    #[test]
    fn loops_keep_native_stack_bounded_through_many_backedges() {
        let f = Fixture::new(
            r#"control main() exits result {
            loop flip(bool state=true) {
                delta state {true=>yes,false=>no}
                [yes]:again flip(false);
                [no]:again flip(true);
            }
        } outcome result {@exit 0 done}"#,
        );
        let scenario = f.root.join("scenario.json");
        fs::write(
            &scenario,
            json!({"entry":"main","arguments":[],"calls":[]}).to_string(),
        )
        .unwrap();
        for opt in [0, 2] {
            let llvm = f.build("llvm", opt);
            let rust = f.build("rust", opt);
            let run = |binary: &Path| {
                Command::new("/bin/sh")
                    .args([
                        "-c",
                        "ulimit -s 512 || exit 99; exec \"$@\"",
                        "tellur-loop-stack",
                    ])
                    .arg(binary)
                    .arg("--scenario")
                    .arg(&scenario)
                    .args(["--fuel", "200000"])
                    .output()
                    .unwrap()
            };
            let a = run(&llvm);
            let b = run(&rust);
            assert_eq!(a.status.code(), Some(1), "{a:?}");
            assert_eq!(a.status.code(), b.status.code(), "{a:?}; {b:?}");
            assert_eq!(a.stdout, b.stdout);
            assert_eq!(a.stderr, b.stderr);
            assert!(String::from_utf8_lossy(&a.stderr).contains("E_FUEL"));
        }
    }

    #[test]
    fn constructors_deliver_directly_and_keep_faults_and_every_fuel_cut() {
        let source = include_str!("../playground/llvm_constructors.tellur");
        let checked = tellur::compile(
            source,
            Manifest::from_json(include_str!("../playground/manifest.json")).unwrap(),
        )
        .unwrap();
        let ir = checked.llvm_source("main").unwrap();
        assert!(ir.contains("call void @tellur_rt_v1_construct_call"));
        for (id, region) in checked
            .regions()
            .enumerate()
            .filter(|(_, r)| r.is_constructor())
        {
            let body = ir
                .split(&format!("define void @control_{id}("))
                .nth(1)
                .unwrap()
                .split("\n}")
                .next()
                .unwrap();
            assert!(
                body.contains("call void @tellur_rt_v1_constructed"),
                "{}",
                region.name
            );
            assert!(!body.contains("call void @tellur_rt_v1_complete("));
        }
        carrier_matrix(source, &[false,true].map(|fail|json!({"entry":"main","arguments":[{"kind":"bool","value":fail}],"calls":[]})), &(0..=45).map(Some).chain([None]).collect::<Vec<_>>());
    }
    #[test]
    fn constructors_preserve_written_order_short_circuiting_and_nested_owners() {
        carrier_matrix(
            r#"
struct message {
 string text;
 constructor(string text) { self.text = move text; }
}
struct pair {
 message left;
 message right;
 constructor(message a, message b) {self.right = move b; self.left = move a;}
}
control main() exits done {
 auto p = pair(message("left"), message("right"));
 emit(move p.left.text);
 emit(move p.right.text);
 exit done;
}
"#,
            &[json!({"entry":"main","arguments":[],"calls":[]})],
            &[None],
        );
        carrier_matrix(
            r#"
struct mark {string text; u32 number; bool yes;
 constructor(ref mutable u32 counter, string text) {
  emit(borrow text); deref counter=2;
  self.text=move text; self.number=2; self.yes=true;
 }
}
struct pair {u32 before; mark after;}
control take(u32 before, mark after) exits done {emit(u32_text(before));emit(move after.text);exit done;}
control main() exits done {
 mutable u32 n=1;
 take(n, mark(borrow mutable n, "call"));
 n=1;
 auto p=pair {n, mark(borrow mutable n,"record")};
 emit(u32_text(p.before));emit(move p.after.text);
 n=1;
 bool same=n==(mark(borrow mutable n,"binary")).number;
 delta same {true=>fault "left operand was evaluated too late",false=>compared}
 [compared]:
 bool skipped=false && (mark(borrow mutable n,"WRONG and")).yes;
 bool skipped_or=true || (mark(borrow mutable n,"WRONG or")).yes;
 bool evaluated=true && (mark(borrow mutable n,"and")).yes;
 bool evaluated_or=false || (mark(borrow mutable n,"or")).yes;
 delta skipped {true=>fault "bad and",false=>and_checked}
 [and_checked]: delta skipped_or {true=>or_checked,false=>fault "bad or"}
 [or_checked]:exit done;
}
"#,
            &[json!({"entry":"main","arguments":[],"calls":[]})],
            &[None],
        );
        carrier_matrix(
            r#"
struct mark {string text; constructor(string text) {emit(borrow text);self.text=move text;}}
control make() exits ready {mark first,mark second} {exit ready {mark("first"),mark("second")};}
control main() exits done {
 auto result=capture make();
 delta result {ready {auto first,auto second}=>ready}
 [ready]:emit(move first.text);emit(move second.text);
 vector<mark> values=[mark("list one"),mark("list two")];drop(values);
 delta false {true=>yes,false=>no}
 [yes]:auto unused=mark("WRONG inactive");drop(unused);exit done;
 [no]:exit done;
}
"#,
            &[json!({"entry":"main","arguments":[],"calls":[]})],
            &[None],
        );
        carrier_matrix(
            r#"struct message {string text;constructor(string text){self.text=move text;}}
 control show(ref message value) exits done {emit(borrow value.text);exit done;}
 control main() exits done {show(borrow message("temporary"));loop work(message value=message("loop")){emit(move value.text);exit done;}}"#,
            &[json!({"entry":"main","arguments":[],"calls":[]})],
            &[None],
        );
    }
    #[test]
    fn constructors_preserve_shared_exclusive_and_nested_carrier_origins() {
        carrier_matrix(
            r#"
struct text_pair['a, 'b] {
    ref string left <- 'a;
    ref string right <- 'b;
    constructor(ref string left, ref string right)[left, right] {
        self.left = left;
        self.right = right;
    }
}
control main() exits done {
    mutable string left = "left";
    mutable string right = "right";
    auto pair = text_pair(borrow left, borrow right);
    emit(pair.left);
    left = "released left";
    emit(pair.right);
    right = "released right";
    exit done;
}
"#,
            &[json!({"entry":"main","arguments":[],"calls":[]})],
            &[None],
        );
        carrier_matrix(
            r#"
struct text_pair['a, 'b] {
    ref mutable string left <- 'a;
    ref mutable string right <- 'b;
    constructor(ref mutable string left, ref mutable string right)[left, right] {
        self.left = left;
        self.right = right;
    }
}
control main() exits done {
    mutable string left = "left";
    mutable string right = "right";
    auto pair = text_pair(borrow mutable left, borrow mutable right);
    deref pair.left = "edited"; emit(pair.left);
    left = "released left";
    emit(pair.right);
    right = "released right";
    exit done;
}
"#,
            &[json!({"entry":"main","arguments":[],"calls":[]})],
            &[None],
        );
        carrier_matrix(
            r#"
struct view['a] {
    ref string text <- 'a;
    constructor(ref string input)[input] { self.text = input; }
}
struct wrapper['a] {
    view['a] inner;
    constructor(ref string input)[input] { self.inner = view(input); }
}
struct borrowed_view['storage, 'text] {
    ref view['text] value <- 'storage;
    constructor(ref view input, bool first)[input, input['a]] {
        delta first { true => first, false => second }
        [first]: self.value = input;
        [second]: self.value = input;
        [entry]: emit(self.value.text);
    }
}
control unwrap(view input) exits ready { view[input['a]] value } {
    exit ready { input };
}
control main() exits done {
    mutable string owner = "nested";
    auto wrapped = wrapper(borrow owner);
    auto a = borrowed_view(borrow wrapped.inner, true);
    auto b = borrowed_view(borrow wrapped.inner, false);
    emit(a.value.text);
    emit(b.value.text);
    drop(a);
    drop(b);
    drop(wrapped);
    delta unwrap(view(borrow owner)) { ready { auto value } => ready }
    [ready]: emit(value.text);
    owner = "released";
    exit done;
}
"#,
            &[json!({"entry":"main","arguments":[],"calls":[]})],
            &[None],
        );
        carrier_matrix(
            r#"
struct view['a] { ref string text <- 'a; }
struct optional_view['storage, 'text] {
    option<ref view>['storage, 'text] value;
    constructor(ref view input, bool present)[input, input['a]] {
        delta present { true => present, false => absent }
        [present]: self.value = some { input };
        [absent]: self.value = none {};
    }
}
control show(optional_view input) exits done {
    delta input.value { some { auto view } => present, none => absent }
    [present]: emit(view.text);
    [entry]: exit done;
}
control main() exits done {
    mutable string owner = "nested optional";
    view saved = view { borrow owner };
    show(optional_view(borrow saved, false));
    show(optional_view(borrow saved, true));
    drop(saved);
    owner = "released";
    exit done;
}
"#,
            &[json!({"entry":"main","arguments":[],"calls":[]})],
            &[None],
        );
    }

    #[test]
    fn imported_generic_constructors_preserve_names_and_partial_initialization_faults() {
        let f = Fixture::new(
            r#"
import "helpers.tellur" as helpers;
control main(bool fail) exits result faults {
    auto value = helpers.wrapper<helpers.message>(helpers.message(fail));
    emit(move value.value.text);
    exit done;
}
outcome result {@exit 0 done}
"#,
        );
        fs::write(
            f.root.join("helpers.tellur"),
            r#"
pub struct wrapper<t> {
    pub t value;
    pub constructor(t value) {self.value = move value;}
}
pub struct message {
    pub string text;
    pub constructor(bool fail) faults {
        self.text = "initialized";
        delta fail {true => failed, false => ready}
        [failed]: fault "partial initialization";
    }
}
"#,
        )
        .unwrap();
        for expanded in [false, true] {
            for opt in [0, 2] {
                build_pair(&f, expanded, opt);
                for fail in [false, true] {
                    let path = f.root.join("scenario.json");
                    fs::write(&path, json!({"entry":"main","arguments":[{"kind":"bool","value":fail}],"calls":[]}).to_string()).unwrap();
                    for fuel in (0..=40).map(Some).chain([None]) {
                        let a = execute(&f.root.join(format!("llvm-{opt}")), &path, fuel);
                        let b = execute(&f.root.join(format!("rust-{opt}")), &path, fuel);
                        assert_eq!(
                            a.status.code(),
                            b.status.code(),
                            "{expanded}/{opt}/{fail}/{fuel:?}"
                        );
                        assert_eq!(a.stdout, b.stdout, "{expanded}/{opt}/{fail}/{fuel:?}");
                        assert_eq!(a.stderr, b.stderr, "{expanded}/{opt}/{fail}/{fuel:?}");
                        if fuel.is_none() {
                            assert_eq!(a.status.success(), !fail);
                            if fail {
                                assert!(
                                    String::from_utf8_lossy(&a.stderr)
                                        .contains("message.constructor")
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn named_boundaries_preserve_settlement_retained_owners_faults_and_fuel() {
        carrier_matrix(include_str!("../playground/llvm_supervision.tellur"),
            &[false,true].map(|fail|json!({"entry":"main","arguments":[{"kind":"bool","value":fail}],"calls":[]})),
            &(0..=65).map(Some).chain([None]).collect::<Vec<_>>());
        carrier_matrix(
            r#"
struct text {string value; constructor(string value){self.value=move value;}}
boundary request(text value) retains value {emit(borrow value.value);fault "stopped";}
contingency(string reason){emit(move value.value);emit(reason);terminate;}
control main() exits done {
    loop work(u32 n=0) {
        supervise request(text("owned"));
        delta checked_add(n,1) {sum {u32 next}=>step,overflow=>bad}
        [bad]:fault "counter";
        [step]:delta next<3 {true=>more,false=>last}
        [more]:again work(next);
        [last]:exit done;
    }
}
"#,
            &[json!({"entry":"main","arguments":[],"calls":[]})],
            &[None, Some(0), Some(20), Some(75)],
        );
    }

    #[test]
    fn nested_named_boundaries_preserve_escalation_secondary_faults_and_outer_causes() {
        let source = r#"
boundary inner(u32 mode,string context) retains context,mode {
    delta mode==0 {true=>normal,false=>broken}
    [normal]:emit("inner normal");terminate;
    [broken]:fault "inner first";
}
contingency(string reason) {
    emit(context);emit(reason);
    delta mode==1 {true=>handled,false=>escaping}
    [handled]:terminate;
    [escaping]:delta mode==2 {true=>original,false=>secondary}
    [original]:escalate;
    [secondary]:fault "cleanup second";
}
boundary outer(u32 mode,string context) retains context {
    supervise inner(mode,clone(borrow context));
    emit("outer normal");terminate;
}
contingency(string reason){emit(context);emit(reason);terminate;}
control main(u32 mode) exits done {supervise outer(mode,"outer context");emit("parent");exit done;}
"#;
        carrier_matrix(source, &(0..4).map(|mode|json!({"entry":"main","arguments":[{"kind":"u32","value":mode}],"calls":[]})).collect::<Vec<_>>(), &(0..=80).map(Some).chain([None]).collect::<Vec<_>>());
        carrier_matrix(
            &source.replace(
                "supervise outer(mode,\"outer context\")",
                "supervise inner(mode,\"unhandled context\")",
            ),
            &[2, 3].map(
                |mode| json!({"entry":"main","arguments":[{"kind":"u32","value":mode}],"calls":[]}),
            ),
            &[None, Some(12), Some(25)],
        );
    }

    #[test]
    fn named_boundaries_keep_retained_resource_cleanup_and_host_errors_outside_source_faults() {
        let f = Fixture::new(
            r#"
boundary request(handle input,bool fail) retains input {
    delta read(borrow input) {data {bytes bytes}=>read,failed {read_error error}=>bad}
    [bad]:fault "read failed";
    [read]:delta fail {true=>broken,false=>normal}
    [broken]:fault "after read";
    [normal]:delta close(move input) {closed=>closed,failed {close_error close_error}=>close_failed}
    [closed|close_failed]:terminate;
}
contingency(string reason) {
    emit(reason);
    delta close(move input) {closed=>closed,failed {close_error error}=>close_failed}
    [closed|close_failed]:terminate;
}
control main(bool fail) exits result {
    delta open("file") {opened {handle input}=>opened,not_found=>absent,denied=>denied}
    [absent|denied]:exit done;
    [opened]:supervise request(move input,fail);exit done;
}
outcome result {@exit 0 done}
"#,
        );
        let mut manifest: Json =
            serde_json::from_str(&fs::read_to_string(&f.manifest).unwrap()).unwrap();
        manifest["primitives"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|p| p["name"] == "close")
            .unwrap()["contingent"] = json!(true);
        fs::write(&f.manifest, manifest.to_string()).unwrap();
        let checked = compile_file(
            &f.source,
            Manifest::from_json(&manifest.to_string()).unwrap(),
        )
        .unwrap();
        for expanded in [false, true] {
            for opt in [0, 2] {
                build_pair(&f, expanded, opt);
                for fail in [false, true] {
                    for malformed in [false, true] {
                        let calls = vec![
                            json!({"operation":"open","outcome":"opened","fields":[{"kind":"opaque","value":{"ty":"handle","token":"owned"}}]}),
                            json!({"operation":"read","outcome":"data","fields":[if malformed {json!({"kind":"bool","value":true})} else {json!({"kind":"bytes","value":[65]})}]}),
                            json!({"operation":"close","outcome":"closed","fields":[]}),
                        ];
                        let scenario = json!({"entry":"main","arguments":[{"kind":"bool","value":fail}],"calls":calls});
                        let path = f.root.join("scenario.json");
                        fs::write(&path, scenario.to_string()).unwrap();
                        for fuel in (0..=60).map(Some).chain([None]) {
                            let a = execute(&f.root.join(format!("llvm-{opt}")), &path, fuel);
                            let b = execute(&f.root.join(format!("rust-{opt}")), &path, fuel);
                            assert_eq!(
                                a.status.code(),
                                b.status.code(),
                                "{expanded}/{opt}/{fail}/{malformed}/{fuel:?}"
                            );
                            assert_eq!(
                                a.stdout, b.stdout,
                                "{expanded}/{opt}/{fail}/{malformed}/{fuel:?}"
                            );
                            assert_eq!(
                                a.stderr, b.stderr,
                                "{expanded}/{opt}/{fail}/{malformed}/{fuel:?}"
                            );
                            if fuel.is_none() && !malformed {
                                assert_eq!(
                                    report(&a),
                                    serde_json::to_value(
                                        checked
                                            .run(&serde_json::from_value(scenario.clone()).unwrap())
                                            .unwrap()
                                    )
                                    .unwrap()
                                );
                            }
                            if fuel.is_none() && malformed {
                                assert!(!a.status.success());
                                assert!(
                                    !String::from_utf8_lossy(&a.stdout)
                                        .contains("\"kind\":\"fault\"")
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn imported_named_boundaries_keep_private_helpers_and_source_names() {
        let f = Fixture::new(
            r#"import "helpers.tellur" as helpers; control main() exits result {supervise helpers.request("retained");exit done;} outcome result {@exit 0 done}"#,
        );
        fs::write(
            f.root.join("helpers.tellur"),
            r#"
control fail() faults {fault "imported fault";}
@contingent control report(string message) exits done {emit(message);exit done;}
pub boundary request(string context) retains context {fail();}
contingency(string reason){report(reason);emit(context);terminate;}
"#,
        )
        .unwrap();
        let checked = compile_file(
            &f.source,
            Manifest::from_json(&fs::read_to_string(&f.manifest).unwrap()).unwrap(),
        )
        .unwrap();
        let scenario = json!({"entry":"main","arguments":[],"calls":[]});
        let path = f.root.join("scenario.json");
        fs::write(&path, scenario.to_string()).unwrap();
        for expanded in [false, true] {
            for opt in [0, 2] {
                build_pair(&f, expanded, opt);
                for fuel in (0..=45).map(Some).chain([None]) {
                    let a = execute(&f.root.join(format!("llvm-{opt}")), &path, fuel);
                    let b = execute(&f.root.join(format!("rust-{opt}")), &path, fuel);
                    assert_eq!(a.status.code(), b.status.code());
                    assert_eq!(a.stdout, b.stdout);
                    assert_eq!(a.stderr, b.stderr);
                    if fuel.is_none() {
                        assert_eq!(
                            report(&a),
                            serde_json::to_value(
                                checked
                                    .run(&serde_json::from_value(scenario.clone()).unwrap())
                                    .unwrap()
                            )
                            .unwrap()
                        );
                    }
                }
            }
        }
    }

    fn lexical_matrix(f: &Fixture, scenarios: &[Json], fuels: &[Option<usize>]) {
        let mut manifest: Json =
            serde_json::from_str(&fs::read_to_string(&f.manifest).unwrap()).unwrap();
        manifest["launch"]["main"]["exit_codes"] = json!({"done":0});
        fs::write(&f.manifest, manifest.to_string()).unwrap();
        let checked = compile_file(
            &f.source,
            Manifest::from_json(&manifest.to_string()).unwrap(),
        )
        .unwrap();
        for expanded in [false, true] {
            let checked = if expanded {
                checked.expanded("main").unwrap()
            } else {
                checked.clone()
            };
            for opt in [0, 2] {
                build_pair(f, expanded, opt);
                for scenario in scenarios {
                    let path = f.root.join("scenario.json");
                    fs::write(&path, scenario.to_string()).unwrap();
                    for &fuel in fuels {
                        let a = execute(&f.root.join(format!("llvm-{opt}")), &path, fuel);
                        let b = execute(&f.root.join(format!("rust-{opt}")), &path, fuel);
                        assert_eq!(
                            a.status.code(),
                            b.status.code(),
                            "{expanded}/{opt}/{scenario}/{fuel:?}: {a:?}; {b:?}"
                        );
                        assert_eq!(a.stdout, b.stdout, "{expanded}/{opt}/{scenario}/{fuel:?}");
                        assert_eq!(a.stderr, b.stderr, "{expanded}/{opt}/{scenario}/{fuel:?}");
                        if fuel.is_none() {
                            if let Ok(expected) = checked.run_completion(
                                &serde_json::from_value(scenario.clone()).unwrap(),
                                10000,
                            ) {
                                assert_eq!(report(&a), serde_json::to_value(expected).unwrap());
                            }
                        }
                    }
                }
            }
        }
    }
    #[test]
    fn lexical_boundaries_recover_with_preserved_supervisor_state_and_structured_faults() {
        lexical_matrix(
            &Fixture::new(include_str!("../playground/recovery.tellur")),
            &[json!({"entry":"main","arguments":[],"calls":[]})],
            &(0..=90).map(Some).chain([None]).collect::<Vec<_>>(),
        );
        lexical_matrix(&Fixture::new(include_str!("../playground/llvm_recovery.tellur")),&[false,true].map(|fail|json!({"entry":"main","arguments":[{"kind":"bool","value":fail}],"calls":[]})),&(0..=95).map(Some).chain([None]).collect::<Vec<_>>());
    }
    #[test]
    fn lexical_boundaries_finalize_fallthrough_outward_again_and_expanded_completions() {
        for source in [
            r#"control main() exits done {string context="context";fault boundary {emit(borrow context);} contingency [context] {escalate;} emit(context);exit done;}"#,
            r#"control main() exits done {loop L(u32 n=0) {delta n<3 {true=>more,false=>last}[last]:exit done;[more]:delta checked_add(n,1) {sum {u32 next}=>next,overflow=>bad}[bad]:fault "counter";[next]:fault boundary {emit(u32_text(next));again L(next);} contingency [] {escalate;}}}"#,
            r#"control helper() exits done {string text} {fault boundary {loop L(){exit done {"out"};}} contingency [] {escalate;}} control main() exits done {delta helper(){done {string text}=>done}[done]:emit(text);exit done;}"#,
            r#"control helper() exits done {string text} {loop L(){fault boundary {exit done {"out"};} contingency [] {escalate;}}} control main() exits done {delta helper(){done {string text}=>done}[done]:emit(text);exit done;}"#,
        ] {
            lexical_matrix(
                &Fixture::new(source),
                &[json!({"entry":"main","arguments":[],"calls":[]})],
                &(0..=60).map(Some).chain([None]).collect::<Vec<_>>(),
            );
        }
    }
    #[test]
    fn lexical_boundaries_preserve_nested_secondary_causes_and_process_statuses() {
        let source = r#"control fail() faults {fault "stop";} boundary request() {fail();} contingency(string reason){emit("escalating");escalate;} control main() exits done {fault boundary {supervise request();emit("forbidden");exit done;} contingency [] {emit("stopping");terminate process 23;}}"#;
        for status in [0, 23, 255] {
            lexical_matrix(
                &Fixture::new(&source.replace("process 23", &format!("process {status}"))),
                &[json!({"entry":"main","arguments":[],"calls":[]})],
                &(0..=45).map(Some).chain([None]).collect::<Vec<_>>(),
            );
        }
        lexical_matrix(
            &Fixture::new(
                r#"control main() exits done {fault boundary {fault boundary {fault "first";} contingency [] {fault "second";}} contingency [] {emit(borrow @fault.message);terminate process 17;}}"#,
            ),
            &[json!({"entry":"main","arguments":[],"calls":[]})],
            &(0..=40).map(Some).chain([None]).collect::<Vec<_>>(),
        );
    }
    #[test]
    fn lexical_boundaries_release_resources_and_bypass_malformed_host_responses() {
        let f = Fixture::new(
            r#"
control main(bool broken) exits done {
 delta open("file") {opened {handle input}=>opened,not_found=>absent,denied=>denied}
 [absent|denied]:exit done;
 [opened]:fault boundary {
  delta read(borrow input) {data {bytes data}=>read,failed {read_error read_error}=>bad}
  [bad]:fault "read";
  [read]:delta broken {true=>broken,false=>healthy}
  [broken]:fault "after read";
  [healthy]:delta close(move input) {closed=>closed,failed {close_error close_error}=>close_failed}
  [closed|close_failed]:exit done;
 } contingency [input] {
  emit(borrow @fault.message);
  delta close(move input) {closed=>closed,failed {close_error close_error}=>close_failed}
  [closed|close_failed]:terminate process 7;
 }
}"#,
        );
        let mut manifest: Json =
            serde_json::from_str(&fs::read_to_string(&f.manifest).unwrap()).unwrap();
        manifest["primitives"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|p| p["name"] == "close")
            .unwrap()["contingent"] = json!(true);
        fs::write(&f.manifest, manifest.to_string()).unwrap();
        let scenarios=[false,true].into_iter().flat_map(|fail|[false,true].map(move|malformed|json!({"entry":"main","arguments":[{"kind":"bool","value":fail}],"calls":[{"operation":"open","outcome":"opened","fields":[{"kind":"opaque","value":{"ty":"handle","token":"owned"}}]},{"operation":"read","outcome":"data","fields":[if malformed {json!({"kind":"bool","value":true})} else {json!({"kind":"bytes","value":[65]})}]},{"operation":"close","outcome":"closed","fields":[]}]}))).collect::<Vec<_>>();
        lexical_matrix(
            &f,
            &scenarios,
            &(0..=65).map(Some).chain([None]).collect::<Vec<_>>(),
        );
    }
    #[test]
    fn lexical_boundaries_compose_with_imported_named_work_and_constructor_delivery() {
        let f = Fixture::new(
            r#"import "helper.tellur" as h;control main() exits done {fault boundary {supervise h.request();exit done;} contingency [] {emit(borrow @fault.message);terminate process 5;}}"#,
        );
        fs::write(f.root.join("helper.tellur"),r#"struct text {string value;constructor(string input){self.value=input;}} control make() exits ready {text value}{fault boundary {exit ready {text("owned")};} contingency [] {escalate;}} pub boundary request() faults {delta make(){ready {text value}=>ready}[ready]:emit(value.value);fault "request";} contingency(string reason){emit(reason);escalate;}"#).unwrap();
        lexical_matrix(
            &f,
            &[json!({"entry":"main","arguments":[],"calls":[]})],
            &(0..=75).map(Some).chain([None]).collect::<Vec<_>>(),
        );
    }

    #[test]
    fn every_control_keeps_scratch_allocations_in_its_entry_block() {
        for source in [
            include_str!("../playground/llvm_recovery.tellur"),
            include_str!("../playground/llvm_supervision.tellur"),
            include_str!("../playground/llvm_constructors.tellur"),
            include_str!("../playground/llvm_loops.tellur"),
            include_str!("../playground/llvm_collections.tellur"),
            include_str!("../playground/llvm_carriers.tellur"),
            "control main(bool a,bool b) exits result {bool value=a && !b || b;exit done {value};} outcome result {@exit 0 done {bool value}}",
        ] {
            let checked = tellur::compile(
                source,
                Manifest::from_json(include_str!("../playground/manifest.json")).unwrap(),
            )
            .unwrap();
            let ir = checked.llvm_source("main").unwrap();
            let mut controls = 0;
            let mut allocations = 0;
            for body in ir.split("define void @control_").skip(1) {
                controls += 1;
                let body = body.split("\n}").next().unwrap();
                let mut left_entry = false;
                for line in body.lines() {
                    if line.trim_start().starts_with("br ") {
                        left_entry = true;
                    }
                    if line.contains(" = alloca ") {
                        allocations += 1;
                        assert!(!left_entry, "scratch allocated after entry: {line}\n{body}");
                    }
                }
            }
            assert!(controls > 0 && allocations > 0);
        }
    }

    #[test]
    fn frontend_spawned_frames_preserve_fifo_join_delivery_and_nested_fault_fuel_reports() {
        let f = Fixture::new(
            r#"
async control leaf(string text) exits done {string value} {
    emit(borrow text);yield;exit done {text};
}
async control child(bool fail, string text) exits done {string value} faults {
    delta await leaf(text) {done {string value}=>ready}
    delta fail {true=>bad,false=>good}
    [bad]:fault "spawned failure 😀";
    [ready & good]:exit done {value};
}
async control startup(bool fail) exits done faults {
    auto first=spawn child(fail,"first");
    auto second=spawn child(false,"second");
    emit("queued");
    delta await second {done {string second_value}=>second_done}
    [second_done]:emit(second_value);
    delta await first {done {string first_value}=>first_done}
    [first_done]:emit(first_value);exit done;
}
control main(bool fail) exits done faults {forward run startup(fail);}
"#,
        );
        let scenarios = [false, true].map(
            |fail| json!({"entry":"main","arguments":[{"kind":"bool","value":fail}],"calls":[]}),
        );
        lexical_matrix(
            &f,
            &scenarios,
            &(0..=44).map(Some).chain([None]).collect::<Vec<_>>(),
        );
        let ir = fs::read_to_string(f.root.join("llvm-2.ll")).unwrap();
        assert!(
            ir.contains("call void @tellur_rt_v1_spawn")
                && ir.contains("call void @tellur_rt_v1_join")
        );
        assert!(!ir.contains("llvm.coro") && !ir.contains("malloc"));
    }

    #[test]
    fn frontend_cancellation_preserves_queued_running_and_completed_join_reports() {
        for before_cancel in ["", "yield;", "yield;yield;yield;yield;"] {
            let source = r#"
async control leaf(string text) exits done {string value} {
    emit(borrow text);yield;exit done {text};
}
async control child(bool fail, string text) exits done {string value} faults {
    auto nested=spawn leaf(text);
    delta await nested {done {string value}=>ready}
    delta fail {true=>bad,false=>good}
    [bad]:fault "cancelled child still faults";
    [ready & good]:exit done {value};
}
async control startup(bool fail) exits done faults {
    auto first=spawn child(fail,"first");
    auto second=spawn child(false,"second");
    emit("queued");
    BEFORE_CANCEL
    auto moved=move first;cancel moved;cancel moved;emit("requested");
    delta await second {done {string second_value}=>second_done}
    [second_done]:emit(second_value);
    delta await moved {done {string first_value}=>first_done}
    [first_done]:emit(first_value);exit done;
}
control main(bool fail) exits done faults {forward run startup(fail);}
"#
            .replace("BEFORE_CANCEL", before_cancel);
            let f = Fixture::new(&source);
            let scenarios = [false, true].map(|fail|
                json!({"entry":"main","arguments":[{"kind":"bool","value":fail}],"calls":[]}));
            lexical_matrix(
                &f,
                &scenarios,
                &(0..=52).map(Some).chain([None]).collect::<Vec<_>>(),
            );
            let ir = fs::read_to_string(f.root.join("llvm-2.ll")).unwrap();
            assert!(ir.contains("call void @tellur_rt_v1_cancel"));
        }
    }

    #[test]
    fn frontend_cancellation_keeps_exclusive_caller_loans_and_ordinary_completion() {
        let f = Fixture::new(include_str!("../playground/llvm_cancel.tellur"));
        let scenarios = [false, true].map(|fail|
            json!({"entry":"main","arguments":[{"kind":"bool","value":fail},{"kind":"string","value":"original owner"}],"calls":[]}));
        lexical_matrix(
            &f,
            &scenarios,
            &(0..=44).map(Some).chain([None]).collect::<Vec<_>>(),
        );
    }

    #[test]
    fn frontend_constructors_preserve_owned_and_borrowed_destinations_through_yielded_helpers() {
        let f = Fixture::new(include_str!("../playground/llvm_async_constructors.tellur"));
        let scenarios = [false, true].map(
            |fail| json!({"entry":"main","arguments":[{"kind":"bool","value":fail}],"calls":[]}),
        );
        lexical_matrix(
            &f,
            &scenarios,
            &(0..=45).map(Some).chain([None]).collect::<Vec<_>>(),
        );
        let ir = compile_file(
            &f.source,
            Manifest::from_json(&fs::read_to_string(&f.manifest).unwrap()).unwrap(),
        )
        .unwrap()
        .llvm_source("main")
        .unwrap();
        assert!(ir.contains("%compiled_control_v4") && ir.contains("@tellur_rt_v4_run_entry"));
        assert!(
            ir.contains("call void @tellur_rt_v1_construct_child")
                && ir.contains("call void @tellur_rt_v1_constructed")
        );
    }

    #[test]
    fn frontend_constructors_preserve_exclusive_results_nested_carriers_and_ancestor_owners() {
        for source in [
            r#"
async control pause() exits done {yield;exit done;}
struct text_pair['a,'b] {
 ref mutable string left <- 'a; ref mutable string right <- 'b;
 constructor(ref mutable string left, ref mutable string right)[left,right] {
  self.left=left;run pause();self.right=right;
 }
}
control main() exits done {
 mutable string left="left";mutable string right="right";
 auto pair=text_pair(borrow mutable left,borrow mutable right);
 deref pair.left="edited";emit(pair.left);left="released left";
 emit(pair.right);right="released right";exit done;
}
"#,
            r#"
async control pause() exits done {yield;exit done;}
struct view['a] {ref string text <- 'a;constructor(ref string input)[input] {run pause();self.text=input;}}
struct wrapper['a] {view['a] inner;constructor(ref string input)[input] {self.inner=view(input);run pause();}}
struct borrowed_view['storage,'text] {
 ref view['text] value <- 'storage;
 constructor(ref view input,bool first)[input,input['a]] {
  run pause();delta first {true=>first,false=>second}
  [first]:self.value=input;[second]:self.value=input;[entry]:emit(self.value.text);
 }
}
control unwrap(view input) exits ready {view[input['a]] value} {exit ready {input};}
control main() exits done {
 mutable string owner="nested";auto wrapped=wrapper(borrow owner);
 auto a=borrowed_view(borrow wrapped.inner,true);auto b=borrowed_view(borrow wrapped.inner,false);
 emit(a.value.text);emit(b.value.text);drop(a);drop(b);drop(wrapped);
 delta unwrap(view(borrow owner)) {ready {auto value}=>ready}
 [ready]:emit(value.text);owner="released";exit done;
}
"#,
        ] {
            let f = Fixture::new(source);
            lexical_matrix(
                &f,
                &[json!({"entry":"main","arguments":[],"calls":[]})],
                &(0..=80).map(Some).chain([None]).collect::<Vec<_>>(),
            );
        }
    }

    #[test]
    fn frontend_constructors_preserve_written_order_short_circuits_and_repeated_nested_calls() {
        let f = Fixture::new(
            r#"
async control pause() exits done {yield;exit done;}
struct mark {string text;u32 number;bool yes;
 constructor(ref mutable u32 counter,string text) {
  emit(borrow text);run pause();deref counter=2;
  self.text=move text;self.number=2;self.yes=true;
 }
}
struct pair {u32 before;mark after;}
control take(u32 before,mark after) exits done {emit(u32_text(before));emit(move after.text);exit done;}
control main() exits done {
 mutable u32 n=1;take(n,mark(borrow mutable n,"call"));n=1;
 auto p=pair {n,mark(borrow mutable n,"record")};emit(u32_text(p.before));emit(move p.after.text);n=1;
 bool same=n==(mark(borrow mutable n,"binary")).number;
 delta same {true=>fault "left operand too late",false=>compared}
 [compared]:bool skipped=false && (mark(borrow mutable n,"WRONG and")).yes;
 bool skipped_or=true || (mark(borrow mutable n,"WRONG or")).yes;
 bool evaluated=true && (mark(borrow mutable n,"and")).yes;
 bool evaluated_or=false || (mark(borrow mutable n,"or")).yes;
 delta skipped {true=>fault "bad and",false=>and_checked}
 [and_checked]:delta skipped_or {true=>or_checked,false=>fault "bad or"}
 [or_checked]:exit done;
}
"#,
        );
        lexical_matrix(
            &f,
            &[json!({"entry":"main","arguments":[],"calls":[]})],
            &(0..=110).map(Some).chain([None]).collect::<Vec<_>>(),
        );
    }

    #[test]
    fn frontend_constructors_preserve_imported_generic_names_and_partial_initialization_faults() {
        let f = Fixture::new(
            r#"
import "helpers.tellur" as helpers;
control main(bool fail) exits done faults {
 auto value=helpers.wrapper<helpers.message>(helpers.message(fail));emit(move value.value.text);exit done;
}
"#,
        );
        fs::write(
            f.root.join("helpers.tellur"),
            r#"
async control pause() exits done {yield;exit done;}
pub struct wrapper<t> {pub t value;pub constructor(t value) {run pause();self.value=move value;}}
pub struct message {pub string text;pub constructor(bool fail) faults {
 self.text="initialized";run pause();delta fail {true=>failed,false=>ready}
 [failed]:fault "partial initialization after yield";
}}
"#,
        )
        .unwrap();
        lexical_matrix(&f,&[false,true].map(|fail|json!({"entry":"main","arguments":[{"kind":"bool","value":fail}],"calls":[]})),&(0..=50).map(Some).chain([None]).collect::<Vec<_>>());
    }

    #[test]
    fn frontend_constructors_recover_after_partial_work_and_reuse_the_same_inline_frame() {
        let f = Fixture::new(
            r#"
async control helper(bool fail) exits done faults {
 yield;delta fail {true=>failed,false=>ready}
 [failed]:fault "constructor child failed";[ready]:exit done;
}
struct message {string text;constructor(bool fail) {self.text="complete";run helper(fail);}}
async control session(bool fail) exits done {
 supervised loop attempts(mutable bool first=true) {
  bool broken=fail && first;first=false;
  fault boundary {
   auto value=message(broken);emit(value.text);exit done;
  } contingency [] {emit(borrow @fault.message);recover again attempts;}
 }
}
control main(bool fail) exits done {forward run session(fail);}
"#,
        );
        lexical_matrix(&f,&[false,true].map(|fail|json!({"entry":"main","arguments":[{"kind":"bool","value":fail}],"calls":[]})),&(0..=65).map(Some).chain([None]).collect::<Vec<_>>());
    }

    #[test]
    fn frontend_constructors_compose_with_cold_spawned_borrowed_parents_and_task_sets() {
        let source = r#"
async control pause() exits done {yield;exit done;}
struct view['a] {ref mutable string text <- 'a;constructor(ref mutable string input)[input] {run pause();self.text=input;}}
async control child(ref mutable string input) exits done {view[input] value} {exit done {view(input)};}
async control startup() exits done {
 mutable string text="caller";auto pending=defer child(borrow mutable text);
 delta await pending {done {auto value}=>ready}
 [ready]:deref value.text="edited";emit(value.text);drop(value);text="released";emit(text);exit done;
}
control main() exits done {forward run startup();}
"#;
        for source in [
            source.to_owned(),
            source.replace("defer child", "spawn child"),
        ] {
            lexical_matrix(
                &Fixture::new(&source),
                &[json!({"entry":"main","arguments":[],"calls":[]})],
                &(0..=45).map(Some).chain([None]).collect::<Vec<_>>(),
            );
        }
        let f = Fixture::new(
            r#"
async control pause() exits done {yield;exit done;}
struct message {string text;constructor(string input) {run pause();self.text=input;}}
async control member(string input) exits done {message value} {exit done {message(input)};}
async control startup() exits done {
 tasks<member> active=tasks();spawn member("first") into active;spawn member("second") into active;
 loop drain(tasks<member> pending=move active) {
  delta await next pending {done {auto value}=>next,empty=>empty}
  [next]:emit(value.text);again drain(pending);[empty]:exit done;
 }
}
control main() exits done {forward run startup();}
"#,
        );
        lexical_matrix(
            &f,
            &[json!({"entry":"main","arguments":[],"calls":[]})],
            &(0..=70).map(Some).chain([None]).collect::<Vec<_>>(),
        );
    }

    #[test]
    fn frontend_constructors_preserve_pending_host_cancellation_driver_faults_and_spawned_delivery()
    {
        let f = host_fixture(
            r#"
struct message {string text;constructor(string input,bool fail) {
 delta run tick(input) {ticked {string reply}=>ready,cancelled=>stopped}
 [ready]:self.text=reply;[stopped]:self.text="cancelled";
 [entry]:delta fail {true=>bad,false=>good}
 [bad]:fault "constructor after host";
}}
async control member(bool fail,string input) exits done {message value} faults {yield;exit done {message(input,fail)};}
async control startup(bool fail,bool stop) exits done faults {
 auto first=spawn member(fail,"first");auto second=spawn member(false,"second");
 delta stop {true=>stop,false=>keep}
 [stop]:cancel first;
 [entry]:delta await second {done {auto second_value}=>second_done}
 [second_done]:emit(second_value.text);
 delta await first {done {auto first_value}=>first_done}
 [first_done]:emit(first_value.text);exit done;
}
control main(bool fail,bool stop) exits done faults {forward run startup(fail,stop);}
"#,
        );
        let scenarios=[false,true].into_iter().flat_map(|fail|[false,true].into_iter().flat_map(move |stop|[false,true].map(move |malformed|json!({"entry":"main","arguments":[{"kind":"bool","value":fail},{"kind":"bool","value":stop}],"calls":vec![json!({"operation":"tick","outcome":if malformed {"invalid"}else{"ticked"},"fields":[{"kind":"string","value":"reply"}],"waits":5});if stop {1}else{2}]})))).collect::<Vec<_>>();
        lexical_matrix(
            &f,
            &scenarios,
            &(0..=65).map(Some).chain([None]).collect::<Vec<_>>(),
        );
    }

    #[test]
    fn frontend_constructors_preserve_resource_fields_through_handoffs_and_retire_before_caller_faults()
     {
        let f = Fixture::new(
            r#"
async control helper() exits done {yield;exit done;}
struct file_owner {handle file;constructor(handle input) {self.file=move input;run helper();}}
control main(bool fail) exits done {
 fault boundary {
  delta open("file") {opened {handle input}=>opened,not_found=>missing,denied=>denied}
  [missing|denied]:exit done;
  [opened]:auto value=file_owner(input);
  delta close(move value.file) {closed=>closed,failed {auto problem}=>close_failed}
  [closed|close_failed]:delta fail {true=>bad,false=>done}
  [bad]:fault "after closing constructed owner";[done]:exit done;
 } contingency [] {emit(borrow @fault.message);terminate process 9;}
}
"#,
        );
        let scenarios=[false,true].map(|fail|json!({"entry":"main","arguments":[{"kind":"bool","value":fail}],"calls":[{"operation":"open","outcome":"opened","fields":[{"kind":"opaque","value":{"ty":"handle","token":"partial"}}]},{"operation":"close","outcome":"closed","fields":[]}]}));
        lexical_matrix(
            &f,
            &scenarios,
            &(0..=55).map(Some).chain([None]).collect::<Vec<_>>(),
        );
    }

    #[test]
    fn frontend_async_frames_preserve_nested_handoffs_guards_and_exact_fault_fuel_reports() {
        let f = Fixture::new(
            r#"
async control leaf(bool fail, ref string text) exits done faults {
    emit("leaf before");yield;
    delta fail {true=>bad,false=>good}
    [bad]: fault "nested failure 😀";
    [good]: emit(text);exit done;
}
async control child(bool fail, ref string text) exits done faults {
    yield;await leaf(fail,text);emit("child after");exit done;
}
async control startup(bool fail, bool pick, string text) exits done faults {
    emit("startup");
    delta pick {true=>selected,false=>skipped}
    [selected]:await child(fail,borrow text);
    [selected | skipped]:yield;emit(text);exit done;
}
control wrapper(bool fail, bool pick, string text) exits done faults {forward run startup(fail,pick,text);}
control main(bool fail, bool pick, string text) exits done faults {forward wrapper(fail,pick,text);}
"#,
        );
        let scenarios = [false,true].into_iter().flat_map(|fail| [false,true].map(move |pick|
            json!({"entry":"main","arguments":[{"kind":"bool","value":fail},{"kind":"bool","value":pick},{"kind":"string","value":"owned 😀 and \u{0}"}],"calls":[]}))).collect::<Vec<_>>();
        lexical_matrix(
            &f,
            &scenarios,
            &(0..=42).map(Some).chain([None]).collect::<Vec<_>>(),
        );
        let ir = fs::read_to_string(f.root.join("llvm-2.ll")).unwrap();
        assert!(ir.contains("define i32 @resume_") && ir.contains("switch i32"));
        assert!(
            ir.contains("@tellur_rt_v1_run_child") && ir.contains("call void @tellur_rt_v1_await")
        );
        assert!(
            ir.contains("%compiled_control") && ir.contains("@async_controls = private constant")
        );
        assert!(ir.contains("%frame_") && !ir.contains("llvm.coro") && !ir.contains("malloc"));
    }
    #[test]
    fn frontend_async_frames_preserve_exclusive_caller_origins_and_temporary_owned_arguments() {
        for source in [
            r#"
async control lend(ref mutable string input) exits lent {ref mutable string value from input} {
    yield;emit("child");exit lent {input};
}
async control startup(ref mutable string input) exits done {
    emit("before");delta await lend(input) {lent {auto value}=>lent}
    [lent]:yield;deref value="changed";emit(value);exit done;
}
control wrapper(ref mutable string input) exits done {forward run startup(input);}
control main() exits done {
    mutable string text="initial";wrapper(borrow mutable text);
    text="settled";emit(borrow text);exit done;
}
"#,
            r#"
struct message {string text;constructor(string text) {self.text=move text;}}
async control length(ref string text, message message) exits sized {u32 length} {
    yield;emit(text);emit(borrow message.text);exit sized {9};
}
async control outer() exits done {
    auto saved=capture (await length(borrow "temporary", message("owned")));
    delta saved {sized {u32 length}=>sized}
    [sized]:yield;emit(u32_text(length));exit done;
}
control main() exits done {forward run outer();}
"#,
        ] {
            lexical_matrix(
                &Fixture::new(source),
                &[json!({"entry":"main","arguments":[],"calls":[]})],
                &(0..=48).map(Some).chain([None]).collect::<Vec<_>>(),
            );
        }
    }
    #[test]
    fn frontend_async_frames_keep_unused_controls_and_multiple_runs_independent() {
        let f = Fixture::new(
            r#"
async control unused() exits done {yield;exit done;}
async control echo(string text) exits done {yield;emit(text);exit done;}
control main() exits done {run echo("first");run echo("second");exit done;}
control plain() exits done {emit("synchronous");exit done;}
"#,
        );
        let mut manifest: Json =
            serde_json::from_str(&fs::read_to_string(&f.manifest).unwrap()).unwrap();
        manifest["launch"] =
            json!({"main":{"exit_codes":{"done":0}},"plain":{"exit_codes":{"done":0}}});
        fs::write(&f.manifest, manifest.to_string()).unwrap();
        lexical_matrix(
            &f,
            &[json!({"entry":"main","arguments":[],"calls":[]})],
            &(0..=22).map(Some).chain([None]).collect::<Vec<_>>(),
        );
        // A separate synchronous-only root does not enter the retained launcher.
        for opt in [0, 2] {
            build_pair(&f, false, opt);
            let path = f.root.join("plain.json");
            fs::write(
                &path,
                json!({"entry":"plain","arguments":[],"calls":[]}).to_string(),
            )
            .unwrap();
            let a = execute(&f.root.join(format!("llvm-{opt}")), &path, None);
            let b = execute(&f.root.join(format!("rust-{opt}")), &path, None);
            assert_eq!(a.status.code(), b.status.code());
            assert_eq!(a.stdout, b.stdout);
            assert_eq!(a.stderr, b.stderr);
        }
    }

    #[test]
    fn frontend_sets_drain_in_completion_order_and_empty_next_does_not_handoff() {
        let scenario = json!({"entry":"main","arguments":[],"calls":[]});
        let f = Fixture::new(include_str!("../playground/task_sets.tellur"));
        let expected = successful_reference(&f, &scenario);
        let emitted = |report: &Json| {
            report["events"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|e| e["kind"] == "emit")
                .map(|e| e["message"].as_str().unwrap().to_owned())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            emitted(&expected),
            ["2", "slow", "fast", "joined fast", "joined slow"]
        );
        lexical_matrix(
            &f,
            &[scenario.clone()],
            &(0..=70).map(Some).chain([None]).collect::<Vec<_>>(),
        );
        let ir = fs::read_to_string(f.root.join("llvm-2.ll")).unwrap();
        for call in [
            "call i64 @tellur_rt_v1_task_set",
            "call void @tellur_rt_v1_spawn_into",
            "call i8 @tellur_rt_v1_next",
        ] {
            assert!(ir.contains(call), "{call}");
        }
        let f = Fixture::new(
            r#"
async control child() exits done {emit("child");yield;exit done;}
async control startup() exits done {
    tasks<child> active=tasks();cancel active;
    auto sibling=spawn child();
    delta await next active {done=>member,empty=>drained}
    [member]:fault "unexpected member";
    [drained]:emit("empty");await sibling;exit done;
}
control main() exits done {forward run startup();}
"#,
        );
        assert_eq!(
            emitted(&successful_reference(&f, &scenario)),
            ["empty", "child"]
        );
        lexical_matrix(
            &f,
            &[scenario],
            &(0..=32).map(Some).chain([None]).collect::<Vec<_>>(),
        );
    }

    #[test]
    fn frontend_sets_preserve_remainders_owned_payloads_and_member_or_parent_faults() {
        let f = Fixture::new(
            r#"
struct message {string text;}
async control member(string text, bool slow, bool fail) exits accepted {message value}, skipped faults {
    yield;
    delta slow {true=>slow,false=>fast}
    [slow]:yield;yield;
    [slow|fast]:delta fail {true=>bad,false=>good}
    [bad]:fault "member failed";
    [good & slow]:drop(text);exit skipped;
    [good & fast]:exit accepted {message {text}};
}
async control startup(bool parent_fail,bool member_fail) exits done faults {
    tasks<member> initial=tasks();
    spawn member("slow",true,false) into initial;
    auto moved=move initial;
    spawn member("fast",false,member_fail) into moved;
    delta parent_fail {true=>bad,false=>good}
    [bad]:fault "parent failed";
    loop drain(tasks<member> pending=move moved) {
        delta await next pending {accepted {message value}=>accepted,skipped=>skipped,empty=>drained}
        [accepted]:emit(value.text);
        [skipped]:emit("skipped");
        [accepted|skipped]:emit(u32_text(count(borrow pending)));again drain(pending);
        [drained]:exit done;
    }
}
control main(bool parent_fail,bool member_fail) exits done faults {forward run startup(parent_fail,member_fail);}
"#,
        );
        let scenarios=[false,true].into_iter().flat_map(|parent|[false,true].map(move |member| json!({"entry":"main","arguments":[{"kind":"bool","value":parent},{"kind":"bool","value":member}],"calls":[]}))).collect::<Vec<_>>();
        let report = successful_reference(&f, &scenarios[0]);
        assert_eq!(
            report["events"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|e| e["kind"] == "emit")
                .map(|e| e["message"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["fast", "1", "skipped", "0"]
        );
        lexical_matrix(
            &f,
            &scenarios,
            &(0..=65).map(Some).chain([None]).collect::<Vec<_>>(),
        );
    }

    #[test]
    fn frontend_sets_bound_feeding_and_reuse_loop_carried_remainders() {
        let f = Fixture::new(include_str!("../playground/llvm_task_sets.tellur"));
        let scenarios=[0,1,2,6].map(|limit|json!({"entry":"main","arguments":[{"kind":"u32","value":limit}],"calls":(1..=limit).map(|n|json!({"operation":"checked_add","outcome":"sum","fields":[{"kind":"u32","value":n}]})).collect::<Vec<_>>() }));
        for scenario in &scenarios {
            let report = successful_reference(&f, scenario);
            let mut active = 0;
            let mut spawned = 0;
            for event in report["events"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|e| e["kind"] == "emit")
            {
                if event["message"].as_str().unwrap().starts_with("spawn ") {
                    active += 1;
                    spawned += 1;
                    assert!(active <= 2);
                } else {
                    assert!(active > 0);
                    active -= 1;
                }
            }
            assert_eq!(active, 0);
            assert_eq!(spawned, scenario["arguments"][0]["value"].as_u64().unwrap());
        }
        lexical_matrix(
            &f,
            &scenarios,
            &(0..=135).map(Some).chain([None]).collect::<Vec<_>>(),
        );
    }

    #[test]
    fn frontend_sets_keep_resource_completion_and_fault_effects_in_member_order() {
        let f = Fixture::new(
            r#"
async control member(string path, bool fail) exits done faults {
    delta open(path) {opened {handle h}=>opened,not_found=>missing,denied=>denied}
    [missing|denied]:yield;exit done;
    [opened]:delta close(h) {closed=>closed,failed {close_error error}=>close_failed}
    [closed|close_failed]:delta fail {true=>bad,false=>good}
    [bad]:fault "after resource completion";
    [good]:yield;exit done;
}
async control startup(bool fail) exits done faults {
    tasks<member> active=tasks();spawn member("first",false) into active;spawn member("second",fail) into active;
    loop drain(tasks<member> pending=move active) {
        delta await next pending {done=>member,empty=>drained}
        [member]:emit("joined");again drain(pending);
        [drained]:exit done;
    }
}
control main(bool fail) exits done faults {forward run startup(fail);}
"#,
        );
        let scenarios=[false,true].map(|fail|json!({"entry":"main","arguments":[{"kind":"bool","value":fail}],"calls":[
            {"operation":"open","outcome":"opened","fields":[{"kind":"opaque","value":{"ty":"handle","token":"first"}}]},
            {"operation":"close","outcome":"closed","fields":[]},
            {"operation":"open","outcome":"opened","fields":[{"kind":"opaque","value":{"ty":"handle","token":"second"}}]},
            {"operation":"close","outcome":"closed","fields":[]}
        ]}));
        let report = successful_reference(&f, &scenarios[0]);
        assert_eq!(
            report["events"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|e| e["kind"] == "emit")
                .map(|e| e["message"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["joined", "joined"]
        );
        lexical_matrix(
            &f,
            &scenarios,
            &(0..=65).map(Some).chain([None]).collect::<Vec<_>>(),
        );
    }

    #[test]
    fn frontend_sets_keep_native_stack_bounded_during_repeated_spawn_and_reap() {
        let f = Fixture::new(
            r#"
async control member() exits done {yield;exit done;}
async control startup() exits done {
    tasks<member> active=tasks();spawn member() into active;
    loop repeat(tasks<member> pending=move active) {
        delta await next pending {done=>member,empty=>drained}
        [member]:spawn member() into pending;again repeat(pending);
        [drained]:exit done;
    }
}
control main() exits result {forward run startup();}
outcome result {@exit 0 done}
"#,
        );
        let scenario = f.root.join("scenario.json");
        fs::write(
            &scenario,
            json!({"entry":"main","arguments":[],"calls":[]}).to_string(),
        )
        .unwrap();
        for expanded in [false, true] {
            for opt in [0, 2] {
                build_pair(&f, expanded, opt);
                let run = |backend| {
                    Command::new("/bin/sh")
                        .args([
                            "-c",
                            "ulimit -s 512 || exit 99; exec \"$@\"",
                            "tellur-retained-set-stack",
                        ])
                        .arg(f.root.join(format!("{backend}-{opt}")))
                        .arg("--scenario")
                        .arg(&scenario)
                        .args(["--fuel", "100000"])
                        .output()
                        .unwrap()
                };
                let a = run("llvm");
                let b = run("rust");
                assert_eq!(a.status.code(), Some(1), "{a:?}");
                assert_eq!(a.status.code(), b.status.code());
                assert_eq!(a.stdout, b.stdout);
                assert_eq!(a.stderr, b.stderr);
                assert!(String::from_utf8_lossy(&a.stderr).contains("E_FUEL"));
            }
        }
    }

    #[test]
    fn frontend_sets_cancel_current_members_but_preserve_later_insertions() {
        for (yields, waits, stopped) in [(0, 20, true), (3, 20, true), (12, 0, false)] {
            let f = host_fixture(&format!(
                r#"
async control member(string label) exits ready {{string text}}, stopped {{
    delta await tick(label) {{ticked {{string text}}=>ready,cancelled=>stopped}}
    [ready]:exit ready {{text}};
    [stopped]:exit stopped;
}}
async control startup() exits done {{
    tasks<member> active=tasks();cancel active;
    spawn member("old") into active;{pause}cancel active;cancel active;
    spawn member("new") into active;
    loop drain(tasks<member> pending=move active) {{
        delta await next pending {{ready {{string text}}=>ready,stopped=>stopped,empty=>drained}}
        [ready]:emit(text);
        [stopped]:emit("cancelled");
        [ready|stopped]:again drain(pending);
        [drained]:exit done;
    }}
}}
control main() exits done {{forward run startup();}}
"#,
                pause = "yield;".repeat(yields)
            ));
            let mut calls = vec![];
            if yields > 0 {
                calls.push(host_call("old", waits));
            }
            calls.push(host_call("new", 0));
            let scenario = json!({"entry":"main","arguments":[],"calls":calls});
            let report = successful_reference(&f, &scenario);
            let emitted = report["events"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|e| e["kind"] == "emit")
                .map(|e| e["message"].as_str().unwrap())
                .collect::<Vec<_>>();
            assert!(
                emitted.contains(&"new")
                    && emitted.contains(&if stopped { "cancelled" } else { "old" }),
                "{yields}/{report}"
            );
            assert_eq!(emitted.len(), 2);
            lexical_matrix(
                &f,
                &[scenario],
                &(0..=75).map(Some).chain([None]).collect::<Vec<_>>(),
            );
            let ir = fs::read_to_string(f.root.join("llvm-2.ll")).unwrap();
            assert!(ir.contains("call void @tellur_rt_v1_cancel_set"));
        }
    }

    #[test]
    fn frontend_sets_of_host_operations_keep_exact_producers_and_readiness_order() {
        let f = host_fixture(
            r#"
async control startup() exits done {
    tasks<tick> active=tasks();spawn tick("first") into active;spawn tick("second") into active;
    loop drain(tasks<tick> pending=move active) {
        delta await next pending {ticked {string text}=>ready,empty=>drained}
        [ready]:emit(text);again drain(pending);
        [drained]:exit done;
    }
}
control main() exits done {forward run startup();}
"#,
        );
        let scenario = json!({"entry":"main","arguments":[],"calls":[host_call("first",7),host_call("second",0)]});
        let report = successful_reference(&f, &scenario);
        assert_eq!(
            report["events"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|e| e["kind"] == "emit")
                .map(|e| e["message"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["second", "first"]
        );
        lexical_matrix(
            &f,
            &[scenario],
            &(0..=40).map(Some).chain([None]).collect::<Vec<_>>(),
        );
    }

    #[test]
    fn frontend_loops_preserve_owned_swaps_resets_nested_awaits_and_expanded_completion() {
        let f = Fixture::new(include_str!("../playground/llvm_async_loops.tellur"));
        let scenarios = [0, 1, 3].into_iter().flat_map(|limit| [false,true].map(move |fail|
            json!({"entry":"main","arguments":[{"kind":"u32","value":limit},{"kind":"bool","value":fail}],"calls":(1..=limit).map(|n|json!({"operation":"checked_add","outcome":"sum","fields":[{"kind":"u32","value":n}]})).collect::<Vec<_>>() }))).collect::<Vec<_>>();
        for scenario in &scenarios {
            if scenario["arguments"][1]["value"] == false
                || scenario["arguments"][0]["value"].as_u64().unwrap() < 3
            {
                successful_reference(&f, scenario);
            }
        }
        lexical_matrix(
            &f,
            &scenarios,
            &(0..=76).map(Some).chain([None]).collect::<Vec<_>>(),
        );
    }
    #[test]
    fn frontend_loops_reuse_cold_spawned_and_inline_frames_with_exclusive_caller_loans() {
        let f = Fixture::new(
            r#"
async control append(ref mutable string text) exits done {
    yield;deref text=concat(clone(text),"!");exit done;
}
control inline_append(ref mutable string text) exits done {forward run append(text);}
async control repeat(ref mutable string text) exits done {
    loop turn(u32 count=0) {
        delta count<2 {true=>more,false=>finished}
        [finished]:exit done;
        [more]:
            auto cold=defer append(text);await cold;
            auto child=spawn append(text);cancel child;await child;
            inline_append(text);
            delta checked_add(count,1) {sum {u32 next}=>incremented,overflow=>bad}
        [bad]:fault "counter";
        [incremented]:again turn(next);
    }
}
control main() exits done {
    mutable string text="owner";run repeat(borrow mutable text);
    text=concat(move text," joined");emit(text);exit done;
}
"#,
        );
        let scenario = json!({"entry":"main","arguments":[],"calls":(1..=2).map(|n|json!({"operation":"checked_add","outcome":"sum","fields":[{"kind":"u32","value":n}]})).collect::<Vec<_>>()});
        let expected = successful_reference(&f, &scenario);
        assert_eq!(
            expected["events"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|e| e["kind"] == "emit")
                .map(|e| e["message"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["owner!!!!!! joined"]
        );
        lexical_matrix(
            &f,
            &[scenario],
            &(0..=85).map(Some).chain([None]).collect::<Vec<_>>(),
        );
    }
    #[test]
    fn frontend_loops_repeat_scripted_host_readiness_without_stale_child_state() {
        let f = host_fixture(
            r#"
async control startup() exits done {
    loop turn(u32 count=0) {
        delta count<3 {true=>more,false=>finished}
        [finished]:exit done;
        [more]:delta await tick("loop") {ticked {string text}=>ticked}
        [ticked]:emit(text);delta checked_add(count,1) {sum {u32 next}=>incremented,overflow=>bad}
        [bad]:fault "counter";
        [incremented]:again turn(next);
    }
}
control main() exits done {forward run startup();}
"#,
        );
        let scenarios=[0,1,7].map(|waits|json!({"entry":"main","arguments":[],"calls":(["first","second","third"].into_iter().enumerate().flat_map(|(n,label)|[host_call(label,waits),json!({"operation":"checked_add","outcome":"sum","fields":[{"kind":"u32","value":n+1}]})]).collect::<Vec<_>>()) }));
        for scenario in &scenarios {
            successful_reference(&f, scenario);
        }
        lexical_matrix(
            &f,
            &scenarios,
            &(0..=65).map(Some).chain([None]).collect::<Vec<_>>(),
        );
    }
    #[test]
    fn frontend_loops_resume_nested_expanded_helpers_at_their_outer_continuation() {
        let f = Fixture::new(
            r#"
async control leaf() exits done {yield;exit done;}
control handoff() exits done {forward run leaf();}
control identity(bool value) exits yes,no {
    loop inner() {
        delta value {true=>yes,false=>no}
        [yes]:exit yes;
        [no]:exit no;
    }
}
control main() exits done {
    loop outer(bool first=true) {
        delta identity(first) {yes=>first,no=>later}
        [first]:handoff();emit("first");again outer(false);
        [later]:handoff();emit("second");exit done;
    }
}
"#,
        );
        let checked = compile_file(
            &f.source,
            Manifest::from_json(&fs::read_to_string(&f.manifest).unwrap()).unwrap(),
        )
        .unwrap();
        let expanded = checked.expanded("main").unwrap();
        fn has_unwind(code: &[tellur::ir::Instruction]) -> bool {
            code.iter().any(|i| match &i.action {
                tellur::ir::Action::Complete {
                    unwind: Some(_), ..
                } => true,
                tellur::ir::Action::Loop { body, .. } => has_unwind(body),
                _ => false,
            })
        }
        assert!(has_unwind(&expanded.region("main").unwrap().code));
        let scenario = json!({"entry":"main","arguments":[],"calls":[]});
        successful_reference(&f, &scenario);
        lexical_matrix(
            &f,
            &[scenario],
            &(0..=45).map(Some).chain([None]).collect::<Vec<_>>(),
        );
    }

    #[test]
    fn frontend_loops_keep_native_stack_bounded_through_repeated_nested_calls() {
        let f = Fixture::new(
            r#"
async control child() exits done {yield;exit done;}
async control spin() exits done {
    loop turn(bool state=true) {
        await child();
        delta state {true=>yes,false=>no}
        [yes]:again turn(false);
        [no]:again turn(true);
    }
}
control main() exits result {forward run spin();}
outcome result {@exit 0 done}
"#,
        );
        let scenario = f.root.join("scenario.json");
        fs::write(
            &scenario,
            json!({"entry":"main","arguments":[],"calls":[]}).to_string(),
        )
        .unwrap();
        for expanded in [false, true] {
            for opt in [0, 2] {
                build_pair(&f, expanded, opt);
                let run = |backend| {
                    Command::new("/bin/sh")
                        .args([
                            "-c",
                            "ulimit -s 512 || exit 99; exec \"$@\"",
                            "tellur-retained-loop-stack",
                        ])
                        .arg(f.root.join(format!("{backend}-{opt}")))
                        .arg("--scenario")
                        .arg(&scenario)
                        .args(["--fuel", "100000"])
                        .output()
                        .unwrap()
                };
                let a = run("llvm");
                let b = run("rust");
                assert_eq!(a.status.code(), Some(1), "{a:?}");
                assert_eq!(a.status.code(), b.status.code());
                assert_eq!(a.stdout, b.stdout);
                assert_eq!(a.stderr, b.stderr);
                assert!(String::from_utf8_lossy(&a.stderr).contains("E_FUEL"));
            }
        }
    }

    #[test]
    fn frontend_cold_tasks_preserve_capture_order_nested_owners_and_guarded_faults() {
        let f = Fixture::new(
            r#"
struct packet {string text; bytes data;}
async control leaf(bool fail, packet input) exits ready {packet result} faults {
    emit("leaf entered");yield;
    delta fail {true=>bad,false=>good}
    [bad]:fault "cold child failure 😀";
    [good]:emit(borrow input.text);exit ready {input};
}
async control middle(bool fail, packet input) exits ready {packet result} faults {
    auto pending=defer leaf(fail,input);emit("middle captured");yield;
    auto moved=move pending;forward await moved;
}
async control startup(bool pick, bool fail, string text, bytes data) exits done {bytes result} faults {
    delta pick {true=>selected,false=>skipped}
    [selected]:
        auto pending=defer middle(fail,packet {text,data});
        emit("root captured");auto moved=move pending;
        delta await moved {ready {packet result}=>ready}
        [ready]:emit(borrow result.text);exit done {result.data};
    [skipped]:emit(borrow text);exit done {data};
}
control main(bool pick,bool fail,string text,bytes data) exits done {bytes result} faults {forward run startup(pick,fail,text,data);}
"#,
        );
        let scenarios=[false,true].into_iter().flat_map(|pick| [false,true].map(move |fail|
            json!({"entry":"main","arguments":[{"kind":"bool","value":pick},{"kind":"bool","value":fail},{"kind":"string","value":"cold 😀\u{0} owner"},{"kind":"bytes","value":[0,65,255,42]}],"calls":[]}))).collect::<Vec<_>>();
        lexical_matrix(
            &f,
            &scenarios,
            &(0..=65).map(Some).chain([None]).collect::<Vec<_>>(),
        );
        let ir = fs::read_to_string(f.root.join("llvm-2.ll")).unwrap();
        assert!(
            ir.contains("call i64 @tellur_rt_v1_defer")
                && ir.contains("call void @tellur_rt_v1_await_task")
        );
        assert!(!ir.contains("malloc") && !ir.contains("llvm.coro"));
    }
    #[test]
    fn frontend_cold_tasks_preserve_exclusive_and_contained_caller_origins() {
        for source in [
            r#"
async control lend(ref mutable string input) exits lent {ref mutable string value from input} {
    emit("body");yield;exit lent {input};
}
async control startup(ref mutable string input) exits done {
    auto pending=defer lend(input);emit("captured");yield;
    auto moved=move pending;delta await moved {lent {auto value}=>lent}
    [lent]:deref value="changed";yield;emit(value);exit done;
}
control main() exits done {
    mutable string text="initial";run startup(borrow mutable text);
    text="released";emit(borrow text);exit done;
}
"#,
            r#"
struct view['a] {ref string text <- 'a;}
async control lend(view input) exits lent {view[input['a]] value} {yield;exit lent {input};}
async control startup() exits done {
    mutable string owner="contained owner";view saved=view {borrow owner};
    auto pending=defer lend(saved);drop(saved);emit("captured view");
    delta await pending {lent {auto value}=>lent}
    [lent]:yield;emit(value.text);owner="released view";emit(borrow owner);exit done;
}
control main() exits done {forward run startup();}
"#,
        ] {
            lexical_matrix(
                &Fixture::new(source),
                &[json!({"entry":"main","arguments":[],"calls":[]})],
                &(0..=48).map(Some).chain([None]).collect::<Vec<_>>(),
            );
        }
    }

    #[test]
    fn frontend_cold_tasks_preserve_local_wrappers_and_live_resource_capture() {
        lexical_matrix(
            &Fixture::new(
                r#"
async control work(string input) exits ready {string value} {emit("body");yield;exit ready {input};}
async control startup() exits done {
    auto pending=defer work("wrapped");auto wrapped=some {some {pending}};emit("captured");
    delta wrapped {some {auto inner}=>inner,none=>empty}
    [inner]:delta inner {some {auto task}=>task,none=>empty_inner}
    [task]:delta await task {ready {string text}=>ready}
    [ready]:emit(borrow text);
    [ready|empty|empty_inner]:exit done;
}
control main() exits done {forward run startup();}
"#,
            ),
            &[json!({"entry":"main","arguments":[],"calls":[]})],
            &(0..=32).map(Some).chain([None]).collect::<Vec<_>>(),
        );
        let source=include_str!("../playground/llvm_resources.tellur")
            .replace("control read_owned", "async control read_owned")
            .replace("delta close(input.file)", "yield;delta close(input.file)")
            .replace("control main(string filename, bool fail)", "async control startup(string filename, bool fail)")
            .replace("[opened]: delta read_owned(file_owner { file })", "[opened]: auto pending=defer read_owned(file_owner {file});emit(\"captured file\");delta await pending")
            + "\ncontrol main(string filename,bool fail) exits process_result {forward run startup(filename,fail);}";
        let f = Fixture::new(&source);
        let file = f.root.join("input.dat");
        let bytes = vec![0, 65, 255, 10];
        fs::write(&file, &bytes).unwrap();
        let scenario = json!({"entry":"main","arguments":[{"kind":"string","value":"scripted"},{"kind":"bool","value":false}],"calls":[
            {"operation":"open","outcome":"opened","fields":[{"kind":"opaque","value":{"ty":"handle","token":"file-0"}}]},
            {"operation":"read","outcome":"data","fields":[{"kind":"bytes","value":bytes}]},
            {"operation":"close","outcome":"closed","fields":[]}
        ]});
        let mut failed = scenario.clone();
        failed["arguments"][1]["value"] = json!(true);
        lexical_matrix(
            &f,
            &[scenario, failed],
            &(0..=45).map(Some).chain([None]).collect::<Vec<_>>(),
        );
        for expanded in [false, true] {
            for opt in [0, 2] {
                build_pair(&f, expanded, opt);
                for fail in [false, true] {
                    let args = [file.to_str().unwrap(), if fail { "true" } else { "false" }];
                    let a = Command::new(f.root.join(format!("llvm-{opt}")))
                        .arg("--json")
                        .args(args)
                        .output()
                        .unwrap();
                    let b = Command::new(f.root.join(format!("rust-{opt}")))
                        .arg("--json")
                        .args(args)
                        .output()
                        .unwrap();
                    assert_eq!(a.status.code(), Some(if fail { 1 } else { 0 }), "{a:?}");
                    assert_eq!(a.status.code(), b.status.code());
                    assert_eq!(a.stdout, b.stdout);
                    assert_eq!(a.stderr, b.stderr);
                    if !fail {
                        assert_eq!(
                            report(&a)["fields"],
                            json!([{"kind":"bytes","value":bytes}])
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn frontend_spawned_tasks_preserve_capture_order_nested_owners_and_guarded_faults() {
        let f = Fixture::new(
            r#"
struct packet {string text; bytes data;}
async control leaf(bool fail, packet input) exits ready {packet result} faults {
    emit("leaf entered");yield;
    delta fail {true=>bad,false=>good}
    [bad]:fault "cold child failure 😀";
    [good]:emit(borrow input.text);exit ready {input};
}
async control middle(bool fail, packet input) exits ready {packet result} faults {
    auto pending=spawn leaf(fail,input);emit("middle captured");yield;
    auto moved=move pending;forward await moved;
}
async control startup(bool pick, bool fail, string text, bytes data) exits done {bytes result} faults {
    delta pick {true=>selected,false=>skipped}
    [selected]:
        auto pending=spawn middle(fail,packet {text,data});
        emit("root captured");auto moved=move pending;
        delta await moved {ready {packet result}=>ready}
        [ready]:emit(borrow result.text);exit done {result.data};
    [skipped]:emit(borrow text);exit done {data};
}
control main(bool pick,bool fail,string text,bytes data) exits done {bytes result} faults {forward run startup(pick,fail,text,data);}
"#,
        );
        let scenarios=[false,true].into_iter().flat_map(|pick| [false,true].map(move |fail|
            json!({"entry":"main","arguments":[{"kind":"bool","value":pick},{"kind":"bool","value":fail},{"kind":"string","value":"cold 😀\u{0} owner"},{"kind":"bytes","value":[0,65,255,42]}],"calls":[]}))).collect::<Vec<_>>();
        lexical_matrix(
            &f,
            &scenarios,
            &(0..=65).map(Some).chain([None]).collect::<Vec<_>>(),
        );
        let ir = fs::read_to_string(f.root.join("llvm-2.ll")).unwrap();
        assert!(
            ir.contains("call void @tellur_rt_v1_spawn")
                && ir.contains("call void @tellur_rt_v1_join")
        );
        assert!(!ir.contains("malloc") && !ir.contains("llvm.coro"));
    }
    #[test]
    fn frontend_spawned_tasks_preserve_exclusive_and_contained_caller_origins() {
        for source in [
            r#"
async control lend(ref mutable string input) exits lent {ref mutable string value from input} {
    emit("body");yield;exit lent {input};
}
async control startup(ref mutable string input) exits done {
    auto pending=spawn lend(input);emit("captured");yield;
    auto moved=move pending;delta await moved {lent {auto value}=>lent}
    [lent]:deref value="changed";yield;emit(value);exit done;
}
control main() exits done {
    mutable string text="initial";run startup(borrow mutable text);
    text="released";emit(borrow text);exit done;
}
"#,
            r#"
struct view['a] {ref string text <- 'a;}
async control lend(view input) exits lent {view[input['a]] value} {yield;exit lent {input};}
async control startup() exits done {
    mutable string owner="contained owner";view saved=view {borrow owner};
    auto pending=spawn lend(saved);drop(saved);emit("captured view");
    delta await pending {lent {auto value}=>lent}
    [lent]:yield;emit(value.text);owner="released view";emit(borrow owner);exit done;
}
control main() exits done {forward run startup();}
"#,
        ] {
            lexical_matrix(
                &Fixture::new(source),
                &[json!({"entry":"main","arguments":[],"calls":[]})],
                &(0..=48).map(Some).chain([None]).collect::<Vec<_>>(),
            );
        }
    }

    #[test]
    fn frontend_spawned_tasks_preserve_local_wrappers_and_live_resource_capture() {
        lexical_matrix(
            &Fixture::new(
                r#"
async control work(string input) exits ready {string value} {emit("body");yield;exit ready {input};}
async control startup() exits done {
    auto pending=spawn work("wrapped");auto wrapped=some {some {pending}};emit("captured");
    delta wrapped {some {auto inner}=>inner,none=>empty}
    [inner]:delta inner {some {auto task}=>task,none=>empty_inner}
    [task]:delta await task {ready {string text}=>ready}
    [ready]:emit(borrow text);
    [ready|empty|empty_inner]:exit done;
}
control main() exits done {forward run startup();}
"#,
            ),
            &[json!({"entry":"main","arguments":[],"calls":[]})],
            &(0..=32).map(Some).chain([None]).collect::<Vec<_>>(),
        );
        let source=include_str!("../playground/llvm_resources.tellur")
            .replace("control read_owned", "async control read_owned")
            .replace("[read & closed]: exit", "[read & closed]: yield;exit")
            .replace("control main(string filename, bool fail)", "async control startup(string filename, bool fail)")
            .replace("[opened]: delta read_owned(file_owner { file })", "[opened]: auto pending=spawn read_owned(file_owner {file});emit(\"captured file\");delta await pending")
            + "\ncontrol main(string filename,bool fail) exits process_result {forward run startup(filename,fail);}";
        let f = Fixture::new(&source);
        let file = f.root.join("input.dat");
        let bytes = vec![0, 65, 255, 10];
        fs::write(&file, &bytes).unwrap();
        let scenario = json!({"entry":"main","arguments":[{"kind":"string","value":"scripted"},{"kind":"bool","value":false}],"calls":[
            {"operation":"open","outcome":"opened","fields":[{"kind":"opaque","value":{"ty":"handle","token":"file-0"}}]},
            {"operation":"read","outcome":"data","fields":[{"kind":"bytes","value":bytes}]},
            {"operation":"close","outcome":"closed","fields":[]}
        ]});
        let mut failed = scenario.clone();
        failed["arguments"][1]["value"] = json!(true);
        lexical_matrix(
            &f,
            &[scenario, failed],
            &(0..=45).map(Some).chain([None]).collect::<Vec<_>>(),
        );
        for expanded in [false, true] {
            for opt in [0, 2] {
                build_pair(&f, expanded, opt);
                for fail in [false, true] {
                    let args = [file.to_str().unwrap(), if fail { "true" } else { "false" }];
                    let a = Command::new(f.root.join(format!("llvm-{opt}")))
                        .arg("--json")
                        .args(args)
                        .output()
                        .unwrap();
                    let b = Command::new(f.root.join(format!("rust-{opt}")))
                        .arg("--json")
                        .args(args)
                        .output()
                        .unwrap();
                    assert_eq!(a.status.code(), Some(if fail { 1 } else { 0 }), "{a:?}");
                    assert_eq!(a.status.code(), b.status.code());
                    assert_eq!(a.stdout, b.stdout);
                    assert_eq!(a.stderr, b.stderr);
                    if !fail {
                        assert_eq!(
                            report(&a)["fields"],
                            json!([{"kind":"bytes","value":bytes}])
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn frontend_spawned_imports_keep_exact_producer_types_across_repeated_runs() {
        let f = Fixture::new(
            r#"
import "worker.tellur" as work;
async control startup(string text) exits done {
    auto task=spawn work.echo(text);yield;
    delta await task {ready {string result}=>ready}
    [ready]:emit(result);exit done;
}
control main() exits done {run startup("first");run startup("second");exit done;}
"#,
        );
        fs::write(
            f.root.join("worker.tellur"),
            r#"
pub async control echo(string text) exits ready {string result} {yield;exit ready {text};}
"#,
        )
        .unwrap();
        lexical_matrix(
            &f,
            &[json!({"entry":"main","arguments":[],"calls":[]})],
            &(0..=30).map(Some).chain([None]).collect::<Vec<_>>(),
        );
    }

    #[test]
    fn retained_operations_nested_in_synchronous_scopes_preserve_implicit_wrappers() {
        let prefix = "async control leaf() exits done {yield;exit done;} control wrapper() exits done {forward run leaf();}";
        let main = "control main() exits result {fault boundary {wrapper();exit done;} contingency [] {escalate;}} outcome result {@exit 0 done}";
        lexical_matrix(
            &Fixture::new(&format!("{prefix}{main}")),
            &[json!({"entry":"main","arguments":[],"calls":[]})],
            &(0..=30).map(Some).chain([None]).collect::<Vec<_>>(),
        );
    }
    fn host_fixture(source: &str) -> Fixture {
        let f = Fixture::new(source);
        let mut manifest: Json =
            serde_json::from_str(&fs::read_to_string(&f.manifest).unwrap()).unwrap();
        let extra: Json =
            serde_json::from_str(include_str!("../playground/llvm_host.manifest.json")).unwrap();
        manifest["primitives"]
            .as_array_mut()
            .unwrap()
            .extend(extra["primitives"].as_array().unwrap().iter().cloned());
        fs::write(&f.manifest, manifest.to_string()).unwrap();
        f
    }
    fn host_call(label: &str, waits: u32) -> Json {
        json!({"operation":"tick","outcome":"ticked","fields":[{"kind":"string","value":label}],"waits":waits})
    }
    fn successful_reference(f: &Fixture, scenario: &Json) -> Json {
        let checked = compile_file(
            &f.source,
            Manifest::from_json(&fs::read_to_string(&f.manifest).unwrap()).unwrap(),
        )
        .unwrap();
        serde_json::to_value(
            checked
                .run_completion(&serde_json::from_value(scenario.clone()).unwrap(), 10000)
                .unwrap(),
        )
        .unwrap()
    }
    #[test]
    fn frontend_host_operations_share_run_await_cold_and_spawned_tasks() {
        let f = host_fixture(include_str!("../playground/llvm_host.tellur"));
        let scenarios = [0,1,7].map(|waits| json!({"entry":"main","arguments":[],"calls":[host_call("run",waits),host_call("cold",waits),host_call("spawn",waits),host_call("await",waits)]}));
        for scenario in &scenarios {
            let report = successful_reference(&f, scenario);
            let emitted = report["events"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|e| e["kind"] == "emit")
                .map(|e| e["message"].as_str().unwrap())
                .collect::<Vec<_>>();
            assert_eq!(
                emitted,
                ["captured", "run", "cold", "queued", "spawn", "await"]
            );
        }
        lexical_matrix(
            &f,
            &scenarios,
            &(0..=55).map(Some).chain([None]).collect::<Vec<_>>(),
        );
    }

    #[test]
    fn frontend_host_operations_preserve_sibling_order_and_inline_run_isolation() {
        let f = host_fixture(
            r#"
control inline_tick() exits ticked {string text} {forward run tick("inline");}
async control startup() exits done {
    auto left=spawn tick("left");auto right=spawn tick("right");
    emit("queued");
    delta inline_tick() {ticked {string ran_text}=>ran}
    [ran]:emit(ran_text);
    auto moved=some {left};
    delta moved {some {auto task}=>task,none=>empty}
    [task]:delta await task {ticked {string left_text}=>left}
    [left]:emit(left_text);
    [left|empty]:delta await right {ticked {string right_text}=>right}
    [right]:emit(right_text);exit done;
}
control main() exits done {forward run startup();}
"#,
        );
        let scenarios=[0,1,9].map(|waits|json!({"entry":"main","arguments":[],"calls":[host_call("inline",waits),host_call("left",waits),host_call("right",waits)]}));
        for scenario in &scenarios {
            successful_reference(&f, scenario);
        }
        lexical_matrix(
            &f,
            &scenarios,
            &(0..=60).map(Some).chain([None]).collect::<Vec<_>>(),
        );
    }
    #[test]
    fn frontend_host_operations_cancel_before_start_while_pending_and_after_completion() {
        for (yields, waits, interrupted) in [(0, 20, true), (3, 20, true), (12, 0, false)] {
            for call in ["await", "run"] {
                let f = host_fixture(&format!(
                    r#"
{worker} control operation() exits done {{
    delta {call} tick("request") {{ticked {{string text}}=>ready,cancelled=>stopped}}
    [ready]:emit(text);exit done;
    [stopped]:emit("cancelled");exit done;
}}
async control worker() exits done {{ {enter} operation(); exit done; }}
async control startup() exits done {{
    auto task=spawn worker();{pause}cancel task;cancel task;await task;exit done;
}}
control main() exits done {{forward run startup();}}
"#,
                    pause = "yield;".repeat(yields),
                    worker = if call == "await" { "async" } else { "" },
                    enter = if call == "await" { "await" } else { "" }
                ));
                // run enters inline; a pending host leaf still yields to the source scheduler.
                let stopped = interrupted;
                let scenario = json!({"entry":"main","arguments":[],"calls":if yields==0 {vec![]} else {vec![host_call("complete",waits)]}});
                let report = successful_reference(&f, &scenario);
                let emitted = report["events"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|e| e["kind"] == "emit")
                    .map(|e| e["message"].as_str().unwrap())
                    .collect::<Vec<_>>();
                assert_eq!(
                    emitted,
                    vec![if stopped { "cancelled" } else { "complete" }],
                    "{call}/{yields}: {report}"
                );
                assert_eq!(
                    report["events"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .filter(|e| e["kind"] == "primitive")
                        .count(),
                    usize::from(!stopped)
                );
                lexical_matrix(
                    &f,
                    &[scenario],
                    &(0..=45).map(Some).chain([None]).collect::<Vec<_>>(),
                );
            }
        }
        // Without the explicit cancelled arm, inherited sticky requests do not interrupt I/O.
        let f = host_fixture(
            r#"
async control worker() exits done {delta await tick("request") {ticked {string text}=>ready}[ready]:emit(text);exit done;}
async control startup() exits done {auto task=spawn worker();cancel task;await task;exit done;}
control main() exits done {forward run startup();}
"#,
        );
        let scenario =
            json!({"entry":"main","arguments":[],"calls":[host_call("ordinary completion",5)]});
        successful_reference(&f, &scenario);
        lexical_matrix(
            &f,
            &[scenario],
            &(0..=30).map(Some).chain([None]).collect::<Vec<_>>(),
        );
    }
    #[test]
    fn frontend_host_operations_keep_borrowed_resources_and_validate_responses() {
        let f = Fixture::new(
            r#"
async control startup() exits done {
    delta open("input") {opened {handle file}=>opened,not_found=>absent,denied=>blocked}
    [absent|blocked]:exit done;
    [opened]:delta await read(borrow file) {data {bytes data}=>read,failed {read_error error}=>failed}
    [read]:emit("read");
    [opened]:delta close(move file) {closed=>closed,failed {close_error close_problem}=>failed_close}
    [opened]:exit done;
}
control main() exits done {forward run startup();}
"#,
        );
        let mut manifest: Json =
            serde_json::from_str(&fs::read_to_string(&f.manifest).unwrap()).unwrap();
        manifest["primitives"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|p| p["name"] == "read")
            .unwrap()["async"] = json!(true);
        fs::write(&f.manifest, manifest.to_string()).unwrap();
        let scenario = json!({"entry":"main","arguments":[],"calls":[
            {"operation":"open","outcome":"opened","fields":[{"kind":"opaque","value":{"ty":"handle","token":"file-0"}}],"waits":0},
            {"operation":"read","outcome":"data","fields":[{"kind":"bytes","value":[0,65,255]}],"waits":3},
            {"operation":"close","outcome":"closed","fields":[],"waits":0}
        ]});
        successful_reference(&f, &scenario);
        let mut malformed = scenario.clone();
        malformed["calls"][1]["fields"] = json!([{"kind":"bool","value":true}]);
        let mut wrong_operation = scenario.clone();
        wrong_operation["calls"][1]["operation"] = json!("close");
        let mut missing = scenario.clone();
        missing["calls"] = json!([]);
        lexical_matrix(
            &f,
            &[scenario, malformed, wrong_operation, missing],
            &(0..=38).map(Some).chain([None]).collect::<Vec<_>>(),
        );
    }

    #[test]
    fn frontend_host_operations_retire_pending_work_on_source_faults() {
        for fault in [
            "yield;yield;yield;fault \"sibling failed\";",
            "fault \"sibling failed\";",
        ] {
            let f = host_fixture(&format!(
                r#"
async control worker() exits done {{delta await tick("pending") {{ticked {{string text}}=>ready}}[ready]:emit(text);exit done;}}
async control failing() exits done faults {{{fault}}}
async control startup() exits done faults {{auto pending=spawn worker();auto failed=spawn failing();await pending;await failed;exit done;}}
control main() exits done faults {{forward run startup();}}
"#
            ));
            let scenario =
                json!({"entry":"main","arguments":[],"calls":[host_call("must not publish",100)]});
            let checked = compile_file(
                &f.source,
                Manifest::from_json(&fs::read_to_string(&f.manifest).unwrap()).unwrap(),
            )
            .unwrap();
            assert!(
                checked
                    .run_completion(&serde_json::from_value(scenario.clone()).unwrap(), 10000)
                    .is_err()
            );
            lexical_matrix(
                &f,
                &[scenario],
                &(0..=40).map(Some).chain([None]).collect::<Vec<_>>(),
            );
        }
    }

    #[test]
    fn frontend_named_supervision_preserves_borrowed_work_and_administrative_settlement() {
        let f = Fixture::new(include_str!("../playground/llvm_async_supervision.tellur"));
        lexical_matrix(
        &f,
        &[false, true].map(
            |fail| json!({"entry":"main","arguments":[{"kind":"bool","value":fail}],"calls":[]}),
        ),
        &(0..=65).map(Some).chain([None]).collect::<Vec<_>>(),
    );
        let ir = fs::read_to_string(f.root.join("llvm-2.ll")).unwrap();
        assert!(
            ir.contains("call void @tellur_rt_v1_supervise_children")
                && ir.contains("@retained_")
                && ir.contains("ret i32 12")
        );
        assert!(
            !ir.contains("call void @tellur_rt_v1_supervise(")
                && !ir.contains("llvm.coro")
                && !ir.contains("malloc")
        );
    }

    #[test]
    fn frontend_named_supervision_preserves_nested_escalation_and_secondary_causes() {
        let f = Fixture::new(
            r#"async control pause() exits done {
    yield;
    exit done;
}
boundary inner(u32 mode, string context) retains context, mode {
    run pause();
    delta mode == 0 {
        true => normal,
        false => broken
    }
    [normal]: emit("inner normal");
        terminate;
    [broken]: fault "inner first";
}
contingency(string reason) {
    emit(context);
    emit(reason);
    delta mode == 1 {
        true => handled,
        false => escaping
    }
    [handled]: terminate;
    [escaping]: delta mode == 2 {
        true => original,
        false => secondary
    }
    [original]: escalate;
    [secondary]: fault "cleanup second";
}
boundary outer(u32 mode, string context) retains context {
    supervise inner(mode, clone(borrow context));
    emit("outer normal");
    terminate;
}
contingency(string reason) {
    emit(context);
    emit(reason);
    terminate;
}
control main(u32 mode) exits done {
    supervise outer(mode, "outer context");
    emit("parent");
    exit done;
}
"#,
        );
        lexical_matrix(
        &f,
        &(0..4)
            .map(
                |mode| json!({"entry":"main","arguments":[{"kind":"u32","value":mode}],"calls":[]}),
            )
            .collect::<Vec<_>>(),
        &(0..=80).map(Some).chain([None]).collect::<Vec<_>>(),
    );
    }

    #[test]
    fn frontend_named_supervision_reuses_loop_frames_and_retained_declaration_order() {
        let f = Fixture::new(
            r#"async control examine(ref string text, bool broken) exits done faults {
    yield;
    emit(text);
    delta broken {
        true => bad,
        false => ok
    }
    [bad]: fault "request failed";
    [ok]: exit done;
}
boundary request(u32 index, string context) retains context, index {
    run examine(borrow context, index == 1);
    emit(u32_text(index));
    terminate;
}
contingency(string reason) {
    emit(context);
    emit(u32_text(index));
    emit(reason);
    terminate;
}
control main() exits done {
    loop requests(u32 index = 0, string parent = "parent") {
        supervise request(index, concat("request ", u32_text(index)));
        emit(borrow parent);
        delta index == 2 {
            true => last,
            false => more
        }
        [last]: exit done;
        [more]: delta checked_add(index, 1) {
            sum { u32 next } => next,
            overflow => bad
        }
        [bad]: fault "counter";
        [next]: again requests(next, parent);
    }
}
"#,
        );
        lexical_matrix(
            &f,
            &[
                json!({"entry":"main","arguments":[],"calls":[{"operation":"checked_add","outcome":"sum","fields":[{"kind":"u32","value":1}]},{"operation":"checked_add","outcome":"sum","fields":[{"kind":"u32","value":2}]}]}),
            ],
            &(0..=80).map(Some).chain([None]).collect::<Vec<_>>(),
        );
    }

    #[test]
    fn frontend_named_supervision_retires_spawned_and_set_members_before_handover() {
        for source in [
            r#"async control child(bool fail, string text) exits done { string result } faults {
    emit(borrow text);
    yield;
    delta fail {
        true => bad,
        false => ok
    }
    [bad]: fault "child failed";
    [ok]: exit done { text };
}
async control work(bool fail) exits done faults {
    auto first = spawn child(fail, "first");
    auto second = spawn child(false, "second");
    delta await second {
        done { string second_result } => second_done
    }
    [second_done]: emit(second_result);
        delta await first {
            done { string first_result } => first_done
        }
    [first_done]: emit(first_result);
        exit done;
}
boundary request(bool fail, string context) retains context {
    run work(fail);
    emit("request complete");
    terminate;
}
contingency(string reason) {
    emit(context);
    emit(reason);
    terminate;
}
async control startup(bool fail) exits done {
    supervise request(fail, "retained");
    emit("parent");
    exit done;
}
control main(bool fail) exits done {
    forward run startup(fail);
}
"#,
            r#"async control child(bool fail, string text) exits done { string result } faults {
    emit(borrow text);
    yield;
    delta fail {
        true => bad,
        false => ok
    }
    [bad]: fault "set member failed";
    [ok]: exit done { text };
}
async control work(bool fail) exits done faults {
    tasks<child> children = tasks();
    spawn child(fail, "first") into children;
    spawn child(false, "second") into children;
    loop reap(tasks<child> pending = move children) {
        delta await next pending {
            done { string text } => member,
            empty => drained
        }
        [member]: emit(text);
            again reap(pending);
        [drained]: exit done;
    }
}
boundary request(bool fail, string context) retains context {
    run work(fail);
    emit("request complete");
    terminate;
}
contingency(string reason) {
    emit(context);
    emit(reason);
    terminate;
}
async control startup(bool fail) exits done {
    supervise request(fail, "retained");
    emit("parent");
    exit done;
}
control main(bool fail) exits done {
    forward run startup(fail);
}
"#,
        ] {
            lexical_matrix(&Fixture::new(source),&[false,true].map(|fail|json!({"entry":"main","arguments":[{"kind":"bool","value":fail}],"calls":[]})),&(0..=80).map(Some).chain([None]).collect::<Vec<_>>());
        }
    }

    #[test]
    fn frontend_named_supervision_keeps_outside_tasks_and_owning_fault_authority() {
        for source in [
            r#"boundary request(bool broken, string context) retains context {
    delta broken {
        true => bad,
        false => ok
    }
    [bad]: fault "local request";
    [ok]: emit(context);
        terminate;
}
contingency(string reason) {
    emit(context);
    emit(reason);
    terminate;
}
control helper() exits done {
    supervise request(true, "helper context");
    exit done;
}
async control worker(ref string text) exits done { ref string text from text } {
    emit("child started");
    yield;
    exit done { text };
}
async control startup() exits done {
    string text = "child result";
    auto child = spawn worker(borrow text);
    helper();
    emit("parent still owns execution");
    delta await child {
        done { auto result } => joined
    }
    [joined]: emit(result);
        supervise request(false, "direct context");
        exit done;
}
control main() exits done {
    forward run startup();
}
"#,
            r#"async control crash() exits done faults {
    emit("outside child");
    yield;
    fault "outside child fault";
}
async control pause() exits done {
    emit("helper work");
    yield;
    yield;
    exit done;
}
boundary local(string context) retains context {
    run pause();
    emit("local success");
    terminate;
}
contingency(string reason) {
    emit("wrong local handler");
    emit(context);
    emit(reason);
    terminate;
}
control helper() exits done {
    supervise local("local context");
    exit done;
}
async control startup() exits done faults {
    auto child = spawn crash();
    helper();
    await child;
    exit done;
}
boundary owner(string context) retains context {
    run startup();
    terminate;
}
contingency(string reason) {
    emit(context);
    emit(reason);
    terminate;
}
control main() exits done {
    supervise owner("owning handler");
    emit("parent");
    exit done;
}
"#,
        ] {
            lexical_matrix(
                &Fixture::new(source),
                &[json!({"entry":"main","arguments":[],"calls":[]})],
                &(0..=80).map(Some).chain([None]).collect::<Vec<_>>(),
            );
        }
    }

    #[test]
    fn frontend_named_supervision_preserves_host_readiness_cancellation_and_driver_errors() {
        {
            let f = host_fixture(
                r#"async control operation(bool fail) exits done faults {
    delta await tick("request") {
        ticked { string text } => ready,
        cancelled => stopped
    }
    [stopped]: emit("cancelled");
        exit done;
    [ready]: emit(text);
        delta fail {
            true => bad,
            false => ok
        }
    [bad]: fault "after host";
    [ok]: exit done;
}
boundary request(bool fail, string context) retains context {
    run operation(fail);
    emit("request complete");
    terminate;
}
contingency(string reason) {
    emit("owning handler");
    emit(context);
    emit(reason);
    terminate;
}
async control supervisor(bool fail) exits done {
    supervise request(fail, "retained host");
    emit("supervisor continued");
    exit done;
}
control main(bool fail) exits done {
    forward run supervisor(fail);
}
"#,
            );
            let mut scenarios = vec![
                json!({"entry":"main","arguments":[{"kind":"bool","value":false}],"calls":[{"operation":"tick","outcome":"ticked","fields":[{"kind":"string","value":"completed"}],"waits":7}]}),
                json!({"entry":"main","arguments":[{"kind":"bool","value":true}],"calls":[{"operation":"tick","outcome":"ticked","fields":[{"kind":"string","value":"completed"}],"waits":7}]}),
            ];
            for scenario in &scenarios {
                successful_reference(&f, scenario);
            }
            let mut malformed = scenarios[0].clone();
            malformed["calls"][0]["fields"] = json!([{"kind":"bool","value":true}]);
            let mut wrong = scenarios[0].clone();
            wrong["calls"][0]["operation"] = json!("wrong_operation");
            let mut missing = scenarios[0].clone();
            missing["calls"] = json!([]);
            scenarios.extend([malformed, wrong, missing]);
            lexical_matrix(
                &f,
                &scenarios,
                &(0..=65).map(Some).chain([None]).collect::<Vec<_>>(),
            );
        }
        {
            let f = host_fixture(
                r#"async control operation(bool fail) exits done faults {
    delta await tick("request") {
        ticked { string text } => ready,
        cancelled => stopped
    }
    [stopped]: emit("cancelled");
        exit done;
    [ready]: emit(text);
        delta fail {
            true => bad,
            false => ok
        }
    [bad]: fault "after host";
    [ok]: exit done;
}
boundary request(bool fail, string context) retains context {
    run operation(fail);
    emit("request complete");
    terminate;
}
contingency(string reason) {
    emit("owning handler");
    emit(context);
    emit(reason);
    terminate;
}
async control supervisor(bool fail) exits done {
    supervise request(fail, "retained host");
    emit("supervisor continued");
    exit done;
}
async control startup(bool fail) exits done {
    auto task = spawn supervisor(fail);
    cancel task;
    cancel task;
    await task;
    exit done;
}
control main(bool fail) exits done {
    forward run startup(fail);
}
"#,
            );
            let scenarios = vec![
                json!({"entry":"main","arguments":[{"kind":"bool","value":false}],"calls":[]}),
                json!({"entry":"main","arguments":[{"kind":"bool","value":true}],"calls":[]}),
            ];
            for scenario in &scenarios {
                successful_reference(&f, scenario);
            }
            lexical_matrix(
                &f,
                &scenarios,
                &(0..=65).map(Some).chain([None]).collect::<Vec<_>>(),
            );
        }
        {
            let f = host_fixture(
                r#"async control operation(bool fail) exits done faults {
    delta await tick("request") {
        ticked { string text } => ready,
        cancelled => stopped
    }
    [stopped]: emit("cancelled");
        exit done;
    [ready]: emit(text);
        delta fail {
            true => bad,
            false => ok
        }
    [bad]: fault "after host";
    [ok]: exit done;
}
boundary request(bool fail, string context) retains context {
    run operation(fail);
    emit("request complete");
    terminate;
}
contingency(string reason) {
    emit("owning handler");
    emit(context);
    emit(reason);
    terminate;
}
async control supervisor(bool fail) exits done {
    supervise request(fail, "retained host");
    emit("supervisor continued");
    exit done;
}
async control startup(bool fail) exits done {
    auto task = spawn supervisor(fail);
    yield;
    yield;
    yield;
    cancel task;
    cancel task;
    await task;
    exit done;
}
control main(bool fail) exits done {
    forward run startup(fail);
}
"#,
            );
            let scenarios = vec![
                json!({"entry":"main","arguments":[{"kind":"bool","value":false}],"calls":[{"operation":"tick","outcome":"ticked","fields":[{"kind":"string","value":"completed"}],"waits":7}]}),
                json!({"entry":"main","arguments":[{"kind":"bool","value":true}],"calls":[{"operation":"tick","outcome":"ticked","fields":[{"kind":"string","value":"completed"}],"waits":7}]}),
            ];
            for scenario in &scenarios {
                successful_reference(&f, scenario);
            }
            lexical_matrix(
                &f,
                &scenarios,
                &(0..=65).map(Some).chain([None]).collect::<Vec<_>>(),
            );
        }
        {
            let f = host_fixture(
                r#"async control operation(bool fail) exits done faults {
    delta await tick("request") {
        ticked { string text } => ready,
        cancelled => stopped
    }
    [stopped]: emit("cancelled");
        exit done;
    [ready]: emit(text);
        delta fail {
            true => bad,
            false => ok
        }
    [bad]: fault "after host";
    [ok]: exit done;
}
boundary request(bool fail, string context) retains context {
    run operation(fail);
    emit("request complete");
    terminate;
}
contingency(string reason) {
    emit("owning handler");
    emit(context);
    emit(reason);
    terminate;
}
async control supervisor(bool fail) exits done {
    supervise request(fail, "retained host");
    emit("supervisor continued");
    exit done;
}
async control startup(bool fail) exits done {
    auto task = spawn supervisor(fail);
    yield;
    yield;
    yield;
    yield;
    yield;
    yield;
    yield;
    yield;
    yield;
    yield;
    yield;
    yield;
    yield;
    yield;
    yield;
    yield;
    cancel task;
    cancel task;
    await task;
    exit done;
}
control main(bool fail) exits done {
    forward run startup(fail);
}
"#,
            );
            let scenarios = vec![
                json!({"entry":"main","arguments":[{"kind":"bool","value":false}],"calls":[{"operation":"tick","outcome":"ticked","fields":[{"kind":"string","value":"completed"}],"waits":0}]}),
                json!({"entry":"main","arguments":[{"kind":"bool","value":true}],"calls":[{"operation":"tick","outcome":"ticked","fields":[{"kind":"string","value":"completed"}],"waits":0}]}),
            ];
            for scenario in &scenarios {
                successful_reference(&f, scenario);
            }
            lexical_matrix(
                &f,
                &scenarios,
                &(0..=65).map(Some).chain([None]).collect::<Vec<_>>(),
            );
        }
    }

    #[test]
    fn frontend_named_supervision_retains_aggregate_owners_through_awaited_and_spawned_parents() {
        for invocation in [
            "await supervisor(enabled, fail);",
            "auto task=defer supervisor(enabled, fail);await task;",
            "auto task=spawn supervisor(enabled, fail);await task;",
        ] {
            let f = Fixture::new(&format!(
                r#"
struct envelope {{string label;vector<string> words;}}
async control examine(ref envelope context,bool fail) exits done faults {{
    yield;emit(borrow context.label);
    emit(u32_text(vector_length(borrow context.words)));
    delta fail {{true=>bad,false=>ok}}
    [bad]:fault "aggregate request";
    [ok]:exit done;
}}
boundary request(vector<string> extra,envelope context,bool fail) retains context,extra {{
    run examine(borrow context,fail);
    emit(u32_text(vector_length(borrow extra)));terminate;
}}
contingency(string reason) {{
    emit(context.label);
    emit(u32_text(vector_length(borrow context.words)));
    emit(u32_text(vector_length(borrow extra)));
    emit(reason);terminate;
}}
async control supervisor(bool enabled,bool fail) exits done {{
    string parent="parent retained";
    delta enabled {{true=>selected,false=>skipped}}
    [selected]:supervise request(["extra"],envelope {{"envelope",["a","b"]}},fail);
    [skipped]:emit("skipped");
    [entry]:emit(parent);exit done;
}}
async control startup(bool enabled,bool fail) exits done {{{invocation}exit done;}}
control main(bool enabled,bool fail) exits done {{forward run startup(enabled,fail);}}
"#
            ));
            lexical_matrix(
                &f,
                &[(false,true),(true,false),(true,true)].map(|(enabled,fail)|json!({
                    "entry":"main","arguments":[{"kind":"bool","value":enabled},{"kind":"bool","value":fail}],"calls":[]
                })),
                &(0..=80).map(Some).chain([None]).collect::<Vec<_>>(),
            );
        }
    }

    #[test]
    fn frontend_named_supervision_preserves_imported_private_and_contingent_names() {
        let f = Fixture::new(
            r#"import "helpers.tellur" as helpers;
control main() exits done {
    supervise helpers.request("retained");
    emit("parent");
    exit done;
}
"#,
        );
        fs::write(
            f.root.join("helpers.tellur"),
            r#"async control fail() exits done faults {
    yield;
    fault "imported fault";
}
@contingent
control report(ref string context, string reason) exits done {
    emit(context);
    emit(reason);
    exit done;
}
pub boundary request(string context) retains context {
    run fail();
    terminate;
}
contingency(string reason) {
    report(borrow context, reason);
    terminate;
}
"#,
        )
        .unwrap();
        lexical_matrix(
            &f,
            &[json!({"entry":"main","arguments":[],"calls":[]})],
            &(0..=50).map(Some).chain([None]).collect::<Vec<_>>(),
        );
    }

    #[test]
    fn frontend_lexical_scopes_preserve_normal_fallthrough_and_task_set_members() {
        for source in [
            "async control work() exits done {fault boundary {yield;exit done;} contingency [] {escalate;}} async control startup() exits done {tasks<work> set=tasks();spawn work() into set;loop reap(tasks<work> pending=move set){delta await next pending {done=>next,empty=>empty} [next]:again reap(pending);[empty]:exit done;}} control main() exits result {forward run startup();} outcome result {@exit 0 done}",
            "async control helper() exits done {loop L(){fault boundary {yield;exit done;} contingency [] {escalate;}}} control main() exits result {forward run helper();} outcome result {@exit 0 done}",
            "async control helper() exits done {string text=\"owner\";fault boundary {yield;emit(borrow text);} contingency [text] {escalate;} emit(text);exit done;} control main() exits result {forward run helper();} outcome result {@exit 0 done}",
        ] {
            lexical_matrix(
                &Fixture::new(source),
                &[json!({"entry":"main","arguments":[],"calls":[]})],
                &(0..=35).map(Some).chain([None]).collect::<Vec<_>>(),
            );
        }
    }
    #[test]
    fn frontend_lexical_recovery_preserves_entry_values_and_reused_path_facts() {
        let f = Fixture::new(include_str!("../playground/llvm_async_recovery.tellur"));
        lexical_matrix(&f, &[false,true].map(|fail| json!({"entry":"main","arguments":[{"kind":"bool","value":fail}],"calls":[]})),
            &(0..=75).map(Some).chain([None]).collect::<Vec<_>>());
        let ir = fs::read_to_string(f.root.join("llvm-2.ll")).unwrap();
        assert!(ir.contains("%compiled_control_v4") && ir.contains("@tellur_rt_v4_run_entry"));
        assert!(ir.contains("@tellur_rt_v1_scope") && ir.contains("@tellur_rt_v1_loop"));
        assert!(ir.contains("define void @reset_") && ir.contains("@scopes_"));
    }

    #[test]
    fn frontend_lexical_scopes_preserve_outward_again_and_expanded_completion() {
        for source in [
            r#"async control pause() exits done {yield;exit done;} control helper() exits done {string text} {loop L(){fault boundary {run pause();exit done {"expanded"};} contingency [] {escalate;}}} control main() exits done {delta helper(){done {string text}=>done}[done]:emit(text);exit done;}"#,
            r#"async control session() exits done {loop L(u32 n=0){delta n<3 {true=>more,false=>last}[last]:exit done;[more]:delta checked_add(n,1){sum {u32 next}=>next,overflow=>bad}[bad]:fault "counter";[next]:fault boundary {yield;emit(u32_text(next));again L(next);} contingency [] {escalate;}}} control main() exits done {forward run session();}"#,
            r#"async control session() exits done {yield;fault boundary {} contingency [] {escalate;} emit("after empty");exit done;} control main() exits done {forward run session();}"#,
        ] {
            lexical_matrix(
                &Fixture::new(source),
                &[json!({"entry":"main","arguments":[],"calls":[]})],
                &(0..=65).map(Some).chain([None]).collect::<Vec<_>>(),
            );
        }
    }
    #[test]
    fn frontend_lexical_scopes_preserve_nested_fault_causes_and_process_status() {
        for status in [0, 23, 255] {
            let source = r#"async control fail() exits done faults {yield;fault "first";} control main() exits done {fault boundary {fault boundary {run fail();exit done;} contingency [] {fault "second";}} contingency [] {emit(borrow @fault.message);terminate process 23;}}"#;
            lexical_matrix(
                &Fixture::new(&source.replace("process 23", &format!("process {status}"))),
                &[json!({"entry":"main","arguments":[],"calls":[]})],
                &(0..=40).map(Some).chain([None]).collect::<Vec<_>>(),
            );
        }
        lexical_matrix(
            &Fixture::new(
                r#"async control session() exits done {supervised loop L(mutable bool first=true){bool fail=first;first=false;fault boundary {delta fail {true=>bad,false=>ok}[ok]:exit done;[bad]:fault boundary {yield;fault "first";} contingency [] {fault "second";}} contingency [] {emit(borrow @fault.message);recover again L;}}} control main() exits done {forward run session();}"#,
            ),
            &[json!({"entry":"main","arguments":[],"calls":[]})],
            &(0..=60).map(Some).chain([None]).collect::<Vec<_>>(),
        );
    }
    #[test]
    fn frontend_lexical_scopes_compose_with_named_work_and_cold_borrowed_children() {
        for source in [
            r#"async control child(ref string text) exits done faults {yield;emit(text);fault "child";} async control borrow_work(ref string context) exits done faults {auto work=defer child(context);await work;exit done;} boundary request(string context) retains context {supervised loop L(mutable bool first=true){bool fail=first;first=false;string request_context=clone(borrow context);fault boundary {delta fail {true=>bad,false=>ok}[ok]:emit(request_context);terminate;[bad]:run borrow_work(borrow request_context);terminate;} contingency [] {emit(borrow @fault.message);recover again L;}}} contingency(string reason){emit(context);emit(reason);terminate;} async control session() exits done {supervise request("named owner");exit done;} control main() exits done {forward run session();}"#,
            r#"async control child() exits done faults {yield;fault "named child";} boundary request(){run child();terminate;} contingency(string reason){emit(reason);escalate;} async control session() exits done {supervised loop L(mutable bool first=true){bool fail=first;first=false;fault boundary {delta fail {true=>bad,false=>ok}[ok]:exit done;[bad]:supervise request();exit done;} contingency [] {emit(borrow @fault.message);recover again L;}}} control main() exits done {forward run session();}"#,
        ] {
            lexical_matrix(
                &Fixture::new(source),
                &[json!({"entry":"main","arguments":[],"calls":[]})],
                &(0..=70).map(Some).chain([None]).collect::<Vec<_>>(),
            );
        }
    }
    #[test]
    fn frontend_lexical_recovery_preserves_pending_host_and_driver_error_authority() {
        let source = include_str!("../playground/llvm_async_recovery.tellur").replace("            yield;", r#"            delta await tick("request") {ticked {string reply}=>ready,cancelled=>stopped}
            [ready]:emit(reply);
            [ready|stopped]:yield;"#);
        assert!(source.contains("await tick"));
        let f = host_fixture(&source);
        let mut scenarios = vec![
            json!({"entry":"main","arguments":[{"kind":"bool","value":false}],"calls":[host_call("first",5)]}),
            json!({"entry":"main","arguments":[{"kind":"bool","value":true}],"calls":[host_call("first",5),host_call("second",3)]}),
        ];
        for scenario in &scenarios {
            successful_reference(&f, scenario);
        }
        let mut wrong = scenarios[1].clone();
        wrong["calls"][0]["fields"] = json!([{"kind":"bool","value":true}]);
        scenarios.push(wrong);
        let mut missing = scenarios[1].clone();
        missing["calls"] = json!([]);
        scenarios.push(missing);
        lexical_matrix(
            &f,
            &scenarios,
            &(0..=80).map(Some).chain([None]).collect::<Vec<_>>(),
        );
    }

    #[test]
    fn frontend_lexical_cancellation_preserves_exclusive_ancestor_storage() {
        for pauses in ["", "yield;", "yield;yield;yield;"] {
            let source=r#"async control worker(ref mutable string text) exits done {string request="local request";fault boundary {yield;emit(request);} contingency [] {escalate;}deref text="updated";yield;emit(text);exit done;} async control session() exits done {mutable string owner="original";auto child=spawn worker(borrow mutable owner);PAUSE cancel child;await child;emit(owner);exit done;} control main() exits done {forward run session();}"#.replace("PAUSE",pauses);
            lexical_matrix(
                &Fixture::new(&source),
                &[json!({"entry":"main","arguments":[],"calls":[]})],
                &(0..=50).map(Some).chain([None]).collect::<Vec<_>>(),
            );
        }
    }
    #[test]
    fn frontend_lexical_recovery_retires_borrowing_siblings_before_reusing_owners() {
        let source = r#"async control child(ref string text) exits done faults {yield;emit(text);fault "sibling";}
async control session(bool parent_fault) exits done {
 supervised loop L(mutable bool first=true) {
  bool fail=first;first=false;bool fail_parent=parent_fault;mutable string context="retained owner";
  fault boundary {
   delta fail {true=>work,false=>finished}
   [finished]:emit(context);exit done;
   [work]:auto child=spawn child(borrow context);yield;
    delta fail_parent {true=>parent,false=>join}
   [parent]:fault "parent";
   [join]:await child;exit done;
  } contingency [context] {context="retired borrowers";emit(context);recover again L;}
 }
}
control main(bool parent_fault) exits done {forward run session(parent_fault);}"#;
        lexical_matrix(&Fixture::new(source), &[false,true].map(|parent|json!({"entry":"main","arguments":[{"kind":"bool","value":parent}],"calls":[]})),
            &(0..=65).map(Some).chain([None]).collect::<Vec<_>>());
    }
    #[test]
    fn frontend_lexical_imported_helpers_preserve_guarded_scope_paths_and_borrows() {
        let f = Fixture::new(
            r#"import "helper.tellur" as h;
async control session(bool selected) exits done {
 string owner="imported owner";
 delta selected {true=>work,false=>skip}
 [skip]:emit(owner);exit done;
 [work]:fault boundary {await h.inspect(borrow owner);emit(owner);exit done;} contingency [owner] {emit(borrow @fault.message);escalate;}
}
control main(bool selected) exits done {forward run session(selected);}"#,
        );
        fs::write(f.root.join("helper.tellur"),r#"pub async control inspect(ref string owner) exits done faults {string request=clone(owner);fault boundary {yield;emit(request);} contingency [] {escalate;}emit(owner);exit done;}"#).unwrap();
        lexical_matrix(&f,&[false,true].map(|selected|json!({"entry":"main","arguments":[{"kind":"bool","value":selected}],"calls":[]})),
            &(0..=50).map(Some).chain([None]).collect::<Vec<_>>());
    }

    fn launch_signal(name: &str) -> Json {
        json!({"operation":"$signal","outcome":name,"fields":[],"waits":0})
    }
    fn signal_fixture(source: &str, policy: bool) -> Fixture {
        let f = host_fixture(source);
        let mut manifest: Json =
            serde_json::from_str(&fs::read_to_string(&f.manifest).unwrap()).unwrap();
        manifest["launch"]["main"] = json!({"exit_codes":{"done":0}});
        if policy {
            manifest["launch"]["main"]["signals"] = json!("cancel");
        }
        fs::write(&f.manifest, manifest.to_string()).unwrap();
        f
    }
    #[test]
    fn frontend_launch_signals_cancel_pending_sets_and_before_first_host_start() {
        let f = signal_fixture(
            include_str!("../playground/llvm_launch_signals.tellur"),
            true,
        );
        let mut scenarios = vec![];
        for name in ["SIGINT", "SIGTERM"] {
            scenarios.push(json!({"entry":"main","arguments":[],"calls":[launch_signal(name)]}));
            let idle = json!({"entry":"main","arguments":[],"calls":[host_call("first",0),host_call("member",5),host_call("second",5),launch_signal(name)]});
            let expected = successful_reference(&f, &idle);
            assert_eq!(expected["outcome"], "done");
            let emits = expected["events"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|e| e["kind"] == "emit")
                .map(|e| e["message"].as_str().unwrap())
                .collect::<Vec<_>>();
            assert_eq!(emits, ["first", "member stopped", "stopping"]);
            scenarios.push(idle);
        }
        lexical_matrix(
            &f,
            &scenarios,
            &(0..=90).map(Some).chain([None]).collect::<Vec<_>>(),
        );
    }
    #[test]
    fn frontend_launch_signals_remain_sticky_for_later_children_and_do_not_interrupt_joins() {
        let f = signal_fixture(
            r#"
async control worker(ref mutable string text) exits done {
    yield;delta await tick("worker") {ticked {string reply}=>ready,cancelled=>stopped}
    [ready]:deref text=reply;[stopped]:deref text="cancelled child";[entry]:exit done;
}
async control startup() exits done {
    delta await tick("entry") {ticked {string reply}=>ready,cancelled=>stopped}
    [ready]:emit(reply);[stopped]:emit("entry stopped");
    [entry]:mutable string text="original";
    auto cold=defer worker(borrow mutable text);
    await cold;emit(borrow text);
    auto active=spawn worker(borrow mutable text);
    delta await active {done=>joined}
    [joined]:emit(borrow text);
    delta await tick("nonobserving") {ticked {string final_reply}=>completed}
    [completed]:emit(final_reply);await worker(borrow mutable text);emit(text);exit done;
}
control main() exits done {forward run startup();}
"#,
            true,
        );
        let scenarios=["SIGINT","SIGTERM"].map(|name|json!({"entry":"main","arguments":[],"calls":[launch_signal(name),host_call("nonobserving completed",5)]}));
        for scenario in &scenarios {
            let expected = successful_reference(&f, scenario);
            let emits = expected["events"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|e| e["kind"] == "emit")
                .map(|e| e["message"].as_str().unwrap())
                .collect::<Vec<_>>();
            assert_eq!(
                emits,
                [
                    "entry stopped",
                    "cancelled child",
                    "cancelled child",
                    "nonobserving completed",
                    "cancelled child"
                ]
            );
        }
        lexical_matrix(
            &f,
            &scenarios,
            &(0..=80).map(Some).chain([None]).collect::<Vec<_>>(),
        );
    }
    #[test]
    fn frontend_launch_signals_preserve_constructor_destinations_and_caller_recovery() {
        let f = signal_fixture(
            r#"
struct message {string text;constructor(bool fail) {
    delta run tick("constructor") {ticked {string reply}=>ready,cancelled=>stopped}
    [ready]:self.text=reply;[stopped]:self.text="constructor cancelled";
    [entry]:delta fail {true=>bad,false=>good}[bad]:fault "constructor source fault";
}}
async control service(bool fail) exits done {
    supervised loop again_request(mutable bool first=fail) {
        bool broken=first;first=false;
        fault boundary {auto value=message(broken);emit(value.text);exit done;}
        contingency [] {emit(borrow @fault.message);recover again again_request;}
    }
}
control main(bool fail) exits done {forward run service(fail);}
"#,
            true,
        );
        let scenarios=[false,true].map(|fail|json!({"entry":"main","arguments":[{"kind":"bool","value":fail}],"calls":[host_call("pending",5),launch_signal("SIGTERM")]}));
        for scenario in &scenarios {
            successful_reference(&f, scenario);
        }
        lexical_matrix(
            &f,
            &scenarios,
            &(0..=80).map(Some).chain([None]).collect::<Vec<_>>(),
        );
    }
    #[test]
    fn frontend_launch_signals_use_the_selected_entry_policy_and_preserve_synchronous_entries() {
        let f = signal_fixture(
            r#"
async control startup() exits done {
    delta await tick("request") {ticked {string text}=>ready,cancelled=>stopped}
    [ready]:emit(text);[stopped]:emit("stopped");[entry]:exit done;
}
control main() exits result {forward run startup();}
control other() exits result {forward run startup();}
control plain() exits result {emit("synchronous");exit done;}
outcome result {@exit 0 done}
"#,
            true,
        );
        let scenarios = [
            json!({"entry":"main","arguments":[],"calls":[launch_signal("SIGINT")]}),
            json!({"entry":"other","arguments":[],"calls":[launch_signal("SIGINT")]}),
            json!({"entry":"other","arguments":[],"calls":[host_call("ordinary",3)]}),
            json!({"entry":"plain","arguments":[],"calls":[]}),
            json!({"entry":"plain","arguments":[],"calls":[launch_signal("SIGTERM")]}),
        ];
        lexical_matrix(
            &f,
            &scenarios,
            &(0..=26).map(Some).chain([None]).collect::<Vec<_>>(),
        );
        let path = f.root.join("policy.json");
        fs::write(&path, scenarios[1].to_string()).unwrap();
        let got = execute(&f.root.join("llvm-2"), &path, None);
        assert!(!got.status.success());
        assert!(String::from_utf8_lossy(&got.stderr).contains("declare launch.<entry>.signals"));
        // Signal policy alone does not force a synchronous source into task frames.
        let f = signal_fixture(
            "control main() exits done {emit(\"plain\");exit done;}",
            true,
        );
        lexical_matrix(
            &f,
            &[json!({"entry":"main","arguments":[],"calls":[]})],
            &[None],
        );
        let ir = fs::read_to_string(f.root.join("llvm-2.ll")).unwrap();
        assert!(!ir.contains("@async_controls ="));
    }
    #[test]
    fn frontend_launch_signal_driver_errors_bypass_supervision_and_preserve_reports() {
        let source = r#"
async control work() exits done {
    fault boundary {
        delta await tick("request") {ticked {string text}=>ready,cancelled=>stopped}
        [ready]:emit(text);[stopped]:emit("stopped");[entry]:exit done;
    } contingency [] {emit("unexpected recovery");escalate;}
}
control main() exits done {forward run work();}
"#;
        for policy in [false, true] {
            let f = signal_fixture(source, policy);
            let scenarios = if policy {
                vec![
                    json!({"entry":"main","arguments":[],"calls":[launch_signal("SIGINT"),launch_signal("SIGTERM")]}),
                    json!({"entry":"main","arguments":[],"calls":[host_call("pending",4),launch_signal("SIGTERM"),launch_signal("SIGINT")]}),
                    json!({"entry":"main","arguments":[],"calls":[launch_signal("UNKNOWN")]}),
                    json!({"entry":"main","arguments":[],"calls":[host_call("pending",4),launch_signal("UNKNOWN")]}),
                    json!({"entry":"main","arguments":[],"calls":[host_call("ready",0),host_call("unused",0)]}),
                ]
            } else {
                vec![json!({"entry":"main","arguments":[],"calls":[launch_signal("SIGTERM")]})]
            };
            lexical_matrix(
                &f,
                &scenarios,
                &(0..=25).map(Some).chain([None]).collect::<Vec<_>>(),
            );
            for scenario in &scenarios {
                let path = f.root.join("error.json");
                fs::write(&path, scenario.to_string()).unwrap();
                let out = execute(&f.root.join("llvm-2"), &path, None);
                assert!(!out.status.success());
                assert!(String::from_utf8_lossy(&out.stderr).contains("E_DRIVER"));
                assert!(!String::from_utf8_lossy(&out.stdout).contains("unexpected recovery"));
            }
        }
    }

    #[test]
    fn frontend_launch_signals_preserve_live_first_default_and_second_signal_dispositions() {
        use std::io::{BufRead, Read};
        use std::os::unix::process::ExitStatusExt;
        use std::process::{Child, Stdio};
        use std::time::{Duration, Instant};
        struct OwnedChild(Option<Child>);
        impl Drop for OwnedChild {
            fn drop(&mut self) {
                if let Some(child) = self.0.as_mut() {
                    let _ = child.kill();
                    let _ = child.wait();
                }
            }
        }
        unsafe extern "C" {
            fn waitpid(pid: i32, status: *mut i32, options: i32) -> i32;
        }
        let f = host_fixture(
            r#"async control startup(string marker) exits stopped {
    emit("signal test ready");
    loop wait_marker {
        yield;
        delta open(clone(borrow marker)) {
            opened { handle file } => ready,
            not_found => absent,
            denied => denied
        }
        [absent | denied]: again wait_marker;
        [ready]:
            delta close(move file) {
                closed => closed,
                failed { close_error problem } => failed
            }
        [failed]: drop(problem);
        [closed | failed]:
            delta await tick("after signal") {
                ticked { string reply } => replied,
                cancelled => cancelled
            }
        [replied]: emit(reply);
            exit stopped;
        [cancelled]: emit("cancelled by launch signal");
            exit stopped;
    }
}
control main(string marker) exits result {
    forward run startup(marker);
}
outcome result {
    @exit 0 stopped
}
"#,
        );
        for policy in [true, false] {
            let mut manifest: Json =
                serde_json::from_str(&fs::read_to_string(&f.manifest).unwrap()).unwrap();
            manifest["launch"]["main"] = json!({"exit_codes":{"stopped":0}});
            if policy {
                manifest["launch"]["main"]["signals"] = json!("cancel");
            }
            fs::write(&f.manifest, manifest.to_string()).unwrap();
            for expanded in [false, true] {
                for opt in [0, 2] {
                    build_pair(&f, expanded, opt);
                    for backend in ["rust", "llvm"] {
                        for mode in if policy {
                            vec!["INT", "TERM", "second"]
                        } else {
                            vec!["INT", "TERM"]
                        } {
                            let marker = f.root.join("continue-signal");
                            let _ = fs::remove_file(&marker);
                            let child = Command::new(f.root.join(format!("{backend}-{opt}")))
                                .arg(&marker)
                                .stdout(Stdio::piped())
                                .stderr(Stdio::piped())
                                .spawn()
                                .unwrap();
                            let mut child = OwnedChild(Some(child));
                            let stdout = child.0.as_mut().unwrap().stdout.take().unwrap();
                            let (tx, rx) = std::sync::mpsc::channel();
                            let reader = std::thread::spawn(move || {
                                let mut reader = std::io::BufReader::new(stdout);
                                let mut first = String::new();
                                reader.read_line(&mut first).unwrap();
                                let _ = tx.send(first.clone());
                                reader.read_to_string(&mut first).unwrap();
                                first
                            });
                            assert_eq!(
                                rx.recv_timeout(Duration::from_secs(15)).unwrap(),
                                "signal test ready\n"
                            );
                            let pid = child.0.as_ref().unwrap().id();
                            let send = |name: &str| {
                                assert!(
                                    Command::new("/bin/kill")
                                        .arg(format!("-{name}"))
                                        .arg(pid.to_string())
                                        .status()
                                        .unwrap()
                                        .success()
                                );
                            };
                            if mode == "second" {
                                send("STOP");
                                let until = Instant::now() + Duration::from_secs(5);
                                loop {
                                    let mut status = 0;
                                    let result = unsafe { waitpid(pid as i32, &mut status, 3) };
                                    if result == pid as i32 {
                                        assert_eq!(status & 0xff, 0x7f);
                                        break;
                                    }
                                    assert_eq!(result, 0);
                                    assert!(Instant::now() < until, "child did not stop");
                                    std::thread::sleep(Duration::from_millis(1));
                                }
                                send("INT");
                                send("TERM");
                                send("CONT");
                            } else {
                                send(mode);
                                if policy {
                                    fs::write(&marker, "continue").unwrap();
                                }
                            }
                            let until = Instant::now() + Duration::from_secs(15);
                            loop {
                                if child.0.as_mut().unwrap().try_wait().unwrap().is_some() {
                                    break;
                                }
                                assert!(
                                    Instant::now() < until,
                                    "{backend}/{expanded}/{opt}/{policy}/{mode}: child did not exit"
                                );
                                std::thread::sleep(Duration::from_millis(2));
                            }
                            let output = child.0.take().unwrap().wait_with_output().unwrap();
                            let stdout = reader.join().unwrap();
                            assert!(output.stderr.is_empty(), "{output:?}");
                            if !policy {
                                assert_eq!(
                                    output.status.signal(),
                                    Some(if mode == "INT" { 2 } else { 15 })
                                );
                                assert_eq!(stdout, "signal test ready\n");
                            } else if mode == "second" {
                                assert!(
                                    matches!(output.status.code(), Some(130 | 143)),
                                    "{output:?}"
                                );
                                assert_eq!(stdout, "signal test ready\n");
                            } else {
                                assert!(output.status.success(), "{output:?}");
                                assert_eq!(
                                    stdout,
                                    "signal test ready\ncancelled by launch signal\nstopped\n"
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn unsupported_checked_operations_create_no_build_outputs() {
        // Rust adapter host leaves remain gated through every supported task form.
        for body in [
            "await tick();",
            "auto task=defer tick();await task;",
            "auto task=spawn tick();await task;",
        ] {
            let f = Fixture::new(&format!(
                "async control startup() exits done {{{body} exit done;}} control main() exits result {{forward run startup();}} outcome result {{@exit 0 done}}"
            ));
            let mut manifest: Json =
                serde_json::from_str(&fs::read_to_string(&f.manifest).unwrap()).unwrap();
            manifest["primitives"].as_array_mut().unwrap().push(json!({
                "name":"tick", "async":true, "parameters":[],
                "outcomes":[{"name":"ticked","fields":[]}],
                "effects":["clock.tick"], "returns":true, "rust":{"execution":"poll"}
            }));
            manifest["rust"] = json!({"source":"host.rs","lockfile":"Cargo.lock"});
            fs::write(
                f.root.join("host.rs"),
                "// Adapter remains outside this backend checkpoint.\n",
            )
            .unwrap();
            fs::write(f.root.join("Cargo.lock"), "version = 4\n").unwrap();
            fs::write(&f.manifest, manifest.to_string()).unwrap();
            let out = f.command("llvm", 0).output().unwrap();
            assert!(!out.status.success());
            assert!(
                String::from_utf8_lossy(&out.stderr).contains("E_BACKEND"),
                "{out:?}"
            );
            assert!(!f.root.join("llvm-0.tellur-build").exists());
            assert!(!f.root.join("llvm-0.ll").exists());
        }
    }
}
