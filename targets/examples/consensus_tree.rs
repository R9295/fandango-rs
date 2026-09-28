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
//! Usage:
//! ```text
//! RUSTFLAGS="-Znext-solver" cargo run -p fandango-targets --example consensus_tree -- [-n ITERATIONS] [-s SEED] [-d DEPTH]
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
use rand::{Rng, SeedableRng};
use rand::rngs::StdRng;
use serde_json::Value;

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

/// Draw `nodes`, the children of `parent`, and theirs, each line starting with `prefix`.
/// Each canonical node, paired with its certificates in `canonical`, is marked `finalize`,
/// or `skip` if its slot is skipped, followed by the certificates; every other node is
/// skipped, which goes without saying. A node settled by [`NOTAR_FALLBACK_CERT`] also names
/// the finalization it waits on: that of the next canonical node settled by any other
/// certificates but [`SKIP_CERT`], or none. `cited` is the block the nodes build on: `parent`,
/// or the block before it if `parent` is a skipped slot. It is drawn when it differs from
/// `parent`.
fn draw(out: &mut String, nodes: &[Node], canonical: &[(Label, &str)], parent: Label, cited: Label, prefix: &mut String) {
    for (i, node) in nodes.iter().enumerate() {
        let last = i + 1 == nodes.len();
        let position = canonical.iter().position(|&(label, _)| label == node.label);
        let settled = position.map(|i| canonical[i].1);
        let skipped = settled == Some(SKIP_CERT);
        let _ = write!(out, "{prefix}{}{}", if last { "└── " } else { "├── " }, node.label);
        if let Some(certificates) = settled {
            let _ = write!(out, "  {}  {certificates}", if skipped { "skip" } else { "finalize" });
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
        if cited != parent {
            let _ = write!(out, "  (parent {cited})");
        }
        out.push('\n');
        let len = prefix.len();
        prefix.push_str(if last { "    " } else { "│   " });
        let next = if skipped { cited } else { node.label };
        draw(out, &node.children, canonical, node.label, next, prefix);
        prefix.truncate(len);
    }
}

fn main() -> Result<(), Error> {
    let args = Args::parse();
    let mut rng = match args.seed {
        Some(seed) => StdRng::seed_from_u64(seed),
        None => StdRng::from_os_rng(),
    };
    // Keep the generated trees stable for a seed while choosing which slots to skip and
    // which certificates settle each canonical slot.
    let mut skip_rng = match args.seed {
        Some(seed) => StdRng::seed_from_u64(seed ^ 0x736b_6970_0000_0000),
        None => StdRng::from_os_rng(),
    };
    let mut certificate_rng = match args.seed {
        Some(seed) => StdRng::seed_from_u64(seed ^ 0x6365_7274_0000_0000),
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
        let settled: Vec<(Label, &str)> = canonical
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
        let mut tree = String::from("0\n");
        let root = Label { depth: 0, index: 0 };
        draw(&mut tree, &roots, &settled, root, root, &mut String::new());
        let path = canonical
            .iter()
            .map(|label| if skipped.contains(label) { format!("{label} (skip)") } else { label.to_string() })
            .collect::<Vec<_>>()
            .join(" → ");
        println!("tree {iteration:06}\n{tree}path  {path}\n");
    }

    println!("Drew {emitted} nonempty trees from {} attempts.", args.iterations);
    Ok(())
}
