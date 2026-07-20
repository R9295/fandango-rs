//! Generate Yul programs from `grammars/yul.fan`, report grammar k-path coverage, and
//! run each program through a two-stage oracle: compile with `solc`, then (if it
//! compiles) execute the resulting bytecode with `evm`.
//!
//! k-path coverage measures how many distinct length-`k` paths through the grammar's
//! node graph have been exercised by the generated inputs. The *universe* of paths is
//! derived from the grammar itself (via [`KPaths::new`]); each generated tree marks the
//! paths it walks as covered.
//!
//! The oracle mirrors the `gcc` validity check used for the C-language example:
//!   1. `solc --strict-assembly --bin -` compiles the generated Yul. Its exit status is
//!      ground truth for "is this valid Yul?"; on success it emits EVM bytecode.
//!   2. `evm run <bytecode>` executes that bytecode and we classify the result as
//!      succeeded (STOP/RETURN), reverted (REVERT), or errored (invalid opcode, stack
//!      underflow, out of gas, ...).
//! The oracle runs on each *distinct* program once — the generator emits many duplicate
//! inputs (mostly the empty block `{ }`), and re-checking them would bias the statistics.
//! If `solc` is missing, only k-path coverage is reported; if `evm` is missing, only the
//! solc validity stage runs.
//!
//! Usage:
//! ```text
//! cargo run --example yul_kpath --features yul -- [K] [SAMPLES] [SEED]
//! ```
//! Defaults: `K = 2`, `SAMPLES = 2000`, `SEED` = OS entropy.

use anyhow::{Context, Error, anyhow};
use fandango::lang::FandangoNode;
use fandango::tuple_list::tuple_list;
use fandango::typing::{AsNode, Node, NodeLookup, Structured};
use fandango::visitor::{VisitableChildren, Visitor};
use fandango::visitor::navigation::CountNodes;
use fandango::visitor::write::WriteVisitor;
use fandango_core::visitor::altpath::{AltPathUpdate, AltPathVisit, AltPathVisitor, AltPaths};
use mappable_rc::Mrc;
use fandango_core::visitor::kpath::{KPathUpdate, KPaths};
use fandango_runtime::evolvers::Evolver;
use fandango_runtime::evolvers::multi::{AltPathDiversityHook, Nsga2Evolver};
use fandango_runtime::measurement::FitnessMeasurer;
use fandango_runtime::operators::{Checker, DepthLimiter};
use fandango_runtime::population::Individual;
use fandango_targets::yul::{self, TypeMut, YulConstraintVisitor, YulFixHook, nonterminal_start};
use num_rational::Ratio;
use rand::SeedableRng;
use rand::rngs::StdRng;
use std::cmp::Reverse;
use std::collections::HashSet;
use std::convert::Infallible;
use std::num::NonZeroUsize;
use std::ops::ControlFlow;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Fitness driving a tree's node count toward `n` (closer = better), so the population
/// evolves away from trivial inputs toward substantial programs.
struct NodeGoal {
    n: usize,
}
impl<'a, N> FitnessMeasurer<'a, N> for NodeGoal
where
    N: Node,
{
    type Measurement = Reverse<usize>;
    type Error = Infallible;
    fn evaluate(&mut self, node: &'a N) -> Result<Self::Measurement, Self::Error> {
        Ok(Reverse(self.n.abs_diff(node.count_nodes())))
    }
}

/// Counts alt-paths that no individual has covered yet.
struct CountNovel {
    novel: usize,
}

impl AltPathVisitor for CountNovel {
    type Value = usize;
    type Break = Infallible;
    type Error = Infallible;

    fn visit_path(
        &mut self,
        count: usize,
        _path: &Mrc<[usize]>,
    ) -> Result<ControlFlow<Self::Break>, Self::Error> {
        if count == 0 {
            self.novel += 1;
        }
        Ok(ControlFlow::Continue(()))
    }

    fn value(self) -> Self::Value {
        self.novel
    }
}

/// Fitness rewarding individuals that reach alt-paths the population has not covered yet.
///
/// Deliberately **stateful**: it accumulates one [`AltPaths`] table across evaluations, so
/// an individual is scored by how many *novel* alt-paths it contributes rather than by its
/// absolute coverage. Absolute coverage would mostly track program size (already an
/// objective); novelty is what actually pushes back against the population converging onto
/// clones. Higher is better, so the measurement is a plain count, not a `Reverse`.
struct AltPathNovelty {
    k: NonZeroUsize,
    seen: Option<AltPaths>,
}

impl AltPathNovelty {
    fn new(k: NonZeroUsize) -> Self {
        Self { k, seen: None }
    }
}

impl<'a, N> FitnessMeasurer<'a, N> for AltPathNovelty
where
    N: Node + AsNode + 'a,
    for<'b> N::Type<'b>: NodeLookup + VisitableChildren<N::Type<'b>>,
{
    type Measurement = usize;
    type Error = Error;

    fn evaluate(&mut self, node: &'a N) -> Result<Self::Measurement, Self::Error> {
        let k = self.k;
        let table = match self.seen.as_mut() {
            Some(t) => t,
            None => {
                let FandangoNode::Program(program) = node.root() else {
                    return Err(anyhow!("root node was not a program node"));
                };
                self.seen
                    .insert(AltPaths::new::<<N as Node>::Type<'_>>(k, program))
            }
        };

        // Score against the table as it stands, then fold this individual in so the next
        // evaluation sees these paths as already-covered.
        let novel = AltPathVisit::new(table, CountNovel { novel: 0 })
            .visit(node, 0)
            .expect("alt-path visit never errors")
            .continue_value()
            .expect("alt-path visit never breaks")
            .value();
        let _ = AltPathUpdate::inserting(table)
            .visit(node, 0)
            .expect("alt-path update never errors")
            .continue_value()
            .expect("alt-path update never breaks");
        Ok(novel)
    }
}

const USAGE: &str = "usage: yul_kpath [K] [SAMPLES] [SEED]\n  \
    K        k-path length, >= 1        [default 2]\n  \
    SAMPLES  number of programs         [default 2000]\n  \
    SEED     u64 RNG seed               [default: OS entropy]\n\
    (with cargo: `cargo run --example yul_kpath --features yul -- 2 2000 42`)";

/// Parse a positional argument, falling back to `default` when absent and
/// reporting usage on a malformed value.
fn parse_or<T>(arg: Option<&String>, default: T, name: &str) -> Result<T, Error>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    match arg {
        None => Ok(default),
        Some(s) => s
            .parse::<T>()
            .map_err(|e| anyhow!("could not parse {name} from {s:?}: {e}\n{USAGE}")),
    }
}

/// Recompute both NSGA-II objectives for one individual: `(node count, scope violations)`.
/// Recomputed from the tree rather than decoded out of the combined measurement, so the
/// plot shows the raw objective values.
fn objectives(program: &nonterminal_start) -> (usize, usize) {
    let nodes = program.count_nodes();
    let violations = YulConstraintVisitor::default()
        .visit(program, 0)
        .expect("scope checker never errors")
        .continue_value()
        .expect("scope checker never breaks")
        .violations()
        .violations()
        .len();
    (nodes, violations)
}

/// Render a generated Yul tree to its source text.
fn to_source(program: &nonterminal_start) -> Result<String, Error> {
    let bytes = WriteVisitor::new(Vec::new())
        .visit(program, 0)?
        .continue_value()
        .expect("write visitor never breaks")
        .output();
    Ok(String::from_utf8(bytes)?)
}

/// Is `solc` available on `PATH`?
fn solc_available() -> bool {
    Command::new("solc")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Is the `evm` interpreter available on `PATH`? Probed by running `STOP` (0x00), which
/// always succeeds — `evm` has no `--version` subcommand in this build.
fn evm_available() -> bool {
    Command::new("evm")
        .args(["run", "00"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Compile Yul with `solc --strict-assembly --bin`. On success returns the compiled EVM
/// bytecode as a hex string; on failure returns the first solc diagnostic line.
///
/// Errors writing to the child's stdin (e.g. a broken pipe when `solc` bails out early on
/// a parse error) are ignored — the exit status is the authoritative verdict.
fn solc_compile(source: &str) -> Result<String, String> {
    // Write to a temp file rather than piping stdin: the `solc` on PATH may be a Python
    // shim (solc-select) that deadlocks on piped stdin.
    let path = std::env::temp_dir().join("yul_kpath_oracle.yul");
    if let Err(e) = std::fs::write(&path, source) {
        return Err(format!("failed to write temp file: {e}"));
    }
    let output = Command::new("solc")
        .arg("--strict-assembly")
        .arg("--bin")
        .arg(&path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output();
    let output = match output {
        Ok(o) => o,
        Err(e) => return Err(format!("failed to run solc: {e}")),
    };
    if !output.status.success() {
        let diagnostic = String::from_utf8_lossy(&output.stderr)
            .lines()
            .find(|l| l.contains("Error:"))
            .unwrap_or("solc rejected the program")
            .trim()
            .to_string();
        return Err(diagnostic);
    }
    // The bytecode hex is printed on the line directly after the "Binary representation:"
    // header.
    let stdout = String::from_utf8_lossy(&output.stdout);
    let bytecode = stdout
        .lines()
        .skip_while(|l| !l.contains("Binary representation:"))
        .nth(1)
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .unwrap_or("")
        .to_string();
    Ok(bytecode)
}

/// Result of executing bytecode in the `evm` interpreter.
enum EvmOutcome {
    /// Ran to a normal halt (STOP / RETURN).
    Success,
    /// Hit a REVERT.
    Reverted,
    /// Any other execution error (invalid opcode, stack underflow, out of gas, ...).
    Errored(String),
}

/// Execute EVM bytecode with `evm run <hex>`. The tool exits 0 even on execution errors,
/// so the outcome is read from its diagnostic output rather than the exit status.
fn evm_run(bytecode_hex: &str) -> EvmOutcome {
    let output = Command::new("evm")
        .args(["run", bytecode_hex])
        .stdin(Stdio::null())
        .output();
    let output = match output {
        Ok(o) => o,
        Err(e) => return EvmOutcome::Errored(format!("failed to spawn evm: {e}")),
    };
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    match text.find("error:") {
        None => EvmOutcome::Success,
        Some(idx) => {
            let msg = text[idx..].lines().next().unwrap_or("error:").trim().to_string();
            if msg.to_ascii_lowercase().contains("revert") {
                EvmOutcome::Reverted
            } else {
                EvmOutcome::Errored(msg)
            }
        }
    }
}

fn main() -> Result<(), Error> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let k: usize = parse_or(args.first(), 2, "K")?;
    let samples: usize = parse_or(args.get(1), 2000, "SAMPLES")?;
    let k = NonZeroUsize::new(k).with_context(|| format!("K must be >= 1\n{USAGE}"))?;

    let mut sampler = match args.get(2) {
        Some(s) => StdRng::seed_from_u64(parse_or(Some(s), 0u64, "SEED")?),
        None => StdRng::from_os_rng(),
    };

    // With HARNESS_DEBUG=1, echo every generated program and its compiled bytecode.
    let debug = std::env::var("HARNESS_DEBUG").ok().as_deref() == Some("1");

    let solc_on = solc_available();
    // The evm execution stage needs bytecode from solc, so it is only meaningful when
    // solc is present too.
    let evm_on = solc_on && evm_available();
    match (solc_on, evm_on) {
        (false, _) => println!("oracle: DISABLED (`solc` not found on PATH)"),
        (true, false) => {
            println!("oracle: solc --strict-assembly  (evm stage DISABLED: `evm` not found)");
        }
        (true, true) => println!("oracle: solc --strict-assembly --bin  ->  evm run"),
    }

    // The generator walks the grammar structure, bounded so recursive productions
    // terminate. Depth is kept modest so the recursive visitors don't overflow the
    // stack on deeply nested trees.
    let generator = DepthLimiter::new(yul::STRUCTURE.inner(), 30);
    let mut generators = tuple_list!(generator);

    // Validity is GUARANTEED, not competed for: YulFixHook repairs scope on every
    // individual the evolver creates, so it never has to be an objective. That frees the
    // objective space for the two things that actually trade off — SIZE and alt-path
    // NOVELTY — and keeps Pareto sorting a strong signal.
    //
    // Both earlier configurations failed at one end of this dial: validity + size (2 obj)
    // gave 100% valid but only 6 distinct programs; adding novelty as a third objective
    // gave 72 distinct but ~10% valid, because three objectives leave nearly everything
    // non-dominated and dominance stops filtering out invalid programs.
    const TARGET_NODES: usize = 300;
    const POP: usize = 100;
    const REPLICATION: usize = 120;
    const WARMUP: usize = 12;

    let mut runtime = Nsga2Evolver::new::<nonterminal_start>(
        // Two objectives that genuinely compete: SIZE (reach the node target) and
        // alt-path NOVELTY (reach grammar structure nobody has reached yet). Bigger
        // programs are not automatically more novel, so the front stays a real curve.
        tuple_list!(NodeGoal { n: TARGET_NODES }, AltPathNovelty::new(k)),
        // The fix hook runs inside the diversity hook, so every individual is
        // scope-repaired the moment it is created — mutation and crossover included.
        AltPathDiversityHook::new(YulFixHook, k),
        POP,
        REPLICATION,
        Ratio::new(80, 100),
    )
    .expect("crossover rate must be <= 1");
    println!(
        "generation: NSGA-II · objectives = scope-validity + size({TARGET_NODES} nodes) \
         · pop {POP} · {WARMUP} warm-up gens"
    );

    // Build the k-path universe directly from the grammar graph.
    let mut kpaths = KPaths::new::<TypeMut<'static>>(k, nonterminal_start::ROOT.inner());
    let (_, total) = kpaths.k_paths();
    println!("grammar = grammars/yul.fan   k = {k}   total {k}-paths in grammar = {total}");

    // Warm up: evolve toward the target size before harvesting samples.
    // Log every individual's objective pair each generation, so the run can be plotted in
    // objective space afterwards: (generation, nodes, violations).
    let mut evo_log: Vec<(usize, usize, usize)> = Vec::new();
    let mut population = runtime.initial(&mut generators, &mut sampler)?;
    for ind in &population {
        let (nodes, violations) = objectives(ind.node());
        evo_log.push((0, nodes, violations));
    }
    for generation in 1..=WARMUP {
        population = runtime.step(&mut generators, &mut sampler, population)?;
        for ind in &population {
            let (nodes, violations) = objectives(ind.node());
            evo_log.push((generation, nodes, violations));
        }
    }
    let mut generation = WARMUP;

    // k-alt-path: paths between alternation branch-edges, where k counts alternations
    // traversed rather than nodes. Covers (k+1)-path with far fewer subdomains.
    let mut altpaths = AltPaths::new::<TypeMut<'static>>(k, nonterminal_start::ROOT.inner());
    let (_, alt_total) = altpaths.alt_paths();
    // The honest comparison is against (k+1)-path: that is what k-alt-path covers, so the
    // ratio shows how much less we must store for the same covering power.
    let kplus1 = NonZeroUsize::new(k.get() + 1).expect("k + 1 is non-zero");
    let (_, kplus1_total) =
        KPaths::new::<TypeMut<'static>>(kplus1, nonterminal_start::ROOT.inner()).k_paths();
    println!(
        "                                  total {k}-alt-paths = {alt_total} · \
         {kplus1}-paths = {kplus1_total} (k-alt-path covers {kplus1}-path)"
    );
    // k-alt-path provably covers (k+1)-path, but empirically covers much longer paths
    // (the paper's j ~ 3k+1), which is where the 5-10x storage reduction comes from.
    // Print the path_j curve so the real ratio is visible rather than assumed.
    {
        let mut line = alloc_string_for_curve();
        for j in 1..=6usize {
            let jj = NonZeroUsize::new(j).expect("j is non-zero");
            let (_, jt) =
                KPaths::new::<TypeMut<'static>>(jj, nonterminal_start::ROOT.inner()).k_paths();
            line.push_str(&format!(
                "  {j}-path={jt} ({:.1}x)",
                jt as f64 / alt_total.max(1) as f64
            ));
        }
        println!("                                  vs{line}");
    }

    let mut alt_updater = AltPathUpdate::inserting(&mut altpaths);
    let mut updater = KPathUpdate::inserting(&mut kpaths);
    let mut total_len = 0usize;
    let report_every = 1;

    // Oracle tallies. The oracle runs on distinct programs only (deduplicated below).
    let mut seen: HashSet<String> = HashSet::new();
    let mut solc_checked = 0usize;
    let mut duplicates = 0usize;
    let mut solc_valid = 0usize;
    let mut solc_time = Duration::ZERO;
    let mut evm_success = 0usize;
    let mut evm_reverted = 0usize;
    let mut evm_errored = 0usize;
    let mut evm_time = Duration::ZERO;
    let mut first_valid: Option<(String, String, String)> = None; // (yul, bytecode, evm outcome)
    let mut first_invalid: Option<(String, String)> = None; // (yul, solc diagnostic)

    // Harvest programs from the evolving population, stepping to produce more as needed.
    // Individuals are already scope-repaired by the fix hook, so no fixer call here.
    let mut i = 0usize;
    'harvest: loop {
      for ind in &population {
        i += 1;
        let program = ind.node();
        let source = to_source(program)?;
        if !debug && i <= 2 {
            println!("--- sample #{i} ({} chars) ---\n{source}", source.len());
        }
        total_len += source.len();

        // Stage 1/2: solc + evm oracle. Run once per DISTINCT program — the generator
        // emits many duplicates (mostly `{ }`) that would otherwise bias the tallies.
        if solc_on {
            if seen.insert(source.clone()) {
                solc_checked += 1;
                let start = Instant::now();
                let compiled = solc_compile(&source);
                solc_time += start.elapsed();
                match compiled {
                    Ok(bytecode) => {
                        solc_valid += 1;
                        let mut outcome = String::from("not run");
                        if evm_on && !bytecode.is_empty() {
                            let estart = Instant::now();
                            outcome = match evm_run(&bytecode) {
                                EvmOutcome::Success => {
                                    evm_success += 1;
                                    "success".to_string()
                                }
                                EvmOutcome::Reverted => {
                                    evm_reverted += 1;
                                    "reverted".to_string()
                                }
                                EvmOutcome::Errored(msg) => {
                                    evm_errored += 1;
                                    format!("error ({msg})")
                                }
                            };
                            evm_time += estart.elapsed();
                        }
                        if debug {
                            println!("[debug] #{i:>5}  yul: {source}");
                            println!("[debug]         bytecode: 0x{bytecode}   evm: {outcome}");
                        }
                        if first_valid.is_none() {
                            first_valid = Some((source.clone(), bytecode, outcome));
                        }
                    }
                    Err(diagnostic) => {
                        if debug {
                            println!("[debug] #{i:>5}  yul: {source}");
                            println!("[debug]         solc rejected: {diagnostic}");
                        }
                        if first_invalid.is_none() {
                            first_invalid = Some((source.clone(), diagnostic));
                        }
                    }
                }
            } else {
                duplicates += 1;
                if debug {
                    println!("[debug] #{i:>5}  yul: {source}   (duplicate — oracle skipped)");
                }
            }
        } else if debug {
            println!("[debug] #{i:>5}  yul: {source}   (bytecode unavailable: `solc` not found)");
        }

        // Mark every k-path this tree walks as covered.
        updater = updater
            .visit(program, 0)
            .expect("k-path update never errors")
            .continue_value()
            .expect("k-path update never breaks");
        alt_updater = alt_updater
            .visit(program, 0)
            .expect("k-alt-path update never errors")
            .continue_value()
            .expect("k-alt-path update never breaks");

        if i % report_every == 0 || i == samples {
            let (uncovered, total) = updater.kpaths().k_paths();
            let covered = total - uncovered;
            let oracle_note = if evm_on {
                format!(
                    "   solc {solc_valid}/{solc_checked} distinct  evm[ok {evm_success}, rev {evm_reverted}, err {evm_errored}]"
                )
            } else if solc_on {
                format!(
                    "   solc-valid {solc_valid}/{solc_checked} distinct ({:.1}%)",
                    percent(solc_valid, solc_checked)
                )
            } else {
                String::new()
            };
            println!(
                "  after {i:>6} samples: covered {covered:>7} / {total}  ({:5.2}%){oracle_note}",
                percent(covered, total)
            );
        }

        if i >= samples {
            break 'harvest;
        }
      }
      population = runtime.step(&mut generators, &mut sampler, population)?;
      generation += 1;
      for ind in &population {
          let (nodes, violations) = objectives(ind.node());
          evo_log.push((generation, nodes, violations));
      }
    }

    // Dump the objective-space trace for plotting.
    {
        use std::io::Write as _;
        let path = std::path::Path::new("nsga2_population.csv");
        let mut file = std::fs::File::create(path)?;
        writeln!(file, "generation,nodes,violations")?;
        for (g, n, v) in &evo_log {
            writeln!(file, "{g},{n},{v}")?;
        }
        println!(
            "NSGA-II objective log: {} rows over {} generations -> {}",
            evo_log.len(),
            generation + 1,
            path.display()
        );
    }

    let (alt_uncovered, alt_total) = alt_updater.altpaths().alt_paths();
    let alt_covered = alt_total - alt_uncovered;
    let (uncovered, total) = updater.kpaths().k_paths();
    let covered = total - uncovered;
    println!();
    println!(
        "Covered {alt_covered} of {alt_total} {k}-alt-paths ({:.2}%)  \
         [{:.1}x fewer subdomains than {kplus1}-path, which it covers]",
        percent(alt_covered, alt_total),
        kplus1_total as f64 / alt_total.max(1) as f64
    );
    println!(
        "Covered {covered} of {total} {k}-paths ({:.2}%) from {samples} programs \
         ({total_len} chars of Yul total).",
        percent(covered, total)
    );
    println!(
        "Evolution: NSGA-II toward {TARGET_NODES} nodes, pop {POP}, {WARMUP} warm-up gens \
         (avg {:.0} chars/program).",
        total_len as f64 / samples as f64
    );

    if solc_on {
        println!(
            "Distinct programs: {solc_checked} of {samples} generated \
             ({duplicates} duplicates skipped)."
        );
        println!(
            "Valid (solc): {solc_valid} of {solc_checked} distinct compiled ({:.2}%), \
             solc wall time {:.2}s.",
            percent(solc_valid, solc_checked),
            solc_time.as_secs_f64()
        );
        if evm_on {
            println!(
                "Executed (evm), of the {solc_valid} compiled: {evm_success} succeeded, \
                 {evm_reverted} reverted, {evm_errored} errored (evm wall time {:.2}s).",
                evm_time.as_secs_f64()
            );
        }
        if let Some((yul, bytecode, outcome)) = &first_valid {
            println!("  first accepted: {yul}");
            println!("        bytecode: 0x{bytecode}   evm: {outcome}");
        }
        if let Some((yul, why)) = &first_invalid {
            println!("  first rejected: {yul}");
            if !why.is_empty() {
                println!("             why: {why}");
            }
        }
    }

    Ok(())
}

fn alloc_string_for_curve() -> String {
    String::new()
}

fn percent(part: usize, whole: usize) -> f64 {
    if whole == 0 {
        0.0
    } else {
        part as f64 / whole as f64 * 100.0
    }
}
