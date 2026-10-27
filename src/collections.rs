use crate::{
    collection_support::Collection,
    ir::BorrowSource,
    manifest::{Field, Mode, Outcome, Parameter},
};
use std::collections::BTreeMap;
pub const ELEMENT: usize = usize::MAX;
pub fn is_operation(name: &str) -> bool {
    [
        "array_get",
        "array_get_mut",
        "array_replace",
        "array_into_vector",
        "vector_get",
        "vector_get_mut",
        "vector_push",
        "vector_pop",
        "vector_remove",
        "vector_replace",
        "vector_take",
        "map_get",
        "map_get_mut",
        "map_insert",
        "map_remove",
        "map_take",
    ]
    .contains(&name)
}
pub fn is_length(name: &str) -> bool {
    ["array_length", "vector_length", "map_length"].contains(&name)
}
pub struct Contract {
    pub parameters: Vec<Parameter>,
    pub outcomes: Vec<Outcome>,
    pub borrow_returns: BTreeMap<(String, usize), Vec<BorrowSource>>,
}
pub fn contract(name: &str, ty: &str) -> Option<Contract> {
    if !is_operation(name) {
        return None;
    }
    let c = Collection::parse(ty)?;
    let op = name.strip_prefix(&format!("{}_", c.name()))?;
    let t = c.element();
    let consuming = matches!(op, "take" | "into_vector");
    let mutable = !consuming && op != "get";
    let p = |name: &str, ty: String, mode| Parameter {
        name: name.into(),
        ty,
        mode,
    };
    let mut parameters = vec![p(
        "values",
        if consuming {
            ty.into()
        } else {
            format!("&{}{ty}", if mutable { "mutable " } else { "" })
        },
        if consuming || mutable {
            Mode::Consume
        } else {
            Mode::Borrow
        },
    )];
    let f = |ty: &str| Field {
        name: "value".into(),
        ty: ty.into(),
    };
    let o = |name: &str, types: Vec<String>| Outcome {
        name: name.into(),
        fields: types
            .iter()
            .enumerate()
            .map(|(i, t)| Field {
                name: format!("value_{i}"),
                ..f(t)
            })
            .collect(),
    };
    let mut borrow_returns = BTreeMap::new();
    let outcomes = match op {
        "get" | "get_mut" => {
            parameters.push(match &c {
                Collection::Map(k, _) => p("key", k.clone(), Mode::Borrow),
                _ => p("index", "U32".into(), Mode::Value),
            });
            borrow_returns.insert(
                ("found".into(), 0),
                vec![BorrowSource {
                    guard: false,
                    contained: None,
                    parameter: 0,
                    fields: vec![ELEMENT],
                    opaque: false,
                }],
            );
            vec![
                o(
                    "found",
                    vec![format!("&{}{t}", if mutable { "mutable " } else { "" })],
                ),
                o(
                    if matches!(c, Collection::Map(..)) {
                        "missing"
                    } else {
                        "out_of_bounds"
                    },
                    vec![],
                ),
            ]
        }
        "replace" => {
            parameters.push(p("index", "U32".into(), Mode::Value));
            parameters.push(p("value", t.into(), Mode::Value));
            vec![
                o("replaced", vec![t.into()]),
                o("out_of_bounds", vec![t.into()]),
            ]
        }
        "push" => {
            parameters.push(p("value", t.into(), Mode::Value));
            vec![o("pushed", vec![])]
        }
        "pop" => vec![o("item", vec![t.into()]), o("empty", vec![])],
        "remove" => {
            parameters.push(match &c {
                Collection::Map(k, _) => p("key", k.clone(), Mode::Borrow),
                _ => p("index", "U32".into(), Mode::Value),
            });
            vec![
                o(
                    if matches!(c, Collection::Map(..)) {
                        "removed"
                    } else {
                        "item"
                    },
                    vec![t.into()],
                ),
                o(
                    if matches!(c, Collection::Map(..)) {
                        "missing"
                    } else {
                        "out_of_bounds"
                    },
                    vec![],
                ),
            ]
        }
        "insert" => {
            let Collection::Map(k, _) = &c else {
                return None;
            };
            parameters.push(p("key", k.clone(), Mode::Value));
            parameters.push(p("value", t.into(), Mode::Value));
            vec![o("inserted", vec![]), o("replaced", vec![t.into()])]
        }
        "take" => {
            let mut fields = if let Collection::Map(k, _) = &c {
                vec![k.clone(), t.into()]
            } else {
                vec![t.into()]
            };
            fields.push(ty.into());
            vec![o("item", fields), o("empty", vec![])]
        }
        "into_vector" => vec![o("converted", vec![format!("vector<{t}>")])],
        _ => return None,
    };
    Some(Contract {
        parameters,
        outcomes,
        borrow_returns,
    })
}
