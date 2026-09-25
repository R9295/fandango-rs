//! Target for the tree grammar in `grammars/alpenglow.fan`.
//!
//! Each generated scenario is a tree grown from a root at depth 0, whose nodes have up to
//! [`Tree::MAX_CHILDREN`] children each.  The grammar picks how many children each node has.
//! [`fix`] prunes the tree to [`Tree::MAX_DEPTH`] deep and [`Tree::MAX_WIDTH`] nodes at each
//! depth, labels every node (see [`Label`]) and returns the [`Tree`], whose `Display` draws it.
//!
//! The grammar is recursive, rather than having a production per depth, so that its nodes
//! are boxed: unboxed, a scenario would hold room for a full tree of 4^8 nodes inline.
//!
//! A scenario (on one line):
//! ```text
//! {"id": "0", "children": [{"id": "1a", "children": [{"id": "2a", "children": []},
//!                                                    {"id": "2b", "children": []}]},
//!                          {"id": "1b", "children": [{"id": "2c", "children": []}]}]}
//! ```

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

/// A node's label: `0` for the root, else its depth then its index among the nodes of its
/// depth, left to right, in letters: `a` to `z`, then `aa`, `ab` and so on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Label {
    /// The node's depth.
    pub depth: usize,
    /// The node's index among the nodes of its depth.
    pub index: usize,
}

impl Label {
    /// The root's label.
    pub const ROOT: Self = Self { depth: 0, index: 0 };
}

impl fmt::Display for Label {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if *self == Self::ROOT {
            return f.write_str("0");
        }
        let mut letters = Vec::new();
        let mut index = self.index;
        loop {
            letters.push(char::from(b"abcdefghijklmnopqrstuvwxyz"[index % 26]));
            if index < 26 {
                break;
            }
            index = index / 26 - 1;
        }
        write!(f, "{}", self.depth)?;
        letters.iter().rev().try_for_each(|letter| write!(f, "{letter}"))
    }
}

/// A labelled node and the nodes below it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tree {
    /// The node's label.
    pub label: Label,
    /// The node's children, left to right.
    pub children: Vec<Tree>,
}

impl Tree {
    /// Deepest a node can be.
    pub const MAX_DEPTH: usize = 8;
    /// Most children of a node.
    pub const MAX_CHILDREN: usize = 4;
    /// Most nodes across an entire depth.
    pub const MAX_WIDTH: usize = 5;

    /// The path from the root to the deepest node, excluding the root. If several nodes
    /// have the same greatest depth, choose the leftmost one. An empty tree has no path.
    #[must_use]
    pub fn canonical_path(&self) -> Vec<Label> {
        fn visit(tree: &Tree, path: &mut Vec<Label>, best: &mut Vec<Label>) {
            if path.len() > best.len() {
                best.clone_from(path);
            }
            for child in &tree.children {
                path.push(child.label);
                visit(child, path, best);
                path.pop();
            }
        }

        let mut best = Vec::new();
        visit(self, &mut Vec::new(), &mut best);
        best
    }
}

/// Draw `children`, and theirs, each line starting with `prefix`.
fn draw(f: &mut fmt::Formatter<'_>, children: &[Tree], prefix: &mut String) -> fmt::Result {
    for (i, child) in children.iter().enumerate() {
        let last = i + 1 == children.len();
        writeln!(f, "{prefix}{}{}", if last { "└── " } else { "├── " }, child.label)?;
        let len = prefix.len();
        prefix.push_str(if last { "    " } else { "│   " });
        draw(f, &child.children, prefix)?;
        prefix.truncate(len);
    }
    Ok(())
}

/// Draw the tree from its root, a node per line.
impl fmt::Display for Tree {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "{}", self.label)?;
        draw(f, &self.children, &mut String::new())
    }
}

#[cfg(not(feature = "static_defs"))]
mod defs {
    use core::convert::Infallible;
    use fandango::Fandango;

    /// Base for the Alpenglow grammar stored in `alpenglow.fan`.
    #[derive(Fandango)]
    #[fandango(grammar = "grammars/alpenglow.fan", parse = false, dynamic = true)]
    pub struct Alpenglow(Infallible);
}

#[cfg(feature = "static_defs")]
mod defs {
    use super::{Label, Tree};
    use alloc::collections::VecDeque;
    use alloc::format;
    use alloc::vec::Vec;
    use core::convert::Infallible;
    use core::ops::ControlFlow;
    use fandango::Fandango;
    use fandango::generation::{Generated, RawSampler};
    use fandango::typing::{AsNodeMut, AsNodeRef, DowncastMut, Node, OpaqueMut};
    use fandango::visitor::write::WriteVisitor;
    use fandango::visitor::{VisitMutResult, VisitableChildrenMut, Visitor, VisitorMut};

    /// Base for the Alpenglow grammar stored in `alpenglow.fan`.
    #[derive(Fandango)]
    #[fandango(grammar = "grammars/alpenglow.fan", parse = false)]
    pub struct Alpenglow(Infallible);

    /// Render a node to the text it generates.
    fn render<'program, N>(node: &'program N) -> Vec<u8>
    where
        N: Node<Type<'program> = Type<'program>>,
        Type<'program>: From<&'program N> + AsNodeRef<N>,
    {
        let Ok(ControlFlow::Continue(writer)) = WriteVisitor::new(Vec::new()).visit(node, 0);
        writer.output()
    }

    /// Count the immediate nodes in a rendered `<children>` list.
    fn direct_children(children: &nonterminal_children) -> usize {
        let mut nesting = 0;
        let mut count = 0;
        for byte in render(children) {
            match byte {
                b'{' => {
                    if nesting == 0 {
                        count += 1;
                    }
                    nesting += 1;
                }
                b'}' => nesting -= 1,
                _ => {}
            }
        }
        count
    }

    /// Generate `N`s until one's rendering is accepted.
    fn generate_where<N, S, G>(sampler: &mut S, generators: &mut G, accept: impl Fn(&[u8]) -> bool) -> N
    where
        N: Generated<S, G>,
        for<'program> N: Node<Type<'program> = Type<'program>> + 'program,
        for<'program> Type<'program>: From<&'program N> + AsNodeRef<N>,
    {
        loop {
            let candidate = N::generate(sampler, generators, 0);
            if accept(&render(&candidate)) {
                return candidate;
            }
        }
    }

    /// Prune a generated scenario to [`Tree::MAX_DEPTH`] deep and [`Tree::MAX_WIDTH`] nodes
    /// per depth, label every node, and return the tree.
    pub fn fix<S, G>(scenario: &mut nonterminal_start, sampler: &mut S, generators: &mut G) -> Tree
    where
        S: RawSampler,
        nonterminal_children: Generated<S, G>,
        nonterminal_depth: Generated<S, G>,
        nonterminal_letters: Generated<S, G>,
        nonterminal_letter: Generated<S, G>,
    {
        let root = Tree { label: Label::ROOT, children: Vec::new() };
        let mut counts = [0; Tree::MAX_DEPTH + 1];
        counts[0] = 1;
        let fixer = Fix { sampler, generators, open: Vec::from([root]), counts, fields: VecDeque::new() };
        let Ok(ControlFlow::Continue(mut fixer)) = fixer.visit_mut(scenario, 0);
        debug_assert_eq!(fixer.open.len(), 1, "every node closed");
        fixer.open.pop().expect("the root")
    }

    /// Visitor which labels each `<node>` and, once it has visited the node's children,
    /// closes it into its parent, and empties a `<children>` list if it exceeds a depth limit.
    struct Fix<'a, S, G> {
        sampler: &'a mut S,
        generators: &'a mut G,
        /// The nodes being visited, root first.
        open: Vec<Tree>,
        /// How many nodes of each depth are labelled.
        counts: [usize; Tree::MAX_DEPTH + 1],
        /// The label being placed, rendered: its depth, then each letter.
        fields: VecDeque<Vec<u8>>,
    }

    impl<S, G> Fix<'_, S, G>
    where
        S: RawSampler,
    {
        /// Replace `node` with one rendering to the next field.
        fn place<N>(&mut self, node: &mut N)
        where
            N: Generated<S, G>,
            for<'program> N: Node<Type<'program> = Type<'program>> + 'program,
            for<'program> Type<'program>: From<&'program N> + AsNodeRef<N>,
        {
            let field = self.fields.pop_front().expect("a field for the depth and every letter");
            *node = generate_where(self.sampler, self.generators, |rendered| rendered == field);
        }
    }

    impl<S, G, T> VisitorMut<T> for Fix<'_, S, G>
    where
        S: RawSampler,
        nonterminal_children: Generated<S, G>,
        nonterminal_depth: Generated<S, G>,
        nonterminal_letters: Generated<S, G>,
        nonterminal_letter: Generated<S, G>,
        T: VisitableChildrenMut<T>
            + AsNodeMut<nonterminal_node>
            + AsNodeMut<nonterminal_children>
            + AsNodeMut<nonterminal_depth>
            + AsNodeMut<nonterminal_letters>
            + AsNodeMut<nonterminal_letter>,
    {
        type Continue = Self;
        type Break = Infallible;
        type Error = Infallible;

        fn visit_mut<'program, N>(mut self, node: &'program mut N, _idx: usize) -> VisitMutResult<Self, T>
        where
            N: Node<TypeMut<'program> = T>,
            T: From<&'program mut N> + AsNodeMut<N>,
        {
            let mut visited = node.opaque_mut();
            if visited.downcast_mut::<nonterminal_node>().is_some() {
                let depth = self.open.len();
                let label = Label { depth, index: self.counts[depth] };
                self.counts[depth] += 1;
                self.open.push(Tree { label, children: Vec::new() });
                let rendered = format!("{label}").into_bytes();
                self.fields = rendered.chunks(1).map(<[u8]>::to_vec).collect();
                let Ok(ControlFlow::Continue(mut fixer)) = visited.visit_each_mut(self);
                let closed = fixer.open.pop().expect("the node just opened");
                fixer.open.last_mut().expect("the root is never closed").children.push(closed);
                return Ok(ControlFlow::Continue(fixer));
            } else if let Some(children) = visited.downcast_mut::<nonterminal_children>() {
                let child_depth = self.open.len();
                if child_depth > Tree::MAX_DEPTH
                    || direct_children(children) > Tree::MAX_WIDTH - self.counts[child_depth]
                {
                    *children = generate_where(self.sampler, self.generators, |rendered| rendered == b"[]");
                    return Ok(ControlFlow::Continue(self));
                }
            } else if let Some(depth) = visited.downcast_mut::<nonterminal_depth>() {
                self.place(depth);
                return Ok(ControlFlow::Continue(self));
            } else if let Some(letters) = visited.downcast_mut::<nonterminal_letters>() {
                let count = self.fields.len();
                *letters = generate_where(self.sampler, self.generators, |rendered| rendered.len() == count);
            } else if let Some(letter) = visited.downcast_mut::<nonterminal_letter>() {
                self.place(letter);
                return Ok(ControlFlow::Continue(self));
            }
            visited.visit_each_mut(self)
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use alloc::string::String;
        use fandango::tuple_list::tuple_list;
        use fandango_runtime::operators::DepthLimiter;
        use rand::SeedableRng;
        use rand::rngs::StdRng;

        /// The scenario `tree` describes, written out independently of the grammar.
        fn json(tree: &Tree) -> String {
            let children: Vec<String> = tree.children.iter().map(json).collect();
            format!("{{\"id\": \"{}\", \"children\": [{}]}}", tree.label, children.join(", "))
        }

        /// Check depth, child count and labels. `last` also records the total width of each depth.
        fn check(tree: &Tree, depth: usize, last: &mut [Option<usize>; Tree::MAX_DEPTH + 1]) {
            assert!(depth <= Tree::MAX_DEPTH, "{} too deep", tree.label);
            assert_eq!(tree.label.depth, depth, "{} at depth {depth}", tree.label);
            assert_eq!(tree.label.index, last[depth].map_or(0, |index| index + 1), "{} out of order", tree.label);
            last[depth] = Some(tree.label.index);
            assert!(tree.children.len() <= Tree::MAX_CHILDREN, "{} has {} children", tree.label, tree.children.len());
            tree.children.iter().for_each(|child| check(child, depth + 1, last));
        }

        /// How deep `tree` goes, and the most children of any of its nodes.
        fn extent(tree: &Tree) -> (usize, usize) {
            tree.children.iter().map(extent).fold((0, tree.children.len()), |(depth, most), (below, theirs)| {
                (depth.max(below + 1), most.max(theirs))
            })
        }

        #[test]
        fn fixed_scenarios_are_labelled_trees() {
            let mut sampler = StdRng::seed_from_u64(1);
            let mut generators = tuple_list!(DepthLimiter::new(STRUCTURE.inner(), 40));
            let (mut deepest, mut most_children, mut widest) = (false, false, 0);
            for _ in 0..100 {
                let mut scenario = nonterminal_start::generate(&mut sampler, &mut generators, 0);
                let tree = fix(&mut scenario, &mut sampler, &mut generators);
                assert_eq!(String::from_utf8(render(&scenario)).unwrap(), json(&tree) + "\n");
                let mut last = [None; Tree::MAX_DEPTH + 1];
                check(&tree, 0, &mut last);
                assert!(last.iter().all(|index| index.is_none_or(|index| index < Tree::MAX_WIDTH)));
                let (depth, most) = extent(&tree);
                deepest |= depth == Tree::MAX_DEPTH;
                most_children |= most == Tree::MAX_CHILDREN;
                widest = widest.max(last.into_iter().flatten().max().unwrap_or(0) + 1);
            }
            assert!(deepest && most_children, "deepest {deepest}, most children {most_children}");
            assert_eq!(widest, Tree::MAX_WIDTH, "the maximum width should be reachable");
        }
    }
}

pub use defs::*;

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;

    #[test]
    fn labels_count_in_letters() {
        let label = |index| Label { depth: 3, index }.to_string();
        assert_eq!(Label::ROOT.to_string(), "0");
        assert_eq!([label(0), label(25), label(26), label(27), label(701), label(702)], [
            "3a", "3z", "3aa", "3ab", "3zz", "3aaa"
        ]);
    }

    #[test]
    fn draws_the_tree() {
        let node = |depth, index, children| Tree { label: Label { depth, index }, children };
        let tree = node(0, 0, Vec::from([
            node(1, 0, Vec::from([node(2, 0, Vec::new()), node(2, 1, Vec::from([node(3, 0, Vec::new())]))])),
            node(1, 1, Vec::from([node(2, 2, Vec::new())])),
        ]));
        let expected = "\
0
├── 1a
│   ├── 2a
│   └── 2b
│       └── 3a
└── 1b
    └── 2c
";
        assert_eq!(tree.to_string(), expected);
    }

    #[test]
    fn canonical_path_chooses_leftmost_deepest_node() {
        let node = |depth, index, children| Tree { label: Label { depth, index }, children };
        let tree = node(0, 0, Vec::from([
            node(1, 0, Vec::from([node(2, 0, Vec::new())])),
            node(1, 1, Vec::from([node(2, 1, Vec::from([node(3, 0, Vec::new())]))])),
            node(1, 2, Vec::from([node(2, 2, Vec::from([node(3, 1, Vec::new())]))])),
        ]));
        assert_eq!(
            tree.canonical_path(),
            Vec::from([
                Label { depth: 1, index: 1 },
                Label { depth: 2, index: 1 },
                Label { depth: 3, index: 0 },
            ])
        );
        assert!(node(0, 0, Vec::new()).canonical_path().is_empty());
    }
}
