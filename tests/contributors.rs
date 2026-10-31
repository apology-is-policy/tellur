use tellur::{Diagnostic, compile, manifest::Manifest};
fn error(source: &str, code: &str) -> Diagnostic {
    let manifest = Manifest::from_json(include_str!("../playground/manifest.json")).unwrap();
    let error = compile(source, manifest).unwrap_err();
    assert_eq!(error.code, code, "{error}");
    error
}
fn has(error: &Diagnostic, text: &str) {
    assert!(
        error.notes.iter().any(|n| n.message.contains(text)),
        "missing {text}: {error}"
    );
}
fn lacks(error: &Diagnostic, text: &str) {
    assert!(
        error.notes.iter().all(|n| !n.message.contains(text)),
        "unexpected {text}: {error}"
    );
}
const CHOOSE: &str =
    "control leaf(bool flag) exits auto {exit delta flag {true=>ready {},false=>missing {}};}\n";
#[test]
fn captures_and_moves_keep_only_the_missing_tags_sources() {
    let e = error(
        &format!(
            "{CHOOSE}control main() exits done {{\n auto saved=capture leaf(true);\n auto moved=saved;\n delta moved {{ready=>ready}} [ready]:exit done;}}"
        ),
        "E_UNCOVERED",
    );
    has(&e, "`missing` is produced by `leaf`");
    has(&e, "received from `leaf`");
    lacks(&e, "`ready`");
    assert!(e.notes.iter().any(|n| n.span.line == 1));
    assert!(e.notes.iter().any(|n| n.span.line == 3));
}
#[test]
fn stored_adapters_and_delayed_exit_retain_renaming_and_absorption() {
    let source = format!(
        "{CHOOSE}control adapt(bool flag) exits auto {{\n auto saved=capture leaf(flag);\n auto mapped=delta saved {{ready=>available {{}},missing=>unavailable {{}}}};\n exit mapped;}}\ncontrol main() exits done {{delta adapt(false) {{available=>ok}}[ok]:exit done;}}"
    );
    let e = error(&source, "E_UNCOVERED");
    has(&e, "`missing` is produced by `leaf`");
    has(&e, "`unavailable` is constructed by `adapt`");
    lacks(&e, "`ready`");
    lacks(&e, "`available`");
    let e = error(
        &source
            .replace("available", "handled")
            .replace("{handled=>ok}", "{}")
            .replace("[ok]:exit done;", ""),
        "E_UNCOVERED",
    );
    has(&e, "`ready` is produced by `leaf`");
    has(&e, "`missing` is produced by `leaf`");
}
#[test]
fn merged_labels_retain_both_internal_contributors() {
    let e = error(
        &format!(
            "{CHOOSE}control absorb() exits auto {{delta leaf(false) {{ready=>a,missing=>b}}[a | b]:exit handled;}}control main() exits done {{delta absorb() {{}}}}"
        ),
        "E_UNCOVERED",
    );
    has(&e, "`ready` is produced by `leaf`");
    has(&e, "`missing` is produced by `leaf`");
    has(&e, "`handled` is produced by `absorb`");
}
#[test]
fn nested_sum_and_record_payloads_preserve_field_identity() {
    let e = error(
        "outcome reply {left,right} struct pair {reply first;reply second;} variant box {packed {pair value},empty}\ncontrol main() exits done {\n reply first=reply.left {};\n reply second=reply.right {};\n box saved=box.packed {pair {first,second}};\n delta saved {packed {auto pair}=>packed,empty=>empty}\n [packed]:delta pair.first {} [empty]:exit done;}",
        "E_UNCOVERED",
    );
    assert!(e.notes.iter().any(|n| n.span.line == 3), "{e}");
    assert!(e.notes.iter().all(|n| n.span.line != 4), "{e}");
    lacks(&e, "`right`");
}
#[test]
fn disjoint_bindings_are_filtered_by_the_handling_path() {
    let e = error(
        "outcome reply {ready,missing}\ncontrol left() exits reply {exit missing;}\ncontrol right() exits reply {exit missing;}\ncontrol main(bool flag) exits done {delta flag {true=>a,false=>b}\n[a]:reply saved=capture left();\n[b]:reply saved=capture right();\n[a]:delta saved {} [b]:exit delta saved {ready=>done {},missing=>done {}};}",
        "E_UNCOVERED",
    );
    has(&e, "received from `left`");
    lacks(&e, "received from `right`");
}
#[test]
fn overwritten_values_do_not_report_stale_sources() {
    let e = error(
        "variant choice {old,new}\ncontrol main() exits done {\n mutable choice value=choice.old {};\n value=choice.new {};\n delta value {new=>new}[new]:exit done;}",
        "E_UNCOVERED",
    );
    assert!(
        e.notes.is_empty(),
        "old is still a required arm, but its overwritten source is not a contributor: {e}"
    );
    let e = error(
        "variant choice {old,new}\ncontrol main(bool flag) exits done {mutable choice value=choice.old {};delta flag {true=>a,false=>b}[a]:value=choice.new {};[a]:delta value {new=>new}[b | new]:exit done;}",
        "E_UNCOVERED",
    );
    assert!(e.notes.is_empty(), "{e}");
}
#[test]
fn mutable_calls_forget_unknown_updates() {
    let e = error(
        "variant choice {old,new}\ncontrol change(ref mutable choice value) exits done {deref value=choice.new {};exit done;}\ncontrol main() exits done {mutable choice value=choice.old {};change(borrow mutable value);delta value {new=>new}[new]:exit done;}",
        "E_UNCOVERED",
    );
    assert!(e.notes.is_empty(), "{e}");
}
#[test]
fn borrowed_matching_and_cloning_ordinary_variants_keep_known_sources() {
    for operand in ["borrow saved", "copy"] {
        let e = error(
            &format!(
                "variant choice {{ready,missing}} control main() exits done {{choice saved=choice.missing {{}};auto copy=clone(borrow saved);delta {operand} {{ready=>ready}}[ready]:exit done;}}"
            ),
            "E_UNCOVERED",
        );
        has(&e, "`missing` is constructed");
    }
}
#[test]
fn incompatible_adapter_results_explain_the_selected_input() {
    let e = error(
        &format!(
            "{CHOOSE}control main() exits ready {{exit delta leaf(false) {{ready=>ready {{}},missing=>invalid {{}}}};}}"
        ),
        "E_EXIT",
    );
    has(&e, "`missing` is produced by `leaf`");
    lacks(&e, "`ready` is produced");
    let e = error(
        &format!(
            "{CHOOSE}outcome target {{ready}} control main() exits ready {{target saved=delta leaf(false) {{ready=>ready {{}},missing=>invalid {{}}}};exit saved;}}"
        ),
        "E_OUTCOME",
    );
    has(&e, "`missing` is produced by `leaf`");
}
#[test]
fn stored_lineage_is_bounded_across_long_adapter_chains() {
    let mut source = "control f0() exits auto {exit changed;}\n".to_owned();
    for i in 1..45 {
        source.push_str(&format!(
            "control f{i}() exits auto {{auto saved=capture f{}();exit saved;}}\n",
            i - 1
        ));
    }
    source.push_str("control main() exits done {auto saved=capture f44();delta saved {}}");
    let e = error(&source, "E_UNCOVERED");
    assert_eq!(e.notes.len(), 33, "{e}");
    assert!(e.notes.last().unwrap().message.contains("omitted"));
}

#[test]
fn field_replacement_retains_other_fields_only() {
    let e = error(
        "variant choice {old,new} struct pair {choice first;choice second;}\ncontrol main() exits done {\n mutable pair values=pair {first: choice.old {},second: choice.old {}};\n values.first=choice.new {};\n delta values.first {new=>new}[new]:exit done;}",
        "E_UNCOVERED",
    );
    assert!(e.notes.is_empty(), "{e}");
    let e = error(
        "variant choice {old,new} struct pair {choice first;choice second;}\ncontrol main() exits done {\n mutable pair values=pair {first: choice.old {},second: choice.old {}};\n values.first=choice.new {};\n delta values.second {new=>new}[new]:exit done;}",
        "E_UNCOVERED",
    );
    has(&e, "`old` is constructed");
    assert!(e.notes.iter().all(|n| n.span.line == 3), "{e}");
}
#[test]
fn loop_parameters_do_not_claim_the_initializer_as_every_iterations_source() {
    let e = error(
        "variant choice {old,new}\ncontrol main() exits done {loop retry(choice current=choice.old {}) {delta current {new=>new}[new]:exit done;}}",
        "E_UNCOVERED",
    );
    assert!(e.notes.is_empty(), "{e}");
}
#[test]
fn nested_adapters_keep_the_selected_outer_contributor() {
    let e = error(
        &format!(
            "{CHOOSE}control nested() exits auto {{auto saved=delta leaf(false) {{ready=>available {{}},missing=>delta leaf(true) {{ready=>unavailable {{}},missing=>unavailable {{}}}}}};exit saved;}}control main() exits done {{delta nested() {{available=>available}}[available]:exit done;}}"
        ),
        "E_UNCOVERED",
    );
    has(&e, "`missing` is produced by `leaf`");
    has(&e, "`ready` is produced by `leaf`");
    has(&e, "`unavailable` is constructed by `nested`");
    lacks(&e, "`available` is constructed");
}
