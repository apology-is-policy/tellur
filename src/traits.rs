//! Coherent, explicitly selected concrete trait implementations. Associated type
//! normalization is bounded and precedes ordinary ownership/control checking.
use crate::{
    Diagnostic, Result, Span,
    collection_support::{generic, qualified_member},
    generics::substitute,
    manifest::{self, Manifest},
    modules::Visibility,
    syntax::*,
};
use std::collections::{BTreeMap, BTreeSet};
fn error(at: Span, message: impl Into<String>) -> Diagnostic {
    Diagnostic::new("E_TRAIT", at, message)
}
fn signature_error(at: Span, message: impl Into<String>) -> Diagnostic {
    Diagnostic::new("E_TRAIT_SIGNATURE", at, message)
}
fn unique<'a>(names: impl Iterator<Item = &'a str>, at: Span) -> Result<()> {
    let mut seen = BTreeSet::new();
    for n in names {
        if !seen.insert(n) {
            return Err(error(
                at,
                format!("duplicate trait member or parameter `{n}`"),
            ));
        }
    }
    Ok(())
}
fn leaves(ty: &str) -> Vec<String> {
    if let Some((_, inner)) = manifest::reference_inner(ty) {
        return leaves(inner);
    }
    if let Some((app, _)) = qualified_member(ty) {
        return leaves(app);
    }
    if let Some((base, args)) = generic(ty) {
        return std::iter::once(base.into())
            .chain(args.into_iter().flat_map(leaves))
            .collect();
    }
    vec![ty.into()]
}
fn check_decl_type(
    ty: &str,
    params: &BTreeSet<String>,
    arities: &BTreeMap<String, usize>,
    at: Span,
) -> Result<()> {
    if let Some((_, inner)) = manifest::reference_inner(ty) {
        return check_decl_type(inner, params, arities, at);
    }
    if qualified_member(ty).is_some() {
        return Err(error(
            at,
            "trait declarations use lexical self and associated names; dependent trait projections require the generic trait checkpoint",
        ));
    }
    if params.contains(ty) {
        return Ok(());
    }
    if let Some((base, args)) = generic(ty) {
        if params.contains(base) {
            return Err(error(
                at,
                "associated and self types are first-order; they cannot take type arguments",
            ));
        }
        if arities.get(base) != Some(&args.len()) {
            return Err(error(
                at,
                format!("unknown type or wrong arity `{ty}` in trait signature"),
            ));
        }
        for (i, arg) in args.iter().enumerate() {
            if base == "array" && i == 1 {
                if arg.parse::<u32>().is_err() {
                    return Err(error(at, "array length must be a u32 literal"));
                }
            } else {
                check_decl_type(arg, params, arities, at)?;
            }
        }
    } else if arities.get(ty) != Some(&0) {
        return Err(error(
            at,
            format!("unknown or unapplied type `{ty}` in trait signature"),
        ));
    }
    Ok(())
}
/// Resolve named completion schemas before comparing their public origins.
fn declared_signature(r: &Region, contracts: &BTreeMap<String, OutcomeContract>) -> Result<Region> {
    let mut signature = r.clone();
    if let Some(c) = &r.exit_contract {
        if let Some(contract) = contracts.get(c) {
            if !contract.type_parameters.is_empty() {
                return Err(signature_error(
                    r.span,
                    "named trait exit contract has the wrong type arity",
                ));
            }
            crate::origins::bind_contract(&mut signature, contract)?;
            return Ok(signature);
        }
        let (base, args) = generic(c)
            .map(|(b, a)| (b, a.into_iter().map(str::to_string).collect::<Vec<_>>()))
            .unwrap_or((c.as_str(), vec![]));
        if let Some(d) = contracts.get(base) {
            if d.type_parameters.len() != args.len() {
                return Err(signature_error(
                    r.span,
                    "named trait exit contract has the wrong type arity",
                ));
            }
            let bindings = d.type_parameters.iter().cloned().zip(args).collect();
            let mut contract = d.clone();
            for o in &mut contract.outcomes {
                for f in &mut o.fields {
                    f.ty = substitute(&f.ty, &bindings);
                }
            }
            crate::origins::bind_contract(&mut signature, &contract)?;
        } else if !r.exit_origins.is_empty() {
            return Err(Diagnostic::new(
                "E_ORIGIN",
                r.span,
                "origin arguments require a source-declared named outcome contract",
            ));
        }
    }
    Ok(signature)
}

/// Trait assumptions cannot infer lifetime relationships from an implementation.
fn explicit_origins(r: &Region, schemas: &crate::origins::Schemas) -> Result<()> {
    crate::lifetimes::declared_with_schemas(r, true, schemas)?;
    for field in r.exits.iter().flat_map(|o| &o.fields) {
        let ty = manifest::reference_inner(&field.ty).map_or(field.ty.as_str(), |(_, t)| t);
        if schemas.get(ty).is_some_and(|s| !s.leaves.is_empty()) && field.origins.is_empty() {
            return Err(Diagnostic::new(
                "E_ORIGIN",
                r.span,
                "trait aggregate results require explicit bracket origin bindings",
            ));
        }
    }
    crate::origins::declared(r, schemas)?;
    Ok(())
}

fn method_profile(r: &Region) -> Result<()> {
    if !r.modern || !r.type_parameters.is_empty() || r.infer_exits || !r.infer_payloads.is_empty() {
        return Err(signature_error(
            r.span,
            "trait methods use nongeneric control declarations with explicit exit contracts",
        ));
    }
    unique(r.parameters.iter().map(|p| p.name.as_str()), r.span)?;
    unique(r.exits.iter().map(|o| o.name.as_str()), r.span)?;
    for o in &r.exits {
        unique(o.fields.iter().map(|f| f.name.as_str()), r.span)?;
    }
    Ok(())
}
struct Normalizer<'a> {
    implementations: &'a BTreeMap<(String, String), Implementation>,
    values: BTreeMap<String, String>,
    active: BTreeSet<String>,
}
impl Normalizer<'_> {
    fn ty(
        &mut self,
        ty: &str,
        context: Option<(&str, &str)>,
        at: Span,
        depth: usize,
    ) -> Result<String> {
        if depth > 64 || ty.len() > 16384 {
            return Err(error(
                at,
                "associated type normalization exceeds its depth or type-size budget",
            ));
        }
        if let Some((trait_name, target)) = context {
            if ty == "self" {
                return Ok(target.into());
            }
            if self.implementations[&(trait_name.into(), target.into())]
                .associated
                .iter()
                .any(|a| a.name == ty)
            {
                return self.associated(trait_name, target, ty, at, depth + 1);
            }
        }
        if let Some((mutable, inner)) = manifest::reference_inner(ty) {
            return Ok(format!(
                "&{}{}",
                if mutable { "mutable " } else { "" },
                self.ty(inner, context, at, depth + 1)?
            ));
        }
        if let Some((app, member)) = qualified_member(ty) {
            let (base, args) = generic(app).ok_or_else(|| error(at, "invalid trait selection"))?;
            if args.len() != 1 || member.contains('.') {
                return Err(error(
                    at,
                    "an associated type selects one implementing type and one member",
                ));
            }
            let target = self.ty(args[0], context, at, depth + 1)?;
            return self.associated(base, &target, member, at, depth + 1);
        }
        if let Some((base, args)) = generic(ty) {
            let args = args
                .iter()
                .map(|a| self.ty(a, context, at, depth + 1))
                .collect::<Result<Vec<_>>>()?;
            return Ok(format!("{base}<{}>", args.join(",")));
        }
        Ok(ty.into())
    }
    fn associated(
        &mut self,
        trait_name: &str,
        target: &str,
        name: &str,
        at: Span,
        depth: usize,
    ) -> Result<String> {
        let key = format!("{trait_name}<{target}>.{name}");
        if let Some(value) = self.values.get(&key) {
            return Ok(value.clone());
        }
        if depth > 64 || !self.active.insert(key.clone()) {
            return Err(error(
                at,
                format!("cyclic or excessively deep associated type `{key}`"),
            ));
        }
        let implementation = self
            .implementations
            .get(&(trait_name.into(), target.into()))
            .ok_or_else(|| {
                error(
                    at,
                    format!("no implementation of `{trait_name}` for `{target}`"),
                )
            })?;
        let value = implementation
            .associated
            .iter()
            .find(|a| a.name == name)
            .and_then(|a| a.value.clone())
            .ok_or_else(|| error(at, format!("unknown associated type `{key}`")))?;
        let value = self.ty(&value, Some((trait_name, target)), at, depth + 1)?;
        if manifest::reference_inner(&value).is_some() {
            return Err(Diagnostic::new(
                "E_LIFETIME",
                at,
                "associated types are owned types; stored borrowed types require the lifetime checkpoint",
            ));
        }
        self.active.remove(&key);
        self.values.insert(key, value.clone());
        Ok(value)
    }
}
fn in_scope(
    replacements: &BTreeMap<String, String>,
    parameters: &[String],
) -> BTreeMap<String, String> {
    replacements
        .iter()
        .filter(|(key, _)| !leaves(key).iter().any(|name| parameters.contains(name)))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}
pub(crate) fn lower(
    mut unit: Unit,
    manifest: &Manifest,
    visibility: &mut Visibility,
) -> Result<Unit> {
    for r in &unit.regions {
        for bounds in r.type_bounds.values() {
            let mut seen = BTreeSet::new();
            for bound in bounds {
                if !seen.insert(bound) {
                    return Err(Diagnostic::new(
                        "E_BOUND",
                        r.span,
                        "duplicate requirement after module resolution",
                    ));
                }
                if ["copy", "clone", "drop"].contains(&bound.as_str()) {
                    continue;
                }
                let declaration =
                    unit.traits
                        .iter()
                        .find(|t| &t.name == bound)
                        .ok_or_else(|| {
                            Diagnostic::new(
                                "E_BOUND",
                                r.span,
                                format!("unknown capability or trait `{bound}`"),
                            )
                        })?;
                if r.public && !declaration.public {
                    return Err(Diagnostic::new(
                        "E_VISIBILITY",
                        r.span,
                        "public generic control requires a private trait",
                    ));
                }
            }
        }
    }
    if unit.traits.is_empty() && unit.implementations.is_empty() {
        return Ok(unit);
    }
    let private = unit
        .data
        .iter()
        .filter(|d| !d.public)
        .map(|d| d.name.clone())
        .chain(
            unit.contracts
                .iter()
                .filter(|d| !d.public)
                .map(|d| d.name.clone()),
        )
        .collect::<BTreeSet<_>>();
    let mut arities = manifest
        .types
        .iter()
        .map(|d| (d.name().to_string(), 0))
        .collect::<BTreeMap<_, _>>();
    for n in ["Bool", "U32", "String", "Bytes", "Unit"] {
        arities.insert(n.into(), 0);
    }
    for (n, a) in [("Option", 1), ("vector", 1), ("array", 2), ("map", 2)] {
        arities.insert(n.into(), a);
    }
    for d in &unit.data {
        arities.insert(d.name.clone(), d.type_parameters.len());
    }
    for d in &unit.contracts {
        arities.insert(d.name.clone(), d.type_parameters.len());
    }
    let contracts = unit
        .contracts
        .iter()
        .map(|d| (d.name.clone(), d.clone()))
        .collect::<BTreeMap<_, _>>();
    let origin_schemas = crate::origins::schemas(&unit)?;
    let mut declarations = BTreeMap::new();
    for d in &unit.traits {
        if manifest.primitives.iter().any(|p| p.name == d.name)
            || arities.contains_key(&d.name)
            || unit.regions.iter().any(|r| r.name == d.name)
            || ["copy", "clone", "drop", "self"].contains(&d.name.as_str())
            || declarations.insert(d.name.clone(), d.clone()).is_some()
        {
            return Err(error(
                d.span,
                format!("duplicate or reserved trait name `{}`", d.name),
            ));
        }
        unique(
            d.associated
                .iter()
                .map(|a| a.name.as_str())
                .chain(d.methods.iter().map(|m| m.name.as_str())),
            d.span,
        )?;
        let mut params = BTreeSet::from(["self".into()]);
        for a in &d.associated {
            if a.name == "self"
                || [
                    "Bool", "U32", "String", "Bytes", "Unit", "Option", "array", "vector", "map",
                    "auto", "outcome",
                ]
                .contains(&manifest::canonical_type(&a.name).as_str())
            {
                return Err(error(
                    a.span,
                    "associated type name shadows a reserved type",
                ));
            }
            params.insert(a.name.clone());
        }
        for method in &d.methods {
            method_profile(method)?;
            if let Some(c) = &method.exit_contract {
                let base = generic(c).map_or(c.as_str(), |(b, _)| b);
                if generic(c).is_some() || arities.contains_key(base) || params.contains(base) {
                    check_decl_type(c, &params, &arities, method.span)?;
                }
            }
            let signature = declared_signature(method, &contracts)?;
            for f in method
                .parameters
                .iter()
                .chain(signature.exits.iter().flat_map(|o| &o.fields))
            {
                check_decl_type(&f.ty, &params, &arities, method.span)?;
                if d.public
                    && leaves(&f.ty)
                        .iter()
                        .any(|t| !params.contains(t) && private.contains(t))
                {
                    return Err(Diagnostic::new(
                        "E_VISIBILITY",
                        method.span,
                        "public trait signature exposes a private type",
                    ));
                }
            }
            explicit_origins(&signature, &origin_schemas)?;
            if let Some(c) = &method.exit_contract {
                let base = generic(c).map_or(c.as_str(), |(b, _)| b);
                if d.public
                    && leaves(c)
                        .iter()
                        .any(|t| !params.contains(t) && private.contains(t))
                {
                    return Err(Diagnostic::new(
                        "E_VISIBILITY",
                        method.span,
                        "public trait exposes a private exit contract",
                    ));
                }
                if (arities.contains_key(base) || params.contains(base) || generic(c).is_some())
                    && !contracts.contains_key(base)
                    && !matches!(
                        manifest.type_def(base),
                        Some(manifest::TypeDef::Outcome { .. })
                    )
                {
                    return Err(signature_error(
                        method.span,
                        "trait exit contract must be an outcome type",
                    ));
                }
            }
        }
    }
    if unit.implementations.len() > 1024 {
        return Err(error(
            unit.implementations[1024].span,
            "trait implementations exceed the 1024-implementation budget",
        ));
    }
    let mut implementations = BTreeMap::new();
    for i in &unit.implementations {
        let d = declarations
            .get(&i.trait_name)
            .ok_or_else(|| error(i.span, format!("unknown trait `{}`", i.trait_name)))?;
        if manifest::reference_inner(&i.target).is_some() || qualified_member(&i.target).is_some() {
            return Err(error(
                i.span,
                "implementation targets must be explicit concrete owned types",
            ));
        }
        check_decl_type(&i.target, &BTreeSet::new(), &arities, i.span)?;
        if d.module != i.module && !visibility.owns(i.module, &i.target) {
            return Err(Diagnostic::new(
                "E_COHERENCE",
                i.span,
                "implementation module must own the trait or the outer nominal target type",
            ));
        }
        unique(
            i.associated
                .iter()
                .map(|a| a.name.as_str())
                .chain(i.methods.iter().map(|m| m.name.as_str())),
            i.span,
        )?;
        if i.associated
            .iter()
            .map(|a| a.name.as_str())
            .collect::<BTreeSet<_>>()
            != d.associated.iter().map(|a| a.name.as_str()).collect()
            || i.methods
                .iter()
                .map(|m| m.name.as_str())
                .collect::<BTreeSet<_>>()
                != d.methods.iter().map(|m| m.name.as_str()).collect()
        {
            return Err(error(
                i.span,
                "implementation must supply exactly the trait's associated types and controls",
            ));
        }
        for m in &i.methods {
            method_profile(m)?;
        }
        let key = (i.trait_name.clone(), i.target.clone());
        if implementations.insert(key, i.clone()).is_some() {
            return Err(Diagnostic::new(
                "E_COHERENCE",
                i.span,
                "duplicate implementation for the same trait and type",
            ));
        }
    }
    let mut normalizer = Normalizer {
        implementations: &implementations,
        values: BTreeMap::new(),
        active: BTreeSet::new(),
    };
    for ((name, target), i) in &implementations {
        for a in &i.associated {
            normalizer.associated(name, target, &a.name, a.span, 0)?;
        }
    }
    let replacements = normalizer.values;
    // Every substitution comes from a checked trait/type/member identity.
    for data in &mut unit.data {
        let replacements = in_scope(&replacements, &data.type_parameters);
        match &mut data.body {
            DataBody::Struct(fs) => {
                for f in fs {
                    f.ty = substitute(&f.ty, &replacements);
                }
            }
            DataBody::Variant(os) => {
                for o in os {
                    for f in &mut o.fields {
                        f.ty = substitute(&f.ty, &replacements);
                    }
                }
            }
        }
    }
    for d in &mut unit.contracts {
        let replacements = in_scope(&replacements, &d.type_parameters);
        for o in &mut d.outcomes {
            for f in &mut o.fields {
                f.ty = substitute(&f.ty, &replacements);
            }
        }
    }
    for r in &mut unit.regions {
        crate::generic_controls::rewrite_region(r, &in_scope(&replacements, &r.type_parameters));
    }
    for ((trait_name, target), i) in &implementations {
        let d = &declarations[trait_name];
        let public = d.public && !leaves(target).iter().any(|t| private.contains(t));
        let mut bindings = replacements.clone();
        bindings.insert("self".into(), target.clone());
        let mut associated = vec![];
        for a in &i.associated {
            let value = replacements[&format!("{trait_name}<{target}>.{}", a.name)].clone();
            associated.push(value.clone());
            bindings.insert(a.name.clone(), value);
        }
        let mut expected = vec![];
        for mut method in d.methods.clone() {
            method.name = format!("{trait_name}<{target}>.{}", method.name);
            crate::generic_controls::rewrite_region(&mut method, &bindings);
            expected.push(method);
        }
        for mut method in i.methods.clone() {
            let permission = d
                .methods
                .iter()
                .find(|m| m.name == method.name)
                .unwrap()
                .fault_contract
                .unwrap_or(false);
            if method.fault_contract == Some(true) && !permission {
                return Err(Diagnostic::new(
                    "E_FAULT_CONTRACT",
                    method.span,
                    "implementation cannot add faults to a trait control that does not permit them",
                ));
            }
            method.fault_contract = Some(permission);
            let contingent = d
                .methods
                .iter()
                .find(|m| m.name == method.name)
                .unwrap()
                .contingent_contract
                == Some(true);
            if method.contingent_contract == Some(true) && !contingent {
                return Err(Diagnostic::new(
                    "E_CONTINGENT",
                    method.span,
                    "implementation cannot enlarge the trait contingency subset",
                ));
            }
            method.contingent_contract = Some(contingent);
            method.name = format!("{trait_name}<{target}>.{}", method.name);
            method.public = public;
            crate::generic_controls::rewrite_region(&mut method, &bindings);
            method.type_bindings = bindings
                .iter()
                .filter(|(key, _)| {
                    key.as_str() == "self" || i.associated.iter().any(|a| &a.name == *key)
                })
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            visibility.method(method.name.clone(), i.module);
            unit.regions.push(method);
        }
        let requirements = i
            .associated
            .iter()
            .map(|a| {
                (
                    bindings[&a.name].clone(),
                    d.associated
                        .iter()
                        .find(|d| d.name == a.name)
                        .unwrap()
                        .bounds
                        .clone(),
                    a.span,
                )
            })
            .collect();
        unit.trait_checks.push(TraitCheck {
            trait_name: trait_name.clone(),
            requirements,
            span: i.span,
            target: target.clone(),
            public,
            associated,
            expected,
        });
    }
    unit.trait_types = replacements;
    unit.implementations.clear();
    Ok(unit)
}
pub(crate) fn required_types(checks: &[TraitCheck]) -> Vec<(String, Span)> {
    let mut types = vec![];
    for c in checks {
        types.push((c.target.clone(), c.span));
        types.extend(c.associated.iter().map(|t| (t.clone(), c.span)));
        for r in &c.expected {
            types.extend(
                r.parameters
                    .iter()
                    .chain(r.exits.iter().flat_map(|o| &o.fields))
                    .map(|f| {
                        (
                            manifest::reference_inner(&f.ty)
                                .map_or(f.ty.as_str(), |(_, t)| t)
                                .into(),
                            c.span,
                        )
                    }),
            );
            if let Some(t) = &r.exit_contract
                && generic(t).is_some()
            {
                types.push((t.clone(), c.span));
            }
        }
    }
    types
}
fn exits(r: &Region, m: &Manifest) -> Vec<(String, Vec<String>)> {
    let outcomes = if let Some(t) = &r.exit_contract {
        if m.is_outcome(t) { m.outcomes(t) } else { None }
    } else {
        None
    };
    let mut result = if let Some(os) = outcomes {
        os.iter()
            .map(|o| {
                (
                    o.name.clone(),
                    o.fields.iter().map(|f| f.ty.clone()).collect(),
                )
            })
            .collect::<Vec<_>>()
    } else {
        r.exits
            .iter()
            .map(|o| {
                (
                    o.name.clone(),
                    o.fields.iter().map(|f| f.ty.clone()).collect(),
                )
            })
            .collect::<Vec<_>>()
    };
    result.sort();
    result
}
pub(crate) fn check_contracts(
    checks: &[TraitCheck],
    regions: &[Region],
    contracts: &[OutcomeContract],
    schemas: &crate::origins::Schemas,
    m: &Manifest,
) -> Result<()> {
    let contracts = contracts
        .iter()
        .map(|c| (c.name.clone(), c.clone()))
        .collect();
    for c in checks {
        for (ty, bounds, at) in &c.requirements {
            for bound in bounds {
                if !m.has_capability(ty, bound) {
                    return Err(Diagnostic::new(
                        "E_BOUND",
                        *at,
                        format!(
                            "associated type `{}` does not satisfy `{bound}`",
                            manifest::display_type(ty)
                        ),
                    ));
                }
            }
        }
        for t in std::iter::once(&c.target).chain(&c.associated) {
            m.check_type(t).map_err(|mut e| {
                e.span = c.span;
                e
            })?;
        }
        for expected in &c.expected {
            let Some(actual) = regions.iter().find(|r| r.name == expected.name) else {
                continue;
            }; // Symbolic generic checking includes only dependency-reachable ordinary controls.
            let expected = declared_signature(expected, &contracts)?;
            let actual = declared_signature(actual, &contracts)?;
            explicit_origins(&expected, schemas)?;
            explicit_origins(&actual, schemas)?;
            let parameters = |r: &Region| {
                r.parameters
                    .iter()
                    .map(|p| p.ty.clone())
                    .collect::<Vec<_>>()
            };
            if crate::lifetimes::declared_with_schemas(&expected, true, schemas)?
                != crate::lifetimes::declared_with_schemas(&actual, true, schemas)?
            {
                return Err(Diagnostic::new(
                    "E_LIFETIME",
                    actual.span,
                    "implementation return origins must match the trait's parameter positions",
                ));
            }
            let stored = |r: &Region| -> Result<_> {
                let mut returns = crate::origins::declared(r, schemas)?;
                returns.sort_by(|a, b| {
                    (&a.outcome, a.payload, &a.through, &a.fields, &a.sum_leaf).cmp(&(
                        &b.outcome,
                        b.payload,
                        &b.through,
                        &b.fields,
                        &b.sum_leaf,
                    ))
                });
                Ok(returns)
            };
            if stored(&expected)? != stored(&actual)? {
                return Err(Diagnostic::new(
                    "E_ORIGIN",
                    actual.span,
                    "implementation contained origins must match the trait's parameter positions and public groups",
                ));
            }
            if parameters(&expected) != parameters(&actual)
                || exits(&expected, m) != exits(&actual, m)
            {
                return Err(signature_error(
                    actual.span,
                    format!(
                        "implementation `{}` must match declared parameter ownership and exit payloads",
                        actual.name
                    ),
                ));
            }
        }
    }
    Ok(())
}

pub(crate) fn has_witness(checks: &[TraitCheck], trait_name: &str, target: &str) -> bool {
    checks
        .iter()
        .any(|c| c.trait_name == trait_name && c.target == target)
}
/// Construct a signature-only witness inside the definition-checking unit. It
/// never enters the concrete unit or the returned CheckedProgram.
pub(crate) fn abstract_witness(
    unit: &mut Unit,
    manifest: &mut Manifest,
    trait_name: &str,
    target: &str,
    parameter: &str,
    counter: &mut usize,
    names: &mut BTreeMap<String, String>,
) -> Result<()> {
    let declaration = unit
        .traits
        .iter()
        .find(|d| d.name == trait_name)
        .cloned()
        .ok_or_else(|| error(Span::default(), "unknown abstract trait"))?;
    if unit.regions.iter().filter(|r| r.abstract_signature).count() + declaration.methods.len()
        > 4096
    {
        return Err(error(
            declaration.span,
            "generic definition checking exceeds the 4096-abstract-operation budget",
        ));
    }
    let mut bindings = BTreeMap::from([("self".into(), target.into())]);
    let mut associated = vec![];
    for a in &declaration.associated {
        if *counter >= 4096 {
            return Err(error(
                a.span,
                "generic definition checking exceeds the 4096-abstract-type budget",
            ));
        }
        let name = loop {
            let name = format!("_path_poly_{}_associated", *counter);
            *counter += 1;
            if manifest.type_def(&name).is_none() {
                break name;
            }
        };
        let capabilities = manifest::Capabilities::from_bounds(&a.bounds);
        manifest.abstract_types.insert(name.clone(), capabilities);
        manifest.types.push(manifest::TypeDef::Opaque {
            name: name.clone(),
            linear: !capabilities.drop,
            fault: None,
        });
        unit.trait_types
            .insert(format!("{trait_name}<{target}>.{}", a.name), name.clone());
        names.insert(
            name.clone(),
            format!("{trait_name}<{parameter}>.{}", a.name),
        );
        bindings.insert(a.name.clone(), name.clone());
        associated.push(name);
    }
    for mut method in declaration.methods {
        method.name = format!("{trait_name}<{target}>.{}", method.name);
        crate::generic_controls::rewrite_region(&mut method, &bindings);
        method.abstract_signature = true;
        method.public = false;
        unit.regions.push(method);
    }
    unit.trait_checks.push(TraitCheck {
        trait_name: trait_name.into(),
        requirements: vec![],
        span: declaration.span,
        target: target.into(),
        public: false,
        associated,
        expected: vec![],
    });
    Ok(())
}
