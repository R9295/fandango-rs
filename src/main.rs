//! Command-line interface for generating samples from FANDANGO grammars.

#![allow(deprecated)]

extern crate alloc;

mod alpenglow;

use fandango::{
    dynamic::{DynamicNode, DynamicSampler},
    generation::Generated,
    lang::{FandangoNode, Program, Statement},
    visitor::{Visitor, write::WriteVisitor},
};
use rand::{SeedableRng, rngs::StdRng};
use std::{
    env,
    error::Error,
    ffi::OsString,
    fmt, fs,
    fs::File,
    io::{self, Write},
    num::NonZeroUsize,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

#[derive(Debug)]
struct CliError(String);

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Error for CliError {}

#[derive(Debug)]
struct FuzzArgs {
    file: PathBuf,
    count: usize,
    seed: u64,
    output: Option<PathBuf>,
    k_path: Option<NonZeroUsize>,
    coverage_state: Option<PathBuf>,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error}");
        std::process::exit(2);
    }
}

fn run() -> Result<()> {
    match parse_args(env::args_os().skip(1))? {
        Command::Help => {
            print_usage();
            Ok(())
        }
        Command::Fuzz(args) => fuzz(args),
    }
}

#[derive(Debug)]
enum Command {
    Help,
    Fuzz(FuzzArgs),
}

fn parse_args(args: impl IntoIterator<Item = OsString>) -> Result<Command> {
    let mut args = args.into_iter();
    let Some(first) = args.next() else {
        return Ok(Command::Help);
    };

    if first == "-h" || first == "--help" {
        return Ok(Command::Help);
    }

    let mut args = if first == "fuzz" {
        args.collect::<Vec<_>>().into_iter()
    } else if first.to_string_lossy().starts_with('-') {
        std::iter::once(first)
            .chain(args)
            .collect::<Vec<_>>()
            .into_iter()
    } else {
        return Err(cli_error(format!(
            "unknown command {:?}\n\n{}",
            first, USAGE
        )));
    };

    let mut file = None;
    let mut count = 1usize;
    let mut seed = None;
    let mut output = None;
    let mut k_path = None;
    let mut coverage_state = None;

    while let Some(arg) = args.next() {
        match arg.to_string_lossy().as_ref() {
            "-f" | "--file" => {
                let Some(value) = args.next() else {
                    return Err(cli_error("missing value for -f/--file"));
                };
                file = Some(PathBuf::from(value));
            }
            "-n" | "--count" => {
                let Some(value) = args.next() else {
                    return Err(cli_error("missing value for -n/--count"));
                };
                count = value
                    .to_string_lossy()
                    .parse()
                    .map_err(|err| cli_error(format!("invalid count {:?}: {err}", value)))?;
            }
            "-s" | "--seed" => {
                let Some(value) = args.next() else {
                    return Err(cli_error("missing value for -s/--seed"));
                };
                seed = Some(
                    value
                        .to_string_lossy()
                        .parse()
                        .map_err(|err| cli_error(format!("invalid seed {:?}: {err}", value)))?,
                );
            }
            "-o" | "--output" => {
                let Some(value) = args.next() else {
                    return Err(cli_error("missing value for -o/--output"));
                };
                output = Some(PathBuf::from(value));
            }
            "--k-path" => {
                let Some(value) = args.next() else {
                    return Err(cli_error("missing value for --k-path"));
                };
                let parsed = value
                    .to_string_lossy()
                    .parse::<usize>()
                    .map_err(|error| cli_error(format!("invalid k {:?}: {error}", value)))?;
                k_path = Some(
                    NonZeroUsize::new(parsed)
                        .ok_or_else(|| cli_error("--k-path must be greater than zero"))?,
                );
            }
            "--coverage-state" => {
                let Some(value) = args.next() else {
                    return Err(cli_error("missing value for --coverage-state"));
                };
                coverage_state = Some(PathBuf::from(value));
            }
            "-h" | "--help" => return Ok(Command::Help),
            other => {
                return Err(cli_error(format!("unknown argument {other:?}\n\n{USAGE}")));
            }
        }
    }

    let file = file.ok_or_else(|| cli_error("missing required -f/--file argument"))?;
    if coverage_state.is_some() && k_path.is_none() {
        return Err(cli_error("--coverage-state requires --k-path"));
    }
    let seed = match seed {
        Some(seed) => seed,
        None => current_time_seed()?,
    };
    Ok(Command::Fuzz(FuzzArgs {
        file,
        count,
        seed,
        output,
        k_path,
        coverage_state,
    }))
}

fn current_time_seed() -> Result<u64> {
    let millis = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis();
    u64::try_from(millis).map_err(|_| cli_error("current Unix time does not fit in a u64 seed"))
}

fn fuzz(args: FuzzArgs) -> Result<()> {
    if let Some(path) = args.output.as_ref() {
        let mut output = File::create(path)?;
        fuzz_to(&args, &mut output)
    } else {
        let stdout = io::stdout();
        let mut output = stdout.lock();
        fuzz_to(&args, &mut output)
    }
}

fn fuzz_to(args: &FuzzArgs, output: &mut dyn Write) -> Result<()> {
    let mut rng = StdRng::seed_from_u64(args.seed);
    let source = fs::read_to_string(&args.file)?;

    if is_alpenglow_target(&args.file) {
        if source != alpenglow::GRAMMAR {
            return Err(cli_error(format!(
                "{} does not match the Alpenglow grammar compiled into this binary",
                args.file.display()
            )));
        }
        let mut coverage = args
            .k_path
            .map(|k| alpenglow::KPathCoverage::load(k, args.coverage_state.as_deref()))
            .transpose()?;
        for _ in 0..args.count {
            let node = alpenglow::generate_tree(&mut rng);
            if let Some(coverage) = coverage.as_mut() {
                coverage.observe(&node);
            }
            write_node(&node, output)?;
        }
        if let Some(coverage) = coverage {
            if let Some(state) = args.coverage_state.as_deref() {
                coverage.save(state)?;
            }
            let (covered, total) = coverage.totals();
            let percentage = if total == 0 {
                0.0
            } else {
                covered as f64 / total as f64 * 100.0
            };
            eprintln!(
                "k-path({}): {covered}/{total} ({percentage:.2}%)",
                args.k_path.unwrap()
            );
        }
        return Ok(());
    }

    if args.k_path.is_some() {
        return Err(cli_error(
            "k-path reporting is currently available only for alpenglow-simpler.fan",
        ));
    }

    let program = parse_owned_program(source)
        .map_err(|error| cli_error(format!("failed to parse {}: {error}", args.file.display())))?;
    let root = FandangoNode::from(&*program);
    let definition = start_definition(program)?;
    let nonterminals = program.nonterminals();

    for _ in 0..args.count {
        let mut sampler = DynamicSampler::new(root, definition, &nonterminals, &mut rng);
        let node = DynamicNode::generate(&mut sampler, &mut (), 0);
        write_node(&node, output)?;
    }

    Ok(())
}

fn write_node<'a, N, T>(node: &'a N, output: &mut dyn Write) -> Result<()>
where
    N: fandango::typing::Node<Type<'a> = T>,
    T: fandango::visitor::VisitableChildren<T> + From<&'a N> + fandango::typing::AsNodeRef<N>,
{
    let visitor = match WriteVisitor::new(Vec::new()).visit(node, 0)? {
        std::ops::ControlFlow::Continue(visitor) => visitor,
        std::ops::ControlFlow::Break(never) => match never {},
    };
    output.write_all(&visitor.output())?;
    output.write_all(b"\n")?;
    Ok(())
}

fn parse_owned_program(
    source: String,
) -> std::result::Result<&'static mut Program<'static>, String> {
    let source: &'static str = Box::leak(source.into_boxed_str());
    Program::try_from(source)
        .map(|program| Box::leak(Box::new(program)))
        .map_err(|err| err.to_string())
}

fn is_alpenglow_target(path: &Path) -> bool {
    path.file_name()
        .is_some_and(|name| name == "alpenglow-simpler.fan")
}

fn start_definition(program: &'static Program<'static>) -> Result<FandangoNode<'static, 'static>> {
    program
        .statements()
        .iter()
        .find_map(|statement| match statement.inner() {
            Statement::Production(production)
                if production.inner().nonterminal().inner().name() == "start" =>
            {
                Some(FandangoNode::from(production.inner().nonterminal()))
            }
            Statement::Production(_) => None,
            Statement::Constraint(_) | Statement::Python => None,
        })
        .ok_or_else(|| cli_error("grammar has no production to use as a start symbol"))
}

fn print_usage() {
    println!("{USAGE}");
}

fn cli_error(message: impl Into<String>) -> Box<dyn Error> {
    Box::new(CliError(message.into()))
}

const USAGE: &str = "\
Usage:
  fandango-rs -f <grammar.fan> [--count N] [--seed SEED] [--output PATH]
              [--k-path K] [--coverage-state PATH]
  fandango-rs fuzz -f <grammar.fan> [options]

Options:
  -f, --file <path>   Fandango grammar file to generate from
  -n, --count <N>     Number of samples to print [default: 1]
  -s, --seed <SEED>   Deterministic RNG seed [default: current Unix time in milliseconds]
  -o, --output <path> Write generated samples to a file [default: stdout]
      --k-path <K>     Measure cumulative grammar k-path coverage
      --coverage-state <path>
                         Persist k-path coverage across invocations
  -h, --help          Print this help text";
