//! Continuous JavaScript generation with NSGA-II and in-process Tree-sitter checks.
//!
//! Run `js_lang --help` for bounded runs, seed control, and tuning. By default this
//! runs until interrupted. Candidates are counted as they are created, including
//! offspring that do not survive selection. No corpus or execution target is used.

#![feature(alloc_error_hook)]

use anyhow::{Error, anyhow, bail};
use fandango::generation::{Generated, Sampler};
use fandango::lang::FandangoNode;
use fandango::tuple_list::{tuple_list, tuple_list_type};
use fandango::typing::{AsNodeRef, Node, Opaque};
use fandango::visitor::altpath::AltPaths;
use fandango::visitor::navigation::CountNodes;
use fandango::visitor::write::WriteVisitor;
use fandango::visitor::{VisitResult, VisitableChildren, Visitor};
use fandango_runtime::evolvers::Evolver;
use fandango_runtime::evolvers::basic::BasicHook;
use fandango_runtime::evolvers::multi::{
    AltPathDiversityHook, FitnessMeasurerTuple, MeasurementsCombined, Nsga2Evolver,
};
use fandango_runtime::operators::DepthLimiter;
use fandango_targets::javascript::{self, Analysis, SizeGoals, Validator, nonterminal_start};
use num_rational::Ratio;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use std::cell::RefCell;
use std::cmp::Reverse;
use std::collections::VecDeque;
use std::convert::Infallible;
use std::num::NonZeroUsize;
use std::ops::ControlFlow;
use std::rc::Rc;
use std::time::{Duration, Instant};

const USAGE: &str = "Usage: js_lang [OPTIONS]
  --population N       Survivors per generation [100]
  --offspring N        New candidates per generation [100]
  --crossover N        Crossover percentage among evolved candidates [80]
  --fresh N            Percentage replaced with fresh random candidates [10]
  --alt-k N            Alternation choices per coverage path, positive integer [2]
  --seed N             Reproducible RNG seed [random; printed at startup]
  --generations N      Stop after N evolution steps [unlimited; 0 = initial only]
  --seconds N          Stop after N seconds, checked between generations [unlimited]
  --report-every N     Seconds between statistics reports [5]
  --generation-depth N Depth threshold for shortest grammar expansions [24]
  --max-depth N        Maximum generated tree depth [128]
  --max-nodes N        Maximum generated tree nodes [4096]
  --max-bytes N        Maximum source length [65536]
  --max-functions N    Soft upper function-count goal [2]
  --min-statements N   Soft lower statement-count goal [10]
  --min-objects N      Soft lower object-literal goal [1]
  --min-arrays N       Soft lower array-literal goal [1]
  --min-loops N        Soft lower loop-count goal [1]
  -h, --help           Show this help

No time or generation limit is imposed by default; interrupt to stop.
Statistics count evaluations (including duplicates), not unique programs.
Size goals never gate acceptance. Programs are parsed, never executed or saved.
Rare k-alt-paths break Pareto-front ties; coverage reports count accepted inputs.
Large --alt-k values can exhaust memory; path counts grow exponentially.";

#[derive(Clone, Copy, Debug)]
struct Bounds {
    depth: usize,
    nodes: usize,
    bytes: usize,
}

#[derive(Debug)]
struct Options {
    population: usize,
    offspring: usize,
    crossover: usize,
    fresh: usize,
    alt_k: NonZeroUsize,
    seed: Option<u64>,
    generations: Option<usize>,
    seconds: Option<u64>,
    report_every: u64,
    generation_depth: usize,
    bounds: Bounds,
    goals: SizeGoals,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            population: 100,
            offspring: 100,
            crossover: 80,
            fresh: 10,
            alt_k: NonZeroUsize::new(2).unwrap(),
            seed: None,
            generations: None,
            seconds: None,
            report_every: 5,
            generation_depth: 24,
            bounds: Bounds {
                depth: 128,
                nodes: 4096,
                bytes: 65_536,
            },
            goals: SizeGoals::default(),
        }
    }
}

impl Options {
    fn parse(args: impl IntoIterator<Item = String>) -> Result<Self, Error> {
        let mut options = Self::default();
        let mut args = args.into_iter();
        while let Some(flag) = args.next() {
            let value = args
                .next()
                .ok_or_else(|| anyhow!("missing value for {flag}"))?;
            let number = value
                .parse::<usize>()
                .map_err(|_| anyhow!("invalid value for {flag}: {value}"))?;
            match flag.as_str() {
                "--population" => options.population = number,
                "--offspring" => options.offspring = number,
                "--crossover" => options.crossover = number,
                "--fresh" => options.fresh = number,
                "--alt-k" => {
                    options.alt_k = NonZeroUsize::new(number)
                        .ok_or_else(|| anyhow!("alt-k must be positive"))?;
                }
                "--seed" => options.seed = Some(value.parse()?),
                "--generations" => options.generations = Some(number),
                "--seconds" => options.seconds = Some(value.parse()?),
                "--report-every" => options.report_every = value.parse()?,
                "--generation-depth" => options.generation_depth = number,
                "--max-depth" => options.bounds.depth = number,
                "--max-nodes" => options.bounds.nodes = number,
                "--max-bytes" => options.bounds.bytes = number,
                "--max-functions" => options.goals.max_functions = number,
                "--min-statements" => options.goals.min_statements = number,
                "--min-objects" => options.goals.min_objects = number,
                "--min-arrays" => options.goals.min_arrays = number,
                "--min-loops" => options.goals.min_loops = number,
                _ => bail!("unknown option {flag}; use --help"),
            }
        }
        if options.population == 0 || options.offspring == 0 {
            bail!("population and offspring must be positive");
        }
        options
            .population
            .checked_add(options.offspring)
            .ok_or_else(|| anyhow!("population + offspring overflows"))?;
        if options.crossover > 100 || options.fresh > 100 {
            bail!("crossover and fresh percentages must be in 0..=100");
        }
        if options.report_every == 0 || options.seconds == Some(0) {
            bail!("seconds and report-every must be positive");
        }
        // Crossover adds two bounded subtrees before the hook can inspect the
        // result. Keep both generation and retained-tree depths stack-safe.
        if !(8..=64).contains(&options.generation_depth)
            || !(8..=256).contains(&options.bounds.depth)
            || options.bounds.nodes < 8
            || options.bounds.bytes == 0
        {
            bail!(
                "generation-depth must be 8..=64, max-depth 8..=256, max-nodes >= 8, and max-bytes > 0"
            );
        }
        Ok(options)
    }
}

/// Stop traversing as soon as a resource budget is exceeded.
struct TreeBudget {
    remaining: usize,
    remaining_bytes: usize,
    depth: usize,
    max_depth: usize,
}

impl<T: VisitableChildren<T>> Visitor<T> for TreeBudget {
    type Continue = Self;
    type Break = ();
    type Error = Infallible;

    fn visit<'a, N>(mut self, node: &'a N, _idx: usize) -> VisitResult<Self, T>
    where
        N: Node<Type<'a> = T>,
        T: From<&'a N> + AsNodeRef<N>,
    {
        if self.remaining == 0 || self.depth > self.max_depth {
            return Ok(ControlFlow::Break(()));
        }
        self.remaining -= 1;
        if let FandangoNode::String(literal) = node.definition() {
            let Some(remaining) = self.remaining_bytes.checked_sub(literal.inner().len()) else {
                return Ok(ControlFlow::Break(()));
            };
            self.remaining_bytes = remaining;
        }
        self.depth += 1;
        match node.opaque().visit_each(self)? {
            ControlFlow::Continue(mut budget) => {
                budget.depth -= 1;
                Ok(ControlFlow::Continue(budget))
            }
            ControlFlow::Break(()) => Ok(ControlFlow::Break(())),
        }
    }
}

impl Bounds {
    fn accepts<N: Node>(self, node: &N) -> bool {
        TreeBudget {
            remaining: self.nodes,
            remaining_bytes: self.bytes,
            depth: 0,
            max_depth: self.depth,
        }
        .visit(node, 0)
        .unwrap()
        .is_continue()
    }
}

struct GenerationHook {
    bounds: Bounds,
    fresh: usize,
}

impl<N, G, S> BasicHook<N, G, S> for GenerationHook
where
    N: Node + Generated<S, G>,
    S: Sampler<N>,
{
    fn individual_created(
        &mut self,
        node: &mut N,
        generators: &mut G,
        sampler: &mut S,
    ) -> Result<(), Error> {
        if sampler.sample() % 100 < self.fresh {
            *node = N::generate(sampler, generators, 0);
        }
        if !self.bounds.accepts(node) {
            *node = N::generate(sampler, generators, 0);
            if !self.bounds.accepts(node) {
                // Force the existing depth limiter to take shortest expansions.
                // The JavaScript start rule can always expand to an empty script.
                *node = N::generate(sampler, generators, 1_000_000);
                if !self.bounds.accepts(node) {
                    bail!(
                        "resource limits cannot accommodate a minimal program ({} nodes)",
                        node.count_nodes()
                    );
                }
            }
        }
        Ok(())
    }
}

type Objectives = tuple_list_type!(
    Reverse<usize>,
    Reverse<usize>,
    Reverse<usize>,
    Reverse<usize>,
    Reverse<usize>,
    Reverse<usize>,
    Reverse<usize>,
    Reverse<usize>,
    Reverse<usize>,
    Reverse<usize>,
    Reverse<usize>
);

struct Measurement {
    analysis: Analysis,
    sizes: [usize; 5],
}

impl MeasurementsCombined for Measurement {
    type Fitnesses = Objectives;

    fn combine_step(self) -> (Objectives, Ratio<usize>, Vec<VecDeque<usize>>) {
        let v = self.analysis.validity;
        let [functions, statements, objects, arrays, loops] = self.sizes;
        let fitness = tuple_list!(
            Reverse(v.syntax),
            Reverse(v.bindings),
            Reverse(v.initializers),
            Reverse(v.assignments),
            Reverse(v.returns),
            Reverse(v.loop_control),
            Reverse(functions),
            Reverse(statements),
            Reverse(objects),
            Reverse(arrays),
            Reverse(loops)
        );
        // Tree-sitter byte positions are not Fandango tree paths. Leave paths
        // empty so the evolver chooses random mutation/crossover locations;
        // selection still optimizes every validity penalty independently.
        (
            fitness,
            Ratio::new(usize::from(v.is_valid()), 1),
            Vec::new(),
        )
    }
}

struct Statistics {
    evaluated: u64,
    syntax_ok: u64,
    accepted: u64,
    accepted_and_sized: u64,
    bytes: u64,
    coverage: AltPaths,
    covered: usize,
    novel_inputs: u64,
}

impl Statistics {
    fn new(k: NonZeroUsize) -> Self {
        Self {
            evaluated: 0,
            syntax_ok: 0,
            accepted: 0,
            accepted_and_sized: 0,
            bytes: 0,
            coverage: AltPaths::new::<javascript::Type<'_>>(k, javascript::STRUCTURE.inner()),
            covered: 0,
            novel_inputs: 0,
        }
    }

    // Approximate floating-point rates are display-only; counters remain exact.
    #[allow(clippy::cast_precision_loss)]
    fn report(&self, generation: usize, elapsed: Duration) {
        let seconds = elapsed.as_secs_f64().max(f64::EPSILON);
        let evaluations = self.evaluated.max(1) as f64;
        println!(
            "generation={generation} elapsed={seconds:.1}s evaluated={} rate={:.1}/s syntax_ok={} accepted={} acceptance={:.1}% accepted_rate={:.1}/s accepted_and_sized={} mean_bytes={:.1} accepted_alt_paths={}/{} novel_accepted={}",
            self.evaluated,
            self.evaluated as f64 / seconds,
            self.syntax_ok,
            self.accepted,
            self.accepted as f64 * 100.0 / evaluations,
            self.accepted as f64 / seconds,
            self.accepted_and_sized,
            self.bytes as f64 / evaluations,
            self.covered,
            self.coverage.lookup().len(),
            self.novel_inputs,
        );
    }
}

struct Measurer {
    validator: Validator,
    goals: SizeGoals,
    statistics: Rc<RefCell<Statistics>>,
}

impl<'a> FitnessMeasurerTuple<'a, nonterminal_start, Error> for Measurer {
    type Measurements = Measurement;

    fn evaluate(&mut self, node: &'a nonterminal_start) -> Result<Measurement, Error> {
        let bytes = WriteVisitor::new(Vec::new())
            .visit(node, 0)?
            .continue_value()
            .expect("writer never breaks")
            .output();
        let source = String::from_utf8(bytes)?;
        let analysis = self.validator.analyze(&source)?;
        let sizes = self.goals.penalties(analysis.counts);
        let mut stats = self.statistics.borrow_mut();
        stats.evaluated += 1;
        stats.bytes += source.len() as u64;
        stats.syntax_ok += u64::from(analysis.validity.syntax == 0);
        if analysis.validity.is_valid() {
            stats.accepted += 1;
            stats.accepted_and_sized += u64::from(sizes == [0; 5]);
            let newly_covered = stats.coverage.cover(node);
            stats.covered += newly_covered;
            stats.novel_inputs += u64::from(newly_covered > 0);
        }
        Ok(Measurement { analysis, sizes })
    }
}

fn run(options: &Options) -> Result<(), Error> {
    let seed = options.seed.unwrap_or_else(|| rand::rng().random());
    let mut sampler = StdRng::seed_from_u64(seed);
    let stats = Rc::new(RefCell::new(Statistics::new(options.alt_k)));
    let measurer = Measurer {
        validator: Validator::new()?,
        goals: options.goals,
        statistics: Rc::clone(&stats),
    };
    let mut evolver = Nsga2Evolver::new::<nonterminal_start>(
        measurer,
        AltPathDiversityHook::new(
            GenerationHook {
                bounds: options.bounds,
                fresh: options.fresh,
            },
            options.alt_k,
        ),
        options.population,
        options.offspring,
        Ratio::new(options.crossover, 100),
    )
    .ok_or_else(|| anyhow!("invalid evolver settings"))?;
    let mut generators = tuple_list!(DepthLimiter::new(
        javascript::STRUCTURE.inner(),
        options.generation_depth
    ));
    println!(
        "JavaScript NSGA-II seed={seed} population={} offspring={} crossover={}% fresh={}% alt_k={}",
        options.population, options.offspring, options.crossover, options.fresh, options.alt_k
    );
    println!(
        "soft_goals={:?}; Tree-sitter + contextual checks; no execution or delivery",
        options.goals
    );
    let start = Instant::now();
    let mut population = evolver.initial(&mut generators, &mut sampler)?;
    let mut generation = 0;
    stats.borrow().report(generation, start.elapsed());
    let mut last_report = Instant::now();
    loop {
        if options.generations.is_some_and(|limit| generation >= limit)
            || options
                .seconds
                .is_some_and(|limit| start.elapsed() >= Duration::from_secs(limit))
        {
            break;
        }
        population = evolver.step(&mut generators, &mut sampler, population)?;
        generation += 1;
        if last_report.elapsed() >= Duration::from_secs(options.report_every) {
            stats.borrow().report(generation, start.elapsed());
            last_report = Instant::now();
        }
    }
    stats.borrow().report(generation, start.elapsed());
    Ok(())
}

fn install_oom_warning() {
    std::alloc::set_alloc_error_hook(|layout| {
        use std::io::Write;

        // Format directly to stderr without allocating a message buffer. Ignore
        // write failures: the runtime will abort after this hook returns.
        let _ = writeln!(
            std::io::stderr().lock(),
            "warning: out of memory (allocation of {} bytes failed). Reduce --alt-k or population sizes; aborting.",
            layout.size()
        );
    });
}

fn main() -> Result<(), Error> {
    install_oom_warning();
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!("{USAGE}");
        return Ok(());
    }
    run(&Options::parse(args)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_invalid_cli_limits() {
        for args in [
            vec!["--population", "0"],
            vec!["--offspring", "0"],
            vec!["--fresh", "101"],
            vec!["--crossover", "101"],
            vec!["--alt-k", "0"],
            vec!["--seconds", "0"],
            vec!["--max-depth", "10000"],
            vec!["--report-every", "0"],
            vec!["--seed"],
            vec!["--other", "1"],
        ] {
            assert!(Options::parse(args.into_iter().map(str::to_string)).is_err());
        }
        let defaults = Options::default();
        assert!(defaults.generations.is_none() && defaults.seconds.is_none());
        for k in [1, 5, usize::MAX] {
            let options = Options::parse(["--alt-k".to_owned(), k.to_string()]).unwrap();
            assert_eq!(options.alt_k.get(), k);
        }
    }

    #[test]
    fn byte_budget_matches_emitted_source() {
        let mut generators = tuple_list!(DepthLimiter::new(javascript::STRUCTURE.inner(), 24));
        let mut sampler = StdRng::seed_from_u64(7);
        let mut nonempty = 0;
        for _ in 0..100 {
            let node = nonterminal_start::generate(&mut sampler, &mut generators, 0);
            let bytes = WriteVisitor::new(Vec::new())
                .visit(&node, 0)
                .unwrap()
                .continue_value()
                .unwrap()
                .output();
            let mut bounds = Bounds {
                nodes: usize::MAX,
                depth: usize::MAX,
                bytes: bytes.len(),
            };
            assert!(bounds.accepts(&node));
            if !bytes.is_empty() {
                nonempty += 1;
                bounds.bytes -= 1;
                assert!(!bounds.accepts(&node));
            }
        }
        assert!(nonempty > 0);
    }

    #[test]
    fn seed_replays_with_independently_initialized_limiters() {
        let generate = || {
            let mut generators = tuple_list!(DepthLimiter::new(javascript::STRUCTURE.inner(), 12));
            let mut sampler = StdRng::seed_from_u64(42);
            (0..64)
                .map(|_| {
                    let node = nonterminal_start::generate(&mut sampler, &mut generators, 0);
                    WriteVisitor::new(Vec::new())
                        .visit(&node, 0)
                        .unwrap()
                        .continue_value()
                        .unwrap()
                        .output()
                })
                .collect::<Vec<_>>()
        };
        let expected = generate();
        for _ in 0..32 {
            assert_eq!(generate(), expected);
        }
    }

    #[test]
    fn evolution_keeps_running_and_counts_every_offspring() {
        let k = NonZeroUsize::new(2).unwrap();
        let statistics = Rc::new(RefCell::new(Statistics::new(k)));
        let mut evolver = Nsga2Evolver::new::<nonterminal_start>(
            Measurer {
                validator: Validator::new().unwrap(),
                goals: SizeGoals::default(),
                statistics: Rc::clone(&statistics),
            },
            // Every candidate is reduced to an empty, valid script. Its soft
            // goals remain unmet, but generation must continue and acceptance
            // must still count it.
            AltPathDiversityHook::new(
                GenerationHook {
                    bounds: Bounds {
                        nodes: 8,
                        depth: 8,
                        bytes: 1,
                    },
                    fresh: 10,
                },
                k,
            ),
            4,
            6,
            Ratio::new(80, 100),
        )
        .unwrap();
        let mut generators = tuple_list!(DepthLimiter::new(javascript::STRUCTURE.inner(), 12));
        let mut sampler = StdRng::seed_from_u64(42);
        let mut population = evolver.initial(&mut generators, &mut sampler).unwrap();
        for _ in 0..3 {
            population = evolver
                .step(&mut generators, &mut sampler, population)
                .unwrap();
            assert_eq!(population.len(), 4);
        }
        let stats = statistics.borrow();
        assert_eq!(stats.evaluated, 4 + 6 + 3 * 6);
        assert_eq!(stats.accepted, stats.evaluated);
        assert_eq!(stats.accepted_and_sized, 0);
        assert_eq!(stats.covered, 1);
        assert_eq!(stats.novel_inputs, 1);
    }

    #[test]
    fn only_accepted_inputs_add_coverage_and_duplicates_do_not() {
        let statistics = Rc::new(RefCell::new(Statistics::new(NonZeroUsize::new(2).unwrap())));
        let mut measurer = Measurer {
            validator: Validator::new().unwrap(),
            goals: SizeGoals::default(),
            statistics: Rc::clone(&statistics),
        };
        let mut generators = tuple_list!(DepthLimiter::new(javascript::STRUCTURE.inner(), 24));
        let mut sampler = StdRng::seed_from_u64(7);
        let total = statistics.borrow().coverage.lookup().len();
        let mut rejected = 0;
        for _ in 0..200 {
            let node = nonterminal_start::generate(&mut sampler, &mut generators, 0);
            let before = statistics.borrow().covered;
            let measurement = measurer.evaluate(&node).unwrap();
            let after = statistics.borrow().covered;
            if measurement.analysis.validity.is_valid() {
                assert!(after >= before);
            } else {
                rejected += 1;
                assert_eq!(after, before);
            }
            let novel_before = statistics.borrow().novel_inputs;
            measurer.evaluate(&node).unwrap();
            let stats = statistics.borrow();
            assert_eq!(stats.covered, after);
            assert_eq!(stats.novel_inputs, novel_before);
            assert_eq!(stats.coverage.lookup().len(), total);
            assert_eq!(stats.coverage.alt_paths(), (total - after, total));
        }
        assert!(rejected > 0);
        assert!(statistics.borrow().covered > 1);
    }
}
