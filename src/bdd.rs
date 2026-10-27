//! Reduced ordered binary decision diagrams. Nodes are interned, never paths.
use std::collections::HashMap;
pub type Formula = usize;
pub const FALSE: Formula = 0;
pub const TRUE: Formula = 1;
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
struct Node {
    var: usize,
    low: Formula,
    high: Formula,
}
#[derive(Default)]
pub struct Bdd {
    nodes: Vec<Node>,
    unique: HashMap<Node, Formula>,
    cache: HashMap<(u8, Formula, Formula), Formula>,
}
impl Bdd {
    fn node(&self, n: Formula) -> Node {
        if n < 2 {
            Node {
                var: usize::MAX,
                low: n,
                high: n,
            }
        } else {
            self.nodes[n - 2]
        }
    }
    fn make(&mut self, var: usize, low: Formula, high: Formula) -> Formula {
        if low == high {
            return low;
        }
        let node = Node { var, low, high };
        if let Some(n) = self.unique.get(&node) {
            return *n;
        }
        let n = self.nodes.len() + 2;
        self.nodes.push(node);
        self.unique.insert(node, n);
        n
    }
    pub fn variable(&mut self, var: usize) -> Formula {
        self.make(var, FALSE, TRUE)
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
        }
        if op == 1 {
            if a == FALSE || a == b {
                return b;
            }
            if a == TRUE {
                return TRUE;
            }
        }
        if let Some(n) = self.cache.get(&(op, a, b)) {
            return *n;
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
        let n = self.make(var, low, high);
        self.cache.insert((op, a, b), n);
        n
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
        if let Some(n) = self.cache.get(&(2, a, 0)) {
            return *n;
        }
        let node = self.node(a);
        let low = self.not(node.low);
        let high = self.not(node.high);
        let n = self.make(node.var, low, high);
        self.cache.insert((2, a, 0), n);
        n
    }
    pub fn minus(&mut self, a: Formula, b: Formula) -> Formula {
        let nb = self.not(b);
        self.and(a, nb)
    }
    pub fn witness(&self, mut n: Formula) -> Option<HashMap<usize, bool>> {
        if n == FALSE {
            return None;
        }
        let mut result = HashMap::new();
        while n >= 2 {
            let node = self.node(n);
            let high = node.high != FALSE;
            result.insert(node.var, high);
            n = if high { node.high } else { node.low };
        }
        Some(result)
    }
    pub fn evaluate(&self, mut n: Formula, assignment: &HashMap<usize, bool>) -> bool {
        while n >= 2 {
            let node = self.node(n);
            n = if assignment.get(&node.var).copied().unwrap_or(false) {
                node.high
            } else {
                node.low
            };
        }
        n == TRUE
    }
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }
}
