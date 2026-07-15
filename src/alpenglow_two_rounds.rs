//! Generator and constraints for `alpenglow_two_rounds.fan`.

use fandango::{
    Fandango,
    generation::Generator,
    typing::{AsNodeRef, Downcast, Node, Opaque, Structured},
    visitor::{
        VisitResult, VisitableChildren, Visitor,
        kpath::{KPathUpdate, KPaths},
        write::WriteVisitor,
    },
};
use rand::{Rng, SeedableRng};
use std::{
    collections::{HashSet, VecDeque},
    convert::Infallible,
    fmt::Write as _,
    fs, io,
    num::NonZeroUsize,
    ops::ControlFlow,
    path::Path,
};

const NUM_SLOTS: usize = 32;
const VALIDATOR_COUNT: usize = 100;
const BLOCKS_PER_SLOT: usize = 4;
const BLOCK_ID_BYTES: usize = 32;
const TOTAL_STAKE: u64 = 10_000_000_000;

/// Grammar source compiled into the static two-round Alpenglow target.
pub const GRAMMAR: &str = include_str!("../grammars/alpenglow_two_rounds.fan");

#[derive(Fandango)]
#[fandango(grammar = "grammars/alpenglow_two_rounds.fan", parse = true)]
pub struct AlpenglowTwoRounds(Infallible);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Vote {
    Skip,
    Finalize,
    Notarize(usize),
    NotarizeFallback(usize),
    SkipFallback,
    Absent,
}

#[derive(Debug, Eq, PartialEq)]
struct Slot {
    block_ids: [[u8; BLOCK_ID_BYTES]; BLOCKS_PER_SLOT],
    round1: Vec<Vote>,
    round2: Vec<Vote>,
}

#[derive(Debug, Eq, PartialEq)]
struct Scenario {
    stakes: Vec<u64>,
    slots: Vec<Slot>,
}

impl Scenario {
    fn generate<R>(rng: &mut R) -> Self
    where
        R: Rng + ?Sized,
    {
        let stakes = generate_stakes(rng);
        let slots = (0..NUM_SLOTS)
            .map(|_| Slot {
                block_ids: generate_block_ids(rng),
                round1: generate_round(rng),
                round2: generate_round(rng),
            })
            .collect();
        Self { stakes, slots }
    }

    fn parse(input: &str) -> Result<Self, String> {
        let mut lines = input.lines();
        expect_line(&mut lines, "committee:")?;

        let mut stakes = Vec::with_capacity(VALIDATOR_COUNT);
        for validator in 1..=VALIDATOR_COUNT {
            let line = lines
                .next()
                .ok_or_else(|| format!("missing committee line for v{validator}"))?;
            let fields = line.split_ascii_whitespace().collect::<Vec<_>>();
            if fields.len() != 2 || fields[0] != format!("v{validator}") {
                return Err(format!("invalid committee line: {line:?}"));
            }
            let stake = fields[1]
                .strip_prefix("stake=")
                .ok_or_else(|| format!("invalid stake in committee line: {line:?}"))?
                .parse::<u64>()
                .map_err(|error| format!("invalid stake in committee line {line:?}: {error}"))?;
            if stake == 0 {
                return Err(format!("v{validator} has zero stake"));
            }
            stakes.push(stake);
        }

        let mut slots = Vec::with_capacity(NUM_SLOTS);
        for logical_slot in 0..NUM_SLOTS {
            expect_line(&mut lines, "blocks:")?;
            let mut block_ids = [[0_u8; BLOCK_ID_BYTES]; BLOCKS_PER_SLOT];
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
            let round1 = parse_round(
                lines
                    .next()
                    .ok_or_else(|| format!("missing round1 in slot {logical_slot}"))?,
                "  round1:",
            )?;
            let round2 = parse_round(
                lines
                    .next()
                    .ok_or_else(|| format!("missing round2 in slot {logical_slot}"))?,
                "  round2:",
            )?;
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

        Ok(Self { stakes, slots })
    }

    fn render(&self) -> String {
        let mut output = String::with_capacity(512 * 1024);
        output.push_str("committee:\n");
        for (index, stake) in self.stakes.iter().enumerate() {
            writeln!(output, "  v{} stake={stake}", index + 1).unwrap();
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
            render_round(&mut output, &slot.round1);
            output.push_str("\n  round2:");
            render_round(&mut output, &slot.round2);
            output.push_str("\nend-slot\n");
        }
        output
    }

    fn validate(&self) -> Vec<String> {
        let mut violations = Vec::new();
        if self.stakes.len() != VALIDATOR_COUNT {
            violations.push(format!(
                "expected {VALIDATOR_COUNT} validators, got {}",
                self.stakes.len()
            ));
            return violations;
        }
        if self.stakes.contains(&0) {
            violations.push("validator stakes must be positive".to_owned());
        }
        match self
            .stakes
            .iter()
            .try_fold(0_u64, |total, stake| total.checked_add(*stake).ok_or(()))
        {
            Ok(TOTAL_STAKE) => {}
            Ok(total) => violations.push(format!(
                "validator stakes sum to {total}, expected {TOTAL_STAKE}"
            )),
            Err(()) => violations.push("validator stake total overflows u64".to_owned()),
        }

        if self.slots.len() != NUM_SLOTS {
            violations.push(format!(
                "expected {NUM_SLOTS} slots, got {}",
                self.slots.len()
            ));
        }
        for (logical_slot, slot) in self.slots.iter().enumerate() {
            validate_round(logical_slot, 1, &slot.round1, &mut violations);
            validate_round(logical_slot, 2, &slot.round2, &mut violations);
        }
        violations
    }
}

/// Root generator used by the static Fandango grammar.
#[derive(Debug, Default)]
pub struct AlpenglowTwoRoundsGenerator;

impl<R, W> Generator<nonterminal_start, W, R> for AlpenglowTwoRoundsGenerator
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
        let tree = AlpenglowTwoRounds::extract(&input)
            .unwrap_or_else(|error| panic!("Rust generator produced invalid grammar: {error}"));
        if let Err(violations) = validate_tree(&tree) {
            panic!(
                "Rust generator violated two-round Alpenglow constraints: {}",
                violations.join("; ")
            );
        }
        Some(tree)
    }
}

/// Constraint visitor for exact committee, stake, slot, block, and round invariants.
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
        let input = match String::from_utf8(writer.output()) {
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

/// Generate and validate one typed two-round Alpenglow derivation tree.
pub fn generate_tree<R>(rng: &mut R) -> nonterminal_start
where
    R: Rng + SeedableRng,
{
    AlpenglowTwoRoundsGenerator
        .generate(rng, &mut (), 0)
        .expect("AlpenglowTwoRoundsGenerator always handles the root node")
}

/// Persistent k-path coverage for generated two-round Alpenglow trees.
pub struct KPathCoverage {
    paths: KPaths,
    covered: HashSet<Vec<usize>>,
}

impl KPathCoverage {
    /// Load coverage from `state`, or create an empty state if the file does not exist.
    pub fn load(k: NonZeroUsize, state: Option<&Path>) -> io::Result<Self> {
        let mut coverage = Self {
            paths: KPaths::new::<TypeMut<'static>>(k, nonterminal_start::ROOT.inner()),
            covered: HashSet::new(),
        };
        let Some(state) = state else {
            return Ok(coverage);
        };
        let input = match fs::read_to_string(state) {
            Ok(input) => input,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(coverage),
            Err(error) => return Err(error),
        };
        let mut lines = input.lines();
        if lines.next() != Some("fandango-kpath-v1") {
            return Err(invalid_coverage_state("invalid state header"));
        }
        let stored_k = lines
            .next()
            .and_then(|line| line.strip_prefix("k="))
            .and_then(|value| value.parse::<usize>().ok())
            .ok_or_else(|| invalid_coverage_state("invalid k value"))?;
        if stored_k != k.get() {
            return Err(invalid_coverage_state(format!(
                "state uses k={stored_k}, requested k={}",
                k.get()
            )));
        }
        let stored_grammar = lines
            .next()
            .and_then(|line| line.strip_prefix("grammar="))
            .and_then(|value| u64::from_str_radix(value, 16).ok())
            .ok_or_else(|| invalid_coverage_state("invalid grammar fingerprint"))?;
        if stored_grammar != grammar_fingerprint() {
            return Err(invalid_coverage_state(
                "state belongs to a different Alpenglow grammar",
            ));
        }

        for line in lines.filter(|line| !line.is_empty()) {
            let path = line
                .split(',')
                .map(|value| {
                    value.parse::<usize>().map_err(|error| {
                        invalid_coverage_state(format!("invalid path {line:?}: {error}"))
                    })
                })
                .collect::<io::Result<Vec<_>>>()?;
            if path.is_empty() || path.len() > k.get() {
                return Err(invalid_coverage_state(format!(
                    "invalid grammar path: {line}"
                )));
            }
            if !coverage.paths.lookup().contains_key(path.as_slice()) {
                return Err(invalid_coverage_state(format!(
                    "unknown grammar path: {line}"
                )));
            }
            coverage.covered.insert(path);
        }
        Ok(coverage)
    }

    /// Record every k-path present in a generated tree.
    pub fn observe(&mut self, tree: &nonterminal_start) {
        let updater = KPathUpdate::inserting(&mut self.paths)
            .visit(tree, 0)
            .unwrap()
            .continue_value()
            .unwrap();
        self.covered.extend(
            updater
                .kpaths()
                .lookup()
                .iter()
                .filter(|(_, count)| **count != 0)
                .map(|(path, _)| path.to_vec()),
        );
    }

    /// Return `(covered, total)` for this grammar and k.
    pub fn totals(&self) -> (usize, usize) {
        (self.covered.len(), self.paths.k_paths().1)
    }

    /// Persist the covered path set.
    pub fn save(&self, state: &Path) -> io::Result<()> {
        let mut paths = self.covered.iter().collect::<Vec<_>>();
        paths.sort();
        let mut output = format!(
            "fandango-kpath-v1\nk={}\ngrammar={:016x}\n",
            self.paths.k().get(),
            grammar_fingerprint()
        );
        for path in paths {
            for (index, discriminant) in path.iter().enumerate() {
                if index != 0 {
                    output.push(',');
                }
                write!(output, "{discriminant}").unwrap();
            }
            output.push('\n');
        }
        let temporary = state.with_extension("tmp");
        fs::write(&temporary, output)?;
        fs::rename(temporary, state)
    }
}

fn invalid_coverage_state(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

fn grammar_fingerprint() -> u64 {
    GRAMMAR.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
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

fn generate_stakes<R>(rng: &mut R) -> Vec<u64>
where
    R: Rng + ?Sized,
{
    let mut cuts = HashSet::with_capacity(VALIDATOR_COUNT - 1);
    while cuts.len() < VALIDATOR_COUNT - 1 {
        cuts.insert(rng.random_range(1..TOTAL_STAKE));
    }
    let mut cuts = cuts.into_iter().collect::<Vec<_>>();
    cuts.sort_unstable();

    let mut stakes = Vec::with_capacity(VALIDATOR_COUNT);
    let mut previous = 0;
    for cut in cuts {
        stakes.push(cut - previous);
        previous = cut;
    }
    stakes.push(TOTAL_STAKE - previous);
    stakes
}

fn generate_block_ids<R>(rng: &mut R) -> [[u8; BLOCK_ID_BYTES]; BLOCKS_PER_SLOT]
where
    R: Rng + ?Sized,
{
    std::array::from_fn(|_| rng.random())
}

fn generate_round<R>(rng: &mut R) -> Vec<Vote>
where
    R: Rng + ?Sized,
{
    (0..VALIDATOR_COUNT).map(|_| generate_vote(rng)).collect()
}

fn generate_vote<R>(rng: &mut R) -> Vote
where
    R: Rng + ?Sized,
{
    match rng.random_range(0..6) {
        0 => Vote::Skip,
        1 => Vote::Finalize,
        2 => Vote::Notarize(rng.random_range(0..BLOCKS_PER_SLOT)),
        3 => Vote::NotarizeFallback(rng.random_range(0..BLOCKS_PER_SLOT)),
        4 => Vote::SkipFallback,
        5 => Vote::Absent,
        _ => unreachable!(),
    }
}

fn render_round(output: &mut String, votes: &[Vote]) {
    for (validator, vote) in votes.iter().enumerate() {
        write!(output, " v{}=", validator + 1).unwrap();
        render_vote(output, *vote);
    }
}

fn render_vote(output: &mut String, vote: Vote) {
    match vote {
        Vote::Skip => output.push_str("skip"),
        Vote::Finalize => output.push_str("finalize"),
        Vote::Notarize(block) => write!(output, "notarize(b{})", block + 1).unwrap(),
        Vote::NotarizeFallback(block) => {
            write!(output, "notarizefallback(b{})", block + 1).unwrap();
        }
        Vote::SkipFallback => output.push_str("skip-fallback"),
        Vote::Absent => output.push_str("absent"),
    }
}

fn parse_round(line: &str, prefix: &str) -> Result<Vec<Vote>, String> {
    let assignments = line
        .strip_prefix(prefix)
        .ok_or_else(|| format!("invalid round line: {line:?}"))?
        .split_ascii_whitespace()
        .collect::<Vec<_>>();
    if assignments.len() != VALIDATOR_COUNT {
        return Err(format!(
            "round has {} assignments, expected {VALIDATOR_COUNT}",
            assignments.len()
        ));
    }

    let mut votes = Vec::with_capacity(VALIDATOR_COUNT);
    for (validator, assignment) in assignments.into_iter().enumerate() {
        let (name, encoded_vote) = assignment
            .split_once('=')
            .ok_or_else(|| format!("invalid round assignment: {assignment:?}"))?;
        if name != format!("v{}", validator + 1) {
            return Err(format!("round validators are out of order at {name:?}"));
        }
        votes.push(parse_vote(encoded_vote)?);
    }
    Ok(votes)
}

fn parse_vote(encoded: &str) -> Result<Vote, String> {
    match encoded {
        "skip" => Ok(Vote::Skip),
        "finalize" => Ok(Vote::Finalize),
        "skip-fallback" => Ok(Vote::SkipFallback),
        "absent" => Ok(Vote::Absent),
        _ => {
            if let Some(block) = encoded
                .strip_prefix("notarize(b")
                .and_then(|value| value.strip_suffix(')'))
            {
                return Ok(Vote::Notarize(parse_block_number(block)?));
            }
            if let Some(block) = encoded
                .strip_prefix("notarizefallback(b")
                .and_then(|value| value.strip_suffix(')'))
            {
                return Ok(Vote::NotarizeFallback(parse_block_number(block)?));
            }
            Err(format!("invalid vote: {encoded:?}"))
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

fn validate_round(logical_slot: usize, round: usize, votes: &[Vote], violations: &mut Vec<String>) {
    if votes.len() != VALIDATOR_COUNT {
        violations.push(format!(
            "slot {logical_slot} round {round} has {} assignments, expected {VALIDATOR_COUNT}",
            votes.len()
        ));
    }
    for vote in votes {
        match vote {
            Vote::Notarize(block) | Vote::NotarizeFallback(block) if *block >= BLOCKS_PER_SLOT => {
                violations.push(format!(
                    "slot {logical_slot} round {round} references block {}",
                    block + 1
                ));
            }
            _ => {}
        }
    }
}

fn decode_block_id(encoded: &str) -> Result<[u8; BLOCK_ID_BYTES], String> {
    if encoded.len() != BLOCK_ID_BYTES * 2 {
        return Err(format!("invalid block ID length: {}", encoded.len()));
    }
    let mut block_id = [0_u8; BLOCK_ID_BYTES];
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

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{SeedableRng, rngs::StdRng};

    #[test]
    fn generated_scenario_has_requested_shape() {
        let scenario = Scenario::generate(&mut StdRng::seed_from_u64(7));

        assert_eq!(scenario.stakes.len(), VALIDATOR_COUNT);
        assert!(scenario.stakes.iter().all(|stake| *stake > 0));
        assert_eq!(scenario.stakes.iter().sum::<u64>(), TOTAL_STAKE);
        assert_eq!(scenario.slots.len(), NUM_SLOTS);
        assert!(scenario.slots.iter().all(|slot| {
            slot.round1.len() == VALIDATOR_COUNT && slot.round2.len() == VALIDATOR_COUNT
        }));
        assert!(scenario.validate().is_empty());
    }

    #[test]
    fn rendered_scenario_round_trips() {
        let scenario = Scenario::generate(&mut StdRng::seed_from_u64(11));
        let parsed = Scenario::parse(&scenario.render()).unwrap();

        assert_eq!(parsed, scenario);
    }

    #[test]
    fn typed_tree_generation_validates() {
        let tree = generate_tree(&mut StdRng::seed_from_u64(13));

        assert!(validate_tree(&tree).is_ok());
    }
}
