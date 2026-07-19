//! Target for the Yul grammar stored in `grammars/yul.fan`.
//!
//! [Yul](https://docs.soliditylang.org/en/latest/yul.html) is Ethereum's intermediate
//! language. The grammar is a context-free grammar over Yul *source text*.
//!
//! Value counts, keyword-safe identifiers, and the "every block opens with a declaration"
//! invariant are baked into the grammar structurally. The one semantic rule left to
//! Rust is **variable scope**, repaired by [`ScopeFixer`] — a scaled-down version of the
//! C-language target's declaration fixer: a scope-stack `VisitorMut` that registers each
//! `<var_decl>` and rewrites any out-of-scope `<var_use>` to an in-scope declared name.

#[cfg(not(feature = "static_defs"))]
mod defs {
    use core::convert::Infallible;
    use fandango::Fandango;

    /// Base for the Yul grammar stored in yul.fan.
    #[derive(Fandango)]
    #[fandango(grammar = "grammars/yul.fan", parse = false, dynamic = true)]
    pub struct Yul(Infallible);
}

#[cfg(feature = "static_defs")]
mod defs {
    use alloc::vec::Vec;
    use anyhow::Error;
    use core::convert::Infallible;
    use core::ops::ControlFlow;
    use fandango::Fandango;
    use fandango::generation::Generated;
    use fandango::typing::{AsNodeMut, DowncastMut, Node, Nth, OpaqueMut};
    use fandango::visitor::{VisitMutResult, VisitableChildrenMut, VisitorMut};
    use fandango_runtime::evolvers::basic::BasicHook;

    /// Base for the Yul grammar stored in yul.fan.
    #[derive(Fandango)]
    #[fandango(grammar = "grammars/yul.fan", parse = false)]
    pub struct Yul(Infallible);

    /// A [`VisitorMut`] that repairs variable scope in a generated Yul tree.
    ///
    /// It maintains a stack of scopes (one frame per `<block>`). On a `<var_decl>` it
    /// registers the declared name in the current frame, renaming it to a fresh name if
    /// it would shadow/redeclare an in-scope variable. On a `<var_use>` it rewrites the
    /// name to an in-scope declared variable when the used name is not in scope. Because
    /// every block opens with a declaration, there is always at least one in-scope name.
    pub struct ScopeFixer<'a, S, G> {
        sampler: &'a mut S,
        generator: &'a mut G,
        scopes: Vec<Vec<nonterminal_identifier>>,
    }

    impl<'a, S, G> ScopeFixer<'a, S, G> {
        /// Create a new scope fixer using the given sampler and generators (needed to
        /// mint fresh identifiers when repairing shadowed declarations).
        pub fn new(sampler: &'a mut S, generator: &'a mut G) -> Self {
            Self {
                sampler,
                generator,
                scopes: Vec::new(),
            }
        }

        /// Is `name` declared in any enclosing scope?
        fn in_scope(&self, name: &nonterminal_identifier) -> bool {
            self.scopes
                .iter()
                .any(|frame| frame.iter().any(|declared| declared == name))
        }

        /// Pick any in-scope declared name (innermost frame first).
        fn pick_in_scope(&self) -> Option<nonterminal_identifier> {
            self.scopes.iter().rev().find_map(|frame| frame.last().cloned())
        }
    }

    impl<S, G, T> VisitorMut<T> for ScopeFixer<'_, S, G>
    where
        nonterminal_identifier: Generated<S, G>,
        T: VisitableChildrenMut<T>
            + AsNodeMut<nonterminal_block>
            + AsNodeMut<nonterminal_var_decl>
            + AsNodeMut<nonterminal_var_use>,
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

            // A block introduces a scope: push a frame, visit its children, pop it.
            if visited.downcast_mut::<nonterminal_block>().is_some() {
                self.scopes.push(Vec::new());
                let result = visited.visit_each_mut(self);
                let Ok(ControlFlow::Continue(mut visitor)) = result;
                visitor.scopes.pop();
                return Ok(ControlFlow::Continue(visitor));
            }

            if let Some(decl) = visited.downcast_mut::<nonterminal_var_decl>() {
                // Register the declared name, renaming it if it shadows/redeclares.
                let name = decl.nth::<0>().clone();
                let registered = if self.in_scope(&name) {
                    let mut fresh = nonterminal_identifier::generate(self.sampler, self.generator, 0);
                    while self.in_scope(&fresh) {
                        fresh = nonterminal_identifier::generate(self.sampler, self.generator, 0);
                    }
                    *decl.nth_mut::<0>() = fresh.clone();
                    fresh
                } else {
                    name
                };
                if let Some(frame) = self.scopes.last_mut() {
                    frame.push(registered);
                }
            } else if let Some(used) = visited.downcast_mut::<nonterminal_var_use>() {
                // Rewrite an out-of-scope use to an in-scope declared variable.
                let name = used.nth::<0>().clone();
                if !self.in_scope(&name)
                    && let Some(replacement) = self.pick_in_scope()
                {
                    *used.nth_mut::<0>() = replacement;
                }
            }

            visited.visit_each_mut(self)
        }
    }

    /// A [`BasicHook`] that repairs variable scope on every individual an evolver creates,
    /// so mutation and crossover keep producing valid Yul as the population evolves.
    pub struct YulFixHook;

    impl<N, G, S> BasicHook<N, G, S> for YulFixHook
    where
        N: Node,
        for<'a, 'b> ScopeFixer<'a, S, G>: VisitorMut<N::TypeMut<'b>>,
    {
        fn individual_created(
            &mut self,
            node: &mut N,
            generators: &mut G,
            sampler: &mut S,
        ) -> Result<(), Error> {
            let fixer = ScopeFixer::new(sampler, generators);
            let _ = fixer.visit_mut(node, 0);
            Ok(())
        }
    }

    #[cfg(test)]
    mod test {
        extern crate std;

        use super::*;
        use alloc::vec::Vec;
        use fandango::tuple_list::tuple_list;
        use fandango::typing::Structured;
        use fandango::visitor::Visitor;
        use fandango::visitor::write::WriteVisitor;
        use fandango_runtime::operators::DepthLimiter;
        use rand::SeedableRng;
        use rand::rngs::StdRng;
        use std::collections::BTreeSet;
        use std::process::{Command, Stdio};
        use std::string::{String, ToString};

        fn render(tree: &nonterminal_start) -> String {
            let bytes = WriteVisitor::new(Vec::new())
                .visit(tree, 0)
                .unwrap()
                .continue_value()
                .unwrap()
                .output();
            String::from_utf8(bytes).unwrap()
        }

        /// (valid, first solc `Error:` line). Uses a temp FILE rather than stdin: the
        /// `solc` on PATH may be a Python shim (solc-select) that deadlocks on a piped
        /// stdin.
        fn solc_check(src: &str, path: &std::path::Path) -> (bool, String) {
            std::fs::write(path, src).unwrap();
            let out = Command::new("solc")
                .arg("--strict-assembly")
                .arg(path)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .output()
                .expect("solc must be on PATH for this test");
            let stderr = std::string::String::from_utf8_lossy(&out.stderr);
            let first = stderr
                .lines()
                .find(|l| l.contains("Error:"))
                .unwrap_or("")
                .to_string();
            (out.status.success(), first)
        }

        #[test]
        fn yul_scope_fixer_makes_valid() {
            let mut rng = StdRng::seed_from_u64(7);
            let mut generators =
                tuple_list!(DepthLimiter::new(nonterminal_start::ROOT.inner(), 30));

            let tmp = std::env::temp_dir().join("yul_scope_fixer_test.yul");
            let mut seen = BTreeSet::new();
            let mut valid = 0usize;
            let mut total = 0usize;
            let mut fails: Vec<(String, String)> = Vec::new();

            for _ in 0..300 {
                let mut tree = nonterminal_start::generate(&mut rng, &mut generators, 0);
                let _ = ScopeFixer::new(&mut rng, &mut generators).visit_mut(&mut tree, 0);
                let src = render(&tree);
                if !seen.insert(src.clone()) {
                    continue;
                }
                total += 1;
                let (ok, why) = solc_check(&src, &tmp);
                if ok {
                    valid += 1;
                } else if fails.len() < 10 {
                    fails.push((src, why));
                }
            }

            std::eprintln!("yul scope fixer: valid {valid}/{total} distinct");
            for (src, why) in &fails {
                std::eprintln!("FAIL [{why}]: {src}");
            }
            assert!(total > 0);
            assert!(
                valid * 100 >= total * 90,
                "distinct validity {valid}/{total} < 90%"
            );
        }
    }
}

pub use defs::*;
