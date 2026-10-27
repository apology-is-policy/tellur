//! Structural/type validation at transformation and backend boundaries.
//! Source ownership and path coverage remain the front end's obligations.
use crate::{
    Diagnostic, Result, Span,
    ir::*,
    manifest::{Field, TypeDef},
};
use std::collections::{BTreeMap, BTreeSet};
fn error(message: impl Into<String>) -> Diagnostic {
    Diagnostic::new("E_IR", Span::default(), message)
}
fn same(a: &str, b: &str) -> Result<()> {
    if a == b {
        Ok(())
    } else {
        Err(error(format!("IR type mismatch: {a}, expected {b}")))
    }
}
struct Verifier<'a> {
    program: &'a CheckedProgram,
    region: &'a Region,
    instructions: BTreeSet<Id>,
}
impl Verifier<'_> {
    fn slot(&self, id: Id) -> Result<&str> {
        self.region
            .slots
            .get(&id)
            .map(|s| s.ty.as_str())
            .ok_or_else(|| error(format!("unknown slot {id}")))
    }
    fn atom(&self, id: Id) -> Result<()> {
        if self.region.atoms.contains_key(&id) {
            Ok(())
        } else {
            Err(error(format!("unknown atom {id}")))
        }
    }
    fn guard(&self, guard: &Guard) -> Result<()> {
        match guard {
            Guard::True => Ok(()),
            Guard::Atom(id) => self.atom(*id),
            Guard::And(a, b) | Guard::Or(a, b) => {
                self.guard(a)?;
                self.guard(b)
            }
        }
    }
    fn expr(&self, e: &Expr) -> Result<String> {
        let m = self.program.manifest();
        Ok(match e {
            Expr::Length(e) => {
                let ty = self.expr(e)?;
                let (mutable, inner) = crate::manifest::reference_inner(&ty)
                    .ok_or_else(|| error("length needs a reference"))?;
                if mutable || crate::collection_support::Collection::parse(inner).is_none() {
                    return Err(error("invalid length operand"));
                }
                "U32".into()
            }
            Expr::Pure(name, args) => {
                let p = m
                    .primitives
                    .iter()
                    .find(|p| p.name == *name)
                    .filter(|p| crate::manifest::pure_signature(p).is_some())
                    .ok_or_else(|| error("unknown or invalid pure expression contract"))?;
                let fields = p
                    .parameters
                    .iter()
                    .map(|p| Field {
                        name: p.name.clone(),
                        ty: p.ty.clone(),
                    })
                    .collect::<Vec<_>>();
                self.args(args, &fields)?;
                "String".into()
            }
            Expr::Reference(p, ty) => {
                let (mutable, inner) = crate::manifest::reference_inner(ty)
                    .ok_or_else(|| error("invalid reference type"))?;
                if mutable
                    && p.indirect
                    && !crate::manifest::reference_inner(self.slot(p.root)?)
                        .is_some_and(|(mutable, _)| mutable)
                {
                    return Err(error("exclusive reborrow of a shared reference"));
                }
                same(&self.expr(&Expr::Place(p.clone()))?, inner)?;
                ty.clone()
            }
            Expr::Value(Value::Reference { .. }) => {
                return Err(error("embedded reference literal"));
            }
            Expr::Value(v) => {
                crate::runtime::validate(v, &v.ty(), m)?;
                v.ty()
            }
            Expr::Place(p) | Expr::Move(p) => {
                let mut ty = self.slot(p.root)?;
                if p.indirect {
                    ty = crate::manifest::reference_inner(ty)
                        .ok_or_else(|| error("dereference of non-reference slot"))?
                        .1;
                }
                for index in &p.fields {
                    ty = &m
                        .field_at(ty, *index)
                        .ok_or_else(|| error("invalid place projection"))?
                        .ty;
                }
                ty.into()
            }
            Expr::Project(base, index) => {
                let ty = self.expr(base)?;
                if m.linear(&ty) {
                    return Err(error("owning temporary projection"));
                }
                m.field_at(&ty, *index)
                    .ok_or_else(|| error("invalid projection"))?
                    .ty
                    .clone()
            }
            Expr::Construct(ty, name, args) => {
                m.check_local_storage(ty)?;
                let fields = if let Some(c) = crate::collection_support::Collection::parse(ty) {
                    same(name, c.name())?;
                    if matches!(c, crate::collection_support::Collection::Map(..))
                        && !args.is_empty()
                    {
                        return Err(error("map literals must be empty"));
                    }
                    c.fields(args.len())
                        .ok_or_else(|| error("invalid collection length"))?
                        .into_iter()
                        .map(|ty| Field {
                            name: "element".into(),
                            ty,
                        })
                        .collect()
                } else if let Some(TypeDef::Record {
                    name: record,
                    fields,
                }) = m.type_def(ty)
                {
                    same(name, record)?;
                    fields.clone()
                } else {
                    m.outcomes(ty)
                        .and_then(|outcomes| outcomes.into_iter().find(|o| o.name == *name))
                        .ok_or_else(|| error("unknown IR constructor"))?
                        .fields
                };
                self.args(args, &fields)?;
                ty.clone()
            }
            Expr::Record(ty, initializers) => {
                let Some(TypeDef::Record { fields, .. }) = m.type_def(ty) else {
                    return Err(error("named IR construction requires a record"));
                };
                if fields.len() != initializers.len() {
                    return Err(error("IR record field count mismatch"));
                }
                let mut seen = BTreeSet::new();
                for (index, value) in initializers {
                    let field = fields
                        .get(*index)
                        .ok_or_else(|| error("invalid IR record field index"))?;
                    if !seen.insert(index) {
                        return Err(error("duplicate IR record field index"));
                    }
                    same(&self.expr(value)?, &field.ty)?;
                }
                ty.clone()
            }
            Expr::Not(e) => {
                same(&self.expr(e)?, "Bool")?;
                "Bool".into()
            }
            Expr::Binary(op, a, b) => {
                let ty = self.expr(a)?;
                same(&self.expr(b)?, &ty)?;
                match op.as_str() {
                    "&&" | "||" => same(&ty, "Bool")?,
                    "<" | "<=" | ">" | ">=" => same(&ty, "U32")?,
                    "==" | "!=" if ["Bool", "U32", "String", "Unit"].contains(&ty.as_str()) => {}
                    _ => return Err(error("invalid IR binary operation")),
                };
                "Bool".into()
            }
        })
    }
    fn args(&self, args: &[Expr], fields: &[Field]) -> Result<()> {
        if args.len() != fields.len() {
            return Err(error("IR argument arity mismatch"));
        }
        for (arg, field) in args.iter().zip(fields) {
            same(&self.expr(arg)?, &field.ty)?;
        }
        Ok(())
    }
    fn code(&mut self, code: &[Instruction], loops: &mut BTreeMap<Id, Vec<Field>>) -> Result<()> {
        for i in code {
            if !self.instructions.insert(i.id) {
                return Err(error("duplicate instruction identity"));
            }
            if i.provenance.calls.len() != i.provenance.call_sites.len() {
                return Err(error("incomplete call provenance"));
            }
            self.guard(&i.guard)?;
            match &i.action {
                Action::Bind(id, e) => same(&self.expr(e)?, self.slot(*id)?)?,
                Action::Assign(p, e) => same(&self.expr(e)?, &self.expr(&Expr::Place(p.clone()))?)?,
                Action::Drop(e) => {
                    let ty = self.expr(e)?;
                    if self.program.manifest().linear(&ty) {
                        return Err(error("drop of a must-complete value"));
                    }
                }
                Action::Release(ids) => {
                    for id in ids {
                        self.slot(*id)?;
                    }
                }
                Action::Emit(e) => same(&self.expr(e)?, "String")?,
                Action::Exit(name, args) => self.args(
                    args,
                    &self
                        .region
                        .exits
                        .iter()
                        .find(|o| o.name == *name)
                        .ok_or_else(|| error("unknown exit"))?
                        .fields,
                )?,
                Action::Again { target, args } => self.args(
                    args,
                    loops
                        .get(target)
                        .ok_or_else(|| error("restart outside target loop"))?,
                )?,
                Action::Loop {
                    id,
                    parameters,
                    local_slots,
                    local_atoms,
                    body,
                } => {
                    if loops.contains_key(id) {
                        return Err(error("duplicate active loop identity"));
                    }
                    let fields = parameters
                        .iter()
                        .map(|(id, _)| {
                            Ok(Field {
                                name: id.to_string(),
                                ty: self.slot(*id)?.into(),
                            })
                        })
                        .collect::<Result<Vec<_>>>()?;
                    self.args(
                        &parameters
                            .iter()
                            .map(|(_, e)| e.clone())
                            .collect::<Vec<_>>(),
                        &fields,
                    )?;
                    for slot in local_slots {
                        self.slot(*slot)?;
                    }
                    for atom in local_atoms {
                        self.atom(*atom)?;
                    }
                    for (slot, _) in parameters {
                        if !local_slots.contains(slot) {
                            return Err(error("loop parameter is not iteration-local"));
                        }
                    }
                    loops.insert(*id, fields);
                    self.code(body, loops)?;
                    loops.remove(id);
                }
                Action::Complete {
                    atom,
                    bindings,
                    unwind,
                    release,
                } => {
                    self.atom(*atom)?;
                    if unwind.is_some_and(|id| !loops.contains_key(&id)) {
                        return Err(error("completion leaves unknown loop"));
                    }
                    for (slot, e) in bindings {
                        same(&self.expr(e)?, self.slot(*slot)?)?;
                    }
                    for id in release {
                        self.slot(*id)?;
                        if bindings.iter().any(|(slot, _)| slot == id) {
                            return Err(error("completion releases its caller payload"));
                        }
                    }
                }
                Action::Decide {
                    operation,
                    args,
                    arms,
                } => {
                    let outcomes = match operation {
                        Operation::Variant(e) => {
                            if !args.is_empty() {
                                return Err(error("variant decision has call args"));
                            }
                            self.program
                                .manifest
                                .outcomes(&self.expr(e)?)
                                .ok_or_else(|| error("not a decision type"))?
                        }
                        Operation::BorrowedVariant(e) => {
                            if !args.is_empty() {
                                return Err(error("borrowed variant decision has call args"));
                            }
                            self.program
                                .manifest
                                .borrowed_outcomes(&self.expr(e)?)
                                .ok_or_else(|| error("not a borrowed decision type"))?
                        }
                        Operation::Region(name) => {
                            let r = self
                                .program
                                .region(name)
                                .ok_or_else(|| error("unknown callee"))?;
                            self.args(
                                args,
                                &r.parameters
                                    .iter()
                                    .map(|p| Field {
                                        name: p.name.clone(),
                                        ty: p.ty.clone(),
                                    })
                                    .collect::<Vec<_>>(),
                            )?;
                            r.exits.clone()
                        }
                        Operation::Collection(name) => {
                            let first = args
                                .first()
                                .ok_or_else(|| error("missing collection argument"))?;
                            let ty = self.expr(first)?;
                            let inner = crate::manifest::reference_inner(&ty)
                                .map_or(ty.as_str(), |(_, t)| t);
                            let c = crate::collections::contract(name, inner)
                                .ok_or_else(|| error("invalid collection contract"))?;
                            self.args(
                                args,
                                &c.parameters
                                    .iter()
                                    .map(|p| Field {
                                        name: p.name.clone(),
                                        ty: p.ty.clone(),
                                    })
                                    .collect::<Vec<_>>(),
                            )?;
                            c.outcomes
                        }
                        Operation::Primitive(name) => {
                            let p = self
                                .program
                                .manifest
                                .primitives
                                .iter()
                                .find(|p| p.name == *name)
                                .ok_or_else(|| error("unknown primitive"))?;
                            self.args(
                                args,
                                &p.parameters
                                    .iter()
                                    .map(|p| Field {
                                        name: p.name.clone(),
                                        ty: p.ty.clone(),
                                    })
                                    .collect::<Vec<_>>(),
                            )?;
                            p.outcomes.clone()
                        }
                    };
                    let mut seen = BTreeSet::new();
                    let mut atoms = BTreeSet::new();
                    for arm in arms {
                        if !seen.insert(&arm.outcome) || !atoms.insert(arm.atom) {
                            return Err(error("duplicate decision arm"));
                        }
                        self.atom(arm.atom)?;
                        let o = outcomes
                            .iter()
                            .find(|o| o.name == arm.outcome)
                            .ok_or_else(|| error("unknown decision arm"))?;
                        if arm.slots.len() != o.fields.len() {
                            return Err(error("arm payload arity"));
                        }
                        for (slot, field) in arm.slots.iter().zip(&o.fields) {
                            same(self.slot(*slot)?, &field.ty)?;
                        }
                    }
                    if seen.len() != outcomes.len() {
                        return Err(error("nonexhaustive IR decision"));
                    }
                }
            }
        }
        Ok(())
    }
}
impl CheckedProgram {
    pub fn verify(&self) -> Result<()> {
        let mut foreign = self.manifest.clone();
        foreign
            .types
            .retain(|t| crate::manifest::outcome_inner(t.name()).is_none());
        // Source specializations are concrete nominal layouts. Foreign manifests
        // still accept identifier-only declarations at the front-end boundary.
        let specializations = foreign
            .types
            .iter()
            .filter(|t| crate::collection_support::generic(t.name()).is_some())
            .map(|t| (t.name().to_string(), Span::default()))
            .collect();
        foreign.validate_at(&specializations)?;
        let mut schemas = BTreeSet::new();
        for t in &self.manifest.types {
            if let Some(producer) = crate::manifest::outcome_inner(t.name()) {
                if !schemas.insert(t.name()) {
                    return Err(error("duplicate completion schema"));
                }
                let TypeDef::Variant { constructors, .. } = t else {
                    return Err(error("invalid completion schema"));
                };
                if producer
                    .strip_prefix("$adapter_")
                    .is_some_and(|id| id.parse::<u64>().is_ok())
                {
                    if constructors.is_empty() {
                        return Err(error("empty adapter completion schema"));
                    }
                    let mut labels = BTreeSet::new();
                    for outcome in constructors {
                        if !labels.insert(&outcome.name) {
                            return Err(error("duplicate adapter outcome"));
                        }
                        for field in &outcome.fields {
                            self.manifest.check_local_storage(&field.ty)?;
                        }
                    }
                    continue;
                }
                let expected = self
                    .region(producer)
                    .map(|r| &r.exits)
                    .or_else(|| {
                        self.manifest
                            .primitives
                            .iter()
                            .find(|p| p.name == producer)
                            .map(|p| &p.outcomes)
                    })
                    .ok_or_else(|| error("unknown completion producer"))?;
                if constructors != expected {
                    return Err(error(
                        "completion schema differs from its producer contract",
                    ));
                }
            }
        }
        for r in self.regions() {
            let mut v = Verifier {
                program: self,
                region: r,
                instructions: BTreeSet::new(),
            };
            for p in &r.parameters {
                same(v.slot(p.id)?, &p.ty)?;
            }
            for outcome in &r.exits {
                for (index, field) in outcome.fields.iter().enumerate() {
                    let Some((mutable, inner)) = crate::manifest::reference_inner(&field.ty) else {
                        continue;
                    };
                    let sources = r
                        .borrow_returns
                        .get(&(outcome.name.clone(), index))
                        .filter(|s| !s.is_empty())
                        .ok_or_else(|| error("missing return lifetime contract"))?;
                    for source in sources {
                        verify_source(&self.manifest, r, source, mutable, inner)?;
                    }
                }
            }
            let mut expected_stored = BTreeSet::new();
            for o in &r.exits {
                for (payload, field) in o.fields.iter().enumerate() {
                    let reference = crate::manifest::reference_inner(&field.ty);
                    let storage = reference.map_or(field.ty.as_str(), |(_, t)| t);
                    for (through, step, ty) in reference_leaves(storage, &self.manifest) {
                        let fields = step.fields;
                        let sum_leaf = step.sum_leaf;
                        let key = (
                            o.name.clone(),
                            payload,
                            through.clone(),
                            fields.clone(),
                            sum_leaf.clone(),
                        );
                        expected_stored.insert(key);
                        let contracts = r
                            .stored_returns
                            .iter()
                            .filter(|c| {
                                c.outcome == o.name
                                    && c.payload == payload
                                    && c.through == through
                                    && c.fields == fields
                                    && c.sum_leaf == sum_leaf
                            })
                            .collect::<Vec<_>>();
                        if contracts.len() != 1 || contracts[0].sources.is_empty() {
                            return Err(error(
                                "missing or duplicate stored return lifetime contract",
                            ));
                        }
                        let (mut mutable, inner) = crate::manifest::reference_inner(&ty).unwrap();
                        let mut carrier = storage.to_owned();
                        for step in &through {
                            let reference = step
                                .reference_type(&carrier, &self.manifest)
                                .ok_or_else(|| error("invalid stored reference layer"))?;
                            let (mode, next) = crate::manifest::reference_inner(&reference)
                                .ok_or_else(|| error("stored layer is not a reference"))?;
                            mutable &= mode;
                            carrier = next.into();
                        }
                        for source in &contracts[0].sources {
                            verify_source(
                                &self.manifest,
                                r,
                                source,
                                mutable && !reference.is_some_and(|(m, _)| !m),
                                inner,
                            )?;
                        }
                    }
                }
            }
            if r.stored_returns.len() != expected_stored.len()
                || r.stored_returns.iter().any(|c| {
                    !expected_stored.contains(&(
                        c.outcome.clone(),
                        c.payload,
                        c.through.clone(),
                        c.fields.clone(),
                        c.sum_leaf.clone(),
                    ))
                })
            {
                return Err(error("extra stored return lifetime contracts"));
            }
            v.code(&r.code, &mut BTreeMap::new())?;
        }
        Ok(())
    }
    /// Human-readable original/expanded graph with definitions and call contexts.
    pub fn inspect(&self, entry: &str) -> Result<String> {
        let region = self
            .region(entry)
            .ok_or_else(|| error(format!("unknown region {entry}")))?;
        fn code(items: &[Instruction], indent: usize, out: &mut String) {
            use std::fmt::Write;
            for i in items {
                let p = &i.provenance;
                let _ = writeln!(
                    out,
                    "{}#{} [{}] {}:{}:{} definition #{}{}{}",
                    " ".repeat(indent),
                    i.id,
                    format_args!("{:?}", i.guard),
                    p.region,
                    p.span.line,
                    p.span.column,
                    p.definition,
                    p.site
                        .as_ref()
                        .map(|s| format!(" site {s}"))
                        .unwrap_or_default(),
                    if p.call_sites.is_empty() {
                        String::new()
                    } else {
                        format!(" via {}", p.call_sites.join(" -> "))
                    }
                );
                match &i.action {
                    Action::Loop {
                        id,
                        parameters,
                        local_slots,
                        local_atoms,
                        body,
                    } => {
                        let _ = writeln!(
                            out,
                            "{}loop #{id} parameters {parameters:?}; reset slots {local_slots:?}, atoms {local_atoms:?}",
                            " ".repeat(indent + 2)
                        );
                        code(body, indent + 4, out);
                    }
                    other => {
                        let _ = writeln!(out, "{}{:?}", " ".repeat(indent + 2), other);
                    }
                }
            }
        }
        let mut out = format!(
            "region {} {:?} exits {:?}\nslots: {:?}\natoms: {:?}\n",
            region.name, region.parameters, region.exits, region.slots, region.atoms
        );
        code(&region.code, 0, &mut out);
        if self.source_files.len() > 1 {
            out.push_str(
                "source map (combined line coordinates and original declaration names):\n",
            );
            out.push_str(&serde_json::to_string_pretty(&self.source_files).unwrap());
            out.push('\n');
        }
        Ok(out)
    }
}

fn verify_source(
    m: &crate::manifest::Manifest,
    r: &Region,
    source: &BorrowSource,
    mutable: bool,
    inner: &str,
) -> Result<()> {
    let parameter = r
        .parameters
        .get(source.parameter)
        .ok_or_else(|| error("invalid return input"))?;
    let (input_mutable, ty) = if let Some(c) = &source.contained {
        if (c.fields.is_empty() && c.sum_leaf.is_none()) || c.group.is_empty() {
            return Err(error("invalid contained origin"));
        }
        let mut storage = crate::manifest::reference_inner(&parameter.ty)
            .map_or(parameter.ty.as_str(), |(_, t)| t)
            .to_owned();
        let mut exclusive = true;
        for step in &c.through {
            let reference = step
                .reference_type(&storage, m)
                .ok_or_else(|| error("invalid contained reference layer"))?;
            let (mutable, inner) = crate::manifest::reference_inner(&reference)
                .ok_or_else(|| error("origin layer does not end at a reference"))?;
            exclusive &= mutable;
            storage = inner.into();
        }
        let mut ty = storage.as_str();
        for field in &c.fields {
            ty = &m
                .field_at(ty, *field)
                .ok_or_else(|| error("invalid contained input field"))?
                .ty;
        }
        let tagged = c
            .sum_leaf
            .as_ref()
            .map(|leaf| {
                leaf.reference_type(ty, m)
                    .ok_or_else(|| error("invalid contained input constructor"))
            })
            .transpose()?;
        let (mutable, inner) = crate::manifest::reference_inner(tagged.as_deref().unwrap_or(ty))
            .ok_or_else(|| error("contained input origin is not a reference"))?;
        (mutable && exclusive, inner.to_owned())
    } else {
        let (mutable, inner) = crate::manifest::reference_inner(&parameter.ty)
            .ok_or_else(|| error("return borrows owned input storage"))?;
        (mutable, inner.to_owned())
    };
    if mutable
        && !source.guard
        && (!input_mutable
            || crate::manifest::reference_inner(&parameter.ty).is_some_and(|(m, _)| !m))
    {
        return Err(error("exclusive return borrows shared input origin"));
    }
    let mut ty = ty;
    for field in &source.fields {
        ty = m
            .borrow_field(&ty, *field)
            .ok_or_else(|| error("invalid return projection"))?
            .1;
    }
    if source.guard {
        if !source.opaque || !m.contains_exclusive_references(&ty) {
            return Err(error("invalid carrier guard source"));
        }
    } else if !source.opaque {
        same(&ty, inner)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn malformed_borrowed_decisions_are_rejected() {
        let source = "control main() exits done {option<string> saved=some {\"hello\"};delta borrow saved {some {auto text}=>some,none=>none}[some]:emit(text);[entry]:drop(saved);exit done;}";
        let m = crate::manifest::Manifest::from_json(include_str!("../playground/manifest.json"))
            .unwrap();
        let original = crate::compile(source, m).unwrap();
        for mutation in 0..3 {
            let mut p = original.clone();
            let r = p.regions.get_mut("main").unwrap();
            let action = r
                .code
                .iter_mut()
                .find_map(|i| match &mut i.action {
                    Action::Decide {
                        operation: Operation::BorrowedVariant(e),
                        args,
                        arms,
                    } => Some((e, args, arms)),
                    _ => None,
                })
                .unwrap();
            match mutation {
                0 => *action.0 = Expr::Value(Value::U32(1)),
                1 => action.1.push(Expr::Value(Value::U32(1))),
                _ => r.slots.get_mut(&action.2[0].slots[0]).unwrap().ty = "String".into(),
            }
            assert!(p.verify().is_err());
        }
    }
    #[test]
    fn malformed_tagged_contracts_are_rejected_at_ir_boundaries() {
        let source = "variant view['a] {found {ref string text <- 'a},empty} control make(ref string input) exits ready {view[input] value} {exit ready {view.found {input}};} control relay(view input) exits ready {view[input['a]] value} {exit ready {input};}";
        for source in [source.to_owned(), source.replace("variant view", "outcome view"),
            "control make(ref string input) exits ready {option<ref string>[input] value} {exit ready {some {input}};} control relay(option<ref string> input) exits ready {option<ref string>[input['source]] value} {exit ready {input};}".into()] {
        let m = crate::manifest::Manifest::from_json(include_str!("../playground/manifest.json"))
            .unwrap();
        let original = crate::compile(&source, m).unwrap();
        for mutation in 0..5 {
            let mut program = original.clone();
            let contract = &mut program.regions.get_mut("relay").unwrap().stored_returns[0];
            match mutation {
                0 => contract.sum_leaf.as_mut().unwrap().outcome = "empty".into(),
                1 => contract.sum_leaf.as_mut().unwrap().payload = 99,
                2 => {
                    contract.sources[0]
                        .contained
                        .as_mut()
                        .unwrap()
                        .sum_leaf
                        .as_mut()
                        .unwrap()
                        .outcome = "missing".into()
                }
                3 => contract.sum_leaf = None,
                _ => contract.sources.clear(),
            }
            assert_eq!(program.verify().unwrap_err().code, "E_IR");
        }
        }
    }
    #[test]
    fn carrier_results_require_storage_and_contained_lifetime_contracts() {
        for ty in ["view", "option<ref mutable string>"] {
            let source = format!(
                "struct view['a] {{ref mutable string text <- 'a;}} control lend(ref mutable {ty} input) exits ready... {{exit ready {{input}};}}"
            );
            let m =
                crate::manifest::Manifest::from_json(include_str!("../playground/manifest.json"))
                    .unwrap();
            let original = crate::compile(&source, m).unwrap();
            for mutation in 0..6 {
                let mut p = original.clone();
                let r = p.regions.get_mut("lend").unwrap();
                match mutation {
                    0 => r.stored_returns.clear(),
                    1 => r.borrow_returns.clear(),
                    2 => r.stored_returns[0].sources.clear(),
                    3 => r.stored_returns[0].sources[0].parameter = 99,
                    4 => r.stored_returns[0].fields.push(99),
                    _ => r.stored_returns.push(r.stored_returns[0].clone()),
                }
                assert_eq!(p.verify().unwrap_err().code, "E_IR");
            }
        }
    }
    #[test]
    fn nested_reference_contract_paths_are_verified_independently() {
        let source = "struct view['text] {ref mutable string text <- 'text;} struct job['storage,'text] {ref mutable view['text] target <- 'storage;} control pass(job input) exits ready... {exit ready {input};}";
        let m = crate::manifest::Manifest::from_json(include_str!("../playground/manifest.json"))
            .unwrap();
        let original = crate::compile(source, m).unwrap();
        for mutation in 0..5 {
            let mut p = original.clone();
            let r = p.regions.get_mut("pass").unwrap();
            let contract = r
                .stored_returns
                .iter_mut()
                .find(|c| !c.through.is_empty())
                .unwrap();
            match mutation {
                0 => contract.through.clear(),
                1 => contract.through[0].fields[0] = 99,
                2 => contract.sources[0]
                    .contained
                    .as_mut()
                    .unwrap()
                    .through
                    .clear(),
                3 => contract.sources[0].contained.as_mut().unwrap().through[0].fields[0] = 99,
                _ => contract.sources.clear(),
            }
            assert_eq!(p.verify().unwrap_err().code, "E_IR");
        }
    }
    #[test]
    fn corrupted_ir_is_rejected() {
        let m =
            crate::manifest::Manifest::from_json(r#"{"version":"0.1","types":[],"primitives":[]}"#)
                .unwrap();
        let mut p = crate::compile("region main() exits Done { [Entry]: exit Done; }", m).unwrap();
        p.regions.get_mut("main").unwrap().code[0].guard = Guard::Atom(999);
        assert_eq!(p.verify().unwrap_err().code, "E_IR");
    }
}
