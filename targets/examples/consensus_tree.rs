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
//! Every block shows what replay finds: `replay complete`, or, for one in [`DEAD_ONE_IN`]
//! blocks off the canonical path, `replay dead`. A skipped slot has no block to replay. An
//! adversarial leader can still build on a dead block, so its descendants still arrive, but
//! replay finds each of them dead too, as Firedancer's replay marks a dead block's whole
//! subtree dead. The drawing notes the dead block they are under.
//!
//! Each canonical node also shows the certificates that settle it: one of [`SKIP_WAYS`] for
//! a skipped slot, else one of [`FINALIZE_WAYS`], picked uniformly among the ways its slot
//! has room for. Some ways also give notar fallback certificates to other blocks in the
//! slot, picked among those replay completes and finds under no dead block: an honest
//! majority certifies no invalid block.
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

/// A block's notar certificate.
const NOTARIZE_CERT: &str = "NOTARIZE_CERT";
/// A slot's final certificate. It finalizes the slot's block once the block is notarized.
const FINALIZE_CERT: &str = "FINALIZE_CERT";
/// A block's fast final certificate.
const FAST_FINALIZE_CERT: &str = "FAST_FINALIZE_CERT";
/// A block's notar fallback certificate. It finalizes the block only through a finalized
/// descendant.
const NOTAR_FALLBACK_CERT: &str = "NOTAR_FALLBACK_CERT";
/// A slot's skip certificate.
const SKIP_CERT: &str = "SKIP_CERT";

/// How the cluster settles a canonical slot.
#[derive(Clone, Copy)]
struct Settle {
    /// The certificates for the canonical block, or [`SKIP_CERT`] for a skipped slot.
    certificates: &'static [&'static str],
    /// How many other blocks in the slot get a notar fallback certificate. A slot holds at
    /// most four notar fallback certificates (Lemma 48, `AG_NOTAR_FALLBACK_CERT_MAX`).
    others: usize,
}

impl Settle {
    /// Whether the slot is skipped: it has no canonical block.
    fn skips(self) -> bool {
        self.certificates.contains(&SKIP_CERT)
    }

    /// Whether the certificates finalize the canonical block directly, not only through a
    /// finalized descendant.
    fn finalizes(self) -> bool {
        let has = |certificate| self.certificates.contains(&certificate);
        (has(NOTARIZE_CERT) && has(FINALIZE_CERT)) || has(FAST_FINALIZE_CERT)
    }
}

/// The ways a slot with a canonical block can be settled.
const FINALIZE_WAYS: [Settle; 10] = [
    Settle { certificates: &[NOTAR_FALLBACK_CERT], others: 0 },
    Settle { certificates: &[NOTARIZE_CERT, FINALIZE_CERT], others: 0 },
    Settle { certificates: &[FAST_FINALIZE_CERT], others: 0 },
    Settle { certificates: &[FAST_FINALIZE_CERT, NOTARIZE_CERT], others: 0 },
    Settle { certificates: &[NOTARIZE_CERT], others: 0 },
    Settle { certificates: &[FINALIZE_CERT], others: 0 },
    Settle { certificates: &[NOTARIZE_CERT, FINALIZE_CERT, FAST_FINALIZE_CERT], others: 0 },
    Settle { certificates: &[NOTARIZE_CERT], others: 1 },
    Settle { certificates: &[NOTARIZE_CERT], others: 2 },
    Settle { certificates: &[NOTAR_FALLBACK_CERT], others: 3 },
];

/// The ways a skipped slot can be settled.
const SKIP_WAYS: [Settle; 3] = [
    Settle { certificates: &[SKIP_CERT], others: 0 },
    Settle { certificates: &[SKIP_CERT], others: 1 },
    Settle { certificates: &[SKIP_CERT], others: 2 },
];

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

/// What replay finds for a node: the result, if the node is a block, and the nearest dead
/// block above it, if any.
#[derive(Clone, Copy)]
struct Found {
    label: Label,
    result: Option<&'static str>,
    under: Option<Label>,
}

/// Find what replay does with each node in preorder. A canonical block always replays
/// completely, and a skipped slot has no block to replay. An off-path block is found dead
/// when `is_dead` says so, and always under a dead block: an adversarial leader can build
/// on a dead block, so its descendants still arrive, but replay marks them dead.
fn replay(
    nodes: &[Node],
    canonical: &[Label],
    skipped: &[Label],
    under: Option<Label>,
    is_dead: &mut impl FnMut() -> bool,
    found: &mut Vec<Found>,
) {
    for node in nodes {
        let label = node.label;
        let result = if skipped.contains(&label) {
            None
        } else if canonical.contains(&label) || (under.is_none() && !is_dead()) {
            Some(REPLAY_COMPLETE)
        } else {
            Some(REPLAY_DEAD)
        };
        found.push(Found { label, result, under });
        let dead = if result == Some(REPLAY_DEAD) { Some(label) } else { under };
        replay(&node.children, canonical, skipped, dead, is_dead, found);
    }
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
/// Each canonical node, with how its slot is settled in `canonical`, is marked `finalize`,
/// or `skip` if its slot is skipped, followed by its certificates; every other node is
/// skipped, which goes without saying. A canonical block its certificates do not finalize
/// directly also names the finalization it waits on: that of the next canonical block
/// they do, or none. Each other block its slot's settle gives a notar fallback certificate,
/// in `fallbacks`, shows the certificate. Every block then shows what replay finds, from
/// `found`.
///
/// Each certificate and replay result drawn is also recorded in `actions`, citing the node's
/// parent the way the drawing does, and each replay result follows a [`REPLAY_ARRIVES`].
struct Drawing<'a> {
    out: String,
    actions: Vec<Action>,
    canonical: &'a [(Label, Settle)],
    fallbacks: &'a [Label],
    found: &'a [Found],
}

impl Drawing<'_> {
    /// Draw `nodes`, the children of `parent`, and theirs, each line starting with `prefix`.
    /// `cited` is the block the nodes build on: `parent`, or the block before it if `parent`
    /// is a skipped slot. It is drawn when it differs from `parent`.
    fn visit(&mut self, nodes: &[Node], parent: Label, cited: Label, prefix: &mut String) {
        let (canonical, fallbacks, found) = (self.canonical, self.fallbacks, self.found);
        for (i, node) in nodes.iter().enumerate() {
            let last = i + 1 == nodes.len();
            let label = node.label;
            let out = &mut self.out;
            let position = canonical.iter().position(|&(l, _)| l == label);
            let settle = position.map(|i| canonical[i].1);
            let skipped = settle.is_some_and(Settle::skips);
            let _ = write!(out, "{prefix}{}{label}", if last { "└── " } else { "├── " });
            let mut certificates: &[&'static str] = &[];
            if let Some(settle) = settle {
                certificates = settle.certificates;
                let _ = write!(out, "  {}  {}", if skipped { "skip" } else { "finalize" }, certificates.join(" + "));
            } else if fallbacks.contains(&label) {
                certificates = &[NOTAR_FALLBACK_CERT];
                let _ = write!(out, "  {NOTAR_FALLBACK_CERT}");
            }
            for &kind in certificates {
                self.actions.push(Action { node: label, parent: cited, kind });
            }
            if let Some(i) = position.filter(|_| settle.is_some_and(|s| !s.skips() && !s.finalizes())) {
                match canonical[i + 1..].iter().find(|(_, s)| s.finalizes()) {
                    Some((l, _)) => {
                        let _ = write!(self.out, "  (waits on finalization of {l})");
                    }
                    None => self.out.push_str("  (waits on none)"),
                }
            }
            let f = found.iter().find(|f| f.label == label).expect("replay visits every node");
            if let Some(result) = f.result {
                self.out.push_str(if result == REPLAY_DEAD { "  replay dead" } else { "  replay complete" });
                self.actions.push(Action { node: label, parent: cited, kind: REPLAY_ARRIVES });
                self.actions.push(Action { node: label, parent: cited, kind: result });
            }
            if let Some(dead) = f.under {
                let _ = write!(self.out, "  (under dead {dead})");
            }
            if cited != parent {
                let _ = write!(self.out, "  (parent {cited})");
            }
            self.out.push('\n');
            let len = prefix.len();
            prefix.push_str(if last { "    " } else { "│   " });
            let next = if skipped { cited } else { label };
            self.visit(&node.children, label, next, prefix);
            prefix.truncate(len);
        }
    }
}

/// Settle each canonical slot one of the ways it has room for, and return how, with the
/// other blocks the ways give notar fallback certificates. A way that gives them to other
/// blocks needs that many blocks in the slot that replay completes, under no dead block.
fn settle(
    canonical: &[Label],
    skipped: &[Label],
    found: &[Found],
    rng: &mut StdRng,
) -> (Vec<(Label, Settle)>, Vec<Label>) {
    let mut settled = Vec::with_capacity(canonical.len());
    let mut fallbacks = Vec::new();
    for &label in canonical {
        let mut live: Vec<Label> = found
            .iter()
            .filter(|f| f.label.depth == label.depth && f.label != label)
            .filter(|f| f.result == Some(REPLAY_COMPLETE) && f.under.is_none())
            .map(|f| f.label)
            .collect();
        let ways: &[Settle] = if skipped.contains(&label) { &SKIP_WAYS } else { &FINALIZE_WAYS };
        let fits: Vec<Settle> = ways.iter().copied().filter(|w| w.others <= live.len()).collect();
        let settle = fits[rng.random_range(0..fits.len())];
        live.shuffle(rng);
        fallbacks.extend_from_slice(&live[..settle.others]);
        settled.push((label, settle));
    }
    (settled, fallbacks)
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
        let mut found = Vec::new();
        let mut is_dead = || dead_rng.random_ratio(1, DEAD_ONE_IN);
        replay(&roots, &canonical, &skipped, None, &mut is_dead, &mut found);

        let (settled, fallbacks) = settle(&canonical, &skipped, &found, &mut certificate_rng);

        let root = Label { depth: 0, index: 0 };
        let mut drawing = Drawing {
            out: String::from("0\n"),
            actions: Vec::new(),
            canonical: &settled,
            fallbacks: &fallbacks,
            found: &found,
        };
        drawing.visit(&roots, root, root, &mut String::new());
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
