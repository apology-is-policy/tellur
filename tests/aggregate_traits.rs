mod support;
use tellur::{compile, manifest::Manifest, runtime::Scenario};
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
fn same(source: &str) -> tellur::CheckedProgram {
    let p = compile(source, manifest()).unwrap();
    assert_eq!(
        p.run(&scenario()).unwrap(),
        p.expanded("main").unwrap().run(&scenario()).unwrap()
    );
    p
}
fn rejects(source: &str, code: &str) {
    let e = compile(source, manifest()).unwrap_err();
    assert_eq!(e.code, code, "{e}\n{source}");
    assert!(!e.message.contains("_path_poly_"), "{e}");
}
const SOURCE: &str = include_str!("fixtures/aggregate_traits.tellur");
#[test]
fn nested_named_contracts_survive_associated_types_generic_dispatch_and_capture() {
    let p = same(SOURCE);
    for name in ["editor<text_editor>.prepare", "prepare<text_editor>"] {
        let r = p.region(name).unwrap();
        assert_eq!(r.stored_returns.len(), 2);
        assert!(r.stored_returns.iter().any(|r| !r.through.is_empty()));
    }
}
#[test]
fn trait_results_retain_both_storage_and_contained_owners() {
    rejects(
        &SOURCE.replace(
            "deref job.target.text =",
            "drop(saved); deref job.target.text =",
        ),
        "E_LOAN",
    );
    rejects(
        &SOURCE.replace(
            "deref job.target.text =",
            "text = \"too soon\"; deref job.target.text =",
        ),
        "E_LOAN",
    );
}
#[test]
fn explicit_group_contracts_are_required_even_without_implementations() {
    for source in [
        "struct view['a] {ref string text <- 'a;} trait reader {control get(ref string input) exits ready {view value};}",
        "struct view['a] {ref string text <- 'a;} trait reader {control get(ref view input) exits ready {ref view value from input};}",
        "outcome reply['a] {found {ref string text <- 'a}} trait reader {control get(ref string input) exits reply;}",
        "struct view['a] {ref string text <- 'a;} trait reader {control get(ref view input) exits ready {view[input['wrong]] value};}",
    ] {
        rejects(
            source,
            if source.starts_with("outcome") {
                "E_LIFETIME"
            } else {
                "E_ORIGIN"
            },
        );
    }
    rejects(
        &SOURCE.replace(
            "preparation<item>[owner, owner['text]]",
            "preparation<item>",
        ),
        "E_ORIGIN",
    );
}
const PAIR: &str = r#"
struct pair['a,'b] {ref string left <- 'a;ref string right <- 'b;}
trait pair_reader {control get(ref pair input) exits ready {pair[input['a],input['b]] value};}
struct reader {u32 tag;}
implement pair_reader for reader {control get(ref pair owner) exits ready {pair[owner['a],owner['b]] result} {
exit ready {pair {owner.left,owner.right}};}}
control main() exits done {string left="left";string right="right";pair p=pair {borrow left,borrow right};
delta pair_reader<reader>.get(borrow p) {ready {auto result}=>ready}
[ready]:emit(result.left);emit(result.right);exit done;}
"#;
#[test]
fn conformance_compares_group_and_parameter_positions_not_variable_names() {
    same(PAIR);
    rejects(
        &PAIR.replace("pair[owner['a],owner['b]]", "pair[owner['b],owner['a]]"),
        "E_ORIGIN",
    );
    rejects(
        &PAIR.replace(
            "pair {owner.left,owner.right}",
            "pair {owner.right,owner.left}",
        ),
        "E_LIFETIME",
    );
}
#[test]
fn unused_generic_bodies_only_have_the_declared_origin_contract() {
    let source = format!(
        "{PAIR} control wrong<r:pair_reader>(ref pair input, ref pair other) exits ready {{pair[input['a],input['b]] value}} {{forward pair_reader<r>.get(other);}}"
    );
    rejects(&source, "E_LIFETIME");
}
#[test]
fn whole_carrier_reference_results_and_group_selectors_work_in_traits() {
    let s = r#"struct view['a] {ref mutable string text <- 'a;}
trait lender {control lend(ref mutable view input) exits ready {ref mutable view[input['a]] value from input};}
struct reader {u32 tag;}
implement lender for reader {control lend(ref mutable view owner) exits ready {ref mutable view[owner['a]] value from owner} {exit ready {owner};}}
control use<r:lender>(ref mutable view input) exits ready {ref mutable view[input['a]] value from input} {forward lender<r>.lend(input);}
control main() exits done {mutable string owner="before";mutable view v=view {borrow mutable owner};
delta use<reader>(borrow mutable v) {ready {auto value}=>ready}
[ready]:deref value.text="after";emit(value.text);exit done;}"#;
    same(s);
    rejects(
        &s.replace("ref mutable view", "ref view")
            .replace("borrow mutable v", "borrow v"),
        "E_MUTABILITY",
    );
}
#[test]
fn direct_reference_results_can_select_public_groups() {
    let s = r#"struct view['a] {ref string text <- 'a;}
trait reader {control get(ref view input) exits ready {ref string value from input['a]};}
implement reader for string {control get(ref view owner) exits ready {ref string value from owner['a]} {exit ready {owner.text};}}
control use<r:reader>(ref view input) exits ready {ref string value from input['a]} {forward reader<r>.get(input);}
control main() exits done {string owner="text";view v=view {borrow owner};delta use<string>(borrow v) {ready {auto value}=>ready}
[ready]:emit(value);exit done;}"#;
    same(s);
    rejects(&s.replace("from owner['a]", "from owner"), "E_LIFETIME");
}
#[test]
fn native_trait_aggregate_contracts_match_reference_and_expansion() {
    for (case, source) in [SOURCE, PAIR].iter().enumerate() {
        let p = compile(source, manifest()).unwrap();
        let expected = p.run(&scenario()).unwrap();
        let root = std::env::temp_dir().join(format!(
            "tellur-aggregate-traits-{}-{case}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        for expanded in [false, true] {
            let program = if expanded {
                p.expanded("main").unwrap()
            } else {
                p.clone()
            };
            for opt in [0, 2] {
                let binary = root.join(format!("traits-{expanded}-{opt}"));
                support::scenario_executable(&program)
                    .build_native("main", &binary, opt)
                    .unwrap();
                let output = std::process::Command::new(binary)
                    .arg("--json")
                    .output()
                    .unwrap();
                assert!(
                    output.status.success(),
                    "{}",
                    String::from_utf8_lossy(&output.stderr)
                );
                assert_eq!(
                    serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap(),
                    serde_json::to_value(&expected).unwrap()
                );
            }
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn swapped_inputs_and_omitted_contracts_cannot_change_the_trait_assumption() {
    let s = r#"struct view['a] {ref string text <- 'a;}
trait reader_contract {control get(ref string first,ref string second) exits ready {view[first] value};}
implement reader_contract for string {control get(ref string a,ref string b) exits ready {view[b] value} {exit ready {view {b}};}}"#;
    rejects(s, "E_ORIGIN");
    compile(
        &s.replace("view[b]", "view[a]")
            .replace("view {b}", "view {a}"),
        manifest(),
    )
    .unwrap();
    rejects(&s.replace("view[b]", "view"), "E_ORIGIN");
}
#[test]
fn grouped_sum_and_option_results_keep_empty_alternatives_and_label_order() {
    for (declaration, ty, full, empty, arms) in [
        (
            "variant result['a] {found {ref string text <- 'a},empty}",
            "result",
            "found {input}",
            "empty {}",
            "found {auto text}=>found,empty=>empty",
        ),
        (
            "outcome result['a] {found {ref string text <- 'a},empty}",
            "result",
            "found {input}",
            "empty {}",
            "found {auto text}=>found,empty=>empty",
        ),
        (
            "",
            "option<ref string>",
            "some {input}",
            "none {}",
            "some {auto text}=>found,none=>empty",
        ),
    ] {
        let s=format!("{declaration} trait lookup {{control get(ref string input,bool present) exits ready {{{ty}[input] result}}, absent;}} implement lookup for string {{control get(ref string owner,bool yes) exits absent,ready {{{ty}[owner] value}} {{delta yes {{true=>yes,false=>no}} [yes]:exit ready {{{full}}}; [no]:exit ready {{{empty}}};}}}} control use<r:lookup>(ref string input,bool yes) exits ready {{{ty}[input] result}},absent {{forward lookup<r>.get(input,yes);}} control main() exits done {{string owner=\"text\";delta use<string>(borrow owner,true) {{ready {{auto result}}=>ready,absent=>absent}} [ready]:delta result {{{arms}}} [found]:emit(text);[entry]:exit done;}}").replace("{input}","{owner}");
        same(&s);
        same(&s.replace("borrow owner,true", "borrow owner,false"));
    }
}
#[test]
fn named_direct_reference_contracts_retain_storage_and_inner_groups() {
    let s = r#"struct view['a] {ref string text <- 'a;}
outcome reply['storage,'text] {found {ref view['text] value <- 'storage},empty}
trait reader_contract {control get(ref view input) exits reply[input,input['a]];}
implement reader_contract for string {control get(ref view owner) exits reply[owner,owner['a]] {exit found {owner};}}
control use<r:reader_contract>(ref view input) exits reply[input,input['a]] {forward reader_contract<r>.get(input);}
control main() exits done {string text="text";view v=view {borrow text};delta use<string>(borrow v) {found {auto result}=>found,empty=>empty}
[found]:emit(result.text);[entry]:exit done;}"#;
    same(s);
}
#[test]
fn guards_still_protect_separate_arguments_and_constructor_fields() {
    let prefix = r#"struct view['a] {ref mutable string text <- 'a;}
control collide(ref mutable view input,ref mutable string text) exits done {exit done;}
struct bundle['storage,'text] {ref mutable view['text] carrier <- 'storage;ref mutable string text <- 'text;}
control take(bundle input) exits done {exit done;}
control main() exits done {mutable string text="before";mutable view v=view {borrow mutable text};"#;
    rejects(
        &format!("{prefix} collide(borrow mutable v,borrow mutable deref v.text);exit done;}}"),
        "E_ALIAS",
    );
    rejects(
        &format!(
            "{prefix} take(bundle {{borrow mutable v,borrow mutable deref v.text}});exit done;}}"
        ),
        "E_ALIAS",
    );
}
#[test]
fn unknown_associated_origin_shapes_and_exclusive_from_shared_remain_rejected() {
    rejects(
        "trait reader {control get(ref string input) exits done[input];}",
        "E_ORIGIN",
    );
    rejects(
        "trait reader {type item; control get(ref item input) exits ready {item[input['a]] value};}",
        "E_ORIGIN",
    );
    rejects(
        "struct view['a] {ref mutable string text <- 'a;} trait reader {control get(ref string input) exits ready {view[input] value};}",
        "E_ORIGIN",
    );
}
#[test]
fn imported_private_layouts_preserve_public_trait_origin_groups() {
    let root = std::env::temp_dir().join(format!(
        "tellur-trait-origins-module-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("library.tellur"),r#"pub struct view['a] {ref string text <- 'a;}
pub trait reader {control get(ref string input) exits ready {view[input] value};}
implement reader for string {control get(ref string owner) exits ready {view[owner] value} {exit ready {view {owner}};}}
pub control print(view input) exits done {emit(input.text);exit done;}"#).unwrap();
    let source = r#"import "library.tellur" as lib;
control fetch<r:lib.reader>(ref string input) exits ready {lib.view[input] value} {forward lib.reader<r>.get(input);}
control main() exits done {string text="private";delta fetch<string>(borrow text) {ready {auto value}=>ready}
[ready]:lib.print(value);exit done;}"#;
    std::fs::write(root.join("main.tellur"), source).unwrap();
    let p = tellur::compile_file(root.join("main.tellur"), manifest()).unwrap();
    assert_eq!(
        p.run(&scenario()).unwrap(),
        p.expanded("main").unwrap().run(&scenario()).unwrap()
    );
    std::fs::write(
        root.join("main.tellur"),
        source.replace("lib.print(value)", "emit(value.text)"),
    )
    .unwrap();
    assert_eq!(
        tellur::compile_file(root.join("main.tellur"), manifest())
            .unwrap_err()
            .diagnostic
            .code,
        "E_VISIBILITY"
    );
    std::fs::remove_dir_all(root).unwrap();
}
