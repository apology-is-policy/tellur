use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};
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
        compile_file(self.root.join("main.tellur"), m())
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
const PEOPLE: &str = include_str!("../playground/modules/people.tellur");
const MAIN: &str = include_str!("../playground/modules/main.tellur");
#[test]
fn exported_payloads_are_explicit_while_importers_can_infer() {
    let f = Fixture::new();
    f.write(
        "lookup.tellur",
        "pub control lookup() exits ready {string text} {exit ready {\"hello\"};}",
    );
    f.write(
        "main.tellur",
        "import \"lookup.tellur\" as lookup;control main() exits ready... {forward lookup.lookup();}",
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
        "views.tellur",
        "pub struct view {pub ref string text;}pub struct hidden {ref string text;}",
    );
    let source = "import \"views.tellur\" as views; control main() exits done {string owner=\"hello\";views.view v=views.view {borrow owner};emit(v.text);exit done;}";
    f.write("main.tellur", source);
    same(&f.check().unwrap());
    f.write("main.tellur", &source.replace("views.view", "views.hidden"));
    f.reject("E_VISIBILITY");
    f.write("people.tellur", PEOPLE);
    f.write("main.tellur", "import \"people.tellur\" as people;struct view {ref people.person value;}control main() exits done {delta people.create(\"Ada\",37) {ready {auto person}=>ready}[ready]:view v=view {borrow person};emit(borrow v.value.name);exit done;}");
    f.reject("E_VISIBILITY");
}
#[test]
fn imports_private_representation_public_fields_and_borrowed_results_work() {
    let f = Fixture::new();
    f.write("people.tellur", PEOPLE);
    f.write("main.tellur", MAIN);
    let p = f.check().unwrap();
    same(&p);
    assert_eq!(p.source_files().len(), 2);
    assert!(p.inspect("main").unwrap().contains("people.tellur"));
    assert_eq!(
        p.source_files().len(),
        p.expanded("main").unwrap().source_files().len()
    );
}
#[test]
fn private_controls_fields_and_constructors_cannot_be_reached() {
    let f = Fixture::new();
    f.write("people.tellur", PEOPLE);
    for source in [
        "import \"people.tellur\" as people;control main() exits done {forward people.internal_note();}",
        "import \"people.tellur\" as people;control main() exits done {people.person value=people.person {\"Ada\",37};exit done;}",
        "import \"people.tellur\" as people;control main() exits done {delta people.create(\"Ada\",37) {ready {auto value}=>created}[created]:emit(value.name);exit done;}",
        "import \"people.tellur\" as people;control main() exits done {delta people.create(\"Ada\",37) {ready {auto value}=>created}[created]:auto view=&value.name;exit done;}",
        "import \"people.tellur\" as people;control main() exits done {delta people.create(\"Ada\",37) {ready {auto value}=>created}[created]:mutable people.person owner=value;owner.name=\"changed\";exit done;}",
    ] {
        f.write("main.tellur", source);
        let e = f.reject("E_VISIBILITY");
        assert_eq!(e.path, f.root.join("main.tellur"));
    }
}
#[test]
fn private_type_names_and_public_interface_leaks_are_rejected() {
    let f = Fixture::new();
    f.write(
        "main.tellur",
        "import \"lib.tellur\" as lib;control main() exits done {exit done;}",
    );
    for source in [
        "struct secret {u32 value;} pub control leak() exits ready {secret value} {exit ready {secret {1}};}",
        "struct secret {u32 value;} pub struct public_box {pub secret value;}",
        "struct secret {u32 value;} pub variant public_choice {yes {secret value},no}",
        "struct secret {u32 value;} pub outcome public_result {yes {secret value},no}",
    ] {
        f.write("lib.tellur", source);
        let e = f.reject("E_VISIBILITY");
        assert_eq!(e.path, f.root.join("lib.tellur"));
    }
    f.write(
        "lib.tellur",
        "struct secret {u32 value;} pub struct public_box {secret value;}",
    );
    f.check().unwrap();
    f.write(
        "main.tellur",
        "import \"lib.tellur\" as lib;control main(lib.secret value) exits done {exit done;}",
    );
    f.reject("E_VISIBILITY");
}
#[test]
fn aliases_and_diamond_imports_share_one_nominal_identity() {
    let f = Fixture::new();
    f.write("people.tellur", PEOPLE);
    f.write("relay.tellur","import \"people.tellur\" as staff;pub control relay(staff.person value) exits ready {staff.person value} {exit ready {value};}");
    f.write("main.tellur","import \"people.tellur\" as first;import \"./people.tellur\" as second;import \"relay.tellur\" as relay;control main() exits done {delta first.create(\"Ada\",37) {ready {second.person value}=>created}[created]:delta relay.relay(value) {ready {first.person returned}=>returned}[returned]:exit done;}");
    let p = f.check().unwrap();
    assert_eq!(p.source_files().len(), 3);
    same(&p);
}
#[test]
fn separate_files_with_same_type_spelling_stay_nominally_distinct() {
    let f = Fixture::new();
    for name in ["a.tellur", "b.tellur"] {
        f.write(name, "pub struct id {pub u32 value;}");
    }
    f.write("main.tellur","import \"a.tellur\" as a;import \"b.tellur\" as b;control main() exits done {a.id first=a.id {7};b.id second=first;exit done;}");
    f.reject("E_TYPE");
}
#[test]
fn private_constructor_tags_do_not_pollute_other_modules_or_builtin_options() {
    let f = Fixture::new();
    f.write(
        "lib.tellur",
        "variant hidden {some {string text},none} pub variant public_choice {yes,no}",
    );
    f.write("main.tellur","import \"lib.tellur\" as lib;control main() exits ready {option<u32> value} {option<u32> value=some {7};exit ready {value};}");
    same(&f.check().unwrap());
    f.write(
        "main.tellur",
        "import \"lib.tellur\" as lib;control main() exits done {auto value=yes {};exit done;}",
    );
    f.reject("E_NAME");
    f.write("main.tellur","import \"lib.tellur\" as lib;control main() exits done {auto value=lib.public_choice.yes {};exit done;}");
    same(&f.check().unwrap());
}
#[test]
fn imports_resolve_relative_to_importer_and_report_cycles_and_missing_files() {
    let f = Fixture::new();
    f.write(
        "main.tellur",
        "import \"sub/a.tellur\" as a;control main() exits done {forward a.go();}",
    );
    f.write(
        "sub/a.tellur",
        "import \"../b.tellur\" as b;pub control go() exits done {forward b.go();}",
    );
    f.write("b.tellur", "pub control go() exits done {exit done;}");
    same(&f.check().unwrap());
    f.write(
        "b.tellur",
        "import \"main.tellur\" as root;pub control go() exits done {exit done;}",
    );
    f.reject("E_IMPORT_CYCLE");
    fs::remove_file(f.root.join("b.tellur")).unwrap();
    let e = f.reject("E_IMPORT");
    assert_eq!(e.path, f.root.join("sub/a.tellur"));
}
#[test]
fn dependency_diagnostics_have_original_file_and_line_and_unsaved_loader_is_used() {
    let f = Fixture::new();
    f.write(
        "main.tellur",
        "import \"lib.tellur\" as lib;\ncontrol main() exits done {forward lib.go();}",
    );
    f.write(
        "lib.tellur",
        "// Header\npub control go() exits done {\n string value=7;\n exit done;\n}",
    );
    let e = f.reject("E_TYPE");
    assert_eq!(e.path, f.root.join("lib.tellur"));
    assert_eq!(e.diagnostic.span.line, 3);
    let p = tellur::modules::compile_with_loader(f.root.join("main.tellur"), m(), |p| {
        if p.ends_with("lib.tellur") {
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
    f.write("lib.tellur", "pub control go() exits done {exit done;}");
    for source in [
        "import \"lib.tellur\" as lib;control lib() exits done {exit done;}",
        "import \"lib.tellur\" as lib;control main() exits done {u32 lib=7;exit done;}",
        "import \"lib.tellur\" as lib;import \"lib.tellur\" as lib;",
    ] {
        f.write("main.tellur", source);
        f.reject("E_NAME");
    }
    assert_eq!(
        compile("import \"lib.tellur\" as lib;", m())
            .unwrap_err()
            .code,
        "E_IMPORT"
    );
}
#[test]
fn imported_resources_retain_ownership_and_completion_requirements() {
    let f = Fixture::new();
    f.write("lib.tellur","pub struct resource_box {handle file;} pub control wrap(handle file) exits ready {resource_box value} {exit ready {resource_box {file}};} pub control unwrap(resource_box value) exits ready {handle file} {exit ready {value.file};}");
    f.write("main.tellur","import \"lib.tellur\" as lib;control main(handle file) exits ready {handle file} {delta lib.wrap(file) {ready {auto box}=>wrapped}[wrapped]:forward lib.unwrap(box);}");
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
    f.write("main.tellur","import \"lib.tellur\" as lib;control main(handle file) exits done {delta lib.wrap(file) {ready {auto box}=>wrapped}[wrapped]:exit done;}");
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
    f.write("lib.tellur", "pub struct id {pub u32 value;}");
    std::os::unix::fs::symlink(f.root.join("lib.tellur"), f.root.join("link.tellur")).unwrap();
    f.write("main.tellur","import \"lib.tellur\" as a;import \"link.tellur\" as b;control main() exits done {a.id first=a.id {7};b.id second=first;exit done;}");
    let p = f.check().unwrap();
    assert_eq!(p.source_files().len(), 2);
    same(&p);
    f.write("lib.tellur", "import \"link.tellur\" as alias;");
    f.reject("E_IMPORT_CYCLE");
}
#[test]
fn source_map_errors_and_cli_imports_survive_a_different_working_directory() {
    let f = Fixture::new();
    f.write("people.tellur", PEOPLE);
    f.write("main.tellur", MAIN);
    fs::write(
        f.root.join("manifest.json"),
        include_str!("../playground/manifest.json"),
    )
    .unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_tellurc"))
        .args(["check"])
        .arg(f.root.join("main.tellur"))
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
    f.write("people.tellur", PEOPLE);
    f.write("main.tellur", MAIN);
    let original = f.check().unwrap();
    let expected = serde_json::to_value(original.run(&scenario()).unwrap()).unwrap();
    let expanded = original.expanded("main").unwrap();
    for (name, p) in [("original", original), ("expanded", expanded)] {
        for opt in [0, 2] {
            let binary = f.root.join(format!("{name}-{opt}"));
            p.build_native("main", &binary, opt).unwrap();
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
    f.write("main.tellur", &" ".repeat(8 * 1024 * 1024 + 1));
    f.reject("E_IMPORT_LIMIT");
    f.write("main.tellur", "import \"m1.tellur\" as next;");
    for i in 1..257 {
        f.write(
            &format!("m{i}.tellur"),
            &format!("import \"m{}.tellur\" as next;", i + 1),
        );
    }
    f.write("m257.tellur", "struct last {}");
    f.reject("E_IMPORT_LIMIT");
}

#[test]
fn constructor_resolution_respects_lexical_binding_order_and_scopes() {
    let f = Fixture::new();
    f.write("lib.tellur","variant state {empty,full {u32 value}} pub control go() exits done {auto first=state.empty; state state=state.empty {};exit done;}");
    f.write(
        "main.tellur",
        "import \"lib.tellur\" as lib;control main() exits done {forward lib.go();}",
    );
    same(&f.check().unwrap());
}

#[test]
fn named_fields_resolve_namespaces_and_preserve_representation_privacy() {
    let f = Fixture::new();
    f.write("people.tellur","pub struct person {pub string name;pub u32 age;} pub control create() exits ready {person value} {exit ready {person {age:36,name:\"Ada\"}};}");
    f.write("main.tellur","import \"people.tellur\" as people; control main() exits auto {auto value=people.person {age:36,name:\"Ada\"};exit ready {value.name,value.age};}");
    let p = f.check().unwrap();
    same(&p);
    assert_eq!(
        p.run(&scenario()).unwrap().fields,
        vec![Value::String("Ada".into()), Value::U32(36)]
    );
    f.write(
        "people.tellur",
        "pub struct person {string name;pub u32 age;}",
    );
    f.reject("E_VISIBILITY");
    f.write(
        "people.tellur",
        "struct person {pub string name;pub u32 age;}",
    );
    f.reject("E_VISIBILITY");
}

#[test]
fn generic_modules_keep_shared_nominal_identity_and_private_representation() {
    let f = Fixture::new();
    let source = "pub struct box<t>{pub t value;} pub variant choice<t>{some {t value},none} pub outcome result<t>{ready {t value},empty}";
    f.write("data.tellur", source);
    f.write("main.tellur","import \"data.tellur\" as a;import \"data.tellur\" as b;control main() exits a.result<string> {a.box<string> first=a.box<string>{value:\"Ada\"};b.box<string> second=first;auto tag=b.choice<string>.some {second.value};exit delta tag {some {auto value}=>ready {value},none=>empty {}};}");
    let p = f.check().unwrap();
    same(&p);
    assert_eq!(
        p.run(&scenario()).unwrap().fields,
        vec![Value::String("Ada".into())]
    );
    f.write("data.tellur", &source.replace("pub t value", "t value"));
    f.reject("E_VISIBILITY");
    f.write("data.tellur","pub struct box<t>{pub t value;}struct hidden{}pub control leak() exits ready {box<hidden> value} {exit ready {box<hidden>{hidden{}}};}");
    f.write(
        "main.tellur",
        "import \"data.tellur\" as data;control main() exits done {exit done;}",
    );
    f.reject("E_VISIBILITY");
    f.write("data.tellur","struct box<t>{pub t value;}pub control leak() exits ready {box<u32> value} {exit ready {box<u32>{1}};}");
    f.reject("E_VISIBILITY");
}

#[test]
fn generic_controls_keep_module_privacy_and_universal_definition_checks() {
    let f = Fixture::new();
    f.write("library.tellur","pub struct box<t>{t value;}pub control wrap<t>(t value) exits ready {box<t> value} {exit ready {box<t>{value}};}pub control unpack<t>(box<t> value) exits ready {t value} {exit ready {value.value};}control secret<t>(t value) exits ready {t value} {exit ready {value};}");
    f.write("main.tellur","import \"library.tellur\" as library;control main() exits auto {exit delta library.wrap<string>(\"Ada\") {ready {auto value}=>delta library.unpack<string>(value) {ready {auto text}=>done {text}}};}");
    let p = f.check().unwrap();
    same(&p);
    assert_eq!(
        p.run(&scenario()).unwrap().fields,
        vec![Value::String("Ada".into())]
    );
    f.write("main.tellur","import \"library.tellur\" as library;control main() exits auto {forward library.secret<string>(\"x\");}");
    f.reject("E_VISIBILITY");
    f.write(
        "main.tellur",
        "import \"library.tellur\" as library;control main() exits done {exit done;}",
    );
    f.write(
        "library.tellur",
        "pub control bad<t>(t value) exits done {exit done;}",
    );
    let e = f.reject("E_RESOURCE");
    assert!(e.path.ends_with("library.tellur"));
    f.write("library.tellur","struct hidden{}pub control bad<t>(t value) exits ready {t value,hidden secret} {exit ready {value,hidden{}};}");
    f.reject("E_VISIBILITY");
}

#[test]
fn generic_parameter_constructor_shadowing_is_independent_of_module_prefixes() {
    let f = Fixture::new();
    f.write("library.tellur","struct item{}pub control bad<item>(item value) exits ready {item value} {auto wrong=item{};exit ready {value};}");
    f.write(
        "main.tellur",
        "import \"library.tellur\" as library;control main() exits done {exit done;}",
    );
    let e = f.reject("E_NAME");
    assert!(e.diagnostic.message.contains("generic definition check"));
}

#[test]
fn qualified_generic_and_non_generic_single_completion_statements_work() {
    let f = Fixture::new();
    f.write("library.tellur","pub control show() exits done {emit(\"plain\");exit done;}pub control observe<t>(&t value) exits done {emit(\"generic\");exit done;}");
    f.write("main.tellur","import \"library.tellur\" as library;control main() exits done {string value=\"Ada\";library.show();library.observe<string>(&value);exit done;}");
    let p = f.check().unwrap();
    same(&p);
}

#[test]
fn public_generic_can_be_used_with_private_arguments_without_exporting_them() {
    let f = Fixture::new();
    let source = "struct hidden{u32 value;}pub control identity<t>(t value) exits ready {t value} {exit ready {value};}control main() exits auto {exit delta identity<hidden>(hidden{7}) {ready {auto value}=>done {value.value}};}";
    f.write("main.tellur", source);
    let p = f.check().unwrap();
    same(&p);
    f.write(
        "main.tellur",
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
    f.write("library.tellur","pub control duplicate<t: clone>(&t value) exits ready {t value} {exit ready {clone(value)};}");
    f.write("main.tellur","import \"library.tellur\" as lib; struct private_box{string value;} control main() exits auto {private_box value=private_box{value:\"Ada\"};exit delta lib.duplicate<private_box>(&value) {ready {auto copy}=>done {copy.value}};}");
    assert_eq!(
        f.check().unwrap().run(&scenario()).unwrap().fields,
        vec![Value::String("Ada".into())]
    );
    f.write("library.tellur","pub control duplicate<t: copy>(&t value) exits ready {t value} {exit ready {clone(value)};}");
    let e = f.reject("E_BOUND");
    assert!(e.to_string().contains("main.tellur"));
    f.write("library.tellur","pub control duplicate<t: drop>(&t value) exits ready {t value} {exit ready {clone(value)};}");
    let e = f.reject("E_CLONE");
    assert!(e.to_string().contains("library.tellur"));
}

#[test]
fn trait_calls_and_associated_types_resolve_through_imports_and_keep_defining_privacy() {
    let f = Fixture::new();
    f.write(
        "reader.tellur",
        "pub trait reader {type item;control read(&self input) exits ready {item value};}",
    );
    f.write("people.tellur","import \"reader.tellur\" as io; pub struct person {string name;} pub control make() exits ready {person value} {exit ready {person {name:\"Ada\"}};} impl io.reader for person {type item=string;control read(&self input) exits ready {item value} {exit ready {clone(&(*input).name)};}}");
    let source = "import \"reader.tellur\" as io; import \"people.tellur\" as people; control main() exits auto {delta people.make() {ready {auto value}=>ready} [ready]:io.reader<people.person>.item expected=\"Ada\";exit delta io.reader<people.person>.read(&value) {ready {auto name}=>done {name==expected}};}";
    f.write("main.tellur", source);
    let p = f.check().unwrap();
    assert_eq!(p.run(&scenario()).unwrap().fields, vec![Value::Bool(true)]);
    assert_eq!(
        p.run(&scenario()).unwrap(),
        p.expanded("main").unwrap().run(&scenario()).unwrap()
    );
    f.write(
        "reader.tellur",
        "trait reader {type item;control read(&self input) exits ready {item value};}",
    );
    f.reject("E_VISIBILITY");
}
#[test]
fn trait_or_type_ownership_controls_coherence_and_diamond_imports_share_implementations() {
    let f = Fixture::new();
    f.write(
        "a.tellur",
        "pub trait mark {} pub struct token {pub u32 value;} impl mark for token {}",
    );
    f.write("b.tellur", "import \"a.tellur\" as a;");
    f.write(
        "main.tellur",
        "import \"a.tellur\" as a; import \"b.tellur\" as b; control main() exits done {exit done;}",
    );
    f.check().unwrap();
    f.write(
        "main.tellur",
        "import \"a.tellur\" as a; impl a.mark for a.token {} ",
    );
    f.reject("E_COHERENCE");
    f.write(
        "main.tellur",
        "import \"a.tellur\" as a; struct local {u32 value;} impl a.mark for vector<local> {}",
    );
    f.reject("E_COHERENCE");
    f.write(
        "main.tellur",
        "import \"a.tellur\" as a; struct local {u32 value;} impl a.mark for local {}",
    );
    f.check().unwrap();
    f.write(
        "main.tellur",
        "import \"a.tellur\" as a; trait local {} impl local for a.token {}",
    );
    f.check().unwrap();
}
#[test]
fn trait_methods_cannot_gain_foreign_private_field_access() {
    let f = Fixture::new();
    f.write("a.tellur", "pub struct hidden {string text;}");
    f.write("main.tellur",r#"import "a.tellur" as a; trait reader {control read(&self input) exits ready {string value};} impl reader for a.hidden {control read(&self input) exits ready {string value} {exit ready {clone(&(*input).text)};}}"#);
    f.reject("E_VISIBILITY");
    f.write("main.tellur",r#"import "a.tellur" as a; trait reader {control read(ref self input) exits ready {string value};} implement reader for a.hidden {control read(ref self input) exits ready {string value} {exit ready {clone(borrow input.text)};}}"#);
    f.reject("E_VISIBILITY");
}

#[test]
fn imported_trait_bounds_preserve_witness_identity_private_access_and_generic_interfaces() {
    let f = Fixture::new();
    f.write("reader.tellur","pub trait reader {type item:clone;control read(&self input) exits ready {item value};} pub control read_one<r:reader>(&r input) exits ready {reader<r>.item value} {forward reader<r>.read(input);}");
    f.write("source.tellur","import \"reader.tellur\" as io; pub struct source {string text;} pub control make() exits ready {source value} {exit ready {source {text:\"Ada\"}};} impl io.reader for source {type item=string;control read(&self input) exits ready {item value} {exit ready {clone(&(*input).text)};}}");
    f.write("main.tellur","import \"reader.tellur\" as io;import \"source.tellur\" as data;control main() exits auto {delta data.make() {ready {auto value}=>ready} [ready]:forward io.read_one<data.source>(&value);}");
    let p = f.check().unwrap();
    assert_eq!(
        p.run(&scenario()).unwrap().fields,
        vec![Value::String("Ada".into())]
    );
    assert!(!p.regions().any(|r| r.name.contains("_path_poly_")));
    f.write("reader.tellur","trait hidden {} pub control bad<t:hidden>(t value) exits ready {t value} {exit ready {value};}");
    f.write(
        "main.tellur",
        r#"import "reader.tellur" as io; control main() exits done {exit done;}"#,
    );
    f.reject("E_VISIBILITY");
}

#[test]
fn borrowed_trait_contracts_hide_private_field_layouts_across_modules() {
    let f = Fixture::new();
    f.write("reader.tellur","pub trait reader {type item:clone;control peek(&self input) exits ready {&item value from input};} pub control peek<r:reader>(&r input) exits ready {&reader<r>.item value from input} {forward reader<r>.peek(input);}");
    f.write("source.tellur","import \"reader.tellur\" as io;pub struct source {string text;} pub control make() exits ready {source value} {exit ready {source {text:\"Ada\"}};} impl io.reader for source {type item=string;control peek(&self owner) exits ready {&item value from owner} {exit ready {&(*owner).text};}}");
    let main = "import \"reader.tellur\" as io;import \"source.tellur\" as data;control main() exits auto {delta data.make() {ready {auto value}=>ready} [ready]:exit delta io.peek<data.source>(&value) {ready {auto text}=>done {clone(text)}};}";
    f.write("main.tellur", main);
    let p = f.check().unwrap();
    same(&p);
    assert_eq!(
        p.run(&scenario()).unwrap().fields,
        vec![Value::String("Ada".into())]
    );
    f.write(
        "main.tellur",
        &main.replace("clone(text)", "clone(&value.text)"),
    );
    f.reject("E_VISIBILITY");
    f.write("main.tellur", main);
    f.write("source.tellur","import \"reader.tellur\" as io;pub struct source {string text;} pub control make() exits ready {source value} {exit ready {source {text:\"Ada\"}};} impl io.reader for source {type item=string;control peek(&self owner) exits ready {&item value from owner} {string local=\"bad\";exit ready {&local};}}");
    let e = f.reject("E_LIFETIME");
    assert!(e.path.ends_with("source.tellur"));
}

#[test]
fn imported_grouped_results_preserve_precision_and_private_field_mappings() {
    let f = Fixture::new();
    let library = "pub struct pair['a,'b] {ref string secret <- 'a;pub ref string right <- 'b;} pub control make(ref string left,ref string right) exits ready {pair[left,right] value} {exit ready {pair {left,right}};}";
    let source = r#"import "pairs.tellur" as pairs;
    control main() exits done {mutable string a="a";string b="b";
    delta pairs.make(borrow a,borrow b) {ready {auto p}=>ready}
    [ready]: a="released";emit(p.right);exit done; }"#;
    f.write("pairs.tellur", library);
    f.write("main.tellur", source);
    same(&f.check().unwrap());
    f.write(
        "pairs.tellur",
        &library.replace("secret", "renamed_private_field"),
    );
    same(&f.check().unwrap());
    f.write(
        "main.tellur",
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
    f.write("library.tellur", library);
    f.write("main.tellur", "import \"library.tellur\" as lib; control relay(lib.view input) exits found {ref string value from input['a]} {forward lib.peek(borrow input);} control main() exits done {string a=\"outside\";delta lib.make(borrow a) {ready {auto p}=>ready} [ready]:delta relay(p) {found {auto value}=>found} [ready & found]:emit(value);exit done;}");
    same(&f.check().unwrap());
    f.write("library.tellur", &library.replace("hidden", "renamed"));
    same(&f.check().unwrap());
    f.write("main.tellur","import \"library.tellur\" as lib; control bad(lib.view input) exits found {ref string value from input['renamed]} {exit found {input.renamed};}");
    f.reject("E_LIFETIME");
}
#[test]
fn private_exclusive_fields_preserve_guards_across_public_selectors() {
    let f = Fixture::new();
    let library = "pub struct editor['a] {ref mutable string hidden <- 'a;} pub control make(ref mutable string owner) exits ready {editor[owner] value} {exit ready {editor {owner}};} pub control peek(ref editor input) exits found {ref string value from input['a]} {exit found {borrow deref input.hidden};}";
    f.write("library.tellur", library);
    let main = "import \"library.tellur\" as lib;control main() exits done {mutable string a=\"a\";delta lib.make(borrow mutable a) {ready {auto p}=>ready} [ready]:delta lib.peek(borrow p) {found {auto child}=>found} [ready & found]:emit(child);drop(p);exit done;}";
    f.write("main.tellur", main);
    same(&f.check().unwrap());
    f.write("library.tellur", &library.replace("hidden", "renamed"));
    same(&f.check().unwrap());
    f.write(
        "main.tellur",
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
    let source = r#"import "library.tellur" as lib;
 control main() exits done {delta lib.create() {ready {auto made}=>made}
 [made]:mutable lib.owner value=made;
 delta lib.split(borrow mutable value) {ready {auto a,auto b}=>split}
 [split]:deref a="A";deref b="B";emit(a);emit(b);drop(value);exit done;}"#;
    f.write("library.tellur", library);
    f.write("main.tellur", source);
    same(&f.check().unwrap());
    f.write("library.tellur", &library.replace("hidden_left", "renamed"));
    same(&f.check().unwrap());
    f.write(
        "main.tellur",
        &source.replace("emit(a);", "drop(value);emit(a);"),
    );
    f.reject("E_LOAN");
    f.write("main.tellur", source);
    f.write(
        "library.tellur",
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
    f.write("views.tellur","pub variant view {found {ref string text},empty} variant hidden {secret {ref string text}} pub control peek(ref string input) exits found {ref string text from input} {exit found {input};}");
    let main = r#"import "views.tellur" as lib;control main() exits done {mutable string owner="hello";lib.view saved=lib.view.found {borrow owner};
 delta saved {found {auto text}=>found,empty=>empty}[found]:emit(text);[empty]:emit("empty");
 [entry]:auto pending=capture lib.peek(borrow owner);exit delta pending {found {auto child}=>done {}};}"#;
    f.write("main.tellur", main);
    same(&f.check().unwrap());
    f.write(
        "main.tellur",
        &main.replace("delta saved", "owner=\"bad\";delta saved"),
    );
    f.reject("E_LOAN");
    f.write("main.tellur", main);
    f.write("views.tellur","variant view {found {ref string text},empty} pub control peek(ref string input) exits found {ref string text from input} {exit found {input};}");
    f.reject("E_VISIBILITY");
}

#[test]
fn nested_borrowed_storage_preserves_imported_layout_and_visibility() {
    let f = Fixture::new();
    let library = "pub variant view<t> {found {ref t text},empty} pub struct box<t> {pub option<view<t>> inner;}";
    f.write("views.tellur", library);
    let source = r#"import "views.tellur" as lib;control main() exits done {
        mutable string owner="hello";
        lib.box<string> saved=lib.box<string> {some {lib.view<string>.found {borrow owner}}};
        delta saved.inner {some {auto inner}=>some,none=>none}
        [some]:delta inner {found {auto text}=>found,empty=>empty}
        [found]:emit(text);owner="free";exit done;[none | empty]:exit done;
    }"#;
    f.write("main.tellur", source);
    same(&f.check().unwrap());
    f.write(
        "main.tellur",
        &source.replace("delta inner", "owner=\"bad\";delta inner"),
    );
    f.reject("E_LOAN");
    f.write("main.tellur", source);
    f.write("views.tellur", &library.replace("pub option", "option"));
    f.reject("E_VISIBILITY");
}

#[test]
fn imported_grouped_variants_preserve_public_groups_and_owner_retention() {
    let f = Fixture::new();
    f.write("views.tellur",r#"pub variant view<t>['source] {found {ref t value <- 'source},empty}
        pub control create(ref string input) exits ready {view<string>[input] value} {exit ready {view<string>.found {input}};}
        pub control unwrap(view<string> input) exits found {ref string value from input['source]},empty {exit delta input {found {auto text}=>found {text},empty=>empty {}};}"#);
    let source = r#"import "views.tellur" as views;
        control main() exits done {mutable string owner="hello";
        delta views.create(borrow owner) {ready {auto value}=>ready}
        [ready]:delta views.unwrap(value) {found {auto text}=>found,empty=>empty}
        [found]:emit(text);[entry]:owner="released";exit done;}"#;
    f.write("main.tellur", source);
    same(&f.check().unwrap());
    f.write(
        "main.tellur",
        &source.replace("emit(text)", "owner=\"too early\";emit(text)"),
    );
    f.reject("E_LOAN");
    f.write("main.tellur", source);
    f.write("views.tellur",r#"struct hidden {string text;} pub variant view['source] {found {ref hidden value <- 'source},empty}"#);
    f.write(
        "main.tellur",
        r#"import "views.tellur" as views; control main() exits done {exit done;}"#,
    );
    f.reject("E_VISIBILITY");
}

#[test]
fn imported_named_origin_schemas_preserve_generic_bindings_and_public_selectors() {
    let f = Fixture::new();
    f.write("library.tellur",r#"pub outcome reply<t>['a] {found {ref t text <- 'a},empty}
pub control produce<t>(ref t input) exits reply<t>[input] {exit found {input};}
pub control wrap<t>(reply<t> input) exits ready {option<reply<t>>[input['a]] value} {exit ready {some {input}};}"#);
    let main = r#"import "library.tellur" as lib;
control main() exits done {mutable string owner="hello";
lib.reply<string> saved=capture lib.produce<string>(borrow owner);
delta lib.wrap<string>(saved) {ready {auto pending}=>received}
[received]:delta pending {some {auto result}=>some,none=>none}
[some]:delta result {found {auto text}=>found,empty=>empty}
[found]:emit(text);[entry]:owner="released";exit done;}"#;
    f.write("main.tellur", main);
    same(&f.check().unwrap());
    let forwarded = format!(
        "{}\ncontrol forward_text(ref string input) exits lib.reply<string>[input] {{forward lib.produce<string>(input);}}",
        main.replace(
            "capture lib.produce<string>(borrow owner)",
            "capture forward_text(borrow owner)"
        )
    );
    f.write("main.tellur", &forwarded);
    same(&f.check().unwrap());
    f.write(
        "main.tellur",
        &main.replace(
            "[found]:emit(text)",
            "[found]:owner=\"too early\";emit(text)",
        ),
    );
    f.reject("E_LOAN");
    f.write(
        "library.tellur",
        "struct secret {string text;} pub outcome reply['a] {found {ref secret value <- 'a},empty}",
    );
    f.write(
        "main.tellur",
        "import \"library.tellur\" as lib; control main() exits done {exit done;}",
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
        "views.tellur",
        r#"pub struct secret['a] {ref string text <- 'a;}
      pub variant wrapped['a] {found {secret['a] value},empty}
      pub control produce(ref string input) exits ready {wrapped[input] value} {
        exit ready {wrapped.found {secret {input}}};
      }
      pub control inspect(ref wrapped input) exits found {ref string text from input['a]},empty {
        exit delta input {found {auto value}=>found {value.text},empty=>empty {}};
      }"#,
    );
    let main = r#"import "views.tellur" as views;
      control main() exits done {mutable string owner="hello";
        delta views.produce(borrow owner) {ready {auto saved}=>ready}
        [ready]:delta views.inspect(borrow saved) {found {auto text}=>found,empty=>empty}
        [found]:emit(text);[entry]:exit done;
      }"#;
    f.write("main.tellur", main);
    same(&f.check().unwrap());
    f.write(
        "main.tellur",
        &main.replace("emit(text)", "owner=\"bad\";emit(text)"),
    );
    f.reject("E_LOAN");
    f.write(
        "main.tellur",
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
    f.write(
        "library.tellur",
        include_str!("fixtures/public_contracts.tellur"),
    );
    f.write("main.tellur", "import \"library.tellur\" as lib;control main() exits done {delta lib.read_setting(true) {ready {auto value}=>ready}[ready]:exit done;}");
    let e = f.reject("E_ARMS");
    assert_eq!(e.path, f.root.join("main.tellur"));
    assert_eq!(e.diagnostic.notes.len(), 3, "{e}");
    for note in &e.diagnostic.notes {
        assert_eq!(note.path.as_ref(), Some(&f.root.join("library.tellur")));
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
