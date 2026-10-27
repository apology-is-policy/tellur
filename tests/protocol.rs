use tellur::{
    compile,
    ir::Value,
    manifest::Manifest,
    protocol::{Symbol, project_execution},
    runtime::Scenario,
};
fn m() -> Manifest {
    Manifest::from_json(include_str!("../playground/manifest.json")).unwrap()
}
fn primitive(operation: &str, outcome: &str) -> Symbol {
    Symbol::Primitive {
        operation: operation.into(),
        outcome: outcome.into(),
    }
}
fn exit(outcome: &str) -> Symbol {
    Symbol::Exit {
        outcome: outcome.into(),
    }
}
#[test]
fn failed_read_still_requires_a_close_before_any_public_exit() {
    let p = compile(include_str!("../playground/read_file.tellur"), m()).unwrap();
    let model = p.protocol("show_file", 20_000).unwrap();
    let prefix = vec![primitive("open", "opened"), primitive("read", "failed")];
    let residual = model.residual(&prefix);
    assert!(residual.prefix_possible && residual.can_complete && !residual.complete);
    assert_eq!(
        residual.next_symbols,
        vec![primitive("close", "closed"), primitive("close", "failed")]
    );
    assert!(!residual.may_silently_diverge);
    let mut skipped = prefix.clone();
    skipped.push(exit("failed"));
    assert!(!model.residual(&skipped).prefix_possible);
    let mut completed = prefix;
    completed.push(primitive("close", "closed"));
    completed.push(exit("failed"));
    assert!(model.residual(&completed).complete);
    completed.push(Symbol::Emit);
    let after = model.residual(&completed);
    assert!(!after.prefix_possible && !after.can_complete && after.next_symbols.is_empty());
}
#[test]
fn structural_possibilities_do_not_claim_data_feasibility() {
    let p = compile(
        "control main() exits auto {exit delta true {true=>done {},false=>impossible {}};}",
        m(),
    )
    .unwrap();
    let model = p.protocol("main", 100).unwrap();
    assert!(model.residual(&[exit("impossible")]).complete);
    let actual = p
        .run(&Scenario {
            entry: "main".into(),
            arguments: vec![],
            calls: vec![],
        })
        .unwrap();
    assert_eq!(actual.outcome, "done");
    assert!(model.residual(&project_execution(&actual)).complete);
}
#[test]
fn loops_reset_decision_facts_and_support_unbounded_prefixes() {
    let m = Manifest::from_json(include_str!("../examples/manifest.json")).unwrap();
    let p = compile(include_str!("../examples/fetch_retry.tellur"), m).unwrap();
    let model = p.protocol("fetch_retry", 20_000).unwrap();
    let mut prefix = vec![];
    // Real code permits only three attempts; this abstraction forgets the count.
    // Repeated tokens exercise graph cycles rather than bounded unrolling.
    for _ in 0..30 {
        prefix.extend([
            primitive("fetch", "Unavailable"),
            primitive("checked_mul", "Product"),
            primitive("pause", "Elapsed"),
            primitive("checked_add", "Sum"),
        ]);
    }
    prefix.push(primitive("fetch", "Received"));
    prefix.push(exit("Done"));
    assert!(model.residual(&prefix).complete);
    assert!(model.state_count() < 1000);
    let stop = model.residual(&[primitive("fetch", "Cancelled")]);
    assert_eq!(stop.next_symbols, vec![exit("Cancelled")]);
}
#[test]
fn silent_cycles_are_distinct_from_completion_and_observable_cycles() {
    for (body, silent, completable) in [
        ("loop repeat() {again repeat();}", true, false),
        (
            "loop repeat() {emit(\"tick\");again repeat();}",
            false,
            false,
        ),
        ("exit done;", false, true),
    ] {
        let p = compile(&format!("control main() exits done {{{body}}}"), m()).unwrap();
        let r = p.protocol("main", 100).unwrap().residual(&[]);
        assert!(r.prefix_possible);
        assert!(!r.complete);
        assert_eq!(r.can_complete, completable);
        assert_eq!(r.may_silently_diverge, silent);
    }
    let p = compile(
        include_str!("../playground/protocol/finite_retry.tellur"),
        m(),
    )
    .unwrap();
    let r = p.protocol("main", 200).unwrap().residual(&[]);
    assert!(r.may_silently_diverge && r.can_complete);
    assert_eq!(
        p.run(&Scenario {
            entry: "main".into(),
            arguments: vec![],
            calls: vec![]
        })
        .unwrap()
        .outcome,
        "done"
    );
}
#[test]
fn nested_callee_loop_completion_resumes_the_caller() {
    let source = "control main() exits done {loop outer() {inner();emit(\"after\");exit done;}} control inner() exits done {loop inner() {exit done;}}";
    let p = compile(source, m()).unwrap();
    let model = p.protocol("main", 200).unwrap();
    let r = model.residual(&[]);
    assert!(!r.may_silently_diverge);
    assert_eq!(r.next_symbols, vec![Symbol::Emit]);
    assert!(model.residual(&[Symbol::Emit, exit("done")]).complete);
    assert!(!model.residual(&[exit("done")]).prefix_possible);
}
#[test]
fn stored_outcomes_do_not_replay_effects_and_repeated_invocations_remain_distinct() {
    let p = compile(include_str!("../playground/algebra/original.tellur"), m()).unwrap();
    let model = p.protocol("main", 1000).unwrap();
    for flag in [false, true] {
        let s = Scenario {
            entry: "main".into(),
            arguments: vec![Value::Bool(flag)],
            calls: vec![],
        };
        let actual = p.run(&s).unwrap();
        assert!(model.residual(&project_execution(&actual)).complete);
        assert!(
            !model
                .residual(&[Symbol::Emit, exit(&actual.outcome)])
                .prefix_possible
        );
    }
    let p = compile(include_str!("../playground/forwarding.tellur"), m()).unwrap();
    let model = p.protocol("main", 1000).unwrap();
    // Data abstraction permits each named message to be empty, but two emits
    // remain possible and a third cannot appear from one of the two invocations.
    assert!(
        model
            .residual(&[Symbol::Emit, Symbol::Emit, exit("done")])
            .complete
    );
    assert!(
        !model
            .residual(&[Symbol::Emit, Symbol::Emit, Symbol::Emit])
            .prefix_possible
    );
    let emit_ids = model
        .edges()
        .iter()
        .filter(|e| e.symbol == Some(Symbol::Emit))
        .map(|e| e.instruction)
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(emit_ids.len(), 2);
}
#[test]
fn state_budget_exhaustion_is_an_error_not_a_partial_model() {
    let p = compile(
        "control main(bool flag) exits auto {exit delta flag {true=>a {},false=>b {}};}",
        m(),
    )
    .unwrap();
    assert_eq!(p.protocol("main", 1).unwrap_err().code, "E_PROTOCOL_LIMIT");
    assert_eq!(p.protocol("main", 0).unwrap_err().code, "E_PROTOCOL");
    assert!(p.protocol("missing", 100).is_err());
}
#[test]
fn collection_and_borrowed_adapters_have_finite_structural_models() {
    for source in [
        include_str!("../playground/collections.tellur"),
        include_str!("../playground/references.tellur"),
        include_str!("../playground/stored_adapters.tellur"),
    ] {
        let p = compile(source, m()).unwrap();
        let model = p.protocol("main", 20_000).unwrap();
        let actual = p
            .run(&Scenario {
                entry: "main".into(),
                arguments: vec![],
                calls: vec![],
            })
            .unwrap();
        assert!(model.residual(&project_execution(&actual)).complete);
    }
}
#[test]
fn protocol_cli_reads_prefixes_and_labels_the_abstraction() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_tellurc"))
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .args([
            "protocol",
            "playground/read_file.path",
            "playground/manifest.json",
            "show_file",
            "playground/protocol/read_failed.json",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        json["claim"],
        "possibilities_in_structural_abstraction_only"
    );
    assert_eq!(json["prefix_possible"], true);
    assert_eq!(json["next_symbols"].as_array().unwrap().len(), 2);
}

#[test]
fn abstract_language_inclusion_reports_shortest_completed_counterexamples() {
    let make = |body: &str| {
        compile(
            &format!("control main(bool flag) exits auto {{{body}}}"),
            m(),
        )
        .unwrap()
        .protocol("main", 200)
        .unwrap()
    };
    let narrow = make("exit done;");
    let wide = make("exit delta flag {true=>done {},false=>failed {}};");
    assert!(narrow.included_in(&wide, 100).unwrap().included);
    let difference = wide.included_in(&narrow, 100).unwrap();
    assert!(!difference.included);
    assert_eq!(difference.counterexample, Some(vec![exit("failed")]));
    assert!(
        !narrow
            .included_in(&make("emit(\"changed\");exit done;"), 100)
            .unwrap()
            .included
    );
    assert_eq!(
        narrow.included_in(&wide, 1).unwrap_err().code,
        "E_PROTOCOL_LIMIT"
    );
}
#[test]
fn inclusion_handles_nondeterministic_epsilon_choices_and_cycles() {
    let a=compile("control main(bool flag) exits done {loop repeat() {delta flag {true=>more,false=>end} [more]:emit(\"tick\");again repeat();[end]:exit done;}}",m()).unwrap().protocol("main",100).unwrap();
    let b=compile("control main(bool flag) exits done {loop repeat() {delta flag {true=>end,false=>more} [more]:emit(\"different text\");again repeat();[end]:exit done;}}",m()).unwrap().protocol("main",100).unwrap();
    assert!(a.included_in(&b, 100).unwrap().included);
    assert!(b.included_in(&a, 100).unwrap().included);
    let c = compile("control main() exits done {emit(\"once\");exit done;}", m())
        .unwrap()
        .protocol("main", 100)
        .unwrap();
    assert!(c.included_in(&a, 100).unwrap().included);
    assert_eq!(
        a.included_in(&c, 100).unwrap().counterexample,
        Some(vec![exit("done")])
    );
}
#[test]
fn protocol_equality_does_not_establish_behavioral_or_divergence_equivalence() {
    let a = compile(include_str!("../playground/algebra/original.tellur"), m()).unwrap();
    let b = compile(include_str!("../playground/algebra/reordered.tellur"), m()).unwrap();
    let pa = a.protocol("main", 1000).unwrap();
    let pb = b.protocol("main", 1000).unwrap();
    assert!(
        pa.included_in(&pb, 1000).unwrap().included && pb.included_in(&pa, 1000).unwrap().included
    );
    let s = Scenario {
        entry: "main".into(),
        arguments: vec![Value::Bool(true)],
        calls: vec![],
    };
    assert_ne!(a.run(&s).unwrap(), b.run(&s).unwrap());
    let make = |body: &str| {
        compile(&format!("control main() exits done {{{body}}}"), m())
            .unwrap()
            .protocol("main", 100)
            .unwrap()
    };
    let silent = make("loop repeat() {again repeat();}");
    let emitting = make("loop repeat() {emit(\"tick\");again repeat();}");
    // Both have empty completed languages; their infinite behavior is different.
    assert!(
        silent.included_in(&emitting, 100).unwrap().included
            && emitting.included_in(&silent, 100).unwrap().included
    );
    assert_ne!(
        silent.residual(&[]).may_silently_diverge,
        emitting.residual(&[]).may_silently_diverge
    );
}
#[test]
fn protocol_comparison_cli_returns_counterexample_and_failure_status() {
    let dir = std::env::temp_dir().join(format!("path-protocol-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let before = dir.join("before.path");
    let after = dir.join("after.path");
    std::fs::write(&before, "control main() exits done {exit done;}").unwrap();
    std::fs::write(
        &after,
        "control main() exits done {emit(\"extra\");exit done;}",
    )
    .unwrap();
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_tellurc"))
        .arg("protocol-compare")
        .arg(before)
        .arg(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("playground/manifest.json"))
        .arg(after)
        .arg("main")
        .output()
        .unwrap();
    assert!(!out.status.success());
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["before_in_after"]["included"], false);
    assert_eq!(
        json["before_in_after"]["counterexample"],
        serde_json::json!([{"kind":"exit","outcome":"done"}])
    );
    std::fs::remove_dir_all(dir).unwrap();
}
