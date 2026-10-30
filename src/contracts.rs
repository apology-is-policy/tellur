//! Portable descriptions of checked public source contracts, not binary ABI or
//! behavioral-equivalence certificates. Bodies and diagnostic locations stay out.
use crate::{
    Diagnostic, Result, Span,
    collection_support::{Collection, generic, qualified_member},
    manifest::{self, Manifest, TypeDef},
    modules::SourceFile,
    syntax,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Component, Path},
};

pub const FORMAT: &str = "tellur.public-contracts";
pub const VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub format: String,
    pub version: u32,
    #[serde(deserialize_with = "crate::launch::unique_map")]
    pub modules: BTreeMap<String, Module>,
    #[serde(deserialize_with = "crate::launch::unique_map")]
    pub manifest_types: BTreeMap<String, Declaration>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Module {
    #[serde(deserialize_with = "crate::launch::unique_map")]
    pub declarations: BTreeMap<String, Declaration>,
    #[serde(deserialize_with = "crate::launch::unique_map")]
    pub implementations: BTreeMap<String, Implementation>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Field {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: String,
    pub origins: Vec<String>,
    pub referent_origins: Vec<String>,
    pub from: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Control {
    #[serde(default, skip_serializing_if = "crate::ir::is_false")]
    pub contingent: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retains: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "crate::ir::is_false")]
    pub faults: bool,
    pub profile: Profile,
    pub type_parameters: Vec<String>,
    #[serde(deserialize_with = "crate::launch::unique_map")]
    pub requirements: BTreeMap<String, Vec<String>>,
    pub parameters: Vec<Field>,
    pub exits: Exits,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Profile {
    Control,
    LegacyRegion,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Exits {
    Named {
        #[serde(rename = "type")]
        ty: String,
        origins: Vec<String>,
    },
    Inline {
        #[serde(deserialize_with = "crate::launch::unique_map")]
        alternatives: BTreeMap<String, Vec<Field>>,
    },
}
/// `None` means never; an empty requirement list means always. Otherwise each
/// named type parameter must provide the enclosing capability (copy/clone/drop).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Capabilities {
    pub copy: Option<BTreeSet<String>>,
    pub clone: Option<BTreeSet<String>>,
    pub drop: Option<BTreeSet<String>>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Declaration {
    Control(Control),
    Struct {
        type_parameters: Vec<String>,
        origin_parameters: Vec<String>,
        fields: Vec<Field>,
        constructible: bool,
        capabilities: Capabilities,
    },
    Variant {
        type_parameters: Vec<String>,
        origin_parameters: Vec<String>,
        #[serde(deserialize_with = "crate::launch::unique_map")]
        alternatives: BTreeMap<String, Vec<Field>>,
        capabilities: Capabilities,
    },
    Outcome {
        type_parameters: Vec<String>,
        origin_parameters: Vec<String>,
        #[serde(deserialize_with = "crate::launch::unique_map")]
        alternatives: BTreeMap<String, Vec<Field>>,
        #[serde(deserialize_with = "crate::launch::unique_map")]
        exit_codes: BTreeMap<String, u8>,
        capabilities: Capabilities,
    },
    Opaque {
        capabilities: Capabilities,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        fault: Option<crate::manifest::FaultDisposition>,
    },
    Trait {
        #[serde(deserialize_with = "crate::launch::unique_map")]
        associated: BTreeMap<String, Vec<String>>,
        #[serde(deserialize_with = "crate::launch::unique_map")]
        controls: BTreeMap<String, Control>,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Implementation {
    pub trait_name: String,
    pub target: String,
    #[serde(deserialize_with = "crate::launch::unique_map")]
    pub associated: BTreeMap<String, String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Change {
    pub kind: &'static str,
    /// RFC 6901 JSON pointer into the snapshot.
    pub path: String,
    pub before: Option<Value>,
    pub after: Option<Value>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Difference {
    pub format: &'static str,
    pub version: u32,
    pub equal: bool,
    pub changes: Vec<Change>,
}
fn error(message: impl Into<String>) -> Diagnostic {
    Diagnostic::new("E_CONTRACT_SNAPSHOT", Span { line: 1, column: 1 }, message)
}
impl Snapshot {
    pub fn from_json(text: &str) -> Result<Self> {
        let snapshot: Self = serde_json::from_str(text)
            .map_err(|e| error(format!("invalid contract snapshot: {e}")))?;
        snapshot.validate()?;
        Ok(snapshot)
    }
    pub(crate) fn validate(&self) -> Result<()> {
        if self.format != FORMAT || self.version != VERSION {
            return Err(error(format!("expected {FORMAT} version {VERSION}")));
        }
        if !self.modules.contains_key("@entry") {
            return Err(error("contract snapshot has no @entry module"));
        }
        Ok(())
    }
    /// Exact structural comparison, deliberately without a compatible/incompatible verdict.
    pub fn compare(&self, after: &Self) -> Result<Difference> {
        self.validate()?;
        after.validate()?;
        let changes = changes(
            &serde_json::to_value(self).unwrap(),
            &serde_json::to_value(after).unwrap(),
        );
        Ok(Difference {
            format: "tellur.public-contract-diff",
            version: VERSION,
            equal: changes.is_empty(),
            changes,
        })
    }
}

pub(crate) fn changes(before: &Value, after: &Value) -> Vec<Change> {
    fn walk(path: &str, before: &Value, after: &Value, changes: &mut Vec<Change>) {
        if before == after {
            return;
        }
        if let (Value::Object(a), Value::Object(b)) = (before, after) {
            for key in a.keys().chain(b.keys()).collect::<BTreeSet<_>>() {
                let path = format!("{path}/{}", key.replace('~', "~0").replace('/', "~1"));
                match (a.get(key), b.get(key)) {
                    (Some(a), Some(b)) => walk(&path, a, b, changes),
                    (a, b) => changes.push(Change {
                        kind: if a.is_none() { "added" } else { "removed" },
                        path,
                        before: a.cloned(),
                        after: b.cloned(),
                    }),
                }
            }
        } else {
            changes.push(Change {
                kind: "changed",
                path: path.into(),
                before: Some(before.clone()),
                after: Some(after.clone()),
            });
        }
    }
    let mut result = vec![];
    walk("", before, after, &mut result);
    result
}

pub(crate) fn relative(path: &Path, base: &Path) -> Result<String> {
    let a: Vec<_> = path.components().collect();
    let b: Vec<_> = base.components().collect();
    let shared = a.iter().zip(&b).take_while(|(a, b)| a == b).count();
    let mut parts = vec!["..".to_string(); b.len() - shared];
    for part in &a[shared..] {
        let Component::Normal(part) = part else {
            return Err(error("module paths must share a filesystem root"));
        };
        parts.push(
            part.to_str()
                .ok_or_else(|| error("snapshot module paths must be UTF-8"))?
                .into(),
        );
    }
    Ok(parts.join("/"))
}

struct Builder<'a> {
    unit: &'a syntax::Unit,
    manifest: &'a Manifest,
    names: BTreeMap<String, (String, String)>,
    dependencies: BTreeSet<String>,
    public: BTreeSet<String>,
}
struct PermissionSearch {
    active: BTreeSet<String>,
    memo: BTreeMap<String, Option<BTreeSet<String>>>,
    remaining: usize,
}
impl Default for PermissionSearch {
    fn default() -> Self {
        Self {
            active: BTreeSet::new(),
            memo: BTreeMap::new(),
            remaining: 20_000,
        }
    }
}
impl Builder<'_> {
    fn name(&mut self, name: &str, parameters: &[String]) -> String {
        if parameters.iter().any(|p| p == name) {
            return name.into();
        }
        if let Some((module, local)) = self.names.get(name) {
            return format!("{module}::{local}");
        }
        if self.manifest.type_def(name).is_some() {
            self.dependencies.insert(name.into());
            return format!("@manifest::{name}");
        }
        manifest::display_type(name)
    }
    fn ty(&mut self, ty: &str, parameters: &[String]) -> String {
        if let Some((mutable, inner)) = manifest::reference_inner(ty) {
            return format!(
                "ref {}{}",
                if mutable { "mutable " } else { "" },
                self.ty(inner, parameters)
            );
        }
        if let Some((application, member)) = qualified_member(ty) {
            return format!("{}.{member}", self.ty(application, parameters));
        }
        if let Some((base, args)) = generic(ty) {
            let base = if base == "Option" {
                "option".into()
            } else {
                self.name(base, parameters)
            };
            let args = args
                .iter()
                .map(|t| self.ty(t, parameters))
                .collect::<Vec<_>>()
                .join(", ");
            return format!("{base}<{args}>");
        }
        self.name(ty, parameters)
    }
    fn field(&mut self, field: &syntax::Field, parameters: &[String]) -> Field {
        Field {
            name: field.name.clone(),
            ty: self.ty(&field.ty, parameters),
            origins: field.origins.iter().map(|o| o.display()).collect(),
            referent_origins: field.referent_origins.iter().map(|o| o.display()).collect(),
            from: None,
        }
    }
    fn alternatives(
        &mut self,
        outcomes: &[syntax::Outcome],
        parameters: &[String],
    ) -> BTreeMap<String, Vec<Field>> {
        outcomes
            .iter()
            .map(|o| {
                (
                    o.name.clone(),
                    o.fields.iter().map(|f| self.field(f, parameters)).collect(),
                )
            })
            .collect()
    }
    fn control(&mut self, region: &syntax::Region, outer: &[String]) -> Control {
        let parameters: Vec<_> = outer
            .iter()
            .chain(&region.type_parameters)
            .cloned()
            .collect();
        let named = region.exit_contract.as_ref().filter(|name| {
            let base = generic(name).map_or(name.as_str(), |(base, _)| base);
            self.unit.contracts.iter().any(|c| c.name == base) || self.manifest.is_outcome(base)
        });
        let exits = if let Some(name) = named {
            Exits::Named {
                ty: self.ty(name, &parameters),
                origins: region.exit_origins.iter().map(|o| o.display()).collect(),
            }
        } else {
            let mut alternatives = self.alternatives(&region.exits, &parameters);
            for ((name, index), origin) in &region.return_origins {
                alternatives.get_mut(name).unwrap()[*index].from = Some(origin.clone());
            }
            Exits::Inline { alternatives }
        };
        Control {
            contingent: region.contingent_contract == Some(true),
            retains: region.boundary.as_ref().map(|b| b.retained.clone()),
            faults: region
                .boundary
                .as_ref()
                .map_or(region.fault_contract, |b| b.faults)
                .unwrap_or(false),
            profile: if region.modern {
                Profile::Control
            } else {
                Profile::LegacyRegion
            },
            type_parameters: region.type_parameters.clone(),
            requirements: region
                .type_bounds
                .iter()
                .map(|(p, bounds)| {
                    let sorted: BTreeSet<_> =
                        bounds.iter().map(|b| self.name(b, &parameters)).collect();
                    (p.clone(), sorted.into_iter().collect())
                })
                .collect(),
            parameters: region
                .parameters
                .iter()
                .map(|f| self.field(f, &parameters))
                .collect(),
            exits,
        }
    }
    fn permission(
        &self,
        ty: &str,
        bound: &str,
        parameters: &[String],
        active: &mut PermissionSearch,
    ) -> Result<Option<BTreeSet<String>>> {
        if active.remaining == 0 {
            return Err(error(
                "public capability expansion exceeds 20000 type visits",
            ));
        }
        active.remaining -= 1;
        if parameters.iter().any(|p| p == ty) {
            return Ok(Some(BTreeSet::from([ty.into()])));
        }
        if let Some((mutable, _)) = manifest::reference_inner(ty) {
            return Ok((bound == "drop" || !mutable).then(BTreeSet::new));
        }
        if let Some(permission) = active.memo.get(ty) {
            return Ok(permission.clone());
        }
        if active.active.len() >= 128 || !active.active.insert(ty.into()) {
            return Err(error(
                "public capability expansion exceeds its finite type budget",
            ));
        }
        let result = self.permission_inner(ty, bound, parameters, active);
        active.active.remove(ty);
        if let Ok(permission) = &result {
            active.memo.insert(ty.into(), permission.clone());
        }
        result
    }
    fn permission_inner(
        &self,
        ty: &str,
        bound: &str,
        parameters: &[String],
        active: &mut PermissionSearch,
    ) -> Result<Option<BTreeSet<String>>> {
        let (base, args) = generic(ty).map_or((ty, vec![]), |(base, args)| (base, args));
        if self.unit.contracts.iter().any(|c| c.name == base) || self.manifest.is_outcome(ty) {
            return Ok(None);
        }
        if let Some(c) = Collection::parse(ty) {
            if bound == "copy" && !matches!(c, Collection::Array(..)) {
                return Ok(None);
            }
            return self.permissions(c.types().into_iter(), bound, parameters, active);
        }
        if let Some(inner) = manifest::option_inner(ty) {
            return self.permission(inner, bound, parameters, active);
        }
        if let Some(data) = self.unit.data.iter().find(|d| d.name == base) {
            let fields: Vec<_> = match &data.body {
                syntax::DataBody::Struct(fields) => fields.iter().collect(),
                syntax::DataBody::Variant(outcomes) => {
                    outcomes.iter().flat_map(|o| &o.fields).collect()
                }
            };
            let bindings = data
                .type_parameters
                .iter()
                .cloned()
                .zip(args.iter().map(|a| a.to_string()))
                .collect();
            let types: Vec<_> = fields
                .iter()
                .map(|f| crate::generics::substitute(&f.ty, &bindings))
                .collect();
            return self.permissions(types.iter().map(String::as_str), bound, parameters, active);
        }
        Ok(self.manifest.has_capability(ty, bound).then(BTreeSet::new))
    }
    fn permissions<'t>(
        &self,
        types: impl Iterator<Item = &'t str>,
        bound: &str,
        parameters: &[String],
        active: &mut PermissionSearch,
    ) -> Result<Option<BTreeSet<String>>> {
        let mut all = BTreeSet::new();
        for ty in types {
            let Some(required) = self.permission(ty, bound, parameters, active)? else {
                return Ok(None);
            };
            all.extend(required);
        }
        Ok(Some(all))
    }
    fn capabilities(&self, ty: &str, parameters: &[String]) -> Result<Capabilities> {
        Ok(Capabilities {
            copy: self.permission(ty, "copy", parameters, &mut PermissionSearch::default())?,
            clone: self.permission(ty, "clone", parameters, &mut PermissionSearch::default())?,
            drop: self.permission(ty, "drop", parameters, &mut PermissionSearch::default())?,
        })
    }
    fn visible_type(&self, ty: &str) -> bool {
        if let Some((_, inner)) = manifest::reference_inner(ty) {
            return self.visible_type(inner);
        }
        if let Some((base, args)) = generic(ty) {
            return (self.public.contains(base) || !self.names.contains_key(base))
                && args.iter().all(|a| self.visible_type(a));
        }
        self.public.contains(ty) || !self.names.contains_key(ty)
    }
}

/// Called only after the same resolved unit has passed the ordinary checker.
pub(crate) fn checked(
    unit: &syntax::Unit,
    manifest: &Manifest,
    sources: &[SourceFile],
) -> Result<Snapshot> {
    // Match the ordinary checker's normalization even for API callers that
    // construct or modify a Manifest directly instead of loading JSON.
    let mut normalized = manifest.clone();
    normalized.canonicalize_types();
    let manifest = &normalized;
    let mut names = BTreeMap::new();
    let mut module_names = vec![];
    let base = sources[0].path.parent().unwrap();
    for (index, source) in sources.iter().enumerate() {
        let module = if index == 0 {
            "@entry".into()
        } else {
            format!("file:{}", relative(&source.path, base)?)
        };
        for (global, local) in &source.names {
            names.insert(global.clone(), (module.clone(), local.clone()));
        }
        module_names.push(module);
    }
    let public = unit
        .data
        .iter()
        .filter(|d| d.public)
        .map(|d| d.name.clone())
        .chain(
            unit.contracts
                .iter()
                .filter(|d| d.public)
                .map(|d| d.name.clone()),
        )
        .chain(
            unit.traits
                .iter()
                .filter(|d| d.public)
                .map(|d| d.name.clone()),
        )
        .collect();
    let mut b = Builder {
        unit,
        manifest,
        names,
        dependencies: BTreeSet::new(),
        public,
    };
    let mut snapshot = Snapshot {
        format: FORMAT.into(),
        version: VERSION,
        modules: BTreeMap::from([("@entry".into(), Module::default())]),
        manifest_types: BTreeMap::new(),
    };
    let mut insert = |name: &str, declaration, names: &BTreeMap<String, (String, String)>| {
        let (module, local) = &names[name];
        snapshot
            .modules
            .entry(module.clone())
            .or_default()
            .declarations
            .insert(local.clone(), declaration);
    };
    for region in unit.regions.iter().filter(|r| r.public) {
        let control = b.control(region, &[]);
        insert(&region.name, Declaration::Control(control), &b.names);
    }
    for data in unit.data.iter().filter(|d| d.public) {
        let ty = if data.type_parameters.is_empty() {
            data.name.clone()
        } else {
            format!("{}<{}>", data.name, data.type_parameters.join(","))
        };
        let capabilities = b.capabilities(&ty, &data.type_parameters)?;
        let declaration = match &data.body {
            syntax::DataBody::Struct(fields) => {
                let constructible = fields.iter().all(|f| data.public_fields.contains(&f.name));
                let mut fields: Vec<_> = fields
                    .iter()
                    .filter(|f| data.public_fields.contains(&f.name))
                    .map(|f| b.field(f, &data.type_parameters))
                    .collect();
                if !constructible {
                    fields.sort_by(|a, b| a.name.cmp(&b.name));
                }
                Declaration::Struct {
                    type_parameters: data.type_parameters.clone(),
                    origin_parameters: data.origin_parameters.clone(),
                    fields,
                    constructible,
                    capabilities,
                }
            }
            syntax::DataBody::Variant(outcomes) => Declaration::Variant {
                type_parameters: data.type_parameters.clone(),
                origin_parameters: data.origin_parameters.clone(),
                alternatives: b.alternatives(outcomes, &data.type_parameters),
                capabilities,
            },
        };
        insert(&data.name, declaration, &b.names);
    }
    for contract in unit.contracts.iter().filter(|c| c.public) {
        let declaration = Declaration::Outcome {
            type_parameters: contract.type_parameters.clone(),
            origin_parameters: contract.origin_parameters.clone(),
            alternatives: b.alternatives(&contract.outcomes, &contract.type_parameters),
            exit_codes: contract.exit_codes.clone(),
            capabilities: Capabilities {
                copy: None,
                clone: None,
                drop: None,
            },
        };
        insert(&contract.name, declaration, &b.names);
    }
    for declaration in unit.traits.iter().filter(|d| d.public) {
        let parameters: Vec<_> = std::iter::once("self".into())
            .chain(declaration.associated.iter().map(|a| a.name.clone()))
            .collect();
        let associated = declaration
            .associated
            .iter()
            .map(|a| {
                (
                    a.name.clone(),
                    a.bounds
                        .iter()
                        .cloned()
                        .collect::<BTreeSet<_>>()
                        .into_iter()
                        .collect(),
                )
            })
            .collect();
        let controls = declaration
            .methods
            .iter()
            .map(|r| (r.name.clone(), b.control(r, &parameters)))
            .collect();
        insert(
            &declaration.name,
            Declaration::Trait {
                associated,
                controls,
            },
            &b.names,
        );
    }
    for implementation in &unit.implementations {
        if !b.public.contains(&implementation.trait_name) || !b.visible_type(&implementation.target)
        {
            continue;
        }
        let trait_name = b.ty(&implementation.trait_name, &[]);
        let target = b.ty(&implementation.target, &[]);
        let associated = implementation
            .associated
            .iter()
            .map(|a| (a.name.clone(), b.ty(a.value.as_ref().unwrap(), &[])))
            .collect();
        snapshot
            .modules
            .entry(module_names[implementation.module].clone())
            .or_default()
            .implementations
            .insert(
                format!("{trait_name} for {target}"),
                Implementation {
                    trait_name,
                    target,
                    associated,
                },
            );
    }
    // Include only manifest types referenced by public surfaces, recursively.
    while let Some(name) = b.dependencies.pop_first() {
        if snapshot.manifest_types.contains_key(&name) {
            continue;
        }
        let capabilities = b.capabilities(&name, &[])?;
        let fields = |fields: &[manifest::Field], b: &mut Builder<'_>| {
            fields
                .iter()
                .map(|f| Field {
                    name: f.name.clone(),
                    ty: b.ty(&f.ty, &[]),
                    origins: vec![],
                    referent_origins: vec![],
                    from: None,
                })
                .collect()
        };
        let declaration = match manifest.type_def(&name).unwrap() {
            TypeDef::Opaque { fault, .. } => Declaration::Opaque {
                capabilities,
                fault: fault.clone(),
            },
            TypeDef::Record { fields: fs, .. } => Declaration::Struct {
                type_parameters: vec![],
                origin_parameters: vec![],
                fields: fields(fs, &mut b),
                constructible: true,
                capabilities,
            },
            TypeDef::Variant { constructors, .. } | TypeDef::Outcome { constructors, .. } => {
                let alternatives = constructors
                    .iter()
                    .map(|o| (o.name.clone(), fields(&o.fields, &mut b)))
                    .collect();
                if manifest.is_outcome(&name) {
                    Declaration::Outcome {
                        type_parameters: vec![],
                        origin_parameters: vec![],
                        alternatives,
                        exit_codes: BTreeMap::new(),
                        capabilities,
                    }
                } else {
                    Declaration::Variant {
                        type_parameters: vec![],
                        origin_parameters: vec![],
                        alternatives,
                        capabilities,
                    }
                }
            }
        };
        snapshot.manifest_types.insert(name, declaration);
    }
    Ok(snapshot)
}
