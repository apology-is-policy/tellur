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
fn nested_scopes_recover_without_affecting_independent_controls() {
    let batch = errors(
        "control one() exits done {[entry] over &: {u32 bad=true;string other=7;}[entry]:exit done;} control two() exits done {u32 bad=true;string other=7;exit done;}",
    );
    assert_eq!(batch.errors.len(), 4, "{batch}");
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

fn codes(source: &str, expected: &[&str]) {
    let batch = errors(source);
    assert_eq!(
        batch.errors.iter().map(|e| e.code).collect::<Vec<_>>(),
        expected,
        "{batch}"
    );
}

#[test]
fn flat_paths_report_independent_errors_after_each_others_exits() {
    codes(
        r#"control main(bool flag) exits done {
        delta flag {true=>yes,false=>no}
        [yes]: u32 first=true; u32 dependent=first; exit done;
        [no]: string second=7; emit(second); exit done;
    }"#,
        &["E_TYPE", "E_TYPE"],
    );
    codes(
        "control main(bool flag) exits done {delta flag {true=>yes,false=>no}[yes]:u32 bad=true;exit done;path later=no;[later]:string other=7;exit done;}",
        &["E_TYPE", "E_TYPE"],
    );
}

#[test]
fn grouped_and_lexical_scopes_recover_and_restore_their_continuations() {
    for scope in ["[yes]:", "[yes] over &:"] {
        codes(
            &format!(
                r#"control main(bool flag) exits done {{
            delta flag {{true=>yes,false=>no}}
            {scope} {{ u32 first=true; string second=7; }}
            [yes]: bool third=8; exit done;
            [no]: string fourth=false; exit done;
        }}"#
            ),
            &["E_TYPE", "E_TYPE", "E_TYPE", "E_TYPE"],
        );
    }
}

#[test]
fn statement_arm_bodies_and_direct_arm_exits_recover_separately() {
    codes(
        r#"control main(bool flag) exits done {
        delta flag {
            true=>{u32 first=true; string second=7; exit done;},
            false=>{u32 third=false; string fourth=8; exit done;}
        }
    }"#,
        &["E_TYPE", "E_TYPE", "E_TYPE", "E_TYPE"],
    );
    codes(
        r#"control main(bool flag) exits ready {u32 number} {
        delta flag {true=>exit ready {"wrong"},false=>exit ready {true}}
    }"#,
        &["E_TYPE", "E_TYPE"],
    );
}

#[test]
fn another_path_can_introduce_a_new_decision_after_a_failure() {
    codes(
        r#"control main(bool flag,bool nested) exits done {
        delta flag {true=>yes,false=>no}
        [yes]: u32 first=true; exit done;
        [no]: delta nested {true=>left,false=>right}
        [left]: string second=7; exit done;
        [right]: bool third=8; exit done;
    }"#,
        &["E_TYPE", "E_TYPE", "E_TYPE"],
    );
}

#[test]
fn reference_taint_does_not_hide_an_independent_paths_loan_error() {
    let batch = errors(
        r#"control main(bool flag) exits done {
        mutable string owner="before";
        auto alias=borrow owner;
        delta flag {true=>yes,false=>no}
        [yes]: u32 bad=alias; exit done;
        [no]: owner="after"; emit(alias); exit done;
    }"#,
    );
    assert_eq!(
        batch.errors.iter().map(|e| e.code).collect::<Vec<_>>(),
        ["E_TYPE", "E_LOAN"],
        "{batch}"
    );
    assert!(batch.errors[1].witness.is_some());
    assert!(format!("{:?}", batch.errors[1].witness).contains("no"));
}

#[test]
fn selectors_joining_a_stalled_path_are_not_narrowed_to_the_other_path() {
    codes(
        r#"control main(bool flag) exits done {
        delta flag {true=>yes,false=>no}
        [yes]: u32 bad=true; exit done;
        [yes|no]: string joined=7;
        [no]: bool later=8; exit done;
    }"#,
        &["E_TYPE"],
    );
}

#[test]
fn skipped_decisions_stall_only_their_known_history_until_an_unknown_selector() {
    codes(
        r#"control main(bool flag) exits done {
        delta flag {true=>yes,false=>no}
        [yes]: u32 bad=true; delta true {true=>missing_left,false=>missing_right}
        [no]: string other=7; exit done;
        [missing_left]: emit(unknown_payload);
    }"#,
        &["E_TYPE", "E_TYPE"],
    );
    codes(
        r#"control main(bool flag) exits done {
        delta flag {true=>yes,false=>no}
        [yes]: {
            u32 bad=true;
            delta true {true=>missing_left,false=>missing_right}
            [missing_left]: emit(unknown_payload);
        }
        [no]: string other=7; exit done;
    }"#,
        &["E_TYPE", "E_TYPE"],
    );
}

#[test]
fn resource_checks_remain_real_on_clean_paths_and_are_suppressed_in_damaged_scopes() {
    codes(
        r#"control main(handle resource,bool flag) exits done {
        delta flag {true=>yes,false=>no}
        [yes]: u32 bad=true; exit done;
        [no]: exit done;
    }"#,
        &["E_TYPE", "E_RESOURCE"],
    );
    codes(
        r#"control main(handle resource) exits done {
        [entry] over &: {
            handle local=move resource;
            u32 first=true;
            string second=7;
        }
        [entry]: bool third=8; exit done;
    }"#,
        &["E_TYPE", "E_TYPE", "E_TYPE"],
    );
}

#[test]
fn loop_body_errors_do_not_create_backedge_or_coverage_cascades() {
    codes(
        r#"control main() exits done {
        loop work(bool flag=true) {
            delta flag {true=>yes,false=>no}
            [yes]: u32 first=true; string second=7; again work(false);
            [no]: bool third=8; exit done;
        }
    }"#,
        &["E_TYPE", "E_TYPE", "E_TYPE"],
    );
    codes(
        r#"control main() exits done {
        loop work(u32 count=0) {
            delta (count==0) {true=>yes,false=>no}
            [yes]: again work(true);
            [no]: string wrong=7; exit done;
        }
    }"#,
        &["E_TYPE", "E_TYPE"],
    );
}

#[test]
fn fault_contexts_and_inline_outcome_adapters_remain_recovery_barriers() {
    for body in [
        "fault boundary {u32 first=true;string second=7;exit done;} contingency [] {terminate process 1;}",
        "fault boundary {exit done;} contingency [] {u32 first=true;string second=7;terminate process 1;}",
        "exit delta true {true=>done {7},false=>done {true}};",
    ] {
        let source = format!(
            "control main() exits done {{{body}}} control other() exits done {{u32 bad=true;string second=7;exit done;}}"
        );
        let batch = errors(&source);
        assert_eq!(batch.errors.len(), 3, "{batch}");
    }
}

#[test]
fn generic_nested_bodies_keep_authored_diagnostics_and_one_global_budget() {
    codes(
        r#"control generic<t>(t input,bool flag) exits ready {t value} {
        delta flag {true=>yes,false=>no}
        [yes]: {u32 first=true;string second=7;exit ready {input};}
        [no]: {bool third=8;exit ready {input};}
    }"#,
        &["E_TYPE", "E_TYPE", "E_TYPE"],
    );
    let first = (0..20)
        .map(|i| format!("u32 first_{i}=true;\n"))
        .collect::<String>();
    let second = (0..20)
        .map(|i| format!("string second_{i}=7;\n"))
        .collect::<String>();
    let batch = errors(&format!(
        "control main(bool flag) exits done {{delta flag {{true=>yes,false=>no}}[yes]:{{{first}exit done;}}[no]:{{{second}exit done;}}}}"
    ));
    assert!(batch.limit_reached);
    assert_eq!(batch.errors.len(), 32);
    assert!(batch.errors.iter().all(|e| e.code == "E_TYPE"));
}

#[test]
fn constructor_branches_recover_without_publishing_partial_initialization() {
    codes(
        r#"struct message {
        string text;
        constructor(bool flag) {
            delta flag {true=>yes,false=>no}
            [yes]: {u32 first=true;self.text="yes";}
            [no]: {string second=7;self.text="no";}
        }
    }
    control caller() exits done {auto value=message(true);exit done;}
    "#,
        &["E_TYPE", "E_TYPE"],
    );
}

#[test]
fn clean_branch_fault_obligations_are_not_swallowed_by_an_enclosing_decision() {
    codes(
        r#"control danger() exits done faults {exit done;}
    control main(handle resource,bool flag) exits done faults {
        delta flag {
            true=>{u32 bad=true;exit done;},
            false=>{danger();exit done;}
        }
    }"#,
        &["E_TYPE", "E_FAULT_RESOURCE"],
    );
}
