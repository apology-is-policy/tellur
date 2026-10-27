//! Structured public origin schemas. Origins do not change nominal layouts or
//! machine-code specialization; the checker instantiates them at each boundary.
use crate::{
    Diagnostic, Result, Span, manifest,
    syntax::{self, OriginArgument},
};
use std::collections::{BTreeMap, BTreeSet};
#[derive(Clone, Debug)]
pub(crate) struct Leaf {
    pub through: Vec<crate::ir::ReferenceStep>,
    pub fields: Vec<usize>,
    pub sum_leaf: Option<crate::ir::SumLeaf>,
    pub group: Option<usize>,
    pub ty: String,
}
#[derive(Clone, Debug)]
pub(crate) struct Schema {
    pub groups: Vec<String>,
    pub leaves: Vec<Leaf>,
}
#[derive(Default)]
pub(crate) struct Schemas(BTreeMap<String, Schema>);
impl Schemas {
    pub fn get(&self, ty: &str) -> Option<Schema> {
        self.lookup(ty, 0)
    }
    fn lookup(&self, ty: &str, depth: usize) -> Option<Schema> {
        if depth >= 64 {
            return None;
        }
        if let Some(inner) = manifest::option_inner(ty) {
            let nested = if manifest::reference_inner(inner).is_some() {
                reference_schema(
                    inner,
                    self.lookup(manifest::reference_inner(inner)?.1, depth + 1)
                        .unwrap_or_else(empty_schema),
                )
            } else {
                self.lookup(inner, depth + 1)?
            };
            return Some(option_schema(nested));
        }
        self.0.get(ty).cloned()
    }
}
fn empty_schema() -> Schema {
    Schema {
        groups: vec![],
        leaves: vec![],
    }
}
fn reference_schema(ty: &str, mut nested: Schema) -> Schema {
    let mut schema = Schema {
        groups: vec!["source".into()],
        leaves: vec![Leaf {
            through: vec![],
            fields: vec![],
            sum_leaf: None,
            group: Some(0),
            ty: ty.into(),
        }],
    };
    schema
        .groups
        .extend(nested.groups.iter().map(|g| format!("referent_{g}")));
    for leaf in &mut nested.leaves {
        leaf.through.insert(
            0,
            crate::ir::ReferenceStep {
                fields: vec![],
                sum_leaf: None,
            },
        );
        leaf.group = leaf.group.map(|g| g + 1);
    }
    schema.leaves.extend(nested.leaves);
    schema
}
fn wrap_leaf(leaf: &mut Leaf, index: usize, outcome: Option<&str>) {
    let (fields, sum_leaf) = if let Some(step) = leaf.through.first_mut() {
        (&mut step.fields, &mut step.sum_leaf)
    } else {
        (&mut leaf.fields, &mut leaf.sum_leaf)
    };
    if let Some(outcome) = outcome {
        *sum_leaf = Some(crate::ir::SumLeaf {
            outcome: outcome.into(),
            payload: index,
            fields: std::mem::take(fields),
            nested: sum_leaf.take().map(Box::new),
        });
    } else {
        fields.insert(0, index);
    }
}
fn option_schema(mut nested: Schema) -> Schema {
    for leaf in &mut nested.leaves {
        wrap_leaf(leaf, 0, Some("Some"));
    }
    nested
}
fn error(at: Span, text: impl Into<String>) -> Diagnostic {
    Diagnostic::new("E_ORIGIN", at, text)
}
pub(crate) fn schemas(unit: &syntax::Unit) -> Result<Schemas> {
    let mut declarations = unit.data.clone();
    declarations.extend(unit.contracts.iter().map(|c| syntax::DataDeclaration {
        origin_parameters: c.origin_parameters.clone(),
        type_parameters: c.type_parameters.clone(),
        public: c.public,
        public_fields: Default::default(),
        span: c.span,
        name_span: c.name_span,
        end: c.end,
        name: c.name.clone(),
        body: syntax::DataBody::Variant(c.outcomes.clone()),
    }));
    let data = declarations.as_slice();
    fn visit(
        name: &str,
        data: &[syntax::DataDeclaration],
        done: &mut BTreeMap<String, Schema>,
        active: &mut BTreeSet<String>,
    ) -> Result<Schema> {
        if let Some(s) = done.get(name) {
            return Ok(s.clone());
        }
        if let Some(inner) = manifest::option_inner(name) {
            if active.len() >= 64 || !active.insert(name.into()) {
                return Err(error(
                    Span::default(),
                    "cyclic or excessively deep option origin schema",
                ));
            }
            let nested = if manifest::reference_inner(inner).is_some() {
                reference_schema(
                    inner,
                    visit(
                        manifest::reference_inner(inner).unwrap().1,
                        data,
                        done,
                        active,
                    )?,
                )
            } else {
                visit(inner, data, done, active)?
            };
            active.remove(name);
            return Ok(option_schema(nested));
        }
        let base = crate::collection_support::generic(name).map_or(name, |(b, _)| b);
        let Some(d) = data
            .iter()
            .find(|d| d.name == name)
            .or_else(|| data.iter().find(|d| d.name == base))
        else {
            return Ok(Schema {
                groups: vec![],
                leaves: vec![],
            });
        };
        let fields: Vec<_> = match &d.body {
            syntax::DataBody::Struct(fields) => fields
                .iter()
                .enumerate()
                .map(|(i, f)| (None, i, f))
                .collect(),
            syntax::DataBody::Variant(outcomes) => outcomes
                .iter()
                .flat_map(|o| {
                    o.fields
                        .iter()
                        .enumerate()
                        .map(move |(i, f)| (Some(o.name.as_str()), i, f))
                })
                .collect(),
        };
        if active.len() >= 64 || !active.insert(d.name.clone()) {
            return Err(error(d.span, "cyclic or excessively deep origin schema"));
        }
        if d.origin_parameters.len() > 256 {
            return Err(error(d.span, "origin schema exceeds 256 groups"));
        }
        let mut groups = BTreeSet::new();
        for n in &d.origin_parameters {
            if !groups.insert(n) {
                return Err(error(d.span, format!("duplicate origin group `'{n}`")));
            }
        }
        let mut schema = Schema {
            groups: d.origin_parameters.clone(),
            leaves: vec![],
        };
        for (outcome, index, field) in fields {
            let nested = if let Some((_, inner)) = manifest::reference_inner(&field.ty) {
                reference_schema(&field.ty, visit(inner, data, done, active)?)
            } else {
                visit(&field.ty, data, done, active)?
            };
            if manifest::reference_inner(&field.ty).is_some()
                && nested.groups.len() > 1
                && !field.origins.is_empty()
                && field.referent_origins.is_empty()
            {
                return Err(error(
                    d.span,
                    "carrier reference fields require both referent bracket mappings and a storage arrow origin",
                ));
            }
            if schema.groups.is_empty()
                && (!field.origins.is_empty() || !field.referent_origins.is_empty())
            {
                return Err(error(
                    d.span,
                    "field origins require declared type origin parameters",
                ));
            }
            let mapped = field
                .origins
                .iter()
                .chain(&field.referent_origins)
                .map(|a| {
                    let OriginArgument::Group(n) = a else {
                        return Err(error(d.span, "field origins must name apostrophe groups"));
                    };
                    schema
                        .groups
                        .iter()
                        .position(|g| g == n)
                        .ok_or_else(|| error(d.span, format!("unknown origin group `'{n}`")))
                })
                .collect::<Result<Vec<_>>>()?;
            if !mapped.is_empty() && nested.leaves.is_empty() {
                return Err(error(
                    d.span,
                    format!("field `{}` has no reference origins to map", field.name),
                ));
            }
            if !schema.groups.is_empty()
                && !nested.leaves.is_empty()
                && (nested.groups.is_empty() || mapped.len() != nested.groups.len())
            {
                return Err(error(
                    d.span,
                    format!(
                        "field `{}` requires a complete mapping of its {} origin groups",
                        field.name,
                        nested.groups.len()
                    ),
                ));
            }
            if schema.leaves.len() + nested.leaves.len() > 4096 {
                return Err(error(d.span, "origin schema exceeds 4096 reference leaves"));
            }
            for mut leaf in nested.leaves {
                wrap_leaf(&mut leaf, index, outcome);
                leaf.group = leaf.group.and_then(|g| mapped.get(g).copied());
                schema.leaves.push(leaf);
            }
        }

        for (index, name) in schema.groups.iter().enumerate() {
            if !schema.leaves.iter().any(|l| l.group == Some(index)) {
                return Err(error(
                    d.span,
                    format!("origin group `'{name}` is not used by a reference field"),
                ));
            }
        }
        active.remove(&d.name);
        done.insert(name.into(), schema.clone());
        Ok(schema)
    }
    // Ordinary recursive-layout errors retain their existing diagnostics.
    if !data.iter().any(|d| {
        !d.origin_parameters.is_empty()
            || match &d.body {
                syntax::DataBody::Struct(fs) => fs.iter().any(|f| !f.origins.is_empty()),
                syntax::DataBody::Variant(os) => os
                    .iter()
                    .any(|o| o.fields.iter().any(|f| !f.origins.is_empty())),
            }
    }) {
        return Ok(Schemas::default());
    }
    let mut done = BTreeMap::new();
    for d in data {
        visit(&d.name, data, &mut done, &mut BTreeSet::new())?;
    }
    Ok(Schemas(done))
}
pub(crate) fn result_type(
    ty: &str,
    schemas: &Schemas,
    m: &manifest::Manifest,
    at: Span,
) -> Result<()> {
    if let Some((_, inner)) = manifest::reference_inner(ty) {
        if manifest::reference_inner(inner).is_some() {
            return m.check_type(inner);
        }
        return borrowed_input_type(inner, schemas, m, at);
    }
    if m.contains_references(ty) {
        let Some(schema) = schemas.get(ty).filter(|s| !s.groups.is_empty()) else {
            return Err(error(
                at,
                "returned reference-bearing storage requires a complete origin schema",
            ));
        };
        if schema.leaves.iter().any(|l| l.group.is_none()) {
            return Err(error(at, "incomplete record origin schema"));
        }
        check_boundary_storage(ty, schemas, m, at)
    } else {
        m.check_type(ty)
    }
}
fn check_boundary_storage(
    ty: &str,
    schemas: &Schemas,
    m: &manifest::Manifest,
    at: Span,
) -> Result<()> {
    if let Some((_, inner)) = manifest::reference_inner(ty) {
        return borrowed_input_type(inner, schemas, m, at);
    }
    if !m.contains_references(ty) {
        return m.check_type(ty);
    }
    let schema = schemas
        .get(ty)
        .filter(|s| !s.groups.is_empty())
        .ok_or_else(|| {
            error(
                at,
                "public borrowed storage requires a complete declared origin schema",
            )
        })?;
    if schema.leaves.iter().any(|l| l.group.is_none()) {
        return Err(error(at, "incomplete origin schema"));
    }
    if let Some(manifest::TypeDef::Record { fields, .. }) = m.type_def(ty) {
        for f in fields {
            check_boundary_storage(&f.ty, schemas, m, at)?;
        }
    } else if let Some(outcomes) = m.outcomes(ty) {
        for o in outcomes {
            for f in o.fields {
                check_boundary_storage(&f.ty, schemas, m, at)?;
            }
        }
    } else {
        return m.check_type(ty);
    }
    Ok(())
}
fn leaf_exclusive(schema: &Schema, leaf: &Leaf) -> bool {
    manifest::reference_inner(&leaf.ty).is_some_and(|(m, _)| m)
        && leaf.through.iter().enumerate().all(|(index, step)| {
            schema
                .leaves
                .iter()
                .find(|l| {
                    l.through == leaf.through[..index]
                        && l.fields == step.fields
                        && l.sum_leaf == step.sum_leaf
                })
                .is_some_and(|l| manifest::reference_inner(&l.ty).is_some_and(|(m, _)| m))
        })
}
pub(crate) fn declared(
    r: &syntax::Region,
    schemas: &Schemas,
) -> Result<Vec<crate::ir::StoredReturn>> {
    let mut returns = vec![];
    for o in &r.exits {
        for (payload, f) in o.fields.iter().enumerate() {
            if f.origins.is_empty() {
                continue;
            }
            let Some(schema) = schemas
                .get(manifest::reference_inner(&f.ty).map_or(f.ty.as_str(), |(_, t)| t))
                .filter(|s| !s.groups.is_empty())
            else {
                return Err(error(
                    r.span,
                    "bracket origins require a type with declared origin groups; direct references use `from input`",
                ));
            };
            if f.origins.len() != schema.groups.len() {
                return Err(error(
                    r.span,
                    format!("{} requires {} origin arguments", f.ty, schema.groups.len()),
                ));
            }
            let inputs = f
                .origins
                .iter()
                .map(|a| input_sources(&a.display(), r, schemas))
                .collect::<Result<Vec<_>>>()?;
            for leaf in &schema.leaves {
                let sources = inputs[leaf.group.expect("complete grouped schema")].clone();
                let mutable = leaf_exclusive(&schema, leaf);
                for source in &sources {
                    if mutable
                        && !manifest::reference_inner(&f.ty).is_some_and(|(m, _)| !m)
                        && !source_mutable(source, r, schemas)
                    {
                        return Err(error(
                            r.span,
                            "exclusive field cannot borrow a shared input origin",
                        ));
                    }
                }
                returns.push(crate::ir::StoredReturn {
                    through: leaf.through.clone(),
                    group: schema.groups[leaf.group.unwrap()].clone(),
                    outcome: o.name.clone(),
                    payload,
                    fields: leaf.fields.clone(),
                    sum_leaf: leaf.sum_leaf.clone(),
                    sources,
                });
            }
        }
    }
    Ok(returns)
}

/// One public group intentionally combines its leaves' possible dependencies.
/// Distinct groups remain distinct, including when their value types coincide.
pub(crate) fn normalize(
    r: &mut crate::ir::Region,
    schemas: &Schemas,
    _m: &manifest::Manifest,
    at: Span,
) -> Result<()> {
    for o in &r.exits {
        for (payload, field) in o.fields.iter().enumerate() {
            let Some(schema) = schemas
                .get(manifest::reference_inner(&field.ty).map_or(field.ty.as_str(), |(_, t)| t))
                .filter(|s| !s.groups.is_empty())
            else {
                continue;
            };
            for leaf in &schema.leaves {
                if !r.stored_returns.iter().any(|c| {
                    c.outcome == o.name
                        && c.payload == payload
                        && c.through == leaf.through
                        && c.fields == leaf.fields
                        && c.sum_leaf == leaf.sum_leaf
                        && !c.sources.is_empty()
                }) {
                    return Err(error(
                        at,
                        "cannot infer an unproduced stored-reference result; write its explicit bracket origin bindings",
                    ));
                }
            }
            for group in 0..schema.groups.len() {
                let paths = schema
                    .leaves
                    .iter()
                    .filter(|l| l.group == Some(group))
                    .map(|l| (&l.through, &l.fields, &l.sum_leaf))
                    .collect::<Vec<_>>();
                let mut sources = vec![];
                for result in r.stored_returns.iter().filter(|r| {
                    r.outcome == o.name
                        && r.payload == payload
                        && paths.contains(&(&r.through, &r.fields, &r.sum_leaf))
                }) {
                    for s in &result.sources {
                        if !sources.contains(s) {
                            sources.push(s.clone());
                        }
                    }
                }
                for result in r.stored_returns.iter_mut().filter(|r| {
                    r.outcome == o.name
                        && r.payload == payload
                        && paths.contains(&(&r.through, &r.fields, &r.sum_leaf))
                }) {
                    result.sources = sources.clone();
                }
            }
        }
        // Lifetime unions deliberately forget correlation between exit paths.
        // Exclusive separation is established while constructing each exit,
        // then instantiated as fresh leaf permissions by the caller. Comparing
        // these anchors cannot establish or refute that separation.
    }
    Ok(())
}

/// Resolve public selectors without treating a private field name as a group.
pub(crate) fn input_sources(
    text: &str,
    r: &syntax::Region,
    schemas: &Schemas,
) -> Result<Vec<crate::ir::BorrowSource>> {
    let (name, group) = if let Some((name, group)) = text.split_once("['") {
        (
            name,
            Some(
                group
                    .strip_suffix(']')
                    .ok_or_else(|| error(r.span, "invalid contained origin selector"))?,
            ),
        )
    } else {
        (text, None)
    };
    let parameter = r
        .parameters
        .iter()
        .position(|p| p.name == name)
        .ok_or_else(|| error(r.span, format!("unknown origin input `{name}`")))?;
    let ty = &r.parameters[parameter].ty;
    if let Some(group) = group {
        let ty = manifest::reference_inner(ty).map_or(ty.as_str(), |(_, ty)| ty);
        let schema = schemas
            .get(ty)
            .ok_or_else(|| error(r.span, "contained origin requires a grouped input"))?;
        let index = schema
            .groups
            .iter()
            .position(|g| g == group)
            .ok_or_else(|| {
                error(
                    r.span,
                    format!("unknown public origin group `'{group}` on `{name}`"),
                )
            })?;
        let mut sources = vec![];
        for leaf in schema.leaves.iter().filter(|l| l.group == Some(index)) {
            sources.push(crate::ir::BorrowSource {
                guard: false,
                parameter,
                fields: vec![],
                opaque: true,
                contained: Some(crate::ir::ContainedOrigin {
                    through: leaf.through.clone(),
                    group: group.into(),
                    sum_leaf: leaf.sum_leaf.clone(),
                    fields: leaf.fields.clone(),
                }),
            });
            if manifest::reference_inner(&leaf.ty).is_some_and(|(m, _)| m) {
                for (index, step) in leaf.through.iter().enumerate() {
                    let parent = schema
                        .leaves
                        .iter()
                        .find(|l| {
                            l.through == leaf.through[..index]
                                && l.fields == step.fields
                                && l.sum_leaf == step.sum_leaf
                        })
                        .unwrap();
                    sources.push(crate::ir::BorrowSource {
                        guard: true,
                        parameter,
                        fields: vec![],
                        opaque: true,
                        contained: Some(crate::ir::ContainedOrigin {
                            through: parent.through.clone(),
                            fields: parent.fields.clone(),
                            sum_leaf: parent.sum_leaf.clone(),
                            group: schema.groups[parent.group.unwrap()].clone(),
                        }),
                    });
                }
            }
            if manifest::reference_inner(&r.parameters[parameter].ty).is_some()
                && manifest::reference_inner(&leaf.ty).is_some_and(|(m, _)| m)
            {
                sources.push(crate::ir::BorrowSource {
                    guard: true,
                    parameter,
                    fields: leaf
                        .through
                        .first()
                        .map_or_else(|| leaf.fields.clone(), |s| s.fields.clone()),
                    opaque: true,
                    contained: None,
                });
            }
        }
        Ok(sources)
    } else {
        if manifest::reference_inner(ty).is_none() {
            return Err(error(
                r.span,
                "result origins cannot borrow owned parameter storage; select a contained origin with input['group]",
            ));
        }
        Ok(vec![crate::ir::BorrowSource {
            guard: false,
            parameter,
            fields: vec![],
            opaque: true,
            contained: None,
        }])
    }
}
pub(crate) fn source_mutable(
    source: &crate::ir::BorrowSource,
    r: &syntax::Region,
    schemas: &Schemas,
) -> bool {
    if source.guard {
        return true;
    }
    let ty = &r.parameters[source.parameter].ty;
    if manifest::reference_inner(ty).is_some_and(|(mutable, _)| !mutable) {
        return false;
    }
    if let Some(c) = &source.contained {
        let ty = manifest::reference_inner(ty).map_or(ty.as_str(), |(_, ty)| ty);
        schemas.get(ty).is_some_and(|s| {
            s.leaves
                .iter()
                .find(|l| {
                    l.through == c.through && l.fields == c.fields && l.sum_leaf == c.sum_leaf
                })
                .is_some_and(|l| leaf_exclusive(&s, l))
        })
    } else {
        manifest::reference_inner(ty).is_some_and(|(m, _)| m)
    }
}
/// A declared guard may retain a whole carrier instead of a narrower actual field.
/// The first argument is the declared source, the second the body-derived source.
pub(crate) fn same_root(a: &crate::ir::BorrowSource, b: &crate::ir::BorrowSource) -> bool {
    a.parameter == b.parameter
        && a.contained == b.contained
        && a.guard == b.guard
        && (!a.guard || b.fields.starts_with(&a.fields))
}

pub(crate) fn borrowed_input_type(
    ty: &str,
    schemas: &Schemas,
    m: &manifest::Manifest,
    at: Span,
) -> Result<()> {
    if !m.contains_references(ty) {
        return m.check_type(ty);
    }
    if !schemas.get(ty).is_some_and(|s| !s.groups.is_empty()) {
        return Err(Diagnostic::new(
            "E_UNSUPPORTED",
            at,
            "borrowing reference-bearing storage requires a declared origin schema",
        ));
    }
    result_type(ty, schemas, m, at)?;
    m.check_local_storage(ty)
}

pub(crate) fn input_type(
    ty: &str,
    schemas: &Schemas,
    m: &manifest::Manifest,
    at: Span,
) -> Result<()> {
    if schemas.get(ty).is_some_and(|s| !s.groups.is_empty()) {
        result_type(ty, schemas, m, at)
    } else {
        m.check_type(ty)
    }
}

/// Instantiate a named completion contract's groups at its control boundary.
pub(crate) fn bind_contract(r: &mut syntax::Region, c: &syntax::OutcomeContract) -> Result<()> {
    if !r.exit_origins.is_empty() && r.exit_origins.len() != c.origin_parameters.len() {
        return Err(error(
            r.span,
            format!(
                "{} requires {} origin arguments",
                c.name,
                c.origin_parameters.len()
            ),
        ));
    }
    r.exits = c.outcomes.clone();
    for o in &mut r.exits {
        for (index, f) in o.fields.iter_mut().enumerate() {
            let mut mapped = vec![];
            if !r.exit_origins.is_empty() {
                for origin in &f.origins {
                    let OriginArgument::Group(group) = origin else {
                        return Err(error(r.span, "named outcome schemas use apostrophe groups"));
                    };
                    let position = c
                        .origin_parameters
                        .iter()
                        .position(|g| g == group)
                        .ok_or_else(|| error(r.span, "unknown named outcome origin group"))?;
                    mapped.push(r.exit_origins[position].clone());
                }
            }
            if manifest::reference_inner(&f.ty).is_some() {
                if let Some(origin) = mapped.first() {
                    r.return_origins
                        .insert((o.name.clone(), index), origin.display());
                }
                f.origins = if r.exit_origins.is_empty() {
                    vec![]
                } else {
                    f.referent_origins
                        .iter()
                        .map(|origin| {
                            let OriginArgument::Group(group) = origin else {
                                return Err(error(
                                    r.span,
                                    "named outcome schemas use apostrophe groups",
                                ));
                            };
                            let position = c
                                .origin_parameters
                                .iter()
                                .position(|g| g == group)
                                .ok_or_else(|| {
                                    error(r.span, "unknown named outcome origin group")
                                })?;
                            r.exit_origins.get(position).cloned().ok_or_else(|| {
                                error(
                                    r.span,
                                    "nested named outcome results require explicit origin bindings",
                                )
                            })
                        })
                        .collect::<Result<Vec<_>>>()?
                };
                f.referent_origins.clear();
            } else {
                f.origins = mapped;
            }
        }
    }
    Ok(())
}
