use tellur::{compile, ir::Action, manifest::Manifest};
fn manifest() -> Manifest {
    Manifest::from_json(include_str!("../examples/manifest.json")).unwrap()
}
#[test]
fn qualified_patterns_resolve_and_aliases_cannot_duplicate_arms() {
    compile("region test(x: Option<U32>) exits Done { [Entry]: delta x -> [Option.None => N,Option.Some(n) => S]; [Entry]: exit Done; }",manifest()).unwrap();
    for (pattern, code) in [
        ("Wrong.None => N,Some(x) => S", "E_ARMS"),
        ("None => N,Option.None => N2,Some(x) => S", "E_ARMS"),
    ] {
        assert_eq!(compile(&format!("region test(x: Option<U32>) exits Done {{ [Entry]: delta x -> [{pattern}]; [Entry]: exit Done; }}"),manifest()).unwrap_err().code,code);
    }
    compile("region test(x: FileState) exits Done(file: Option<Handle>) { [Entry]: delta x -> [FileState.Missing => N,FileState.Present(h) => P,FileState.Blocked(reason) => B]; [N | B]: exit Done(None); [P]: exit Done(Some(h)); }",manifest()).unwrap();
}
#[test]
fn expansion_preserves_named_sites_and_internal_exit_contributors() {
    let source = "region classify(flag: Bool) exits Failed { [Entry]: choose = delta flag -> [true => A,false => B]; [A]: exit Failed; [B]: exit Failed; } region caller(flag: Bool) exits Done { [Entry]: first = delta classify(flag) -> [Failed => F1]; second = delta classify(flag) -> [Failed => F2]; [Entry]: exit Done; }";
    let original = compile(source, manifest()).unwrap();
    let p = original.expanded("caller").unwrap();
    p.verify().unwrap();
    let code = &p.region("caller").unwrap().code;
    let completions = code
        .iter()
        .filter(|i| matches!(i.action, Action::Complete { .. }))
        .collect::<Vec<_>>();
    assert_eq!(completions.len(), 4);
    assert!(
        completions
            .iter()
            .all(|i| i.provenance.region == "classify")
    );
    let description = p.inspect("caller").unwrap();
    assert!(
        description.contains("caller::first#")
            && description.contains("caller::second#")
            && description.contains("site choose")
    );
    assert_eq!(
        code.iter()
            .filter(|i| i.provenance.site.as_deref() == Some("choose"))
            .count(),
        2
    );
}
#[test]
fn malformed_source_never_panics_for_bounded_generated_inputs() {
    let tokens = [
        "region",
        "main",
        "(",
        ")",
        "exits",
        "Done",
        "{",
        "}",
        "[",
        ":",
        ";",
        "delta",
        "->",
        "=>",
        "Some",
        "None",
        "Option",
        "<",
        ">",
        "\"é😀\"",
        "@",
        "again",
        "loop",
        "&",
        "|",
        ":=",
        "0u32",
    ];
    let mut seed = 0x51a7_u64;
    for _ in 0..1500 {
        let mut source = String::new();
        for _ in 0..40 {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            source.push_str(tokens[(seed >> 32) as usize % tokens.len()]);
            source.push(' ');
        }
        let result = std::panic::catch_unwind(|| compile(&source, manifest()));
        assert!(result.is_ok(), "panicked on {source}");
    }
}

#[test]
fn malformed_boolean_driver_representation_is_rejected() {
    use tellur::{ir::Value, runtime::Scenario};
    let p=compile("region test(flag: Bool) exits Done { [Entry]: delta flag -> [true => A,false => B]; [Entry]: exit Done; }",manifest()).unwrap();
    let s = Scenario {
        entry: "test".into(),
        arguments: vec![Value::Construct {
            ty: "Bool".into(),
            name: "true".into(),
            fields: vec![],
        }],
        calls: vec![],
    };
    assert_eq!(p.run(&s).unwrap_err().code, "E_DRIVER");
}

#[test]
fn bounded_mutations_of_valid_source_are_diagnosed_without_panics() {
    let source = include_str!("../examples/optional_handle.tellur");
    for (index, _) in source.char_indices().step_by(3) {
        let mut edited = source.to_string();
        edited.remove(index);
        assert!(
            std::panic::catch_unwind(|| compile(&edited, manifest())).is_ok(),
            "panic at byte {index}"
        );
    }
}
