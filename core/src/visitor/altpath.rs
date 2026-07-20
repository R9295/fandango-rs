//! Utilities associated with computing the k-alt-path coverage of a grammar.
//!
//! `k`-alt-path refines [`crate::visitor::kpath`] by observing that only *alternations*
//! carry information. At a nonterminal or a concatenation, expansion always traverses the
//! outgoing edges, so those steps are implied by the rules of derivation; at an
//! alternation, exactly one branch is taken, and that choice is the only thing a path
//! records. Restricting paths to run between alternation branch-edges therefore removes
//! the redundancy in `k`-path while *increasing* sensitivity: `k`-alt-path covers
//! `(k+1)`-path with substantially fewer subdomains to store and count.
//!
//! Formally, with `N_alt ⊆ N` the alternation nodes of the grammar graph, the alt-paths
//! are
//!
//! ```text
//! A = { x_1 ⊔ … ⊔ x_n ∈ P : x_1, x_{n-1} ∈ N_alt ∧ n > 1 }
//! countalt(x_1 ⊔ … ⊔ x_n) = |{ j ∈ [1, n) : x_j ∈ N_alt }|
//! altpath_k(i) = { p ∈ A_i : 1 ≤ countalt(p) ≤ k }
//! ```
//!
//! i.e. a path that *begins* at an alternation and *ends* at an immediate descendant of
//! an alternation, containing at most `k` alternations. Note that `k` counts
//! **alternations traversed**, not nodes, so `k`-alt-path and `k`-path are not directly
//! comparable at equal `k`: `altpath_k` covers `path_{k+1}`.
//!
//! In this implementation `N_alt` is the set of [`FandangoNode::Alternative`] nodes of the
//! grammar graph. Every production body is an `Alternative` — including single-branch
//! ones, which carry no choice — so those are included as well; this keeps the metric a
//! faithful instantiation over this graph and, usefully, guarantees that every cycle in
//! the graph passes through `N_alt`, which is what bounds path enumeration.

use crate::graph::IntoGraph;
use crate::lang::{FandangoNode, Program};
use crate::typing::{AsNodeRef, DiscriminantLookup, Node, Opaque};
use crate::visitor::{VisitResult, VisitableChildren, Visitor};
use alloc::collections::VecDeque;
use alloc::vec;
use alloc::vec::Vec;
use core::convert::Infallible;
use core::num::NonZeroUsize;
use core::ops::ControlFlow;
use hashbrown::{HashMap, HashSet};

use mappable_rc::Mrc;
use petgraph::visit::EdgeRef;

/// Safety bound on how many nodes an enumerated alt-path may span. Only reachable when a
/// grammar contains a cycle of nonterminals and concatenations with no choice point on it.
const MAX_ALT_PATH_NODES: usize = 64;

/// Represents the current state of a k-alt-path computation.
///
/// Use [`AltPathUpdate`] to update the content of this computation.
#[derive(Clone, Debug)]
pub struct AltPaths {
    k: NonZeroUsize,
    /// Discriminants which correspond to alternation nodes of the grammar graph.
    alt: HashSet<usize>,
    lookup: HashMap<Mrc<[usize]>, usize>,
}

impl AltPaths {
    /// Create a new k-alt-path state for the given program.
    ///
    /// You need to specify `T` here. If you're using a dynamic implementation, use
    /// `::<DynamicNode>`, otherwise specify the `Type` or `TypeMut` of your static grammar.
    ///
    /// `k` bounds the number of *alternations* a path may traverse, not its node length.
    ///
    /// # Panics
    ///
    /// Panics if the program does not contain a `start` nonterminal or if `T` cannot map a
    /// reachable grammar node to its generated-tree discriminant.
    #[must_use]
    pub fn new<T>(k: NonZeroUsize, program: &'static Program) -> AltPaths
    where
        T: DiscriminantLookup,
    {
        // Same graph the static code generator consumes, so the coverage universe stays
        // aligned with the generated nodes and their `VisitableChildren` implementations.
        let (_, graph) = program.into_graph();
        let start = graph
            .node_indices()
            .find(|&index| {
                matches!(
                    graph.node_weight(index),
                    Some(FandangoNode::Nonterminal(nonterminal))
                        if nonterminal.name() == "start"
                )
            })
            .expect("program must contain a start nonterminal");

        let mut edges: HashMap<usize, Vec<usize>> = HashMap::new();
        let mut alt: HashSet<usize> = HashSet::new();
        let mut pending = VecDeque::from([start]);
        let mut visited = HashSet::new();
        while let Some(parent) = pending.pop_front() {
            if !visited.insert(parent) {
                continue;
            }
            let parent_node = *graph.node_weight(parent).unwrap();
            let children = graph
                .edges(parent)
                .map(|edge| edge.target())
                .collect::<Vec<_>>();
            pending.extend(children.iter().copied());
            let children = children
                .into_iter()
                .map(|child| T::lookup_discriminant(graph.node_weight(child).unwrap()))
                .collect();
            let parent_discriminant = T::lookup_discriminant(&parent_node);
            // Only genuine choice points count as alternations. Every production body is
            // an `Alternative` in this AST, including single-branch ones like
            // `<var_decl> ::= <identifier>`; those encode no decision, so counting them
            // would inflate `N_alt` and defeat the whole point of the metric.
            if matches!(parent_node, FandangoNode::Alternative(alternative)
                if alternative.concatenations().len() >= 2)
            {
                alt.insert(parent_discriminant);
            }
            assert!(edges.insert(parent_discriminant, children).is_none());
        }

        Self::from_edges(k, &alt, &edges)
    }

    /// Enumerate every alt-path of the grammar: walks that begin at an alternation, end at
    /// an immediate descendant of an alternation, and traverse between 1 and `k`
    /// alternations.
    fn from_edges(
        k: NonZeroUsize,
        alt: &HashSet<usize>,
        edges: &HashMap<usize, Vec<usize>>,
    ) -> Self {
        let mut collected: HashSet<Mrc<[usize]>> = HashSet::new();

        // Depth-first from every alternation node. `countalt` counts the alternations
        // among the nodes already on the path (i.e. positions `[1, n)`), which is exactly
        // the paper's `countalt` once the final node is appended.
        for &origin in alt {
            let mut stack = vec![(vec![origin], 1usize)];
            while let Some((path, countalt)) = stack.pop() {
                let tail = *path.last().unwrap();
                let Some(children) = edges.get(&tail) else {
                    continue;
                };
                let tail_is_alt = alt.contains(&tail);
                for &child in children {
                    let mut extended = path.clone();
                    extended.push(child);

                    // A path is an alt-path exactly when its second-to-last node is an
                    // alternation -- that is, when the step we just took was a branch
                    // choice.
                    if tail_is_alt && countalt <= k.get() {
                        collected.insert(Mrc::from(extended.clone().into_boxed_slice()));
                    }

                    // Extending past `k` alternations can never yield a shorter path, so
                    // stop. Because single-branch alternations are excluded from `N_alt`,
                    // a cycle of nonterminals/concatenations need not pass through an
                    // alternation, so `countalt` alone no longer bounds enumeration --
                    // hence the explicit node cap. Chains between two real choice points
                    // are only a few nodes long in practice, so this does not truncate
                    // reachable alt-paths.
                    let next_countalt = countalt + usize::from(alt.contains(&child));
                    if next_countalt <= k.get() && extended.len() < MAX_ALT_PATH_NODES {
                        stack.push((extended, next_countalt));
                    }
                }
            }
        }

        Self {
            k,
            alt: alt.clone(),
            lookup: collected.into_iter().map(|path| (path, 0)).collect(),
        }
    }

    /// Get the current k-alt-path totals expressed as `(#uncovered, #total)`.
    #[must_use]
    pub fn alt_paths(&self) -> (usize, usize) {
        (
            self.lookup.values().filter(|v| **v == 0).count(),
            self.lookup.len(),
        )
    }

    /// The `k` of k-alt-path: the maximum number of alternations a path may traverse.
    #[must_use]
    pub const fn k(&self) -> NonZeroUsize {
        self.k
    }

    /// The discriminants treated as alternation nodes.
    #[must_use]
    pub const fn alternations(&self) -> &HashSet<usize> {
        &self.alt
    }

    /// Get the lookup table, mapping a particular alt-path to how often it was observed.
    #[must_use]
    pub const fn lookup(&self) -> &HashMap<Mrc<[usize]>, usize> {
        &self.lookup
    }

    /// Clear the state of the alt-paths table.
    pub fn clear(&mut self) {
        for v in self.lookup.values_mut() {
            *v = 0;
        }
    }
}

/// Visitor used to update the [`AltPaths`] values.
pub struct AltPathUpdate<'a, const INSERT: bool> {
    altpaths: &'a mut AltPaths,
    stack: Vec<usize>,
}

impl<'a, const INSERT: bool> AltPathUpdate<'a, INSERT> {
    fn new(altpaths: &'a mut AltPaths) -> Self {
        Self {
            altpaths,
            stack: Vec::new(),
        }
    }

    /// The [`AltPaths`] contained by this visitor. Useful for multiple usages.
    #[must_use]
    pub const fn altpaths(&self) -> &AltPaths {
        self.altpaths
    }
}

impl<'a> AltPathUpdate<'a, true> {
    /// Update the provided [`AltPaths`] by inserting visited paths.
    pub fn inserting(altpaths: &'a mut AltPaths) -> Self {
        Self::new(altpaths)
    }
}

impl<'a> AltPathUpdate<'a, false> {
    /// Update the provided [`AltPaths`] by removing visited paths.
    pub fn removing(altpaths: &'a mut AltPaths) -> Self {
        Self::new(altpaths)
    }
}

impl<const INSERT: bool, T> Visitor<T> for AltPathUpdate<'_, INSERT>
where
    T: VisitableChildren<T>,
{
    type Continue = Self;
    type Break = Infallible;
    type Error = Infallible;

    fn visit<'program, N>(mut self, node: &'program N, _idx: usize) -> VisitResult<Self, T>
    where
        N: Node<Type<'program> = T>,
        T: From<&'program N> + AsNodeRef<N>,
    {
        self.stack.push(node.discriminant());

        // Every alt-path ending at this node: it qualifies only if the node we descended
        // FROM is an alternation, i.e. the last step was a branch choice.
        let len = self.stack.len();
        if len >= 2 && self.altpaths.alt.contains(&self.stack[len - 2]) {
            let k = self.altpaths.k.get();
            let mut countalt = 0usize;
            // Walk start positions backwards from the parent; `countalt` accumulates the
            // alternations in `[start, len-1)` as we go.
            for start in (0..=len - 2).rev() {
                if self.altpaths.alt.contains(&self.stack[start]) {
                    countalt += 1;
                    if countalt > k {
                        break;
                    }
                    // A valid alt-path starts at an alternation.
                    let slice = &self.stack[start..len];
                    if let Some((_, count)) = self.altpaths.lookup.get_key_value_mut(slice) {
                        if INSERT {
                            *count += 1;
                        } else {
                            *count = count.checked_sub(1).unwrap(); // sanity
                        }
                    }
                }
            }
        }

        let mut visitor = node
            .opaque()
            .visit_each(self)
            .unwrap()
            .continue_value()
            .unwrap();
        visitor.stack.pop();
        Ok(ControlFlow::Continue(visitor))
    }
}

/// Visitor pattern for all alt-paths within a provided input.
///
/// Use with [`AltPathVisit`].
pub trait AltPathVisitor {
    /// The value into which this visitor is transformed after a total traversal of an input.
    type Value;
    /// The value which is returned when the visitor has exited traversal early.
    type Break;
    /// The error type returned in the case of an error during traversal.
    type Error;

    /// Invoked with the current number of times the given alt-path has been observed.
    /// There are no guarantees on call order.
    ///
    /// # Errors
    ///
    /// May emit an [`AltPathVisitor::Error`] as defined by implementation.
    fn visit_path(
        &mut self,
        count: usize,
        path: &Mrc<[usize]>,
    ) -> Result<ControlFlow<Self::Break>, Self::Error>;

    /// Convert this visitor into an accrued value.
    fn value(self) -> Self::Value;
}

/// Visitor for inputs which allows visiting alt-paths with [`AltPathVisitor`]
/// implementations.
pub struct AltPathVisit<'a, V> {
    altpaths: &'a AltPaths,
    stack: Vec<usize>,
    and_then: V,
}

impl<'a, V> AltPathVisit<'a, V>
where
    V: AltPathVisitor,
{
    /// Create a new [`AltPathVisit`], calling the provided [`AltPathVisitor`] along the way.
    pub fn new(altpaths: &'a AltPaths, and_then: V) -> Self {
        Self {
            altpaths,
            stack: Vec::new(),
            and_then,
        }
    }

    /// Extract the value of the contained [`AltPathVisitor`] with [`AltPathVisitor::value`].
    pub fn value(self) -> V::Value {
        self.and_then.value()
    }
}

impl<T, V> Visitor<T> for AltPathVisit<'_, V>
where
    T: VisitableChildren<T>,
    V: AltPathVisitor,
{
    type Continue = Self;
    type Break = V::Break;
    type Error = V::Error;

    fn visit<'program, N>(mut self, node: &'program N, _idx: usize) -> VisitResult<Self, T>
    where
        N: Node<Type<'program> = T>,
        T: From<&'program N> + AsNodeRef<N>,
    {
        self.stack.push(node.discriminant());

        // Same rule as `AltPathUpdate`: a path qualifies only when the node we descended
        // from is an alternation.
        let len = self.stack.len();
        if len >= 2 && self.altpaths.alt.contains(&self.stack[len - 2]) {
            let k = self.altpaths.k.get();
            let mut countalt = 0usize;
            for start in (0..=len - 2).rev() {
                if self.altpaths.alt.contains(&self.stack[start]) {
                    countalt += 1;
                    if countalt > k {
                        break;
                    }
                    let slice = &self.stack[start..len];
                    if let Some((rcd, count)) = self.altpaths.lookup.get_key_value(slice) {
                        if let ControlFlow::Break(b) = self.and_then.visit_path(*count, rcd)? {
                            return Ok(ControlFlow::Break(b));
                        }
                    }
                }
            }
        }

        let mut result = node.opaque().visit_each(self);
        if let Ok(ControlFlow::Continue(visitor)) = &mut result {
            visitor.stack.pop();
        }
        result
    }
}
