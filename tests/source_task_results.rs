use tellur::{
    compile,
    manifest::Manifest,
    runtime::{ExecutionProgress, Scenario},
};
fn manifest() -> Manifest {
    Manifest::from_json(include_str!("../playground/manifest.json")).unwrap()
}
fn scenario() -> Scenario {
    Scenario {
        entry: "main".into(),
        arguments: vec![],
        calls: vec![],
    }
}
fn source(body: &str) -> String {
    format!(
        r#"
async control lend(ref string left,ref string right) exits ready {{ref string value from left}} {{yield;exit ready {{left}};}}
async control lend_mut(ref mutable string text) exits ready {{ref mutable string value from text}} {{yield;exit ready {{text}};}}
async control startup() exits done {{{body}}}
control main() exits done {{forward run startup();}}
"#
    )
}
#[test]
fn selected_result_origins_survive_completion_join_and_quota_partitions() {
    let p = compile(
        include_str!("../playground/borrowed_spawn_results.tellur"),
        manifest(),
    )
    .unwrap();
    let expected = p.run_completion(&scenario(), 1000).unwrap();
    let value = serde_json::to_value(&expected).unwrap();
    let messages: Vec<_> = value["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|e| e["message"].as_str())
        .collect();
    assert_eq!(
        messages,
        [
            "queued",
            "released at join",
            "edited through result",
            "released after result",
            "right"
        ]
    );
    for p in [p.clone(), p.expanded("main").unwrap()] {
        for quota in [1, 3, usize::MAX] {
            let input = scenario();
            let mut session = p.start_execution(&input, 1000);
            assert!(matches!(session.advance(0), ExecutionProgress::Paused));
            loop {
                match session.advance(quota) {
                    ExecutionProgress::Paused => {}
                    ExecutionProgress::Complete(result) => {
                        assert_eq!(result.unwrap(), expected);
                        break;
                    }
                    other => panic!("{other:?}"),
                }
            }
        }
    }
}
#[test]
fn loans_last_through_unjoined_completion_and_selected_result_use() {
    for pause in ["", "yield;", "yield;yield;yield;"] {
        let s = source(&format!(
            "mutable string a=\"a\";mutable string b=\"b\";auto task=spawn lend(borrow a,borrow b);{pause}auto moved=move task;delta await moved {{ready {{auto value}}=>ready}} [ready]:b=\"free\";emit(value);a=\"free\";exit done;"
        ));
        compile(&s, manifest()).unwrap().run(&scenario()).unwrap();
        for bad in [
            s.replace("auto moved=move task;", "a=\"bad\";auto moved=move task;"),
            s.replace("b=\"free\";emit(value)", "a=\"bad\";emit(value)"),
            s.replace("b=\"free\";emit(value)", "drop(a);emit(value)"),
        ] {
            assert_eq!(
                compile(&bad, manifest()).unwrap_err().code,
                "E_LOAN",
                "{bad}"
            );
        }
    }
}
#[test]
fn exclusive_result_adoption_keeps_reference_ancestry() {
    let s = source(
        "mutable string text=\"old\";ref mutable string alias=borrow mutable text;auto task=spawn lend_mut(alias);yield;yield;delta await task {ready {auto value}=>ready} [ready]:deref value=\"result\";emit(value);deref alias=\"parent\";emit(alias);exit done;",
    );
    compile(&s, manifest()).unwrap().run(&scenario()).unwrap();
    assert_eq!(
        compile(
            &s.replace(
                "deref value=\"result\";",
                "deref alias=\"bad\";deref value=\"result\";"
            ),
            manifest()
        )
        .unwrap_err()
        .code,
        "E_LOAN"
    );
    let moved = s.replace("spawn lend_mut(alias)", "spawn lend_mut(move alias)");
    assert_eq!(compile(&moved, manifest()).unwrap_err().code, "E_MOVED");
    compile(
        &moved.replace(
            "deref alias=\"parent\";emit(alias);",
            "text=\"parent\";emit(borrow text);",
        ),
        manifest(),
    )
    .unwrap()
    .run(&scenario())
    .unwrap();
}
#[test]
fn completion_alternatives_and_instances_do_not_share_result_loans() {
    let prefix = r#"async control choose(bool yes,ref string a,ref string b) exits chosen {ref string value from a}, absent {
        yield;delta yes {true=>yes,false=>no} [yes]:exit chosen {a};[no]:exit absent;
    }"#;
    for yes in [true, false] {
        let s = format!(
            "{prefix} async control startup() exits done {{mutable string a=\"a\";mutable string b=\"b\";auto first=spawn choose({yes},borrow a,borrow b);auto second=spawn choose({yes},borrow b,borrow a);yield;yield;delta await first {{chosen {{auto x}}=>x,absent=>none}} [x]:emit(x);[x|none]:delta await second {{chosen {{auto y}}=>y,absent=>empty}} [y]:a=\"free\";emit(y);b=\"free\";exit done;[empty]:a=\"free\";b=\"free\";exit done;}} control main() exits done {{forward run startup();}}"
        );
        compile(&s, manifest()).unwrap().run(&scenario()).unwrap();
        assert_eq!(
            compile(
                &s.replace("[y]:a=\"free\";emit(y)", "[y]:b=\"bad\";emit(y)"),
                manifest()
            )
            .unwrap_err()
            .code,
            "E_LOAN"
        );
    }
}
#[test]
fn generic_forwarding_and_stored_adapters_preserve_selected_origins() {
    let s = r#"
async control lend<t>(ref t text) exits ready {ref t value from text} {yield;exit ready {text};}
async control wrapper(ref string text) exits ready {ref string value from text} {
    auto child=spawn lend<string>(text);forward await child;
}
async control startup() exits done {
    mutable string text="borrowed";auto task=spawn wrapper(borrow text);
    yield;auto result=capture await task;
    delta result {ready {auto value}=>ready} [ready]:emit(value);text="free";exit done;
}
control main() exits done {forward run startup();}
"#;
    let p = compile(s, manifest()).unwrap();
    let expected = p.run(&scenario()).unwrap();
    assert_eq!(expected.events.len(), 1);
    assert_eq!(
        p.expanded("main").unwrap().run(&scenario()).unwrap(),
        expected
    );
    assert_eq!(
        compile(
            &s.replace("delta result", "text=\"bad\";delta result"),
            manifest()
        )
        .unwrap_err()
        .code,
        "E_LOAN"
    );
    let inline = s.replace(
        "auto result=capture await task;",
        "auto result=delta await task {ready {auto x}=>ready {x}};",
    );
    compile(&inline, manifest())
        .unwrap()
        .run(&scenario())
        .unwrap();
}
#[test]
fn local_and_owned_storage_cannot_escape_as_spawned_results() {
    for s in [
        "async control bad(string text) exits auto {exit ready {borrow text};} async control caller() exits done {auto child=spawn bad(\"owned\");delta await child {ready {auto view}=>ready} [ready]:emit(view);exit done;}",
        "async control lend(ref string text) exits ready {ref string value from text} {exit ready {text};} async control bad() exits auto {string text=\"local\";auto child=spawn lend(borrow text);forward await child;}",
    ] {
        assert_eq!(compile(s, manifest()).unwrap_err().code, "E_LIFETIME");
    }
}
#[test]
fn completed_borrowed_result_settles_before_the_owning_contingency() {
    let text = include_str!("fixtures/source_borrowed_result_fault.tellur");
    for text in [
        text.to_owned(),
        text.replace(
            "    yield;\n    yield;\n    delta await result",
            "    delta await result",
        )
        .replace("emit(\"stopped parent resumed\");", "yield;yield;"),
    ]
    .into_iter()
    .flat_map(|text| {
        let nested = text
            .replace("control main()", "async control outer() exits done faults {await startup();exit done;} control main()")
            .replace("run startup();", "run outer();");
        [text, nested]
    }) {
        let p = compile(&text, manifest()).unwrap();
        for p in [p.clone(), p.expanded("main").unwrap()] {
            let result = p.run_completion(&scenario(), 1000).unwrap();
            let value = serde_json::to_value(result).unwrap();
            assert_eq!(value["status"], 23);
            let messages: Vec<_> = value["events"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|e| e["message"].as_str())
                .collect();
            assert_eq!(
                messages,
                ["result completed", "sibling started", "owner contingency"]
            );
        }
    }
}

#[test]
fn direct_results_keep_detached_carrier_guard_authority() {
    let source = include_str!("../playground/guarded_task_captures.tellur")
        .replace("defer ", "spawn ")
        .replace(
            "auto moved = move pending;",
            "yield;yield;auto moved = move pending;",
        );
    let p = compile(&source, manifest()).unwrap();
    let expected = p.run(&scenario()).unwrap();
    assert_eq!(expected.events.len(), 3);
    assert_eq!(
        p.expanded("main").unwrap().run(&scenario()).unwrap(),
        expected
    );
    for marker in ["auto moved = move pending;", "deref value = \"after\";"] {
        for invalid in [
            "drop(saved);",
            "owner=\"bad\";",
            "deref carrier.text=\"bad\";",
            "deref detached=\"bad\";",
        ] {
            let bad = source.replacen(marker, &format!("{invalid}{marker}"), 1);
            assert_eq!(
                compile(&bad, manifest()).unwrap_err().code,
                "E_LOAN",
                "{invalid}"
            );
        }
    }
}
