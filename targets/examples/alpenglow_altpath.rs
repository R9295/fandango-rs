//! Generate `fuzz_ag_votor` scenarios.
//!
//! Each scenario is a JSON input for firedancer's `fuzz_ag_votor` harness, generated and
//! then fixed by [`alpenglow::fix`].
//!
//! Usage:
//! ```text
//! cargo run --release --example alpenglow_altpath --features alpenglow -- [-n ITERATIONS] [-s SEED] [-o OUT_DIR]
//! ```
//! Defaults: 1000 iterations, a seed from OS entropy, no output directory.

use anyhow::{Context, Error};
use clap::Parser;
use fandango::generation::Generated;
use fandango::tuple_list::tuple_list;
use fandango::visitor::Visitor;
use fandango::visitor::write::WriteVisitor;
use fandango_runtime::operators::DepthLimiter;
use fandango_targets::alpenglow::{self, nonterminal_start};
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

    for iteration in 1..=iterations {
        let mut scenario = nonterminal_start::generate(&mut sampler, &mut generators, 0);
        alpenglow::fix(&mut scenario, &mut sampler, &mut generators);
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

    Ok(())
}
