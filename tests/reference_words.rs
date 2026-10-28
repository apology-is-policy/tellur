mod support;
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
fn rejects(source: &str, code: &str) {
    let e = compile(source, m()).unwrap_err();
    assert_eq!(e.code, code, "{e}");
}
const SOURCE: &str = r#"
struct contents {string text;}
struct message {contents contents;}
trait reader {type t_item;control peek(ref self input) exits found {ref t_item value from input};}
implement reader for message {
 type t_item=string;
 control peek(ref self input) exits found {ref t_item value from input} {exit found {borrow input.contents.text};}
}
control peek<r:reader>(ref r input) exits found {ref reader<r>.t_item value from input} {forward reader<r>.peek(input);}
control change(ref mutable message input) exits done {input.contents.text="changed";exit done;}
control main() exits done {string text} {
 mutable message source=message {contents {"before"}};
 change(borrow mutable source);
 exit delta peek<message>(borrow source) {found {ref string text}=>done {clone(text)}};
}
"#;
#[test]
fn reference_words_work_through_generic_traits_and_nested_field_access() {
    let p = compile(SOURCE, m()).unwrap();
    let result = p.run(&scenario()).unwrap();
    assert_eq!(result.fields, vec![Value::String("changed".into())]);
    assert_eq!(
        result,
        p.expanded("main").unwrap().run(&scenario()).unwrap()
    );
    let explicit = SOURCE
        .replace(
            "borrow input.contents.text",
            "borrow (deref input).contents.text",
        )
        .replace("input.contents.text=", "(deref input).contents.text=");
    assert_eq!(
        result,
        compile(&explicit, m()).unwrap().run(&scenario()).unwrap()
    );
    let legacy = explicit
        .replace("deref ", "*")
        .replace("implement reader", "impl reader")
        .replace("ref mutable ", "&mutable ")
        .replace("ref ", "&")
        .replace("borrow mutable ", "&mutable ")
        .replace("borrow ", "&")
        .replace("deref ", "*");
    assert_eq!(
        result,
        compile(&legacy, m()).unwrap().run(&scenario()).unwrap()
    );
}
#[test]
fn deref_is_an_addressable_place_and_parentheses_do_not_turn_it_into_a_call() {
    let source = r#"control change(ref mutable string input) exits done {deref(input)="changed";exit done;}
 control main() exits done {string text,u32 n} {
 mutable string owner="before";ref mutable string editor=borrow mutable owner;change(editor);
 mutable u32 n=1;ref mutable u32 number=borrow mutable n;deref number=2;
 ref string view=borrow owner;emit(view);exit done {owner,n};}"#;
    let p = compile(source, m()).unwrap();
    let result = p.run(&scenario()).unwrap();
    assert_eq!(
        result.fields,
        vec![Value::String("changed".into()), Value::U32(2)]
    );
    assert_eq!(
        result,
        p.expanded("main").unwrap().run(&scenario()).unwrap()
    );
}
#[test]
fn automatic_field_access_does_not_grant_mutation_or_move_permissions() {
    rejects(
        &SOURCE.replace(
            "change(ref mutable message input)",
            "change(ref message input)",
        ),
        "E_MUTABILITY",
    );
    rejects(
        &SOURCE.replace(
            "exit found {borrow input.contents.text};",
            "auto stolen=input.contents.text;exit found {borrow stolen};",
        ),
        "E_BORROW_MOVE",
    );
    rejects(
        &SOURCE.replace(
            "exit found {borrow input.contents.text};",
            "exit found {borrow mutable input.contents.text};",
        ),
        "E_MUTABILITY",
    );
    let source = r#"struct pair {string left;string right;}
 control main() exits done {mutable pair owner=pair {"left","right"};ref mutable pair input=borrow mutable owner;
 ref string child=borrow input.left;input.left="bad";emit(child);exit done;}"#;
    rejects(source, "E_LOAN");
    compile(
        &source.replace("input.left=\"bad\"", "input.right=\"fine\""),
        m(),
    )
    .unwrap();
}
#[test]
fn automatic_field_access_preserves_liveness_moves_and_mutable_binding_distinction() {
    let source = r#"struct box {string text;}
 control main() exits done {mutable box owner=box {"before"};ref box view=borrow owner;
 owner.text="changed";emit(borrow view.text);exit done;}"#;
    rejects(source, "E_LOAN");
    compile(
        &source.replace(
            "owner.text=\"changed\";emit(borrow view.text);",
            "emit(borrow view.text);owner.text=\"changed\";",
        ),
        m(),
    )
    .unwrap();
    rejects(
        "control main() exits done {mutable string owner=\"before\";mutable ref string view=borrow owner;exit done;}",
        "E_UNSUPPORTED",
    );
    rejects(
        "control main() exits done {mutable string owner=\"before\";ref mutable string first=borrow mutable owner;auto second=first;deref first=\"bad\";emit(second);exit done;}",
        "E_MOVED",
    );
}
#[test]
fn path_intersection_and_boolean_operators_keep_their_meanings() {
    let source = r#"control main() exits done {string source="hello";ref string view=borrow source;
 delta true {true=>yes,false=>no}
 [yes]:delta (true && !false) {true=>ready,false=>bad}
 [yes & ready]:emit(view);exit done;
 [no | bad]:exit done;}"#;
    let p = compile(source, m()).unwrap();
    assert_eq!(
        p.run(&scenario()).unwrap(),
        p.expanded("main").unwrap().run(&scenario()).unwrap()
    );
}
#[test]
fn native_word_spelling_matches_interpreter_and_certified_expansion() {
    let p = compile(SOURCE, m()).unwrap();
    let expected = p.run(&scenario()).unwrap();
    let root = std::env::temp_dir().join(format!("path-reference-words-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    for expanded in [false, true] {
        let p = if expanded {
            p.expanded("main").unwrap()
        } else {
            p.clone()
        };
        for opt in [0, 2] {
            let binary = root.join(format!("words-{expanded}-{opt}"));
            support::scenario_executable(&p)
                .build_native("main", &binary, opt)
                .unwrap();
            let actual = std::process::Command::new(binary)
                .arg("--json")
                .output()
                .unwrap();
            assert!(
                actual.status.success(),
                "{}",
                String::from_utf8_lossy(&actual.stderr)
            );
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&actual.stdout).unwrap(),
                serde_json::to_value(&expected).unwrap()
            );
        }
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn reference_auto_infers_referents_without_weakening_modes_or_ownership() {
    let source = r#"control echo(ref mutable string input) exits ready {ref mutable string text from input} {exit ready {input};}
    control main() exits done {string text,u32 count} {
        mutable string first="before";
        ref mutable auto first_b=borrow mutable first;
        ref mutable auto moved=first_b;
        delta echo(moved) {ready {ref mutable auto returned}=>ready}
        [ready]: deref returned="after";
        mutable u32 number=1;ref mutable auto counter=borrow mutable number;deref counter=2;
        ref auto view=borrow first;ref auto copy=view;emit(copy);
        exit done {first,number};
    }"#;
    let p = compile(source, m()).unwrap();
    let result = p.run(&scenario()).unwrap();
    assert_eq!(
        result.fields,
        vec![Value::String("after".into()), Value::U32(2)]
    );
    assert_eq!(
        result,
        p.expanded("main").unwrap().run(&scenario()).unwrap()
    );
    rejects(
        &source.replace("ref mutable auto first_b", "ref auto first_b"),
        "E_TYPE",
    );
    rejects(
        &source.replace("ref auto view", "ref mutable auto view"),
        "E_TYPE",
    );
    rejects(
        &source.replace(
            "ready {ref mutable auto returned}",
            "ready {ref auto returned}",
        ),
        "E_TYPE",
    );
    rejects(
        &source.replace(
            "delta echo(moved)",
            "deref first_b=\"bad\";delta echo(moved)",
        ),
        "E_MOVED",
    );
    rejects(
        &source.replace(
            "deref returned=\"after\";",
            "first=\"bad\";deref returned=\"after\";",
        ),
        "E_LOAN",
    );
    rejects(
        &source.replace("mutable string first", "string first"),
        "E_MUTABILITY",
    );
    rejects(
        "control bad(ref auto input) exits done {exit done;}",
        "E_MANIFEST",
    );
}
#[test]
fn reference_auto_also_infers_stored_reference_fields_and_shared_payloads() {
    let source = r#"struct view {ref string text;}
    control peek(ref string input) exits ready... {exit ready {input};}
    control main() exits done... {
        string owner="hello";view record=view {borrow owner};ref auto field=record.text;
        exit delta peek(field) {ready {ref auto text}=>done {clone(text)}};
    }"#;
    let p = compile(source, m()).unwrap();
    assert_eq!(
        p.run(&scenario()).unwrap().fields,
        vec![Value::String("hello".into())]
    );
    assert_eq!(
        p.run(&scenario()).unwrap(),
        p.expanded("main").unwrap().run(&scenario()).unwrap()
    );
}
