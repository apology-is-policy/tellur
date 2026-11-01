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
fn explicit_stored_adapters_recover_independent_arms_and_retire_the_result() {
    codes(
        "outcome result {ready {u32 value}} control choose(bool flag) exits done {result saved=delta flag {true=>ready {true},false=>ready {\"bad\"}};emit(saved);string hidden=7;exit done;}",
        &["E_TYPE", "E_TYPE"],
    );
    codes(
        "outcome result {ready {u32 value},missing} control choose(bool flag) exits done {result saved=delta flag {true=>unknown {},false=>ready {}};exit saved;}",
        &["E_OUTCOME", "E_ARITY"],
    );
    // The target type is explicit even when the enclosing control infers exits.
    codes(
        "outcome result {ready {u32 value}} control choose(bool flag) exits auto {result saved=delta flag {true=>ready {true},false=>ready {\"bad\"}};exit saved;}",
        &["E_TYPE", "E_TYPE"],
    );
}

#[test]
fn explicit_stored_adapter_recovery_preserves_nested_and_outer_domains() {
    codes(
        r#"outcome result {ready {u32 value}}
        control choose(bool flag,bool other) exits done {
            delta flag {true=>selected,false=>separate}
            [selected]: result saved=delta other {
                true=>delta flag {true=>ready {true},false=>ready {"bad"}},
                false=>ready {false}
            };
            emit(saved);
            [separate]: string fourth=7;exit done;
            [entry]: bool hidden=8;
        }"#,
        &["E_TYPE", "E_TYPE", "E_TYPE", "E_TYPE"],
    );
    // Restoring the parent must preserve an already completed disjoint history.
    codes(
        r#"outcome result {ready {u32 value}}
        control choose(bool flag) exits done {
            delta flag {true=>finished,false=>pending}
            [finished]: exit done;
            [pending]: result saved=delta flag {true=>ready {true},false=>ready {"bad"}};
            [entry]: string hidden=7;
        }"#,
        &["E_TYPE", "E_TYPE"],
    );
}

#[test]
fn explicit_stored_adapter_recovery_replays_ownership_and_borrows() {
    codes(
        r#"outcome result {ready {handle owned,u32 value}}
        control choose(bool flag,handle resource) exits done {
            result saved=delta flag {true=>ready {move resource,true},false=>ready {move resource,"bad"}};
            exit done;
        }"#,
        &["E_TYPE", "E_TYPE"],
    );
    codes(
        r#"outcome result['a] {ready {ref string value <- 'a,u32 count}}
        control choose(bool flag,ref string input) exits done {
            result saved=delta flag {true=>ready {input,true},false=>ready {input,"bad"}};
            exit done;
        }"#,
        &["E_TYPE", "E_TYPE"],
    );
    // An omitted arm's local owner cannot create a completion cascade; clean
    // independent arms still have to complete their own payload resources.
    codes(
        r#"outcome result {ready {u32 value}}
        variant input {left {handle owned},right {handle owned}}
        control choose(input value) exits done {
            result saved=delta value {left {auto owned}=>ready {true},right {auto owned}=>ready {1}};
            exit done;
        }"#,
        &["E_TYPE", "E_RESOURCE"],
    );
}

#[test]
fn explicit_stored_adapter_recovery_respects_scope_loop_and_fault_barriers() {
    codes(
        r#"outcome result['a] {ready {ref string value <- 'a,u32 count}}
        control choose(bool flag) exits done {
            mutable string owner="before";
            delta flag {true=>selected,false=>separate}
            [selected] over &: {
                result saved=delta flag {
                    true=>ready {borrow owner,1},false=>ready {borrow owner,true}
                };
                emit(saved);
            }
            [separate]: owner="after";u32 second=false;exit done;
            [entry]: string hidden=7;
        }"#,
        &["E_TYPE", "E_TYPE"],
    );
    codes(
        r#"outcome result {ready {u32 value}}
        control choose(bool flag) exits done {
            loop next(bool again_flag=flag) {
                result saved=delta again_flag {true=>ready {true},false=>ready {"bad"}};
                again next(again_flag);
            }
        }"#,
        &["E_TYPE", "E_TYPE"],
    );
    codes(
        r#"outcome result {ready {u32 value}}
        control choose(bool flag) exits done {
            string earlier=7;
            result saved=delta flag {true=>ready {true},false=>ready {"bad"}};
            exit done;
        }"#,
        &["E_TYPE"],
    );
    codes(
        r#"outcome result {ready {u32 value}}
        control main(bool flag) exits done {
            fault boundary {
                result saved=delta flag {true=>ready {true},false=>ready {"bad"}};
            } contingency [] {terminate process 1;}
            exit done;
        }"#,
        &["E_TYPE"],
    );
    codes(
        r#"outcome result {ready {u32 value}}
        control main(bool flag) exits done {
            fault boundary {} contingency [flag] {
                result saved=delta flag {true=>ready {true},false=>ready {"bad"}};
                terminate process 1;
            }
            exit done;
        }"#,
        &["E_TYPE"],
    );
}

#[test]
fn explicit_stored_adapter_recovery_keeps_headers_and_lifetimes_real() {
    codes(
        "outcome result {ready {u32 value}} control choose(bool flag) exits done {result saved=delta flag {true=>ready {true},false=>delta flag {true=>ready {\"bad\"}}};exit done;}",
        &["E_TYPE", "E_ARMS"],
    );
    codes(
        r#"outcome result['a] {ready {ref string value <- 'a,u32 count}}
        variant input {left {string value},right {string value}}
        control choose(input value) exits done {
            result saved=delta value {
                left {auto word}=>ready {borrow word,true},
                right {auto word}=>ready {borrow word,1}
            };
            exit done;
        }"#,
        &["E_TYPE", "E_LIFETIME"],
    );
}

#[test]
fn explicit_stored_adapter_arms_share_the_budget_and_check_generic_definitions() {
    codes(
        "outcome result<t> {ready {t value,u32 count}} control choose<t>(t input,bool flag) exits done {result<t> saved=delta flag {true=>ready {input,true},false=>ready {input,\"bad\"}};exit done;}",
        &["E_TYPE", "E_TYPE"],
    );
    fn nested(depth: usize) -> String {
        if depth == 0 {
            return "ready {true}".into();
        }
        format!(
            "delta flag {{true=>{},false=>{}}}",
            nested(depth - 1),
            nested(depth - 1)
        )
    }
    let batch = errors(&format!(
        "outcome result {{ready {{u32 value}}}} control choose(bool flag) exits done {{result saved={};exit done;}}",
        nested(6)
    ));
    assert!(batch.limit_reached, "{batch}");
    assert_eq!(batch.errors.len(), tellur::diagnostics::MAX_DIAGNOSTICS);
    assert!(batch.errors.iter().all(|e| e.code == "E_TYPE"));
}

#[test]
fn explicit_exit_adapters_recover_independent_terminal_arms() {
    codes(
        "control choose(bool flag) exits ready {u32 value} {exit delta flag {true=>ready {true},false=>ready {\"wrong\"}};} control caller() exits ready {u32 value} {forward choose(true);}",
        &["E_TYPE", "E_TYPE"],
    );
    codes(
        "outcome result {ready {u32 value},missing} control choose(bool flag) exits result {exit delta flag {true=>unknown {},false=>ready {true}};}",
        &["E_EXIT", "E_TYPE"],
    );
    codes(
        "control choose(bool flag) exits done {exit delta flag {true=>done {7},false=>done {true}};}",
        &["E_ARITY", "E_ARITY"],
    );
}

#[test]
fn explicit_exit_adapters_recover_nested_arms_without_narrowing_outer_selectors() {
    codes(
        r#"control choose(bool flag,bool other) exits ready {u32 value} {
            delta flag {true=>selected,false=>separate}
            [selected]: exit delta other {
                true=>[inner]: delta flag {true=>ready {true},false=>ready {"bad"}},
                false=>ready {false}
            };
            [separate]: string fourth=7;exit ready {1};
            [entry]: bool hidden=8;
        }"#,
        &["E_TYPE", "E_TYPE", "E_TYPE", "E_TYPE"],
    );
}

#[test]
fn explicit_exit_adapters_replay_owned_and_borrowed_payloads_cleanly() {
    codes(
        r#"control choose(bool flag,handle resource) exits ready {handle owned,u32 value} {
            exit delta flag {true=>ready {move resource,true},false=>ready {move resource,"bad"}};
        }"#,
        &["E_TYPE", "E_TYPE"],
    );
    codes(
        r#"control choose(bool flag,ref string input)
            exits ready {ref string value from input,u32 count} {
            exit delta flag {true=>ready {input,true},false=>ready {input,"bad"}};
        }"#,
        &["E_TYPE", "E_TYPE"],
    );
    // Resource completion remains real on the independent clean arm.
    codes(
        "control choose(bool flag,handle resource) exits ready {u32 value} {exit delta flag {true=>ready {true},false=>ready {1}};}",
        &["E_TYPE", "E_RESOURCE"],
    );
}

#[test]
fn exit_adapter_headers_and_inferred_stored_results_remain_barriers() {
    for source in [
        "control choose(bool flag) exits ready {u32 value} {exit delta flag {true=>ready {true}};}",
        "control choose(bool flag) exits auto {exit delta flag {true=>ready {missing},false=>ready {also_missing}};}",
        "control choose(bool flag) exits ready... {exit delta flag {true=>ready {missing},false=>ready {also_missing}};}",
        "control choose(bool flag) exits done {auto saved=delta flag {true=>ready {missing},false=>ready {also_missing}};exit done;}",
    ] {
        let batch = errors(source);
        assert_eq!(batch.errors.len(), 1, "{batch}");
    }
    // Header errors of a nested decision stop this adapter's replay.
    codes(
        "control choose(bool flag) exits ready {u32 value} {exit delta flag {true=>ready {true},false=>delta flag {true=>ready {\"bad\"}}};}",
        &["E_TYPE", "E_ARMS"],
    );
}

#[test]
fn exit_adapter_recovery_does_not_cross_uncertain_entry_or_fault_contexts() {
    codes(
        "control choose(bool flag) exits ready {u32 value} {string first=7;exit delta flag {true=>ready {true},false=>ready {\"bad\"}};}",
        &["E_TYPE"],
    );
    codes(
        "control main(bool flag) exits ready {u32 value} {fault boundary {exit delta flag {true=>ready {true},false=>ready {\"bad\"}};} contingency [] {terminate process 1;}}",
        &["E_TYPE"],
    );
    codes(
        "control main(bool flag) exits done {fault boundary {} contingency [flag] {exit delta flag {true=>done {},false=>done {}};}exit done;}",
        &["E_CONTINGENCY_EXIT"],
    );
}

#[test]
fn explicit_exit_adapter_arms_share_one_global_budget_and_original_generic_locations() {
    codes(
        "control choose<t>(t input,bool flag) exits ready {t value,u32 count} {exit delta flag {true=>ready {input,true},false=>ready {input,\"bad\"}};}",
        &["E_TYPE", "E_TYPE"],
    );
    fn nested(depth: usize) -> String {
        if depth == 0 {
            return "ready {true}".into();
        }
        format!(
            "delta flag {{true=>{},false=>{}}}",
            nested(depth - 1),
            nested(depth - 1)
        )
    }
    let batch = errors(&format!(
        "control choose(bool flag) exits ready {{u32 value}} {{exit {};}}",
        nested(6)
    ));
    assert!(batch.limit_reached, "{batch}");
    assert_eq!(batch.errors.len(), tellur::diagnostics::MAX_DIAGNOSTICS);
    assert!(batch.errors.iter().all(|e| e.code == "E_TYPE"));
}

#[test]
fn named_boundary_leaf_errors_recover_with_dependent_supervisors_suppressed() {
    codes(
        r#"boundary request(string context) retains context {
            u32 first=true; emit(first); string second=7; terminate;
        } contingency(string reason) {terminate;}
        control caller() exits done {supervise request("test");u32 hidden=false;exit done;}
        control independent() exits done {bool third=8;exit done;}"#,
        &["E_TYPE", "E_TYPE", "E_TYPE"],
    );
    // A failed handler also suppresses its dependent work body and supervisor.
    codes(
        r#"boundary request(string context) retains context {
            u32 hidden=true; terminate;
        } contingency(string reason) {
            u32 first=true; emit(first); string second=7; terminate;
        }
        control caller() exits done {supervise request("test");u32 hidden=false;exit done;}
        control independent() exits done {bool third=8;exit done;}"#,
        &["E_TYPE", "E_TYPE", "E_TYPE"],
    );
}

#[test]
fn named_boundary_recovery_preserves_disjoint_paths_scopes_and_real_ownership_errors() {
    for body in [
        "delta flag {true=>yes,false=>no}[yes]: {u32 first=true;string second=7;terminate;}[no]:bool third=8;terminate;",
        "delta flag {true=>yes,false=>no}[yes] over &: {u32 first=true;string second=7;}[no]:bool third=8;[entry]:terminate;",
    ] {
        codes(
            &format!(
                "boundary request(bool flag) {{{body}}} contingency(string reason) {{terminate;}}"
            ),
            &["E_TYPE", "E_TYPE", "E_TYPE"],
        );
    }
    let batch = errors(
        r#"boundary request(string context) retains context {
            string owner="before"; auto moved=move owner;
            u32 first=true; emit(borrow owner); string second=7; terminate;
        } contingency(string reason) {terminate;}"#,
    );
    assert_eq!(
        batch.errors.iter().map(|e| e.code).collect::<Vec<_>>(),
        ["E_TYPE", "E_MOVED", "E_TYPE"],
        "{batch}"
    );
    assert!(batch.errors[1].witness.is_some());
}

#[test]
fn named_boundary_fault_admission_stalls_uncertain_paths_without_leaking_call_scopes() {
    codes(
        r#"control danger() exits done faults {exit done;}
        boundary request(string context,bool flag) retains context {
            delta flag {true=>yes,false=>no}
            [yes] over &: {drop(context);u32 first=true;danger();bool hidden=7;}
            [no]: string second=7; terminate;
        } contingency(string reason) {terminate;}"#,
        &["E_TYPE", "E_TYPE"],
    );
    codes(
        r#"control danger() exits done faults {exit done;}
        boundary request(handle resource,bool flag) {
            delta flag {true=>yes,false=>no}
            [yes] over &: {u32 first=true;danger();bool hidden=7;}
            [no]: string second=7; terminate;
        } contingency(string reason) {terminate;}"#,
        &["E_TYPE", "E_TYPE"],
    );
    // Fault-only invocations have no ordinary result scope, but the same barrier.
    codes(
        r#"control danger() faults {fault "broken";}
        boundary request(string context) retains context {
            drop(context);u32 first=true;danger();string hidden=7;terminate;
        } contingency(string reason) {terminate;}"#,
        &["E_TYPE"],
    );
}

#[test]
fn named_boundary_clean_fault_obligations_remain_real_barriers() {
    for (parameter, retain, before, code) in [
        (
            "string context",
            "retains context",
            "drop(context);",
            "E_BOUNDARY_RETAIN",
        ),
        ("handle resource", "", "", "E_FAULT_RESOURCE"),
    ] {
        codes(
            &format!(
                "control danger() exits done faults {{exit done;}} boundary request({parameter},bool flag) {retain} {{delta flag {{true=>yes,false=>no}}[yes]:u32 first=true;terminate;[no]:{before}danger();string hidden=7;terminate;}} contingency(string reason) {{terminate;}}"
            ),
            &["E_TYPE", code],
        );
    }
}

#[test]
fn named_contingency_recovery_keeps_permissions_and_administrative_dispositions() {
    codes(
        r#"pub control ordinary() exits done {exit done;}
        boundary request() {terminate;}
        contingency(string reason) {u32 first=true;ordinary();string second=7;terminate;}"#,
        &["E_TYPE", "E_CONTINGENT", "E_TYPE"],
    );
    for terminal in ["terminate;", "escalate;", "fault reason;"] {
        codes(
            &format!(
                "boundary request() {{terminate;}} contingency(string reason) {{u32 first=true;string second=7;{terminal}bool hidden=8;}}"
            ),
            &["E_TYPE", "E_TYPE"],
        );
    }
    // Illegal ordinary exit stays the first-error barrier when the path is clean.
    codes(
        "boundary request() {terminate;} contingency(string reason) {exit done;u32 hidden=true;}",
        &["E_EXIT"],
    );
}

#[test]
fn named_boundary_recovery_uses_the_existing_global_diagnostic_budget() {
    let body = (0..40)
        .map(|i| format!("u32 bad_{i}=true;\n"))
        .collect::<String>();
    for source in [
        format!(
            "boundary request() {{{body}terminate;}} contingency(string reason) {{terminate;}}"
        ),
        format!(
            "boundary request() {{terminate;}} contingency(string reason) {{{body}terminate;}}"
        ),
    ] {
        let batch = errors(&source);
        assert!(batch.limit_reached);
        assert_eq!(batch.errors.len(), tellur::diagnostics::MAX_DIAGNOSTICS);
        assert!(batch.errors.iter().all(|e| e.code == "E_TYPE"));
    }
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
fn handler_terminals_and_stored_outcome_adapters_remain_recovery_barriers() {
    for body in [
        "fault boundary {exit done;} contingency [] {exit done;u32 first=true;string second=7;}",
        "auto saved=delta true {true=>ready {missing},false=>ready {also_missing}};exit done;",
    ] {
        let source = format!(
            "control main() exits done {{{body}}} control other() exits done {{u32 bad=true;string second=7;exit done;}}"
        );
        let batch = errors(&source);
        assert_eq!(batch.errors.len(), 3, "{batch}");
    }
}

#[test]
fn lexical_work_recovers_leaf_errors_but_stalls_handler_and_outer_continuation() {
    codes(
        r#"control main() exits done {
            fault boundary {u32 first=true;string second=7;}
            contingency [] {bool hidden=8;terminate process 1;}
            bool also_hidden=9;exit done;
        }
        control other() exits done {u32 third=false;string fourth=8;exit done;}"#,
        &["E_TYPE", "E_TYPE", "E_TYPE", "E_TYPE"],
    );
}

#[test]
fn lexical_work_restores_outer_selectors_and_checks_disjoint_histories() {
    codes(
        r#"control main(bool flag) exits done {
            delta flag {true=>yes,false=>no}
            [yes]: fault boundary {
                delta flag {true=>available,false=>missing}
                [available] over &: {u32 first=true;string second=7;}
                [missing]: bool third=8;
            } contingency [] {terminate process 1;}
            [no]: string fourth=9;exit done;
            [yes | no]: bool hidden=7;
        }"#,
        &["E_TYPE", "E_TYPE", "E_TYPE", "E_TYPE"],
    );
    // Restoring the parent's entry selector matters: it joins the uncertain yes
    // history with no, so subsequent no statements must also stall.
    codes(
        r#"control main(bool flag) exits done {
            delta flag {true=>yes,false=>no}
            [yes]: fault boundary {u32 first=true;string second=7;}
                contingency [] {terminate process 1;}
            [entry]: bool hidden=8;
            [no]: string also_hidden=9;exit done;
        }"#,
        &["E_TYPE", "E_TYPE"],
    );
}

#[test]
fn lexical_work_nested_boundaries_retire_uncertain_inner_and_outer_domains() {
    codes(
        r#"control main(bool flag) exits done {
            fault boundary {
                delta flag {true=>yes,false=>no}
                [yes]: fault boundary {u32 first=true;string second=7;}
                    contingency [] {escalate;}
                [yes]: bool hidden=9;
                [no]: bool third=8;
            } contingency [] {terminate process 1;}
            string also_hidden=9;exit done;
        }"#,
        &["E_TYPE", "E_TYPE", "E_TYPE"],
    );
}

#[test]
fn lexical_work_keeps_retained_owner_checks_and_skips_uncertain_handover() {
    codes(
        r#"control danger() exits done faults {exit done;}
        control main(bool flag) exits done {
            string context="request";
            fault boundary {
                delta flag {true=>yes,false=>no}
                [yes] over &: {drop(context);u32 first=true;danger();bool hidden=8;}
                [no]: string second=7;
            } contingency [context] {bool hidden=8;terminate process 1;}
            exit done;
        }"#,
        &["E_TYPE", "E_TYPE"],
    );
    codes(
        r#"control danger() exits done faults {exit done;}
        control main(bool flag) exits done {
            string context="request";
            fault boundary {
                delta flag {true=>yes,false=>no}
                [yes]: u32 first=true;
                [no]: drop(context);danger();
            } contingency [context] {terminate process 1;}
            exit done;
        }"#,
        &["E_TYPE", "E_BOUNDARY_RETAIN"],
    );
}

#[test]
fn lexical_work_headers_and_supervisor_state_keep_their_rules() {
    codes(
        "control main() exits done {fault boundary {u32 hidden=true;string also_hidden=7;} contingency [missing] {terminate process 1;}exit done;}",
        &["E_BOUNDARY_CAPTURE"],
    );
    codes(
        "control main() exits done {fault boundary {} contingency [] {u32 first=true;string second=7;terminate process 1;}exit done;}",
        &["E_TYPE", "E_TYPE"],
    );
    codes(
        r#"control main() exits done {
            supervised loop requestor(u32 counter=1) {
                fault boundary {u32 first=counter;string second=7;}
                contingency [] {recover again requestor;}
                exit done;
            }
        }"#,
        &["E_SUPERVISOR_STATE", "E_TYPE"],
    );
}

#[test]
fn lexical_work_uses_one_budget_and_recovers_inside_ordinary_loops() {
    let body = (0..40)
        .map(|i| format!("u32 bad_{i}=true;\n"))
        .collect::<String>();
    let batch = errors(&format!(
        "control main() exits done {{fault boundary {{{body}}} contingency [] {{terminate process 1;}}exit done;}}"
    ));
    assert!(batch.limit_reached);
    assert_eq!(batch.errors.len(), tellur::diagnostics::MAX_DIAGNOSTICS);
    assert!(batch.errors.iter().all(|e| e.code == "E_TYPE"));
    codes(
        "control main() exits done {fault boundary {loop work {u32 first=true;string second=7;exit done;}} contingency [] {terminate process 1;}exit done;}",
        &["E_TYPE", "E_TYPE"],
    );
}

#[test]
fn lexical_handler_recovers_leaf_errors_after_clean_work_only() {
    codes(
        r#"control main() exits done {
            fault boundary {} contingency [] {
                u32 first=true;emit(first);string second=7;terminate process 1;
            }
            bool hidden=8;exit done;
        }
        control other() exits done {u32 third=false;exit done;}"#,
        &["E_TYPE", "E_TYPE", "E_TYPE"],
    );
    codes(
        "control main() exits done {fault boundary {u32 first=true;string second=7;} contingency [] {bool hidden=8;string also_hidden=9;terminate process 1;}exit done;}",
        &["E_TYPE", "E_TYPE"],
    );
}

#[test]
fn lexical_handler_restores_normal_state_and_parent_entry_before_outer_checks() {
    codes(
        r#"control main(bool flag) exits done {
            string owner="before";
            delta flag {true=>yes,false=>no}
            [yes]: fault boundary {drop(owner);} contingency [] {
                u32 first=true;string second=7;terminate process 1;
            }
            [no]: emit(borrow owner);bool third=8;exit done;
        }"#,
        &["E_TYPE", "E_TYPE", "E_TYPE"],
    );
    codes(
        r#"control main(bool flag) exits done {
            delta flag {true=>yes,false=>no}
            [yes]: fault boundary {} contingency [] {
                u32 first=true;string second=7;terminate process 1;
            }
            [entry]: bool hidden=8;
            [no]: string also_hidden=9;exit done;
        }"#,
        &["E_TYPE", "E_TYPE"],
    );
}

#[test]
fn lexical_handler_nested_failure_stalls_outer_work_and_its_handler() {
    codes(
        r#"control main() exits done {
            fault boundary {
                fault boundary {} contingency [] {
                    u32 first=true;string second=7;escalate;
                }
                bool hidden=8;
            } contingency [] {string also_hidden=9;terminate process 1;}
            exit done;
        }"#,
        &["E_TYPE", "E_TYPE"],
    );
}

#[test]
fn lexical_handler_fault_admission_stalls_uncertainty_but_checks_clean_paths() {
    for (no_path, expected) in [
        ("string second=7;", vec!["E_TYPE", "E_TYPE"]),
        ("danger();", vec!["E_TYPE", "E_FAULT_RESOURCE"]),
    ] {
        codes(
            &format!(
                r#"@contingent control danger() exits done faults {{exit done;}}
            control main(handle resource,bool flag) exits done {{
                fault boundary {{}} contingency [resource,flag] {{
                    delta flag {{true=>yes,false=>no}}
                    [yes] over &: {{u32 first=true;danger();bool hidden=8;}}
                    [no]: {no_path}
                    [entry]: terminate process 1;
                }}
                exit done;
            }}"#
            ),
            &expected,
        );
    }
}

#[test]
fn lexical_handler_recovery_preserves_fault_mode_capture_and_terminal_rules() {
    codes(
        r#"pub control ordinary() exits done {exit done;}
        control main() exits done {
            string private="outside";
            fault boundary {} contingency [] {
                u32 first=true;ordinary();emit(private);string second=7;
                terminate process 1;
            }
            exit done;
        }"#,
        &["E_TYPE", "E_CONTINGENT", "E_CONTINGENCY_CAPTURE", "E_TYPE"],
    );
    codes(
        "control main() exits done {fault boundary {} contingency [] {u32 first=true;auto forged=borrow mutable @fault.message;string second=7;terminate process 1;}exit done;}",
        &["E_TYPE", "E_MUTABILITY", "E_TYPE"],
    );
    // Illegal ordinary return is still a terminal barrier on a clean history.
    codes(
        "control main() exits done {fault boundary {} contingency [] {exit done;u32 hidden=true;}exit done;}",
        &["E_CONTINGENCY_EXIT"],
    );
    // A clean administrative terminal still requires completion of its owners.
    codes(
        "control main(handle resource) exits done {fault boundary {} contingency [resource] {terminate process 1;u32 hidden=true;}exit done;}",
        &["E_RESOURCE"],
    );
}

#[test]
fn lexical_handler_loops_and_disjoint_paths_share_the_diagnostic_budget() {
    codes(
        r#"control main(bool flag) exits done {
            fault boundary {} contingency [flag] {
                delta flag {true=>yes,false=>no}
                [yes] over &: {u32 first=true;string second=7;}
                [no]: bool third=8;
                [entry]: terminate process 1;
            }
            exit done;
        }"#,
        &["E_TYPE", "E_TYPE", "E_TYPE"],
    );
    codes(
        "control main() exits done {fault boundary {} contingency [] {loop cleanup {u32 first=true;string second=7;terminate process 1;}}exit done;}",
        &["E_TYPE", "E_TYPE"],
    );
    let body = (0..40)
        .map(|i| format!("u32 bad_{i}=true;\n"))
        .collect::<String>();
    let batch = errors(&format!(
        "control main() exits done {{fault boundary {{}} contingency [] {{{body}terminate process 1;}}exit done;}}"
    ));
    assert!(batch.limit_reached);
    assert_eq!(batch.errors.len(), tellur::diagnostics::MAX_DIAGNOSTICS);
    assert!(batch.errors.iter().all(|e| e.code == "E_TYPE"));
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
