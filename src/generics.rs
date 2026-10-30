//! First-order generic data declarations. Layout specialization has no executable
//! body: ownership/copy/completion properties are derived from concrete fields.
use crate::{
    Diagnostic, Result, Span,
    collection_support::generic,
    manifest::{self, Manifest},
    syntax::*,
};
use std::collections::{BTreeMap, BTreeSet};
fn error(at: Span, message: impl Into<String>) -> Diagnostic {
    Diagnostic::new("E_GENERIC", at, message)
}
#[derive(Clone)]
enum Template {
    Data(DataDeclaration),
    Outcome(OutcomeContract),
}
impl Template {
    fn name(&self) -> &str {
        match self {
            Self::Data(d) => &d.name,
            Self::Outcome(d) => &d.name,
        }
    }
    fn parameters(&self) -> &[String] {
        match self {
            Self::Data(d) => &d.type_parameters,
            Self::Outcome(d) => &d.type_parameters,
        }
    }
    fn span(&self) -> Span {
        match self {
            Self::Data(d) => d.span,
            Self::Outcome(d) => d.span,
        }
    }
    fn fields(&self) -> Vec<&Field> {
        match self {
            Self::Data(d) => match &d.body {
                DataBody::Struct(fs) => fs.iter().collect(),
                DataBody::Variant(os) => os.iter().flat_map(|o| &o.fields).collect(),
            },
            Self::Outcome(d) => d.outcomes.iter().flat_map(|o| &o.fields).collect(),
        }
    }
    fn all_names(&self) -> Vec<Vec<&str>> {
        match self {
            Self::Data(d) => match &d.body {
                DataBody::Struct(fs) => vec![fs.iter().map(|f| f.name.as_str()).collect()],
                DataBody::Variant(os) => variant_names(os),
            },
            Self::Outcome(d) => variant_names(&d.outcomes),
        }
    }
}
fn variant_names(os: &[Outcome]) -> Vec<Vec<&str>> {
    let mut names = vec![os.iter().map(|o| o.name.as_str()).collect()];
    names.extend(
        os.iter()
            .map(|o| o.fields.iter().map(|f| f.name.as_str()).collect::<Vec<_>>()),
    );
    names
}
pub(crate) fn substitute(ty: &str, bindings: &BTreeMap<String, String>) -> String {
    if let Some(ty) = bindings.get(ty) {
        return ty.clone();
    }
    if let Some((mutable, inner)) = manifest::reference_inner(ty) {
        return format!(
            "&{}{}",
            if mutable { "mutable " } else { "" },
            substitute(inner, bindings)
        );
    }
    if let Some((application, member)) = crate::collection_support::qualified_member(ty) {
        let application = substitute(application, bindings);
        let (first, rest) = member.split_once('.').unwrap_or((member, ""));
        let key = format!("{application}.{first}");
        let base = bindings.get(&key).cloned().unwrap_or(key);
        return if rest.is_empty() {
            base
        } else {
            format!("{base}.{rest}")
        };
    }
    if let Some((base, args)) = generic(ty) {
        let base = bindings.get(base).map_or(base, String::as_str);
        return format!(
            "{base}<{}>",
            args.iter()
                .map(|a| substitute(a, bindings))
                .collect::<Vec<_>>()
                .join(",")
        );
    }
    ty.into()
}
fn builtin_arity(name: &str) -> Option<usize> {
    match name {
        "Option" | "vector" => Some(1),
        "array" | "map" => Some(2),
        _ => None,
    }
}
fn builtin(name: &str) -> bool {
    ["Bool", "U32", "String", "Bytes", "Unit"].contains(&name)
}
struct Specializer<'a> {
    templates: BTreeMap<String, Template>,
    arities: BTreeMap<String, usize>,
    manifest: &'a Manifest,
    seen: BTreeSet<String>,
    data: Vec<DataDeclaration>,
    outcomes: Vec<OutcomeContract>,
}
impl Specializer<'_> {
    fn validate_type(&self, ty: &str, params: &[String], at: Span) -> Result<()> {
        if let Some((_, inner)) = manifest::reference_inner(ty) {
            return self.validate_type(inner, params, at);
        }
        if params.iter().any(|p| p == ty) {
            return Ok(());
        }
        if let Some((base, args)) = generic(ty) {
            if params.iter().any(|p| p == base) {
                return Err(error(at, "type parameters cannot take type arguments"));
            }
            let arity = builtin_arity(base)
                .or_else(|| self.arities.get(base).copied())
                .ok_or_else(|| error(at, format!("unknown generic type `{base}`")))?;
            if arity == 0 || arity != args.len() {
                return Err(error(
                    at,
                    format!(
                        "type `{base}` expects {arity} type arguments, found {}",
                        args.len()
                    ),
                ));
            }
            if base == "map"
                && !params.iter().any(|p| p == args[0])
                && !["Bool", "U32", "String"].contains(&args[0])
            {
                return Err(error(
                    at,
                    "map key must be bool, u32, string, or a type parameter checked at specialization",
                ));
            }
            for (i, arg) in args.iter().enumerate() {
                if base == "array" && i == 1 {
                    if arg.parse::<u32>().is_err() {
                        return Err(error(at, "array length must be a u32 literal"));
                    }
                } else {
                    self.validate_type(arg, params, at)?;
                }
            }
            return Ok(());
        }
        if let Some(arity) = self.arities.get(ty) {
            if *arity != 0 {
                return Err(error(
                    at,
                    format!("generic type `{ty}` requires {arity} explicit type arguments"),
                ));
            }
            return Ok(());
        }
        if builtin(ty) || self.manifest.type_def(ty).is_some() {
            Ok(())
        } else {
            Err(error(
                at,
                format!("unknown type `{ty}` in data declaration"),
            ))
        }
    }
    fn require(&mut self, ty: &str, at: Span, depth: usize) -> Result<()> {
        if depth > 64 || ty.len() > 16384 {
            return Err(error(
                at,
                "generic layout exceeds the specialization depth or type-size budget",
            ));
        }
        if let Some((_, inner)) = manifest::reference_inner(ty) {
            return self.require(inner, at, depth + 1);
        }
        let Some((base, args)) = generic(ty) else {
            if self.templates.contains_key(ty) {
                return Err(error(
                    at,
                    format!("generic type `{ty}` requires explicit type arguments"),
                ));
            }
            return Ok(());
        };
        let args = args.iter().map(|a| (*a).to_string()).collect::<Vec<_>>();
        if let Some(template) = self.templates.get(base).cloned() {
            if args.len() != template.parameters().len() {
                return Err(error(
                    at,
                    format!(
                        "type `{base}` expects {} type arguments, found {}",
                        template.parameters().len(),
                        args.len()
                    ),
                ));
            }
            for arg in &args {
                if manifest::reference_inner(arg).is_some() {
                    return Err(error(
                        at,
                        "reference type arguments require generic contained-loan propagation; use a reference field such as ref t instead",
                    ));
                }
                self.validate_type(arg, &[], at)?;
                self.require(arg, at, depth + 1)?;
            }
            if !self.seen.insert(ty.into()) {
                return Ok(());
            }
            if self.seen.len() > 1024 {
                return Err(error(
                    at,
                    "generic layouts exceed the 1024-specialization budget",
                ));
            }
            let bindings = template.parameters().iter().cloned().zip(args).collect();
            let map_fields = |fs: &mut Vec<Field>| {
                for f in fs {
                    f.ty = substitute(&f.ty, &bindings);
                }
            };
            let mut instance = template;
            match &mut instance {
                Template::Data(d) => {
                    d.name = ty.into();
                    d.type_parameters.clear();
                    match &mut d.body {
                        DataBody::Struct(fs) => map_fields(fs),
                        DataBody::Variant(os) => {
                            for o in os {
                                map_fields(&mut o.fields)
                            }
                        }
                    }
                }
                Template::Outcome(d) => {
                    d.name = ty.into();
                    d.type_parameters.clear();
                    for o in &mut d.outcomes {
                        map_fields(&mut o.fields)
                    }
                }
            }
            for field in instance.fields() {
                self.require(&field.ty, at, depth + 1)?;
            }
            match instance {
                Template::Data(d) => self.data.push(d),
                Template::Outcome(d) => self.outcomes.push(d),
            }
        } else {
            if self.arities.contains_key(base) {
                return Err(error(at, format!("type `{base}` is not generic")));
            }
            for arg in args {
                if !arg.bytes().all(|b| b.is_ascii_digit()) {
                    self.require(&arg, at, depth + 1)?;
                }
            }
        }
        Ok(())
    }
    fn fields(&mut self, fs: &[Field], at: Span) -> Result<()> {
        for f in fs {
            self.require(&f.ty, at, 0)?;
        }
        Ok(())
    }
    fn expr(&mut self, e: &Expr, at: Span) -> Result<()> {
        e.checked(at, |at| {
            match &e.kind {
                ExprKind::Call(name, args) | ExprKind::Construct(name, args) => {
                    if self.templates.contains_key(name) {
                        self.require(name, at, 0)?;
                    }
                    if let Some(end) = name.rfind('>') {
                        self.require(&name[..=end], at, 0)?;
                    }
                    for a in args {
                        self.expr(a, at)?;
                    }
                }
                ExprKind::NamedRecord(name, fs) => {
                    self.require(name, at, 0)?;
                    for (_, e) in fs {
                        self.expr(e, at)?;
                    }
                }
                ExprKind::List(args) => {
                    for a in args {
                        self.expr(a, at)?;
                    }
                }
                ExprKind::Borrow(e)
                | ExprKind::BorrowMutable(e)
                | ExprKind::Deref(e)
                | ExprKind::Project(e, _)
                | ExprKind::Unary(e) => self.expr(e, at)?,
                ExprKind::Binary(_, a, b) => {
                    self.expr(a, at)?;
                    self.expr(b, at)?;
                }
                _ => (),
            }
            Ok(())
        })
    }
    fn outcome(&mut self, e: &OutcomeExpr) -> Result<()> {
        match e {
            OutcomeExpr::Forward(_) => (),
            OutcomeExpr::Construct(at, _, args) => {
                for a in args {
                    self.expr(a, *at)?;
                }
            }
            OutcomeExpr::Delta(at, e, arms) => {
                self.expr(e, *at)?;
                for arm in arms {
                    self.fields(&arm.pattern.bindings, arm.span)?;
                    self.outcome(&arm.result)?;
                }
            }
        }
        Ok(())
    }
    fn statement(&mut self, at: Span, s: &Statement) -> Result<()> {
        match s {
            Statement::Forward(e)
            | Statement::Invoke(e)
            | Statement::Drop(e)
            | Statement::Emit(e)
            | Statement::Fault(e) => self.expr(e, at)?,
            Statement::Bind(_, ty, e, _) | Statement::Capture(_, ty, e, _) => {
                if let Some(ty) = ty {
                    self.require(ty, at, 0)?;
                }
                self.expr(e, at)?;
            }
            Statement::Assign(a, b) => {
                self.expr(a, at)?;
                self.expr(b, at)?;
            }
            Statement::Exit(_, args) | Statement::Again(_, args) => {
                for a in args {
                    self.expr(a, at)?;
                }
            }
            Statement::ExitOutcome(e) => self.outcome(e)?,
            Statement::BindOutcome(_, ty, e, _) => {
                if let Some(ty) = ty {
                    self.require(ty, at, 0)?;
                }
                self.outcome(e)?;
            }
            Statement::ExitName(_) => (),
            Statement::Delta(_, e, arms) => {
                self.expr(e, at)?;
                for a in arms {
                    self.fields(&a.bindings, at)?;
                    if let Some(exit) = &a.termination {
                        self.statement(exit.0, &exit.1)?;
                    }
                }
            }
        }
        Ok(())
    }
    fn items(&mut self, items: &[Item]) -> Result<()> {
        for item in items {
            match item {
                Item::Alias(..) => (),
                Item::Group(_, _, body) | Item::Over(_, _, body) => self.items(body)?,
                Item::Loop(at, _, ps, body) => {
                    for p in ps {
                        self.require(&p.field.ty, *at, 0)?;
                        self.expr(&p.initial, *at)?;
                    }
                    self.items(body)?;
                }
                Item::Guard(_, statements) => {
                    for (at, s) in statements {
                        self.statement(*at, s)?;
                    }
                }
            }
        }
        Ok(())
    }
}
fn dependencies(ty: &str, params: &[String], result: &mut BTreeSet<String>) {
    if params.iter().any(|p| p == ty) {
        return;
    }
    if let Some((base, args)) = generic(ty) {
        result.insert(base.into());
        for a in args {
            dependencies(a, params, result);
        }
    } else {
        result.insert(ty.into());
    }
}
fn acyclic(
    name: &str,
    graph: &BTreeMap<String, BTreeSet<String>>,
    active: &mut BTreeSet<String>,
    done: &mut BTreeSet<String>,
    at: Span,
) -> Result<()> {
    if done.contains(name) {
        return Ok(());
    }
    if !active.insert(name.into()) {
        return Err(error(
            at,
            format!("recursive data declarations are not supported (`{name}`)"),
        ));
    }
    if let Some(edges) = graph.get(name) {
        for n in edges {
            if graph.contains_key(n) {
                acyclic(n, graph, active, done, at)?;
            }
        }
    }
    active.remove(name);
    done.insert(name.into());
    Ok(())
}
pub(crate) fn instantiate(
    mut unit: Unit,
    manifest: &Manifest,
    extra: &[(String, Span)],
) -> Result<Unit> {
    crate::origins::schemas(&unit)?;
    if !unit.data.iter().any(|d| !d.type_parameters.is_empty())
        && !unit.contracts.iter().any(|d| !d.type_parameters.is_empty())
        && extra.is_empty()
    {
        return Ok(unit);
    }
    let declarations = unit
        .data
        .iter()
        .cloned()
        .map(Template::Data)
        .chain(unit.contracts.iter().cloned().map(Template::Outcome))
        .collect::<Vec<_>>();
    let mut s = Specializer {
        templates: BTreeMap::new(),
        arities: BTreeMap::new(),
        manifest,
        seen: BTreeSet::new(),
        data: vec![],
        outcomes: vec![],
    };
    for d in &declarations {
        let name = d.name();
        if builtin(&manifest::canonical_type(name))
            || builtin_arity(name).is_some()
            || ["auto", "slice", "outcome", "option"].contains(&name)
            || manifest.type_def(name).is_some()
            || s.arities
                .insert(name.into(), d.parameters().len())
                .is_some()
        {
            return Err(error(
                d.span(),
                format!("invalid or duplicate data name `{name}`"),
            ));
        }
        if !d.parameters().is_empty() {
            s.templates.insert(name.into(), d.clone());
        }
    }
    let mut graph = BTreeMap::new();
    for d in &declarations {
        let mut params = BTreeSet::new();
        for p in d.parameters() {
            if !params.insert(p)
                || builtin(&manifest::canonical_type(p))
                || builtin_arity(p).is_some()
                || ["auto", "outcome", "option", "slice"].contains(&p.as_str())
            {
                return Err(error(
                    d.span(),
                    format!("duplicate or shadowing type parameter `{p}`"),
                ));
            }
        }
        for names in d.all_names() {
            let mut seen = BTreeSet::new();
            for n in names {
                if !seen.insert(n) {
                    return Err(error(
                        d.span(),
                        format!("duplicate field or alternative `{n}`"),
                    ));
                }
            }
        }
        if matches!(d,Template::Data(DataDeclaration{body:DataBody::Variant(os),..}) if os.is_empty())
            || matches!(d,Template::Outcome(OutcomeContract{outcomes:os,..}) if os.is_empty())
        {
            return Err(error(d.span(), "variant or outcome needs an alternative"));
        }
        let mut edges = BTreeSet::new();
        for f in d.fields() {
            s.validate_type(&f.ty, d.parameters(), d.span())?;
            dependencies(&f.ty, d.parameters(), &mut edges);
        }
        graph.insert(d.name().into(), edges);
    }
    let mut done = BTreeSet::new();
    for d in &declarations {
        acyclic(d.name(), &graph, &mut BTreeSet::new(), &mut done, d.span())?;
    }
    unit.data.retain(|d| d.type_parameters.is_empty());
    unit.contracts.retain(|d| d.type_parameters.is_empty());
    for d in &declarations {
        if d.parameters().is_empty() {
            for f in d.fields() {
                s.require(&f.ty, d.span(), 0)?;
            }
        }
    }
    for r in &unit.regions {
        s.fields(&r.parameters, r.span)?;
        for o in &r.exits {
            s.fields(&o.fields, r.span)?;
        }
        if let Some(ty) = &r.exit_contract {
            s.require(ty, r.span, 0)?;
        }
        s.items(&r.body)?;
    }
    for (ty, at) in extra {
        s.require(ty, *at, 0)?;
    }
    unit.data.extend(s.data);
    unit.contracts.extend(s.outcomes);
    Ok(unit)
}
