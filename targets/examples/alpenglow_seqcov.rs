//! Measure t-way sequence coverage of an `alpenglow_altpath` corpus.
//!
//! The events, k = 48 + 80 = 128:
//! - for each slot's canonical node, how the cluster settled it (notar fallback, notar and
//!   final, fast final, fast final and notar, or skip), when its last certificate lands, and
//!   its replay completing: 8 slots x (5 + 1) = 48,
//! - for each node off the canonical chain, by slot and label (its position in the slot, a
//!   to e), replay completing it or finding it dead: 8 slots x 5 x 2 = 80.
//!
//! A t-sequence is covered once some scenario has those t events in that order, not
//! necessarily adjacent.  Up to 10 million t-sequences, every one is counted; past that
//! (t = 4 on), coverage is estimated from random t-sequences of the universe, drawn
//! uniformly, with a 95% interval, as the corpus holds too many distinct ones to keep.  Its universe is every ordered t-tuple of distinct events that one
//! scenario could hold: one settle mode per slot, no skip in the last slot (the canonical
//! tip would lie below it), no replay of a skipped slot, one event per off-chain node, and
//! at most 4 off-chain nodes in a slot, as the canonical node takes one of its 5 labels.
//! Slot 1 holds the root's children, at most 4, so it has at most 3 off-chain nodes, a to
//! d, and `8a`, the leftmost node of the last slot, is always the canonical tip.  Other
//! constraints across slots (child counts, the leftmost deepest tip) are not modelled.
//!
//! Usage:
//! ```text
//! cargo run --release -p fandango-targets --example alpenglow_seqcov --features alpenglow -- CORPUS_DIR [-t MAX_T] [-n SAMPLES] [-s SEED]
//! ```

use anyhow::{Context, Error, bail};
use fandango_targets::alpenglow::Tree;
use clap::Parser;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;

/// Measure t-way sequence coverage of an alpenglow_altpath corpus.
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
}

/// Largest universe counted exactly, rather than sampled.
const EXACT_MAX: u128 = 10_000_000;

const SLOTS: usize = 8;
const WIDTH: usize = 5;
const SKIP: usize = 4;
const REPLAY: usize = 5;
const CANONICAL: usize = SLOTS * 6;
const EVENTS: usize = CANONICAL + SLOTS * WIDTH * 2;

/// Event classes, for the report: settle modes, then canonical replay, then off-chain.
const CLASSES: [&str; 8] = ["fallback", "notar+final", "fast", "fast+notar", "skip", "replay", "off.replay", "off.dead"];

/// An event, numbered from 0: canonical events first, then off-chain ones.
type Event = usize;

fn settle(slot: usize, mode: usize) -> Event {
    (slot - 1) * 6 + mode
}

fn canonical_replay(slot: usize) -> Event {
    (slot - 1) * 6 + REPLAY
}

fn off_chain(slot: usize, index: usize, dead: bool) -> Event {
    CANONICAL + ((slot - 1) * WIDTH + index) * 2 + usize::from(dead)
}

fn slot_of(e: Event) -> usize {
    if e < CANONICAL { e / 6 + 1 } else { (e - CANONICAL) / 2 / WIDTH + 1 }
}

fn class_of(e: Event) -> usize {
    if e < CANONICAL { e % 6 } else { 6 + (e - CANONICAL) % 2 }
}

/// Whether the events could all occur in one scenario.
fn feasible(events: &[Event]) -> bool {
    let mut off_chain_nodes = [0usize; SLOTS + 1];
    for (i, &a) in events.iter().enumerate() {
        let (slot, class) = (slot_of(a), class_of(a));
        if class == SKIP && slot == SLOTS {
            return false;
        }
        if a >= CANONICAL {
            /* The root has at most 4 children, one of them canonical, and the leftmost
               node in the last slot is always the canonical tip */
            let index = (a - CANONICAL) / 2 % WIDTH;
            off_chain_nodes[slot] += 1;
            let max = if slot == 1 { Tree::MAX_CHILDREN - 1 } else { WIDTH - 1 };
            if off_chain_nodes[slot] > max || ( slot == 1 && index >= Tree::MAX_CHILDREN ) || ( slot == SLOTS && index == 0 ) {
                return false;
            }
        }
        for &b in &events[..i] {
            let same_slot = slot == slot_of(b);
            let settles = class < REPLAY && class_of(b) < REPLAY;
            let skipped_replay = (class == SKIP && class_of(b) == REPLAY) || (class == REPLAY && class_of(b) == SKIP);
            let same_node = a >= CANONICAL && b >= CANONICAL && (a - CANONICAL) / 2 == (b - CANONICAL) / 2;
            if a == b || same_node || ( same_slot && ( settles || skipped_replay ) ) {
                return false;
            }
        }
    }
    true
}

/// Number of feasible ordered t-tuples of events, for each t up to `t_max`.
fn universe(t_max: usize) -> Vec<u128> {
    /* Constraints stay within a slot, so count each slot's feasible event sets by size,
       multiply the slots' polynomials, and order each set of t events t! ways. */
    let mut sets = vec![1u128];
    for slot in 1..=SLOTS {
        let slot_events: Vec<Event> = (0..EVENTS).filter(|&e| slot_of(e) == slot).collect();
        let mut per_slot = vec![0u128; slot_events.len() + 1];
        for mask in 0u32..1 << slot_events.len() {
            let set: Vec<Event> = slot_events.iter().enumerate().filter(|(i, _)| mask >> i & 1 == 1).map(|(_, &e)| e).collect();
            if feasible(&set) {
                per_slot[set.len()] += 1;
            }
        }
        let mut next = vec![0u128; sets.len() + per_slot.len() - 1];
        for (a, &x) in sets.iter().enumerate() {
            for (b, &y) in per_slot.iter().enumerate() {
                next[a + b] += x * y;
            }
        }
        sets = next;
    }
    (0..=t_max).map(|t| sets.get(t).copied().unwrap_or(0) * (1..=t as u128).product::<u128>()).collect()
}

fn name(e: Event) -> String {
    let label = if e >= CANONICAL { char::from(b'a' + ((e - CANONICAL) / 2 % WIDTH) as u8).to_string() } else { String::new() };
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

fn parse_node(label: &str) -> Result<(usize, usize), Error> {
    let b = label.as_bytes();
    if b.len() != 2 || !(b'1'..=b'8').contains(&b[0]) || !(b'a'..=b'e').contains(&b[1]) {
        bail!("bad node label {label}");
    }
    Ok(((b[0] - b'0') as usize, (b[1] - b'a') as usize))
}

/// A scenario's events in order.
fn events(text: &str) -> Result<Vec<Event>, Error> {
    let mut nodes: HashMap<(usize, usize), Vec<(String, usize)>> = HashMap::new(); /* action kinds and positions */
    for (position, object) in text.split('{').skip(1).enumerate() {
        let fields: Vec<&str> = object.split('"').collect();
        let value = |key: &str| fields.iter().position(|f| *f == key).map(|i| fields[i + 2]);
        let Some(node) = value("node") else { continue }; /* CLOCK */
        let kind = value("action").context("action without a kind")?.to_string();
        nodes.entry(parse_node(node)?).or_default().push((kind, position));
    }

    let mut events: Vec<(usize, Event)> = Vec::new();
    for ((slot, index), actions) in nodes {
        let at = |kind: &str| actions.iter().find(|(k, _)| k == kind).map(|&(_, p)| p);
        let certs = ["NOTARIZE_CERT", "FINALIZE_CERT", "NOTAR_FALLBACK_CERT", "FAST_FINALIZE_CERT", "SKIP_CERT"];
        let settled = actions.iter().filter(|(k, _)| certs.contains(&k.as_str())).map(|&(_, p)| p).max();
        match settled {
            Some(last_cert) => {
                let mode = if at("SKIP_CERT").is_some() {
                    SKIP
                } else if at("NOTAR_FALLBACK_CERT").is_some() {
                    0
                } else if at("FAST_FINALIZE_CERT").is_some() {
                    if at("NOTARIZE_CERT").is_some() { 3 } else { 2 }
                } else {
                    1
                };
                events.push((last_cert, settle(slot, mode)));
                events.extend(at("REPLAY_COMPLETE").map(|p| (p, canonical_replay(slot))));
            }
            None => match (at("REPLAY_COMPLETE"), at("REPLAY_DEAD")) {
                (Some(p), None) => events.push((p, off_chain(slot, index, false))),
                (None, Some(p)) => events.push((p, off_chain(slot, index, true))),
                _ => bail!("off-chain node {slot}{} neither replayed nor dead", char::from(b'a' + index as u8)),
            },
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
    if args.t == 0 || args.t > 18 {
        bail!("t must be 1 to 18, as 128^t must fit a u128");
    }
    let universe = universe(args.t);
    let exact: Vec<bool> = universe.iter().map(|&u| u <= EXACT_MAX).collect();
    let mut seen: Vec<HashSet<u64>> = vec![HashSet::new(); args.t + 1];
    let mut rng = StdRng::seed_from_u64(args.seed);
    let mut uncovered: Vec<Vec<Vec<Event>>> =
        (0..=args.t).map(|t| if t == 0 || exact[t] { Vec::new() } else { (0..args.samples).map(|_| draw(&mut rng, t)).collect() }).collect();
    let mut files = 0;

    for entry in fs::read_dir(&args.corpus).with_context(|| format!("could not read {}", args.corpus.display()))? {
        let path = entry?.path();
        let order = events(&fs::read_to_string(&path)?).with_context(|| format!("could not parse {}", path.display()))?;
        if !feasible(&order) {
            bail!("{}: events outside the modelled universe", path.display());
        }
        files += 1;
        let mut position = [usize::MAX; EVENTS];
        order.iter().enumerate().for_each(|(i, &e)| position[e] = i);
        for t in 1..=args.t {
            if exact[t] {
                subsequences(&order, t, &mut |key| {
                    seen[t].insert(key);
                });
            } else {
                uncovered[t].retain(|sequence| !in_order(sequence, &position));
            }
        }
    }

    println!("{files} scenarios, k = {EVENTS} events");
    for t in 1..=args.t {
        if exact[t] {
            let covered = seen[t].len() as u128;
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
                    covered[class_of(a)][class_of(b)] += u64::from(seen[2].contains(&((a * EVENTS + b) as u64)));
                }
            }
        }
        println!("\nt=2 coverage by class, first event's class down, second's across:");
        print!("{:>12}", "");
        CLASSES.iter().for_each(|c| print!("{c:>12}"));
        println!();
        for a in 0..n {
            print!("{:>12}", CLASSES[a]);
            for b in 0..n {
                print!("{:>11.1}%", 100.0 * covered[a][b] as f64 / total[a][b].max(1) as f64);
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
        /* 1a notar+final, 2a skipped, 3a fast final citing 1a, 3b off the chain and dead */
        let text = concat!(
            "[{\"node\": \"3a\", \"parent\": \"1a\", \"action\": \"FAST_FINALIZE_CERT\"}, ",
            "{\"node\": \"1a\", \"parent\": \"0\", \"action\": \"NOTARIZE_CERT\"}, ",
            "{\"action\": \"CLOCK\", \"ms\": 250}, ",
            "{\"node\": \"2a\", \"parent\": \"1a\", \"action\": \"SKIP_CERT\"}, ",
            "{\"node\": \"3b\", \"parent\": \"1a\", \"action\": \"REPLAY_DEAD\"}, ",
            "{\"node\": \"3a\", \"parent\": \"1a\", \"action\": \"REPLAY_COMPLETE\"}, ",
            "{\"node\": \"1a\", \"parent\": \"0\", \"action\": \"FINALIZE_CERT\"}, ",
            "{\"node\": \"1a\", \"parent\": \"0\", \"action\": \"REPLAY_COMPLETE\"}]\n"
        );
        assert_eq!(events(text).unwrap(), [
            settle(3, 2),
            settle(2, SKIP),
            off_chain(3, 1, true),
            canonical_replay(3),
            settle(1, 1),
            canonical_replay(1),
        ]);
    }

    #[test]
    fn universe_counts_feasible_tuples() {
        let universe = universe(3);
        assert_eq!(universe[1], (0..EVENTS).filter(|&a| feasible(&[a])).count() as u128);
        let mut pairs = 0;
        let mut triples = 0;
        for a in 0..EVENTS {
            for b in 0..EVENTS {
                pairs += u128::from(feasible(&[a, b]));
                for c in 0..EVENTS {
                    triples += u128::from(feasible(&[a, b, c]));
                }
            }
        }
        assert_eq!(universe[2], pairs);
        assert_eq!(universe[3], triples);
    }

    #[test]
    fn off_chain_nodes_fit_beside_the_canonical_one() {
        let four: Vec<Event> = (0..4).map(|i| off_chain(2, i, false)).collect();
        assert!(feasible(&four));
        assert!(!feasible(&[four.as_slice(), &[off_chain(2, 4, true)]].concat()));
        assert!(!feasible(&[settle(2, SKIP), canonical_replay(2)]));
        assert!(!feasible(&[off_chain(2, 0, false), off_chain(2, 0, true)]));
        assert!(!feasible(&[off_chain(1, 4, false)]));
        assert!(!feasible(&[off_chain(1, 0, false), off_chain(1, 1, false), off_chain(1, 2, false), off_chain(1, 3, false)]));
        assert!(!feasible(&[off_chain(SLOTS, 0, true)]));
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
