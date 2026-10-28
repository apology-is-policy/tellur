//! Generic controls are checked with opaque, must-complete type parameters before
//! concrete specialization. Sealed bounds grant only their declared permissions.
use crate::{
    Diagnostic, Result, Span,
    collection_support::generic,
    generics::substitute,
    ir,
    manifest::{self, Manifest, TypeDef},
    syntax::*,
};
use std::collections::{BTreeMap, BTreeSet};
fn error(at: Span, message: impl Into<String>) -> Diagnostic {
    Diagnostic::new("E_GENERIC", at, message)
}
fn application(name: &str) -> Option<(&str, Vec<&str>)> {
    generic(name)
}
fn map_name(name: &str, bindings: &BTreeMap<String, String>) -> String {
    substitute(name, bindings)
}

fn rewrite_fields(fields: &mut [Field], bindings: &BTreeMap<String, String>) {
    for f in fields {
        f.ty = substitute(&f.ty, bindings);
    }
}
fn rewrite_expr(e: &mut Expr, b: &BTreeMap<String, String>) {
    match e {
        Expr::Call(n, args) | Expr::Construct(n, args) => {
            *n = map_name(n, b);
            for a in args {
                rewrite_expr(a, b);
            }
        }
        Expr::NamedRecord(n, fs) => {
            *n = substitute(n, b);
            for (_, e) in fs {
                rewrite_expr(e, b);
            }
        }
        Expr::List(args) => {
            for a in args {
                rewrite_expr(a, b);
            }
        }
        Expr::Borrow(e)
        | Expr::BorrowMutable(e)
        | Expr::Deref(e)
        | Expr::Project(e, _)
        | Expr::Unary(e) => rewrite_expr(e, b),
        Expr::Binary(_, a, c) => {
            rewrite_expr(a, b);
            rewrite_expr(c, b);
        }
        _ => (),
    }
}
fn rewrite_outcome(e: &mut OutcomeExpr, b: &BTreeMap<String, String>) {
    match e {
        OutcomeExpr::Forward(_) => (),
        OutcomeExpr::Construct(_, _, args) => {
            for a in args {
                rewrite_expr(a, b);
            }
        }
        OutcomeExpr::Delta(_, e, arms) => {
            rewrite_expr(e, b);
            for arm in arms {
                rewrite_fields(&mut arm.pattern.bindings, b);
                rewrite_outcome(&mut arm.result, b);
            }
        }
    }
}
fn rewrite_items(items: &mut [Item], b: &BTreeMap<String, String>) {
    for i in items {
        match i {
            Item::Alias(..) => (),
            Item::Over(_, _, body) => rewrite_items(body, b),
            Item::Loop(_, _, ps, body) => {
                for p in ps {
                    p.field.ty = substitute(&p.field.ty, b);
                    rewrite_expr(&mut p.initial, b);
                }
                rewrite_items(body, b);
            }
            Item::Guard(_, ss) => {
                for (_, s) in ss {
                    match s {
                        Statement::Bind(_, ty, e, _) | Statement::Capture(_, ty, e, _) => {
                            if let Some(t) = ty {
                                *t = substitute(t, b);
                            }
                            rewrite_expr(e, b);
                        }
                        Statement::BindOutcome(_, ty, e, _) => {
                            if let Some(t) = ty {
                                *t = substitute(t, b);
                            }
                            rewrite_outcome(e, b);
                        }
                        Statement::Forward(e)
                        | Statement::Invoke(e)
                        | Statement::Drop(e)
                        | Statement::Emit(e) => rewrite_expr(e, b),
                        Statement::Assign(a, c) => {
                            rewrite_expr(a, b);
                            rewrite_expr(c, b);
                        }
                        Statement::Exit(_, args) | Statement::Again(_, args) => {
                            for a in args {
                                rewrite_expr(a, b);
                            }
                        }
                        Statement::ExitOutcome(e) => rewrite_outcome(e, b),
                        Statement::ExitName(_) => (),
                        Statement::Delta(_, e, arms) => {
                            rewrite_expr(e, b);
                            for arm in arms {
                                rewrite_fields(&mut arm.bindings, b);
                            }
                        }
                    }
                }
            }
        }
    }
}
pub(crate) fn rewrite_region(r: &mut Region, b: &BTreeMap<String, String>) {
    rewrite_fields(&mut r.parameters, b);
    for o in &mut r.exits {
        rewrite_fields(&mut o.fields, b);
    }
    if let Some(t) = &mut r.exit_contract {
        *t = substitute(t, b);
    }
    rewrite_items(&mut r.body, b);
}
type Visit<'a> = dyn FnMut(&str, Span) -> Result<()> + 'a;
fn type_calls(ty: &str, at: Span, visit: &mut Visit<'_>) -> Result<()> {
    if let Some((_, inner)) = manifest::reference_inner(ty) {
        return type_calls(inner, at, visit);
    }
    if let Some(producer) = manifest::outcome_inner(ty) {
        visit(producer, at)?;
    }
    if let Some((_, args)) = generic(ty) {
        for arg in args {
            type_calls(arg, at, visit)?;
        }
    }
    Ok(())
}
fn field_calls(fs: &[Field], at: Span, v: &mut Visit<'_>) -> Result<()> {
    for f in fs {
        type_calls(&f.ty, at, v)?;
    }
    Ok(())
}
fn expr_calls(e: &Expr, at: Span, v: &mut Visit<'_>) -> Result<()> {
    match e {
        Expr::Call(n, args) => {
            v(n, at)?;
            for a in args {
                expr_calls(a, at, v)?;
            }
        }
        Expr::Construct(n, args) => {
            if let Some(end) = n.rfind('>') {
                type_calls(&n[..=end], at, v)?;
            }
            for a in args {
                expr_calls(a, at, v)?;
            }
        }
        Expr::NamedRecord(n, fs) => {
            type_calls(n, at, v)?;
            for (_, e) in fs {
                expr_calls(e, at, v)?;
            }
        }
        Expr::List(args) => {
            for a in args {
                expr_calls(a, at, v)?;
            }
        }
        Expr::Borrow(e)
        | Expr::BorrowMutable(e)
        | Expr::Deref(e)
        | Expr::Project(e, _)
        | Expr::Unary(e) => expr_calls(e, at, v)?,
        Expr::Binary(_, a, b) => {
            expr_calls(a, at, v)?;
            expr_calls(b, at, v)?;
        }
        _ => (),
    }
    Ok(())
}
fn outcome_calls(e: &OutcomeExpr, v: &mut Visit<'_>) -> Result<()> {
    match e {
        OutcomeExpr::Forward(_) => (),
        OutcomeExpr::Construct(at, _, args) => {
            for a in args {
                expr_calls(a, *at, v)?;
            }
        }
        OutcomeExpr::Delta(at, e, arms) => {
            expr_calls(e, *at, v)?;
            for arm in arms {
                field_calls(&arm.pattern.bindings, arm.span, v)?;
                outcome_calls(&arm.result, v)?;
            }
        }
    }
    Ok(())
}
fn item_calls(items: &[Item], v: &mut Visit<'_>) -> Result<()> {
    for i in items {
        match i {
            Item::Alias(..) => (),
            Item::Over(_, _, body) => item_calls(body, v)?,
            Item::Loop(at, _, ps, body) => {
                for p in ps {
                    type_calls(&p.field.ty, *at, v)?;
                    expr_calls(&p.initial, *at, v)?;
                }
                item_calls(body, v)?;
            }
            Item::Guard(_, ss) => {
                for (at, s) in ss {
                    match s {
                        Statement::Bind(_, ty, e, _) | Statement::Capture(_, ty, e, _) => {
                            if let Some(t) = ty {
                                type_calls(t, *at, v)?;
                            }
                            expr_calls(e, *at, v)?;
                        }
                        Statement::BindOutcome(_, ty, e, _) => {
                            if let Some(t) = ty {
                                type_calls(t, *at, v)?;
                            }
                            outcome_calls(e, v)?;
                        }
                        Statement::Forward(e)
                        | Statement::Invoke(e)
                        | Statement::Drop(e)
                        | Statement::Emit(e) => expr_calls(e, *at, v)?,
                        Statement::Assign(a, b) => {
                            expr_calls(a, *at, v)?;
                            expr_calls(b, *at, v)?;
                        }
                        Statement::Exit(_, args) | Statement::Again(_, args) => {
                            for a in args {
                                expr_calls(a, *at, v)?;
                            }
                        }
                        Statement::ExitOutcome(e) => outcome_calls(e, v)?,
                        Statement::ExitName(_) => (),
                        Statement::Delta(_, e, arms) => {
                            expr_calls(e, *at, v)?;
                            for a in arms {
                                field_calls(&a.bindings, *at, v)?;
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(())
}
fn calls(r: &Region, v: &mut Visit<'_>) -> Result<()> {
    field_calls(&r.parameters, r.span, v)?;
    for o in &r.exits {
        field_calls(&o.fields, r.span, v)?;
    }
    if let Some(t) = &r.exit_contract {
        type_calls(t, r.span, v)?;
    }
    item_calls(&r.body, v)
}
fn acyclic(
    name: &str,
    graph: &BTreeMap<String, BTreeSet<String>>,
    state: &mut BTreeMap<String, u8>,
    at: Span,
) -> Result<()> {
    match state.get(name) {
        Some(2) => return Ok(()),
        Some(1) => {
            return Err(Diagnostic::new(
                "E_RECURSION",
                at,
                format!("cyclic generic control call graph at `{name}`"),
            ));
        }
        _ => (),
    }
    state.insert(name.into(), 1);
    for edge in &graph[name] {
        acyclic(edge, graph, state, at)?;
    }
    state.insert(name.into(), 2);
    Ok(())
}
pub(crate) struct TypeArgument {
    pub ty: String,
    pub span: Span,
    pub bounds: Vec<String>,
}
struct Expanded {
    unit: Unit,
    arguments: Vec<TypeArgument>,
}
struct Expander {
    templates: BTreeMap<String, Region>,
    ordinary: BTreeMap<String, Region>,
    seen_ordinary: BTreeSet<String>,
    seen: BTreeSet<String>,
    pending: Vec<(Region, usize)>,
    output: Expanded,
}
impl Expander {
    fn new(mut unit: Unit) -> Self {
        let templates = unit
            .regions
            .iter()
            .filter(|r| !r.type_parameters.is_empty())
            .map(|r| (r.name.clone(), r.clone()))
            .collect();
        let ordinary = unit
            .regions
            .iter()
            .filter(|r| r.type_parameters.is_empty())
            .map(|r| (r.name.clone(), r.clone()))
            .collect();
        unit.regions.clear();
        Self {
            templates,
            ordinary,
            seen_ordinary: BTreeSet::new(),
            seen: BTreeSet::new(),
            pending: vec![],
            output: Expanded {
                unit,
                arguments: vec![],
            },
        }
    }
    fn require(&mut self, name: &str, at: Span, depth: usize) -> Result<()> {
        let Some((base, args)) = application(name) else {
            if let Some((app, member)) = crate::collection_support::qualified_member(name)
                && let Some((trait_name, args)) = application(app)
                && self.output.unit.traits.iter().any(|t| t.name == trait_name)
            {
                if args.len() != 1
                    || !crate::traits::has_witness(
                        &self.output.unit.trait_checks,
                        trait_name,
                        args[0],
                    )
                {
                    return Err(Diagnostic::new(
                        "E_BOUND",
                        at,
                        format!(
                            "no `{trait_name}` implementation or bound for `{}`",
                            args.join(",")
                        ),
                    ));
                }
                if !self.ordinary.contains_key(name) {
                    return Err(Diagnostic::new(
                        "E_TRAIT",
                        at,
                        format!("unknown operation `{member}` of `{app}`"),
                    ));
                }
            }

            if self.templates.contains_key(name) {
                return Err(error(
                    at,
                    format!("generic control `{name}` requires explicit type arguments"),
                ));
            }
            if let Some(region) = self.ordinary.get(name)
                && self.seen_ordinary.insert(name.into())
            {
                self.pending.push((region.clone(), depth));
            }
            return Ok(());
        };
        let Some(template) = self.templates.get(base) else {
            return Ok(());
        };
        if depth > 64 || name.len() > 16384 {
            return Err(error(
                at,
                "generic control exceeds the specialization depth or type-size budget",
            ));
        }
        if args.len() != template.type_parameters.len() {
            return Err(error(
                at,
                format!(
                    "control `{base}` expects {} type arguments, found {}",
                    template.type_parameters.len(),
                    args.len()
                ),
            ));
        }
        if !self.seen.insert(name.into()) {
            return Ok(());
        }
        if self.seen.len() > 1024 {
            return Err(error(
                at,
                "generic controls exceed the 1024-specialization budget",
            ));
        }
        let lexical_bindings: BTreeMap<String, String> = template
            .type_parameters
            .iter()
            .cloned()
            .zip(args.iter().map(|s| (*s).to_string()))
            .collect();
        self.output
            .arguments
            .extend(
                args.iter()
                    .zip(&template.type_parameters)
                    .map(|(s, p)| TypeArgument {
                        ty: (*s).into(),
                        span: at,
                        bounds: template.type_bounds[p].clone(),
                    }),
            );
        let mut instance = template.clone();
        instance.name = name.into();
        instance.type_parameters.clear();
        instance.type_bounds.clear();
        // Resolve lexical parameters before looking up concrete trait projections.
        // An exact projection key may otherwise capture a shadowed type name.
        rewrite_region(&mut instance, &lexical_bindings);
        rewrite_region(&mut instance, &self.output.unit.trait_types);
        instance.type_bindings = lexical_bindings;
        self.pending.push((instance, depth));
        Ok(())
    }
    fn finish(mut self, seeds: &[(String, Span)]) -> Result<Expanded> {
        if seeds.is_empty() {
            for r in self.ordinary.values().cloned().collect::<Vec<_>>() {
                self.require(&r.name, r.span, 0)?;
            }
        }
        for (name, at) in seeds {
            self.require(name, *at, 0)?;
        }
        while let Some((mut r, depth)) = self.pending.pop() {
            // Public generic interfaces are checked at their abstract definition.
            // A private caller's specialization is not itself a new exported API.
            let specialized =
                application(&r.name).is_some_and(|(base, _)| self.templates.contains_key(base));
            if specialized {
                r.public = r.public && seeds.iter().any(|(name, _)| name == &r.name);
            }
            calls(&r, &mut |n, at| {
                self.require(n, at, depth + usize::from(specialized))
            })?;
            self.output.unit.regions.push(r);
        }
        Ok(self.output)
    }
}
pub(crate) fn compile(
    unit: Unit,
    manifest: Manifest,
    visibility: &crate::modules::Visibility,
) -> Result<ir::CheckedProgram> {
    let templates = unit
        .regions
        .iter()
        .filter(|r| !r.type_parameters.is_empty())
        .cloned()
        .collect::<Vec<_>>();
    if templates.is_empty() {
        return crate::check::compile_concrete_unit(unit, manifest, visibility, &[]);
    }
    let names = unit
        .regions
        .iter()
        .map(|r| r.name.clone())
        .collect::<BTreeSet<_>>();
    if names.len() != unit.regions.len() {
        return Err(error(templates[0].span, "duplicate control declaration"));
    }
    let mut graph = BTreeMap::new();
    for r in &unit.regions {
        let mut edges = BTreeSet::new();
        calls(r, &mut |n, _| {
            let base = application(n).map_or(n, |(base, _)| base);
            if names.contains(base) {
                edges.insert(base.into());
            }
            Ok(())
        })?;
        graph.insert(r.name.clone(), edges);
    }
    let mut states = BTreeMap::new();
    for r in &unit.regions {
        acyclic(&r.name, &graph, &mut states, r.span)?;
    }
    let mut symbolic_unit = unit.clone();
    let mut symbolic_manifest = manifest.clone();
    let mut seeds = vec![];
    let mut parameters = BTreeMap::new();
    let mut counter = 0usize;
    for template in &templates {
        if manifest.primitives.iter().any(|p| p.name == template.name)
            || crate::collections::is_operation(&template.name)
            || crate::collections::is_length(&template.name)
        {
            return Err(Diagnostic::new(
                "E_NAME",
                template.span,
                format!("duplicate operation `{}`", template.name),
            ));
        }
        let mut seen = BTreeSet::new();
        let mut args = vec![];
        for p in &template.type_parameters {
            if !seen.insert(p)
                || [
                    "Bool", "U32", "String", "Bytes", "Unit", "Option", "array", "vector", "map",
                    "slice", "outcome", "auto",
                ]
                .contains(&manifest::canonical_type(p).as_str())
            {
                return Err(error(
                    template.span,
                    format!("invalid or duplicate type parameter `{p}`"),
                ));
            }
            if counter >= 4096 {
                return Err(error(
                    template.span,
                    "generic definition checking exceeds the 4096-abstract-type budget",
                ));
            }
            let name = loop {
                let n = format!("_path_poly_{counter}_parameter");
                counter += 1;
                if symbolic_manifest.type_def(&n).is_none() {
                    break n;
                }
            };
            let capabilities = manifest::Capabilities::from_bounds(&template.type_bounds[p]);
            symbolic_manifest
                .abstract_types
                .insert(name.clone(), capabilities);
            symbolic_manifest.types.push(TypeDef::Opaque {
                name: name.clone(),
                linear: !capabilities.drop,
            });
            for bound in &template.type_bounds[p] {
                if !["copy", "clone", "drop"].contains(&bound.as_str()) {
                    crate::traits::abstract_witness(
                        &mut symbolic_unit,
                        &mut symbolic_manifest,
                        bound,
                        &name,
                        p,
                        &mut counter,
                        &mut parameters,
                    )?;
                }
            }
            parameters.insert(name.clone(), p.clone());
            args.push(name);
        }
        seeds.push((
            format!("{}<{}>", template.name, args.join(",")),
            template.span,
        ));
    }
    let checked=(|| {
        let symbolic=Expander::new(symbolic_unit).finish(&seeds)?;
        crate::check::compile_concrete_unit(symbolic.unit,symbolic_manifest,visibility,&symbolic.arguments)
    })().map_err(|mut e|{
        for (internal,name) in &parameters {e.message=e.message.replace(internal,name);for note in &mut e.notes {note.message=note.message.replace(internal,name);}}
        // Keep the precise diagnostic code while making the universal-check context explicit.
        if templates.iter().any(|t| (e.span.line, e.span.column) >= (t.span.line, t.span.column) && (e.span.line, e.span.column) <= (t.end.line, t.end.column)) { e.message=format!("generic definition check: {} (type parameters have only their declared capabilities)",e.message); } e
    })?;
    let concrete = Expander::new(unit).finish(&[])?;
    let mut result = crate::check::compile_concrete_unit(
        concrete.unit,
        manifest,
        visibility,
        &concrete.arguments,
    )?;
    // Prefer the universally checked type over any concrete instantiation at
    // the same source location. A generic body's hover must not depend on callers.
    for (template, (seed, _)) in templates.iter().zip(&seeds) {
        let within = |b: &ir::InferredBinding| {
            (b.span.line, b.span.column) >= (template.span.line, template.span.column)
                && (b.span.line, b.span.column) <= (template.end.line, template.end.column)
        };
        result.inferred_bindings.retain(|b| !within(b));
        for binding in checked
            .inferred_bindings
            .iter()
            .filter(|b| b.control == *seed)
        {
            let mut binding = binding.clone();
            for (internal, name) in &parameters {
                binding.display_type = binding.display_type.replace(internal, name);
            }
            result.inferred_bindings.push(binding);
        }
    }
    for (template, (seed, _)) in templates.iter().zip(&seeds) {
        let region = checked
            .region(seed)
            .expect("a checked symbolic instance exists");
        let mut exits = region.exits.clone();
        for o in &mut exits {
            for f in &mut o.fields {
                for (internal, name) in &parameters {
                    f.ty = f.ty.replace(internal, name);
                }
            }
        }
        result.generic_controls.push(ir::GenericControl {
            name: template.name.clone(),
            type_parameters: template.type_parameters.clone(),
            type_bounds: template.type_bounds.clone(),
            exits,
        });
    }
    Ok(result)
}
