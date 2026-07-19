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
use fandango::generation::Generated;
use fandango::tuple_list::tuple_list;
use fandango::typing::Structured;
use fandango::visitor::Visitor;
use fandango::visitor::VisitorMut;
use fandango::visitor::write::WriteVisitor;
use fandango_core::visitor::kpath::{KPathUpdate, KPaths};
use fandango_runtime::operators::DepthLimiter;
use fandango_targets::yul::{self, ScopeFixer, TypeMut, nonterminal_start};
use rand::SeedableRng;
use rand::rngs::StdRng;
use std::collections::HashSet;
use std::num::NonZeroUsize;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

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

    // Size window. Unfiltered generation is ~50% trivial `{ let _x }` with a heavy tail
    // of giant programs — the signature of a near-critical branching process — so we
    // resample both ends and keep the substantive middle.
    const MIN_CHARS: usize = 40;
    const MAX_CHARS: usize = 400;
    const MAX_TRIES: usize = 40;
    println!("size window: {MIN_CHARS}..={MAX_CHARS} chars (resampled, max {MAX_TRIES} tries)");

    // Build the k-path universe directly from the grammar graph.
    let mut kpaths = KPaths::new::<TypeMut<'static>>(k, nonterminal_start::ROOT.inner());
    let (_, total) = kpaths.k_paths();
    println!("grammar = grammars/yul.fan   k = {k}   total {k}-paths in grammar = {total}");

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

    let mut resampled = 0usize;
    for i in 1..=samples {
        // Generate, repair variable scope, and keep only programs inside the size window;
        // anything trivial or gigantic is thrown away and resampled.
        let (program, source) = {
            let mut tries = 0usize;
            loop {
                let mut candidate =
                    nonterminal_start::generate(&mut sampler, &mut generators, 0);
                let _ =
                    ScopeFixer::new(&mut sampler, &mut generators).visit_mut(&mut candidate, 0);
                let rendered = to_source(&candidate)?;
                tries += 1;
                if (MIN_CHARS..=MAX_CHARS).contains(&rendered.len()) || tries >= MAX_TRIES {
                    resampled += tries - 1;
                    break (candidate, rendered);
                }
            }
        };
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
            .visit(&program, 0)
            .expect("k-path update never errors")
            .continue_value()
            .expect("k-path update never breaks");

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
    }

    let (uncovered, total) = updater.kpaths().k_paths();
    let covered = total - uncovered;
    println!();
    println!(
        "Covered {covered} of {total} {k}-paths ({:.2}%) from {samples} programs \
         ({total_len} chars of Yul total).",
        percent(covered, total)
    );
    println!(
        "Size filter: kept {samples} in {MIN_CHARS}..={MAX_CHARS} chars, discarded {resampled} \
         out-of-window candidates (avg {:.0} chars/program).",
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

fn percent(part: usize, whole: usize) -> f64 {
    if whole == 0 {
        0.0
    } else {
        part as f64 / whole as f64 * 100.0
    }
}
