use super::*;
use serde_json::Value as Json;

#[test]
fn compiled_selection_rejects_before_callback_moves_effects_and_pending_delivery() {
    compare_selection(
        "loops",
        include_str!("../../tests/fixtures/native_resume_loops.tellur"),
        &["false"],
        &[
            "Skip",
            "Local",
            "Effect",
            "Run",
            "Defer",
            "AwaitTask",
            "Loop",
            "Restart",
            "Return",
            "Delivery",
        ],
    );
}

#[test]
fn compiled_storage_resolution_covers_constructors_and_borrowed_result_delivery() {
    compare_selection(
        "constructors",
        include_str!("../../tests/fixtures/native_resume.tellur"),
        &["false"],
        &[
            "Skip",
            "Local",
            "Effect",
            "Call",
            "Run",
            "Await",
            "Defer",
            "AwaitTask",
            "Return",
            "Delivery",
        ],
    );
}

#[test]
fn compiled_carrier_dependencies_survive_join_until_detached_result_delivery() {
    compare_selection(
        "carriers",
        include_str!("../../playground/carrier_tasks.tellur"),
        &[],
        &[
            "Local",
            "Effect",
            "Run",
            "Defer",
            "AwaitTask",
            "Return",
            "Delivery",
        ],
    );
}

#[test]
fn compiled_detached_captures_preserve_checked_guard_semantics() {
    let input = include_str!("../../playground/guarded_task_captures.tellur");
    let observed = compare_selection(
        "detached-guards",
        input,
        &[],
        &[
            "Local",
            "Effect",
            "Run",
            "Await",
            "Defer",
            "AwaitTask",
            "Return",
            "Delivery",
        ],
    );
    let reference = crate::compile(
        input,
        crate::manifest::Manifest::from_json(include_str!("../../playground/manifest.json"))
            .unwrap(),
    )
    .unwrap()
    .run(&crate::runtime::Scenario {
        entry: "main".into(),
        arguments: vec![],
        calls: vec![],
    })
    .unwrap();
    assert_eq!(observed, serde_json::to_value(reference).unwrap());
}

#[test]
fn compiled_aliased_carrier_capture_retains_complete_broad_ancestry() {
    let input = include_str!("../../playground/aliased_carrier_tasks.tellur");
    let observed = compare_selection(
        "aliased-carriers",
        input,
        &[],
        &[
            "Local",
            "Effect",
            "Run",
            "Defer",
            "AwaitTask",
            "Return",
            "Delivery",
        ],
    );
    let p = crate::compile(
        input,
        crate::manifest::Manifest::from_json(include_str!("../../playground/manifest.json"))
            .unwrap(),
    )
    .unwrap();
    let reference = p
        .run(&crate::runtime::Scenario {
            entry: "main".into(),
            arguments: vec![],
            calls: vec![],
        })
        .unwrap();
    assert_eq!(observed, serde_json::to_value(reference).unwrap());
}

#[test]
fn compiled_local_carrier_capture_retains_complete_stored_reference_closure() {
    compare_selection(
        "local-carriers",
        include_str!("../../playground/local_carrier_tasks.tellur"),
        &[],
        &[
            "Local",
            "Effect",
            "Run",
            "Defer",
            "AwaitTask",
            "Return",
            "Delivery",
        ],
    );
}

#[test]
fn compiled_local_guarded_captures_remain_complete_through_result_delivery() {
    let input = include_str!("../../playground/local_guarded_tasks.tellur");
    let observed = compare_selection(
        "local-guards",
        input,
        &[],
        &[
            "Local",
            "Effect",
            "Run",
            "Defer",
            "AwaitTask",
            "Return",
            "Delivery",
        ],
    );
    let p = crate::compile(
        input,
        crate::manifest::Manifest::from_json(include_str!("../../playground/manifest.json"))
            .unwrap(),
    )
    .unwrap();
    let reference = p
        .run(&crate::runtime::Scenario {
            entry: "main".into(),
            arguments: vec![],
            calls: vec![],
        })
        .unwrap();
    assert_eq!(observed, serde_json::to_value(reference).unwrap());
}

#[test]
fn compiled_incoming_shared_leaves_preserve_complete_capture_and_delivery() {
    let input = include_str!("../../playground/incoming_leaf_tasks.tellur");
    let observed = compare_selection(
        "incoming-leaves",
        input,
        &[],
        &[
            "Local",
            "Effect",
            "Run",
            "Defer",
            "AwaitTask",
            "Return",
            "Delivery",
        ],
    );
    let p = crate::compile(
        input,
        crate::manifest::Manifest::from_json(include_str!("../../playground/manifest.json"))
            .unwrap(),
    )
    .unwrap();
    let reference = p
        .run(&crate::runtime::Scenario {
            entry: "main".into(),
            arguments: vec![],
            calls: vec![],
        })
        .unwrap();
    assert_eq!(observed, serde_json::to_value(reference).unwrap());
    let partial = compare_selection(
        "incoming-leaves-partial",
        input,
        &[],
        &[
            "Local",
            "Effect",
            "Run",
            "Defer",
            "AwaitTask",
            "Return",
            "Delivery",
        ],
    );
    assert_eq!(
        observed, partial,
        "metadata coverage cannot change source observations"
    );
}

#[test]
fn compiled_guarded_input_captures_keep_complete_holders_and_partial_callers() {
    let input = include_str!("../../playground/guarded_input_tasks.tellur");
    let transitions = &[
        "Local",
        "Effect",
        "Run",
        "Defer",
        "AwaitTask",
        "Return",
        "Delivery",
    ];
    let observed = compare_selection("guarded-input", input, &[], transitions);
    let partial = compare_selection("guarded-input-partial", input, &[], transitions);
    let p = crate::compile(
        input,
        crate::manifest::Manifest::from_json(include_str!("../../playground/manifest.json"))
            .unwrap(),
    )
    .unwrap();
    let reference = p
        .run(&crate::runtime::Scenario {
            entry: "main".into(),
            arguments: vec![],
            calls: vec![],
        })
        .unwrap();
    assert_eq!(observed, serde_json::to_value(reference).unwrap());
    assert_eq!(
        observed, partial,
        "partial evidence must not change source observations"
    );
}

fn compare_selection(label: &str, input: &str, args: &[&str], expected: &[&str]) -> Json {
    let root =
        std::env::temp_dir().join(format!("tellur-selection-{label}-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let original = crate::compile(
        input,
        crate::manifest::Manifest::from_json(include_str!("../../playground/manifest.json"))
            .unwrap(),
    )
    .unwrap()
    .with_entry_exit_codes("main", BTreeMap::from([("done".into(), 0)]))
    .unwrap();
    let expanded = original
        .expanded("startup")
        .unwrap()
        .expanded("main")
        .unwrap();
    let mut baseline = None;
    for (is_expanded, program) in [(false, original), (true, expanded)] {
        // Expansion can inline the constructor; its explicit destination is
        // exercised by the unexpanded run, with equivalent expanded observations.
        let expected: Vec<_> = expected
            .iter()
            .copied()
            .filter(|kind| !is_expanded || *kind != "Call")
            .collect();
        let entry = program.regions().position(|r| r.name == "main").unwrap();
        let normal = format!(
            "fn entry_{entry}(host:&mut Host,args:Vec<V>)->R<Completion>{{drive(host,args,&MODEL_{entry})}}"
        );
        let expect_guards = matches!(label, "detached-guards" | "local-guards" | "guarded-input");
        let expect_complete = matches!(
            label,
            "loops"
                | "local-guards"
                | "local-carriers"
                | "aliased-carriers"
                | "incoming-leaves"
                | "guarded-input"
        );
        let replacement = format!(
            r#"fn entry_{entry}(host:&mut Host,args:Vec<V>)->R<Completion>{{
            let quotas=std::env::var("TELLUR_NATIVE_MODEL_QUOTAS").unwrap().split(',').map(|s|s.parse::<usize>().unwrap()).collect::<Vec<_>>();
            drive_selection(host,&MODEL_{entry},args,std::env::var("TELLUR_SELECTION_PROBES").unwrap()=="true",&quotas,&{expected:?},{expect_guards},{expect_complete})
        }}"#
        );
        let generated = source(&program, "main").unwrap();
        let generated = if matches!(label, "local-carriers" | "aliased-carriers") {
            let call = "execution::reference_evidence::inherit_checked(v,reference_value)";
            assert!(
                generated.contains(call),
                "input field transport must be exercised"
            );
            generated.replace(call, r#"{
                let before=reference_value.clone();
                let copied=execution::reference_evidence::inherit_checked(v,reference_value);
                assert_eq!(before,copied);
                let V::Reference{evidence,..}=&copied else {panic!("reference expected")};
                assert!(matches!(&evidence.origins,execution::reference_evidence::Origins::Complete(o) if !o.is_empty()), "incoming complete leaf was downgraded");
                copied
            }"#)
        } else {
            generated
        };
        let generated = if matches!(label, "incoming-leaves-partial" | "guarded-input-partial") {
            generated.replace("execution::reference_evidence::inherit_checked(v,reference_value)",r#"{
                let mut incoming=reference_value;
                if let V::Reference{evidence,..}=&mut incoming {
                    if let execution::reference_evidence::Origins::Complete(origins)=&evidence.origins {
                        evidence.origins=execution::reference_evidence::Origins::Partial(origins.clone());
                    }
                }
                execution::reference_evidence::inherit_checked(v,incoming)
            }"#)
        } else {
            generated
        };
        assert_eq!(generated.matches(&normal).count(), 1);
        let generated = generated.replacen(&normal, &replacement, 1).replacen(
            CONTEXTS,
            &format!("{CONTEXTS}\n{}", include_str!("selection_model.rs")),
            1,
        );
        let generated = if matches!(label, "incoming-leaves" | "guarded-input") {
            generated.replace("let dependencies = c.dependencies(&selected)?;", r#"
                let dependencies = c.dependencies(&selected)?;
                assert!(matches!(dependencies, Dependencies::Complete {..}), "every incoming leaf capture must remain complete: {dependencies:?}");
            "#).replace("p.dependencies.require_live(&c.values)?;", r#"
                p.dependencies.require_live(&c.values)?;
                assert!(matches!(p.dependencies, Dependencies::Complete {..}), "incoming leaf pending delivery must remain complete");
            "#)
        } else {
            generated
        };
        let generated = if matches!(label, "incoming-leaves-partial" | "guarded-input-partial") {
            generated.replace("let dependencies = c.dependencies(&selected)?;",r#"
                let dependencies = c.dependencies(&selected)?;
                if c.parked[&selected.id].name == "leaf" {
                    assert!(matches!(dependencies,Dependencies::Partial {..}),"partial caller evidence cannot be promoted");
                }
            "#)
        } else {
            generated
        };
        let generated = if matches!(label, "local-carriers" | "aliased-carriers") {
            generated.replace("let mut checked = 0;", "let mut checked = 0; let mut holder_snapshots = 0;")
                .replace("let dependencies = c.dependencies(&selected)?;", r#"
                let dependencies = c.dependencies(&selected)?;
                if let Some(a)=m.stack.last() && a.control.name=="inspect" {
                    let holder=c.values[a.base()+a.control.parameters[0]].as_ref().unwrap();
                    let origins=Dependencies::holder_origins(&c.values,holder);
                    let super::reference_evidence::Origins::Complete(entries)=origins else {panic!("complete holder snapshot expected")};
                    assert!(entries.len()>=3 && entries.iter().any(|o|o.guard && o.opaque));
                    holder_snapshots+=1;
                }
                "#).replace("assert!(c.host.tasks.is_empty() && c.waits.is_empty() && c.entry.is_none());",
                    "assert!(c.host.tasks.is_empty() && c.waits.is_empty() && c.entry.is_none()); if probes {assert!(holder_snapshots>0);}")
        } else {
            generated
        };
        for opt in [0, 2] {
            let binary = root.join(format!("selection-{is_expanded}-{opt}"));
            let project = program.native_project("main", &binary).unwrap();
            fs::write(&project.2, &generated).unwrap();
            program.finish_native_build(project, opt).unwrap();
            for probes in [false, true] {
                for quotas in ["1", "0,3,1"] {
                    let output = Command::new(&binary)
                        .args(["--json", "--fuel", "1000", "--"])
                        .args(args)
                        .env("TELLUR_SELECTION_PROBES", probes.to_string())
                        .env("TELLUR_NATIVE_MODEL_QUOTAS", quotas)
                        .output()
                        .unwrap();
                    assert!(
                        output.status.success(),
                        "expanded={is_expanded} opt={opt} probes={probes} quotas={quotas}: {}",
                        String::from_utf8_lossy(&output.stderr)
                    );
                    let observed: Json = serde_json::from_slice(&output.stdout).unwrap();
                    if let Some(baseline) = &baseline {
                        assert_eq!(&observed, baseline);
                    } else {
                        baseline = Some(observed);
                    }
                }
            }
        }
    }
    fs::remove_dir_all(root).unwrap();
    baseline.unwrap()
}
