//! Generate Alpenglow vote scenarios and measure grammar k-alt-path diversity.
//!
//! Usage:
//! ```text
//! cargo run --release --example alpenglow_altpath --features alpenglow -- [K] [ITERATIONS] [SEED]
//! ```
//! Defaults: `K = 2`, `ITERATIONS = 1000`, `SEED` = OS entropy.

use anyhow::{Context, Error};
use fandango::generation::Generated;
use fandango::tuple_list::tuple_list;
use fandango::typing::Structured;
use fandango::visitor::Visitor;
use fandango::visitor::write::WriteVisitor;
use fandango_core::visitor::altpath::{AltPathUpdate, AltPaths};
use fandango_runtime::operators::DepthLimiter;
use fandango_targets::alpenglow::{self, TypeMut, nonterminal_start};
use rand::SeedableRng;
use rand::rngs::StdRng;
use std::num::NonZeroUsize;

const HELP: &str = "Generate Alpenglow vote scenarios and measure k-alt-path diversity.\n\
\n\
Usage: alpenglow_altpath [K] [ITERATIONS] [SEED]\n\
\n\
Arguments:\n\
  K           Maximum alternations per alt-path [default: 2]\n\
  ITERATIONS  Number of scenarios to generate [default: 1000]\n\
  SEED        Reproducible u64 RNG seed [default: OS entropy]\n\
\n\
Options:\n\
  -h, --help  Print help\n\
\n\
Example:\n\
  cargo run --release --example alpenglow_altpath --features alpenglow -- 2 1000 42";

fn render(scenario: &nonterminal_start) -> Result<String, Error> {
    let bytes = WriteVisitor::new(Vec::new())
        .visit(scenario, 0)?
        .continue_value()
        .expect("write visitor never breaks")
        .output();
    Ok(String::from_utf8(bytes)?)
}

fn main() -> Result<(), Error> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "-h" || arg == "--help") {
        println!("{HELP}");
        return Ok(());
    }

    let k: usize = args
        .first()
        .map_or(Ok(2), |value| value.parse())
        .context("K must be a positive integer")?;
    let iterations: usize = args
        .get(1)
        .map_or(Ok(1000), |value| value.parse())
        .context("ITERATIONS must be a positive integer")?;
    let k = NonZeroUsize::new(k).context("K must be > 0")?;
    anyhow::ensure!(iterations > 0, "ITERATIONS must be > 0");

    let mut sampler = match args.get(2) {
        Some(seed) => StdRng::seed_from_u64(seed.parse().context("SEED must be a u64")?),
        None => StdRng::from_os_rng(),
    };

    let generator = DepthLimiter::new(alpenglow::STRUCTURE.inner(), 40);
    let mut generators = tuple_list!(generator);
    let mut altpaths = AltPaths::new::<TypeMut<'static>>(k, nonterminal_start::ROOT.inner());
    let (_, total) = altpaths.alt_paths();
    let mut updater = AltPathUpdate::inserting(&mut altpaths);
    let report_every = (iterations / 10).max(1);

    println!("grammar = grammars/alpenglow.fan   k = {k}   total {k}-alt-paths = {total}");

    for iteration in 1..=iterations {
        let scenario = nonterminal_start::generate(&mut sampler, &mut generators, 0);

        if iteration == 1 {
            println!("--- iteration #1 ---");
            print!("{}", render(&scenario)?);
            println!("--- end iteration #1 ---");
        }

        updater = updater
            .visit(&scenario, 0)
            .expect("k-alt-path update never errors")
            .continue_value()
            .expect("k-alt-path update never breaks");

        if iteration % report_every == 0 || iteration == iterations {
            let (uncovered, total) = updater.altpaths().alt_paths();
            let covered = total - uncovered;
            println!(
                "  after {iteration:>6} iterations: diverse {covered:>5} / {total} {k}-alt-paths ({:5.2}%)",
                percent(covered, total)
            );
        }
    }

    let (uncovered, total) = updater.altpaths().alt_paths();
    let covered = total - uncovered;
    println!(
        "Covered {covered} of {total} {k}-alt-paths ({:.2}%) across {iterations} iterations.",
        percent(covered, total)
    );

    Ok(())
}

fn percent(part: usize, whole: usize) -> f64 {
    if whole == 0 {
        0.0
    } else {
        part as f64 / whole as f64 * 100.0
    }
}
