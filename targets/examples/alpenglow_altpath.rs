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
use fandango_targets::alpenglow::{
    self, Certificate, CertificateConstraintVisitor, TypeMut, nonterminal_start,
};
use rand::SeedableRng;
use rand::rngs::StdRng;
use std::num::NonZeroUsize;

const HELP: &str = r"Generate certified Alpenglow vote scenarios and measure k-alt-path diversity.

Usage: alpenglow_altpath [K] [ITERATIONS] [SEED]

Arguments:
  K           Maximum alternations per alt-path [default: 2]
  ITERATIONS  Number of certified scenarios to generate [default: 1000]
  SEED        Reproducible u64 RNG seed [default: OS entropy]

Options:
  -h, --help  Print help

Example:
  cargo run --release --example alpenglow_altpath --features alpenglow -- 2 1000 42";

const MAX_GENERATION_ATTEMPTS: usize = 10_000;

fn render(scenario: &nonterminal_start) -> Result<String, Error> {
    let bytes = WriteVisitor::new(Vec::new())
        .visit(scenario, 0)?
        .continue_value()
        .expect("write visitor never breaks")
        .output();
    Ok(String::from_utf8(bytes)?)
}

fn certificates(scenario: &nonterminal_start) -> Vec<Certificate> {
    CertificateConstraintVisitor::default()
        .visit(scenario, 0)
        .expect("certificate constraint never errors")
        .continue_value()
        .expect("certificate constraint never breaks")
        .certificates()
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
    let mut rejected = 0usize;

    println!("grammar = grammars/alpenglow.fan   k = {k}   total {k}-alt-paths = {total}");

    for iteration in 1..=iterations {
        let (scenario, generated_certificates, attempts) = (1..=MAX_GENERATION_ATTEMPTS)
            .find_map(|attempt| {
                let scenario = nonterminal_start::generate(&mut sampler, &mut generators, 0);
                let generated_certificates = certificates(&scenario);
                (!generated_certificates.is_empty()).then_some((
                    scenario,
                    generated_certificates,
                    attempt,
                ))
            })
            .context("could not generate a certificate-valid scenario in 10,000 attempts")?;
        rejected += attempts - 1;

        if iteration == 1 {
            println!("--- iteration #1 ---");
            print!("{}", render(&scenario)?);
            println!("certificates:");
            for certificate in &generated_certificates {
                println!(
                    "  - {} ({}% distinct stake)",
                    certificate.kind(),
                    certificate.stake_percent()
                );
            }
            println!("--- end iteration #1 ---");
        }

        updater = updater
            .visit(&scenario, 0)
            .expect("k-alt-path update never errors")
            .continue_value()
            .expect("k-alt-path update never breaks");

        if iteration == 1 || iteration % report_every == 0 || iteration == iterations {
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
    println!(
        "Constraint: all {iterations} scenarios produced a certificate; {rejected} uncertified candidates were rejected."
    );

    Ok(())
}

#[allow(clippy::cast_precision_loss)]
fn percent(part: usize, whole: usize) -> f64 {
    if whole == 0 {
        0.0
    } else {
        part as f64 / whole as f64 * 100.0
    }
}
