use tellur::{compile, ir::Value, manifest::Manifest, runtime::Scenario};
fn m() -> Manifest {
    Manifest::from_json(include_str!("../playground/manifest.json")).unwrap()
}
fn scenario() -> Scenario {
    Scenario {
        entry: "main".into(),
        arguments: vec![],
        calls: vec![],
    }
}
const SOURCE: &str = r#"
struct box<item>{item value;}
outcome ready<item>{ready {item value}}
control identity<t>(t value) exits ready<t> {exit ready {value};}
control wrap<t>(t value) exits auto {emit("wrap");exit delta identity<t>(value) {ready {auto item}=>ready {box<t>{item}}};}
control main() exits auto {emit("before");exit delta wrap<string>("Ada") {ready {auto wrapped}=>done {wrapped.value}};}
"#;
#[test]
fn unconstrained_generic_controls_transfer_ownership_and_infer_parametric_contracts() {
    let p = compile(SOURCE, m()).unwrap();
    let actual = p.run(&scenario()).unwrap();
    assert_eq!(actual.fields, vec![Value::String("Ada".into())]);
    assert_eq!(
        actual,
        p.expanded("main").unwrap().run(&scenario()).unwrap()
    );
    assert_eq!(p.generic_controls().len(), 2);
    assert_eq!(
        p.generic_controls()
            .iter()
            .find(|g| g.name == "wrap")
            .unwrap()
            .exits[0]
            .fields[0]
            .ty,
        "box<t>"
    );
    assert!(
        !p.manifest()
            .types
            .iter()
            .any(|t| t.name().contains("_path_poly_"))
    );
}
#[test]
fn generic_bodies_are_checked_even_unused_and_never_gain_concrete_capabilities() {
    for (body, code) in [
        ("exit done;", "E_RESOURCE"),
        (
            "auto duplicate=value;exit done {value,duplicate};",
            "E_MOVED",
        ),
        (
            "auto duplicate=clone(&value);exit done {value,duplicate};",
            "E_CLONE",
        ),
        ("emit(value);exit done {value};", "E_TYPE"),
    ] {
        for suffix in ["", "control main() exits auto {forward bad<u32>(1);}"] {
            let source = format!("control bad<t>(t value) exits auto {{{body}}} {suffix}");
            let e = compile(&source, m()).unwrap_err();
            assert_eq!(e.code, code, "{e}");
            assert!(e.message.contains("generic definition check"), "{e}");
        }
    }
}
#[test]
fn resource_and_outcome_values_can_be_transferred_generically() {
    let source = "control identity<t>(t value) exits ready {t value} {exit ready {value};} control main(handle file) exits ready {handle value} {forward identity<handle>(file);}";
    compile(source, m()).unwrap();
    let source = "outcome answer {yes,no} control identity<t>(t value) exits ready {t value} {exit ready {value};} control main() exits auto {exit delta identity<answer>(answer.yes {}) {ready {auto value}=>delta value {yes=>done {},no=>done {}}};}";
    let p = compile(source, m()).unwrap();
    assert_eq!(
        p.run(&scenario()).unwrap(),
        p.expanded("main").unwrap().run(&scenario()).unwrap()
    );
}
#[test]
fn generic_shared_and_exclusive_borrow_boundaries_preserve_origins() {
    let source = r#"control view<t>(&t value) exits found {&t value} {exit found {value};}
control replace<t>(&mutable t target,t next) exits ready {t previous} {t previous=*target;*target=next;exit ready {previous};}
control main() exits auto {string value="Ada";exit delta view<string>(&value) {found {auto ref}=>done {clone(ref)}};}"#;
    // Moving out of a borrow is forbidden for an unconstrained type, even if some
    // concrete instantiations could copy. The valid view remains independently useful.
    let e = compile(source, m()).unwrap_err();
    assert!(e.message.contains("generic definition check"), "{e}");
    let source = source
        .lines()
        .filter(|l| !l.starts_with("control replace"))
        .collect::<Vec<_>>()
        .join("\n");
    let p = compile(&source, m()).unwrap();
    assert_eq!(
        p.run(&scenario()).unwrap(),
        p.expanded("main").unwrap().run(&scenario()).unwrap()
    );
}
#[test]
fn explicit_arguments_and_acyclic_call_graphs_are_required() {
    for source in [
        "control id<t>(t value) exits auto {exit done {value};}control main() exits auto {forward id(1);}",
        "control id<t>(t value) exits auto {exit done {value};}control main() exits auto {forward id<u32,string>(1);}",
        "control bad<t,t>(t value) exits auto {exit done {value};}",
        "control bad<string>(string value) exits auto {exit done {value};}",
    ] {
        assert_eq!(compile(source, m()).unwrap_err().code, "E_GENERIC");
    }
    let source = "control again<t>(t value) exits done {forward again<option<t>>(some {value});}";
    assert_eq!(compile(source, m()).unwrap_err().code, "E_RECURSION");
    let source = "control id<t>(t value) exits auto {exit done {value};}control main() exits auto {forward id<&string>(\"x\");}";
    assert!(compile(source, m()).is_err());
}
#[test]
fn producer_indexed_capture_and_generic_calls_in_nested_adapters_work() {
    let source = "control id<t>(t value) exits done {t value} {exit done {value};}control main() exits auto {outcome<id<string>> saved=capture id<string>(\"Ada\");exit delta saved {done {auto value}=>delta id<string>(value) {done {auto again}=>done {again}}};}";
    let p = compile(source, m()).unwrap();
    assert_eq!(
        p.run(&scenario()).unwrap(),
        p.expanded("main").unwrap().run(&scenario()).unwrap()
    );
}
#[test]
fn native_generic_controls_match_reference_and_certified_expansion() {
    use std::{fs, process::Command};
    let p = compile(SOURCE, m()).unwrap();
    let expected = serde_json::to_value(p.run(&scenario()).unwrap()).unwrap();
    let root = std::env::temp_dir().join(format!("path-generic-controls-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let file = root.join("scenario.json");
    fs::write(&file, serde_json::to_vec(&scenario()).unwrap()).unwrap();
    for (name, program) in [
        ("original", p.clone()),
        ("expanded", p.expanded("main").unwrap()),
    ] {
        for opt in [0, 2] {
            let bin = root.join(format!("{name}-{opt}"));
            program.build_native("main", &bin, opt).unwrap();
            let out = Command::new(bin)
                .arg("--scenario")
                .arg(&file)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&out.stdout).unwrap(),
                expected
            );
        }
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn separate_type_parameters_never_unify_just_because_one_call_uses_equal_types() {
    let source = "control bad<a,b>(a first,b second) exits ready {a first,a second} {exit ready {first,second};}control main() exits auto {forward bad<u32,u32>(1,2);}";
    let e = compile(source, m()).unwrap_err();
    assert_eq!(e.code, "E_TYPE");
    assert!(e.message.contains("generic definition check"));
}
#[test]
fn extraction_accepts_concrete_callers_and_rejects_generic_template_selection() {
    let source = "control id<t>(t value) exits auto {\n exit ready {value};\n}\ncontrol main() exits auto {\n string value=\"Ada\";\n forward id<string>(value);\n}";
    let e = tellur::extract::extract_tail(source, m(), "id", 2, "finish").unwrap_err();
    assert_eq!(e.code, "E_EXTRACT");
    let result = tellur::extract::extract_tail(source, m(), "main", 6, "finish").unwrap();
    assert_eq!(
        compile(source, m()).unwrap().run(&scenario()).unwrap(),
        compile(&result.source, m())
            .unwrap()
            .run(&scenario())
            .unwrap()
    );
    let e = tellur::extract::extract_tail(source, m(), "main", 6, "id").unwrap_err();
    assert_eq!(e.code, "E_EXTRACT");
}
#[test]
fn abstract_parameter_identities_are_unspellable_and_never_escape_metadata() {
    let e = compile(
        "control main(_path_poly_0_parameter value) exits auto {exit done {value};}",
        m(),
    )
    .unwrap_err();
    assert_eq!(e.code, "E_NAME");
    let p = compile(
        "control pair<a,b>(a first,b second) exits auto {exit ready {first,second};}",
        m(),
    )
    .unwrap();
    assert_eq!(p.regions().count(), 0);
    assert_eq!(
        p.generic_controls()[0].exits[0]
            .fields
            .iter()
            .map(|f| f.ty.as_str())
            .collect::<Vec<_>>(),
        ["a", "b"]
    );
}

#[test]
fn generic_call_statements_and_mutable_returned_borrows_work() {
    let source = "control observe<t>(&t value) exits done {emit(\"observed\");exit done;}control access<t>(&mutable t value) exits found {&mutable t value} {exit found {value};}control main() exits auto {mutable string value=\"before\";observe<string>(&value);delta access<string>(&mutable value) {found {auto ref}=>available}[available]:*ref=\"after\";[available]:exit done {value};}";
    let p = compile(source, m()).unwrap();
    assert_eq!(
        p.run(&scenario()).unwrap().fields,
        vec![Value::String("after".into())]
    );
    assert_eq!(
        p.run(&scenario()).unwrap(),
        p.expanded("main").unwrap().run(&scenario()).unwrap()
    );
    let p = compile(include_str!("../playground/generic_controls.tellur"), m()).unwrap();
    assert_eq!(
        p.run(&scenario()).unwrap(),
        p.expanded("main").unwrap().run(&scenario()).unwrap()
    );
}
#[test]
fn specialization_limit_is_reported_before_unbounded_expansion() {
    let calls = (0..1025)
        .map(|n| format!("noop<array<u32,{n}>>();"))
        .collect::<String>();
    let source = format!(
        "control noop<t>() exits done {{exit done;}}control main() exits done {{{calls}exit done;}}"
    );
    let e = compile(&source, m()).unwrap_err();
    assert_eq!(e.code, "E_GENERIC");
    assert!(e.message.contains("1024"));
}

#[test]
fn type_parameters_shadow_constructor_names_and_reject_higher_kinded_use() {
    for body in [
        "auto wrong=item {};",
        "auto wrong=item();",
        "auto wrong=item<u32> {1};",
    ] {
        let source = format!(
            "struct item<t>{{t value;}}control bad<item>(item value) exits auto {{{body}exit ready {{value}};}}"
        );
        assert!(compile(&source, m()).is_err());
    }
    assert_eq!(
        compile(
            "control u32_text<t>(t value) exits auto {exit ready {value};}",
            m()
        )
        .unwrap_err()
        .code,
        "E_NAME"
    );
}

#[test]
fn bare_type_parameter_names_do_not_accidentally_construct_outer_types() {
    let source = "struct item{}control bad<item>(item value) exits auto {auto wrong=item;exit ready {value};}";
    assert_eq!(compile(source, m()).unwrap_err().code, "E_NAME");
    compile("struct item{}control identity<item>(item item) exits auto {exit ready {item};}control main() exits auto {forward identity<string>(\"x\");}",m()).unwrap();
}

#[test]
fn invalid_generic_definitions_are_reported_before_dependent_caller_arity_errors() {
    let source = "control identity<t>(t value) exits auto {exit delivered;}\ncontrol main() exits done {delta identity<string>(\"Ada\") {delivered {auto value}=>ready}[ready]:emit(value);[entry]:exit done;}";
    let e = compile(source, m()).unwrap_err();
    assert_eq!(e.code, "E_RESOURCE");
    assert_eq!(e.span.line, 1);
    assert!(e.message.contains("generic definition check"));
    let source = "control identity<t>(t value) exits auto {emit(\"generic\");helper();exit delivered {value};}control helper() exits done {emit(\"helper\");exit done;}control main() exits auto {forward identity<string>(\"Ada\");}";
    let p = compile(source, m()).unwrap();
    assert_eq!(
        p.run(&scenario()).unwrap(),
        p.expanded("main").unwrap().run(&scenario()).unwrap()
    );
}
