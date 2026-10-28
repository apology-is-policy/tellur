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
}
const SOURCE: &str = include_str!("fixtures/borrowed_collections.tellur");
#[test]
fn shared_lookup_outlives_collection_and_releases_sources_at_last_use() {
    same(SOURCE);
    rejects(
        &SOURCE.replace("emit(word)", "first=\"early\";emit(word)"),
        "E_LOAN",
    );
    rejects(
        &SOURCE.replace("emit(word)", "second=\"early\";emit(word)"),
        "E_LOAN",
    );
}
const ARRAY: &str = r#"control main() exits done {string first="a";string second="b";
mutable array<ref string,2> values=[borrow first,borrow second];auto copied=values;
delta array_get_mut(borrow mutable values,1) {found {auto text}=>found,out_of_bounds=>missing}
[found]:emit(text);[entry]:delta array_replace(borrow mutable values,0,borrow second) {replaced {auto old}=>replaced,out_of_bounds {auto rejected}=>rejected}
[replaced]:emit(old);[rejected]:emit(rejected);[entry]:delta array_into_vector(copied) {converted {auto vector}=>converted}
[converted]:delta vector_take(vector) {item {auto head,auto tail}=>item,empty=>empty}
[item]:emit(head);drop(tail);[entry]:exit done;}"#;
#[test]
fn arrays_copy_replace_convert_and_vectors_transfer() {
    same(ARRAY);
}
const MAP: &str = r#"control main() exits done {string first="a";string second="b";mutable map<string,ref string> values=map {};
delta map_insert(borrow mutable values,"key",borrow first) {inserted=>inserted,replaced {auto old}=>replaced}
[replaced]:emit(old);[entry]:delta map_insert(borrow mutable values,"key",borrow second) {inserted=>inserted2,replaced {auto previous}=>replaced2}
[replaced2]:emit(previous);[entry]:delta map_get_mut(borrow mutable values,borrow "key") {found {auto text}=>found,missing=>missing}
[found]:emit(text);[entry]:delta map_remove(borrow mutable values,borrow "key") {removed {auto removed}=>removed,missing=>missing2}
[removed]:emit(removed);[entry]:drop(values);exit done;}"#;
#[test]
fn maps_insert_replace_lookup_and_remove() {
    same(MAP);
}
#[test]
fn vector_clone_pop_and_dynamic_removal_keep_independent_retainers() {
    same(
        r#"control main() exits done {
mutable string a="a";mutable string b="b";mutable vector<ref string> values=[borrow a,borrow b];mutable auto copied=clone(borrow values);
delta vector_remove(borrow mutable values,0) {item {auto removed}=>item,out_of_bounds=>missing}
[item]:emit(removed);[entry]:drop(values);delta vector_pop(borrow mutable copied) {item {auto last}=>last,empty=>empty}
[last]:drop(copied);emit(last);[empty]:drop(copied);[entry]:a="free";b="free";exit done;}"#,
    );
}
#[test]
fn insertion_rejects_inner_storage_and_outer_loop_updates() {
    rejects(
        r#"control main() exits done {mutable vector<ref string> values=[];[entry] over &: {string local="bad";vector_push(borrow mutable values,borrow local);}[entry]:emit(u32_text(vector_length(borrow values)));exit done;}"#,
        "E_LIFETIME",
    );
}
#[test]
fn public_exclusive_nested_and_reference_rebinding_stay_gated() {
    for source in [
        "control bad(vector<ref string> input) exits done {exit done;}",
        "control main() exits done {mutable string s=\"x\";vector<ref mutable string> values=[borrow mutable s];exit done;}",
        "control main() exits done {string s=\"x\";vector<vector<ref string>> values=[[borrow s]];exit done;}",
        "struct box {vector<ref string> values;} control main() exits done {string s=\"x\";box b=box {[borrow s]};exit done;}",
    ] {
        assert!(compile(source, manifest()).is_err(), "{source}");
    }
}
#[test]
fn returned_reference_uses_the_original_parameter_origin() {
    same(
        r#"control first(ref string input) exits found {ref string value from input},missing {vector<ref string> values=[input];exit delta vector_get(borrow values,0) {found {auto text}=>found {text},out_of_bounds=>missing {}};}
control main() exits done {string text="ok";delta first(borrow text) {found {auto value}=>found,missing=>missing}[found]:emit(value);[entry]:exit done;}"#,
    );
}
#[test]
fn native_borrowed_collections_match_reference_and_expansion() {
    for (case, source) in [SOURCE, ARRAY, MAP].iter().enumerate() {
        let p = same(source);
        let expected = p.run(&scenario()).unwrap();
        let root = std::env::temp_dir().join(format!(
            "tellur-borrowed-collections-{}-{case}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        for expanded in [false, true] {
            let p = if expanded {
                p.expanded("main").unwrap()
            } else {
                p.clone()
            };
            for opt in [0, 2] {
                let binary = root.join(format!("program-{expanded}-{opt}"));
                support::scenario_executable(&p)
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
fn shared_elements_do_not_gain_mutability_from_a_mutable_collection() {
    rejects(
        &ARRAY.replace("emit(text)", "deref text=\"bad\""),
        "E_MUTABILITY",
    );
    rejects(
        &SOURCE.replace("mutable vector<ref string>", "vector<ref string>"),
        "E_MUTABILITY",
    );
    rejects(
        &SOURCE.replace(
            "vector_push(borrow mutable words, borrow second)",
            "vector_push(borrow mutable words, borrow mutable second)",
        ),
        "E_BORROW",
    );
}
#[test]
fn collection_aliases_keep_inserted_sources_live_and_storage_exclusive() {
    let s = r#"control main() exits done {mutable string owner="before";mutable vector<ref string> values=[];
ref mutable vector<ref string> editor=borrow mutable values;
vector_push(editor,borrow owner);delta vector_get_mut(editor,0) {found {auto text}=>found,out_of_bounds=>missing}
[found]:emit(text);[entry]:drop(editor);drop(values);owner="after";exit done;}"#;
    same(s);
    rejects(
        &s.replace("delta vector_get_mut", "owner=\"bad\";delta vector_get_mut"),
        "E_LOAN",
    );
    rejects(
        &s.replace("delta vector_get_mut", "drop(values);delta vector_get_mut"),
        "E_LOAN",
    );
}
#[test]
fn uninserted_and_replaced_inputs_keep_separate_result_dependencies() {
    let s = r#"control main() exits done {mutable string old="old";mutable string incoming="new";
mutable array<ref string,1> values=[borrow old];
delta array_replace(borrow mutable values,9,borrow incoming) {replaced {auto previous}=>replaced,out_of_bounds {auto rejected}=>rejected}
[replaced]:emit(previous);[rejected]:old="free";emit(rejected);[entry]:exit done;}"#;
    same(s);
    rejects(
        &s.replace("emit(rejected)", "incoming=\"bad\";emit(rejected)"),
        "E_LOAN",
    );
}
#[test]
fn empty_pop_discharges_collection_retainers_only_on_that_branch() {
    let s = r#"control main() exits done {mutable string owner="text";mutable vector<ref string> values=[borrow owner];
delta vector_pop(borrow mutable values) {item {auto text}=>item,empty=>empty}
[item]:emit(text);[empty]:owner="empty";emit(u32_text(vector_length(borrow values)));[entry]:exit done;}"#;
    same(s);
    same(&s.replace("values=[borrow owner]", "values=[]"));
}
#[test]
fn dynamic_removal_remains_conservative_until_collection_last_use() {
    rejects(
        r#"control main() exits done {mutable string owner="text";mutable vector<ref string> values=[borrow owner];
delta vector_pop(borrow mutable values) {item {auto text}=>item,empty=>empty}
[item]:emit(text);owner="too soon";emit(u32_text(vector_length(borrow values)));[entry]:exit done;}"#,
        "E_LOAN",
    );
}
#[test]
fn looping_local_collections_work_but_outer_origin_state_is_gated() {
    same(
        r#"control main() exits done {loop once() {string text="local";mutable vector<ref string> values=[];vector_push(borrow mutable values,borrow text);delta vector_pop(borrow mutable values) {item {auto value}=>item,empty=>empty}[item]:emit(value);[entry]:exit done;}}"#,
    );
    rejects(
        r#"control main() exits done {string text="outer";mutable vector<ref string> values=[borrow text];loop once() {delta vector_pop(borrow mutable values) {item {auto value}=>item,empty=>empty}[item]:emit(value);[entry]:exit done;}}"#,
        "E_LOOP_CAPTURE",
    );
}
#[test]
fn stored_adapters_and_public_collection_borrows_are_explicitly_gated() {
    for source in [
        r#"control main() exits done {string text="x";vector<ref string> values=[borrow text];auto pending=delta vector_take(values) {item {auto head,auto rest}=>saved {rest},empty=>empty {}};delta pending {saved {auto v}=>saved,empty=>empty}[saved]:drop(v);[entry]:exit done;}"#,
        "control use(ref vector<ref string> input) exits done {exit done;}",
        "control use(ref string input) exits ready {vector<ref string> value} {exit ready {[input]};}",
    ] {
        assert!(compile(source, manifest()).is_err(), "{source}");
    }
}
#[test]
fn generic_bodies_can_build_local_reference_collections_without_reference_type_arguments() {
    same(
        r#"control inspect<t:clone>(ref t input) exits ready {t value},missing {vector<ref t> values=[input];exit delta vector_get(borrow values,0) {found {auto value}=>ready {clone(value)},out_of_bounds=>missing {}};}
control main() exits done {string text="generic";delta inspect<string>(borrow text) {ready {auto value}=>ready,missing=>missing}[ready]:emit(value);[entry]:exit done;}"#,
    );
}

#[test]
fn map_take_transfers_values_and_remaining_dependencies() {
    let source = r#"control main() exits done {mutable string owner="text";mutable map<string,ref string> values=map {};
 delta map_insert(borrow mutable values,"key",borrow owner) {inserted=>inserted,replaced {auto old}=>replaced}
 [replaced]:emit(old);[entry]:delta map_take(values) {item {auto key,auto value,auto rest}=>item,empty=>empty}
 [item]:drop(rest);emit(key);emit(value);[entry]:owner="released";exit done;}"#;
    same(source);
    rejects(
        &source.replace("emit(value)", "owner=\"early\";emit(value)"),
        "E_LOAN",
    );
    rejects(
        r#"struct view {ref string text;}control main() exits done {string text="text";view v=view {borrow text};delta vector_get(borrow v,0) {found {auto x}=>found,out_of_bounds=>missing}[entry]:exit done;}"#,
        "E_COLLECTION",
    );
}
