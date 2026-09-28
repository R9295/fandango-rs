//! Generate block trees from `grammars/consensus_tree.fan` and draw them.
//!
//! Each tree is pruned to at most [`MAX_WIDTH`] nodes per depth, the way `alpenglow::fix`
//! does it, and its nodes are labelled in preorder: depth, then index within the depth in
//! letters. Depth is bounded only by the grammar depth limit. The canonical path runs from
//! the root to the deepest node, the leftmost one on a tie. Each node on it is drawn with
//! `finalize`; every other node, at any depth, is skipped.
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
use fandango::Fandango;
use fandango::generation::Generated;
use fandango::tuple_list::tuple_list;
use fandango::visitor::Visitor;
use fandango::visitor::write::WriteVisitor;
use fandango_runtime::operators::DepthLimiter;
use rand::SeedableRng;
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

/// Draw `nodes`, and theirs, each line starting with `prefix`. Canonical nodes are marked
/// `finalize`; every other node is skipped, which goes without saying.
fn draw(out: &mut String, nodes: &[Node], canonical: &[Label], prefix: &mut String) {
    for (i, node) in nodes.iter().enumerate() {
        let last = i + 1 == nodes.len();
        let mark = if canonical.contains(&node.label) { "  finalize" } else { "" };
        *out += &format!("{prefix}{}{}{mark}\n", if last { "└── " } else { "├── " }, node.label);
        let len = prefix.len();
        prefix.push_str(if last { "    " } else { "│   " });
        draw(out, &node.children, canonical, prefix);
        prefix.truncate(len);
    }
}

fn main() -> Result<(), Error> {
    let args = Args::parse();
    let mut rng = match args.seed {
        Some(seed) => StdRng::seed_from_u64(seed),
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
        let mut tree = String::from("0\n");
        draw(&mut tree, &roots, &canonical, &mut String::new());
        let path = canonical.iter().map(ToString::to_string).collect::<Vec<_>>().join(" → ");
        println!("tree {iteration:06}\n{tree}path  {path}\n");
    }

    println!("Drew {emitted} nonempty trees from {} attempts.", args.iterations);
    Ok(())
}
