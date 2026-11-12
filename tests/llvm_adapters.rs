//! Actual typed static libraries, linked to the precompiled C runtime.
#![cfg(all(feature = "llvm", target_os = "macos", target_arch = "aarch64"))]
use serde_json::{Value as Json, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicUsize, Ordering},
};
use tellur::{CheckedProgram, compile, manifest::Manifest};

struct Fixture {
    root: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "tellur-llvm-adapter-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("Cargo.lock"),
            include_str!("../examples/retained_file/Cargo.lock"),
        )
        .unwrap();
        Self { root }
    }
    fn program(&self, source: &str, manifest: &Json) -> CheckedProgram {
        let p = compile(
            source,
            Manifest::from_json_at(&manifest.to_string(), &self.root.join("manifest.json"))
                .unwrap(),
        )
        .unwrap();
        let codes = p
            .regions()
            .find(|r| r.name == "main")
            .unwrap()
            .exits
            .iter()
            .map(|o| (o.name.clone(), 0))
            .collect();
        p.with_entry_exit_codes("main", codes).unwrap()
    }
    fn build(&self, p: &CheckedProgram, name: &str, llvm: bool, opt: u8) -> PathBuf {
        let path = self.root.join(format!("{name}-{llvm}-{opt}"));
        if llvm {
            p.build_llvm("main", &path, opt)
        } else {
            p.build_native("main", &path, opt)
        }
        .unwrap();
        path
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
fn config() -> Json {
    json!({"version":"0.1","types":[],"rust":{"source":"host.rs","lockfile":"Cargo.lock"},"primitives":[{
        "name":"inspect","parameters":[{"name":"value","type":"string","mode":"borrow"}],
        "outcomes":[{"name":"ready","fields":[{"name":"value","type":"string"}]}],"effects":[],"returns":true,"rust":{"execution":"immediate"}
    }]})
}
fn run(binary: &Path, args: &[Json]) -> Output {
    Command::new(binary)
        .args(["--json", "--"])
        .args(args.iter().map(|v| match v["kind"].as_str() {
            Some("string") => v["value"].as_str().unwrap().to_owned(),
            Some("bool" | "u32") => v["value"].to_string(),
            _ => v.to_string(),
        }))
        .output()
        .unwrap()
}
fn report(output: &Output) -> Json {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|_| {
        panic!(
            "status {:?}: {}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}
fn string(s: &str) -> Json {
    json!({"kind":"string","value":s})
}
fn collection(ty: &str, fields: Vec<Json>) -> Json {
    json!({"kind":"construct","value":{"ty":ty,"name":ty.split('<').next().unwrap(),"fields":fields}})
}

#[test]
fn source_modules_and_all_signature_checks_survive_the_static_boundary() {
    let f = Fixture::new();
    let mut m = config();
    m["rust"]["modules"] = json!({"type":"shared files/é.rs"});
    let p = f.program(
        "control main(string text) exits auto {forward inspect(borrow text);}",
        &m,
    );
    // Checking and LLVM emission inspect contracts without reading adapter files.
    assert!(
        p.llvm_source("main")
            .unwrap()
            .contains("@tellur_rt_v2_launch")
    );
    let failure = p
        .build_llvm("main", &f.root.join("missing"), 0)
        .unwrap_err();
    assert_eq!(failure.code, "E_RUST_ADAPTER");
    assert!(failure.message.contains("cannot read Rust adapter"));
    fs::write(f.root.join("host.rs"), "pub use self::r#type::inspect;").unwrap();
    let failure = p
        .build_llvm("main", &f.root.join("missing-module"), 0)
        .unwrap_err();
    assert!(failure.message.contains("module `type`"));
    fs::create_dir(f.root.join("shared files")).unwrap();
    let child = f.root.join("shared files/é.rs");
    fs::write(&child,"//! Child documentation.\npub fn inspect(value:&str)->crate::rust_api::inspect_outcome {crate::rust_api::inspect_outcome::ready(format!(\"{value}!\"))}\n// trailing comment").unwrap();
    let first = f.build(&p, "module", true, 0);
    for input in ["", "ASCII", "žluť 😀"] {
        let output = run(&first, &[string(input)]);
        assert!(output.status.success());
        assert_eq!(
            report(&output)["fields"],
            json!([string(&format!("{input}!"))])
        );
    }
    std::thread::scope(|scope| {
        let first = scope.spawn(|| f.build(&p, "concurrent-a", true, 0));
        let second = scope.spawn(|| f.build(&p, "concurrent-b", true, 0));
        for binary in [first.join().unwrap(), second.join().unwrap()] {
            assert_eq!(
                report(&run(&binary, &[string("parallel")]))["fields"][0],
                string("parallel!")
            );
        }
    });
    fs::write(&child,"pub fn inspect(value:&str)->crate::rust_api::inspect_outcome {crate::rust_api::inspect_outcome::ready(format!(\"{value}?\"))}").unwrap();
    let second = f.build(&p, "changed-module", true, 0);
    assert_eq!(
        report(&run(&second, &[string("new")]))["fields"][0],
        string("new?")
    );
    // The earlier executable remains independently usable after a source-cache change.
    assert_eq!(
        report(&run(&first, &[string("old")]))["fields"][0],
        string("old!")
    );
    fs::write(&child,"pub fn inspect(_:u32)->crate::rust_api::inspect_outcome {crate::rust_api::inspect_outcome::ready(String::new())}").unwrap();
    let unused = f.program("control main() exits done {exit done;}", &m);
    let error = unused
        .build_llvm("main", &f.root.join("wrong-unused"), 0)
        .unwrap_err();
    assert_eq!(error.code, "E_BACKEND");
    assert!(error.message.contains("mismatched types"), "{error}");
}

#[test]
fn scalar_and_nested_collection_adapters_match_rust_in_both_graphs_and_optimizations() {
    let f = Fixture::new();
    let mut m = config();
    let parameters = [
        ("text", "string", "borrow"),
        ("data", "bytes", "consume"),
        ("items", "vector<vector<string>>", "borrow"),
        ("mapping", "map<string,map<u32,bytes>>", "consume"),
        ("pairs", "map<bool,vector<string>>", "borrow"),
        ("nothing", "unit", "borrow"),
        ("choice", "bool", "borrow"),
        ("maximum", "u32", "borrow"),
        ("owned", "bool", "value"),
        ("count", "u32", "value"),
    ];
    m["primitives"][0]["parameters"] = json!(
        parameters
            .iter()
            .map(|(name, ty, mode)| json!({"name":name,"type":ty,"mode":mode}))
            .collect::<Vec<_>>()
    );
    let fields = [
        ("text", "string"),
        ("data", "bytes"),
        ("items", "vector<vector<string>>"),
        ("mapping", "map<string,map<u32,bytes>>"),
        ("pairs", "map<bool,vector<string>>"),
        ("nothing", "unit"),
        ("choice", "bool"),
        ("maximum", "u32"),
    ];
    m["primitives"][0]["outcomes"] = json!([{"name":"ready","fields":fields.iter().map(|(name,ty)|json!({"name":name,"type":ty})).collect::<Vec<_>>()},{"name":"empty","fields":[]}]);
    m["primitives"][0]["rust"]["execution"] = json!("blocking");
    fs::write(f.root.join("host.rs"),r#"
pub fn inspect(text:&str,data:Vec<u8>,items:&[Vec<String>],mapping:std::collections::BTreeMap<String,std::collections::BTreeMap<u32,Vec<u8>>>,pairs:&std::collections::BTreeMap<bool,Vec<String>>,nothing:&(),choice:&bool,maximum:&u32,owned:bool,count:u32)->crate::rust_api::inspect_outcome {
    assert_eq!(*maximum,u32::MAX);assert_eq!(*choice,owned);assert_eq!(count,17);
    if *choice {crate::rust_api::inspect_outcome::ready(text.into(),data,items.to_vec(),mapping,pairs.clone(),*nothing,owned,*maximum)} else {crate::rust_api::inspect_outcome::empty}
}
"#).unwrap();
    // A synchronous foreign call inside an async child does not create a handoff.
    let p=f.program(r#"
async control child(string text, bytes data, vector<vector<string>> items, map<string,map<u32,bytes>> mapping, map<bool,vector<string>> pairs, bool choice) exits auto {
    unit nothing=unit;u32 maximum=4294967295;
    forward inspect(borrow text,data,borrow items,mapping,borrow pairs,borrow nothing,borrow choice,borrow maximum,choice,17);
}
control main(string text, bytes data, vector<vector<string>> items, map<string,map<u32,bytes>> mapping, map<bool,vector<string>> pairs, bool choice) exits auto {
    forward run child(text,data,items,mapping,pairs,choice);
}
"#,&m);
    for expanded in [false, true] {
        let p = if expanded {
            p.expanded("main").unwrap()
        } else {
            p.clone()
        };
        for opt in [0, 2] {
            let rust = f.build(&p, &format!("data-{expanded}"), false, opt);
            let llvm = f.build(&p, &format!("data-{expanded}"), true, opt);
            for filled in [false, true] {
                for choice in [false, true] {
                    let text = string(if filled { "héllo 😀" } else { "" });
                    let data =
                        json!({"kind":"bytes","value":if filled {vec![0,128,255]}else{vec![]}});
                    let items = collection(
                        "vector<vector<String>>",
                        if filled {
                            vec![
                                collection("vector<String>", vec![]),
                                collection("vector<String>", vec![string("é"), string("")]),
                            ]
                        } else {
                            vec![]
                        },
                    );
                    let mapping = collection(
                        "map<String,map<U32,Bytes>>",
                        if filled {
                            vec![
                                string("a"),
                                collection(
                                    "map<U32,Bytes>",
                                    vec![
                                        json!({"kind":"u32","value":0}),
                                        data.clone(),
                                        json!({"kind":"u32","value":4294967295u32}),
                                        json!({"kind":"bytes","value":[]}),
                                    ],
                                ),
                                string("😀"),
                                collection("map<U32,Bytes>", vec![]),
                            ]
                        } else {
                            vec![]
                        },
                    );
                    let pairs = collection(
                        "map<Bool,vector<String>>",
                        if filled {
                            vec![
                                json!({"kind":"bool","value":false}),
                                collection("vector<String>", vec![string("false")]),
                                json!({"kind":"bool","value":true}),
                                collection("vector<String>", vec![]),
                            ]
                        } else {
                            vec![]
                        },
                    );
                    let arguments = vec![
                        text.clone(),
                        data.clone(),
                        items.clone(),
                        mapping.clone(),
                        pairs.clone(),
                        json!({"kind":"bool","value":choice}),
                    ];
                    let a = run(&llvm, &arguments);
                    let b = run(&rust, &arguments);
                    assert_eq!(a.status.code(), b.status.code());
                    assert!(a.status.success(), "{}", String::from_utf8_lossy(&a.stderr));
                    assert_eq!(
                        report(&a),
                        report(&b),
                        "expanded {expanded} opt {opt} filled {filled} choice {choice}"
                    );
                    assert_eq!(
                        report(&a)["fields"],
                        if choice {
                            json!([text,data,items,mapping,pairs,{"kind":"unit"},{"kind":"bool","value":choice},{"kind":"u32","value":4294967295u32}])
                        } else {
                            json!([])
                        }
                    );
                    let scenario = f.root.join("script.json");
                    fs::write(&scenario,json!({"entry":"main","arguments":arguments,"calls":[{"operation":"inspect","outcome":"empty","fields":[]}]}).to_string()).unwrap();
                    let scripted = |binary: &Path| {
                        Command::new(binary)
                            .arg("--scenario")
                            .arg(&scenario)
                            .output()
                            .unwrap()
                    };
                    let a = scripted(&llvm);
                    let b = scripted(&rust);
                    assert_eq!(a.status.code(), b.status.code());
                    assert_eq!(report(&a), report(&b));
                    assert_eq!(report(&a)["outcome"], "empty");
                }
            }
        }
    }
}

#[test]
fn dependencies_keep_explicit_locking_across_repeated_builds() {
    let f = Fixture::new();
    let mut m = config();
    m["rust"]["dependencies"] = json!({"memchr":{"version":"=2.8.3","default_features":false}});
    fs::write(f.root.join("host.rs"),"pub fn inspect(value:&str)->crate::rust_api::inspect_outcome {let at=memchr::memchr(b'!',value.as_bytes()).unwrap();crate::rust_api::inspect_outcome::ready(value[..at].into())}").unwrap();
    let p = f.program(
        "control main(string text) exits auto {forward inspect(borrow text);}",
        &m,
    );
    let lock = fs::read(f.root.join("Cargo.lock")).unwrap();
    let error = p
        .build_llvm("main", &f.root.join("stale-lock"), 0)
        .unwrap_err();
    assert!(error.message.contains("locked"), "{error}");
    assert_eq!(fs::read(f.root.join("Cargo.lock")).unwrap(), lock);
    p.lock_native("main", &f.root.join("lock-out")).unwrap();
    let lock = fs::read(f.root.join("Cargo.lock")).unwrap();
    let first = f.build(&p, "dependency", true, 0);
    assert_eq!(
        report(&run(&first, &[string("héllo!suffix")]))["fields"][0],
        string("héllo")
    );
    let second = f.build(&p, "repeat", true, 0);
    assert_eq!(
        run(&first, &[string("!")]).stdout,
        run(&second, &[string("!")]).stdout
    );
    assert_eq!(fs::read(f.root.join("Cargo.lock")).unwrap(), lock);
    fs::write(f.root.join("Cargo.lock"), "version = 4\n").unwrap();
    let error = p
        .build_llvm("main", &f.root.join("invalid-lock"), 0)
        .unwrap_err();
    assert!(error.message.contains("lock-native"), "{error}");
}

#[test]
fn resource_and_polled_profiles_remain_rejected_before_any_output() {
    let f = Fixture::new();
    for mode in [0, 1] {
        let mut m = config();
        if mode == 0 {
            m["types"] = json!([{"name":"resource","kind":"opaque","linear":true}]);
            m["rust"]["resources"] = json!({"resource":"Resource"});
        } else {
            m["primitives"][0]["async"] = json!(true);
            m["primitives"][0]["parameters"][0]["mode"] = json!("consume");
            m["primitives"][0]["rust"]["execution"] = json!("poll");
        }
        let p = f.program("control main() exits done {exit done;}", &m);
        let path = f.root.join(format!("gated-{mode}"));
        let error = p.build_llvm("main", &path, 0).unwrap_err();
        assert_eq!(error.code, "E_BACKEND");
        assert!(error.message.contains("resources and polled"));
        assert!(!path.exists());
        assert!(!PathBuf::from(format!("{}.tellur-build", path.display())).exists());
        assert!(!PathBuf::from(format!("{}.ll", path.display())).exists());
    }
}

#[test]
fn zero_arguments_owned_data_faults_and_every_fuel_cut_keep_host_semantics() {
    let f = Fixture::new();
    let mut m = config();
    m["primitives"][0]["parameters"] = json!([
        {"name":"value","type":"string","mode":"consume"},
        {"name":"bytes","type":"bytes","mode":"borrow"},
        {"name":"items","type":"vector<u32>","mode":"consume"},
        {"name":"nothing","type":"unit","mode":"value"}
    ]);
    m["primitives"].as_array_mut().unwrap().push(json!({"name":"ping","parameters":[],"outcomes":[{"name":"done","fields":[]}],"effects":[],"returns":true,"rust":{"execution":"immediate"}}));
    fs::write(f.root.join("host.rs"),r#"
pub fn ping()->crate::rust_api::ping_outcome {crate::rust_api::ping_outcome::done}
pub fn inspect(value:String,bytes:&[u8],items:Vec<u32>,nothing:())->crate::rust_api::inspect_outcome {
    assert_eq!(bytes,&[0,255]);assert_eq!(items,vec![0,u32::MAX]);let ()=nothing;crate::rust_api::inspect_outcome::ready(value)
}
"#).unwrap();
    let p = f.program(
        r#"
control main(bool fail, string text, bytes data) exits done faults {
    delta ping() {done=>pinged}
    [pinged]: delta inspect(text,borrow data,[0,4294967295],unit) {ready {string value}=>inspected}
    [inspected]: emit(value);delta fail {true=>bad,false=>good}
    [bad]: fault "after adapter";
    [good]: exit done;
}
"#,
        &m,
    );
    for expanded in [false, true] {
        let p = if expanded {
            p.expanded("main").unwrap()
        } else {
            p.clone()
        };
        for opt in [0, 2] {
            let rust = f.build(&p, &format!("fault-{expanded}"), false, opt);
            let llvm = f.build(&p, &format!("fault-{expanded}"), true, opt);
            for fail in [false, true] {
                for fuel in 0..=18 {
                    let execute = |binary: &Path| {
                        Command::new(binary)
                            .args([
                                "--json",
                                "--fuel",
                                &fuel.to_string(),
                                "--",
                                &fail.to_string(),
                                "hello",
                                r#"{"kind":"bytes","value":[0,255]}"#,
                            ])
                            .output()
                            .unwrap()
                    };
                    let a = execute(&llvm);
                    let b = execute(&rust);
                    assert_eq!(a.status.code(), b.status.code());
                    assert_eq!(a.stdout, b.stdout, "{expanded}/{opt}/{fail}/{fuel}");
                    assert_eq!(a.stderr, b.stderr);
                }
            }
            let scenario = f.root.join("invalid-script.json");
            for fields in [json!([]), json!([{"kind":"bool","value":true}])] {
                fs::write(&scenario,json!({"entry":"main","arguments":[{"kind":"bool","value":false},string("hello"),{"kind":"bytes","value":[0,255]}],"calls":[{"operation":"ping","outcome":"done","fields":[]},{"operation":"inspect","outcome":"ready","fields":fields}]}).to_string()).unwrap();
                let execute = |binary: &Path| {
                    Command::new(binary)
                        .arg("--scenario")
                        .arg(&scenario)
                        .output()
                        .unwrap()
                };
                let a = execute(&llvm);
                let b = execute(&rust);
                assert!(!a.status.success());
                assert_eq!(a.status.code(), b.status.code());
                assert_eq!(a.stdout, b.stdout);
                assert_eq!(a.stderr, b.stderr);
            }
        }
    }
}

#[test]
fn foreign_panics_stay_fatal_and_scripted_calls_never_invoke_user_code() {
    let f = Fixture::new();
    fs::write(f.root.join("host.rs"),"pub fn inspect(_: &str)->crate::rust_api::inspect_outcome {panic!(\"foreign host failure\")}").unwrap();
    let p = f.program(
        "control main() exits auto {forward inspect(borrow \"input\");}",
        &config(),
    );
    for expanded in [false, true] {
        let p = if expanded {
            p.expanded("main").unwrap()
        } else {
            p.clone()
        };
        for opt in [0, 2] {
            let binary = f.build(&p, &format!("panic-{expanded}"), true, opt);
            let output = run(&binary, &[]);
            assert!(!output.status.success());
            assert!(String::from_utf8_lossy(&output.stderr).contains("foreign host failure"));
            assert!(output.stdout.is_empty());
            let scenario = f.root.join("script.json");
            fs::write(&scenario,json!({"entry":"main","arguments":[],"calls":[{"operation":"inspect","outcome":"ready","fields":[string("scripted")]}]}).to_string()).unwrap();
            let output = Command::new(&binary)
                .arg("--scenario")
                .arg(&scenario)
                .output()
                .unwrap();
            assert!(output.status.success());
            assert_eq!(report(&output)["fields"], json!([string("scripted")]));
        }
    }
}
