use tellur::{
    algebra::{BoundaryMismatch, compare_scenarios, outcome_embedding},
    compile,
    ir::{Action, Value},
    manifest::{Field, Manifest, Outcome},
    runtime::Scenario,
};
fn m() -> Manifest {
    Manifest::from_json(include_str!("../playground/manifest.json")).unwrap()
}
fn cases() -> Vec<Scenario> {
    [false, true]
        .into_iter()
        .map(|b| Scenario {
            entry: "main".into(),
            arguments: vec![Value::Bool(b)],
            calls: vec![],
        })
        .collect()
}
const BASE: &str = r#"outcome stage {ready {string text},empty}
control p(bool flag) exits stage {emit("p");exit delta flag {true=>ready {"value"},false=>empty {}};}
control q(stage value) exits stage {emit("q");exit value;}
control r(stage value) exits stage {emit("r");exit value;}
control identity(stage value) exits stage {exit value;}
"#;
fn agree(a: &str, b: &str) {
    let a = compile(&format!("{BASE}{a}"), m()).unwrap();
    let b = compile(&format!("{BASE}{b}"), m()).unwrap();
    for (a, b) in [
        (&a, &b),
        (&a.expanded("main").unwrap(), &b.expanded("main").unwrap()),
    ] {
        let report = compare_scenarios(a, b, &cases(), 10000).unwrap();
        assert!(report.observations_match, "{report:?}");
        assert!(report.declared_outcome_shapes_equal);
    }
}
#[test]
fn boundary_embeddings_have_identity_and_composition_and_require_exact_payloads() {
    let one = Outcome {
        name: "ready".into(),
        fields: vec![Field {
            name: "n".into(),
            ty: "U32".into(),
        }],
    };
    let empty = Outcome {
        name: "empty".into(),
        fields: vec![],
    };
    let failed = Outcome {
        name: "failed".into(),
        fields: vec![],
    };
    let a = vec![one.clone()];
    let b = vec![empty.clone(), one.clone()];
    let c = vec![failed, one.clone(), empty];
    assert_eq!(outcome_embedding(&a, &a).unwrap(), vec![0]);
    let ab = outcome_embedding(&a, &b).unwrap();
    let bc = outcome_embedding(&b, &c).unwrap();
    assert_eq!(
        outcome_embedding(&a, &c).unwrap(),
        ab.iter().map(|i| bc[*i]).collect::<Vec<_>>()
    );
    let mut wrong = one.clone();
    wrong.fields[0].ty = "String".into();
    assert!(matches!(
        outcome_embedding(&a, &[wrong]),
        Err(BoundaryMismatch::Type { .. })
    ));
    assert!(matches!(
        outcome_embedding(&b, &a),
        Err(BoundaryMismatch::Missing(_))
    ));
    assert!(matches!(
        outcome_embedding(&[one.clone(), one], &a),
        Err(BoundaryMismatch::Duplicate(_))
    ));
}
#[test]
fn identity_routing_on_both_sides_preserves_observations() {
    let direct = "control main(bool flag) exits stage {forward p(flag);}";
    agree(
        direct,
        "control main(bool flag) exits stage {auto value=capture p(flag);forward identity(value);}",
    );
    agree(
        direct,
        "control id_input(bool flag) exits passed {bool value} {exit passed {flag};} control main(bool flag) exits stage {exit delta id_input(flag) {passed {auto value}=>delta p(value) {ready=>forward,empty=>forward}};}",
    );
}
#[test]
fn associative_wiring_preserves_effect_order_and_payloads() {
    let left = "control pq(bool flag) exits stage {auto value=capture p(flag);forward q(value);} control main(bool flag) exits stage {auto value=capture pq(flag);forward r(value);}";
    let right = "control qr(stage value) exits stage {auto next=capture q(value);forward r(next);} control main(bool flag) exits stage {auto value=capture p(flag);forward qr(value);}";
    agree(left, right);
}
#[test]
fn moving_capture_across_an_effect_is_a_counterexample() {
    let a=compile(&format!("{BASE}control main(bool flag) exits stage {{auto value=capture p(flag);emit(\"between\");exit value;}}"),m()).unwrap();
    let b = compile(
        &format!(
            "{BASE}control main(bool flag) exits stage {{emit(\"between\");forward p(flag);}}"
        ),
        m(),
    )
    .unwrap();
    let report = compare_scenarios(&a, &b, &cases(), 1000).unwrap();
    assert!(!report.observations_match);
    assert_eq!(report.differences.len(), 2);
    assert!(report.declared_outcome_shapes_equal);
}
#[test]
fn effect_composition_is_neither_commutative_nor_idempotent() {
    let make = |body: &str| {
        compile(
            &format!("control main(bool flag) exits done {{{body}exit done;}}"),
            m(),
        )
        .unwrap()
    };
    for (a, b) in [
        ("emit(\"a\");emit(\"b\");", "emit(\"b\");emit(\"a\");"),
        ("emit(\"a\");", "emit(\"a\");emit(\"a\");"),
    ] {
        assert!(
            !compare_scenarios(&make(a), &make(b), &cases(), 1000)
                .unwrap()
                .observations_match
        );
    }
}
#[test]
fn boolean_masks_are_idempotent_and_distribute_without_duplicating_work() {
    let mut b = tellur::bdd::Bdd::default();
    let a = b.variable(1);
    let c = b.variable(2);
    let d = b.variable(3);
    assert_eq!(b.or(a, a), a);
    assert_eq!(b.and(a, a), a);
    let cd = b.or(c, d);
    let lhs = b.and(a, cd);
    let ac = b.and(a, c);
    let ad = b.and(a, d);
    assert_eq!(lhs, b.or(ac, ad));
    let source = "control main(bool flag) exits done {delta flag {true=>yes,false=>no} [yes|yes]:emit(\"once\"); [entry]:exit done;}";
    let p = compile(source, m()).unwrap();
    assert_eq!(p.run(&cases()[1]).unwrap().events.len(), 1);
}
#[test]
fn expansion_preserves_distinct_contributors_behind_one_public_exit() {
    let source = "control same(bool flag) exits done {delta flag {true=>a,false=>b} [a]:exit done; [b]:exit done;} control main(bool flag) exits done {same(flag);forward same(flag);}";
    let p = compile(source, m()).unwrap();
    let expanded = p.expanded("main").unwrap();
    assert!(
        compare_scenarios(&p, &expanded, &cases(), 1000)
            .unwrap()
            .observations_match
    );
    let completions = expanded
        .region("main")
        .unwrap()
        .code
        .iter()
        .filter(|i| matches!(i.action, Action::Complete { .. }) && i.provenance.region == "same")
        .collect::<Vec<_>>();
    assert_eq!(completions.len(), 4);
    assert_eq!(
        completions
            .iter()
            .map(|i| i.provenance.calls.clone())
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        2
    );
    assert_eq!(
        completions
            .iter()
            .map(|i| i.provenance.definition)
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        2
    );
}
#[test]
fn agreement_is_scoped_and_contract_growth_is_reported_separately() {
    let a = compile("control main(bool flag) exits done {exit done;}", m()).unwrap();
    let b = compile(
        "control main(bool flag) exits done,new_failure {exit done;}",
        m(),
    )
    .unwrap();
    let report = compare_scenarios(&a, &b, &cases(), 100).unwrap();
    assert!(report.observations_match);
    assert!(!report.declared_outcome_shapes_equal);
    assert_eq!(report.claim, "agreement_on_supplied_scenarios_only");
    assert_eq!(
        compare_scenarios(&a, &a, &[], 100).unwrap_err().code,
        "E_COMPARE"
    );
    let infinite = compile(
        "control main(bool flag) exits done {loop repeat() {again repeat();}}",
        m(),
    )
    .unwrap();
    assert_eq!(
        compare_scenarios(&infinite, &infinite, &cases(), 20)
            .unwrap_err()
            .code,
        "E_COMPARE_INCONCLUSIVE"
    );
    let invalid = Scenario {
        entry: "main".into(),
        arguments: vec![],
        calls: vec![],
    };
    assert_eq!(
        compare_scenarios(&a, &a, &[invalid], 100).unwrap_err().code,
        "E_COMPARE_INCONCLUSIVE"
    );
}

#[test]
fn comparison_checks_reachable_type_layouts_and_owning_entry_boundaries() {
    let make = |field: &str, input: &str| {
        compile(&format!("outcome inner {{item {{{field} value}}}} outcome outer {{nested {{inner value}},empty}} control main({input}) exits outer {{exit empty;}}"), m()).unwrap()
    };
    let scenario = Scenario {
        entry: "main".into(),
        arguments: vec![],
        calls: vec![],
    };
    let report =
        compare_scenarios(&make("u32", ""), &make("string", ""), &[scenario], 100).unwrap();
    assert!(report.observations_match);
    assert!(!report.declared_outcome_shapes_equal);
    assert_eq!(report.checked_entries, vec!["main"]);
    // Input layout validation precedes execution, even for a malformed driver.
    for (a, b) in [
        (
            "control main(&string x) exits done {exit done;}",
            "control main(&string x) exits done {exit done;}",
        ),
        (
            "control main(u32 x) exits done {exit done;}",
            "control main(string x) exits done {exit done;}",
        ),
    ] {
        assert_eq!(
            compare_scenarios(
                &compile(a, m()).unwrap(),
                &compile(b, m()).unwrap(),
                &cases(),
                100
            )
            .unwrap_err()
            .code,
            "E_COMPARE_BOUNDARY"
        );
    }
    let mut a = m();
    let mut b = m();
    use tellur::manifest::TypeDef;
    for (manifest, ty) in [(&mut a, "U32"), (&mut b, "String")] {
        manifest.types.push(TypeDef::Record {
            name: "box_value".into(),
            fields: vec![Field {
                name: "item".into(),
                ty: ty.into(),
            }],
        });
    }
    let source = "control main(box_value value) exits done {exit done;}";
    assert_eq!(
        compare_scenarios(
            &compile(source, a).unwrap(),
            &compile(source, b).unwrap(),
            &cases(),
            100
        )
        .unwrap_err()
        .code,
        "E_COMPARE_BOUNDARY"
    );
}

#[test]
fn comparison_cli_reports_finite_agreement_and_counterexamples() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    for (other, success) in [("identity.path", true), ("reordered.path", false)] {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_tellurc"))
            .current_dir(root)
            .args([
                "compare",
                "playground/algebra/original.path",
                "playground/manifest.json",
                &format!("playground/algebra/{other}"),
                "playground/algebra/scenarios.json",
            ])
            .output()
            .unwrap();
        assert_eq!(
            output.status.success(),
            success,
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["claim"], "agreement_on_supplied_scenarios_only");
        assert_eq!(report["cases"], 2);
        assert_eq!(report["observations_match"], success);
        assert_eq!(report["declared_outcome_shapes_equal"], true);
    }
}
