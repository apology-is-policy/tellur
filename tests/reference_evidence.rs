use serde_json::Value as Json;
use tellur::{
    CheckedProgram,
    certificate::ExpansionArtifact,
    compile,
    ir::{Action, Expr, Operation, ReferenceEvidence, ReferenceOrigin, Region},
    manifest::Manifest,
    runtime::Scenario,
};

fn program(source: &str) -> CheckedProgram {
    compile(
        source,
        Manifest::from_json(include_str!("../playground/manifest.json")).unwrap(),
    )
    .unwrap()
}

fn capture_evidence(region: &Region) -> &ReferenceEvidence {
    region
        .code
        .iter()
        .find_map(|instruction| match &instruction.action {
            Action::Decide {
                operation: Operation::Defer(_),
                args,
                ..
            } => match &args[0] {
                Expr::WithReferenceEvidence(_, evidence) => Some(evidence),
                other => panic!("missing explicit partial evidence: {other:?}"),
            },
            _ => None,
        })
        .unwrap()
}
fn capture(region: &Region) -> &[ReferenceOrigin] {
    capture_evidence(region).origins()
}

#[test]
fn fresh_local_carriers_cover_holder_and_all_stored_origins() {
    let source = include_str!("../playground/local_carrier_tasks.tellur");
    let p = program(source);
    let startup = p.regions().find(|r| r.name == "startup").unwrap();
    let evidence = capture_evidence(startup);
    assert!(evidence.is_complete(), "{evidence:?}");
    let names: std::collections::BTreeSet<_> = evidence
        .origins()
        .iter()
        .map(|o| {
            assert!(!o.guard && !o.opaque && !o.place.indirect);
            startup.slots[&o.place.root].name.as_str()
        })
        .collect();
    assert_eq!(names, ["left", "right", "saved"].into_iter().collect());
    let nested = source
        .replace(
            "struct text_pair",
            "struct envelope['a, 'b] { text_pair['a, 'b] values; }\nstruct text_pair",
        )
        .replace(
            "text_pair saved = text_pair { borrow left, borrow right };",
            "envelope saved = envelope { text_pair { borrow left, borrow right } };",
        )
        .replace("borrow saved", "borrow saved.values");
    let projected = program(&nested);
    let projected = projected.regions().find(|r| r.name == "startup").unwrap();
    assert!(capture_evidence(projected).is_complete());
    assert!(
        capture(projected)
            .iter()
            .any(|o| projected.slots[&o.place.root].name == "saved" && o.place.fields == [0])
    );
    let nested = nested
        .replace("ref text_pair input", "ref envelope input")
        .replace("input.left", "input.values.left")
        .replace("input.right", "input.values.right")
        .replace("borrow saved.values", "borrow saved");
    let nested = program(&nested);
    assert!(
        capture_evidence(nested.regions().find(|r| r.name == "startup").unwrap()).is_complete()
    );
    let returned = source.replace("ref string right <- 'right;", "ref string right <- 'right;\n    constructor(ref string left, ref string right)[left, right] { self.left = left; self.right = right; }")
        .replace("text_pair { borrow left, borrow right }", "text_pair(borrow left, borrow right)");
    let returned = program(&returned);
    assert!(
        !capture_evidence(returned.regions().find(|r| r.name == "startup").unwrap()).is_complete()
    );
    let artifact = p.expansion_artifact("startup").unwrap();
    let expanded = p.accept_expansion(&artifact).unwrap();
    let scenario = Scenario {
        entry: "main".into(),
        arguments: vec![],
        calls: vec![],
    };
    assert_eq!(p.run(&scenario).unwrap(), expanded.run(&scenario).unwrap());
    // Every origin and the completeness discriminator are independently checked
    // against the source-derived expansion, including the holder itself.
    for kind in 0..4 {
        let mut altered = serde_json::to_value(&artifact).unwrap();
        fn mutate(v: &mut Json, kind: usize) -> bool {
            if let Some(parts) = v
                .get_mut("WithReferenceEvidence")
                .and_then(Json::as_array_mut)
                && parts[0]
                    .get("Reference")
                    .is_some_and(|r| r[1] == "&text_pair")
                && let Some(origins) = parts[1].get_mut("Complete").and_then(Json::as_array_mut)
            {
                if kind < 3 {
                    origins.remove(kind);
                } else {
                    let origins = origins.clone();
                    parts[1] = serde_json::json!({"Partial": origins});
                }
                return true;
            }
            match v {
                Json::Array(v) => v.iter_mut().any(|v| mutate(v, kind)),
                Json::Object(v) => v.values_mut().any(|v| mutate(v, kind)),
                _ => false,
            }
        }
        assert!(mutate(&mut altered, kind));
        assert_eq!(
            p.accept_expansion(&serde_json::from_value(altered).unwrap())
                .unwrap_err()
                .code,
            "E_EXPANSION_CERTIFICATE"
        );
    }
    // An erased local alias preserves the complete stored closure.
    let aliased = source.replace(
        "auto pending = defer inspect(borrow saved);",
        "ref text_pair alias = borrow saved;\n    auto pending = defer inspect(alias);",
    );
    assert_ne!(aliased, source);
    let alias = program(&aliased);
    assert!(capture_evidence(alias.regions().find(|r| r.name == "startup").unwrap()).is_complete());
    let conditional = source.replace("async control startup() exits done {",
        "async control startup(bool selected) exits done {\n    delta selected { true => yes, false => no }\n    [no]: exit done;\n    [yes]:")
        .replace("run startup()", "run startup(true)");
    let conditional = program(&conditional);
    assert!(
        !capture_evidence(conditional.regions().find(|r| r.name == "startup").unwrap())
            .is_complete()
    );
}

#[test]
fn local_carrier_aliases_retain_broad_ancestors_and_all_their_stored_owners() {
    let source = include_str!("../playground/aliased_carrier_tasks.tellur");
    let p = program(source);
    let region = p.regions().find(|r| r.name == "startup").unwrap();
    let evidence = capture_evidence(region);
    assert!(evidence.is_complete(), "{evidence:?}");
    let saved = region
        .slots
        .values()
        .find(|s| s.name == "saved")
        .unwrap()
        .id;
    let origins = evidence.origins();
    assert!(
        origins
            .iter()
            .all(|o| !o.guard && !o.opaque && !o.place.indirect)
    );
    assert!(
        origins
            .iter()
            .any(|o| o.place.root == saved && o.place.fields == [0])
    );
    assert!(
        origins
            .iter()
            .any(|o| o.place.root == saved && o.place.fields.is_empty())
    );
    for name in ["left", "right", "other"] {
        assert!(
            origins
                .iter()
                .any(|o| region.slots[&o.place.root].name == name),
            "{origins:?}"
        );
    }
    let artifact = p.expansion_artifact("startup").unwrap();
    let expanded = p.accept_expansion(&artifact).unwrap();
    let scenario = Scenario {
        entry: "main".into(),
        arguments: vec![],
        calls: vec![],
    };
    assert_eq!(p.run(&scenario).unwrap(), expanded.run(&scenario).unwrap());
    // Removing the broad origin or the owner outside the projected record must
    // fail even though the runtime reference points only to values.
    let broad = origins
        .iter()
        .position(|o| o.place.root == saved && o.place.fields.is_empty())
        .unwrap();
    let other = origins
        .iter()
        .position(|o| region.slots[&o.place.root].name == "other")
        .unwrap();
    for removal in [Some(broad), Some(other), None] {
        let mut altered = serde_json::to_value(&artifact).unwrap();
        fn mutate(v: &mut Json, removal: Option<usize>) -> bool {
            if let Some(parts) = v
                .get_mut("WithReferenceEvidence")
                .and_then(Json::as_array_mut)
                && parts[0]
                    .get("Reference")
                    .is_some_and(|r| r[1] == "&text_pair")
                && let Some(origins) = parts[1].get_mut("Complete").and_then(Json::as_array_mut)
                && origins.len() == 5
            {
                if let Some(index) = removal {
                    origins.remove(index);
                } else {
                    let origins = origins.clone();
                    parts[1] = serde_json::json!({"Partial":origins});
                }
                return true;
            }
            match v {
                Json::Array(v) => v.iter_mut().any(|v| mutate(v, removal)),
                Json::Object(v) => v.values_mut().any(|v| mutate(v, removal)),
                _ => false,
            }
        }
        assert!(mutate(&mut altered, removal));
        assert_eq!(
            p.accept_expansion(&serde_json::from_value(altered).unwrap())
                .unwrap_err()
                .code,
            "E_EXPANSION_CERTIFICATE"
        );
    }
    let conditional = source.replace("async control startup() exits done {",
        "async control startup(bool selected_path) exits done {\n    delta selected_path { true => yes, false => no }\n    [no]: exit done;\n    [yes]:")
        .replace("run startup()", "run startup(true)");
    let conditional = program(&conditional);
    assert!(
        !capture_evidence(conditional.regions().find(|r| r.name == "startup").unwrap())
            .is_complete()
    );
    let received = source.replace("async control inspect", "control relay(ref text_pair input) exits ready { ref text_pair value from input } { exit ready { input }; }\nasync control inspect")
        .replace("auto pending = defer inspect(selected);", "delta relay(selected) { ready { auto incoming } => received }\n    [received]:\n    auto pending = defer inspect(incoming);");
    let received = program(&received);
    assert!(
        !capture_evidence(received.regions().find(|r| r.name == "startup").unwrap()).is_complete()
    );
}

#[test]
fn captures_preserve_erased_alias_guards_without_inventing_shared_holder_dependencies() {
    let source = include_str!("../playground/guarded_task_captures.tellur");
    let direct = source.replace(
        "    delta detach(carrier) {\n        ready { auto detached } => borrowed\n    }\n\n    [borrowed]:",
        "    ref mutable string detached = borrow mutable deref carrier.text;",
    );
    assert_ne!(source, direct);
    let shared_direct = direct
        .replace(
            "ref mutable string detached = borrow mutable deref carrier.text;",
            "ref string detached = borrow deref carrier.text;",
        )
        .replace("defer lend(detached)", "defer inspect(detached)")
        .replace(
            "deref value = \"after\";",
            "// Shared results preserve the holder guard.",
        )
        .replace(
            "deref detached = \"available after result\";",
            "// The detached shared view still guards its holder.",
        );
    assert_ne!(shared_direct, direct);
    for (source, complete) in [
        (source, false),
        (direct.as_str(), true),
        (shared_direct.as_str(), true),
    ] {
        let p = program(source);
        let exclusive = p.regions().find(|r| r.name == "exclusive").unwrap();
        let origins = capture(exclusive);
        assert_eq!(
            capture_evidence(exclusive).is_complete(),
            complete,
            "{origins:?}"
        );
        if complete {
            let saved = exclusive
                .slots
                .values()
                .find(|s| s.name == "saved")
                .unwrap()
                .id;
            assert!(origins.iter().any(|o| o.place.root == saved
                && o.place.fields.is_empty()
                && o.guard
                && o.opaque));
            assert!(origins.iter().any(|o| o.place.root == saved
                && o.place.fields.is_empty()
                && !o.guard
                && !o.opaque));
            assert!(
                origins
                    .iter()
                    .any(|o| o.place.root == saved && o.place.fields == [0] && o.guard && o.opaque)
            );
        }
        assert!(
            origins
                .iter()
                .any(|o| { o.guard && o.opaque && exclusive.slots[&o.place.root].name == "saved" }),
            "{origins:?}"
        );
        assert!(
            origins
                .iter()
                .any(|o| { !o.guard && exclusive.slots[&o.place.root].name == "owner" }),
            "{origins:?}"
        );
        let shared = p.regions().find(|r| r.name == "shared").unwrap();
        let origins = capture(shared);
        assert!(!origins.is_empty());
        assert!(
            origins
                .iter()
                .all(|o| { !o.guard && shared.slots[&o.place.root].name == "owner" }),
            "{origins:?}"
        );
    }
}

const REPEATED: &str = r#"
struct view['a] { ref mutable string text <- 'a; }
control project(ref mutable view input) exits ready... {
    exit ready { borrow mutable deref input.text };
}
control main() exits done {
    mutable string a="left";mutable string b="right";
    mutable view first=view {borrow mutable a};
    mutable view second=view {borrow mutable b};
    delta project(borrow mutable first) {ready {auto left}=>left}
    [left]:emit(left);
    delta project(borrow mutable second) {ready {auto right}=>right}
    [right]:emit(right);drop(first);drop(second);exit done;
}
"#;

fn walk(value: &Json, f: &mut impl FnMut(&Json)) {
    f(value);
    match value {
        Json::Array(values) => values.iter().for_each(|v| walk(v, f)),
        Json::Object(values) => values.values().for_each(|v| walk(v, f)),
        _ => {}
    }
}

#[test]
fn expansion_remaps_partial_origin_slots_for_each_invocation_and_roundtrips() {
    let p = program(REPEATED);
    let project = p.regions().find(|r| r.name == "project").unwrap();
    let input = project.parameters[0].id;
    let artifact = p.expansion_artifact("main").unwrap();
    let roots: Vec<_> = artifact
        .calls
        .iter()
        .filter(|c| c.callee == "project")
        .map(|c| c.slots[&input])
        .collect();
    assert_eq!(roots.len(), 2);
    assert_ne!(roots[0], roots[1]);
    let encoded = serde_json::to_value(&artifact).unwrap();
    let mut guards = Vec::new();
    walk(&encoded, &mut |v| {
        if let Some(origins) = v.get("Partial").and_then(Json::as_array) {
            for origin in origins {
                if origin["guard"] == true {
                    guards.push(origin["place"]["root"].as_u64().unwrap() as usize);
                }
            }
        }
    });
    for root in roots {
        assert!(guards.contains(&root), "{guards:?}");
    }
    assert!(!guards.contains(&input));
    let decoded: ExpansionArtifact = serde_json::from_value(encoded).unwrap();
    let expanded = p.accept_expansion(&decoded).unwrap();
    let scenario = Scenario {
        entry: "main".into(),
        arguments: vec![],
        calls: vec![],
    };
    assert_eq!(p.run(&scenario).unwrap(), expanded.run(&scenario).unwrap());
}

fn mutate_guarded(value: &mut Json, mutation: usize, other_root: usize) -> bool {
    if let Some(parts) = value
        .get_mut("WithReferenceEvidence")
        .and_then(Json::as_array_mut)
    {
        let Some(origins) = parts[1].get_mut("Partial").and_then(Json::as_array_mut) else {
            return false;
        };
        if let Some(index) = origins.iter().position(|o| o["guard"] == true) {
            match mutation {
                0 => origins.clear(),
                1 => *value = parts[0].clone(),
                2 => origins[index]["guard"] = Json::Bool(false),
                3 => origins[index]["opaque"] = Json::Bool(false),
                4 => origins[index]["place"]["root"] = other_root.into(),
                5 => origins[index]["place"]["fields"]
                    .as_array_mut()
                    .unwrap()
                    .push(999.into()),
                _ => unreachable!(),
            }
            return true;
        }
    }
    match value {
        Json::Array(values) => values
            .iter_mut()
            .any(|v| mutate_guarded(v, mutation, other_root)),
        Json::Object(values) => values
            .values_mut()
            .any(|v| mutate_guarded(v, mutation, other_root)),
        _ => false,
    }
}

#[test]
fn certificates_reject_missing_retargeted_or_weakened_origin_evidence() {
    let p = program(REPEATED);
    let artifact = p.expansion_artifact("main").unwrap();
    let other = artifact
        .region
        .slots
        .values()
        .find(|s| s.name == "b")
        .unwrap()
        .id;
    for mutation in 0..6 {
        let mut value = serde_json::to_value(&artifact).unwrap();
        assert!(mutate_guarded(&mut value, mutation, other));
        let candidate = serde_json::from_value(value).unwrap();
        assert_eq!(
            p.accept_expansion(&candidate).unwrap_err().code,
            "E_EXPANSION_CERTIFICATE"
        );
    }
    let mut forged = serde_json::to_value(&artifact).unwrap();
    fn promote(value: &mut Json) -> bool {
        if let Some(parts) = value
            .get_mut("WithReferenceEvidence")
            .and_then(Json::as_array_mut)
        {
            if let Some(origins) = parts[1].get("Partial").cloned() {
                parts[1] = serde_json::json!({"Complete": origins});
                return true;
            }
        }
        match value {
            Json::Array(values) => values.iter_mut().any(promote),
            Json::Object(values) => values.values_mut().any(promote),
            _ => false,
        }
    }
    assert!(promote(&mut forged));
    let candidate = serde_json::from_value(forged).unwrap();
    assert_eq!(
        p.accept_expansion(&candidate).unwrap_err().code,
        "E_EXPANSION_CERTIFICATE"
    );
}

#[test]
fn received_carrier_fields_keep_partial_origins_on_private_address_reads() {
    let p = program(
        r#"
struct view['a] {ref mutable string text <- 'a;}
control relay(ref mutable view input)
    exits ready {ref mutable view value from input} {exit ready {input};}
control main() exits done {
    mutable string owner="before";
    mutable view saved=view {borrow mutable owner};
    delta relay(borrow mutable saved) {ready {auto value}=>ready}
    [ready]:deref value.text="after";emit(value.text);drop(saved);exit done;
}
"#,
    );
    let mut found = false;
    for r in p.regions() {
        walk(&serde_json::to_value(r).unwrap(), &mut |v| {
            if let Some(parts) = v.get("WithReferenceEvidence").and_then(Json::as_array) {
                found |= parts[0].get("Place").is_some()
                    && parts[1]["Partial"]
                        .as_array()
                        .is_some_and(|o| !o.is_empty());
            }
        });
    }
    assert!(
        found,
        "received reference fields must retain their checked evidence"
    );
}

#[test]
fn direct_capture_closes_unprojected_ancestry_and_preserves_status_in_expansion() {
    let p = program(
        r#"
struct pair { string left; string right; }
async control child(ref mutable string input) exits done { deref input="changed";exit done; }
async control startup() exits done {
    mutable pair owner=pair {"left","right"};
    ref mutable pair whole=borrow mutable owner;
    ref mutable string leaf=borrow mutable whole.left;
    auto pending=defer child(leaf);
    await pending;
    emit(borrow owner.right);exit done;
}
control main() exits done {forward run startup();}
"#,
    );
    let startup = p.regions().find(|r| r.name == "startup").unwrap();
    let mut found = false;
    for i in &startup.code {
        if let Action::Decide {
            operation: Operation::Defer(_),
            args,
            ..
        } = &i.action
        {
            let Expr::WithReferenceEvidence(_, evidence) = &args[0] else {
                panic!("{:?}", args[0])
            };
            let origins = evidence.origins();
            assert!(origins.iter().any(|o| o.place.fields == [0]));
            assert!(
                origins.iter().any(|o| o.place.fields.is_empty()),
                "parent dependency is not narrowed to the child's field"
            );
            assert!(origins.iter().all(|o| !o.guard && !o.opaque));
            found = true;
        }
    }
    assert!(found);
    let scalar = program(
        r#"
async control child(ref mutable string input) exits done {deref input="changed";exit done;}
async control startup() exits done {
    mutable string owner="text";
    ref mutable string first=borrow mutable owner;
    ref mutable string second=borrow mutable deref first;
    auto pending=defer child(second);await pending;exit done;
}
control main() exits done {forward run startup();}
"#,
    );
    let region = scalar.regions().find(|r| r.name == "startup").unwrap();
    assert!(region.code.iter().any(|i|matches!(&i.action,Action::Decide {operation:Operation::Defer(_),args,..} if matches!(&args[0],Expr::WithReferenceEvidence(_,ReferenceEvidence::Complete(_))))));

    let artifact = p.expansion_artifact("startup").unwrap();
    let expanded = p.accept_expansion(&artifact).unwrap();
    let scenario = Scenario {
        entry: "main".into(),
        arguments: vec![],
        calls: vec![],
    };
    assert_eq!(p.run(&scenario).unwrap(), expanded.run(&scenario).unwrap());
    fn weaken(value: &mut Json) -> bool {
        if let Some(parts) = value
            .get_mut("WithReferenceEvidence")
            .and_then(Json::as_array_mut)
        {
            if let Some(origins) = parts[1].get("Complete").cloned() {
                parts[1] = serde_json::json!({"Partial":origins});
                return true;
            }
        }
        match value {
            Json::Array(v) => v.iter_mut().any(weaken),
            Json::Object(v) => v.values_mut().any(weaken),
            _ => false,
        }
    }
    let mut forged = serde_json::to_value(artifact).unwrap();
    assert!(weaken(&mut forged));
    assert_eq!(
        p.accept_expansion(&serde_json::from_value(forged).unwrap())
            .unwrap_err()
            .code,
        "E_EXPANSION_CERTIFICATE"
    );
}

#[test]
fn path_dependent_aliases_and_guarded_captures_stay_partial() {
    let p = program(
        r#"
async control child(ref mutable string input) exits done {emit(input);exit done;}
async control startup(bool selected) exits done {
    mutable string owner="text";
    delta selected {true=>yes,false=>no}
    [yes]:
        ref mutable string local=borrow mutable owner;
        auto pending=defer child(local);await pending;exit done;
    [no]:exit done;
}
control main() exits done {forward run startup(true);}
"#,
    );
    let startup = p.regions().find(|r| r.name == "startup").unwrap();
    assert!(startup.code.iter().any(|i|matches!(&i.action,Action::Decide {operation:Operation::Defer(_),args,..} if matches!(&args[0],Expr::WithReferenceEvidence(_,ReferenceEvidence::Partial(_))))));
    let guarded = program(include_str!("../playground/guarded_task_captures.tellur"));
    let exclusive = guarded.regions().find(|r| r.name == "exclusive").unwrap();
    assert!(exclusive.code.iter().any(|i|matches!(&i.action,Action::Decide {operation:Operation::Defer(_),args,..} if matches!(&args[0],Expr::WithReferenceEvidence(_,ReferenceEvidence::Partial(_))))));
    let shared = guarded.regions().find(|r| r.name == "shared").unwrap();
    assert!(
        capture(shared)
            .iter()
            .all(|o| !o.guard && shared.slots[&o.place.root].name == "owner")
    );
}

#[test]
fn local_guarded_closure_survives_expansion_and_rejects_weakened_certificates() {
    let p = program(include_str!("../playground/local_guarded_tasks.tellur"));
    let startup = p.regions().find(|r| r.name == "startup").unwrap();
    assert!(capture_evidence(startup).is_complete());
    let artifact = p.expansion_artifact("startup").unwrap();
    let encoded = serde_json::to_value(&artifact).unwrap();
    let decoded = serde_json::from_value(encoded.clone()).unwrap();
    let expanded = p.accept_expansion(&decoded).unwrap();
    let scenario = Scenario {
        entry: "main".into(),
        arguments: vec![],
        calls: vec![],
    };
    assert_eq!(p.run(&scenario).unwrap(), expanded.run(&scenario).unwrap());
    fn mutate(v: &mut Json, kind: usize) -> bool {
        if let Some(parts) = v
            .get_mut("WithReferenceEvidence")
            .and_then(Json::as_array_mut)
        {
            if let Some(origins) = parts[1].get_mut("Complete").and_then(Json::as_array_mut) {
                if let Some(i) = origins.iter().position(|o| {
                    o["guard"] == true && o["place"]["fields"].as_array().unwrap().is_empty()
                }) {
                    match kind {
                        0 => {
                            origins.remove(i);
                        }
                        1 => origins[i]["guard"] = false.into(),
                        2 => origins[i]["opaque"] = false.into(),
                        3 => origins[i]["place"]["fields"]
                            .as_array_mut()
                            .unwrap()
                            .push(0.into()),
                        4 => {
                            let origins = Json::Array(origins.clone());
                            parts[1] = serde_json::json!({"Partial": origins});
                        }
                        5 => origins[i]["place"]["indirect"] = true.into(),
                        _ => unreachable!(),
                    }
                    return true;
                }
            }
        }
        match v {
            Json::Array(v) => v.iter_mut().any(|v| mutate(v, kind)),
            Json::Object(v) => v.values_mut().any(|v| mutate(v, kind)),
            _ => false,
        }
    }
    for kind in 0..6 {
        let mut forged = encoded.clone();
        assert!(mutate(&mut forged, kind));
        assert_eq!(
            p.accept_expansion(&serde_json::from_value(forged).unwrap())
                .unwrap_err()
                .code,
            "E_EXPANSION_CERTIFICATE"
        );
    }
    // Conditional guard ancestry and returned permission families stay partial.
    let conditional = include_str!("../playground/local_guarded_tasks.tellur")
        .replace("async control startup()", "async control startup(bool selected)")
        .replace("    mutable string owner", "    delta selected { true => yes, false => no }\n    [no]: exit done;\n    [yes]:\n    mutable string owner")
        .replace("run startup()", "run startup(true)");
    let p = program(&conditional);
    assert!(!capture_evidence(p.regions().find(|r| r.name == "startup").unwrap()).is_complete());
}

#[test]
fn incoming_singleton_record_fields_preserve_transport_markers_across_expansion() {
    let source = r#"
struct view['a] { ref string text <- 'a; }
struct envelope['a] { view['a] nested; }
struct grouped['a] { ref string left <- 'a; ref string right <- 'a; }
control owned(view input) exits done {emit(input.text);drop(input);exit done;}
control borrowed(ref view input) exits done {emit(input.text);exit done;}
control nested(ref envelope input) exits done {emit(input.nested.text);exit done;}
control unioned(ref grouped input) exits done {emit(input.left);emit(input.right);exit done;}
control main() exits done {
    string owner="hello";
    view saved=view {borrow owner};
    borrowed(borrow saved);
    owned(move saved);
    envelope wrapped=envelope {view {borrow owner}};
    nested(borrow wrapped);
    drop(wrapped);
    grouped pair=grouped {borrow owner, borrow owner};
    unioned(borrow pair);
    drop(pair);
    exit done;
}
"#;
    let p = program(source);
    for (name, expected) in [("owned", 1), ("borrowed", 1), ("nested", 1), ("unioned", 0)] {
        let r = p.regions().find(|r| r.name == name).unwrap();
        let count = r
            .code
            .iter()
            .filter(|i| {
                matches!(
                    &i.action,
                    Action::Bind(
                        _,
                        Expr::WithReferenceEvidence(_, ReferenceEvidence::Incoming)
                    )
                )
            })
            .count();
        assert_eq!(count, expected, "{name}");
    }
    for input in [
        include_str!("../playground/public_carriers.tellur"),
        include_str!("../playground/public_variants.tellur"),
    ] {
        let unsupported = program(input);
        let relay = unsupported.regions().find(|r| r.name == "relay").unwrap();
        walk(&serde_json::to_value(relay).unwrap(), &mut |v| {
            if let Some(parts) = v.get("WithReferenceEvidence").and_then(Json::as_array) {
                assert_ne!(parts[1], "Incoming", "nested-reference or tagged transport");
            }
        });
    }
    let artifact = p.expansion_artifact("main").unwrap();
    let expanded = p.accept_expansion(&artifact).unwrap();
    let scenario = Scenario {
        entry: "main".into(),
        arguments: vec![],
        calls: vec![],
    };
    assert_eq!(p.run(&scenario).unwrap(), expanded.run(&scenario).unwrap());
    let original = serde_json::to_value(&artifact).unwrap();
    fn mutate(value: &mut Json, kind: usize) -> bool {
        if let Some(parts) = value
            .get_mut("WithReferenceEvidence")
            .and_then(Json::as_array_mut)
            && parts[1] == "Incoming"
        {
            match kind {
                0 => parts[1] = serde_json::json!({"Partial": []}),
                1 => parts[1] = serde_json::json!({"Complete": []}),
                2 => {
                    parts[0]["Place"]["fields"] = serde_json::json!([]);
                }
                _ => {
                    *value = parts[0].clone();
                }
            }
            return true;
        }
        match value {
            Json::Array(values) => values.iter_mut().any(|v| mutate(v, kind)),
            Json::Object(values) => values.values_mut().any(|v| mutate(v, kind)),
            _ => false,
        }
    }
    for kind in 0..4 {
        let mut candidate = original.clone();
        assert!(mutate(&mut candidate, kind));
        let candidate: ExpansionArtifact = serde_json::from_value(candidate).unwrap();
        assert_eq!(
            p.accept_expansion(&candidate).unwrap_err().code,
            "E_EXPANSION_CERTIFICATE"
        );
    }
}

#[test]
fn singleton_shared_input_leaves_close_direct_and_aliased_projection_ancestry() {
    let source = include_str!("../playground/incoming_leaf_tasks.tellur");
    let p = program(source);
    for name in ["inspect", "owned"] {
        let r = p.regions().find(|r| r.name == name).unwrap();
        let captures = r
            .code
            .iter()
            .filter_map(|i| match &i.action {
                Action::Decide {
                    operation: Operation::Defer(_),
                    args,
                    ..
                } => Some(&args[0]),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(captures.len(), if name == "inspect" { 2 } else { 1 });
        for value in captures {
            let Expr::WithReferenceEvidence(_, ReferenceEvidence::Complete(origins)) = value else {
                panic!("{name}: {value:?}")
            };
            assert!(
                origins
                    .iter()
                    .all(|o| o.place.indirect && !o.guard && !o.opaque)
            );
            assert!(
                origins.iter().any(|o| o.place.fields.is_empty()),
                "unprojected input pointee retained"
            );
            assert!(origins.iter().any(|o| !o.place.fields.is_empty()));
            assert!(
                origins
                    .iter()
                    .all(|o| r.slots[&o.place.root].name.starts_with("$stored_address_"))
            );
        }
    }
    let artifact = p.expansion_artifact("startup").unwrap();
    let expanded = p.accept_expansion(&artifact).unwrap();
    let scenario = Scenario {
        entry: "main".into(),
        arguments: vec![],
        calls: vec![],
    };
    assert_eq!(p.run(&scenario).unwrap(), expanded.run(&scenario).unwrap());
    let artifact = p.expansion_artifact("inspect").unwrap();
    fn weaken(value: &mut Json, partial: bool) -> bool {
        if let Some(parts) = value
            .get_mut("WithReferenceEvidence")
            .and_then(Json::as_array_mut)
            && let Some(origins) = parts[1].get_mut("Complete").and_then(Json::as_array_mut)
            && origins.iter().any(|o| {
                o["place"]["indirect"] == true
                    && o["place"]["fields"]
                        .as_array()
                        .is_some_and(|f| !f.is_empty())
            })
        {
            if partial {
                parts[1] = serde_json::json!({"Partial":origins});
            } else {
                origins.retain(|o| !o["place"]["fields"].as_array().is_some_and(Vec::is_empty));
            }
            return true;
        }
        match value {
            Json::Array(v) => v.iter_mut().any(|v| weaken(v, partial)),
            Json::Object(v) => v.values_mut().any(|v| weaken(v, partial)),
            _ => false,
        }
    }
    for partial in [false, true] {
        let mut candidate = serde_json::to_value(&artifact).unwrap();
        assert!(weaken(&mut candidate, partial));
        let candidate: ExpansionArtifact = serde_json::from_value(candidate).unwrap();
        assert_eq!(
            p.accept_expansion(&candidate).unwrap_err().code,
            "E_EXPANSION_CERTIFICATE"
        );
    }
    let unsupported = program(
        r#"
struct view['a] {ref mutable string text <- 'a;}
async control reader(ref string input) exits done {exit done;}
async control exclusive(ref mutable view input) exits done {
    auto pending=defer reader(input.text);await pending;exit done;
}
struct shared['a] {ref string text <- 'a;}
async control conditional(shared input,bool choose) exits done {
    delta choose {true=>yes,false=>no}
    [yes]: ref string selected=input.text;auto pending=defer reader(selected);await pending;
    [entry]: drop(input);exit done;
}
control main() exits done {exit done;}
"#,
    );
    for name in ["exclusive", "conditional"] {
        assert!(
            !capture_evidence(unsupported.regions().find(|r| r.name == name).unwrap())
                .is_complete(),
            "{name}"
        );
    }
    // A second leaf in the same public group is not a singleton proof.
    let grouped = source
        .replace(
            "ref text_pair values <- 'a;",
            "ref text_pair values <- 'a; ref text_pair other <- 'a;",
        )
        .replace(
            "text_view { borrow owner }",
            "text_view { borrow owner, borrow owner }",
        );
    let grouped = program(&grouped);
    assert!(
        !capture_evidence(grouped.regions().find(|r| r.name == "inspect").unwrap()).is_complete()
    );
    assert!(
        !capture_evidence(grouped.regions().find(|r| r.name == "owned").unwrap()).is_complete()
    );
    // Extra permissions introduced by a returned-reference contract remain partial.
    let received=source.replace("async control leaf", "control relay(ref text_pair input) exits ready {ref text_pair value from input} {exit ready {input};}\nasync control leaf")
        .replace("ref text_pair copied = input.values;", "delta relay(input.values) {ready {auto received} => relayed}\n[relayed]:\nref text_pair copied = received;");
    let received = program(&received);
    let r = received.regions().find(|r| r.name == "inspect").unwrap();
    assert!(r.code.iter().any(|i|matches!(&i.action,Action::Decide {operation:Operation::Defer(_),args,..} if matches!(&args[0],Expr::WithReferenceEvidence(_,ReferenceEvidence::Partial(_))))));
    // This exception must not certify a whole incoming carrier closure.
    let whole = source
        .replace(
            "async control owned",
            "async control whole(ref text_view input) exits done {exit done;}\nasync control owned",
        )
        .replace(
            "drop(input);",
            "auto carrier = defer whole(borrow input); await carrier; drop(input);",
        );
    let whole = program(&whole);
    let r = whole.regions().find(|r| r.name == "owned").unwrap();
    assert!(r.code.iter().any(|i|matches!(&i.action,Action::Decide {operation:Operation::Defer(n),args,..} if n=="whole" && matches!(&args[0],Expr::WithReferenceEvidence(_,ReferenceEvidence::Partial(_))))));
}
