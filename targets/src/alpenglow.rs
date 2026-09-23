//! Target for the Alpenglow votor scenario grammar in `grammars/alpenglow.fan`.
//!
//! Each generated scenario is a JSON list of actions for firedancer's `fuzz_ag_votor`
//! harness (`src/choreo/votor/fuzz_ag_votor.c`), which fixes the cluster and one block per
//! slot, 1 to 15, each building on the one before. The harness runs the actions in list
//! order.
//!
//! The grammar gives every slot a group of actions: its certificates, either `FINALIZE` and
//! `NOTAR` or `FAST_FINALIZE`, and its replay outcome. [`fix`] names each group's slot and
//! actions, and shuffles all actions, so each slot gets exactly one certificate choice and
//! one replay outcome, `REPLAY_COMPLETED` or `REPLAY_DEAD`, in any order. As every slot's
//! block is finalized and the blocks form a chain, the finalized blocks form a chain.

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
    use alloc::format;
    use alloc::vec::{IntoIter, Vec};
    use core::convert::Infallible;
    use core::ops::ControlFlow;
    use fandango::Fandango;
    use fandango::generation::{Generated, RawSampler};
    use fandango::typing::{AsNodeMut, AsNodeRef, ChildAccessor, DowncastMut, Node, OpaqueMut};
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

    /// Fix a generated scenario: the `i`-th slot group gets slot `i`'s certificates and a
    /// random replay outcome, then all actions are shuffled.
    pub fn fix<S, G>(scenario: &mut nonterminal_start, sampler: &mut S, generators: &mut G)
    where
        S: RawSampler,
        nonterminal_slot: Generated<S, G>,
        nonterminal_kind: Generated<S, G>,
    {
        let assign = Assign { sampler: &mut *sampler, generators, slot: 0, actions: Vec::new() };
        let Ok(ControlFlow::Continue(Assign { mut actions, .. })) = assign.visit_mut(scenario, 0);
        for i in (1..actions.len()).rev() {
            actions.swap(i, sampler.sample() % (i + 1));
        }
        let Ok(ControlFlow::Continue(_)) = Place(actions.into_iter()).visit_mut(scenario, 0);
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

    /// Visitor which names each slot group's slot and actions, and collects them.
    struct Assign<'a, S, G> {
        sampler: &'a mut S,
        generators: &'a mut G,
        slot: usize,
        actions: Vec<nonterminal_action>,
    }

    impl<S, G> Assign<'_, S, G>
    where
        S: RawSampler,
        nonterminal_slot: Generated<S, G>,
        nonterminal_kind: Generated<S, G>,
    {
        /// Make `action` the slot's `kind` (rendered, quotes included) and collect it.
        fn assign(&mut self, action: &mut nonterminal_action, kind: &[u8]) {
            let slot = format!("{}", self.slot).into_bytes();
            let (_, slot_node, _, kind_node, _) = action.child_mut().children_mut();
            *slot_node = generate_rendering(self.sampler, self.generators, &slot);
            *kind_node = generate_rendering(self.sampler, self.generators, kind);
            self.actions.push(action.clone());
        }
    }

    impl<S, G, T> VisitorMut<T> for Assign<'_, S, G>
    where
        S: RawSampler,
        nonterminal_slot: Generated<S, G>,
        nonterminal_kind: Generated<S, G>,
        T: VisitableChildrenMut<T> + AsNodeMut<nonterminal_slot_group>,
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
            let Some(group) = visited.downcast_mut::<nonterminal_slot_group>() else {
                return visited.visit_each_mut(self);
            };
            self.slot += 1;
            let replay: &[u8] = if self.sampler.sample() % 2 == 0 { b"\"REPLAY_COMPLETED\"" } else { b"\"REPLAY_DEAD\"" };
            match group.child_mut() {
                nonterminal_slot_group_0::variant_0(slow) => {
                    let (finalize, _, notar, _, replayed) = slow.children_mut();
                    self.assign(finalize, b"\"FINALIZE\"");
                    self.assign(notar, b"\"NOTAR\"");
                    self.assign(replayed, replay);
                }
                nonterminal_slot_group_0::variant_1(fast) => {
                    let (fast_finalize, _, replayed) = fast.children_mut();
                    self.assign(fast_finalize, b"\"FAST_FINALIZE\"");
                    self.assign(replayed, replay);
                }
            }
            Ok(ControlFlow::Continue(self))
        }
    }

    /// Visitor which puts the given actions in place, in order.
    struct Place(IntoIter<nonterminal_action>);

    impl<T> VisitorMut<T> for Place
    where
        T: VisitableChildrenMut<T> + AsNodeMut<nonterminal_action>,
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
            if let Some(action) = visited.downcast_mut::<nonterminal_action>() {
                *action = self.0.next().expect("as many actions as assigned");
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

        #[test]
        fn every_slot_gets_one_certificate_choice_and_one_replay_outcome() {
            let mut sampler = StdRng::seed_from_u64(1);
            let mut generators = tuple_list!(DepthLimiter::new(STRUCTURE.inner(), 40));
            let mut shuffled = false;
            for _ in 0..100 {
                let mut scenario = nonterminal_start::generate(&mut sampler, &mut generators, 0);
                fix(&mut scenario, &mut sampler, &mut generators);
                let json = String::from_utf8(render(&scenario)).unwrap();
                let actions: Vec<(usize, &str)> = json
                    .split("{\"slot\": ")
                    .skip(1)
                    .map(|action| {
                        let (slot, rest) = action.split_once(", \"action\": \"").unwrap();
                        (slot.parse().unwrap(), rest.split('"').next().unwrap())
                    })
                    .collect();
                for slot in 1..=15 {
                    let mut kinds: Vec<&str> = actions.iter().filter(|a| a.0 == slot).map(|a| a.1).collect();
                    kinds.sort_unstable();
                    assert!(
                        matches!(
                            kinds.as_slice(),
                            ["FINALIZE", "NOTAR", "REPLAY_COMPLETED" | "REPLAY_DEAD"]
                                | ["FAST_FINALIZE", "REPLAY_COMPLETED" | "REPLAY_DEAD"]
                        ),
                        "slot {slot}: {kinds:?}"
                    );
                }
                shuffled |= !actions.is_sorted_by_key(|a| a.0);
            }
            assert!(shuffled);
        }
    }
}

pub use defs::*;
