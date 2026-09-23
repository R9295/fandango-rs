//! Target for the Alpenglow block tree grammar in `grammars/alpenglow.fan`.
//!
//! Each generated scenario is a tree of blocks over one to eight depths.  Depth `d` is slot
//! `d`, and the root, block `0`, is slot 0.  A depth holds up to five blocks, `<d>a` to
//! `<d>e`, each building on a block of an earlier depth, and resolves one way:
//! - `FAST_FINALIZE`, `FINALIZE` or `NOTAR_FALLBACK`: one of its blocks is certified,
//! - `SKIP` or `SKIP_FALLBACK`: the slot is skipped.  It may hold no blocks at all.
//!
//! The certified blocks form the finalized chain: each builds on the block certified at the
//! nearest earlier depth, or on the root.  The deepest certified block is fast finalized or
//! finalized, so the notar fallback blocks before it are finalized as its ancestors.
//!
//! The grammar picks how many depths there are, each depth's outcome and how many blocks it
//! holds.  [`fix`] numbers the slots and blocks, picks each certified block, and draws every
//! other block's parent from the root and the blocks of earlier depths, which forks the
//! tree.  It returns the [`Tree`], whose `Display` draws it.
//!
//! A scenario (on one line):
//! ```text
//! {"slots": [{"slot": 1, "outcome": "FINALIZE", "block": "1b", "blocks": [{"id": "1a", "parent": "0"}, {"id": "1b", "parent": "0"}]},
//!            {"slot": 2, "outcome": "SKIP", "blocks": []},
//!            {"slot": 3, "outcome": "FAST_FINALIZE", "block": "3a", "blocks": [{"id": "3a", "parent": "1b"}]}]}
//! ```

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;
use core::iter;
use fandango::generation::RawSampler;

/// How a depth resolved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// A fast finalization certificate for one of its blocks.
    FastFinalize,
    /// Notarization and finalization certificates for one of its blocks.
    Finalize,
    /// A notar fallback certificate for one of its blocks, finalized by a descendant.
    NotarFallback,
    /// A skip certificate.
    Skip,
    /// A skip certificate from skip fallback votes.
    SkipFallback,
}

impl Outcome {
    /// Every outcome.
    pub const ALL: [Self; 5] = [Self::FastFinalize, Self::Finalize, Self::NotarFallback, Self::Skip, Self::SkipFallback];

    /// The outcome's name in a scenario.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::FastFinalize => "FAST_FINALIZE",
            Self::Finalize => "FINALIZE",
            Self::NotarFallback => "NOTAR_FALLBACK",
            Self::Skip => "SKIP",
            Self::SkipFallback => "SKIP_FALLBACK",
        }
    }

    /// Whether the outcome certifies one of the depth's blocks.
    #[must_use]
    pub fn certifies(self) -> bool {
        matches!(self, Self::FastFinalize | Self::Finalize | Self::NotarFallback)
    }

    /// The outcome as rendered in a scenario, quotes included.
    #[must_use]
    pub fn rendered(self) -> Vec<u8> {
        format!("\"{}\"", self.name()).into_bytes()
    }
}

/// A block: the root in slot 0, else the `index`-th block of its slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlockId {
    /// The block's slot, its depth.
    pub slot: usize,
    /// The block's index in its slot.
    pub index: usize,
}

impl BlockId {
    /// The root, in slot 0.
    pub const ROOT: Self = Self { slot: 0, index: 0 };
}

impl fmt::Display for BlockId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if *self == Self::ROOT {
            f.write_str("0")
        } else {
            write!(f, "{}{}", self.slot, char::from(b"abcde"[self.index]))
        }
    }
}

/// A block and the block it builds on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Block {
    /// The block.
    pub id: BlockId,
    /// The block it builds on, in an earlier slot.
    pub parent: BlockId,
}

/// A depth: its slot's blocks and how the slot resolved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Depth {
    /// The depth's slot.
    pub slot: usize,
    /// How the slot resolved.
    pub outcome: Outcome,
    /// The certified block, if `outcome` certifies one.
    pub certified: Option<BlockId>,
    /// The slot's blocks, in index order.
    pub blocks: Vec<Block>,
}

/// A block tree, depth by depth.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tree {
    /// The depths, slot 1 first.
    pub depths: Vec<Depth>,
}

impl Tree {
    /// Most depths in a tree.
    pub const MAX_DEPTH: usize = 8;
    /// Most blocks in a depth.
    pub const MAX_BLOCKS: usize = 5;

    /// Build a tree whose `d`-th depth has the `d`-th outcome and block count of `shape`.
    ///
    /// Each certified block is drawn from its depth and builds on the chain so far.  Every
    /// other block builds on a block drawn from the root and the earlier depths.  If the
    /// deepest certified block would only be notar fallback certified, it is fast finalized
    /// or finalized instead, so the whole chain is finalized.
    ///
    /// # Panics
    /// If `shape` has more than [`Self::MAX_DEPTH`] depths, a depth has more than
    /// [`Self::MAX_BLOCKS`] blocks, or a certified depth has none.
    pub fn build<S: RawSampler>(sampler: &mut S, shape: &[(Outcome, usize)]) -> Self {
        assert!(shape.len() <= Self::MAX_DEPTH, "{} depths", shape.len());
        let mut outcomes: Vec<Outcome> = shape.iter().map(|&(outcome, _)| outcome).collect();
        if let Some(tip) =
            outcomes.iter_mut().rev().find(|outcome| outcome.certifies()).filter(|tip| **tip == Outcome::NotarFallback)
        {
            *tip = if sampler.sample().is_multiple_of(2) { Outcome::FastFinalize } else { Outcome::Finalize };
        }

        let mut tip = BlockId::ROOT;
        let mut earlier = Vec::from([BlockId::ROOT]);
        let mut depths = Vec::new();
        for (depth, (&(_, count), outcome)) in shape.iter().zip(outcomes).enumerate() {
            let slot = depth + 1;
            assert!(count <= Self::MAX_BLOCKS, "slot {slot}: {count} blocks");
            assert!(count > 0 || !outcome.certifies(), "slot {slot}: {} without blocks", outcome.name());
            let certified = outcome.certifies().then(|| BlockId { slot, index: sampler.sample() % count });
            let mut blocks = Vec::new();
            for index in 0..count {
                let id = BlockId { slot, index };
                let parent = if Some(id) == certified { tip } else { earlier[sampler.sample() % earlier.len()] };
                blocks.push(Block { id, parent });
            }
            tip = certified.unwrap_or(tip);
            earlier.extend(blocks.iter().map(|block| block.id));
            depths.push(Depth { slot, outcome, certified, blocks });
        }
        Self { depths }
    }

    /// The finalized chain: the root, then each certified block.
    pub fn chain(&self) -> impl Iterator<Item = BlockId> + '_ {
        iter::once(BlockId::ROOT).chain(self.depths.iter().filter_map(|depth| depth.certified))
    }

    /// Draw the blocks building on `parent`, and theirs, each line starting with `prefix`.
    fn draw_children(&self, f: &mut fmt::Formatter<'_>, parent: BlockId, prefix: &mut String) -> fmt::Result {
        let children: Vec<(&Depth, &Block)> = self
            .depths
            .iter()
            .flat_map(|depth| depth.blocks.iter().filter(move |block| block.parent == parent).map(move |block| (depth, block)))
            .collect();
        for (i, (depth, block)) in children.iter().enumerate() {
            let last = i + 1 == children.len();
            write!(f, "{prefix}{}{}", if last { "└── " } else { "├── " }, block.id)?;
            if Some(block.id) == depth.certified {
                write!(f, " ● {}", depth.outcome.name())?;
            } else if !depth.outcome.certifies() {
                write!(f, " ✗ {}", depth.outcome.name())?;
            }
            writeln!(f)?;
            let len = prefix.len();
            prefix.push_str(if last { "    " } else { "│   " });
            self.draw_children(f, block.id, prefix)?;
            prefix.truncate(len);
        }
        Ok(())
    }
}

/// Draw the tree from the root, marking certified blocks `●` and the blocks of skipped slots
/// `✗`, then list each depth's outcome and blocks, and the finalized chain.
impl fmt::Display for Tree {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "{}", BlockId::ROOT)?;
        self.draw_children(f, BlockId::ROOT, &mut String::new())?;
        writeln!(f)?;
        for depth in &self.depths {
            write!(f, "slot {}  {:<14}", depth.slot, depth.outcome.name())?;
            if depth.blocks.is_empty() {
                write!(f, "  no blocks")?;
            }
            for block in &depth.blocks {
                write!(f, "  {}{}", block.id, if Some(block.id) == depth.certified { "●" } else { "" })?;
            }
            writeln!(f)?;
        }
        write!(f, "chain   ")?;
        for (i, id) in self.chain().enumerate() {
            write!(f, "{}{id}", if i == 0 { "" } else { " → " })?;
        }
        writeln!(f)
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
    use super::{Outcome, Tree};
    use alloc::format;
    use alloc::vec::{IntoIter, Vec};
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

    /// Fix a generated scenario into a block tree with the depths, outcomes and block counts
    /// it was generated with (see [`Tree::build`]), and return the tree.
    pub fn fix<S, G>(scenario: &mut nonterminal_start, sampler: &mut S, generators: &mut G) -> Tree
    where
        S: RawSampler,
        nonterminal_slot: Generated<S, G>,
        nonterminal_certified: Generated<S, G>,
        nonterminal_block_id: Generated<S, G>,
    {
        let Ok(ControlFlow::Continue(Survey(shape))) = Survey(Vec::new()).visit_mut(scenario, 0);
        let tree = Tree::build(sampler, &shape);
        let place = Place { sampler, generators, fields: fields(&tree).into_iter() };
        let Ok(ControlFlow::Continue(place)) = place.visit_mut(scenario, 0);
        debug_assert!(place.fields.as_slice().is_empty(), "as many fields as placed");
        tree
    }

    /// The fields [`Place`] puts in a scenario for `tree`, rendered, in order: each depth's
    /// slot, then, if certified, its outcome and certified block, then its blocks' ids and
    /// parents.
    fn fields(tree: &Tree) -> Vec<Vec<u8>> {
        let mut fields = Vec::new();
        for depth in &tree.depths {
            fields.push(format!("{}", depth.slot).into_bytes());
            if let Some(certified) = depth.certified {
                fields.push(depth.outcome.rendered());
                fields.push(format!("\"{certified}\"").into_bytes());
            }
            for block in &depth.blocks {
                fields.push(format!("\"{}\"", block.id).into_bytes());
                fields.push(format!("\"{}\"", block.parent).into_bytes());
            }
        }
        fields
    }

    /// Generate `N`s until one renders to `expected`.
    fn generate_rendering<N, S, G>(sampler: &mut S, generators: &mut G, expected: &[u8]) -> N
    where
        N: Generated<S, G>,
        for<'program> N: Node<Type<'program> = Type<'program>> + 'program,
        for<'program> Type<'program>: From<&'program N> + AsNodeRef<N>,
    {
        loop {
            let candidate = N::generate(sampler, generators, 0);
            if render(&candidate) == expected {
                return candidate;
            }
        }
    }

    /// Visitor which collects each depth's outcome and block count.
    struct Survey(Vec<(Outcome, usize)>);

    impl Survey {
        /// Start a depth with the outcome `rendered`.
        fn start(&mut self, rendered: &[u8]) {
            let outcome = Outcome::ALL.into_iter().find(|outcome| outcome.rendered() == rendered);
            self.0.push((outcome.expect("the grammar renders an outcome"), 0));
        }
    }

    impl<T> VisitorMut<T> for Survey
    where
        T: VisitableChildrenMut<T>
            + AsNodeMut<nonterminal_certified>
            + AsNodeMut<nonterminal_skipped>
            + AsNodeMut<nonterminal_block>,
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
            if let Some(outcome) = visited.downcast_mut::<nonterminal_certified>() {
                self.start(&render(&*outcome));
            } else if let Some(outcome) = visited.downcast_mut::<nonterminal_skipped>() {
                self.start(&render(&*outcome));
            } else if visited.downcast_mut::<nonterminal_block>().is_some() {
                self.0.last_mut().expect("a depth's outcome comes before its blocks").1 += 1;
            } else {
                return visited.visit_each_mut(self);
            }
            Ok(ControlFlow::Continue(self))
        }
    }

    /// Visitor which puts the given fields in place, in order.
    struct Place<'a, S, G> {
        sampler: &'a mut S,
        generators: &'a mut G,
        fields: IntoIter<Vec<u8>>,
    }

    impl<S, G> Place<'_, S, G>
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
            let field = self.fields.next().expect("a field for every slot, certified outcome and block id");
            *node = generate_rendering(self.sampler, self.generators, &field);
        }
    }

    impl<S, G, T> VisitorMut<T> for Place<'_, S, G>
    where
        S: RawSampler,
        nonterminal_slot: Generated<S, G>,
        nonterminal_certified: Generated<S, G>,
        nonterminal_block_id: Generated<S, G>,
        T: VisitableChildrenMut<T>
            + AsNodeMut<nonterminal_slot>
            + AsNodeMut<nonterminal_certified>
            + AsNodeMut<nonterminal_block_id>,
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
            if let Some(slot) = visited.downcast_mut::<nonterminal_slot>() {
                self.place(slot);
            } else if let Some(outcome) = visited.downcast_mut::<nonterminal_certified>() {
                self.place(outcome);
            } else if let Some(id) = visited.downcast_mut::<nonterminal_block_id>() {
                self.place(id);
            } else {
                return visited.visit_each_mut(self);
            }
            Ok(ControlFlow::Continue(self))
        }
    }

    #[cfg(test)]
    mod tests {
        use super::super::{BlockId, Depth};
        use super::*;
        use alloc::string::String;
        use fandango::tuple_list::tuple_list;
        use fandango_runtime::operators::DepthLimiter;
        use rand::SeedableRng;
        use rand::rngs::StdRng;

        /// The scenario `tree` describes, written out independently of the grammar.
        fn json(tree: &Tree) -> String {
            let depths: Vec<String> = tree
                .depths
                .iter()
                .map(|depth| {
                    let blocks: Vec<String> = depth
                        .blocks
                        .iter()
                        .map(|block| format!("{{\"id\": \"{}\", \"parent\": \"{}\"}}", block.id, block.parent))
                        .collect();
                    let certified = depth.certified.map_or(String::new(), |id| format!(", \"block\": \"{id}\""));
                    format!(
                        "{{\"slot\": {}, \"outcome\": \"{}\"{certified}, \"blocks\": [{}]}}",
                        depth.slot,
                        depth.outcome.name(),
                        blocks.join(", ")
                    )
                })
                .collect();
            format!("{{\"slots\": [{}]}}\n", depths.join(", "))
        }

        /// Check that every depth resolves and that the certified blocks form a chain ending
        /// in a finalized block.
        fn check(tree: &Tree) {
            assert!((1..=Tree::MAX_DEPTH).contains(&tree.depths.len()));
            let mut tip = BlockId::ROOT;
            let mut earlier = Vec::from([BlockId::ROOT]);
            for (i, Depth { slot, outcome, certified, blocks }) in tree.depths.iter().enumerate() {
                assert_eq!(*slot, i + 1);
                assert!(blocks.len() <= Tree::MAX_BLOCKS);
                for (index, block) in blocks.iter().enumerate() {
                    assert_eq!(block.id, BlockId { slot: *slot, index });
                    assert!(earlier.contains(&block.parent), "{} builds on {}", block.id, block.parent);
                }
                assert_eq!(certified.is_some(), outcome.certifies(), "slot {slot}");
                if let Some(certified) = certified {
                    let block = blocks.iter().find(|block| block.id == *certified).expect("certified block in its slot");
                    assert_eq!(block.parent, tip, "{certified} builds on the chain");
                    tip = *certified;
                }
                earlier.extend(blocks.iter().map(|block| block.id));
            }
            if let Some(last) = tree.depths.iter().rev().find(|depth| depth.certified.is_some()) {
                assert!(matches!(last.outcome, Outcome::FastFinalize | Outcome::Finalize), "chain ends {:?}", last.outcome);
            }
            assert_eq!(tree.chain().last(), Some(tip));
        }

        #[test]
        fn fixed_scenarios_are_trees_with_a_finalized_chain() {
            let mut sampler = StdRng::seed_from_u64(1);
            let mut generators = tuple_list!(DepthLimiter::new(STRUCTURE.inner(), 40));
            let (mut outcomes, mut deepest, mut full, mut empty, mut forks) = (Vec::new(), false, false, false, false);
            for _ in 0..500 {
                let mut scenario = nonterminal_start::generate(&mut sampler, &mut generators, 0);
                let tree = fix(&mut scenario, &mut sampler, &mut generators);
                assert_eq!(String::from_utf8(render(&scenario)).unwrap(), json(&tree));
                check(&tree);
                deepest |= tree.depths.len() == Tree::MAX_DEPTH;
                for depth in &tree.depths {
                    outcomes.push(depth.outcome);
                    full |= depth.blocks.len() == Tree::MAX_BLOCKS;
                    empty |= depth.blocks.is_empty();
                    forks |= depth.blocks.iter().any(|block| Some(block.id) != depth.certified && block.parent != BlockId::ROOT);
                }
            }
            assert!(Outcome::ALL.iter().all(|outcome| outcomes.contains(outcome)), "{outcomes:?}");
            assert!(deepest && full && empty && forks);
        }
    }
}

pub use defs::*;

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;

    #[test]
    fn draws_the_tree_and_its_chain() {
        let block = |slot, index, parent| Block { id: BlockId { slot, index }, parent };
        let tree = Tree {
            depths: Vec::from([
                Depth {
                    slot: 1,
                    outcome: Outcome::NotarFallback,
                    certified: Some(BlockId { slot: 1, index: 0 }),
                    blocks: Vec::from([block(1, 0, BlockId::ROOT), block(1, 1, BlockId::ROOT)]),
                },
                Depth {
                    slot: 2,
                    outcome: Outcome::SkipFallback,
                    certified: None,
                    blocks: Vec::from([block(2, 0, BlockId { slot: 1, index: 0 })]),
                },
                Depth { slot: 3, outcome: Outcome::Skip, certified: None, blocks: Vec::new() },
                Depth {
                    slot: 4,
                    outcome: Outcome::FastFinalize,
                    certified: Some(BlockId { slot: 4, index: 1 }),
                    blocks: Vec::from([
                        block(4, 0, BlockId { slot: 2, index: 0 }),
                        block(4, 1, BlockId { slot: 1, index: 0 }),
                    ]),
                },
            ]),
        };
        let expected = "\
0
├── 1a ● NOTAR_FALLBACK
│   ├── 2a ✗ SKIP_FALLBACK
│   │   └── 4a
│   └── 4b ● FAST_FINALIZE
└── 1b

slot 1  NOTAR_FALLBACK  1a●  1b
slot 2  SKIP_FALLBACK   2a
slot 3  SKIP            no blocks
slot 4  FAST_FINALIZE   4a  4b●
chain   0 → 1a → 4b
";
        assert_eq!(tree.to_string(), expected);
    }
}
