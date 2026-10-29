//! Ordered, resolved graph. Masks reference identities, never source name strings.
use crate::{Span, manifest::Manifest};
use std::collections::{BTreeMap, BTreeSet};
pub type Id = usize;
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Guard {
    True,
    Atom(Id),
    And(Box<Guard>, Box<Guard>),
    Or(Box<Guard>, Box<Guard>),
}
impl Guard {
    pub fn and(self, other: Self) -> Self {
        if self == Self::True {
            other
        } else if other == Self::True {
            self
        } else {
            Self::And(Box::new(self), Box::new(other))
        }
    }
    pub fn evaluate(&self, facts: &BTreeSet<Id>) -> bool {
        match self {
            Self::True => true,
            Self::Atom(id) => facts.contains(id),
            Self::And(a, b) => a.evaluate(facts) && b.evaluate(facts),
            Self::Or(a, b) => a.evaluate(facts) || b.evaluate(facts),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "lowercase",
    deny_unknown_fields
)]
pub enum Value {
    Bool(bool),
    U32(u32),
    String(String),
    Bytes(Vec<u8>),
    Unit,
    #[serde(skip)]
    Reference {
        ty: String,
        place: Place,
    },
    Opaque {
        ty: String,
        token: String,
    },
    Construct {
        ty: String,
        name: String,
        fields: Vec<Value>,
    },
}
impl Value {
    pub fn ty(&self) -> String {
        match self {
            Self::Bool(_) => "Bool".into(),
            Self::U32(_) => "U32".into(),
            Self::String(_) => "String".into(),
            Self::Bytes(_) => "Bytes".into(),
            Self::Unit => "Unit".into(),
            Self::Opaque { ty, .. } | Self::Construct { ty, .. } | Self::Reference { ty, .. } => {
                ty.clone()
            }
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Place {
    pub indirect: bool,
    pub root: Id,
    pub fields: Vec<usize>,
}
impl Place {
    pub fn overlaps(&self, other: &Self) -> bool {
        self.root == other.root
            && self.indirect == other.indirect
            && (self.fields.starts_with(&other.fields) || other.fields.starts_with(&self.fields))
    }
}
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Expr {
    Value(Value),
    Place(Place),
    Move(Place),
    Reference(Place, String),
    /// Projection of a non-linear temporary. Owned record projections use Place.
    Project(Box<Expr>, usize),
    Construct(String, String, Vec<Expr>),
    /// Record fields in written evaluation order, paired with declaration indices.
    Record(String, Vec<(usize, Expr)>),
    /// Total, manifest-declared builtin; no control alternatives or observable effects.
    Pure(String, Vec<Expr>),
    Length(Box<Expr>),
    Not(Box<Expr>),
    Binary(String, Box<Expr>, Box<Expr>),
}
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Arm {
    pub outcome: String,
    pub atom: Id,
    pub slots: Vec<Id>,
}
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Operation {
    Primitive(String),
    Collection(String),
    Region(String),
    Variant(Expr),
    /// Inspect an active alternative without consuming its owner.
    BorrowedVariant(Expr),
}
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Action {
    Bind(Id, Expr),
    Assign(Place, Expr),
    Drop(Expr),
    /// Release remaining slot storage after its completion obligations are discharged.
    Release(Vec<Id>),
    Emit(Expr),
    Exit(String, Vec<Expr>),
    Loop {
        id: Id,
        parameters: Vec<(Id, Expr)>,
        local_slots: Vec<Id>,
        local_atoms: Vec<Id>,
        body: Vec<Instruction>,
    },
    Again {
        target: Id,
        args: Vec<Expr>,
    },
    Decide {
        operation: Operation,
        args: Vec<Expr>,
        arms: Vec<Arm>,
    },
    /// Expansion-only: a callee exit establishes a caller outcome and payload.
    Complete {
        atom: Id,
        bindings: Vec<(Id, Expr)>,
        /// Leave this callee loop, without restarting or exiting the caller.
        unwind: Option<Id>,
        release: Vec<Id>,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Provenance {
    pub region: String,
    pub site: Option<String>,
    pub call_sites: Vec<String>,
    pub definition: Id,
    pub calls: Vec<Id>,
    pub span: Span,
}
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Instruction {
    pub id: Id,
    pub guard: Guard,
    pub action: Action,
    pub provenance: Provenance,
}
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Slot {
    pub id: Id,
    pub name: String,
    pub ty: String,
}
/// A reference under a constructor cannot be projected until that tag is decided.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub struct SumLeaf {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nested: Option<Box<SumLeaf>>,
    pub outcome: String,
    pub payload: usize,
    pub fields: Vec<usize>,
}
impl SumLeaf {
    pub(crate) fn reference_type(&self, ty: &str, m: &crate::manifest::Manifest) -> Option<String> {
        let outcomes = m.outcomes(ty)?;
        let mut ty = outcomes
            .iter()
            .find(|o| o.name == self.outcome)?
            .fields
            .get(self.payload)?
            .ty
            .clone();
        for index in &self.fields {
            ty = m.field_at(&ty, *index)?.ty.clone();
        }
        match &self.nested {
            Some(next) => next.reference_type(&ty, m),
            None => Some(ty),
        }
    }
}
/// A projection to a stored reference, followed by one dereference. Keeping
/// these steps separate prevents conflating carrier storage and its contents.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub struct ReferenceStep {
    pub fields: Vec<usize>,
    pub sum_leaf: Option<SumLeaf>,
}
impl ReferenceStep {
    pub(crate) fn reference_type(&self, ty: &str, m: &crate::manifest::Manifest) -> Option<String> {
        let mut ty = ty.to_owned();
        for field in &self.fields {
            ty = m.field_at(&ty, *field)?.ty.clone();
        }
        match &self.sum_leaf {
            Some(tag) => tag.reference_type(&ty, m),
            None => Some(ty),
        }
    }
}
pub(crate) fn reference_leaves(
    ty: &str,
    m: &crate::manifest::Manifest,
) -> Vec<(Vec<ReferenceStep>, ReferenceStep, String)> {
    fn walk(
        ty: &str,
        m: &crate::manifest::Manifest,
        through: Vec<ReferenceStep>,
        fields: Vec<usize>,
        out: &mut Vec<(Vec<ReferenceStep>, ReferenceStep, String)>,
    ) {
        if let Some((_, inner)) = crate::manifest::reference_inner(ty) {
            let step = ReferenceStep {
                fields,
                sum_leaf: None,
            };
            out.push((through.clone(), step.clone(), ty.into()));
            let mut through = through;
            through.push(step);
            walk(inner, m, through, vec![], out);
        } else if let Some(crate::manifest::TypeDef::Record { fields: fs, .. }) = m.type_def(ty) {
            for (i, f) in fs.iter().enumerate() {
                let mut path = fields.clone();
                path.push(i);
                walk(&f.ty, m, through.clone(), path, out);
            }
        } else if let Some(outcomes) = m.outcomes(ty) {
            for o in outcomes {
                for (payload, f) in o.fields.iter().enumerate() {
                    let mut nested = vec![];
                    walk(&f.ty, m, vec![], vec![], &mut nested);
                    for (mut prefix, mut step, ty) in nested {
                        let first = prefix.first_mut().unwrap_or(&mut step);
                        first.sum_leaf = Some(SumLeaf {
                            outcome: o.name.clone(),
                            payload,
                            fields: std::mem::take(&mut first.fields),
                            nested: first.sum_leaf.take().map(Box::new),
                        });
                        first.fields = fields.clone();
                        let mut path = through.clone();
                        path.extend(prefix);
                        out.push((path, step, ty));
                    }
                }
            }
        }
    }
    let mut out = vec![];
    walk(ty, m, vec![], vec![], &mut out);
    out
}
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ContainedOrigin {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub through: Vec<ReferenceStep>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sum_leaf: Option<SumLeaf>,
    pub group: String,
    pub fields: Vec<usize>,
}
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BorrowSource {
    /// Retain authorizing carrier storage without claiming it is the pointee.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub guard: bool,
    /// Select a reference stored inside the input, rather than its own storage.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contained: Option<ContainedOrigin>,
    pub parameter: usize,
    pub fields: Vec<usize>,
    /// The path anchors an owner; the actual pointee is hidden by a contract.
    /// Exact sources instead describe the pointee's concrete projection.
    #[serde(default)]
    pub opaque: bool,
}
/// A stored reference leaf has its own dependency contract. Paths use nominal
/// record layout indices and guarded constructor steps within each reference
/// layer. `through` contains the preceding reference/dereference steps. For a
/// reference result, these paths start in the referenced carrier; borrow_returns
/// separately describes the carrier storage lifetime.
/// Neither is an alias for a public origin-group name.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StoredReturn {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub through: Vec<ReferenceStep>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sum_leaf: Option<SumLeaf>,
    pub group: String,
    pub outcome: String,
    pub payload: usize,
    pub fields: Vec<usize>,
    pub sources: Vec<BorrowSource>,
}
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Region {
    #[serde(default)]
    pub stored_returns: Vec<StoredReturn>,
    #[serde(with = "borrow_return_entries")]
    pub borrow_returns: BTreeMap<(String, usize), Vec<BorrowSource>>,
    pub name: String,
    pub parameters: Vec<Slot>,
    pub exits: Vec<crate::manifest::Outcome>,
    pub slots: BTreeMap<Id, Slot>,
    pub atoms: BTreeMap<Id, String>,
    pub code: Vec<Instruction>,
}
#[derive(Clone, Debug)]
pub struct GenericControl {
    pub name: String,
    pub type_parameters: Vec<String>,
    pub type_bounds: BTreeMap<String, Vec<String>>,
    pub exits: Vec<crate::manifest::Outcome>,
}
/// Editor-only inference facts; deliberately excluded from executable IR and certificates.
#[derive(Clone, Debug)]
pub(crate) struct InferredBinding {
    pub control: String,
    pub span: Span,
    pub name: String,
    /// Source spelling, including recursive anonymous payloads, captured before symbolic layouts disappear.
    pub display_type: String,
}
#[derive(Clone, Debug)]
pub struct CheckedProgram {
    /// Bounded source evidence; excluded from executable IR and certificates.
    pub(crate) contributors: BTreeMap<String, BTreeMap<String, Vec<crate::absorption::Raw>>>,
    pub(crate) selected_histories: Vec<crate::history::SelectedHistory>,
    /// Defaults from each control's final named contract, not from its callees.
    pub(crate) outcome_exit_codes: BTreeMap<String, BTreeMap<String, u8>>,
    pub(crate) inferred_bindings: Vec<InferredBinding>,
    pub(crate) manifest: Manifest,
    pub(crate) regions: BTreeMap<String, Region>,
    pub(crate) next_id: Id,
    pub(crate) source_files: Vec<crate::modules::SourceFile>,
    pub(crate) generic_controls: Vec<GenericControl>,
}
impl CheckedProgram {
    pub fn generic_controls(&self) -> &[GenericControl] {
        &self.generic_controls
    }
    pub fn source_files(&self) -> &[crate::modules::SourceFile] {
        &self.source_files
    }
    pub fn region(&self, name: &str) -> Option<&Region> {
        self.regions.get(name)
    }
    pub fn regions(&self) -> impl Iterator<Item = &Region> {
        self.regions.values()
    }
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }
}

// JSON object keys cannot represent (outcome, payload-index) pairs.
mod borrow_return_entries {
    use super::*;
    use serde::{Deserialize, Serialize};
    type Returns = BTreeMap<(String, usize), Vec<BorrowSource>>;
    pub fn serialize<S: serde::Serializer>(value: &Returns, s: S) -> Result<S::Ok, S::Error> {
        value.iter().collect::<Vec<_>>().serialize(s)
    }
    pub fn deserialize<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Returns, D::Error> {
        let entries = Vec::<((String, usize), Vec<BorrowSource>)>::deserialize(d)?;
        let mut result = Returns::new();
        for (key, value) in entries {
            if result.insert(key, value).is_some() {
                return Err(serde::de::Error::custom("duplicate borrow-return entry"));
            }
        }
        Ok(result)
    }
}
