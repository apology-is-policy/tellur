mod support;
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};
use support::BuildTestExecutable;
use tellur::{
    compile, compile_file, ir::Value, manifest::Manifest, modules::ModuleError, runtime::Scenario,
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Fixture {
    root: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "path-modules-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        Self {
            root: fs::canonicalize(root).unwrap(),
        }
    }
    fn write(&self, name: &str, source: &str) {
        let p = self.root.join(name);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, source).unwrap();
    }
    fn check(&self) -> std::result::Result<tellur::CheckedProgram, ModuleError> {
        compile_file(self.root.join("main.vn"), m())
    }
    fn reject(&self, code: &str) -> ModuleError {
        let e = self.check().unwrap_err();
        assert_eq!(e.diagnostic.code, code, "{e}");
        e
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
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
fn same(p: &tellur::CheckedProgram) {
    assert_eq!(
        p.run(&scenario()).unwrap(),
        p.expanded("main").unwrap().run(&scenario()).unwrap()
    );
}
const PEOPLE: &str = include_str!("../playground/modules/people.vn");
const MAIN: &str = include_str!("../playground/modules/main.vn");
#[test]
fn exported_payloads_are_explicit_while_importers_can_infer() {
    let f = Fixture::new();
    f.write(
        "lookup.vn",
        "pub control lookup() exits ready {string text} {exit ready {\"hello\"};}",
    );
    f.write(
        "main.vn",
        "import \"lookup.vn\" as lookup;control main() exits ready... {forward lookup.lookup();}",
    );
    let p = f.check().unwrap();
    same(&p);
    assert_eq!(
        p.run(&scenario()).unwrap().fields,
        vec![Value::String("hello".into())]
    );
}
#[test]
fn imported_stored_views_preserve_construction_and_pointee_privacy() {
    let f = Fixture::new();
    f.write(
        "views.vn",
        "pub struct view {pub ref string text;}pub struct hidden {ref string text;}",
    );
    let source = "import \"views.vn\" as views; control main() exits done {string owner=\"hello\";views.view v=views.view {borrow owner};emit(v.text);exit done;}";
    f.write("main.vn", source);
    same(&f.check().unwrap());
    f.write("main.vn", &source.replace("views.view", "views.hidden"));
    f.reject("E_VISIBILITY");
    f.write("people.vn", PEOPLE);
    f.write("main.vn", "import \"people.vn\" as people;struct view {ref people.person value;}control main() exits done {delta people.create(\"Ada\",37) {ready {auto person}=>ready}[ready]:view v=view {borrow person};emit(borrow v.value.name);exit done;}");
    f.reject("E_VISIBILITY");
}
#[test]
fn imports_private_representation_public_fields_and_borrowed_results_work() {
    let f = Fixture::new();
    f.write("people.vn", PEOPLE);
    f.write("main.vn", MAIN);
    let p = f.check().unwrap();
    same(&p);
    assert_eq!(p.source_files().len(), 2);
    assert!(p.inspect("main").unwrap().contains("people.vn"));
    assert_eq!(
        p.source_files().len(),
        p.expanded("main").unwrap().source_files().len()
    );
}
#[test]
fn private_controls_fields_and_constructors_cannot_be_reached() {
    let f = Fixture::new();
    f.write("people.vn", PEOPLE);
    for source in [
        "import \"people.vn\" as people;control main() exits done {forward people.internal_note();}",
        "import \"people.vn\" as people;control main() exits done {people.person value=people.person {\"Ada\",37};exit done;}",
        "import \"people.vn\" as people;control main() exits done {delta people.create(\"Ada\",37) {ready {auto value}=>created}[created]:emit(value.name);exit done;}",
        "import \"people.vn\" as people;control main() exits done {delta people.create(\"Ada\",37) {ready {auto value}=>created}[created]:auto view=&value.name;exit done;}",
        "import \"people.vn\" as people;control main() exits done {delta people.create(\"Ada\",37) {ready {auto value}=>created}[created]:mutable people.person owner=value;owner.name=\"changed\";exit done;}",
    ] {
        f.write("main.vn", source);
        let e = f.reject("E_VISIBILITY");
        assert_eq!(e.path, f.root.join("main.vn"));
    }
}
#[test]
fn private_type_names_and_public_interface_leaks_are_rejected() {
    let f = Fixture::new();
    f.write(
        "main.vn",
        "import \"lib.vn\" as lib;control main() exits done {exit done;}",
    );
    for source in [
        "struct secret {u32 value;} pub control leak() exits ready {secret value} {exit ready {secret {1}};}",
        "struct secret {u32 value;} pub struct public_box {pub secret value;}",
        "struct secret {u32 value;} pub variant public_choice {yes {secret value},no}",
        "struct secret {u32 value;} pub outcome public_result {yes {secret value},no}",
    ] {
        f.write("lib.vn", source);
        let e = f.reject("E_VISIBILITY");
        assert_eq!(e.path, f.root.join("lib.vn"));
    }
    f.write(
        "lib.vn",
        "struct secret {u32 value;} pub struct public_box {secret value;}",
    );
    f.check().unwrap();
    f.write(
        "main.vn",
        "import \"lib.vn\" as lib;control main(lib.secret value) exits done {exit done;}",
    );
    f.reject("E_VISIBILITY");
}
#[test]
fn aliases_and_diamond_imports_share_one_nominal_identity() {
    let f = Fixture::new();
    f.write("people.vn", PEOPLE);
    f.write("relay.vn","import \"people.vn\" as staff;pub control relay(staff.person value) exits ready {staff.person value} {exit ready {value};}");
    f.write("main.vn","import \"people.vn\" as first;import \"./people.vn\" as second;import \"relay.vn\" as relay;control main() exits done {delta first.create(\"Ada\",37) {ready {second.person value}=>created}[created]:delta relay.relay(value) {ready {first.person returned}=>returned}[returned]:exit done;}");
    let p = f.check().unwrap();
    assert_eq!(p.source_files().len(), 3);
    same(&p);
}
#[test]
fn separate_files_with_same_type_spelling_stay_nominally_distinct() {
    let f = Fixture::new();
    for name in ["a.vn", "b.vn"] {
        f.write(name, "pub struct id {pub u32 value;}");
    }
    f.write("main.vn","import \"a.vn\" as a;import \"b.vn\" as b;control main() exits done {a.id first=a.id {7};b.id second=first;exit done;}");
    f.reject("E_TYPE");
}
#[test]
fn private_constructor_tags_do_not_pollute_other_modules_or_builtin_options() {
    let f = Fixture::new();
    f.write(
        "lib.vn",
        "variant hidden {some {string text},none} pub variant public_choice {yes,no}",
    );
    f.write("main.vn","import \"lib.vn\" as lib;control main() exits ready {option<u32> value} {option<u32> value=some {7};exit ready {value};}");
    same(&f.check().unwrap());
    f.write(
        "main.vn",
        "import \"lib.vn\" as lib;control main() exits done {auto value=yes {};exit done;}",
    );
    f.reject("E_NAME");
    f.write("main.vn","import \"lib.vn\" as lib;control main() exits done {auto value=lib.public_choice.yes {};exit done;}");
    same(&f.check().unwrap());
}
#[test]
fn imports_resolve_relative_to_importer_and_report_cycles_and_missing_files() {
    let f = Fixture::new();
    f.write(
        "main.vn",
        "import \"sub/a.vn\" as a;control main() exits done {forward a.go();}",
    );
    f.write(
        "sub/a.vn",
        "import \"../b.vn\" as b;pub control go() exits done {forward b.go();}",
    );
    f.write("b.vn", "pub control go() exits done {exit done;}");
    same(&f.check().unwrap());
    f.write(
        "b.vn",
        "import \"main.vn\" as root;pub control go() exits done {exit done;}",
    );
    f.reject("E_IMPORT_CYCLE");
    fs::remove_file(f.root.join("b.vn")).unwrap();
    let e = f.reject("E_IMPORT");
    assert_eq!(e.path, f.root.join("sub/a.vn"));
}
#[test]
fn dependency_diagnostics_have_original_file_and_line_and_unsaved_loader_is_used() {
    let f = Fixture::new();
    f.write(
        "main.vn",
        "import \"lib.vn\" as lib;\ncontrol main() exits done {forward lib.go();}",
    );
    f.write(
        "lib.vn",
        "// Header\npub control go() exits done {\n string value=7;\n exit done;\n}",
    );
    let e = f.reject("E_TYPE");
    assert_eq!(e.path, f.root.join("lib.vn"));
    assert_eq!(e.diagnostic.span.line, 3);
    let p = tellur::modules::compile_with_loader(f.root.join("main.vn"), m(), |p| {
        if p.ends_with("lib.vn") {
            Ok("pub control go() exits done {exit done;}".into())
        } else {
            fs::read_to_string(p)
        }
    })
    .unwrap();
    same(&p);
}
#[test]
fn namespace_collisions_and_text_only_import_compilation_are_diagnosed() {
    let f = Fixture::new();
    f.write("lib.vn", "pub control go() exits done {exit done;}");
    for source in [
        "import \"lib.vn\" as lib;control lib() exits done {exit done;}",
        "import \"lib.vn\" as lib;control main() exits done {u32 lib=7;exit done;}",
        "import \"lib.vn\" as lib;import \"lib.vn\" as lib;",
    ] {
        f.write("main.vn", source);
        f.reject("E_NAME");
    }
    assert_eq!(
        compile("import \"lib.vn\" as lib;", m()).unwrap_err().code,
        "E_IMPORT"
    );
}
#[test]
fn imported_resources_retain_ownership_and_completion_requirements() {
    let f = Fixture::new();
    f.write("lib.vn","pub struct resource_box {handle file;} pub control wrap(handle file) exits ready {resource_box value} {exit ready {resource_box {file}};} pub control unwrap(resource_box value) exits ready {handle file} {exit ready {value.file};}");
    f.write("main.vn","import \"lib.vn\" as lib;control main(handle file) exits ready {handle file} {delta lib.wrap(file) {ready {auto box}=>wrapped}[wrapped]:forward lib.unwrap(box);}");
    let p = f.check().unwrap();
    let s = Scenario {
        entry: "main".into(),
        arguments: vec![Value::Opaque {
            ty: "handle".into(),
            token: "f".into(),
        }],
        calls: vec![],
    };
    assert_eq!(
        p.run(&s).unwrap(),
        p.expanded("main").unwrap().run(&s).unwrap()
    );
    f.write("main.vn","import \"lib.vn\" as lib;control main(handle file) exits done {delta lib.wrap(file) {ready {auto box}=>wrapped}[wrapped]:exit done;}");
    f.reject("E_RESOURCE");
}

#[test]
fn public_interface_rules_also_apply_to_text_compilation() {
    let e = compile(
        "struct secret {u32 value;} pub control leak() exits ready {secret value} {exit ready {secret {7}};}",
        m(),
    )
    .unwrap_err();
    assert_eq!(e.code, "E_VISIBILITY");
}
#[cfg(unix)]
#[test]
fn symlinks_share_identity_and_cannot_hide_an_import_cycle() {
    let f = Fixture::new();
    f.write("lib.vn", "pub struct id {pub u32 value;}");
    std::os::unix::fs::symlink(f.root.join("lib.vn"), f.root.join("link.vn")).unwrap();
    f.write("main.vn","import \"lib.vn\" as a;import \"link.vn\" as b;control main() exits done {a.id first=a.id {7};b.id second=first;exit done;}");
    let p = f.check().unwrap();
    assert_eq!(p.source_files().len(), 2);
    same(&p);
    f.write("lib.vn", "import \"link.vn\" as alias;");
    f.reject("E_IMPORT_CYCLE");
}
#[test]
fn source_map_errors_and_cli_imports_survive_a_different_working_directory() {
    let f = Fixture::new();
    f.write("people.vn", PEOPLE);
    f.write("main.vn", MAIN);
    fs::write(
        f.root.join("manifest.json"),
        include_str!("../playground/manifest.json"),
    )
    .unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_tellurc"))
        .args(["check"])
        .arg(f.root.join("main.vn"))
        .current_dir(std::env::temp_dir())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
#[test]
fn native_modules_preserve_borrows_and_effect_order_before_and_after_expansion() {
    let f = Fixture::new();
    f.write("people.vn", PEOPLE);
    f.write("main.vn", MAIN);
    let original = f.check().unwrap();
    let expected = serde_json::to_value(original.run(&scenario()).unwrap()).unwrap();
    let expanded = original.expanded("main").unwrap();
    for (name, p) in [("original", original), ("expanded", expanded)] {
        for opt in [0, 2] {
            let binary = f.root.join(format!("{name}-{opt}"));
            support::scenario_executable(&p)
                .build_for_test("main", &binary, opt)
                .unwrap();
            let scenario_file = f.root.join("case.json");
            fs::write(&scenario_file, serde_json::to_vec(&scenario()).unwrap()).unwrap();
            let output = std::process::Command::new(binary)
                .arg("--scenario")
                .arg(scenario_file)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap(),
                expected,
                "{name} O{opt}"
            );
        }
    }
}

#[test]
fn import_budget_is_an_error_instead_of_a_partial_program() {
    let f = Fixture::new();
    f.write("main.vn", &" ".repeat(8 * 1024 * 1024 + 1));
    f.reject("E_IMPORT_LIMIT");
    f.write("main.vn", "import \"m1.vn\" as next;");
    for i in 1..257 {
        f.write(
            &format!("m{i}.vn"),
            &format!("import \"m{}.vn\" as next;", i + 1),
        );
    }
    f.write("m257.vn", "struct last {}");
    f.reject("E_IMPORT_LIMIT");
}

#[test]
fn constructor_resolution_respects_lexical_binding_order_and_scopes() {
    let f = Fixture::new();
    f.write("lib.vn","variant state {empty,full {u32 value}} pub control go() exits done {auto first=state.empty; state state=state.empty {};exit done;}");
    f.write(
        "main.vn",
        "import \"lib.vn\" as lib;control main() exits done {forward lib.go();}",
    );
    same(&f.check().unwrap());
}

#[test]
fn named_fields_resolve_namespaces_and_preserve_representation_privacy() {
    let f = Fixture::new();
    f.write("people.vn","pub struct person {pub string name;pub u32 age;} pub control create() exits ready {person value} {exit ready {person {age:36,name:\"Ada\"}};}");
    f.write("main.vn","import \"people.vn\" as people; control main() exits auto {auto value=people.person {age:36,name:\"Ada\"};exit ready {value.name,value.age};}");
    let p = f.check().unwrap();
    same(&p);
    assert_eq!(
        p.run(&scenario()).unwrap().fields,
        vec![Value::String("Ada".into()), Value::U32(36)]
    );
    f.write("people.vn", "pub struct person {string name;pub u32 age;}");
    f.reject("E_VISIBILITY");
    f.write("people.vn", "struct person {pub string name;pub u32 age;}");
    f.reject("E_VISIBILITY");
}

#[test]
fn generic_modules_keep_shared_nominal_identity_and_private_representation() {
    let f = Fixture::new();
    let source = "pub struct box<t>{pub t value;} pub variant choice<t>{some {t value},none} pub outcome result<t>{ready {t value},empty}";
    f.write("data.vn", source);
    f.write("main.vn","import \"data.vn\" as a;import \"data.vn\" as b;control main() exits a.result<string> {a.box<string> first=a.box<string>{value:\"Ada\"};b.box<string> second=first;auto tag=b.choice<string>.some {second.value};exit delta tag {some {auto value}=>ready {value},none=>empty {}};}");
    let p = f.check().unwrap();
    same(&p);
    assert_eq!(
        p.run(&scenario()).unwrap().fields,
        vec![Value::String("Ada".into())]
    );
    f.write("data.vn", &source.replace("pub t value", "t value"));
    f.reject("E_VISIBILITY");
    f.write("data.vn","pub struct box<t>{pub t value;}struct hidden{}pub control leak() exits ready {box<hidden> value} {exit ready {box<hidden>{hidden{}}};}");
    f.write(
        "main.vn",
        "import \"data.vn\" as data;control main() exits done {exit done;}",
    );
    f.reject("E_VISIBILITY");
    f.write("data.vn","struct box<t>{pub t value;}pub control leak() exits ready {box<u32> value} {exit ready {box<u32>{1}};}");
    f.reject("E_VISIBILITY");
}

#[test]
fn generic_controls_keep_module_privacy_and_universal_definition_checks() {
    let f = Fixture::new();
    f.write("library.vn","pub struct box<t>{t value;}pub control wrap<t>(t value) exits ready {box<t> value} {exit ready {box<t>{value}};}pub control unpack<t>(box<t> value) exits ready {t value} {exit ready {value.value};}control secret<t>(t value) exits ready {t value} {exit ready {value};}");
    f.write("main.vn","import \"library.vn\" as library;control main() exits auto {exit delta library.wrap<string>(\"Ada\") {ready {auto value}=>delta library.unpack<string>(value) {ready {auto text}=>done {text}}};}");
    let p = f.check().unwrap();
    same(&p);
    assert_eq!(
        p.run(&scenario()).unwrap().fields,
        vec![Value::String("Ada".into())]
    );
    f.write("main.vn","import \"library.vn\" as library;control main() exits auto {forward library.secret<string>(\"x\");}");
    f.reject("E_VISIBILITY");
    f.write(
        "main.vn",
        "import \"library.vn\" as library;control main() exits done {exit done;}",
    );
    f.write(
        "library.vn",
        "pub control bad<t>(t value) exits done {exit done;}",
    );
    let e = f.reject("E_RESOURCE");
    assert!(e.path.ends_with("library.vn"));
    f.write("library.vn","struct hidden{}pub control bad<t>(t value) exits ready {t value,hidden secret} {exit ready {value,hidden{}};}");
    f.reject("E_VISIBILITY");
}

#[test]
fn generic_parameter_constructor_shadowing_is_independent_of_module_prefixes() {
    let f = Fixture::new();
    f.write("library.vn","struct item{}pub control bad<item>(item value) exits ready {item value} {auto wrong=item{};exit ready {value};}");
    f.write(
        "main.vn",
        "import \"library.vn\" as library;control main() exits done {exit done;}",
    );
    let e = f.reject("E_NAME");
    assert!(e.diagnostic.message.contains("generic definition check"));
}

#[test]
fn qualified_generic_and_non_generic_single_completion_statements_work() {
    let f = Fixture::new();
    f.write("library.vn","pub control show() exits done {emit(\"plain\");exit done;}pub control observe<t>(&t value) exits done {emit(\"generic\");exit done;}");
    f.write("main.vn","import \"library.vn\" as library;control main() exits done {string value=\"Ada\";library.show();library.observe<string>(&value);exit done;}");
    let p = f.check().unwrap();
    same(&p);
}

#[test]
fn public_generic_can_be_used_with_private_arguments_without_exporting_them() {
    let f = Fixture::new();
    let source = "struct hidden{u32 value;}pub control identity<t>(t value) exits ready {t value} {exit ready {value};}control main() exits auto {exit delta identity<hidden>(hidden{7}) {ready {auto value}=>done {value.value}};}";
    f.write("main.vn", source);
    let p = f.check().unwrap();
    same(&p);
    f.write(
        "main.vn",
        &source
            .replace(
                "control main() exits auto",
                "pub control main() exits done {hidden value}",
            )
            .replace("done {value.value}", "done {value}"),
    );
    f.reject("E_VISIBILITY");
}

#[test]
fn imported_generic_bounds_check_definitions_and_private_concrete_arguments() {
    let f = Fixture::new();
    f.write("library.vn","pub control duplicate<t: clone>(&t value) exits ready {t value} {exit ready {clone(value)};}");
    f.write("main.vn","import \"library.vn\" as lib; struct private_box{string value;} control main() exits auto {private_box value=private_box{value:\"Ada\"};exit delta lib.duplicate<private_box>(&value) {ready {auto copy}=>done {copy.value}};}");
    assert_eq!(
        f.check().unwrap().run(&scenario()).unwrap().fields,
        vec![Value::String("Ada".into())]
    );
    f.write("library.vn","pub control duplicate<t: copy>(&t value) exits ready {t value} {exit ready {clone(value)};}");
    let e = f.reject("E_BOUND");
    assert!(e.to_string().contains("main.vn"));
    f.write("library.vn","pub control duplicate<t: drop>(&t value) exits ready {t value} {exit ready {clone(value)};}");
    let e = f.reject("E_CLONE");
    assert!(e.to_string().contains("library.vn"));
}

#[test]
fn trait_calls_and_associated_types_resolve_through_imports_and_keep_defining_privacy() {
    let f = Fixture::new();
    f.write(
        "reader.vn",
        "pub trait reader {type item;control read(&self input) exits ready {item value};}",
    );
    f.write("people.vn","import \"reader.vn\" as io; pub struct person {string name;} pub control make() exits ready {person value} {exit ready {person {name:\"Ada\"}};} impl io.reader for person {type item=string;control read(&self input) exits ready {item value} {exit ready {clone(&(*input).name)};}}");
    let source = "import \"reader.vn\" as io; import \"people.vn\" as people; control main() exits auto {delta people.make() {ready {auto value}=>ready} [ready]:io.reader<people.person>.item expected=\"Ada\";exit delta io.reader<people.person>.read(&value) {ready {auto name}=>done {name==expected}};}";
    f.write("main.vn", source);
    let p = f.check().unwrap();
    assert_eq!(p.run(&scenario()).unwrap().fields, vec![Value::Bool(true)]);
    assert_eq!(
        p.run(&scenario()).unwrap(),
        p.expanded("main").unwrap().run(&scenario()).unwrap()
    );
    f.write(
        "reader.vn",
        "trait reader {type item;control read(&self input) exits ready {item value};}",
    );
    f.reject("E_VISIBILITY");
}
#[test]
fn trait_or_type_ownership_controls_coherence_and_diamond_imports_share_implementations() {
    let f = Fixture::new();
    f.write(
        "a.vn",
        "pub trait mark {} pub struct token {pub u32 value;} impl mark for token {}",
    );
    f.write("b.vn", "import \"a.vn\" as a;");
    f.write(
        "main.vn",
        "import \"a.vn\" as a; import \"b.vn\" as b; control main() exits done {exit done;}",
    );
    f.check().unwrap();
    f.write(
        "main.vn",
        "import \"a.vn\" as a; impl a.mark for a.token {} ",
    );
    f.reject("E_COHERENCE");
    f.write(
        "main.vn",
        "import \"a.vn\" as a; struct local {u32 value;} impl a.mark for vector<local> {}",
    );
    f.reject("E_COHERENCE");
    f.write(
        "main.vn",
        "import \"a.vn\" as a; struct local {u32 value;} impl a.mark for local {}",
    );
    f.check().unwrap();
    f.write(
        "main.vn",
        "import \"a.vn\" as a; trait local {} impl local for a.token {}",
    );
    f.check().unwrap();
}
#[test]
fn trait_methods_cannot_gain_foreign_private_field_access() {
    let f = Fixture::new();
    f.write("a.vn", "pub struct hidden {string text;}");
    f.write("main.vn",r#"import "a.vn" as a; trait reader {control read(&self input) exits ready {string value};} impl reader for a.hidden {control read(&self input) exits ready {string value} {exit ready {clone(&(*input).text)};}}"#);
    f.reject("E_VISIBILITY");
    f.write("main.vn",r#"import "a.vn" as a; trait reader {control read(ref self input) exits ready {string value};} implement reader for a.hidden {control read(ref self input) exits ready {string value} {exit ready {clone(borrow input.text)};}}"#);
    f.reject("E_VISIBILITY");
}

#[test]
fn imported_trait_bounds_preserve_witness_identity_private_access_and_generic_interfaces() {
    let f = Fixture::new();
    f.write("reader.vn","pub trait reader {type item:clone;control read(&self input) exits ready {item value};} pub control read_one<r:reader>(&r input) exits ready {reader<r>.item value} {forward reader<r>.read(input);}");
    f.write("source.vn","import \"reader.vn\" as io; pub struct source {string text;} pub control make() exits ready {source value} {exit ready {source {text:\"Ada\"}};} impl io.reader for source {type item=string;control read(&self input) exits ready {item value} {exit ready {clone(&(*input).text)};}}");
    f.write("main.vn","import \"reader.vn\" as io;import \"source.vn\" as data;control main() exits auto {delta data.make() {ready {auto value}=>ready} [ready]:forward io.read_one<data.source>(&value);}");
    let p = f.check().unwrap();
    assert_eq!(
        p.run(&scenario()).unwrap().fields,
        vec![Value::String("Ada".into())]
    );
    assert!(!p.regions().any(|r| r.name.contains("_path_poly_")));
    f.write("reader.vn","trait hidden {} pub control bad<t:hidden>(t value) exits ready {t value} {exit ready {value};}");
    f.write(
        "main.vn",
        r#"import "reader.vn" as io; control main() exits done {exit done;}"#,
    );
    f.reject("E_VISIBILITY");
}

#[test]
fn borrowed_trait_contracts_hide_private_field_layouts_across_modules() {
    let f = Fixture::new();
    f.write("reader.vn","pub trait reader {type item:clone;control peek(&self input) exits ready {&item value from input};} pub control peek<r:reader>(&r input) exits ready {&reader<r>.item value from input} {forward reader<r>.peek(input);}");
    f.write("source.vn","import \"reader.vn\" as io;pub struct source {string text;} pub control make() exits ready {source value} {exit ready {source {text:\"Ada\"}};} impl io.reader for source {type item=string;control peek(&self owner) exits ready {&item value from owner} {exit ready {&(*owner).text};}}");
    let main = "import \"reader.vn\" as io;import \"source.vn\" as data;control main() exits auto {delta data.make() {ready {auto value}=>ready} [ready]:exit delta io.peek<data.source>(&value) {ready {auto text}=>done {clone(text)}};}";
    f.write("main.vn", main);
    let p = f.check().unwrap();
    same(&p);
    assert_eq!(
        p.run(&scenario()).unwrap().fields,
        vec![Value::String("Ada".into())]
    );
    f.write(
        "main.vn",
        &main.replace("clone(text)", "clone(&value.text)"),
    );
    f.reject("E_VISIBILITY");
    f.write("main.vn", main);
    f.write("source.vn","import \"reader.vn\" as io;pub struct source {string text;} pub control make() exits ready {source value} {exit ready {source {text:\"Ada\"}};} impl io.reader for source {type item=string;control peek(&self owner) exits ready {&item value from owner} {string local=\"bad\";exit ready {&local};}}");
    let e = f.reject("E_LIFETIME");
    assert!(e.path.ends_with("source.vn"));
}

#[test]
fn imported_grouped_results_preserve_precision_and_private_field_mappings() {
    let f = Fixture::new();
    let library = "pub struct pair['a,'b] {ref string secret <- 'a;pub ref string right <- 'b;} pub control make(ref string left,ref string right) exits ready {pair[left,right] value} {exit ready {pair {left,right}};}";
    let source = r#"import "pairs.vn" as pairs;
    control main() exits done {mutable string a="a";string b="b";
    delta pairs.make(borrow a,borrow b) {ready {auto p}=>ready}
    [ready]: a="released";emit(p.right);exit done; }"#;
    f.write("pairs.vn", library);
    f.write("main.vn", source);
    same(&f.check().unwrap());
    f.write(
        "pairs.vn",
        &library.replace("secret", "renamed_private_field"),
    );
    same(&f.check().unwrap());
    f.write(
        "main.vn",
        &source
            .replace("a=\"released\";", "")
            .replace("emit(p.right)", "emit(p.renamed_private_field)"),
    );
    f.reject("E_VISIBILITY");
}
#[test]
fn contained_selectors_use_public_groups_without_exposing_private_fields() {
    let f = Fixture::new();
    let library = "pub struct view['a] {ref string hidden <- 'a;} pub control make(ref string owner) exits ready {view[owner] value} {exit ready {view {owner}};} pub control peek(ref view input) exits found {ref string value from input['a]} {exit found {input.hidden};}";
    f.write("library.vn", library);
    f.write("main.vn", "import \"library.vn\" as lib; control relay(lib.view input) exits found {ref string value from input['a]} {forward lib.peek(borrow input);} control main() exits done {string a=\"outside\";delta lib.make(borrow a) {ready {auto p}=>ready} [ready]:delta relay(p) {found {auto value}=>found} [ready & found]:emit(value);exit done;}");
    same(&f.check().unwrap());
    f.write("library.vn", &library.replace("hidden", "renamed"));
    same(&f.check().unwrap());
    f.write("main.vn","import \"library.vn\" as lib; control bad(lib.view input) exits found {ref string value from input['renamed]} {exit found {input.renamed};}");
    f.reject("E_LIFETIME");
}
#[test]
fn private_exclusive_fields_preserve_guards_across_public_selectors() {
    let f = Fixture::new();
    let library = "pub struct editor['a] {ref mutable string hidden <- 'a;} pub control make(ref mutable string owner) exits ready {editor[owner] value} {exit ready {editor {owner}};} pub control peek(ref editor input) exits found {ref string value from input['a]} {exit found {borrow deref input.hidden};}";
    f.write("library.vn", library);
    let main = "import \"library.vn\" as lib;control main() exits done {mutable string a=\"a\";delta lib.make(borrow mutable a) {ready {auto p}=>ready} [ready]:delta lib.peek(borrow p) {found {auto child}=>found} [ready & found]:emit(child);drop(p);exit done;}";
    f.write("main.vn", main);
    same(&f.check().unwrap());
    f.write("library.vn", &library.replace("hidden", "renamed"));
    same(&f.check().unwrap());
    f.write(
        "main.vn",
        &main.replace("emit(child);drop(p)", "drop(p);emit(child)"),
    );
    f.reject("E_LOAN");
}
#[test]
fn opaque_splits_hide_private_layout_and_preserve_exclusive_permissions() {
    let f = Fixture::new();
    let library = r#"pub struct owner {string hidden_left;string hidden_right;}
 pub control create() exits ready {owner value} {exit ready {owner {"left","right"}};}
 pub control split(ref mutable owner input) exits ready {ref mutable string a from input,ref mutable string b from input} {exit ready {borrow mutable input.hidden_left,borrow mutable input.hidden_right};}"#;
    let source = r#"import "library.vn" as lib;
 control main() exits done {delta lib.create() {ready {auto made}=>made}
 [made]:mutable lib.owner value=made;
 delta lib.split(borrow mutable value) {ready {auto a,auto b}=>split}
 [split]:deref a="A";deref b="B";emit(a);emit(b);drop(value);exit done;}"#;
    f.write("library.vn", library);
    f.write("main.vn", source);
    same(&f.check().unwrap());
    f.write("library.vn", &library.replace("hidden_left", "renamed"));
    same(&f.check().unwrap());
    f.write(
        "main.vn",
        &source.replace("emit(a);", "drop(value);emit(a);"),
    );
    f.reject("E_LOAN");
    f.write("main.vn", source);
    f.write(
        "library.vn",
        &library.replace(
            "borrow mutable input.hidden_right",
            "borrow mutable input.hidden_left",
        ),
    );
    f.reject("E_ALIAS");
}
#[test]
fn borrowed_variant_payloads_use_imported_visibility_and_local_lifetimes() {
    let f = Fixture::new();
    f.write("views.vn","pub variant view {found {ref string text},empty} variant hidden {secret {ref string text}} pub control peek(ref string input) exits found {ref string text from input} {exit found {input};}");
    let main = r#"import "views.vn" as lib;control main() exits done {mutable string owner="hello";lib.view saved=lib.view.found {borrow owner};
 delta saved {found {auto text}=>found,empty=>empty}[found]:emit(text);[empty]:emit("empty");
 [entry]:auto pending=capture lib.peek(borrow owner);exit delta pending {found {auto child}=>done {}};}"#;
    f.write("main.vn", main);
    same(&f.check().unwrap());
    f.write(
        "main.vn",
        &main.replace("delta saved", "owner=\"bad\";delta saved"),
    );
    f.reject("E_LOAN");
    f.write("main.vn", main);
    f.write("views.vn","variant view {found {ref string text},empty} pub control peek(ref string input) exits found {ref string text from input} {exit found {input};}");
    f.reject("E_VISIBILITY");
}

#[test]
fn nested_borrowed_storage_preserves_imported_layout_and_visibility() {
    let f = Fixture::new();
    let library = "pub variant view<t> {found {ref t text},empty} pub struct box<t> {pub option<view<t>> inner;}";
    f.write("views.vn", library);
    let source = r#"import "views.vn" as lib;control main() exits done {
        mutable string owner="hello";
        lib.box<string> saved=lib.box<string> {some {lib.view<string>.found {borrow owner}}};
        delta saved.inner {some {auto inner}=>some,none=>none}
        [some]:delta inner {found {auto text}=>found,empty=>empty}
        [found]:emit(text);owner="free";exit done;[none | empty]:exit done;
    }"#;
    f.write("main.vn", source);
    same(&f.check().unwrap());
    f.write(
        "main.vn",
        &source.replace("delta inner", "owner=\"bad\";delta inner"),
    );
    f.reject("E_LOAN");
    f.write("main.vn", source);
    f.write("views.vn", &library.replace("pub option", "option"));
    f.reject("E_VISIBILITY");
}

#[test]
fn imported_grouped_variants_preserve_public_groups_and_owner_retention() {
    let f = Fixture::new();
    f.write("views.vn",r#"pub variant view<t>['source] {found {ref t value <- 'source},empty}
        pub control create(ref string input) exits ready {view<string>[input] value} {exit ready {view<string>.found {input}};}
        pub control unwrap(view<string> input) exits found {ref string value from input['source]},empty {exit delta input {found {auto text}=>found {text},empty=>empty {}};}"#);
    let source = r#"import "views.vn" as views;
        control main() exits done {mutable string owner="hello";
        delta views.create(borrow owner) {ready {auto value}=>ready}
        [ready]:delta views.unwrap(value) {found {auto text}=>found,empty=>empty}
        [found]:emit(text);[entry]:owner="released";exit done;}"#;
    f.write("main.vn", source);
    same(&f.check().unwrap());
    f.write(
        "main.vn",
        &source.replace("emit(text)", "owner=\"too early\";emit(text)"),
    );
    f.reject("E_LOAN");
    f.write("main.vn", source);
    f.write("views.vn",r#"struct hidden {string text;} pub variant view['source] {found {ref hidden value <- 'source},empty}"#);
    f.write(
        "main.vn",
        r#"import "views.vn" as views; control main() exits done {exit done;}"#,
    );
    f.reject("E_VISIBILITY");
}

#[test]
fn imported_named_origin_schemas_preserve_generic_bindings_and_public_selectors() {
    let f = Fixture::new();
    f.write("library.vn",r#"pub outcome reply<t>['a] {found {ref t text <- 'a},empty}
pub control produce<t>(ref t input) exits reply<t>[input] {exit found {input};}
pub control wrap<t>(reply<t> input) exits ready {option<reply<t>>[input['a]] value} {exit ready {some {input}};}"#);
    let main = r#"import "library.vn" as lib;
control main() exits done {mutable string owner="hello";
lib.reply<string> saved=capture lib.produce<string>(borrow owner);
delta lib.wrap<string>(saved) {ready {auto pending}=>received}
[received]:delta pending {some {auto result}=>some,none=>none}
[some]:delta result {found {auto text}=>found,empty=>empty}
[found]:emit(text);[entry]:owner="released";exit done;}"#;
    f.write("main.vn", main);
    same(&f.check().unwrap());
    let forwarded = format!(
        "{}\ncontrol forward_text(ref string input) exits lib.reply<string>[input] {{forward lib.produce<string>(input);}}",
        main.replace(
            "capture lib.produce<string>(borrow owner)",
            "capture forward_text(borrow owner)"
        )
    );
    f.write("main.vn", &forwarded);
    same(&f.check().unwrap());
    f.write(
        "main.vn",
        &main.replace(
            "[found]:emit(text)",
            "[found]:owner=\"too early\";emit(text)",
        ),
    );
    f.reject("E_LOAN");
    f.write(
        "library.vn",
        "struct secret {string text;} pub outcome reply['a] {found {ref secret value <- 'a},empty}",
    );
    f.write(
        "main.vn",
        "import \"library.vn\" as lib; control main() exits done {exit done;}",
    );
    f.reject("E_VISIBILITY");
}

#[test]
fn public_controls_cannot_expose_private_named_completion_contracts() {
    for declaration in [
        "outcome hidden {found {string text}} pub control produce() exits hidden {exit found {\"x\"};}",
        "outcome hidden['a] {found {ref string text <- 'a}} pub control produce(ref string input) exits hidden[input] {exit found {input};}",
        "outcome hidden<t>['a] {found {ref t text <- 'a}} pub control produce<t>(ref t input) exits hidden<t>[input] {exit found {input};}",
    ] {
        assert_eq!(compile(declaration, m()).unwrap_err().code, "E_VISIBILITY");
        compile(&format!("pub {declaration}"), m()).unwrap();
    }
}

#[test]
fn imported_sum_carriers_preserve_public_origins_and_private_payload_fields() {
    let f = Fixture::new();
    f.write(
        "views.vn",
        r#"pub struct secret['a] {ref string text <- 'a;}
      pub variant wrapped['a] {found {secret['a] value},empty}
      pub control produce(ref string input) exits ready {wrapped[input] value} {
        exit ready {wrapped.found {secret {input}}};
      }
      pub control inspect(ref wrapped input) exits found {ref string text from input['a]},empty {
        exit delta input {found {auto value}=>found {value.text},empty=>empty {}};
      }"#,
    );
    let main = r#"import "views.vn" as views;
      control main() exits done {mutable string owner="hello";
        delta views.produce(borrow owner) {ready {auto saved}=>ready}
        [ready]:delta views.inspect(borrow saved) {found {auto text}=>found,empty=>empty}
        [found]:emit(text);[entry]:exit done;
      }"#;
    f.write("main.vn", main);
    same(&f.check().unwrap());
    f.write(
        "main.vn",
        &main.replace("emit(text)", "owner=\"bad\";emit(text)"),
    );
    f.reject("E_LOAN");
    f.write(
        "main.vn",
        &main
            .replace(
                "delta views.inspect(borrow saved) {found {auto text}=>found,empty=>empty}",
                "delta borrow saved {found {auto value}=>found,empty=>empty}",
            )
            .replace("emit(text)", "emit(value.text)"),
    );
    f.reject("E_VISIBILITY");
}

#[test]
fn forwarding_contributors_resolve_to_original_module_locations() {
    let f = Fixture::new();
    f.write("library.vn", include_str!("fixtures/public_contracts.vn"));
    f.write("main.vn", "import \"library.vn\" as lib;control main() exits done {delta lib.read_setting(true) {ready {auto value}=>ready}[ready]:exit done;}");
    let e = f.reject("E_UNCOVERED");
    assert_eq!(e.path, f.root.join("main.vn"));
    assert_eq!(e.diagnostic.notes.len(), 3, "{e}");
    for note in &e.diagnostic.notes {
        assert_eq!(note.path.as_ref(), Some(&f.root.join("library.vn")));
    }
    assert_eq!(
        e.diagnostic
            .notes
            .iter()
            .map(|n| n.span.line)
            .collect::<Vec<_>>(),
        vec![5, 9, 13]
    );
    assert!(!e.to_string().contains("_path_m_"), "{e}");
}

#[test]
fn stored_adapter_contributors_resolve_across_module_boundaries() {
    let f = Fixture::new();
    f.write(
        "leaf.vn",
        "pub control source() exits missing {exit missing;}",
    );
    f.write(
        "adapter.vn",
        r#"import "leaf.vn" as leaf;
control adapt() exits auto {
 auto saved=capture leaf.source();
 auto mapped=delta saved {missing=>unavailable {}};
 exit mapped;}
pub control run() exits unavailable {forward adapt();}"#,
    );
    f.write("main.vn", r#"import "adapter.vn" as adapter;control main() exits done {auto saved=capture adapter.run();delta saved {}}"#);
    let e = f.reject("E_UNCOVERED");
    for file in ["leaf.vn", "adapter.vn", "main.vn"] {
        assert!(
            e.diagnostic
                .notes
                .iter()
                .any(|n| n.path.as_ref() == Some(&f.root.join(file))),
            "{e}"
        );
    }
    assert!(!e.to_string().contains("_path_m_"), "{e}");
}

#[test]
fn constructors_respect_visibility_without_sealing_public_fields() {
    let f = Fixture::new();
    f.write(
        "library.vn",
        "pub struct message {pub string text;pub constructor(string text){self.text=move text;}}",
    );
    f.write("main.vn", "import \"library.vn\" as lib;control main() exits done {auto first=lib.message(\"constructed\");auto second=lib.message {text:\"raw\"};emit(move first.text);emit(move second.text);exit done;}");
    same(&f.check().unwrap());
    f.write(
        "library.vn",
        "pub struct message {pub string text;constructor(string text){self.text=move text;}}",
    );
    f.reject("E_PRIVATE");
    f.write("main.vn", "import \"library.vn\" as lib;control main() exits done {auto value=lib.message {text:\"raw\"};emit(move value.text);exit done;}");
    f.check().unwrap();
    f.write(
        "library.vn",
        "pub struct message {string text;pub constructor(string text){self.text=move text;}}",
    );
    f.reject("E_VISIBILITY");
    f.write("main.vn", "import \"library.vn\" as lib;control main() exits done {auto value=lib.message(\"constructed\");drop(value);exit done;}");
    f.check().unwrap();
    f.write("library.vn", "pub struct message {string text;pub constructor(string text){emit(borrow self.text);self.text=move text;}}");
    let e = f.reject("E_UNAVAILABLE");
    assert!(e.diagnostic.message.contains("self.text"), "{e}");
}

#[test]
fn imported_generic_constructor_forwards_owned_private_representation() {
    let f = Fixture::new();
    f.write("library.vn","pub struct box<t> {t value;pub constructor(t value){self.value=move value;}}pub control show(box<string> value) exits done {emit(move value.value);exit done;}");
    f.write("main.vn","import \"library.vn\" as lib;control main() exits done {lib.show(lib.box<string>(\"generic\"));exit done;}");
    same(&f.check().unwrap());
}

#[test]
fn imported_generic_borrowed_constructors_preserve_public_origins_and_privacy() {
    let f = Fixture::new();
    let library = "pub struct view<t>['a] {ref t value <- 'a; pub constructor(ref t input)[input] {self.value=input;}} pub control read(ref view<string> input) exits found {ref string value from input['a]} {exit found {input.value};}";
    f.write("library.vn", library);
    let source = "import \"library.vn\" as lib; control main() exits done {mutable string owner=\"borrowed\"; auto view=lib.view<string>(borrow owner); delta lib.read(borrow view) {found {auto text}=>found} [found]:emit(text); drop(view); owner=\"released\"; exit done;}";
    f.write("main.vn", source);
    same(&f.check().unwrap());
    f.write(
        "main.vn",
        &source.replace("emit(text);", "owner=\"bad\";emit(text);"),
    );
    f.reject("E_LOAN");
    f.write("main.vn", source);
    f.write(
        "library.vn",
        &library.replace("pub constructor", "constructor"),
    );
    f.reject("E_PRIVATE");
    f.write("library.vn", &library.replace("[input]", "[missing]"));
    f.reject("E_ORIGIN");
}

#[test]
fn imported_private_borrowed_representation_composes_inside_public_raw_storage() {
    let f = Fixture::new();
    let library = "pub struct view<t>['a] {ref t value <- 'a;pub constructor(ref t input)[input] {self.value=input;}}pub struct packet['a] {pub view<string>['a] value;}pub control show(packet input) exits done {emit(input.value.value);exit done;}";
    f.write("library.vn", library);
    let source = "import \"library.vn\" as lib;control main() exits done {mutable string owner=\"nested\";auto packet=lib.packet {value:lib.view<string>(borrow owner)};lib.show(move packet);owner=\"released\";exit done;}";
    f.write("main.vn", source);
    same(&f.check().unwrap());
    f.write(
        "main.vn",
        &source.replace(
            "lib.show(move packet);",
            "owner=\"too early\";lib.show(move packet);",
        ),
    );
    f.reject("E_LOAN");
    f.write("main.vn", source);
    f.write(
        "library.vn",
        &library.replace("pub constructor", "constructor"),
    );
    f.reject("E_PRIVATE");
    f.write(
        "library.vn",
        &library.replace("pub view<string>", "view<string>"),
    );
    f.reject("E_VISIBILITY");
}

#[test]
fn fresh_imported_constructor_projections_preserve_origins_and_field_privacy() {
    let f = Fixture::new();
    let library = "pub struct view<t>['a] {pub ref t value <- 'a;string hidden;pub constructor(ref t input)[input] {self.value=input;self.hidden=\"private\";}}";
    f.write("library.vn", library);
    let source = "import \"library.vn\" as lib;control main() exits done {mutable string owner=\"projected\";ref auto text=lib.view<string>(borrow owner).value;emit(text);owner=\"released\";exit done;}";
    f.write("main.vn", source);
    same(&f.check().unwrap());
    f.write(
        "main.vn",
        &source.replace("emit(text);", "owner=\"early\";emit(text);"),
    );
    f.reject("E_LOAN");
    f.write(
        "main.vn",
        &source
            .replace("ref auto text", "auto text")
            .replace(").value", ").hidden"),
    );
    f.reject("E_VISIBILITY");
    f.write("main.vn", source);
    f.write(
        "library.vn",
        &library.replace("pub constructor", "constructor"),
    );
    f.reject("E_PRIVATE");
}

#[test]
fn imported_generic_constructor_calls_release_only_nonescaping_inputs() {
    let f = Fixture::new();
    let library = "pub struct changed<t> {pub t value;pub constructor(ref mutable string input,t value){deref input=\"changed\";self.value=move value;}}pub struct log['a] {pub changed<string> first;pub changed<string> second;pub ref string latest <- 'a;}";
    f.write("library.vn", library);
    let source = "import \"library.vn\" as lib;control main() exits done {mutable string owner=\"before\";auto value=lib.log {lib.changed<string>(borrow mutable owner,\"first\"),lib.changed<string>(borrow mutable owner,\"second\"),borrow owner};emit(move value.first.value);emit(move value.second.value);emit(value.latest);owner=\"released\";exit done;}";
    f.write("main.vn", source);
    same(&f.check().unwrap());
    f.write(
        "main.vn",
        &source.replace("emit(value.latest);", "owner=\"early\";emit(value.latest);"),
    );
    f.reject("E_LOAN");
    f.write("main.vn", source);
    f.write(
        "library.vn",
        &library.replace("pub constructor", "constructor"),
    );
    f.reject("E_PRIVATE");
}

#[test]
fn shared_vector_window_checks_range_and_keeps_the_element_origin() {
    let f = Fixture::new();
    f.write("traversal.vn", include_str!("../library/traversal.vn"));
    let caller = r#"
import "traversal.vn" as t;
control main(u32 start, u32 end, u32 index) exits auto {
    mutable vector<string> words = ["one", "two"];
    delta t.range<string>(borrow words, start, end) {
        ready { auto view } => ready, invalid => exit invalid
    }
    [ready]: delta t.get<string>(borrow view, index) {
        found { ref string word } => found, finished => exit finished
    }
    [found]: emit(word);
        delta vector_push(borrow mutable words, "three") {pushed => appended}
    [appended]: exit read {vector_length(borrow words)};
}"#;
    f.write("main.vn", caller);
    let program = f.check().unwrap();
    let expanded = program.expanded("main").unwrap();
    for (start, end, index, outcome) in [
        (0, 0, 0, "finished"),
        (2, 2, 0, "finished"),
        (1, 0, 0, "invalid"),
        (0, 3, 0, "invalid"),
        (3, 3, 0, "invalid"),
        (1, 2, 0, "read"),
        (1, 2, 1, "finished"),
        (1, 2, u32::MAX, "finished"),
    ] {
        let calls = if outcome == "invalid" {
            vec![]
        } else {
            let sum = u32::checked_add(start, index);
            vec![tellur::runtime::ScriptCall {
                operation: "checked_add".into(),
                outcome: if sum.is_some() { "sum" } else { "overflow" }.into(),
                fields: sum.into_iter().map(Value::U32).collect(),
                waits: 0,
            }]
        };
        let scenario = Scenario {
            entry: "main".into(),
            arguments: vec![Value::U32(start), Value::U32(end), Value::U32(index)],
            calls,
        };
        let result = program.run(&scenario).unwrap();
        assert_eq!(result, expanded.run(&scenario).unwrap());
        assert_eq!(result.outcome, outcome);
        if outcome == "read" {
            assert_eq!(result.fields, vec![Value::U32(3)]);
        }
    }
    f.write(
        "main.vn",
        &caller
            .replace("[found]: emit(word);", "[found]:")
            .replace("[appended]: exit read", "[appended]: emit(word); exit read"),
    );
    f.reject("E_LOAN");

    // A later use of the view itself must also prevent structural mutation.
    f.write("main.vn", &caller.replace("[ready]: delta t.get", "[ready]: delta vector_push(borrow mutable words, \"early\") { pushed => changed } [changed]: delta t.get"));
    f.reject("E_LOAN");

    // Two overlapping readers may survive independently of the other window.
    let overlapping = r#"
import "traversal.vn" as t;
control main() exits auto {
    mutable vector<string> words = ["first", "second"];
    delta t.range<string>(borrow words, 0, 2) { ready {auto a} => first, invalid => exit bad }
    [first]: delta t.range<string>(borrow words, 1, 2) {ready {auto b} => second, invalid => exit bad}
    [second]: delta t.get<string>(borrow a, 1) {found {ref string x} => left, finished => exit bad}
    [left]: delta t.get<string>(borrow b, 0) {found {ref string y} => right, finished => exit bad}
    [right]: emit(x); emit(y); exit done;
}"#;
    f.write("main.vn", overlapping);
    let program = f.check().unwrap();
    let scenario = Scenario {
        entry: "main".into(),
        arguments: vec![],
        calls: vec![
            tellur::runtime::ScriptCall {
                operation: "checked_add".into(),
                outcome: "sum".into(),
                fields: vec![Value::U32(1)],
                waits: 0
            };
            2
        ],
    };
    let result = program.run(&scenario).unwrap();
    assert_eq!(result.outcome, "done");
    assert_eq!(
        result,
        program.expanded("main").unwrap().run(&scenario).unwrap()
    );

    f.write("main.vn", &caller.replace("[\"one\", \"two\"]", "[]"));
    let empty = f.check().unwrap();
    let zero = Scenario {
        entry: "main".into(),
        arguments: vec![Value::U32(0); 3],
        calls: vec![tellur::runtime::ScriptCall {
            operation: "checked_add".into(),
            outcome: "sum".into(),
            fields: vec![Value::U32(0)],
            waits: 0,
        }],
    };
    assert_eq!(empty.run(&zero).unwrap().outcome, "finished");
}

#[test]
fn first_match_borrows_the_source_and_routes_predicate_failures() {
    let f = Fixture::new();
    f.write("search.vn", include_str!("../library/search.vn"));
    let example = include_str!("../examples/first_match/main.vn");
    let rules = example
        .split("control main()")
        .next()
        .unwrap()
        .replace("../../library/search.vn", "search.vn");
    let caller = r#"
control main(vector<u32> input, u32 limit) exits auto {
    mutable vector<u32> readings = input;
    valid_reading rule = valid_reading { limit };
    delta search.try_first<valid_reading>(borrow readings, borrow rule) {
        found { ref u32 reading } => hit,
        missing => miss,
        failed { string reason } => exit invalid { reason }
    }
    [hit]: u32 saved = deref reading;
        delta vector_push(borrow mutable readings, 100) { pushed => changed }
    [changed]: exit ok { saved };
    [miss]: exit absent;
}"#;
    f.write("main.vn", &(rules.clone() + caller));
    let program = f.check().unwrap();
    let expanded = program.expanded("main").unwrap();
    for (input, skipped, outcome, fields) in [
        (vec![11, 0], 0, "ok", vec![Value::U32(11)]),
        (vec![2, 19, 17, 0], 1, "ok", vec![Value::U32(19)]),
        (
            vec![2, 0, 19],
            1,
            "invalid",
            vec![Value::String("sensor offline".into())],
        ),
        (vec![1, 2], 2, "absent", vec![]),
        (vec![], 0, "absent", vec![]),
    ] {
        let scenario = Scenario {
            entry: "main".into(),
            arguments: vec![
                Value::Construct {
                    ty: "vector<U32>".into(),
                    name: "vector".into(),
                    fields: input.into_iter().map(Value::U32).collect(),
                },
                Value::U32(10),
            ],
            calls: (1..=skipped)
                .map(|next| tellur::runtime::ScriptCall {
                    operation: "checked_add".into(),
                    outcome: "sum".into(),
                    fields: vec![Value::U32(next)],
                    waits: 0,
                })
                .collect(),
        };
        let observed = program.run(&scenario).unwrap();
        assert_eq!(expanded.run(&scenario).unwrap(), observed);
        assert_eq!(observed.outcome, outcome);
        assert_eq!(observed.fields, fields);
    }
    let uncovered = caller.replace(
        "missing => miss,\n        failed { string reason } => exit invalid { reason }",
        "missing => miss",
    );
    f.write("main.vn", &(rules.clone() + &uncovered));
    let error = f.reject("E_UNCOVERED");
    assert!(
        error
            .diagnostic
            .notes
            .iter()
            .any(|n| n.path.as_ref() == Some(&f.root.join("main.vn"))
                && n.message.contains("failed"))
    );
    assert!(error.diagnostic.notes.iter().any(|n| n.path.as_ref()
        == Some(&f.root.join("search.vn"))
        && n.message.contains("failed")));
    f.write(
        "main.vn",
        &(rules + &caller.replace("exit ok { saved }", "exit ok { deref reading }")),
    );
    f.reject("E_LOAN");
}

#[test]
fn fold_returns_the_owned_resource_on_exhaustion_stop_and_failure() {
    use tellur::runtime::ScriptCall;
    let f = Fixture::new();
    let source = include_str!("fixtures/owned_fold.vn");
    f.write("reduction.vn", include_str!("../library/reduction.vn"));
    f.write("main.vn", source);
    let checked = f.check().unwrap();
    let expanded = checked.expanded("main").unwrap();
    let settled = checked.expanded("settle").unwrap();
    let mut binaries = Vec::new();
    for (name, program) in [("original", &checked), ("expanded", &settled)] {
        let binary = f.root.join(name);
        support::scenario_executable(program)
            .build_for_test("settle", &binary, 3)
            .unwrap();
        binaries.push(binary);
    }
    let token = Value::Opaque {
        ty: "handle".into(),
        token: "scale-7".into(),
    };
    for (items, advances, outcome, last) in [
        (vec![], 0, "finished", 99),
        (vec![2, 3], 2, "finished", 3),
        (vec![10, 0], 0, "stopped", 10),
        (vec![2, 10, 0], 1, "stopped", 10),
        (vec![0, 10], 0, "failed", 99),
        (vec![2, 0, 10], 1, "failed", 2),
    ] {
        let scenario = Scenario {
            entry: "main".into(),
            arguments: vec![
                token.clone(),
                Value::Construct {
                    ty: "vector<U32>".into(),
                    name: "vector".into(),
                    fields: items.into_iter().map(Value::U32).collect(),
                },
            ],
            calls: (1..=advances)
                .map(|n| ScriptCall {
                    operation: "checked_add".into(),
                    outcome: "sum".into(),
                    fields: vec![Value::U32(n)],
                    waits: 0,
                })
                .collect(),
        };
        let result = checked.run(&scenario).unwrap();
        assert_eq!(result, expanded.run(&scenario).unwrap());
        assert_eq!(result.outcome, outcome);
        assert_eq!(result.events.len(), advances as usize);
        let mut expected = vec![Value::Construct {
            ty: "ledger".into(),
            name: "ledger".into(),
            fields: vec![token.clone(), Value::U32(last)],
        }];
        if outcome == "failed" {
            expected.push(Value::String("offline".into()));
        }
        assert_eq!(result.fields, expected);

        let mut settle = scenario;
        settle.entry = "settle".into();
        settle.calls.push(ScriptCall {
            operation: "close".into(),
            outcome: "closed".into(),
            fields: vec![],
            waits: 0,
        });
        let execution = checked.run(&settle).unwrap();
        assert_eq!(execution, settled.run(&settle).unwrap());
        assert_eq!(execution.outcome, "done");
        assert_eq!(execution.events.len(), advances as usize + 1);
        assert!(matches!(execution.events.last(),
            Some(tellur::runtime::Event::Primitive { operation, arguments, .. })
            if operation == "close" && arguments == std::slice::from_ref(&token)));
        let scenario_file = f.root.join("settle.json");
        fs::write(&scenario_file, serde_json::to_vec(&settle).unwrap()).unwrap();
        let expected = serde_json::to_value(execution).unwrap();
        for binary in &binaries {
            let run = std::process::Command::new(binary)
                .arg("--scenario")
                .arg(&scenario_file)
                .output()
                .unwrap();
            assert!(
                run.status.success(),
                "{}",
                String::from_utf8_lossy(&run.stderr)
            );
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&run.stdout).unwrap(),
                expected
            );
        }
    }
    // The caller cannot discard the resource on just one of the three exits.
    let (rule, _) = source.split_once("control main(").unwrap();
    let caller = r#"
control main(handle token, vector<u32> source) exits ready { handle token }, rejected {
    log_rule rule = log_rule { stop: 10 };
    ledger initial = ledger { token: token, last: 99 };
    delta reduction.fold<log_rule>(borrow source, borrow rule, initial) {
        finished { ledger result } => exit ready { result.token },
        stopped { ledger final_state } => exit ready { final_state.token },
        failed { ledger partial, string reason } => exit rejected
    }
}"#;
    f.write("main.vn", &format!("{rule}{caller}"));
    f.reject("E_RESOURCE");
}

#[test]
fn consuming_batch_split_and_join_preserve_owned_elements_and_invalid_input() {
    let f = Fixture::new();
    f.write("batches.vn", include_str!("../library/sequence_batches.vn"));
    f.write(
        "main.vn",
        r#"
import "batches.vn" as batches;
control main(vector<handle> original, u32 at) exits
    split { vector<handle> prefix, vector<handle> suffix },
    out_of_bounds { vector<handle> original }
{
    forward batches.split_at<handle>(original, at);
}
control combine(vector<handle> left, vector<handle> right)
    exits joined { vector<handle> values }
{
    forward batches.join<handle>(left, right);
}
"#,
    );
    let checked = f.check().unwrap();
    let expanded = checked.expanded("main").unwrap();
    let combined = checked.expanded("combine").unwrap();
    let vector = |fields| Value::Construct {
        ty: "vector<handle>".into(),
        name: "vector".into(),
        fields,
    };
    let handles: Vec<_> = (0..4)
        .map(|i| Value::Opaque {
            ty: "handle".into(),
            token: format!("request-{i}"),
        })
        .collect();
    let mut scenarios = Vec::new();
    for n in [0, 1, 4] {
        for at in [0, 1, 2, 4, 5, u32::MAX] {
            let source = handles[..n].to_vec();
            let scenario = Scenario {
                entry: "main".into(),
                arguments: vec![vector(source.clone()), Value::U32(at)],
                calls: vec![],
            };
            let result = checked.run(&scenario).unwrap();
            assert_eq!(result, expanded.run(&scenario).unwrap());
            if at as usize > n {
                assert_eq!(result.outcome, "out_of_bounds");
                assert_eq!(result.fields, vec![vector(source)]);
            } else {
                let at = at as usize;
                assert_eq!(result.outcome, "split");
                assert_eq!(
                    result.fields,
                    vec![vector(source[..at].to_vec()), vector(source[at..].to_vec())]
                );
                let join = Scenario {
                    entry: "combine".into(),
                    arguments: result.fields.clone(),
                    calls: vec![],
                };
                let joined = checked.run(&join).unwrap();
                assert_eq!(joined, combined.run(&join).unwrap());
                assert_eq!(joined.fields, vec![vector(source)]);
                scenarios.push((join, joined));
            }
            scenarios.push((scenario, result));
        }
    }
    for (name, program, entry) in [
        ("original", &checked, "main"),
        ("expanded-split", &expanded, "main"),
        ("original-join", &checked, "combine"),
        ("expanded-join", &combined, "combine"),
    ] {
        let binary = f.root.join(name);
        support::scenario_executable(program)
            .build_for_test(entry, &binary, 3)
            .unwrap();
        for (scenario, expected) in scenarios.iter().filter(|(s, _)| s.entry == entry) {
            let path = f.root.join("case.json");
            fs::write(&path, serde_json::to_vec(scenario).unwrap()).unwrap();
            let result = std::process::Command::new(&binary)
                .arg("--scenario")
                .arg(path)
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&result.stdout).unwrap(),
                serde_json::to_value(expected).unwrap()
            );
        }
    }
    // Every outstanding resource must be returned or explicitly settled.
    f.write(
        "main.vn",
        r#"
import "batches.vn" as batches;
control main(vector<handle> source) exits done {
    delta batches.split_at<handle>(source, 1) {
        split { auto prefix, auto suffix } => exit done,
        out_of_bounds { auto original } => exit done
    }
}"#,
    );
    f.reject("E_RESOURCE");
    f.write("main.vn", r#"
import "batches.vn" as batches;
control main(vector<u32> source) exits done { u32 first, vector<u32> left, vector<u32> right }, empty {
    delta vector_get(borrow source, 0) {
        found { auto first } => found,
        out_of_bounds => exit empty
    }
    [found]: delta batches.split_at<u32>(source, 1) {
        split { auto left, auto right } => exit done { deref first, left, right },
        out_of_bounds { auto original } => exit empty
    }
}"#);
    f.reject("E_LOAN");
}

#[test]
fn consuming_partition_preserves_stability_resources_and_early_stop() {
    use tellur::runtime::ScriptCall;
    let f = Fixture::new();
    let source = include_str!("fixtures/owned_partition.vn");
    f.write("partition.vn", include_str!("../library/partition.vn"));
    f.write("main.vn", source);
    let checked = f.check().unwrap();
    let expanded = checked.expanded("main").unwrap();
    let settled = checked.expanded("settle").unwrap();
    let vector = |fields| Value::Construct {
        ty: "vector<ticket>".into(),
        name: "vector".into(),
        fields,
    };
    let mut cases = Vec::new();
    for ranks in [
        vec![],
        vec![3],
        vec![1, 4, 2, 3, 1],
        vec![0, 2],
        vec![3, 1, 0, 2],
        vec![1, 3, 0],
    ] {
        let tickets: Vec<_> = ranks
            .iter()
            .enumerate()
            .map(|(i, &rank)| Value::Construct {
                ty: "ticket".into(),
                name: "ticket".into(),
                fields: vec![
                    Value::Opaque {
                        ty: "handle".into(),
                        token: format!("ticket-{i}"),
                    },
                    Value::U32(rank),
                ],
            })
            .collect();
        let stop = ranks.iter().position(|&r| r == 0);
        let end = stop.unwrap_or(ranks.len());
        let left: Vec<_> = (0..end).filter(|&i| ranks[i] < 3).collect();
        let right: Vec<_> = (0..end).filter(|&i| ranks[i] >= 3).collect();
        let mut fields = vec![
            vector(left.iter().map(|&i| tickets[i].clone()).collect()),
            vector(right.iter().map(|&i| tickets[i].clone()).collect()),
        ];
        if let Some(i) = stop {
            fields.extend([
                tickets[i].clone(),
                vector(tickets[i + 1..].to_vec()),
                Value::String("unclassified".into()),
            ]);
        }
        let scenario = Scenario {
            entry: "main".into(),
            arguments: vec![vector(tickets.clone()), Value::U32(3)],
            calls: vec![],
        };
        let result = checked.run(&scenario).unwrap();
        assert_eq!(result, expanded.run(&scenario).unwrap());
        assert_eq!(
            result.outcome,
            if stop.is_some() { "stopped" } else { "split" }
        );
        assert_eq!(result.fields, fields);
        // Only visited elements emit, in input order, including the stopping element.
        let emitted: Vec<_> = result
            .events
            .iter()
            .filter_map(|e| match e {
                tellur::runtime::Event::Emit { message } => Some(message.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(
            emitted,
            ranks[..end + usize::from(stop.is_some())]
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
        );
        cases.push((scenario.clone(), result));
        let close_order: Vec<_> = left
            .into_iter()
            .chain(right)
            .chain(end..ranks.len())
            .collect();
        let mut scenario = scenario;
        scenario.entry = "settle".into();
        scenario.calls = close_order
            .iter()
            .enumerate()
            .map(|(n, _)| ScriptCall {
                operation: "close".into(),
                outcome: if n == 0 { "failed" } else { "closed" }.into(),
                fields: if n == 0 {
                    vec![Value::Opaque {
                        ty: "close_error".into(),
                        token: "closed with error".into(),
                    }]
                } else {
                    vec![]
                },
                waits: 0,
            })
            .collect();
        let result = checked.run(&scenario).unwrap();
        assert_eq!(result, settled.run(&scenario).unwrap());
        let closed: Vec<_> = result
            .events
            .iter()
            .filter_map(|e| match e {
                tellur::runtime::Event::Primitive {
                    operation,
                    arguments,
                    ..
                } if operation == "close" => Some(arguments.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(
            closed,
            close_order
                .iter()
                .map(|&i| vec![Value::Opaque {
                    ty: "handle".into(),
                    token: format!("ticket-{i}")
                }])
                .collect::<Vec<_>>()
        );
        assert_eq!(closed.len(), tickets.len());
        cases.push((scenario, result));
    }
    for (name, program, entry) in [
        ("original", &checked, "main"),
        ("expanded", &expanded, "main"),
        ("original-settle", &checked, "settle"),
        ("expanded-settle", &settled, "settle"),
    ] {
        let binary = f.root.join(name);
        support::scenario_executable(program)
            .build_for_test(entry, &binary, 3)
            .unwrap();
        for (scenario, expected) in cases.iter().filter(|(s, _)| s.entry == entry) {
            let path = f.root.join("case.json");
            fs::write(&path, serde_json::to_vec(scenario).unwrap()).unwrap();
            let result = std::process::Command::new(&binary)
                .arg("--scenario")
                .arg(&path)
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&result.stdout).unwrap(),
                serde_json::to_value(expected).unwrap()
            );
        }
    }
    let rules = source.split_once("control main(").unwrap().0;
    f.write(
        "main.vn",
        &format!(
            r#"{rules}
control main(vector<ticket> source) exits done {{
    pivot rule = pivot {{ 3 }};
    delta partition.partition<pivot>(source, borrow rule) {{
        split {{ auto left, auto right }} => exit done,
        stopped {{ auto left, auto right, auto current, auto remaining, auto reason }} => exit done
    }}
}}"#
        ),
    );
    f.reject("E_RESOURCE");
    f.write("main.vn", &format!(r#"{rules}
control main(vector<ticket> source) exits done {{ u32 rank, vector<ticket> left, vector<ticket> right }},
    empty {{ vector<ticket> original }} {{
    pivot rule = pivot {{ 3 }};
    delta vector_get(borrow source, 0) {{
        found {{ auto first }} => found,
        out_of_bounds => exit empty {{ source }}
    }}
    [found]: delta partition.partition<pivot>(source, borrow rule) {{
        split {{ auto left, auto right }} => exit done {{ first.rank, left, right }},
        stopped {{ auto left, auto right, auto current, auto remaining, auto reason }} => exit done {{ first.rank, left, right }}
    }}
}}"#));
    f.reject("E_LOAN");
}

#[test]
fn stable_ordering_preserves_duplicate_resources_and_comparator_effects() {
    use tellur::runtime::{Event, ScriptCall};
    let f = Fixture::new();
    let source = include_str!("fixtures/owned_ordering.vn");
    f.write("ordering.vn", include_str!("../library/ordering.vn"));
    f.write(
        "sequence_batches.vn",
        include_str!("../library/sequence_batches.vn"),
    );
    f.write("main.vn", source);
    let checked = f.check().unwrap();
    let expanded = checked.expanded("main").unwrap();
    let settled = checked.expanded("settle").unwrap();
    let vector = |fields| Value::Construct {
        ty: "vector<ticket>".into(),
        name: "vector".into(),
        fields,
    };
    // Independent index-only merge model checks the observable comparator sequence.
    // Rust's stable sort supplies the final ownership/stability oracle separately.
    fn comparisons(keys: &[u32]) -> Vec<String> {
        let mut runs: Vec<Vec<usize>> = (0..keys.len()).map(|i| vec![i]).collect();
        let mut events = Vec::new();
        while runs.len() > 1 {
            runs = runs
                .chunks(2)
                .map(|pair| {
                    if pair.len() == 1 {
                        return pair[0].clone();
                    }
                    let (a, b) = (&pair[0], &pair[1]);
                    let (mut i, mut j) = (0, 0);
                    let mut merged = Vec::new();
                    while i < a.len() && j < b.len() {
                        events.extend([a[i].to_string(), b[j].to_string()]);
                        if keys[a[i]] <= keys[b[j]] {
                            merged.push(a[i]);
                            i += 1;
                        } else {
                            merged.push(b[j]);
                            j += 1;
                        }
                    }
                    merged.extend_from_slice(&a[i..]);
                    merged.extend_from_slice(&b[j..]);
                    merged
                })
                .collect();
        }
        events
    }
    let mut cases = Vec::new();
    for keys in [
        vec![],
        vec![3],
        vec![4, 3, 2, 1],
        vec![2, 1, 2, 1, 3],
        vec![1, 2, 3, 4],
        vec![3, 3, 3, 3, 3],
        vec![u32::MAX, 0, u32::MAX, 1, 0],
        vec![4, 1, 5, 2, 0, 4, 3, 1, 5],
    ] {
        let tickets: Vec<_> = keys
            .iter()
            .enumerate()
            .map(|(i, &key)| Value::Construct {
                ty: "ticket".into(),
                name: "ticket".into(),
                fields: vec![
                    Value::U32(key),
                    Value::Opaque {
                        ty: "handle".into(),
                        token: format!("ticket-{i}"),
                    },
                    Value::U32(i as u32),
                ],
            })
            .collect();
        let mut order: Vec<_> = (0..keys.len()).collect();
        order.sort_by_key(|&i| keys[i]);
        for entry in ["main", "settle"] {
            let scenario = Scenario {
                entry: entry.into(),
                arguments: vec![vector(tickets.clone())],
                calls: if entry == "settle" {
                    order
                        .iter()
                        .enumerate()
                        .map(|(n, _)| ScriptCall {
                            operation: "close".into(),
                            outcome: if n == 0 { "failed" } else { "closed" }.into(),
                            fields: if n == 0 {
                                vec![Value::Opaque {
                                    ty: "close_error".into(),
                                    token: "closed with error".into(),
                                }]
                            } else {
                                vec![]
                            },
                            waits: 0,
                        })
                        .collect()
                } else {
                    vec![]
                },
            };
            let result = checked.run(&scenario).unwrap();
            assert_eq!(
                result,
                if entry == "main" {
                    expanded.run(&scenario)
                } else {
                    settled.run(&scenario)
                }
                .unwrap()
            );
            assert_eq!(
                result.outcome,
                if entry == "main" { "sorted" } else { "done" }
            );
            assert_eq!(
                result.fields,
                if entry == "main" {
                    vec![vector(order.iter().map(|&i| tickets[i].clone()).collect())]
                } else {
                    vec![]
                }
            );
            let emitted: Vec<_> = result
                .events
                .iter()
                .filter_map(|e| match e {
                    Event::Emit { message } => Some(message.clone()),
                    _ => None,
                })
                .collect();
            assert_eq!(emitted, comparisons(&keys));
            let closed: Vec<_> = result
                .events
                .iter()
                .filter_map(|e| match e {
                    Event::Primitive {
                        operation,
                        arguments,
                        ..
                    } if operation == "close" => Some(arguments.clone()),
                    _ => None,
                })
                .collect();
            assert_eq!(
                closed,
                if entry == "settle" {
                    order
                        .iter()
                        .map(|&i| {
                            vec![Value::Opaque {
                                ty: "handle".into(),
                                token: format!("ticket-{i}"),
                            }]
                        })
                        .collect::<Vec<_>>()
                } else {
                    vec![]
                }
            );
            cases.push((scenario, result));
        }
    }
    for (name, program, entry) in [
        ("original", &checked, "main"),
        ("expanded", &expanded, "main"),
        ("original-settle", &checked, "settle"),
        ("expanded-settle", &settled, "settle"),
    ] {
        let binary = f.root.join(name);
        support::scenario_executable(program)
            .build_for_test(entry, &binary, 3)
            .unwrap();
        for (scenario, expected) in cases.iter().filter(|(s, _)| s.entry == entry) {
            let path = f.root.join("case.json");
            fs::write(&path, serde_json::to_vec(scenario).unwrap()).unwrap();
            let result = std::process::Command::new(&binary)
                .arg("--scenario")
                .arg(&path)
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&result.stdout).unwrap(),
                serde_json::to_value(expected).unwrap()
            );
        }
    }
    let rules = source.split_once("control main(").unwrap().0;
    f.write(
        "main.vn",
        &format!(
            r#"{rules}
control main(vector<ticket> source) exits done {{
    key_order rule=key_order {{}};
    delta ordering.sort<key_order>(source,borrow rule) {{ sorted {{auto values}}=>exit done }}
}}"#
        ),
    );
    f.reject("E_RESOURCE");
    f.write("main.vn",&format!(r#"{rules}
control main(vector<ticket> source) exits done {{u32 key,vector<ticket> values}}, empty {{vector<ticket> values}} {{
    key_order rule=key_order {{}};
    delta vector_get(borrow source,0) {{ found {{auto first}}=>found,out_of_bounds=>exit empty {{source}} }}
    [found]: delta ordering.sort<key_order>(source,borrow rule) {{ sorted {{auto values}}=>exit done {{first.key,values}} }}
}}"#));
    f.reject("E_LOAN");
}

#[test]
fn ordered_bounds_preserve_resources_effects_and_first_equivalent_identity() {
    use tellur::runtime::{Event, ScriptCall};
    let f = Fixture::new();
    let source = include_str!("fixtures/ordered_bounds.vn");
    for (name, text) in [
        ("ordering.vn", include_str!("../library/ordering.vn")),
        (
            "ordered_search.vn",
            include_str!("../library/ordered_search.vn"),
        ),
        (
            "sequence_batches.vn",
            include_str!("../library/sequence_batches.vn"),
        ),
    ] {
        f.write(name, text);
    }
    f.write("main.vn", source);
    let checked = f.check().unwrap();
    let expanded = checked.expanded("main").unwrap();
    let settled = checked.expanded("settle").unwrap();
    let ticket = |key, id| Value::Construct {
        ty: "ticket".into(),
        name: "ticket".into(),
        fields: vec![
            Value::U32(key),
            Value::Opaque {
                ty: "handle".into(),
                token: format!("ticket-{id}"),
            },
            Value::U32(id),
        ],
    };
    fn arithmetic(keys: &[u32], key: u32, descending: bool, upper: bool) -> Vec<ScriptCall> {
        let mut calls = Vec::new();
        let mut push = |operation: &str, outcome: &str, value: usize| {
            calls.push(ScriptCall {
                operation: operation.into(),
                outcome: outcome.into(),
                fields: vec![Value::U32(value as u32)],
                waits: 0,
            })
        };
        let (mut start, mut count) = (0, keys.len());
        while count > 0 {
            let half = count / 2;
            let middle = start + half;
            push("checked_div", "quotient", half);
            push("checked_add", "sum", middle);
            let before = if descending {
                keys[middle] > key
            } else {
                keys[middle] < key
            };
            if before || (upper && keys[middle] == key) {
                start = middle + 1;
                push("checked_add", "sum", start);
                push("checked_sub", "difference", count - half);
                count -= half + 1;
                push("checked_sub", "difference", count);
            } else {
                count = half;
            }
        }
        calls
    }
    let mut cases = vec![];
    for descending in [false, true] {
        for mut keys in [vec![], vec![3], vec![1, 2, 2, 2, 9], vec![0, 0, u32::MAX]] {
            if descending {
                keys.reverse();
            }
            let tickets: Vec<_> = keys
                .iter()
                .enumerate()
                .map(|(i, &key)| ticket(key, i as u32))
                .collect();
            let vector = Value::Construct {
                ty: "vector<ticket>".into(),
                name: "vector".into(),
                fields: tickets.clone(),
            };
            for key in [0, 1, 2, 3, 4, 9, u32::MAX] {
                let needle = ticket(key, 999);
                let lower = keys.partition_point(|&x| if descending { x > key } else { x < key });
                let upper = keys.partition_point(|&x| if descending { x >= key } else { x <= key });
                for entry in ["main", "settle"] {
                    let scenario = Scenario {
                        entry: entry.into(),
                        arguments: vec![vector.clone(), needle.clone(), Value::Bool(descending)],
                        calls: {
                            let mut calls = arithmetic(&keys, key, descending, false);
                            calls.extend(arithmetic(&keys, key, descending, true));
                            calls.extend(arithmetic(&keys, key, descending, false));
                            if entry == "settle" {
                                calls.extend((0..=keys.len()).map(|_| ScriptCall {
                                    operation: "close".into(),
                                    outcome: "closed".into(),
                                    fields: vec![],
                                    waits: 0,
                                }));
                            }
                            calls
                        },
                    };
                    let result = checked.run(&scenario).unwrap();
                    for event in &result.events {
                        if let Event::Primitive {
                            operation,
                            arguments,
                            fields,
                            ..
                        } = event
                            && operation.starts_with("checked_")
                        {
                            let [Value::U32(a), Value::U32(b)] = arguments.as_slice() else {
                                panic!("arithmetic arguments")
                            };
                            let expected = match operation.as_str() {
                                "checked_add" => a.checked_add(*b),
                                "checked_sub" => a.checked_sub(*b),
                                "checked_div" => a.checked_div(*b),
                                _ => panic!("unexpected arithmetic"),
                            }
                            .unwrap();
                            assert_eq!(fields, &vec![Value::U32(expected)]);
                        }
                    }

                    assert_eq!(
                        result,
                        if entry == "main" {
                            expanded.run(&scenario)
                        } else {
                            settled.run(&scenario)
                        }
                        .unwrap()
                    );
                    assert_eq!(
                        result.outcome,
                        if entry == "main" { "queried" } else { "done" }
                    );
                    if entry == "main" {
                        assert_eq!(
                            result.fields,
                            vec![
                                Value::Bool(lower < upper),
                                Value::U32(lower as u32),
                                Value::U32(upper as u32),
                                Value::U32(lower as u32),
                                Value::U32(if lower < upper { lower as u32 } else { 0 }),
                                vector.clone(),
                                needle.clone()
                            ]
                        );
                    } else {
                        assert!(result.fields.is_empty());
                        let closes: Vec<_> = result
                            .events
                            .iter()
                            .filter_map(|event| match event {
                                Event::Primitive {
                                    operation,
                                    arguments,
                                    ..
                                } if operation == "close" => Some(arguments.clone()),
                                _ => None,
                            })
                            .collect();
                        let expected: Vec<_> = (0..keys.len())
                            .map(|i| {
                                vec![Value::Opaque {
                                    ty: "handle".into(),
                                    token: format!("ticket-{i}"),
                                }]
                            })
                            .chain(std::iter::once(vec![Value::Opaque {
                                ty: "handle".into(),
                                token: "ticket-999".into(),
                            }]))
                            .collect();
                        assert_eq!(closes, expected);
                    }
                    if !descending && keys == [1, 2, 2, 2, 9] && key == 2 {
                        let emits: Vec<_> = result
                            .events
                            .iter()
                            .filter_map(|event| match event {
                                Event::Emit { message } => Some(message.clone()),
                                _ => None,
                            })
                            .collect();
                        let expected: Vec<_> = [2, 1, 0, 2, 4, 3, 2, 1, 0, 1]
                            .into_iter()
                            .flat_map(|i| [i.to_string(), "999".into()])
                            .collect();
                        assert_eq!(emits, expected);
                    }
                    cases.push((scenario, result));
                }
            }
        }
    }
    for (name, program, entry) in [
        ("original", &checked, "main"),
        ("expanded", &expanded, "main"),
        ("original-settle", &checked, "settle"),
        ("expanded-settle", &settled, "settle"),
    ] {
        for opt in [0, 3] {
            let binary = f.root.join(format!("{name}-{opt}"));
            support::scenario_executable(program)
                .build_for_test(entry, &binary, opt)
                .unwrap();
            for (scenario, expected) in cases.iter().filter(|(s, _)| s.entry == entry) {
                let path = f.root.join("case.json");
                fs::write(&path, serde_json::to_vec(scenario).unwrap()).unwrap();
                let result = std::process::Command::new(&binary)
                    .arg("--scenario")
                    .arg(&path)
                    .output()
                    .unwrap();
                assert!(
                    result.status.success(),
                    "{}",
                    String::from_utf8_lossy(&result.stderr)
                );
                assert_eq!(
                    serde_json::from_slice::<serde_json::Value>(&result.stdout).unwrap(),
                    serde_json::to_value(expected).unwrap()
                );
            }
        }
    }
}

#[test]
fn ordered_search_keeps_owner_obligations_and_live_result_loans() {
    let f = Fixture::new();
    for (name, text) in [
        ("ordering.vn", include_str!("../library/ordering.vn")),
        (
            "ordered_search.vn",
            include_str!("../library/ordered_search.vn"),
        ),
        (
            "sequence_batches.vn",
            include_str!("../library/sequence_batches.vn"),
        ),
    ] {
        f.write(name, text);
    }
    let rules = include_str!("fixtures/ordered_bounds.vn")
        .split_once("control main(")
        .unwrap()
        .0;
    f.write(
        "main.vn",
        &format!(
            r#"{rules}
control main(vector<ticket> source, ticket needle) exits done {{
    key_order rule=key_order {{false}};
    delta search.first_equal<key_order>(borrow source,borrow rule,borrow needle) {{
        found {{u32 index,auto value}}=>exit done,
        missing {{u32 insertion}}=>exit done
    }}
}}"#
        ),
    );
    f.reject("E_RESOURCE");
    f.write("main.vn", &format!(r#"{rules}
control main(vector<ticket> source, ticket needle)
    exits found {{u32 key,vector<ticket> values,ticket needle}}, missing {{vector<ticket> values,ticket needle}}
{{
    key_order rule=key_order {{false}};
    delta search.first_equal<key_order>(borrow source,borrow rule,borrow needle) {{
        found {{u32 index,auto value}}=>hit,
        missing {{u32 insertion}}=>exit missing {{source,needle}}
    }}
    [hit]: exit found {{value.key,source,needle}};
}}"#));
    f.reject("E_LOAN");
    f.write("main.vn", include_str!("fixtures/ordered_bounds.vn"));
    f.write(
        "ordered_search.vn",
        &include_str!("../library/ordered_search.vn").replace(
            "exit found { index, value }",
            "exit found { index, needle }",
        ),
    );
    f.reject("E_LIFETIME");
}
