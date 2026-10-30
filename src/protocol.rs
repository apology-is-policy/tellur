//! Finite structural protocol abstraction. Values and data correlations are
//! forgotten; declared alternatives are nondeterministic. This is not a concrete
//! reachability, termination, or program-equivalence decision procedure.
use crate::{Diagnostic, Result, Span, ir::*};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Symbol {
    Emit,
    Primitive { operation: String, outcome: String },
    Exit { outcome: String },
    Fault,
    IsolatedFault,
}
/// Forget payload values, arguments, emit text and primitive effect annotations.
/// The final exit token distinguishes completed traces from proper prefixes.
pub fn project_execution(execution: &crate::runtime::Execution) -> Vec<Symbol> {
    use crate::runtime::Event;
    execution
        .events
        .iter()
        .map(|event| match event {
            Event::Fault { .. } => Symbol::IsolatedFault,
            Event::Emit { .. } => Symbol::Emit,
            Event::Primitive {
                operation, outcome, ..
            } => Symbol::Primitive {
                operation: operation.clone(),
                outcome: outcome.clone(),
            },
        })
        .chain([Symbol::Exit {
            outcome: execution.outcome.clone(),
        }])
        .collect()
}
#[derive(Clone, Debug, serde::Serialize)]
pub struct Edge {
    pub from: usize,
    pub to: usize,
    pub symbol: Option<Symbol>,
    pub instruction: Id,
}
#[derive(Clone, Debug, serde::Serialize)]
pub struct Protocol {
    abstraction: &'static str,
    entry: String,
    states: usize,
    initial: usize,
    accepting: BTreeSet<usize>,
    #[serde(skip_serializing_if = "BTreeSet::is_empty")]
    faulted: BTreeSet<usize>,
    edges: Vec<Edge>,
}
#[derive(Clone, Debug, serde::Serialize)]
pub struct Residual {
    pub claim: &'static str,
    pub prefix_possible: bool,
    pub complete: bool,
    pub can_complete: bool,
    pub faulted: bool,
    pub can_fault: bool,
    pub may_silently_diverge: bool,
    pub next_symbols: Vec<Symbol>,
    pub residual_states: Vec<usize>,
}
fn error(message: impl Into<String>) -> Diagnostic {
    Diagnostic::new("E_PROTOCOL", Span::default(), message)
}
#[derive(Clone)]
enum Step {
    Silent,
    Emit,
    Exit(String),
    Fault,
    Decide(Vec<(Id, Option<Symbol>)>),
    Loop(Id),
    Again(Id),
    Complete(Id, Option<Id>),
}
struct Node {
    id: Id,
    guard: Guard,
    step: Step,
    next: usize,
}
struct Loop {
    body: usize,
    after: usize,
    reset: Vec<Id>,
}
struct Graph {
    nodes: Vec<Node>,
    loops: BTreeMap<Id, Loop>,
}
const ACCEPT: usize = 0;
const FALLTHROUGH: usize = 1;
const FAULT: usize = 2;
impl Graph {
    fn new() -> Self {
        Self {
            nodes: vec![
                Node {
                    id: 0,
                    guard: Guard::True,
                    step: Step::Silent,
                    next: 0,
                },
                Node {
                    id: 0,
                    guard: Guard::True,
                    step: Step::Silent,
                    next: 1,
                },
                Node {
                    id: 0,
                    guard: Guard::True,
                    step: Step::Silent,
                    next: FAULT,
                },
            ],
            loops: BTreeMap::new(),
        }
    }
    fn sequence(
        &mut self,
        code: &[Instruction],
        mut next: usize,
        p: &CheckedProgram,
    ) -> Result<usize> {
        for i in code.iter().rev() {
            let step = match &i.action {
                Action::Loop {
                    id,
                    local_atoms,
                    body,
                    ..
                } => {
                    let body = self.sequence(body, FALLTHROUGH, p)?;
                    self.loops.insert(
                        *id,
                        Loop {
                            body,
                            after: next,
                            reset: local_atoms.clone(),
                        },
                    );
                    Step::Loop(*id)
                }
                Action::Again { target, .. } => Step::Again(*target),
                Action::Complete { atom, unwind, .. } => Step::Complete(*atom, *unwind),
                Action::Exit(name, _) => Step::Exit(name.clone()),
                Action::Fault(_) => Step::Fault,
                Action::Supervise { .. } | Action::Terminate | Action::Escalate => {
                    return Err(error(
                        "protocol analysis of supervision boundaries is not implemented; isolation cannot be erased or treated as a normal outcome",
                    ));
                }
                Action::Emit(_) => Step::Emit,
                Action::Decide {
                    operation, arms, ..
                } => {
                    let primitive = match operation {
                        Operation::Region(_) => {
                            return Err(error("protocol input still contains an unexpanded call"));
                        }
                        Operation::Primitive(name) => p
                            .manifest()
                            .primitives
                            .iter()
                            .find(|x| x.name == *name)
                            .filter(|x| !x.pure),
                        _ => None,
                    };
                    Step::Decide(
                        arms.iter()
                            .map(|a| {
                                (
                                    a.atom,
                                    primitive.map(|p| Symbol::Primitive {
                                        operation: p.name.clone(),
                                        outcome: a.outcome.clone(),
                                    }),
                                )
                            })
                            .collect(),
                    )
                }
                _ => Step::Silent,
            };
            self.nodes.push(Node {
                id: i.id,
                guard: i.guard.clone(),
                step,
                next,
            });
            next = self.nodes.len() - 1;
        }
        Ok(next)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct State {
    pc: usize,
    facts: BTreeSet<Id>,
}
impl CheckedProgram {
    /// Complete finite abstraction, or an explicit state-budget error. The limit
    /// is not an execution-depth bound and never produces a partial success.
    pub fn protocol(&self, entry: &str, state_limit: usize) -> Result<Protocol> {
        if state_limit == 0 {
            return Err(error("protocol state limit must be positive"));
        }
        let expanded = self.expanded(entry)?;
        let mut graph = Graph::new();
        let start = graph.sequence(
            &expanded.region(entry).unwrap().code,
            FALLTHROUGH,
            &expanded,
        )?;
        let initial = State {
            pc: start,
            facts: BTreeSet::new(),
        };
        let mut ids = BTreeMap::from([(initial.clone(), 0)]);
        let mut pending = VecDeque::from([initial]);
        let mut result = Protocol {
            abstraction: "structural_protocol_ignoring_values_v1",
            entry: entry.into(),
            states: 0,
            initial: 0,
            accepting: BTreeSet::new(),
            faulted: BTreeSet::new(),
            edges: vec![],
        };
        while let Some(state) = pending.pop_front() {
            let from = ids[&state];
            if state.pc == ACCEPT {
                result.accepting.insert(from);
                continue;
            }
            if state.pc == FAULT {
                result.faulted.insert(from);
                continue;
            }
            if state.pc == FALLTHROUGH {
                return Err(error(
                    "structural execution reached an undisposed fallthrough",
                ));
            }
            let n = &graph.nodes[state.pc];
            let mut transitions = vec![];
            if !n.guard.evaluate(&state.facts) {
                transitions.push((
                    None,
                    State {
                        pc: n.next,
                        facts: state.facts.clone(),
                    },
                ));
            } else {
                let mut next = State {
                    pc: n.next,
                    facts: state.facts.clone(),
                };
                match &n.step {
                    Step::Silent => transitions.push((None, next)),
                    Step::Emit => transitions.push((Some(Symbol::Emit), next)),
                    Step::Fault => transitions.push((
                        Some(Symbol::Fault),
                        State {
                            pc: FAULT,
                            facts: BTreeSet::new(),
                        },
                    )),
                    Step::Exit(outcome) => transitions.push((
                        Some(Symbol::Exit {
                            outcome: outcome.clone(),
                        }),
                        State {
                            pc: ACCEPT,
                            facts: BTreeSet::new(),
                        },
                    )),
                    Step::Decide(arms) => {
                        for (atom, symbol) in arms {
                            let mut branch = next.clone();
                            branch.facts.insert(*atom);
                            transitions.push((symbol.clone(), branch));
                        }
                    }
                    Step::Loop(id) | Step::Again(id) => {
                        let l = graph
                            .loops
                            .get(id)
                            .ok_or_else(|| error("unknown abstract loop target"))?;
                        for atom in &l.reset {
                            next.facts.remove(atom);
                        }
                        next.pc = l.body;
                        transitions.push((None, next));
                    }
                    Step::Complete(atom, unwind) => {
                        next.facts.insert(*atom);
                        if let Some(id) = unwind {
                            next.pc = graph
                                .loops
                                .get(id)
                                .ok_or_else(|| error("unknown abstract completion target"))?
                                .after;
                        }
                        transitions.push((None, next));
                    }
                }
            }
            for (symbol, target) in transitions {
                let to = if let Some(id) = ids.get(&target) {
                    *id
                } else {
                    if ids.len() >= state_limit {
                        return Err(Diagnostic::new(
                            "E_PROTOCOL_LIMIT",
                            Span::default(),
                            format!(
                                "protocol exceeds {state_limit} states; no partial model is returned"
                            ),
                        ));
                    }
                    let id = ids.len();
                    ids.insert(target.clone(), id);
                    pending.push_back(target);
                    id
                };
                result.edges.push(Edge {
                    from,
                    to,
                    symbol,
                    instruction: n.id,
                });
            }
        }
        result.states = ids.len();
        Ok(result)
    }
}
impl Protocol {
    pub fn state_count(&self) -> usize {
        self.states
    }
    pub fn edges(&self) -> &[Edge] {
        &self.edges
    }
    /// Left residual of the abstract trace language after an exact token prefix.
    /// Divergence is a structural possibility, never proof of concrete divergence.
    pub fn residual(&self, prefix: &[Symbol]) -> Residual {
        let outgoing = self.outgoing();
        let mut current = epsilon(&outgoing, BTreeSet::from([self.initial]));
        for symbol in prefix {
            current = advance(&outgoing, &current, symbol);
        }
        let next_symbols = current
            .iter()
            .flat_map(|s| outgoing[*s].iter())
            .filter_map(|e| e.symbol.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let mut reachable = current.clone();
        let mut pending = current.iter().copied().collect::<Vec<_>>();
        while let Some(s) = pending.pop() {
            for edge in &outgoing[s] {
                if reachable.insert(edge.to) {
                    pending.push(edge.to);
                }
            }
        }
        // An epsilon cycle in the closure permits an infinite silent abstract
        // execution. Kahn's algorithm avoids recursion on user-sized graphs.
        let mut indegree = current
            .iter()
            .map(|s| (*s, 0usize))
            .collect::<BTreeMap<_, _>>();
        for s in &current {
            for e in &outgoing[*s] {
                if e.symbol.is_none() {
                    *indegree.get_mut(&e.to).unwrap() += 1;
                }
            }
        }
        let mut ready = indegree
            .iter()
            .filter_map(|(s, n)| (*n == 0).then_some(*s))
            .collect::<Vec<_>>();
        let mut removed = 0;
        while let Some(s) = ready.pop() {
            removed += 1;
            for e in &outgoing[s] {
                if e.symbol.is_none() {
                    let n = indegree.get_mut(&e.to).unwrap();
                    *n -= 1;
                    if *n == 0 {
                        ready.push(e.to);
                    }
                }
            }
        }
        Residual {
            claim: "possibilities_in_structural_abstraction_only",
            prefix_possible: !current.is_empty(),
            complete: !current.is_disjoint(&self.accepting),
            can_complete: !reachable.is_disjoint(&self.accepting),
            faulted: !current.is_disjoint(&self.faulted),
            can_fault: !reachable.is_disjoint(&self.faulted),
            may_silently_diverge: removed < current.len(),
            next_symbols,
            residual_states: current.into_iter().collect(),
        }
    }
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct Inclusion {
    pub claim: &'static str,
    pub included: bool,
    pub discovered_pairs: usize,
    pub counterexample: Option<Vec<Symbol>>,
}
fn epsilon(outgoing: &[Vec<&Edge>], seeds: BTreeSet<usize>) -> BTreeSet<usize> {
    let mut seen = seeds;
    let mut pending = seen.iter().copied().collect::<Vec<_>>();
    while let Some(s) = pending.pop() {
        for edge in &outgoing[s] {
            if edge.symbol.is_none() && seen.insert(edge.to) {
                pending.push(edge.to);
            }
        }
    }
    seen
}
fn advance(outgoing: &[Vec<&Edge>], states: &BTreeSet<usize>, symbol: &Symbol) -> BTreeSet<usize> {
    epsilon(
        outgoing,
        states
            .iter()
            .flat_map(|s| outgoing[*s].iter())
            .filter(|e| e.symbol.as_ref() == Some(symbol))
            .map(|e| e.to)
            .collect(),
    )
}
impl Protocol {
    fn outgoing(&self) -> Vec<Vec<&Edge>> {
        let mut edges = vec![vec![]; self.states];
        for e in &self.edges {
            edges[e.from].push(e);
        }
        edges
    }
    /// Exact inclusion of finite normal-completion or fault words of these models.
    /// This forgets infinite behavior and concrete data feasibility. Exhausting
    /// the subset-product budget returns an error, never a positive conclusion.
    pub fn included_in(&self, other: &Self, pair_limit: usize) -> Result<Inclusion> {
        if pair_limit == 0 {
            return Err(error("protocol comparison limit must be positive"));
        }
        let left = self.outgoing();
        let right = other.outgoing();
        let initial = (
            epsilon(&left, BTreeSet::from([self.initial])),
            epsilon(&right, BTreeSet::from([other.initial])),
        );
        let mut ids = BTreeMap::from([(initial.clone(), 0)]);
        let mut states = VecDeque::from([initial]);
        let mut parents = vec![None::<(usize, Symbol)>];
        let claim = if self.faulted.is_empty() && other.faulted.is_empty() {
            "inclusion_of_completed_structural_traces_only"
        } else {
            "inclusion_of_terminated_structural_traces_only"
        };
        while let Some((a, b)) = states.pop_front() {
            let id = ids[&(a.clone(), b.clone())];
            if (!a.is_disjoint(&self.accepting) || !a.is_disjoint(&self.faulted))
                && b.is_disjoint(&other.accepting)
                && b.is_disjoint(&other.faulted)
            {
                let mut witness = vec![];
                let mut cursor = id;
                while let Some((parent, symbol)) = &parents[cursor] {
                    witness.push(symbol.clone());
                    cursor = *parent;
                }
                witness.reverse();
                return Ok(Inclusion {
                    claim,
                    included: false,
                    discovered_pairs: ids.len(),
                    counterexample: Some(witness),
                });
            }
            let alphabet = a
                .iter()
                .flat_map(|s| left[*s].iter())
                .filter_map(|e| e.symbol.clone())
                .collect::<BTreeSet<_>>();
            for symbol in alphabet {
                let next = (advance(&left, &a, &symbol), advance(&right, &b, &symbol));
                if !ids.contains_key(&next) {
                    if ids.len() >= pair_limit {
                        return Err(Diagnostic::new(
                            "E_PROTOCOL_LIMIT",
                            Span::default(),
                            format!(
                                "protocol comparison exceeds {pair_limit} state pairs; inclusion is inconclusive"
                            ),
                        ));
                    }
                    ids.insert(next.clone(), ids.len());
                    parents.push(Some((id, symbol)));
                    states.push_back(next);
                }
            }
        }
        Ok(Inclusion {
            claim,
            included: true,
            discovered_pairs: ids.len(),
            counterexample: None,
        })
    }
}
