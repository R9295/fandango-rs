//! Generate `fuzz_ag_votor` scenarios.
//!
//! Each scenario is a JSON input for firedancer's `fuzz_ag_votor` harness. Scenarios that
//! violate a constraint of [`ConstraintVisitor`] are rejected and generated again.
//!
//! Usage:
//! ```text
//! cargo run --release --example alpenglow_altpath --features alpenglow -- [-n ITERATIONS] [-s SEED] [-o OUT_DIR] [--max-attempts ATTEMPTS]
//! ```
//! Defaults: 1000 iterations, a seed from OS entropy, no output directory, 10,000
//! attempts per scenario.

use anyhow::{Context, Error};
use clap::Parser;
use fandango::generation::Generated;
use fandango::tuple_list::tuple_list;
use fandango::visitor::Visitor;
use fandango::visitor::write::WriteVisitor;
use fandango_runtime::operators::DepthLimiter;
use fandango_targets::alpenglow::{self, ConstraintVisitor, nonterminal_start};
use rand::SeedableRng;
use rand::rngs::StdRng;
use std::fs;
use std::num::NonZeroUsize;
use std::path::PathBuf;

/// Generate fuzz_ag_votor scenarios.
#[derive(Parser)]
#[command(
    after_help = "Example:\n  cargo run --release --example alpenglow_altpath --features alpenglow -- -n 1000 -s 42 -o corpus"
)]
struct Args {
    /// Number of scenarios to generate
    #[arg(short = 'n', long, default_value = "1000")]
    iterations: NonZeroUsize,

    /// Reproducible RNG seed [default: OS entropy]
    #[arg(short, long)]
    seed: Option<u64>,

    /// Directory to write each scenario to as <ITERATION>.json
    #[arg(short, long)]
    out_dir: Option<PathBuf>,

    /// Candidates to generate per scenario before giving up
    #[arg(long, default_value = "10000")]
    max_attempts: NonZeroUsize,
}

fn check(scenario: &nonterminal_start) -> ConstraintVisitor {
    ConstraintVisitor::default()
        .visit(scenario, 0)
        .expect("constraint never errors")
        .continue_value()
        .expect("constraint never breaks")
}

fn render(scenario: &nonterminal_start) -> Result<String, Error> {
    let bytes = WriteVisitor::new(Vec::new())
        .visit(scenario, 0)?
        .continue_value()
        .expect("write visitor never breaks")
        .output();
    Ok(String::from_utf8(bytes)?)
}

fn main() -> Result<(), Error> {
    let args = Args::parse();
    let iterations = args.iterations.get();

    let mut sampler = match args.seed {
        Some(seed) => StdRng::seed_from_u64(seed),
        None => StdRng::from_os_rng(),
    };

    if let Some(dir) = &args.out_dir {
        fs::create_dir_all(dir).with_context(|| format!("could not create {}", dir.display()))?;
    }

    let generator = DepthLimiter::new(alpenglow::STRUCTURE.inner(), 40);
    let mut generators = tuple_list!(generator);

    let mut rejected = 0usize;
    for iteration in 1..=iterations {
        let mut accepted = None;
        let mut closest = usize::MAX;
        for _ in 0..args.max_attempts.get() {
            let candidate = nonterminal_start::generate(&mut sampler, &mut generators, 0);
            let missing = check(&candidate).missing_votes().len();
            if missing == 0 {
                accepted = Some(candidate);
                break;
            }
            rejected += 1;
            closest = closest.min(missing);
        }
        let scenario = accepted.with_context(|| {
            format!(
                "could not generate scenario #{iteration} in {} attempts: every candidate left \
                 some node without a vote in some slot (the closest missed {closest} node-slot votes)",
                args.max_attempts
            )
        })?;
        let json = render(&scenario)?;

        if let Some(dir) = &args.out_dir {
            let path = dir.join(format!("{iteration:06}.json"));
            fs::write(&path, &json).with_context(|| format!("could not write {}", path.display()))?;
        }
    }

    match &args.out_dir {
        Some(dir) => println!("Wrote {iterations} scenarios to {}.", dir.display()),
        None => println!("Generated {iterations} scenarios."),
    }
    println!("Rejected {rejected} candidates that violated a constraint.");

    Ok(())
}
