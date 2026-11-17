// An exclusively owned relaxed radix tree. Only boundary nodes participate in a join.
use crate::movement;

const WIDTH: usize = 32;
const MIN: usize = WIDTH / 2;

enum Node<T> {
    Leaf(Vec<T>),
    Branch {
        children: Vec<Node<T>>,
        ends: Vec<usize>,
    },
}

impl<T> Node<T> {
    fn branch(children: Vec<Self>) -> Self {
        let mut size = 0;
        let ends = children
            .iter()
            .map(|child| {
                size += child.len();
                size
            })
            .collect();
        Self::Branch { children, ends }
    }
    fn len(&self) -> usize {
        match self {
            Self::Leaf(values) => values.len(),
            Self::Branch { ends, .. } => *ends.last().unwrap(),
        }
    }
    fn slots(&self) -> usize {
        match self {
            Self::Leaf(values) => values.len(),
            Self::Branch { children, .. } => children.len(),
        }
    }
    fn get(&self, index: usize, height: usize) -> &T {
        match self {
            Self::Leaf(values) => &values[index],
            Self::Branch { children, ends } => {
                // Each child has at most 32^height elements. This radix estimate
                // cannot overshoot the correct child; size tables correct relaxation.
                let guess = index.checked_shr((height * 5) as u32).unwrap_or(0);
                let child = if index < ends[guess] {
                    guess
                } else {
                    guess + ends[guess..].partition_point(|&end| end <= index)
                };
                children[child].get(index - start(ends, child), height - 1)
            }
        }
    }
    fn visit(&self, f: &mut impl FnMut(&T)) {
        match self {
            Self::Leaf(values) => values.iter().for_each(f),
            Self::Branch { children, .. } => children.iter().for_each(|child| child.visit(f)),
        }
    }
    // Insert on the right edge and return a sibling when the node overflows.
    fn push(&mut self, value: T) -> Option<Self> {
        match self {
            Self::Leaf(values) => {
                if values.len() < WIDTH {
                    movement::transfer(1);
                    if values.len() == values.capacity() {
                        movement::growth(values.len());
                    }
                    values.push(value);
                    return None;
                }
                movement::transfer(values.len() - MIN + 1);
                movement::growth(values.len() - MIN);
                let mut tail = values.split_off(MIN);
                tail.push(value);
                Some(Self::Leaf(tail))
            }
            Self::Branch { children, ends } => {
                let last = children.len() - 1;
                let sibling = children[last].push(value);
                ends[last] = start(ends, last) + children[last].len();
                let sibling = sibling?;
                ends.push(ends[last] + sibling.len());
                children.push(sibling);
                if children.len() <= WIDTH {
                    return None;
                }
                ends.truncate(MIN);
                Some(Self::branch(children.split_off(MIN)))
            }
        }
    }
}

fn start(ends: &[usize], child: usize) -> usize {
    if child == 0 { 0 } else { ends[child - 1] }
}

// Repack underfilled roots with a neighbor before attaching them below a parent.
fn pack<T>(mut slots: Vec<T>) -> (Vec<T>, Option<Vec<T>>) {
    let tail = if slots.len() > WIDTH {
        Some(slots.split_off(slots.len() / 2))
    } else {
        None
    };
    (slots, tail)
}

fn unite<T>(left: Node<T>, lh: usize, right: Node<T>, rh: usize) -> (Node<T>, Option<Node<T>>) {
    if lh == rh {
        if left.slots() >= MIN && right.slots() >= MIN {
            return (left, Some(right));
        }
        return match (left, right) {
            (Node::Leaf(mut a), Node::Leaf(b)) => {
                movement::transfer(b.len());
                if a.len() + b.len() > a.capacity() {
                    movement::growth(a.len());
                }
                a.extend(b);
                if a.len() > WIDTH {
                    movement::transfer(a.len() - a.len() / 2);
                }
                let (front, back) = pack(a);
                (Node::Leaf(front), back.map(Node::Leaf))
            }
            (
                Node::Branch {
                    children: mut a, ..
                },
                Node::Branch { children: b, .. },
            ) => {
                a.extend(b);
                pack_branch(a)
            }
            _ => unreachable!("equal heights have the same node kind"),
        };
    }
    if lh > rh {
        let Node::Branch { mut children, .. } = left else {
            unreachable!()
        };
        let (edge, sibling) = unite(children.pop().unwrap(), lh - 1, right, rh);
        children.push(edge);
        children.extend(sibling);
        pack_branch(children)
    } else {
        let Node::Branch { mut children, .. } = right else {
            unreachable!()
        };
        let (edge, sibling) = unite(left, lh, children.remove(0), rh - 1);
        children.splice(0..0, std::iter::once(edge).chain(sibling));
        pack_branch(children)
    }
}

fn pack_branch<T>(children: Vec<Node<T>>) -> (Node<T>, Option<Node<T>>) {
    let (front, back) = pack(children);
    (Node::branch(front), back.map(Node::branch))
}

pub struct Tree<T> {
    root: Node<T>,
    height: usize,
}

impl<T> Tree<T> {
    pub fn new() -> Self {
        Self {
            root: Node::Leaf(Vec::new()),
            height: 0,
        }
    }
    pub fn len(&self) -> usize {
        self.root.len()
    }
    pub fn get(&self, i: usize) -> Option<&T> {
        (i < self.len()).then(|| self.root.get(i, self.height))
    }
    pub fn visit(&self, mut f: impl FnMut(&T)) {
        self.root.visit(&mut f);
    }
    pub fn push(&mut self, value: T) {
        if let Some(right) = self.root.push(value) {
            let left = std::mem::replace(&mut self.root, Node::Leaf(Vec::new()));
            self.root = Node::branch(vec![left, right]);
            self.height += 1;
        }
    }
    pub fn join(self, other: Self) -> Self {
        if self.len() == 0 {
            return other;
        }
        if other.len() == 0 {
            return self;
        }
        let height = self.height.max(other.height);
        let (root, sibling) = unite(self.root, self.height, other.root, other.height);
        match sibling {
            None => Self { root, height },
            Some(right) => Self {
                root: Node::branch(vec![root, right]),
                height: height + 1,
            },
        }
    }
    fn from_children(mut children: Vec<Node<T>>, height: usize) -> Self {
        match children.len() {
            0 => Self::new(),
            1 => Self {
                root: children.pop().unwrap(),
                height: height - 1,
            },
            _ => Self {
                root: Node::branch(children),
                height,
            },
        }
    }
    pub fn split(self, at: usize) -> Result<(Self, Self), Self> {
        if at > self.len() {
            return Err(self);
        }
        Ok(self.split_valid(at))
    }
    fn split_valid(self, at: usize) -> (Self, Self) {
        if at == 0 {
            return (Self::new(), self);
        }
        if at == self.len() {
            return (self, Self::new());
        }
        match self.root {
            Node::Leaf(mut values) => {
                movement::transfer(values.len() - at);
                let tail = values.split_off(at);
                (
                    Self {
                        root: Node::Leaf(values),
                        height: 0,
                    },
                    Self {
                        root: Node::Leaf(tail),
                        height: 0,
                    },
                )
            }
            Node::Branch { mut children, ends } => {
                let i = ends.partition_point(|&end| end <= at);
                let offset = at - start(&ends, i);
                let mut right = children.split_off(i);
                if offset == 0 {
                    return (
                        Self::from_children(children, self.height),
                        Self::from_children(right, self.height),
                    );
                }
                let edge = Self {
                    root: right.remove(0),
                    height: self.height - 1,
                };
                let (a, b) = edge.split_valid(offset);
                (
                    Self::from_children(children, self.height).join(a),
                    b.join(Self::from_children(right, self.height)),
                )
            }
        }
    }
}

impl<T> Tree<T> {
    pub fn pop_front(&mut self) -> Option<T> {
        if self.len() == 0 {
            return None;
        }
        let (first, rest) = std::mem::replace(self, Self::new()).split_valid(1);
        *self = rest;
        let Node::Leaf(mut leaf) = first.root else {
            unreachable!()
        };
        movement::transfer(1);
        leaf.pop()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn audit<T>(tree: &Tree<T>) {
        fn check<T>(node: &Node<T>, height: usize, root: bool) -> usize {
            assert!(node.slots() <= WIDTH);
            if !root {
                assert!(node.slots() >= MIN);
            }
            match node {
                Node::Leaf(values) => {
                    assert_eq!(height, 0);
                    values.len()
                }
                Node::Branch { children, ends } => {
                    assert!(height > 0 && children.len() >= 2);
                    assert_eq!(children.len(), ends.len());
                    let mut total = 0;
                    for (child, end) in children.iter().zip(ends) {
                        total += check(child, height - 1, false);
                        assert_eq!(total, *end);
                    }
                    total
                }
            }
        }
        assert_eq!(check(&tree.root, tree.height, true), tree.len());
    }

    #[test]
    fn scattered_forest_rejoins_preserve_density_and_indexing() {
        let mut trees = vec![Tree::new()];
        let mut expected = vec![Vec::new()];
        let mut seed = 93usize;
        let mut next = 0;
        for step in 0..3000 {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let i = seed % trees.len();
            if step % 3 == 0 {
                for _ in 0..(seed % 97) {
                    trees[i].push(next);
                    expected[i].push(next);
                    next += 1;
                }
            } else if step % 3 == 1 {
                let tree = trees.remove(i);
                let mut model = expected.remove(i);
                let at = seed % (tree.len() + 1);
                let tail = model.split_off(at);
                let (a, b) = tree.split_valid(at);
                trees.extend([a, b]);
                expected.extend([model, tail]);
            } else if trees.len() > 1 {
                let a = trees.remove(i);
                let mut left = expected.remove(i);
                let j = seed % trees.len();
                let b = trees.remove(j);
                left.extend(expected.remove(j));
                trees.push(a.join(b));
                expected.push(left);
            }
            for (tree, model) in trees.iter().zip(&expected) {
                audit(tree);
                let mut index = 0;
                tree.visit(|value| {
                    assert_eq!(*value, model[index]);
                    assert_eq!(tree.get(index), Some(value));
                    index += 1;
                });
                assert_eq!(index, model.len());
            }
        }
    }

    #[test]
    fn unequal_heights_and_unaligned_seams_preserve_every_owner() {
        struct Ticket<'a>(usize, &'a RefCell<Vec<usize>>);
        impl Drop for Ticket<'_> {
            fn drop(&mut self) {
                self.1.borrow_mut().push(self.0);
            }
        }
        let retired = RefCell::new(Vec::new());
        let mut tree = Tree::new();
        let mut model = Vec::new();
        for n in [1, 15, 16, 17, 31, 32, 33, 256, 1025, 17001] {
            let mut batch = Tree::new();
            let start = model.len();
            for i in start..start + n {
                batch.push(Ticket(i, &retired));
                model.push(i);
            }
            tree = tree.join(batch);
            audit(&tree);
        }
        let mut random = 91u32;
        let len = tree.len();
        for at in [0, len, len + 1, 1, 31, 32, 33]
            .into_iter()
            .chain((0..300).map(|_| {
                random = random.wrapping_mul(1664525).wrapping_add(1013904223);
                random as usize % len
            }))
        {
            tree = match tree.split(at) {
                Ok((a, b)) => {
                    audit(&a);
                    audit(&b);
                    assert_eq!(a.len(), at);
                    model.rotate_left(at);
                    b.join(a)
                }
                Err(original) => {
                    assert!(at > len);
                    original
                }
            };
            audit(&tree);
            let mut seen = Vec::new();
            tree.visit(|v| seen.push(v.0));
            assert_eq!(seen, model);
            assert_eq!(tree.get(at).map(|v| v.0), model.get(at).copied());
            assert!(retired.borrow().is_empty());
        }
        for expected in model {
            assert_eq!(tree.pop_front().unwrap().0, expected);
        }
        assert!(tree.pop_front().is_none());
        drop(tree);
        let mut ids = retired.into_inner();
        ids.sort_unstable();
        assert_eq!(ids, (0..len).collect::<Vec<_>>());
    }
}
