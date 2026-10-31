//! Public carrier paths retain a separate contract for every reference layer.
use super::*;

pub(super) type PublicLoan = (Vec<ReferenceStep>, ReferenceStep, Vec<LoanOrigin>, Vec<Id>);
pub(super) type PublicAddresses = Vec<(Vec<ReferenceStep>, Id)>;
impl Checker<'_> {
    pub(super) fn attach_received_contents(&mut self, root: Id) {
        if manifest::reference_inner(&self.result.slots[&root].ty).is_none() {
            return;
        }
        let contents = self
            .scopes
            .iter()
            .flat_map(|s| s.loans.values())
            .filter_map(|l| {
                l.holder
                    .as_ref()
                    .filter(|h| h.root == root && h.indirect)
                    .map(|h| (h.fields.clone(), l.clone()))
            })
            .collect();
        if let Some(parent) = self
            .scopes
            .iter_mut()
            .flat_map(|s| s.loans.values_mut())
            .find(|l| l.target.place.root == root)
        {
            parent.contents = contents;
        }
    }
    pub(super) fn refresh_public_contents(&mut self, addresses: &PublicAddresses) {
        for (path, address) in addresses.iter().rev() {
            let contents = addresses
                .iter()
                .filter(|(p, _)| p.len() == path.len() + 1 && p.starts_with(path))
                .map(|(p, a)| {
                    (
                        p.last().unwrap().fields.clone(),
                        self.scopes
                            .last()
                            .unwrap()
                            .loans
                            .values()
                            .find(|l| l.target.place.root == *a)
                            .unwrap()
                            .clone(),
                    )
                })
                .collect();
            self.scopes
                .last_mut()
                .unwrap()
                .loans
                .values_mut()
                .find(|l| l.target.place.root == *address)
                .unwrap()
                .contents = contents;
        }
    }
    pub(super) fn receive_public_contents(
        &mut self,
        root: Id,
        ty: &str,
        entries: &[PublicLoan],
        domain: Formula,
        span: Span,
    ) -> Result<(Vec<(Id, Expr)>, PublicAddresses)> {
        let mut entries = entries.to_vec();
        entries.sort_by_key(|(through, _, _, _)| through.len());
        let mut addresses: PublicAddresses = vec![];
        let mut bindings = vec![];
        for (through, step, origins, parents) in entries {
            let (holder, inner) = if through.is_empty() {
                (root, ty.to_owned())
            } else {
                let address = addresses
                    .iter()
                    .find(|(p, _)| *p == through)
                    .ok_or_else(|| self.error("E_ORIGIN", span, "missing public reference layer"))?
                    .1;
                (
                    address,
                    manifest::reference_inner(&self.result.slots[&address].ty)
                        .unwrap()
                        .1
                        .to_owned(),
                )
            };
            let address = if let Some(tag) = &step.sum_leaf {
                self.receive_tagged(
                    holder,
                    &inner,
                    &step.fields,
                    tag,
                    origins,
                    parents,
                    domain,
                    span,
                )?
            } else {
                let (address, value) = self.receive_stored(
                    "$public",
                    holder,
                    &inner,
                    &step.fields,
                    origins,
                    parents,
                    domain,
                    span,
                )?;
                if through.iter().all(|s| s.sum_leaf.is_none()) {
                    bindings.push((address, value));
                }
                address
            };
            let mut path = through;
            path.push(step);
            addresses.push((path, address));
        }
        self.refresh_public_contents(&addresses);
        self.attach_received_contents(root);
        Ok((bindings, addresses))
    }
}

impl Checker<'_> {
    pub(super) fn call_origin_dependencies(
        &mut self,
        source: &BorrowSource,
        arg: &syntax::Expr,
        param: &manifest::Parameter,
        domain: Formula,
        span: Span,
    ) -> Result<Vec<(Vec<LoanOrigin>, Vec<Id>)>> {
        if let syntax::ExprKind::Move(value) = &arg.kind {
            return self.call_origin_dependencies(source, value, param, domain, span);
        }
        let Some(contained) = source.contained.as_ref().filter(|c| !c.through.is_empty()) else {
            return Ok(self
                .call_origin_targets(source, arg, param, domain, span)?
                .iter()
                .map(|target| (self.origins(target), self.parents(target)))
                .collect());
        };
        let arg = match &arg.kind {
            syntax::ExprKind::Call(n, args) if n == "clone" => match args.as_slice() {
                [
                    syntax::Expr {
                        kind: syntax::ExprKind::Borrow(v),
                        ..
                    },
                ] => v.as_ref(),
                _ => arg,
            },
            _ => arg,
        };
        let holder = if let Some((mutable, _)) = manifest::reference_inner(&param.ty) {
            self.reference_argument(arg, mutable, domain, span)?
        } else {
            self.resolve_place(arg, span)?.ok_or_else(|| {
                self.error(
                    "E_UNSUPPORTED",
                    span,
                    "bind a nested borrowed container locally before selecting its origins",
                )
            })?
        };
        let mut contents = self
            .snapshot_carrier_contents(&holder, domain)
            .into_iter()
            .map(|(f, l)| (f, l, self.parents(&holder)))
            .collect::<Vec<_>>();
        for step in &contained.through {
            contents = contents
                .into_iter()
                .filter(|(f, l, _)| *f == step.fields && l.sum_leaf == step.sum_leaf)
                .flat_map(|(_, l, mut parents)| {
                    parents.extend(l.parents.iter().copied());
                    if l.mutable {
                        parents.push(l.id);
                    }
                    l.contents
                        .into_iter()
                        .map(move |(f, child)| (f, child, parents.clone()))
                })
                .collect();
        }
        Ok(contents
            .into_iter()
            .filter(|(f, l, _)| *f == contained.fields && l.sum_leaf == contained.sum_leaf)
            .map(|(_, loan, mut parents)| {
                parents.extend(loan.parents);
                if loan.mutable {
                    parents.push(loan.id);
                }
                parents.sort_unstable();
                parents.dedup();
                (loan.origins, parents)
            })
            .collect())
    }
}
