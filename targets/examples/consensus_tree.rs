//! Generate block trees from `grammars/consensus_tree.fan` and draw them.
//!
//! Each tree is pruned to at most [`MAX_WIDTH`] nodes per depth, the way `alpenglow::fix`
//! does it, and its nodes are labelled in preorder: depth, then index within the depth in
//! letters. Depth is bounded only by the grammar depth limit. The canonical path runs from
//! the root to the deepest node, the leftmost one on a tie. Each node on it is drawn with
//! `finalize`, except that one in [`SKIP_ONE_IN`] canonical slots before the tip is skipped
//! instead: the slot has no canonical block, so the nodes under it cite the block before it
//! as their parent. Every other node, at any depth, is skipped without saying so.
//!
//! Each canonical node also shows the certificates that settle it: a skipped slot gets
//! `SKIP_CERT`, and a finalized one gets one of [`FINALIZE_WAYS`], picked uniformly.
//!
//! Every block also shows what replay finds: `replay complete`, or, for one in
//! [`DEAD_ONE_IN`] blocks off the canonical path, `replay dead`. A skipped slot has no
//! block to replay. An adversarial leader can still build on a dead block, so its
//! descendants still arrive, and replay finds each of them dead or complete like any other
//! off-path block. The drawing notes the dead block they are under.
//!
//! Each tree is then flattened into a shuffled JSON list of actions, as `alpenglow_altpath`
//! does: every certificate and replay result above becomes one action naming its node and
//! the block that node cites as its parent. Each replay result also comes with a
//! `REPLAY_ARRIVES` action for its block, which the shuffle keeps ahead of the result: a
//! block's replay cannot finish before the block arrives. Between each pair of actions,
//! the clock advances by one of [`CLOCK_STEPS_MS`], picked by its weight. With `-o`, each
//! action list is also written to `<OUT_DIR>/<ITERATION>.json` on one line, as
//! `alpenglow_altpath` writes its corpus.
//!
//! Usage:
//! ```text
//! RUSTFLAGS="-Znext-solver" cargo run -p fandango-targets --example consensus_tree -- [-n ITERATIONS] [-s SEED] [-d DEPTH] [-o OUT_DIR]
//! ```

extern crate alloc;

use anyhow::{Context, Error};
use clap::Parser;
use core::convert::Infallible;
use core::fmt;
use core::fmt::Write;
use fandango::Fandango;
use fandango::generation::Generated;
use fandango::tuple_list::tuple_list;
use fandango::visitor::Visitor;
use fandango::visitor::write::WriteVisitor;
use fandango_runtime::operators::DepthLimiter;
use rand::seq::SliceRandom;
use rand::{Rng, SeedableRng};
use rand::rngs::StdRng;
use serde_json::Value;
use std::fs;
use std::path::PathBuf;

/// Base for the grammar stored in `consensus_tree.fan`.
#[derive(Fandango)]
#[fandango(grammar = "grammars/consensus_tree.fan", parse = false)]
pub struct ConsensusTree(Infallible);

/// Generate block trees and draw them.
#[derive(Parser)]
struct Args {
    /// Number of trees to generate
    #[arg(short = 'n', long, default_value = "5")]
    iterations: usize,

    /// Reproducible RNG seed [default: OS entropy]
    #[arg(short, long)]
    seed: Option<u64>,

    /// Grammar depth limit, which bounds how deep a tree can grow
    #[arg(short, long, default_value = "40")]
    depth: usize,

    /// Directory to write each tree's action list to as <ITERATION>.json
    #[arg(short, long)]
    out_dir: Option<PathBuf>,
}

/// Most nodes across an entire depth.
const MAX_WIDTH: usize = 5;

/// One in this many canonical slots before the tip is skipped.
const SKIP_ONE_IN: u32 = 4;

/// The certificates that can finalize a canonical slot, as in `alpenglow_altpath`. A notar
/// fallback certificate finalizes its block only through a finalized descendant.
const FINALIZE_WAYS: [&str; 4] = [
    NOTAR_FALLBACK_CERT,
    "NOTARIZE_CERT + FINALIZE_CERT",
    "FAST_FINALIZE_CERT",
    "FAST_FINALIZE_CERT + NOTARIZE_CERT",
];

/// One in this many off-path blocks is found dead by replay.
const DEAD_ONE_IN: u32 = 4;

/// A block arrives for replay.
const REPLAY_ARRIVES: &str = "REPLAY_ARRIVES";
/// Replay finishes a block.
const REPLAY_COMPLETE: &str = "REPLAY_COMPLETE";
/// Replay finds a block invalid.
const REPLAY_DEAD: &str = "REPLAY_DEAD";

/// Milliseconds the clock advances between two actions, each with its weight out of
/// [`CLOCK_WEIGHT_TOTAL`]: 50 ms and 100 ms 45% of the time each, 500 ms 10%.
const CLOCK_STEPS_MS: [(u64, u32); 3] = [(50, 9), (100, 9), (500, 2)];
/// The sum of the weights in [`CLOCK_STEPS_MS`].
const CLOCK_WEIGHT_TOTAL: u32 = 20;

/// The certificate that skips a canonical slot.
const SKIP_CERT: &str = "SKIP_CERT";

/// The certificate that finalizes its block only through a finalized descendant.
const NOTAR_FALLBACK_CERT: &str = "NOTAR_FALLBACK_CERT";

/// A node's label: `0` for the root, else its depth then its index among the nodes of its
/// depth, in letters, as in `alpenglow::Label`.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Label {
    depth: usize,
    index: usize,
}

impl fmt::Display for Label {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.depth == 0 {
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
struct Node {
    label: Label,
    children: Vec<Node>,
}

/// Label a `children` list at `depth + 1` in preorder, emptying it when it would make its
/// depth wider than [`MAX_WIDTH`].
fn prune(list: &[Value], depth: usize, counts: &mut Vec<usize>) -> Result<Vec<Node>, Error> {
    let depth = depth + 1;
    if counts.len() <= depth {
        counts.resize(depth + 1, 0);
    }
    if list.len() > MAX_WIDTH - counts[depth] {
        return Ok(Vec::new());
    }
    let mut nodes = Vec::with_capacity(list.len());
    for child in list {
        let label = Label { depth, index: counts[depth] };
        counts[depth] += 1;
        let children = child["children"].as_array().context("a node without children")?;
        nodes.push(Node { label, children: prune(children, depth, counts)? });
    }
    Ok(nodes)
}

/// The labels from the root to the deepest node, the leftmost one on a tie, root excluded.
fn canonical_path(nodes: &[Node]) -> Vec<Label> {
    fn visit(nodes: &[Node], path: &mut Vec<Label>, best: &mut Vec<Label>) {
        if path.len() > best.len() {
            best.clone_from(path);
        }
        for node in nodes {
            path.push(node.label);
            visit(&node.children, path, best);
            path.pop();
        }
    }
    let mut best = Vec::new();
    visit(nodes, &mut Vec::new(), &mut best);
    best
}

/// An action on `node`, which cites `parent`.
#[derive(Clone, Copy)]
struct Action {
    node: Label,
    parent: Label,
    kind: &'static str,
}

impl Action {
    /// Whether this action reports how a block's replay ended.
    fn is_replay_result(self) -> bool {
        self.kind == REPLAY_COMPLETE || self.kind == REPLAY_DEAD
    }
}

/// The action as `alpenglow_altpath` writes it.
impl fmt::Display for Action {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self { node, parent, kind } = self;
        write!(f, "{{\"node\": \"{node}\", \"parent\": \"{parent}\", \"action\": \"{kind}\"}}")
    }
}

/// Move each block's [`REPLAY_ARRIVES`] ahead of its replay result where they are out of
/// order, by swapping the two.
fn arrivals_first(actions: &mut [Action]) {
    for i in 0..actions.len() {
        if !actions[i].is_replay_result() {
            continue;
        }
        let node = actions[i].node;
        if let Some(j) = actions[i + 1..].iter().position(|a| a.node == node && a.kind == REPLAY_ARRIVES) {
            actions.swap(i, i + 1 + j);
        }
    }
}

/// Draws a tree, a node per line, and flattens it into actions in preorder.
///
/// Each canonical node, paired with its certificates in `canonical`, is marked `finalize`,
/// or `skip` if its slot is skipped, followed by the certificates; every other node is
/// skipped, which goes without saying. A node settled by [`NOTAR_FALLBACK_CERT`] also names
/// the finalization it waits on: that of the next canonical node settled by any other
/// certificates but [`SKIP_CERT`], or none.
///
/// Every block is then marked with what replay finds. A canonical block always replays
/// completely, and a skipped slot has no block to replay. An off-path block is found dead
/// when `is_dead` says so, even under a dead block: an adversarial leader can build on a
/// dead block, so its descendants still arrive and replay. `dead` in [`Self::visit`] is only
/// the nearest dead block above, for the drawing.
///
/// Each certificate and replay result drawn is also recorded in `actions`, citing the node's
/// parent the way the drawing does, and each replay result follows a [`REPLAY_ARRIVES`].
struct Drawing<'a, F> {
    out: String,
    actions: Vec<Action>,
    canonical: &'a [(Label, &'static str)],
    is_dead: F,
}

impl<F: FnMut() -> bool> Drawing<'_, F> {
    /// Draw `nodes`, the children of `parent`, and theirs, each line starting with `prefix`.
    /// `cited` is the block the nodes build on: `parent`, or the block before it if `parent`
    /// is a skipped slot. It is drawn when it differs from `parent`. `dead` is the nearest dead
    /// block the nodes are under, if any.
    /// Record that `node`, which cites `parent`, arrives and that its replay ends in `result`.
    fn replayed(&mut self, node: Label, parent: Label, result: &'static str) {
        self.actions.push(Action { node, parent, kind: REPLAY_ARRIVES });
        self.actions.push(Action { node, parent, kind: result });
    }

    fn visit(&mut self, nodes: &[Node], parent: Label, cited: Label, dead: Option<Label>, prefix: &mut String) {
        let canonical = self.canonical;
        for (i, node) in nodes.iter().enumerate() {
            let last = i + 1 == nodes.len();
            let out = &mut self.out;
            let position = canonical.iter().position(|&(label, _)| label == node.label);
            let settled = position.map(|i| canonical[i].1);
            let skipped = settled == Some(SKIP_CERT);
            let _ = write!(out, "{prefix}{}{}", if last { "└── " } else { "├── " }, node.label);
            if let Some(certificates) = settled {
                let _ = write!(out, "  {}  {certificates}", if skipped { "skip" } else { "finalize" });
                for certificate in certificates.split(" + ") {
                    self.actions.push(Action { node: node.label, parent: cited, kind: certificate });
                }
            }
            if let Some(i) = position.filter(|_| settled == Some(NOTAR_FALLBACK_CERT)) {
                let finalized = canonical[i + 1..]
                    .iter()
                    .find(|&&(_, certificates)| certificates != SKIP_CERT && certificates != NOTAR_FALLBACK_CERT);
                match finalized {
                    Some((label, _)) => {
                        let _ = write!(out, "  (waits on finalization of {label})");
                    }
                    None => out.push_str("  (waits on none)"),
                }
            }
            let dead = if skipped {
                dead
            } else if settled.is_some() {
                self.out.push_str("  replay complete");
                self.replayed(node.label, cited, REPLAY_COMPLETE);
                dead
            } else {
                let is_dead = (self.is_dead)();
                self.out.push_str(if is_dead { "  replay dead" } else { "  replay complete" });
                if let Some(dead) = dead {
                    let _ = write!(self.out, "  (under dead {dead})");
                }
                self.replayed(node.label, cited, if is_dead { REPLAY_DEAD } else { REPLAY_COMPLETE });
                if is_dead { Some(node.label) } else { dead }
            };
            let out = &mut self.out;
            if cited != parent {
                let _ = write!(out, "  (parent {cited})");
            }
            out.push('\n');
            let len = prefix.len();
            prefix.push_str(if last { "    " } else { "│   " });
            let next = if skipped { cited } else { node.label };
            self.visit(&node.children, node.label, next, dead, prefix);
            prefix.truncate(len);
        }
    }
}

/// Pick a clock step from [`CLOCK_STEPS_MS`] by weight.
fn clock_step(rng: &mut StdRng) -> u64 {
    let mut pick = rng.random_range(0..CLOCK_WEIGHT_TOTAL);
    for (ms, weight) in CLOCK_STEPS_MS {
        if pick < weight {
            return ms;
        }
        pick -= weight;
    }
    unreachable!("the weights sum to CLOCK_WEIGHT_TOTAL")
}

fn main() -> Result<(), Error> {
    let args = Args::parse();
    let mut rng = match args.seed {
        Some(seed) => StdRng::seed_from_u64(seed),
        None => StdRng::from_os_rng(),
    };
    // Keep the generated trees stable for a seed while choosing which slots to skip, which
    // certificates settle each canonical slot, which off-path blocks are dead, and how the
    // actions are shuffled, and how far the clock steps between them.
    let mut skip_rng = match args.seed {
        Some(seed) => StdRng::seed_from_u64(seed ^ 0x736b_6970_0000_0000),
        None => StdRng::from_os_rng(),
    };
    let mut certificate_rng = match args.seed {
        Some(seed) => StdRng::seed_from_u64(seed ^ 0x6365_7274_0000_0000),
        None => StdRng::from_os_rng(),
    };
    let mut dead_rng = match args.seed {
        Some(seed) => StdRng::seed_from_u64(seed ^ 0x6465_6164_0000_0000),
        None => StdRng::from_os_rng(),
    };
    let mut clock_rng = match args.seed {
        Some(seed) => StdRng::seed_from_u64(seed ^ 0x636c_6f63_6b00_0000),
        None => StdRng::from_os_rng(),
    };
    let mut shuffle_rng = match args.seed {
        Some(seed) => StdRng::seed_from_u64(seed ^ 0x7368_7566_666c_6500),
        None => StdRng::from_os_rng(),
    };
    let mut generators = tuple_list!(DepthLimiter::new(STRUCTURE.inner(), args.depth));
    let mut emitted = 0;

    for iteration in 1..=args.iterations {
        let scenario = nonterminal_start::generate(&mut rng, &mut generators, 0);
        let bytes = WriteVisitor::new(Vec::new())
            .visit(&scenario, 0)?
            .continue_value()
            .expect("write visitor never breaks")
            .output();
        let json: Value = serde_json::from_slice(&bytes).context("the grammar generates JSON")?;
        let roots = prune(json["children"].as_array().context("a root without children")?, 0, &mut Vec::new())?;
        if roots.is_empty() {
            continue;
        }
        emitted += 1;

        let canonical = canonical_path(&roots);
        let (_tip, before_tip) = canonical.split_last().expect("a nonempty tree has a path");
        let skipped: Vec<Label> =
            before_tip.iter().copied().filter(|_| skip_rng.random_ratio(1, SKIP_ONE_IN)).collect();
        let settled: Vec<(Label, &'static str)> = canonical
            .iter()
            .map(|&label| {
                let certificates = if skipped.contains(&label) {
                    SKIP_CERT
                } else {
                    FINALIZE_WAYS[certificate_rng.random_range(0..FINALIZE_WAYS.len())]
                };
                (label, certificates)
            })
            .collect();
        let root = Label { depth: 0, index: 0 };
        let mut drawing = Drawing {
            out: String::from("0\n"),
            actions: Vec::new(),
            canonical: &settled,
            is_dead: || dead_rng.random_ratio(1, DEAD_ONE_IN),
        };
        drawing.visit(&roots, root, root, None, &mut String::new());
        let tree = drawing.out;
        let mut actions = drawing.actions;
        actions.shuffle(&mut shuffle_rng);
        arrivals_first(&mut actions);
        let mut stepped = Vec::with_capacity(2 * actions.len());
        for (i, action) in actions.iter().enumerate() {
            if i > 0 {
                let ms = clock_step(&mut clock_rng);
                stepped.push(format!("{{\"action\": \"CLOCK\", \"ms\": {ms}}}"));
            }
            stepped.push(action.to_string());
        }
        let actions = stepped;
        let path = canonical
            .iter()
            .map(|label| if skipped.contains(label) { format!("{label} (skip)") } else { label.to_string() })
            .collect::<Vec<_>>()
            .join(" → ");
        println!("tree {iteration:06}\n{tree}path  {path}\nactions\n[\n  {}\n]\n", actions.join(",\n  "));

        if let Some(dir) = &args.out_dir {
            if emitted == 1 {
                fs::create_dir_all(dir).with_context(|| format!("could not create {}", dir.display()))?;
            }
            let path = dir.join(format!("{iteration:06}.json"));
            fs::write(&path, format!("[{}]\n", actions.join(", ")))
                .with_context(|| format!("could not write {}", path.display()))?;
        }
    }

    match &args.out_dir {
        Some(_) if emitted == 0 => println!("No nonempty trees after {} attempts; no corpus written.", args.iterations),
        Some(dir) => println!("Wrote {emitted} nonempty trees from {} attempts to {}.", args.iterations, dir.display()),
        None => println!("Drew {emitted} nonempty trees from {} attempts.", args.iterations),
    }
    Ok(())
}
