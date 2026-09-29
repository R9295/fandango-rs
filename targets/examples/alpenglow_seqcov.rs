//! Measure t-way sequence coverage of a `consensus_tree` corpus.
//!
//! The events, k = 18 x 30 = 540, for each slot 1 to 18, named in the report as
//! `class:slot`, or `class:slot label` for an off-chain node, as in `fast:14` or
//! `off.dead:15a`:
//! - its canonical node's settle mode, by the certificates it has, timed by the last to land:
//!   `fallback` (notar fallback), `notar+fin` (notar and final), `fast` (fast final),
//!   `fast+notar`, `notar`, `final`, `n+f+fast` (notar, final and fast final), or `skip`,
//! - its canonical block arriving for replay, `arrive`, and replay completing it, `replay`,
//! - for each node off the canonical chain, by label (its position in the slot, a to e), its
//!   block arriving, `off.arrive`, replay completing it, `off.replay`, or finding it dead,
//!   `off.dead`, and its notar fallback certificate, `off.fallbk`: 5 x 4.
//!
//! Of these, 531 can occur: `1e` is never an off-chain node, as the root has at most 4
//! children, nor is `18a`, the canonical tip, and slot 18 is never skipped.
//!
//! A t-sequence is covered once some scenario has those t events in that order, not
//! necessarily adjacent.  Up to 10 million t-sequences, every one is counted; past that
//! (t = 3 on), coverage is estimated from random t-sequences of the universe, drawn
//! uniformly, with a 95% interval, as the corpus holds too many distinct ones to keep.  Its
//! universe is every ordered t-tuple of distinct events that one scenario could hold: one
//! settle mode per slot, no replay of a skipped slot, at most one replay result per block,
//! after its arrival, canonical replay results in slot order, each after the arrivals of the
//! canonical blocks up to its own, no notar fallback certificate for a dead block, at most 4
//! off-chain nodes in a slot, as the canonical node takes one of its 5 labels, and at most 3
//! notar fallback certificates for them, beside a notar fallback certificate for the
//! canonical node, 2 beside a lone notar or a skip certificate, and none beside the others.
//! Slot 1 holds the root's children, at most 4, so it has at most 3 off-chain nodes, a to d.
//! No tree is deeper than 18 slots, so slot 18 is never skipped, and `18a`, its leftmost
//! node, is always the canonical tip.  Other constraints (child counts, the leftmost deepest
//! tip, which block an off-chain node builds on and so which replay results it waits on, and
//! the blocks under a dead one being dead) are not modelled.
//!
//! Usage:
//! ```text
//! cargo run --release -p fandango-targets --example alpenglow_seqcov -- CORPUS_DIR [-t MAX_T] [-n SAMPLES] [-s SEED] [-j JOBS]
//! ```

use anyhow::{Context, Error, bail};
use clap::Parser;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering::Relaxed};
use std::thread;

/// Measure t-way sequence coverage of a `consensus_tree` corpus.
#[derive(Parser)]
struct Args {
    /// Directory of scenario action lists
    corpus: PathBuf,

    /// Largest sequence length to measure
    #[arg(short, default_value = "3")]
    t: usize,

    /// Random t-sequences to check for each t with too many to count
    #[arg(short = 'n', long, default_value = "10000")]
    samples: usize,

    /// Seed for drawing the random t-sequences
    #[arg(short, long, default_value = "0")]
    seed: u64,

    /// Threads [default: one per CPU]
    #[arg(short, long)]
    jobs: Option<usize>,
}

/// A set of packed t-sequences, one bit each, that threads add to together.
struct Bitset(Vec<AtomicU64>);

impl Bitset {
    fn new(bits: usize) -> Self {
        Self((0..bits.div_ceil(64)).map(|_| AtomicU64::new(0)).collect())
    }

    fn insert(&self, key: u64) {
        let (word, bit) = (&self.0[(key / 64) as usize], 1u64 << (key % 64));
        /* Most keys are already set, and a load leaves the cache line shared */
        if word.load(Relaxed) & bit == 0 {
            word.fetch_or(bit, Relaxed);
        }
    }

    fn contains(&self, key: u64) -> bool {
        self.0[(key / 64) as usize].load(Relaxed) & (1u64 << (key % 64)) != 0
    }

    fn len(&self) -> u128 {
        self.0.iter().map(|w| u128::from(w.load(Relaxed).count_ones())).sum()
    }
}

/// Largest universe counted exactly, rather than sampled.
const EXACT_MAX: u128 = 10_000_000;

/// Deepest slot: `consensus_tree`'s default depth limit grows trees at most this deep.
const SLOTS: usize = 18;
/// Most nodes in a slot.
const WIDTH: usize = 5;
/// Most children of a node, the root's included, as `consensus_tree.fan` gives them.
const MAX_CHILDREN: usize = 4;

/// Certificates, one bit each.
const NOTAR_FALLBACK: u8 = 1;
const NOTARIZE: u8 = 2;
const FINALIZE: u8 = 4;
const FAST_FINALIZE: u8 = 8;
const SKIP_CERT: u8 = 16;
const CERTS: [(&str, u8); 5] = [
    ("NOTAR_FALLBACK_CERT", NOTAR_FALLBACK),
    ("NOTARIZE_CERT", NOTARIZE),
    ("FINALIZE_CERT", FINALIZE),
    ("FAST_FINALIZE_CERT", FAST_FINALIZE),
    ("SKIP_CERT", SKIP_CERT),
];

/// The certificates for a canonical node in each settle mode, as `consensus_tree` settles
/// slots.
const MODES: [u8; 8] = [
    NOTAR_FALLBACK,
    NOTARIZE | FINALIZE,
    FAST_FINALIZE,
    FAST_FINALIZE | NOTARIZE,
    NOTARIZE,
    FINALIZE,
    NOTARIZE | FINALIZE | FAST_FINALIZE,
    SKIP_CERT,
];
const SKIP: usize = 7;
/// Most notar fallback certificates for off-chain nodes beside each settle mode.
const OTHERS: [usize; MODES.len()] = [3, 0, 0, 0, 2, 0, 0, 2];

/// A slot's events: settle modes, the canonical block's arrival and replay result, then 4
/// events for each off-chain node.
const ARRIVE: usize = MODES.len();
const REPLAY: usize = ARRIVE + 1;
const OFF_CHAIN: usize = REPLAY + 1;
const OFF_ARRIVE: usize = 0;
const OFF_REPLAY: usize = 1;
const OFF_DEAD: usize = 2;
const OFF_FALLBACK: usize = 3;
const PER_NODE: usize = 4;
const PER_SLOT: usize = OFF_CHAIN + WIDTH * PER_NODE;
const EVENTS: usize = SLOTS * PER_SLOT;

/// Event classes, for the report: settle modes, canonical events, then off-chain ones.
const CLASSES: [&str; OFF_CHAIN + PER_NODE] = [
    "fallback", "notar+fin", "fast", "fast+notar", "notar", "final", "n+f+fast", "skip",
    "arrive", "replay", "off.arrive", "off.replay", "off.dead", "off.fallbk",
];

/// An event, numbered from 0, slot by slot.
type Event = usize;

fn settle(slot: usize, mode: usize) -> Event {
    (slot - 1) * PER_SLOT + mode
}

fn canonical_arrive(slot: usize) -> Event {
    (slot - 1) * PER_SLOT + ARRIVE
}

fn canonical_replay(slot: usize) -> Event {
    (slot - 1) * PER_SLOT + REPLAY
}

fn off_chain(slot: usize, index: usize, kind: usize) -> Event {
    (slot - 1) * PER_SLOT + OFF_CHAIN + index * PER_NODE + kind
}

fn slot_of(e: Event) -> usize {
    e / PER_SLOT + 1
}

fn class_of(e: Event) -> usize {
    let local = e % PER_SLOT;
    if local < OFF_CHAIN { local } else { OFF_CHAIN + (local - OFF_CHAIN) % PER_NODE }
}

/// The label index of an off-chain event's node.
fn node_of(e: Event) -> Option<usize> {
    let local = e % PER_SLOT;
    (local >= OFF_CHAIN).then(|| (local - OFF_CHAIN) / PER_NODE)
}

/// Whether event `b` can come before event `a` in one scenario.
fn compatible(b: Event, a: Event) -> bool {
    let (sa, sb, ca, cb) = (slot_of(a), slot_of(b), class_of(a), class_of(b));
    let same_slot = sa == sb;
    let settles = ca < ARRIVE && cb < ARRIVE;
    let skipped_replay = (ca == SKIP && (cb == ARRIVE || cb == REPLAY)) || (cb == SKIP && (ca == ARRIVE || ca == REPLAY));
    /* A canonical replay result follows the arrival and result of every canonical block up to it */
    let canonical_early = cb == REPLAY && ((ca == ARRIVE && sa <= sb) || (ca == REPLAY && sa < sb));
    let kind = |c: usize| c - OFF_CHAIN;
    let same_node = same_slot && node_of(a).is_some() && node_of(a) == node_of(b);
    let node_clash = same_node && {
        let (ka, kb) = (kind(ca), kind(cb));
        let results = |k: usize| k == OFF_REPLAY || k == OFF_DEAD;
        (results(ka) && results(kb))
            || ((ka, kb) == (OFF_DEAD, OFF_FALLBACK) || (ka, kb) == (OFF_FALLBACK, OFF_DEAD))
            || (ka == OFF_ARRIVE && results(kb))
    };
    !(a == b || (same_slot && (settles || skipped_replay)) || canonical_early || node_clash)
}

/// The most off-chain nodes in a slot.
fn max_nodes(slot: usize) -> usize {
    if slot == 1 { MAX_CHILDREN - 1 } else { WIDTH - 1 }
}

/// Whether an off-chain node can have a label: the root has at most 4 children, and the
/// leftmost node in the last slot is always the canonical tip.
fn off_chain_label(slot: usize, index: usize) -> bool {
    !((slot == 1 && index >= MAX_CHILDREN) || (slot == SLOTS && index == 0))
}

/// Whether the events could all occur in one scenario, in this order.
fn feasible(events: &[Event]) -> bool {
    let mut mode = [None; SLOTS + 1];
    let mut nodes = [[false; WIDTH]; SLOTS + 1];
    let mut fallbacks = [0usize; SLOTS + 1];
    for (i, &a) in events.iter().enumerate() {
        let (slot, class) = (slot_of(a), class_of(a));
        if class == SKIP && slot == SLOTS {
            return false;
        }
        if class < ARRIVE {
            mode[slot] = Some(class);
        }
        if let Some(index) = node_of(a) {
            if !off_chain_label(slot, index) {
                return false;
            }
            nodes[slot][index] = true;
            fallbacks[slot] += usize::from(class == OFF_CHAIN + OFF_FALLBACK);
        }
        if !events[..i].iter().all(|&b| compatible(b, a)) {
            return false;
        }
    }
    (1..=SLOTS).all(|slot| {
        nodes[slot].iter().filter(|&&n| n).count() <= max_nodes(slot)
            && fallbacks[slot] <= mode[slot].map_or(OTHERS[0], |m| OTHERS[m])
    })
}

/// A slot's feasible event sets, counted by size, whether they hold the canonical block's
/// arrival and its replay result, and how many off-chain blocks' arrivals and replay results
/// they hold both of.
fn slot_sets(slot: usize) -> HashMap<(usize, bool, bool, usize), u128> {
    /* Each off-chain node holds some of its events: at most one replay result, and no notar
       fallback certificate for a dead block.  Count the nodes' sets together by size, pairs
       of arrival and result, nodes and notar fallback certificates. */
    let bit = |kind: usize| 1u32 << kind;
    let node_sets: Vec<u32> = (0..1u32 << PER_NODE)
        .filter(|s| s & (bit(OFF_REPLAY) | bit(OFF_DEAD)) != bit(OFF_REPLAY) | bit(OFF_DEAD))
        .filter(|s| s & (bit(OFF_DEAD) | bit(OFF_FALLBACK)) != bit(OFF_DEAD) | bit(OFF_FALLBACK))
        .collect();
    let mut off: HashMap<(usize, usize, usize, usize), u128> = HashMap::from([((0, 0, 0, 0), 1)]);
    for _ in (0..WIDTH).filter(|&i| off_chain_label(slot, i)) {
        let mut next = HashMap::new();
        for (&(size, pairs, nodes, fallbacks), &count) in &off {
            for &s in &node_sets {
                let pair = s & bit(OFF_ARRIVE) != 0 && s & (bit(OFF_REPLAY) | bit(OFF_DEAD)) != 0;
                let key = (
                    size + s.count_ones() as usize,
                    pairs + usize::from(pair),
                    nodes + usize::from(s != 0),
                    fallbacks + usize::from(s & bit(OFF_FALLBACK) != 0),
                );
                *next.entry(key).or_default() += count;
            }
        }
        off = next;
    }

    let mut sets = HashMap::new();
    for mode in [None].into_iter().chain((0..MODES.len()).map(Some)) {
        if mode == Some(SKIP) && slot == SLOTS {
            continue;
        }
        for (arrive, replay) in [(false, false), (true, false), (false, true), (true, true)] {
            if mode == Some(SKIP) && (arrive || replay) {
                continue;
            }
            let max_fallbacks = mode.map_or(OTHERS[0], |m| OTHERS[m]);
            for (&(size, pairs, nodes, fallbacks), &count) in &off {
                if nodes <= max_nodes(slot) && fallbacks <= max_fallbacks {
                    let size = size + usize::from(mode.is_some()) + usize::from(arrive) + usize::from(replay);
                    *sets.entry((size, arrive, replay, pairs)).or_default() += count;
                }
            }
        }
    }
    sets
}

fn factorial(n: usize) -> u128 {
    (1..=n as u128).product()
}

/// Number of feasible ordered t-tuples of events in `slots`, for each t up to `t_max`.
fn universe(slots: &[usize], t_max: usize) -> Vec<u128> {
    /* Which events a scenario can hold is constrained only within a slot, so take each
       slot's feasible event sets in turn, and count the orders of the canonical events so
       far: each canonical arrival fits anywhere among them, and each canonical replay result
       after all of them. */
    let mut ways: HashMap<(usize, usize, usize), u128> = HashMap::from([((0, 0, 0), 1)]);
    for &slot in slots {
        let sets = slot_sets(slot);
        let mut next = HashMap::new();
        for (&(size, canonical, pairs), &w) in &ways {
            for (&(s, arrive, replay, p), &count) in &sets {
                if size + s > t_max {
                    continue;
                }
                let orders = if arrive { canonical as u128 + 1 } else { 1 };
                let key = (size + s, canonical + usize::from(arrive) + usize::from(replay), pairs + p);
                *next.entry(key).or_default() += w * count * orders;
            }
        }
        ways = next;
    }
    /* Interleave the canonical events with the rest, where each off-chain block's arrival
       comes before its replay result */
    let mut universe = vec![0u128; t_max + 1];
    for ((t, canonical, pairs), w) in ways {
        let choose = factorial(t) / (factorial(canonical) * factorial(t - canonical));
        universe[t] += w * choose * (factorial(t - canonical) >> pairs);
    }
    universe
}

fn name(e: Event) -> String {
    let label = node_of(e).map_or(String::new(), |i| char::from(b'a' + i as u8).to_string());
    format!("{}:{}{label}", CLASSES[class_of(e)], slot_of(e))
}

/// A uniformly random feasible t-sequence: draw t distinct events until they are feasible.
fn draw(rng: &mut StdRng, t: usize) -> Vec<Event> {
    let mut events: Vec<Event> = (0..EVENTS).collect();
    loop {
        for i in 0..t {
            let j = rng.random_range(i..EVENTS);
            events.swap(i, j);
        }
        if feasible(&events[..t]) {
            return events[..t].to_vec();
        }
    }
}

/// Whether a scenario, by each event's position in it, holds the sequence in order.
fn in_order(sequence: &[Event], position: &[usize; EVENTS]) -> bool {
    sequence.windows(2).all(|w| position[w[0]] < position[w[1]]) && position[sequence[sequence.len() - 1]] != usize::MAX
}

/// A node's slot and index, or (0, 0) for the root.
fn parse_node(label: &str) -> Result<(usize, usize), Error> {
    if label == "0" {
        return Ok((0, 0));
    }
    let bad = || format!("bad node label {label}");
    let (digits, letter) = label.split_at(label.find(|c: char| !c.is_ascii_digit()).with_context(bad)?);
    let slot: usize = digits.parse().with_context(bad)?;
    if !(1..=SLOTS).contains(&slot) || letter.len() != 1 || !(b'a'..b'a' + WIDTH as u8).contains(&letter.as_bytes()[0]) {
        bail!("{}, as slots run 1 to {SLOTS} with at most {WIDTH} nodes", bad());
    }
    Ok((slot, (letter.as_bytes()[0] - b'a') as usize))
}

/// A scenario's events in order.
fn events(text: &str) -> Result<Vec<Event>, Error> {
    let mut nodes: HashMap<(usize, usize), Vec<(String, usize)>> = HashMap::new(); /* action kinds and positions */
    let mut parents = HashMap::new();
    for (position, object) in text.split('{').skip(1).enumerate() {
        let fields: Vec<&str> = object.split('"').collect();
        let value = |key: &str| fields.iter().position(|f| *f == key).map(|i| fields[i + 2]);
        let Some(node) = value("node") else { continue }; /* CLOCK */
        let kind = value("action").context("action without a kind")?.to_string();
        let node = parse_node(node)?;
        parents.insert(node, parse_node(value("parent").context("action without a parent")?)?);
        nodes.entry(node).or_default().push((kind, position));
    }

    /* The canonical chain runs up from the tip, the leftmost node of the deepest slot,
       through the block each node cites, and takes in the skipped slots it passes over */
    let deepest = nodes.keys().map(|&(slot, _)| slot).max().context("a scenario without nodes")?;
    let mut canonical: Vec<(usize, usize)> =
        nodes.iter().filter(|(_, actions)| actions.iter().any(|(k, _)| k == "SKIP_CERT")).map(|(&n, _)| n).collect();
    let mut node = (deepest, 0);
    while node.0 > 0 {
        canonical.push(node);
        let parent = *parents.get(&node).with_context(|| format!("canonical node {node:?} has no actions"))?;
        if parent.0 >= node.0 {
            bail!("node {node:?} cites {parent:?}, no shallower than itself");
        }
        node = parent;
    }
    canonical.sort_unstable();
    if !canonical.iter().map(|&(slot, _)| slot).eq(1..=deepest) {
        bail!("canonical nodes {canonical:?} do not take one per slot");
    }

    let mut events: Vec<(usize, Event)> = Vec::new();
    for (&(slot, index), actions) in &nodes {
        let at = |kind: &str| actions.iter().find(|(k, _)| k == kind).map(|&(_, p)| p);
        let label = || format!("{slot}{}", char::from(b'a' + index as u8));
        if canonical.contains(&(slot, index)) {
            let mut certs = 0;
            let mut last_cert = 0;
            for (kind, p) in actions {
                match CERTS.iter().find(|(k, _)| k == kind) {
                    Some(&(_, bit)) => {
                        certs |= bit;
                        last_cert = last_cert.max(*p);
                    }
                    None if kind == "REPLAY_ARRIVES" || kind == "REPLAY_COMPLETE" => {}
                    None => bail!("canonical node {} has a {kind}", label()),
                }
            }
            let mode = MODES.iter().position(|&m| m == certs).with_context(|| format!("canonical node {} has certificates {certs:#07b}", label()))?;
            events.push((last_cert, settle(slot, mode)));
            events.extend(at("REPLAY_ARRIVES").map(|p| (p, canonical_arrive(slot))));
            events.extend(at("REPLAY_COMPLETE").map(|p| (p, canonical_replay(slot))));
        } else {
            let kinds = [
                ("REPLAY_ARRIVES", OFF_ARRIVE),
                ("REPLAY_COMPLETE", OFF_REPLAY),
                ("REPLAY_DEAD", OFF_DEAD),
                ("NOTAR_FALLBACK_CERT", OFF_FALLBACK),
            ];
            if let Some((kind, _)) = actions.iter().find(|(k, _)| !kinds.iter().any(|(known, _)| k == known)) {
                bail!("off-chain node {} has a {kind}", label());
            }
            events.extend(kinds.iter().filter_map(|&(kind, off)| at(kind).map(|p| (p, off_chain(slot, index, off)))));
        }
    }
    events.sort_unstable();
    Ok(events.into_iter().map(|(_, e)| e).collect())
}

/// Every in-order t-subsequence of `order`, packed base `EVENTS` into a u64.
fn subsequences(order: &[Event], t: usize, into: &mut impl FnMut(u64)) {
    fn go(order: &[Event], t: usize, key: u64, into: &mut impl FnMut(u64)) {
        if t == 0 {
            into(key);
            return;
        }
        for (i, &e) in order.iter().enumerate() {
            go(&order[i + 1..], t - 1, key * EVENTS as u64 + e as u64, into);
        }
    }
    go(order, t, 0, into);
}

fn main() -> Result<(), Error> {
    let args = Args::parse();
    if args.t == 0 || args.t > 14 {
        bail!("t must be 1 to 14, as {EVENTS}^t must fit a u128");
    }
    let universe = universe(&(1..=SLOTS).collect::<Vec<_>>(), args.t);
    let exact: Vec<bool> = universe.iter().map(|&u| u <= EXACT_MAX).collect();
    let seen: Vec<Bitset> = (0..=args.t).map(|t| Bitset::new(if t > 0 && exact[t] { EVENTS.pow(t as u32) } else { 0 })).collect();
    let mut rng = StdRng::seed_from_u64(args.seed);
    let samples: Vec<Vec<Vec<Event>>> =
        (0..=args.t).map(|t| if t == 0 || exact[t] { Vec::new() } else { (0..args.samples).map(|_| draw(&mut rng, t)).collect() }).collect();
    let found: Vec<Vec<AtomicBool>> = samples.iter().map(|s| s.iter().map(|_| AtomicBool::new(false)).collect()).collect();

    let paths: Vec<PathBuf> = fs::read_dir(&args.corpus)
        .with_context(|| format!("could not read {}", args.corpus.display()))?
        .map(|entry| entry.map(|e| e.path()))
        .collect::<Result<_, _>>()?;
    let jobs = args.jobs.unwrap_or_else(|| thread::available_parallelism().map_or(1, |n| n.get()));
    let next = AtomicUsize::new(0);

    /* Each thread takes the next file until none are left, adding the sequences it finds
       to the shared bitsets and flags */
    let scan = || -> Result<(), Error> {
        let mut unfound: Vec<Vec<usize>> = samples.iter().map(|s| (0..s.len()).collect()).collect();
        while let Some(path) = paths.get(next.fetch_add(1, Relaxed)) {
            let order = events(&fs::read_to_string(path)?).with_context(|| format!("could not parse {}", path.display()))?;
            if !feasible(&order) {
                bail!("{}: events outside the modelled universe", path.display());
            }
            let mut position = [usize::MAX; EVENTS];
            order.iter().enumerate().for_each(|(i, &e)| position[e] = i);
            for t in 1..=args.t {
                if exact[t] {
                    subsequences(&order, t, &mut |key| seen[t].insert(key));
                } else {
                    unfound[t].retain(|&i| {
                        if found[t][i].load(Relaxed) {
                            return false;
                        }
                        let hit = in_order(&samples[t][i], &position);
                        if hit {
                            found[t][i].store(true, Relaxed);
                        }
                        !hit
                    });
                }
            }
        }
        Ok(())
    };
    thread::scope(|scope| {
        let workers: Vec<_> = (0..jobs).map(|_| scope.spawn(&scan)).collect();
        workers.into_iter().try_for_each(|w| w.join().expect("scan thread panicked"))
    })?;
    let files = paths.len();
    let uncovered: Vec<Vec<&Vec<Event>>> =
        samples.iter().zip(&found).map(|(s, f)| s.iter().zip(f).filter(|(_, f)| !f.load(Relaxed)).map(|(s, _)| s).collect()).collect();

    println!("{files} scenarios, k = {EVENTS} events");
    for t in 1..=args.t {
        if exact[t] {
            let covered = seen[t].len();
            println!("t={t}: {covered}/{} sequences ({:.2}%)", universe[t], 100.0 * covered as f64 / universe[t] as f64);
        } else {
            let n = args.samples as f64;
            let p = (args.samples - uncovered[t].len()) as f64 / n;
            let margin = 1.96 * (p * (1.0 - p) / n).sqrt();
            println!("t={t}: {:.2}% +- {:.2}% of {} sequences, from {} samples", 100.0 * p, 100.0 * margin, universe[t], args.samples);
            for sequence in uncovered[t].iter().take(3) {
                println!("  missing: {}", sequence.iter().map(|&e| name(e)).collect::<Vec<_>>().join(" "));
            }
        }
    }

    /* Pairs by event class: which orders of two events the corpus reaches */
    if args.t >= 2 {
        let n = CLASSES.len();
        let mut total = vec![vec![0u64; n]; n];
        let mut covered = vec![vec![0u64; n]; n];
        for a in 0..EVENTS {
            for b in 0..EVENTS {
                if feasible(&[a, b]) {
                    total[class_of(a)][class_of(b)] += 1;
                    covered[class_of(a)][class_of(b)] += u64::from(seen[2].contains((a * EVENTS + b) as u64));
                }
            }
        }
        println!("\nt=2 coverage by class, first event's class down, second's across:");
        print!("{:>11}", "");
        CLASSES.iter().for_each(|c| print!("{c:>11}"));
        println!();
        for a in 0..n {
            print!("{:>11}", CLASSES[a]);
            for b in 0..n {
                if total[a][b] == 0 {
                    print!("{:>11}", "-");
                } else {
                    print!("{:>10.1}%", 100.0 * covered[a][b] as f64 / total[a][b] as f64);
                }
            }
            println!();
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_in_scenario_order() {
        /* 1a notarized and 1b given a notar fallback cert, 2a skipped and 2b under 1b dead,
           3a the tip citing 1a, fast finalized */
        let text = concat!(
            "[{\"node\": \"1b\", \"parent\": \"0\", \"action\": \"REPLAY_ARRIVES\"}, ",
            "{\"node\": \"3a\", \"parent\": \"1a\", \"action\": \"FAST_FINALIZE_CERT\"}, ",
            "{\"node\": \"1a\", \"parent\": \"0\", \"action\": \"REPLAY_ARRIVES\"}, ",
            "{\"action\": \"CLOCK\", \"ms\": 50}, ",
            "{\"node\": \"1b\", \"parent\": \"0\", \"action\": \"NOTAR_FALLBACK_CERT\"}, ",
            "{\"node\": \"2a\", \"parent\": \"1a\", \"action\": \"SKIP_CERT\"}, ",
            "{\"node\": \"1a\", \"parent\": \"0\", \"action\": \"REPLAY_COMPLETE\"}, ",
            "{\"node\": \"1b\", \"parent\": \"0\", \"action\": \"REPLAY_COMPLETE\"}, ",
            "{\"node\": \"2b\", \"parent\": \"1b\", \"action\": \"REPLAY_ARRIVES\"}, ",
            "{\"node\": \"3a\", \"parent\": \"1a\", \"action\": \"REPLAY_ARRIVES\"}, ",
            "{\"node\": \"1a\", \"parent\": \"0\", \"action\": \"NOTARIZE_CERT\"}, ",
            "{\"node\": \"2b\", \"parent\": \"1b\", \"action\": \"REPLAY_DEAD\"}, ",
            "{\"node\": \"3a\", \"parent\": \"1a\", \"action\": \"REPLAY_COMPLETE\"}]\n"
        );
        let order = events(text).unwrap();
        assert_eq!(order, [
            off_chain(1, 1, OFF_ARRIVE),
            settle(3, 2),
            canonical_arrive(1),
            off_chain(1, 1, OFF_FALLBACK),
            settle(2, SKIP),
            canonical_replay(1),
            off_chain(1, 1, OFF_REPLAY),
            off_chain(2, 1, OFF_ARRIVE),
            canonical_arrive(3),
            settle(1, 4),
            off_chain(2, 1, OFF_DEAD),
            canonical_replay(3),
        ]);
        assert!(feasible(&order));
    }

    #[test]
    fn universe_counts_feasible_tuples() {
        let count = |events: &[Event], t: usize| -> u128 {
            let mut tuples = 0;
            for &a in events {
                if t == 1 {
                    tuples += u128::from(feasible(&[a]));
                    continue;
                }
                for &b in events {
                    if t == 2 {
                        tuples += u128::from(feasible(&[a, b]));
                        continue;
                    }
                    for &c in events {
                        tuples += u128::from(feasible(&[a, b, c]));
                    }
                }
            }
            tuples
        };
        let all: Vec<Event> = (0..EVENTS).collect();
        let universe_all = universe(&(1..=SLOTS).collect::<Vec<_>>(), 2);
        assert_eq!(universe_all[1], count(&all, 1));
        assert_eq!(universe_all[2], count(&all, 2));
        /* All triples are too many to check, so check those in the first two slots and the last */
        let slots = [1, 2, SLOTS];
        let some: Vec<Event> = all.iter().copied().filter(|&e| slots.contains(&slot_of(e))).collect();
        assert_eq!(universe(&slots, 3)[3], count(&some, 3));
    }

    #[test]
    fn off_chain_nodes_fit_beside_the_canonical_one() {
        let four: Vec<Event> = (0..4).map(|i| off_chain(2, i, OFF_REPLAY)).collect();
        assert!(feasible(&four));
        assert!(!feasible(&[four.as_slice(), &[off_chain(2, 4, OFF_DEAD)]].concat()));
        assert!(!feasible(&[settle(2, SKIP), canonical_replay(2)]));
        assert!(!feasible(&[off_chain(2, 0, OFF_REPLAY), off_chain(2, 0, OFF_DEAD)]));
        assert!(!feasible(&[off_chain(2, 0, OFF_DEAD), off_chain(2, 0, OFF_FALLBACK)]));
        assert!(!feasible(&[off_chain(1, 4, OFF_REPLAY)]));
        assert!(!feasible(&(0..4).map(|i| off_chain(1, i, OFF_REPLAY)).collect::<Vec<_>>()));
        assert!(!feasible(&[off_chain(SLOTS, 0, OFF_DEAD)]));
        assert!(!feasible(&[settle(SLOTS, SKIP)]));
    }

    #[test]
    fn notar_fallback_certs_fit_the_settle_mode() {
        let fallbacks: Vec<Event> = (0..3).map(|i| off_chain(2, i, OFF_FALLBACK)).collect();
        assert!(feasible(&fallbacks));
        assert!(feasible(&[fallbacks.as_slice(), &[settle(2, 0)]].concat()));
        assert!(!feasible(&[fallbacks.as_slice(), &[settle(2, 4)]].concat()));
        assert!(feasible(&[settle(2, SKIP), fallbacks[0], fallbacks[1]]));
        assert!(!feasible(&[settle(2, 2), fallbacks[0]]));
        assert!(!feasible(&[fallbacks.as_slice(), &[off_chain(2, 3, OFF_FALLBACK)]].concat()));
    }

    #[test]
    fn replay_results_follow_arrivals_and_the_chain() {
        assert!(feasible(&[off_chain(2, 1, OFF_ARRIVE), off_chain(2, 1, OFF_DEAD)]));
        assert!(!feasible(&[off_chain(2, 1, OFF_REPLAY), off_chain(2, 1, OFF_ARRIVE)]));
        assert!(!feasible(&[canonical_replay(2), canonical_arrive(2)]));
        assert!(feasible(&[canonical_replay(2), canonical_replay(3)]));
        assert!(!feasible(&[canonical_replay(3), canonical_replay(2)]));
        assert!(feasible(&[canonical_replay(2), canonical_arrive(3)]));
        assert!(!feasible(&[canonical_replay(3), canonical_arrive(2)]));
        assert!(feasible(&[canonical_arrive(3), canonical_replay(2)]));
        assert!(feasible(&[off_chain(3, 0, OFF_REPLAY), canonical_replay(2)]));
    }

    #[test]
    fn in_order_needs_every_event_in_order() {
        let mut position = [usize::MAX; EVENTS];
        position[7] = 0;
        position[3] = 4;
        position[9] = 2;
        assert!(in_order(&[7, 9, 3], &position));
        assert!(!in_order(&[9, 7, 3], &position));
        assert!(!in_order(&[7, 9, 3, 1], &position));
        assert!(!in_order(&[1, 7], &position));
    }

    #[test]
    fn draws_are_feasible() {
        let mut rng = StdRng::seed_from_u64(1);
        assert!((0..1000).all(|_| feasible(&draw(&mut rng, 5))));
    }

    #[test]
    fn subsequences_keep_order() {
        let mut got = Vec::new();
        subsequences(&[1, 2, 3], 2, &mut |key| got.push(key));
        let pack = |a: u64, b: u64| a * EVENTS as u64 + b;
        assert_eq!(got, [pack(1, 2), pack(1, 3), pack(2, 3)]);
    }
}
