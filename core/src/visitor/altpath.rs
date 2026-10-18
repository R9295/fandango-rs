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
//! Implements Section IV, equations (17)–(19) of
//! [An analysis of the relationship of input metrics](https://arxiv.org/pdf/2609.11824).
//! Only genuine choices (two or more branches) count as alternations. Full paths,
//! including intervening nonterminals and concatenations, are retained. The paper
//! considers context-free grammars without repetition operators; express repetitions
//! through recursive alternatives when using this metric.
//!
//! Enumeration requires every reachable cycle to contain a choice. A grammar with
//! a mandatory recursive cycle cannot derive a finite tree through that cycle and
//! is rejected explicitly instead of silently truncating long paths.

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

/// Represents the current state of a k-alt-path computation.
///
/// Use [`AltPathUpdate`] to update the content of this computation.
#[derive(Clone, Debug)]
pub struct AltPaths {
    k: NonZeroUsize,
    /// Discriminants which correspond to alternation nodes of the grammar graph.
    alt: HashSet<usize>,
    lookup: HashMap<Mrc<[usize]>, usize>,
    /// Distinct nonterminal use sites, keyed by generated parent type and child index.
    references: HashMap<(usize, usize), usize>,
    /// Map use-site IDs back to generated nonterminal discriminants.
    reference_types: HashMap<usize, usize>,
}

impl AltPaths {
    /// Create a new k-alt-path state for the given program.
    ///
    /// You need to specify `T` here. If you're using a dynamic implementation, use
    /// `::<&DynamicNode>`, otherwise specify the `Type` or `TypeMut` of your static grammar.
    ///
    /// `k` bounds the number of *alternations* a path may traverse, not its node length.
    ///
    /// # Panics
    ///
    /// Panics if the program does not contain a `start` nonterminal or if `T` cannot map a
    /// reachable grammar node to its generated-tree discriminant, or if a reachable
    /// cycle contains no alternation, or if a reachable repetition/optional operator
    /// is present. Path counts can grow exponentially with `k`.
    #[must_use]
    pub fn new<T>(k: NonZeroUsize, program: &'static Program) -> AltPaths
    where
        T: DiscriminantLookup,
    {
        // Start from the generator's graph, then distinguish nonterminal use sites
        // as required by Section II-C2 of the paper. Generated node types share a
        // discriminant across uses; visitors recover the use site from the parent
        // and child index without changing the generated tree representation.
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
        let mut nonterminals = HashSet::new();
        let mut pending = VecDeque::from([start]);
        let mut visited = HashSet::new();
        while let Some(parent) = pending.pop_front() {
            if !visited.insert(parent) {
                continue;
            }
            let parent_node = *graph.node_weight(parent).unwrap();
            assert!(
                !matches!(parent_node, FandangoNode::Operator(_)),
                "k-alt-path requires recursive alternatives instead of repetition/optional operators"
            );
            let mut child_edges = graph.edges(parent).collect::<Vec<_>>();
            // The code generator assigns child indices in source order too.
            child_edges.sort_by_key(|edge| edge.weight().start());
            let children = child_edges.iter().map(EdgeRef::target).collect::<Vec<_>>();
            pending.extend(children.iter().copied());
            let children = children
                .into_iter()
                .map(|child| T::lookup_discriminant(graph.node_weight(child).unwrap()))
                .collect();
            let parent_discriminant = T::lookup_discriminant(&parent_node);
            if matches!(parent_node, FandangoNode::Nonterminal(_)) {
                nonterminals.insert(parent_discriminant);
            }
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

        let mut references = HashMap::new();
        let mut reference_types = HashMap::new();
        let mut next_id = *edges.keys().max().unwrap();
        // Stable ordering makes path IDs reproducible across processes too.
        let mut parents = edges.keys().copied().collect::<Vec<_>>();
        parents.sort_unstable();
        for parent in parents {
            for (index, child) in edges.get_mut(&parent).unwrap().iter_mut().enumerate() {
                if nonterminals.contains(child) {
                    next_id = next_id.checked_add(1).expect("alt-path node ID overflow");
                    references.insert((parent, index), next_id);
                    reference_types.insert(next_id, *child);
                    *child = next_id;
                }
            }
        }
        for (&reference, &discriminant) in &reference_types {
            edges.insert(reference, edges[&discriminant].clone());
        }
        let mut paths = Self::from_edges(k, &alt, &edges);
        paths.references = references;
        paths.reference_types = reference_types;
        paths
    }

    /// Enumerate every alt-path of the grammar: walks that begin at an alternation, end at
    /// an immediate descendant of an alternation, and traverse between 1 and `k`
    /// alternations.
    fn from_edges(
        k: NonZeroUsize,
        alt: &HashSet<usize>,
        edges: &HashMap<usize, Vec<usize>>,
    ) -> Self {
        // Removing choice nodes must leave a DAG. This ensures that bounding the
        // number of choices bounds enumeration, without any arbitrary node cutoff.
        let mut incoming = edges
            .keys()
            .chain(edges.values().flatten())
            .filter(|node| !alt.contains(*node))
            .map(|&node| (node, 0usize))
            .collect::<HashMap<_, _>>();
        for (&parent, children) in edges {
            if !alt.contains(&parent) {
                for child in children {
                    if let Some(count) = incoming.get_mut(child) {
                        *count += 1;
                    }
                }
            }
        }
        let mut ready = incoming
            .iter()
            .filter_map(|(&node, &count)| (count == 0).then_some(node))
            .collect::<Vec<_>>();
        let mut visited = 0;
        while let Some(node) = ready.pop() {
            visited += 1;
            if let Some(children) = edges.get(&node) {
                for child in children {
                    if let Some(count) = incoming.get_mut(child) {
                        *count -= 1;
                        if *count == 0 {
                            ready.push(*child);
                        }
                    }
                }
            }
        }
        assert_eq!(
            visited,
            incoming.len(),
            "k-alt-path requires every reachable cycle to contain an alternation"
        );

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

                    // Once the kth choice edge is recorded, no extension can
                    // contribute another alt-path within the budget.
                    if tail_is_alt && countalt == k.get() {
                        continue;
                    }
                    let next_countalt = countalt + usize::from(alt.contains(&child));
                    if next_countalt <= k.get() {
                        stack.push((extended, next_countalt));
                    }
                }
            }
        }

        Self {
            k,
            alt: alt.clone(),
            lookup: collected.into_iter().map(|path| (path, 0)).collect(),
            references: HashMap::new(),
            reference_types: HashMap::new(),
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
    ///
    /// Path entries are graph node IDs: nonterminal references have distinct use-site
    /// IDs. Use [`Self::node_discriminant`] to resolve an ID to its generated type.
    #[must_use]
    pub const fn lookup(&self) -> &HashMap<Mrc<[usize]>, usize> {
        &self.lookup
    }

    /// Resolve a coverage node ID to its generated-tree discriminant.
    #[must_use]
    pub fn node_discriminant(&self, id: usize) -> usize {
        self.reference_types.get(&id).copied().unwrap_or(id)
    }

    fn visit_id(&self, parent: Option<usize>, index: usize, discriminant: usize) -> usize {
        parent
            .and_then(|parent| {
                self.references
                    .get(&(self.node_discriminant(parent), index))
            })
            .copied()
            .unwrap_or(discriminant)
    }

    /// Collect the distinct alt-paths in one input. Repeated occurrences count once.
    #[must_use]
    #[allow(clippy::missing_panics_doc)] // Both visitor outcomes are Infallible.
    pub fn paths_in<N: Node>(&self, node: &N) -> HashSet<Mrc<[usize]>> {
        AltPathVisit::new(self, CollectPaths(HashSet::new()))
            .visit(node, 0)
            .unwrap()
            .continue_value()
            .unwrap()
            .value()
    }

    /// Mark an input's paths as covered and return the number newly covered.
    ///
    /// This is cumulative set coverage: zero counts become one, existing counts
    /// stay unchanged. Storage is fixed by the grammar and `k`, and counters do
    /// not grow during endless fuzzing. Do not pair this with removal updates;
    /// use [`AltPathUpdate`] for balanced occurrence counting instead.
    #[allow(clippy::missing_panics_doc)] // Collected paths come from this same table.
    pub fn cover<N: Node>(&mut self, node: &N) -> usize {
        let paths = self.paths_in(node);
        let mut newly_covered = 0;
        for path in paths {
            let count = self.lookup.get_mut(&path).unwrap();
            if *count == 0 {
                *count = 1;
                newly_covered += 1;
            }
        }
        newly_covered
    }

    /// Clear the state of the alt-paths table.
    pub fn clear(&mut self) {
        for v in self.lookup.values_mut() {
            *v = 0;
        }
    }
}

struct CollectPaths(HashSet<Mrc<[usize]>>);

impl AltPathVisitor for CollectPaths {
    type Value = HashSet<Mrc<[usize]>>;
    type Break = Infallible;
    type Error = Infallible;

    fn visit_path(
        &mut self,
        _count: usize,
        path: &Mrc<[usize]>,
    ) -> Result<ControlFlow<Self::Break>, Self::Error> {
        self.0.insert(path.clone());
        Ok(ControlFlow::Continue(()))
    }

    fn value(self) -> Self::Value {
        self.0
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

    fn visit<'program, N>(mut self, node: &'program N, idx: usize) -> VisitResult<Self, T>
    where
        N: Node<Type<'program> = T>,
        T: From<&'program N> + AsNodeRef<N>,
    {
        let id = self
            .altpaths
            .visit_id(self.stack.last().copied(), idx, node.discriminant());
        self.stack.push(id);

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

    fn visit<'program, N>(mut self, node: &'program N, idx: usize) -> VisitResult<Self, T>
    where
        N: Node<Type<'program> = T>,
        T: From<&'program N> + AsNodeRef<N>,
    {
        let id = self
            .altpaths
            .visit_id(self.stack.last().copied(), idx, node.discriminant());
        self.stack.push(id);

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
                    if let Some((rcd, count)) = self.altpaths.lookup.get_key_value(slice)
                        && let ControlFlow::Break(b) = self.and_then.visit_path(*count, rcd)?
                    {
                        return Ok(ControlFlow::Break(b));
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

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(k: usize, alt: &[usize], edges: &[(usize, &[usize])]) -> AltPaths {
        AltPaths::from_edges(
            NonZeroUsize::new(k).unwrap(),
            &alt.iter().copied().collect(),
            &edges
                .iter()
                .map(|&(node, children)| (node, children.to_vec()))
                .collect(),
        )
    }

    fn keys(paths: &AltPaths) -> HashSet<Vec<usize>> {
        paths.lookup.keys().map(|path| path.to_vec()).collect()
    }

    #[test]
    fn counts_choice_edges_and_keeps_intervening_nodes() {
        // Two choices separated by a deterministic chain; the first choice
        // also has a terminal branch. Endpoints are included in the path.
        let edges: &[(usize, &[usize])] = &[(0, &[1, 9]), (1, &[2]), (2, &[3]), (3, &[4, 5])];
        let one = paths(1, &[0, 3], edges);
        assert_eq!(
            keys(&one),
            HashSet::from([vec![0, 1], vec![0, 9], vec![3, 4], vec![3, 5]])
        );
        let two = paths(2, &[0, 3], edges);
        let mut expected = keys(&one);
        expected.extend([vec![0, 1, 2, 3, 4], vec![0, 1, 2, 3, 5]]);
        assert_eq!(keys(&two), expected);
        assert_eq!(two.alt_paths(), (6, 6));
    }

    #[test]
    fn final_alternation_is_not_a_traversed_choice() {
        let one = paths(1, &[0, 1], &[(0, &[1, 2]), (1, &[3, 4])]);
        assert!(one.lookup.contains_key(&[0, 1][..]));
        assert!(!one.lookup.contains_key(&[0, 1, 3][..]));
        let two = paths(2, &[0, 1], &[(0, &[1, 2]), (1, &[3, 4])]);
        assert!(two.lookup.contains_key(&[0, 1, 3][..]));
    }

    #[test]
    fn deterministic_chains_have_no_length_cutoff() {
        let mut edges = (0..100)
            .map(|i| (i, vec![i + 1]))
            .collect::<HashMap<_, _>>();
        edges.insert(100, vec![101, 102]);
        edges.get_mut(&0).unwrap().push(103);
        let paths = AltPaths::from_edges(
            NonZeroUsize::new(2).unwrap(),
            &HashSet::from([0, 100]),
            &edges,
        );
        let expected = (0..=101).collect::<Vec<_>>();
        assert!(paths.lookup.contains_key(expected.as_slice()));
        assert_eq!(paths.alt_paths(), (6, 6));
    }

    #[test]
    fn recursive_choices_are_bounded_by_k() {
        let paths = paths(3, &[0], &[(0, &[1, 2]), (1, &[0])]);
        assert_eq!(
            keys(&paths),
            HashSet::from([
                vec![0, 1],
                vec![0, 2],
                vec![0, 1, 0, 1],
                vec![0, 1, 0, 2],
                vec![0, 1, 0, 1, 0, 1],
                vec![0, 1, 0, 1, 0, 2],
            ])
        );
    }

    #[test]
    #[should_panic(expected = "every reachable cycle to contain an alternation")]
    fn rejects_unbounded_mandatory_cycles() {
        paths(2, &[0], &[(0, &[1, 3]), (1, &[2]), (2, &[1])]);
    }

    #[test]
    fn deterministic_grammar_has_no_alt_paths() {
        assert_eq!(
            paths(2, &[], &[(0, &[1]), (1, &[2, 3])]).alt_paths(),
            (0, 0)
        );
    }

    #[test]
    #[allow(deprecated)] // A tiny dynamic fixture avoids a generated test grammar.
    #[should_panic(expected = "recursive alternatives instead of repetition/optional operators")]
    fn rejects_operators_outside_the_papers_definition() {
        let program = alloc::boxed::Box::leak(alloc::boxed::Box::new(
            Program::try_from("<start> ::= 'a'*;").unwrap(),
        ));
        let _ =
            AltPaths::new::<&crate::dynamic::DynamicNode>(NonZeroUsize::new(2).unwrap(), program);
    }
}
