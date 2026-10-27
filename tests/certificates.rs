use tellur::{
    CheckedProgram,
    certificate::ExpansionArtifact,
    compile,
    ir::{Action, Expr, Guard, Instruction, Value},
    manifest::Manifest,
    runtime::Scenario,
};
fn m() -> Manifest {
    Manifest::from_json(include_str!("../playground/manifest.json")).unwrap()
}
const SOURCE: &str = r#"
control main(bool flag) exits done {string text} {
    auto first=capture outer(flag);
    emit("between");
    exit delta first {done {auto text}=>delta outer(flag) {done {auto other}=>done {concat(text,other)}}};
}
control outer(bool flag) exits done {string text} {
    loop attempt(bool value=flag) {
        delta value {true=>retry,false=>finish}
        [retry]:again attempt(false);
        [finish]:forward inner(value);
    }
}
control inner(bool flag) exits done {string text} {
    emit("inner");
    exit delta flag {true=>done {"a"},false=>done {"b"}};
}
"#;
fn p() -> CheckedProgram {
    compile(SOURCE, m()).unwrap()
}
fn visit(code: &mut [Instruction], f: &mut impl FnMut(&mut Instruction)) {
    for i in code {
        f(i);
        if let Action::Loop { body, .. } = &mut i.action {
            visit(body, f);
        }
    }
}
fn rejected(p: &CheckedProgram, a: &ExpansionArtifact) {
    assert_eq!(
        p.accept_expansion(a).unwrap_err().code,
        "E_EXPANSION_CERTIFICATE"
    );
}
#[test]
fn nested_loop_expansion_roundtrips_and_preserves_repeated_invocations() {
    let p = p();
    let a = p.expansion_artifact("main").unwrap();
    assert_eq!(a.calls.len(), 4);
    assert_eq!(a.calls.iter().filter(|r| r.callee == "inner").count(), 2);
    let encoded = serde_json::to_string(&a).unwrap();
    let decoded: ExpansionArtifact = serde_json::from_str(&encoded).unwrap();
    let expanded = p.accept_expansion(&decoded).unwrap();
    for flag in [false, true] {
        let s = Scenario {
            entry: "main".into(),
            arguments: vec![Value::Bool(flag)],
            calls: vec![],
        };
        assert_eq!(p.run(&s).unwrap(), expanded.run(&s).unwrap());
        assert_eq!(p.run(&s).unwrap().fields, vec![Value::String("bb".into())]);
    }
    // Certificate order has no semantics; instruction order does.
    let mut reversed = a.clone();
    reversed.calls.reverse();
    p.accept_expansion(&reversed).unwrap();
    let again = expanded.expansion_artifact("main").unwrap();
    assert!(again.calls.is_empty());
    assert_eq!(again.region, a.region);
}
#[test]
fn identity_correspondences_must_be_total_fresh_and_used_exactly_once() {
    let p = p();
    let base = p.expansion_artifact("main").unwrap();
    for mutation in 0..11 {
        let mut a = base.clone();
        match mutation {
            0 => {
                a.calls.pop();
            }
            1 => a.calls.push(a.calls[0].clone()),
            2 => {
                a.calls[0].slots.pop_first();
            }
            3 => {
                let id = *a.calls[0].slots.values().next().unwrap();
                *a.calls[0].instructions.values_mut().next().unwrap() = id;
            }
            4 => {
                *a.calls[0].slots.values_mut().next().unwrap() = 0;
            }
            5 => a.next_id += 1,
            6 => a.calls[0].call = usize::MAX,
            7 => a.calls[0].callee = "missing".into(),
            8 => a.calls[0].parameters.clear(),
            9 => a.version = 2,
            10 => {
                a.calls[0].instructions.insert(usize::MAX, usize::MAX);
            }
            _ => unreachable!(),
        }
        rejected(&p, &a);
    }
}
#[test]
fn validation_rejects_effect_changes_erasure_duplication_and_reordering() {
    let p = p();
    let base = p.expansion_artifact("main").unwrap();
    for mutation in 0..4 {
        let mut a = base.clone();
        match mutation {
            0 => visit(&mut a.region.code, &mut |i| {
                if let Action::Emit(e) = &mut i.action {
                    *e = Expr::Value(Value::String("changed".into()));
                }
            }),
            1 => {
                a.region.code.pop();
            }
            2 => a.region.code.push(a.region.code[0].clone()),
            3 => a.region.code.reverse(),
            _ => unreachable!(),
        }
        rejected(&p, &a);
    }
    let changed = compile(&SOURCE.replace("between", "changed"), m()).unwrap();
    rejected(&changed, &base);
}
#[test]
fn completion_payload_release_unwind_guard_and_provenance_are_checked() {
    let p = p();
    let base = p.expansion_artifact("main").unwrap();
    for mutation in 0..7 {
        let mut a = base.clone();
        let mut count = 0;
        visit(&mut a.region.code, &mut |i| {
            if let Action::Complete {
                bindings,
                release,
                unwind,
                atom,
            } = &mut i.action
            {
                count += 1;
                match mutation {
                    0 => bindings.clear(),
                    1 => release.clear(),
                    2 => *unwind = Some(usize::MAX),
                    3 => *atom = usize::MAX,
                    4 => i.guard = Guard::True,
                    5 => i.provenance.calls.clear(),
                    6 => i.provenance.definition = usize::MAX,
                    _ => unreachable!(),
                }
            }
        });
        assert!(count > 0);
        rejected(&p, &a);
    }
}
#[test]
fn loop_reset_sets_and_parameter_order_are_checked() {
    let p = p();
    let base = p.expansion_artifact("main").unwrap();
    for mutation in 0..3 {
        let mut a = base.clone();
        visit(&mut a.region.code, &mut |i| match &mut i.action {
            Action::Loop {
                local_slots,
                local_atoms,
                ..
            } => match mutation {
                0 => local_slots.clear(),
                1 => local_atoms.clear(),
                _ => (),
            },
            Action::Bind(_, value) if mutation == 2 => *value = Expr::Value(Value::Bool(true)),
            _ => (),
        });
        rejected(&p, &a);
    }
}
#[test]
fn equivalent_boolean_guard_forms_are_allowed_without_weakening_selection() {
    let p = p();
    let mut a = p.expansion_artifact("main").unwrap();
    visit(&mut a.region.code, &mut |i| {
        i.guard = Guard::Or(Box::new(i.guard.clone()), Box::new(i.guard.clone()))
    });
    p.accept_expansion(&a).unwrap();
}
#[test]
fn public_boundary_borrow_summary_and_slot_metadata_are_checked() {
    let p=compile("control main(&string text) exits auto {forward view(text);} control view(&string text) exits auto {exit found {text};}",m()).unwrap();
    let base = p.expansion_artifact("main").unwrap();
    let json = serde_json::to_string(&base).unwrap();
    p.accept_expansion(&serde_json::from_str(&json).unwrap())
        .unwrap();
    for mutation in 0..5 {
        let mut a = base.clone();
        match mutation {
            0 => a.region.borrow_returns.clear(),
            1 => a.region.exits[0].name = "changed".into(),
            2 => a.region.parameters[0].ty = "&U32".into(),
            3 => a.region.slots.values_mut().next().unwrap().name = "erased_name".into(),
            4 => a.region.name = "other".into(),
            _ => unreachable!(),
        }
        rejected(&p, &a);
    }
}
#[test]
fn cli_verifies_serialized_artifact_and_rejects_tampering() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_tellurc"))
        .current_dir(root)
        .args([
            "certify-expansion",
            "playground/forwarding.path",
            "playground/manifest.json",
            "main",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut artifact: ExpansionArtifact = serde_json::from_slice(&output.stdout).unwrap();
    let file = std::env::temp_dir().join(format!("path-certificate-{}.json", std::process::id()));
    for valid in [true, false] {
        if !valid {
            artifact.region.code.pop();
        }
        std::fs::write(&file, serde_json::to_vec(&artifact).unwrap()).unwrap();
        let result = std::process::Command::new(env!("CARGO_BIN_EXE_tellurc"))
            .current_dir(root)
            .args([
                "verify-expansion",
                "playground/forwarding.path",
                "playground/manifest.json",
            ])
            .arg(&file)
            .output()
            .unwrap();
        assert_eq!(
            result.status.success(),
            valid,
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        if !valid {
            assert!(String::from_utf8_lossy(&result.stderr).contains("E_EXPANSION_CERTIFICATE"));
        }
    }
    std::fs::remove_file(file).unwrap();
}

#[test]
fn staged_expansion_keeps_existing_completion_and_provenance_mappings() {
    let p = p();
    let staged = p.expanded("outer").unwrap();
    let artifact = staged.expansion_artifact("main").unwrap();
    let expanded = staged.accept_expansion(&artifact).unwrap();
    assert_eq!(artifact.calls.len(), 2);
    let scenario = Scenario {
        entry: "main".into(),
        arguments: vec![Value::Bool(true)],
        calls: vec![],
    };
    assert_eq!(p.run(&scenario).unwrap(), expanded.run(&scenario).unwrap());
}
