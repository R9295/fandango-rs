//! Generate single-lineage, turbine-driven block chains from `grammars/rotor_tree.fan` and
//! flatten them into action lists for Firedancer's rotor.
//!
//! The lineage is one chain of blocks from the root, a block per slot, each labelled by its
//! slot then `a`, as in `consensus_tree`. The grammar picks how long it is and how many FEC
//! sets each block would have. Every block is the slot's eager block: it is only ever built
//! from turbine, so nothing here is named by a certificate before its dmr is known, repaired,
//! or deduplicated. Forks, notar and repair driven blocks, and our own leader blocks are
//! left out.
//!
//! A block takes one of the paths of the rotor model, in this order:
//!
//! ```text
//! eager → final   TURBINE + SEND… + LAST(dmr) + FINAL
//! eager           TURBINE + SEND… + INVALID    two FEC sets at one index: it stops, never completes
//! eager           TURBINE + SEND… + SKIPPED    freed by a skip: never completes
//! ```
//!
//! `TURBINE` is the first shred, which creates the eager block and links it to its parent,
//! `SEND` completes a FEC set and `LAST` completes the last one, which gives the block its
//! dmr. A block never completes, and so ends the lineage, if it is invalid or skipped: its
//! children could only wait for a parent they never see, and be named once it is finalized.
//!
//! A block's `TURBINE` can arrive before its parent finishes. Rotor only delivers a FEC set
//! once the parent has been delivered in full, so a block whose FEC sets complete earlier
//! `WAIT`s on its parent, and the drawing says so. A `FINAL` comes only once the dmr of the
//! block and of every block above it is known, as a certificate that came earlier would name
//! a block rotor does not have yet and make it by repair.
//!
//! The lineage is flattened into a shuffled JSON list of actions, as `consensus_tree` does,
//! each naming its node and the block that node cites as its parent. A `SEND`, `LAST` or
//! `INVALID` also names its FEC set as `fec`. The shuffle keeps each block's actions in the
//! order above and every `FINAL` after the `LAST` of its block and of those above it. Between
//! each pair of actions, the clock advances by one of [`CLOCK_STEPS_MS`], picked by its weight.
//! With `-o`, each action list is also written to `<OUT_DIR>/<ITERATION>.json` on one line.
//!
//! Usage:
//! ```text
//! RUSTFLAGS="-Znext-solver" cargo run -p fandango-targets --example rotor_tree -- [-n ITERATIONS] [-s SEED] [-d DEPTH] [-o OUT_DIR]
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

/// Base for the grammar stored in `rotor_tree.fan`.
#[derive(Fandango)]
#[fandango(grammar = "grammars/rotor_tree.fan", parse = false)]
pub struct RotorTree(Infallible);

/// Generate lineages and draw them.
#[derive(Parser)]
struct Args {
    /// Number of lineages to generate
    #[arg(short = 'n', long, default_value = "5")]
    iterations: usize,

    /// Reproducible RNG seed [default: OS entropy]
    #[arg(short, long)]
    seed: Option<u64>,

    /// Grammar depth limit, which bounds how many blocks a lineage can have
    #[arg(short, long, default_value = "40")]
    depth: usize,

    /// Directory to write each lineage's action list to as <ITERATION>.json
    #[arg(short, long)]
    out_dir: Option<PathBuf>,
}

/// One in this many lineages ends in a block that never completes.
const DEAD_TIP_ONE_IN: u32 = 4;

/// The first shred of a block arrives from turbine.
const TURBINE: &str = "TURBINE";
/// A FEC set of a block completes.
const SEND: &str = "SEND";
/// The last FEC set of a block completes, which gives it its dmr.
const LAST: &str = "LAST";
/// A block is finalized.
const FINAL: &str = "FINAL";
/// A block's leader signs two FEC sets at one index.
const INVALID: &str = "INVALID";
/// A block's slot is declared skipped, which frees the block.
const SKIPPED: &str = "SKIPPED";

/// Milliseconds the clock advances between two actions, each with its weight out of
/// [`CLOCK_WEIGHT_TOTAL`]: 50 ms and 100 ms 45% of the time each, 500 ms 10%.
const CLOCK_STEPS_MS: [(u64, u32); 3] = [(50, 9), (100, 9), (500, 2)];
/// The sum of the weights in [`CLOCK_STEPS_MS`].
const CLOCK_WEIGHT_TOTAL: u32 = 20;

/// A node's label: `0` for the root, else its depth, which is its slot, then its index among
/// the nodes of its depth, in letters, as in `consensus_tree`. A lineage has one node per
/// depth, so the index is always `a`.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Label {
    depth: usize,
    index: usize,
}

/// The root, which every lineage extends.
const ROOT: Label = Label { depth: 0, index: 0 };

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

/// How a block ends.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Fate {
    /// Completes, then is finalized.
    Final,
    /// Never completes: its leader signs two FEC sets at one index.
    Invalid,
    /// Never completes: a skip frees it.
    Skipped,
}

/// A block of the lineage.
#[derive(Clone, Copy)]
struct Block {
    label: Label,
    parent: Label,
    fate: Fate,
    /// FEC sets the block has once complete, which it never reaches unless it is final.
    fecs: u32,
    /// FEC sets completed by a `SEND`, the first ones: all but the last if the block
    /// completes, fewer than `fecs` if it does not.
    sends: u32,
    /// The index that gets two FEC sets if the block is invalid, one that has a FEC set.
    conflict: u32,
}

impl Block {
    /// The actions on the block, in the order it takes them.
    fn actions(self) -> Vec<Action> {
        let Self { label, parent, fate, fecs, sends, conflict } = self;
        let mut kinds = vec![(TURBINE, None)];
        kinds.extend((0..sends).map(|fec| (SEND, Some(fec))));
        match fate {
            Fate::Final => kinds.extend([(LAST, Some(fecs - 1)), (FINAL, None)]),
            Fate::Invalid => kinds.push((INVALID, Some(conflict))),
            Fate::Skipped => kinds.push((SKIPPED, None)),
        }
        kinds
            .into_iter()
            .enumerate()
            .map(|(step, (kind, fec))| Action { node: label, parent, kind, fec, step })
            .collect()
    }
}

/// Make a block of each `fecs` count, in order. All but possibly the tip are final: a block that
/// never completes has no child that can.
fn plan(counts: &[u32], rng: &mut StdRng) -> Vec<Block> {
    let tip_dies = rng.random_ratio(1, DEAD_TIP_ONE_IN);
    let mut parent = ROOT;
    let mut blocks = Vec::with_capacity(counts.len());
    for (i, &fecs) in counts.iter().enumerate() {
        let label = Label { depth: i + 1, index: 0 };
        let dies = tip_dies && i + 1 == counts.len();
        let fate = match (dies, rng.random_bool(0.5)) {
            (false, _) => Fate::Final,
            (true, true) => Fate::Invalid,
            (true, false) => Fate::Skipped,
        };
        let sends = if dies { rng.random_range(0..fecs) } else { fecs - 1 };
        let conflict = rng.random_range(0..sends.max(1));
        blocks.push(Block { label, parent, fate, fecs, sends, conflict });
        parent = label;
    }
    blocks
}

/// An action on `node`, which cites `parent`.
#[derive(Clone, Copy)]
struct Action {
    node: Label,
    parent: Label,
    kind: &'static str,
    /// The FEC set the action is on, if it is on one.
    fec: Option<u32>,
    /// How many actions on the node come before this one.
    step: usize,
}

/// The action as `consensus_tree` writes it, with the FEC set it names.
impl fmt::Display for Action {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self { node, parent, kind, fec, .. } = self;
        write!(f, "{{\"node\": \"{node}\", \"parent\": \"{parent}\", \"action\": \"{kind}\"")?;
        if let Some(fec) = fec {
            write!(f, ", \"fec\": {fec}")?;
        }
        f.write_str("}")
    }
}

/// Put shuffled actions in an order rotor could see them in: a block's actions come in the
/// order it takes them, and its `FINAL` waits for the `LAST` of the block and of every block
/// above it. Every other action keeps its shuffled place.
fn order(shuffled: Vec<Action>, blocks: usize) -> Vec<Action> {
    let mut taken = vec![0; blocks + 1];
    let mut lasts = vec![false; blocks + 1];
    let ready = |a: &Action, taken: &[usize], lasts: &[bool]| {
        taken[a.node.depth] == a.step && (a.kind != FINAL || lasts[1..=a.node.depth].iter().all(|&last| last))
    };
    let mut waiting = Vec::new();
    let mut ordered = Vec::with_capacity(shuffled.len());
    for action in shuffled {
        waiting.push(action);
        while let Some(i) = waiting.iter().position(|a| ready(a, &taken, &lasts)) {
            let action = waiting.remove(i);
            taken[action.node.depth] += 1;
            if action.kind == LAST {
                lasts[action.node.depth] = true;
            }
            ordered.push(action);
        }
    }
    assert!(waiting.is_empty(), "every action's predecessors are emitted");
    ordered
}

/// Which blocks wait on their parent, by depth: those with a `SEND` or `LAST` before the parent
/// is delivered in full. Rotor delivers a block once it is complete and its parent is
/// delivered, and the root always is.
fn waits(ordered: &[Action], blocks: usize) -> Vec<bool> {
    let mut complete = vec![false; blocks + 1];
    let mut delivered = vec![false; blocks + 1];
    let mut waits = vec![false; blocks + 1];
    delivered[0] = true;
    for action in ordered {
        let depth = action.node.depth;
        waits[depth] |= (action.kind == SEND || action.kind == LAST) && !delivered[depth - 1];
        complete[depth] |= action.kind == LAST;
        for depth in 1..=blocks {
            delivered[depth] |= complete[depth] && delivered[depth - 1];
        }
    }
    waits
}

/// Draw the lineage, a block per line, in the order of `ordered`.
///
/// Each block shows its actions the way the model lists them. Its `TURBINE` says so if it comes
/// before its parent finishes, and a `WAIT` follows if its FEC sets complete before the parent
/// is delivered.
fn draw(blocks: &[Block], ordered: &[Action]) -> String {
    let waits = waits(ordered, blocks.len());
    let at = |label: Label, kind: &str| ordered.iter().position(|a| a.node == label && a.kind == kind);
    let mut out = String::from("0\n");
    for (i, block) in blocks.iter().enumerate() {
        let Block { label, parent, fate, sends, conflict, .. } = *block;
        let early = at(label, TURBINE).zip(at(parent, LAST)).is_some_and(|(turbine, last)| turbine < last);
        let mut steps = vec![if early { format!("{TURBINE} (before {parent} finishes)") } else { TURBINE.to_string() }];
        if waits[label.depth] {
            steps.push(format!("WAIT({parent})"));
        }
        match sends {
            0 => {}
            1 => steps.push(SEND.to_string()),
            _ => steps.push(format!("{SEND}…")),
        }
        let (role, outcome) = match fate {
            Fate::Final => {
                steps.extend([format!("{LAST}(dmr)"), FINAL.to_string()]);
                ("eager → final", "✔")
            }
            Fate::Invalid => {
                steps.push(format!("{INVALID} (two FEC sets at index {conflict})"));
                ("eager", "stops, never completes")
            }
            Fate::Skipped => {
                steps.push(SKIPPED.to_string());
                ("eager", "freed")
            }
        };
        let _ = writeln!(out, "{}└── {label}  {role}  {}  {outcome}", "    ".repeat(i), steps.join(" + "));
    }
    out
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

/// A reproducible RNG for `seed` set apart by `salt`, or one from OS entropy without a seed.
fn stream(seed: Option<u64>, salt: u64) -> StdRng {
    match seed {
        Some(seed) => StdRng::seed_from_u64(seed ^ salt),
        None => StdRng::from_os_rng(),
    }
}

fn main() -> Result<(), Error> {
    let args = Args::parse();
    let mut rng = stream(args.seed, 0);
    // Keep the generated lineages stable for a seed while choosing how each one ends, how the
    // actions are shuffled, and how far the clock steps between them.
    let mut fate_rng = stream(args.seed, 0x6661_7465_0000_0000);
    let mut shuffle_rng = stream(args.seed, 0x7368_7566_666c_6500);
    let mut clock_rng = stream(args.seed, 0x636c_6f63_6b00_0000);
    let mut generators = tuple_list!(DepthLimiter::new(STRUCTURE.inner(), args.depth));

    if let Some(dir) = &args.out_dir {
        fs::create_dir_all(dir).with_context(|| format!("could not create {}", dir.display()))?;
    }

    for iteration in 1..=args.iterations {
        let scenario = nonterminal_start::generate(&mut rng, &mut generators, 0);
        let bytes = WriteVisitor::new(Vec::new())
            .visit(&scenario, 0)?
            .continue_value()
            .expect("write visitor never breaks")
            .output();
        let json: Value = serde_json::from_slice(&bytes).context("the grammar generates JSON")?;
        let counts = json["blocks"]
            .as_array()
            .context("a lineage without blocks")?
            .iter()
            .map(|block| {
                let fecs = block["fecs"].as_u64().context("a block without FEC sets")?;
                u32::try_from(fecs).context("a block with too many FEC sets")
            })
            .collect::<Result<Vec<_>, Error>>()?;

        let blocks = plan(&counts, &mut fate_rng);
        let mut actions: Vec<Action> = blocks.iter().flat_map(|block| block.actions()).collect();
        actions.shuffle(&mut shuffle_rng);
        let actions = order(actions, blocks.len());
        let tree = draw(&blocks, &actions);

        let mut stepped = Vec::with_capacity(2 * actions.len());
        for (i, action) in actions.iter().enumerate() {
            if i > 0 {
                let ms = clock_step(&mut clock_rng);
                stepped.push(format!("{{\"action\": \"CLOCK\", \"ms\": {ms}}}"));
            }
            stepped.push(action.to_string());
        }
        let actions = stepped;
        let path = blocks
            .iter()
            .map(|block| match block.fate {
                Fate::Final => block.label.to_string(),
                Fate::Invalid => format!("{} (invalid)", block.label),
                Fate::Skipped => format!("{} (skipped)", block.label),
            })
            .collect::<Vec<_>>()
            .join(" → ");
        println!("lineage {iteration:06}\n{tree}path  {path}\nactions\n[\n  {}\n]\n", actions.join(",\n  "));

        if let Some(dir) = &args.out_dir {
            let path = dir.join(format!("{iteration:06}.json"));
            fs::write(&path, format!("[{}]\n", actions.join(", ")))
                .with_context(|| format!("could not write {}", path.display()))?;
        }
    }

    match &args.out_dir {
        Some(dir) => println!("Wrote {} lineages to {}.", args.iterations, dir.display()),
        None => println!("Drew {} lineages.", args.iterations),
    }
    Ok(())
}
