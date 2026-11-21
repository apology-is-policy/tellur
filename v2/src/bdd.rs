//! Reduced ordered binary decision diagrams (ROBDDs).
//!
//! History formulas (`[K1]` to `[K8]`) and the Boolean structure of fact-logic
//! queries (`docs/v2/facts.md`, section 7) are represented as interned nodes
//! in a [`Bdd`]. `FALSE` is `0` and `TRUE` is `1`.

use std::collections::{BTreeMap, HashMap};

pub type Formula = usize;

pub const FALSE: Formula = 0;
pub const TRUE: Formula = 1;

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub struct Node {
    pub var: usize,
    pub low: Formula,
    pub high: Formula,
}

#[derive(Clone, Debug, Default)]
pub struct Bdd {
    nodes: Vec<Node>,
    unique: HashMap<Node, Formula>,
    cache: HashMap<(u8, Formula, Formula), Formula>,
}

impl Bdd {
    pub fn new() -> Bdd {
        Bdd::default()
    }

    pub fn node(&self, formula: Formula) -> Node {
        if formula < 2 {
            Node {
                var: usize::MAX,
                low: formula,
                high: formula,
            }
        } else {
            self.nodes[formula - 2]
        }
    }

    fn make(&mut self, var: usize, low: Formula, high: Formula) -> Formula {
        if low == high {
            return low;
        }
        let node = Node { var, low, high };
        if let Some(existing) = self.unique.get(&node) {
            return *existing;
        }
        let id = self.nodes.len() + 2;
        self.nodes.push(node);
        self.unique.insert(node, id);
        id
    }

    pub fn variable(&mut self, var: usize) -> Formula {
        self.make(var, FALSE, TRUE)
    }

    pub fn literal(&mut self, var: usize, positive: bool) -> Formula {
        if positive {
            self.make(var, FALSE, TRUE)
        } else {
            self.make(var, TRUE, FALSE)
        }
    }

    fn apply(&mut self, op: u8, mut a: Formula, mut b: Formula) -> Formula {
        if a > b {
            std::mem::swap(&mut a, &mut b);
        }
        if op == 0 {
            if a == FALSE {
                return FALSE;
            }
            if a == TRUE || a == b {
                return b;
            }
        } else if op == 1 {
            if a == FALSE || a == b {
                return b;
            }
            if a == TRUE {
                return TRUE;
            }
        }
        if let Some(cached) = self.cache.get(&(op, a, b)) {
            return *cached;
        }
        let na = self.node(a);
        let nb = self.node(b);
        let var = na.var.min(nb.var);
        let (al, ah) = if na.var == var {
            (na.low, na.high)
        } else {
            (a, a)
        };
        let (bl, bh) = if nb.var == var {
            (nb.low, nb.high)
        } else {
            (b, b)
        };
        let low = self.apply(op, al, bl);
        let high = self.apply(op, ah, bh);
        let result = self.make(var, low, high);
        self.cache.insert((op, a, b), result);
        result
    }

    pub fn and(&mut self, a: Formula, b: Formula) -> Formula {
        self.apply(0, a, b)
    }

    pub fn or(&mut self, a: Formula, b: Formula) -> Formula {
        self.apply(1, a, b)
    }

    pub fn not(&mut self, a: Formula) -> Formula {
        if a < 2 {
            return 1 - a;
        }
        if let Some(cached) = self.cache.get(&(2, a, 0)) {
            return *cached;
        }
        let node = self.node(a);
        let low = self.not(node.low);
        let high = self.not(node.high);
        let result = self.make(node.var, low, high);
        self.cache.insert((2, a, 0), result);
        result
    }

    pub fn minus(&mut self, a: Formula, b: Formula) -> Formula {
        let not_b = self.not(b);
        self.and(a, not_b)
    }

    pub fn implies(&mut self, a: Formula, b: Formula) -> Formula {
        let not_a = self.not(a);
        self.or(not_a, b)
    }

    pub fn iff(&mut self, a: Formula, b: Formula) -> Formula {
        let ab = self.implies(a, b);
        let ba = self.implies(b, a);
        self.and(ab, ba)
    }

    /// Returns a deterministic satisfying assignment for `formula`, preferring
    /// `false` on variables where the low branch can reach `TRUE` so that
    /// counterexample histories stay short ([M6]).
    pub fn witness(&self, mut formula: Formula) -> Option<BTreeMap<usize, bool>> {
        if formula == FALSE {
            return None;
        }
        let mut out = BTreeMap::new();
        while formula >= 2 {
            let node = self.node(formula);
            let value = if node.low != FALSE {
                formula = node.low;
                false
            } else {
                formula = node.high;
                true
            };
            out.insert(node.var, value);
        }
        Some(out)
    }

    pub fn evaluate(&self, mut formula: Formula, assignment: &BTreeMap<usize, bool>) -> bool {
        while formula >= 2 {
            let node = self.node(formula);
            formula = if assignment.get(&node.var).copied().unwrap_or(false) {
                node.high
            } else {
                node.low
            };
        }
        formula == TRUE
    }
}
