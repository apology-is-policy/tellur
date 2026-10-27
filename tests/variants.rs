use serde_json::json;
use tellur::{
    compile,
    ir::Value,
    manifest::Manifest,
    runtime::{Scenario, ScriptCall},
};

fn manifest() -> Manifest {
    let mut m: serde_json::Value =
        serde_json::from_str(include_str!("../examples/manifest.json")).unwrap();
    m["types"].as_array_mut().unwrap().extend([
        json!({"name":"Parcel","kind":"record","fields":[{"name":"state","type":"FileState"},{"name":"spare","type":"Handle"},{"name":"label","type":"String"}]}),
        json!({"name":"Bundle","kind":"variant","constructors":[{"name":"Empty","fields":[]},{"name":"Pair","fields":[{"name":"pair","type":"HandlePair"}]},{"name":"Maybe","fields":[{"name":"optional","type":"Option<Handle>"}]}]}),
        json!({"name":"OptionalBox","kind":"record","fields":[{"name":"file","type":"Option<Handle>"}]}),
    ]);
    m["primitives"].as_array_mut().unwrap().extend([
        json!({"name":"inspect_state","parameters":[{"name":"state","type":"FileState","mode":"borrow"}],"outcomes":[{"name":"Done","fields":[]}],"effects":[],"returns":true}),
        json!({"name":"borrow_and_wrap","parameters":[{"name":"h","type":"Handle","mode":"borrow"},{"name":"file","type":"Option<Handle>","mode":"consume"}],"outcomes":[{"name":"Done","fields":[]}],"effects":[],"returns":true}),
        json!({"name":"receive","parameters":[],"outcomes":[{"name":"Got","fields":[{"name":"state","type":"FileState"}]}],"effects":[],"returns":true}),
    ]);
    Manifest::from_json(&m.to_string()).unwrap()
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
fn value(ty: &str, name: &str, fields: Vec<Value>) -> Value {
    Value::Construct {
        ty: ty.into(),
        name: name.into(),
        fields,
    }
}
fn scenario(args: Vec<Value>) -> Scenario {
    Scenario {
        entry: "test".into(),
        arguments: args,
        calls: vec![],
    }
}
fn call(name: &str, outcome: &str, fields: Vec<Value>) -> ScriptCall {
    ScriptCall {
        operation: name.into(),
        outcome: outcome.into(),
        fields,
    }
}
fn equivalent(p: &tellur::CheckedProgram, s: &Scenario) -> tellur::runtime::Execution {
    let actual = p.run(s).unwrap();
    assert_eq!(actual, p.expanded(&s.entry).unwrap().run(s).unwrap());
    actual
}

#[test]
fn optional_file_all_open_and_close_outcomes() {
    let p = checked(include_str!("../examples/optional_handle.tellur"));
    for (open, close, expected) in [
        ("NotFound", None, "Done"),
        ("Denied", None, "Denied"),
        ("Opened", Some("Closed"), "Done"),
        ("Opened", Some("Failed"), "Failed"),
    ] {
        let mut calls = vec![call(
            "open",
            open,
            if open == "Opened" {
                vec![opaque("Handle", "file")]
            } else {
                vec![]
            },
        )];
        if let Some(close) = close {
            calls.push(call(
                "close",
                close,
                if close == "Failed" {
                    vec![opaque("CloseError", "error")]
                } else {
                    vec![]
                },
            ));
        }
        let result = equivalent(
            &p,
            &Scenario {
                entry: "optional_file".into(),
                arguments: vec![Value::String("config".into())],
                calls,
            },
        );
        assert_eq!(result.outcome, expected);
        if expected == "Failed" {
            assert_eq!(result.fields, vec![opaque("CloseError", "error")]);
        }
    }
}
#[test]
fn owned_variant_all_open_and_close_outcomes() {
    let p = checked(include_str!("../examples/owned_variant.tellur"));
    for (open, close) in [
        ("NotFound", None),
        ("Denied", None),
        ("Opened", Some("Closed")),
        ("Opened", Some("Failed")),
    ] {
        let mut calls = vec![call(
            "open",
            open,
            if open == "Opened" {
                vec![opaque("Handle", "file")]
            } else {
                vec![]
            },
        )];
        if let Some(close) = close {
            calls.push(call(
                "close",
                close,
                if close == "Failed" {
                    vec![opaque("CloseError", "error")]
                } else {
                    vec![]
                },
            ));
        }
        let result = equivalent(
            &p,
            &Scenario {
                entry: "variant_file".into(),
                arguments: vec![Value::String("config".into())],
                calls,
            },
        );
        assert_eq!(
            result.outcome,
            if close == Some("Failed") {
                "Failed"
            } else {
                "Done"
            }
        );
        assert_eq!(
            result
                .events
                .iter()
                .filter(|e| matches!(e, tellur::runtime::Event::Emit { .. }))
                .count(),
            usize::from(open == "Denied")
        );
    }
}
#[test]
fn decision_consumes_sum_even_on_empty_arm() {
    for source in [
        "region test(file: Option<Handle>) exits Done { [Entry]: delta file -> [None => N,Some(h) => S]; [N]: again_file := file; exit Done; }",
        "region test(file: Option<Handle>) exits Done { [Entry]: delta file -> [None => N,Some(h) => S]; [Entry]: delta file -> [None => N2,Some(h2) => S2]; }",
    ] {
        reject(source, "E_MOVED");
    }
}
#[test]
fn resource_payload_unavailable_on_other_arm() {
    let message = reject(
        "region test(file: Option<Handle>) exits Done { [Entry]: delta file -> [None => N,Some(h) => S]; [N]: duplicate := h; exit Done; }",
        "E_UNAVAILABLE",
    );
    assert!(message.ends_with(": N"), "{message}");
}
#[test]
fn payload_cannot_be_discarded_or_duplicated() {
    let message = reject(
        "region test(file: Option<Handle>) exits Done { [Entry]: delta file -> [None => N,Some(h) => S]; [Entry]: exit Done; }",
        "E_RESOURCE",
    );
    assert!(
        message.contains("`h`") && message.ends_with(": S"),
        "{message}"
    );
    reject(
        "region test(file: Option<Handle>) exits Done { [Entry]: delta file -> [None => N,Some(h) => S]; [S]: a := h; b := h; exit Done; }",
        "E_MOVED",
    );
}
#[test]
fn unexamined_sum_is_an_obligation_even_for_known_empty_constructor() {
    reject(
        "region test() exits Done { [Entry]: state := FileState.Missing; exit Done; }",
        "E_RESOURCE",
    );
    reject(
        "region test() exits Done { [Entry]: box := OptionalBox(None); exit Done; }",
        "E_RESOURCE",
    );
}
#[test]
fn variant_patterns_require_exhaustive_payload_bindings() {
    reject(
        "region test(file: Option<Handle>) exits Done { [Entry]: delta file -> [None => N]; [N]: exit Done; }",
        "E_ARMS",
    );
    reject(
        "region test(file: Option<Handle>) exits Done { [Entry]: delta file -> [None => N,Some => S]; [Entry]: exit Done; }",
        "E_ARITY",
    );
    reject(
        "region test(file: Option<Handle>) exits Done { [Entry]: delta file -> [None => N,Some(h) => S,Some(h2) => S2]; }",
        "E_ARMS",
    );
}
#[test]
fn sum_fields_need_a_decision_not_record_projection() {
    reject(
        "region test(state: FileState) exits Done { [Entry]: h := state.handle; exit Done; }",
        "E_FIELD",
    );
}
#[test]
fn variant_decision_of_record_field_is_a_partial_move() {
    let p = checked(
        "region test(p: Parcel) exits Done(spare: Handle, found: Option<Handle>) { [Entry]: delta p.state -> [Missing => N,Present(h) => S,Blocked(reason) => B]; emit(p.label); [N | B]: exit Done(p.spare,None); [S]: exit Done(p.spare,Some(h)); }",
    );
    for (ctor, payload) in [
        ("Missing", vec![]),
        ("Present", vec![opaque("Handle", "owned")]),
        ("Blocked", vec![Value::String("blocked".into())]),
    ] {
        let s = scenario(vec![value(
            "Parcel",
            "Parcel",
            vec![
                value("FileState", ctor, payload),
                opaque("Handle", "spare"),
                Value::String("label".into()),
            ],
        )]);
        let result = equivalent(&p, &s);
        assert_eq!(result.fields[0], opaque("Handle", "spare"));
        assert_eq!(result.fields[1].ty(), "Option<Handle>");
    }
    reject(
        "region test(p: Parcel) exits Done { [Entry]: delta p.state -> [Missing => N,Present(h) => S,Blocked(reason) => B]; q := p; }",
        "E_PARTIAL_MOVE",
    );
}
#[test]
fn nested_record_payload_exposes_per_field_obligations() {
    let source = "region test(b: Bundle) exits Done { [Entry]: delta b -> [Empty => E,Pair(p) => P,Maybe(file) => M]; [P]: delta close(p.left) -> [Closed => C,Failed(e) => F]; exit Done; }";
    assert!(reject(source, "E_RESOURCE").contains("p.right"));
}
#[test]
fn nested_options_transfer_one_layer_at_a_time() {
    let p = checked(
        "region test(file: Option<Option<Handle>>) exits Done(file: Option<Handle>) { [Entry]: delta file -> [None => N,Some(inner) => S]; [N]: exit Done(None); [S]: delta inner -> [None => N2,Some(h) => S2]; [N2]: exit Done(None); [S2]: exit Done(Some(h)); }",
    );
    for inner in [
        value("Option<Handle>", "None", vec![]),
        value("Option<Handle>", "Some", vec![opaque("Handle", "owned")]),
    ] {
        let actual = equivalent(
            &p,
            &scenario(vec![value(
                "Option<Option<Handle>>",
                "Some",
                vec![inner.clone()],
            )]),
        );
        assert_eq!(actual.fields, vec![inner]);
    }
    equivalent(
        &p,
        &scenario(vec![value("Option<Option<Handle>>", "None", vec![])]),
    );
}
#[test]
fn optional_constructor_infers_some_and_checks_contextual_none() {
    let p = checked(
        "region test(h: Handle) exits Done(file: Option<Handle>, nested: Option<Option<U32>>) { [Entry]: file := Some(h); exit Done(file,Some(None)); }",
    );
    assert_eq!(
        p.run(&scenario(vec![opaque("Handle", "owned")]))
            .unwrap()
            .fields,
        vec![
            value("Option<Handle>", "Some", vec![opaque("Handle", "owned")]),
            value(
                "Option<Option<U32>>",
                "Some",
                vec![value("Option<U32>", "None", vec![])]
            )
        ]
    );
    let p = checked(
        "region test() exits Done(a: Option<U32>,b: Option<U32>) { [Entry]: exit Done(Option.None(),Option.Some(42)); }",
    );
    assert_eq!(
        p.run(&scenario(vec![])).unwrap().fields,
        vec![
            value("Option<U32>", "None", vec![]),
            value("Option<U32>", "Some", vec![Value::U32(42)])
        ]
    );
}
#[test]
fn none_without_type_context_has_actionable_error() {
    assert!(
        reject(
            "region test() exits Done { [Entry]: file := None; exit Done; }",
            "E_INFER"
        )
        .contains("expected Option<T>")
    );
    reject(
        "region test() exits Done { [Entry]: file := Some(None); exit Done; }",
        "E_INFER",
    );
}
#[test]
fn option_constructor_wrong_payload_or_arity_is_rejected() {
    for source in [
        "region test() exits Done(file: Option<U32>) { [Entry]: exit Done(Some(\"text\")); }",
        "region test() exits Done(file: Option<U32>) { [Entry]: exit Done(Some(Some(1))); }",
    ] {
        reject(source, "E_TYPE");
    }
    for expression in ["Some()", "Some(1,2)", "None(1)"] {
        reject(
            &format!(
                "region test() exits Done(file: Option<U32>) {{ [Entry]: exit Done({expression}); }}"
            ),
            "E_ARITY",
        );
    }
}
#[test]
fn constructor_move_invalidates_original_resource() {
    for expression in ["Some(h)", "FileState.Present(h)"] {
        reject(
            &format!(
                "region test(h: Handle) exits Done {{ [Entry]: file := {expression}; duplicate := h; exit Done; }}"
            ),
            "E_MOVED",
        );
    }
}
#[test]
fn nested_option_constructor_cannot_hide_duplicate_or_borrow_conflict() {
    reject(
        "region test(h: Handle) exits Done(a: Option<Handle>,b: Option<Handle>) { [Entry]: exit Done(Some(h),Some(h)); }",
        "E_ALIAS",
    );
    reject(
        "region test(h: Handle) exits Done { [Entry]: delta borrow_and_wrap(h,Some(h)) -> [Done => D]; [D]: exit Done; }",
        "E_ALIAS",
    );
}
#[test]
fn synchronous_borrow_keeps_sum_available_for_later_consuming_decision() {
    let p = checked(
        "region test(state: FileState) exits Done(file: Option<Handle>) { [Entry]: delta inspect_state(state) -> [Done => Inspected]; [Inspected]: delta state -> [Missing => N,Present(h) => S,Blocked(reason) => B]; [N | B]: exit Done(None); [S]: exit Done(Some(h)); }",
    );
    let mut s = scenario(vec![value(
        "FileState",
        "Present",
        vec![opaque("Handle", "owned")],
    )]);
    s.calls = vec![call("inspect_state", "Done", vec![])];
    equivalent(&p, &s);
}
#[test]
fn resource_variant_moves_are_path_dependent() {
    let p = checked(
        "region test(file: Option<Handle>,flag: Bool) exits Done(file: Option<Handle>) { [Entry]: delta flag -> [true => A,false => B]; [A]: delta file -> [None => N,Some(h) => S]; [N]: exit Done(None); [S]: exit Done(Some(h)); [B]: exit Done(file); }",
    );
    for flag in [false, true] {
        for file in [
            value("Option<Handle>", "None", vec![]),
            value("Option<Handle>", "Some", vec![opaque("Handle", "owned")]),
        ] {
            assert_eq!(
                equivalent(&p, &scenario(vec![file.clone(), Value::Bool(flag)])).fields,
                vec![file]
            );
        }
    }
}
#[test]
fn path_disjoint_constructor_definitions_share_a_slot() {
    let p = checked(
        "region test(h: Handle,flag: Bool) exits Done(file: Option<Handle>) { [Entry]: delta flag -> [true => A,false => B]; [A]: file := Some(h); [B]: file := Option.Some(h); [Entry]: exit Done(file); }",
    );
    for flag in [false, true] {
        equivalent(
            &p,
            &scenario(vec![opaque("Handle", "owned"), Value::Bool(flag)]),
        );
    }
}
#[test]
fn resource_payload_cannot_escape_nested_scope_without_transfer() {
    reject(
        "region test(file: Option<Handle>) exits Done { [Entry] over &: { [Entry]: delta file -> [None => N,Some(h) => S]; } [Entry]: exit Done; }",
        "E_RESOURCE",
    );
}
#[test]
fn looping_callee_destructures_and_reassembles_resource_option() {
    let p = checked(
        "region unwrap(file: Option<Handle>) exits Done(file: Option<Handle>) { loop L(current: Option<Handle> = file, first: Bool = true) { [Entry]: delta first -> [true => Initial,false => Later]; [Initial]: delta current -> [None => N,Some(h) => S]; [N]: again L(None,false); [S]: again L(Some(h),false); [Later]: exit Done(current); } } region test(h: Handle) exits Done(file: Option<Handle>) { [Entry]: delta unwrap(Some(h)) -> [Done(file) => Ready]; [Ready]: exit Done(file); }",
    );
    assert_eq!(
        equivalent(&p, &scenario(vec![opaque("Handle", "owned")])).fields,
        vec![value(
            "Option<Handle>",
            "Some",
            vec![opaque("Handle", "owned")]
        )]
    );
}
#[test]
fn loop_cannot_restart_with_consumed_sum_or_leave_payload_behind() {
    reject(
        "region test(file: Option<Handle>) exits Done { loop L(file2: Option<Handle> = file) { [Entry]: delta file2 -> [None => N,Some(h) => S]; [Entry]: again L(file2); } }",
        "E_MOVED",
    );
    reject(
        "region test(file: Option<Handle>) exits Done { loop L(file2: Option<Handle> = file) { [Entry]: delta file2 -> [None => N,Some(h) => S]; [Entry]: again L(None); } }",
        "E_RESOURCE",
    );
}
#[test]
fn host_resource_variant_payload_is_validated_and_transferred() {
    let p = checked(
        "region test() exits Done(file: Option<Handle>) { [Entry]: delta receive() -> [Got(state) => Ready]; [Ready]: delta state -> [Missing => N,Present(h) => S,Blocked(reason) => B]; [N | B]: exit Done(None); [S]: exit Done(Some(h)); }",
    );
    let mut s = scenario(vec![]);
    s.calls = vec![call(
        "receive",
        "Got",
        vec![value(
            "FileState",
            "Present",
            vec![opaque("Handle", "owned")],
        )],
    )];
    equivalent(&p, &s);
    s.calls = vec![call(
        "receive",
        "Got",
        vec![value("FileState", "Present", vec![Value::U32(1)])],
    )];
    assert_eq!(p.run(&s).unwrap_err().code, "E_DRIVER");
}
#[test]
fn non_linear_variant_can_still_be_decided_more_than_once() {
    checked(
        "region test(file: Option<U32>) exits Done { [Entry]: delta file -> [None => N,Some(x) => S]; [Entry]: delta file -> [None => N2,Some(y) => S2]; [Entry]: exit Done; }",
    );
}
#[test]
fn existing_some_none_constructor_names_keep_their_meaning() {
    let mut m: serde_json::Value = json!({"version":"0.1","types":[{"name":"Choice","kind":"variant","constructors":[{"name":"Some","fields":[]},{"name":"None","fields":[]}]}],"primitives":[]});
    let source = "region test() exits Done(choice: Choice,file: Option<U32>) { [Entry]: exit Done(Some,Option.None); }";
    let p = compile(source, Manifest::from_json(&m.to_string()).unwrap()).unwrap();
    assert_eq!(
        p.run(&scenario(vec![])).unwrap().fields[0],
        value("Choice", "Some", vec![])
    );
    m["types"].as_array_mut().unwrap().push(
        json!({"name":"Other","kind":"variant","constructors":[{"name":"Some","fields":[]}]}),
    );
    assert_eq!(
        compile(source, Manifest::from_json(&m.to_string()).unwrap())
            .unwrap_err()
            .code,
        "E_NAME"
    );
}
#[test]
fn recursive_resource_sums_remain_rejected() {
    for ty in ["Loop", "Option<Loop>"] {
        let m = json!({"version":"0.1","types":[{"name":"Loop","kind":"variant","constructors":[{"name":"More","fields":[{"name":"next","type":ty}]}]}],"primitives":[]});
        assert!(
            Manifest::from_json(&m.to_string())
                .unwrap_err()
                .message
                .contains("recursive")
        );
    }
}

#[test]
fn alternative_resource_payloads_can_define_the_same_disjoint_slot() {
    let m = Manifest::from_json(r#"{"version":"0.1","types":[{"name":"Handle","kind":"opaque","linear":true},{"name":"Either","kind":"variant","constructors":[{"name":"Left","fields":[{"name":"h","type":"Handle"}]},{"name":"Right","fields":[{"name":"h","type":"Handle"}]}]}],"primitives":[]}"#).unwrap();
    let p=compile("region test(e: Either) exits Done(h: Handle) { [Entry]: delta e -> [Left(h) => L,Right(h) => R]; [L | R]: exit Done(h); }",m).unwrap();
    for constructor in ["Left", "Right"] {
        assert_eq!(
            equivalent(
                &p,
                &scenario(vec![value(
                    "Either",
                    constructor,
                    vec![opaque("Handle", constructor)]
                )])
            )
            .fields,
            vec![opaque("Handle", constructor)]
        );
    }
}
#[test]
fn freshly_constructed_variant_transfers_all_resource_payloads() {
    let m = Manifest::from_json(r#"{"version":"0.1","types":[{"name":"Handle","kind":"opaque","linear":true},{"name":"PairVariant","kind":"variant","constructors":[{"name":"Pair","fields":[{"name":"a","type":"Handle"},{"name":"b","type":"Handle"}]}]}],"primitives":[]}"#).unwrap();
    let p=compile("region test(a: Handle,b: Handle) exits Done(a: Handle,b: Handle) { [Entry]: delta Pair(a,b) -> [Pair(left,right) => Ready]; [Ready]: exit Done(left,right); }",m.clone()).unwrap();
    let values = vec![opaque("Handle", "left"), opaque("Handle", "right")];
    assert_eq!(equivalent(&p, &scenario(values.clone())).fields, values);
    assert_eq!(compile("region test(a: Handle,b: Handle) exits Done(a: Handle) { [Entry]: delta Pair(a,b) -> [Pair(left,right) => Ready]; [Ready]: exit Done(left); }",m).unwrap_err().code,"E_RESOURCE");
}
