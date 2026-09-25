//! Generate Alpenglow block tree scenarios and draw them.
//!
//! Each scenario starts as a JSON block tree. [`alpenglow::fix`] labels it, and the output
//! corpus contains a shuffled flat JSON list of actions for its nodes. One in
//! [`DEAD_ONE_IN`] nodes off the canonical path is found dead by replay.
//!
//! Usage:
//! ```text
//! cargo run --release -p fandango-targets --example alpenglow_altpath --features alpenglow -- [-n ITERATIONS] [-m MAX_NODES] [-s SEED] [-o OUT_DIR] [-q]
//! ```
//! Defaults: 1000 iterations, at most 128 nodes per tree, a seed from OS entropy, no output
//! directory, every nonempty tree drawn.

use anyhow::{Context, Error};
use clap::Parser;
use fandango::dynamic::DefinitionOf;
use fandango::generation::{Generated, RawSampler, Sampler};
use fandango::lang::FandangoNode;
use fandango::tuple_list::tuple_list;
use fandango::typing::AsStaticNode;
#[cfg(test)]
use fandango::visitor::Visitor;
#[cfg(test)]
use fandango::visitor::write::WriteVisitor;
use fandango_runtime::operators::DepthLimiter;
use fandango_targets::alpenglow::{self, Tree, nonterminal_children_0, nonterminal_start};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use rand::seq::SliceRandom;
use std::fs;
use std::num::NonZeroUsize;
use std::path::PathBuf;

/// Generate Alpenglow block tree scenarios and draw them.
#[derive(Parser)]
#[command(
    after_help = "Example:\n  cargo run --release -p fandango-targets --example alpenglow_altpath --features alpenglow -- -n 1000 -m 128 -s 42 -o corpus"
)]
struct Args {
    /// Number of scenarios to generate
    #[arg(short = 'n', long, default_value = "1000")]
    iterations: NonZeroUsize,

    /// Maximum number of nodes generated for each scenario, including the root
    #[arg(short = 'm', long, default_value = "128")]
    max_nodes: NonZeroUsize,

    /// Reproducible RNG seed [default: OS entropy]
    #[arg(short, long)]
    seed: Option<u64>,

    /// Directory to write each scenario's action list to as <ITERATION>.json
    #[arg(short, long)]
    out_dir: Option<PathBuf>,

    /// Do not draw the nonempty trees
    #[arg(short, long)]
    quiet: bool,
}

/// Count nodes while the grammar is generated, before `fix` gets a chance to prune it.
struct BoundedSampler {
    rng: StdRng,
    nodes: usize,
    max_nodes: usize,
}

impl RawSampler for BoundedSampler {
    fn sample(&mut self) -> usize {
        RawSampler::sample(&mut self.rng)
    }

    fn reseed(&mut self, seed: u64) {
        RawSampler::reseed(&mut self.rng, seed);
        self.nodes = 1;
    }
}

impl<N: AsStaticNode> Sampler<N> for BoundedSampler {
    fn sample_kleene(&mut self) -> usize {
        Sampler::<N>::sample_kleene(&mut self.rng)
    }

    fn sample_plus(&mut self) -> usize {
        Sampler::<N>::sample_plus(&mut self.rng)
    }

    fn sample_optional(&mut self) -> bool {
        Sampler::<N>::sample_optional(&mut self.rng)
    }

    fn sample_repetition(&mut self, lower: usize, upper: usize) -> usize {
        Sampler::<N>::sample_repetition(&mut self.rng, lower, upper)
    }

    fn sample_alternative(&mut self, count: usize) -> usize {
        let choice = Sampler::<N>::sample_alternative(&mut self.rng, count);
        if N::static_definition() != nonterminal_children_0::static_definition() {
            return choice;
        }

        // The first five <children> alternatives contain 0, 1, 2, 3, and 4 nodes;
        // the remaining alternatives are empty. Reject a branch that exceeds the budget.
        assert_eq!(
            count,
            alpenglow::Tree::MAX_CHILDREN + 3,
            "update the node budget for the grammar"
        );
        let added = if choice <= alpenglow::Tree::MAX_CHILDREN {
            choice
        } else {
            0
        };
        if added > self.max_nodes - self.nodes {
            0
        } else {
            self.nodes += added;
            choice
        }
    }
}

impl<N: AsStaticNode> DefinitionOf<N> for BoundedSampler {
    fn root_of(&self) -> FandangoNode<'static, 'static> {
        N::static_root()
    }

    fn definition_of(&self) -> FandangoNode<'static, 'static> {
        N::static_definition()
    }
}

#[cfg(test)]
fn render(scenario: &nonterminal_start) -> Result<String, Error> {
    let bytes = WriteVisitor::new(Vec::new())
        .visit(scenario, 0)?
        .continue_value()
        .expect("write visitor never breaks")
        .output();
    Ok(String::from_utf8(bytes)?)
}

/// One in this many nodes off the canonical path is found dead by replay.
const DEAD_ONE_IN: u32 = 4;

/// Flatten the tree into actions in preorder. The root has no parent and is omitted.
///
/// Nodes on the canonical path are replayed and get notar and final certificates. A node
/// off it is found dead by replay when `is_dead` says so, and is replayed otherwise. Replay
/// drops the descendants of a dead block without reporting them, so they get no actions.
/// `is_dead` is only asked about nodes off the canonical path: the cluster finalizes the
/// canonical path, so replay never finds a block on it dead.
fn action_entries(tree: &Tree, is_dead: &mut impl FnMut() -> bool) -> Vec<String> {
    fn append(
        parent: &Tree,
        canonical: &[alpenglow::Label],
        is_dead: &mut impl FnMut() -> bool,
        entries: &mut Vec<String>,
    ) {
        for child in &parent.children {
            let on_path = canonical.first() == Some(&child.label);
            let dead = !on_path && is_dead();
            let actions: &[&str] = if on_path {
                &["NOTARIZE_CERT", "FINALIZE_CERT", "REPLAY_COMPLETE"]
            } else if dead {
                &["REPLAY_DEAD"]
            } else {
                &["REPLAY_COMPLETE"]
            };
            for action in actions {
                entries.push(format!(
                    "{{\"node\": \"{}\", \"parent\": \"{}\", \"action\": \"{action}\"}}",
                    child.label, parent.label
                ));
            }
            if !dead {
                append(child, if on_path { &canonical[1..] } else { &[] }, is_dead, entries);
            }
        }
    }

    let mut entries = Vec::new();
    append(tree, &tree.canonical_path(), is_dead, &mut entries);
    entries
}

fn render_actions(entries: &[String]) -> String {
    format!("[{}]\n", entries.join(", "))
}

fn main() -> Result<(), Error> {
    let args = Args::parse();
    let iterations = args.iterations.get();

    let rng = match args.seed {
        Some(seed) => StdRng::seed_from_u64(seed),
        None => StdRng::from_os_rng(),
    };
    // Keep the generated trees stable for a seed while varying action order.
    let mut shuffle_rng = match args.seed {
        Some(seed) => StdRng::seed_from_u64(seed ^ 0x7368_7566_666c_6500),
        None => StdRng::from_os_rng(),
    };
    let mut dead_rng = match args.seed {
        Some(seed) => StdRng::seed_from_u64(seed ^ 0x6465_6164_0000_0000),
        None => StdRng::from_os_rng(),
    };
    let mut sampler = BoundedSampler {
        rng,
        nodes: 1,
        max_nodes: args.max_nodes.get(),
    };

    let generator = DepthLimiter::new(alpenglow::STRUCTURE.inner(), 40);
    let mut generators = tuple_list!(generator);
    let mut emitted = 0;

    for iteration in 1..=iterations {
        sampler.nodes = 1;
        let mut scenario = nonterminal_start::generate(&mut sampler, &mut generators, 0);
        let tree = alpenglow::fix(&mut scenario, &mut sampler, &mut generators);
        if tree.children.is_empty() {
            continue;
        }
        emitted += 1;
        let mut actions = action_entries(&tree, &mut || dead_rng.random_ratio(1, DEAD_ONE_IN));
        actions.shuffle(&mut shuffle_rng);

        if !args.quiet {
            let path = tree
                .canonical_path()
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(" → ");
            println!("scenario {iteration:06}\n{tree}path  {path}");
        }

        if let Some(dir) = &args.out_dir {
            if emitted == 1 {
                fs::create_dir_all(dir).with_context(|| format!("could not create {}", dir.display()))?;
            }
            let path = dir.join(format!("{iteration:06}.json"));
            fs::write(&path, render_actions(&actions))
                .with_context(|| format!("could not write {}", path.display()))?;
        }
    }

    match &args.out_dir {
        Some(_) if emitted == 0 => println!("No nonempty scenarios after {iterations} attempts; no corpus written."),
        Some(dir) => println!("Wrote {emitted} nonempty scenarios from {iterations} attempts to {}.", dir.display()),
        None => println!("Generated {emitted} nonempty scenarios from {iterations} attempts."),
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn count_nodes(tree: &alpenglow::Tree) -> usize {
        1 + tree.children.iter().map(count_nodes).sum::<usize>()
    }

    #[test]
    fn action_list_uses_parent_links_and_canonical_certificates() {
        let node = |depth, index, children| Tree {
            label: alpenglow::Label { depth, index },
            children,
        };
        let tree = node(0, 0, vec![
            node(1, 0, vec![node(2, 0, vec![])]),
            node(1, 1, vec![node(2, 1, vec![]), node(2, 2, vec![node(3, 0, vec![])])]),
        ]);
        assert_eq!(render_actions(&action_entries(&tree, &mut || false)), concat!(
            "[{\"node\": \"1a\", \"parent\": \"0\", \"action\": \"REPLAY_COMPLETE\"}, ",
            "{\"node\": \"2a\", \"parent\": \"1a\", \"action\": \"REPLAY_COMPLETE\"}, ",
            "{\"node\": \"1b\", \"parent\": \"0\", \"action\": \"NOTARIZE_CERT\"}, ",
            "{\"node\": \"1b\", \"parent\": \"0\", \"action\": \"FINALIZE_CERT\"}, ",
            "{\"node\": \"1b\", \"parent\": \"0\", \"action\": \"REPLAY_COMPLETE\"}, ",
            "{\"node\": \"2b\", \"parent\": \"1b\", \"action\": \"REPLAY_COMPLETE\"}, ",
            "{\"node\": \"2c\", \"parent\": \"1b\", \"action\": \"NOTARIZE_CERT\"}, ",
            "{\"node\": \"2c\", \"parent\": \"1b\", \"action\": \"FINALIZE_CERT\"}, ",
            "{\"node\": \"2c\", \"parent\": \"1b\", \"action\": \"REPLAY_COMPLETE\"}, ",
            "{\"node\": \"3a\", \"parent\": \"2c\", \"action\": \"NOTARIZE_CERT\"}, ",
            "{\"node\": \"3a\", \"parent\": \"2c\", \"action\": \"FINALIZE_CERT\"}, ",
            "{\"node\": \"3a\", \"parent\": \"2c\", \"action\": \"REPLAY_COMPLETE\"}]\n"
        ));
        assert_eq!(render_actions(&action_entries(&node(0, 0, vec![]), &mut || false)), "[]\n");
    }

    #[test]
    fn dead_nodes_are_off_the_canonical_path_and_hide_their_descendants() {
        let node = |depth, index, children| Tree {
            label: alpenglow::Label { depth, index },
            children,
        };
        let tree = node(0, 0, vec![
            node(1, 0, vec![node(2, 0, vec![])]),
            node(1, 1, vec![node(2, 1, vec![]), node(2, 2, vec![node(3, 0, vec![])])]),
        ]);
        assert_eq!(render_actions(&action_entries(&tree, &mut || true)), concat!(
            "[{\"node\": \"1a\", \"parent\": \"0\", \"action\": \"REPLAY_DEAD\"}, ",
            "{\"node\": \"1b\", \"parent\": \"0\", \"action\": \"NOTARIZE_CERT\"}, ",
            "{\"node\": \"1b\", \"parent\": \"0\", \"action\": \"FINALIZE_CERT\"}, ",
            "{\"node\": \"1b\", \"parent\": \"0\", \"action\": \"REPLAY_COMPLETE\"}, ",
            "{\"node\": \"2b\", \"parent\": \"1b\", \"action\": \"REPLAY_DEAD\"}, ",
            "{\"node\": \"2c\", \"parent\": \"1b\", \"action\": \"NOTARIZE_CERT\"}, ",
            "{\"node\": \"2c\", \"parent\": \"1b\", \"action\": \"FINALIZE_CERT\"}, ",
            "{\"node\": \"2c\", \"parent\": \"1b\", \"action\": \"REPLAY_COMPLETE\"}, ",
            "{\"node\": \"3a\", \"parent\": \"2c\", \"action\": \"NOTARIZE_CERT\"}, ",
            "{\"node\": \"3a\", \"parent\": \"2c\", \"action\": \"FINALIZE_CERT\"}, ",
            "{\"node\": \"3a\", \"parent\": \"2c\", \"action\": \"REPLAY_COMPLETE\"}]\n"
        ));
    }

    #[test]
    fn generated_dead_nodes_are_never_canonical() {
        let mut sampler = BoundedSampler {
            rng: StdRng::seed_from_u64(7),
            nodes: 1,
            max_nodes: 128,
        };
        let mut generators = tuple_list!(DepthLimiter::new(alpenglow::STRUCTURE.inner(), 40));
        let mut dead_seen = false;
        for _ in 0..200 {
            sampler.nodes = 1;
            let mut scenario = nonterminal_start::generate(&mut sampler, &mut generators, 0);
            let tree = alpenglow::fix(&mut scenario, &mut sampler, &mut generators);
            let canonical: Vec<String> = tree.canonical_path().iter().map(ToString::to_string).collect();
            for entry in action_entries(&tree, &mut || true) {
                if !entry.contains("REPLAY_DEAD") {
                    continue;
                }
                dead_seen = true;
                for label in &canonical {
                    assert!(
                        !entry.contains(&format!("\"node\": \"{label}\"")),
                        "canonical node {label} is dead: {entry}"
                    );
                }
            }
        }
        assert!(dead_seen);
    }

    #[test]
    fn generated_trees_respect_node_budget() {
        for max_nodes in [1, 32] {
            let mut sampler = BoundedSampler {
                rng: StdRng::seed_from_u64(1),
                nodes: 1,
                max_nodes,
            };
            let mut generators = tuple_list!(DepthLimiter::new(alpenglow::STRUCTURE.inner(), 40));
            let mut branched = false;
            for _ in 0..100 {
                sampler.nodes = 1;
                let mut scenario = nonterminal_start::generate(&mut sampler, &mut generators, 0);
                let generated_nodes = render(&scenario).unwrap().matches("\"id\":").count();
                assert!(
                    generated_nodes <= max_nodes,
                    "generated {generated_nodes} nodes with a budget of {max_nodes}"
                );
                let tree = alpenglow::fix(&mut scenario, &mut sampler, &mut generators);
                let nodes = count_nodes(&tree);
                assert!(
                    nodes <= max_nodes,
                    "generated {nodes} nodes with a budget of {max_nodes}"
                );
                branched |= nodes > 1;
            }
            assert_eq!(branched, max_nodes > 1);
        }
    }
}
