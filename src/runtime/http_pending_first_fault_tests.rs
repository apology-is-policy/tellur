//! Repeated-source faults before the first operation has produced a result.
use super::*;

fn program() -> CheckedProgram {
    let original: serde_json::Value =
        serde_json::from_str(include_str!("../../examples/http_hello/manifest.json")).unwrap();
    let mut receive = original["primitives"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == "http_receive")
        .unwrap()
        .clone();
    receive.as_object_mut().unwrap().remove("rust");
    let mut manifest = serde_json::json!({"version":"0.1","types":[{"name":"http_connection","kind":"opaque","linear":true,"fault":"release"}],"primitives":[receive,{"name":"close_test","parameters":[{"name":"connection","type":"http_connection","mode":"consume"}],"outcomes":[{"name":"done","fields":[]}],"effects":["close"],"returns":true,"contingent":true}]});
    manifest["rust"] = serde_json::json!({"source":concat!(env!("CARGO_MANIFEST_DIR"),"/library/host/loopback_http.rs"),"lockfile":concat!(env!("CARGO_MANIFEST_DIR"),"/examples/http_hello/Cargo.lock"),"resources":{"http_connection":"Connection"},"dependencies":original["rust"]["dependencies"]});
    let primitives = manifest["primitives"].as_array_mut().unwrap();
    primitives.push(serde_json::json!({"name":"open_case","parameters":[{"name":"name","type":"string","mode":"consume"}],"outcomes":[{"name":"opened","fields":[{"name":"connection","type":"http_connection"}]}],"effects":["open"],"returns":true}));
    primitives.push(serde_json::json!({"name":"checkpoint","parameters":[],"outcomes":[{"name":"done","fields":[]}],"effects":["checkpoint"],"returns":true,"contingent":true}));
    crate::compile(
        include_str!("../../tests/fixtures/native_source_repeated_http.tellur"),
        Manifest::from_json(&manifest.to_string()).unwrap(),
    )
    .unwrap()
    .with_entry_exit_codes("main", BTreeMap::from([("done".into(), 0)]))
    .unwrap()
}
fn run(
    p: &CheckedProgram,
    label: &str,
    fuel: usize,
    quotas: &[usize],
    reverse: bool,
    early: bool,
) -> RunResult {
    let mut calls = (0..3)
        .map(|id| ScriptCall {
            operation: "open_case".into(),
            outcome: "opened".into(),
            fields: vec![Value::Opaque {
                ty: "http_connection".into(),
                token: format!("rust:{id}"),
            }],
            waits: 0,
        })
        .collect::<Vec<_>>();
    for operation in ["checkpoint", "close_test"] {
        calls.push(ScriptCall {
            operation: operation.into(),
            outcome: "done".into(),
            fields: vec![],
            waits: 0,
        });
    }
    let scenario = Scenario {
        entry: "main".into(),
        arguments: vec![Value::Bool(true)],
        calls,
    };
    let plans = BTreeMap::from([
        (
            "left".into(),
            vec![
                response(p, label, "left", 2),
                Response {
                    waits: 0,
                    outcome: "failed".into(),
                    fields: vec![Value::String("request reception already started".into())],
                },
            ],
        ),
        ("right".into(), vec![response(p, "request", "right", 4)]),
        ("other".into(), vec![response(p, "request", "other", 7)]),
    ]);
    let names = ["other", "left", "right"]
        .iter()
        .enumerate()
        .map(|(id, name)| (format!("rust:{id}"), name.to_string()))
        .collect();
    http_source_operation_sequence_model(p, &scenario, fuel, quotas, reverse, early, plans, names)
}
#[test]
fn repeated_http_pending_first_fault_preserves_counts_fuel_and_quota_turns() {
    let p = program();
    let mut expanded = p.clone();
    for name in p.regions.keys() {
        expanded = expanded.expanded(name).unwrap();
    }
    let mut comparisons = 0;
    for p in [&p, &expanded] {
        for label in ["request", "disconnected"] {
            for reverse in [false, true] {
                for early in [false, true] {
                    let normal = observation(run(p, label, 1000, &[1], reverse, early));
                    assert!(normal.get("outcome").is_some());
                    // Only independent work completes HTTP; neither the first
                    // stopped operation nor its never-started successor emits.
                    let received = normal["events"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .filter(|e| e["kind"] == "primitive" && e["operation"] == "http_receive")
                        .count();
                    assert_eq!(received, 1);
                    let mut finished = false;
                    for fuel in 0..250 {
                        let expected = observation(run(p, label, fuel, &[1], reverse, early));
                        for quotas in [&[0, 2, 0, 7][..], &[3, 1, 9][..]] {
                            assert_eq!(
                                observation(run(p, label, fuel, quotas, reverse, early)),
                                expected
                            );
                            comparisons += 1;
                        }
                        if expected.get("outcome").is_some() {
                            assert_eq!(expected, normal);
                            finished = true;
                            break;
                        }
                    }
                    assert!(finished);
                }
            }
        }
    }
    eprintln!("pending-first HTTP reference comparisons: {comparisons}");
}
