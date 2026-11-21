//! From the syntax tree to the typed core.
//!
//! This is the part of checking that has to be done before a program can run
//! at all: names and types are resolved, expressions are typed ([C1] to [C4]),
//! type usage kinds and fault-releasability are computed ([T1] to [T6]), and
//! the rules of well-formedness that are about the shape of the text are
//! enforced ([W1] to [W8]). Every error names the clause it comes from.

use crate::int::Int;
use crate::syntax;
use crate::typed::{
    Alternative, Arithmetic, Arm, Body, BodyId, BorrowMode, BoundaryDef, BoundaryId, BuiltinOp,
    Command, Comparison, Control, ControlBody, ControlId, Expr, Function, FunctionId, IntType,
    Kind, LoopHead, Operation, OutcomeDef, OutcomeId, Parameter, Place, PlaceStep, Program,
    ResourceDef, ResourceId, Selector, Site, SiteId, Statement, StructDef, StructId, Type, Var,
    VarId, VarRef, VariantConstructor, VariantDef, VariantId,
};
use std::collections::{HashMap, HashSet};
use std::fmt;

#[derive(Clone, Debug)]
pub struct Error {
    pub line: u32,
    pub clause: &'static str,
    pub message: String,
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "line {}: {} [{}]",
            self.line, self.message, self.clause
        )
    }
}

fn fail<T>(line: u32, clause: &'static str, message: impl Into<String>) -> Result<T, Error> {
    Err(Error {
        line,
        clause,
        message: message.into(),
    })
}

/// An integer literal that nothing else types is a `u32` (surface, section
/// 13, choice 10).
const DEFAULT_INTEGER: Type = Type::Int(IntType::U32);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// Arithmetic belongs to a type, and a literal must be a value of it.
    Expression,
    /// Arithmetic is that of the mathematical integers ([S4], [E-Formula]).
    Formula,
    /// A contract outcome condition `⇐ φ`, where `old(y)` is allowed ([W7]).
    ContractCondition,
}

trait Scope {
    fn find(&self, name: &str) -> Option<(VarRef, Type)>;
    fn find_parameter(&self, _name: &str) -> Option<(usize, Type)> {
        None
    }
}

struct SignatureScope<'a> {
    parameters: &'a [Parameter],
    fields: &'a [Parameter],
}

impl Scope for SignatureScope<'_> {
    fn find(&self, name: &str) -> Option<(VarRef, Type)> {
        if let Some(index) = self.fields.iter().position(|field| field.name == name) {
            return Some((VarRef::Field(index), self.fields[index].ty.clone()));
        }
        let index = self
            .parameters
            .iter()
            .position(|parameter| parameter.name == name)?;
        Some((VarRef::Parameter(index), self.parameters[index].ty.clone()))
    }

    fn find_parameter(&self, name: &str) -> Option<(usize, Type)> {
        let index = self
            .parameters
            .iter()
            .position(|parameter| parameter.name == name)?;
        Some((index, self.parameters[index].ty.clone()))
    }
}

struct Globals<'a> {
    struct_ids: HashMap<&'a str, StructId>,
    variant_ids: HashMap<&'a str, VariantId>,
    resource_ids: HashMap<&'a str, ResourceId>,
    outcome_ids: HashMap<String, OutcomeId>,
    boundary_ids: HashMap<&'a str, BoundaryId>,
    function_ids: HashMap<&'a str, FunctionId>,
    function_types: Vec<(Vec<Type>, Type)>,
    control_ids: HashMap<&'a str, ControlId>,
    structs: Vec<StructDef>,
    variants: Vec<VariantDef>,
    resources: Vec<ResourceDef>,
    outcomes: Vec<OutcomeDef>,
}

impl Globals<'_> {
    /// Resolves a syntactic `Type` into a canonical `typed::Type`.
    /// `allow_ref` is true at top-level parameter, local, loop-parameter and
    /// payload-field positions ([W6]).
    fn resolve_type(
        &self,
        ty: &Type,
        allow_ref: bool,
        in_outcome: bool,
        line: u32,
    ) -> Result<Type, Error> {
        match ty {
            Type::Bool => Ok(Type::Bool),
            Type::Int(i) => Ok(Type::Int(*i)),
            Type::String => Ok(Type::String),
            Type::Ref(mode, inner) => {
                if in_outcome {
                    return fail(
                        line,
                        "T6",
                        "`outcome⟨K⟩` is not allowed when a payload contains a borrow",
                    );
                }
                if !allow_ref {
                    return fail(
                        line,
                        "W6",
                        "borrow types `&τ` and `&~τ` may not occur inside another type",
                    );
                }
                if matches!(**inner, Type::Ref(_, _)) {
                    return fail(line, "W6", "a borrow may not refer to a borrow");
                }
                let resolved_inner = self.resolve_type(inner, false, false, line)?;
                Ok(Type::Ref(*mode, Box::new(resolved_inner)))
            }
            Type::Vector(elem) => {
                let resolved_elem = self.resolve_type(elem, false, in_outcome, line)?;
                Ok(Type::Vector(Box::new(resolved_elem)))
            }
            Type::Map(k, v) => {
                let rk = self.resolve_type(k, false, in_outcome, line)?;
                let rv = self.resolve_type(v, false, in_outcome, line)?;
                Ok(Type::Map(Box::new(rk), Box::new(rv)))
            }
            Type::OutcomeNamed(name) => {
                let Some(&id) = self.outcome_ids.get(name.as_str()) else {
                    return fail(
                        line,
                        "C1",
                        format!("`{name}` is not a declared outcome or control"),
                    );
                };
                Ok(Type::Outcome(id, self.outcomes[id].name.clone()))
            }
            Type::Named(name) => {
                if let Some(t) = Type::from_name(name) {
                    return Ok(t);
                }
                if let Some(&id) = self.struct_ids.get(name.as_str()) {
                    return Ok(Type::Struct(id, name.clone()));
                }
                if let Some(&id) = self.variant_ids.get(name.as_str()) {
                    return Ok(Type::Variant(id, name.clone()));
                }
                if let Some(&id) = self.resource_ids.get(name.as_str()) {
                    return Ok(Type::Resource(id, name.clone()));
                }
                if let Some(&id) = self.outcome_ids.get(name.as_str()) {
                    return Ok(Type::Outcome(id, self.outcomes[id].name.clone()));
                }
                fail(line, "C1", format!("`{name}` is not a declared type"))
            }
            Type::Struct(id, name) => Ok(Type::Struct(*id, name.clone())),
            Type::Variant(id, name) => Ok(Type::Variant(*id, name.clone())),
            Type::Resource(id, name) => Ok(Type::Resource(*id, name.clone())),
            Type::Outcome(id, name) => Ok(Type::Outcome(*id, name.clone())),
        }
    }

    fn is_clonable(&self, ty: &Type) -> bool {
        match ty.deref_type() {
            Type::Bool | Type::Int(_) | Type::String => true,
            Type::Struct(id, _) => self.structs[*id].clonable,
            Type::Variant(id, _) => self.variants[*id].clonable,
            Type::Resource(_, _) | Type::Outcome(_, _) | Type::OutcomeNamed(_) => false,
            Type::Vector(elem) => self.is_clonable(elem),
            Type::Map(k, v) => self.is_clonable(k) && self.is_clonable(v),
            Type::Ref(_, _) | Type::Named(_) => false,
        }
    }

    fn is_comparable(&self, ty: &Type) -> bool {
        match ty.deref_type() {
            Type::Bool | Type::Int(_) | Type::String => true,
            Type::Struct(id, _) => self.structs[*id].comparable,
            Type::Variant(id, _) => self.variants[*id].comparable,
            _ => false,
        }
    }

    fn kind(&self, ty: &Type) -> Kind {
        match ty {
            Type::Bool | Type::Int(_) | Type::Ref(BorrowMode::Shared, _) => Kind::Copy,
            Type::String | Type::Ref(BorrowMode::Exclusive, _) => Kind::Drop,
            Type::Struct(id, _) => self.structs[*id].kind,
            Type::Variant(id, _) => self.variants[*id].kind,
            Type::Resource(_, _) | Type::Outcome(_, _) | Type::OutcomeNamed(_) => Kind::Must,
            Type::Vector(elem) => Kind::Drop.max(self.kind(elem)),
            Type::Map(k, v) => Kind::Drop.max(self.kind(k)).max(self.kind(v)),
            Type::Named(_) => Kind::Drop,
        }
    }

    fn is_fault_releasable(&self, ty: &Type) -> bool {
        match ty {
            Type::Bool | Type::Int(_) | Type::String | Type::Ref(_, _) => true,
            Type::Struct(id, _) => self.structs[*id].fault_releasable,
            Type::Variant(id, _) => self.variants[*id].fault_releasable,
            Type::Resource(id, _) => self.resources[*id].on_fault_release,
            Type::Outcome(id, _) => self.outcomes[*id].fault_releasable,
            Type::Vector(elem) => self.is_fault_releasable(elem),
            Type::Map(k, v) => self.is_fault_releasable(k) && self.is_fault_releasable(v),
            Type::Named(_) | Type::OutcomeNamed(_) => false,
        }
    }

    fn holds_obligation(&self, ty: &Type) -> bool {
        match ty {
            Type::Bool | Type::Int(_) | Type::String | Type::Ref(_, _) => false,
            Type::Struct(id, _) => self.structs[*id].holds_obligation,
            Type::Variant(id, _) => self.variants[*id].holds_obligation,
            Type::Resource(_, _) | Type::Outcome(_, _) | Type::OutcomeNamed(_) => true,
            Type::Vector(elem) => self.holds_obligation(elem),
            Type::Map(k, v) => self.holds_obligation(k) || self.holds_obligation(v),
            Type::Named(_) => false,
        }
    }
}

struct Typer<'a> {
    globals: &'a Globals<'a>,
    scope: &'a dyn Scope,
    mode: Mode,
    line: u32,
}

impl Typer<'_> {
    fn error<T>(&self, message: impl Into<String>) -> Result<T, Error> {
        fail(self.line, "C1", message)
    }

    fn function(&self, name: &str) -> Result<FunctionId, Error> {
        match self.globals.function_ids.get(name) {
            Some(id) => Ok(*id),
            None => self.error(format!("`{name}` is not a function")),
        }
    }

    /// The type an expression has by itself, or `None` for one made of
    /// integer literals, `[]` or `∅`, which takes the type its place expects.
    fn kind(&self, expr: &syntax::Expr) -> Result<Option<Type>, Error> {
        Ok(match expr {
            syntax::Expr::Integer(_) | syntax::Expr::EmptyMap => None,
            syntax::Expr::Boolean(_) => Some(Type::Bool),
            syntax::Expr::String(_) => Some(Type::String),
            syntax::Expr::Name(name) => match self.scope.find(name) {
                Some((_, ty)) => Some(ty.deref_type().clone()),
                None => return self.error(format!("`{name}` is not a name here")),
            },
            syntax::Expr::Old(name) => {
                if self.mode != Mode::ContractCondition {
                    return fail(
                        self.line,
                        "W7",
                        "`old(…)` may only appear in an outcome condition `⇐ φ`",
                    );
                }
                let Some((_, ty)) = self.scope.find_parameter(name) else {
                    return fail(
                        self.line,
                        "W7",
                        format!("`{name}` is not a parameter of this control"),
                    );
                };
                let Type::Ref(BorrowMode::Exclusive, inner) = ty else {
                    return fail(
                        self.line,
                        "W7",
                        format!("`old({name})` requires `{name}` to be an `&~` parameter"),
                    );
                };
                Some(*inner)
            }
            syntax::Expr::Move(inner) => self.kind(inner)?,
            syntax::Expr::Borrow(mode, inner) => {
                let inner_ty = self.kind(inner)?.ok_or_else(|| Error {
                    line: self.line,
                    clause: "C1",
                    message: "borrowed place has no known type".to_string(),
                })?;
                Some(Type::Ref(*mode, Box::new(inner_ty)))
            }
            syntax::Expr::Field(base, field) => {
                if let syntax::Expr::Name(vname) = &**base
                    && self.scope.find(vname).is_none()
                    && let Some(&vid) = self.globals.variant_ids.get(vname.as_str())
                {
                    return Ok(Some(Type::Variant(vid, vname.clone())));
                }
                let Some(base_ty) = self.kind(base)? else {
                    return Ok(None);
                };
                let Type::Struct(sid, sname) = base_ty.deref_type() else {
                    return self.error(format!("cannot read field `{field}` on `{base_ty}`"));
                };
                let Some(f) = self.globals.structs[*sid]
                    .fields
                    .iter()
                    .find(|f| f.name == *field)
                else {
                    return self.error(format!("struct `{sname}` has no field `{field}`"));
                };
                Some(f.ty.clone())
            }
            syntax::Expr::Index(base, _) => {
                let Some(base_ty) = self.kind(base)? else {
                    return Ok(None);
                };
                let Type::Vector(elem_ty) = base_ty.deref_type() else {
                    return self.error(format!("cannot index value of type `{base_ty}`"));
                };
                Some((**elem_ty).clone())
            }
            syntax::Expr::Struct(name, _) => {
                let Some(&id) = self.globals.struct_ids.get(name.as_str()) else {
                    return self.error(format!("`{name}` is not a struct type"));
                };
                Some(Type::Struct(id, name.clone()))
            }
            syntax::Expr::Variant(vname, _, _) => {
                let Some(&id) = self.globals.variant_ids.get(vname.as_str()) else {
                    return self.error(format!("`{vname}` is not a variant type"));
                };
                Some(Type::Variant(id, vname.clone()))
            }
            syntax::Expr::Vector(items) => {
                let mut elem_ty: Option<Type> = None;
                for item in items {
                    if let Some(t) = self.kind(item)? {
                        if let Some(prev) = &elem_ty
                            && *prev != t
                        {
                            return self.error(format!(
                                "vector elements have conflicting types {prev} and {t}"
                            ));
                        }
                        elem_ty = Some(t);
                    }
                }
                elem_ty.map(|t| Type::Vector(Box::new(t)))
            }
            syntax::Expr::Len(_) => Some(Type::Int(IntType::Usize)),
            syntax::Expr::Clone(place) => self.kind(place)?.map(|t| t.deref_type().clone()),
            syntax::Expr::Unary(syntax::Unary::Not, _) => Some(Type::Bool),
            syntax::Expr::Unary(syntax::Unary::Negate, operand) => self.kind(operand)?,
            syntax::Expr::Binary(operator, left, right) => {
                if operator.level() <= 3 {
                    return Ok(Some(Type::Bool));
                }
                match (self.kind(left)?, self.kind(right)?) {
                    (Some(a), Some(b)) if a != b => {
                        return self.error(format!(
                            "the operands of `{}` have the types {a} and {b}",
                            operator.glyph()
                        ));
                    }
                    (Some(a), _) | (_, Some(a)) => Some(a),
                    (None, None) => None,
                }
            }
            syntax::Expr::Call(name, _) => {
                if !self.globals.function_ids.contains_key(name.as_str()) && name == "trim" {
                    return Ok(Some(Type::String));
                }
                Some(self.globals.function_types[self.function(name)?].1.clone())
            }
        })
    }

    fn infer(&self, expr: &syntax::Expr) -> Result<Expr, Error> {
        let ty = match self.kind(expr)? {
            Some(ty) => ty,
            None => match expr {
                syntax::Expr::EmptyMap => {
                    return self.error("`∅` needs a map type annotation");
                }
                syntax::Expr::Vector(items) if items.is_empty() => {
                    return self.error("`[]` needs a vector type annotation");
                }
                syntax::Expr::Vector(_) => Type::Vector(Box::new(DEFAULT_INTEGER)),
                _ => DEFAULT_INTEGER,
            },
        };
        self.build(expr, &ty)
    }

    fn literal(&self, value: Int, expected: &Type) -> Result<Expr, Error> {
        let Type::Int(ty) = expected else {
            return self.error(format!(
                "the integer {value} stands where {expected} is expected"
            ));
        };
        if self.mode == Mode::Expression {
            let (least, greatest) = ty.literal_bounds();
            if value < least || value > greatest {
                return self.error(format!(
                    "{value} is not a value of {} on every target",
                    ty.name()
                ));
            }
        }
        Ok(Expr::Integer(value, *ty))
    }

    fn integer_type(&self, expected: &Type, what: &str) -> Result<IntType, Error> {
        match expected {
            Type::Int(ty) => Ok(*ty),
            other => self.error(format!("{what} stands where {other} is expected")),
        }
    }

    fn resolve_place(&self, expr: &syntax::Expr) -> Result<(Place, bool), Error> {
        match expr {
            syntax::Expr::Name(name) => {
                let Some((VarRef::Local(var), ty)) = self.scope.find(name) else {
                    return self.error(format!("`{name}` is not a place here"));
                };
                let through_exc = matches!(ty, Type::Ref(BorrowMode::Exclusive, _));
                let val_ty = ty.deref_type().clone();
                Ok((
                    Place {
                        root: var,
                        root_ty: ty,
                        steps: Vec::new(),
                        ty: val_ty,
                    },
                    through_exc,
                ))
            }
            syntax::Expr::Field(base, field_name) => {
                let (mut place, through_exc) = self.resolve_place(base)?;
                let Type::Struct(sid, sname) = &place.ty else {
                    return self.error(format!(
                        "cannot access field `{field_name}` on `{}`",
                        place.ty
                    ));
                };
                let sid = *sid;
                let Some((fidx, field)) = self.globals.structs[sid]
                    .fields
                    .iter()
                    .enumerate()
                    .find(|(_, f)| f.name == *field_name)
                else {
                    return self.error(format!("struct `{sname}` has no field `{field_name}`"));
                };
                let fty = field.ty.clone();
                place.steps.push(PlaceStep::Field(sid, fidx, fty.clone()));
                place.ty = fty;
                Ok((place, through_exc))
            }
            syntax::Expr::Index(base, idx_expr) => {
                let (mut place, through_exc) = self.resolve_place(base)?;
                let Type::Vector(elem_ty) = &place.ty else {
                    return self.error(format!("cannot index value of type `{}`", place.ty));
                };
                let elem_ty = (**elem_ty).clone();
                let idx = self.build(idx_expr, &Type::Int(IntType::Usize))?;
                place.steps.push(PlaceStep::Index(idx, elem_ty.clone()));
                place.ty = elem_ty;
                Ok((place, through_exc))
            }
            _ => self.error("expected a place (`x`, `p.f` or `p[i]`)"),
        }
    }

    fn build(&self, expr: &syntax::Expr, expected: &Type) -> Result<Expr, Error> {
        // When the expected type is `&τ` or `&~τ`, handle borrow operands (`[W8]`, `[U11]`).
        if let Type::Ref(exp_mode, exp_inner) = expected {
            match expr {
                syntax::Expr::Borrow(mode, inner) => {
                    if mode != exp_mode {
                        return self.error(format!(
                            "expected {expected}, found {} borrow",
                            match mode {
                                BorrowMode::Shared => "shared",
                                BorrowMode::Exclusive => "exclusive",
                            }
                        ));
                    }
                    let (place, _) = self.resolve_place(inner)?;
                    if place.ty != **exp_inner {
                        return self.error(format!(
                            "borrowed place has type {}, and {} is expected",
                            place.ty, exp_inner
                        ));
                    }
                    return Ok(Expr::Borrow(*mode, place, expected.clone()));
                }
                syntax::Expr::Name(name) => {
                    if let Some((var, var_ty)) = self.scope.find(name) {
                        if var_ty == *expected {
                            return Ok(Expr::Var(var, var_ty));
                        }
                        if *exp_mode == BorrowMode::Shared
                            && (var_ty == Type::Ref(BorrowMode::Exclusive, exp_inner.clone())
                                || var_ty == **exp_inner)
                        {
                            return Ok(Expr::Var(var, (**exp_inner).clone()));
                        }
                    }
                }
                _ if *exp_mode == BorrowMode::Shared => {
                    // For `&τ` parameters in surface calls or functions ([W5a], [U11]),
                    // allow reading an expression of type `τ` in place.
                    return self.build(expr, exp_inner);
                }
                _ => {}
            }
            return self.error(format!("expected {expected}"));
        }

        match expr {
            syntax::Expr::Integer(value) => self.literal(value.clone(), expected),
            syntax::Expr::Boolean(value) => {
                if *expected != Type::Bool {
                    return self.error(format!("a bool stands where {expected} is expected"));
                }
                Ok(Expr::Boolean(*value))
            }
            syntax::Expr::String(value) => {
                if *expected != Type::String {
                    return self.error(format!("a string stands where {expected} is expected"));
                }
                Ok(Expr::String(value.clone()))
            }
            syntax::Expr::Name(name) => {
                let Some((var, ty)) = self.scope.find(name) else {
                    return self.error(format!("`{name}` is not a name here"));
                };
                // [W8]: A name of borrow type stands for the value it refers to
                // everywhere a value is expected.
                let val_ty = ty.deref_type();
                if val_ty != expected {
                    return self.error(format!(
                        "`{name}` has the type {val_ty}, and {expected} is expected"
                    ));
                }
                Ok(Expr::Var(var, val_ty.clone()))
            }
            syntax::Expr::Old(name) => {
                if self.mode != Mode::ContractCondition {
                    return fail(
                        self.line,
                        "W7",
                        "`old(…)` may only appear in an outcome condition `⇐ φ`",
                    );
                }
                let Some((idx, ty)) = self.scope.find_parameter(name) else {
                    return fail(
                        self.line,
                        "W7",
                        format!("`{name}` is not a parameter of this control"),
                    );
                };
                let Type::Ref(BorrowMode::Exclusive, inner) = ty else {
                    return fail(
                        self.line,
                        "W7",
                        format!("`old({name})` requires `{name}` to be an `&~` parameter"),
                    );
                };
                if *inner != *expected {
                    return self.error(format!(
                        "`old({name})` has the type {inner}, and {expected} is expected"
                    ));
                }
                Ok(Expr::Old(idx, *inner))
            }
            syntax::Expr::Move(inner) => {
                let built = self.build(inner, expected)?;
                Ok(Expr::Move(Box::new(built), expected.clone()))
            }
            syntax::Expr::Borrow(_, _) => {
                self.error(format!("a borrow stands where {expected} is expected"))
            }
            syntax::Expr::Field(base, field) => {
                if let syntax::Expr::Name(vname) = &**base
                    && self.scope.find(vname).is_none()
                    && self.globals.variant_ids.contains_key(vname.as_str())
                {
                    return self.build(
                        &syntax::Expr::Variant(vname.clone(), field.clone(), Vec::new()),
                        expected,
                    );
                }
                let base_expr = self.infer(base)?;
                let Type::Struct(sid, sname) = base_expr.ty().deref_type().clone() else {
                    return self.error(format!(
                        "cannot access field `{field}` on `{}`",
                        base_expr.ty()
                    ));
                };
                let Some((fidx, fdef)) = self.globals.structs[sid]
                    .fields
                    .iter()
                    .enumerate()
                    .find(|(_, f)| f.name == *field)
                else {
                    return self.error(format!("struct `{sname}` has no field `{field}`"));
                };
                if fdef.ty != *expected {
                    return self.error(format!(
                        "`{sname}.{field}` has type {}, and {expected} is expected",
                        fdef.ty
                    ));
                }
                Ok(Expr::Field(
                    Box::new(base_expr),
                    sid,
                    fidx,
                    expected.clone(),
                ))
            }
            syntax::Expr::Index(base, idx) => {
                let base_expr = self.infer(base)?;
                let Type::Vector(elem_ty) = base_expr.ty().deref_type().clone() else {
                    return self.error(format!("cannot index value of type `{}`", base_expr.ty()));
                };
                if *elem_ty != *expected {
                    return self.error(format!(
                        "element has type {elem_ty}, and {expected} is expected"
                    ));
                }
                let idx_expr = self.build(idx, &Type::Int(IntType::Usize))?;
                Ok(Expr::Index(
                    Box::new(base_expr),
                    Box::new(idx_expr),
                    expected.clone(),
                ))
            }
            syntax::Expr::Struct(name, fields) => {
                let Some(&sid) = self.globals.struct_ids.get(name.as_str()) else {
                    return self.error(format!("`{name}` is not a struct type"));
                };
                let st = &self.globals.structs[sid];
                let st_ty = Type::Struct(sid, name.clone());
                if st_ty != *expected {
                    return self.error(format!(
                        "`{name}` has type {st_ty}, and {expected} is expected"
                    ));
                }
                if fields.len() != st.fields.len() {
                    return self.error(format!(
                        "struct `{name}` has {} fields, and {} are given",
                        st.fields.len(),
                        fields.len()
                    ));
                }
                let mut built = Vec::with_capacity(fields.len());
                for (expr, fdef) in fields.iter().zip(&st.fields) {
                    built.push(self.build(expr, &fdef.ty)?);
                }
                Ok(Expr::Struct(sid, built, st_ty))
            }
            syntax::Expr::Variant(vname, cname, fields) => {
                let Some(&vid) = self.globals.variant_ids.get(vname.as_str()) else {
                    return self.error(format!("`{vname}` is not a variant type"));
                };
                let vdef = &self.globals.variants[vid];
                let v_ty = Type::Variant(vid, vname.clone());
                if v_ty != *expected {
                    return self.error(format!(
                        "`{vname}.{cname}` has type {v_ty}, and {expected} is expected"
                    ));
                }
                let Some((cidx, cdef)) = vdef
                    .constructors
                    .iter()
                    .enumerate()
                    .find(|(_, c)| c.name == *cname)
                else {
                    return self.error(format!("variant `{vname}` has no constructor `{cname}`"));
                };
                if fields.len() != cdef.fields.len() {
                    return self.error(format!(
                        "constructor `{vname}.{cname}` has {} fields, and {} are given",
                        cdef.fields.len(),
                        fields.len()
                    ));
                }
                let mut built = Vec::with_capacity(fields.len());
                for (expr, fdef) in fields.iter().zip(&cdef.fields) {
                    built.push(self.build(expr, &fdef.ty)?);
                }
                Ok(Expr::Variant(vid, cidx, built, v_ty))
            }
            syntax::Expr::Vector(items) => {
                let Type::Vector(elem_ty) = expected else {
                    return self.error(format!(
                        "a vector literal stands where {expected} is expected"
                    ));
                };
                let mut built = Vec::with_capacity(items.len());
                for item in items {
                    built.push(self.build(item, elem_ty)?);
                }
                Ok(Expr::Vector(built, expected.clone()))
            }
            syntax::Expr::EmptyMap => {
                if !matches!(expected, Type::Map(_, _)) {
                    return self.error(format!("`∅` stands where {expected} is expected"));
                }
                Ok(Expr::EmptyMap(expected.clone()))
            }
            syntax::Expr::Len(place) => {
                if *expected != Type::Int(IntType::Usize) {
                    return self
                        .error(format!("`len(…)` yields usize, and {expected} is expected"));
                }
                let built = self.infer(place)?;
                match built.ty().deref_type() {
                    Type::Vector(_) | Type::String | Type::Map(_, _) => {}
                    other => {
                        return self.error(format!(
                            "`len(…)` applies to vectors, strings and maps, not {other}"
                        ));
                    }
                }
                Ok(Expr::Len(Box::new(built)))
            }
            syntax::Expr::Clone(place) => {
                let built = self.build(place, expected)?;
                if !self.globals.is_clonable(expected) {
                    return self.error(format!("type `{expected}` cannot be cloned"));
                }
                Ok(Expr::Clone(Box::new(built), expected.clone()))
            }
            syntax::Expr::Unary(syntax::Unary::Not, operand) => {
                if *expected != Type::Bool {
                    return self.error(format!("`¬` stands where {expected} is expected"));
                }
                Ok(Expr::Not(Box::new(self.build(operand, &Type::Bool)?)))
            }
            syntax::Expr::Unary(syntax::Unary::Negate, operand) => {
                let ty = self.integer_type(expected, "a negation")?;
                if let syntax::Expr::Integer(value) = &**operand {
                    return self.literal(value.negate(), expected);
                }
                Ok(Expr::Negate(Box::new(self.build(operand, expected)?), ty))
            }
            syntax::Expr::Binary(operator, left, right) => {
                self.binary(*operator, left, right, expected)
            }
            syntax::Expr::Call(name, arguments) => {
                if !self.globals.function_ids.contains_key(name.as_str()) && name == "trim" {
                    if *expected != Type::String {
                        return self
                            .error(format!("`trim` yields string, and {expected} is expected"));
                    }
                    if arguments.len() != 1 {
                        return self.error(format!(
                            "`trim` takes 1 argument, and {} are given",
                            arguments.len()
                        ));
                    }
                    let arg = self.build(&arguments[0], &Type::String)?;
                    return Ok(Expr::Trim(Box::new(arg)));
                }
                let id = self.function(name)?;
                let (parameters, result) = &self.globals.function_types[id];
                if result != expected {
                    return self.error(format!(
                        "`{name}` yields {result}, and {expected} is expected"
                    ));
                }
                if parameters.len() != arguments.len() {
                    return self.error(format!(
                        "`{name}` takes {} arguments, and {} are given",
                        parameters.len(),
                        arguments.len()
                    ));
                }
                let mut built = Vec::new();
                for (argument, ty) in arguments.iter().zip(parameters) {
                    built.push(self.build(argument, ty)?);
                }
                Ok(Expr::Call(id, built, result.clone()))
            }
        }
    }

    fn binary(
        &self,
        operator: syntax::Binary,
        left: &syntax::Expr,
        right: &syntax::Expr,
        expected: &Type,
    ) -> Result<Expr, Error> {
        use syntax::Binary as B;
        let arithmetic = match operator {
            B::Add => Some(Arithmetic::Add),
            B::Subtract => Some(Arithmetic::Subtract),
            B::Multiply => Some(Arithmetic::Multiply),
            B::Divide => Some(Arithmetic::Divide),
            B::Remainder => Some(Arithmetic::Remainder),
            _ => None,
        };
        if let Some(arithmetic) = arithmetic {
            let ty = self.integer_type(expected, "arithmetic")?;
            return Ok(Expr::Arithmetic(
                arithmetic,
                Box::new(self.build(left, expected)?),
                Box::new(self.build(right, expected)?),
                ty,
            ));
        }
        if *expected != Type::Bool {
            return self.error(format!(
                "`{}` yields a bool, and {expected} is expected",
                operator.glyph()
            ));
        }
        match operator {
            B::And | B::Or => {
                let left = Box::new(self.build(left, &Type::Bool)?);
                let right = Box::new(self.build(right, &Type::Bool)?);
                Ok(if operator == B::And {
                    Expr::And(left, right)
                } else {
                    Expr::Or(left, right)
                })
            }
            _ => {
                let operand = match (self.kind(left)?, self.kind(right)?) {
                    (Some(a), Some(b)) if a != b => {
                        return self.error(format!(
                            "the operands of `{}` have the types {a} and {b}",
                            operator.glyph()
                        ));
                    }
                    (Some(a), _) | (_, Some(a)) => a,
                    (None, None) => DEFAULT_INTEGER,
                };
                let comparison = match operator {
                    B::Equal => Comparison::Equal,
                    B::NotEqual => Comparison::NotEqual,
                    B::Less => Comparison::Less,
                    B::LessEqual => Comparison::LessEqual,
                    B::Greater => Comparison::Greater,
                    _ => Comparison::GreaterEqual,
                };
                let ordered = !matches!(comparison, Comparison::Equal | Comparison::NotEqual);
                if ordered {
                    if !matches!(operand, Type::Int(_)) {
                        return self
                            .error(format!("`{}` does not order {operand}s", operator.glyph()));
                    }
                } else if !self.globals.is_comparable(&operand) {
                    return self.error(format!(
                        "`{}` cannot compare values of type {operand}",
                        operator.glyph()
                    ));
                }
                Ok(Expr::Compare(
                    comparison,
                    Box::new(self.build(left, &operand)?),
                    Box::new(self.build(right, &operand)?),
                ))
            }
        }
    }
}

struct PendingLeave {
    statement: usize,
    alternative: usize,
    kinds: Vec<Option<Type>>,
    payload: Vec<syntax::Expr>,
    line: u32,
}

struct LoopFrame {
    name: String,
    parameters: Vec<VarId>,
    alternatives: Vec<String>,
    arity: Vec<usize>,
    leaves: Vec<PendingLeave>,
}

struct Frame {
    body: BodyId,
    names: HashMap<String, VarId>,
    /// Names bound in bodies nested in this one ([W4]).
    nested: HashSet<String>,
    /// The sites a selector of this body may mention so far ([W2]).
    sites: HashMap<String, SiteId>,
    looping: Option<LoopFrame>,
}

struct Builder<'g, 'a> {
    globals: &'g mut Globals<'a>,
    controls: &'g [Control],
    boundaries: &'g [(String, Vec<Parameter>, bool, Vec<String>)],
    control: Option<ControlId>,
    in_boundary: bool,
    in_contingency: bool,
    vars: Vec<Var>,
    sites: Vec<Site>,
    bodies: Vec<Body>,
    site_names: HashSet<String>,
    frames: Vec<Frame>,
}

impl Scope for Builder<'_, '_> {
    fn find(&self, name: &str) -> Option<(VarRef, Type)> {
        let id = self
            .frames
            .iter()
            .rev()
            .find_map(|frame| frame.names.get(name))?;
        Some((VarRef::Local(*id), self.vars[*id].ty.clone()))
    }
}

impl<'g, 'a> Builder<'g, 'a> {
    fn typer(&self, mode: Mode, line: u32) -> Typer<'_> {
        Typer {
            globals: self.globals,
            scope: self,
            mode,
            line,
        }
    }

    fn frame(&mut self) -> &mut Frame {
        self.frames.last_mut().expect("a frame")
    }

    fn declare(
        &mut self,
        raw_name: &str,
        ty: Type,
        mutable: bool,
        line: u32,
    ) -> Result<VarId, Error> {
        let (has_prefix, name) = match raw_name.strip_prefix('~') {
            Some(stripped) => (true, stripped),
            None => (false, raw_name),
        };
        let mutable = mutable || has_prefix;
        let current = self.frames.len() - 1;
        if let Some(id) = self.frames[current].names.get(name).copied() {
            let var = &self.vars[id];
            if var.ty != ty || var.mutable != mutable {
                return fail(
                    line,
                    "W4",
                    format!("the binders of `{name}` in one body differ in type or mutability"),
                );
            }
            return Ok(id);
        }
        let outer = self.frames[..current]
            .iter()
            .any(|frame| frame.names.contains_key(name));
        if outer || self.frames[current].nested.contains(name) {
            return fail(
                line,
                "W4",
                format!("`{name}` is bound in two nested bodies"),
            );
        }
        let body = self.frames[current].body;
        self.vars.push(Var {
            name: name.to_string(),
            ty,
            mutable,
            body,
        });
        let id = self.vars.len() - 1;
        self.frames[current].names.insert(name.to_string(), id);
        Ok(id)
    }

    fn new_site(
        &mut self,
        name: &str,
        alternatives: Vec<String>,
        line: u32,
    ) -> Result<SiteId, Error> {
        if !self.site_names.insert(name.to_string()) {
            return fail(
                line,
                "W1",
                format!("the site name `{name}` is used twice in one control"),
            );
        }
        let body = self.frame().body;
        self.sites.push(Site {
            name: name.to_string(),
            alternatives,
            body,
        });
        Ok(self.sites.len() - 1)
    }

    fn selector(&self, selector: &syntax::Selector, line: u32) -> Result<Selector, Error> {
        Ok(match selector {
            syntax::Selector::Top => Selector::Top,
            syntax::Selector::Not(inner) => Selector::Not(Box::new(self.selector(inner, line)?)),
            syntax::Selector::And(left, right) => Selector::And(
                Box::new(self.selector(left, line)?),
                Box::new(self.selector(right, line)?),
            ),
            syntax::Selector::Or(left, right) => Selector::Or(
                Box::new(self.selector(left, line)?),
                Box::new(self.selector(right, line)?),
            ),
            syntax::Selector::Atom(site, alternative) => {
                let Some(id) = self
                    .frames
                    .iter()
                    .rev()
                    .find_map(|frame| frame.sites.get(site))
                else {
                    return fail(
                        line,
                        "W2",
                        format!(
                            "no site `{site}` precedes this statement in its body or around it"
                        ),
                    );
                };
                let Some(index) = self.sites[*id]
                    .alternatives
                    .iter()
                    .position(|name| name == alternative)
                else {
                    return fail(
                        line,
                        "W2",
                        format!("the site `{site}` has no alternative `{alternative}`"),
                    );
                };
                Selector::Atom(*id, index)
            }
        })
    }

    fn push(&mut self, command: Command, selector: Selector, line: u32) {
        let body = self.frame().body;
        self.bodies[body].statements.push(Statement {
            selector,
            command,
            line,
        });
    }

    fn statements(&mut self, statements: &[syntax::Statement]) -> Result<(), Error> {
        for statement in statements {
            let line = statement.line;
            let selector = self.selector(&statement.selector, line)?;
            let command = self.command(&statement.command, line)?;
            self.push(command, selector, line);
        }
        Ok(())
    }

    fn is_mutable_place(&self, place: &Place, through_exc: bool) -> bool {
        self.vars[place.root].mutable || through_exc
    }

    fn arguments(
        &self,
        what: &str,
        arguments: &[syntax::Expr],
        types: &[Type],
        clause: &'static str,
        line: u32,
    ) -> Result<Vec<Expr>, Error> {
        if arguments.len() != types.len() {
            return fail(
                line,
                clause,
                format!(
                    "{what} takes {} values, and {} are given",
                    types.len(),
                    arguments.len()
                ),
            );
        }
        let typer = self.typer(Mode::Expression, line);
        let mut built = Vec::with_capacity(arguments.len());
        for (argument, ty) in arguments.iter().zip(types) {
            if let Type::Ref(BorrowMode::Exclusive, inner) = ty {
                let place_expr = match argument {
                    syntax::Expr::Borrow(BorrowMode::Exclusive, p) => &**p,
                    other => other,
                };
                let (place, through_exc) = typer.resolve_place(place_expr)?;
                if !self.is_mutable_place(&place, through_exc) {
                    return fail(
                        line,
                        "C1",
                        format!("`{}` is not a mutable place", self.vars[place.root].name),
                    );
                }
                if place.ty != **inner {
                    return fail(
                        line,
                        clause,
                        format!(
                            "borrowed place has type {}, and {} is expected",
                            place.ty, inner
                        ),
                    );
                }
                built.push(Expr::Borrow(BorrowMode::Exclusive, place, ty.clone()));
            } else {
                built.push(typer.build(argument, ty)?);
            }
        }
        Ok(built)
    }

    fn resolve_operation(
        &mut self,
        operation: &str,
        arguments: &[syntax::Expr],
        line: u32,
    ) -> Result<(ControlId, Operation, Vec<Expr>, Vec<Alternative>), Error> {
        if let Some(callee) = self.globals.control_ids.get(operation).copied() {
            if self.in_contingency && !self.controls[callee].contingent {
                return fail(
                    line,
                    "F6",
                    format!(
                        "`{operation}` is not marked `contingent` and cannot be called inside a contingency"
                    ),
                );
            }
            let types: Vec<Type> = self.controls[callee]
                .parameters
                .iter()
                .map(|parameter| parameter.ty.clone())
                .collect();
            let built_args =
                self.arguments(&format!("`{operation}`"), arguments, &types, "C1", line)?;
            let contract = self.controls[callee].contract.clone();
            return Ok((callee, Operation::Control(callee), built_args, contract));
        }

        if operation == "outcome" {
            if arguments.len() != 1 {
                return fail(
                    line,
                    "C1",
                    format!("`outcome` takes 1 value, and {} are given", arguments.len()),
                );
            }
            let arg = self.typer(Mode::Expression, line).infer(&arguments[0])?;
            let Type::Outcome(oid, _) = arg.ty() else {
                return fail(
                    line,
                    "C1",
                    format!(
                        "`outcome` expects a captured outcome value, found {}",
                        arg.ty()
                    ),
                );
            };
            let contract = self.globals.outcomes[oid].alternatives.clone();
            return Ok((0, Operation::Outcome(oid), vec![arg], contract));
        }

        if let Some(&vid) = self.globals.variant_ids.get(operation) {
            if arguments.len() != 1 {
                return fail(
                    line,
                    "C1",
                    format!(
                        "`{operation}` takes 1 variant value, and {} are given",
                        arguments.len()
                    ),
                );
            }
            let raw = &arguments[0];
            let by_value = matches!(raw, syntax::Expr::Move(_))
                || !matches!(
                    raw,
                    syntax::Expr::Name(_)
                        | syntax::Expr::Field(_, _)
                        | syntax::Expr::Index(_, _)
                        | syntax::Expr::Borrow(_, _)
                );
            let inner_raw = match raw {
                syntax::Expr::Borrow(_, inner) => &**inner,
                other => other,
            };
            let expected = Type::Variant(vid, operation.to_string());
            let arg = self
                .typer(Mode::Expression, line)
                .build(inner_raw, &expected)?;
            let contract: Vec<Alternative> = self.globals.variants[vid]
                .constructors
                .iter()
                .map(|c| Alternative {
                    name: c.name.clone(),
                    fields: c.fields.clone(),
                    condition: None,
                })
                .collect();
            return Ok((
                0,
                Operation::Variant { id: vid, by_value },
                vec![arg],
                contract,
            ));
        }

        self.resolve_builtin_op(operation, arguments, line)
    }

    fn resolve_builtin_op(
        &mut self,
        operation: &str,
        arguments: &[syntax::Expr],
        line: u32,
    ) -> Result<(ControlId, Operation, Vec<Expr>, Vec<Alternative>), Error> {
        let typer = self.typer(Mode::Expression, line);
        fn unwrap_place(e: &syntax::Expr) -> &syntax::Expr {
            match e {
                syntax::Expr::Borrow(_, inner) => inner,
                other => other,
            }
        }
        match operation {
            "push" => {
                if arguments.len() != 2 {
                    return fail(line, "C1", "`push` takes 2 arguments");
                }
                let (place, through_exc) = typer.resolve_place(unwrap_place(&arguments[0]))?;
                if !self.is_mutable_place(&place, through_exc) {
                    return fail(
                        line,
                        "C1",
                        format!("`{}` is not a mutable place", self.vars[place.root].name),
                    );
                }
                let Type::Vector(elem_ty) = &place.ty else {
                    return fail(line, "C1", "`push` requires a vector place");
                };
                let elem_ty = (**elem_ty).clone();
                let val_expr = typer.build(&arguments[1], &elem_ty)?;
                let borrow_ty = Type::Ref(BorrowMode::Exclusive, Box::new(place.ty.clone()));
                let contract = vec![Alternative {
                    name: "done".to_string(),
                    fields: Vec::new(),
                    condition: None,
                }];
                Ok((
                    0,
                    Operation::Builtin {
                        op: BuiltinOp::Push,
                        target: Some(place.clone()),
                        contract: contract.clone(),
                    },
                    vec![
                        Expr::Borrow(BorrowMode::Exclusive, place, borrow_ty),
                        val_expr,
                    ],
                    contract,
                ))
            }
            "pop" => {
                if arguments.len() != 1 {
                    return fail(line, "C1", "`pop` takes 1 argument");
                }
                let (place, through_exc) = typer.resolve_place(unwrap_place(&arguments[0]))?;
                if !self.is_mutable_place(&place, through_exc) {
                    return fail(
                        line,
                        "C1",
                        format!("`{}` is not a mutable place", self.vars[place.root].name),
                    );
                }
                let Type::Vector(elem_ty) = &place.ty else {
                    return fail(line, "C1", "`pop` requires a vector place");
                };
                let elem_ty = (**elem_ty).clone();
                let borrow_ty = Type::Ref(BorrowMode::Exclusive, Box::new(place.ty.clone()));
                let contract = vec![
                    Alternative {
                        name: "item".to_string(),
                        fields: vec![Parameter {
                            name: "value".to_string(),
                            ty: elem_ty,
                        }],
                        condition: None,
                    },
                    Alternative {
                        name: "empty".to_string(),
                        fields: Vec::new(),
                        condition: None,
                    },
                ];
                Ok((
                    0,
                    Operation::Builtin {
                        op: BuiltinOp::Pop,
                        target: Some(place.clone()),
                        contract: contract.clone(),
                    },
                    vec![Expr::Borrow(BorrowMode::Exclusive, place, borrow_ty)],
                    contract,
                ))
            }
            "take" => {
                if arguments.len() != 1 {
                    return fail(line, "C1", "`take` takes 1 argument");
                }
                let vec_expr = typer.infer(&arguments[0])?;
                let Type::Vector(elem_ty) = vec_expr.ty() else {
                    return fail(line, "C1", "`take` requires a vector argument");
                };
                let vec_ty = Type::Vector(elem_ty.clone());
                let contract = vec![
                    Alternative {
                        name: "item".to_string(),
                        fields: vec![
                            Parameter {
                                name: "head".to_string(),
                                ty: *elem_ty,
                            },
                            Parameter {
                                name: "tail".to_string(),
                                ty: vec_ty,
                            },
                        ],
                        condition: None,
                    },
                    Alternative {
                        name: "empty".to_string(),
                        fields: Vec::new(),
                        condition: None,
                    },
                ];
                Ok((
                    0,
                    Operation::Builtin {
                        op: BuiltinOp::Take,
                        target: None,
                        contract: contract.clone(),
                    },
                    vec![vec_expr],
                    contract,
                ))
            }
            "get" => {
                if arguments.len() != 2 {
                    return fail(line, "C1", "`get` takes 2 arguments");
                }
                let raw_map = unwrap_place(&arguments[0]);
                let map_expr = if let Ok((place, _)) = typer.resolve_place(raw_map) {
                    let bty = Type::Ref(BorrowMode::Shared, Box::new(place.ty.clone()));
                    Expr::Borrow(BorrowMode::Shared, place, bty)
                } else {
                    typer.infer(raw_map)?
                };
                let Type::Map(k_ty, v_ty) = map_expr.ty().deref_type().clone() else {
                    return fail(line, "C1", "`get` requires a map argument");
                };
                let raw_key = unwrap_place(&arguments[1]);
                let key_expr = if let Ok((place, _)) = typer.resolve_place(raw_key)
                    && place.ty == *k_ty
                {
                    let bty = Type::Ref(BorrowMode::Shared, Box::new(place.ty.clone()));
                    Expr::Borrow(BorrowMode::Shared, place, bty)
                } else {
                    typer.build(raw_key, &k_ty)?
                };
                let contract = vec![
                    Alternative {
                        name: "found".to_string(),
                        fields: vec![Parameter {
                            name: "value".to_string(),
                            ty: *v_ty,
                        }],
                        condition: None,
                    },
                    Alternative {
                        name: "missing".to_string(),
                        fields: Vec::new(),
                        condition: None,
                    },
                ];
                Ok((
                    0,
                    Operation::Builtin {
                        op: BuiltinOp::Get,
                        target: None,
                        contract: contract.clone(),
                    },
                    vec![map_expr, key_expr],
                    contract,
                ))
            }
            "insert" => {
                if arguments.len() != 3 {
                    return fail(line, "C1", "`insert` takes 3 arguments");
                }
                let (place, through_exc) = typer.resolve_place(unwrap_place(&arguments[0]))?;
                if !self.is_mutable_place(&place, through_exc) {
                    return fail(
                        line,
                        "C1",
                        format!("`{}` is not a mutable place", self.vars[place.root].name),
                    );
                }
                let Type::Map(k_ty, v_ty) = &place.ty else {
                    return fail(line, "C1", "`insert` requires a map place");
                };
                let (k_ty, v_ty) = ((**k_ty).clone(), (**v_ty).clone());
                let key_expr = typer.build(&arguments[1], &k_ty)?;
                let val_expr = typer.build(&arguments[2], &v_ty)?;
                let borrow_ty = Type::Ref(BorrowMode::Exclusive, Box::new(place.ty.clone()));
                let contract = vec![
                    Alternative {
                        name: "inserted".to_string(),
                        fields: Vec::new(),
                        condition: None,
                    },
                    Alternative {
                        name: "replaced".to_string(),
                        fields: vec![Parameter {
                            name: "previous".to_string(),
                            ty: v_ty,
                        }],
                        condition: None,
                    },
                ];
                Ok((
                    0,
                    Operation::Builtin {
                        op: BuiltinOp::Insert,
                        target: Some(place.clone()),
                        contract: contract.clone(),
                    },
                    vec![
                        Expr::Borrow(BorrowMode::Exclusive, place, borrow_ty),
                        key_expr,
                        val_expr,
                    ],
                    contract,
                ))
            }
            "remove" => {
                if arguments.len() != 2 {
                    return fail(line, "C1", "`remove` takes 2 arguments");
                }
                let (place, through_exc) = typer.resolve_place(unwrap_place(&arguments[0]))?;
                if !self.is_mutable_place(&place, through_exc) {
                    return fail(
                        line,
                        "C1",
                        format!("`{}` is not a mutable place", self.vars[place.root].name),
                    );
                }
                let Type::Map(k_ty, v_ty) = &place.ty else {
                    return fail(line, "C1", "`remove` requires a map place");
                };
                let (k_ty, v_ty) = ((**k_ty).clone(), (**v_ty).clone());
                let raw_key = unwrap_place(&arguments[1]);
                let key_expr = if let Ok((kplace, _)) = typer.resolve_place(raw_key)
                    && kplace.ty == k_ty
                {
                    let bty = Type::Ref(BorrowMode::Shared, Box::new(kplace.ty.clone()));
                    Expr::Borrow(BorrowMode::Shared, kplace, bty)
                } else {
                    typer.build(raw_key, &k_ty)?
                };
                let borrow_ty = Type::Ref(BorrowMode::Exclusive, Box::new(place.ty.clone()));
                let contract = vec![
                    Alternative {
                        name: "removed".to_string(),
                        fields: vec![Parameter {
                            name: "previous".to_string(),
                            ty: v_ty,
                        }],
                        condition: None,
                    },
                    Alternative {
                        name: "missing".to_string(),
                        fields: Vec::new(),
                        condition: None,
                    },
                ];
                Ok((
                    0,
                    Operation::Builtin {
                        op: BuiltinOp::Remove,
                        target: Some(place.clone()),
                        contract: contract.clone(),
                    },
                    vec![
                        Expr::Borrow(BorrowMode::Exclusive, place, borrow_ty),
                        key_expr,
                    ],
                    contract,
                ))
            }
            "entry_at" => {
                if arguments.len() != 2 {
                    return fail(line, "C1", "`entry_at` takes 2 arguments");
                }
                let raw_map = unwrap_place(&arguments[0]);
                let map_expr = if let Ok((place, _)) = typer.resolve_place(raw_map) {
                    let bty = Type::Ref(BorrowMode::Shared, Box::new(place.ty.clone()));
                    Expr::Borrow(BorrowMode::Shared, place, bty)
                } else {
                    typer.infer(raw_map)?
                };
                let Type::Map(k_ty, v_ty) = map_expr.ty().deref_type().clone() else {
                    return fail(line, "C1", "`entry_at` requires a map argument");
                };
                let pos_expr = typer.build(&arguments[1], &Type::Int(IntType::Usize))?;
                let contract = vec![
                    Alternative {
                        name: "item".to_string(),
                        fields: vec![
                            Parameter {
                                name: "key".to_string(),
                                ty: *k_ty,
                            },
                            Parameter {
                                name: "value".to_string(),
                                ty: *v_ty,
                            },
                            Parameter {
                                name: "next".to_string(),
                                ty: Type::Int(IntType::Usize),
                            },
                        ],
                        condition: None,
                    },
                    Alternative {
                        name: "finished".to_string(),
                        fields: Vec::new(),
                        condition: None,
                    },
                ];
                Ok((
                    0,
                    Operation::Builtin {
                        op: BuiltinOp::EntryAt,
                        target: None,
                        contract: contract.clone(),
                    },
                    vec![map_expr, pos_expr],
                    contract,
                ))
            }
            "emit" => {
                if arguments.len() != 1 {
                    return fail(line, "C1", "`emit` takes 1 argument");
                }
                let raw_text = unwrap_place(&arguments[0]);
                let text_expr = if let Ok((place, _)) = typer.resolve_place(raw_text)
                    && place.ty == Type::String
                {
                    let bty = Type::Ref(BorrowMode::Shared, Box::new(Type::String));
                    Expr::Borrow(BorrowMode::Shared, place, bty)
                } else {
                    typer.build(raw_text, &Type::String)?
                };
                let contract = vec![Alternative {
                    name: "done".to_string(),
                    fields: Vec::new(),
                    condition: None,
                }];
                Ok((
                    0,
                    Operation::Builtin {
                        op: BuiltinOp::Emit,
                        target: None,
                        contract: contract.clone(),
                    },
                    vec![text_expr],
                    contract,
                ))
            }
            _ => fail(line, "C1", format!("`{operation}` is not a control")),
        }
    }

    fn intern_outcome_for_contract(&mut self, name: &str, contract: &[Alternative]) -> OutcomeId {
        if let Some(&id) = self.globals.outcome_ids.get(name) {
            return id;
        }
        // Check if an existing outcome has the exact same alternative names and field types.
        for (id, existing) in self.globals.outcomes.iter().enumerate() {
            if existing.alternatives.len() == contract.len()
                && existing.alternatives.iter().zip(contract).all(|(a, b)| {
                    a.name == b.name
                        && a.fields.len() == b.fields.len()
                        && a.fields
                            .iter()
                            .zip(&b.fields)
                            .all(|(fa, fb)| fa.ty == fb.ty)
                })
            {
                return id;
            }
        }
        let fault_releasable = contract.iter().all(|alt| {
            alt.fields
                .iter()
                .all(|f| self.globals.is_fault_releasable(&f.ty))
        });
        let id = self.globals.outcomes.len();
        self.globals.outcomes.push(OutcomeDef {
            name: name.to_string(),
            public: false,
            fault_releasable,
            alternatives: contract.to_vec(),
        });
        self.globals.outcome_ids.insert(name.to_string(), id);
        id
    }

    fn command(&mut self, command: &syntax::Command, line: u32) -> Result<Command, Error> {
        match command {
            syntax::Command::Bind {
                mutable,
                name,
                ty,
                value,
            } => {
                let resolved_ty = match ty {
                    Some(t) => Some(self.globals.resolve_type(t, true, false, line)?),
                    None => None,
                };
                let known = self
                    .frames
                    .last()
                    .and_then(|frame| frame.names.get(name))
                    .copied();
                let expected = resolved_ty.or_else(|| known.map(|id| self.vars[id].ty.clone()));
                let typer = self.typer(Mode::Expression, line);
                let value = match expected {
                    Some(expected) => typer.build(value, &expected)?,
                    None => typer.infer(value)?,
                };
                let var = self.declare(name, value.ty(), *mutable, line)?;
                Ok(Command::Bind { var, value })
            }
            syntax::Command::Unpack {
                struct_name,
                names,
                value,
            } => {
                let Some(&sid) = self.globals.struct_ids.get(struct_name.as_str()) else {
                    return fail(
                        line,
                        "C1",
                        format!("`{struct_name}` is not a declared struct"),
                    );
                };
                let expected = Type::Struct(sid, struct_name.clone());
                let value = self.typer(Mode::Expression, line).build(value, &expected)?;
                let field_types: Vec<Type> = self.globals.structs[sid]
                    .fields
                    .iter()
                    .map(|f| f.ty.clone())
                    .collect();
                if names.len() != field_types.len() {
                    return fail(
                        line,
                        "C1",
                        format!(
                            "struct `{struct_name}` has {} fields, and {} names are given",
                            field_types.len(),
                            names.len()
                        ),
                    );
                }
                let mut vars = Vec::with_capacity(names.len());
                for (name, fty) in names.iter().zip(field_types) {
                    vars.push(self.declare(name, fty, false, line)?);
                }
                Ok(Command::Unpack {
                    struct_id: sid,
                    vars,
                    value,
                })
            }
            syntax::Command::Capture {
                mutable,
                name,
                ty,
                operation,
                arguments,
            } => {
                let (_, op_kind, built_args, contract) =
                    self.resolve_operation(operation, arguments, line)?;
                // [T6]: outcome<K> is allowed only when no payload of K contains a borrow.
                for alt in &contract {
                    for f in &alt.fields {
                        if matches!(f.ty, Type::Ref(_, _)) {
                            return fail(
                                line,
                                "T6",
                                "`outcome⟨K⟩` is not allowed when a payload contains a borrow",
                            );
                        }
                    }
                }
                let outcome_id = self.intern_outcome_for_contract(operation, &contract);
                let out_ty =
                    Type::Outcome(outcome_id, self.globals.outcomes[outcome_id].name.clone());
                if let Some(written_ty) = ty {
                    let resolved_written =
                        self.globals.resolve_type(written_ty, true, false, line)?;
                    if resolved_written != out_ty {
                        return fail(
                            line,
                            "C4",
                            format!(
                                "captured outcome has type {out_ty}, and {resolved_written} is written"
                            ),
                        );
                    }
                }
                let var = self.declare(name, out_ty, *mutable, line)?;
                Ok(Command::Capture {
                    var,
                    outcome_id,
                    operation: op_kind,
                    arguments: built_args,
                })
            }
            syntax::Command::Assign { name, value } => {
                if matches!(value, syntax::Expr::Borrow(_, _)) {
                    return fail(line, "W8", "an assignment never takes a borrow");
                }
                let Some((VarRef::Local(var), ty)) = self.find(name) else {
                    return fail(line, "C1", format!("`{name}` is not a name here"));
                };
                if matches!(ty, Type::Ref(BorrowMode::Shared, _)) {
                    return fail(
                        line,
                        "W8",
                        format!("shared borrow `{name}` cannot be assigned"),
                    );
                }
                let through_exc = matches!(ty, Type::Ref(BorrowMode::Exclusive, _));
                if !self.vars[var].mutable && !through_exc {
                    return fail(line, "C1", format!("`{name}` is not a mutable place"));
                }
                let target_ty = ty.deref_type().clone();
                let value = self
                    .typer(Mode::Expression, line)
                    .build(value, &target_ty)?;
                Ok(Command::Assign { var, value })
            }
            syntax::Command::AssignPlace { target, value } => {
                if matches!(value, syntax::Expr::Borrow(_, _)) {
                    return fail(line, "W8", "an assignment never takes a borrow");
                }
                let mut place_expr = syntax::Expr::Name(target.root.clone());
                for step in &target.steps {
                    place_expr = match step {
                        syntax::PlaceStep::Field(f) => {
                            syntax::Expr::Field(Box::new(place_expr), f.clone())
                        }
                        syntax::PlaceStep::Index(idx) => {
                            syntax::Expr::Index(Box::new(place_expr), Box::new(idx.clone()))
                        }
                    };
                }
                let typer = self.typer(Mode::Expression, line);
                let (place, through_exc) = typer.resolve_place(&place_expr)?;
                if !self.is_mutable_place(&place, through_exc) {
                    return fail(
                        line,
                        "C1",
                        format!("`{}` is not a mutable place", self.vars[place.root].name),
                    );
                }
                let value = typer.build(value, &place.ty)?;
                Ok(Command::AssignPlace {
                    target: place,
                    value,
                })
            }
            syntax::Command::Oblige(formula) => Ok(Command::Oblige(
                self.typer(Mode::Formula, line)
                    .build(formula, &Type::Bool)?,
            )),
            syntax::Command::Test { site, condition } => {
                let condition = self
                    .typer(Mode::Expression, line)
                    .build(condition, &Type::Bool)?;
                let id = self.new_site(site, vec!["true".into(), "false".into()], line)?;
                self.frame().sites.insert(site.clone(), id);
                Ok(Command::Test {
                    site: id,
                    condition,
                })
            }
            syntax::Command::Decide {
                site,
                operation,
                arguments,
                arms,
            } => self.decide(site, operation, arguments, arms, line),
            syntax::Command::Loop {
                site,
                name,
                parameters,
                invariant,
                body,
                arms,
            } => self.looping(site, name, parameters, invariant.as_ref(), body, arms, line),
            syntax::Command::Boundary {
                site,
                body,
                captures,
                message,
                contingency,
            } => self.lexical_boundary(site, body, captures, message.as_deref(), contingency, line),
            syntax::Command::Supervise {
                boundary,
                arguments,
            } => {
                let Some(&bid) = self.globals.boundary_ids.get(boundary.as_str()) else {
                    return fail(line, "C1", format!("`{boundary}` is not a named boundary"));
                };
                let types: Vec<Type> = self.boundaries[bid]
                    .1
                    .iter()
                    .map(|p| p.ty.clone())
                    .collect();
                let built_args = self.arguments(
                    &format!("boundary `{boundary}`"),
                    arguments,
                    &types,
                    "C1",
                    line,
                )?;
                Ok(Command::Supervise {
                    boundary: bid,
                    arguments: built_args,
                })
            }
            syntax::Command::Again { name, arguments } => {
                let Some(frame) = self.frames.iter().rev().find(|frame| {
                    frame
                        .looping
                        .as_ref()
                        .is_some_and(|looping| looping.name == *name)
                }) else {
                    return fail(
                        line,
                        "W3",
                        format!("`↑ {name}` is not inside the loop `{name}`"),
                    );
                };
                let body = frame.body;
                let looping = frame.looping.as_ref().expect("a loop frame");
                let types: Vec<Type> = looping
                    .parameters
                    .iter()
                    .map(|var| self.vars[*var].ty.clone())
                    .collect();
                let arguments =
                    self.arguments(&format!("the loop `{name}`"), arguments, &types, "C1", line)?;
                Ok(Command::Again { body, arguments })
            }
            syntax::Command::Leave {
                alternative,
                payload,
            } => {
                let typer = self.typer(Mode::Expression, line);
                let mut kinds = Vec::new();
                for operand in payload {
                    kinds.push(typer.kind(operand)?);
                }
                let statement = self.bodies[self.frames.last().expect("a frame").body]
                    .statements
                    .len();
                let Some(looping) = self.frame().looping.as_mut() else {
                    return fail(line, "W3", "`□` is not inside a loop body");
                };
                let Some(index) = looping
                    .alternatives
                    .iter()
                    .position(|name| name == alternative)
                else {
                    return fail(
                        line,
                        "C3",
                        format!("the loop statement does not list the alternative `{alternative}`"),
                    );
                };
                if looping.arity[index] != payload.len() {
                    return fail(
                        line,
                        "C3",
                        format!(
                            "`{alternative}` has {} payload names at the loop statement, and {} values leave",
                            looping.arity[index],
                            payload.len()
                        ),
                    );
                }
                looping.leaves.push(PendingLeave {
                    statement,
                    alternative: index,
                    kinds,
                    payload: payload.clone(),
                    line,
                });
                Ok(Command::Leave {
                    alternative: index,
                    payload: Vec::new(),
                })
            }
            syntax::Command::Exit {
                alternative,
                payload,
            } => {
                let Some(control_id) = self.control else {
                    return fail(line, "F6", "`■` is not allowed inside a named boundary");
                };
                let contract = &self.controls[control_id].contract;
                let Some(index) = contract.iter().position(|entry| entry.name == *alternative)
                else {
                    return fail(
                        line,
                        "C3",
                        format!("`{alternative}` is not an alternative of the control's contract"),
                    );
                };
                let types: Vec<Type> = contract[index]
                    .fields
                    .iter()
                    .map(|field| field.ty.clone())
                    .collect();
                let payload = self.arguments(
                    &format!("the alternative `{alternative}`"),
                    payload,
                    &types,
                    "C3",
                    line,
                )?;
                Ok(Command::Exit {
                    alternative: index,
                    payload,
                })
            }
            syntax::Command::Fault { message, assumed } => {
                let message = self
                    .typer(Mode::Expression, line)
                    .build(message, &Type::String)?;
                Ok(Command::Fault {
                    message,
                    assumed: *assumed,
                })
            }
            syntax::Command::Impossible => Ok(Command::Impossible),
            syntax::Command::Terminate(status) => {
                if !self.in_boundary && !self.in_contingency {
                    return fail(
                        line,
                        "F6",
                        "`terminate` is only allowed inside a fault boundary",
                    );
                }
                let built = match status {
                    Some(expr) => Some(self.typer(Mode::Expression, line).infer(expr)?),
                    None => None,
                };
                Ok(Command::Terminate(built))
            }
            syntax::Command::Escalate => {
                if !self.in_contingency {
                    return fail(
                        line,
                        "F5",
                        "`escalate` is only allowed inside a contingency body",
                    );
                }
                Ok(Command::Escalate)
            }
            syntax::Command::Recover { loop_name } => {
                if !self.in_contingency {
                    return fail(
                        line,
                        "F5",
                        "`recover` is only allowed inside a lexical contingency body",
                    );
                }
                let Some(frame) = self.frames.iter().rev().find(|frame| {
                    frame
                        .looping
                        .as_ref()
                        .is_some_and(|looping| looping.name == *loop_name)
                }) else {
                    return fail(
                        line,
                        "F5",
                        format!("`recover ↑ {loop_name}` is not inside the loop `{loop_name}`"),
                    );
                };
                Ok(Command::Recover { body: frame.body })
            }
        }
    }

    fn decide(
        &mut self,
        site: &str,
        operation: &str,
        arguments: &[syntax::Expr],
        arms: &[syntax::Arm],
        line: u32,
    ) -> Result<Command, Error> {
        let (callee_id, op_kind, built_args, contract) =
            self.resolve_operation(operation, arguments, line)?;
        let mut listed = Vec::new();
        for arm in arms {
            let Some(index) = contract
                .iter()
                .position(|entry| entry.name == arm.alternative)
            else {
                return fail(
                    line,
                    "C2",
                    format!(
                        "`{}` is not an alternative of `{operation}`",
                        arm.alternative
                    ),
                );
            };
            if listed.contains(&index) {
                return fail(
                    line,
                    "C2",
                    format!("the alternative `{}` is listed twice", arm.alternative),
                );
            }
            if arm.names.len() != contract[index].fields.len() {
                return fail(
                    line,
                    "C2",
                    format!(
                        "`{}` has {} payload fields, and {} names are given",
                        arm.alternative,
                        contract[index].fields.len(),
                        arm.names.len()
                    ),
                );
            }
            listed.push(index);
        }
        let names = arms.iter().map(|arm| arm.alternative.clone()).collect();
        let id = self.new_site(site, names, line)?;
        let mut built = Vec::new();
        for (arm, index) in arms.iter().zip(&listed) {
            let mut vars = Vec::new();
            for (name, field) in arm.names.iter().zip(&contract[*index].fields) {
                vars.push(self.declare(name, field.ty.clone(), false, line)?);
            }
            built.push(Arm {
                alternative: *index,
                vars,
            });
        }
        self.frame().sites.insert(site.to_string(), id);
        Ok(Command::Decide {
            site: id,
            operation: callee_id,
            op_kind,
            arguments: built_args,
            arms: built,
        })
    }

    fn lexical_boundary(
        &mut self,
        site: &str,
        body: &[syntax::Statement],
        captures: &[String],
        message: Option<&str>,
        contingency: &[syntax::Statement],
        line: u32,
    ) -> Result<Command, Error> {
        let mut capture_vars = Vec::with_capacity(captures.len());
        let mut capture_types = Vec::with_capacity(captures.len());
        let mut seen = HashSet::new();
        for cap in captures {
            if !seen.insert(cap.as_str()) {
                return fail(
                    line,
                    "F5",
                    format!("`{cap}` is listed twice in contingency captures"),
                );
            }
            let Some((VarRef::Local(var), ty)) = self.find(cap) else {
                return fail(line, "C1", format!("`{cap}` is not a name here"));
            };
            if matches!(ty, Type::Ref(_, _)) {
                return fail(
                    line,
                    "F5",
                    format!("captured name `{cap}` in boundary must own its value"),
                );
            }
            capture_vars.push(var);
            capture_types.push((cap.clone(), ty));
        }

        let site_id = self.new_site(site, vec!["done".to_string()], line)?;
        let parent = self.frame().body;

        // 1. Resolve `B₁` (`body`).
        self.bodies.push(Body {
            statements: Vec::new(),
            parent: Some(parent),
            head: None,
        });
        let b1_id = self.bodies.len() - 1;
        let prev_in_boundary = self.in_boundary;
        self.in_boundary = true;
        self.frames.push(Frame {
            body: b1_id,
            names: HashMap::new(),
            nested: HashSet::new(),
            sites: HashMap::new(),
            looping: None,
        });
        self.statements(body)?;
        let finished_b1 = self.frames.pop().expect("boundary frame");
        self.in_boundary = prev_in_boundary;
        self.frame().nested.extend(finished_b1.names.into_keys());
        self.frame().nested.extend(finished_b1.nested);

        // 2. Resolve `B₂` (`contingency`).
        self.bodies.push(Body {
            statements: Vec::new(),
            parent: Some(parent),
            head: None,
        });
        let b2_id = self.bodies.len() - 1;
        let prev_in_contingency = self.in_contingency;
        self.in_contingency = true;
        self.frames.push(Frame {
            body: b2_id,
            names: HashMap::new(),
            nested: HashSet::new(),
            sites: HashMap::new(),
            looping: None,
        });
        let mut contingency_vars = Vec::with_capacity(capture_types.len());
        for (cap_name, cap_ty) in capture_types {
            let var = self.vars.len();
            self.vars.push(Var {
                name: cap_name.clone(),
                ty: cap_ty,
                mutable: false,
                body: b2_id,
            });
            self.frame().names.insert(cap_name, var);
            contingency_vars.push(var);
        }
        let message_var = if let Some(msg) = message {
            let var = self.vars.len();
            self.vars.push(Var {
                name: msg.to_string(),
                ty: Type::String,
                mutable: false,
                body: b2_id,
            });
            self.frame().names.insert(msg.to_string(), var);
            Some(var)
        } else {
            None
        };
        self.statements(contingency)?;
        let _finished_b2 = self.frames.pop().expect("contingency frame");
        self.in_contingency = prev_in_contingency;

        self.frame().sites.insert(site.to_string(), site_id);
        Ok(Command::Boundary {
            site: site_id,
            body: b1_id,
            captures: capture_vars,
            contingency_vars,
            message_var,
            contingency: b2_id,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn looping(
        &mut self,
        site: &str,
        name: &str,
        parameters: &[syntax::LoopParameter],
        invariant: Option<&syntax::Expr>,
        body: &[syntax::Statement],
        arms: &[syntax::Arm],
        line: u32,
    ) -> Result<Command, Error> {
        let mut resolved_params = Vec::with_capacity(parameters.len());
        let mut initial = Vec::with_capacity(parameters.len());
        for parameter in parameters {
            let rty = self
                .globals
                .resolve_type(&parameter.ty, true, false, line)?;
            initial.push(
                self.typer(Mode::Expression, line)
                    .build(&parameter.initial, &rty)?,
            );
            resolved_params.push((parameter.name.clone(), rty, parameter.mutable));
        }
        let mut alternatives: Vec<String> = Vec::new();
        for arm in arms {
            if alternatives.contains(&arm.alternative) {
                return fail(
                    line,
                    "C3",
                    format!("the alternative `{}` is listed twice", arm.alternative),
                );
            }
            alternatives.push(arm.alternative.clone());
        }
        let id = self.new_site(site, alternatives.clone(), line)?;
        let parent = self.frame().body;
        self.bodies.push(Body {
            statements: Vec::new(),
            parent: Some(parent),
            head: None,
        });
        let inner = self.bodies.len() - 1;
        self.frames.push(Frame {
            body: inner,
            names: HashMap::new(),
            nested: HashSet::new(),
            sites: HashMap::new(),
            looping: Some(LoopFrame {
                name: name.to_string(),
                parameters: Vec::new(),
                alternatives,
                arity: arms.iter().map(|arm| arm.names.len()).collect(),
                leaves: Vec::new(),
            }),
        });
        let mut vars = Vec::new();
        for (pname, pty, pmut) in resolved_params {
            if vars.iter().any(|var: &VarId| self.vars[*var].name == pname) {
                return fail(
                    line,
                    "W4",
                    format!("the loop parameter `{pname}` is named twice"),
                );
            }
            vars.push(self.declare(&pname, pty, pmut, line)?);
        }
        self.frame()
            .looping
            .as_mut()
            .expect("a loop frame")
            .parameters = vars.clone();
        let invariant = match invariant {
            Some(formula) => Some(
                self.typer(Mode::Formula, line)
                    .build(formula, &Type::Bool)?,
            ),
            None => None,
        };
        self.bodies[inner].head = Some(LoopHead {
            name: name.to_string(),
            site: id,
            parameters: vars,
            invariant,
        });
        self.statements(body)?;

        // All `□` of one alternative agree on the types of its payload ([C3]).
        let looping = self.frame().looping.take().expect("a loop frame");
        let mut types: Vec<Vec<Type>> = Vec::new();
        for (index, arm) in arms.iter().enumerate() {
            let leaves: Vec<&PendingLeave> = looping
                .leaves
                .iter()
                .filter(|leave| leave.alternative == index)
                .collect();
            if leaves.is_empty() {
                return fail(
                    line,
                    "C3",
                    format!(
                        "no `□` of the loop body uses the listed alternative `{}`",
                        arm.alternative
                    ),
                );
            }
            let mut row = Vec::new();
            for place in 0..arm.names.len() {
                let mut decided: Option<Type> = None;
                for leave in &leaves {
                    match (&decided, &leave.kinds[place]) {
                        (Some(a), Some(b)) if a != b => {
                            return fail(
                                leave.line,
                                "C3",
                                format!(
                                    "the `□ {}` statements differ in a payload type: {a} and {b}",
                                    arm.alternative
                                ),
                            );
                        }
                        (None, Some(b)) => decided = Some(b.clone()),
                        _ => {}
                    }
                }
                row.push(decided.unwrap_or(DEFAULT_INTEGER));
            }
            types.push(row);
        }
        for leave in &looping.leaves {
            let payload = self.arguments(
                "the alternative",
                &leave.payload,
                &types[leave.alternative],
                "C3",
                leave.line,
            )?;
            self.bodies[inner].statements[leave.statement].command = Command::Leave {
                alternative: leave.alternative,
                payload,
            };
        }

        let finished = self.frames.pop().expect("a loop frame");
        let outer = self.frame();
        outer.nested.extend(finished.names.into_keys());
        outer.nested.extend(finished.nested);
        let mut bound = Vec::new();
        for (arm, row) in arms.iter().zip(&types) {
            let mut vars = Vec::new();
            for (name, ty) in arm.names.iter().zip(row) {
                vars.push(self.declare(name, ty.clone(), false, line)?);
            }
            bound.push(vars);
        }
        self.frame().sites.insert(site.to_string(), id);
        Ok(Command::Loop {
            site: id,
            body: inner,
            initial,
            arms: bound,
        })
    }
}

fn resolve_parameters(
    globals: &Globals<'_>,
    parameters: &[syntax::Parameter],
    allow_ref: bool,
    in_outcome: bool,
    line: u32,
    what: &str,
) -> Result<Vec<Parameter>, Error> {
    let mut out: Vec<Parameter> = Vec::new();
    for parameter in parameters {
        if out.iter().any(|earlier| earlier.name == parameter.name) {
            return fail(
                line,
                "W4",
                format!("{what} `{}` is named twice", parameter.name),
            );
        }
        let ty = globals.resolve_type(&parameter.ty, allow_ref, in_outcome, line)?;
        out.push(Parameter {
            name: parameter.name.clone(),
            ty,
        });
    }
    Ok(out)
}

fn calls(expr: &Expr, out: &mut Vec<FunctionId>) {
    match expr {
        Expr::Boolean(_)
        | Expr::Integer(_, _)
        | Expr::String(_)
        | Expr::Var(_, _)
        | Expr::Old(_, _)
        | Expr::EmptyMap(_) => {}
        Expr::Move(inner, _)
        | Expr::Negate(inner, _)
        | Expr::Not(inner)
        | Expr::Field(inner, _, _, _)
        | Expr::Len(inner)
        | Expr::Clone(inner, _)
        | Expr::Trim(inner) => calls(inner, out),
        Expr::Borrow(_, place, _) => {
            for step in &place.steps {
                if let PlaceStep::Index(idx, _) = step {
                    calls(idx, out);
                }
            }
        }
        Expr::Index(base, idx, _) => {
            calls(base, out);
            calls(idx, out);
        }
        Expr::Struct(_, items, _) | Expr::Variant(_, _, items, _) | Expr::Vector(items, _) => {
            for item in items {
                calls(item, out);
            }
        }
        Expr::Arithmetic(_, left, right, _)
        | Expr::Compare(_, left, right)
        | Expr::And(left, right)
        | Expr::Or(left, right) => {
            calls(left, out);
            calls(right, out);
        }
        Expr::Call(id, arguments, _) => {
            out.push(*id);
            for argument in arguments {
                calls(argument, out);
            }
        }
    }
}

/// Checks that no struct or variant contains itself by value ([T1]).
fn check_type_cycles(
    globals: &Globals<'_>,
    source_structs: &[&syntax::StructDecl],
    source_variants: &[&syntax::VariantDecl],
) -> Result<(), Error> {
    #[derive(Clone, Copy, PartialEq, Eq, Hash)]
    enum Node {
        Struct(StructId),
        Variant(VariantId),
    }

    fn collect_direct_deps(ty: &Type, deps: &mut Vec<Node>) {
        match ty {
            Type::Struct(id, _) => deps.push(Node::Struct(*id)),
            Type::Variant(id, _) => deps.push(Node::Variant(*id)),
            // A vector or map is heap-allocated, so `[V]` does not have infinite size,
            // though we still compute its kind via fixed point.
            _ => {}
        }
    }

    let mut edges: HashMap<Node, Vec<Node>> = HashMap::new();
    for (id, st) in globals.structs.iter().enumerate() {
        let mut deps = Vec::new();
        for f in &st.fields {
            collect_direct_deps(&f.ty, &mut deps);
        }
        edges.insert(Node::Struct(id), deps);
    }
    for (id, var) in globals.variants.iter().enumerate() {
        let mut deps = Vec::new();
        for c in &var.constructors {
            for f in &c.fields {
                collect_direct_deps(&f.ty, &mut deps);
            }
        }
        edges.insert(Node::Variant(id), deps);
    }

    for (&start, start_edges) in &edges {
        let mut pending = start_edges.clone();
        let mut seen = HashSet::new();
        while let Some(next) = pending.pop() {
            if next == start {
                let (line, name) = match start {
                    Node::Struct(id) => (source_structs[id].line, &globals.structs[id].name),
                    Node::Variant(id) => (source_variants[id].line, &globals.variants[id].name),
                };
                return fail(
                    line,
                    "T1",
                    format!("type `{name}` contains itself by value"),
                );
            }
            if seen.insert(next)
                && let Some(more) = edges.get(&next)
            {
                pending.extend(more.iter().copied());
            }
        }
    }
    Ok(())
}

/// Computes `kind`, `fault_releasable`, `holds_obligation`, `clonable`, and
/// `comparable` for all structs, variants, and outcomes ([T1] to [T3]).
fn compute_type_properties(
    globals: &mut Globals<'_>,
    source_structs: &[&syntax::StructDecl],
    source_variants: &[&syntax::VariantDecl],
) -> Result<(), Error> {
    let rounds = globals.structs.len() + globals.variants.len() + globals.outcomes.len() + 2;
    for _ in 0..rounds {
        for sid in 0..globals.structs.len() {
            let is_copy = globals.structs[sid].is_copy;
            let mut max_kind = if is_copy { Kind::Copy } else { Kind::Drop };
            let mut fault_rel = true;
            let mut holds_ob = false;
            let mut clonable = true;
            let mut comparable = true;
            for i in 0..globals.structs[sid].fields.len() {
                let fty = globals.structs[sid].fields[i].ty.clone();
                max_kind = max_kind.max(globals.kind(&fty));
                fault_rel = fault_rel && globals.is_fault_releasable(&fty);
                holds_ob = holds_ob || globals.holds_obligation(&fty);
                clonable = clonable && globals.is_clonable(&fty);
                comparable = comparable && globals.is_comparable(&fty);
            }
            let st = &mut globals.structs[sid];
            st.kind = max_kind;
            st.fault_releasable = fault_rel;
            st.holds_obligation = holds_ob;
            st.clonable = clonable;
            st.comparable = comparable;
        }
        for vid in 0..globals.variants.len() {
            let is_copy = globals.variants[vid].is_copy;
            let mut max_kind = if is_copy { Kind::Copy } else { Kind::Drop };
            let mut fault_rel = true;
            let mut holds_ob = false;
            let mut clonable = true;
            let mut comparable = true;
            for c in 0..globals.variants[vid].constructors.len() {
                for f in 0..globals.variants[vid].constructors[c].fields.len() {
                    let fty = globals.variants[vid].constructors[c].fields[f].ty.clone();
                    max_kind = max_kind.max(globals.kind(&fty));
                    fault_rel = fault_rel && globals.is_fault_releasable(&fty);
                    holds_ob = holds_ob || globals.holds_obligation(&fty);
                    clonable = clonable && globals.is_clonable(&fty);
                    comparable = comparable && globals.is_comparable(&fty);
                }
            }
            let var = &mut globals.variants[vid];
            var.kind = max_kind;
            var.fault_releasable = fault_rel;
            var.holds_obligation = holds_ob;
            var.clonable = clonable;
            var.comparable = comparable;
        }
        for oid in 0..globals.outcomes.len() {
            let mut fault_rel = true;
            for a in 0..globals.outcomes[oid].alternatives.len() {
                for f in 0..globals.outcomes[oid].alternatives[a].fields.len() {
                    let fty = globals.outcomes[oid].alternatives[a].fields[f].ty.clone();
                    fault_rel = fault_rel && globals.is_fault_releasable(&fty);
                }
            }
            globals.outcomes[oid].fault_releasable = fault_rel;
        }
    }

    // [T1], [G4b]: a `copy` struct or variant must have only `copy` components.
    for (sid, st) in globals.structs.iter().enumerate() {
        if st.is_copy && st.kind != Kind::Copy {
            return fail(
                source_structs[sid].line,
                "T1",
                format!(
                    "`copy struct {}` has a field whose type is not `copy`",
                    st.name
                ),
            );
        }
    }
    for (vid, var) in globals.variants.iter().enumerate() {
        if var.is_copy && var.kind != Kind::Copy {
            return fail(
                source_variants[vid].line,
                "T1",
                format!(
                    "`copy variant {}` has a field whose type is not `copy`",
                    var.name
                ),
            );
        }
    }
    Ok(())
}

/// Resolves and types a program, or reports the first error with its clause.
pub fn resolve(program: &syntax::Program) -> Result<Program, Error> {
    let mut source_structs = Vec::new();
    let mut source_variants = Vec::new();
    let mut source_outcomes = Vec::new();
    let mut source_resources = Vec::new();
    let mut source_boundaries = Vec::new();
    let mut source_functions = Vec::new();
    let mut source_controls = Vec::new();

    for declaration in &program.declarations {
        match declaration {
            syntax::Declaration::Struct(s) => source_structs.push(s),
            syntax::Declaration::Variant(v) => source_variants.push(v),
            syntax::Declaration::Outcome(o) => source_outcomes.push(o),
            syntax::Declaration::Resource(r) => source_resources.push(r),
            syntax::Declaration::Boundary(b) => source_boundaries.push(b),
            syntax::Declaration::Function(f) => source_functions.push(f),
            syntax::Declaration::Control(c) => source_controls.push(c),
        }
    }

    let mut globals = Globals {
        struct_ids: HashMap::new(),
        variant_ids: HashMap::new(),
        resource_ids: HashMap::new(),
        outcome_ids: HashMap::new(),
        boundary_ids: HashMap::new(),
        function_ids: HashMap::new(),
        function_types: Vec::new(),
        control_ids: HashMap::new(),
        structs: Vec::new(),
        variants: Vec::new(),
        resources: Vec::new(),
        outcomes: Vec::new(),
    };

    let mut type_names: HashSet<&str> = HashSet::new();
    for s in &source_structs {
        if Type::from_name(&s.name).is_some() || !type_names.insert(&s.name) {
            return fail(s.line, "C1", format!("type `{}` is declared twice", s.name));
        }
        let id = globals.structs.len();
        globals.struct_ids.insert(&s.name, id);
        globals.structs.push(StructDef {
            name: s.name.clone(),
            public: s.public,
            is_copy: s.is_copy,
            kind: if s.is_copy { Kind::Copy } else { Kind::Drop },
            fault_releasable: true,
            holds_obligation: false,
            clonable: true,
            comparable: true,
            fields: Vec::new(),
        });
    }
    for v in &source_variants {
        if Type::from_name(&v.name).is_some() || !type_names.insert(&v.name) {
            return fail(v.line, "C1", format!("type `{}` is declared twice", v.name));
        }
        let id = globals.variants.len();
        globals.variant_ids.insert(&v.name, id);
        globals.variants.push(VariantDef {
            name: v.name.clone(),
            public: v.public,
            is_copy: v.is_copy,
            kind: if v.is_copy { Kind::Copy } else { Kind::Drop },
            fault_releasable: true,
            holds_obligation: false,
            clonable: true,
            comparable: true,
            constructors: Vec::new(),
        });
    }
    for r in &source_resources {
        if Type::from_name(&r.name).is_some() || !type_names.insert(&r.name) {
            return fail(r.line, "C1", format!("type `{}` is declared twice", r.name));
        }
        let id = globals.resources.len();
        globals.resource_ids.insert(&r.name, id);
        globals.resources.push(ResourceDef {
            name: r.name.clone(),
            on_fault_release: r.on_fault_release,
        });
    }
    for o in &source_outcomes {
        if globals.outcome_ids.contains_key(&o.name) {
            return fail(
                o.line,
                "C1",
                format!("outcome `{}` is declared twice", o.name),
            );
        }
        let id = globals.outcomes.len();
        globals.outcome_ids.insert(o.name.clone(), id);
        globals.outcomes.push(OutcomeDef {
            name: o.name.clone(),
            public: o.public,
            fault_releasable: true,
            alternatives: Vec::new(),
        });
    }

    // Resolve fields of structs, variants and outcomes.
    for (id, s) in source_structs.iter().enumerate() {
        globals.structs[id].fields =
            resolve_parameters(&globals, &s.fields, false, false, s.line, "the field")?;
    }
    for (id, v) in source_variants.iter().enumerate() {
        let mut ctors: Vec<VariantConstructor> = Vec::new();
        for c in &v.constructors {
            if ctors.iter().any(|earlier| earlier.name == c.name) {
                return fail(
                    v.line,
                    "W7",
                    format!("variant `{}` lists constructor `{}` twice", v.name, c.name),
                );
            }
            let fields =
                resolve_parameters(&globals, &c.fields, false, false, v.line, "the field")?;
            ctors.push(VariantConstructor {
                name: c.name.clone(),
                fields,
            });
        }
        globals.variants[id].constructors = ctors;
    }
    for (id, o) in source_outcomes.iter().enumerate() {
        let mut alts: Vec<Alternative> = Vec::new();
        for alt in &o.alternatives {
            if alts.iter().any(|earlier| earlier.name == alt.name) {
                return fail(
                    o.line,
                    "W7",
                    format!("outcome `{}` lists `{}` twice", o.name, alt.name),
                );
            }
            let fields = resolve_parameters(
                &globals,
                &alt.fields,
                false,
                true,
                o.line,
                "the payload field",
            )?;
            alts.push(Alternative {
                name: alt.name.clone(),
                fields,
                condition: None,
            });
        }
        globals.outcomes[id].alternatives = alts;
    }

    check_type_cycles(&globals, &source_structs, &source_variants)?;
    compute_type_properties(&mut globals, &source_structs, &source_variants)?;

    let mut functions: Vec<Function> = Vec::new();
    for function in &source_functions {
        if globals
            .function_ids
            .insert(&function.name, functions.len())
            .is_some()
        {
            return fail(
                function.line,
                "C1",
                format!("`{}` is declared twice", function.name),
            );
        }
        let parameters = resolve_parameters(
            &globals,
            &function.parameters,
            true,
            false,
            function.line,
            "the parameter",
        )?;
        // [W5a]: A function's parameters have types of kind `copy`, or shared
        // borrow types `&τ`. Its result is not a borrow or a view.
        for param in &parameters {
            if globals.kind(&param.ty) != Kind::Copy {
                return fail(
                    function.line,
                    "W5a",
                    format!(
                        "function parameter `{}` has type `{}`, which is not `copy` or `&τ`",
                        param.name, param.ty
                    ),
                );
            }
        }
        let result = globals.resolve_type(&function.result, true, false, function.line)?;
        if matches!(result, Type::Ref(_, _)) {
            return fail(
                function.line,
                "W5a",
                "the result of a function may not be a borrow",
            );
        }
        globals.function_types.push((
            parameters
                .iter()
                .map(|parameter| parameter.ty.clone())
                .collect(),
            result.clone(),
        ));
        functions.push(Function {
            name: function.name.clone(),
            parameters,
            precondition: None,
            result,
            body: Expr::Boolean(false),
        });
    }

    let mut controls: Vec<Control> = Vec::new();
    for control in &source_controls {
        let line = control.line;
        if globals.function_ids.contains_key(control.name.as_str())
            || globals
                .control_ids
                .insert(&control.name, controls.len())
                .is_some()
        {
            return fail(line, "C1", format!("`{}` is declared twice", control.name));
        }
        let parameters = resolve_parameters(
            &globals,
            &control.parameters,
            true,
            false,
            line,
            "the parameter",
        )?;
        let mut contract: Vec<Alternative> = Vec::new();
        for alternative in &control.contract {
            if contract
                .iter()
                .any(|earlier| earlier.name == alternative.name)
            {
                return fail(
                    line,
                    "W7",
                    format!("the contract lists `{}` twice", alternative.name),
                );
            }
            let fields = resolve_parameters(
                &globals,
                &alternative.fields,
                true,
                false,
                line,
                "the payload field",
            )?;
            if let Some(field) = fields
                .iter()
                .find(|field| parameters.iter().any(|p| p.name == field.name))
            {
                return fail(
                    line,
                    "W7",
                    format!(
                        "the payload field `{}` has the name of a parameter",
                        field.name
                    ),
                );
            }
            contract.push(Alternative {
                name: alternative.name.clone(),
                fields,
                condition: None,
            });
        }
        // Register the control's contract in `outcomes` if not already registered
        // so `outcome⟨control_name⟩` works as a type.
        if !globals.outcome_ids.contains_key(&control.name)
            && contract
                .iter()
                .all(|a| a.fields.iter().all(|f| !matches!(f.ty, Type::Ref(_, _))))
        {
            let fault_rel = contract
                .iter()
                .all(|a| a.fields.iter().all(|f| globals.is_fault_releasable(&f.ty)));
            let oid = globals.outcomes.len();
            globals.outcome_ids.insert(control.name.clone(), oid);
            globals.outcomes.push(OutcomeDef {
                name: control.name.clone(),
                public: control.public,
                fault_releasable: fault_rel,
                alternatives: contract.clone(),
            });
        }
        controls.push(Control {
            name: control.name.clone(),
            public: control.public,
            contingent: control.contingent,
            faults: control.faults,
            parameters,
            precondition: None,
            contract,
            uses: control.uses.clone(),
            body: None,
        });
    }

    #[allow(clippy::type_complexity)]
    let mut boundary_headers: Vec<(String, Vec<Parameter>, Vec<usize>, bool, Vec<String>)> =
        Vec::new();
    for b in &source_boundaries {
        if globals
            .boundary_ids
            .insert(&b.name, boundary_headers.len())
            .is_some()
        {
            return fail(
                b.line,
                "C1",
                format!("boundary `{}` is declared twice", b.name),
            );
        }
        // [F6]: No parameter of a named boundary is or contains a borrow.
        let parameters = resolve_parameters(
            &globals,
            &b.parameters,
            false,
            false,
            b.line,
            "the parameter",
        )?;
        let mut retains_idx = Vec::with_capacity(b.retains.len());
        for ret_name in &b.retains {
            let Some(idx) = parameters.iter().position(|p| p.name == *ret_name) else {
                return fail(
                    b.line,
                    "F6",
                    format!("retained name `{ret_name}` is not among the boundary's parameters"),
                );
            };
            if retains_idx.contains(&idx) {
                return fail(
                    b.line,
                    "F6",
                    format!("retained name `{ret_name}` is listed twice"),
                );
            }
            retains_idx.push(idx);
        }
        boundary_headers.push((b.name.clone(), parameters, retains_idx, false, Vec::new()));
    }

    // Formulas of signatures, and the bodies of functions.
    for (id, function) in source_functions.iter().enumerate() {
        let scope = SignatureScope {
            parameters: &functions[id].parameters,
            fields: &[],
        };
        let typer = |mode| Typer {
            globals: &globals,
            scope: &scope,
            mode,
            line: function.line,
        };
        let precondition = match &function.precondition {
            Some(formula) => Some(typer(Mode::Formula).build(formula, &Type::Bool)?),
            None => None,
        };
        let body = typer(Mode::Expression).build(&function.body, &functions[id].result)?;
        functions[id].precondition = precondition;
        functions[id].body = body;
    }
    for (id, control) in source_controls.iter().enumerate() {
        let line = control.line;
        let formula = |formula: &syntax::Expr, fields: &[Parameter], mode: Mode| {
            let scope = SignatureScope {
                parameters: &controls[id].parameters,
                fields,
            };
            Typer {
                globals: &globals,
                scope: &scope,
                mode,
                line,
            }
            .build(formula, &Type::Bool)
        };
        let precondition = match &control.precondition {
            Some(text) => Some(formula(text, &[], Mode::Formula)?),
            None => None,
        };
        let mut conditions = Vec::new();
        for (index, alternative) in control.contract.iter().enumerate() {
            conditions.push(match &alternative.condition {
                Some(text) => Some(formula(
                    text,
                    &controls[id].contract[index].fields,
                    Mode::ContractCondition,
                )?),
                None => None,
            });
        }
        controls[id].precondition = precondition;
        for (index, condition) in conditions.into_iter().enumerate() {
            controls[id].contract[index].condition = condition;
        }
    }

    // [W5]: functions are not recursive.
    let mut edges: Vec<Vec<FunctionId>> = Vec::new();
    for function in &functions {
        let mut out = Vec::new();
        calls(&function.body, &mut out);
        if let Some(precondition) = &function.precondition {
            calls(precondition, &mut out);
        }
        edges.push(out);
    }
    for start in 0..functions.len() {
        let mut pending = edges[start].clone();
        let mut seen = HashSet::new();
        while let Some(next) = pending.pop() {
            if next == start {
                return fail(
                    source_functions[start].line,
                    "W5",
                    format!("the function `{}` reaches itself", functions[start].name),
                );
            }
            if seen.insert(next) {
                pending.extend(edges[next].iter().copied());
            }
        }
    }

    // Resolve named boundaries first so `supervise` knows their effects and fault behavior.
    let mut boundaries: Vec<BoundaryDef> = Vec::with_capacity(source_boundaries.len());
    let summary_for_builder: Vec<(String, Vec<Parameter>, bool, Vec<String>)> = boundary_headers
        .iter()
        .map(|(n, p, _, mf, u)| (n.clone(), p.clone(), *mf, u.clone()))
        .collect();
    for (bid, b) in source_boundaries.iter().enumerate() {
        let (_, ref params, ref retains_idx, _, _) = boundary_headers[bid];
        // Build `B₁` (`body`).
        let mut b1_builder = Builder {
            globals: &mut globals,
            controls: &controls,
            boundaries: &summary_for_builder,
            control: None,
            in_boundary: true,
            in_contingency: false,
            vars: Vec::new(),
            sites: Vec::new(),
            bodies: vec![Body {
                statements: Vec::new(),
                parent: None,
                head: None,
            }],
            site_names: HashSet::new(),
            frames: vec![Frame {
                body: 0,
                names: HashMap::new(),
                nested: HashSet::new(),
                sites: HashMap::new(),
                looping: None,
            }],
        };
        for param in params {
            b1_builder.declare(&param.name, param.ty.clone(), false, b.line)?;
        }
        b1_builder.statements(&b.body)?;
        let body_ctrl = ControlBody {
            vars: b1_builder.vars,
            sites: b1_builder.sites,
            bodies: b1_builder.bodies,
        };

        // Build `B₂` (`contingency`).
        let mut b2_builder = Builder {
            globals: &mut globals,
            controls: &controls,
            boundaries: &summary_for_builder,
            control: None,
            in_boundary: true,
            in_contingency: true,
            vars: Vec::new(),
            sites: Vec::new(),
            bodies: vec![Body {
                statements: Vec::new(),
                parent: None,
                head: None,
            }],
            site_names: HashSet::new(),
            frames: vec![Frame {
                body: 0,
                names: HashMap::new(),
                nested: HashSet::new(),
                sites: HashMap::new(),
                looping: None,
            }],
        };
        for &ridx in retains_idx {
            let p = &params[ridx];
            b2_builder.declare(&p.name, p.ty.clone(), false, b.line)?;
        }
        if retains_idx
            .iter()
            .any(|&ridx| params[ridx].name == b.message)
        {
            return fail(
                b.line,
                "W4",
                format!(
                    "contingency parameter `{}` collides with a retained parameter",
                    b.message
                ),
            );
        }
        b2_builder.declare(&b.message, Type::String, false, b.line)?;
        b2_builder.statements(&b.contingency)?;
        let contingency_ctrl = ControlBody {
            vars: b2_builder.vars,
            sites: b2_builder.sites,
            bodies: b2_builder.bodies,
        };

        // [F6]: `supervise b(ō)` may fault exactly when a history of `B₂` ends
        // with `escalate` or `fault`.
        let may_fault = contingency_ctrl.bodies.iter().any(|body| {
            body.statements
                .iter()
                .any(|s| matches!(s.command, Command::Escalate | Command::Fault { .. }))
        });
        let mut uses = Vec::new();
        for cb in [&body_ctrl, &contingency_ctrl] {
            for inner in &cb.bodies {
                for s in &inner.statements {
                    match &s.command {
                        Command::Decide { op_kind, .. }
                        | Command::Capture {
                            operation: op_kind, ..
                        } => match op_kind {
                            Operation::Control(cid) => {
                                for u in &controls[*cid].uses {
                                    if !uses.contains(u) {
                                        uses.push(u.clone());
                                    }
                                }
                            }
                            Operation::Builtin {
                                op: BuiltinOp::Emit,
                                ..
                            } => {
                                let eff = "console.write".to_string();
                                if !uses.contains(&eff) {
                                    uses.push(eff);
                                }
                            }
                            _ => {}
                        },
                        _ => {}
                    }
                }
            }
        }
        boundaries.push(BoundaryDef {
            name: b.name.clone(),
            parameters: params.clone(),
            retains: retains_idx.clone(),
            body: body_ctrl,
            message: b.message.clone(),
            contingency: contingency_ctrl,
            may_fault,
            uses,
            line: b.line,
        });
    }

    let boundary_summary: Vec<(String, Vec<Parameter>, bool, Vec<String>)> = boundaries
        .iter()
        .map(|b| {
            (
                b.name.clone(),
                b.parameters.clone(),
                b.may_fault,
                b.uses.clone(),
            )
        })
        .collect();

    // Bodies of controls.
    let mut bodies = Vec::new();
    for (id, control) in source_controls.iter().enumerate() {
        if control.external {
            bodies.push(None);
            continue;
        }
        let mut builder = Builder {
            globals: &mut globals,
            controls: &controls,
            boundaries: &boundary_summary,
            control: Some(id),
            in_boundary: false,
            in_contingency: false,
            vars: Vec::new(),
            sites: Vec::new(),
            bodies: vec![Body {
                statements: Vec::new(),
                parent: None,
                head: None,
            }],
            site_names: HashSet::new(),
            frames: vec![Frame {
                body: 0,
                names: HashMap::new(),
                nested: HashSet::new(),
                sites: HashMap::new(),
                looping: None,
            }],
        };
        for parameter in &controls[id].parameters {
            builder.declare(&parameter.name, parameter.ty.clone(), false, control.line)?;
        }
        builder.statements(&control.body)?;
        bodies.push(Some(ControlBody {
            vars: builder.vars,
            sites: builder.sites,
            bodies: builder.bodies,
        }));
    }
    for (control, body) in controls.iter_mut().zip(bodies) {
        control.body = body;
    }
    Ok(Program {
        structs: globals.structs,
        variants: globals.variants,
        resources: globals.resources,
        outcomes: globals.outcomes,
        boundaries,
        functions,
        controls,
    })
}
