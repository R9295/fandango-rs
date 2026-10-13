//! Generate block trees from `grammars/rotor_tree.fan` and flatten them into action lists for
//! Firedancer's rotor.
//!
//! The grammar picks a tree of blocks and how many FEC sets each has, pruned to at most
//! [`MAX_WIDTH`] blocks per depth. Slots go to blocks in a random order that keeps each block
//! after its parent, with one in [`GAP_ONE_IN`] slots left empty. One in [`VERSION_ONE_IN`]
//! blocks goes in the slot of the block placed before it, as another version of that slot, the
//! way an equivocating leader makes them, up to [`MAX_VERSIONS`] a slot. Each block is labelled
//! by its slot, then its index among its slot's versions in letters, as in `consensus_tree`. The
//! deepest path, the lowest slot on a tie, is finalized, and the other versions of its slots
//! lose. One in [`LEADER_ONE_IN`] slots with one version is our own leader slot.
//!
//! A block reaches rotor through any of these, each a sequence of actions:
//!
//! ```text
//! turbine   TURBINE + SEND… + LAST                       completes the slot's eager blk
//!           TURBINE + SEND…                              never completes
//!           TURBINE + SEND… + INVALID (+ SEND… + LAST)   invalidates its slot, the rest ignored
//! leader    LEADER + SEND… + LAST                        our own block, never repaired
//! certs     NOTAR, FINAL                                 name the block by id
//! repair    REPAIR + SEND_K… (+ KLAST)                   repairs the blk rotor holds by its id
//! replay    DEAD or KILL, ROOT
//! ```
//!
//! `TURBINE` is the block's first shred, with the header naming its parent. `SEND` completes a
//! FEC set and `LAST` the last one, which gives an eager blk its dmr. `INVALID` is a shred that
//! invalidates the slot (see [`Reason`]). Turbine delivers at most one version of a slot, and
//! in one in [`NO_TURBINE_ONE_IN`] slots none. `LEADER` is the first shred of our own block,
//! whose `SEND` and `LAST` are our own FEC sets. We only build on a block replay has in full,
//! replay never fails it or anything above it, and nothing under it arrives before we made it.
//!
//! `NOTAR` and `FINAL` are certificates naming the block's id. One in [`NOTAR_ONE_IN`] blocks
//! is notarized. Every finalized block gets its own `FINAL`, except one in [`IMPLIED_ONE_IN`]
//! with a finalized child that is not ours, which is finalized only through that child, unless
//! it is rooted. One in [`VOTED_ONE_IN`] finalized blocks that turbine delivers in full gets its
//! `FINAL` only after its `LAST`. Rotor creates a blk by id for a certificate, or for a parent
//! that a blk by id names (`NAMED`), unless it already holds one. `REPAIR` is the
//! `ParentAndFecSetCount` response for that blk, and `SEND_K` a `FecSetRoot` response followed by
//! the FEC set's shreds, `KLAST` for the last one. They come in order, and only while rotor
//! holds the blk by id. The finalized path is repaired in full, every other block up to a
//! random FEC set, and one in [`NO_REPAIR_ONE_IN`] not at all. Repair shreds reach the slot's
//! eager blk too, unless the slot is settled or invalidated: they build one when there is none,
//! and invalidate the slot when it holds another version.
//!
//! `DEAD` is replay rejecting a block whose id it knows: from the block's first FEC set
//! delivered by id, or once its eager blk is delivered in full. `KILL` is replay failing a
//! block it has part of before it knows the id. Replay sends rotor nothing for it and drops the
//! rest of the block and everything under it, so a harness only uses `KILL` for its model of
//! replay. One in [`DEAD_ONE_IN`] blocks off the finalized path gets a `DEAD`, and one in
//! [`KILL_ONE_IN`] a `KILL`, if replay gets that far, but never under a block replay already
//! failed.
//!
//! Every slot between two finalized blocks, empty or not, is `SKIPPED` once a block after it is
//! finalized. `ROOT` is replay rooting one in [`ROOT_ONE_IN`] finalized blocks, in order, once
//! it has them in full, and every one the window needs to reach the next finalized block.
//!
//! The actions are shuffled. Each then waits until the tiles could send it: in its sequence's
//! order, inside the window of `--slot-max` slots after the root, and once rotor or replay got
//! far enough. An action that never could is left out. Rotor runs as a port of `fd_rotor.c`,
//! which decides what each action does. The tile drops anything at or below the root, and the
//! shreds and `REPAIR` of a block whose parent is below it, so those are left out of the
//! action list too. A tree is not drawn if its finalized path does not fit the window, or if
//! rotor would need more blks than it holds for the window.
//!
//! Each block is drawn with the roles it took in rotor, its actions with what came of them,
//! and how it ended. Between the actions are what rotor did to it:
//! - `WAIT(P)`: a FEC set waits on its parent.
//! - `NAMED (by X)`: a child's blk created it.
//! - `DEDUP`: its eager blk absorbed its blk by id.
//! - `ORPHAN`: it lost its parent.
//! - `FINAL (via X)`: a finalized child promoted it.
//! - `INVALIDATED (by X's …)`: another block's shred invalidated its slot.
//! - `dropped by replay`: it was delivered under a block replay failed.
//!
//! Each tree is written as JSON. `blocks` lists each node with its parent and its number of
//! FEC sets, and `"leader": true` for our own. `actions` names each action's node and parent,
//! or for `SKIPPED` its slot. A `SEND`, `LAST`, `SEND_K`, `KLAST` or `INVALID` also names its
//! FEC set as `fec`, and an `INVALID` its `reason`. With `-o`, each tree is also written to
//! `<OUT_DIR>/<ITERATION>.json` on one line.
//!
//! Usage:
//! ```text
//! RUSTFLAGS="-Znext-solver" cargo run -p fandango-targets --example rotor_tree -- [-n ITERATIONS] [-s SEED] [-d DEPTH] [-w SLOT_MAX] [-o OUT_DIR]
//! ```

extern crate alloc;

use anyhow::{Context, Error};
use clap::Parser;
use core::cmp::Reverse;
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
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;

/// Base for the grammar stored in `rotor_tree.fan`.
#[derive(Fandango)]
#[fandango(grammar = "grammars/rotor_tree.fan", parse = false)]
pub struct RotorTree(Infallible);

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
    #[arg(short, long, default_value = "24")]
    depth: usize,

    /// Slots in rotor's window: the tile drops anything for a slot at or past the root plus this
    #[arg(short = 'w', long, default_value = "16")]
    slot_max: usize,

    /// Directory to write each tree to as <ITERATION>.json
    #[arg(short, long)]
    out_dir: Option<PathBuf>,
}

/// Most blocks across an entire depth of a tree.
const MAX_WIDTH: usize = 3;

/// One in this many slots is left empty.
const GAP_ONE_IN: u32 = 4;

/// One in this many blocks is another version of the slot of the block placed before it.
const VERSION_ONE_IN: u32 = 3;

/// Most versions of a slot.
const MAX_VERSIONS: usize = 3;

/// One in this many slots with one version is our own leader slot.
const LEADER_ONE_IN: u32 = 8;

/// One in this many slots that are not ours has no version turbine delivers.
const NO_TURBINE_ONE_IN: u32 = 4;

/// One in this many blocks gets a notarization certificate.
const NOTAR_ONE_IN: u32 = 2;

/// One in this many finalized blocks with a finalized child, not rooted, gets no finalization
/// certificate of its own.
const IMPLIED_ONE_IN: u32 = 4;

/// One in this many blocks off the finalized path never gets a `ParentAndFecSetCount` response.
const NO_REPAIR_ONE_IN: u32 = 4;

/// One in this many finalized blocks is rooted by replay, besides those the window needs.
const ROOT_ONE_IN: u32 = 2;

/// One in this many blocks off the finalized path is rejected by replay with a `DEAD`.
const DEAD_ONE_IN: u32 = 3;

/// One in this many blocks off the finalized path is failed by replay before it knows its id.
const KILL_ONE_IN: u32 = 4;

/// One in this many finalized blocks turbine delivers in full is finalized only after its last
/// FEC set arrives, as the cluster votes once it has the block.
const VOTED_ONE_IN: u32 = 2;

/// One in this many invalid blocks has the rest of its shreds arrive anyway.
const LATE_ONE_IN: u32 = 2;

/// The first shred of a block arrives from turbine.
const TURBINE: &str = "TURBINE";
/// The first shred of our own block.
const LEADER: &str = "LEADER";
/// A FEC set of a block completes, from turbine or our own.
const SEND: &str = "SEND";
/// The last FEC set of a block completes, from turbine or our own.
const LAST: &str = "LAST";
/// A shred invalidates its block's slot.
const INVALID: &str = "INVALID";
/// A notarization certificate names a block.
const NOTAR: &str = "NOTAR";
/// A finalization certificate names a block.
const FINAL: &str = "FINAL";
/// A `ParentAndFecSetCount` response for a block rotor holds by id.
const REPAIR: &str = "REPAIR";
/// A `FecSetRoot` response and the shreds of a FEC set of a block rotor holds by id.
const SEND_K: &str = "SEND_K";
/// `SEND_K` for the block's last FEC set.
const KLAST: &str = "KLAST";
/// Replay rejects a block it knows the id of.
const DEAD: &str = "DEAD";
/// Replay fails a block before it knows its id, and tells rotor nothing.
const KILL: &str = "KILL";
/// Replay roots a finalized block.
const ROOT: &str = "ROOT";
/// A finalized block after a slot declares it skipped.
const SKIPPED: &str = "SKIPPED";

/// Why a shred invalidates its block's slot, as `fd_rotor_shred_insert` checks them.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Reason {
    /// A second FEC set at an index that has one.
    Eqvoc,
    /// A shred at a new index whose parent is another slot, at or above the root.
    Parent,
    /// A shred of a FEC set that says the slot ends there, below the highest one received.
    Complete,
    /// The block's first shred is shred 0 with a bad block header.
    Header,
}

impl Reason {
    /// The reason as the action list names it.
    fn name(self) -> &'static str {
        match self {
            Reason::Eqvoc => "eqvoc",
            Reason::Parent => "parent",
            Reason::Complete => "complete",
            Reason::Header => "header",
        }
    }

    /// What the shred at FEC set `fec` says, for the drawing.
    fn what(self, fec: u32) -> String {
        match self {
            Reason::Eqvoc => format!("two FEC sets at index {fec}"),
            Reason::Parent => format!("another parent at index {fec}"),
            Reason::Complete => format!("slot ends at index {fec}, below the highest"),
            Reason::Header => "bad block header".to_string(),
        }
    }
}

/// A node's label: `0` for the root, else its slot then its index among the versions of its
/// slot, in letters, as in `consensus_tree`.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Label {
    slot: usize,
    index: usize,
}

/// The root every tree grows from.
const GENESIS: Label = Label { slot: 0, index: 0 };

impl fmt::Display for Label {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.slot == 0 {
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
        write!(f, "{}", self.slot)?;
        letters.iter().rev().try_for_each(|letter| write!(f, "{letter}"))
    }
}

/// A block as the grammar generates it, before it has a slot.
struct Shape {
    fecs: u32,
    children: Vec<Shape>,
}

/// Read a `children` list at `depth + 1`, emptying it when it would make its depth wider than
/// [`MAX_WIDTH`], as `consensus_tree` does.
fn prune(list: &[Value], depth: usize, counts: &mut Vec<usize>) -> Result<Vec<Shape>, Error> {
    let depth = depth + 1;
    if counts.len() <= depth {
        counts.resize(depth + 1, 0);
    }
    if list.len() > MAX_WIDTH - counts[depth] {
        return Ok(Vec::new());
    }
    counts[depth] += list.len();
    list.iter()
        .map(|node| {
            let fecs = node["fecs"].as_u64().context("a block without FEC sets")?;
            let children = node["children"].as_array().context("a block without children")?;
            Ok(Shape { fecs: u32::try_from(fecs).context("a block with too many FEC sets")?, children: prune(children, depth, counts)? })
        })
        .collect()
}

/// A block with its slot, its parent's index if it is not the root, and its depth.
struct Slotted {
    label: Label,
    parent: Option<usize>,
    depth: usize,
    fecs: u32,
}

/// Give each block a slot, in a random order that keeps it after its parent, leaving one in
/// [`GAP_ONE_IN`] slots empty and putting one in [`VERSION_ONE_IN`] blocks in the slot before
/// it. The blocks come back in slot order.
fn assign(roots: Vec<Shape>, rng: &mut StdRng) -> Vec<Slotted> {
    let mut open: Vec<(Shape, Option<usize>, usize)> = roots.into_iter().map(|shape| (shape, None, 1)).collect();
    let mut slotted: Vec<Slotted> = Vec::new();
    let mut slot = 0;
    let mut versions = 0;
    while !open.is_empty() {
        let (shape, parent, depth) = open.swap_remove(rng.random_range(0..open.len()));
        let after = parent.map_or(0, |p| slotted[p].label.slot);
        if !(slot > after && versions < MAX_VERSIONS && rng.random_ratio(1, VERSION_ONE_IN)) {
            slot += 1;
            while rng.random_ratio(1, GAP_ONE_IN) {
                slot += 1;
            }
            versions = 0;
        }
        let index = slotted.len();
        slotted.push(Slotted { label: Label { slot, index: versions }, parent, depth, fecs: shape.fecs });
        versions += 1;
        open.extend(shape.children.into_iter().map(|child| (child, Some(index), depth + 1)));
    }
    slotted
}

/// The blocks from the root to the deepest one, the lowest slot on a tie, root excluded.
fn canonical(slotted: &[Slotted]) -> Vec<usize> {
    let tip = (0..slotted.len()).max_by_key(|&b| (slotted[b].depth, Reverse(b))).expect("a nonempty tree");
    let mut path: Vec<usize> = core::iter::successors(Some(tip), |&b| slotted[b].parent).collect();
    path.reverse();
    path
}

/// How many blks rotor holds with a window of `slot_max` slots, `fd_rotor_blk_max`: it fails
/// an assertion when it needs one more.
fn blk_max(slot_max: usize) -> usize {
    (slot_max * 5).div_ceil(2)
}

/// Whether each finalized block is within `slot_max` slots of the one before it, so that
/// rooting that one brings it into the window.
fn fits(slotted: &[Slotted], canonical: &[usize], slot_max: usize) -> bool {
    let mut before = 0;
    canonical.iter().all(|&c| {
        let slot = slotted[c].label.slot;
        let fits = slot < before + slot_max;
        before = slot;
        fits
    })
}

/// How turbine delivers a block.
#[derive(Clone, Copy)]
enum Turbine {
    /// Every FEC set.
    Complete,
    /// Its first `sends` FEC sets.
    Incomplete { sends: u32 },
    /// Its first `sends` FEC sets, then a shred at FEC set `fec` that invalidates the slot for
    /// `reason`, then the rest of its shreds anyway if `late`.
    Invalid { reason: Reason, sends: u32, fec: u32, late: bool },
}

/// What replay does to a block besides replaying it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum End {
    Kept,
    Dead,
    Kill,
}

/// A block of the tree, and how it reaches rotor and replay.
#[allow(clippy::struct_excessive_bools)] /* each is its own choice of the plan */
struct Block {
    label: Label,
    parent: Option<usize>,
    fecs: u32,
    /// On the finalized path.
    canonical: bool,
    /// Our own block.
    leader: bool,
    turbine: Option<Turbine>,
    notar: bool,
    /// Gets a finalization certificate of its own.
    certified: bool,
    /// Its finalization certificate comes after turbine delivers it.
    voted: bool,
    /// How many of its FEC sets repair by id completes, if it gets a `ParentAndFecSetCount`.
    repair: Option<u32>,
    end: End,
    rooted: bool,
}

/// Pick how each block reaches rotor: our leader slots, the version turbine delivers in each
/// other slot, the certificates, repair, and what replay does.
fn plan(slotted: Vec<Slotted>, canonical: &[usize], slot_max: usize, rng: &mut StdRng) -> Vec<Block> {
    let n = slotted.len();
    let mut versions: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for (b, block) in slotted.iter().enumerate() {
        versions.entry(block.label.slot).or_default().push(b);
    }
    let mut leader = vec![false; n];
    let mut turbine = vec![false; n];
    for list in versions.values() {
        if list.len() == 1 && rng.random_ratio(1, LEADER_ONE_IN) {
            leader[list[0]] = true;
        } else if !rng.random_ratio(1, NO_TURBINE_ONE_IN) {
            turbine[list[rng.random_range(0..list.len())]] = true;
        }
    }
    // Replay never fails a block we built on, nor anything above it.
    let mut built_on = vec![false; n];
    for b in (0..n).filter(|&b| leader[b]) {
        for a in core::iter::successors(slotted[b].parent, |&a| slotted[a].parent) {
            built_on[a] = true;
        }
    }
    // Root at random, and wherever the next finalized block would not fit the window.
    let mut rooted = vec![false; n];
    let mut root = 0;
    for (i, &c) in canonical.iter().enumerate() {
        let next = canonical.get(i + 1).map(|&next| slotted[next].label.slot);
        rooted[c] = rng.random_ratio(1, ROOT_ONE_IN) || next.is_some_and(|next| next >= root + slot_max);
        if rooted[c] {
            root = slotted[c].label.slot;
        }
    }
    slotted
        .into_iter()
        .enumerate()
        .map(|(b, Slotted { label, parent, fecs, .. })| {
            let on_path = canonical.contains(&b);
            // Only a child that repair names its parent through can finalize it: not our own,
            // which we only build once replay has the parent in full.
            let next = canonical.iter().position(|&c| c == b).and_then(|i| canonical.get(i + 1));
            let named = next.is_some_and(|&next| !leader[next]);
            let turbine = turbine[b].then(|| match rng.random_range(0..4) {
                0 | 1 => Turbine::Complete,
                2 => Turbine::Incomplete { sends: rng.random_range(0..fecs) },
                _ => invalid(fecs, rng),
            });
            Block {
                label,
                parent,
                fecs,
                canonical: on_path,
                leader: leader[b],
                turbine,
                voted: on_path && matches!(turbine, Some(Turbine::Complete)) && rng.random_ratio(1, VOTED_ONE_IN),
                notar: !leader[b] && rng.random_ratio(1, NOTAR_ONE_IN),
                certified: on_path && (rooted[b] || !named || !rng.random_ratio(1, IMPLIED_ONE_IN)),
                repair: if leader[b] {
                    None
                } else if on_path {
                    Some(fecs)
                } else if rng.random_ratio(1, NO_REPAIR_ONE_IN) {
                    None
                } else {
                    Some(rng.random_range(0..=fecs))
                },
                end: if on_path || leader[b] || built_on[b] {
                    End::Kept
                } else {
                    let roll = rng.random_range(0..DEAD_ONE_IN * KILL_ONE_IN);
                    if roll < KILL_ONE_IN {
                        End::Dead
                    } else if roll < KILL_ONE_IN + DEAD_ONE_IN {
                        End::Kill
                    } else {
                        End::Kept
                    }
                },
                rooted: rooted[b],
            }
        })
        .collect()
}

/// Pick how turbine's version of a block of `fecs` FEC sets is invalidated, among the reasons
/// it has room for: a slot end below the highest FEC set needs two received, so three in the
/// block.
fn invalid(fecs: u32, rng: &mut StdRng) -> Turbine {
    let reasons: &[Reason] = if fecs >= 3 {
        &[Reason::Eqvoc, Reason::Parent, Reason::Complete, Reason::Header]
    } else {
        &[Reason::Eqvoc, Reason::Parent, Reason::Header]
    };
    let reason = reasons[rng.random_range(0..reasons.len())];
    let (sends, fec) = match reason {
        Reason::Eqvoc => {
            let sends = rng.random_range(0..fecs);
            (sends, rng.random_range(0..sends.max(1)))
        }
        Reason::Parent => {
            let sends = rng.random_range(0..fecs);
            (sends, sends.max(1))
        }
        Reason::Complete => {
            let sends = rng.random_range(2..fecs);
            (sends, rng.random_range(0..sends - 1))
        }
        Reason::Header => (0, 0),
    };
    Turbine::Invalid { reason, sends, fec, late: rng.random_ratio(1, LATE_ONE_IN) }
}

/// The sequences of a block's actions, each in its own order: its shreds, its repair by id, its
/// certificates and what replay does.
const CHAINS: usize = 5;
const SHREDS: usize = 0;
const REPAIRS: usize = 1;
const NOTARS: usize = 2;
const FINALS: usize = 3;
const ENDS: usize = 4;

impl Block {
    /// The actions on block `b`, this one, each in its sequence.
    fn actions(&self, b: usize) -> Vec<Action> {
        let n = self.fecs;
        let sends = |from: u32, to: u32| (from..to).map(|fec| (SEND, Some(fec), None));
        let mut shreds: Vec<(&'static str, Option<u32>, Option<Reason>)> = Vec::new();
        if self.leader {
            shreds.push((LEADER, None, None));
            shreds.extend(sends(0, n - 1));
            shreds.push((LAST, Some(n - 1), None));
        } else if let Some(turbine) = self.turbine {
            match turbine {
                Turbine::Complete => {
                    shreds.push((TURBINE, None, None));
                    shreds.extend(sends(0, n - 1));
                    shreds.push((LAST, Some(n - 1), None));
                }
                Turbine::Incomplete { sends: count } => {
                    shreds.push((TURBINE, None, None));
                    shreds.extend(sends(0, count));
                }
                Turbine::Invalid { reason, sends: count, fec, late } => {
                    if reason != Reason::Header {
                        shreds.push((TURBINE, None, None));
                    }
                    shreds.extend(sends(0, count));
                    shreds.push((INVALID, Some(fec), Some(reason)));
                    if late {
                        shreds.extend(sends(count, n - 1));
                        shreds.push((LAST, Some(n - 1), None));
                    }
                }
            }
        }
        let repairs = self.repair.map_or_else(Vec::new, |count| {
            core::iter::once((REPAIR, None, None))
                .chain((0..count).map(|fec| (if fec + 1 == n { KLAST } else { SEND_K }, Some(fec), None)))
                .collect()
        });
        let one = |kind: &'static str, on: bool| if on { vec![(kind, None, None)] } else { Vec::new() };
        let end = match self.end {
            End::Kept => Vec::new(),
            End::Dead => one(DEAD, true),
            End::Kill => one(KILL, true),
        };
        [(SHREDS, shreds), (REPAIRS, repairs), (NOTARS, one(NOTAR, self.notar)), (FINALS, one(FINAL, self.certified)), (ENDS, end)]
            .into_iter()
            .flat_map(|(chain, kinds)| {
                kinds.into_iter().enumerate().map(move |(step, (kind, fec, reason))| Action {
                    kind,
                    block: Some(b),
                    slot: self.label.slot,
                    fec,
                    reason,
                    chain: b * CHAINS + chain,
                    step,
                })
            })
            .collect()
    }
}

/// A `ROOT` for each rooted block, in order, as one more sequence after every block's.
fn root_actions(blocks: &[Block], canonical: &[usize]) -> Vec<Action> {
    canonical
        .iter()
        .filter(|&&c| blocks[c].rooted)
        .enumerate()
        .map(|(step, &c)| Action {
            kind: ROOT,
            block: Some(c),
            slot: blocks[c].label.slot,
            fec: None,
            reason: None,
            chain: blocks.len() * CHAINS,
            step,
        })
        .collect()
}

/// A `SKIPPED` for every slot between two blocks of the finalized path.
fn skips(blocks: &[Block], canonical: &[usize]) -> Vec<Action> {
    let mut skips = Vec::new();
    let mut after = 0;
    for &c in canonical {
        for slot in after + 1..blocks[c].label.slot {
            skips.push(Action { kind: SKIPPED, block: None, slot, fec: None, reason: None, chain: 0, step: 0 });
        }
        after = blocks[c].label.slot;
    }
    skips
}

/// An action on a block, or for `SKIPPED` on a slot.
#[derive(Clone, Copy)]
struct Action {
    kind: &'static str,
    block: Option<usize>,
    slot: usize,
    /// The FEC set the action is on, if it is on one.
    fec: Option<u32>,
    /// Why an `INVALID` invalidates the slot.
    reason: Option<Reason>,
    /// The sequence it is in, and how many actions of it come before.
    chain: usize,
    step: usize,
}

impl Action {
    /// Whether the tile drops this action when the parent of its block is below the root.
    fn needs_parent(&self) -> bool {
        matches!(self.kind, TURBINE | LEADER | SEND | LAST | INVALID | REPAIR | SEND_K | KLAST)
    }

    /// The action as the action list writes it.
    fn json(&self, blocks: &[Block]) -> String {
        let Some(b) = self.block else {
            return format!("{{\"slot\": {}, \"action\": \"{SKIPPED}\"}}", self.slot);
        };
        let parent = blocks[b].parent.map_or(GENESIS, |p| blocks[p].label);
        let mut out = format!("{{\"node\": \"{}\", \"parent\": \"{parent}\", \"action\": \"{}\"", blocks[b].label, self.kind);
        if let Some(fec) = self.fec {
            let _ = write!(out, ", \"fec\": {fec}");
        }
        if let Some(reason) = self.reason {
            let _ = write!(out, ", \"reason\": \"{}\"", reason.name());
        }
        out.push('}');
        out
    }
}

/// A block id as rotor holds it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mr {
    /// None yet: an eager blk before it completes.
    Null,
    /// The genesis root's.
    Genesis,
    /// The id of a block of the tree.
    Block(usize),
}

/// A FEC set rotor holds, `fd_rotor_fec_t`: FEC set `k` of `block`.
#[derive(Clone, PartialEq)]
struct Fec {
    block: usize,
    k: u32,
    complete: bool,
}

/// A blk rotor holds, `fd_rotor_blk_t`, as much of it as delivering FEC sets needs.
#[derive(Clone, PartialEq)]
struct Blk {
    slot: usize,
    dmr: Mr,
    parent: Option<usize>,
    /// Newest first, as rotor links them.
    children: Vec<usize>,
    cmpl: u32,
    rcvd: u32,
    buff: u32,
    cons: u32,
    parent_slot: Option<usize>,
    parent_mr: Mr,
    fecs: BTreeMap<u32, usize>,
    connected: i8,
}

/// A slot's roles and state, `fd_rotor_slot_meta_t`.
#[derive(Clone, Copy, PartialEq, Default)]
struct Meta {
    slot: Option<usize>,
    eager: Option<usize>,
    notar: Option<usize>,
    fin: Option<usize>,
    invalidated: bool,
    skipped: bool,
    leader: bool,
}

/// What a block's shreds tell rotor about it.
#[derive(Clone, Copy, PartialEq)]
struct Info {
    slot: usize,
    parent_slot: usize,
    parent_mr: Mr,
    fecs: u32,
}

/// A role in a slot a blk is promoted to.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Role {
    Notar,
    Final,
}

/// Why a blk was freed or lost its parent.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Why {
    /// Another version of its slot, the block given, was finalized.
    Final(usize),
    /// It completed as a block other than its slot's finalized one.
    Dedup,
    /// It was the copy by id of a block its eager blk completed as.
    Merged,
    /// Replay rejected it.
    Dead,
    /// Its slot was skipped.
    Skipped,
    /// The root, the block given, passed it.
    Root(usize),
    /// Its parent's slot completed as another block, the one given.
    Completed(usize),
}

/// Something rotor did to a block, for the drawing.
#[derive(Clone, Copy, PartialEq)]
enum Note {
    /// A certificate, `NOTAR` or `FINAL`, created a blk for it.
    Created { block: usize, by: &'static str },
    /// A blk by id of a child, the block given if known, created a blk for it.
    Named { block: usize, by: Option<usize> },
    Role { block: usize, role: Role },
    /// Its eager blk completed and has its dmr.
    Dmr { block: usize },
    /// Its eager blk absorbed its blk by id.
    Dedup { block: usize },
    Freed { block: usize, why: Why },
    Orphan { block: usize, why: Why },
    /// Complete FEC sets of it wait on its parent.
    Wait { block: usize },
    /// A shred of another block, the one given, invalidated the slot its eager blk holds.
    Invalidated { block: usize, by: Option<usize> },
    /// Replay drops its FEC sets as they build on a failed block, the one given.
    Drops { block: usize, failed: usize },
}

impl Note {
    fn block(self) -> usize {
        match self {
            Note::Created { block, .. }
            | Note::Named { block, .. }
            | Note::Role { block, .. }
            | Note::Dmr { block }
            | Note::Dedup { block }
            | Note::Freed { block, .. }
            | Note::Orphan { block, .. }
            | Note::Wait { block }
            | Note::Invalidated { block, .. }
            | Note::Drops { block, .. } => block,
        }
    }
}

/// A port of `fd_rotor.c`, the parts that decide which blks rotor holds, how they link and
/// which FEC sets it delivers. Each function is named after the one it ports. Repair
/// requests, telemetry and the reasm deque's order are left out.
#[derive(Clone, PartialEq)]
struct Model {
    slot_max: usize,
    infos: Vec<Info>,
    root: usize,
    root_blk: Option<usize>,
    metas: Vec<Meta>,
    /// Every blk ever created, `None` once freed.
    blks: Vec<Option<Blk>>,
    /// The blks of each slot, newest first.
    map: BTreeMap<usize, Vec<usize>>,
    fecs: Vec<Option<Fec>>,
    fec_map: BTreeMap<(usize, u32), usize>,
    /// The FEC sets delivered to replay, in order: the blk, its FEC set, and its block.
    delivered: Vec<(usize, u32, usize)>,
    /// The block whose action is being taken.
    cause: Option<usize>,
    notes: Vec<Note>,
}

impl Model {
    /// `fd_rotor_init` at the genesis root.
    fn new(infos: Vec<Info>, slot_max: usize) -> Self {
        let mut model = Model {
            slot_max,
            infos,
            root: 0,
            root_blk: None,
            metas: vec![Meta::default(); slot_max],
            blks: Vec::new(),
            map: BTreeMap::new(),
            fecs: Vec::new(),
            fec_map: BTreeMap::new(),
            delivered: Vec::new(),
            cause: None,
            notes: Vec::new(),
        };
        let root = model.blk_insert(0, Mr::Genesis);
        model.meta(0).fin = Some(root);
        model.meta(0).eager = None;
        model.blk_mut(root).connected = 1;
        model.root_blk = Some(root);
        model
    }

    fn blk(&self, b: usize) -> &Blk {
        self.blks[b].as_ref().expect("a held blk")
    }

    fn blk_mut(&mut self, b: usize) -> &mut Blk {
        self.blks[b].as_mut().expect("a held blk")
    }

    fn fec(&self, f: usize) -> &Fec {
        self.fecs[f].as_ref().expect("a held FEC set")
    }

    /// `fd_rotor_slot_meta`: the slot's entry, reset if it held another slot.
    fn meta(&mut self, slot: usize) -> &mut Meta {
        let meta = &mut self.metas[slot % self.slot_max];
        if meta.slot != Some(slot) {
            *meta = Meta { slot: Some(slot), ..Meta::default() };
        }
        meta
    }

    /// The slot's entry, without resetting it.
    fn peek(&self, slot: usize) -> Meta {
        let meta = self.metas[slot % self.slot_max];
        if meta.slot == Some(slot) { meta } else { Meta::default() }
    }

    /// The blks rotor holds.
    fn held(&self) -> impl Iterator<Item = usize> + '_ {
        (0..self.blks.len()).filter(|&b| self.blks[b].is_some())
    }

    /// The block blk `b` holds: its id, or for an eager blk the block of its FEC sets.
    fn version(&self, b: usize) -> Option<usize> {
        let blk = self.blk(b);
        match blk.dmr {
            Mr::Block(block) => Some(block),
            Mr::Genesis => None,
            Mr::Null => blk.fecs.values().next().map(|&f| self.fec(f).block),
        }
    }

    fn in_slot(&self, slot: usize) -> Vec<usize> {
        self.map.get(&slot).cloned().unwrap_or_default()
    }

    /// The first blk of `slot` with id `mr`.
    fn find(&self, slot: usize, mr: Mr) -> Option<usize> {
        self.in_slot(slot).into_iter().find(|&b| self.blk(b).dmr == mr)
    }

    fn link(&mut self, b: usize, parent: usize) {
        self.blk_mut(b).parent = Some(parent);
        self.blk_mut(parent).children.insert(0, b);
    }

    /// Take blk `b` out of its parent's children.
    fn cut(&mut self, b: usize) {
        if let Some(parent) = self.blk(b).parent {
            self.blk_mut(parent).children.retain(|&c| c != b);
        }
    }

    fn blk_insert(&mut self, slot: usize, dmr: Mr) -> usize {
        let b = self.blks.len();
        self.blks.push(Some(Blk {
            slot,
            dmr,
            parent: None,
            children: Vec::new(),
            cmpl: 0,
            rcvd: 0,
            buff: 0,
            cons: 0,
            parent_slot: None,
            parent_mr: Mr::Null,
            fecs: BTreeMap::new(),
            connected: 0,
        }));
        self.map.entry(slot).or_default().insert(0, b);
        let meta = self.meta(slot);
        if dmr == Mr::Null {
            meta.eager = Some(b);
        } else if meta.notar.is_none() {
            meta.notar = Some(b);
        }
        b
    }

    fn fec_insert(&mut self, block: usize, k: u32) -> usize {
        if let Some(&f) = self.fec_map.get(&(block, k)) {
            return f;
        }
        let f = self.fecs.len();
        self.fecs.push(Some(Fec { block, k, complete: false }));
        self.fec_map.insert((block, k), f);
        f
    }

    /// The dmr of a complete eager blk, which holds the FEC sets of one block, as turbine and
    /// repair never splice two.
    fn compute_dmr(&mut self, b: usize) {
        let block = self.version(b).expect("a complete blk holds FEC sets");
        let (blk, info) = (self.blk(b), self.infos[block]);
        let whole = (0..blk.cmpl).all(|k| self.fec(blk.fecs[&k]).block == block);
        assert!(
            whole && blk.cmpl == info.fecs && blk.parent_slot == Some(info.parent_slot) && blk.parent_mr == info.parent_mr,
            "the eager blk of slot {} spliced two blocks",
            blk.slot
        );
        self.blk_mut(b).dmr = Mr::Block(block);
        self.notes.push(Note::Dmr { block });
    }

    /// Advance blk `b`'s complete prefix of FEC sets.
    fn buffer(&mut self, b: usize) {
        let blk = self.blk(b);
        let end = if blk.cmpl > 0 { blk.cmpl } else { u32::MAX };
        let mut buff = blk.buff;
        while buff < end && blk.fecs.get(&buff).is_some_and(|&f| self.fec(f).complete) {
            buff += 1;
        }
        self.blk_mut(b).buff = buff;
    }

    fn consume(&mut self, b: usize) {
        let slot = self.blk(b).slot;
        let invalidated = self.meta(slot).invalidated;
        let blk = self.blk(b);
        if invalidated && blk.dmr == Mr::Null {
            return;
        }
        let Some(parent) = blk.parent else { return };
        let parent = self.blk(parent);
        if blk.cons == 0 && blk.parent != self.root_blk && (parent.cmpl == 0 || parent.cons != parent.cmpl) {
            return;
        }
        let (cons, buff) = (blk.cons, blk.buff);
        if let Some(block) = self.version(b) {
            self.delivered.extend((cons..buff).map(|k| (b, k, block)));
        }
        self.blk_mut(b).cons = buff;
    }

    fn cascade(&mut self, b: usize) {
        for c in self.blk(b).children.clone() {
            self.consume(c);
            let child = self.blk(c);
            if child.cmpl > 0 && child.cons == child.cmpl && !child.children.is_empty() {
                self.cascade(c);
            }
        }
    }

    /// Consume blk `b`, and cascade if that delivered it in full.
    fn deliver(&mut self, b: usize) {
        self.consume(b);
        let blk = self.blk(b);
        if blk.cmpl > 0 && blk.cons == blk.cmpl {
            self.cascade(b);
        }
    }

    fn promote(&mut self, b: usize, role: Role) {
        let slot = self.blk(b).slot;
        let meta = self.meta(slot);
        let held = match role {
            Role::Notar => &mut meta.notar,
            Role::Final => &mut meta.fin,
        };
        if *held == Some(b) {
            return;
        }
        *held = Some(b);
        if let Some(block) = self.version(b) {
            self.notes.push(Note::Role { block, role });
        }
    }

    fn connect_ancestors(&mut self, mut b: usize) -> usize {
        loop {
            self.blk_mut(b).connected = -1;
            let root = self.root;
            let Some(parent_slot) = self.blk(b).parent_slot.filter(|&slot| slot >= root) else { break };
            let slot = self.blk(b).slot;
            let fin = self.meta(slot).fin == Some(b);
            let linked = self.blk(b).parent;
            let parent_mr = self.blk(b).parent_mr;
            let mut parent = self
                .in_slot(parent_slot)
                .into_iter()
                .find(|&p| self.blk(p).dmr == parent_mr && (parent_slot == root || parent_mr != Mr::Null));
            if parent.is_none() && parent_slot == root {
                break; /* names a root other than ours */
            }
            if parent.is_none() {
                parent = self.blk_notarized(parent_slot, parent_mr);
                if let (Some(_), Mr::Block(block)) = (parent, parent_mr) {
                    self.notes.push(Note::Named { block, by: self.cause });
                }
            }
            let Some(parent) = parent else { break };
            if linked != Some(parent) {
                if linked.is_some() {
                    self.cut(b);
                }
                self.link(b, parent);
            }
            if fin && self.meta(parent_slot).fin.is_none() {
                self.promote(parent, Role::Final);
            }
            self.deliver(b);
            if self.blk(parent).connected == 1 {
                break;
            }
            b = parent;
        }
        b
    }

    fn connect_descendants(&mut self, b: usize) {
        let (slot, parent) = (self.blk(b).slot, self.blk(b).parent);
        let rooted = slot == self.root && self.meta(slot).fin == Some(b);
        let connected = i8::from(rooted || parent.is_some_and(|p| self.blk(p).connected == 1));
        if self.blk(b).connected == connected {
            return;
        }
        self.blk_mut(b).connected = connected;
        self.spread(b, connected);
    }

    /// Copy `connected` down blk `b`'s subtree, skipping children that already match.
    fn spread(&mut self, b: usize, connected: i8) {
        for c in self.blk(b).children.clone() {
            if self.blk(c).connected != connected {
                self.blk_mut(c).connected = connected;
                self.spread(c, connected);
            }
        }
    }

    fn unlink(&mut self, b: usize, why: Why) {
        self.cut(b);
        for c in core::mem::take(&mut self.blk_mut(b).children) {
            self.blk_mut(c).parent = None;
            self.connect_descendants(c);
            if self.blk(c).slot > self.root
                && let Some(block) = self.version(c)
            {
                self.notes.push(Note::Orphan { block, why });
            }
        }
    }

    fn prune(&mut self, b: usize, why: Why) {
        let (slot, block) = (self.blk(b).slot, self.version(b));
        let others: Vec<usize> = self.in_slot(slot).into_iter().filter(|&o| o != b).collect();
        for (k, f) in self.blk(b).fecs.clone() {
            if others.iter().any(|&o| self.blk(o).fecs.get(&k) == Some(&f)) {
                continue;
            }
            let fec = self.fecs[f].take().expect("a held FEC set");
            self.fec_map.remove(&(fec.block, fec.k));
        }
        self.unlink(b, why);
        let meta = self.meta(slot);
        for role in [&mut meta.eager, &mut meta.notar, &mut meta.fin] {
            if *role == Some(b) {
                *role = None;
            }
        }
        self.forget(b);
        if let Some(block) = block {
            self.notes.push(Note::Freed { block, why });
        }
    }

    /// Take blk `b` out of the map and free it.
    fn forget(&mut self, b: usize) {
        let slot = self.blk(b).slot;
        let list = self.map.get_mut(&slot).expect("a held blk's slot");
        list.retain(|&o| o != b);
        if list.is_empty() {
            self.map.remove(&slot);
        }
        self.blks[b] = None;
    }

    fn dedup(&mut self, eager: usize) {
        let (slot, dmr) = (self.blk(eager).slot, self.blk(eager).dmr);
        let notar = self.in_slot(slot).into_iter().find(|&b| b != eager && self.blk(b).dmr == dmr);
        let Some(notar) = notar else {
            if self.meta(slot).fin.is_some() {
                self.prune(eager, Why::Dedup); /* turbine built a blk the cluster did not finalize */
            }
            return;
        };
        for c in core::mem::take(&mut self.blk_mut(notar).children) {
            self.link(c, eager);
            self.connect_descendants(c);
        }
        if let Some(parent) = self.blk(notar).parent
            && self.blk(eager).parent.is_none_or(|p| self.blk(p).dmr == Mr::Null)
        {
            if self.blk(eager).parent.is_some() {
                self.cut(eager);
            }
            self.link(eager, parent);
            self.connect_descendants(eager);
        }
        if self.meta(slot).fin == Some(notar) {
            self.meta(slot).fin = None;
            self.promote(eager, Role::Final);
        }
        if self.meta(slot).notar == Some(notar) {
            self.meta(slot).notar = None;
            self.promote(eager, Role::Notar);
        }
        if let Mr::Block(block) = dmr {
            self.notes.push(Note::Dedup { block });
        }
        self.prune(notar, Why::Merged);
        let root = self.connect_ancestors(eager);
        self.connect_descendants(root);
    }

    fn blk_dead(&mut self, slot: usize, mr: Mr) {
        let Some(dead) = self.find(slot, mr) else { return };
        if self.meta(slot).eager == Some(dead) {
            self.meta(slot).invalidated = true; /* turbine must not rebuild it */
        }
        self.prune(dead, Why::Dead);
    }

    fn blk_finalized(&mut self, slot: usize, mr: Mr) {
        let Mr::Block(winner) = mr else { unreachable!("a certificate names a block") };
        let mut fin = self.find(slot, mr);
        if fin.is_none() {
            fin = self.blk_notarized(slot, mr);
            if fin.is_some() {
                self.notes.push(Note::Created { block: winner, by: FINAL });
            }
        }
        let Some(fin) = fin else { return }; /* the slot is already finalized with another blk */
        self.promote(fin, Role::Final);
        for b in self.in_slot(slot).into_iter().filter(|&b| b != fin) {
            for c in core::mem::take(&mut self.blk_mut(b).children) {
                if self.blk(c).parent_mr == mr {
                    self.link(c, fin);
                } else {
                    self.blk_mut(c).parent = None;
                    if let Some(block) = self.version(c) {
                        self.notes.push(Note::Orphan { block, why: Why::Final(winner) });
                    }
                }
                self.connect_descendants(c);
            }
            if self.meta(slot).eager == Some(b) && self.blk(b).dmr == Mr::Null {
                continue; /* turbine may still complete as the finalized blk, see dedup */
            }
            self.prune(b, Why::Final(winner));
        }
        let root = self.connect_ancestors(fin);
        self.connect_descendants(root);
        let blk = self.blk(fin);
        if blk.cmpl > 0 && blk.cons == blk.cmpl {
            self.cascade(fin);
        }
    }

    fn blk_notarized(&mut self, slot: usize, mr: Mr) -> Option<usize> {
        if mr == Mr::Null {
            return None;
        }
        let meta = *self.meta(slot);
        if meta.fin.is_some() || meta.skipped {
            return None; /* the slot is settled */
        }
        if let Some(dup) = self.find(slot, mr) {
            if self.meta(slot).notar.is_none() {
                self.promote(dup, Role::Notar);
            }
            let root = self.connect_ancestors(dup);
            self.connect_descendants(root);
            return None;
        }
        Some(self.blk_insert(slot, mr))
    }

    fn blk_parented(&mut self, slot: usize, mr: Mr, parent_slot: usize, parent_mr: Mr, fecs: u32) {
        let Some(b) = self.find(slot, mr) else { return };
        if self.blk(b).parent_slot.is_some() || parent_slot >= slot {
            return;
        }
        let blk = self.blk_mut(b);
        blk.parent_slot = Some(parent_slot);
        blk.parent_mr = parent_mr;
        blk.cmpl = fecs;
        let eager = self.meta(parent_slot).eager;
        let mut parent = self
            .in_slot(parent_slot)
            .into_iter()
            .find(|&p| self.blk(p).dmr == parent_mr && !(Some(p) == eager && self.blk(p).dmr == Mr::Null));
        let created = parent.is_none();
        if created {
            parent = self.blk_notarized(parent_slot, parent_mr);
            if let (Some(_), Mr::Block(block)) = (parent, parent_mr) {
                self.notes.push(Note::Named { block, by: self.cause });
            }
        }
        let Some(parent) = parent else { return };
        self.link(b, parent);
        self.connect_descendants(b);
        if self.meta(slot).fin == Some(b) {
            self.promote(parent, Role::Final); /* finalized ancestry */
        }
        self.deliver(b);
        if !created {
            let root = self.connect_ancestors(parent);
            self.connect_descendants(root);
        }
    }

    fn fec_complete(&mut self, block: usize, k: u32, leader: bool) {
        let info = self.infos[block];
        self.shred_insert(block, k, false, k + 1 == info.fecs);
        let Some(&f) = self.fec_map.get(&(block, k)) else { return };
        self.fecs[f].as_mut().expect("a held FEC set").complete = true;
        let slot = info.slot;
        self.meta(slot).leader |= leader;
        let meta = *self.meta(slot);
        let complete = meta.eager.filter(|&e| {
            let blk = self.blk(e);
            !meta.invalidated
                && blk.cmpl > 0
                && blk.dmr == Mr::Null
                && (0..blk.cmpl).all(|j| blk.fecs.get(&j).is_some_and(|&g| self.fec(g).complete))
        });
        if let Some(eager) = complete {
            self.compute_dmr(eager);
            let dmr = self.blk(eager).dmr;
            let mut keep = Vec::new();
            for c in core::mem::take(&mut self.blk_mut(eager).children) {
                if self.blk(c).parent_mr == dmr {
                    keep.insert(0, c);
                    continue;
                }
                self.blk_mut(c).parent = None;
                self.connect_descendants(c);
                if let (Some(child), Mr::Block(completed)) = (self.version(c), dmr) {
                    self.notes.push(Note::Orphan { block: child, why: Why::Completed(completed) });
                }
            }
            self.blk_mut(eager).children = keep;
            self.dedup(eager);
            if self.fec_map.get(&(block, k)) != Some(&f) {
                return; /* dedup pruned eager and with it the FEC set */
            }
        }
        for b in self.in_slot(slot) {
            if self.blk(b).fecs.get(&k) != Some(&f) {
                continue;
            }
            self.buffer(b);
            self.deliver(b);
        }
    }

    fn fec_notarized(&mut self, slot: usize, mr: Mr, block: usize, k: u32) {
        let Some(b) = self.find(slot, mr) else { return };
        if self.blk(b).fecs.contains_key(&k) {
            return; /* duplicate response */
        }
        let eager = self.meta(slot).eager;
        let eqvoc = eager.is_some_and(|e| {
            self.blk(e).fecs.get(&k).is_some_and(|&f| {
                let fec = self.fec(f);
                (fec.block, fec.k) != (block, k)
            })
        });
        if eqvoc {
            self.slot_invalidated(slot);
        }
        let f = self.fec_insert(block, k);
        self.blk_mut(b).fecs.insert(k, f);
        if !self.fec(f).complete {
            return;
        }
        self.buffer(b);
        self.deliver(b);
    }

    fn root_advanced(&mut self, slot: usize, mr: Mr) {
        let Mr::Block(rooted) = mr else { unreachable!("replay roots a block") };
        let old = self.root;
        self.root = slot; /* connect_descendants sees the new root while freed ancestors drop their children */
        self.root_blk = None;
        for s in old..=slot {
            /* the blocks first: a blk's FEC sets can be freed through a sibling */
            let held: Vec<(usize, Option<usize>)> = self.in_slot(s).into_iter().map(|b| (b, self.version(b))).collect();
            for (b, block) in held {
                for (_, f) in core::mem::take(&mut self.blk_mut(b).fecs) {
                    if let Some(fec) = self.fecs[f].take() {
                        self.fec_map.remove(&(fec.block, fec.k));
                    }
                }
                if s == slot && self.blk(b).dmr == mr {
                    self.meta(slot).fin = Some(b);
                    self.root_blk = Some(b);
                    self.connect_descendants(b);
                    continue;
                }
                self.unlink(b, Why::Root(rooted));
                self.forget(b);
                if let Some(block) = block {
                    self.notes.push(Note::Freed { block, why: Why::Root(rooted) });
                }
            }
        }
    }

    /// A shred of FEC set `k` of `block`: shred 0, with its header, if `first`, else its last.
    fn shred_insert(&mut self, block: usize, k: u32, first: bool, slot_complete: bool) {
        let info = self.infos[block];
        let slot = info.slot;
        let fec = self.fec_map.get(&(block, k)).copied();
        let meta = *self.meta(slot);
        if meta.skipped || meta.invalidated || (meta.fin.is_some() && meta.eager.is_none()) {
            return;
        }
        let eager = match meta.eager {
            Some(eager) => eager,
            None => self.blk_insert(slot, Mr::Null),
        };
        self.blk_mut(eager).parent_slot.get_or_insert(info.parent_slot); /* created empty by a child */
        let blk = self.blk(eager);
        let eqvoc = blk.fecs.get(&k).is_some_and(|&f| Some(f) != fec);
        let parent_conflict = blk.parent_slot != Some(info.parent_slot);
        let complete_conflict = (blk.cmpl > 0 && k >= blk.cmpl) || (slot_complete && k + 1 < blk.rcvd);
        if eqvoc || parent_conflict || complete_conflict {
            self.slot_invalidated(slot);
            return;
        }
        if first {
            self.blk_mut(eager).parent_mr = info.parent_mr;
        }
        if first && self.blk(eager).parent.is_none() {
            let parent_slot = info.parent_slot;
            let pmeta = *self.meta(parent_slot);
            let mut parent = self.find(parent_slot, info.parent_mr);
            if parent.is_none()
                && pmeta.fin.is_none()
                && !pmeta.skipped
                && pmeta.eager.is_none_or(|p| self.blk(p).dmr == Mr::Null)
            {
                /* the parent slot's unfinished eager blk, its dmr is checked when it completes */
                parent = Some(match pmeta.eager {
                    Some(p) => p,
                    None => self.blk_insert(parent_slot, Mr::Null),
                });
            }
            if let Some(parent) = parent {
                self.link(eager, parent);
                self.connect_descendants(eager);
            }
        }
        if slot_complete {
            self.blk_mut(eager).cmpl = k + 1;
        }
        let f = match fec {
            Some(f) => f,
            None => self.fec_insert(block, k),
        };
        let blk = self.blk_mut(eager);
        blk.fecs.insert(k, f);
        blk.rcvd = blk.rcvd.max(k + 1);
    }

    /// A shred of `block`'s slot that `fd_rotor_shred_insert` finds conflicting for `reason`,
    /// at FEC set `k`. It never names a FEC set rotor holds but for `Reason::Complete`, where it
    /// is a shred of the block's own FEC set `k` that says the slot ends there.
    fn invalid(&mut self, block: usize, reason: Reason, k: u32) {
        let info = self.infos[block];
        let slot = info.slot;
        let meta = *self.meta(slot);
        if meta.skipped || meta.invalidated || (meta.fin.is_some() && meta.eager.is_none()) {
            return;
        }
        let eager = match meta.eager {
            Some(eager) => eager,
            None => self.blk_insert(slot, Mr::Null),
        };
        self.blk_mut(eager).parent_slot.get_or_insert(info.parent_slot);
        let blk = self.blk(eager);
        let conflict = match reason {
            Reason::Eqvoc => blk.fecs.contains_key(&k),
            Reason::Parent | Reason::Header => true,
            Reason::Complete => (blk.cmpl > 0 && k >= blk.cmpl) || k + 1 < blk.rcvd,
        };
        assert!(conflict, "an INVALID in slot {slot} that rotor would take as a good shred");
        self.slot_invalidated(slot);
    }

    fn slot_invalidated(&mut self, slot: usize) {
        let meta = self.meta(slot);
        meta.invalidated = true;
        if let Some(block) = meta.eager.and_then(|eager| self.version(eager))
            && self.cause != Some(block)
        {
            self.notes.push(Note::Invalidated { block, by: self.cause });
        }
    }

    fn slot_skipped(&mut self, slot: usize) {
        self.meta(slot).skipped = true; /* late repair requests or shreds for the slot must not recreate blks in it */
        for b in self.in_slot(slot) {
            self.prune(b, Why::Skipped);
        }
    }
}

/// What came of an action.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Effect {
    /// Rotor took it, or for `KILL` replay did.
    Applied,
    /// Rotor took it and nothing changed.
    Ignored,
    /// The tile dropped it before rotor: at or below the root, or a shred or `REPAIR` whose
    /// block's parent is below the root.
    Dropped,
}

/// An action taken, what came of it, what rotor did to blocks as it took it, and for a
/// `TURBINE` whether its parent had not completed yet.
struct Entry {
    action: Action,
    effect: Effect,
    notes: Vec<Note>,
    early: bool,
}

/// The actions taken so far on the port of rotor, and what replay saw.
struct Run<'a> {
    blocks: &'a [Block],
    canonical: &'a [usize],
    slot_max: usize,
    model: Model,
    /// How many actions of each sequence have been taken.
    taken: Vec<usize>,
    /// For each block: its `FINAL` or `LAST` taken even if dropped, it or something above it
    /// failed by replay, its FEC sets all received, all delivered, any delivered, its id known
    /// to replay, it waited on its parent, and replay dropped its FEC sets.
    finals: Vec<bool>,
    lasts: Vec<bool>,
    failed: Vec<bool>,
    complete: Vec<bool>,
    full: Vec<bool>,
    some: Vec<bool>,
    known: Vec<bool>,
    waited: Vec<bool>,
    drops: Vec<bool>,
    /// Slots rotor invalidated.
    invalidated: BTreeSet<usize>,
    /// Most blks rotor held at once.
    peak: usize,
    log: Vec<Entry>,
}

impl<'a> Run<'a> {
    fn new(blocks: &'a [Block], canonical: &'a [usize], slot_max: usize) -> Self {
        let infos = blocks
            .iter()
            .map(|block| {
                let parent = block.parent.map(|p| &blocks[p]);
                Info {
                    slot: block.label.slot,
                    parent_slot: parent.map_or(0, |p| p.label.slot),
                    parent_mr: block.parent.map_or(Mr::Genesis, Mr::Block),
                    fecs: block.fecs,
                }
            })
            .collect();
        let n = blocks.len();
        Self {
            blocks,
            canonical,
            slot_max,
            model: Model::new(infos, slot_max),
            taken: vec![0; n * CHAINS + 1],
            finals: vec![false; n],
            lasts: vec![false; n],
            failed: vec![false; n],
            complete: vec![false; n],
            full: vec![false; n],
            some: vec![false; n],
            known: vec![false; n],
            waited: vec![false; n],
            drops: vec![false; n],
            invalidated: BTreeSet::new(),
            peak: 1,
            log: Vec::new(),
        }
    }

    fn label(&self, b: Option<usize>) -> Label {
        b.map_or(GENESIS, |b| self.blocks[b].label)
    }

    /// The first of block `b` and the blocks above it that replay failed.
    fn failed_at(&self, b: usize) -> Option<usize> {
        core::iter::successors(Some(b), |&a| self.blocks[a].parent).find(|&a| self.failed[a])
    }

    /// Whether block `b` or a finalized block under it has its `FINAL` taken.
    fn finalized(&self, b: usize) -> bool {
        self.canonical.iter().skip_while(|&&c| c != b).any(|&c| self.finals[c])
    }

    /// Whether `a` can be taken now: inside the window, in its sequence's order, and once
    /// rotor or replay got far enough for the tiles to send it.
    fn ready(&self, a: &Action) -> bool {
        let root = self.model.root;
        if a.slot >= root + self.slot_max {
            return false;
        }
        if a.kind == SKIPPED {
            return self.canonical.iter().any(|&c| self.finals[c] && self.blocks[c].label.slot > a.slot);
        }
        if self.taken[a.chain] != a.step {
            return false;
        }
        let b = a.block.expect("a block action names its block");
        let mut above = core::iter::successors(self.blocks[b].parent, |&p| self.blocks[p].parent);
        if !above.all(|p| !self.blocks[p].leader || self.lasts[p]) {
            return false; /* nobody builds on our own block before we made it */
        }
        let held = self.model.find(a.slot, Mr::Block(b)).map(|x| self.model.blk(x));
        match a.kind {
            LEADER => self.blocks[b].parent.is_none_or(|p| self.full[p] && self.failed_at(p).is_none()),
            INVALID if a.reason == Some(Reason::Parent) => (root..a.slot).any(|slot| slot != self.model.infos[b].parent_slot),
            NOTAR | FINAL if self.blocks[b].leader => self.lasts[b],
            FINAL if self.blocks[b].voted => self.lasts[b],
            REPAIR => held.is_some_and(|blk| blk.parent_slot.is_none()),
            SEND_K | KLAST => {
                let k = a.fec.expect("a SEND_K names its FEC set");
                a.slot > root
                    && held.is_some_and(|blk| {
                        blk.parent_slot.is_some_and(|slot| slot >= root)
                            && blk.fecs.get(&k).is_none_or(|&f| !self.model.fec(f).complete)
                    })
            }
            DEAD => a.slot > root && self.known[b] && held.is_some() && self.failed_at(b).is_none(),
            KILL => a.slot > root && self.some[b] && !self.known[b] && self.failed_at(b).is_none(),
            ROOT => self.finalized(b) && self.full[b],
            _ => true,
        }
    }

    /// Whether the tile drops `a` before rotor.
    fn dropped(&self, a: &Action) -> bool {
        let root = self.model.root;
        a.slot <= root || (a.needs_parent() && a.block.is_some_and(|b| self.model.infos[b].parent_slot < root))
    }

    /// Hand `a` to the port of rotor.
    fn apply(&mut self, a: &Action) {
        let model = &mut self.model;
        let Some(b) = a.block else {
            model.slot_skipped(a.slot);
            return;
        };
        let (block, info, mr) = (&self.blocks[b], model.infos[b], Mr::Block(b));
        match a.kind {
            TURBINE | LEADER => model.shred_insert(b, 0, true, false),
            SEND | LAST => model.fec_complete(b, a.fec.expect("a SEND names its FEC set"), block.leader),
            INVALID => model.invalid(b, a.reason.expect("an INVALID has its reason"), a.fec.expect("an INVALID names its FEC set")),
            NOTAR => {
                if model.blk_notarized(a.slot, mr).is_some() {
                    model.notes.push(Note::Created { block: b, by: NOTAR });
                }
            }
            FINAL => model.blk_finalized(a.slot, mr),
            REPAIR => model.blk_parented(a.slot, mr, info.parent_slot, info.parent_mr, block.fecs),
            SEND_K | KLAST => {
                let k = a.fec.expect("a SEND_K names its FEC set");
                model.fec_notarized(a.slot, mr, b, k);
                if k == 0 {
                    model.shred_insert(b, 0, true, false);
                }
                model.fec_complete(b, k, false);
            }
            DEAD => model.blk_dead(a.slot, mr),
            ROOT => model.root_advanced(a.slot, mr),
            _ => {}
        }
    }

    /// Take `a` now, and note what replay saw of it.
    fn take(&mut self, a: Action) {
        let early = a.kind == TURBINE && a.block.and_then(|b| self.blocks[b].parent).is_some_and(|p| !self.complete[p]);
        let mark = self.model.delivered.len();
        let mut notes = Vec::new();
        let effect = if self.dropped(&a) {
            Effect::Dropped
        } else {
            self.model.cause = a.block;
            let before = self.model.clone();
            self.apply(&a);
            notes = core::mem::take(&mut self.model.notes);
            if a.kind == KILL || self.model != before { Effect::Applied } else { Effect::Ignored }
        };
        if a.kind != SKIPPED {
            self.taken[a.chain] += 1;
        }
        if let Some(b) = a.block {
            self.finals[b] |= a.kind == FINAL;
            self.lasts[b] |= a.kind == LAST;
            self.failed[b] |= a.kind == KILL || (a.kind == DEAD && effect == Effect::Applied);
        }
        if effect != Effect::Dropped {
            self.observe(&a, mark, &mut notes);
        }
        self.log.push(Entry { action: a, effect, notes, early });
    }

    /// Note what rotor delivered since `mark`, and which blocks are complete, delivered or
    /// waiting on their parent.
    fn observe(&mut self, a: &Action, mark: usize, notes: &mut Vec<Note>) {
        for i in mark..self.model.delivered.len() {
            let block = self.model.delivered[i].2;
            self.some[block] = true;
            if let Some(failed) = self.blocks[block].parent.and_then(|p| self.failed_at(p))
                && !self.drops[block]
            {
                self.drops[block] = true;
                notes.push(Note::Drops { block, failed });
            }
        }
        for b in self.model.held().collect::<Vec<_>>() {
            let Some(block) = self.model.version(b) else { continue };
            let blk = self.model.blk(b);
            let meta = self.model.peek(blk.slot);
            self.complete[block] |= blk.cmpl > 0 && blk.buff == blk.cmpl;
            self.full[block] |= blk.cmpl > 0 && blk.cons == blk.cmpl;
            self.known[block] |= (blk.cmpl > 0 && blk.cons == blk.cmpl) || (blk.cons > 0 && meta.eager != Some(b));
            let stuck = meta.invalidated && blk.dmr == Mr::Null;
            if blk.buff > blk.cons && blk.parent.is_some() && !stuck && !self.waited[block] {
                self.waited[block] = true;
                notes.push(Note::Wait { block });
            }
        }
        if self.model.peek(a.slot).invalidated {
            self.invalidated.insert(a.slot);
        }
        self.peak = self.peak.max(self.model.held().count());
    }

    /// Take shuffled actions in an order the tiles could send them in. An action waits until
    /// it is ready; every other action keeps its shuffled place. An action that is never ready
    /// is never sent.
    fn run(&mut self, shuffled: Vec<Action>) {
        let mut waiting = Vec::new();
        for action in shuffled {
            waiting.push(action);
            while let Some(i) = waiting.iter().position(|a| self.ready(a)) {
                self.take(waiting.remove(i));
            }
        }
    }

    /// What `note` says in block `b`'s steps, taken with `entry`, if anything.
    fn note(&self, b: usize, note: Note, entry: &Entry) -> Option<String> {
        let parent = self.label(self.blocks[b].parent);
        Some(match note {
            Note::Named { by, .. } => match by {
                Some(by) => format!("NAMED (by {})", self.blocks[by].label),
                None => "NAMED".to_string(),
            },
            Note::Role { role: Role::Final, .. } if entry.action.block != Some(b) => match entry.action.block {
                Some(by) => format!("{FINAL} (via {})", self.blocks[by].label),
                None => format!("{FINAL} (implied)"),
            },
            Note::Dedup { .. } => "DEDUP".to_string(),
            Note::Orphan { why, .. } => match why {
                Why::Final(x) => format!("ORPHAN (FINAL {})", self.blocks[x].label),
                Why::Dedup => format!("ORPHAN ({parent} pruned by DEDUP)"),
                Why::Dead => format!("ORPHAN ({parent} DEAD)"),
                Why::Skipped => format!("ORPHAN ({parent} skipped)"),
                Why::Root(x) => format!("ORPHAN (ROOT {})", self.blocks[x].label),
                Why::Completed(x) => format!("ORPHAN (slot {} completed as {})", parent.slot, self.blocks[x].label),
                Why::Merged => "ORPHAN".to_string(),
            },
            Note::Wait { .. } => format!("WAIT({parent})"),
            Note::Invalidated { by, .. } => match by {
                Some(by) => format!("INVALIDATED (by {}'s {})", self.blocks[by].label, entry.action.kind),
                None => "INVALIDATED".to_string(),
            },
            Note::Drops { failed, .. } => format!("dropped by replay ({} failed)", self.blocks[failed].label),
            Note::Created { .. } | Note::Dmr { .. } | Note::Freed { .. } | Note::Role { .. } => return None,
        })
    }

    /// Block `b`'s actions, each noting what rotor or the tile made of it, and what rotor did
    /// to it in between.
    fn steps(&self, b: usize) -> String {
        // Each step's name, what came of it, and for a SEND or SEND_K how many in a row.
        let mut steps: Vec<(String, Effect, usize)> = Vec::new();
        for entry in &self.log {
            let a = entry.action;
            if a.block == Some(b) {
                let has = |want: Note| entry.notes.contains(&want);
                let name = match a.kind {
                    TURBINE if entry.early => format!("{TURBINE} (before {} finishes)", self.label(self.blocks[b].parent)),
                    LAST if has(Note::Dmr { block: b }) => format!("{LAST}(dmr)"),
                    INVALID => format!("{INVALID} ({})", a.reason.expect("an INVALID has its reason").what(a.fec.unwrap_or(0))),
                    FINAL if has(Note::Created { block: b, by: FINAL }) => format!("{FINAL} (created)"),
                    REPAIR => format!("{REPAIR} (parent = {})", self.label(self.blocks[b].parent)),
                    KILL => format!("{KILL} (no DEAD)"),
                    kind => kind.to_string(),
                };
                match steps.last_mut() {
                    Some((last, effect, count)) if matches!(a.kind, SEND | SEND_K) && *last == name && *effect == entry.effect => *count += 1,
                    _ => steps.push((name, entry.effect, 1)),
                }
            }
            for &note in entry.notes.iter().filter(|note| note.block() == b) {
                let Some(text) = self.note(b, note, entry) else { continue };
                if steps.last().is_none_or(|(last, ..)| *last != text) {
                    steps.push((text, Effect::Applied, 1)); /* once, when both its blks lose a parent */
                }
            }
        }
        if steps.is_empty() {
            return "nothing".to_string();
        }
        steps
            .into_iter()
            .map(|(name, effect, count)| {
                let run = if count > 1 { "…" } else { "" };
                let note = match effect {
                    Effect::Applied => "",
                    Effect::Ignored => " (ignored)",
                    Effect::Dropped => " (dropped)",
                };
                format!("{name}{run}{note}")
            })
            .collect::<Vec<_>>()
            .join(" + ")
    }

    /// The roles block `b` took in rotor, each once, in the order it first took them.
    fn roles(&self, b: usize) -> String {
        let mut roles: Vec<&str> = Vec::new();
        for entry in &self.log {
            let a = entry.action;
            if a.block == Some(b) && entry.effect == Effect::Applied {
                roles.extend(match a.kind {
                    TURBINE => Some("eager"),
                    LEADER => Some("leader"),
                    NOTAR => Some("notar"),
                    FINAL => Some("final"),
                    _ => None,
                });
            }
            for &note in entry.notes.iter().filter(|note| note.block() == b) {
                roles.extend(match note {
                    Note::Named { .. } => Some("named"),
                    Note::Role { role: Role::Notar, .. } if !roles.contains(&"final") => Some("notar"),
                    Note::Role { role: Role::Final, .. } => Some("final"),
                    _ => None,
                });
            }
        }
        let mut seen = BTreeSet::new();
        roles.retain(|role| seen.insert(*role));
        if roles.is_empty() { "unseen".to_string() } else { roles.join(" → ") }
    }

    /// How block `b` ended: finalized, delivered and maybe rooted, or else how far it got and
    /// what freed it.
    fn outcome(&self, b: usize) -> String {
        if self.blocks[b].canonical {
            let rooted = self.log.iter().any(|e| e.action.block == Some(b) && e.action.kind == ROOT && e.effect == Effect::Applied);
            return (if rooted {
                "✔ rooted"
            } else if self.full[b] {
                "✔"
            } else {
                "✘ never delivered"
            })
            .to_string();
        }
        let turbine = self.log.iter().any(|e| {
            e.action.block == Some(b) && matches!(e.action.kind, TURBINE | LEADER | INVALID) && e.effect == Effect::Applied
        });
        let status = if self.full[b] {
            "delivered"
        } else if self.complete[b] {
            "complete"
        } else if turbine && self.invalidated.contains(&self.blocks[b].label.slot) {
            "stops"
        } else if self.some[b] {
            "partly delivered"
        } else {
            "never completes"
        };
        if self.model.held().any(|x| self.model.version(x) == Some(b)) {
            return format!("{status}, held");
        }
        let mut freed = self.log.iter().flat_map(|e| &e.notes).filter_map(|note| match *note {
            Note::Freed { block, why } if block == b => Some(why),
            _ => None,
        });
        let end = match freed.next_back() {
            Some(Why::Final(x)) => format!("pruned by FINAL {}", self.blocks[x].label),
            Some(Why::Dedup) => "pruned by DEDUP".to_string(),
            Some(Why::Merged) => "merged".to_string(),
            Some(Why::Dead) => "pruned by DEAD".to_string(),
            Some(Why::Skipped) => "freed by SKIPPED".to_string(),
            Some(Why::Root(x)) => format!("freed by ROOT {}", self.blocks[x].label),
            Some(Why::Completed(_)) | None => return "never held".to_string(),
        };
        format!("{status}, {end}")
    }

    /// Draw the blocks under `parent`, and theirs, each line starting with `prefix`.
    fn draw(&self, parent: Option<usize>, prefix: &mut String, out: &mut String) {
        let children: Vec<usize> = (0..self.blocks.len()).filter(|&b| self.blocks[b].parent == parent).collect();
        for (i, &b) in children.iter().enumerate() {
            let last = i + 1 == children.len();
            let label = self.blocks[b].label;
            let _ = writeln!(out, "{prefix}{}{label}  {}  {}  {}", if last { "└── " } else { "├── " }, self.roles(b), self.steps(b), self.outcome(b));
            let len = prefix.len();
            prefix.push_str(if last { "    " } else { "│   " });
            self.draw(Some(b), prefix, out);
            prefix.truncate(len);
        }
    }

    /// The skipped slots, each with the blocks it held.
    fn skipped(&self) -> String {
        let skipped: Vec<String> = self
            .log
            .iter()
            .filter(|e| e.action.kind == SKIPPED)
            .map(|e| {
                let slot = e.action.slot;
                let held: Vec<String> = self.blocks.iter().filter(|b| b.label.slot == slot).map(|b| b.label.to_string()).collect();
                let text = if held.is_empty() { format!("{slot} (empty)") } else { held.join("/") };
                if e.effect == Effect::Dropped { format!("{text} (dropped)") } else { text }
            })
            .collect();
        if skipped.is_empty() { "none".to_string() } else { skipped.join(", ") }
    }
}

/// The tree's blocks as the action list writes them.
fn blocks_json(blocks: &[Block]) -> Vec<String> {
    blocks
        .iter()
        .map(|block| {
            let parent = block.parent.map_or(GENESIS, |p| blocks[p].label);
            let leader = if block.leader { ", \"leader\": true" } else { "" };
            format!("{{\"node\": \"{}\", \"parent\": \"{parent}\", \"fecs\": {}{leader}}}", block.label, block.fecs)
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
    // Keep the generated trees stable for a seed while choosing slots, how each block reaches
    // rotor, and how the actions are shuffled.
    let mut slot_rng = stream(args.seed, 0x736c_6f74_0000_0000);
    let mut fate_rng = stream(args.seed, 0x6661_7465_0000_0000);
    let mut shuffle_rng = stream(args.seed, 0x7368_7566_666c_6500);
    let mut generators = tuple_list!(DepthLimiter::new(STRUCTURE.inner(), args.depth));
    let mut emitted = 0;
    let mut wide = 0;

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
        let slotted = assign(roots, &mut slot_rng);
        let canonical = canonical(&slotted);
        if !fits(&slotted, &canonical, args.slot_max) {
            wide += 1;
            continue;
        }

        let blocks = plan(slotted, &canonical, args.slot_max, &mut fate_rng);
        let mut actions: Vec<Action> = blocks.iter().enumerate().flat_map(|(b, block)| block.actions(b)).collect();
        actions.extend(root_actions(&blocks, &canonical));
        actions.extend(skips(&blocks, &canonical));
        actions.shuffle(&mut shuffle_rng);
        let mut run = Run::new(&blocks, &canonical, args.slot_max);
        run.run(actions);
        if run.peak > blk_max(args.slot_max) {
            wide += 1;
            continue;
        }
        emitted += 1;

        let mut tree = String::from("0\n");
        run.draw(None, &mut String::new(), &mut tree);
        let path = canonical.iter().map(|&c| blocks[c].label.to_string()).collect::<Vec<_>>().join(" → ");
        let nodes = blocks_json(&blocks);
        let actions: Vec<String> =
            run.log.iter().filter(|e| e.effect != Effect::Dropped).map(|e| e.action.json(&blocks)).collect();
        println!(
            "tree {iteration:06}\n{tree}path  {path}\nskipped  {}\nblocks\n[\n  {}\n]\nactions\n[\n  {}\n]\n",
            run.skipped(),
            nodes.join(",\n  "),
            actions.join(",\n  ")
        );

        if let Some(dir) = &args.out_dir {
            if emitted == 1 {
                fs::create_dir_all(dir).with_context(|| format!("could not create {}", dir.display()))?;
            }
            let path = dir.join(format!("{iteration:06}.json"));
            fs::write(&path, format!("{{\"blocks\": [{}], \"actions\": [{}]}}\n", nodes.join(", "), actions.join(", ")))
                .with_context(|| format!("could not write {}", path.display()))?;
        }
    }

    let wide = if wide > 0 {
        format!(" ({wide} more did not fit a window of {} slots or its {} blks)", args.slot_max, blk_max(args.slot_max))
    } else {
        String::new()
    };
    match &args.out_dir {
        Some(_) if emitted == 0 => println!("No nonempty trees after {} attempts{wide}; no corpus written.", args.iterations),
        Some(dir) => println!("Wrote {emitted} nonempty trees from {} attempts{wide} to {}.", args.iterations, dir.display()),
        None => println!("Drew {emitted} nonempty trees from {} attempts{wide}.", args.iterations),
    }
    Ok(())
}
