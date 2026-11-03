//! Generated pending borrowers stopped by a separately selected owning fault.
use super::*;

fn fault_manifest(root: &Path) -> Manifest {
    let mut config = serde_json::to_value(manifest(root)).unwrap();
    config["primitives"].as_array_mut().unwrap().push(json!({
        "name":"checkpoint","parameters":[],"outcomes":[{"name":"done","fields":[]}],
        "effects":["checkpoint"],"returns":true,"contingent":true,
        "rust":{"execution":"immediate"}
    }));
    let host = HOST
        .replace("pub struct Probe {", "pub struct Probe { pub id:usize,")
        .replace("push(\"resource drop\".into())", "push(format!(\"resource drop {}\", self.id))")
        .replace("trace.borrow_mut().push(\"make\".into());", "let id=trace.borrow().iter().filter(|s|s.starts_with(\"make \")).count(); trace.borrow_mut().push(format!(\"make {id}\"));")
        .replace("Probe {alive:", "Probe {id,alive:")
        .replace("push(\"close\".into())", "push(format!(\"close {}\",value.id))");
    fs::write(root.join("host.rs"), format!(r#"{host}
pub fn checkpoint() -> rust_api::checkpoint_outcome {{
    TRACE.with(|t| {{
        let mut trace=t.borrow_mut();
        for expected in ["drop left","drop right","resource drop 1","resource drop 2","resource drop 3"] {{
            assert!(trace.iter().any(|s|s==expected),"supplier settlement missing: {{expected}}");
        }}
        assert!(!trace.iter().any(|s|s=="resource drop 0"));
        trace.push("checkpoint".into());
    }});
    rust_api::checkpoint_outcome::done
}}
"#)).unwrap();
    Manifest::from_json_at(&config.to_string(), &root.join("manifest.json")).unwrap()
}

fn fault_source(p: &CheckedProgram) -> String {
    let mut code = source(p, true);
    let original = include_str!("pending_operation_driver.rs");
    assert_eq!(code.matches(original).count(), 1);
    code = code.replacen(
        original,
        &format!("{original}\n{}", include_str!("pending_fault_driver.rs")),
        1,
    );
    code = code.replace(
        "pub use adapter_operation_model::drive_pending;",
        "pub use adapter_operation_model::{drive_pending,drive_pending_fault};",
    );
    code = code.replace(
        "AdmissionStop, drive_pending};",
        "AdmissionStop, drive_pending, drive_pending_fault};",
    );
    let id = |name: &str| p.regions.keys().position(|r| r == name).unwrap();
    let main = id("main");
    let entry = format!("drive_pending(host,args,&MODEL_{main})");
    assert_eq!(code.matches(&entry).count(), 1);
    code.replacen(
        &entry,
        &format!(
            "drive_pending_fault(host,[&MODEL_{main},&MODEL_{},&MODEL_{},&MODEL_{}])",
            id("lexical"),
            id("named"),
            id("independent")
        ),
        1,
    )
}

fn compare_fault(
    p: &CheckedProgram,
    binary: &Path,
    root: &Path,
    fuel: usize,
    mode: u8,
    quotas: &str,
    independent: bool,
) -> bool {
    let quota = quotas
        .split(',')
        .map(|s| s.parse().unwrap())
        .collect::<Vec<_>>();
    let reference = crate::runtime::pending_fault_model(p, fuel, mode, &quota, independent);
    let log = root.join("trace.log");
    let events = root.join("events.json");
    let actual = Command::new(binary)
        .args(["--json", "--fuel", "10000"])
        .env("TELLUR_FAULT_FUEL", fuel.to_string())
        .env("TELLUR_FAULT_MODE", mode.to_string())
        .env("TELLUR_FAULT_INDEPENDENT", independent.to_string())
        .env("TELLUR_NATIVE_MODEL_QUOTAS", quotas)
        .env("TELLUR_OPERATION_LOG", &log)
        .env("TELLUR_FAULT_EVENTS", &events)
        .output()
        .unwrap();
    let context = format!(
        "{}, fuel={fuel}, mode={mode}, quotas={quotas}, independent={independent}",
        binary.display()
    );
    assert_eq!(
        actual.status.code(),
        Some(if reference.is_ok() { 0 } else { 1 }),
        "{context}: {}",
        String::from_utf8_lossy(&actual.stderr)
    );
    let expected_events = match &reference {
        Ok(result) => {
            let value = serde_json::to_value(result).unwrap();
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&actual.stdout).unwrap(),
                value,
                "{context}"
            );
            assert!(actual.stderr.is_empty(), "{context}");
            value["events"].clone()
        }
        Err(failure) => {
            assert!(actual.stdout.is_empty(), "{context}");
            let mut report: crate::fault::Report = serde_json::from_slice(&actual.stderr).unwrap();
            normalize_fuel(&mut report, &failure.report);
            assert_eq!(report, failure.report, "{context}");
            serde_json::to_value(&failure.events).unwrap()
        }
    };
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&fs::read(&events).unwrap()).unwrap(),
        expected_events,
        "{context}"
    );
    let trace = fs::read_to_string(&log).unwrap();
    let lines = trace.lines().collect::<Vec<_>>();
    let position = |text: &str| {
        assert_eq!(
            lines.iter().filter(|s| **s == text).count(),
            1,
            "{context}: {text}: {trace}"
        );
        lines.iter().position(|s| *s == text).unwrap()
    };
    for (name, id) in [("left", 1), ("right", 3)] {
        position(&format!("start {name}"));
        assert!(position(&format!("poll {name} partial @57")) < position(&format!("drop {name}")));
        assert!(
            position(&format!("drop {name}")) < position(&format!("resource drop {id}")),
            "{context}"
        );
    }
    for id in 0..4 {
        position(&format!("make {id}"));
        position(&format!("resource drop {id}"));
    }
    assert!(
        !lines
            .iter()
            .any(|s| s.starts_with("sample ") || s.starts_with("deliver ") || *s == "inspect"),
        "{context}"
    );
    if lines.contains(&"checkpoint") {
        let checkpoint = position("checkpoint");
        for id in 1..4 {
            assert!(position(&format!("resource drop {id}")) < checkpoint);
        }
        assert!(checkpoint < position("resource drop 0"));
    }
    reference.is_ok()
}

#[test]
fn generated_pending_fault_retires_nested_borrowers_before_owning_contingency() {
    let root = std::env::temp_dir().join(format!("tellur-pending-fault-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let p = crate::compile(
        include_str!("../../tests/fixtures/native_pending_fault.tellur"),
        fault_manifest(&root),
    )
    .unwrap()
    .with_entry_exit_codes("main", BTreeMap::from([("done".into(), 0)]))
    .unwrap();
    let mut expanded = p.clone();
    for region in p.regions.keys() {
        expanded = expanded.expanded(region).unwrap();
    }
    for (expanded, p) in [(false, p), (true, expanded)] {
        for opt in [0, 2] {
            let binary = root.join(format!("fault-{expanded}-{opt}"));
            let project = p.native_project("main", &binary).unwrap();
            fs::write(&project.2, fault_source(&p)).unwrap();
            p.finish_native_build(project, opt).unwrap();
            for mode in 0..3 {
                for quotas in ["1", "0,2,0,7", "3,1,9"] {
                    let mut finished = false;
                    for fuel in 0..64 {
                        if compare_fault(&p, &binary, &root, fuel, mode, quotas, false) {
                            finished = true;
                            break;
                        }
                    }
                    assert!(
                        finished,
                        "must sweep all settlement and contingency fuel cuts"
                    );
                    assert!(compare_fault(&p, &binary, &root, 64, mode, quotas, true));
                }
            }
        }
    }
    fs::remove_dir_all(root).unwrap();
}
