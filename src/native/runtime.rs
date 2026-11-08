#[cfg(test)]
#[path = "execution.rs"]
pub mod execution;
#[cfg(test)]
#[path = "frames.rs"]
mod frames;
#[cfg(test)]
#[path = "tasks.rs"]
mod tasks;
pub use frames::Frames;
#[cfg(test)]
#[path = "boundary_state_tests.rs"]
mod boundary_state_tests;
#[cfg(test)]
#[path = "../collection_support.rs"]
mod collection_support;
#[cfg(test)]
#[path = "context_state_tests.rs"]
mod context_state_tests;
#[cfg(test)]
#[path = "../fault.rs"]
mod fault;
#[cfg(test)]
#[path = "resource_scope_tests.rs"]
mod resource_scope_tests;
#[cfg(test)]
#[path = "result_adoption_tests.rs"]
mod result_adoption_tests;
// Embedded in generated programs. This runtime contains values and primitive
// adapters only; region execution is generated as native Rust control flow.
use execution::reference_evidence::ReferenceEvidence;
use serde::{Deserialize, Serialize};
use serde_json::{Value as Json, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::{CString, c_char, c_int, c_void},
    sync::Arc,
};
pub type R<T> = Result<T, String>;
pub type Outcome = (String, Vec<V>);
pub enum Completion {
    /// Internal constructor delivery; the completed value is in caller storage.
    Constructed,
    Process(u8),
    Outcome(Outcome),
    Settled,
    Again(usize, Vec<V>),
    Recover(usize, Vec<V>),
    Complete(usize),
    Fallthrough,
}
impl Completion {
    pub fn constructed(self) -> R<()> {
        match self {
            Self::Constructed => Ok(()),
            _ => Err("E_NATIVE: constructor did not initialize its destination".into()),
        }
    }

    pub fn outcome(self) -> R<Outcome> {
        match self {
            Self::Outcome(value) => Ok(value),
            _ => Err(
                "E_NATIVE: administrative settlement cannot enter an outcome continuation".into(),
            ),
        }
    }
    pub fn settled(self) -> R<()> {
        match self {
            Self::Settled => Ok(()),
            _ => Err("E_NATIVE: supervised boundary returned an ordinary outcome".into()),
        }
    }
}
fn snake_name(name: &str) -> String {
    let mut out = String::new();
    for (i, c) in name.chars().enumerate() {
        if i > 0 && c.is_ascii_uppercase() {
            out.push('_');
        }
        out.push(c.to_ascii_lowercase());
    }
    out
}
pub fn pure(name: &str, args: Vec<V>) -> R<V> {
    match (name, args.as_slice()) {
        ("u32_text", [V::U32(n)]) => Ok(V::String(n.to_string())),
        ("concat", [V::String(a), V::String(b)]) => Ok(V::String(format!("{a}{b}"))),
        _ => Err("E_NATIVE: invalid pure expression contract".into()),
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "lowercase",
    deny_unknown_fields
)]
pub enum V {
    Bool(bool),
    U32(u32),
    String(String),
    Bytes(Vec<u8>),
    Unit,
    #[serde(skip)]
    Reference {
        evidence: ReferenceEvidence,
        ty: String,
        root: usize,
        fields: Vec<usize>,
    },
    #[serde(skip)]
    Task {
        binding: Option<execution::TaskBinding>,
        ty: String,
        operation: String,
        arguments: Vec<V>,
    },
    Opaque {
        ty: String,
        token: String,
    },
    Construct {
        ty: String,
        name: String,
        fields: Vec<V>,
    },
}
// Descriptive provenance must not change value comparisons.
impl PartialEq for V {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Bool(a), Self::Bool(b)) => a == b,
            (Self::U32(a), Self::U32(b)) => a == b,
            (Self::String(a), Self::String(b)) => a == b,
            (Self::Bytes(a), Self::Bytes(b)) => a == b,
            (Self::Unit, Self::Unit) => true,
            (
                Self::Reference {
                    ty: a,
                    root: ar,
                    fields: af,
                    ..
                },
                Self::Reference {
                    ty: b,
                    root: br,
                    fields: bf,
                    ..
                },
            ) => a == b && ar == br && af == bf,
            (
                Self::Task {
                    binding: a,
                    ty: at,
                    operation: ao,
                    arguments: aa,
                },
                Self::Task {
                    binding: b,
                    ty: bt,
                    operation: bo,
                    arguments: ba,
                },
            ) => a == b && at == bt && ao == bo && aa == ba,
            (Self::Opaque { ty: a, token: av }, Self::Opaque { ty: b, token: bv }) => {
                a == b && av == bv
            }
            (
                Self::Construct {
                    ty: a,
                    name: an,
                    fields: af,
                },
                Self::Construct {
                    ty: b,
                    name: bn,
                    fields: bf,
                },
            ) => a == b && an == bn && af == bf,
            _ => false,
        }
    }
}
impl Eq for V {}
impl V {
    pub fn ty(&self) -> &str {
        match self {
            Self::Bool(_) => "Bool",
            Self::U32(_) => "U32",
            Self::String(_) => "String",
            Self::Bytes(_) => "Bytes",
            Self::Unit => "Unit",
            Self::Task { ty, .. }
            | Self::Opaque { ty, .. }
            | Self::Construct { ty, .. }
            | Self::Reference { ty, .. } => ty,
        }
    }
}
pub fn await_arguments(mut values: Vec<V>, expected: &str) -> R<Vec<V>> {
    match values.pop() {
        Some(V::Task {
            binding: None,
            operation,
            arguments,
            ..
        }) if values.is_empty() && operation == expected => Ok(arguments),
        _ => Err("E_NATIVE: invalid pending task".into()),
    }
}
pub fn boolean(v: V) -> R<bool> {
    if let V::Bool(v) = v {
        Ok(v)
    } else {
        Err("E_NATIVE: expected Bool".into())
    }
}
pub fn project(v: &V, fields: &[usize]) -> R<V> {
    let mut v = v;
    for index in fields {
        let V::Construct { fields, .. } = v else {
            return Err("E_NATIVE: invalid field projection".into());
        };
        v = fields.get(*index).ok_or("E_NATIVE: invalid field index")?;
    }
    Ok(v.clone())
}
/// The expression evaluator already owns its temporary; transfer the selected
/// field without cloning its allocation. Static checking excludes obligations
/// in discarded sibling fields.
pub fn project_owned(value: V, index: usize) -> R<V> {
    let V::Construct { fields, .. } = value else {
        return Err("E_NATIVE: invalid field projection".into());
    };
    fields
        .into_iter()
        .nth(index)
        .ok_or_else(|| "E_NATIVE: invalid field index".into())
}
pub fn address(
    values: &[Option<V>],
    root: usize,
    fields: &[usize],
    indirect: bool,
) -> R<(usize, Vec<usize>)> {
    if !indirect {
        return Ok((root, fields.to_vec()));
    }
    let V::Reference {
        root, fields: base, ..
    } = slot(values, root)?
    else {
        return Err("E_NATIVE: invalid reference".into());
    };
    let mut result = base.clone();
    result.extend_from_slice(fields);
    Ok((*root, result))
}
pub fn reference(values: &Frames, root: usize, fields: &[usize], indirect: bool, ty: &str) -> R<V> {
    let evidence = ReferenceEvidence::from_place(values, root, fields, indirect);
    let (root, fields) = address(values, root, fields, indirect)?;
    Ok(V::Reference {
        evidence,
        ty: ty.into(),
        root,
        fields,
    })
}
pub fn read_place(values: &[Option<V>], root: usize, fields: &[usize], indirect: bool) -> R<V> {
    let (root, fields) = address(values, root, fields, indirect)?;
    project(slot(values, root)?, &fields)
}
pub fn take_place(values: &mut [Option<V>], root: usize, fields: &[usize], indirect: bool) -> R<V> {
    let (root, fields) = address(values, root, fields, indirect)?;
    take(values, root, &fields)
}
pub fn assign_place(
    values: &mut [Option<V>],
    root: usize,
    fields: &[usize],
    indirect: bool,
    value: V,
) -> R<()> {
    let (root, fields) = address(values, root, fields, indirect)?;
    assign(values, root, &fields, value)
}
pub fn mutable_place<'a>(
    values: &'a mut [Option<V>],
    root: usize,
    fields: &[usize],
) -> R<&'a mut V> {
    let mut value = values
        .get_mut(root)
        .and_then(Option::as_mut)
        .ok_or("E_NATIVE: unavailable place")?;
    for index in fields {
        let V::Construct { fields, .. } = value else {
            return Err("E_NATIVE: invalid mutable projection".into());
        };
        value = fields
            .get_mut(*index)
            .ok_or("E_NATIVE: invalid field index")?;
    }
    Ok(value)
}
pub fn take(values: &mut [Option<V>], root: usize, fields: &[usize]) -> R<V> {
    if fields.is_empty() {
        values
            .get_mut(root)
            .and_then(Option::take)
            .ok_or_else(|| "E_NATIVE: unavailable moved value".into())
    } else {
        Ok(std::mem::replace(
            mutable_place(values, root, fields)?,
            V::Unit,
        ))
    }
}
pub fn assign(values: &mut [Option<V>], root: usize, fields: &[usize], value: V) -> R<()> {
    if fields.is_empty() {
        *values.get_mut(root).ok_or("E_NATIVE: invalid assignment")? = Some(value);
    } else {
        *mutable_place(values, root, fields)? = value;
    }
    Ok(())
}
pub fn slot(values: &[Option<V>], id: usize) -> R<&V> {
    values
        .get(id)
        .and_then(Option::as_ref)
        .ok_or_else(|| format!("E_NATIVE: uninitialized slot {id}"))
}
pub fn borrowed_variant(values: &[Option<V>], reference: V) -> R<Outcome> {
    let V::Reference {
        ty,
        root,
        fields,
        evidence,
    } = reference
    else {
        return Err("E_NATIVE: invalid borrowed decision reference".into());
    };
    let mutable = ty.starts_with("&mutable ");
    match project(slot(values, root)?, &fields)? {
        V::Bool(b) => Ok((b.to_string(), vec![])),
        V::Construct {
            name,
            fields: payloads,
            ..
        } => {
            let payloads = payloads
                .iter()
                .enumerate()
                .map(|(i, value)| {
                    if let V::Reference {
                        ty,
                        root,
                        fields,
                        evidence: contained,
                    } = value
                    {
                        let (exclusive, inner) = if let Some(inner) = ty.strip_prefix("&mutable ") {
                            (mutable, inner)
                        } else {
                            (false, ty.strip_prefix('&').expect("reference type"))
                        };
                        let mut inherited = contained.clone();
                        inherited.origins.extend(evidence.origins.clone());
                        V::Reference {
                            evidence: inherited,
                            ty: format!("&{}{}", if exclusive { "mutable " } else { "" }, inner),
                            root: *root,
                            fields: fields.clone(),
                        }
                    } else {
                        let mut path = fields.clone();
                        path.push(i);
                        V::Reference {
                            evidence: evidence.projected(&[i]),
                            ty: format!("&{}{}", if mutable { "mutable " } else { "" }, value.ty()),
                            root,
                            fields: path,
                        }
                    }
                })
                .collect();
            Ok((name, payloads))
        }
        _ => Err("E_NATIVE: invalid borrowed decision storage".into()),
    }
}
pub fn variant(v: V) -> R<Outcome> {
    match v {
        V::Bool(b) => Ok((b.to_string(), vec![])),
        V::Construct { name, fields, .. } => Ok((name, fields)),
        _ => Err("E_NATIVE: invalid decision value".into()),
    }
}
pub fn binary(op: &str, a: V, b: V) -> R<V> {
    let b = match (op, a, b) {
        ("==", a, b) => a == b,
        ("!=", a, b) => a != b,
        ("<", V::U32(a), V::U32(b)) => a < b,
        ("<=", V::U32(a), V::U32(b)) => a <= b,
        (">", V::U32(a), V::U32(b)) => a > b,
        (">=", V::U32(a), V::U32(b)) => a >= b,
        _ => return Err("E_NATIVE: invalid binary operation".into()),
    };
    Ok(V::Bool(b))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScriptCall {
    operation: String,
    outcome: String,
    fields: Vec<V>,
    /// Scripted readiness of an async host operation: pending reports before completion.
    #[serde(default)]
    waits: u32,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scenario {
    pub entry: String,
    pub arguments: Vec<V>,
    calls: Vec<ScriptCall>,
}
fn fields(m: &Json, ty: &str, constructor: &str) -> R<Vec<String>> {
    if let Some(inner) = ty.strip_prefix("Option<").and_then(|s| s.strip_suffix('>')) {
        return match constructor {
            "Some" => Ok(vec![inner.into()]),
            "None" => Ok(vec![]),
            _ => Err("E_DRIVER: invalid option constructor".into()),
        };
    }
    let t = m["types"]
        .as_array()
        .and_then(|types| types.iter().find(|t| t["name"] == ty))
        .ok_or("E_DRIVER: unknown aggregate type")?;
    let f = if t["kind"] == "record" && constructor == ty {
        &t["fields"]
    } else {
        &t["constructors"]
            .as_array()
            .and_then(|cs| cs.iter().find(|c| c["name"] == constructor))
            .ok_or("E_DRIVER: invalid constructor")?["fields"]
    };
    Ok(f.as_array()
        .ok_or("E_DRIVER: invalid aggregate")?
        .iter()
        .map(|f| f["type"].as_str().unwrap().into())
        .collect())
}
pub fn validate(v: &V, expected: &str, m: &Json) -> R<()> {
    if v.ty() != expected {
        return Err(format!("E_DRIVER: expected {expected}, found {}", v.ty()));
    }
    match v {
        V::Opaque { ty, .. } => {
            if !m["types"]
                .as_array()
                .unwrap()
                .iter()
                .any(|t| t["name"] == *ty && t["kind"] == "opaque")
            {
                return Err("E_DRIVER: invalid opaque type".into());
            }
        }
        V::Construct {
            ty,
            name,
            fields: values,
        } => {
            if let Some(c) = collection_support::Collection::parse(ty) {
                if name != c.name() {
                    return Err("E_DRIVER: invalid collection constructor".into());
                }
                let types = c
                    .fields(values.len())
                    .ok_or("E_DRIVER: invalid collection length")?;
                validate_args(values, &types, m)?;
                if matches!(c, collection_support::Collection::Map(..))
                    && !collection_support::sorted_map(values)
                {
                    return Err("E_DRIVER: map keys must be unique and sorted".into());
                }
                return Ok(());
            }
            let expected = fields(m, ty, name)?;
            validate_args(values, &expected, m)?;
        }
        _ => {}
    }
    Ok(())
}
pub fn validate_args(values: &[V], types: &[String], m: &Json) -> R<()> {
    if values.len() != types.len() {
        return Err("E_DRIVER: argument/payload arity mismatch".into());
    }
    for (v, t) in values.iter().zip(types) {
        validate(v, t, m)?;
    }
    Ok(())
}
unsafe extern "C" {
    fn fopen(path: *const c_char, mode: *const c_char) -> *mut c_void;
    fn fread(ptr: *mut c_void, size: usize, count: usize, file: *mut c_void) -> usize;
    fn ferror(file: *mut c_void) -> c_int;
    fn fclose(file: *mut c_void) -> c_int;
}
/// A live resource scope, unrelated to invocation or diagnostic identities.
#[derive(Clone, Debug)]
struct ResourceScope {
    registry: Arc<()>,
    id: u64,
    parent: Option<u64>,
}
struct PreparedResourceScope {
    scope: ResourceScope,
    owned: BTreeSet<String>,
}
struct Resource {
    ty: &'static str,
    value: Box<dyn std::any::Any>,
    scope: Option<u64>,
}
/// Host registry for activation-owned objects. Tokens never clone a Rust owner.
/// Dropping the host releases remaining objects; fallible protocol completion
/// still belongs to explicit consuming adapter operations.
#[derive(Default)]
pub struct Resources {
    values: BTreeMap<String, Resource>,
    identity: Arc<()>,
    next_scope: u64,
    current: Option<u64>,
    scopes: BTreeMap<u64, Option<u64>>,
}
impl Resources {
    pub fn insert<T: 'static>(&mut self, ty: &'static str, value: T) -> R<V> {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        // Process-wide identities prevent reuse even across separate hosts.
        let id = NEXT
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .map_err(|_| "E_HOST: resource identity exhausted")?;
        let token = format!("rust:{id}");
        self.values.insert(
            token.clone(),
            Resource {
                ty,
                value: Box::new(value),
                scope: self.current,
            },
        );
        Ok(V::Opaque {
            ty: ty.into(),
            token,
        })
    }
    fn token<'a>(&self, value: &'a V, expected: &str) -> R<&'a str> {
        let V::Opaque { ty, token } = value else {
            return Err("E_HOST: expected resource handle".into());
        };
        if ty != expected {
            return Err("E_HOST: resource type mismatch".into());
        }
        let actual = self
            .values
            .get(token)
            .ok_or("E_HOST: unknown or consumed resource handle")?;
        if actual.ty != expected {
            return Err("E_HOST: resource identity/type mismatch".into());
        }
        Ok(token)
    }
    pub fn get<T: 'static>(&self, value: &V, ty: &str) -> R<&T> {
        let token = self.token(value, ty)?;
        self.values[token]
            .value
            .downcast_ref()
            .ok_or_else(|| "E_HOST: Rust resource type mismatch".into())
    }
    pub fn take<T: 'static>(&mut self, value: &V, ty: &str) -> R<T> {
        self.get::<T>(value, ty)?;
        let V::Opaque { token, .. } = value else {
            unreachable!()
        };
        let resource = self.values.remove(token).unwrap();
        Ok(*resource
            .value
            .downcast::<T>()
            .map_err(|_| "E_HOST: Rust resource type mismatch")?)
    }
    /// Check all handle identities and ownership aliases before any removal/call.
    pub fn validate_call(&self, inputs: &[(&V, &str, bool)]) -> R<()> {
        let mut seen = BTreeMap::new();
        for (value, ty, consume) in inputs {
            let token = self.token(value, ty)?;
            if let Some(previous) = seen.insert(token, *consume)
                && (previous || *consume)
            {
                return Err("E_HOST: aliased consuming resource arguments".into());
            }
        }
        Ok(())
    }
    /// Collect registered owned leaves, never following borrowed references.
    /// Nonregistered opaque script/builtin values have no Rust owner to adopt.
    fn owned_tokens<'a>(&self, values: impl IntoIterator<Item = &'a V>) -> R<BTreeSet<String>> {
        fn collect(r: &Resources, v: &V, into: &mut BTreeSet<String>) -> R<()> {
            match v {
                V::Opaque { ty, token } => {
                    if let Some(resource) = r.values.get(token) {
                        if resource.ty != ty {
                            return Err("E_HOST: resource identity/type mismatch".into());
                        }
                        into.insert(token.clone());
                    }
                }
                V::Construct { fields, .. }
                | V::Task {
                    arguments: fields, ..
                } => {
                    for value in fields {
                        collect(r, value, into)?;
                    }
                }
                _ => {}
            }
            Ok(())
        }
        let mut owned = BTreeSet::new();
        for value in values {
            collect(self, value, &mut owned)?;
        }
        Ok(owned)
    }
    /// A completed result must name each registered owner exactly once. Rust
    /// resource identities from another host or a retired object are never raw
    /// opaque script values; borrowed references are intentionally not followed.
    fn result_tokens(&self, values: &[V]) -> R<BTreeSet<String>> {
        fn collect(r: &Resources, value: &V, owned: &mut BTreeSet<String>) -> R<()> {
            match value {
                V::Opaque { ty, token } => match r.values.get(token) {
                    Some(resource) => {
                        if resource.ty != ty {
                            return Err("E_HOST: result resource identity/type mismatch".into());
                        }
                        if !owned.insert(token.clone()) {
                            return Err("E_HOST: duplicate owner in completed result".into());
                        }
                    }
                    None if token.starts_with("rust:") => {
                        return Err("E_HOST: result resource is foreign or retired".into());
                    }
                    None => {}
                },
                V::Construct { fields, .. }
                | V::Task {
                    arguments: fields, ..
                } => {
                    for field in fields {
                        collect(r, field, owned)?;
                    }
                }
                _ => {}
            }
            Ok(())
        }
        let mut owned = BTreeSet::new();
        for value in values {
            collect(self, value, &mut owned)?;
        }
        Ok(owned)
    }
    fn prepare_scope<'a>(
        &self,
        values: impl IntoIterator<Item = &'a V>,
    ) -> R<PreparedResourceScope> {
        self.prepare_scope_at(values, self.current)
    }
    fn prepare_scope_at<'a>(
        &self,
        values: impl IntoIterator<Item = &'a V>,
        parent: Option<u64>,
    ) -> R<PreparedResourceScope> {
        if parent.is_some_and(|id| !self.scopes.contains_key(&id)) {
            return Err("E_HOST: active resource scope has retired".into());
        }
        let owned = self.owned_tokens(values)?;
        let id = self
            .next_scope
            .checked_add(1)
            .ok_or("E_HOST: resource scope identity exhausted")?;
        Ok(PreparedResourceScope {
            scope: ResourceScope {
                registry: self.identity.clone(),
                id,
                parent,
            },
            owned,
        })
    }
    fn commit_scope(&mut self, prepared: PreparedResourceScope) -> ResourceScope {
        assert_eq!(self.current, prepared.scope.parent);
        let scope = self.register_scope(prepared);
        self.current = Some(scope.id);
        scope
    }
    /// Register dormant independent ownership without selecting its execution.
    fn register_scope(&mut self, prepared: PreparedResourceScope) -> ResourceScope {
        let PreparedResourceScope { scope, owned } = prepared;
        assert!(Arc::ptr_eq(&self.identity, &scope.registry));
        assert!(scope.parent.is_none_or(|id| self.scopes.contains_key(&id)));
        assert_eq!(self.next_scope.checked_add(1), Some(scope.id));
        self.next_scope = scope.id;
        assert!(self.scopes.insert(scope.id, scope.parent).is_none());
        for token in owned {
            self.values.get_mut(&token).unwrap().scope = Some(scope.id);
        }
        scope
    }
    fn current_scope(&self) -> R<Option<ResourceScope>> {
        self.current
            .map(|id| {
                let parent = *self
                    .scopes
                    .get(&id)
                    .ok_or("E_HOST: selected resource scope has retired")?;
                Ok(ResourceScope {
                    registry: self.identity.clone(),
                    id,
                    parent,
                })
            })
            .transpose()
    }
    fn require_scope(&self, scope: &ResourceScope) -> R<()> {
        if !Arc::ptr_eq(&self.identity, &scope.registry)
            || self.scopes.get(&scope.id) != Some(&scope.parent)
        {
            return Err("E_HOST: resource scope is foreign or retired".into());
        }
        Ok(())
    }
    fn require_quiescent_scope(&self, scope: &ResourceScope) -> R<()> {
        self.require_scope(scope)?;
        if self.current != Some(scope.id)
            || self.scopes.values().any(|parent| *parent == Some(scope.id))
        {
            return Err("E_HOST: resource scope still has active nested work".into());
        }
        Ok(())
    }
    /// Internal context selection, not a source handoff or a cleanup operation.
    #[allow(dead_code)]
    fn select_scope(&mut self, scope: Option<&ResourceScope>) -> R<()> {
        if let Some(scope) = scope {
            self.require_scope(scope)?;
        }
        self.current = scope.map(|scope| scope.id);
        Ok(())
    }
    fn release_scope_except(&mut self, scope: &ResourceScope, keep: &BTreeSet<String>) -> R<()> {
        self.require_quiescent_scope(scope)?;
        self.values
            .retain(|token, resource| resource.scope != Some(scope.id) || keep.contains(token));
        Ok(())
    }
    fn finish_scope(&mut self, scope: &ResourceScope, release: bool) -> R<()> {
        self.require_quiescent_scope(scope)?;
        if release {
            self.values
                .retain(|_, resource| resource.scope != Some(scope.id));
        } else {
            // Normal lexical exits can deliver owned outcomes and cold tasks.
            for resource in self
                .values
                .values_mut()
                .filter(|r| r.scope == Some(scope.id))
            {
                resource.scope = scope.parent;
            }
        }
        self.scopes.remove(&scope.id);
        self.current = scope.parent;
        Ok(())
    }
}
pub type RustAdapter = fn(&mut Resources, &str, Vec<V>) -> R<Outcome>;
/// Starts a Rust-bound async host operation from its validated arguments.
pub type RustStart = fn(&mut Resources, &str, Vec<V>) -> R<Box<dyn HostPoll>>;
/// A started live async host operation. Adapter state is dropped on completion or
/// when its owning context stops, before any supplier resource retires.
pub trait HostPoll {
    fn poll(&mut self, resources: &mut Resources) -> R<HostProgress>;
}
pub enum HostProgress {
    Ready(Outcome),
    /// Poll again after the delay, when the descriptor is ready, or both.
    Pending {
        delay: Option<std::time::Duration>,
        descriptor: Option<Descriptor>,
    },
}
/// One descriptor a pending operation waits on, for reading or for writing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Descriptor {
    pub fd: std::os::fd::RawFd,
    pub writable: bool,
}
/// Internal descriptor readiness for live host operations (Mio in generated crates).
pub trait ReadinessBackend {
    fn register(&mut self, token: u64, descriptor: Descriptor) -> R<()>;
    fn deregister(&mut self, fd: std::os::fd::RawFd) -> R<()>;
    /// Tokens that became readable, after at most `timeout` (None waits indefinitely).
    fn wait(&mut self, timeout: Option<std::time::Duration>) -> R<Vec<u64>>;
}
pub type RustReadiness = fn() -> R<Option<Box<dyn ReadinessBackend>>>;
/// How a started async host operation completes.
pub enum HostStart {
    Scripted {
        outcome: String,
        fields: Vec<V>,
        waits: u32,
    },
    Live(Box<dyn HostPoll>),
}
/// Retained boundary lifecycle; no value arena or executable continuation is cloned.
/// The currently executing tracker remains in Host until a context handoff exists.
pub struct BoundaryState {
    name: String,
    parent: Option<fault::Tracker>,
    scope: ResourceScope,
    fault_message: Option<String>,
    lexical: bool,
    contingency_active: bool,
    contingency_depth: usize,
}
/// Execution-local host state. Resource owners, effects, input cursor and fuel
/// remain in Host. This record is moved, never cloned, during context selection.
pub struct HostContext {
    registry: Arc<()>,
    tracker: fault::Tracker,
    contingencies: Vec<String>,
    scope: Option<ResourceScope>,
    /// The task this context executes; a root context has none.
    task: Option<tasks::TaskId>,
}
/// A live independent ownership root, distinct from diagnostics and frame IDs.
pub struct ContextRoot {
    scope: ResourceScope,
    retired: bool,
}
pub struct Host {
    contingencies: Vec<String>,
    pub rust_adapter: Option<RustAdapter>,
    pub rust_start: Option<RustStart>,
    pub readiness: Option<Box<dyn ReadinessBackend>>,
    pub next_readiness_token: u64,
    pub manifest: Json,
    script: Option<Vec<ScriptCall>>,
    cursor: usize,
    events: Vec<Json>,
    pub trace: bool,
    pub fuel: Option<usize>,
    files: BTreeMap<String, *mut c_void>,
    next_file: usize,
    resources: Resources,
    tasks: tasks::Registry,
    activations: fault::Tracker,
    /// The task of the selected context, exchanged with its host context.
    current_task: Option<tasks::TaskId>,
}
impl Drop for Host {
    fn drop(&mut self) {
        for (_, file) in std::mem::take(&mut self.files) {
            unsafe {
                fclose(file);
            }
        }
    }
}
impl Host {
    pub fn new(
        manifest: Json,
        script: Option<Vec<ScriptCall>>,
        trace: bool,
        fuel: Option<usize>,
    ) -> Self {
        Self {
            rust_adapter: None,
            rust_start: None,
            readiness: None,
            next_readiness_token: 0,
            manifest,
            script,
            cursor: 0,
            events: vec![],
            trace,
            fuel,
            files: BTreeMap::new(),
            next_file: 0,
            resources: Resources::default(),
            tasks: tasks::Registry::default(),
            contingencies: vec![],
            activations: fault::Tracker::default(),
            current_task: None,
        }
    }
    pub fn enter(&mut self, control: &str) -> R<()> {
        self.activations.enter(control)
    }
    /// Admit a dormant independent root without changing the selected context.
    /// Owned inputs transfer scope membership; borrowed inputs do not.
    pub fn prepare_context<'a>(
        &mut self,
        inputs: impl IntoIterator<Item = &'a V>,
    ) -> R<(ContextRoot, HostContext)> {
        self.resources.current_scope()?;
        let prepared = self.resources.prepare_scope_at(inputs, None)?;
        let tracker = self.activations.child();
        let scope = self.resources.register_scope(prepared);
        let context = HostContext {
            registry: self.resources.identity.clone(),
            tracker,
            contingencies: vec![],
            scope: Some(scope.clone()),
            task: None,
        };
        Ok((
            ContextRoot {
                scope,
                retired: false,
            },
            context,
        ))
    }
    /// Validate a parked context against its original independent ownership root.
    /// A scheduler must do this before selection, even for a zero instruction quota.
    pub fn require_context(&self, context: &HostContext, root: &ContextRoot) -> R<()> {
        if root.retired || !Arc::ptr_eq(&context.registry, &self.resources.identity) {
            return Err("E_NATIVE: context is foreign or retired".into());
        }
        self.resources.require_scope(&root.scope)?;
        let scope = context
            .scope
            .as_ref()
            .ok_or("E_NATIVE: context has no resource scope")?;
        self.resources.require_scope(scope)?;
        let mut ancestor = Some(scope.id);
        while let Some(id) = ancestor {
            if id == root.scope.id {
                return Ok(());
            }
            ancestor = *self
                .resources
                .scopes
                .get(&id)
                .ok_or("E_NATIVE: context resource ancestor has retired")?;
        }
        Err("E_NATIVE: selected resource scope belongs to another context".into())
    }
    /// Select and later restore a context by exchanging the same record. Both
    /// scopes and host ownership are checked before any field changes. The caller
    /// must keep exclusive access to Host until restoring its prior context.
    pub fn exchange_context(&mut self, context: &mut HostContext) -> R<()> {
        if !Arc::ptr_eq(&context.registry, &self.resources.identity) {
            return Err("E_NATIVE: context belongs to another host".into());
        }
        let previous = self.resources.current_scope()?;
        if let Some(scope) = &context.scope {
            self.resources.require_scope(scope)?;
        }
        self.resources.select_scope(context.scope.as_ref())?;
        context.scope = previous;
        std::mem::swap(&mut context.tracker, &mut self.activations);
        std::mem::swap(&mut context.contingencies, &mut self.contingencies);
        std::mem::swap(&mut context.task, &mut self.current_task);
        Ok(())
    }
    /// The selected context's task, or a task beneath which it was created, has a
    /// cancellation request.
    pub fn task_cancelled(&self) -> bool {
        self.current_task
            .is_some_and(|task| self.tasks.is_cancelled(task))
    }
    /// An owner admitted by the selected context belongs to its task.
    fn record_owner(&mut self, owner: tasks::OwnerId) {
        if let Some(task) = self.current_task {
            self.tasks.record_owner_task(owner, task);
        }
    }
    /// Release the resources an unstarted host operation would have consumed.
    pub fn release_consumed(&mut self, name: &str, args: &[V]) -> R<()> {
        let p = self.declared_primitive(name)?;
        let consumed: Vec<_> = p["parameters"]
            .as_array()
            .unwrap()
            .iter()
            .zip(args)
            .filter(|(param, _)| param["mode"] == "consume")
            .map(|(_, value)| Some(value.clone()))
            .collect();
        self.release_frame(&consumed);
        Ok(())
    }
    /// Retire the selected root after all nested work has settled. Normal outgoing
    /// owners survive; a failed root releases even owners stranded in temporaries.
    pub fn finish_context(&mut self, root: &mut ContextRoot, failed: bool) -> R<()> {
        if root.retired {
            return Err("E_NATIVE: context root already retired".into());
        }
        self.resources.finish_scope(&root.scope, failed)?;
        root.retired = true;
        Ok(())
    }
    /// Adopt a completed parked context's result owners and retire its resource
    /// root once. None receives into the currently selected host scope; Some
    /// receives into another parked context's selected scope. Neither context is
    /// selected or executed. The caller proves completion, invocation and fault
    /// domain authority before this host-level resource operation.
    pub fn adopt_context_result(
        &mut self,
        source: &HostContext,
        root: &mut ContextRoot,
        receiver: Option<(&HostContext, &ContextRoot)>,
        fields: &[V],
    ) -> R<()> {
        self.require_context(source, root)?;
        let selected = self.resources.current_scope()?;
        let source_id = root.scope.id;
        if source.scope.as_ref().map(|scope| scope.id) != Some(source_id)
            || selected.as_ref().is_some_and(|scope| scope.id == source_id)
            || self
                .resources
                .scopes
                .values()
                .any(|parent| *parent == Some(source_id))
        {
            return Err(
                "E_NATIVE: result source must be parked without nested resource work".into(),
            );
        }
        let target = if let Some((context, root)) = receiver {
            self.require_context(context, root)?;
            context.scope.as_ref().map(|scope| scope.id)
        } else {
            selected.as_ref().map(|scope| scope.id)
        };
        if target == Some(source_id) {
            return Err("E_NATIVE: result source cannot receive its own owners".into());
        }
        let owned = self.resources.result_tokens(fields)?;
        let retained: BTreeSet<_> = self
            .resources
            .values
            .iter()
            .filter_map(|(token, resource)| {
                (resource.scope == Some(source_id)).then_some(token.clone())
            })
            .collect();
        if owned != retained {
            return Err("E_NATIVE: result ownership differs from its retained scope".into());
        }
        // Exclusive access to Host keeps preflight and transfer atomic. There are
        // no fallible operations, callbacks or resource destructors below.
        for token in owned {
            self.resources.values.get_mut(&token).unwrap().scope = target;
        }
        self.resources.scopes.remove(&source_id);
        root.retired = true;
        Ok(())
    }
    pub fn leave(&mut self) {
        self.activations.leave();
    }
    pub fn at(&mut self, site: usize) {
        self.activations.at(site);
    }
    pub fn fault<T>(&mut self, value: V) -> R<T> {
        let V::String(message) = value else {
            return Err("E_NATIVE: fault requires string".into());
        };
        self.activations.source_fault();
        Err(format!("E_FAULT: {message}"))
    }
    pub fn source_faulted(&self) -> bool {
        self.activations.is_source_fault()
    }
    pub fn escalate<T>(&mut self) -> R<T> {
        self.activations.source_fault();
        Err(self
            .contingencies
            .last()
            .cloned()
            .unwrap_or_else(|| "E_NATIVE: escalate outside contingency".into()))
    }
    pub fn release_frame(&mut self, values: &[Option<V>]) {
        fn release(resources: &mut Resources, v: &V) {
            match v {
                V::Opaque { token, .. } => {
                    resources.values.remove(token);
                }
                V::Construct { fields, .. }
                | V::Task {
                    arguments: fields, ..
                } => {
                    for v in fields {
                        release(resources, v);
                    }
                }
                _ => {}
            }
        }
        for v in values.iter().flatten() {
            release(&mut self.resources, v);
        }
    }
    /// Start a boundary before invoking work. Lexical boundaries introduce their
    /// own diagnostic activation; named work enters its generated control instead.
    pub fn begin_boundary<'a>(
        &mut self,
        name: &str,
        inputs: impl IntoIterator<Item = &'a V>,
        lexical: bool,
    ) -> R<BoundaryState> {
        let prepared = self.resources.prepare_scope(inputs)?;
        let mut child = self.activations.child();
        if lexical {
            child.enter(name)?;
        }
        let parent = std::mem::replace(&mut self.activations, child);
        let scope = self.resources.commit_scope(prepared);
        Ok(BoundaryState {
            name: name.into(),
            parent: Some(parent),
            scope,
            fault_message: None,
            lexical,
            contingency_active: false,
            contingency_depth: self.contingencies.len(),
        })
    }
    fn require_boundary(&self, state: &BoundaryState) -> R<()> {
        if state.parent.is_none() {
            return Err("E_NATIVE: boundary already retired".into());
        }
        self.resources.require_quiescent_scope(&state.scope)?;
        let expected = state.contingency_depth + usize::from(state.contingency_active);
        if self.contingencies.len() != expected {
            return Err("E_NATIVE: boundary contingency context is not selected".into());
        }
        Ok(())
    }
    /// Settle unretained owners and establish the original cause before handler
    /// entry. The caller must already have retired nested borrower activations.
    pub fn boundary_fault(
        &mut self,
        state: &mut BoundaryState,
        message: &str,
        keep: &BTreeSet<String>,
    ) -> R<fault::Report> {
        self.require_boundary(state)?;
        if state.fault_message.is_some() || !self.source_faulted() {
            return Err(
                "E_NATIVE: boundary cannot enter contingency again or without a fault".into(),
            );
        }
        state.fault_message = Some(message.into());
        let report = self
            .activations
            .failure(&state.name, fault::Phase::Execution, message);
        if self.trace {
            self.events.push(json!({"kind":"fault","report":report}));
        } else {
            eprintln!("{report}");
        }
        self.resources.release_scope_except(&state.scope, keep)?;
        let mut tracker = self.activations.child();
        tracker.with_cause(report.clone());
        self.activations = tracker;
        Ok(report)
    }
    pub fn enter_contingency(&mut self, state: &mut BoundaryState) -> R<()> {
        self.require_boundary(state)?;
        if state.fault_message.is_none() || state.contingency_active {
            return Err("E_NATIVE: invalid boundary contingency entry".into());
        }
        if state.lexical {
            self.enter(&format!("{}$contingency", state.name))?;
        }
        self.contingencies
            .push(state.fault_message.as_ref().unwrap().clone());
        state.contingency_active = true;
        Ok(())
    }
    /// Finalize exactly once, retaining the original error channel and cause.
    /// Normal lexical completion promotes survivors; named work always settles.
    pub fn finish_boundary<T>(&mut self, state: &mut BoundaryState, result: &R<T>) -> R<()> {
        self.require_boundary(state)?;
        self.resources.finish_scope(
            &state.scope,
            !state.lexical || state.fault_message.is_some(),
        )?;
        if state.contingency_active {
            self.contingencies.pop();
            state.contingency_active = false;
        }
        let mut parent = state.parent.take().unwrap();
        parent.absorb(&self.activations);
        if let Err(message) = result {
            if self.source_faulted() {
                parent.source_fault();
            }
            parent.with_cause(self.activations.failure(
                &state.name,
                fault::Phase::Execution,
                message,
            ));
        }
        self.activations = parent;
        Ok(())
    }
    pub fn lexical(
        &mut self,
        values: &mut Frames,
        atoms: &mut Vec<bool>,
        base: usize,
        name: &str,
        retained: &[usize],
        retire: &[usize],
        work_slots: &[usize],
        handler_slots: &[usize],
        fault_slot: usize,
        work: impl FnOnce(&mut Host, &mut Frames, &mut Vec<bool>) -> R<Completion>,
        handler: impl FnOnce(&mut Host, &mut Frames, &mut Vec<bool>) -> R<Completion>,
    ) -> R<Completion> {
        let mut state = self.begin_boundary(
            name,
            retained
                .iter()
                .chain(retire)
                .filter_map(|id| values[base + id].as_ref()),
            true,
        )?;
        let attempted = work(self, values, atoms);
        let result = (|| match attempted {
            Err(message) if self.source_faulted() => {
                let keep = self
                    .resources
                    .owned_tokens(retained.iter().filter_map(|id| values[base + id].as_ref()))?;
                for id in retire {
                    values[base + id] = None;
                }
                let report = self.boundary_fault(&mut state, &message, &keep)?;
                let (message_text, stack, paths) = report.source_fields();
                let strings = |xs: Vec<String>| V::Construct {
                    ty: "vector<String>".into(),
                    name: "vector".into(),
                    fields: xs.into_iter().map(V::String).collect(),
                };
                values[base + fault_slot] = Some(V::Construct {
                    ty: "fault_report".into(),
                    name: "fault_report".into(),
                    fields: vec![V::String(message_text), strings(stack), strings(paths)],
                });
                self.enter_contingency(&mut state)?;
                handler(self, values, atoms)
            }
            other => other,
        })();
        for id in work_slots.iter().chain(handler_slots) {
            values[base + id] = None;
        }
        self.finish_boundary(&mut state, &result)?;
        result
    }
    pub fn supervise(
        &mut self,
        values: &mut Frames,
        args: Vec<V>,
        work: fn(&mut Host, &mut Frames, Vec<V>, &mut Vec<V>) -> R<Completion>,
        handler: fn(&mut Host, &mut Frames, Vec<V>, &mut Vec<V>) -> R<Completion>,
        name: &str,
    ) -> R<()> {
        let mut state = self.begin_boundary(name, &args, false)?;
        let mut retained = vec![];
        let attempted = work(self, values, args, &mut retained);
        let result = (|| match attempted {
            Err(message) if self.source_faulted() => {
                let keep = self.resources.owned_tokens(&retained)?;
                self.boundary_fault(&mut state, &message, &keep)?;
                retained.push(V::String(
                    message.strip_prefix("E_FAULT: ").unwrap_or(&message).into(),
                ));
                self.enter_contingency(&mut state)?;
                handler(self, values, retained, &mut vec![]).and_then(Completion::settled)
            }
            other => other.and_then(Completion::settled),
        })();
        // Includes owners stranded in temporary arguments at a failing adapter.
        self.finish_boundary(&mut state, &result)?;
        result
    }
    fn failure(
        &mut self,
        entry: &str,
        phase: fault::Phase,
        message: String,
        json_output: bool,
    ) -> String {
        let report = self.activations.failure(entry, phase, &message);
        if json_output {
            serde_json::to_string(&report).unwrap_or_else(|_| report.to_string())
        } else {
            report.to_string()
        }
    }
    pub fn tick(&mut self, line: usize, column: usize) -> R<()> {
        if let Some(fuel) = &mut self.fuel {
            if *fuel == 0 {
                return Err(format!(
                    "E_FUEL at {line}:{column}: host instruction limit reached"
                ));
            }
            *fuel -= 1;
        }
        Ok(())
    }
    pub fn emit(&mut self, v: V) -> R<()> {
        let V::String(message) = v else {
            return Err("E_NATIVE: emit requires String".into());
        };
        if self.trace {
            self.events.push(json!({"kind":"emit","message":message}));
        } else {
            println!("{message}");
        }
        Ok(())
    }
    fn declared_primitive(&self, name: &str) -> R<Json> {
        self.manifest["primitives"]
            .as_array()
            .and_then(|ps| ps.iter().find(|p| p["name"] == name))
            .cloned()
            .ok_or_else(|| "E_DRIVER: undeclared primitive".into())
    }
    /// Start an async host operation. Its scripted completion is bound and
    /// validated now; pending reports never consume or replay script entries.
    pub fn start_host_operation(&mut self, name: &str, args: &[V]) -> R<HostStart> {
        let p = self.declared_primitive(name)?;
        if p["async"] != true {
            return Err(format!("E_NATIVE: `{name}` is not an async host operation"));
        }
        let types = p["parameters"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["type"].as_str().unwrap().into())
            .collect::<Vec<_>>();
        validate_args(args, &types, &self.manifest)?;
        let Some(script) = &self.script else {
            if p["rust"].is_object() {
                let start = self
                    .rust_start
                    .ok_or("E_HOST: Rust adapter is not linked")?;
                return Ok(HostStart::Live(start(
                    &mut self.resources,
                    name,
                    args.to_vec(),
                )?));
            }
            return Err(format!(
                "E_HOST: no live adapter for async host operation `{name}`; supply --scenario"
            ));
        };
        let call = script
            .get(self.cursor)
            .ok_or_else(|| format!("E_DRIVER: script exhausted before {name}"))?;
        if call.operation != name {
            return Err(format!(
                "E_DRIVER: expected {name}, found {}",
                call.operation
            ));
        }
        let o = p["outcomes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|o| o["name"] == call.outcome.as_str())
            .ok_or_else(|| format!("E_DRIVER: undeclared outcome {}", call.outcome))?;
        let types = o["fields"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f["type"].as_str().unwrap().into())
            .collect::<Vec<_>>();
        validate_args(&call.fields, &types, &self.manifest)?;
        let started = HostStart::Scripted {
            outcome: call.outcome.clone(),
            fields: call.fields.clone(),
            waits: call.waits,
        };
        self.cursor += 1;
        Ok(started)
    }
    /// One poll of a live operation; a completion must match its declared outcome.
    pub fn poll_host_operation(
        &mut self,
        name: &str,
        operation: &mut Box<dyn HostPoll>,
    ) -> R<HostProgress> {
        let progress = operation.poll(&mut self.resources)?;
        if let HostProgress::Ready((outcome, fields)) = &progress {
            let p = self.declared_primitive(name)?;
            let o = p["outcomes"]
                .as_array()
                .unwrap()
                .iter()
                .find(|o| o["name"] == outcome.as_str())
                .ok_or_else(|| format!("E_DRIVER: undeclared outcome {outcome}"))?;
            let types = o["fields"]
                .as_array()
                .unwrap()
                .iter()
                .map(|f| f["type"].as_str().unwrap().into())
                .collect::<Vec<_>>();
            validate_args(fields, &types, &self.manifest)?;
        }
        Ok(progress)
    }
    /// Publish a completed host operation once, with its start-time arguments.
    pub fn finish_host_operation(
        &mut self,
        name: &str,
        args: Vec<V>,
        outcome: &str,
        fields: &[V],
    ) -> R<()> {
        if self.trace {
            let p = self.declared_primitive(name)?;
            self.events.push(json!({"kind":"primitive","operation":name,"arguments":args,"outcome":outcome,"fields":fields,"effects":p["effects"]}));
        }
        Ok(())
    }
    pub fn primitive(&mut self, name: &str, args: Vec<V>) -> R<Outcome> {
        let p = self.manifest["primitives"]
            .as_array()
            .and_then(|ps| ps.iter().find(|p| p["name"] == name))
            .ok_or("E_DRIVER: undeclared primitive")?
            .clone();
        let types = p["parameters"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["type"].as_str().unwrap().into())
            .collect::<Vec<_>>();
        validate_args(&args, &types, &self.manifest)?;
        if p["pure"] == true {
            self.check_contract(name, &p)?;
            return Ok((
                p["outcomes"][0]["name"].as_str().unwrap().into(),
                vec![pure(name, args)?],
            ));
        }
        let (outcome, values) = if let Some(script) = &self.script {
            let call = script
                .get(self.cursor)
                .ok_or_else(|| format!("E_DRIVER: script exhausted before {name}"))?;
            if call.operation != name {
                return Err(format!(
                    "E_DRIVER: expected {name}, found {}",
                    call.operation
                ));
            }
            if call.waits != 0 {
                return Err(format!(
                    "E_DRIVER: scripted waits require an async host operation; `{name}` completes immediately"
                ));
            }
            self.cursor += 1;
            (call.outcome.clone(), call.fields.clone())
        } else if p["rust"].is_object() {
            self.rust_adapter
                .ok_or("E_HOST: Rust adapter is not linked")?(
                &mut self.resources,
                name,
                args.clone(),
            )?
        } else {
            let modern = self.check_contract(name, &p)?;
            let (mut outcome, mut values) = self.real(name, &args)?;
            if modern {
                outcome = snake_name(&outcome);
                for value in &mut values {
                    if let V::Opaque { ty, .. } = value {
                        *ty = snake_name(ty);
                    }
                }
            }
            (outcome, values)
        };
        let o = p["outcomes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|o| o["name"] == outcome)
            .ok_or_else(|| format!("E_DRIVER: undeclared outcome {outcome}"))?;
        let types = o["fields"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f["type"].as_str().unwrap().into())
            .collect::<Vec<_>>();
        validate_args(&values, &types, &self.manifest)?;
        if self.trace {
            self.events.push(json!({"kind":"primitive","operation":name,"arguments":args,"outcome":outcome,"fields":values,"effects":p["effects"]}));
        }
        Ok((outcome, values))
    }
    fn check_contract(&self, name: &str, p: &Json) -> R<bool> {
        let (params, outcomes, effects) = contract(name).ok_or_else(|| {
            format!(
                "E_HOST: no live adapter for `{name}`; supply --scenario or implement an adapter"
            )
        })?;
        for modern in [false, true] {
            let type_name = |t: &str| {
                if modern && ["Handle", "ReadError", "CloseError"].contains(&t) {
                    snake_name(t)
                } else {
                    t.to_string()
                }
            };
            let actual_params = p["parameters"]
                .as_array()
                .unwrap()
                .iter()
                .map(|p| {
                    (
                        p["type"].as_str().unwrap().to_string(),
                        p["mode"].as_str().unwrap().to_string(),
                    )
                })
                .collect::<Vec<_>>();
            let expected_params = params
                .iter()
                .map(|(t, m)| (type_name(t), m.to_string()))
                .collect::<Vec<_>>();
            let actual_outcomes = p["outcomes"]
                .as_array()
                .unwrap()
                .iter()
                .map(|o| {
                    (
                        o["name"].as_str().unwrap().to_string(),
                        o["fields"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .map(|f| f["type"].as_str().unwrap().to_string())
                            .collect::<Vec<_>>(),
                    )
                })
                .collect::<BTreeMap<_, _>>();
            let expected_outcomes = outcomes
                .iter()
                .map(|(n, fields)| {
                    (
                        if modern { snake_name(n) } else { n.to_string() },
                        fields.iter().map(|t| type_name(t)).collect::<Vec<_>>(),
                    )
                })
                .collect::<BTreeMap<_, _>>();
            if actual_params != expected_params
                || actual_outcomes != expected_outcomes
                || p["effects"] != json!(effects)
            {
                continue;
            }
            for (ty, linear) in [
                ("Handle", true),
                ("ReadError", false),
                ("CloseError", false),
            ] {
                let public = type_name(ty);
                let used = params.iter().any(|(t, _)| *t == ty)
                    || outcomes.iter().any(|(_, fields)| fields.contains(&ty));
                if used
                    && !self.manifest["types"].as_array().unwrap().iter().any(|t| {
                        t["name"] == public && t["kind"] == "opaque" && t["linear"] == linear
                    })
                {
                    return Err(format!(
                        "E_HOST: `{public}` does not match the live adapter type contract"
                    ));
                }
            }
            return Ok(modern);
        }
        Err(format!(
            "E_HOST: `{name}` manifest does not match the live adapter contract"
        ))
    }
    fn real(&mut self, name: &str, args: &[V]) -> R<Outcome> {
        fn text(v: &V) -> &str {
            if let V::String(s) = v {
                s
            } else {
                unreachable!()
            }
        }
        fn number(v: &V) -> u32 {
            if let V::U32(n) = v {
                *n
            } else {
                unreachable!()
            }
        }
        fn opaque(ty: &str, token: String) -> V {
            V::Opaque {
                ty: ty.into(),
                token,
            }
        }
        let (outcome, values) = match name {
            "parse_u32" => {
                let s = text(&args[0]);
                if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
                    ("Malformed", vec![])
                } else {
                    match s.parse::<u32>() {
                        Ok(n) => ("Number", vec![V::U32(n)]),
                        Err(_) => ("Overflow", vec![]),
                    }
                }
            }
            "checked_add" | "checked_sub" | "checked_mul" | "checked_div" => {
                let a = number(&args[0]);
                let b = number(&args[1]);
                let result = match name {
                    "checked_add" => a.checked_add(b),
                    "checked_sub" => a.checked_sub(b),
                    "checked_mul" => a.checked_mul(b),
                    _ => a.checked_div(b),
                };
                match result {
                    Some(n) => (
                        match name {
                            "checked_add" => "Sum",
                            "checked_sub" => "Difference",
                            "checked_mul" => "Product",
                            _ => "Quotient",
                        },
                        vec![V::U32(n)],
                    ),
                    None => (
                        match name {
                            "checked_sub" => "Underflow",
                            "checked_div" => "ZeroDivisor",
                            _ => "Overflow",
                        },
                        vec![],
                    ),
                }
            }
            "u32_text" => ("Text", vec![V::String(number(&args[0]).to_string())]),
            "concat" => (
                "Text",
                vec![V::String(format!("{}{}", text(&args[0]), text(&args[1])))],
            ),
            "decode_utf8" => {
                let V::Bytes(bytes) = &args[0] else {
                    unreachable!()
                };
                match String::from_utf8(bytes.clone()) {
                    Ok(s) => ("Text", vec![V::String(s)]),
                    Err(_) => ("Invalid", vec![]),
                }
            }
            "open" => {
                let Ok(path) = CString::new(text(&args[0])) else {
                    return Ok(("Denied".into(), vec![]));
                };
                let file = unsafe { fopen(path.as_ptr(), c"rb".as_ptr()) };
                if file.is_null() {
                    if std::io::Error::last_os_error().kind() == std::io::ErrorKind::NotFound {
                        ("NotFound", vec![])
                    } else {
                        ("Denied", vec![])
                    }
                } else {
                    let token = format!("file-{}", self.next_file);
                    self.next_file += 1;
                    self.files.insert(token.clone(), file);
                    ("Opened", vec![opaque("Handle", token)])
                }
            }
            "read" => {
                let V::Opaque { token, .. } = &args[0] else {
                    unreachable!()
                };
                let file = *self
                    .files
                    .get(token)
                    .ok_or("E_HOST: invalid/closed file handle")?;
                let mut bytes = vec![];
                let mut buffer = [0u8; 8192];
                loop {
                    let n = unsafe { fread(buffer.as_mut_ptr().cast(), 1, buffer.len(), file) };
                    bytes.extend_from_slice(&buffer[..n]);
                    if n < buffer.len() {
                        break;
                    }
                }
                if unsafe { ferror(file) } != 0 {
                    (
                        "Failed",
                        vec![opaque(
                            "ReadError",
                            std::io::Error::last_os_error().to_string(),
                        )],
                    )
                } else {
                    ("Data", vec![V::Bytes(bytes)])
                }
            }
            "close" => {
                let V::Opaque { token, .. } = &args[0] else {
                    unreachable!()
                };
                let file = self
                    .files
                    .remove(token)
                    .ok_or("E_HOST: invalid/closed file handle")?;
                if unsafe { fclose(file) } == 0 {
                    ("Closed", vec![])
                } else {
                    (
                        "Failed",
                        vec![opaque(
                            "CloseError",
                            std::io::Error::last_os_error().to_string(),
                        )],
                    )
                }
            }
            _ => return Err(format!("E_HOST: unsupported live primitive {name}")),
        };
        Ok((outcome.into(), values))
    }
    pub fn finish(&self, result: Outcome, json_output: bool) -> R<()> {
        if self.script.as_ref().is_some_and(|s| self.cursor != s.len()) {
            return Err("E_DRIVER: unconsumed script calls".into());
        }
        if json_output {
            println!(
                "{}",
                json!({"outcome":result.0,"fields":result.1,"events":self.events})
            );
        } else if result.1.is_empty() {
            println!("{}", result.0);
        } else {
            println!("{} {}", result.0, serde_json::to_string(&result.1).unwrap());
        }
        Ok(())
    }
}
type Contract = (
    Vec<(&'static str, &'static str)>,
    Vec<(&'static str, Vec<&'static str>)>,
    Vec<&'static str>,
);
fn contract(name: &str) -> Option<Contract> {
    Some(match name {
        "parse_u32" => (
            vec![("String", "value")],
            vec![
                ("Number", vec!["U32"]),
                ("Malformed", vec![]),
                ("Overflow", vec![]),
            ],
            vec![],
        ),
        "checked_add" | "checked_mul" | "checked_sub" | "checked_div" => (
            vec![("U32", "value"), ("U32", "value")],
            vec![
                (
                    match name {
                        "checked_add" => "Sum",
                        "checked_mul" => "Product",
                        "checked_sub" => "Difference",
                        _ => "Quotient",
                    },
                    vec!["U32"],
                ),
                (
                    match name {
                        "checked_sub" => "Underflow",
                        "checked_div" => "ZeroDivisor",
                        _ => "Overflow",
                    },
                    vec![],
                ),
            ],
            vec![],
        ),
        "u32_text" => (
            vec![("U32", "value")],
            vec![("Text", vec!["String"])],
            vec![],
        ),
        "concat" => (
            vec![("String", "value"), ("String", "value")],
            vec![("Text", vec!["String"])],
            vec![],
        ),
        "decode_utf8" => (
            vec![("Bytes", "value")],
            vec![("Text", vec!["String"]), ("Invalid", vec![])],
            vec![],
        ),
        "open" => (
            vec![("String", "value")],
            vec![
                ("Opened", vec!["Handle"]),
                ("NotFound", vec![]),
                ("Denied", vec![]),
            ],
            vec!["file.open"],
        ),
        "read" => (
            vec![("Handle", "borrow")],
            vec![("Data", vec!["Bytes"]), ("Failed", vec!["ReadError"])],
            vec!["file.read"],
        ),
        "close" => (
            vec![("Handle", "consume")],
            vec![("Closed", vec![]), ("Failed", vec!["CloseError"])],
            vec!["file.close"],
        ),
        _ => return None,
    })
}
pub type EntryFn = fn(&mut Host, Vec<V>) -> R<Completion>;
pub struct Entry {
    pub name: &'static str,
    pub display_name: &'static str,
    pub run: EntryFn,
    pub arguments: Vec<String>,
    pub exit_codes: &'static [(&'static str, u8)],
}
pub fn launch(
    manifest: &str,
    default_entry: &str,
    entries: &[Entry],
    rust_adapter: Option<RustAdapter>,
    rust_start: Option<RustStart>,
    rust_readiness: RustReadiness,
    sites: &str,
) -> R<u8> {
    let manifest: Json = serde_json::from_str(manifest).map_err(|e| e.to_string())?;
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let mut positional = vec![];
    let mut scenario = None;
    let mut json_output = false;
    let mut fuel = None;
    let mut entry = default_entry.to_string();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--" => {
                positional.extend_from_slice(&args[i + 1..]);
                break;
            }
            "--json" => json_output = true,
            "--scenario" => {
                i += 1;
                let file = args.get(i).ok_or("--scenario requires a filename")?;
                scenario = Some(
                    serde_json::from_str::<Scenario>(
                        &std::fs::read_to_string(file).map_err(|e| e.to_string())?,
                    )
                    .map_err(|e| e.to_string())?,
                );
                json_output = true;
            }
            "--entry" => {
                i += 1;
                entry = args.get(i).ok_or("--entry requires a name")?.clone();
            }
            "--fuel" => {
                i += 1;
                fuel = Some(
                    args.get(i)
                        .ok_or("--fuel requires a number")?
                        .parse()
                        .map_err(|_| "invalid fuel")?,
                );
            }
            "--help" => {
                println!(
                    "Usage: program [--entry CONTROL] [--json] [--fuel N] [--scenario FILE] [--] ARGUMENTS\nScalar arguments: String, U32, Bool, Unit. Aggregates use a JSON Value argument.\nAvailable executable entries: {}",
                    entries
                        .iter()
                        .map(|e| e.name)
                        .collect::<Vec<_>>()
                        .join(", ")
                );
                return Ok(0);
            }
            s if s.starts_with("--") => return Err(format!("unknown option {s}")),
            _ => positional.push(args[i].clone()),
        };
        i += 1;
    }
    if let Some(s) = &scenario {
        entry = s.entry.clone();
        if !positional.is_empty() {
            return Err("positional arguments cannot accompany --scenario".into());
        }
    }
    let selected = entries.iter().find(|e| e.name == entry).ok_or_else(|| {
        format!("E_ENTRY: `{entry}` is not a configured executable entry; use --help")
    })?;
    let types = &selected.arguments;
    let (values, script) = if let Some(s) = scenario {
        (s.arguments, Some(s.calls))
    } else {
        if positional.len() != types.len() {
            return Err(format!(
                "{entry} expects {} arguments, found {}; use --help",
                types.len(),
                positional.len()
            ));
        }
        let values = positional
            .iter()
            .zip(types)
            .map(|(s, ty)| {
                Ok(match ty.as_str() {
                    "String" => V::String(s.clone()),
                    "U32" => V::U32(s.parse().map_err(|_| "invalid U32 argument")?),
                    "Bool" => V::Bool(
                        s.parse()
                            .map_err(|_| "Bool argument must be true or false")?,
                    ),
                    "Unit" if s == "unit" => V::Unit,
                    _ => serde_json::from_str(s).map_err(|e| format!("invalid value JSON: {e}"))?,
                })
            })
            .collect::<R<Vec<_>>>()?;
        (values, None)
    };
    validate_args(&values, types, &manifest)?;
    // A new live invocation has no pre-existing resource owners. Reject JSON
    // handles before executing any code, including handles nested in aggregates.
    // Otherwise a guessed token could alias an object acquired later by the entry.
    fn supplied_resource(value: &V, bindings: &Json) -> bool {
        match value {
            V::Opaque { ty, .. } => bindings.get(ty).is_some(),
            V::Construct { fields, .. } => fields.iter().any(|v| supplied_resource(v, bindings)),
            _ => false,
        }
    }
    if script.is_none()
        && values
            .iter()
            .any(|v| supplied_resource(v, &manifest["rust"]["resources"]))
    {
        return Err("E_HOST: live resource arguments cannot be supplied as JSON; acquire resources inside the executable entry".into());
    }
    let mut host = Host::new(manifest, script, json_output, fuel);
    host.rust_adapter = rust_adapter;
    host.rust_start = rust_start;
    host.readiness = rust_readiness()?;
    host.activations = fault::Tracker::new(
        serde_json::from_str(sites).map_err(|e| format!("E_NATIVE: invalid source table: {e}"))?,
    );
    let result = (selected.run)(&mut host, values).map_err(|error| {
        host.failure(
            selected.display_name,
            fault::Phase::Execution,
            error,
            json_output,
        )
    })?;
    if let Completion::Process(status) = result {
        if host.script.as_ref().is_some_and(|s| host.cursor != s.len()) {
            return Err("E_DRIVER: unconsumed script calls".into());
        }
        // This is administrative completion, with no outcome tag or payload.
        if json_output {
            println!(
                "{}",
                json!({"termination":"process","status":status,"events":host.events})
            );
        }
        return Ok(status);
    }
    let result = result.outcome()?;
    let code = selected
        .exit_codes
        .iter()
        .find(|(name, _)| *name == result.0)
        .ok_or_else(|| {
            host.failure(
                selected.display_name,
                fault::Phase::ResultValidation,
                format!("E_EXIT_CODE: no status for {}.{}", selected.name, result.0),
                json_output,
            )
        })?
        .1;
    host.finish(result, json_output).map_err(|error| {
        host.failure(
            selected.display_name,
            fault::Phase::ResultValidation,
            error,
            json_output,
        )
    })?;
    Ok(code)
}

impl collection_support::CollectionValue for V {
    fn parts(&self) -> Option<(&str, &str, &[Self])> {
        if let Self::Construct { ty, name, fields } = self {
            Some((ty, name, fields))
        } else {
            None
        }
    }
    fn parts_mut(&mut self) -> Option<&mut Vec<Self>> {
        if let Self::Construct { fields, .. } = self {
            Some(fields)
        } else {
            None
        }
    }
    fn construct(ty: String, name: String, fields: Vec<Self>) -> Self {
        Self::Construct { ty, name, fields }
    }
    fn number(&self) -> Option<u32> {
        if let Self::U32(n) = self {
            Some(*n)
        } else {
            None
        }
    }
    fn key_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        match (self, other) {
            (Self::Bool(a), Self::Bool(b)) => Some(a.cmp(b)),
            (Self::U32(a), Self::U32(b)) => Some(a.cmp(b)),
            (Self::String(a), Self::String(b)) => Some(a.cmp(b)),
            _ => None,
        }
    }
}

pub fn collection_length(value: V, values: &mut [Option<V>]) -> R<V> {
    let V::Reference { root, fields, .. } = value else {
        return Err("invalid collection length argument".into());
    };
    let value = mutable_place(values, root, &fields)?;
    if let V::Construct { ty, fields, .. } = &*value
        && ty.starts_with("$tasks<")
    {
        return u32::try_from(fields.len())
            .map(V::U32)
            .map_err(|_| "E_NATIVE: task set count exceeds u32".into());
    }
    Ok(V::U32(collection_support::length(value)?))
}
pub fn collection(name: &str, args: Vec<V>, values: &mut [Option<V>]) -> R<Outcome> {
    let mut args = args.into_iter();
    let mut first = args.next().ok_or("missing collection")?;
    let evidence = if let V::Reference { evidence, .. } = &first {
        evidence.clone()
    } else {
        ReferenceEvidence::default()
    };
    let address = if let V::Reference { root, fields, .. } = &first {
        Some((*root, fields.clone()))
    } else {
        None
    };
    let container = if let Some((root, fields)) = &address {
        mutable_place(values, *root, fields)?
    } else {
        &mut first
    };
    let (outcome, payloads) = collection_support::operate(name, container, args.collect())?;
    let fields = payloads
        .into_iter()
        .map(|p| match p {
            collection_support::Payload::Owned(v) => Ok(v),
            collection_support::Payload::Borrow(index, ty) => {
                let (root, mut fields) = address.clone().ok_or("borrow of collection temporary")?;
                fields.push(index);
                Ok(V::Reference {
                    ty,
                    root,
                    fields,
                    evidence: evidence.projected(&[index]),
                })
            }
        })
        .collect::<R<Vec<_>>>()?;
    Ok((outcome, fields))
}
