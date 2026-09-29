//! Generate block trees from `grammars/consensus_tree.fan` and turn them into bank scenarios
//! for Firedancer's `test_bank_scenarios`.
//!
//! Trees are pruned and labelled as in `consensus_tree`: at most [`MAX_WIDTH`] nodes per
//! depth, each labelled by its depth, then its index within the depth in letters. Every node
//! is a block replay sees, so there are no skipped slots. The canonical path runs from the
//! root to the deepest node, the leftmost one on a tie, and each node on it is replayed to
//! frozen and then rooted.
//!
//! Every other node gets a [`Fate`]: which of `fd_banks`' transitions its bank goes through.
//! A node whose parent never freezes can only get a bank and stay in INIT, or die there,
//! since replay only starts a block once its parent's bank is frozen. A node under a dead
//! block still arrives, as an adversarial leader can build on one: the harness skips its
//! `NEW_BANK` if the parent is dead by then, and otherwise the parent's death kills it.
//!
//! On top of its fate, one in [`LEADER_ONE_IN`] nodes is a leader bank, which is never
//! evicted, and one in [`HELD_ONE_IN`] is held: an `ACQUIRE` and a later `RELEASE` stand in
//! for the scheduler holding a reference. Node-less actions come last: a `PRUNE` for each
//! bank that can die or be evicted, and one `EVICT` in [`EVICT_ONE_IN`] nodes, half of them
//! protecting a random node.
//!
//! Each tree is flattened into a shuffled JSON list of actions, then put in an order replay
//! could produce: a block's bank is made after its parent's, started after its parent is
//! frozen, frozen or found dead after it starts, and rooted along the path in order. With
//! `-o`, each action list is written to `<OUT_DIR>/<ITERATION>.json` on one line.
//!
//! Usage:
//! ```text
//! RUSTFLAGS="-Znext-solver" cargo run -p fandango-targets --example replay_tree -- [-n ITERATIONS] [-s SEED] [-d DEPTH] [-o OUT_DIR]
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
pub struct ReplayTree(Infallible);

/// Generate block trees and turn them into bank scenarios.
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

/// One in this many nodes is a leader bank.
const LEADER_ONE_IN: u32 = 8;

/// One in this many nodes is held for a while.
const HELD_ONE_IN: u32 = 4;

/// One in this many nodes adds an `EVICT`.
const EVICT_ONE_IN: u32 = 3;

/// Make a bank for a block.
const NEW_BANK: &str = "NEW_BANK";
/// Start replaying a block on its bank.
const BLOCK_START: &str = "BLOCK_START";
/// Freeze a bank once its block replays.
const FINALIZE: &str = "FINALIZE";
/// Mark a bank and everything under it dead.
const DEAD: &str = "DEAD";
/// Take a reference on a bank.
const ACQUIRE: &str = "ACQUIRE";
/// Drop a reference on a bank.
const RELEASE: &str = "RELEASE";
/// Advance the root toward a bank.
const ROOT: &str = "ROOT";
/// Mark one evictable leaf prunable, protecting the named node if any.
const EVICT: &str = "EVICT";
/// Free one dead or prunable bank.
const PRUNE: &str = "PRUNE";

/// Which transitions a node's bank goes through.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Fate {
    /// Replayed and frozen: INIT, REPLAYABLE, FROZEN.
    Frozen,
    /// Found dead before replay started: INIT, DEAD.
    DeadAtInit,
    /// Found dead during replay: INIT, REPLAYABLE, DEAD.
    DeadInReplay,
    /// Found dead after it froze: INIT, REPLAYABLE, FROZEN, DEAD.
    DeadAfterFreeze,
    /// Never started, left for the root to prune: INIT.
    StalledAtInit,
    /// Started but never finished, left for the root to prune: INIT, REPLAYABLE.
    StalledInReplay,
}

impl Fate {
    /// Every fate, each with its weight.
    const ALL: [(Fate, u32); 6] = [
        (Fate::Frozen, 6),
        (Fate::DeadAtInit, 2),
        (Fate::DeadInReplay, 2),
        (Fate::DeadAfterFreeze, 1),
        (Fate::StalledAtInit, 1),
        (Fate::StalledInReplay, 1),
    ];

    /// The fates open to a node whose parent never freezes.
    const AT_INIT: [(Fate, u32); 2] = [(Fate::DeadAtInit, 1), (Fate::StalledAtInit, 1)];

    /// The node actions this fate takes, in order.
    fn actions(self) -> &'static [&'static str] {
        match self {
            Fate::Frozen => &[NEW_BANK, BLOCK_START, FINALIZE],
            Fate::DeadAtInit => &[NEW_BANK, DEAD],
            Fate::DeadInReplay => &[NEW_BANK, BLOCK_START, DEAD],
            Fate::DeadAfterFreeze => &[NEW_BANK, BLOCK_START, FINALIZE, DEAD],
            Fate::StalledAtInit => &[NEW_BANK],
            Fate::StalledInReplay => &[NEW_BANK, BLOCK_START],
        }
    }

    /// Whether the bank reaches FROZEN, so its children can start.
    fn freezes(self) -> bool {
        self.actions().contains(&FINALIZE)
    }

    /// Whether the bank dies.
    fn dies(self) -> bool {
        self.actions().contains(&DEAD)
    }

    fn name(self) -> &'static str {
        match self {
            Fate::Frozen => "frozen",
            Fate::DeadAtInit => "dead at init",
            Fate::DeadInReplay => "dead in replay",
            Fate::DeadAfterFreeze => "dead after freeze",
            Fate::StalledAtInit => "stalled at init",
            Fate::StalledInReplay => "stalled in replay",
        }
    }

    /// Pick a fate from `fates` by weight.
    fn pick(fates: &[(Fate, u32)], rng: &mut StdRng) -> Fate {
        let total: u32 = fates.iter().map(|&(_, w)| w).sum();
        let mut pick = rng.random_range(0..total);
        for &(fate, weight) in fates {
            if pick < weight {
                return fate;
            }
            pick -= weight;
        }
        unreachable!("pick is below the total weight")
    }
}

/// A node's label: `0` for the root, else its depth then its index among the nodes of its
/// depth, in letters, as in `consensus_tree`.
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

/// An action, on `node` citing `parent` unless it names no node.
#[derive(Clone, Copy)]
struct Action {
    node: Option<(Label, Label)>,
    kind: &'static str,
    leader: bool,
}

impl fmt::Display for Action {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self { node, kind, leader } = self;
        match node {
            None => write!(f, "{{\"action\": \"{kind}\"}}"),
            Some((node, parent)) if *leader => {
                write!(f, "{{\"node\": \"{node}\", \"parent\": \"{parent}\", \"action\": \"{kind}\", \"leader\": 1}}")
            }
            Some((node, parent)) => write!(f, "{{\"node\": \"{node}\", \"parent\": \"{parent}\", \"action\": \"{kind}\"}}"),
        }
    }
}

/// What the tree says about one node.
struct Plan {
    label: Label,
    parent: Label,
    fate: Fate,
    rooted: bool,
    leader: bool,
    held: bool,
    under: Option<Label>,
}

/// Choose each node's fate and modifiers in preorder. `parent_freezes` is whether the parent
/// reaches FROZEN, and `under` the nearest dead block above, if any.
#[allow(clippy::too_many_arguments)]
fn plan(
    nodes: &[Node],
    parent: Label,
    parent_freezes: bool,
    under: Option<Label>,
    canonical: &[Label],
    rng: &mut StdRng,
    plans: &mut Vec<Plan>,
) {
    for node in nodes {
        let label = node.label;
        let rooted = canonical.contains(&label);
        let fate = if rooted {
            Fate::Frozen
        } else if parent_freezes {
            Fate::pick(&Fate::ALL, rng)
        } else {
            Fate::pick(&Fate::AT_INIT, rng)
        };
        let leader = rng.random_ratio(1, LEADER_ONE_IN);
        let held = rng.random_ratio(1, HELD_ONE_IN);
        plans.push(Plan { label, parent, fate, rooted, leader, held, under });
        let dead = if fate.dies() { Some(label) } else { under };
        plan(&node.children, label, fate.freezes(), dead, canonical, rng, plans);
    }
}

/// Put shuffled actions in an order replay could take them in. An action waits until the
/// actions it depends on are placed; every other action keeps its shuffled place.
fn replay_order(shuffled: Vec<Action>, plans: &[Plan], canonical: &[Label]) -> Vec<Action> {
    let root = Label { depth: 0, index: 0 };
    let mut done: Vec<(Label, &'static str)> = vec![(root, NEW_BANK), (root, FINALIZE), (root, ROOT)];
    let mut waiting = Vec::new();
    let mut ordered = Vec::with_capacity(shuffled.len());
    let parent_of = |label: Label| plans.iter().find(|p| p.label == label).map(|p| p.parent);
    let ready = |a: &Action, done: &[(Label, &'static str)]| {
        let Some((node, _)) = a.node else { return true };
        let has = |label: Label, kind: &'static str| done.contains(&(label, kind));
        let parent = parent_of(node).unwrap_or(root);
        match a.kind {
            NEW_BANK => has(parent, NEW_BANK),
            BLOCK_START => has(node, NEW_BANK) && has(parent, FINALIZE),
            FINALIZE => has(node, BLOCK_START),
            DEAD => {
                let fate = plans.iter().find(|p| p.label == node).map(|p| p.fate);
                match fate {
                    Some(Fate::DeadInReplay) => has(node, BLOCK_START),
                    Some(Fate::DeadAfterFreeze) => has(node, FINALIZE),
                    _ => has(node, NEW_BANK),
                }
            }
            ACQUIRE => has(node, NEW_BANK),
            RELEASE => has(node, ACQUIRE),
            ROOT => {
                let i = canonical.iter().position(|&l| l == node).expect("only path nodes are rooted");
                let before = if i == 0 { root } else { canonical[i - 1] };
                has(node, FINALIZE) && has(before, ROOT)
            }
            EVICT => true,
            _ => unreachable!("every node action is listed"),
        }
    };
    for action in shuffled {
        waiting.push(action);
        while let Some(i) = waiting.iter().position(|a| ready(a, &done)) {
            let action = waiting.remove(i);
            if let Some((node, _)) = action.node {
                if action.kind != EVICT {
                    done.push((node, action.kind));
                }
            }
            ordered.push(action);
        }
    }
    assert!(waiting.is_empty(), "every action's dependencies are emitted");
    ordered
}

/// Draw a tree, a node per line, from its plans.
fn draw(nodes: &[Node], plans: &[Plan], prefix: &mut String, out: &mut String) {
    for (i, node) in nodes.iter().enumerate() {
        let last = i + 1 == nodes.len();
        let p = plans.iter().find(|p| p.label == node.label).expect("every node is planned");
        let _ = write!(out, "{prefix}{}{}", if last { "└── " } else { "├── " }, p.label);
        if p.rooted {
            out.push_str("  root");
        }
        let _ = write!(out, "  {}", p.fate.name());
        if p.leader {
            out.push_str("  leader");
        }
        if p.held {
            out.push_str("  held");
        }
        let mut kinds: Vec<&str> = p.fate.actions().to_vec();
        if p.held {
            kinds.extend_from_slice(&[ACQUIRE, RELEASE]);
        }
        if p.rooted {
            kinds.push(ROOT);
        }
        let _ = write!(out, "  {}", kinds.join(" + "));
        if let Some(dead) = p.under {
            let _ = write!(out, "  (under dead {dead})");
        }
        out.push('\n');
        let len = prefix.len();
        prefix.push_str(if last { "    " } else { "│   " });
        draw(&node.children, plans, prefix, out);
        prefix.truncate(len);
    }
}

fn main() -> Result<(), Error> {
    let args = Args::parse();
    let mut rng = match args.seed {
        Some(seed) => StdRng::seed_from_u64(seed),
        None => StdRng::from_os_rng(),
    };
    // Keep the generated trees stable for a seed while choosing fates and modifiers, the
    // node-less actions, and the shuffle.
    let mut plan_rng = match args.seed {
        Some(seed) => StdRng::seed_from_u64(seed ^ 0x706c_616e_0000_0000),
        None => StdRng::from_os_rng(),
    };
    let mut extra_rng = match args.seed {
        Some(seed) => StdRng::seed_from_u64(seed ^ 0x6578_7472_6100_0000),
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
        let root = Label { depth: 0, index: 0 };
        let mut plans = Vec::new();
        plan(&roots, root, true, None, &canonical, &mut plan_rng, &mut plans);

        let mut actions = Vec::new();
        for p in &plans {
            let node = Some((p.label, p.parent));
            for &kind in p.fate.actions() {
                actions.push(Action { node, kind, leader: kind == NEW_BANK && p.leader });
            }
            if p.held {
                actions.push(Action { node, kind: ACQUIRE, leader: false });
                actions.push(Action { node, kind: RELEASE, leader: false });
            }
            if p.rooted {
                actions.push(Action { node, kind: ROOT, leader: false });
            }
            if p.fate.dies() || p.under.is_some() {
                actions.push(Action { node: None, kind: PRUNE, leader: false });
            }
            if extra_rng.random_ratio(1, EVICT_ONE_IN) {
                let protect = if extra_rng.random_bool(0.5) {
                    let q = &plans[extra_rng.random_range(0..plans.len())];
                    Some((q.label, q.parent))
                } else {
                    None
                };
                actions.push(Action { node: protect, kind: EVICT, leader: false });
                actions.push(Action { node: None, kind: PRUNE, leader: false });
            }
        }
        actions.shuffle(&mut shuffle_rng);
        let actions = replay_order(actions, &plans, &canonical);

        let mut tree = String::from("0  root  frozen  init bank\n");
        draw(&roots, &plans, &mut String::new(), &mut tree);
        let path = canonical.iter().map(ToString::to_string).collect::<Vec<_>>().join(" → ");
        let actions: Vec<String> = actions.iter().map(ToString::to_string).collect();
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
