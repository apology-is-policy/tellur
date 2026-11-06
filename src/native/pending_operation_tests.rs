//! Generated checked-source experiments; all adapter admission remains private.
use super::*;
use crate::{
    manifest::Manifest,
    runtime::{Scenario, ScriptCall},
};
use serde_json::json;

const HOST: &str = r#"
use crate::rust_api;
use std::{cell::{Cell,RefCell}, rc::Rc};
thread_local! { static TRACE: Rc<RefCell<Vec<String>>> = Default::default(); }
pub struct Probe { pub alive: Rc<Cell<bool>>, pub trace: Rc<RefCell<Vec<String>>> }
impl Drop for Probe {
    fn drop(&mut self) {
        assert!(self.alive.replace(false));
        self.trace.borrow_mut().push("resource drop".into());
    }
}
pub struct TraceGuard;
impl Drop for TraceGuard {
    fn drop(&mut self) {
        if let Ok(path) = std::env::var("TELLUR_OPERATION_LOG") {
            TRACE.with(|t| std::fs::write(path, t.borrow().join("\n")).unwrap());
        }
    }
}
pub fn make() -> rust_api::make_outcome {
    let trace=TRACE.with(Clone::clone);
    trace.borrow_mut().push("make".into());
    rust_api::make_outcome::made(Probe {alive:Rc::new(Cell::new(true)),trace})
}
pub fn sample(value: &Probe, request: String) -> rust_api::sample_outcome {
    assert!(value.alive.get());
    value.trace.borrow_mut().push(format!("sample {request}"));
    rust_api::sample_outcome::ready(format!("response to {request}"))
}
pub fn inspect(value: &Probe) -> rust_api::inspect_outcome {
    assert!(value.alive.get()); value.trace.borrow_mut().push("inspect".into());
    rust_api::inspect_outcome::done
}
pub fn close(value: Probe) -> rust_api::close_outcome {
    value.trace.borrow_mut().push("close".into()); drop(value);
    rust_api::close_outcome::done
}
"#;
fn manifest(root: &Path) -> Manifest {
    fs::write(root.join("host.rs"), HOST).unwrap();
    fs::write(
        root.join("Cargo.lock"),
        include_str!("../../examples/retained_file/Cargo.lock"),
    )
    .unwrap();
    Manifest::from_json_at(&json!({
        "version":"0.1", "types":[{"name":"probe","kind":"opaque","linear":true,"fault":"release"}],
        "rust":{"source":"host.rs","lockfile":"Cargo.lock","resources":{"probe":"Probe"}},
        "primitives":[
            {"name":"make","parameters":[],"outcomes":[{"name":"made","fields":[{"name":"value","type":"probe"}]}],"effects":["make"],"returns":true,"rust":{"execution":"immediate"}},
            {"name":"sample","parameters":[{"name":"value","type":"probe","mode":"borrow"},{"name":"request","type":"string","mode":"consume"}],"outcomes":[{"name":"ready","fields":[{"name":"text","type":"string"}]}],"effects":["sample"],"returns":true,"rust":{"execution":"immediate"}},
            {"name":"inspect","parameters":[{"name":"value","type":"probe","mode":"borrow"}],"outcomes":[{"name":"done","fields":[]}],"effects":["inspect"],"returns":true,"rust":{"execution":"immediate"}},
            {"name":"close","parameters":[{"name":"value","type":"probe","mode":"consume"}],"outcomes":[{"name":"done","fields":[]}],"effects":["close"],"returns":true,"contingent":true,"rust":{"execution":"immediate"}}
        ]
    }).to_string(), &root.join("manifest.json")).unwrap()
}
fn source(p: &CheckedProgram, pending: bool) -> String {
    let mut code = p.native_source("main").unwrap();
    code = code.replace(
        "fn main() -> std::process::ExitCode {",
        "fn main() -> std::process::ExitCode {let _trace_guard=rust_host::TraceGuard;",
    );
    if !pending {
        return code;
    }
    let callbacks = continuations::source(p);
    assert_eq!(code.matches(&callbacks).count(), 1);
    code = code.replacen(&callbacks, &continuations::pending_operation_source(p), 1);
    // Enable only the seven pending-operation model gates, not unit-test modules.
    let mut lines = EXECUTION.lines().peekable();
    let mut execution = String::new();
    let mut gates = 0;
    while let Some(line) = lines.next() {
        if line.trim() == "#[cfg(test)]"
            && lines.peek().is_some_and(|next| {
                let next = next.trim();
                next.starts_with("Adapter")
                    || next.starts_with("Step::Adapter")
                    || next.starts_with("Pending::Adapter")
                    || next.starts_with("if let Some(Pending::Adapter")
                    || next.starts_with("if matches!(&self.stack.last().unwrap().pending,")
            })
        {
            gates += 1;
        } else {
            execution.push_str(line);
            execution.push('\n');
        }
    }
    assert_eq!(gates, 7);
    assert_eq!(code.matches(EXECUTION).count(), 1);
    code = code.replacen(EXECUTION, &execution, 1);
    let model = include_str!("adapter_operation_model.rs");
    let mut model = model[..model.find("fn returning(").unwrap()].to_string();
    let start = model.find("struct Probe {").unwrap();
    let end = model.find("struct Lease {").unwrap();
    model.replace_range(start..end, "type Probe = crate::rust_host::Probe;\n");
    model.push_str(include_str!("pending_operation_driver.rs"));
    let declaration = "#[cfg(test)]\n#[path = \"adapter_operation_model.rs\"]\npub(super) mod adapter_operation_model;";
    assert_eq!(code.matches(declaration).count(), 1);
    code = code.replacen(declaration, &format!("pub(super) mod adapter_operation_model {{\n{model}\n}}\npub use adapter_operation_model::drive_pending;"), 1);
    code = code.replace(
        "pub use contexts::{AdmissionProgress, AdmissionStop};",
        "pub use contexts::{AdmissionProgress, AdmissionStop, drive_pending};",
    );
    code.replace(
        "run_entry(host,args,&MODEL_",
        "drive_pending(host,args,&MODEL_",
    )
}
fn scenario(stop: bool, nested: bool) -> Scenario {
    let call = |operation: &str, outcome: &str, fields: Vec<Value>| ScriptCall {
        operation: operation.into(),
        outcome: outcome.into(),
        fields,
        waits: 0,
    };
    let mut calls = vec![call(
        "make",
        "made",
        vec![Value::Opaque {
            ty: "probe".into(),
            token: "rust:0".into(),
        }],
    )];
    if nested {
        calls.push(call(
            "make",
            "made",
            vec![Value::Opaque {
                ty: "probe".into(),
                token: "rust:1".into(),
            }],
        ));
    }
    calls.push(call(
        "sample",
        "ready",
        vec![Value::String("response to request".into())],
    ));
    calls.push(call("inspect", "done", vec![]));
    if nested && !stop {
        calls.push(call("close", "done", vec![]));
    }
    calls.push(call("close", "done", vec![]));
    Scenario {
        entry: "main".into(),
        arguments: vec![Value::Bool(stop)],
        calls,
    }
}
fn normalize_fuel(actual: &mut crate::fault::Report, expected: &crate::fault::Report) {
    if expected.message.starts_with("E_FUEL") {
        assert!(actual.message.starts_with("E_FUEL"));
        actual.message = expected.message.clone();
    }
    if let (Some(a), Some(e)) = (&mut actual.cause, &expected.cause) {
        normalize_fuel(a, e);
    }
}
fn compare(label: &str, text: &str, nested: bool) {
    let root = std::env::temp_dir().join(format!(
        "tellur-pending-generated-{label}-{}",
        std::process::id()
    ));
    fs::create_dir_all(&root).unwrap();
    let p = crate::compile(text, manifest(&root))
        .unwrap()
        .with_entry_exit_codes("main", BTreeMap::from([("done".into(), 0)]))
        .unwrap();
    let expanded = p.expanded("main").unwrap();
    for (expanded, p) in [(false, p), (true, expanded)] {
        for opt in [0, 2] {
            let normal = root.join(format!("normal-{expanded}-{opt}"));
            let pending = root.join(format!("pending-{expanded}-{opt}"));
            for (binary, model) in [(&normal, false), (&pending, true)] {
                let project = p.native_project("main", binary).unwrap();
                fs::write(&project.2, source(&p, model)).unwrap();
                p.finish_native_build(project, opt).unwrap();
            }
            for stop in [false, true] {
                let scenario = scenario(stop, nested);
                let mut finished = false;
                for fuel in 0..150 {
                    let run = |binary: &Path, quotas: &str| {
                        let log = root.join("trace.log");
                        let output = Command::new(binary)
                            .args([
                                "--json",
                                "--fuel",
                                &fuel.to_string(),
                                "--",
                                &stop.to_string(),
                            ])
                            .env("TELLUR_OPERATION_LOG", &log)
                            .env("TELLUR_NATIVE_MODEL_QUOTAS", quotas)
                            .output()
                            .unwrap();
                        let trace = fs::read_to_string(&log).unwrap_or_default();
                        (output, trace)
                    };
                    let (expected, trace) = run(&normal, "1");
                    let reference = p.run_completion(&scenario, fuel);
                    match &reference {
                        Ok(result) => assert_eq!(
                            serde_json::from_slice::<serde_json::Value>(&expected.stdout).unwrap(),
                            serde_json::to_value(result).unwrap()
                        ),
                        Err(failure) => {
                            assert_eq!(expected.status.code(), Some(1));
                            let mut report: crate::fault::Report =
                                serde_json::from_slice(&expected.stderr).unwrap();
                            normalize_fuel(&mut report, &failure.report);
                            assert_eq!(report, failure.report);
                        }
                    }
                    for quotas in ["1", "0,2,0,7", "3,1,9"] {
                        let (actual, operations) = run(&pending, quotas);
                        let context = format!(
                            "{label}, expanded={expanded}, opt={opt}, stop={stop}, fuel={fuel}, quotas={quotas}"
                        );
                        assert_eq!(
                            actual.status.code(),
                            expected.status.code(),
                            "{context}: {}",
                            String::from_utf8_lossy(&actual.stderr)
                        );
                        assert_eq!(actual.stdout, expected.stdout, "{context}");
                        assert_eq!(actual.stderr, expected.stderr, "{context}");
                        let resource_trace = operations
                            .lines()
                            .filter(|s| {
                                !["start ", "poll ", "deliver ", "drop "]
                                    .iter()
                                    .any(|p| s.starts_with(p))
                            })
                            .collect::<Vec<_>>()
                            .join("\n");
                        assert_eq!(resource_trace, trace, "{context}");
                        let count = |prefix: &str| {
                            operations.lines().filter(|s| s.starts_with(prefix)).count()
                        };
                        let started = count("start ");
                        assert!(started <= 1, "{context}");
                        assert_eq!(count("sample "), started, "{context}");
                        assert_eq!(count("poll "), started * 3, "{context}");
                        assert_eq!(count("deliver "), started, "{context}");
                        assert_eq!(count("drop "), started, "{context}");
                    }
                    if reference.is_ok() {
                        assert_eq!(expected.status.code(), Some(if stop { 23 } else { 0 }));
                        assert!(trace.contains("sample request"));
                        assert_eq!(
                            trace.lines().filter(|s| *s == "resource drop").count(),
                            if nested { 2 } else { 1 }
                        );
                        finished = true;
                        break;
                    }
                }
                assert!(finished, "must reach completion through all fuel cuts");
            }
        }
    }
    fs::remove_dir_all(root).unwrap();
}
#[test]
fn generated_pending_delivery_preserves_real_borrowed_results_and_all_fuel_cuts() {
    compare(
        "borrowed",
        include_str!("../../tests/fixtures/native_pending_operation.tellur"),
        false,
    );
}
#[test]
fn generated_pending_delivery_preserves_nested_supplier_and_fault_reports() {
    let text = include_str!("../../tests/fixtures/native_pending_operation.tellur");
    let helper = r#"
control nested(bool stop) exits done faults {
    fault boundary {
        delta make() {made {probe inner} => made}
        [made]:
            run worker(borrow inner, "request", stop);
            close(inner);
    } contingency [] {emit("nested contingency");escalate;}
    exit done;
}
"#;
    let main = text.find("control main(").unwrap();
    let mut text = format!("{}{helper}{}", &text[..main], &text[main..]);
    let start = text.rfind("run worker(borrow resource").unwrap();
    let end = start + text[start..].find("} contingency [resource]").unwrap();
    text.replace_range(start..end, "nested(stop);\n        ");
    compare("nested", &text, true);
}

#[path = "pending_operation_fault_tests.rs"]
mod fault_tests;

#[path = "source_operation_tests.rs"]
mod source_tests;

#[path = "http_continuation_tests.rs"]
mod http_tests;
