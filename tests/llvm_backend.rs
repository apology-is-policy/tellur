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
    fn unsupported_primitive_signatures_fail_before_output_creation() {
        for borrowed in [false, true] {
            let f = Fixture::new(if borrowed {
                "control main(string input) exits result {delta custom(borrow input) {done=>done} [done]:exit done;} outcome result {@exit 0 done}"
            } else {
                "control main() exits result {delta custom() {done=>done,bad {auto ignored}=>bad} [done|bad]:exit done;} outcome result {@exit 0 done}"
            });
            let mut manifest: Json =
                serde_json::from_str(&fs::read_to_string(&f.manifest).unwrap()).unwrap();
            let (parameters, outcomes) = if borrowed {
                (
                    json!([{"name":"input","type":"String","mode":"borrow"}]),
                    json!([{"name":"done","fields":[]}]),
                )
            } else {
                (
                    json!([]),
                    json!([{"name":"done","fields":[]},{"name":"bad","fields":[{"name":"error","type":"read_error"}]}]),
                )
            };
            manifest["primitives"].as_array_mut().unwrap().push(json!({"name":"custom","parameters":parameters,"outcomes":outcomes,"effects":[],"returns":true}));
            fs::write(&f.manifest, manifest.to_string()).unwrap();
            let checked = compile_file(
                &f.source,
                Manifest::from_json(&manifest.to_string()).unwrap(),
            )
            .unwrap();
            assert_eq!(checked.llvm_source("main").unwrap_err().code, "E_BACKEND");
            let out = f.command("llvm", 0).output().unwrap();
            assert!(!out.status.success());
            assert!(String::from_utf8_lossy(&out.stderr).contains("E_BACKEND"));
            assert!(!f.root.join("llvm-0.tellur-build").exists());
            assert!(!f.root.join("llvm-0.ll").exists());
        }
    }

    #[test]
    fn unsupported_checked_operations_create_no_build_outputs() {
        for source in [
            "struct ticket {u32 value;constructor(u32 input){self.value=input;}} control main() exits result {drop(ticket(1));exit done;} outcome result {@exit 0 done}",
            "struct box {option<read_error> value;} control main() exits result {drop(box {none});exit done;} outcome result {@exit 0 done}",
            "variant choice {one {read_error error},two} control main() exits result {drop(choice.two {});exit done;} outcome result {@exit 0 done}",
            "struct view['a] {ref string value <- 'a;} control main() exits result {string s=\"text\";auto held=view {borrow s};drop(held);exit done;} outcome result {@exit 0 done}",
            "struct box {vector<u32> value;} control main() exits result {drop(box {[1,2]});exit done;} outcome result {@exit 0 done}",
            "struct box {read_error value;} control main(read_error e) exits result {drop(box {e});exit done;} outcome result {@exit 0 done}",
            "control main() exits result {exit done;} async control helper() exits done {exit done;} outcome result {@exit 0 done}",
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
