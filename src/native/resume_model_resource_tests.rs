use super::*;
use serde_json::json;

const HOST: &str = r#"
use crate::rust_api;
use std::io::Write;
pub struct Counter { path: String, id: u32 }
impl Counter {
    fn record(&self, what: &str) {
        writeln!(std::fs::OpenOptions::new().create(true).append(true)
            .open(&self.path).unwrap(), "{what} {}", self.id).unwrap();
    }
}
impl Drop for Counter { fn drop(&mut self) { self.record("drop"); } }
pub fn make(path: &str, id: u32) -> rust_api::make_outcome {
    rust_api::make_outcome::made(Counter { path: path.into(), id })
}
pub fn inspect(value: &Counter) -> rust_api::inspect_outcome {
    value.record("borrow"); rust_api::inspect_outcome::done
}
pub fn close(value: Counter) -> rust_api::close_outcome {
    value.record("close"); drop(value); rust_api::close_outcome::done
}
"#;
const SOURCE: &str = r#"
async control observe(ref counter value) exits done {
    inspect(value);
    exit done;
}
async control dispose(counter value) exits done {
    close(value);
    exit done;
}
async control work(ref counter parent, counter child, bool stop) exits done faults {
    auto borrowed = defer observe(parent);
    auto owned = defer dispose(child);
    delta stop { true => failed, false => ready }
    [failed]: fault "abandoned children";
    [ready]: await borrowed; await owned; exit done;
}
control main(string path, bool stop) exits done {
    delta make(borrow path, 1) { made { counter parent } => parent }
    [parent]: delta make(borrow path, 2) { made { counter child } => child }
    [child]: fault boundary {
        run work(borrow parent, child, stop);
    } contingency [parent] {
        close(parent);
        terminate process 23;
    }
    [child]: close(parent); exit done;
}
"#;

#[test]
fn compiled_boundary_pauses_preserve_real_owner_cleanup_at_every_fuel_cut() {
    compare_resources("lexical", SOURCE, [0, 23]);
}

#[test]
fn compiled_named_handover_preserves_real_owners_before_work_frame_retirement() {
    let source = SOURCE
        .replace(
            "control main(string path, bool stop) exits done {",
            r#"boundary request(counter parent, counter child, bool stop) retains parent {
    run work(borrow parent, child, stop);
    close(parent);
    terminate;
} contingency(string reason) {close(parent);terminate;}
control main(string path, bool stop) exits done {"#,
        )
        .replace(
            r#"[child]: fault boundary {
        run work(borrow parent, child, stop);
    } contingency [parent] {
        close(parent);
        terminate process 23;
    }
    [child]: close(parent); exit done;"#,
            "[child]: supervise request(parent, child, stop); exit done;",
        );
    assert!(source.contains("supervise request(parent, child, stop)"));
    compare_resources("named", &source, [0, 0]);
}

fn compare_resources(label: &str, source: &str, statuses: [i32; 2]) {
    let root = std::env::temp_dir().join(format!(
        "tellur-resume-resources-{label}-{}",
        std::process::id()
    ));
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("host.rs"), HOST).unwrap();
    fs::write(
        root.join("Cargo.lock"),
        include_str!("../../examples/retained_file/Cargo.lock"),
    )
    .unwrap();
    let config = json!({
        "version":"0.1",
        "types":[{"name":"counter","kind":"opaque","linear":true,"fault":"release"}],
        "rust":{"source":"host.rs","lockfile":"Cargo.lock","resources":{"counter":"Counter"}},
        "primitives":[
            {"name":"make","parameters":[{"name":"path","type":"string","mode":"borrow"},{"name":"id","type":"u32","mode":"value"}],"outcomes":[{"name":"made","fields":[{"name":"value","type":"counter"}]}],"effects":["make"],"returns":true,"rust":{"execution":"immediate"}},
            {"name":"inspect","parameters":[{"name":"value","type":"counter","mode":"borrow"}],"outcomes":[{"name":"done","fields":[]}],"effects":["observe"],"returns":true,"rust":{"execution":"immediate"}},
            {"name":"close","parameters":[{"name":"value","type":"counter","mode":"consume"}],"outcomes":[{"name":"done","fields":[]}],"effects":["close"],"returns":true,"contingent":true,"rust":{"execution":"immediate"}}
        ]
    });
    let manifest =
        crate::manifest::Manifest::from_json_at(&config.to_string(), &root.join("manifest.json"))
            .unwrap();
    let p = crate::compile(source, manifest)
        .unwrap()
        .with_entry_exit_codes("main", BTreeMap::from([("done".into(), 0)]))
        .unwrap();
    for (expanded, program) in [(false, p.clone()), (true, p.expanded("main").unwrap())] {
        for opt in [0, 2] {
            let normal = root.join(format!("normal-{expanded}-{opt}"));
            program.build_native("main", &normal, opt).unwrap();
            let resumed = root.join(format!("resumed-{expanded}-{opt}"));
            let project = program.native_project("main", &resumed).unwrap();
            fs::write(&project.2, super::source(&program, "main").unwrap()).unwrap();
            program.finish_native_build(project, opt).unwrap();
            let log = root.join("owners.log");
            for stop in [false, true] {
                let mut finished = false;
                for fuel in 0..150 {
                    let run = |binary: &Path, quotas: &str| {
                        if log.exists() {
                            fs::remove_file(&log).unwrap();
                        }
                        let output = Command::new(binary)
                            .args([
                                "--json",
                                "--fuel",
                                &fuel.to_string(),
                                "--",
                                log.to_str().unwrap(),
                                &stop.to_string(),
                            ])
                            .env("TELLUR_NATIVE_MODEL_QUOTAS", quotas)
                            .output()
                            .unwrap();
                        let drops = fs::read_to_string(&log).unwrap_or_default();
                        (output, drops)
                    };
                    let (expected, drops) = run(&normal, "1");
                    for quotas in ["1", "0,2,0,7", "3,1,9"] {
                        let (actual, actual_drops) = run(&resumed, quotas);
                        let context = format!(
                            "expanded={expanded},opt={opt},stop={stop},fuel={fuel},quotas={quotas}"
                        );
                        assert_eq!(actual.status.code(), expected.status.code(), "{context}");
                        assert_eq!(actual.stdout, expected.stdout, "{context}");
                        assert_eq!(actual.stderr, expected.stderr, "{context}");
                        assert_eq!(actual_drops, drops, "{context}");
                    }
                    if expected.status.code() == Some(statuses[usize::from(stop)]) {
                        assert_eq!(
                            drops,
                            if stop {
                                "drop 2\nclose 1\ndrop 1\n"
                            } else {
                                "borrow 1\nclose 2\ndrop 2\nclose 1\ndrop 1\n"
                            }
                        );
                        finished = true;
                        break;
                    }
                }
                assert!(
                    finished,
                    "resource program did not reach normal/process completion"
                );
            }
        }
    }
    fs::remove_dir_all(root).unwrap();
}
