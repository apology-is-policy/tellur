use crate::{
    Diagnostic, Result, Span,
    bdd::{Bdd, FALSE, Formula, TRUE},
    ir::{self, *},
    manifest::{self, Manifest, Mode},
    syntax::{self, Item, Mask, Statement},
};
use std::collections::{BTreeMap, BTreeSet};
mod collection_loans;
mod public;
mod stored;
mod sums;

struct Access {
    origin: LoanOrigin,
    mode: Mode,
    temporary: bool,
    exclusive: bool,
}
#[derive(Clone)]
struct Signature {
    stored_returns: Vec<StoredReturn>,
    parameters: Vec<manifest::Parameter>,
    outcomes: Vec<manifest::Outcome>,
    region: bool,
    completion_type: Option<String>,
    borrow_returns: BTreeMap<(String, usize), Vec<BorrowSource>>,
}
#[derive(Clone)]
struct Binding {
    slot: Slot,
    defined: Formula,
    owned: BTreeMap<Vec<usize>, Formula>,
    moved: BTreeMap<Vec<usize>, Formula>,
    mutable: bool,
    borrowed: bool,
}
#[derive(Clone)]
struct ResolvedPlace {
    /// Access through a carrier must validate that alias before reading its field.
    /// Copying the contained reference does not retain this access requirement.
    carriers: Vec<Id>,
    scope: usize,
    name: String,
    place: Place,
    ty: String,
    via: Option<Id>,
}
type CarrierContents = Vec<(Vec<usize>, Loan)>;

/// Keep runtime access separate from the input/owner origins that constrain a loan.
/// Stable local aliases may be erased; returned references use indirect access slots.
#[derive(Clone)]
struct Loan {
    /// Checked dependencies inside a stored reference's referent.
    contents: CarrierContents,
    sum_leaf: Option<SumLeaf>,
    id: Id,
    target: ResolvedPlace,
    origins: Vec<LoanOrigin>,
    parents: Vec<Id>,
    mutable: bool,
    active: Formula,
    defined: Formula,
    /// Storage that owns this reference value, independent of the pointee.
    holder: Option<Place>,
}
#[derive(Clone, PartialEq, Eq)]
struct LoanOrigin {
    /// A checked call (or incoming record) grants separate result permissions.
    /// Descendants retain their ancestry; an owner has no child permission and
    /// therefore still conflicts with every live opaque child.
    permissions: Vec<Permission>,
    guard: bool,
    place: Place,
    opaque: bool,
}
/// A permission identifies a result leaf and any projections taken through it.
/// Families are local to one checked invocation, never public lifetime names.
#[derive(Clone, PartialEq, Eq)]
struct Permission {
    family: Id,
    member: usize,
    fields: Vec<usize>,
}
impl Permission {
    fn new(family: Id, member: usize) -> Self {
        Self {
            family,
            member,
            fields: vec![],
        }
    }
    fn disjoint(&self, other: &Self) -> bool {
        self.family == other.family
            && (self.member != other.member
                || (!self.fields.starts_with(&other.fields)
                    && !other.fields.starts_with(&self.fields)))
    }
}
impl LoanOrigin {
    fn overlaps(&self, other: &Self) -> bool {
        self.place.overlaps(&other.place)
            && !self
                .permissions
                .iter()
                .any(|a| other.permissions.iter().any(|b| a.disjoint(b)))
    }
    fn project(&mut self, fields: &[usize]) {
        if !self.opaque && !self.guard {
            self.place.fields.extend_from_slice(fields);
        }
        if !self.guard
            && let Some(permission) = self.permissions.last_mut()
        {
            permission.fields.extend_from_slice(fields);
        }
    }
}
#[derive(Default)]
struct Scope {
    values: BTreeMap<String, Binding>,
    paths: BTreeMap<String, (Guard, Formula)>,
    sites: BTreeSet<String>,
    loans: BTreeMap<String, Loan>,
}
#[derive(Clone)]
struct LoopContext {
    name: String,
    id: Id,
    parameters: Vec<manifest::Parameter>,
    scope: usize,
}
struct Checker<'a> {
    inferred_bindings: Vec<InferredBinding>,
    contained_inputs: BTreeMap<Id, (usize, ContainedOrigin)>,
    origin_schemas: &'a crate::origins::Schemas,
    declared_stored: Vec<StoredReturn>,
    declared_returns: crate::lifetimes::Returns,
    manifest: &'a mut Manifest,
    signatures: &'a BTreeMap<String, Signature>,
    next: &'a mut Id,
    bdd: Bdd,
    dead: Formula,
    scopes: Vec<Scope>,
    result: Region,
    atom_formulas: BTreeMap<Id, Formula>,
    loop_context: Option<LoopContext>,
    infer_exits: bool,
    infer_payloads: BTreeSet<String>,
    pending_payloads: BTreeSet<String>,
    modern: bool,
    future_names: BTreeSet<String>,
    visibility: &'a crate::modules::Visibility,
    type_bindings: &'a BTreeMap<String, String>,
}
/// Leaves write their result before lexical payload slots are released. The
/// destination slot is resolved after every leaf has contributed to its type.
struct OutcomeBinding {
    loans: Vec<Loan>,
    ty: String,
    expected: Option<Vec<manifest::Outcome>>,
    outcomes: Vec<manifest::Outcome>,
    writes: Vec<usize>,
}
impl Checker<'_> {
    fn moves(&self, ty: &str) -> bool {
        self.manifest.linear(ty)
            || (self.manifest.contains_references(ty) && !self.manifest.is_cloneable(ty))
            || (self.modern && !self.manifest.is_copy(ty))
    }
    fn argument_mode(&self, parameter: &manifest::Parameter) -> Mode {
        if parameter.mode == Mode::Value && self.moves(&parameter.ty) {
            Mode::Consume
        } else {
            parameter.mode.clone()
        }
    }
    fn fresh(&mut self) -> Id {
        let id = *self.next;
        *self.next += 1;
        id
    }
    fn error(&self, code: &'static str, span: Span, message: impl Into<String>) -> Diagnostic {
        Diagnostic::new(code, span, message)
    }
    fn witness(&self, f: Formula) -> String {
        let assignment = self
            .bdd
            .witness(f)
            .expect("witness is requested only for nonempty domains");
        let names: Vec<_> = self
            .atom_formulas
            .iter()
            .filter(|(_, f)| self.bdd.evaluate(**f, &assignment))
            .map(|(id, _)| self.result.atoms[id].as_str())
            .collect();
        format!(
            "structural witness (may reach; not a proven feasible input): {}",
            if names.is_empty() {
                "Entry".into()
            } else {
                names.join(" & ")
            }
        )
    }
    fn fail_domain(&self, code: &'static str, span: Span, message: &str, f: Formula) -> Diagnostic {
        self.error(code, span, format!("{message}; {}", self.witness(f)))
    }
    fn path(&self, name: &str) -> Option<(Guard, Formula)> {
        let name = if name == "entry" { "Entry" } else { name };
        self.scopes
            .iter()
            .rev()
            .find_map(|s| s.paths.get(name).cloned())
    }
    fn mask(&mut self, mask: &Mask, span: Span) -> Result<(Guard, Formula)> {
        match mask {
            Mask::Name(n) => self.path(n).ok_or_else(|| {
                self.error(
                    "E_PATH",
                    span,
                    format!("unknown, future, or out-of-scope path `{n}`"),
                )
            }),
            Mask::And(a, b) | Mask::Or(a, b) => {
                let (ga, fa) = self.mask(a, span)?;
                let (gb, fb) = self.mask(b, span)?;
                Ok(if matches!(mask, Mask::And(..)) {
                    (ga.and(gb), self.bdd.and(fa, fb))
                } else {
                    (Guard::Or(Box::new(ga), Box::new(gb)), self.bdd.or(fa, fb))
                })
            }
        }
    }
    fn find_value(&self, name: &str) -> Option<(usize, Binding)> {
        self.scopes
            .iter()
            .enumerate()
            .rev()
            .find_map(|(i, s)| s.values.get(name).cloned().map(|v| (i, v)))
    }
    fn define(&mut self, name: &str, ty: &str, domain: Formula, span: Span) -> Result<Id> {
        if self.find_loan(name).is_some() {
            return Err(self.error(
                "E_REDEFINE",
                span,
                format!("`{name}` is already a reference binding"),
            ));
        }
        let scope = self.scopes.len() - 1;
        let binding = if let Some((i, mut binding)) = self.find_value(name) {
            if i != scope {
                return Err(self.error(
                    "E_SHADOW",
                    span,
                    format!("inner definition of outer value `{name}` is forbidden"),
                ));
            }
            if binding.slot.ty != ty {
                return Err(self.error(
                    "E_TYPE",
                    span,
                    format!(
                        "inconsistent types for `{name}`: {} and {ty}",
                        binding.slot.ty
                    ),
                ));
            }
            let overlap = self.bdd.and(binding.defined, domain);
            if overlap != FALSE {
                return Err(self.fail_domain(
                    "E_REDEFINE",
                    span,
                    &format!("overlapping definitions of `{name}`"),
                    overlap,
                ));
            }
            binding.defined = self.bdd.or(binding.defined, domain);
            for owned in binding.owned.values_mut() {
                *owned = self.bdd.or(*owned, domain);
            }
            binding
        } else {
            let id = self.fresh();
            let slot = Slot {
                id,
                name: name.into(),
                ty: ty.into(),
            };
            self.result.slots.insert(id, slot.clone());
            Binding {
                slot,
                defined: domain,
                owned: self
                    .manifest
                    .resource_paths(ty)
                    .into_iter()
                    .map(|path| (path, domain))
                    .collect(),
                moved: BTreeMap::new(),
                mutable: false,
                borrowed: false,
            }
        };
        let id = binding.slot.id;
        self.scopes[scope].values.insert(name.into(), binding);
        Ok(id)
    }
    fn resolve_place(
        &self,
        expression: &syntax::Expr,
        span: Span,
    ) -> Result<Option<ResolvedPlace>> {
        if let syntax::Expr::Deref(inner) = expression {
            let (_, _, loan, holder) = self.reference_source(inner, span)?.ok_or_else(|| {
                self.error(
                    "E_BORROW",
                    span,
                    "dereference requires a reference name or stored reference field",
                )
            })?;
            let mut target = loan.target.clone();
            if let Some(holder) = holder {
                target.carriers.extend(holder.carriers);
                target.carriers.extend(holder.via);
            }
            target.via = Some(loan.id);
            return Ok(Some(target));
        }
        if let syntax::Expr::Project(base, field) = expression
            && let Some(mut target) = self.resolve_place(base, span)?
        {
            if manifest::reference_inner(&target.ty).is_some() {
                let (_, _, loan) = self.stored_loan(&target).ok_or_else(|| {
                    self.error("E_LIFETIME", span, "missing stored reference provenance")
                })?;
                let mut carriers = target.carriers;
                carriers.extend(target.via);
                target = loan.target.clone();
                target.carriers.extend(carriers);
                target.via = Some(loan.id);
            }
            let (index, definition) =
                self.manifest
                    .record_field(&target.ty, field)
                    .ok_or_else(|| {
                        self.error(
                            "E_FIELD",
                            span,
                            format!("type `{}` has no record field `{field}`", target.ty),
                        )
                    })?;
            self.visibility
                .field(&self.result.name, &target.ty, field, span)?;
            target.place.fields.push(index);
            target.ty = definition.ty.clone();
            return Ok(Some(target));
        }
        fn components(e: &syntax::Expr) -> Option<Vec<&str>> {
            match e {
                syntax::Expr::Name(name) => Some(name.split('.').collect()),
                syntax::Expr::Project(base, field) => {
                    let mut parts = components(base)?;
                    parts.push(field);
                    Some(parts)
                }
                _ => None,
            }
        }
        let Some(parts) = components(expression) else {
            return Ok(None);
        };
        // Only field access dereferences implicitly. A bare reference name
        // remains a reference value in calls, returns and local bindings.
        let mut target = if parts.len() > 1
            && let Some((_, loan)) = self.find_loan(parts[0])
        {
            let mut target = loan.target.clone();
            target.via = Some(loan.id);
            target
        } else if let Some((scope, binding)) = self.find_value(parts[0]) {
            ResolvedPlace {
                carriers: vec![],
                scope,
                name: parts[0].into(),
                place: Place {
                    indirect: false,
                    root: binding.slot.id,
                    fields: vec![],
                },
                ty: binding.slot.ty,
                via: None,
            }
        } else {
            return Ok(None);
        };
        for name in &parts[1..] {
            if manifest::reference_inner(&target.ty).is_some() {
                let (_, _, loan) = self.stored_loan(&target).ok_or_else(|| {
                    self.error("E_LIFETIME", span, "missing stored reference provenance")
                })?;
                let mut carriers = target.carriers;
                carriers.extend(target.via);
                target = loan.target.clone();
                target.carriers.extend(carriers);
                target.via = Some(loan.id);
            }
            let (index, field) = self
                .manifest
                .record_field(&target.ty, name)
                .ok_or_else(|| {
                    self.error(
                        "E_FIELD",
                        span,
                        format!("type `{}` has no record field `{name}`", target.ty),
                    )
                })?;
            self.visibility
                .field(&self.result.name, &target.ty, name, span)?;
            target.place.fields.push(index);
            target.ty = field.ty.clone();
        }
        Ok(Some(target))
    }
    fn find_loan(&self, name: &str) -> Option<(usize, &Loan)> {
        self.scopes
            .iter()
            .enumerate()
            .rev()
            .find_map(|(i, s)| s.loans.get(name).map(|l| (i, l)))
    }
    fn loan(&self, id: Id) -> &Loan {
        self.scopes
            .iter()
            .flat_map(|s| s.loans.values())
            .find(|l| l.id == id)
            .unwrap()
    }
    /// Storage read used to obtain a contained reference. A copied-out shared
    /// reference has no holder and no longer needs permission to access it.
    fn holder_access(&self, place: &ResolvedPlace) -> Option<ResolvedPlace> {
        let loan = self.loan(place.via?);
        let holder = loan.holder.as_ref()?;
        let (scope, name, binding) = self.scopes.iter().enumerate().find_map(|(i, s)| {
            s.values
                .iter()
                .find(|(_, b)| b.slot.id == holder.root)
                .map(|(n, b)| (i, n, b))
        })?;
        let mut ty = if holder.indirect {
            manifest::reference_inner(&binding.slot.ty)?.1
        } else {
            &binding.slot.ty
        };
        for field in &holder.fields {
            ty = &self.manifest.field_at(ty, *field)?.ty;
        }
        let via = place
            .carriers
            .iter()
            .rev()
            .find(|id| {
                let target = &self.loan(**id).target.place;
                target.root == holder.root
                    && target.indirect == holder.indirect
                    && holder.fields.starts_with(&target.fields)
            })
            .copied();
        Some(ResolvedPlace {
            carriers: place
                .carriers
                .iter()
                .copied()
                .take_while(|id| Some(*id) != via)
                .collect(),
            scope,
            name: name.clone(),
            place: holder.clone(),
            ty: ty.into(),
            via,
        })
    }
    fn origins(&self, place: &ResolvedPlace) -> Vec<LoanOrigin> {
        if let Some(id) = place.via {
            let loan = self.loan(id);
            let suffix = &place.place.fields[loan.target.place.fields.len()..];
            let mut origins: Vec<_> = loan
                .origins
                .iter()
                .map(|p| {
                    let mut p = p.clone();
                    p.project(suffix);
                    p
                })
                .collect();
            if loan.mutable
                && let Some(holder) = self.holder_access(place).filter(|h| h.via.is_some())
            {
                for mut origin in self.origins(&holder) {
                    origin.guard = true;
                    origin.opaque = true;
                    if !origins.contains(&origin) {
                        origins.push(origin);
                    }
                }
            }
            origins
        } else {
            vec![LoanOrigin {
                permissions: vec![],
                place: place.place.clone(),
                opaque: false,
                guard: false,
            }]
        }
    }
    fn parents(&self, place: &ResolvedPlace) -> Vec<Id> {
        let mut parents = vec![];
        for id in place.via.iter().chain(&place.carriers) {
            let loan = self.loan(*id);
            for parent in loan
                .parents
                .iter()
                .copied()
                .chain(loan.mutable.then_some(*id))
            {
                if !parents.contains(&parent) {
                    parents.push(parent);
                }
            }
        }
        parents
    }
    fn check_loans(
        &mut self,
        place: &ResolvedPlace,
        domain: Formula,
        exclusive: bool,
        span: Span,
    ) -> Result<()> {
        if let Some(holder) = self.holder_access(place) {
            self.check_loans(&holder, domain, exclusive, span)?;
        }
        for id in &place.carriers {
            let carrier = self.loan(*id);
            if exclusive && !carrier.mutable {
                return Err(self.error(
                    "E_MUTABILITY",
                    span,
                    "cannot obtain exclusive access through a shared carrier reference",
                ));
            }
            let unavailable = self.bdd.minus(domain, carrier.active);
            if unavailable != FALSE {
                return Err(self.fail_domain(
                    "E_MOVED",
                    span,
                    "carrier reference was moved or released",
                    unavailable,
                ));
            }
        }
        let mut ignored = vec![];
        if let Some(id) = place.via {
            let loan = self.loan(id).clone();
            let missing = self.bdd.minus(domain, loan.defined);
            if missing != FALSE {
                return Err(self.fail_domain(
                    "E_UNAVAILABLE",
                    span,
                    "reference is not defined on every selected path",
                    missing,
                ));
            }
            let released = self.bdd.minus(domain, loan.active);
            if released != FALSE {
                return Err(self.fail_domain(
                    "E_MOVED",
                    span,
                    "reference was moved or released",
                    released,
                ));
            }
            if exclusive && !loan.mutable {
                return Err(self.error(
                    "E_MUTABILITY",
                    span,
                    "cannot write through a shared reference",
                ));
            }
            ignored = loan.parents;
            ignored.push(id);
        }
        // A reconstructed carrier authorizes the guards of the contained
        // permissions it received. Detached reborrows are not in this tree.
        fn contained_permissions(loan: &Loan, out: &mut Vec<Id>) {
            for (_, child) in &loan.contents {
                out.push(child.id);
                out.extend(&child.parents);
                contained_permissions(child, out);
            }
        }
        let mut contained = vec![];
        if let Some(id) = place.via {
            contained_permissions(self.loan(id), &mut contained);
        }
        let origins = self.origins(place);
        for loan in self.retained_loans() {
            if ignored.contains(&loan.id) {
                continue;
            }
            if (exclusive || loan.mutable)
                && origins.iter().filter(|a| !a.guard).any(|a| {
                    loan.origins.iter().any(|b| {
                        // A contained permission's storage guard protects its
                        // carrier from other aliases, not from access through
                        // that very holder. Detached children have no holder
                        // and still freeze this carrier normally.
                        let own_holder = loan.holder.as_ref().is_some_and(|h| {
                            h.root == place.place.root
                                && h.indirect == place.place.indirect
                                && h.fields.starts_with(&place.place.fields)
                        });
                        !(b.guard && (own_holder || contained.contains(&loan.id))) && a.overlaps(b)
                    })
                })
            {
                let overlap = self.bdd.and(domain, loan.active);
                if overlap != FALSE {
                    return Err(self.fail_domain("E_LOAN",span,"access conflicts with a live reference; release it with drop or end its scope",overlap));
                }
            }
        }
        Ok(())
    }
    fn reference_argument(
        &mut self,
        expression: &syntax::Expr,
        mutable: bool,
        domain: Formula,
        span: Span,
    ) -> Result<ResolvedPlace> {
        use syntax::Expr as S;
        let source = self.reference_source(expression, span)?;
        if let Some((_, _, _, Some(holder))) = &source {
            self.use_place(holder.clone(), domain, false, span)?;
        }
        let target = match expression {
            _ if source.is_some() => {
                let (_, _, loan, holder) = source.as_ref().unwrap();
                let mut target = loan.target.clone();
                if let Some(holder) = holder {
                    target.carriers.extend(holder.carriers.iter().copied());
                    target.carriers.extend(holder.via);
                }
                target.via = Some(loan.id);
                target
            }
            S::Borrow(inner) | S::BorrowMutable(inner) => {
                if matches!(expression, S::BorrowMutable(_)) != mutable {
                    return Err(self.error(
                        "E_BORROW",
                        span,
                        "reference mutability does not match the parameter or declaration",
                    ));
                }
                self.resolve_place(inner, span)?.ok_or_else(|| {
                    self.error(
                        "E_LIFETIME",
                        span,
                        "a reference requires a named owner, not a temporary",
                    )
                })?
            }
            _ => {
                return Err(self.error(
                    "E_BORROW",
                    span,
                    "borrowed parameters require borrow owner, borrow mutable owner, or a reference name",
                ));
            }
        };
        // Local borrows have concrete retained provenance; public parameters and
        // results validate their origin schemas separately at the boundary.
        if manifest::reference_inner(&target.ty).is_some() {
            return Err(self.error(
                "E_UNSUPPORTED",
                span,
                "references to references are not implemented",
            ));
        }
        self.manifest.check_local_storage(&target.ty)?;
        self.use_place(target.clone(), domain, false, span)?;
        self.check_loans(&target, domain, mutable, span)?;
        self.check_carrier_contents(&target, domain, mutable, span)?;
        if mutable && target.via.is_none() {
            let owner = &self.scopes[target.scope].values[&target.name];
            if !owner.mutable || owner.borrowed {
                return Err(self.error(
                    "E_MUTABILITY",
                    span,
                    "an exclusive reference requires a mutable owner",
                ));
            }
        }
        Ok(target)
    }
    fn add_loan(
        &mut self,
        name: &str,
        mut target: ResolvedPlace,
        origins: Vec<LoanOrigin>,
        parents: Vec<Id>,
        mutable: bool,
        domain: Formula,
    ) {
        let id = self.fresh();
        target.via = None;
        target.carriers.clear();
        self.scopes.last_mut().unwrap().loans.insert(
            name.into(),
            Loan {
                contents: vec![],
                sum_leaf: None,
                id,
                target,
                origins,
                parents,
                mutable,
                active: domain,
                defined: domain,
                holder: None,
            },
        );
    }
    fn bind_reference(
        &mut self,
        name: &str,
        annotation: Option<&str>,
        expression: &syntax::Expr,
        mutable_binding: bool,
        domain: Formula,
        span: Span,
    ) -> Result<bool> {
        use syntax::Expr as S;
        let copied = self.reference_source(expression, span)?;
        let mutable = match expression {
            S::Borrow(_) => false,
            S::BorrowMutable(_) => true,
            _ if copied.is_some() => copied.as_ref().unwrap().2.mutable,
            _ => return Ok(false),
        };
        if mutable_binding {
            return Err(self.error(
                "E_UNSUPPORTED",
                span,
                "reference rebinding is not implemented; assign through the reference",
            ));
        }
        if self.find_value(name).is_some() || self.find_loan(name).is_some() {
            return Err(self.error(
                "E_REDEFINE",
                span,
                "reference names must be unique in their lexical scope",
            ));
        }
        let target = self.reference_argument(expression, mutable, domain, span)?;
        let ty = format!("&{}{}", if mutable { "mutable " } else { "" }, target.ty);
        if let Some(expected) = annotation {
            self.expect_binding_type(&ty, expected, span)?;
        }
        let origins = self.origins(&target);
        let parents = if let Some(source) = copied {
            self.transfer_reference(&source, domain, span)?;
            source.2.parents
        } else {
            self.parents(&target)
        };
        self.add_loan(name, target, origins, parents, mutable, domain);
        Ok(true)
    }
    fn exit_value(
        &mut self,
        name: &str,
        index: usize,
        arg: &syntax::Expr,
        domain: Formula,
        span: Span,
        expected: Option<&str>,
    ) -> Result<(Expr, String)> {
        if let Some(value) = self.exit_stored(name, index, arg, domain, span, expected)? {
            return Ok(value);
        }
        let reference = match arg {
            syntax::Expr::Borrow(_) => Some(false),
            syntax::Expr::BorrowMutable(_) => Some(true),
            _ => self
                .reference_source(arg, span)?
                .map(|(_, _, l, _)| l.mutable),
        };
        let Some(mutable) = reference else {
            return self.expr_expected(arg, domain, true, span, expected);
        };
        let target = self.reference_argument(arg, mutable, domain, span)?;
        let ty = format!("&{}{}", if mutable { "mutable " } else { "" }, target.ty);
        crate::origins::result_type(&ty, self.origin_schemas, self.manifest, span)?;
        if self.manifest.contains_references(&target.ty) {
            self.exit_carrier_contents(name, index, &target, span)?;
        }
        if let Some(expected) = expected {
            self.expect_type(&ty, expected, span)?;
        }
        let origins = self.origins(&target);
        let mut sources = vec![];
        for origin in &origins {
            let source = self.return_source(origin, span)?;
            if !sources.contains(&source) {
                sources.push(source);
            }
        }
        if let Some(declared) = self.declared_returns.get(&(name.into(), index)) {
            if sources.iter().any(|source| {
                !declared
                    .iter()
                    .any(|d| crate::origins::same_root(d, source))
            }) {
                return Err(self.error("E_LIFETIME", span, format!("returned reference for `{name}` payload {} does not come from its declared input", index + 1)));
            }
            sources = declared.clone();
        }
        if sources.is_empty() {
            return Err(self.error(
                "E_LIFETIME",
                span,
                "cannot infer an unproduced borrowed payload; declare its `from input` origin",
            ));
        }
        let contract = self
            .result
            .borrow_returns
            .entry((name.into(), index))
            .or_default();
        for source in sources {
            if !contract.contains(&source) {
                contract.push(source);
            }
        }
        let value = Expr::Reference(target.place.clone(), ty.clone());
        let parents = self.parents(&target);
        let temp = format!("$return_{}", self.fresh());
        self.add_loan(&temp, target, origins, parents, mutable, domain);
        Ok((value, ty))
    }
    fn place_name(&self, place: &Place) -> String {
        let slot = &self.result.slots[&place.root];
        let mut name = slot.name.clone();
        let mut ty = slot.ty.clone();
        if place.indirect {
            ty = manifest::reference_inner(&ty)
                .expect("reference place")
                .1
                .into();
        }
        for index in &place.fields {
            let (suffix, field_ty) = self
                .manifest
                .borrow_field(&ty, *index)
                .expect("resolved borrow field");
            name.push_str(&suffix);
            ty = field_ty;
        }
        name
    }
    fn use_place(
        &mut self,
        resolved: ResolvedPlace,
        domain: Formula,
        moving: bool,
        span: Span,
    ) -> Result<(Expr, String)> {
        let moved_value = moving && self.moves(&resolved.ty);
        self.check_loans(
            &resolved,
            domain,
            moved_value && resolved.via.is_none(),
            span,
        )?;
        if moved_value && resolved.via.is_some() {
            return Err(self.error(
                "E_BORROW_MOVE",
                span,
                "cannot move an owned value through a reference; clone it explicitly",
            ));
        }
        if moved_value
            && self
                .loop_context
                .as_ref()
                .is_some_and(|l| resolved.scope < l.scope)
        {
            return Err(self.error(
                "E_LOOP_CAPTURE",
                span,
                "an owning value consumed in a loop must enter through a loop parameter",
            ));
        }
        let binding = &self.scopes[resolved.scope].values[&resolved.name];
        let missing = self.bdd.minus(domain, binding.defined);
        let name = self.place_name(&resolved.place);
        if missing != FALSE {
            return Err(self.fail_domain(
                "E_UNAVAILABLE",
                span,
                &format!("value `{name}` is unavailable"),
                missing,
            ));
        }
        for (path, moved) in &binding.moved {
            let overlap = self.bdd.and(domain, *moved);
            if overlap == FALSE {
                continue;
            }
            if resolved.place.fields.starts_with(path) {
                return Err(self.fail_domain(
                    "E_MOVED",
                    span,
                    &format!("place `{name}` was already consumed or moved"),
                    overlap,
                ));
            }
            if path.starts_with(&resolved.place.fields) {
                let field = self.place_name(&Place {
                    indirect: false,
                    root: resolved.place.root,
                    fields: path.clone(),
                });
                return Err(self.fail_domain(
                    "E_PARTIAL_MOVE",
                    span,
                    &format!("cannot use `{name}` as a whole; `{field}` was moved"),
                    overlap,
                ));
            }
        }
        if moved_value && binding.borrowed {
            return Err(self.error(
                "E_BORROW_MOVE",
                span,
                format!("cannot move `{name}` out of a shared borrow; clone it explicitly"),
            ));
        }
        if moved_value {
            let binding = self.scopes[resolved.scope]
                .values
                .get_mut(&resolved.name)
                .unwrap();
            for (path, owned) in &mut binding.owned {
                if path.starts_with(&resolved.place.fields) {
                    *owned = self.bdd.minus(*owned, domain);
                }
            }
            let moved = binding
                .moved
                .entry(resolved.place.fields.clone())
                .or_insert(FALSE);
            *moved = self.bdd.or(*moved, domain);
        }
        Ok((
            if moved_value {
                Expr::Move(resolved.place)
            } else {
                Expr::Place(resolved.place)
            },
            resolved.ty,
        ))
    }
    fn expr(
        &mut self,
        expr: &syntax::Expr,
        domain: Formula,
        moving: bool,
        span: Span,
    ) -> Result<(Expr, String)> {
        self.expr_expected(expr, domain, moving, span, None)
    }
    fn assignment(
        &mut self,
        target: &syntax::Expr,
        value: &syntax::Expr,
        guard: Guard,
        domain: Formula,
        span: Span,
    ) -> Result<()> {
        if let syntax::Expr::Name(name) = target
            && self.find_loan(name).is_some()
        {
            return Err(self.error(
                "E_UNSUPPORTED",
                span,
                "reference rebinding is not implemented; assign through *reference instead",
            ));
        }
        let target = self
            .resolve_place(target, span)?
            .ok_or_else(|| self.error("E_NAME", span, "assignment requires a declared place"))?;
        if self.manifest.contains_references(&target.ty) {
            return Err(self.error("E_UNSUPPORTED", span, "replacing reference-bearing storage requires contained-loan rebinding; assign through its reference or update an owned field"));
        }
        self.check_loans(&target, domain, true, span)?;
        let binding = &self.scopes[target.scope].values[&target.name];
        if target.via.is_none() && (!binding.mutable || binding.borrowed) {
            return Err(self.error(
                "E_MUTABILITY",
                span,
                format!("`{}` is not a mutable owned binding", target.name),
            ));
        }
        let missing = self.bdd.minus(domain, binding.defined);
        if missing != FALSE {
            return Err(self.fail_domain(
                "E_UNAVAILABLE",
                span,
                "assignment target is not declared on every selected path",
                missing,
            ));
        }
        let (value, ty) = self.expr_expected(value, domain, true, span, Some(&target.ty))?;
        self.expect_type(&ty, &target.ty, span)?;
        if target.place.indirect && self.manifest.linear(&target.ty) {
            return Err(self.error(
                "E_RESOURCE",
                span,
                "assignment through a reference cannot discard a must-complete resource",
            ));
        }
        let binding = self.scopes[target.scope].values[&target.name].clone();
        for (path, moved) in &binding.moved {
            if target.place.fields.starts_with(path) && path != &target.place.fields {
                let overlap = self.bdd.and(domain, *moved);
                if overlap != FALSE {
                    return Err(self.fail_domain(
                        "E_PARTIAL_MOVE",
                        span,
                        "cannot assign a field of a moved parent; reinitialize the parent",
                        overlap,
                    ));
                }
            }
        }
        for (path, owned) in &binding.owned {
            if path.starts_with(&target.place.fields) {
                let overlap = self.bdd.and(domain, *owned);
                if overlap != FALSE {
                    return Err(self.fail_domain("E_RESOURCE", span, "assignment would discard an outstanding resource; close or transfer it first", overlap));
                }
            }
        }
        let binding = self.scopes[target.scope]
            .values
            .get_mut(&target.name)
            .unwrap();
        for (path, moved) in &mut binding.moved {
            if path.starts_with(&target.place.fields) {
                *moved = self.bdd.minus(*moved, domain);
            }
        }
        for (path, owned) in &mut binding.owned {
            if path.starts_with(&target.place.fields) {
                *owned = self.bdd.or(*owned, domain);
            }
        }
        self.emit(guard, Action::Assign(target.place, value), span);
        Ok(())
    }
    fn collection_type(&self, arg: &syntax::Expr, span: Span) -> Result<String> {
        let e = match arg {
            syntax::Expr::Borrow(e) | syntax::Expr::BorrowMutable(e) => e.as_ref(),
            _ => arg,
        };
        if let Some((_, _, loan, _)) = self.reference_source(e, span)? {
            return Ok(loan.target.ty);
        }
        self.resolve_place(e,span)?.map(|p|p.ty).ok_or_else(||self.error("E_COLLECTION",span,"collection operations require a named collection or reference as their first argument"))
    }
    fn signature(
        &self,
        name: &str,
        args: &[syntax::Expr],
        span: Span,
    ) -> Result<Option<Signature>> {
        if crate::collections::is_operation(name) {
            let first = args
                .first()
                .ok_or_else(|| self.error("E_ARITY", span, "missing collection argument"))?;
            let ty = self.collection_type(first, span)?;
            let c = crate::collections::contract(name, &ty).ok_or_else(|| {
                self.error(
                    "E_COLLECTION",
                    span,
                    format!("{name} does not apply to {ty}"),
                )
            })?;
            return Ok(Some(Signature {
                stored_returns: vec![],
                parameters: c.parameters,
                outcomes: c.outcomes,
                borrow_returns: c.borrow_returns,
                region: false,
                completion_type: None,
            }));
        }
        Ok(self.signatures.get(name).cloned())
    }
    fn expr_expected(
        &mut self,
        expr: &syntax::Expr,
        domain: Formula,
        moving: bool,
        span: Span,
        expected: Option<&str>,
    ) -> Result<(Expr, String)> {
        use syntax::Expr as S;
        if let Some(place) = self.resolve_place(expr, span)? {
            if self.manifest.contains_references(&place.ty) {
                return Err(self.error("E_UNSUPPORTED", span, "stored references currently support local record construction, binding, field access and drop; this value boundary needs contained-loan propagation"));
            }
            return self.use_place(place, domain, moving, span);
        }
        if self
            .stored_type(expr, span)?
            .is_some_and(|ty| self.manifest.contains_references(&ty))
        {
            return Err(self.error(
                "E_UNSUPPORTED",
                span,
                "bind the reference-bearing record locally before accessing its fields",
            ));
        }
        let literal = match expr {
            S::String(s) => Some(Value::String(s.clone())),
            S::Bool(b) => Some(Value::Bool(*b)),
            S::U32(n) => Some(Value::U32(*n)),
            S::Unit => Some(Value::Unit),
            S::Bytes(b) => Some(Value::Bytes(b.clone())),
            _ => None,
        };
        if let Some(value) = literal {
            let ty = value.ty();
            return Ok((Expr::Value(value), ty));
        }
        match expr {
            S::Name(name) if self.find_loan(name).is_some() => Err(self.error("E_LIFETIME", span, "local references cannot escape or enter aggregates; dereference with deref name, copy into a local reference, or pass to a borrowed parameter")),
            S::Name(name) => {
                let (base,suffix)=name.split_once('.').map_or((name.as_str(),None),|(a,b)|(a,Some(b)));
                let name=self.type_bindings.get(base).map(|ty|suffix.map_or_else(||ty.clone(),|tag|format!("{ty}.{tag}"))).unwrap_or_else(||name.clone());
                self.construct(&name, &[], domain, span, expected)
            },
            S::Borrow(_) | S::BorrowMutable(_) => Err(self.error("E_LIFETIME", span, "a reference must be bound locally, passed to a borrowed parameter, or returned through a reference exit")),
            S::Call(name, args) => {
                if crate::collections::is_length(name) {
                    if args.len()!=1 { return Err(self.error("E_ARITY",span,"length expects one borrowed collection")); }
                    let ty=self.collection_type(&args[0],span)?;
                    let c=crate::collection_support::Collection::parse(&ty).filter(|c|name==&format!("{}_length",c.name())).ok_or_else(||self.error("E_COLLECTION",span,"length operation does not match collection type"))?;
                    let _=c;
                    let parameters=vec![manifest::Parameter{name:"values".into(),ty:format!("&{ty}"),mode:Mode::Borrow}];
                    let mut values=self.arguments(args,&parameters,domain,span)?;
                    return Ok((Expr::Length(Box::new(values.remove(0))),"U32".into()));
                }
                if name == "clone" {
                    let dereferenced;
                    let value = match args.as_slice() {
                        [S::Borrow(value)] => value.as_ref(),
                        [value] if self.reference_source(value, span)?.is_some() => {
                            dereferenced = S::Deref(Box::new(args[0].clone()));
                            &dereferenced
                        }
                        _ => return Err(self.error("E_BORROW", span, "clone expects a shared borrow or reference: clone(borrow value) or clone(reference)")),
                    };
                    let (value, ty) = self.expr(value, domain, false, span)?;
                    if self.manifest.contains_outcomes(&ty) {return Err(self.error("E_OUTCOME",span,"completed outcomes cannot be cloned; decide or transfer them"));}
                    if self.manifest.linear(&ty) { return Err(self.error("E_CLONE", span, "a must-complete resource cannot be cloned")); }
                    if !self.manifest.is_cloneable(&ty) { return Err(self.error("E_CLONE", span, "cloning this type requires a clone or copy capability")); }
                    return Ok((value, ty));
                }
                if let Some(p) = self
                    .manifest
                    .primitives
                    .iter()
                    .find(|p| p.name == *name && p.pure)
                {
                    let args = self.arguments(args, &p.parameters.clone(), domain, span)?;
                    Ok((Expr::Pure(name.clone(), args), "String".into()))
                } else {
                    self.construct(name, args, domain, span, expected)
                }
            }
            S::List(args) => {
                let (ty,c)=if let Some(ty)=expected {
                    let c=crate::collection_support::Collection::parse(ty).filter(|c|!matches!(c,crate::collection_support::Collection::Map(..))).ok_or_else(||self.error("E_TYPE",span,"list literal requires an array or vector type"))?;
                    (ty.to_owned(),c)
                } else { return Err(self.error("E_INFER",span,"list literal needs an array or vector type annotation")); };
                let fields=c.fields(args.len()).ok_or_else(||self.error("E_ARITY",span,"array literal length does not match its type"))?;
                let parameters=fields.into_iter().map(|ty|manifest::Parameter{name:"element".into(),ty,mode:Mode::Value}).collect::<Vec<_>>();
                let values=self.arguments(args,&parameters,domain,span)?;
                Ok((Expr::Construct(ty.clone(),c.name().into(),values),ty))
            }
            S::Construct(name, args) if name=="map" => {
                let ty=expected.ok_or_else(||self.error("E_INFER",span,"map {} needs a map<key, value> type annotation"))?;
                if !matches!(crate::collection_support::Collection::parse(ty),Some(crate::collection_support::Collection::Map(..))) || !args.is_empty() {
                    return Err(self.error("E_COLLECTION",span,"use map {} for an empty map, then map_insert to handle replacement explicitly"));
                }
                Ok((Expr::Construct(ty.into(),"map".into(),vec![]),ty.into()))
            }
            S::Construct(name, args) => self.construct(name, args, domain, span, expected),
            S::NamedRecord(name, fields) => self.named_record(name, fields, domain, span),
            S::Project(base, name) => {
                let (base, ty) = self.expr(base, domain, false, span)?;
                if self.manifest.linear(&ty) {
                    return Err(self.error("E_TEMPORARY", span, "bind a resource-containing temporary to a name before projecting a field; its remaining resources need an explicit disposition"));
                }
                let (index, field) = self.manifest.record_field(&ty, name).ok_or_else(|| {
                    self.error(
                        "E_FIELD",
                        span,
                        format!("type `{ty}` has no record field `{name}`"),
                    )
                })?;
                self.visibility.field(&self.result.name, &ty, name, span)?;
                Ok((Expr::Project(Box::new(base), index), field.ty.clone()))
            }
            S::Unary(e) => {
                let (e, ty) = self.expr(e, domain, false, span)?;
                self.expect_type(&ty, "Bool", span)?;
                Ok((Expr::Not(Box::new(e)), "Bool".into()))
            }
            S::Binary(op, a, b) => {
                let (a, ta) = self.expr(a, domain, false, span)?;
                let (b, tb) = self.expr(b, domain, false, span)?;
                self.expect_type(&tb, &ta, span)?;
                if ["&&", "||"].contains(&op.as_str()) {
                    self.expect_type(&ta, "Bool", span)?;
                } else if ["<", "<=", ">", ">="].contains(&op.as_str()) {
                    self.expect_type(&ta, "U32", span)?;
                } else if !["Bool", "U32", "String", "Unit"].contains(&ta.as_str()) {
                    return Err(self.error(
                        "E_TYPE",
                        span,
                        format!("equality is not defined for {ta}"),
                    ));
                }
                Ok((
                    Expr::Binary(op.clone(), Box::new(a), Box::new(b)),
                    "Bool".into(),
                ))
            }
            _ => unreachable!(),
        }
    }
    fn named_record(
        &mut self,
        name: &str,
        initializers: &[(String, syntax::Expr)],
        domain: Formula,
        span: Span,
    ) -> Result<(Expr, String)> {
        let fields = match self.manifest.type_def(name) {
            Some(manifest::TypeDef::Record { fields, .. }) => fields.clone(),
            _ => {
                return Err(self.error(
                    "E_CONSTRUCT",
                    span,
                    format!("named fields require a struct type, found `{name}`"),
                ));
            }
        };
        self.visibility.construct(&self.result.name, name, span)?;
        let mut seen = BTreeSet::new();
        let mut indices = vec![];
        let mut parameters = vec![];
        let mut args = vec![];
        for (field, value) in initializers {
            let (index, declaration) = fields
                .iter()
                .enumerate()
                .find(|(_, f)| f.name == *field)
                .ok_or_else(|| {
                    self.error(
                        "E_FIELD",
                        span,
                        format!("struct `{name}` has no field `{field}`"),
                    )
                })?;
            if !seen.insert(index) {
                return Err(self.error(
                    "E_FIELD",
                    span,
                    format!("field `{field}` is initialized more than once"),
                ));
            }
            indices.push(index);
            parameters.push(manifest::Parameter {
                name: field.clone(),
                ty: declaration.ty.clone(),
                mode: Mode::Value,
            });
            args.push(value.clone());
        }
        if seen.len() != fields.len() {
            let missing = fields
                .iter()
                .enumerate()
                .filter(|(i, _)| !seen.contains(i))
                .map(|(_, f)| f.name.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            return Err(self.error(
                "E_FIELD",
                span,
                format!("missing fields for `{name}`: {missing}"),
            ));
        }
        // Type/ownership checking follows source order, as does execution. Only the
        // final layout uses declaration order; sorting expressions would change moves.
        let values = self.arguments(&args, &parameters, domain, span)?;
        Ok((
            Expr::Record(name.into(), indices.into_iter().zip(values).collect()),
            name.into(),
        ))
    }
    fn construct(
        &mut self,
        name: &str,
        args: &[syntax::Expr],
        domain: Formula,
        span: Span,
        expected: Option<&str>,
    ) -> Result<(Expr, String)> {
        if let Some(ty) = expected
            && let Some(manifest::TypeDef::Outcome { constructors, .. }) =
                self.manifest.type_def(ty)
            && let Some(outcome) = constructors
                .iter()
                .find(|o| name == o.name || name == format!("{ty}.{}", o.name))
                .cloned()
        {
            let parameters = outcome
                .fields
                .iter()
                .map(|f| manifest::Parameter {
                    name: f.name.clone(),
                    ty: f.ty.clone(),
                    mode: Mode::Value,
                })
                .collect::<Vec<_>>();
            let values = self.arguments(args, &parameters, domain, span)?;
            return Ok((Expr::Construct(ty.into(), outcome.name, values), ty.into()));
        }
        if self.signatures.contains_key(name) || crate::collections::is_operation(name) {
            return Err(self.error(
                "E_EFFECT",
                span,
                format!("operation `{name}` must be invoked through delta"),
            ));
        }
        if let Some(constructor) = self.manifest.option_constructor_visible(name, |ty| {
            self.visibility
                .constructor_visible(&self.result.name, ty, name)
        }) {
            let arity = usize::from(constructor == "Some");
            if args.len() != arity {
                return Err(self.error(
                    "E_ARITY",
                    span,
                    format!("{name} expects {arity} arguments, found {}", args.len()),
                ));
            }
            if let Some(inner) = expected.and_then(manifest::option_inner) {
                let ty = format!("Option<{inner}>");
                let fields = if constructor == "Some" {
                    self.arguments(
                        args,
                        &[manifest::Parameter {
                            name: "value".into(),
                            ty: inner.into(),
                            mode: if self.manifest.linear(inner) {
                                Mode::Consume
                            } else {
                                Mode::Value
                            },
                        }],
                        domain,
                        span,
                    )?
                } else {
                    vec![]
                };
                return Ok((Expr::Construct(ty.clone(), constructor.into(), fields), ty));
            }
            if constructor == "None" {
                return Err(self.error("E_INFER", span, "None needs an expected Option<T> type; use it in a typed constructor, call, exit, or loop argument"));
            }
            let (value, inner) = self.expr(&args[0], domain, true, span)?;
            let ty = format!("Option<{inner}>");
            return Ok((
                Expr::Construct(ty.clone(), constructor.into(), vec![value]),
                ty,
            ));
        }
        let (ty, constructor, fields) = self
            .manifest
            .constructor_visible(name, |ty| {
                self.visibility
                    .constructor_visible(&self.result.name, ty, name)
            })
            .map_err(|_| {
                self.error(
                    "E_NAME",
                    span,
                    format!("unknown value or ambiguous constructor `{name}`"),
                )
            })?;
        self.visibility.construct(&self.result.name, &ty, span)?;
        let args = self.arguments(
            args,
            &fields
                .iter()
                .map(|f| manifest::Parameter {
                    name: f.name.clone(),
                    ty: f.ty.clone(),
                    mode: if self.manifest.linear(&f.ty) {
                        Mode::Consume
                    } else {
                        Mode::Value
                    },
                })
                .collect::<Vec<_>>(),
            domain,
            span,
        )?;
        Ok((Expr::Construct(ty.clone(), constructor, args), ty))
    }
    /// `auto` inside a reference binding infers only its referent. The reference
    /// kind remains an exact requirement; this does not insert a reborrow.
    fn expect_binding_type(&self, actual: &str, expected: &str, span: Span) -> Result<()> {
        if let Some((expected_mutable, "auto")) = manifest::reference_inner(expected)
            && manifest::reference_inner(actual)
                .is_some_and(|(mutable, _)| mutable == expected_mutable)
        {
            return Ok(());
        }
        self.expect_type(actual, expected, span)
    }
    fn expect_type(&self, actual: &str, expected: &str, span: Span) -> Result<()> {
        if actual == expected {
            Ok(())
        } else {
            Err(self.error(
                "E_TYPE",
                span,
                format!(
                    "expected {}, found {}",
                    if manifest::reference_inner(expected).is_some() {
                        manifest::display_type(expected)
                    } else {
                        expected.into()
                    },
                    if manifest::reference_inner(actual).is_some() {
                        manifest::display_type(actual)
                    } else {
                        actual.into()
                    }
                ),
            ))
        }
    }
    fn arguments(
        &mut self,
        args: &[syntax::Expr],
        parameters: &[manifest::Parameter],
        domain: Formula,
        span: Span,
    ) -> Result<Vec<Expr>> {
        if args.len() != parameters.len() {
            return Err(self.error(
                "E_ARITY",
                span,
                format!(
                    "expected {} arguments, found {}",
                    parameters.len(),
                    args.len()
                ),
            ));
        }
        self.argument_aliases(args, parameters, domain, span)?;
        let first_argument_loan = *self.next;
        let result = args.iter()
            .zip(parameters)
            .map(|(arg, p)| {
                if let Some((mutable,inner))=manifest::reference_inner(&p.ty) {
                    let target=self.reference_argument(arg,mutable,domain,span)?;
                    self.expect_type(&target.ty,inner,span)?;
                    return Ok(Expr::Reference(target.place,p.ty.clone()));
                }
                let mode = self.argument_mode(p);
                // A named local reference already expresses borrowing at the call.
                let dereferenced;
                let arg = if self.reference_source(arg, span)?.is_some()
                    && mode == Mode::Borrow {
                    dereferenced = syntax::Expr::Borrow(Box::new(syntax::Expr::Deref(Box::new(arg.clone()))));
                    &dereferenced
                } else { arg };
                let arg = match (arg, &mode) {
                    (syntax::Expr::Borrow(inner), Mode::Borrow) => inner.as_ref(),
                    (syntax::Expr::Borrow(_), _) => return Err(self.error("E_BORROW", span, "borrow supplied to an owning parameter")),
                    (syntax::Expr::BorrowMutable(_), _) => return Err(self.error("E_UNSUPPORTED", span, "exclusive borrow supplied to a parameter without an exclusive reference type")),
                    (_, Mode::Borrow) if self.modern => return Err(self.error("E_BORROW", span, format!("parameter `{}` requires an explicit shared borrow (`borrow value`)", p.name))),
                    _ => arg,
                };
                if mode == Mode::Borrow && self.manifest.linear(&p.ty) && self.resolve_place(arg, span)?.is_none() {
                    return Err(self.error("E_TEMPORARY", span, "borrow requires a named place; bind the resource-containing value first so its ownership is tracked"));
                }
                if self.manifest.contains_references(&p.ty) {
                    return self.stored_argument(arg, &p.ty, domain, span);
                }
                let (expr, ty) = self.expr_expected(arg, domain, mode == Mode::Consume, span, Some(&p.ty))?;
                self.expect_type(&ty, &p.ty, span)?;
                Ok(expr)
            })
            .collect();
        // Temporary holders protect earlier arguments during evaluation. The
        // callee owns them for the invocation; returned permissions get their
        // own holders before checking the caller's continuation.
        for scope in &mut self.scopes {
            for (name, loan) in &mut scope.loans {
                if loan.id >= first_argument_loan && name.starts_with("$stored_") {
                    loan.active = FALSE;
                }
            }
        }
        result
    }
    fn argument_aliases(
        &mut self,
        args: &[syntax::Expr],
        parameters: &[manifest::Parameter],
        domain: Formula,
        span: Span,
    ) -> Result<()> {
        // A loan lasts through the invocation, including construction of other arguments.
        let mut uses = vec![];
        for (argument, (arg, param)) in args.iter().zip(parameters).enumerate() {
            let mut accesses = vec![];
            self.accesses(arg, &self.argument_mode(param), span, &mut accesses)?;
            let place = match arg {
                syntax::Expr::Borrow(inner) | syntax::Expr::BorrowMutable(inner) => inner.as_ref(),
                _ => arg,
            };
            let carrier = self.reference_source(place, span)?.is_some()
                || self.resolve_place(place, span)?.is_some();
            uses.extend(
                accesses
                    .into_iter()
                    .map(|access| (argument, carrier, access)),
            );
        }
        for (index, (argument, carrier, access)) in uses.iter().enumerate() {
            for (other_argument, _, other) in &uses[..index] {
                // A whole carrier authorizes its own contained permissions. Its
                // storage guards still conflict with separate arguments, and
                // constructor expressions must establish their own separation.
                if *carrier
                    && argument == other_argument
                    && (access.origin.guard || other.origin.guard)
                {
                    continue;
                }
                // Ordinary use-after-move is diagnosed during evaluation. An exclusive
                // call borrow also excludes temporary reads in later arguments.
                if other.temporary
                    || (access.temporary && other.mode == Mode::Consume && !other.exclusive)
                {
                    continue;
                }
                if access.origin.overlaps(&other.origin)
                    && (access.mode == Mode::Consume || other.mode == Mode::Consume)
                {
                    return Err(self.fail_domain("E_ALIAS",span,&format!("places `{}` and `{}` overlap while one is moved or exclusively borrowed",self.place_name(&access.origin.place),self.place_name(&other.origin.place)),domain));
                }
            }
        }
        Ok(())
    }
    fn contained_accesses(&self, carrier: &ResolvedPlace, exclusive: bool, uses: &mut Vec<Access>) {
        if !self.manifest.contains_references(&carrier.ty) {
            return;
        }
        for loan in self
            .scopes
            .iter()
            .flat_map(|s| s.loans.values())
            .filter(|l| {
                l.holder.as_ref().is_some_and(|h| {
                    h.root == carrier.place.root
                        && h.indirect == carrier.place.indirect
                        && h.fields.starts_with(&carrier.place.fields)
                })
            })
        {
            uses.extend(loan.origins.iter().map(|origin| Access {
                origin: origin.clone(),
                mode: if exclusive && loan.mutable && !origin.guard {
                    Mode::Consume
                } else {
                    Mode::Borrow
                },
                temporary: false,
                exclusive: exclusive && loan.mutable,
            }));
        }
    }
    fn accesses(
        &self,
        expr: &syntax::Expr,
        mode: &Mode,
        span: Span,
        uses: &mut Vec<Access>,
    ) -> Result<()> {
        if let Some((_, _, loan, _)) = self.reference_source(expr, span)? {
            self.contained_accesses(&loan.target, *mode != Mode::Borrow, uses);
            uses.extend(loan.origins.iter().map(|p| Access {
                origin: p.clone(),
                mode: if p.guard { Mode::Borrow } else { mode.clone() },
                temporary: *mode == Mode::Value,
                exclusive: *mode == Mode::Consume,
            }));
            return Ok(());
        }
        if let Some(resolved) = self.resolve_place(expr, span)? {
            self.contained_accesses(&resolved, *mode != Mode::Borrow, uses);
            let mode = if *mode == Mode::Consume && !self.moves(&resolved.ty) {
                Mode::Value
            } else {
                mode.clone()
            };
            uses.extend(self.origins(&resolved).into_iter().map(|p| Access {
                origin: p.clone(),
                mode: if p.guard { Mode::Borrow } else { mode.clone() },
                temporary: mode == Mode::Value,
                exclusive: false,
            }));
            return Ok(());
        }
        match expr {
            syntax::Expr::Borrow(inner) => self.accesses(inner, &Mode::Borrow, span, uses)?,
            syntax::Expr::BorrowMutable(inner) => {
                if let Some(target) = self.resolve_place(inner, span)? {
                    self.contained_accesses(&target, true, uses);
                    uses.extend(self.origins(&target).into_iter().map(|p| Access {
                        origin: p.clone(),
                        mode: if p.guard { Mode::Borrow } else { Mode::Consume },
                        temporary: false,
                        exclusive: true,
                    }));
                } else {
                    self.accesses(inner, &Mode::Consume, span, uses)?;
                }
            }
            syntax::Expr::NamedRecord(name, fields) => {
                for (field, value) in fields {
                    let mode = self
                        .manifest
                        .record_field(name, field)
                        .map(|(_, f)| {
                            if self.moves(&f.ty) {
                                Mode::Consume
                            } else {
                                Mode::Value
                            }
                        })
                        .unwrap_or(Mode::Consume);
                    self.accesses(value, &mode, span, uses)?;
                }
            }
            syntax::Expr::List(args) => {
                for arg in args {
                    self.accesses(arg, &Mode::Consume, span, uses)?;
                }
            }
            syntax::Expr::Call(name, args)
                if name == "clone" || crate::collections::is_length(name) =>
            {
                let mut reads = vec![];
                for arg in args {
                    self.accesses(arg, &Mode::Borrow, span, &mut reads)?;
                }
                uses.extend(reads.into_iter().map(|mut access| {
                    access.temporary = true;
                    access.exclusive = false;
                    access
                }));
            }
            syntax::Expr::Call(name, args) | syntax::Expr::Construct(name, args) => {
                if let Some(sig) = self.signatures.get(name) {
                    for (arg, p) in args.iter().zip(&sig.parameters) {
                        self.accesses(arg, &self.argument_mode(p), span, uses)?;
                    }
                    return Ok(());
                }
                if self.manifest.option_constructor_visible(name, |ty| {
                    self.visibility
                        .constructor_visible(&self.result.name, ty, name)
                }) == Some("Some")
                {
                    for arg in args {
                        self.accesses(arg, &Mode::Consume, span, uses)?;
                    }
                    return Ok(());
                }
                if let Ok((_, _, fields)) = self.manifest.constructor_visible(name, |ty| {
                    self.visibility
                        .constructor_visible(&self.result.name, ty, name)
                }) {
                    for (arg, field) in args.iter().zip(fields) {
                        let mode = if self.moves(&field.ty) {
                            Mode::Consume
                        } else {
                            Mode::Value
                        };
                        self.accesses(arg, &mode, span, uses)?;
                    }
                }
            }
            syntax::Expr::Project(base, _) | syntax::Expr::Unary(base) => {
                self.accesses(base, &Mode::Value, span, uses)?
            }
            syntax::Expr::Binary(_, a, b) => {
                self.accesses(a, &Mode::Value, span, uses)?;
                self.accesses(b, &Mode::Value, span, uses)?;
            }
            _ => {}
        }
        Ok(())
    }
    fn resources(&mut self, domain: Formula, span: Span, only_scope: Option<usize>) -> Result<()> {
        for (i, scope) in self.scopes.iter().enumerate() {
            if only_scope.is_some_and(|s| s != i) {
                continue;
            }
            for binding in scope.values.values() {
                for (path, owned) in &binding.owned {
                    let leak = self.bdd.and(domain, *owned);
                    if leak != FALSE {
                        if self.manifest.contains_outcomes(&binding.slot.ty) {
                            return Err(self.fail_domain(
                                "E_OUTCOME",
                                span,
                                &format!(
                                    "`{}` contains an unhandled outcome; decide or transfer it",
                                    binding.slot.name
                                ),
                                leak,
                            ));
                        }
                        let name = self.place_name(&Place {
                            indirect: false,
                            root: binding.slot.id,
                            fields: path.clone(),
                        });
                        return Err(self.fail_domain(
                            "E_RESOURCE",
                            span,
                            &format!("resource `{name}` remains owned; consume or transfer it"),
                            leak,
                        ));
                    }
                }
            }
        }
        Ok(())
    }
    fn emit(&mut self, guard: Guard, action: Action, span: Span) {
        let id = self.fresh();
        self.result.code.push(Instruction {
            id,
            guard,
            action,
            provenance: Provenance {
                definition: id,
                calls: vec![],
                region: self.result.name.clone(),
                site: None,
                call_sites: vec![],
                span,
            },
        });
    }
    fn statement(
        &mut self,
        guard: Guard,
        domain: Formula,
        statement: &Statement,
        span: Span,
    ) -> Result<()> {
        self.statement_inner(guard, domain, statement, span)?;
        if let Statement::Bind(name, ..)
        | Statement::Capture(name, ..)
        | Statement::BindOutcome(name, ..) = statement
        {
            let ty = if let Some((_, loan)) = self.find_loan(name) {
                Some(format!(
                    "&{}{}",
                    if loan.mutable { "mutable " } else { "" },
                    loan.target.ty
                ))
            } else {
                self.find_value(name).map(|(_, b)| b.slot.ty)
            };
            if let Some(ty) = ty {
                self.inferred_bindings.push(InferredBinding {
                    control: self.result.name.clone(),
                    display_type: self.manifest.display_inferred_type(&ty),
                    span,
                    name: name.clone(),
                });
            }
        }
        Ok(())
    }
    fn statement_inner(
        &mut self,
        guard: Guard,
        domain: Formula,
        statement: &Statement,
        span: Span,
    ) -> Result<()> {
        let overlap = self.bdd.and(domain, self.dead);
        if overlap != FALSE {
            return Err(self.fail_domain(
                "E_TERMINATED",
                span,
                "selector overlaps an already terminated history",
                overlap,
            ));
        }
        if domain == FALSE {
            return Err(self.error("E_EMPTY", span, "structurally empty executable selector"));
        }
        match statement {
            Statement::Forward(operand) => {
                let syntax::Expr::Call(callee, args) = operand else {
                    return Err(self.error(
                        "E_FORWARD",
                        span,
                        "forward expects an operation call; use exit saved for a completed outcome",
                    ));
                };
                let sig = self.signature(callee, args, span)?.ok_or_else(|| {
                    self.error("E_FORWARD", span, "forward requires a declared operation")
                })?;
                if !self.infer_exits && self.infer_payloads.is_empty() {
                    crate::algebra::outcome_embedding(&sig.outcomes, &self.result.exits)
                        .map_err(|e| self.error(e.code(), span, e.to_string()))?;
                }
                let arms = self.identity_arms(&sig.outcomes, span);
                self.exit_outcome(
                    &syntax::OutcomeExpr::Delta(span, operand.clone(), arms),
                    guard,
                    domain,
                )?;
            }
            Statement::Invoke(operand) => {
                let syntax::Expr::Call(callee, args) = operand else {
                    return Err(self.error(
                        "E_DISPOSITION",
                        span,
                        "a call statement requires an operation call",
                    ));
                };
                let sig = self.signature(callee, args, span)?.ok_or_else(|| {
                    self.error(
                        "E_DISPOSITION",
                        span,
                        "call statements require a declared operation",
                    )
                })?;
                if sig.outcomes.len() != 1 || !sig.outcomes[0].fields.is_empty() {
                    return Err(self.error("E_DISPOSITION",span,"a call statement requires exactly one outcome with no payload; use delta, capture or forward"));
                }
                let atom = format!("$call_{}", self.fresh());
                let pattern = syntax::Arm {
                    outcome: sig.outcomes[0].name.clone(),
                    bindings: vec![],
                    atom: atom.clone(),
                };
                self.scopes.push(Scope::default());
                self.decision(guard, domain, &None, operand, &[pattern], span, None, None)?;
                if let Some((Guard::Atom(id), _)) = self.path(&atom) {
                    self.result.atoms.insert(id, sig.outcomes[0].name.clone());
                }
                self.scopes.pop();
            }
            Statement::Again(name, args) => {
                let target = self
                    .loop_context
                    .clone()
                    .filter(|l| l.name == *name)
                    .ok_or_else(|| {
                        self.error(
                            "E_LOOP",
                            span,
                            format!("`again {name}` must name the enclosing tail loop"),
                        )
                    })?;
                let args = self.arguments(args, &target.parameters, domain, span)?;
                self.resources(domain, span, None)?;
                self.dead = self.bdd.or(self.dead, domain);
                self.emit(
                    guard,
                    Action::Again {
                        target: target.id,
                        args,
                    },
                    span,
                );
            }
            Statement::Bind(name, annotation, expression, mutable) => {
                if self.bind_stored(
                    name,
                    annotation.as_deref(),
                    expression,
                    *mutable,
                    (guard.clone(), domain),
                    span,
                )? {
                    return Ok(());
                }
                if self.bind_reference(
                    name,
                    annotation.as_deref(),
                    expression,
                    *mutable,
                    domain,
                    span,
                )? {
                    return Ok(());
                }
                if let Some(expected) = annotation {
                    self.manifest.check_type(expected).map_err(|mut error| {
                        error.span = span;
                        error
                    })?;
                }
                let (expr, ty) =
                    self.expr_expected(expression, domain, true, span, annotation.as_deref())?;
                if let Some(expected) = annotation {
                    self.expect_type(&ty, expected, span)?;
                }
                let id = self.define(name, &ty, domain, span)?;
                let binding = self
                    .scopes
                    .last_mut()
                    .unwrap()
                    .values
                    .get_mut(name)
                    .unwrap();
                if binding.defined != domain && binding.mutable != *mutable {
                    return Err(self.error(
                        "E_MUTABILITY",
                        span,
                        "disjoint definitions must agree on mutability",
                    ));
                }
                binding.mutable = *mutable;
                self.emit(guard, Action::Bind(id, expr), span);
            }
            Statement::Drop(expression) => {
                if self.drop_stored(expression, guard.clone(), domain, span)? {
                    return Ok(());
                }
                if let syntax::Expr::Name(name) = expression
                    && let Some((scope, loan)) = self.find_loan(name)
                {
                    let loan = loan.clone();
                    if loan.mutable {
                        let mut target = loan.target.clone();
                        target.via = Some(loan.id);
                        self.check_loans(&target, domain, true, span)?;
                    }
                    let missing = self.bdd.minus(domain, loan.active);
                    if missing != FALSE {
                        return Err(self.fail_domain(
                            "E_MOVED",
                            span,
                            "reference is unavailable or already released",
                            missing,
                        ));
                    }
                    if self.loop_context.as_ref().is_some_and(|l| scope < l.scope) {
                        return Err(self.error(
                            "E_LOOP_CAPTURE",
                            span,
                            "cannot release an outer reference inside a repeating loop",
                        ));
                    }
                    self.scopes[scope].loans.get_mut(name).unwrap().active =
                        self.bdd.minus(loan.active, domain);
                    return Ok(());
                }
                let (expr, ty) = self.expr(expression, domain, true, span)?;
                if self.manifest.contains_outcomes(&ty) {
                    return Err(self.error(
                        "E_OUTCOME",
                        span,
                        "drop cannot discard an unhandled outcome",
                    ));
                }
                if self.manifest.linear(&ty) {
                    return Err(self.error(
                        "E_RESOURCE",
                        span,
                        "drop cannot discard a must-complete resource; close or transfer it",
                    ));
                }
                self.emit(guard, Action::Drop(expr), span);
            }
            Statement::Assign(target, expression) => {
                self.assignment(target, expression, guard, domain, span)?;
            }
            Statement::Emit(expression) => {
                let dereferenced;
                let expression = if self.reference_source(expression, span)?.is_some() {
                    dereferenced = syntax::Expr::Borrow(Box::new(syntax::Expr::Deref(Box::new(
                        expression.clone(),
                    ))));
                    &dereferenced
                } else {
                    expression
                };
                let (expression, moving) = if let syntax::Expr::Borrow(inner) = expression {
                    (inner.as_ref(), false)
                } else {
                    (expression, self.modern)
                };
                let (expr, ty) = self.expr(expression, domain, moving, span)?;
                self.expect_type(&ty, "String", span)?;
                self.emit(guard, Action::Emit(expr), span);
            }
            Statement::Exit(name, args) => {
                if self.infer_exits || self.infer_payloads.contains(name) {
                    if self.infer_payloads.contains(name) && args.is_empty() {
                        return Err(self.error("E_INFER", span, format!("exit `{name}...` requires a nonempty payload; remove ... for a payload-free exit")));
                    }
                    let mut values = vec![];
                    let mut fields = vec![];
                    for (index, arg) in args.iter().enumerate() {
                        let (value, ty) = self.exit_value(name, index, arg, domain, span, None)?;
                        values.push(value);
                        fields.push(manifest::Field {
                            name: format!("value_{index}"),
                            ty,
                        });
                    }
                    if self.pending_payloads.remove(name) {
                        self.result
                            .exits
                            .iter_mut()
                            .find(|o| o.name == *name)
                            .unwrap()
                            .fields = fields;
                    } else if let Some(previous) =
                        self.result.exits.iter().find(|o| o.name == *name)
                    {
                        if previous.fields.len() != fields.len() {
                            return Err(self.error(
                                "E_INFER",
                                span,
                                format!("inconsistent payload arity for inferred exit `{name}`"),
                            ));
                        }
                        for (a, b) in previous.fields.iter().zip(&fields) {
                            self.expect_type(&b.ty, &a.ty, span)?;
                        }
                    } else {
                        self.result.exits.push(manifest::Outcome {
                            name: name.clone(),
                            fields,
                        });
                    }
                    self.resources(domain, span, None)?;
                    self.dead = self.bdd.or(self.dead, domain);
                    self.emit(guard, Action::Exit(name.clone(), values), span);
                    return Ok(());
                }
                let outcome = self
                    .result
                    .exits
                    .iter()
                    .find(|o| o.name == *name)
                    .cloned()
                    .ok_or_else(|| {
                        self.error("E_EXIT", span, format!("undeclared exit `{name}`"))
                    })?;
                if args.len() != outcome.fields.len() {
                    return Err(self.error("E_ARITY", span, "wrong exit payload arity"));
                }
                let parameters = outcome
                    .fields
                    .iter()
                    .map(|f| manifest::Parameter {
                        name: f.name.clone(),
                        ty: f.ty.clone(),
                        mode: if let Some((mutable, _)) = manifest::reference_inner(&f.ty) {
                            if mutable { Mode::Consume } else { Mode::Borrow }
                        } else {
                            Mode::Value
                        },
                    })
                    .collect::<Vec<_>>();
                self.argument_aliases(args, &parameters, domain, span)?;
                let mut values = vec![];
                for (index, (arg, field)) in args.iter().zip(&outcome.fields).enumerate() {
                    let (value, ty) =
                        self.exit_value(name, index, arg, domain, span, Some(&field.ty))?;
                    self.expect_type(&ty, &field.ty, span)?;
                    values.push(value);
                }
                self.resources(domain, span, None)?;
                self.dead = self.bdd.or(self.dead, domain);
                self.emit(guard, Action::Exit(name.clone(), values), span);
            }
            Statement::Capture(name, annotation, operand, mutable) => self.capture(
                name,
                annotation.as_deref(),
                operand,
                *mutable,
                guard,
                domain,
                span,
            )?,
            Statement::ExitName(name) => {
                if let Some((_, binding)) = self.find_value(name) {
                    if !self.manifest.is_outcome(&binding.slot.ty) {
                        return Err(self.error("E_OUTCOME",span,"exit value must be a completed outcome; use exit label {} for a nullary label"));
                    }
                    let outcomes = self.manifest.outcomes(&binding.slot.ty).unwrap();
                    let arms = self.identity_arms(&outcomes, span);
                    self.exit_outcome(
                        &syntax::OutcomeExpr::Delta(span, syntax::Expr::Name(name.clone()), arms),
                        guard,
                        domain,
                    )?;
                } else {
                    self.statement(guard, domain, &Statement::Exit(name.clone(), vec![]), span)?;
                }
            }
            Statement::ExitOutcome(e) => self.exit_outcome(e, guard, domain)?,
            Statement::BindOutcome(name, annotation, e, mutable) => {
                self.bind_outcome(
                    name,
                    annotation.as_deref(),
                    e,
                    *mutable,
                    guard,
                    domain,
                    span,
                )?;
            }
            Statement::Delta(site, operand, arms) => {
                self.decision(guard, domain, site, operand, arms, span, None, None)?
            }
        }
        Ok(())
    }
    fn identity_arms(
        &mut self,
        outcomes: &[manifest::Outcome],
        span: Span,
    ) -> Vec<syntax::InlineArm> {
        let serial = self.fresh();
        outcomes
            .iter()
            .enumerate()
            .map(|(i, o)| {
                let fields = o
                    .fields
                    .iter()
                    .enumerate()
                    .map(|(j, f)| syntax::Field {
                        type_span: Span::default(),
                        origins: vec![],
                        referent_origins: vec![],
                        name: format!("$identity_{serial}_{i}_{j}"),
                        ty: f.ty.clone(),
                    })
                    .collect::<Vec<_>>();
                syntax::InlineArm {
                    span,
                    pattern: syntax::Arm {
                        outcome: o.name.clone(),
                        bindings: fields.clone(),
                        atom: o.name.clone(),
                    },
                    result: syntax::OutcomeExpr::Construct(
                        span,
                        o.name.clone(),
                        fields
                            .iter()
                            .map(|f| syntax::Expr::Name(f.name.clone()))
                            .collect(),
                    ),
                }
            })
            .collect()
    }
    #[allow(clippy::too_many_arguments)]
    fn capture(
        &mut self,
        name: &str,
        annotation: Option<&str>,
        operand: &syntax::Expr,
        mutable: bool,
        guard: Guard,
        domain: Formula,
        span: Span,
    ) -> Result<()> {
        let syntax::Expr::Call(producer, _) = operand else {
            return Err(self.error("E_OUTCOME", span, "capture expects an operation call"));
        };
        let sig=self.signatures.get(producer).cloned().ok_or_else(||self.error("E_OUTCOME",span,"capture currently accepts source controls and manifest primitives; use delta for generic collection operations"))?;
        let producer_type = format!("outcome<{producer}>");
        let mut ty = sig
            .completion_type
            .clone()
            .unwrap_or_else(|| producer_type.clone());
        if let Some(expected) = annotation {
            if let Some(manifest::TypeDef::Outcome { constructors, .. }) =
                self.manifest.type_def(expected)
            {
                crate::algebra::outcome_embedding(&sig.outcomes, constructors)
                    .map_err(|e| self.error(e.code(), span, e.to_string()))?;
            } else if expected != producer_type {
                self.expect_type(&ty, expected, span)?;
            }
            ty = expected.into();
        }
        let serial = self.fresh();
        let patterns = sig
            .outcomes
            .iter()
            .enumerate()
            .map(|(i, o)| syntax::Arm {
                outcome: o.name.clone(),
                atom: format!("$capture_{serial}_{i}"),
                bindings: o
                    .fields
                    .iter()
                    .enumerate()
                    .map(|(j, f)| syntax::Field {
                        type_span: Span::default(),
                        origins: vec![],
                        referent_origins: vec![],
                        name: format!("$payload_{serial}_{i}_{j}"),
                        ty: f.ty.clone(),
                    })
                    .collect(),
            })
            .collect::<Vec<_>>();
        // Evaluate the call before the new binding is introduced, so self-capture
        // cannot accidentally resolve an uninitialized output as an argument.
        self.scopes.push(Scope::default());
        self.decision(
            guard.clone(),
            domain,
            &Some(format!("capture_{name}")),
            operand,
            &patterns,
            span,
            None,
            None,
        )?;
        let mut packs = vec![];
        let mut packed_loans = vec![];
        for (pattern, outcome) in patterns.iter().zip(&sig.outcomes) {
            let (selected_guard, selected) = self.path(&pattern.atom).unwrap();
            let mut fields = vec![];
            for (payload, field) in pattern.bindings.iter().enumerate() {
                let (value, _, loans) = self.pack_payload(
                    &syntax::Expr::Name(field.name.clone()),
                    Some(&field.ty),
                    &outcome.name,
                    payload,
                    selected,
                    span,
                )?;
                fields.push(value);
                packed_loans.extend(loans);
            }
            packs.push((
                guard.clone().and(selected_guard),
                Expr::Construct(ty.clone(), outcome.name.clone(), fields),
            ));
            if let Some((Guard::Atom(id), _)) = self.path(&pattern.atom) {
                self.result.atoms.insert(id, outcome.name.clone());
            }
        }
        self.resources(domain, span, Some(self.scopes.len() - 1))?;
        self.check_sum_scope(&packed_loans, span)?;
        let temporary = self.scopes.pop().unwrap();
        let slots = temporary
            .values
            .values()
            .map(|b| b.slot.id)
            .collect::<Vec<_>>();
        let id = self.define(name, &ty, domain, span)?;
        let binding = self
            .scopes
            .last_mut()
            .unwrap()
            .values
            .get_mut(name)
            .unwrap();
        if binding.defined != domain && binding.mutable != mutable {
            return Err(self.error(
                "E_MUTABILITY",
                span,
                "disjoint definitions must agree on mutability",
            ));
        }
        binding.mutable = mutable;
        self.install_sum_loans(id, packed_loans);
        for (g, pack) in packs {
            self.emit(g, Action::Bind(id, pack), span);
        }
        self.emit(guard, Action::Release(slots), span);
        Ok(())
    }
    fn exit_outcome(
        &mut self,
        e: &syntax::OutcomeExpr,
        guard: Guard,
        domain: Formula,
    ) -> Result<()> {
        self.lower_outcome(e, guard, domain, None)
    }
    #[allow(clippy::too_many_arguments)]
    fn bind_outcome(
        &mut self,
        name: &str,
        annotation: Option<&str>,
        e: &syntax::OutcomeExpr,
        mutable: bool,
        guard: Guard,
        domain: Formula,
        span: Span,
    ) -> Result<()> {
        let expected = if let Some(ty) = annotation {
            self.manifest.check_local_storage(ty)?;
            if !self.manifest.is_outcome(ty) {
                return Err(self.error(
                    "E_OUTCOME",
                    span,
                    "an adapter binding requires an outcome type",
                ));
            }
            Some(self.manifest.outcomes(ty).unwrap())
        } else {
            None
        };
        let ty = annotation
            .map(str::to_owned)
            .unwrap_or_else(|| format!("outcome<$adapter_{}>", self.fresh()));
        let mut sink = OutcomeBinding {
            loans: vec![],
            ty,
            expected,
            outcomes: vec![],
            writes: vec![],
        };
        self.lower_outcome(e, guard, domain, Some(&mut sink))?;
        if sink.writes.is_empty() {
            return Err(self.error(
                "E_INFER",
                span,
                "a stored adapter requires at least one result",
            ));
        }
        if sink.expected.is_none() {
            self.manifest.types.push(manifest::TypeDef::Variant {
                name: sink.ty.clone(),
                constructors: sink.outcomes,
            });
        }
        let id = self.define(name, &sink.ty, domain, span)?;
        let binding = self
            .scopes
            .last_mut()
            .unwrap()
            .values
            .get_mut(name)
            .unwrap();
        if binding.defined != domain && binding.mutable != mutable {
            return Err(self.error(
                "E_MUTABILITY",
                span,
                "disjoint definitions must agree on mutability",
            ));
        }
        binding.mutable = mutable;
        self.install_sum_loans(id, sink.loans);
        for index in sink.writes {
            let Action::Bind(slot, _) = &mut self.result.code[index].action else {
                unreachable!()
            };
            *slot = id;
        }
        Ok(())
    }
    fn lower_outcome(
        &mut self,
        e: &syntax::OutcomeExpr,
        guard: Guard,
        domain: Formula,
        sink: Option<&mut OutcomeBinding>,
    ) -> Result<()> {
        match e {
            syntax::OutcomeExpr::Forward(span) => Err(self.error(
                "E_FORWARD",
                *span,
                "bare forward is only valid as an inline delta arm result",
            )),
            syntax::OutcomeExpr::Construct(span, name, args) if sink.is_some() => {
                let sink = sink.unwrap();
                let expected = if let Some(contract) = &sink.expected {
                    Some(
                        contract
                            .iter()
                            .find(|o| o.name == *name)
                            .ok_or_else(|| {
                                self.error(
                                    "E_OUTCOME",
                                    *span,
                                    format!("outcome `{name}` is absent from {}", sink.ty),
                                )
                            })?
                            .fields
                            .clone(),
                    )
                } else {
                    sink.outcomes
                        .iter()
                        .find(|o| o.name == *name)
                        .map(|o| o.fields.clone())
                };
                if expected.as_ref().is_some_and(|f| f.len() != args.len()) {
                    return Err(self.error(
                        "E_ARITY",
                        *span,
                        format!("inconsistent payload arity for outcome `{name}`"),
                    ));
                }
                let mut fields = vec![];
                let mut values = vec![];
                for (index, arg) in args.iter().enumerate() {
                    let expected_ty = expected.as_ref().map(|f| f[index].ty.as_str());
                    let (value, ty, loans) =
                        self.pack_payload(arg, expected_ty, name, index, domain, *span)?;
                    sink.loans.extend(loans);
                    if let Some(expected) = expected_ty {
                        self.expect_type(&ty, expected, *span)?;
                    }
                    fields.push(manifest::Field {
                        name: format!("value_{index}"),
                        ty,
                    });
                    values.push(value);
                }
                if !sink.outcomes.iter().any(|o| o.name == *name) {
                    sink.outcomes.push(manifest::Outcome {
                        name: name.clone(),
                        fields,
                    });
                }
                sink.writes.push(self.result.code.len());
                self.emit(
                    guard,
                    Action::Bind(0, Expr::Construct(sink.ty.clone(), name.clone(), values)),
                    *span,
                );
                Ok(())
            }
            syntax::OutcomeExpr::Construct(span, name, args) => self.statement(
                guard,
                domain,
                &Statement::Exit(name.clone(), args.clone()),
                *span,
            ),
            syntax::OutcomeExpr::Delta(span, operand, arms) => {
                let patterns = arms.iter().map(|a| a.pattern.clone()).collect::<Vec<_>>();
                self.decision(
                    guard,
                    domain,
                    &None,
                    operand,
                    &patterns,
                    *span,
                    Some(arms),
                    sink,
                )
            }
        }
    }
    #[allow(clippy::too_many_arguments)]
    fn decision(
        &mut self,
        guard: Guard,
        domain: Formula,
        site: &Option<String>,
        operand: &syntax::Expr,
        arms: &[syntax::Arm],
        span: Span,
        inline: Option<&[syntax::InlineArm]>,
        mut sink: Option<&mut OutcomeBinding>,
    ) -> Result<()> {
        if let Some(site) = site
            && !self.scopes.last_mut().unwrap().sites.insert(site.clone())
        {
            return Err(self.error("E_SITE", span, format!("duplicate decision site `{site}`")));
        }
        let collection = self.prepare_collection_loans(operand, domain, span)?;
        let mut return_loans = BTreeMap::new();
        if let Some(plan) = &collection {
            for outcome in &plan.outcomes {
                for (index, field) in outcome.fields.iter().enumerate() {
                    if manifest::reference_inner(&field.ty).is_some() {
                        let loans = plan.result_loans(&outcome.name);
                        let mut origins = vec![];
                        let mut parents = vec![];
                        for loan in loans {
                            for origin in &loan.origins {
                                if !origins.contains(origin) {
                                    origins.push(origin.clone());
                                }
                            }
                            for id in loan.parents.iter().copied().chain([loan.id]) {
                                if !parents.contains(&id) {
                                    parents.push(id);
                                }
                            }
                        }
                        return_loans
                            .insert((outcome.name.clone(), index, vec![]), (origins, parents));
                    }
                }
            }
        }
        let mut borrowed_contents = vec![];
        // A nested tag cannot be projected until its own delta runs. Carry its
        // checked loan intact, rebasing only the payload's storage holder.
        let mut nested_loans = vec![];
        let mut retained_contents = vec![];
        let mut public_returns = vec![];
        if let syntax::Expr::Call(callee, call_args) = operand
            && let Some(sig) = self.signature(callee, call_args, span)?
        {
            if call_args.len() != sig.parameters.len() {
                return Err(self.error("E_ARITY", span, "wrong argument count"));
            }
            let returns = sig
                .borrow_returns
                .iter()
                .map(|((o, i), sources)| ((o.clone(), *i, vec![]), sources, None, None))
                .chain(sig.stored_returns.iter().map(|r| {
                    (
                        (r.outcome.clone(), r.payload, r.fields.clone()),
                        &r.sources,
                        r.sum_leaf.as_ref(),
                        Some(r.through.clone()),
                    )
                }));
            // The checked exit construction guarantees that distinct reference
            // leaves coexist without exclusive aliasing, even if their lifetime
            // anchors are identical or unioned across different control paths.
            let family = self.fresh();
            for (member, (key, sources, tag, through)) in returns.enumerate() {
                let mut origins = vec![];
                let mut parents = vec![];
                for source in sources {
                    let dependencies = self.call_origin_dependencies(
                        source,
                        &call_args[source.parameter],
                        &sig.parameters[source.parameter],
                        domain,
                        span,
                    )?;
                    for (target_origins, target_parents) in dependencies {
                        for mut origin in target_origins {
                            origin.project(&source.fields);
                            origin.guard |= source.guard;
                            origin.opaque |= source.opaque;
                            if !origin.guard {
                                origin.permissions.push(Permission::new(family, member));
                            }
                            if !origins.contains(&origin) {
                                origins.push(origin);
                            }
                        }
                        for id in target_parents {
                            if !parents.contains(&id) {
                                parents.push(id);
                            }
                        }
                    }
                }
                if let Some(through) = through {
                    public_returns.push((
                        key.0,
                        key.1,
                        (
                            through,
                            ReferenceStep {
                                fields: key.2,
                                sum_leaf: tag.cloned(),
                            },
                            origins,
                            parents,
                        ),
                    ));
                } else {
                    return_loans.insert(key, (origins, parents));
                }
            }
        }
        let (operation, args, outcomes, variant_type) = if let syntax::Expr::Call(name, args) =
            operand
            && let Some(sig) = self.signature(name, args, span)?
        {
            let args = self.arguments(args, &sig.parameters, domain, span)?;
            (
                if sig.region {
                    Operation::Region(name.clone())
                } else if crate::collections::is_operation(name) {
                    Operation::Collection(name.clone())
                } else {
                    Operation::Primitive(name.clone())
                },
                args,
                sig.outcomes,
                None,
            )
        } else if let Some((target, mutable)) = self.borrowed_sum_target(operand, domain, span)? {
            let ty = format!("&{}{}", if mutable { "mutable " } else { "" }, target.ty);
            let outcomes = self.borrowed_sum_payloads(
                &target,
                mutable,
                &mut return_loans,
                &mut borrowed_contents,
                &mut retained_contents,
                domain,
                span,
            )?;
            (
                Operation::BorrowedVariant(Expr::Reference(target.place, ty)),
                vec![],
                outcomes,
                Some(target.ty),
            )
        } else {
            // A resource-carrying sum is consumed as a whole. Its sealed
            // obligation becomes the selected arm's payload obligations.
            let (expr, ty) = if let Some(ty) = self.stored_type(operand, span)?.filter(|t| {
                self.manifest.contains_references(t) && self.manifest.outcomes(t).is_some()
            }) {
                let stored = self.stored_value(operand, &ty, domain, span, true)?;
                for (_, id) in stored.loans {
                    let mut loan = self.loan(id).clone();
                    let mut leaf = loan.sum_leaf.take().expect("tagged loan");
                    if let Some(nested) = leaf.nested.take() {
                        loan.sum_leaf = Some(*nested);
                        nested_loans.push((leaf, loan));
                    } else {
                        if !loan.contents.is_empty() {
                            retained_contents.push((leaf.clone(), loan.contents.clone()));
                        }
                        let entry = return_loans
                            .entry((leaf.outcome, leaf.payload, leaf.fields))
                            .or_insert((vec![], vec![]));
                        for origin in loan.origins {
                            if !entry.0.contains(&origin) {
                                entry.0.push(origin);
                            }
                        }
                        for parent in loan.parents {
                            if !entry.1.contains(&parent) {
                                entry.1.push(parent);
                            }
                        }
                    }
                    for scope in &mut self.scopes {
                        if let Some(loan) = scope.loans.values_mut().find(|l| l.id == id) {
                            loan.active = FALSE;
                        }
                    }
                }
                self.empty_sum_payloads(&ty, &mut return_loans);
                (stored.expression, ty)
            } else {
                self.expr(operand, domain, true, span)?
            };
            let outcomes = self.manifest.outcomes(&ty).ok_or_else(|| {
                self.error(
                    "E_DECISION",
                    span,
                    format!("{ty} is not a closed variant or Bool"),
                )
            })?;
            (Operation::Variant(expr), vec![], outcomes, Some(ty))
        };
        let mut arms = arms.to_vec();
        for arm in &mut arms {
            if let Some((qualifier, outcome)) = arm.outcome.rsplit_once('.') {
                let valid = variant_type.as_deref().is_some_and(|ty| {
                    qualifier == ty
                        || (["Option", "option"].contains(&qualifier)
                            && manifest::option_inner(ty).is_some())
                });
                if !valid {
                    return Err(self.error(
                        "E_ARMS",
                        span,
                        format!("wrong variant qualifier in `{}`", arm.outcome),
                    ));
                }
                arm.outcome = outcome.to_owned();
            }
            if variant_type
                .as_deref()
                .and_then(manifest::option_inner)
                .is_some()
            {
                arm.outcome = match arm.outcome.as_str() {
                    "some" => "Some".into(),
                    "none" => "None".into(),
                    _ => arm.outcome.clone(),
                };
            }
        }
        let mut seen = BTreeSet::new();
        for arm in &arms {
            if !seen.insert(arm.outcome.clone()) {
                return Err(self.error(
                    "E_ARMS",
                    span,
                    format!("duplicate outcome `{}`", arm.outcome),
                ));
            }
            if !outcomes.iter().any(|o| o.name == arm.outcome) {
                return Err(self.error(
                    "E_ARMS",
                    span,
                    format!("unknown outcome `{}`", arm.outcome),
                ));
            }
        }
        let missing = outcomes
            .iter()
            .filter(|o| !seen.contains(&o.name))
            .map(|o| o.name.clone())
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            return Err(self.error(
                "E_ARMS",
                span,
                format!("missing explicit outcomes: {}", missing.join(", ")),
            ));
        }
        if let Some(inline) = inline {
            let serial = self.fresh();
            for (index, (arm, source)) in arms.iter_mut().zip(inline).enumerate() {
                if matches!(source.result, syntax::OutcomeExpr::Forward(_))
                    && arm.bindings.is_empty()
                {
                    let outcome = outcomes.iter().find(|o| o.name == arm.outcome).unwrap();
                    arm.bindings = outcome
                        .fields
                        .iter()
                        .enumerate()
                        .map(|(j, f)| syntax::Field {
                            type_span: Span::default(),
                            origins: vec![],
                            referent_origins: vec![],
                            name: format!("$identity_{serial}_{index}_{j}"),
                            ty: f.ty.clone(),
                        })
                        .collect();
                }
            }
        }
        let mut remaining = domain;
        let mut resolved = vec![];
        let mut branch_scopes = vec![];
        let mut stored_bindings = vec![];
        for (index, arm) in arms.iter().enumerate() {
            let span = inline.map_or(span, |arms| arms[index].span);
            if inline.is_some() {
                self.scopes.push(Scope::default());
            }
            if inline.is_none() && self.path(&arm.atom).is_some() {
                return Err(self.error(
                    "E_PATH",
                    span,
                    format!("path atom `{}` is not fresh", arm.atom),
                ));
            }
            let outcome = outcomes.iter().find(|o| o.name == arm.outcome).unwrap();
            if arm.bindings.len() != outcome.fields.len() {
                return Err(self.error(
                    "E_ARITY",
                    span,
                    format!("wrong payload arity for {}", arm.outcome),
                ));
            }
            let atom = self.fresh();
            let selected = if index + 1 == arms.len() {
                remaining
            } else {
                let variable = self.bdd.variable(atom);
                let selected = self.bdd.and(remaining, variable);
                remaining = self.bdd.minus(remaining, variable);
                selected
            };
            self.scopes
                .last_mut()
                .unwrap()
                .paths
                .insert(arm.atom.clone(), (Guard::Atom(atom), selected));
            self.result.atoms.insert(atom, arm.atom.clone());
            self.atom_formulas.insert(atom, selected);
            if let Some(plan) = &collection {
                self.update_collection_loans(plan, &arm.outcome, selected);
            }
            let mut slots = vec![];
            for (field_index, (binding, field)) in
                arm.bindings.iter().zip(&outcome.fields).enumerate()
            {
                if binding.type_span.line != 0 {
                    self.inferred_bindings.push(InferredBinding {
                        control: self.result.name.clone(),
                        display_type: self.manifest.display_inferred_type(&field.ty),
                        span: binding.type_span,
                        name: binding.name.clone(),
                    });
                }
                if binding.ty != "auto" {
                    self.expect_binding_type(&field.ty, &binding.ty, span)?;
                }
                if let Some((mutable, inner)) = manifest::reference_inner(&field.ty) {
                    if self.find_value(&binding.name).is_some()
                        || self.find_loan(&binding.name).is_some()
                    {
                        return Err(self.error(
                            "E_REDEFINE",
                            span,
                            "reference payload names must be unique",
                        ));
                    }
                    let (origins, parents) = return_loans
                        .get(&(arm.outcome.clone(), field_index, vec![]))
                        .cloned()
                        .ok_or_else(|| {
                            self.error(
                                "E_LIFETIME",
                                span,
                                "missing checked return lifetime contract",
                            )
                        })?;
                    let key = format!("$reference_{}", binding.name);
                    let id = self.define(&key, &field.ty, selected, span)?;
                    self.result.slots.get_mut(&id).unwrap().name = binding.name.clone();
                    self.add_loan(
                        &binding.name,
                        ResolvedPlace {
                            carriers: vec![],
                            scope: self.scopes.len() - 1,
                            name: key,
                            place: Place {
                                root: id,
                                fields: vec![],
                                indirect: true,
                            },
                            ty: inner.into(),
                            via: None,
                        },
                        origins,
                        parents,
                        mutable,
                        selected,
                    );
                    for ((o, i, fields), (origins, parents)) in &return_loans {
                        if o == &arm.outcome && *i == field_index && !fields.is_empty() {
                            let (address, expr) = self.receive_stored(
                                &binding.name,
                                id,
                                inner,
                                fields,
                                origins.clone(),
                                parents.clone(),
                                selected,
                                span,
                            )?;
                            stored_bindings.push((atom, address, expr));
                        }
                    }
                    if matches!(
                        operation,
                        Operation::Variant(_) | Operation::BorrowedVariant(_)
                    ) {
                        let mut contents: Vec<_> = borrowed_contents
                            .iter()
                            .filter(|(leaf, _)| {
                                leaf.outcome == arm.outcome && leaf.payload == field_index
                            })
                            .map(|(leaf, loan)| (leaf.fields.clone(), loan.clone()))
                            .collect();
                        contents.extend(
                            retained_contents
                                .iter()
                                .filter(|(leaf, _)| {
                                    leaf.outcome == arm.outcome
                                        && leaf.payload == field_index
                                        && leaf.fields.is_empty()
                                })
                                .flat_map(|(_, contents)| contents.clone()),
                        );
                        for (address, value) in
                            self.restore_carrier_contents(id, inner, &contents, selected, span)?
                        {
                            stored_bindings.push((atom, address, value));
                        }
                    }
                    slots.push(id);
                } else {
                    if self.manifest.contains_references(&field.ty)
                        && (self.find_value(&binding.name).is_some()
                            || self.find_loan(&binding.name).is_some())
                    {
                        return Err(self.error(
                            "E_REDEFINE",
                            span,
                            "returned record payload names must be unique",
                        ));
                    }
                    let root = self.define(&binding.name, &field.ty, selected, span)?;
                    if let Some(plan) = &collection
                        && crate::collection_support::Collection::parse(&field.ty).is_some()
                    {
                        self.attach_collection_loans(
                            root,
                            plan.result_loans(&arm.outcome),
                            selected,
                        );
                    }
                    for (leaf, loan) in &nested_loans {
                        if leaf.outcome == arm.outcome && leaf.payload == field_index {
                            let active = self.bdd.and(selected, loan.active);
                            let id = self.stash_stored_loan(loan.clone(), active);
                            let stored = self
                                .scopes
                                .last_mut()
                                .unwrap()
                                .loans
                                .values_mut()
                                .find(|l| l.id == id)
                                .unwrap();
                            stored.holder = Some(Place {
                                root,
                                fields: leaf.fields.clone(),
                                indirect: false,
                            });
                        }
                    }
                    for ((o, i, path), (origins, parents)) in &return_loans {
                        if o == &arm.outcome && *i == field_index && !path.is_empty() {
                            let (id, value) = self.receive_stored(
                                &binding.name,
                                root,
                                &field.ty,
                                path,
                                origins.clone(),
                                parents.clone(),
                                selected,
                                span,
                            )?;
                            stored_bindings.push((atom, id, value));
                            let contents: Vec<_> = retained_contents
                                .iter()
                                .filter(|(leaf, _)| {
                                    leaf.outcome == *o && leaf.payload == *i && leaf.fields == *path
                                })
                                .flat_map(|(_, contents)| contents.clone())
                                .collect();
                            let inner = manifest::reference_inner(&self.result.slots[&id].ty)
                                .unwrap()
                                .1
                                .to_owned();
                            let loan = self
                                .scopes
                                .last_mut()
                                .unwrap()
                                .loans
                                .values_mut()
                                .find(|l| l.target.place.root == id)
                                .unwrap();
                            loan.contents = contents.clone();
                            for (address, value) in self
                                .restore_carrier_contents(id, &inner, &contents, selected, span)?
                            {
                                stored_bindings.push((atom, address, value));
                            }
                        }
                    }
                    slots.push(root);
                }
                let entries = public_returns
                    .iter()
                    .filter(|(o, i, _)| *o == arm.outcome && *i == field_index)
                    .map(|(_, _, e)| e.clone())
                    .collect::<Vec<_>>();
                if !entries.is_empty() {
                    let root = *slots.last().unwrap();
                    let ty =
                        manifest::reference_inner(&field.ty).map_or(field.ty.as_str(), |(_, t)| t);
                    let (bindings, _) =
                        self.receive_public_contents(root, ty, &entries, selected, span)?;
                    stored_bindings
                        .extend(bindings.into_iter().map(|(id, value)| (atom, id, value)));
                }
            }
            if inline.is_some() {
                branch_scopes.push((self.scopes.pop().unwrap(), atom, selected));
            }
            resolved.push(Arm {
                outcome: arm.outcome.clone(),
                atom,
                slots,
            });
        }
        self.emit(
            guard.clone(),
            Action::Decide {
                operation,
                args,
                arms: resolved,
            },
            span,
        );
        self.result.code.last_mut().unwrap().provenance.site = site.clone();
        for (atom, id, value) in stored_bindings {
            self.emit(
                guard.clone().and(Guard::Atom(atom)),
                Action::Bind(id, value),
                span,
            );
        }
        if let Some(inline) = inline {
            for ((arm, pattern), (scope, atom, selected)) in
                inline.iter().zip(&arms).zip(branch_scopes)
            {
                self.scopes.push(scope);
                let branch_guard = guard.clone().and(Guard::Atom(atom));
                let forwarded;
                let result = if let syntax::OutcomeExpr::Forward(span) = &arm.result {
                    forwarded = syntax::OutcomeExpr::Construct(
                        *span,
                        pattern.outcome.clone(),
                        pattern
                            .bindings
                            .iter()
                            .map(|f| syntax::Expr::Name(f.name.clone()))
                            .collect(),
                    );
                    &forwarded
                } else {
                    &arm.result
                };
                self.lower_outcome(result, branch_guard.clone(), selected, sink.as_deref_mut())?;
                if let Some(sink) = sink.as_ref() {
                    self.check_sum_scope(&sink.loans, arm.span)?;
                    self.resources(selected, arm.span, Some(self.scopes.len() - 1))?;
                    let slots = self
                        .scopes
                        .last()
                        .unwrap()
                        .values
                        .values()
                        .map(|b| b.slot.id)
                        .collect();
                    self.emit(branch_guard, Action::Release(slots), arm.span);
                }
                self.scopes.pop();
            }
        }
        Ok(())
    }
    fn expire_loans(&mut self, live: &BTreeSet<String>) {
        // A live borrow of a carrier keeps its contained loans live as well.
        let mut live = live.clone();
        loop {
            let previous = live.len();
            for (name, loan) in self.scopes.iter().flat_map(|s| &s.loans) {
                if self.manifest.contains_references(&loan.target.ty)
                    && live.iter().any(|n| {
                        crate::liveness::overlaps(
                            n,
                            &loan
                                .holder
                                .as_ref()
                                .map(|h| self.place_name(h))
                                .unwrap_or_else(|| name.clone()),
                        )
                    })
                {
                    live.insert(self.place_name(&loan.target.place));
                }
            }
            if previous == live.len() {
                break;
            }
        }
        for (scope, s) in self.scopes.iter_mut().enumerate() {
            if self.loop_context.as_ref().is_some_and(|l| scope < l.scope) {
                continue;
            }
            for (name, loan) in &mut s.loans {
                let live_name = if let Some(h) = &loan.holder {
                    let slot = &self.result.slots[&h.root];
                    let mut name = slot.name.clone();
                    let mut ty = if h.indirect {
                        manifest::reference_inner(&slot.ty).unwrap().1.to_owned()
                    } else {
                        slot.ty.clone()
                    };
                    for index in &h.fields {
                        let (suffix, inner) = self
                            .manifest
                            .borrow_field(&ty, *index)
                            .expect("stored reference field");
                        name.push_str(&suffix);
                        ty = inner;
                    }
                    name
                } else {
                    name.clone()
                };
                if !live
                    .iter()
                    .any(|n| crate::liveness::overlaps(n, &live_name))
                {
                    loan.active = FALSE;
                }
            }
        }
    }
    fn items(&mut self, items: &[Item], outer: Guard, context: Formula) -> Result<()> {
        let continuation = self.future_names.clone();
        for (index, item) in items.iter().enumerate() {
            let mut future = continuation.clone();
            crate::liveness::items(&items[index + 1..], &mut future);
            self.future_names = future.clone();
            let mut live = future.clone();
            crate::liveness::items(std::slice::from_ref(item), &mut live);
            self.expire_loans(&live);
            match item {
                Item::Loop(span, name, parameters, body) => {
                    if index + 1 != items.len() {
                        return Err(self.error(
                            "E_LOOP",
                            *span,
                            "tail loop must be the last item of its containing body",
                        ));
                    }
                    if self.loop_context.is_some() {
                        return Err(self.error(
                            "E_LOOP",
                            *span,
                            "lexically nested loops are not supported",
                        ));
                    }
                    let domain = self.bdd.minus(context, self.dead);
                    if domain == FALSE {
                        return Err(self.error(
                            "E_EMPTY",
                            *span,
                            "loop has no live entry histories",
                        ));
                    }
                    let params: Vec<_> = parameters
                        .iter()
                        .map(|p| manifest::Parameter {
                            name: p.field.name.clone(),
                            ty: p.field.ty.clone(),
                            mode: if self.manifest.linear(&p.field.ty) {
                                Mode::Consume
                            } else {
                                Mode::Value
                            },
                        })
                        .collect();
                    let mut names = BTreeSet::new();
                    for p in &params {
                        self.manifest.check_type(&p.ty)?;
                        if !names.insert(&p.name) {
                            return Err(self.error("E_NAME", *span, "duplicate loop parameter"));
                        }
                    }
                    let initial = self.arguments(
                        &parameters
                            .iter()
                            .map(|p| p.initial.clone())
                            .collect::<Vec<_>>(),
                        &params,
                        domain,
                        *span,
                    )?;
                    // Outer linear obligations must enter through explicit loop parameters.
                    self.resources(domain, *span, None)?;
                    let old_slots: BTreeSet<_> = self.result.slots.keys().copied().collect();
                    let old_atoms: BTreeSet<_> = self.result.atoms.keys().copied().collect();
                    let enclosing_code = std::mem::take(&mut self.result.code);
                    let enclosing_dead = self.dead;
                    self.scopes.push(Scope::default());
                    self.scopes
                        .last_mut()
                        .unwrap()
                        .paths
                        .insert("Entry".into(), (Guard::True, domain));
                    let mut bindings = vec![];
                    for (p, initial) in params.iter().zip(initial) {
                        let slot = self.define(&p.name, &p.ty, domain, *span)?;
                        bindings.push((slot, initial));
                    }
                    let id = self.fresh();
                    self.loop_context = Some(LoopContext {
                        name: name.clone(),
                        id,
                        parameters: params,
                        scope: self.scopes.len() - 1,
                    });
                    self.items(body, Guard::True, domain)?;
                    let uncovered = self.bdd.minus(domain, self.dead);
                    if uncovered != FALSE {
                        return Err(self.fail_domain(
                            "E_UNCOVERED",
                            *span,
                            "loop has histories with no exit or again",
                            uncovered,
                        ));
                    }
                    self.loop_context = None;
                    self.scopes.pop();
                    let body = std::mem::replace(&mut self.result.code, enclosing_code);
                    // The tail loop does not return to this lexical continuation.
                    self.dead = self.bdd.or(enclosing_dead, domain);
                    let local_slots = self
                        .result
                        .slots
                        .keys()
                        .filter(|id| !old_slots.contains(id))
                        .copied()
                        .collect();
                    let local_atoms = self
                        .result
                        .atoms
                        .keys()
                        .filter(|id| !old_atoms.contains(id))
                        .copied()
                        .collect();
                    self.emit(
                        outer.clone(),
                        Action::Loop {
                            id,
                            parameters: bindings,
                            local_slots,
                            local_atoms,
                            body,
                        },
                        *span,
                    );
                }
                Item::Alias(span, name, mask) => {
                    if self.path(name).is_some() {
                        return Err(self.error(
                            "E_PATH",
                            *span,
                            format!("path `{name}` already exists"),
                        ));
                    }
                    let (guard, domain) = self.mask(mask, *span)?;
                    let domain = self.bdd.and(context, domain);
                    self.scopes
                        .last_mut()
                        .unwrap()
                        .paths
                        .insert(name.clone(), (outer.clone().and(guard), domain));
                }
                Item::Guard(mask, statements) => {
                    for (index, (span, statement)) in statements.iter().enumerate() {
                        let mut live = future.clone();
                        for (_, s) in &statements[index..] {
                            crate::liveness::statement(s, &mut live);
                        }
                        self.expire_loans(&live);
                        let (guard, domain) = self.mask(mask, *span)?;
                        let domain = self.bdd.and(context, domain);
                        self.statement(outer.clone().and(guard), domain, statement, *span)?;
                    }
                }
                Item::Over(span, mask, body) => {
                    let (guard, domain) = self.mask(mask, *span)?;
                    let domain = self.bdd.and(context, domain);
                    self.scopes.push(Scope::default());
                    self.items(body, outer.clone().and(guard), domain)?;
                    self.resources(domain, *span, Some(self.scopes.len() - 1))?;
                    let slots = self
                        .scopes
                        .last()
                        .unwrap()
                        .values
                        .values()
                        .map(|b| b.slot.id)
                        .collect::<Vec<_>>();
                    if !slots.is_empty() {
                        let (guard, _) = self.mask(mask, *span)?;
                        self.emit(outer.clone().and(guard), Action::Release(slots), *span);
                    }
                    self.scopes.pop();
                }
            }
        }
        self.future_names = continuation;
        Ok(())
    }
}

pub fn compile(source: &str, manifest: Manifest) -> Result<CheckedProgram> {
    let unit = syntax::parse_unit(source)?;
    if let Some(import) = unit.imports.first() {
        return Err(Diagnostic::new(
            "E_IMPORT",
            import.span,
            "imports require a source path; use compile_file or the tellurc CLI",
        ));
    }
    compile_unit(unit, manifest, &crate::modules::Visibility::default())
}
pub(crate) fn compile_unit(
    unit: syntax::Unit,
    manifest: Manifest,
    visibility: &crate::modules::Visibility,
) -> Result<CheckedProgram> {
    let mut visibility = visibility.clone();
    let unit = crate::traits::lower(unit, &manifest, &mut visibility)?;
    crate::generic_controls::compile(unit, manifest, &visibility)
}
pub(crate) fn compile_concrete_unit(
    unit: syntax::Unit,
    mut manifest: Manifest,
    visibility: &crate::modules::Visibility,
    type_arguments: &[crate::generic_controls::TypeArgument],
) -> Result<CheckedProgram> {
    manifest.canonicalize_types();
    manifest.validate()?;
    let public_interfaces = crate::modules::PublicInterfaces::new(&unit);
    let mut argument_types = type_arguments
        .iter()
        .map(|a| (a.ty.clone(), a.span))
        .collect::<Vec<_>>();
    argument_types.extend(crate::traits::required_types(&unit.trait_checks));
    let unit = crate::generics::instantiate(unit, &manifest, &argument_types)?;
    let origin_schemas = crate::origins::schemas(&unit)?;
    let mut parsed = unit.regions;
    for data in &unit.data {
        let fields = |fs: &[syntax::Field]| {
            fs.iter()
                .map(|f| manifest::Field {
                    name: f.name.clone(),
                    ty: f.ty.clone(),
                })
                .collect()
        };
        manifest.types.push(match &data.body {
            syntax::DataBody::Struct(fs) => manifest::TypeDef::Record {
                name: data.name.clone(),
                fields: fields(fs),
            },
            syntax::DataBody::Variant(os) => manifest::TypeDef::Variant {
                name: data.name.clone(),
                constructors: os
                    .iter()
                    .map(|o| manifest::Outcome {
                        name: o.name.clone(),
                        fields: fields(&o.fields),
                    })
                    .collect(),
            },
        });
    }
    for contract in &unit.contracts {
        manifest.types.push(manifest::TypeDef::Outcome {
            name: contract.name.clone(),
            constructors: contract
                .outcomes
                .iter()
                .map(|o| manifest::Outcome {
                    name: o.name.clone(),
                    fields: o
                        .fields
                        .iter()
                        .map(|f| manifest::Field {
                            name: f.name.clone(),
                            ty: f.ty.clone(),
                        })
                        .collect(),
                })
                .collect(),
        });
    }
    // Keep declaration locations when validating the combined finite layouts.
    let mut declarations = unit
        .data
        .iter()
        .map(|d| (d.name.clone(), d.span))
        .chain(unit.contracts.iter().map(|d| (d.name.clone(), d.span)))
        .collect::<Vec<_>>();
    declarations.sort_by_key(|(_, span)| (span.line, span.column));
    let declaration_spans = declarations.into_iter().collect();
    manifest.validate_at(&declaration_spans)?;
    crate::traits::check_contracts(
        &unit.trait_checks,
        &parsed,
        &unit.contracts,
        &origin_schemas,
        &manifest,
    )?;
    for argument in type_arguments {
        let (ty, at) = (&argument.ty, argument.span);
        manifest.check_type(ty).map_err(|mut e| {
            e.span = at;
            e
        })?;
        for bound in &argument.bounds {
            if !manifest.has_capability(ty, bound)
                && !crate::traits::has_witness(&unit.trait_checks, bound, ty)
            {
                return Err(Diagnostic::new(
                    "E_BOUND",
                    at,
                    format!(
                        "type `{}` does not satisfy capability `{bound}`",
                        manifest::display_type(ty)
                    ),
                ));
            }
        }
    }
    for region in &mut parsed {
        if let Some(c) = region
            .exit_contract
            .as_ref()
            .and_then(|name| unit.contracts.iter().find(|c| c.name == *name))
        {
            crate::origins::bind_contract(region, c)?;
            continue;
        }
        if !region.exit_origins.is_empty() {
            return Err(Diagnostic::new(
                "E_ORIGIN",
                region.span,
                "origin arguments require a source-declared named outcome contract",
            ));
        }
        if let Some(name) = &region.exit_contract
            && let Some(manifest::TypeDef::Outcome { constructors, .. }) = manifest.type_def(name)
        {
            region.exits = constructors
                .iter()
                .map(|o| syntax::Outcome {
                    name: o.name.clone(),
                    fields: o
                        .fields
                        .iter()
                        .map(|f| syntax::Field {
                            type_span: Span::default(),
                            origins: vec![],
                            referent_origins: vec![],
                            name: f.name.clone(),
                            ty: f.ty.clone(),
                        })
                        .collect(),
                })
                .collect();
        } else {
            region.exit_contract = None;
        }
    }
    // These private schemas cannot be declared by a foreign manifest (its names
    // must be identifiers). Keep them out of ordinary constructor lookup.
    for p in manifest.primitives.clone() {
        manifest.types.push(manifest::TypeDef::Variant {
            name: format!("outcome<{}>", p.name),
            constructors: p.outcomes,
        });
    }
    for r in &parsed {
        manifest.types.push(manifest::TypeDef::Variant {
            name: format!("outcome<{}>", r.name),
            constructors: r
                .exits
                .iter()
                .map(|o| manifest::Outcome {
                    name: o.name.clone(),
                    fields: o
                        .fields
                        .iter()
                        .map(|f| manifest::Field {
                            name: f.name.clone(),
                            ty: f.ty.clone(),
                        })
                        .collect(),
                })
                .collect(),
        });
    }
    let mut signatures = BTreeMap::new();
    for p in &manifest.primitives {
        signatures.insert(
            p.name.clone(),
            Signature {
                stored_returns: vec![],
                parameters: p.parameters.clone(),
                outcomes: p.outcomes.clone(),
                region: false,
                completion_type: None,
                borrow_returns: BTreeMap::new(),
            },
        );
    }
    for region in &parsed {
        if signatures.contains_key(&region.name)
            || crate::collections::is_operation(&region.name)
            || crate::collections::is_length(&region.name)
        {
            return Err(Diagnostic::new(
                "E_NAME",
                region.span,
                format!("duplicate operation `{}`", region.name),
            ));
        }
        let mut names = BTreeSet::new();
        for f in &region.parameters {
            if let Some((_, inner)) = manifest::reference_inner(&f.ty) {
                crate::origins::input_type(inner, &origin_schemas, &manifest, region.span)?;
                crate::origins::borrowed_input_type(
                    inner,
                    &origin_schemas,
                    &manifest,
                    region.span,
                )?;
            } else {
                crate::origins::input_type(&f.ty, &origin_schemas, &manifest, region.span)?;
            }
            if !names.insert(&f.name) {
                return Err(Diagnostic::new(
                    "E_NAME",
                    region.span,
                    "duplicate parameter",
                ));
            }
        }
        let mut names = BTreeSet::new();
        for o in &region.exits {
            if !names.insert(&o.name) {
                return Err(Diagnostic::new("E_NAME", region.span, "duplicate exit"));
            }
            let mut fields = BTreeSet::new();
            for f in &o.fields {
                crate::origins::result_type(&f.ty, &origin_schemas, &manifest, region.span)?;
                if !fields.insert(&f.name) {
                    return Err(Diagnostic::new(
                        "E_NAME",
                        region.span,
                        "duplicate exit field",
                    ));
                }
            }
        }
        signatures.insert(
            region.name.clone(),
            Signature {
                stored_returns: crate::origins::declared(region, &origin_schemas)?,
                parameters: region
                    .parameters
                    .iter()
                    .map(|f| manifest::Parameter {
                        name: f.name.clone(),
                        ty: f.ty.clone(),
                        mode: if let Some((mutable, _)) = manifest::reference_inner(&f.ty) {
                            if mutable { Mode::Consume } else { Mode::Borrow }
                        } else if manifest.linear(&f.ty)
                            || (region.modern && !manifest.is_copy(&f.ty))
                        {
                            Mode::Consume
                        } else {
                            Mode::Value
                        },
                    })
                    .collect(),
                outcomes: region
                    .exits
                    .iter()
                    .map(|o| manifest::Outcome {
                        name: o.name.clone(),
                        fields: o
                            .fields
                            .iter()
                            .map(|f| manifest::Field {
                                name: f.name.clone(),
                                ty: f.ty.clone(),
                            })
                            .collect(),
                    })
                    .collect(),
                region: true,
                completion_type: region.exit_contract.clone(),
                borrow_returns: crate::lifetimes::declared_with_schemas(
                    region,
                    region.abstract_signature,
                    &origin_schemas,
                )?,
            },
        );
    }
    let mut regions = BTreeMap::new();
    let mut inferred_bindings = vec![];
    let mut next = 0;
    for index in crate::inference::dependency_order(&parsed)? {
        let source = &parsed[index];
        for f in &source.parameters {
            if let Some((_, inner)) = manifest::reference_inner(&f.ty) {
                crate::origins::input_type(inner, &origin_schemas, &manifest, source.span)?;
                crate::origins::borrowed_input_type(
                    inner,
                    &origin_schemas,
                    &manifest,
                    source.span,
                )?;
            } else {
                crate::origins::input_type(&f.ty, &origin_schemas, &manifest, source.span)?;
            }
        }
        for f in source.exits.iter().flat_map(|o| &o.fields) {
            crate::origins::result_type(&f.ty, &origin_schemas, &manifest, source.span)?;
        }
        let mut checker = Checker {
            inferred_bindings: vec![],
            contained_inputs: BTreeMap::new(),
            origin_schemas: &origin_schemas,
            declared_stored: signatures[&source.name].stored_returns.clone(),
            declared_returns: signatures[&source.name].borrow_returns.clone(),
            manifest: &mut manifest,
            signatures: &signatures,
            next: &mut next,
            bdd: Bdd::default(),
            dead: FALSE,
            scopes: vec![Scope::default()],
            atom_formulas: BTreeMap::new(),
            loop_context: None,
            infer_exits: source.infer_exits,
            infer_payloads: source.infer_payloads.clone(),
            pending_payloads: source.infer_payloads.clone(),
            modern: source.modern,
            future_names: BTreeSet::new(),
            visibility,
            type_bindings: &source.type_bindings,
            result: Region {
                stored_returns: signatures[&source.name].stored_returns.clone(),
                borrow_returns: signatures[&source.name].borrow_returns.clone(),
                name: source.name.clone(),
                parameters: vec![],
                exits: signatures[&source.name].outcomes.clone(),
                slots: BTreeMap::new(),
                atoms: BTreeMap::new(),
                code: vec![],
            },
        };
        checker.scopes[0]
            .paths
            .insert("Entry".into(), (Guard::True, TRUE));
        for param in &source.parameters {
            let borrowed = param.ty.starts_with('&');
            let (mutable, ty) = manifest::reference_inner(&param.ty).unwrap_or((false, &param.ty));
            let key = if borrowed {
                format!("$borrowed_{}", param.name)
            } else {
                param.name.clone()
            };
            let id = checker.define(&key, &param.ty, TRUE, source.span)?;
            if borrowed {
                checker.result.slots.get_mut(&id).unwrap().name = param.name.clone();
                let binding = checker.scopes[0].values.get_mut(&key).unwrap();
                binding.borrowed = true;
                binding.owned.clear();
                let loan_id = checker.fresh();
                checker.scopes[0].loans.insert(
                    param.name.clone(),
                    Loan {
                        contents: vec![],
                        sum_leaf: None,
                        id: loan_id,
                        target: ResolvedPlace {
                            carriers: vec![],
                            scope: 0,
                            name: key,
                            place: Place {
                                indirect: true,
                                root: id,
                                fields: vec![],
                            },
                            ty: ty.into(),
                            via: None,
                        },
                        origins: vec![LoanOrigin {
                            permissions: vec![],
                            guard: false,
                            place: Place {
                                root: id,
                                fields: vec![],
                                indirect: true,
                            },
                            opaque: false,
                        }],
                        parents: vec![],
                        mutable,
                        active: TRUE,
                        defined: TRUE,
                        holder: None,
                    },
                );
            }
            checker
                .result
                .parameters
                .push(checker.result.slots[&id].clone());
        }
        checker.initialize_contained_inputs(source.span)?;
        if source.abstract_signature {
            if checker.manifest.abstract_types.is_empty() {
                return Err(Diagnostic::new(
                    "E_INTERNAL",
                    source.span,
                    "abstract trait signature escaped definition checking",
                ));
            }
            regions.insert(source.name.clone(), checker.result);
            continue;
        }
        checker.items(&source.body, Guard::True, TRUE)?;
        let live = checker.bdd.not(checker.dead);
        if live != FALSE {
            return Err(checker.fail_domain(
                "E_UNCOVERED",
                source.span,
                "region has histories with no explicit exit",
                live,
            ));
        }
        if source.infer_exits && checker.result.exits.is_empty() {
            return Err(Diagnostic::new(
                "E_INFER",
                source.span,
                "exits auto requires at least one explicit exit; annotate a non-returning control's contract",
            ));
        }
        if !checker.pending_payloads.is_empty() {
            return Err(Diagnostic::new(
                "E_INFER",
                source.span,
                format!(
                    "cannot infer payloads for exits never produced by the checked body: {}; write their explicit payload types",
                    checker
                        .pending_payloads
                        .iter()
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            ));
        }
        inferred_bindings.extend(checker.inferred_bindings);
        let mut result = checker.result;
        crate::origins::normalize(&mut result, &origin_schemas, &manifest, source.span)?;
        if let Some(manifest::TypeDef::Variant { constructors, .. }) = manifest
            .types
            .iter_mut()
            .find(|t| t.name() == format!("outcome<{}>", source.name))
        {
            *constructors = result.exits.clone();
        }
        signatures.get_mut(&source.name).unwrap().outcomes = result.exits.clone();
        signatures.get_mut(&source.name).unwrap().borrow_returns = result.borrow_returns.clone();
        signatures.get_mut(&source.name).unwrap().stored_returns = result.stored_returns.clone();
        regions.insert(source.name.clone(), result);
    }
    fn visit(
        name: &str,
        regions: &BTreeMap<String, Region>,
        stack: &mut BTreeSet<String>,
        done: &mut BTreeSet<String>,
    ) -> Result<()> {
        if done.contains(name) {
            return Ok(());
        }
        if !stack.insert(name.into()) {
            return Err(Diagnostic::new(
                "E_RECURSION",
                Span::default(),
                format!("cyclic region call graph at `{name}`"),
            ));
        }
        fn callees(code: &[Instruction]) -> Vec<&str> {
            let mut result = vec![];
            for instruction in code {
                match &instruction.action {
                    Action::Decide {
                        operation: Operation::Region(name),
                        ..
                    } => result.push(name.as_str()),
                    Action::Loop { body, .. } => result.extend(callees(body)),
                    _ => {}
                }
            }
            result
        }
        for callee in callees(&regions[name].code) {
            visit(callee, regions, stack, done)?;
        }
        stack.remove(name);
        done.insert(name.into());
        Ok(())
    }
    let mut done = BTreeSet::new();
    for name in regions.keys() {
        visit(name, &regions, &mut BTreeSet::new(), &mut done)?;
    }
    let program = ir::CheckedProgram {
        inferred_bindings,
        manifest,
        regions,
        next_id: next,
        source_files: vec![],
        generic_controls: vec![],
    };
    program.verify()?;
    public_interfaces.check(&program)?;
    Ok(program)
}
