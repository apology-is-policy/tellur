use tellur::{
    compile,
    ir::Value,
    manifest::{Field, Manifest, TypeDef},
    runtime::{Scenario, ScriptCall},
};
fn manifest() -> Manifest {
    Manifest::from_json(include_str!("../playground/manifest.json")).unwrap()
}
fn run(source: &str, args: Vec<Value>) -> tellur::runtime::Execution {
    let p = compile(source, manifest()).unwrap();
    let s = Scenario {
        entry: "main".into(),
        arguments: args,
        calls: vec![],
    };
    let result = p.run(&s).unwrap();
    assert_eq!(result, p.expanded("main").unwrap().run(&s).unwrap());
    result
}
fn rejects(source: &str, code: &str) {
    let e = compile(source, manifest()).unwrap_err();
    assert_eq!(e.code, code, "{e}");
}
fn vector(ty: &str, fields: Vec<Value>) -> Value {
    Value::Construct {
        ty: format!("vector<{ty}>"),
        name: "vector".into(),
        fields,
    }
}
const PROGRAM: &str = r#"
control first(&vector<string> values) exits auto {
    [entry]: delta vector_get(values,0) {found {&string value}=>yes,out_of_bounds=>no}
    [yes]: exit found {value}; [no]: exit empty;
}
control main(u32 index) exits auto {
    [entry]: mutable vector<string> words=["one","two"];
        delta vector_push(&mutable words,"three") {pushed=>added}
    [added]: delta first(&words) {found {auto view}=>read,empty=>unexpected}
    [unexpected]: exit failed;
    [read]: string original=clone(view);
        delta vector_replace(&mutable words,index,"new") {replaced {string previous}=>changed,out_of_bounds {string rejected}=>outside}
    [changed]: emit(previous);
    [outside]: emit(rejected);
    [changed|outside]: delta vector_get_mut(&mutable words,0) {found {auto edit}=>editing,out_of_bounds=>bad}
    [bad]: exit failed;
    [editing]: *edit="edited";
        mutable map<string,u32> counts=map {};
        delta map_insert(&mutable counts,"b",2) {inserted=>b_added,replaced {u32 old_b}=>b_replaced}
    [b_added|b_replaced]: delta map_insert(&mutable counts,"a",1) {inserted=>a_added,replaced {u32 old_a}=>a_replaced}
    [a_added|a_replaced]: delta map_insert(&mutable counts,"b",20) {inserted=>new_b,replaced {u32 old}=>old_b}
    [new_b|old_b]: delta map_get_mut(&mutable counts,&"a") {found {auto number}=>number_found,missing=>number_missing}
    [number_missing]: exit failed;
    [number_found]: *number=10;
        array<u32,3> fixed=[4,5,6];
        u32 length=vector_length(&words);
        exit done {original,words,counts,fixed,length,array_length(&fixed),map_length(&counts)};
}
"#;
#[test]
fn collection_lookups_mutation_and_lengths() {
    // Length reads must precede moving the corresponding map into the result.
    let source=PROGRAM.replace("exit done {original,words,counts,fixed,length,array_length(&fixed),map_length(&counts)};", "u32 map_size=map_length(&counts); exit done {original,words,counts,fixed,length,array_length(&fixed),map_size};");
    for i in [0, 1, 5] {
        let r = run(&source, vec![Value::U32(i)]);
        assert_eq!(r.outcome, "done");
        assert_eq!(r.fields[0], Value::String("one".into()));
        assert_eq!(
            r.fields[4..],
            vec![Value::U32(3), Value::U32(3), Value::U32(2)]
        );
        let Value::Construct { fields, .. } = &r.fields[2] else {
            panic!()
        };
        assert_eq!(
            fields,
            &vec![
                Value::String("a".into()),
                Value::U32(10),
                Value::String("b".into()),
                Value::U32(20)
            ]
        );
    }
    rejects(PROGRAM, "E_MOVED");
}
#[test]
fn arrays_copy_only_when_elements_copy_vectors_always_move() {
    let r = run(
        "control main() exits auto { [entry]: array<u32,2> a=[1,2]; auto b=a; vector<string> c=[\"hi\"]; auto d=clone(&c); exit done {a,b,c,d}; }",
        vec![],
    );
    assert_eq!(r.fields[0], r.fields[1]);
    assert_eq!(r.fields[2], r.fields[3]);
    rejects(
        "control main() exits auto { [entry]: vector<u32> a=[1]; auto b=a; exit done {a,b}; }",
        "E_MOVED",
    );
    rejects(
        "control main() exits auto { [entry]: array<string,1> a=[\"hi\"]; auto b=a; exit done {a,b}; }",
        "E_MOVED",
    );
}
#[test]
fn literal_context_arity_and_types_are_checked() {
    rejects(
        "control main() exits auto { [entry]: array<u32,2> a=[1]; exit done {a}; }",
        "E_ARITY",
    );
    rejects(
        "control main() exits auto { [entry]: vector<u32> a=[\"wrong\"]; exit done {a}; }",
        "E_TYPE",
    );
    rejects(
        "control main() exits auto { [entry]: auto a=[]; exit done {a}; }",
        "E_INFER",
    );
    rejects(
        "control main(map<handle,u32> m) exits auto { [entry]: exit done {m}; }",
        "E_MANIFEST",
    );
    rejects(
        "control main(vector<&string> m) exits auto { [entry]: exit done {m}; }",
        "E_MANIFEST",
    );
    let r = run(
        "control main() exits auto { [entry]: vector<array<u32,2>> nested=[[1,2],[3,4]]; exit done {nested}; }",
        vec![],
    );
    assert_eq!(r.fields[0].ty(), "vector<array<U32,2>>");
}
#[test]
fn borrowed_element_keeps_collection_frozen_through_helper_calls() {
    let source = r#"control at(&vector<string> v,u32 i) exits auto {
        [entry]: delta vector_get(v,i) {found {auto r}=>yes,out_of_bounds=>no}
        [yes]: exit found {r}; [no]: exit missing;
    }
    control main() exits done { [entry]: mutable vector<string> v=["hello"];
        delta at(&v,0) {found {auto r}=>yes,missing=>no}
        [yes]: delta vector_push(&mutable v,"bad") {pushed=>pushed}
        [pushed]: emit(r); exit done; [no]: exit done;
    }"#;
    rejects(source, "E_LOAN");
    rejects(
        &source
            .replace(
                "delta vector_push(&mutable v,\"bad\") {pushed=>pushed}",
                "auto moved=v;",
            )
            .replace("[pushed]:", "[yes]:"),
        "E_LOAN",
    );
    let allowed = source
        .replace(
            "delta vector_push(&mutable v,\"bad\") {pushed=>pushed}",
            "emit(r); delta vector_push(&mutable v,\"good\") {pushed=>pushed}",
        )
        .replace("[pushed]: emit(r);", "[pushed]:");
    run(&allowed, vec![]);
}
#[test]
fn borrowed_lookup_failure_leaves_owner_available() {
    let source = "control main() exits auto { [entry]: vector<string> v=[]; delta vector_get(&v,0) {found {auto r}=>yes,out_of_bounds=>no} [yes]: exit done {clone(r)}; [no]: exit empty {v}; }";
    assert_eq!(run(source, vec![]).fields, vec![vector("String", vec![])]);
}
#[test]
fn collection_aliasing_and_shared_mutation_rejected() {
    rejects(
        "control main() exits done { [entry]: mutable vector<string> v=[\"x\"]; delta vector_push(&mutable v,clone(&v)) {pushed=>p} [p]: exit done; }",
        "E_ALIAS",
    );
    rejects(
        "control main() exits done { [entry]: vector<string> v=[\"x\"]; delta vector_get(&v,0) {found {auto r}=>yes,out_of_bounds=>no} [yes]: *r=\"bad\"; exit done; [no]: exit done; }",
        "E_MUTABILITY",
    );
    rejects(
        "control main() exits done { [entry]: mutable vector<string> v=[\"x\"]; delta vector_get_mut(&mutable v,0) {found {auto r}=>yes,out_of_bounds=>no} [yes]: auto moved=*r; exit done; [no]: exit done; }",
        "E_BORROW_MOVE",
    );
}
#[test]
fn arrays_convert_and_vectors_remove_and_pop() {
    let source = r#"control main() exits auto { [entry]: array<string,2> a=["a","b"];
        delta array_into_vector(a) {converted {vector<string> v}=>converted}
        [converted]: mutable vector<string> work=v;
        delta vector_remove(&mutable work,5) {item {auto unexpected}=>bad,out_of_bounds=>outside}
        [bad]: exit failed;
        [outside]: delta vector_pop(&mutable work) {item {string b}=>popped,empty=>bad_empty}
        [bad_empty]: exit failed;
        [popped]: exit done {b,work}; }"#;
    assert_eq!(
        run(source, vec![]).fields,
        vec![
            Value::String("b".into()),
            vector("String", vec![Value::String("a".into())])
        ]
    );
}
#[test]
fn resource_container_must_be_drained_and_removed_values_completed() {
    let source = r#"control main(handle h) exits done { [entry]: vector<handle> input=[h];
        loop drain(vector<handle> pending=input) {
            [entry]: delta vector_take(pending) {item {handle item,vector<handle> rest}=>next,empty=>end}
            [next]: delta close(item) {closed=>closed,failed {auto error}=>failed}
            [closed|failed]: again drain(rest);
            [end]: exit done;
        }
    }"#;
    let p = compile(source, manifest()).unwrap();
    let s = Scenario {
        entry: "main".into(),
        arguments: vec![Value::Opaque {
            ty: "handle".into(),
            token: "file".into(),
        }],
        calls: vec![ScriptCall {
            operation: "close".into(),
            outcome: "closed".into(),
            fields: vec![],
        }],
    };
    let r = p.run(&s).unwrap();
    assert_eq!(r, p.expanded("main").unwrap().run(&s).unwrap());
    assert_eq!(r.events.len(), 1);
    rejects(
        "control main(handle h) exits done { [entry]: vector<handle> values=[h]; exit done; }",
        "E_RESOURCE",
    );
    rejects(
        "control main(handle h) exits done { [entry]: vector<handle> values=[h]; drop(values); exit done; }",
        "E_RESOURCE",
    );
    rejects(
        "control main(handle h) exits done { [entry]: vector<handle> values=[h]; auto copy=clone(&values); exit done; }",
        "E_CLONE",
    );
    rejects(
        "control main(handle h) exits done { [entry]: mutable vector<handle> values=[h]; delta vector_pop(&mutable values) {item {auto h2}=>yes,empty=>no} [yes|no]: exit done; }",
        "E_RESOURCE",
    );
}
#[test]
fn replacing_resource_returns_old_or_rejected_value() {
    let source = r#"control main(handle first,handle second,u32 index) exits auto {
        [entry]: mutable array<handle,1> values=[first];
        delta array_replace(&mutable values,index,second) {replaced {handle previous}=>yes,out_of_bounds {handle rejected}=>no}
        [yes]: exit replaced {values,previous}; [no]: exit rejected {values,rejected};
    }"#;
    let a = Value::Opaque {
        ty: "handle".into(),
        token: "first".into(),
    };
    let b = Value::Opaque {
        ty: "handle".into(),
        token: "second".into(),
    };
    assert_eq!(
        run(source, vec![a.clone(), b.clone(), Value::U32(0)]).fields[1],
        a
    );
    assert_eq!(run(source, vec![a, b.clone(), Value::U32(3)]).fields[1], b);
}
#[test]
fn maps_drain_in_key_order_and_remove_explicitly() {
    let source = r#"control main() exits auto { [entry]: mutable map<string,u32> m=map {};
        delta map_insert(&mutable m,"b",2) {inserted=>b,replaced {auto ignored}=>bx}
        [b|bx]: delta map_insert(&mutable m,"a",1) {inserted=>a,replaced {auto ignored2}=>ax}
        [a|ax]: delta map_remove(&mutable m,&"absent") {removed {auto unexpected}=>bad,missing=>missing}
        [bad]: exit failed;
        [missing]: delta map_take(m) {item {string key,u32 value,map<string,u32> rest}=>item,empty=>empty}
        [empty]: exit failed; [item]: exit done {key,value,rest}; }"#;
    let r = run(source, vec![]);
    assert_eq!(r.fields[0], Value::String("a".into()));
    assert_eq!(r.fields[1], Value::U32(1));
}
#[test]
fn recursive_collection_layouts_and_builtin_shadowing_are_rejected() {
    let mut m = manifest();
    m.types.push(TypeDef::Record {
        name: "recursive".into(),
        fields: vec![Field {
            name: "nested".into(),
            ty: "vector<recursive>".into(),
        }],
    });
    assert!(m.validate().unwrap_err().message.contains("recursive"));
    rejects(
        "control vector_get() exits done { [entry]: exit done; }",
        "E_NAME",
    );
}
#[test]
fn nested_literal_moves_cannot_hide_argument_aliases() {
    rejects(
        "control consume(vector<string> v,string s) exits done { [entry]: exit done; } control main() exits done { [entry]: string s=\"x\"; delta consume([s],s) {done=>d} [d]: exit done; }",
        "E_ALIAS",
    );
}
#[test]
fn driver_collection_payloads_are_validated() {
    let p = compile(
        "control main(array<u32,2> a) exits auto { [entry]: exit done {a}; }",
        manifest(),
    )
    .unwrap();
    let malformed = Value::Construct {
        ty: "array<U32,2>".into(),
        name: "array".into(),
        fields: vec![Value::U32(1)],
    };
    assert!(
        p.run(&Scenario {
            entry: "main".into(),
            arguments: vec![malformed],
            calls: vec![]
        })
        .is_err()
    );
    let p = compile(
        "control main(map<string,u32> a) exits auto { [entry]: exit done {a}; }",
        manifest(),
    )
    .unwrap();
    for keys in [["b", "a"], ["a", "a"]] {
        let malformed = Value::Construct {
            ty: "map<String,U32>".into(),
            name: "map".into(),
            fields: keys
                .into_iter()
                .flat_map(|k| [Value::String(k.into()), Value::U32(1)])
                .collect(),
        };
        assert!(
            p.run(&Scenario {
                entry: "main".into(),
                arguments: vec![malformed],
                calls: vec![]
            })
            .is_err()
        );
    }
}
#[test]
fn native_collections_match_interpreter_and_expansion_at_o0_o2() {
    let source=PROGRAM.replace("exit done {original,words,counts,fixed,length,array_length(&fixed),map_length(&counts)};", "u32 map_size=map_length(&counts); exit done {original,words,counts,fixed,length,array_length(&fixed),map_size};");
    let p = compile(&source, manifest()).unwrap();
    let root = std::env::temp_dir().join(format!("path-collections-native-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    for expanded in [false, true] {
        let program = if expanded {
            p.expanded("main").unwrap()
        } else {
            p.clone()
        };
        for opt in [0, 2] {
            let binary = root.join(format!("collections-{expanded}-{opt}"));
            program.build_native("main", &binary, opt).unwrap();
            for index in [0, 1, 5] {
                let expected = p
                    .run(&Scenario {
                        entry: "main".into(),
                        arguments: vec![Value::U32(index)],
                        calls: vec![],
                    })
                    .unwrap();
                let actual = std::process::Command::new(&binary)
                    .args(["--json", "--", &index.to_string()])
                    .output()
                    .unwrap();
                assert!(
                    actual.status.success(),
                    "{}",
                    String::from_utf8_lossy(&actual.stderr)
                );
                assert_eq!(
                    serde_json::from_slice::<serde_json::Value>(&actual.stdout).unwrap(),
                    serde_json::to_value(expected).unwrap()
                );
            }
        }
    }
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn collection_edge_operations_preserve_values_and_empty_outcomes() {
    let source = r#"control main() exits auto { [entry]:
        mutable array<u32,1> a=[7];
        delta array_get_mut(&mutable a,0) {found {auto p}=>yes,out_of_bounds=>no}
        [no]: exit failed;
        [yes]: *p=8; delta array_get(&a,9) {found {auto q}=>bad,out_of_bounds=>outside}
        [bad]: exit failed;
        [outside]: mutable vector<string> v=["a","b"];
        delta vector_remove(&mutable v,0) {item {string removed}=>removed,out_of_bounds=>bad_remove}
        [bad_remove]: exit failed;
        [removed]: delta vector_pop(&mutable v) {item {string popped}=>popped,empty=>bad_pop}
        [bad_pop]: exit failed;
        [popped]: delta vector_pop(&mutable v) {item {auto bad_item}=>bad_empty,empty=>empty}
        [bad_empty]: exit failed;
        [empty]: mutable map<u32,string> m=map {};
        delta map_insert(&mutable m,7,"seven") {inserted=>inserted,replaced {auto old}=>replaced}
        [inserted|replaced]: delta map_get(&m,&8) {found {auto unexpected}=>bad_get,missing=>missing}
        [bad_get]: exit failed;
        [missing]: delta map_remove(&mutable m,&7) {removed {string seven}=>seven,missing=>bad_missing}
        [bad_missing]: exit failed;
        [seven]: delta map_take(m) {item {auto k,auto val,auto rest}=>bad_take,empty=>end}
        [bad_take]: exit failed;
        [end]: exit done {a,removed,popped,seven};
    }"#;
    let result = run(source, vec![]);
    assert_eq!(result.outcome, "done");
    assert_eq!(
        result.fields[1..],
        vec![
            Value::String("a".into()),
            Value::String("b".into()),
            Value::String("seven".into())
        ]
    );
}
#[test]
fn resource_map_insertion_replacement_and_draining_transfer_every_handle() {
    let source = r#"control main(handle first,handle second) exits auto { [entry]:
        mutable map<string,handle> m=map {};
        delta map_insert(&mutable m,"key",first) {inserted=>inserted,replaced {handle old}=>replaced}
        [replaced]: exit unexpected {m,old,second};
        [inserted]: delta map_insert(&mutable m,"key",second) {inserted=>new,replaced {handle previous}=>old}
        [new]: exit unexpected_new {m};
        [old]: delta map_take(m) {item {string key,handle current,map<string,handle> rest}=>item,empty=>empty}
        [empty]: exit unexpected_empty {previous};
        [item]: delta map_take(rest) {item {auto key2,auto value2,auto rest2}=>extra,empty=>end}
        [extra]: exit extra {previous,current,value2,rest2};
        [end]: exit done {previous,current};
    }"#;
    let args = ["first", "second"]
        .map(|token| Value::Opaque {
            ty: "handle".into(),
            token: token.into(),
        })
        .to_vec();
    assert_eq!(run(source, args.clone()).fields, args);
    rejects(
        &source.replace("exit done {previous,current};", "exit done {current};"),
        "E_RESOURCE",
    );
}
#[test]
fn nested_element_borrow_and_map_growth_invalidation_are_checked() {
    let source = r#"control main() exits auto { [entry]: mutable vector<vector<string>> rows=[["a"]];
        delta vector_get_mut(&mutable rows,0) {found {auto row}=>row,out_of_bounds=>no}
        [no]: exit failed;
        [row]: delta vector_get_mut(row,0) {found {auto cell}=>cell,out_of_bounds=>empty}
        [empty]: exit failed;
        [cell]: *cell="changed"; exit done {rows};
    }"#;
    assert_eq!(run(source, vec![]).outcome, "done");
    rejects(
        &source.replace(
            "*cell=\"changed\";",
            "delta vector_push(row,\"new\") {pushed=>pushed} [pushed]: *cell=\"changed\";",
        ),
        "E_LOAN",
    );
    let source = r#"control main() exits done { [entry]: mutable map<string,u32> m=map {};
        delta map_get(&m,&"key") {found {auto r}=>yes,missing=>no}
        [yes]: delta map_insert(&mutable m,"other",2) {inserted=>i,replaced {auto old}=>r}
        [i|r]: emit(u32_text(*r)); exit done; [no]: exit done;
    }"#;
    rejects(source, "E_LOAN");
}
#[test]
fn playground_collections_have_expected_output() {
    let r = run(include_str!("../playground/collections.tellur"), vec![]);
    assert_eq!(
        r.events,
        ["hello", "3", "20", "3", "hello", "path", "collections"]
            .into_iter()
            .map(|s| tellur::runtime::Event::Emit { message: s.into() })
            .collect::<Vec<_>>()
    );
}

#[test]
fn array_lengths_are_canonical_and_zero_length_arrays_work() {
    let r = run(
        "control main() exits auto { [entry]: array<u32,02> a=[1,2]; array<u32,2> b=a; array<string,0> empty=[]; delta array_into_vector(empty) {converted {auto v}=>ready} [ready]: exit done {a,b,v}; }",
        vec![],
    );
    assert_eq!(r.fields[0], r.fields[1]);
    assert_eq!(r.fields[2], vector("String", vec![]));
    assert_eq!(r.fields[0].ty(), "array<U32,2>");
}
