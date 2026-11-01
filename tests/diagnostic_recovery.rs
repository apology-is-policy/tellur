use tellur::{Diagnostics, check_source, manifest::Manifest};

fn errors(source: &str) -> Diagnostics {
    check_source(
        source,
        Manifest::from_json(include_str!("../playground/manifest.json")).unwrap(),
    )
    .unwrap_err()
}

#[test]
fn independent_statements_report_together_and_dependencies_are_suppressed() {
    let source = "control broken() exits done {\n\
        u32 first=true;\n\
        emit(first);\n\
        string second=7;\n\
        u32 third=missing;\n\
        string dependent=third;\n\
        bool fourth=5;\n\
        exit done;\n}\n\
        control caller() exits done {forward broken();}\n\
        control separate() exits done {u32 other=false;exit done;}";
    let batch = errors(source);
    assert!(!batch.limit_reached);
    assert_eq!(
        batch.errors.iter().map(|e| e.span.line).collect::<Vec<_>>(),
        [2, 4, 5, 7, 11],
        "{batch}"
    );
    assert!(batch.errors.iter().all(|e| e.end.is_some()));
    assert_eq!(
        tellur::compile(
            source,
            Manifest::from_json(include_str!("../playground/manifest.json")).unwrap()
        )
        .unwrap_err(),
        batch.errors[0]
    );
}

#[test]
fn partially_moved_arguments_are_replayed_from_clean_ownership_state() {
    let batch = errors(
        r#"
control take(string text,u32 number) exits done {emit(borrow text);exit done;}
control main() exits done {
    string owner="before";
    take(move owner,true);
    emit(borrow owner);
    string independent=7;
    exit done;
}
"#,
    );
    assert_eq!(batch.errors.len(), 2, "{batch}");
    assert!(batch.errors.iter().all(|e| e.code == "E_TYPE"), "{batch}");
}

#[test]
fn borrowed_owners_and_nested_carriers_are_tainted_together() {
    for setup in [
        "auto alias=borrow owner;",
        "auto first=borrow owner; auto alias=first;",
        "auto alias=holder {text:borrow owner};",
    ] {
        let access = if setup.contains("holder") {
            "alias.text"
        } else {
            "alias"
        };
        let batch = errors(&format!(
            r#"
struct holder['a] {{ref string text <- 'a;}}
control main() exits done {{
    mutable string owner="before";
    {setup}
    u32 bad={access};
    owner="after";
    emit({access});
    string independent=7;
    exit done;
}}
"#
        ));
        assert_eq!(batch.errors.len(), 2, "{setup}: {batch}");
        assert!(batch.errors.iter().all(|e| e.code == "E_TYPE"), "{batch}");
    }
}

#[test]
fn unrelated_ownership_errors_keep_their_witnesses() {
    let batch = errors(
        r#"
control main() exits done {
    string owner="before";
    auto moved=move owner;
    u32 bad=true;
    emit(borrow owner);
    string other=7;
    exit done;
}
"#,
    );
    assert_eq!(
        batch.errors.iter().map(|e| e.code).collect::<Vec<_>>(),
        ["E_TYPE", "E_MOVED", "E_TYPE"],
        "{batch}"
    );
    assert!(batch.errors[1].witness.is_some());
}

#[test]
fn incomplete_resources_coverage_and_fault_obligations_do_not_cascade() {
    for source in [
        "control main(handle resource) exits done {u32 bad=true;string other=7;exit done;}",
        "control main() exits done {u32 bad=true;string other=7;}",
        "control danger() exits done faults {exit done;} control main(handle resource) exits done faults {u32 bad=true;danger();string other=7;exit done;}",
    ] {
        let batch = errors(source);
        assert!(batch.errors.iter().all(|e| e.code == "E_TYPE"), "{batch}");
        // A whole-state fault-obligation check stops the replay conservatively.
        assert_eq!(
            batch.errors.len(),
            if source.contains("danger") { 1 } else { 2 },
            "{batch}"
        );
    }
}

#[test]
fn structural_and_termination_boundaries_stop_the_replay() {
    for boundary in [
        "exit done;",
        "delta true {true=>yes,false=>no}",
        "[entry] over &: {exit done;}",
        "loop work {exit done;}",
    ] {
        let batch = errors(&format!(
            "control main() exits done {{u32 bad=true; {boundary} [entry]: string other=7;}}"
        ));
        assert_eq!(batch.errors.len(), 1, "{boundary}: {batch}");
        assert_eq!(batch.errors[0].code, "E_TYPE", "{batch}");
    }
    let batch = errors("control main() exits done {exit done;u32 one=1;string two=\"x\";}");
    assert_eq!(batch.errors.len(), 1, "{batch}");
    assert_eq!(batch.errors[0].code, "E_TERMINATED");
}

#[test]
fn nested_scopes_remain_first_error_without_affecting_independent_controls() {
    let batch = errors(
        "control one() exits done {[entry] over &: {u32 bad=true;string other=7;}[entry]:exit done;} control two() exits done {u32 bad=true;string other=7;exit done;}",
    );
    assert_eq!(batch.errors.len(), 3, "{batch}");
    assert!(batch.errors.iter().all(|e| e.code == "E_TYPE"));
}

#[test]
fn generic_and_constructor_bodies_use_the_same_bounded_recovery() {
    for source in [
        "control generic<t>(t input) exits ready {t value} {u32 bad=true;string other=7;exit ready {input};}",
        "struct message {string text;constructor() {u32 bad=true;string other=7;self.text=\"ok\";}}",
    ] {
        let batch = errors(source);
        assert_eq!(batch.errors.len(), 2, "{batch}");
        assert!(batch.errors.iter().all(|e| e.code == "E_TYPE"), "{batch}");
        assert!(!batch.to_string().contains("_path_poly_"));
    }
}

#[test]
fn body_and_global_budgets_are_shared() {
    let body = (0..40)
        .map(|i| format!("u32 bad_{i}=true;\n"))
        .collect::<String>();
    let batch = errors(&format!("control main() exits done {{\n{body}exit done;}}"));
    assert_eq!(batch.errors.len(), tellur::diagnostics::MAX_DIAGNOSTICS);
    assert!(batch.limit_reached);
    assert_eq!(batch.errors.last().unwrap().span.line, 33);
}
