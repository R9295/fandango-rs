//! Generate one lineage of blocks from `grammars/rotor_tree.fan`, with forks, delivered by
//! turbine and repair, and flatten it into an action list for Firedancer's rotor.
//!
//! The grammar picks how many blocks the lineage has, how many FEC sets each has, and how many
//! forks each slot has. The lineage can be any length, bounded only by `--depth`. Each lineage
//! block builds on the one before it, with one in [`GAP_ONE_IN`] slots between them skipped: the
//! lineage has no block there. It skips never so many that the next block falls out of the
//! window. Every lineage block is finalized, but for the tip in one in [`OPEN_TIP_ONE_IN`]
//! lineages, which then either completes or has turbine stop partway.
//!
//! A slot holds at most five blocks: its lineage block and up to four forks, other versions of
//! the slot the leader equivocated, or in a skipped slot up to five forks, as many as
//! [`SKIPPED_FORKS`] picks. A fork builds on the last lineage block before its slot half the
//! time and otherwise on any block before its slot, lineage block or fork, so a fork can have
//! forks after it. It is never finalized and either completes or has turbine stop partway. Blocks are labelled by slot then
//! version as in `consensus_tree`: `a` for the lineage block, `b` on for the forks, also in a
//! skipped slot.
//!
//! Every block reaches rotor through turbine. At most one version of a slot is repairable: from
//! its `REPAIR` action on, which may come at any point, the mock answers rotor's repair requests
//! for it. It refuses every request before then, and every request for any other block. Rotor
//! asks again until a block completes, so a repairable block is still delivered in full when
//! its `REPAIR` comes late, as long as rotor asks for it. A fork is never the one,
//! as a positional repair request names no version: the mock would answer it with the fork's
//! shreds, which conflict with the lineage block. Every finalized lineage block is repairable:
//! turbine alone does not guarantee a block, as the shred tile's FEC resolver may spill a partly
//! received FEC set, which only repair recovers, and the mock sends each turbine shred once. The
//! open tip is repairable one in [`REPAIR_ONE_IN`] times, and always in a slot with forks if
//! turbine never completes it. In a slot with forks, rotor fills the lineage block through turbine
//! only if it has every FEC set before any fork's shred arrives, as a conflicting shred
//! invalidates the slot, so the forks' shreds wait for a lineage block that is not repairable to
//! get its `LAST`. A skipped slot has no lineage block for its forks to conflict with, so one in
//! [`REPAIR_ONE_IN`] skipped slots with forks has one of them repairable, and their shreds wait for
//! nothing.
//!
//! ```text
//! finalized   TURBINE + SEND… + LAST + FINAL
//! implied     TURBINE + SEND… + LAST          finalized only through a later block's FINAL
//! open tip    TURBINE + SEND… (+ LAST)
//! fork        TURBINE + SEND… (+ LAST)
//! repairable  + REPAIR, anywhere among them
//! any         + PART k, after TURBINE and before the rest of FEC set k
//! ```
//!
//! `TURBINE` is the block's first shred, with the header naming its parent. `PART` sends half of a
//! FEC set's data shreds, too few to recover it, so the set stays in progress in the shred tile's
//! FEC resolver, which spills its oldest in-progress set once too many are. One in
//! [`PART_ONE_IN`] FEC sets of a block gets one, after the block's `TURBINE` and before the `SEND`
//! or `LAST` that completes the set, if any. One in [`CODE_ONE_IN`] `SEND`s and `LAST`s sends the
//! set's coding shreds instead of the data shreds it still lacks (`"code": true`), so the shred
//! tile recovers the data shreds by erasure coding. `SEND` completes a
//! FEC set and `LAST` the last one, which gives the slot's eager blk its dmr. `FINAL` is a
//! finalization certificate naming the block by id. It comes only once the block and every
//! block above it is delivered in full or repairable, as one that came earlier would have rotor
//! create a blk by id for a block, which only repair fills. One in [`IMPLIED_ONE_IN`] finalized
//! blocks, other than the last and those the window needs, has no `FINAL` of its own.
//!
//! Every skipped slot before a finalized block is `SKIPPED` once a block after it is finalized.
//!
//! These are the only actions: shreds and certificates, what the node is sent, and `REPAIR`,
//! which only changes what the mock answers. The node roots on its own, every block it finalizes
//! once it holds it in full.
//!
//! The actions are shuffled. Each then waits until the tiles could send it: in its block's
//! order, once the blocks and certificates it needs came, and inside the window of
//! `--slot-max` slots after the deepest finalized block turbine delivered in full, which the
//! node can root. A block whose next one would not fit the window otherwise gets a `FINAL` of its
//! own.
//!
//! The lineage is drawn as a tree, each block under its parent as `consensus_tree` draws it, a
//! block's forks before the lineage goes on. Each block shows `TURBINE` if turbine delivered it
//! and `REPAIR` if it is repairable, and each `SKIPPED` slot is drawn in slot order with
//! `SKIPPED`. A block under a skipped slot names its parent.
//!
//! Each lineage is written as JSON. `blocks` lists each node in slot order with its parent, its
//! number of FEC sets, as `repair` whether it is repairable, and as `replay` whether the node
//! replays it: `yes` if repair always delivers it in full, `no` if neither turbine nor repair
//! can, and `maybe` if that depends on the order rotor sees its slot's versions in, as for every
//! fork, or on whether the FEC resolver spills a set turbine sends once.
//! `actions` names each action's node and parent, or for `SKIPPED` its slot. A `SEND` or `LAST`
//! also names its FEC set as `fec`. With `-o`, each lineage is also written to
//! `<OUT_DIR>/<ITERATION>.json` on one line.
//!
//! Usage:
//! ```text
//! RUSTFLAGS="-Znext-solver" cargo run -p fandango-targets --example rotor_tree -- [-n ITERATIONS] [-s SEED] [-d DEPTH] [-w SLOT_MAX] [-o OUT_DIR]
//! ```

extern crate alloc;

use anyhow::{Context, Error};
use clap::Parser;
use core::convert::Infallible;
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
    #[arg(short, long, default_value = "256")]
    depth: usize,

    /// Slots in rotor's window: the tile drops anything for a slot at or past the root plus this
    #[arg(short = 'w', long, default_value = "16")]
    slot_max: usize,

    /// Directory to write each lineage to as <ITERATION>.json
    #[arg(short, long)]
    out_dir: Option<PathBuf>,
}

/// One in this many slots is skipped: the lineage has no block there.
const GAP_ONE_IN: u32 = 4;

/// One in this many lineages has a tip that is not finalized.
const OPEN_TIP_ONE_IN: u32 = 3;

/// One in this many finalized blocks with a finalized child, other than those the window needs,
/// gets no finalization certificate of its own.
const IMPLIED_ONE_IN: u32 = 4;

/// One in this many open tips is repairable, unless its slot needs it to be. Every finalized
/// lineage block is.
const REPAIR_ONE_IN: u32 = 2;

/// How many forks a skipped slot holds, each with its weight out of their sum: none a third of the
/// time, as the grammar gives a lineage block's slot.
const SKIPPED_FORKS: [(usize, u32); 6] = [(0, 3), (1, 2), (2, 1), (3, 1), (4, 1), (5, 1)];

/// The most FEC sets a block has, as the grammar's `<fecs>`.
const FECS_MAX: u32 = 4;

/// One in this many FEC sets of a block gets a `PART`.
const PART_ONE_IN: u32 = 3;

/// One in this many `SEND`s and `LAST`s completes its FEC sets from coding shreds.
const CODE_ONE_IN: u32 = 3;

/// The first shred of a block arrives from turbine.
const TURBINE: &str = "TURBINE";
/// A FEC set of a block completes.
const SEND: &str = "SEND";
/// The last FEC set of a block completes.
const LAST: &str = "LAST";
/// A finalization certificate names a block.
const FINAL: &str = "FINAL";
/// A finalized block after a slot declares it skipped.
const SKIPPED: &str = "SKIPPED";
/// The mock starts serving repair for a repairable block.
const REPAIR: &str = "REPAIR";
/// Half of a FEC set's data shreds arrive from turbine.
const PART: &str = "PART";

/// A block: the lineage blocks first, block `b` building on block `b - 1` and the first on the
/// genesis root, then the forks.
struct Block {
    slot: usize,
    /// Its version in its slot: 0 for the lineage block, 1 on for its forks.
    version: u8,
    /// The block it builds on, or the genesis root for `None`.
    parent: Option<usize>,
    /// The lineage block of its slot, itself for a lineage block, `None` for a skipped slot.
    lineage: Option<usize>,
    /// Its slot has forks.
    forked: bool,
    fecs: u32,
    /// FEC sets a `SEND` completes, the first ones.
    sends: u32,
    /// Turbine delivers its last FEC set.
    last: bool,
    finalized: bool,
    /// Gets a finalization certificate of its own.
    certified: bool,
    /// The mock answers repair requests for it.
    repairable: bool,
    /// Which of its FEC sets get a `PART`.
    parts: Vec<bool>,
    /// Which of its FEC sets turbine completes from coding shreds, by the `SEND` or `LAST` on it.
    coded: Vec<bool>,
}

/// Block `b`'s label, or the genesis root's for `None`.
fn label(blocks: &[Block], b: Option<usize>) -> String {
    b.map_or_else(|| "0".to_string(), |b| format!("{}{}", blocks[b].slot, char::from(b'a' + blocks[b].version)))
}

/// The label of block `b`'s parent.
fn parent(blocks: &[Block], b: usize) -> String {
    label(blocks, blocks[b].parent)
}

/// Whether the node replays block `b`: `yes`, `no`, or `maybe`.
fn replay(blocks: &[Block], b: usize) -> &'static str {
    let block = &blocks[b];
    if block.version > 0 {
        return "maybe"; /* rotor may take it, or splice it, as the slot's eager blk */
    }
    match (block.finalized, block.forked, block.repairable, block.last) {
        (true, ..) => "yes",
        (false, true, true, _) => "maybe", /* repair may fill another version's eager blk */
        (false, false, true, _) => "yes",
        (false, _, false, true) => "maybe", /* the FEC resolver may spill a set turbine sent once */
        (false, _, false, false) => "no",
    }
}

/// Give each lineage block of `counts` FEC sets and forks' FEC sets a slot, and pick how far
/// turbine delivers it, which blocks are finalized and which get their own certificate. Then
/// pick, from `repair_rng`, which are repairable, and from `fork_rng` each fork's parent and how
/// far turbine delivers it, and how many forks each skipped slot holds.
fn plan(counts: &[(u32, Vec<u32>)], slot_max: usize, rng: &mut StdRng, repair_rng: &mut StdRng, fork_rng: &mut StdRng) -> Vec<Block> {
    let mut slots = Vec::with_capacity(counts.len());
    let mut slot = 0;
    for _ in counts {
        let after = slot;
        slot += 1;
        while slot + 1 < after + slot_max && rng.random_ratio(1, GAP_ONE_IN) {
            slot += 1;
        }
        slots.push(slot);
    }
    let finals = if rng.random_ratio(1, OPEN_TIP_ONE_IN) { counts.len() - 1 } else { counts.len() };
    // The slot of the last block with a certificate of its own, the deepest the node can have
    // finalized before the next block's shreds.
    let mut certified_slot = 0;
    let mut blocks = Vec::with_capacity(counts.len());
    for (b, (fecs, forks)) in counts.iter().enumerate() {
        let (fecs, forked) = (*fecs, !forks.is_empty());
        let finalized = b < finals;
        let last = finalized || rng.random_bool(0.5);
        let next = slots.get(b + 1);
        // Certify at random, and wherever the next block would not fit the window otherwise.
        let needed = next.is_some_and(|&next| next >= certified_slot + slot_max);
        let certified = finalized && (needed || b + 1 == finals || !rng.random_ratio(1, IMPLIED_ONE_IN));
        if certified {
            certified_slot = slots[b];
        }
        let repairable = repair_rng.random_ratio(1, REPAIR_ONE_IN) || finalized || (forked && !last);
        blocks.push(Block {
            slot: slots[b],
            version: 0,
            parent: b.checked_sub(1),
            lineage: Some(b),
            forked,
            fecs,
            sends: if last { fecs - 1 } else { rng.random_range(0..fecs) },
            last,
            finalized,
            certified,
            repairable,
            parts: Vec::new(),
            coded: Vec::new(),
        });
    }
    // Add to `forks` a fork of `fecs` FEC sets in `slot`, at or before lineage block `b`'s,
    // repairable if `repairable`. It builds on lineage block `b - 1` half the time, else on any
    // block before `slot`: a lineage block before `b` or a fork in `forks`.
    let lineage_len = counts.len();
    let fork = |rng: &mut StdRng, forks: &mut Vec<Block>, slot: usize, b: usize, version: u8, fecs: u32, lineage: Option<usize>, repairable: bool| {
        let earlier: Vec<usize> =
            (0..b).chain(forks.iter().enumerate().filter(|(_, f)| f.slot < slot).map(|(i, _)| lineage_len + i)).collect();
        let parent = if earlier.is_empty() || rng.random_bool(0.5) { b.checked_sub(1) } else { Some(earlier[rng.random_range(0..earlier.len())]) };
        let last = rng.random_bool(0.5);
        forks.push(Block {
            slot,
            version,
            parent,
            lineage,
            forked: true,
            fecs,
            sends: if last { fecs - 1 } else { rng.random_range(0..fecs) },
            last,
            finalized: false,
            certified: false,
            repairable,
            parts: Vec::new(),
            coded: Vec::new(),
        });
    };
    // In slot order, so a fork can build on any fork before it.
    let mut forks_of = Vec::new();
    let weight_total: u32 = SKIPPED_FORKS.iter().map(|&(_, weight)| weight).sum();
    for (b, (_, forks)) in counts.iter().enumerate() {
        for slot in b.checked_sub(1).map_or(0, |prev| slots[prev]) + 1..slots[b] {
            let mut pick = repair_rng.random_range(0..weight_total);
            let count = SKIPPED_FORKS.iter().find(|&&(_, weight)| pick < weight || { pick -= weight; false }).map_or(0, |&(count, _)| count);
            let repairable = (count > 0 && repair_rng.random_ratio(1, REPAIR_ONE_IN)).then(|| repair_rng.random_range(0..count));
            for (i, version) in (1..).take(count).enumerate() {
                let fecs = fork_rng.random_range(1..=FECS_MAX);
                fork(fork_rng, &mut forks_of, slot, b, version, fecs, None, repairable == Some(i));
            }
        }
        for (version, &fecs) in (1..).zip(forks) {
            fork(fork_rng, &mut forks_of, slots[b], b, version, fecs, Some(b), false);
        }
    }
    blocks.extend(forks_of);
    blocks
}

/// An action on a block, or for `SKIPPED` on a slot.
#[derive(Clone, Copy)]
struct Action {
    kind: &'static str,
    block: Option<usize>,
    slot: usize,
    /// The FEC set the action is on, if it is on one.
    fec: Option<u32>,
    /// How many of its block's shreds come before it.
    step: usize,
    /// A `SEND` or `LAST` that sends coding shreds instead of the data shreds its sets lack.
    code: bool,
}

impl Action {
    /// The action as the action list writes it.
    fn json(&self, blocks: &[Block]) -> String {
        let Some(b) = self.block else {
            return format!("{{\"slot\": {}, \"action\": \"{SKIPPED}\"}}", self.slot);
        };
        let mut out =
            format!("{{\"node\": \"{}\", \"parent\": \"{}\", \"action\": \"{}\"", label(blocks, Some(b)), parent(blocks, b), self.kind);
        if let Some(fec) = self.fec {
            let _ = write!(out, ", \"fec\": {fec}");
        }
        if self.code {
            out.push_str(", \"code\": true");
        }
        out.push('}');
        out
    }
}

/// Every action of the lineage: each block's shreds and certificate, a `REPAIR` for each
/// repairable block, and a `SKIPPED` for every skipped slot before a finalized block.
fn actions(blocks: &[Block]) -> Vec<Action> {
    let mut actions = Vec::new();
    for (b, block) in blocks.iter().enumerate() {
        let on = |kind, fec: Option<u32>, step| {
            let code = (kind == SEND || kind == LAST) && fec.is_some_and(|k| block.coded[k as usize]);
            Action { kind, block: Some(b), slot: block.slot, fec, step, code }
        };
        actions.push(on(TURBINE, None, 0));
        actions.extend((0..block.sends).map(|fec| on(SEND, Some(fec), fec as usize + 1)));
        if block.last {
            actions.push(on(LAST, Some(block.fecs - 1), block.fecs as usize));
        }
        if block.certified {
            actions.push(on(FINAL, None, 0));
        }
        if block.repairable {
            actions.push(on(REPAIR, None, 0));
        }
        actions.extend((0..block.fecs).filter(|&fec| block.parts[fec as usize]).map(|fec| on(PART, Some(fec), 0)));
    }
    let mut after = 0;
    for block in blocks.iter().filter(|block| block.finalized) {
        actions.extend((after + 1..block.slot).map(|slot| Action { kind: SKIPPED, block: None, slot, fec: None, step: 0, code: false }));
        after = block.slot;
    }
    actions
}

/// The actions taken so far, and what the tiles saw of them.
struct Run<'a> {
    blocks: &'a [Block],
    /// How many lineage blocks there are, the first of `blocks`.
    lineage_len: usize,
    slot_max: usize,
    /// How many shreds of each block have been taken, `PART`s aside.
    taken: Vec<usize>,
    /// Which `PART`s of each block have been taken, by FEC set.
    parted: Vec<Vec<bool>>,
    lasts: Vec<bool>,
    /// How many lineage blocks from the first rotor has delivered in full: those with their
    /// `LAST` taken, up to the first without.
    delivered: usize,
    /// The deepest block whose `FINAL` was taken.
    finalized: Option<usize>,
    log: Vec<Action>,
}

impl<'a> Run<'a> {
    fn new(blocks: &'a [Block], slot_max: usize) -> Self {
        Self {
            blocks,
            lineage_len: blocks.iter().filter(|block| block.version == 0).count(),
            slot_max,
            taken: vec![0; blocks.len()],
            parted: blocks.iter().map(|block| vec![false; block.fecs as usize]).collect(),
            lasts: vec![false; blocks.len()],
            delivered: 0,
            finalized: None,
            log: Vec::new(),
        }
    }

    /// Whether `a` can be taken now: inside the window, in its block's order, and once the
    /// blocks and certificates it needs came.
    fn ready(&self, a: &Action) -> bool {
        // The node roots each block it finalizes once it holds it in full, so the window can start
        // at the deepest finalized block turbine delivered. Repair may finish later.
        let root = match self.finalized {
            Some(f) if self.delivered > 0 => self.blocks[f.min(self.delivered - 1)].slot,
            _ => 0,
        };
        if a.slot >= root + self.slot_max {
            return false;
        }
        let Some(b) = a.block else {
            return self.finalized.is_some_and(|f| self.blocks[f].slot > a.slot);
        };
        // A fork's shreds wait for its slot's lineage block, unless repair delivers that.
        let waits = match self.blocks[b].lineage {
            Some(l) if self.blocks[b].version > 0 => !self.blocks[l].repairable && !self.lasts[l],
            _ => false,
        };
        match a.kind {
            FINAL => (0..=b).all(|i| self.lasts[i] || self.blocks[i].repairable),
            REPAIR => true,
            _ if waits => false,
            PART => self.taken[b] >= 1,
            /* the rest of a FEC set waits for its PART */
            _ => self.taken[b] == a.step && a.fec.is_none_or(|k| !self.blocks[b].parts[k as usize] || self.parted[b][k as usize]),
        }
    }

    /// Take `a` now.
    fn take(&mut self, a: Action) {
        if let Some(b) = a.block {
            match a.kind {
                FINAL => self.finalized = self.finalized.max(Some(b)),
                REPAIR => {}
                PART => self.parted[b][a.fec.expect("a PART names its FEC set") as usize] = true,
                kind => {
                    self.taken[b] += 1;
                    if kind == LAST {
                        self.lasts[b] = true;
                        while self.delivered < self.lineage_len && self.lasts[self.delivered] {
                            self.delivered += 1;
                        }
                    }
                }
            }
        }
        self.log.push(a);
    }

    /// Take shuffled actions in an order the tiles could send them in. An action waits until
    /// it is ready; every other action keeps its shuffled place.
    fn run(&mut self, shuffled: Vec<Action>) {
        let mut waiting = Vec::new();
        for action in shuffled {
            waiting.push(action);
            while let Some(i) = waiting.iter().position(|a| self.ready(a)) {
                self.take(waiting.remove(i));
            }
        }
        assert!(waiting.is_empty(), "every action becomes ready");
    }

    /// Whether turbine delivered block `b`: its `TURBINE` was taken.
    fn turbine(&self, b: usize) -> bool {
        self.log.iter().any(|a| a.block == Some(b) && a.kind == TURBINE)
    }

    /// Draw block `b` on one line after `prefix`, the last of its siblings if `last`.
    fn line(&self, out: &mut String, prefix: &str, b: usize, last: bool, under_skip: bool) {
        let _ = write!(out, "{prefix}{}{}", if last { "└── " } else { "├── " }, label(self.blocks, Some(b)));
        if self.turbine(b) {
            let _ = write!(out, "  {TURBINE}");
        }
        if self.blocks[b].repairable {
            out.push_str("  REPAIR");
        }
        if under_skip {
            let _ = write!(out, "  (parent {})", parent(self.blocks, b));
        }
        out.push('\n');
    }

    /// Draw fork `f` after `prefix`, the last of its siblings if `last`, and the forks on it under
    /// it.
    fn subtree(&self, out: &mut String, prefix: &str, f: usize, last: bool) {
        self.line(out, prefix, f, last, false);
        let children = self.forks_on(Some(f));
        let prefix = format!("{prefix}{}", if last { "    " } else { "│   " });
        for (i, &c) in children.iter().enumerate() {
            self.subtree(out, &prefix, c, i + 1 == children.len());
        }
    }

    /// The forks that build on block `b`, or on the genesis root for `None`, in slot order.
    fn forks_on(&self, b: Option<usize>) -> Vec<usize> {
        let mut forks: Vec<usize> = (self.lineage_len..self.blocks.len()).filter(|&f| self.blocks[f].parent == b).collect();
        forks.sort_by_key(|&f| (self.blocks[f].slot, self.blocks[f].version));
        forks
    }

    /// Draw the lineage as a tree, a block or skipped slot per line. Under each block come the
    /// forks on it, in slot order, each with the forks on it, and then the lineage goes on, through the skipped slots before its
    /// next block, each under the one before it as `consensus_tree` draws. A block under a
    /// skipped slot names its parent.
    fn draw(&self) -> String {
        let skipped: Vec<usize> = self.log.iter().filter(|a| a.kind == SKIPPED).map(|a| a.slot).collect();
        let mut out = String::from("0\n");
        let mut prefix = String::new();
        let mut after = 0;
        let mut node = None;
        loop {
            let next = node.map_or(0, |b| b + 1);
            let forks = self.forks_on(node);
            for (i, &f) in forks.iter().enumerate() {
                self.subtree(&mut out, &prefix, f, i + 1 == forks.len() && next == self.lineage_len);
            }
            if next == self.lineage_len {
                return out;
            }
            let slot = self.blocks[next].slot;
            let mut under_skip = false;
            for skip in (after + 1..slot).filter(|skip| skipped.contains(skip)) {
                let _ = writeln!(out, "{prefix}└── {skip}  {SKIPPED}");
                prefix.push_str("    ");
                under_skip = true;
            }
            self.line(&mut out, &prefix, next, true, under_skip);
            prefix.push_str("    ");
            after = slot;
            node = Some(next);
        }
    }
}

/// The blocks as the action list writes them, in slot order.
fn blocks_json(blocks: &[Block]) -> Vec<String> {
    let mut order: Vec<usize> = (0..blocks.len()).collect();
    order.sort_by_key(|&b| (blocks[b].slot, blocks[b].version));
    order
        .into_iter()
        .map(|b| {
            let (node, parent, block) = (label(blocks, Some(b)), parent(blocks, b), &blocks[b]);
            let replay = replay(blocks, b);
            format!("{{\"node\": \"{node}\", \"parent\": \"{parent}\", \"fecs\": {}, \"repair\": {}, \"replay\": \"{replay}\"}}", block.fecs, block.repairable)
        })
        .collect()
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
    anyhow::ensure!(args.slot_max >= 2, "a window needs at least 2 slots");
    let mut rng = stream(args.seed, 0);
    // Keep the generated lineages stable for a seed while choosing slots and fates, which blocks
    // are repairable, the forks' parents and fates, which FEC sets get a PART, which arrive as
    // coding shreds, and how the actions are shuffled.
    let mut fate_rng = stream(args.seed, 0x6661_7465_0000_0000);
    let mut repair_rng = stream(args.seed, 0x7265_7061_6972_0000);
    let mut fork_rng = stream(args.seed, 0x666f_726b_0000_0000);
    let mut part_rng = stream(args.seed, 0x7061_7274_0000_0000);
    let mut code_rng = stream(args.seed, 0x636f_6465_0000_0000);
    let mut shuffle_rng = stream(args.seed, 0x7368_7566_666c_6500);
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
        let fecs = |block: &Value| -> Result<u32, Error> {
            let fecs = block["fecs"].as_u64().context("a block without FEC sets")?;
            u32::try_from(fecs).context("a block with too many FEC sets")
        };
        let counts = json["blocks"]
            .as_array()
            .context("a lineage without blocks")?
            .iter()
            .map(|block| {
                let forks = block["forks"].as_array().context("a block without forks")?;
                Ok((fecs(block)?, forks.iter().map(fecs).collect::<Result<Vec<_>, Error>>()?))
            })
            .collect::<Result<Vec<_>, Error>>()?;

        let mut blocks = plan(&counts, args.slot_max, &mut fate_rng, &mut repair_rng, &mut fork_rng);
        for block in &mut blocks {
            block.parts = (0..block.fecs).map(|_| part_rng.random_ratio(1, PART_ONE_IN)).collect();
            block.coded = (0..block.fecs).map(|_| code_rng.random_ratio(1, CODE_ONE_IN)).collect();
        }
        let mut actions = actions(&blocks);
        actions.shuffle(&mut shuffle_rng);
        let mut run = Run::new(&blocks, args.slot_max);
        run.run(actions);

        let nodes = blocks_json(&blocks);
        let actions: Vec<String> = run.log.iter().map(|a| a.json(&blocks)).collect();
        println!("lineage {iteration:06}\n{}", run.draw());

        if let Some(dir) = &args.out_dir {
            let path = dir.join(format!("{iteration:06}.json"));
            fs::write(&path, format!("{{\"blocks\": [{}], \"actions\": [{}]}}\n", nodes.join(", "), actions.join(", ")))
                .with_context(|| format!("could not write {}", path.display()))?;
        }
    }

    match &args.out_dir {
        Some(dir) => println!("Wrote {} lineages to {}.", args.iterations, dir.display()),
        None => println!("Drew {} lineages.", args.iterations),
    }
    Ok(())
}
