//! Generate one lineage of blocks from `grammars/rotor_tree.fan`, with forks, delivered by
//! turbine and repair, and flatten it into an action list for Firedancer's rotor.
//!
//! The grammar picks how many blocks the lineage has and how many forks each slot has. The lineage can be any length, bounded only by `--depth`. Each lineage
//! block builds on the one before it, with one in [`GAP_ONE_IN`] slots between them skipped: the
//! lineage has no block there. It skips never so many that the next block falls out of the
//! window. Every lineage block is finalized, but for the tip in one in [`OPEN_TIP_ONE_IN`]
//! lineages.
//!
//! A slot holds at most five blocks: its lineage block and up to four forks, other versions of
//! the slot the leader equivocated, or in a skipped slot up to five forks, as many as
//! [`SKIPPED_FORKS`] picks. A fork builds on the last lineage block before its slot half the
//! time and otherwise on any block before its slot, lineage block or fork, so a fork can have
//! forks after it. It is never finalized. Blocks are labelled by slot then version as in `consensus_tree`: `a` for the lineage block, `b` on for the forks, also in a
//! skipped slot.
//!
//! Turbine delivers every block in full. At most one version of a slot is repairable: from
//! its `REPAIR` action on, which may come at any point, the mock answers rotor's repair requests
//! for it. It refuses every request before then, and every request for any other block. Rotor
//! asks again until a block completes, so a repairable block is still delivered in full when
//! its `REPAIR` comes late, as long as rotor asks for it. A fork is never the one,
//! as a positional repair request names no version: the mock would answer it with the fork's
//! shreds, which conflict with the lineage block. Every finalized lineage block is repairable:
//! turbine alone does not guarantee a block, as the shred tile's FEC resolver may spill a partly
//! received FEC set, which only repair recovers, and the mock sends each turbine shred once. The
//! open tip is repairable one in [`REPAIR_ONE_IN`] times. In a slot with forks, rotor fills the
//! lineage block through turbine only if it has every FEC set before any fork's shred arrives, as
//! a conflicting shred invalidates the slot, so the forks' `TURBINE` waits for a lineage block that
//! is not repairable to get its own. A skipped slot has no lineage block for its forks to conflict with, so one in
//! [`REPAIR_ONE_IN`] skipped slots with forks has one of them repairable, and their shreds wait for
//! nothing.
//!
//! ```text
//! finalized   TURBINE + NOTAR + FINAL
//! implied     TURBINE + NOTAR  finalized only through a later block's FINAL
//! open tip    TURBINE
//! fork        TURBINE
//! repairable  + REPAIR, anywhere among them
//! ```
//!
//! `TURBINE` sends every shred of the block, the first with the header naming its parent, the
//! last completing it and giving the slot's eager blk its dmr. `FINAL` is a
//! finalization certificate naming the block by id. It comes only once the block and every
//! block above it is delivered in full or repairable, as one that came earlier would have rotor
//! create a blk by id for a block, which only repair fills. One in [`IMPLIED_ONE_IN`] finalized
//! blocks, other than the last and those the window needs, has no `FINAL` of its own. `NOTAR` is
//! a notarization certificate naming the block by id, which every finalized block gets, under the
//! same condition as `FINAL`.
//!
//! Every skipped slot before a finalized block is `SKIPPED` once a block after it is finalized.
//!
//! These are the only actions: shreds and certificates, what the node is sent, and `REPAIR`,
//! which only changes what the mock answers. The node roots on its own, every block it finalizes
//! once it holds it in full.
//!
//! The actions are shuffled. Each then waits until the tiles could send it: once the blocks and certificates it needs came, and inside the window of
//! `--slot-max` slots after the deepest finalized block turbine delivered, which the
//! node can root. A block whose next one would not fit the window otherwise gets a `FINAL` of its
//! own.
//!
//! The lineage is drawn as a tree, each block under its parent as `consensus_tree` draws it, a
//! block's forks before the lineage goes on. Each block shows `TURBINE` if turbine delivered it,
//! `REPAIR` if it is repairable and `DEAD` if it is dead, and each `SKIPPED` slot is drawn in slot order with
//! `SKIPPED`. A block under a skipped slot names its parent.
//!
//! Each lineage is written as JSON. `blocks` lists each node in slot order with its parent, as
//! `repair` whether it is repairable, and as `replay` whether the node
//! replays it: `yes` if repair always delivers it in full, and `maybe` if that depends on the order rotor sees its slot's versions in, as for every
//! fork, or on whether the FEC resolver spills a set turbine sends once, and `dead` whether its
//! footer declares a wrong bank hash. One in [`DEAD_ONE_IN`] blocks the cluster does not finalize
//! is dead: replay marks it dead once it has it in full, and drops what builds on it from then
//! on, so a block under a dead one is at best `maybe`.
//! `actions` names each action's node and parent, or for `SKIPPED` its slot. With `-o`, each lineage is also written to
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
use std::collections::{BTreeMap, BTreeSet};
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

/// One in this many blocks the cluster does not finalize, the forks and an open tip, is dead: its
/// footer declares a bank hash its execution does not reach, so replay marks it dead.
const DEAD_ONE_IN: u32 = 4;

/// Alpenglow leader window size (consecutive leader slots).
const LEADER_WINDOW_SLOTS: usize = 4;

/// One in this many eligible window-start blocks uses Fast Leader Handover (`UpdateParent`).
const FLH_ONE_IN: u32 = 2;

/// Under Alpenglow's <20% Byzantine stake bound:
/// - A slot with no `Notarize`/`Finalize` cert can have at most 4 conflicting `NotarizeFallback` certs.
/// - A slot with a `Notarize` cert (and no `Finalize` cert) can have at most 1 conflicting `NotarizeFallback` cert.
/// - A finalized slot (`Finalize` / `FinalizeFast`) can have 0 conflicting `NotarizeFallback` certs.
const MAX_NOTAR_FALLBACK_BLOCKS: usize = 4;
const MAX_NOTAR_FALLBACK_WITH_NOTAR: usize = 1;

/// One in this many implied-finalized lineage blocks (finalized only via a descendant's `FINAL`)
/// receives a `NOTAR_FALLBACK` certificate instead of `NOTAR`.
const NOTAR_FALLBACK_LINEAGE_ONE_IN: u32 = 2;

/// Probability (1 in N) that an eligible non-dead fork or open tip gets a `NOTAR_FALLBACK`
/// certificate, subject to the per-slot Alpenglow bounds above.
const NOTAR_FALLBACK_FORK_ONE_IN: u32 = 2;

/// Every shred of a block arrives from turbine.
const TURBINE: &str = "TURBINE";
/// A finalization certificate names a block.
const FINAL: &str = "FINAL";
/// A notarization certificate names a block.
const NOTAR: &str = "NOTAR";
/// A notarization-fallback certificate names a block.
const NOTAR_FALLBACK: &str = "NOTAR_FALLBACK";
/// A finalized block after a slot declares it skipped.
const SKIPPED: &str = "SKIPPED";
/// The mock starts serving repair for a repairable block.
const REPAIR: &str = "REPAIR";

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
    finalized: bool,
    /// Gets a finalization certificate of its own.
    certified: bool,
    /// Gets a notarization certificate (`NOTAR`).
    notar: bool,
    /// Gets a notarization-fallback certificate (`NOTAR_FALLBACK`).
    notar_fallback: bool,
    /// The mock answers repair requests for it.
    repairable: bool,
    /// Its footer declares a wrong bank hash, so replay marks it dead. Never a finalized block.
    dead: bool,
    /// Optimistic parent this window-start block started on before `UpdateParent`, if any.
    optimistic_parent: Option<usize>,
}

/// Block `b`'s label, or the genesis root's for `None`.
fn label(blocks: &[Block], b: Option<usize>) -> String {
    b.map_or_else(|| "0".to_string(), |b| format!("{}{}", blocks[b].slot, char::from(b'a' + blocks[b].version)))
}

/// The label of block `b`'s parent.
fn parent(blocks: &[Block], b: usize) -> String {
    label(blocks, blocks[b].parent)
}

/// Whether the node replays block `b`: `yes` or `maybe`. A dead block is replayed, and
/// only then marked dead: replay drops what builds on it once it is, but may take a descendant
/// in full before.
fn replay(blocks: &[Block], b: usize) -> &'static str {
    let alive = replay_alive(blocks, b);
    let mut above = blocks[b].parent;
    while let Some(a) = above {
        if blocks[a].dead {
            return "maybe";
        }
        above = blocks[a].parent;
    }
    alive
}

/// Whether the node replays block `b` were no block above it dead.
fn replay_alive(blocks: &[Block], b: usize) -> &'static str {
    let block = &blocks[b];
    if block.version > 0 {
        return "maybe"; /* rotor may take it, or splice it, as the slot's eager blk */
    }
    match (block.finalized, block.forked, block.repairable) {
        (true, ..) => "yes",
        (false, true, true) => "maybe", /* repair may fill another version's eager blk */
        (false, false, true) => "yes",
        (false, _, false) => "maybe", /* the FEC resolver may spill a set turbine sent once */
    }
}

/// Give each lineage block, with `forks[b]` forks in its slot, a slot, and pick which
/// blocks are finalized and which get their own certificate. Then pick, from `repair_rng`, which
/// are repairable and how many forks each skipped slot holds, and from `fork_rng` each fork's
/// parent.
fn plan(forks: &[usize], slot_max: usize, rng: &mut StdRng, repair_rng: &mut StdRng, fork_rng: &mut StdRng) -> Vec<Block> {
    let mut slots = Vec::with_capacity(forks.len());
    let mut slot = 0;
    for _ in forks {
        let after = slot;
        slot += 1;
        while slot + 1 < after + slot_max && rng.random_ratio(1, GAP_ONE_IN) {
            slot += 1;
        }
        slots.push(slot);
    }
    let finals = if rng.random_ratio(1, OPEN_TIP_ONE_IN) { forks.len() - 1 } else { forks.len() };
    // The slot of the last block with a certificate of its own, the deepest the node can have
    // finalized before the next block's shreds.
    let mut certified_slot = 0;
    let mut blocks = Vec::with_capacity(forks.len());
    for (b, &count) in forks.iter().enumerate() {
        let forked = count > 0;
        let finalized = b < finals;
        let next = slots.get(b + 1);
        // Certify at random, and wherever the next block would not fit the window otherwise.
        let needed = next.is_some_and(|&next| next >= certified_slot + slot_max);
        let certified = finalized && (needed || b + 1 == finals || !rng.random_ratio(1, IMPLIED_ONE_IN));
        if certified {
            certified_slot = slots[b];
        }
        let repairable = repair_rng.random_ratio(1, REPAIR_ONE_IN) || finalized;
        blocks.push(Block {
            slot: slots[b],
            version: 0,
            parent: b.checked_sub(1),
            lineage: Some(b),
            forked,
            finalized,
            certified,
            notar: finalized,
            notar_fallback: false,
            repairable,
            dead: false,
            optimistic_parent: None,
        });
    }
    // Add to `forks` a fork in `slot`, at or before lineage block `b`'s,
    // repairable if `repairable`. It builds on lineage block `b - 1` half the time, else on any
    // block before `slot`: a lineage block before `b` or a fork in `forks`.
    let lineage_len = forks.len();
    let fork = |rng: &mut StdRng, forks: &mut Vec<Block>, slot: usize, b: usize, version: u8, lineage: Option<usize>, repairable: bool| {
        let earlier: Vec<usize> =
            (0..b).chain(forks.iter().enumerate().filter(|(_, f)| f.slot < slot).map(|(i, _)| lineage_len + i)).collect();
        let parent = if earlier.is_empty() || rng.random_bool(0.5) { b.checked_sub(1) } else { Some(earlier[rng.random_range(0..earlier.len())]) };
        forks.push(Block {
            slot,
            version,
            parent,
            lineage,
            forked: true,
            finalized: false,
            certified: false,
            notar: false,
            notar_fallback: false,
            repairable,
            dead: false,
            optimistic_parent: None,
        });
    };
    // In slot order, so a fork can build on any fork before it.
    let mut forks_of = Vec::new();
    let weight_total: u32 = SKIPPED_FORKS.iter().map(|&(_, weight)| weight).sum();
    for (b, &count) in forks.iter().enumerate() {
        for slot in b.checked_sub(1).map_or(0, |prev| slots[prev]) + 1..slots[b] {
            let mut pick = repair_rng.random_range(0..weight_total);
            let skipped = SKIPPED_FORKS.iter().find(|&&(_, weight)| pick < weight || { pick -= weight; false }).map_or(0, |&(count, _)| count);
            let repairable = (skipped > 0 && repair_rng.random_ratio(1, REPAIR_ONE_IN)).then(|| repair_rng.random_range(0..skipped));
            for (i, version) in (1..).take(skipped).enumerate() {
                fork(fork_rng, &mut forks_of, slot, b, version, None, repairable == Some(i));
            }
        }
        for version in (1..).take(count) {
            fork(fork_rng, &mut forks_of, slots[b], b, version, Some(b), false);
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
}

impl Action {
    /// The action as the action list writes it.
    fn json(&self, blocks: &[Block]) -> String {
        let Some(b) = self.block else {
            return format!("{{\"slot\": {}, \"action\": \"{SKIPPED}\"}}", self.slot);
        };
        format!("{{\"node\": \"{}\", \"parent\": \"{}\", \"action\": \"{}\"}}", label(blocks, Some(b)), parent(blocks, b), self.kind)
    }
}

/// Skipped slots that receive a `SKIPPED` certificate: every gap slot before a finalized block.
fn skipped_slots(blocks: &[Block]) -> BTreeSet<usize> {
    let mut skipped = BTreeSet::new();
    let mut after = 0;
    for block in blocks.iter().filter(|block| block.finalized) {
        skipped.extend(after + 1..block.slot);
        after = block.slot;
    }
    skipped
}

/// Assign `NOTAR` and `NOTAR_FALLBACK` certificates across slots in slot order, respecting
/// Alpenglow's <20% Byzantine stake bounds:
/// - Directly finalized slot (`FINAL`): `NOTAR` on the finalized block, 0 conflicting `NOTAR_FALLBACK`.
/// - Implied-finalized slot (no `FINAL`): either `NOTAR` on lineage + <=1 `NOTAR_FALLBACK` fork,
///   or `NOTAR_FALLBACK` on lineage + <=3 `NOTAR_FALLBACK` forks (<=4 total).
/// - Non-finalized slot (open tip or skipped): <=4 `NOTAR_FALLBACK` blocks (or `NOTAR` on open tip
///   + <=1 `NOTAR_FALLBACK` fork).
/// - No `dead` block can be certified, and every certified fork must extend genesis or a certified
///   parent via either a consecutive slot or a skip-certified window start.
fn assign_notar_fallbacks(blocks: &mut [Block], rng: &mut StdRng) {
    let skipped = skipped_slots(blocks);
    let mut by_slot: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for (i, b) in blocks.iter().enumerate() {
        by_slot.entry(b.slot).or_default().push(i);
    }

    for (_slot, indices) in by_slot {
        let lineage_idx = indices.iter().copied().find(|&i| blocks[i].version == 0);
        let has_final = lineage_idx.is_some_and(|i| blocks[i].certified);
        if has_final {
            // Finalized slot: lineage block must have NOTAR + FINAL; 0 conflicting NOTAR_FALLBACK.
            continue;
        }

        // First decide whether an implied-finalized lineage block uses NOTAR_FALLBACK instead of NOTAR,
        // or whether an alive open tip gets NOTAR or NOTAR_FALLBACK.
        if let Some(l) = lineage_idx {
            if blocks[l].finalized {
                // Implied-finalized lineage block: switch to NOTAR_FALLBACK half the time.
                if rng.random_ratio(1, NOTAR_FALLBACK_LINEAGE_ONE_IN) {
                    blocks[l].notar = false;
                    blocks[l].notar_fallback = true;
                }
            } else if !blocks[l].dead && is_cert_eligible(blocks, l, &skipped) {
                // Alive open tip: optionally assign NOTAR or NOTAR_FALLBACK.
                match rng.random_range(0..4u32) {
                    0 => blocks[l].notar = true,
                    1 => blocks[l].notar_fallback = true,
                    _ => {}
                }
            }
        }

        let has_notar = indices.iter().any(|&i| blocks[i].notar);
        let existing_nf = indices.iter().filter(|&&i| blocks[i].notar_fallback).count();
        let max_nf = if has_notar {
            MAX_NOTAR_FALLBACK_WITH_NOTAR
        } else {
            MAX_NOTAR_FALLBACK_BLOCKS
        };
        let remaining_cap = max_nf.saturating_sub(existing_nf);
        if remaining_cap == 0 {
            continue;
        }

        let mut eligible_forks: Vec<usize> = indices
            .iter()
            .copied()
            .filter(|&i| {
                blocks[i].version > 0
                    && !blocks[i].dead
                    && !blocks[i].notar
                    && !blocks[i].notar_fallback
                    && is_cert_eligible(blocks, i, &skipped)
            })
            .collect();
        eligible_forks.shuffle(rng);

        let mut added = 0;
        for f in eligible_forks {
            if added >= remaining_cap {
                break;
            }
            if rng.random_ratio(1, NOTAR_FALLBACK_FORK_ONE_IN) {
                blocks[f].notar_fallback = true;
                added += 1;
            }
        }
    }

    validate_alpenglow_cert_bounds(blocks, &skipped);
}

fn is_cert_eligible(blocks: &[Block], idx: usize, skipped: &BTreeSet<usize>) -> bool {
    let b = &blocks[idx];
    if b.dead {
        return false;
    }
    let parent_slot = match b.parent {
        None => 0,
        Some(p) => {
            if !(blocks[p].notar || blocks[p].notar_fallback) {
                return false;
            }
            blocks[p].slot
        }
    };
    if parent_slot + 1 == b.slot {
        return true;
    }
    let is_window_start = b.slot == 1 || b.slot.is_multiple_of(LEADER_WINDOW_SLOTS);
    is_window_start && (parent_slot + 1..b.slot).all(|g| skipped.contains(&g))
}

fn validate_alpenglow_cert_bounds(blocks: &[Block], skipped: &BTreeSet<usize>) {
    let mut by_slot: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for (i, b) in blocks.iter().enumerate() {
        by_slot.entry(b.slot).or_default().push(i);
    }
    for (slot, indices) in by_slot {
        let finals = indices.iter().filter(|&&i| blocks[i].certified).count();
        let notars = indices.iter().filter(|&&i| blocks[i].notar).count();
        let nfs = indices.iter().filter(|&&i| blocks[i].notar_fallback).count();
        assert!(finals <= 1, "slot {slot}: >1 FINAL");
        assert!(notars <= 1, "slot {slot}: >1 NOTAR");
        if finals == 1 {
            assert_eq!(notars, 1, "slot {slot}: FINAL requires NOTAR");
            assert_eq!(nfs, 0, "slot {slot}: FINAL forbids conflicting NOTAR_FALLBACK");
            assert!(!skipped.contains(&slot), "slot {slot}: FINAL forbids SKIPPED");
        } else if notars == 1 {
            assert!(nfs <= MAX_NOTAR_FALLBACK_WITH_NOTAR, "slot {slot}: NOTAR allows <=1 NOTAR_FALLBACK");
        } else {
            assert!(nfs <= MAX_NOTAR_FALLBACK_BLOCKS, "slot {slot}: at most 4 NOTAR_FALLBACK");
        }
        assert!(notars + nfs <= MAX_NOTAR_FALLBACK_BLOCKS, "slot {slot}: total certified blocks > 4");
        for &i in &indices {
            assert!(!(blocks[i].notar && blocks[i].notar_fallback), "block {i} has both NOTAR and NOTAR_FALLBACK");
            if blocks[i].certified || blocks[i].notar || blocks[i].notar_fallback {
                assert!(!blocks[i].dead, "dead block {i} cannot be certified");
                if let Some(p) = blocks[i].parent {
                    assert!(
                        blocks[p].notar || blocks[p].notar_fallback,
                        "certified block {i} has uncertified parent {p}"
                    );
                }
            }
        }
    }
}

/// Assign Fast Leader Handover (`UpdateParent`) optimistic parents to eligible blocks.
///
/// FLH is only allowed on window-start slots (`slot > 0 && slot % LEADER_WINDOW_SLOTS == 0`).
/// A valid optimistic parent `opt` must satisfy `parent_slot <= opt.slot < block.slot` and
/// `opt != block.parent`.
fn assign_flh(blocks: &mut [Block], rng: &mut StdRng) {
    for b in 0..blocks.len() {
        let slot = blocks[b].slot;
        if slot == 0 || !slot.is_multiple_of(LEADER_WINDOW_SLOTS) {
            continue;
        }
        let p_slot = blocks[b].parent.map_or(0, |p| blocks[p].slot);
        let candidates: Vec<usize> = (0..blocks.len())
            .filter(|&c| blocks[c].slot >= p_slot && blocks[c].slot < slot && Some(c) != blocks[b].parent)
            .collect();
        if !candidates.is_empty() && rng.random_ratio(1, FLH_ONE_IN) {
            blocks[b].optimistic_parent = Some(candidates[rng.random_range(0..candidates.len())]);
        }
    }
    validate_flh(blocks);
}

fn validate_flh(blocks: &[Block]) {
    for (i, b) in blocks.iter().enumerate() {
        let Some(opt) = b.optimistic_parent else { continue };
        assert!(
            b.slot > 0 && b.slot.is_multiple_of(LEADER_WINDOW_SLOTS),
            "block {i} (slot {}): FLH is only valid at window start",
            b.slot
        );
        let p_slot = b.parent.map_or(0, |p| blocks[p].slot);
        assert!(
            blocks[opt].slot >= p_slot && blocks[opt].slot < b.slot,
            "block {i} (slot {}): optimistic parent {opt} (slot {}) must satisfy parent_slot ({p_slot}) <= opt_slot < slot",
            b.slot,
            blocks[opt].slot
        );
        assert_ne!(
            Some(opt),
            b.parent,
            "block {i}: optimistic parent must differ from parent"
        );
    }
}

/// Every action of the lineage: each block's shreds and certificates, a `REPAIR` for each
/// repairable block, and a `SKIPPED` for every skipped slot before a finalized block.
fn actions(blocks: &[Block]) -> Vec<Action> {
    let mut actions = Vec::new();
    for (b, block) in blocks.iter().enumerate() {
        let on = |kind| Action { kind, block: Some(b), slot: block.slot };
        actions.push(on(TURBINE));
        if block.certified {
            actions.push(on(FINAL));
        }
        if block.notar {
            actions.push(on(NOTAR));
        }
        if block.notar_fallback {
            actions.push(on(NOTAR_FALLBACK));
        }
        if block.repairable {
            actions.push(on(REPAIR));
        }
    }
    for slot in skipped_slots(blocks) {
        actions.push(Action { kind: SKIPPED, block: None, slot });
    }
    actions
}

/// The actions taken so far, and what the tiles saw of them.
struct Run<'a> {
    blocks: &'a [Block],
    /// How many lineage blocks there are, the first of `blocks`.
    lineage_len: usize,
    slot_max: usize,
    /// Which blocks have had their `TURBINE` taken.
    fulls: Vec<bool>,
    /// How many lineage blocks from the first turbine has delivered: those with their
    /// `TURBINE` taken, up to the first without.
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
            fulls: vec![false; blocks.len()],
            delivered: 0,
            finalized: None,
            log: Vec::new(),
        }
    }

    /// Whether `a` can be taken now: inside the window, and once the
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
            Some(l) if self.blocks[b].version > 0 => !self.blocks[l].repairable && !self.fulls[l],
            _ => false,
        };
        match a.kind {
            FINAL | NOTAR => (0..=b).all(|i| self.fulls[i] || self.blocks[i].repairable),
            NOTAR_FALLBACK => {
                if self.blocks[b].version == 0 {
                    (0..=b).all(|i| self.fulls[i] || self.blocks[i].repairable)
                } else {
                    true
                }
            }
            REPAIR => true,
            _ => !waits,
        }
    }

    /// Take `a` now.
    fn take(&mut self, a: Action) {
        if let Some(b) = a.block {
            match a.kind {
                FINAL => self.finalized = self.finalized.max(Some(b)),
                NOTAR | NOTAR_FALLBACK | REPAIR => {}
                _ => {
                    self.fulls[b] = true;
                    while self.delivered < self.lineage_len && self.fulls[self.delivered] {
                        self.delivered += 1;
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
        if let Some(opt) = self.blocks[b].optimistic_parent {
            let _ = write!(out, "  FLH(opt={})", label(self.blocks, Some(opt)));
        }
        if self.blocks[b].notar_fallback {
            out.push_str("  NF");
        }
        if self.blocks[b].repairable {
            out.push_str("  REPAIR");
        }
        if self.blocks[b].dead {
            out.push_str("  DEAD");
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
            let mut out = format!(
                "{{\"node\": \"{node}\", \"parent\": \"{parent}\", \"repair\": {}, \"replay\": \"{replay}\", \"dead\": {}",
                block.repairable, block.dead
            );
            if let Some(opt) = block.optimistic_parent {
                let _ = write!(out, ", \"optimistic_parent\": \"{}\"", label(blocks, Some(opt)));
            }
            out.push('}');
            out
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
    // are repairable, the forks' parents and fates, which blocks are dead, FLH optimistic parents,
    // and how the actions are shuffled.
    let mut fate_rng = stream(args.seed, 0x6661_7465_0000_0000);
    let mut repair_rng = stream(args.seed, 0x7265_7061_6972_0000);
    let mut fork_rng = stream(args.seed, 0x666f_726b_0000_0000);
    let mut dead_rng = stream(args.seed, 0x6465_6164_0000_0000);
    let mut flh_rng = stream(args.seed, 0x666c_6800_0000_0000);
    let mut nf_rng = stream(args.seed, 0x6e6f_7461_7266_6200);
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
        let forks = json["blocks"]
            .as_array()
            .context("a lineage without blocks")?
            .iter()
            .map(|block| {
                Ok(block["forks"].as_array().context("a block without forks")?.len())
            })
            .collect::<Result<Vec<_>, Error>>()?;

        let mut blocks = plan(&forks, args.slot_max, &mut fate_rng, &mut repair_rng, &mut fork_rng);
        for block in &mut blocks {
            block.dead = !block.finalized && dead_rng.random_ratio(1, DEAD_ONE_IN);
        }
        assign_flh(&mut blocks, &mut flh_rng);
        assign_notar_fallbacks(&mut blocks, &mut nf_rng);
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
