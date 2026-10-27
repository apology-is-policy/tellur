use crate::{Diagnostic, Result, Span};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Field {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Outcome {
    pub name: String,
    pub fields: Vec<Field>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Value,
    Borrow,
    Consume,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Parameter {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: String,
    pub mode: Mode,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Primitive {
    pub name: String,
    pub parameters: Vec<Parameter>,
    pub outcomes: Vec<Outcome>,
    pub effects: Vec<String>,
    pub returns: bool,
    #[serde(default)]
    pub pure: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
pub enum TypeDef {
    Opaque {
        name: String,
        linear: bool,
    },
    Variant {
        name: String,
        constructors: Vec<Outcome>,
    },
    Outcome {
        name: String,
        constructors: Vec<Outcome>,
    },
    Record {
        name: String,
        fields: Vec<Field>,
    },
}
impl TypeDef {
    pub fn name(&self) -> &str {
        match self {
            Self::Opaque { name, .. }
            | Self::Variant { name, .. }
            | Self::Outcome { name, .. }
            | Self::Record { name, .. } => name,
        }
    }
}
/// Sealed permissions used only while checking generic definitions.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Capabilities {
    pub copy: bool,
    pub clone: bool,
    pub drop: bool,
}
impl Capabilities {
    pub fn from_bounds(bounds: &[String]) -> Self {
        let copy = bounds.iter().any(|b| b == "copy");
        let clone = copy || bounds.iter().any(|b| b == "clone");
        Self {
            copy,
            clone,
            drop: clone || bounds.iter().any(|b| b == "drop"),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    #[serde(skip)]
    pub(crate) abstract_types: BTreeMap<String, Capabilities>,
    pub version: String,
    pub types: Vec<TypeDef>,
    pub primitives: Vec<Primitive>,
}
fn error(message: impl Into<String>) -> Diagnostic {
    Diagnostic::new("E_MANIFEST", Span::default(), message)
}
/// References are compiler types; foreign manifest types cannot contain them yet.
pub fn reference_inner(ty: &str) -> Option<(bool, &str)> {
    ty.strip_prefix("&mutable ")
        .map(|t| (true, t))
        .or_else(|| ty.strip_prefix('&').map(|t| (false, t)))
}
/// Producer-indexed and anonymous compiler schemas; named outcomes use TypeDef::Outcome.
pub fn outcome_inner(ty: &str) -> Option<&str> {
    ty.strip_prefix("outcome<")
        .and_then(|s| s.strip_suffix('>'))
}
/// The wire/IR spelling remains compatible with v0.1 fixtures.
pub fn canonical_type(name: &str) -> String {
    if let Some((application, member)) = crate::collection_support::qualified_member(name) {
        return format!("{}.{member}", canonical_type(application));
    }
    if let Some(inner) = name
        .strip_prefix("option<")
        .or_else(|| name.strip_prefix("Option<"))
        .and_then(|s| s.strip_suffix('>'))
    {
        return format!("Option<{}>", canonical_type(inner));
    }
    if let Some((base, args)) = crate::collection_support::generic(name)
        && base != "outcome"
    {
        return format!(
            "{base}<{}>",
            args.iter()
                .enumerate()
                .map(|(index, a)| {
                    if base == "array" && index == 1 && a.bytes().all(|c| c.is_ascii_digit()) {
                        a.parse::<u32>()
                            .map(|n| n.to_string())
                            .unwrap_or_else(|_| a.to_string())
                    } else {
                        canonical_type(a)
                    }
                })
                .collect::<Vec<_>>()
                .join(",")
        );
    }
    match name {
        "bool" => "Bool",
        "u32" => "U32",
        "string" => "String",
        "bytes" => "Bytes",
        "unit" => "Unit",
        _ => name,
    }
    .into()
}
pub fn display_type(name: &str) -> String {
    if let Some((application, member)) = crate::collection_support::qualified_member(name) {
        return format!("{}.{member}", display_type(application));
    }
    if let Some(inner) = name.strip_prefix("&mutable ") {
        return format!("ref mutable {}", display_type(inner));
    }
    if let Some(inner) = name.strip_prefix('&') {
        return format!("ref {}", display_type(inner));
    }
    if let Some(inner) = option_inner(name) {
        return format!("option<{}>", display_type(inner));
    }
    if let Some(c) = crate::collection_support::Collection::parse(name) {
        return match c {
            crate::collection_support::Collection::Array(t, n) => {
                format!("array<{}, {n}>", display_type(&t))
            }
            crate::collection_support::Collection::Vector(t) => {
                format!("vector<{}>", display_type(&t))
            }
            crate::collection_support::Collection::Map(k, v) => {
                format!("map<{}, {}>", display_type(&k), display_type(&v))
            }
        };
    }
    if let Some((base, args)) = crate::collection_support::generic(name)
        && base != "outcome"
    {
        return format!(
            "{base}<{}>",
            args.iter()
                .map(|a| display_type(a))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    match name {
        "Bool" => "bool",
        "U32" => "u32",
        "String" => "string",
        "Bytes" => "bytes",
        "Unit" => "unit",
        _ => name,
    }
    .into()
}
/// Only builtins with an implemented, total expression evaluator can be marked pure.
pub fn pure_signature(p: &Primitive) -> Option<Vec<&'static str>> {
    let expected = match p.name.as_str() {
        "u32_text" => vec!["U32"],
        "concat" => vec!["String", "String"],
        _ => return None,
    };
    (p.pure
        && p.returns
        && p.effects.is_empty()
        && p.parameters.len() == expected.len()
        && p.parameters
            .iter()
            .zip(&expected)
            .all(|(p, t)| p.ty == *t && p.mode == Mode::Value)
        && p.outcomes.len() == 1
        && ["Text", "text"].contains(&p.outcomes[0].name.as_str())
        && p.outcomes[0].fields.len() == 1
        && p.outcomes[0].fields[0].ty == "String")
        .then_some(expected)
}
pub fn identifier(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}
fn unique<'a>(names: impl Iterator<Item = &'a str>) -> Result<()> {
    let mut seen = BTreeSet::new();
    for name in names {
        if !identifier(name) || !seen.insert(name) {
            return Err(error(format!("invalid or duplicate name `{name}`")));
        }
    }
    Ok(())
}
impl Manifest {
    /// Copy is separate from the obligation to complete a resource protocol.
    pub fn is_outcome(&self, ty: &str) -> bool {
        outcome_inner(ty).is_some() || matches!(self.type_def(ty), Some(TypeDef::Outcome { .. }))
    }
    pub fn is_copy(&self, ty: &str) -> bool {
        if let Some((mutable, _)) = reference_inner(ty) {
            return !mutable;
        }
        if let Some(c) = self.abstract_types.get(ty) {
            return c.copy;
        }
        if self.is_outcome(ty) {
            return false;
        }
        if let Some(c) = crate::collection_support::Collection::parse(ty) {
            return matches!(c, crate::collection_support::Collection::Array(ref t, _) if self.is_copy(t));
        }
        if let Some(inner) = option_inner(ty) {
            return self.is_copy(inner);
        }
        match ty {
            "Bool" | "U32" | "Unit" => true,
            "String" | "Bytes" => false,
            _ => match self.type_def(ty) {
                Some(TypeDef::Opaque { linear, .. }) => !linear,
                Some(TypeDef::Record { fields, .. }) => fields.iter().all(|f| self.is_copy(&f.ty)),
                Some(
                    TypeDef::Variant { constructors, .. } | TypeDef::Outcome { constructors, .. },
                ) => constructors
                    .iter()
                    .all(|o| o.fields.iter().all(|f| self.is_copy(&f.ty))),
                None => false,
            },
        }
    }
    pub fn is_cloneable(&self, ty: &str) -> bool {
        if let Some((mutable, _)) = reference_inner(ty) {
            return !mutable;
        }
        if let Some(c) = self.abstract_types.get(ty) {
            return c.clone;
        }
        if self.is_outcome(ty) {
            return false;
        }
        if let Some(c) = crate::collection_support::Collection::parse(ty) {
            return c.types().iter().all(|t| self.is_cloneable(t));
        }
        if let Some(inner) = option_inner(ty) {
            return self.is_cloneable(inner);
        }
        match ty {
            "Bool" | "U32" | "Unit" | "String" | "Bytes" => true,
            _ => match self.type_def(ty) {
                Some(TypeDef::Opaque { linear, .. }) => !linear,
                Some(TypeDef::Record { fields, .. }) => {
                    fields.iter().all(|f| self.is_cloneable(&f.ty))
                }
                Some(TypeDef::Variant { constructors, .. }) => constructors
                    .iter()
                    .all(|o| o.fields.iter().all(|f| self.is_cloneable(&f.ty))),
                _ => false,
            },
        }
    }
    pub(crate) fn has_capability(&self, ty: &str, bound: &str) -> bool {
        match bound {
            "copy" => self.is_copy(ty),
            "clone" => self.is_cloneable(ty),
            "drop" => !self.linear(ty),
            _ => false,
        }
    }
    pub fn from_json(text: &str) -> Result<Self> {
        let mut manifest: Self = serde_json::from_str(text).map_err(|e| error(e.to_string()))?;
        manifest.canonicalize_types();
        manifest.validate()?;
        Ok(manifest)
    }
    pub fn canonicalize_types(&mut self) {
        for ty in &mut self.types {
            match ty {
                TypeDef::Opaque { .. } => {}
                TypeDef::Record { fields, .. } => {
                    for f in fields {
                        f.ty = canonical_type(&f.ty);
                    }
                }
                TypeDef::Variant { constructors, .. } | TypeDef::Outcome { constructors, .. } => {
                    for o in constructors {
                        for f in &mut o.fields {
                            f.ty = canonical_type(&f.ty);
                        }
                    }
                }
            }
        }
        for p in &mut self.primitives {
            for param in &mut p.parameters {
                param.ty = canonical_type(&param.ty);
            }
            for o in &mut p.outcomes {
                for f in &mut o.fields {
                    f.ty = canonical_type(&f.ty);
                }
            }
        }
    }
    pub fn type_def(&self, name: &str) -> Option<&TypeDef> {
        self.types.iter().find(|t| t.name() == name)
    }
    pub fn linear(&self, name: &str) -> bool {
        fn visit(manifest: &Manifest, name: &str, seen: &mut BTreeSet<String>) -> bool {
            if !seen.insert(name.into()) {
                return false;
            }
            if manifest.is_outcome(name) {
                return true;
            }
            if let Some(inner) = option_inner(name) {
                return visit(manifest, inner, seen);
            }
            if let Some(c) = crate::collection_support::Collection::parse(name) {
                return c.types().iter().any(|t| visit(manifest, t, seen));
            }
            match manifest.type_def(name) {
                Some(TypeDef::Opaque { linear, .. }) => *linear,
                Some(TypeDef::Record { fields, .. }) => {
                    fields.iter().any(|f| visit(manifest, &f.ty, seen))
                }
                Some(
                    TypeDef::Variant { constructors, .. } | TypeDef::Outcome { constructors, .. },
                ) => constructors
                    .iter()
                    .flat_map(|c| &c.fields)
                    .any(|f| visit(manifest, &f.ty, seen)),
                _ => false,
            }
        }
        visit(self, name, &mut BTreeSet::new())
    }
    /// Outstanding obligations in a validated layout. A resource-carrying sum is
    /// one sealed obligation until a decision transfers its active payload.
    pub fn resource_paths(&self, name: &str) -> Vec<Vec<usize>> {
        if (option_inner(name).is_some()
            || crate::collection_support::Collection::parse(name).is_some())
            && self.linear(name)
        {
            return vec![vec![]];
        }
        match self.type_def(name) {
            Some(TypeDef::Opaque { linear: true, .. }) => vec![vec![]],
            Some(TypeDef::Variant { .. } | TypeDef::Outcome { .. }) if self.linear(name) => {
                vec![vec![]]
            }
            Some(TypeDef::Record { fields, .. }) => fields
                .iter()
                .enumerate()
                .flat_map(|(i, f)| {
                    self.resource_paths(&f.ty).into_iter().map(move |mut p| {
                        p.insert(0, i);
                        p
                    })
                })
                .collect(),
            _ => vec![],
        }
    }
    pub fn record_field(&self, ty: &str, name: &str) -> Option<(usize, &Field)> {
        match self.type_def(ty) {
            Some(TypeDef::Record { fields, .. }) => {
                fields.iter().enumerate().find(|(_, f)| f.name == name)
            }
            _ => None,
        }
    }
    pub fn field_at(&self, ty: &str, index: usize) -> Option<&Field> {
        match self.type_def(ty) {
            Some(TypeDef::Record { fields, .. }) => fields.get(index),
            _ => None,
        }
    }
    /// Static return-borrow projections; collection elements are conservative wildcards.
    pub fn borrow_field(&self, ty: &str, index: usize) -> Option<(String, String)> {
        if index == crate::collections::ELEMENT {
            let c = crate::collection_support::Collection::parse(ty)?;
            return Some(("[*]".into(), c.element().into()));
        }
        self.field_at(ty, index)
            .map(|f| (format!(".{}", f.name), f.ty.clone()))
    }
    pub fn valid_type(&self, name: &str) -> bool {
        if let Some(c) = crate::collection_support::Collection::parse(name) {
            if let crate::collection_support::Collection::Map(k, _) = &c
                && !["Bool", "U32", "String"].contains(&k.as_str())
            {
                return false;
            }
            return c.types().iter().all(|t| self.valid_type(t));
        }
        if let Some(inner) = option_inner(name) {
            return self.valid_type(inner);
        }
        ["Bool", "U32", "String", "Bytes", "Unit"].contains(&name) || self.type_def(name).is_some()
    }
    pub fn outcomes(&self, name: &str) -> Option<Vec<Outcome>> {
        if name == "Bool" {
            return Some(vec![
                Outcome {
                    name: "true".into(),
                    fields: vec![],
                },
                Outcome {
                    name: "false".into(),
                    fields: vec![],
                },
            ]);
        }
        if let Some(inner) = option_inner(name) {
            return Some(vec![
                Outcome {
                    name: "None".into(),
                    fields: vec![],
                },
                Outcome {
                    name: "Some".into(),
                    fields: vec![Field {
                        name: "value".into(),
                        ty: inner.into(),
                    }],
                },
            ]);
        }
        match self.type_def(name) {
            Some(TypeDef::Variant { constructors, .. } | TypeDef::Outcome { constructors, .. }) => {
                Some(constructors.clone())
            }
            _ => None,
        }
    }
    pub fn constructor(&self, name: &str) -> Result<(String, String, Vec<Field>)> {
        self.constructor_visible(name, |_| true)
    }
    pub(crate) fn constructor_visible(
        &self,
        name: &str,
        visible: impl Fn(&str) -> bool,
    ) -> Result<(String, String, Vec<Field>)> {
        let mut candidates = vec![];
        for ty in &self.types {
            if outcome_inner(ty.name()).is_some() || !visible(ty.name()) {
                continue;
            }
            match ty {
                TypeDef::Variant {
                    name: t,
                    constructors,
                } => {
                    for c in constructors {
                        if (name == c.name && crate::collection_support::generic(t).is_none())
                            || name == format!("{t}.{}", c.name)
                        {
                            candidates.push((t.clone(), c.name.clone(), c.fields.clone()));
                        }
                    }
                }
                TypeDef::Outcome {
                    name: t,
                    constructors,
                } => {
                    for c in constructors {
                        if name == format!("{t}.{}", c.name) {
                            candidates.push((t.clone(), c.name.clone(), c.fields.clone()));
                        }
                    }
                }
                TypeDef::Record { name: t, fields } if name == t => {
                    candidates.push((t.clone(), t.clone(), fields.clone()))
                }
                _ => {}
            }
        }
        if candidates.len() != 1 {
            return Err(error(format!("unknown or ambiguous constructor `{name}`")));
        }
        Ok(candidates.remove(0))
    }
    /// Qualified builtin constructors always work. Existing manifest constructors
    /// retain their meaning when their short names happen to be Some or None.
    pub fn option_constructor(&self, name: &str) -> Option<&'static str> {
        self.option_constructor_visible(name, |_| true)
    }
    pub(crate) fn option_constructor_visible(
        &self,
        name: &str,
        visible: impl Fn(&str) -> bool,
    ) -> Option<&'static str> {
        match name {
            "Option.Some" | "option.some" => Some("Some"),
            "Option.None" | "option.none" => Some("None"),
            "Some" | "None" | "some" | "none" => {
                let declared = self
                    .types
                    .iter()
                    .filter(|ty| {
                        !self.is_outcome(ty.name())
                            && crate::collection_support::generic(ty.name()).is_none()
                            && visible(ty.name())
                    })
                    .any(|ty| match ty {
                        TypeDef::Record { name: record, .. } => record == name,
                        TypeDef::Variant { constructors, .. }
                        | TypeDef::Outcome { constructors, .. } => {
                            constructors.iter().any(|c| c.name == name)
                        }
                        _ => false,
                    });
                if declared {
                    None
                } else if name == "Some" || name == "some" {
                    Some("Some")
                } else {
                    Some("None")
                }
            }
            _ => None,
        }
    }
    pub fn validate(&self) -> Result<()> {
        self.validate_at(&BTreeMap::new())
    }
    pub(crate) fn validate_at(&self, spans: &BTreeMap<String, Span>) -> Result<()> {
        if self.version != "0.1" {
            return Err(error("expected manifest version 0.1"));
        }
        let mut names = BTreeSet::new();
        for ty in &self.types {
            let specialization = spans.contains_key(ty.name())
                && crate::collection_support::generic(ty.name()).is_some_and(|(base, args)| {
                    identifier(base)
                        && !["outcome", "Option", "array", "vector", "map"].contains(&base)
                        && args.iter().all(|arg| self.valid_type(arg))
                });
            if (!identifier(ty.name()) && !specialization) || !names.insert(ty.name()) {
                let mut e = error(format!("invalid or duplicate name `{}`", ty.name()));
                e.span = spans.get(ty.name()).copied().unwrap_or_default();
                return Err(e);
            }
        }
        unique(self.primitives.iter().map(|p| p.name.as_str()))?;
        if self.primitives.iter().any(|p| {
            crate::collections::is_operation(&p.name) || crate::collections::is_length(&p.name)
        }) {
            return Err(error("cannot redeclare a collection builtin"));
        }
        let mut dependencies = BTreeMap::<String, Vec<String>>::new();
        for ty in &self.types {
            let result: Result<()> = (|| {
                if [
                    "Bool", "U32", "String", "Bytes", "Unit", "Option", "bool", "u32", "string",
                    "bytes", "unit", "option", "auto", "array", "vector", "map", "slice",
                    "outcome",
                ]
                .contains(&ty.name())
                {
                    return Err(error("cannot redeclare builtin type"));
                }
                let fields: Vec<&Field> = match ty {
                    TypeDef::Opaque { .. } => vec![],
                    TypeDef::Record { fields, .. } => {
                        unique(fields.iter().map(|f| f.name.as_str()))?;
                        fields.iter().collect()
                    }
                    TypeDef::Variant { constructors, .. }
                    | TypeDef::Outcome { constructors, .. } => {
                        if constructors.is_empty() {
                            return Err(error("variant must have constructors"));
                        }
                        unique(constructors.iter().map(|c| c.name.as_str()))?;
                        for c in constructors {
                            unique(c.fields.iter().map(|f| f.name.as_str()))?;
                        }
                        constructors.iter().flat_map(|c| c.fields.iter()).collect()
                    }
                };
                for f in &fields {
                    self.check_local_storage(&f.ty)?;
                }
                dependencies.insert(
                    ty.name().into(),
                    fields
                        .iter()
                        .flat_map(|f| {
                            fn leaves(t: &str) -> Vec<String> {
                                if let Some(inner) = option_inner(t) {
                                    return leaves(inner);
                                }
                                if let Some(c) = crate::collection_support::Collection::parse(t) {
                                    return c.types().iter().flat_map(|t| leaves(t)).collect();
                                }
                                vec![t.into()]
                            }
                            leaves(&f.ty)
                        })
                        .collect(),
                );
                Ok(())
            })();
            result.map_err(|mut e| {
                e.span = spans.get(ty.name()).copied().unwrap_or_default();
                e
            })?;
        }
        fn visit(
            name: &str,
            graph: &BTreeMap<String, Vec<String>>,
            visiting: &mut BTreeSet<String>,
            done: &mut BTreeSet<String>,
            spans: &BTreeMap<String, Span>,
        ) -> Result<()> {
            if done.contains(name) {
                return Ok(());
            }
            if !visiting.insert(name.into()) {
                let mut e = error(format!("recursive type `{name}` is unsupported"));
                e.span = spans.get(name).copied().unwrap_or_default();
                return Err(e);
            }
            if let Some(deps) = graph.get(name) {
                for dep in deps {
                    visit(dep, graph, visiting, done, spans)?;
                }
            }
            visiting.remove(name);
            done.insert(name.into());
            Ok(())
        }
        let mut done = BTreeSet::new();
        for name in dependencies.keys() {
            visit(name, &dependencies, &mut BTreeSet::new(), &mut done, spans)?;
        }
        for p in &self.primitives {
            if p.pure && pure_signature(p).is_none() {
                return Err(error(format!(
                    "`{}` has no supported pure expression contract (currently u32_text and concat)",
                    p.name
                )));
            }
            if !p.returns || p.outcomes.is_empty() {
                return Err(error(format!(
                    "primitive `{}` must return and declare outcomes",
                    p.name
                )));
            }
            unique(p.parameters.iter().map(|p| p.name.as_str()))?;
            unique(p.outcomes.iter().map(|o| o.name.as_str()))?;
            for param in &p.parameters {
                self.check_type(&param.ty)?;
                if self.linear(&param.ty) && param.mode == Mode::Value {
                    return Err(error("linear parameters require borrow or consume mode"));
                }
                if !self.linear(&param.ty) && param.mode == Mode::Consume && self.is_copy(&param.ty)
                {
                    return Err(error(
                        "Copy parameters use value or borrow mode; consume requires an owning type",
                    ));
                }
            }
            for o in &p.outcomes {
                unique(o.fields.iter().map(|f| f.name.as_str()))?;
                for f in &o.fields {
                    self.check_type(&f.ty)?;
                }
            }
        }
        Ok(())
    }
    pub fn contains_outcomes(&self, name: &str) -> bool {
        fn visit(m: &Manifest, t: &str, seen: &mut BTreeSet<String>) -> bool {
            if m.is_outcome(t) {
                return true;
            }
            if !seen.insert(t.into()) {
                return false;
            }
            if let Some(inner) = option_inner(t) {
                return visit(m, inner, seen);
            }
            if let Some(c) = crate::collection_support::Collection::parse(t) {
                return c.types().iter().any(|t| visit(m, t, seen));
            }
            match m.type_def(t) {
                Some(TypeDef::Record { fields, .. }) => {
                    fields.iter().any(|f| visit(m, &f.ty, seen))
                }
                Some(
                    TypeDef::Variant { constructors, .. } | TypeDef::Outcome { constructors, .. },
                ) => constructors
                    .iter()
                    .any(|o| o.fields.iter().any(|f| visit(m, &f.ty, seen))),
                _ => false,
            }
        }
        visit(self, name, &mut BTreeSet::new())
    }
    /// A borrowed decision lends payload storage; it never discharges handling.
    pub(crate) fn borrowed_outcomes(&self, reference: &str) -> Option<Vec<Outcome>> {
        let (mutable, inner) = reference_inner(reference)?;
        let mut outcomes = self.outcomes(inner)?;
        for outcome in &mut outcomes {
            for field in &mut outcome.fields {
                let (exclusive, inner) = reference_inner(&field.ty)
                    .map_or((mutable, field.ty.as_str()), |(m, t)| (mutable && m, t));
                field.ty = format!("&{}{}", if exclusive { "mutable " } else { "" }, inner);
            }
        }
        Some(outcomes)
    }
    pub fn contains_references(&self, name: &str) -> bool {
        fn visit(m: &Manifest, t: &str, seen: &mut BTreeSet<String>) -> bool {
            if reference_inner(t).is_some() {
                return true;
            }
            if !seen.insert(t.into()) {
                return false;
            }
            if let Some(inner) = option_inner(t) {
                return visit(m, inner, seen);
            }
            if let Some(c) = crate::collection_support::Collection::parse(t) {
                return c.types().iter().any(|t| visit(m, t, seen));
            }
            match m.type_def(t) {
                Some(TypeDef::Record { fields, .. }) => {
                    fields.iter().any(|f| visit(m, &f.ty, seen))
                }
                Some(
                    TypeDef::Variant { constructors, .. } | TypeDef::Outcome { constructors, .. },
                ) => constructors
                    .iter()
                    .any(|o| o.fields.iter().any(|f| visit(m, &f.ty, seen))),
                _ => false,
            }
        }
        visit(self, name, &mut BTreeSet::new())
    }
    pub fn check_type(&self, name: &str) -> Result<()> {
        if self.contains_references(name) {
            return Err(error(
                "stored reference-bearing outcomes/aggregates are not implemented; use an immediate exit delta adapter",
            ));
        }
        if self.valid_type(name) {
            Ok(())
        } else {
            Err(error(format!("unknown or unsupported type `{name}`")))
        }
    }
    /// Snapshot inferred types for the editor while anonymous/symbolic schemas
    /// still exist. Only anonymous structure is expanded; nominal layouts stay private.
    pub(crate) fn display_inferred_type(&self, ty: &str) -> String {
        fn format(m: &Manifest, ty: &str, depth: usize, remaining: &mut usize) -> String {
            if depth >= 64 || *remaining == 0 {
                return "…".into();
            }
            *remaining -= 1;
            if let Some((mutable, inner)) = reference_inner(ty) {
                return format!(
                    "ref {}{}",
                    if mutable { "mutable " } else { "" },
                    format(m, inner, depth + 1, remaining)
                );
            }
            if ty.starts_with("outcome<$adapter_")
                && let Some(outcomes) = m.outcomes(ty)
            {
                let descriptions = outcomes
                    .iter()
                    .map(|o| {
                        if o.fields.is_empty() {
                            return o.name.clone();
                        }
                        let fields = o
                            .fields
                            .iter()
                            .map(|f| {
                                format!("{} {}", format(m, &f.ty, depth + 1, remaining), f.name)
                            })
                            .collect::<Vec<_>>()
                            .join(", ");
                        format!("{} {{ {fields} }}", o.name)
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                return format!("outcome {{ {descriptions} }}");
            }
            if let Some((base, args)) = crate::collection_support::generic(ty) {
                let base = if base == "Option" { "option" } else { base };
                let args = args
                    .iter()
                    .map(|a| format(m, a, depth + 1, remaining))
                    .collect::<Vec<_>>()
                    .join(", ");
                return format!("{base}<{args}>");
            }
            display_type(ty)
        }
        format(self, ty, 0, &mut 512)
    }
    /// Finite local records and tagged values may nest borrowed payloads.
    /// Collection elements and public call boundaries keep their stricter gates.
    pub(crate) fn check_local_storage(&self, name: &str) -> Result<()> {
        fn visit(m: &Manifest, name: &str, seen: &mut BTreeSet<String>) -> Result<()> {
            if let Some((_, inner)) = reference_inner(name) {
                if reference_inner(inner).is_some() {
                    return m.check_type(inner);
                }
                return visit(m, inner, seen);
            }
            if !seen.insert(name.into()) {
                return Ok(()); // The layout validator reports recursive types.
            }
            if let Some(TypeDef::Record { fields, .. }) = m.type_def(name) {
                for field in fields {
                    visit(m, &field.ty, seen)?;
                }
            } else if let Some(outcomes) = m.outcomes(name) {
                for outcome in outcomes {
                    for field in outcome.fields {
                        visit(m, &field.ty, seen)?;
                    }
                }
            } else {
                m.check_type(name)?;
            }
            Ok(())
        }
        visit(self, name, &mut BTreeSet::new())
    }
    pub(crate) fn contains_exclusive_references(&self, ty: &str) -> bool {
        fn visit(m: &Manifest, ty: &str, seen: &mut BTreeSet<String>) -> bool {
            if let Some((mutable, _)) = reference_inner(ty) {
                return mutable;
            }
            if !seen.insert(ty.into()) {
                return false;
            }
            if let Some(TypeDef::Record { fields, .. }) = m.type_def(ty) {
                fields.iter().any(|f| visit(m, &f.ty, seen))
            } else if let Some(outcomes) = m.outcomes(ty) {
                outcomes
                    .iter()
                    .any(|o| o.fields.iter().any(|f| visit(m, &f.ty, seen)))
            } else {
                false
            }
        }
        visit(self, ty, &mut BTreeSet::new())
    }
}
pub fn option_inner(name: &str) -> Option<&str> {
    name.strip_prefix("Option<")?.strip_suffix('>')
}
