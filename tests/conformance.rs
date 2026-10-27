use tellur::{
    compile,
    ir::{Action, Operation, Value},
    manifest::Manifest,
    runtime::{Event, Scenario, ScriptCall},
};
const LOAD: &str = include_str!("../examples/load.tellur");
const PORT: &str = include_str!("../examples/port.tellur");
const REFRESH: &str = include_str!("../examples/refresh.tellur");
fn manifest() -> Manifest {
    Manifest::from_json(include_str!("../examples/manifest.json")).unwrap()
}
fn checked(source: &str) -> tellur::CheckedProgram {
    compile(source, manifest()).unwrap()
}
fn reject(source: &str, code: &str) -> String {
    let e = compile(source, manifest()).unwrap_err();
    assert_eq!(e.code, code, "{e}");
    e.message
}
fn opaque(ty: &str, token: &str) -> Value {
    Value::Opaque {
        ty: ty.into(),
        token: token.into(),
    }
}
fn call(operation: &str, outcome: &str, fields: Vec<Value>) -> ScriptCall {
    ScriptCall {
        operation: operation.into(),
        outcome: outcome.into(),
        fields,
    }
}
fn loader_calls(open: &str, read_bad: bool, close_bad: bool, token: &str) -> Vec<ScriptCall> {
    if open != "Opened" {
        return vec![call("open", open, vec![])];
    }
    vec![
        call("open", "Opened", vec![opaque("Handle", token)]),
        if read_bad {
            call("read", "Failed", vec![opaque("ReadError", token)])
        } else {
            call("read", "Data", vec![Value::Bytes(token.as_bytes().into())])
        },
        if close_bad {
            call("close", "Failed", vec![opaque("CloseError", token)])
        } else {
            call("close", "Closed", vec![])
        },
    ]
}
fn scenario(entry: &str, calls: Vec<ScriptCall>) -> Scenario {
    Scenario {
        entry: entry.into(),
        arguments: vec![Value::String("config.txt".into())],
        calls,
    }
}

#[test]
fn c01_port_cases() {
    let program = checked(PORT);
    let cases = [
        (None, None, "Ready", Value::U32(8080)),
        (
            Some("443"),
            Some(("Number", vec![Value::U32(443)])),
            "Ready",
            Value::U32(443),
        ),
        (
            Some("0"),
            Some(("Number", vec![Value::U32(0)])),
            "Invalid",
            Value::String("Port must be between 1 and 65535".into()),
        ),
        (
            Some("65536"),
            Some(("Number", vec![Value::U32(65536)])),
            "Invalid",
            Value::String("Port must be between 1 and 65535".into()),
        ),
        (
            Some("abc"),
            Some(("Malformed", vec![])),
            "Invalid",
            Value::String("Expected a decimal integer".into()),
        ),
        (
            Some("4294967296"),
            Some(("Overflow", vec![])),
            "Invalid",
            Value::String("Port must be between 1 and 65535".into()),
        ),
    ];
    for (input, parsed, outcome, payload) in cases {
        let argument = Value::Construct {
            ty: "Option<String>".into(),
            name: if input.is_some() { "Some" } else { "None" }.into(),
            fields: input
                .map(|s| vec![Value::String(s.into())])
                .unwrap_or_default(),
        };
        let calls = parsed
            .map(|(o, f)| vec![call("parse_u32", o, f)])
            .unwrap_or_default();
        let result = program
            .run(&Scenario {
                entry: "port".into(),
                arguments: vec![argument],
                calls,
            })
            .unwrap();
        assert_eq!(result.outcome, outcome);
        assert_eq!(result.fields, vec![payload]);
    }
}
#[test]
fn c02_missing_arm() {
    let message = reject(
        &PORT
            .replace(",\n            Overflow  => TooLarge", "")
            .replace(",\n            Overflow => TooLarge", ""),
        "E_ARMS",
    );
    assert!(message.contains("Overflow"));
}
#[test]
fn c03_missing_disposition() {
    reject(
        &PORT.replace(
            "    [TooLarge | OutOfRange]: exit Invalid(\"Port must be between 1 and 65535\");",
            "    [OutOfRange]: exit Invalid(\"Port must be between 1 and 65535\");",
        ),
        "E_UNCOVERED",
    );
}
#[test]
fn c04_unavailable_payload() {
    reject(
        &PORT.replace("[Default]: port := 8080u32;", "[Default]: port := n;"),
        "E_NAME",
    );
    let source = PORT.replace(
        "[InRange]: port := n;",
        "[Default]: emit(n);\n    [InRange]: port := n;",
    );
    reject(&source, "E_UNAVAILABLE");
}
#[test]
fn c05_overlapping_definitions() {
    reject(
        &PORT.replace(
            "[InRange]: port := n;",
            "[InRange]: port := n; port := 443u32;",
        ),
        "E_REDEFINE",
    );
}
#[test]
fn c06_future_path() {
    reject(
        &LOAD.replace(
            "[Open]:\n        Δ read",
            "[CloseBad]: emit(\"future\");\n    [Open]:\n        Δ read",
        ),
        "E_PATH",
    );
}
#[test]
fn c07_all_loader_outcomes() {
    let program = checked(LOAD);
    for open in ["NotFound", "Denied", "Opened"] {
        for read_bad in [false, true] {
            for close_bad in [false, true] {
                let result = program
                    .run(&scenario(
                        "load",
                        loader_calls(open, read_bad, close_bad, "h1"),
                    ))
                    .unwrap();
                let emits = result
                    .events
                    .iter()
                    .filter(|e| matches!(e, Event::Emit { .. }))
                    .count();
                assert_eq!(
                    emits,
                    usize::from(open == "Opened" && (read_bad || close_bad))
                );
                assert_eq!(
                    result.outcome,
                    if open == "NotFound" {
                        "Missing"
                    } else if open == "Denied" || read_bad || close_bad {
                        "Failed"
                    } else {
                        "Loaded"
                    }
                );
                if open == "Opened" && read_bad && close_bad {
                    assert_eq!(
                        result.fields,
                        vec![Value::Construct {
                            ty: "LoadError".into(),
                            name: "Both".into(),
                            fields: vec![opaque("ReadError", "h1"), opaque("CloseError", "h1")]
                        }]
                    );
                }
            }
        }
    }
}
#[test]
fn c08_double_failure_witness() {
    let message = reject(
        &LOAD.replace("    [ReadBad & CloseBad]: exit Failed(Both(re, ce));", ""),
        "E_UNCOVERED",
    );
    assert!(
        message.contains("ReadBad")
            && message.contains("CloseBad")
            && message.contains("may reach")
    );
}
#[test]
fn c09_resource_leak() {
    let source = "region leak(h: Handle) exits Done { [Entry]: exit Done; }";
    reject(source, "E_RESOURCE");
}
#[test]
fn c10_use_after_consume() {
    let source=LOAD.replace("[ReadBad | CloseBad]: emit", "[Open]: delta read(handle) -> [Data(x) => ExtraData, Failed(xe) => ExtraError];\n    [ReadBad | CloseBad]: emit");
    reject(&source, "E_MOVED");
}
#[test]
fn c11_terminal_overlap() {
    reject(
        "region bad() exits Done { [Entry]: exit Done; emit(\"late\"); }",
        "E_TERMINATED",
    );
}
#[test]
fn c12_nested_equivalence() {
    let nested=LOAD.replace("[ReadOK & CloseOK]: exit Loaded(bytes);\n    [ReadBad & CloseOK]: exit Failed(ReadOnly(re));\n    [ReadOK & CloseBad]: exit Failed(CloseOnly(ce));\n    [ReadBad & CloseBad]: exit Failed(Both(re, ce));", "[ReadOK] over &: {\n [CloseOK]: exit Loaded(bytes);\n [CloseBad]: exit Failed(CloseOnly(ce));\n }\n [ReadBad] over &: {\n [CloseOK]: exit Failed(ReadOnly(re));\n [CloseBad]: exit Failed(Both(re, ce));\n }");
    assert_ne!(nested, LOAD);
    let a = checked(LOAD);
    let b = checked(&nested);
    for r in [false, true] {
        for c in [false, true] {
            let s = scenario("load", loader_calls("Opened", r, c, "h"));
            assert_eq!(a.run(&s).unwrap(), b.run(&s).unwrap());
        }
    }
}
const TWO: &str = "\nregion two(path: String) exits Done {\n [Entry]: first = delta load(path) -> [Loaded(a) => A, Missing => B, Failed(e) => C];\n [Entry]: second = delta load(\"second.txt\") -> [Loaded(b) => D, Missing => E, Failed(f) => F];\n [A & D]: emit(\"both loaded\");\n [C & F]: emit(\"both failed\");\n [Entry]: exit Done;\n}";
#[test]
fn c13_independent_calls_and_generated_expansion_matrix() {
    let program = checked(&format!("{LOAD}{TWO}"));
    let expanded = program.expanded("two").unwrap();
    assert!(
        !expanded
            .region("two")
            .unwrap()
            .code
            .iter()
            .any(|i| matches!(
                &i.action,
                Action::Decide {
                    operation: Operation::Region(_),
                    ..
                }
            ))
    );
    let cases = [
        ("NotFound", false, false),
        ("Denied", false, false),
        ("Opened", false, false),
        ("Opened", false, true),
        ("Opened", true, false),
        ("Opened", true, true),
    ];
    for a in cases {
        for b in cases {
            let mut calls = loader_calls(a.0, a.1, a.2, "first");
            calls.extend(loader_calls(b.0, b.1, b.2, "second"));
            let s = scenario("two", calls);
            assert_eq!(program.run(&s).unwrap(), expanded.run(&s).unwrap());
            let opens: Vec<_> = expanded
                .run(&s)
                .unwrap()
                .events
                .into_iter()
                .filter_map(|e| {
                    if let Event::Primitive {
                        operation,
                        arguments,
                        ..
                    } = e
                    {
                        (operation == "open").then_some(arguments)
                    } else {
                        None
                    }
                })
                .collect();
            assert_eq!(
                opens,
                vec![
                    vec![Value::String("config.txt".into())],
                    vec![Value::String("second.txt".into())]
                ]
            );
        }
    }
    let origins = expanded
        .region("two")
        .unwrap()
        .code
        .iter()
        .filter(|i| !i.provenance.calls.is_empty())
        .map(|i| i.provenance.calls.clone())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(origins.len(), 2);
}
#[test]
fn c14_callee_exit_resumes_caller() {
    let program = checked(REFRESH);
    let s = scenario("refresh", loader_calls("Opened", true, true, "h"));
    let original = program.run(&s).unwrap();
    assert_eq!(
        original,
        program.expanded("refresh").unwrap().run(&s).unwrap()
    );
    assert_eq!(original.outcome, "Done");
    assert!(original.events.contains(&Event::Emit {
        message: "Keep current configuration".into()
    }));
}
#[test]
fn c15_public_exit_breaks_caller() {
    reject(
        &REFRESH.replace(
            "Missing, Failed(error: LoadError)",
            "Missing, NewFailure, Failed(error: LoadError)",
        ),
        "E_ARMS",
    );
}
#[test]
fn c16_internal_failure_keeps_caller_valid() {
    let mut manifest = manifest();
    manifest
        .primitives
        .iter_mut()
        .find(|p| p.name == "open")
        .unwrap()
        .outcomes
        .push(tellur::manifest::Outcome {
            name: "Offline".into(),
            fields: vec![],
        });
    let source = REFRESH
        .replace(
            "Denied => CannotOpen",
            "Denied => CannotOpen, Offline => Offline",
        )
        .replace("[CannotOpen]: exit", "[CannotOpen | Offline]: exit");
    let program = compile(&source, manifest).unwrap();
    let s = scenario("refresh", vec![call("open", "Offline", vec![])]);
    assert_eq!(
        program.run(&s).unwrap(),
        program.expanded("refresh").unwrap().run(&s).unwrap()
    );
}
#[test]
fn retry_fixture_compiles() {
    checked(include_str!("../examples/fetch_retry.tellur"));
}

#[test]
fn c21_generated_acyclic_call_chains() {
    for depth in 1..9 {
        let mut source="region r0(flag: Bool) exits A, B { [Entry]: delta flag -> [true => A, false => B]; [A]: exit A; [B]: exit B; }".to_string();
        for i in 1..=depth {
            source.push_str(&format!("\nregion r{i}(flag: Bool) exits A, B {{ [Entry]: delta r{}(flag) -> [A => A, B => B]; [A]: emit(\"a{i}\"); exit A; [B]: emit(\"b{i}\"); exit B; }}",i-1));
        }
        let program = checked(&source);
        let name = format!("r{depth}");
        let expanded = program.expanded(&name).unwrap();
        for flag in [false, true] {
            let s = Scenario {
                entry: name.clone(),
                arguments: vec![Value::Bool(flag)],
                calls: vec![],
            };
            assert_eq!(program.run(&s).unwrap(), expanded.run(&s).unwrap());
        }
    }
}
#[test]
fn c22_union_runs_once() {
    let program = checked(
        "region test(x: Bool) exits Done { [Entry]: delta x -> [true => A, false => B]; [Entry]: delta x -> [true => C, false => D]; [A | C]: emit(\"once\"); [Entry]: exit Done; }",
    );
    let s = Scenario {
        entry: "test".into(),
        arguments: vec![Value::Bool(true)],
        calls: vec![],
    };
    assert_eq!(program.run(&s).unwrap().events.len(), 1);
}
#[test]
fn c23_impossible_selector() {
    reject(
        "region bad(x: Bool) exits Done { [Entry]: delta x -> [true => A, false => B]; [A & B]: emit(\"impossible\"); [Entry]: exit Done; }",
        "E_EMPTY",
    );
}
#[test]
fn c24_skipped_producer_is_false() {
    let program = checked(
        "region test(x: Bool) exits Done { [Entry]: delta x -> [true => A, false => B]; [A]: delta x -> [true => C, false => D]; [B | C]: emit(\"selected once\"); [Entry]: exit Done; }",
    );
    for b in [false, true] {
        let s = Scenario {
            entry: "test".into(),
            arguments: vec![Value::Bool(b)],
            calls: vec![],
        };
        assert_eq!(program.run(&s).unwrap().events.len(), 1);
    }
}
#[test]
fn later_producer_is_false_on_terminated_histories() {
    checked(
        "region test(x: Bool) exits Done { [Entry]: delta x -> [true => A, false => B]; [A]: exit Done; [B]: delta x -> [true => C, false => D]; [C | D]: exit Done; }",
    );
}
#[test]
fn nested_alias_keeps_context() {
    checked(
        "region test(x: Bool) exits Done { [Entry]: delta x -> [true => A, false => B]; [A] over &: { path Local = Entry; [Local]: exit Done; } [B]: exit Done; }",
    );
}
#[test]
fn nested_values_and_paths_do_not_escape() {
    reject(
        "region test() exits Done { [Entry] over &: { [Entry]: x := 1; } [Entry]: emit(x); exit Done; }",
        "E_NAME",
    );
    reject(
        "region test() exits Done { [Entry] over &: { path Local = Entry; } [Local]: exit Done; }",
        "E_PATH",
    );
}
#[test]
fn path_dependent_moves() {
    checked(
        "region move(h: Handle, flag: Bool) exits Done { [Entry]: delta flag -> [true => A, false => B]; [A]: other := h; delta close(other) -> [Closed => AC, Failed(e) => AF]; [B]: delta close(h) -> [Closed => BC, Failed(e) => BF]; [Entry]: exit Done; }",
    );
}
#[test]
fn resource_transfer_through_regions_and_expansion() {
    let source = "region identity(h: Handle) exits Owned(h: Handle) { [Entry]: exit Owned(h); }\nregion test(h: Handle) exits Done { [Entry]: delta identity(h) -> [Owned(next) => Ready]; [Ready]: delta close(next) -> [Closed => OK, Failed(e) => Bad]; [Entry]: exit Done; }";
    let program = checked(source);
    let s = Scenario {
        entry: "test".into(),
        arguments: vec![opaque("Handle", "h")],
        calls: vec![call("close", "Closed", vec![])],
    };
    assert_eq!(
        program.run(&s).unwrap(),
        program.expanded("test").unwrap().run(&s).unwrap()
    );
}
#[test]
fn consume_and_borrow_same_invocation_rejected_in_either_order() {
    for modes in [["borrow", "consume"], ["consume", "borrow"]] {
        let json = serde_json::json!({"version":"0.1","types":[{"name":"Handle","kind":"opaque","linear":true}],"primitives":[{"name":"bad","parameters":[{"name":"a","type":"Handle","mode":modes[0]},{"name":"b","type":"Handle","mode":modes[1]}],"outcomes":[{"name":"Done","fields":[]}],"effects":[],"returns":true}]});
        let manifest = Manifest::from_json(&json.to_string()).unwrap();
        let error=compile("region test(h: Handle) exits Done { [Entry]: delta bad(h,h) -> [Done => D]; [D]: exit Done; }",manifest).unwrap_err();
        assert_eq!(error.code, "E_ALIAS");
    }
}
#[test]
fn nested_scope_cannot_leak_resource() {
    reject(
        "region test() exits Done { [Entry] over &: { [Entry]: delta open(\"x\") -> [Opened(h) => A, NotFound => B, Denied => C]; } [Entry]: exit Done; }",
        "E_RESOURCE",
    );
}
#[test]
fn recursion_rejected() {
    reject(
        "region test() exits Done { [Entry]: delta test() -> [Done => D]; [D]: exit Done; }",
        "E_RECURSION",
    );
}
#[test]
fn literal_and_expression_errors() {
    reject(
        "region test() exits Done { [Entry]: x := 4294967296; exit Done; }",
        "E_LITERAL",
    );
    reject(
        "region test() exits Done { [Entry]: x := 1 < 2 < 3; exit Done; }",
        "E_PARSE",
    );
    reject(
        "region test() exits Done { [Entry]: emit(1); exit Done; }",
        "E_TYPE",
    );
    reject(
        "region test() exits Done { [Entry]: x := bytes\"0g\"; exit Done; }",
        "E_LITERAL",
    );
    reject(
        "region test() exits Done { [Entry]: x := open(\"x\"); exit Done; }",
        "E_EFFECT",
    );
}
#[test]
fn invalid_driver_payload_rejected() {
    let program = checked(LOAD);
    let s = scenario("load", vec![call("open", "Opened", vec![Value::U32(1)])]);
    assert_eq!(program.run(&s).unwrap_err().code, "E_DRIVER");
}
#[test]
fn invalid_manifest_rejected() {
    for json in [
        r#"{"version":"0.1","types":[{"name":"R","kind":"record","fields":[{"name":"r","type":"R"}]}],"primitives":[]}"#,
        r#"{"version":"0.1","types":[{"name":"Handle","kind":"opaque","linear":true},{"name":"R","kind":"variant","constructors":[{"name":"Owned","fields":[{"name":"h","type":"Unknown"}]}]}],"primitives":[]}"#,
    ] {
        assert_eq!(Manifest::from_json(json).unwrap_err().code, "E_MANIFEST");
    }
}
#[test]
fn bdd_matches_boolean_truth_tables() {
    use std::collections::HashMap;
    use tellur::bdd::*;
    let mut b = Bdd::default();
    let a = b.variable(1);
    let c = b.variable(2);
    let either = b.or(a, c);
    let both = b.and(a, c);
    let xor = b.minus(either, both);
    for av in [false, true] {
        for cv in [false, true] {
            let assignment = HashMap::from([(1, av), (2, cv)]);
            assert_eq!(b.evaluate(xor, &assignment), av ^ cv);
        }
    }
    let witness = b.witness(xor).unwrap();
    assert!(b.evaluate(xor, &witness));
    assert!(b.witness(FALSE).is_none());
}
#[test]
fn many_independent_decisions_stay_factored() {
    let mut source = "region test(x: Bool) exits Done {".to_string();
    for i in 0..100 {
        source.push_str(&format!(
            "[Entry]: delta x -> [true => T{i}, false => F{i}];"
        ));
    }
    source.push_str("[Entry]: exit Done; }");
    checked(&source);
}
