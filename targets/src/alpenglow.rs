//! Target for the Alpenglow votor scenario grammar in `grammars/alpenglow.fan`.
//!
//! Each generated scenario is a JSON input for firedancer's `fuzz_ag_votor` harness
//! (`src/choreo/votor/fuzz_ag_votor.c`): the blocks, the staked nodes (the votor under
//! test signs as the first), and the actions to run.
//!
//! The blocks are hardcoded: sixteen slots, with the root `0` in slot 0 and two competing
//! blocks `Na` and `Nb` in each slot `N` from 1 to 15. They form two forks off the root:
//! `Na` builds on `(N-1)a` and `Nb` on `(N-1)b`. Actions name one of these 30 non-root
//! blocks.
//!
//! The nodes are hardcoded: twenty validators sharing a total stake of 10,000,000.
//! Nodes `0`-`9` are honest (620,000 each, 62% total), nodes `10`-`14` are maybe absent
//! (380,000 each, 19% total) and nodes `15`-`19` are maybe Byzantine (380,000 each, 19%
//! total).
//!
//! The actions run in fifteen turns, one per slot from 1 to 15. Each turn holds twenty
//! turn votes (any `VOTE_*`, `VOTE_ABSENT` included), each of which may be preceded by one
//! free action of any kind and followed by a duplicate vote. Free votes come from the maybe
//! absent and maybe Byzantine nodes only. The grammar leaves the node, target slot and
//! duplicate of each turn vote to [`ConstraintFixer`].
//!
//! [`ConstraintVisitor`] checks the constraints on the actions:
//! 1. Every node casts a turn vote in every turn.
//! 2. The honest nodes' turn votes for a slot are all the same vote.
//!
//! [`ConstraintFixer`] fixes each turn vote:
//! - The `i`-th turn vote of a turn is cast by node `i`.
//! - It targets its turn's slot, except with a [`DELAY_PERCENT`] chance, when it targets
//!   another slot, past or future, instead: the vote is delayed or early.
//! - An honest node casts the slot's honest vote: `VOTE_SKIP` with a
//!   [`HONEST_SKIP_PERCENT`] chance, else `VOTE_NOTAR`, for one of the slot's blocks, drawn
//!   once per slot. The other nodes' votes stay as generated.
//! - With a [`DUPLICATE_PERCENT`] chance, it is followed by an exact duplicate of one of
//!   the node's turn votes so far, from this turn or an earlier one.

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
    use alloc::collections::VecDeque;
    use alloc::vec;
    use alloc::vec::Vec;
    use core::convert::Infallible;
    use core::ops::ControlFlow;
    use fandango::Fandango;
    use fandango::generation::{Generated, RawSampler};
    use fandango::typing::{
        AsNodeMut, AsNodeRef, ChildAccessor, Downcast, DowncastMut, Node, Opaque, OpaqueMut,
    };
    use fandango::visitor::write::WriteVisitor;
    use fandango::visitor::{
        VisitMutResult, VisitResult, VisitableChildren, VisitableChildrenMut, Visitor, VisitorMut,
    };
    use fandango_runtime::measurement::Violations;
    use fandango_runtime::operators::{Checker, Fixer};
    use num_rational::Ratio;

    /// Base for the Alpenglow grammar stored in `alpenglow.fan`.
    #[derive(Fandango)]
    #[fandango(grammar = "grammars/alpenglow.fan", parse = false)]
    pub struct Alpenglow(Infallible);

    const NODE_COUNT: usize = 20;
    const HONEST_COUNT: usize = 10; // nodes 0 to 9
    const SLOT_COUNT: usize = 15; // slots 1 to 15, one turn each, the root slot 0 takes no votes

    const VOTE_NOTAR: &[u8] = b"\"VOTE_NOTAR\"";
    const VOTE_SKIP: &[u8] = b"\"VOTE_SKIP\"";

    /// Chance, in percent, that a turn vote targets another slot than its turn's.
    pub const DELAY_PERCENT: usize = 10;

    /// Chance, in percent, that a turn vote is followed by a duplicate of one of the node's
    /// turn votes so far.
    pub const DUPLICATE_PERCENT: usize = 10;

    /// Chance, in percent, that the honest nodes vote skip for a slot rather than notar.
    pub const HONEST_SKIP_PERCENT: usize = 33;

    /// The vote every honest node casts for a slot.
    #[derive(Clone, Copy, Debug)]
    struct HonestVote {
        kind: &'static [u8], // rendered vote kind
        fork: u8,            // b'a' or b'b'
    }

    /// Render a node to the text it generates.
    fn render<'program, N>(node: &'program N) -> Vec<u8>
    where
        N: Node<Type<'program> = Type<'program>>,
        Type<'program>: From<&'program N> + AsNodeRef<N>,
    {
        let Ok(ControlFlow::Continue(writer)) = WriteVisitor::new(Vec::new()).visit(node, 0);
        writer.output()
    }

    /// Parse the number in a rendered node id or block id, ignoring other characters.
    fn number(bytes: &[u8]) -> Option<usize> {
        let mut digits = bytes.iter().filter(|byte| byte.is_ascii_digit()).peekable();
        digits.peek()?;
        digits.try_fold(0usize, |number, digit| {
            number.checked_mul(10)?.checked_add(usize::from(digit - b'0'))
        })
    }

    /// Return the fork of a rendered block id such as `"7a"`.
    fn fork(block_id: &[u8]) -> Option<u8> {
        block_id.iter().rev().copied().find(u8::is_ascii_alphabetic)
    }

    /// Visitor which collects the violations of the constraints on the actions.
    #[derive(Clone, Debug, Default)]
    pub struct ConstraintVisitor {
        turn_votes: usize,
        voted: [[bool; NODE_COUNT]; SLOT_COUNT],
        honest: [Option<(Vec<u8>, Vec<u8>)>; SLOT_COUNT], // first honest turn vote per target slot: rendered block id and kind
        split: [bool; SLOT_COUNT], // an honest turn vote for the slot differs from it
    }

    impl ConstraintVisitor {
        /// Return the `(node, slot)` pairs for which the node cast no turn vote during the
        /// slot's turn (constraint 1).
        #[must_use]
        pub fn missing_votes(&self) -> Vec<(usize, usize)> {
            let mut missing = Vec::new();
            for (turn, voted) in self.voted.iter().enumerate() {
                for (node, &voted) in voted.iter().enumerate() {
                    if !voted {
                        missing.push((node, turn + 1));
                    }
                }
            }
            missing
        }

        /// Return the slots for which the honest nodes' turn votes differ (constraint 2).
        #[must_use]
        pub fn split_honest_votes(&self) -> Vec<usize> {
            (1..=SLOT_COUNT).filter(|&slot| self.split[slot - 1]).collect()
        }

        /// Return the number of constraint violations.
        #[must_use]
        pub fn violation_count(&self) -> usize {
            self.missing_votes().len() + self.split_honest_votes().len()
        }
    }

    impl Checker for ConstraintVisitor {
        fn violations(self) -> Violations {
            let checked = NODE_COUNT * SLOT_COUNT + SLOT_COUNT;
            let violated = self.violation_count();
            Violations::new(
                Ratio::new(checked - violated, checked),
                vec![VecDeque::new(); violated],
            )
        }
    }

    impl<T> Visitor<T> for ConstraintVisitor
    where
        T: VisitableChildren<T> + AsNodeRef<nonterminal_turn_vote>,
    {
        type Continue = Self;
        type Break = Infallible;
        type Error = Infallible;

        fn visit<'program, N>(mut self, node: &'program N, _idx: usize) -> VisitResult<Self, T>
        where
            N: Node<Type<'program> = T>,
            T: From<&'program N> + AsNodeRef<N>,
        {
            let visited = node.opaque();
            if let Some(turn_vote) = visited.downcast::<nonterminal_turn_vote>() {
                let turn = self.turn_votes / NODE_COUNT;
                self.turn_votes += 1;
                let (_, vote, _) = turn_vote.child().children();
                let (_, node_id, ..) = vote.child().children();
                if let Some(node) = number(&render(node_id))
                    && node < NODE_COUNT
                    && turn < SLOT_COUNT
                {
                    self.voted[turn][node] = true;
                    if node < HONEST_COUNT {
                        let (_, _, _, block_id, _, vote_kind, _) = vote.child().children();
                        let block_id = render(block_id);
                        if let Some(slot) = number(&block_id)
                            && (1..=SLOT_COUNT).contains(&slot)
                        {
                            let cast = (block_id, render(vote_kind));
                            match &self.honest[slot - 1] {
                                None => self.honest[slot - 1] = Some(cast),
                                Some(first) => self.split[slot - 1] |= *first != cast,
                            }
                        }
                    }
                }
                return Ok(ControlFlow::Continue(self)); // free actions and duplicates are no turn votes
            }
            visited.visit_each(self)
        }
    }

    /// Visitor which fixes the node, target slot, honest vote and duplicate of each turn
    /// vote.
    pub struct ConstraintFixer<'a, S, G> {
        sampler: &'a mut S,
        generators: &'a mut G,
        turn_votes: usize,
        cast: [Vec<nonterminal_vote>; NODE_COUNT], // each node's turn votes so far
        honest: [Option<HonestVote>; SLOT_COUNT],  // each slot's honest vote, once drawn
    }

    impl<'a, S, G> ConstraintFixer<'a, S, G> {
        /// Create a fixer drawing from `sampler` and `generators`, as the scenario's
        /// generation does.
        pub fn new(sampler: &'a mut S, generators: &'a mut G) -> Self {
            Self {
                sampler,
                generators,
                turn_votes: 0,
                cast: core::array::from_fn(|_| Vec::new()),
                honest: [None; SLOT_COUNT],
            }
        }
    }

    impl<'a, S, G> Fixer<'a, S, G> for ConstraintFixer<'a, S, G> {
        fn new(sampler: &'a mut S, generators: &'a mut G) -> Self {
            Self::new(sampler, generators)
        }
    }

    impl<S, G> ConstraintFixer<'_, S, G>
    where
        S: RawSampler,
        nonterminal_node_id: Generated<S, G>,
        nonterminal_block_id: Generated<S, G>,
        nonterminal_vote_kind: Generated<S, G>,
    {
        /// Return `true` with a `percent` chance.
        fn chance(&mut self, percent: usize) -> bool {
            self.sampler.sample() % 100 < percent
        }

        /// Generate node ids until one names `node`.
        fn generate_node_id(&mut self, node: usize) -> nonterminal_node_id {
            loop {
                let candidate = nonterminal_node_id::generate(self.sampler, self.generators, 0);
                if number(&render(&candidate)) == Some(node) {
                    return candidate;
                }
            }
        }

        /// Generate block ids until one names a block in `slot`, of `fork` if given.
        fn generate_block_id(&mut self, slot: usize, fork: Option<u8>) -> nonterminal_block_id {
            loop {
                let candidate = nonterminal_block_id::generate(self.sampler, self.generators, 0);
                let rendered = render(&candidate);
                if number(&rendered) == Some(slot) && fork.is_none_or(|f| self::fork(&rendered) == Some(f)) {
                    return candidate;
                }
            }
        }

        /// Generate vote kinds until one renders to `kind`.
        fn generate_vote_kind(&mut self, kind: &[u8]) -> nonterminal_vote_kind {
            loop {
                let candidate = nonterminal_vote_kind::generate(self.sampler, self.generators, 0);
                if render(&candidate) == kind {
                    return candidate;
                }
            }
        }

        /// Return the vote every honest node casts for `slot`, drawing it the first time.
        fn honest_vote(&mut self, slot: usize) -> HonestVote {
            if let Some(vote) = self.honest[slot - 1] {
                return vote;
            }
            let kind = if self.chance(HONEST_SKIP_PERCENT) { VOTE_SKIP } else { VOTE_NOTAR };
            let fork = if self.chance(50) { b'a' } else { b'b' };
            let vote = HonestVote { kind, fork };
            self.honest[slot - 1] = Some(vote);
            vote
        }

        fn fix_turn_vote(&mut self, turn_vote: &mut nonterminal_turn_vote) {
            let node = self.turn_votes % NODE_COUNT;
            let turn_slot = self.turn_votes / NODE_COUNT + 1;
            self.turn_votes += 1;

            let slot = if self.chance(DELAY_PERCENT) {
                // Another slot than the turn's, past or future
                let other = self.sampler.sample() % (SLOT_COUNT - 1) + 1;
                if other >= turn_slot { other + 1 } else { other }
            } else {
                turn_slot
            };

            let (_, vote, duplicate) = turn_vote.child_mut().children_mut();
            let (_, node_id, _, block_id, _, vote_kind, _) = vote.child_mut().children_mut();
            *node_id = self.generate_node_id(node);
            if node < HONEST_COUNT {
                let honest = self.honest_vote(slot);
                *block_id = self.generate_block_id(slot, Some(honest.fork));
                *vote_kind = self.generate_vote_kind(honest.kind);
            } else {
                *block_id = self.generate_block_id(slot, None);
            }
            self.cast[node].push(vote.clone());

            *duplicate = if self.chance(DUPLICATE_PERCENT) {
                let cast = &self.cast[node];
                let original = cast[self.sampler.sample() % cast.len()].clone();
                nonterminal_maybe_duplicate::new(nonterminal_maybe_duplicate_0::from_1th(
                    nonterminal_maybe_duplicate_0_1::new(Default::default(), original),
                ))
            } else {
                nonterminal_maybe_duplicate::new(nonterminal_maybe_duplicate_0::from_0th(
                    Default::default(),
                ))
            };
        }
    }

    impl<S, G, T> VisitorMut<T> for ConstraintFixer<'_, S, G>
    where
        S: RawSampler,
        nonterminal_node_id: Generated<S, G>,
        nonterminal_block_id: Generated<S, G>,
        nonterminal_vote_kind: Generated<S, G>,
        T: VisitableChildrenMut<T> + AsNodeMut<nonterminal_turn_vote>,
    {
        type Continue = Self;
        type Break = Infallible;
        type Error = Infallible;

        fn visit_mut<'program, N>(
            mut self,
            node: &'program mut N,
            _idx: usize,
        ) -> VisitMutResult<Self, T>
        where
            N: Node<TypeMut<'program> = T>,
            T: From<&'program mut N> + AsNodeMut<N>,
        {
            let mut visited = node.opaque_mut();
            if let Some(turn_vote) = visited.downcast_mut::<nonterminal_turn_vote>() {
                self.fix_turn_vote(turn_vote);
                return Ok(ControlFlow::Continue(self)); // free actions stay as generated
            }
            visited.visit_each_mut(self)
        }
    }
}

pub use defs::*;
