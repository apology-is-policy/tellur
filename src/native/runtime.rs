#[cfg(test)]
#[path = "../collection_support.rs"]
mod collection_support;
#[cfg(test)]
#[path = "../fault.rs"]
mod fault;
// Embedded in generated programs. This runtime contains values and primitive
// adapters only; region execution is generated as native Rust control flow.
use serde::{Deserialize, Serialize};
use serde_json::{Value as Json, json};
use std::{
    collections::BTreeMap,
    ffi::{CString, c_char, c_int, c_void},
};
pub type R<T> = Result<T, String>;
pub type Outcome = (String, Vec<V>);
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
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
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
        ty: String,
        root: usize,
        fields: Vec<usize>,
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
impl V {
    pub fn ty(&self) -> &str {
        match self {
            Self::Bool(_) => "Bool",
            Self::U32(_) => "U32",
            Self::String(_) => "String",
            Self::Bytes(_) => "Bytes",
            Self::Unit => "Unit",
            Self::Opaque { ty, .. } | Self::Construct { ty, .. } | Self::Reference { ty, .. } => ty,
        }
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
pub fn reference(
    values: &[Option<V>],
    root: usize,
    fields: &[usize],
    indirect: bool,
    ty: &str,
) -> R<V> {
    let (root, fields) = address(values, root, fields, indirect)?;
    Ok(V::Reference {
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
    let V::Reference { ty, root, fields } = reference else {
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
                    if let V::Reference { ty, root, fields } = value {
                        let (exclusive, inner) = if let Some(inner) = ty.strip_prefix("&mutable ") {
                            (mutable, inner)
                        } else {
                            (false, ty.strip_prefix('&').expect("reference type"))
                        };
                        V::Reference {
                            ty: format!("&{}{}", if exclusive { "mutable " } else { "" }, inner),
                            root: *root,
                            fields: fields.clone(),
                        }
                    } else {
                        let mut path = fields.clone();
                        path.push(i);
                        V::Reference {
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
/// Host registry for activation-owned objects. Tokens never clone a Rust owner.
/// Dropping the host releases remaining objects; fallible protocol completion
/// still belongs to explicit consuming adapter operations.
#[derive(Default)]
pub struct Resources {
    values: BTreeMap<String, (&'static str, Box<dyn std::any::Any>)>,
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
        self.values.insert(token.clone(), (ty, Box::new(value)));
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
        let (actual, _) = self
            .values
            .get(token)
            .ok_or("E_HOST: unknown or consumed resource handle")?;
        if *actual != expected {
            return Err("E_HOST: resource identity/type mismatch".into());
        }
        Ok(token)
    }
    pub fn get<T: 'static>(&self, value: &V, ty: &str) -> R<&T> {
        let token = self.token(value, ty)?;
        self.values[token]
            .1
            .downcast_ref()
            .ok_or_else(|| "E_HOST: Rust resource type mismatch".into())
    }
    pub fn take<T: 'static>(&mut self, value: &V, ty: &str) -> R<T> {
        self.get::<T>(value, ty)?;
        let V::Opaque { token, .. } = value else {
            unreachable!()
        };
        let (_, value) = self.values.remove(token).unwrap();
        Ok(*value
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
}
pub type RustAdapter = fn(&mut Resources, &str, Vec<V>) -> R<Outcome>;
pub struct Host {
    contingencies: Vec<String>,
    pub rust_adapter: Option<RustAdapter>,
    pub manifest: Json,
    script: Option<Vec<ScriptCall>>,
    cursor: usize,
    events: Vec<Json>,
    pub trace: bool,
    pub fuel: Option<usize>,
    files: BTreeMap<String, *mut c_void>,
    next_file: usize,
    resources: Resources,
    activations: fault::Tracker,
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
            manifest,
            script,
            cursor: 0,
            events: vec![],
            trace,
            fuel,
            files: BTreeMap::new(),
            next_file: 0,
            resources: Resources::default(),
            contingencies: vec![],
            activations: fault::Tracker::default(),
        }
    }
    pub fn enter(&mut self, control: &str) -> R<()> {
        self.activations.enter(control)
    }
    pub fn leave(&mut self) {
        self.activations.leave();
    }
    pub fn at(&mut self, site: usize) {
        self.activations.at(site);
    }
    pub fn fault(&mut self, value: V) -> R<Outcome> {
        let V::String(message) = value else {
            return Err("E_NATIVE: fault requires string".into());
        };
        self.activations.source_fault();
        Err(format!("E_FAULT: {message}"))
    }
    pub fn source_faulted(&self) -> bool {
        self.activations.is_source_fault()
    }
    pub fn escalate(&mut self) -> R<Outcome> {
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
                V::Construct { fields, .. } => {
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
    pub fn supervise(
        &mut self,
        values: &mut Vec<Option<V>>,
        args: Vec<V>,
        work: fn(&mut Host, &mut Vec<Option<V>>, Vec<V>, &mut Vec<V>) -> R<Outcome>,
        handler: fn(&mut Host, &mut Vec<Option<V>>, Vec<V>, &mut Vec<V>) -> R<Outcome>,
        name: &str,
    ) -> R<()> {
        fn tokens(v: &V, into: &mut std::collections::BTreeSet<String>) {
            match v {
                V::Opaque { token, .. } => {
                    into.insert(token.clone());
                }
                V::Construct { fields, .. } => {
                    for v in fields {
                        tokens(v, into);
                    }
                }
                _ => {}
            }
        }
        let before: std::collections::BTreeSet<_> = self.resources.values.keys().cloned().collect();
        let mut owned = std::collections::BTreeSet::new();
        for v in &args {
            tokens(v, &mut owned);
        }
        let child = self.activations.child();
        let mut parent = std::mem::replace(&mut self.activations, child);
        let mut retained = vec![];
        let result = work(self, values, args, &mut retained);
        let result = match result {
            Err(message) if self.source_faulted() => {
                let report = self
                    .activations
                    .failure(name, fault::Phase::Execution, &message);
                if self.trace {
                    self.events.push(json!({"kind":"fault", "report": report}));
                } else {
                    eprintln!("{report}");
                }
                let mut handler_tracker = self.activations.child();
                handler_tracker.with_cause(report);
                self.activations = handler_tracker;
                retained.push(V::String(
                    message.strip_prefix("E_FAULT: ").unwrap_or(&message).into(),
                ));
                self.contingencies.push(message);
                let answer = handler(self, values, retained, &mut vec![]).map(|_| ());
                self.contingencies.pop();
                answer
            }
            other => other.map(|_| ()),
        };
        // Retire this boundary's objects, including owners moved into temporary
        // arguments at a failing host call. Parent resources remain untouched.
        self.resources
            .values
            .retain(|token, _| before.contains(token) && !owned.contains(token));
        parent.absorb(&self.activations);
        if let Err(message) = &result {
            if self.source_faulted() {
                parent.source_fault();
            }
            let report = self
                .activations
                .failure(name, fault::Phase::Execution, message);
            parent.with_cause(report);
        }
        self.activations = parent;
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
pub type EntryFn = fn(&mut Host, Vec<V>) -> R<Outcome>;
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
    Ok(V::U32(collection_support::length(mutable_place(
        values, root, &fields,
    )?)?))
}
pub fn collection(name: &str, args: Vec<V>, values: &mut [Option<V>]) -> R<Outcome> {
    let mut args = args.into_iter();
    let mut first = args.next().ok_or("missing collection")?;
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
                Ok(V::Reference { ty, root, fields })
            }
        })
        .collect::<R<Vec<_>>>()?;
    Ok((outcome, fields))
}
