use tellur::{CheckedProgram, compile, manifest::Manifest, runtime::Scenario};

const SOURCE: &str = include_str!("../playground/guarded_task_captures.tellur");

fn check(source: &str) -> CheckedProgram {
    let manifest = Manifest::from_json(include_str!("../playground/manifest.json")).unwrap();
    compile(source, manifest).unwrap()
}

fn run(source: &str) -> CheckedProgram {
    let program = check(source);
    let scenario = Scenario {
        entry: "main".into(),
        arguments: vec![],
        calls: vec![],
    };
    assert_eq!(
        program.run(&scenario).unwrap(),
        program.expanded("startup").unwrap().run(&scenario).unwrap()
    );
    program
}

fn rejects(source: &str, code: &str) {
    let manifest = Manifest::from_json(include_str!("../playground/manifest.json")).unwrap();
    let error = compile(source, manifest).unwrap_err();
    assert_eq!(error.code, code, "{error}\n{source}");
}

#[test]
fn detached_exclusive_capture_keeps_call_origin_guards_through_move_and_result() {
    let program = run(SOURCE);
    let detach = program.regions().find(|r| r.name == "detach").unwrap();
    let sources = &detach.borrow_returns[&("ready".into(), 0)];
    assert!(sources.iter().any(|s| s.guard && s.parameter == 0));
    assert!(sources.iter().any(|s| !s.guard && s.contained.is_some()));
    let direct = SOURCE.replace(
        "    delta detach(carrier) {\n        ready { auto detached } => borrowed\n    }\n\n    [borrowed]:",
        "    ref mutable string detached = borrow mutable deref carrier.text;",
    );
    assert_ne!(direct, SOURCE);
    run(&direct);
    for source in [SOURCE, direct.as_str()] {
        for marker in ["auto moved = move pending;", "deref value = \"after\";"] {
            for action in [
                "drop(saved);",
                "owner = \"too soon\";",
                "deref carrier.text = \"too soon\";",
                "deref detached = \"too soon\";",
            ] {
                rejects(
                    &source.replacen(marker, &format!("{action}\n{marker}"), 1),
                    "E_LOAN",
                );
            }
        }
        let transferred = source.replace("defer lend(detached)", "defer lend(move detached)");
        rejects(&transferred, "E_MOVED");
        run(&transferred.replace(
            "        deref detached = \"available after result\";\n        emit(detached);\n",
            "",
        ));
    }
}

#[test]
fn shared_reborrow_of_exclusive_contents_keeps_its_carrier_guard() {
    let source = SOURCE
        .replace("borrow mutable deref input.text", "borrow deref input.text")
        .replace("defer lend(detached)", "defer inspect(detached)")
        .replace("        deref value = \"after\";\n", "")
        .replace("        deref detached = \"available after result\";\n", "");
    run(&source);
    for marker in ["auto moved = move pending;", "emit(value);"] {
        for action in ["drop(saved);", "deref carrier.text = \"too soon\";"] {
            rejects(
                &source.replacen(marker, &format!("{action}\n{marker}"), 1),
                "E_LOAN",
            );
        }
    }
    rejects(
        &source.replacen(
            "emit(value);",
            "deref value = \"not exclusive\";emit(value);",
            1,
        ),
        "E_MUTABILITY",
    );
}

#[test]
fn copied_shared_contents_release_holder_but_retain_the_selected_owner() {
    // The runnable example drops the shared carrier before task creation.
    // Its copied reference keeps only the external owner alive through await.
    run(SOURCE);
    let start = SOURCE.find("async control shared()").unwrap();
    let (before, shared) = SOURCE.split_at(start);
    for marker in ["auto moved = move pending;", "emit(value);"] {
        rejects(
            &format!(
                "{before}{}",
                shared.replacen(marker, &format!("owner = \"too soon\";\n{marker}"), 1)
            ),
            "E_LOAN",
        );
    }
}
