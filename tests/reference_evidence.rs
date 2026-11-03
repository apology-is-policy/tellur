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

fn capture(region: &Region) -> &[ReferenceOrigin] {
    region
        .code
        .iter()
        .find_map(|instruction| match &instruction.action {
            Action::Decide {
                operation: Operation::Defer(_),
                args,
                ..
            } => match &args[0] {
                Expr::WithReferenceEvidence(_, ReferenceEvidence::Partial(origins)) => {
                    Some(origins.as_slice())
                }
                other => panic!("missing explicit partial evidence: {other:?}"),
            },
            _ => None,
        })
        .unwrap()
}

#[test]
fn captures_preserve_erased_alias_guards_without_inventing_shared_holder_dependencies() {
    let source = include_str!("../playground/guarded_task_captures.tellur");
    let direct = source.replace(
        "    delta detach(carrier) {\n        ready { auto detached } => borrowed\n    }\n\n    [borrowed]:",
        "    ref mutable string detached = borrow mutable deref carrier.text;",
    );
    assert_ne!(source, direct);
    for source in [source, direct.as_str()] {
        let p = program(source);
        let exclusive = p.regions().find(|r| r.name == "exclusive").unwrap();
        let origins = capture(exclusive);
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
        let origins = parts[1]["Partial"].as_array_mut().unwrap();
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
    assert!(
        serde_json::from_value::<ReferenceEvidence>(serde_json::json!({"Complete": []})).is_err()
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
