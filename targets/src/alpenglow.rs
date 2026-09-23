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
//! The actions run slot by slot, from 1 to 15. In each slot, every node in turn casts a
//! vote action (any `VOTE_*`, `VOTE_ABSENT` included) for one of the slot's two blocks,
//! and each of these votes may be preceded by one free action of any kind.
//!
//! [`ConstraintVisitor`] checks the constraints on the actions:
//! 1. Every node casts at least one vote action in every slot from 1 to 15. The grammar
//!    guarantees this one.
//!
//! There is no `ConstraintFixer`; callers reject scenarios that violate a constraint.

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
    use fandango::typing::{AsNodeRef, Node};
    use fandango::visitor::write::WriteVisitor;
    use fandango::visitor::{VisitResult, VisitableChildren, Visitor};
    use fandango_runtime::measurement::Violations;
    use fandango_runtime::operators::Checker;
    use num_rational::Ratio;

    /// Base for the Alpenglow grammar stored in `alpenglow.fan`.
    #[derive(Fandango)]
    #[fandango(grammar = "grammars/alpenglow.fan", parse = false)]
    pub struct Alpenglow(Infallible);

    const NODE_COUNT: usize = 20;
    const SLOT_COUNT: usize = 15; // slots 1 to 15, the root slot 0 takes no votes

    /// Visitor which collects the violations of the constraints on the actions.
    ///
    /// Visit the whole scenario: the visitor reads the actions from its rendered JSON, as
    /// the node and slot of most votes are literal text in the grammar.
    #[derive(Clone, Debug, Default)]
    pub struct ConstraintVisitor {
        voted: [[bool; NODE_COUNT]; SLOT_COUNT],
    }

    impl ConstraintVisitor {
        /// Return the `(node, slot)` pairs for which the node cast no vote action in the
        /// slot.
        #[must_use]
        pub fn missing_votes(&self) -> Vec<(usize, usize)> {
            let mut missing = Vec::new();
            for (slot_index, voted) in self.voted.iter().enumerate() {
                for (node, &voted) in voted.iter().enumerate() {
                    if !voted {
                        missing.push((node, slot_index + 1));
                    }
                }
            }
            missing
        }

        /// Record the vote actions in `json`, each of the form
        /// `{"node_id": 3, "block_id": "7a", "action": "VOTE_SKIP"}`.
        fn record_votes(&mut self, json: &str) {
            for object in json.split(r#"{"node_id": "#).skip(1) {
                let object = object.split('}').next().unwrap_or_default();
                // Entries of the nodes list name no block
                let Some((node, rest)) = object.split_once(r#", "block_id": ""#) else {
                    continue;
                };
                let slot = rest.split(|c: char| !c.is_ascii_digit()).next().unwrap_or_default();
                if let (Ok(node), Ok(slot)) = (node.parse::<usize>(), slot.parse::<usize>())
                    && node < NODE_COUNT
                    && (1..=SLOT_COUNT).contains(&slot)
                    && object.contains(r#""action": "VOTE_"#)
                {
                    self.voted[slot - 1][node] = true;
                }
            }
        }
    }

    impl Checker for ConstraintVisitor {
        fn violations(self) -> Violations {
            let checked = NODE_COUNT * SLOT_COUNT;
            let missing = self.missing_votes().len();
            Violations::new(
                Ratio::new(checked - missing, checked),
                vec![VecDeque::new(); missing],
            )
        }
    }

    impl<T> Visitor<T> for ConstraintVisitor
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
            let bytes = WriteVisitor::new(Vec::new())
                .visit(node, 0)?
                .continue_value()
                .unwrap()
                .output();
            self.record_votes(core::str::from_utf8(&bytes).unwrap_or_default());
            Ok(ControlFlow::Continue(self))
        }
    }
}

pub use defs::*;
