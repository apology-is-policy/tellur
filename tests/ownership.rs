use tellur::{
    compile,
    ir::Value,
    manifest::{Field, Manifest, Mode, Outcome, Parameter, Primitive, TypeDef},
    runtime::{Event, Scenario, ScriptCall},
};
fn field(name: &str, ty: &str) -> Field {
    Field {
        name: name.into(),
        ty: ty.into(),
    }
}
fn manifest() -> Manifest {
    let mut m = Manifest::from_json(include_str!("../examples/manifest.json")).unwrap();
    m.types.extend([
        TypeDef::Record {
            name: "Envelope".into(),
            fields: vec![field("pair", "HandlePair"), field("serial", "U32")],
        },
        TypeDef::Record {
            name: "Settings".into(),
            fields: vec![field("port", "U32"), field("enabled", "Bool")],
        },
    ]);
    for (name, params) in [
        ("inspect", vec![("p", "HandlePair", Mode::Borrow)]),
        ("dispose", vec![("p", "HandlePair", Mode::Consume)]),
        (
            "mixed",
            vec![
                ("p", "HandlePair", Mode::Borrow),
                ("h", "Handle", Mode::Consume),
            ],
        ),
        (
            "reverse_mixed",
            vec![
                ("h", "Handle", Mode::Consume),
                ("p", "HandlePair", Mode::Borrow),
            ],
        ),
        (
            "nested_mixed",
            vec![
                ("p", "HandlePair", Mode::Borrow),
                ("q", "HandlePair", Mode::Consume),
            ],
        ),
        (
            "borrow_and_take",
            vec![
                ("a", "Handle", Mode::Borrow),
                ("b", "Handle", Mode::Consume),
            ],
        ),
        (
            "two_borrows",
            vec![
                ("p", "HandlePair", Mode::Borrow),
                ("h", "Handle", Mode::Borrow),
            ],
        ),
    ] {
        m.primitives.push(Primitive {
            name: name.into(),
            parameters: params
                .into_iter()
                .map(|(n, t, mode)| Parameter {
                    name: n.into(),
                    ty: t.into(),
                    mode,
                })
                .collect(),
            outcomes: vec![Outcome {
                name: "Done".into(),
                fields: vec![],
            }],
            effects: vec![],
            returns: true,
            pure: false,
            rust: None,
        });
    }
    m.validate().unwrap();
    m
}
fn checked(s: &str) -> tellur::CheckedProgram {
    compile(s, manifest()).unwrap()
}
fn reject(s: &str, code: &str) -> String {
    let e = compile(s, manifest()).unwrap_err();
    assert_eq!(e.code, code, "{e}");
    e.message
}
fn opaque(ty: &str, token: &str) -> Value {
    Value::Opaque {
        ty: ty.into(),
        token: token.into(),
    }
}
fn pair() -> Value {
    Value::Construct {
        ty: "HandlePair".into(),
        name: "HandlePair".into(),
        fields: vec![
            opaque("Handle", "left"),
            opaque("Handle", "right"),
            Value::String("label".into()),
        ],
    }
}
fn close(outcome: &str, token: &str) -> ScriptCall {
    ScriptCall {
        operation: "close".into(),
        outcome: outcome.into(),
        fields: if outcome == "Closed" {
            vec![]
        } else {
            vec![opaque("CloseError", token)]
        },
    }
}
fn s(args: Vec<Value>, calls: Vec<ScriptCall>) -> Scenario {
    Scenario {
        entry: "test".into(),
        arguments: args,
        calls,
    }
}
fn closing(place: &str, suffix: &str) -> String {
    format!("delta close({place}) -> [Closed => C{suffix},Failed(e{suffix}) => F{suffix}];")
}
#[test]
fn owned_record_fixture_preserves_both_errors() {
    let p = checked(include_str!("../examples/partial_moves.tellur"));
    for a in ["Closed", "Failed"] {
        for b in ["Closed", "Failed"] {
            let s = Scenario {
                entry: "close_pair".into(),
                arguments: vec![opaque("Handle", "left"), opaque("Handle", "right")],
                calls: vec![close(a, "left"), close(b, "right")],
            };
            let result = p.run(&s).unwrap();
            assert_eq!(
                result.outcome,
                if a == "Closed" && b == "Closed" {
                    "Closed"
                } else {
                    "Failed"
                }
            );
            assert_eq!(
                result
                    .events
                    .iter()
                    .filter(|e| matches!(e, Event::Emit { .. }))
                    .count(),
                1
            );
            if a == "Failed" && b == "Failed" {
                assert_eq!(
                    result.fields,
                    vec![Value::Construct {
                        ty: "PairCloseError".into(),
                        name: "TwoCloseFailures".into(),
                        fields: vec![opaque("CloseError", "left"), opaque("CloseError", "right")]
                    }]
                );
            }
        }
    }
}
#[test]
fn ownership_is_derived_through_nested_records() {
    let m = manifest();
    assert!(m.linear("HandlePair"));
    assert!(m.linear("Envelope"));
    assert!(!m.linear("Settings"));
    assert_eq!(m.resource_paths("Envelope"), vec![vec![0, 0], vec![0, 1]]);
}
#[test]
fn records_cannot_enter_value_mode_primitive_parameters() {
    let mut m = manifest();
    m.primitives
        .iter_mut()
        .find(|p| p.name == "dispose")
        .unwrap()
        .parameters[0]
        .mode = Mode::Value;
    assert!(
        m.validate()
            .unwrap_err()
            .message
            .contains("borrow or consume")
    );
}
#[test]
fn whole_use_after_partial_move_is_rejected() {
    let e = reject(
        "region test(p: HandlePair) exits Done { [Entry]: h := p.left; delta inspect(p) -> [Done => D]; [Entry]: exit Done; }",
        "E_PARTIAL_MOVE",
    );
    assert!(e.contains("p.left") && e.contains("as a whole"));
}
#[test]
fn whole_move_invalidates_copyable_fields_too() {
    reject(
        "region test(p: HandlePair) exits Done { [Entry]: q := p; emit(p.label); exit Done; }",
        "E_MOVED",
    );
}
#[test]
fn moving_nested_record_invalidates_its_fields_but_not_siblings() {
    let p = checked(
        "region test(e: Envelope) exits Done(serial: U32,pair: HandlePair) { [Entry]: p := e.pair; serial := e.serial; exit Done(serial,p); }",
    );
    let envelope = Value::Construct {
        ty: "Envelope".into(),
        name: "Envelope".into(),
        fields: vec![pair(), Value::U32(42)],
    };
    assert_eq!(
        p.run(&s(vec![envelope], vec![])).unwrap().fields,
        vec![Value::U32(42), pair()]
    );
    reject(
        "region test(e: Envelope) exits Done { [Entry]: p := e.pair; emit(e.pair.label); exit Done; }",
        "E_MOVED",
    );
}
#[test]
fn moved_field_cannot_be_read_twice() {
    reject(
        "region test(p: HandlePair) exits Done { [Entry]: a := p.left; b := p.left; exit Done; }",
        "E_MOVED",
    );
}
#[test]
fn leak_diagnostic_identifies_surviving_field() {
    let source = format!(
        "region test(p: HandlePair) exits Done {{ [Entry]: {} exit Done; }}",
        closing("p.left", "")
    );
    let e = reject(&source, "E_RESOURCE");
    assert!(e.contains("p.right"));
}
#[test]
fn nested_record_field_leak_is_named() {
    let e = reject(
        "region test(e: Envelope) exits Done { [Entry]: exit Done; }",
        "E_RESOURCE",
    );
    assert!(e.contains("e.pair.left"));
}
#[test]
fn path_dependent_field_moves_and_disjoint_definitions() {
    let source = format!(
        "region test(p: HandlePair,flag: Bool) exits Done {{ [Entry]: delta flag -> [true => A,false => B]; [A]: h := p.left; [B]: h := p.right; [Entry]: {} [A]: {} [B]: {} [Entry]: exit Done; }}",
        closing("h", "1"),
        closing("p.right", "2"),
        closing("p.left", "3")
    );
    let p = checked(&source);
    for b in [false, true] {
        let result = p
            .run(&s(
                vec![pair(), Value::Bool(b)],
                vec![close("Closed", ""), close("Closed", "")],
            ))
            .unwrap();
        let args = result
            .events
            .iter()
            .filter_map(|e| match e {
                Event::Primitive { arguments, .. } => Some(arguments[0].clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            args,
            if b {
                vec![opaque("Handle", "left"), opaque("Handle", "right")]
            } else {
                vec![opaque("Handle", "right"), opaque("Handle", "left")]
            }
        );
    }
}
#[test]
fn use_after_move_witness_selects_only_affected_family() {
    let e = reject(
        "region test(p: HandlePair,flag: Bool) exits Done { [Entry]: delta flag -> [true => A,false => B]; [A]: h := p.left; [Entry]: q := p; exit Done; }",
        "E_PARTIAL_MOVE",
    );
    assert!(e.ends_with(": A"), "{e}");
}
#[test]
fn whole_record_move_remains_valid_on_disjoint_family() {
    checked(
        "region test(p: HandlePair,flag: Bool) exits Done { [Entry]: delta flag -> [true => A,false => B]; [A]: h := p.left; delta close(h) -> [Closed => C1,Failed(e1) => F1]; delta close(p.right) -> [Closed => C2,Failed(e2) => F2]; [B]: delta dispose(p) -> [Done => D]; [Entry]: exit Done; }",
    );
}
#[test]
fn whole_borrow_conflicts_with_field_consumption_in_both_orders() {
    for call in ["mixed(p,p.left)", "reverse_mixed(p.left,p)"] {
        reject(
            &format!(
                "region test(p: HandlePair) exits Done {{ [Entry]: delta {call} -> [Done => D]; [Entry]: exit Done; }}"
            ),
            "E_ALIAS",
        );
    }
}
#[test]
fn nested_argument_construction_does_not_hide_a_move() {
    reject(
        "region test(p: HandlePair) exits Done { [Entry]: delta nested_mixed(p,HandlePair(p.left,p.right,p.label)) -> [Done => D]; [Entry]: exit Done; }",
        "E_ALIAS",
    );
}
#[test]
fn duplicate_moves_in_constructor_are_rejected() {
    reject(
        "region test(h: Handle) exits Done(pair: HandlePair) { [Entry]: exit Done(HandlePair(h,h,\"x\")); }",
        "E_ALIAS",
    );
}
#[test]
fn disjoint_field_borrow_and_move_is_allowed() {
    checked(
        "region test(p: HandlePair) exits Done { [Entry]: delta borrow_and_take(p.left,p.right) -> [Done => D]; [D]: delta close(p.left) -> [Closed => C,Failed(e) => F]; [Entry]: exit Done; }",
    );
}
#[test]
fn shared_borrows_of_parent_and_field_are_allowed() {
    checked(
        "region test(p: HandlePair) exits Done(pair: HandlePair) { [Entry]: delta two_borrows(p,p.left) -> [Done => D]; [D]: exit Done(p); }",
    );
}
#[test]
fn copying_metadata_before_whole_move_is_allowed() {
    checked(
        "region test(p: HandlePair) exits Done(label: String,p: HandlePair) { [Entry]: exit Done(p.label,p); }",
    );
    reject(
        "region test(p: HandlePair) exits Done(p: HandlePair,label: String) { [Entry]: exit Done(p,p.label); }",
        "E_MOVED",
    );
}
#[test]
fn non_linear_projections_and_precedence() {
    let p = checked(
        "region test() exits Done(n: U32,flag: Bool) { [Entry]: settings := Settings(42,true); value := (settings).port; flag := !Settings(1,false).enabled && value == 42; exit Done(value,flag); }",
    );
    assert_eq!(
        p.run(&s(vec![], vec![])).unwrap().fields,
        vec![Value::U32(42), Value::Bool(true)]
    );
}
#[test]
fn unknown_field_and_non_record_projection_are_rejected() {
    reject(
        "region test(p: HandlePair) exits Done { [Entry]: x := p.missing; exit Done; }",
        "E_FIELD",
    );
    reject(
        "region test() exits Done { [Entry]: x := (1).missing; exit Done; }",
        "E_FIELD",
    );
}
#[test]
fn owned_temporary_projection_cannot_discard_remaining_fields() {
    reject(
        "region test(a: Handle,b: Handle) exits Done { [Entry]: emit(HandlePair(a,b,\"x\").label); exit Done; }",
        "E_TEMPORARY",
    );
}
#[test]
fn borrowing_an_owned_temporary_cannot_drop_its_obligation() {
    reject(
        "region test(a: Handle,b: Handle) exits Done { [Entry]: delta inspect(HandlePair(a,b,\"x\")) -> [Done => D]; [Entry]: exit Done; }",
        "E_TEMPORARY",
    );
}
#[test]
fn consuming_owned_temporary_is_allowed() {
    checked(
        "region test(a: Handle,b: Handle) exits Done { [Entry]: delta dispose(HandlePair(a,b,\"x\")) -> [Done => D]; [D]: exit Done; }",
    );
}
#[test]
fn reassembling_partial_record_transfers_remaining_obligations() {
    let p = checked(
        "region test(p: HandlePair) exits Done(pair: HandlePair) { [Entry]: left := p.left; result := HandlePair(left,p.right,p.label); exit Done(result); }",
    );
    assert_eq!(
        p.run(&s(vec![pair()], vec![])).unwrap().fields,
        vec![pair()]
    );
}
#[test]
fn record_ownership_across_region_calls_and_expansion() {
    let p = checked(
        "region split(p: HandlePair) exits Split(left: Handle,right: Handle,label: String) { [Entry]: a := p.left; exit Split(a,p.right,p.label); } region test(p: HandlePair) exits Done(pair: HandlePair) { [Entry]: delta split(p) -> [Split(a,b,label) => Ready]; [Ready]: exit Done(HandlePair(a,b,label)); }",
    );
    let s = s(vec![pair()], vec![]);
    assert_eq!(p.run(&s).unwrap().fields, vec![pair()]);
    assert_eq!(
        p.run(&s).unwrap(),
        p.expanded("test").unwrap().run(&s).unwrap()
    );
}
#[test]
fn record_ownership_survives_loop_restarts_and_expansion() {
    let source = "region swap(p: HandlePair) exits Swapped(pair: HandlePair) { [Entry]: exit Swapped(HandlePair(p.right,p.left,p.label)); } region test(p: HandlePair) exits Done(pair: HandlePair) { loop L(current: HandlePair = p,first: Bool = true) { [Entry]: delta first -> [true => First,false => Later]; [First]: delta swap(current) -> [Swapped(next) => Ready]; [Ready]: again L(next,false); [Later]: exit Done(current); } }";
    let p = checked(source);
    let s = s(vec![pair()], vec![]);
    let expected = Value::Construct {
        ty: "HandlePair".into(),
        name: "HandlePair".into(),
        fields: vec![
            opaque("Handle", "right"),
            opaque("Handle", "left"),
            Value::String("label".into()),
        ],
    };
    assert_eq!(p.run(&s).unwrap().fields, vec![expected]);
    assert_eq!(
        p.run(&s).unwrap(),
        p.expanded("test").unwrap().run(&s).unwrap()
    );
}
#[test]
fn restarting_with_a_partial_record_is_rejected() {
    reject(
        "region test(p: HandlePair) exits Done { loop L(current: HandlePair = p) { [Entry]: delta close(current.left) -> [Closed => C,Failed(e) => F]; [Entry]: again L(current); } }",
        "E_PARTIAL_MOVE",
    );
}
#[test]
fn resources_in_variants_and_options_are_sealed_obligations() {
    let mut m = manifest();
    m.types.push(TypeDef::Variant {
        name: "Choice".into(),
        constructors: vec![Outcome {
            name: "Owned".into(),
            fields: vec![field("p", "HandlePair")],
        }],
    });
    m.validate().unwrap();
    assert!(m.linear("Choice"));
    assert_eq!(m.resource_paths("Choice"), vec![Vec::<usize>::new()]);
    assert_eq!(
        m.resource_paths("Option<HandlePair>"),
        vec![Vec::<usize>::new()]
    );
}
#[test]
fn direct_and_indirect_recursive_records_are_rejected_without_recursion_overflow() {
    for json in [
        r#"{"version":"0.1","types":[{"name":"A","kind":"record","fields":[{"name":"a","type":"A"}]}],"primitives":[]}"#,
        r#"{"version":"0.1","types":[{"name":"A","kind":"record","fields":[{"name":"b","type":"B"}]},{"name":"B","kind":"record","fields":[{"name":"a","type":"A"}]}],"primitives":[]}"#,
    ] {
        assert!(
            Manifest::from_json(json)
                .unwrap_err()
                .message
                .contains("recursive")
        );
    }
}
#[test]
fn qualified_constructors_still_resolve() {
    checked(
        "region test(a: ReadError,b: CloseError) exits Done(error: LoadError) { [Entry]: exit Done(LoadError.Both(a,b)); }",
    );
}
