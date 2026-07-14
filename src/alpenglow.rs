//! Rust implementation of the generator and constraints for `alpenglow-simpler.fan`.

use fandango::{
    Fandango,
    generation::Generator,
    typing::{AsNodeRef, Downcast, Node, Opaque},
    visitor::{VisitResult, VisitableChildren, Visitor, write::WriteVisitor},
};
use rand::{Rng, SeedableRng, seq::SliceRandom};
use std::{
    collections::{HashSet, VecDeque},
    convert::Infallible,
    fmt::Write as _,
    ops::ControlFlow,
};

const NUM_SLOTS: usize = 32;
const VALIDATOR_COUNT: usize = 100;
const BYZANTINE_VALIDATORS: usize = 19;
const MAX_ABSENT_VALIDATORS: usize = 20;
const BLOCKS_PER_SLOT: usize = 4;
const HONEST_BLOCK_VOTE_PROBABILITY: f64 = 0.90;
const HONEST_FALLBACK_VOTE_PROBABILITY: f64 = 0.50;
const QUORUM_VALIDATORS: usize = 60;
const SAFE_VALIDATORS: usize = 40;
const MIN_NOTARIZE_VALIDATORS: usize = 20;
const LEADER_WINDOW_SLOTS: usize = 4;

/// Grammar source compiled into the static Alpenglow target.
pub const GRAMMAR: &str = include_str!("../grammars/alpenglow-simpler.fan");

#[derive(Fandango)]
#[fandango(grammar = "grammars/alpenglow-simpler.fan", parse = true)]
pub struct Alpenglow(Infallible);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Role {
    Honest,
    Absent,
    Byzantine,
}

impl Role {
    fn as_str(self) -> &'static str {
        match self {
            Self::Honest => "honest",
            Self::Absent => "absent",
            Self::Byzantine => "byzantine",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum Vote {
    Absent,
    Block(usize),
    Skip,
    NotarFallback(usize),
    SkipFallback,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Round2Vote {
    Notarize(usize),
    Finalize,
    NotarFallback(usize),
    SkipFallback,
    None,
}

#[derive(Debug)]
struct Slot {
    block_ids: [[u8; 32]; BLOCKS_PER_SLOT],
    round1: Vec<Vec<Vote>>,
    round2: Vec<Round2Vote>,
}

#[derive(Debug)]
struct Scenario {
    roles: Vec<Role>,
    slots: Vec<Slot>,
}

impl Scenario {
    fn generate<R>(rng: &mut R) -> Self
    where
        R: Rng + ?Sized,
    {
        let roles = generate_roles(rng);
        let mut used_block_ids = HashSet::new();
        let mut slots = Vec::with_capacity(NUM_SLOTS);
        let mut observer_already_skipped = false;

        for logical_slot in 0..NUM_SLOTS {
            if logical_slot.is_multiple_of(LEADER_WINDOW_SLOTS) {
                observer_already_skipped = false;
            }

            let block_ids = generate_block_ids(rng, &mut used_block_ids);
            let mut round1 = roles
                .iter()
                .copied()
                .map(|role| generate_vote_set(rng, role))
                .collect::<Vec<_>>();

            // v1's fallback behavior is the oracle output, not a round-one input.
            round1[0].truncate(1);
            if observer_already_skipped {
                round1[0] = vec![Vote::Skip];
            }

            let (round2, skips_rest_of_window) =
                simulate_round2(&round1, logical_slot, observer_already_skipped);
            slots.push(Slot {
                block_ids,
                round1,
                round2,
            });
            observer_already_skipped |= skips_rest_of_window;
        }

        Self { roles, slots }
    }

    fn parse(input: &str) -> Result<Self, String> {
        let mut lines = input.lines();
        expect_line(&mut lines, "committee:")?;

        let mut roles = Vec::with_capacity(VALIDATOR_COUNT);
        for validator in 1..=VALIDATOR_COUNT {
            let line = lines
                .next()
                .ok_or_else(|| format!("missing committee line for v{validator}"))?;
            let fields = line.split_ascii_whitespace().collect::<Vec<_>>();
            if fields.len() != 3 || fields[0] != format!("v{validator}") || fields[1] != "stake=1%"
            {
                return Err(format!("invalid committee line: {line:?}"));
            }
            let role = match fields[2].strip_prefix("role=") {
                Some("honest") => Role::Honest,
                Some("absent") => Role::Absent,
                Some("byzantine") => Role::Byzantine,
                _ => return Err(format!("invalid role in committee line: {line:?}")),
            };
            roles.push(role);
        }

        let mut slots = Vec::with_capacity(NUM_SLOTS);
        for logical_slot in 0..NUM_SLOTS {
            expect_line(&mut lines, "blocks:")?;
            let mut block_ids = [[0_u8; 32]; BLOCKS_PER_SLOT];
            for (block, block_id) in block_ids.iter_mut().enumerate() {
                let line = lines
                    .next()
                    .ok_or_else(|| format!("missing b{} in slot {logical_slot}", block + 1))?;
                let expected_prefix = format!("  b{}=", block + 1);
                let encoded = line.strip_prefix(&expected_prefix).ok_or_else(|| {
                    format!("invalid block line in slot {logical_slot}: {line:?}")
                })?;
                *block_id = decode_block_id(encoded)?;
            }

            expect_line(&mut lines, "slot")?;
            let round1_line = lines
                .next()
                .ok_or_else(|| format!("missing round1 in slot {logical_slot}"))?;
            let assignments = round1_line
                .strip_prefix("  round1:")
                .ok_or_else(|| format!("invalid round1 line: {round1_line:?}"))?
                .split_ascii_whitespace()
                .collect::<Vec<_>>();
            if assignments.len() != VALIDATOR_COUNT {
                return Err(format!(
                    "slot {logical_slot} has {} round1 assignments",
                    assignments.len()
                ));
            }
            let mut round1 = Vec::with_capacity(VALIDATOR_COUNT);
            for (validator, assignment) in assignments.into_iter().enumerate() {
                let (name, votes) = assignment
                    .split_once('=')
                    .ok_or_else(|| format!("invalid round1 assignment: {assignment:?}"))?;
                if name != format!("v{}", validator + 1) {
                    return Err(format!("round1 validators are out of order at {name:?}"));
                }
                round1.push(parse_vote_set(votes)?);
            }

            let round2_line = lines
                .next()
                .ok_or_else(|| format!("missing round2 in slot {logical_slot}"))?;
            let encoded_round2 = round2_line
                .strip_prefix("  round2: v1=")
                .ok_or_else(|| format!("invalid round2 line: {round2_line:?}"))?;
            let round2 = encoded_round2
                .split(',')
                .map(parse_round2_vote)
                .collect::<Result<Vec<_>, _>>()?;
            if round2.is_empty()
                || round2.len() > 4
                || (round2.len() > 1 && round2.contains(&Round2Vote::None))
            {
                return Err(format!("invalid round2 vote set in slot {logical_slot}"));
            }
            expect_line(&mut lines, "end-slot")?;

            slots.push(Slot {
                block_ids,
                round1,
                round2,
            });
        }

        if let Some(extra) = lines.find(|line| !line.trim().is_empty()) {
            return Err(format!("unexpected trailing line: {extra:?}"));
        }
        Ok(Self { roles, slots })
    }

    fn render(&self) -> String {
        let mut output = String::with_capacity(256 * 1024);
        output.push_str("committee:\n");
        for (index, role) in self.roles.iter().enumerate() {
            writeln!(output, "  v{} stake=1% role={}", index + 1, role.as_str()).unwrap();
        }

        for slot in &self.slots {
            output.push_str("blocks:\n");
            for (index, block_id) in slot.block_ids.iter().enumerate() {
                write!(output, "  b{}=", index + 1).unwrap();
                for byte in block_id {
                    write!(output, "{byte:02x}").unwrap();
                }
                output.push('\n');
            }
            output.push_str("slot\n  round1:");
            for (index, votes) in slot.round1.iter().enumerate() {
                write!(output, " v{}=", index + 1).unwrap();
                render_votes(&mut output, votes);
            }
            output.push_str("\n  round2: v1=");
            render_round2_votes(&mut output, &slot.round2);
            output.push_str("\nend-slot\n");
        }
        output
    }

    fn validate(&self) -> Vec<String> {
        let mut violations = Vec::new();
        if self.roles.len() != VALIDATOR_COUNT {
            violations.push(format!(
                "expected {VALIDATOR_COUNT} validators, got {}",
                self.roles.len()
            ));
            return violations;
        }
        if self.slots.len() != NUM_SLOTS {
            violations.push(format!(
                "expected {NUM_SLOTS} slots, got {}",
                self.slots.len()
            ));
        }
        if self.roles[0] != Role::Honest {
            violations.push("v1 must be honest".to_owned());
        }
        let byzantine = self
            .roles
            .iter()
            .filter(|role| **role == Role::Byzantine)
            .count();
        if byzantine != BYZANTINE_VALIDATORS {
            violations.push(format!(
                "expected {BYZANTINE_VALIDATORS} Byzantine validators, got {byzantine}"
            ));
        }
        let absent = self
            .roles
            .iter()
            .filter(|role| **role == Role::Absent)
            .count();
        if absent > MAX_ABSENT_VALIDATORS {
            violations.push(format!(
                "expected at most {MAX_ABSENT_VALIDATORS} absent validators, got {absent}"
            ));
        }

        let mut block_ids = HashSet::new();
        let mut observer_already_skipped = false;
        for (logical_slot, slot) in self.slots.iter().enumerate() {
            if logical_slot.is_multiple_of(LEADER_WINDOW_SLOTS) {
                observer_already_skipped = false;
            }
            for block_id in slot.block_ids {
                if !block_ids.insert(block_id) {
                    violations.push(format!("duplicate block ID in slot {logical_slot}"));
                }
            }
            if slot.round1.len() != VALIDATOR_COUNT {
                violations.push(format!(
                    "slot {logical_slot} has {} round1 assignments",
                    slot.round1.len()
                ));
                continue;
            }
            for (validator, (votes, role)) in slot.round1.iter().zip(&self.roles).enumerate() {
                if infer_role(votes) != *role {
                    violations.push(format!(
                        "v{}'s votes do not match its role in slot {logical_slot}",
                        validator + 1
                    ));
                }
                if votes
                    .first()
                    .is_none_or(|vote| !matches!(vote, Vote::Absent | Vote::Block(_) | Vote::Skip))
                {
                    violations.push(format!(
                        "v{} has no primary vote in slot {logical_slot}",
                        validator + 1
                    ));
                }
            }
            if !matches!(slot.round1[0].as_slice(), [Vote::Block(_) | Vote::Skip]) {
                violations.push(format!(
                    "v1 must have exactly one primary vote in slot {logical_slot}"
                ));
                continue;
            }
            if observer_already_skipped && slot.round1[0] != [Vote::Skip] {
                violations.push(format!(
                    "v1 must skip the remainder of its leader window in slot {logical_slot}"
                ));
            }
            let (expected_round2, skips_rest_of_window) =
                simulate_round2(&slot.round1, logical_slot, observer_already_skipped);
            if slot.round2 != expected_round2 {
                violations.push(format!(
                    "wrong round2 votes in slot {logical_slot}: expected {expected_round2:?}, got {:?}",
                    slot.round2
                ));
            }
            observer_already_skipped |= skips_rest_of_window;
        }
        violations
    }
}

/// Root generator used by the static Fandango grammar.
#[derive(Debug, Default)]
pub struct AlpenglowGenerator;

impl<R, W> Generator<nonterminal_start, W, R> for AlpenglowGenerator
where
    R: Rng + SeedableRng,
{
    fn generate(
        &mut self,
        sampler: &mut R,
        _remaining: &mut W,
        _depth: usize,
    ) -> Option<nonterminal_start> {
        let input = Scenario::generate(sampler).render();
        let tree = Alpenglow::extract(&input)
            .unwrap_or_else(|error| panic!("Rust generator produced invalid grammar: {error}"));
        if let Err(violations) = validate_tree(&tree) {
            panic!(
                "Rust generator violated Alpenglow constraints: {}",
                violations.join("; ")
            );
        }
        Some(tree)
    }
}

/// Constraint visitor corresponding to the former Python `where` clauses.
#[derive(Debug, Default)]
pub struct ConstraintVisitor {
    checked: bool,
    violations: Vec<String>,
    paths: Vec<VecDeque<usize>>,
}

impl ConstraintVisitor {
    /// Constraint violations discovered while visiting the root.
    pub fn violations(&self) -> &[String] {
        &self.violations
    }

    /// Derivation-tree paths associated with the violations.
    pub fn paths(&self) -> &[VecDeque<usize>] {
        &self.paths
    }

    fn record(&mut self, index: usize, violation: String) {
        self.violations.push(violation);
        self.paths.push(VecDeque::from([index]));
    }
}

impl<T> Visitor<T> for ConstraintVisitor
where
    T: VisitableChildren<T> + AsNodeRef<nonterminal_start>,
{
    type Continue = Self;
    type Break = Infallible;
    type Error = Infallible;

    fn visit<'program, N>(mut self, node: &'program N, index: usize) -> VisitResult<Self, T>
    where
        N: Node<Type<'program> = T>,
        T: From<&'program N> + AsNodeRef<N>,
    {
        let visited = node.opaque();
        let Some(tree) = visited.downcast::<nonterminal_start>() else {
            return Ok(ControlFlow::Continue(self));
        };
        self.checked = true;

        let writer = WriteVisitor::new(Vec::new())
            .visit(tree, index)
            .unwrap()
            .continue_value()
            .unwrap();
        let bytes = writer.output();
        let input = match String::from_utf8(bytes) {
            Ok(input) => input,
            Err(error) => {
                self.record(index, format!("generated tree is not UTF-8: {error}"));
                return Ok(ControlFlow::Continue(self));
            }
        };
        match Scenario::parse(&input) {
            Ok(scenario) => {
                for violation in scenario.validate() {
                    self.record(index, violation);
                }
            }
            Err(error) => self.record(index, error),
        }
        Ok(ControlFlow::Continue(self))
    }
}

/// Generate and validate one typed Alpenglow derivation tree.
pub fn generate_tree<R>(rng: &mut R) -> nonterminal_start
where
    R: Rng + SeedableRng,
{
    AlpenglowGenerator
        .generate(rng, &mut (), 0)
        .expect("AlpenglowGenerator always handles the root node")
}

fn validate_tree(tree: &nonterminal_start) -> Result<(), Vec<String>> {
    let visitor = ConstraintVisitor::default()
        .visit(tree, 0)
        .unwrap()
        .continue_value()
        .unwrap();
    debug_assert!(visitor.checked);
    if visitor.violations().is_empty() {
        Ok(())
    } else {
        Err(visitor.violations)
    }
}

fn generate_roles<R>(rng: &mut R) -> Vec<Role>
where
    R: Rng + ?Sized,
{
    let mut roles = vec![Role::Honest; VALIDATOR_COUNT];
    let mut eligible = (1..VALIDATOR_COUNT).collect::<Vec<_>>();
    eligible.shuffle(rng);
    let absent = rng.random_range(0..=MAX_ABSENT_VALIDATORS);
    for validator in eligible.iter().take(BYZANTINE_VALIDATORS) {
        roles[*validator] = Role::Byzantine;
    }
    for validator in eligible.iter().skip(BYZANTINE_VALIDATORS).take(absent) {
        roles[*validator] = Role::Absent;
    }
    roles
}

fn generate_block_ids<R>(rng: &mut R, used: &mut HashSet<[u8; 32]>) -> [[u8; 32]; BLOCKS_PER_SLOT]
where
    R: Rng + ?Sized,
{
    std::array::from_fn(|_| {
        loop {
            let block_id = rng.random();
            if used.insert(block_id) {
                break block_id;
            }
        }
    })
}

fn generate_vote_set<R>(rng: &mut R, role: Role) -> Vec<Vote>
where
    R: Rng + ?Sized,
{
    match role {
        Role::Absent => vec![Vote::Absent],
        Role::Honest => {
            let primary = if rng.random::<f64>() < HONEST_BLOCK_VOTE_PROBABILITY {
                Vote::Block(rng.random_range(0..BLOCKS_PER_SLOT))
            } else {
                Vote::Skip
            };
            if rng.random::<f64>() >= HONEST_FALLBACK_VOTE_PROBABILITY {
                return vec![primary];
            }
            let fallback = match primary {
                Vote::Skip => Vote::NotarFallback(rng.random_range(0..BLOCKS_PER_SLOT)),
                Vote::Block(_) => Vote::SkipFallback,
                Vote::Absent | Vote::NotarFallback(_) | Vote::SkipFallback => unreachable!(),
            };
            vec![primary, fallback]
        }
        Role::Byzantine => {
            let mut votes = [
                Vote::Block(0),
                Vote::Block(1),
                Vote::Block(2),
                Vote::Block(3),
                Vote::Skip,
            ];
            votes.shuffle(rng);
            votes[..rng.random_range(2..=votes.len())].to_vec()
        }
    }
}

fn simulate_round2(
    round1: &[Vec<Vote>],
    logical_slot: usize,
    observer_already_skipped: bool,
) -> (Vec<Round2Vote>, bool) {
    let mut block_counts = [0_usize; BLOCKS_PER_SLOT];
    let mut skip_count = usize::from(observer_already_skipped);
    let mut observer_vote = observer_already_skipped.then_some(Vote::Skip);
    let mut bad_window = observer_already_skipped;
    let mut its_over = false;
    let mut skips_rest_of_window = observer_already_skipped;
    let mut safe_to_notar_sent = [false; BLOCKS_PER_SLOT];
    let mut safe_to_skip_sent = false;
    let mut pending_safe_to_notar = Vec::new();
    let mut emitted = Vec::new();
    let first_in_leader_window = logical_slot.is_multiple_of(LEADER_WINDOW_SLOTS);

    for (validator, votes) in round1.iter().enumerate() {
        if observer_already_skipped && validator == 0 {
            continue;
        }
        let accepted_vote = votes[0];
        if accepted_vote == Vote::Absent {
            continue;
        }
        if validator == 0 {
            observer_vote = Some(accepted_vote);
            match accepted_vote {
                Vote::Block(block) => emitted.push(Round2Vote::Notarize(block)),
                Vote::Skip => {
                    bad_window = true;
                    skips_rest_of_window = true;
                }
                Vote::Absent | Vote::NotarFallback(_) | Vote::SkipFallback => unreachable!(),
            }
        }
        match accepted_vote {
            Vote::Block(block) => block_counts[block] += 1,
            Vote::Skip => skip_count += 1,
            Vote::Absent | Vote::NotarFallback(_) | Vote::SkipFallback => unreachable!(),
        }

        let Some(observer_vote) = observer_vote else {
            continue;
        };
        for block in 0..BLOCKS_PER_SLOT {
            let safe_to_notar = !safe_to_notar_sent[block]
                && observer_vote != Vote::Block(block)
                && (block_counts[block] >= SAFE_VALIDATORS
                    || (block_counts[block] >= MIN_NOTARIZE_VALIDATORS
                        && skip_count + block_counts[block] >= QUORUM_VALIDATORS));
            if !safe_to_notar {
                continue;
            }
            safe_to_notar_sent[block] = true;
            skips_rest_of_window = true;
            if first_in_leader_window {
                if !its_over {
                    emitted.push(Round2Vote::NotarFallback(block));
                    bad_window = true;
                }
            } else {
                pending_safe_to_notar.push(block);
            }
        }

        let total_notarized = block_counts.iter().sum::<usize>();
        let top_notarized = block_counts.iter().copied().max().unwrap_or_default();
        let safe_to_skip = !safe_to_skip_sent
            && matches!(observer_vote, Vote::Block(_))
            && skip_count + total_notarized - top_notarized >= SAFE_VALIDATORS;
        if safe_to_skip {
            safe_to_skip_sent = true;
            skips_rest_of_window = true;
            if !its_over {
                emitted.push(Round2Vote::SkipFallback);
                bad_window = true;
            }
        }

        if let Vote::Block(block) = accepted_vote
            && block_counts[block] == QUORUM_VALIDATORS
            && observer_vote == Vote::Block(block)
            && !bad_window
            && !its_over
        {
            emitted.push(Round2Vote::Finalize);
            its_over = true;
        }
    }

    for block in pending_safe_to_notar {
        skips_rest_of_window = true;
        if !its_over {
            emitted.push(Round2Vote::NotarFallback(block));
        }
    }
    if emitted.is_empty() {
        emitted.push(Round2Vote::None);
    }
    (emitted, skips_rest_of_window)
}

fn infer_role(votes: &[Vote]) -> Role {
    match votes {
        [Vote::Absent] => Role::Absent,
        [Vote::Block(_) | Vote::Skip]
        | [Vote::Skip, Vote::NotarFallback(_)]
        | [Vote::Block(_), Vote::SkipFallback] => Role::Honest,
        _ => Role::Byzantine,
    }
}

fn render_votes(output: &mut String, votes: &[Vote]) {
    for (index, vote) in votes.iter().enumerate() {
        if index != 0 {
            output.push(',');
        }
        match vote {
            Vote::Absent => output.push_str("absent"),
            Vote::Block(block) => write!(output, "b{}", block + 1).unwrap(),
            Vote::Skip => output.push_str("skip"),
            Vote::NotarFallback(block) => {
                write!(output, "notar-fallback(b{})", block + 1).unwrap();
            }
            Vote::SkipFallback => output.push_str("skip-fallback"),
        }
    }
}

fn render_round2_votes(output: &mut String, votes: &[Round2Vote]) {
    for (index, vote) in votes.iter().enumerate() {
        if index != 0 {
            output.push(',');
        }
        match vote {
            Round2Vote::Notarize(block) => {
                write!(output, "notarize(b{})", block + 1).unwrap();
            }
            Round2Vote::Finalize => output.push_str("finalize"),
            Round2Vote::NotarFallback(block) => {
                write!(output, "notar-fallback(b{})", block + 1).unwrap();
            }
            Round2Vote::SkipFallback => output.push_str("skip-fallback"),
            Round2Vote::None => output.push_str("none"),
        }
    }
}

fn parse_vote_set(encoded: &str) -> Result<Vec<Vote>, String> {
    let votes = encoded
        .split(',')
        .map(parse_vote)
        .collect::<Result<Vec<_>, _>>()?;
    if votes.is_empty() || votes.len() > 5 || (votes.contains(&Vote::Absent) && votes.len() != 1) {
        return Err(format!("invalid vote set: {encoded:?}"));
    }
    Ok(votes)
}

fn parse_vote(encoded: &str) -> Result<Vote, String> {
    match encoded {
        "absent" => Ok(Vote::Absent),
        "skip" => Ok(Vote::Skip),
        "skip-fallback" => Ok(Vote::SkipFallback),
        _ => {
            if let Some(block) = encoded
                .strip_prefix("notar-fallback(b")
                .and_then(|value| value.strip_suffix(')'))
            {
                return Ok(Vote::NotarFallback(parse_block_number(block)?));
            }
            let block = encoded
                .strip_prefix('b')
                .ok_or_else(|| format!("invalid vote: {encoded:?}"))?;
            Ok(Vote::Block(parse_block_number(block)?))
        }
    }
}

fn parse_round2_vote(encoded: &str) -> Result<Round2Vote, String> {
    match encoded {
        "finalize" => Ok(Round2Vote::Finalize),
        "skip-fallback" => Ok(Round2Vote::SkipFallback),
        "none" => Ok(Round2Vote::None),
        _ => {
            if let Some(block) = encoded
                .strip_prefix("notarize(b")
                .and_then(|value| value.strip_suffix(')'))
            {
                return Ok(Round2Vote::Notarize(parse_block_number(block)?));
            }
            if let Some(block) = encoded
                .strip_prefix("notar-fallback(b")
                .and_then(|value| value.strip_suffix(')'))
            {
                return Ok(Round2Vote::NotarFallback(parse_block_number(block)?));
            }
            Err(format!("invalid round2 vote: {encoded:?}"))
        }
    }
}

fn parse_block_number(encoded: &str) -> Result<usize, String> {
    let number = encoded
        .parse::<usize>()
        .map_err(|error| format!("invalid block number {encoded:?}: {error}"))?;
    if !(1..=BLOCKS_PER_SLOT).contains(&number) {
        return Err(format!("block number is out of range: {number}"));
    }
    Ok(number - 1)
}

fn decode_block_id(encoded: &str) -> Result<[u8; 32], String> {
    if encoded.len() != 64 {
        return Err(format!("invalid block ID length: {}", encoded.len()));
    }
    let mut block_id = [0_u8; 32];
    for (index, byte) in block_id.iter_mut().enumerate() {
        let offset = index * 2;
        *byte = (hex_nibble(encoded.as_bytes()[offset])? << 4)
            | hex_nibble(encoded.as_bytes()[offset + 1])?;
    }
    Ok(block_id)
}

fn hex_nibble(byte: u8) -> Result<u8, String> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        _ => Err(format!("invalid hexadecimal digit: {byte:?}")),
    }
}

fn expect_line<'a>(
    lines: &mut impl Iterator<Item = &'a str>,
    expected: &str,
) -> Result<(), String> {
    let actual = lines
        .next()
        .ok_or_else(|| format!("expected {expected:?}, reached end of input"))?;
    if actual == expected {
        Ok(())
    } else {
        Err(format!("expected {expected:?}, got {actual:?}"))
    }
}
