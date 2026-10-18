# fandango-rs

# Note:
always compile with -Znext-solver RUSTFLAGS.

This commit represents the artifact for the submission.
Thanks for checking it out!
We're quite proud of it, and happy that you're working with our code.

## Evaluation

Sections 4 and 5 of the paper include a number of experiments.
All section 4 tables may be reproduced by entering the `baselines/` folder and executing `profile.sh`.
You'll need to have Rust and Python 3 already installed for this to work.

The table presented in section 5 is as follows:
```tex
\begin{table}
    \centering
    \caption{Results of \tool{} and \fandango{} generating C programs until one minute is surpassed.
    }
    \label{tab:compiler-test-summary}
    \begin{threeparttable}
    \begin{tabular}{ccrrrrr}
        \toprule
         & \multicolumn{6}{c}{\tool{}} \\
          \cmidrule(l{0.25em}r{0.25em}){2-7}
        Objectives & Multi? & \# Valid & $5$-path & Gen. Time & {\tt gcc} Time & Total \\
        \midrule
        $\varnothing$ & N/A & \unconstrainedCompileAvgRs/\unconstrainedTotalAvgRs & \unconstrainedKPathCoverageAvgRs & \unconstrainedGenTimeAvgRs & \unconstrainedCompileTimeAvgRs & \unconstrainedTotalTimeAvgRs \\
        \multirow{2}*{Validity} & \xmark & \singleValidCompileAvgRs/\singleValidTotalAvgRs & \singleValidKPathCoverageAvgRs & \singleValidGenTimeAvgRs & \singleValidCompileTimeAvgRs & \singleValidTotalTimeAvgRs \\
         & \cmark & \multiValidCompileAvgRs/\multiValidTotalAvgRs & \multiValidKPathCoverageAvgRs & \multiValidGenTimeAvgRs & \multiValidCompileTimeAvgRs & \multiValidTotalTimeAvgRs \\
        \multirow{2}*{Validity $\cup$ Generation} & \xmark & \singleValidAndSizedCompileAvgRs/\singleValidAndSizedTotalAvgRs & \singleValidAndSizedKPathCoverageAvgRs & \singleValidAndSizedGenTimeAvgRs & \singleValidAndSizedCompileTimeAvgRs & \singleValidAndSizedTotalTimeAvgRs \\
         & \cmark & \multiValidAndSizedCompileAvgRs/\multiValidAndSizedTotalAvgRs & \multiValidAndSizedKPathCoverageAvgRs & \multiValidAndSizedGenTimeAvgRs & \multiValidAndSizedCompileTimeAvgRs & \multiValidAndSizedTotalTimeAvgRs \\
        \midrule
         & \multicolumn{6}{c}{\fandango{}} \\
          \cmidrule(l{0.25em}r{0.25em}){2-7}
        Objectives & Multi? & \# Valid & $5$-path & Gen. Time & {\tt gcc} Time & Total \\
        \midrule
        $\varnothing$ & N/A & \unconstrainedCompileAvgPy/\unconstrainedTotalAvgPy & \unconstrainedKPathCoverageAvgPy & \unconstrainedGenTimeAvgPy & \unconstrainedCompileTimeAvgPy & \unconstrainedTotalTimeAvgPy \\
        Validity & \xmark & \validConstrainedCompileAvgPy/\validConstrainedTotalAvgPy & \validConstrainedKPathCoverageAvgPy & \validConstrainedGenTimeAvgPy & \validConstrainedCompileTimeAvgPy & \validConstrainedTotalTimeAvgPy \\
        Validity $\cup$ Generation & \xmark & \textit{n.d.}\tnote{3} & \textit{n.d.}\tnote{3} & \textit{n.d.}\tnote{3} & \textit{n.d.}\tnote{3} & \textit{n.d.}\tnote{4} \\
        \bottomrule
    \end{tabular}
    \begin{tablenotes}
        \item[3] Unable to produce any inputs.
        \item[4] Process self-aborted before producing any inputs.
    \end{tablenotes}
    \end{threeparttable}
\end{table}
```

These macros are generated automatically by running our compiler evaluation script from the `case-study` directory.
To see the macros, inspect the generated files in `compiler-testing-results` (the folder is re-generated each time the script is run).

To run the experiment:

```
cd case-study
./compiler_experiment.sh
```

To see the results and see the results to fill the table, simply execute:

```
cat compiler-testing-results/c_lang_*.txt
```

## Continuous JavaScript generation

The `js_lang` example evolves ordinary JavaScript scripts with NSGA-II. It uses
`tree-sitter` and `tree-sitter-javascript` for parsing. On macOS it also embeds
the system WebKit JavaScriptCore and executes accepted programs in-process. Node.js
is not required. Other platforms default to the `JSC` environment variable or
`jsc` on `PATH`.

```sh
RUSTFLAGS="-Znext-solver" cargo run --release -p fandango-targets --features javascript --example js_lang
```

This runs until interrupted. For a reproducible, bounded run, append
`-- --seed 42 --generations 100`, or use `-- --seconds 30`. Limits are checked
between generations, including initial population creation in elapsed time.
Append `-- --help` to see all settings.

Embedded execution creates a fresh global context per program inside one shared
VM. JavaScriptCore's watchdog enforces the default 250 ms timeout. Before each
evaluation, `js-crashes/last-input.js` is overwritten with the active input; if
JavaScriptCore crashes, the harness terminates and that file is the reproducer.
Uncaught exceptions and watchdog timeouts are expected results.

Use `--jsc PATH` to run a custom or sanitizer-enabled shell in an isolated process
instead. This slower mode can classify signals, sanitizer reports, WebKit
assertions, and fatal exits, then save unique `.js` reproducers and `.txt` reports.
Stderr is retained up to 64 KiB. `--jsc-arg` adds shell options,
`--exec-timeout-ms` changes the limit, `--crash-dir` moves artifacts, and
`--no-exec` disables execution.

The grammar includes `let`/`const`, expressions, functions and calls, objects,
arrays, conditionals, and for/for-in/for-of/while/do-while loops. It excludes
modules, JSX, `var`, classes, async/generators, labels, and destructuring.
Only crash reproducers are saved; ordinary generated programs are discarded.
Native expressions cover `new`, Map/Set/WeakMap/WeakSet, buffers and typed arrays,
DataView and Atomics, BigInt, Math, Symbol, Proxy/Reflect, Promise, and common
Array/String/Object methods.

Acceptance requires a Tree-sitter tree without error or missing nodes and no
contextual violations: conflicting lexical bindings, missing `const`
initializers, invalid assignment/update targets, returns outside functions, or
break/continue outside loops in the current function. Undefined names and runtime
type errors are allowed. These checks cover the generated subset, not all
ECMAScript early errors. Violating candidates participate in evolution, but are
not counted as accepted.

Each validity penalty and size penalty is a separate objective. The soft size
goals are at most two functions, at least ten statements, and at least one object
literal, array literal, and loop. Size never gates acceptance, and reaching full
validity does not stop evolution. Statement counts include function declarations
and control-flow statements, but exclude blocks and for-header declarations.
For valid candidates, seven independent rewards maximize references to visible
bindings, semantic expression nesting, call expressions, control-flow constructs,
state-changing assignments/updates, and distinct mixed-type operand pairs. The
last two rewards recognize literal/container types and obvious unary or binary
results, then distinct native constructors and methods. These remain separate
NSGA-II objectives, so no fixed score or completion threshold is needed. Invalid
candidates receive zero for each reward.

The harness uses **k-alt-path** grammar coverage from Section IV of
[An analysis of the relationship of input metrics](https://arxiv.org/pdf/2609.11824).
Paths begin at an alternation and end immediately after an alternation's chosen
edge, retaining every intervening node. `k` counts choice edges, not tree depth;
all paths with one through `k` choices are included, including terminal branches.
Separate references to the same nonterminal retain their source positions, so
left and right operands can contribute different paths.
Only rules reachable from `<start>` contribute to the universe. The implementation
enumerates paths without a node-length cutoff and rejects mandatory recursive
cycles without a choice. Repetition and optional operators are also rejected:
use recursive alternatives, as the paper does not define these operators.

`--alt-k 2` is the default; any positive value fitting `usize` is accepted.
The path universe grows exponentially, so large values can exhaust memory.
The harness uses Rust's nightly allocation-error hook to print a warning with
the failed allocation size before aborting. An OS kill cannot run this handler.
Within the final Pareto front, candidates
containing rarer paths are preferred. Rarity counts distinct individuals, not
repeated occurrences within a program, and is recalculated each generation.
Each candidate ranks by its rarest path, with stable ordering for ties. This
selection heuristic is our use of the paper's metric; the paper does not specify
an evolutionary algorithm. It leaves validity and size objectives independent.

Defaults are 100 survivors, 100 offspring per generation, 80% crossover, and 10%
fresh random replacements. The hook limits retained trees to 4,096 nodes,
depth 128, and 65,536 source bytes; generation switches to shortest expansions at
depth 24. Oversized candidates are regenerated, falling back to an empty script.
All these settings and size goals have CLI overrides. The crossover percentage
applies before fresh replacements. Mutation locations are random: Tree-sitter
positions are not Fandango tree paths.

Reports show evaluations/second, syntax passes, acceptance rate, accepted
programs/second, accepted programs meeting all size goals, and mean source size.
`accepted_alt_paths=covered/total` tracks cumulative coverage from accepted
programs, and `novel_accepted` counts accepted evaluations adding at least one
previously unseen path. Duplicate programs add no coverage. The denominator is
the grammar universe; contextual checks and resource bounds can make some paths
unattainable. Coverage is a grammar metric, not target code coverage, and never
gates acceptance or stops the run when saturated.
`accepted_reward_max` reports the best accepted reuse/nesting/call/control-flow/
mutation/mixed-type-pair/native-function counts seen so far, in that order.
Execution counters distinguish clean completion, ordinary exceptions, timeouts,
and, in isolated-process mode, crashes and unique saved crashes. `mean_exec_ms`
measures evaluation time. Execution never changes validity. Ordinary exceptions
add one independent soft fitness penalty; clean exits, timeouts, and crashes add
zero. This guides evolution toward code that runs inside the engine while
retaining hot loops.
Every created candidate is measured once, including initial candidates and
discarded offspring; the counters include duplicate programs. The harness retains
the current population, aggregate counters, and fixed coverage tables, without an
accumulating corpus or source history. The seed printed at startup can reproduce
generation when used with a generation limit and identical settings.

## Navigating this repository

There are a number of individual software packages provided here, summarized below:

- *core*: This is the core typing, parsing, generation, and visitor logic for fandango-rs.
  If you want to know how we prep the type system for generated types, look here.
- *generator* and *derive*: These implement the code generation logic and integration with Rust, respectively.
  The code in generator is a little messy, sorry; it needs a good refactor yet.
- *runtime*: This contains the implementations of (an approximate version of) the original FANDANGO algorithm and NSGA-II.
  There are quite a few type system shenanigans going on to make this work that aren't documented in high detail, sorry again.
- *targets*: These are target-specific visitor implementations which enable the evaluations in sections 4 and 5.
  You may inspect how we implement grammars specifically here.
- *baselines*: This is the benchmarking code for fandango-rs.
  In addition to crates which are specific to each target, we offer two benchmarks, `criterion` and `models`, which benchmark in different ways.
  `criterion` benchmarks were for iterative development, to make sure that we did not worsen performance between versions.
  `models` is what is used in the paper to compare against FANDANGO.
  The script `profile.sh` in this directory runs the Section 4 evaluation.
- *lm3s6965-demo*: Not all the cool things made it into the paper.
  This directory contains an example of running fandango-rs within a baremetal firmware.

## Notice on use of generative AI

This repository was written nearly entirely without AI.
Generative code assistance was used as part of the Section 5 evaluation, but ultimately did not have any specific interesting findings to include in the paper about this.
Files which were in any part produced with generative code assistance are clearly marked at the start of the file.

## Licensing

This crate is licensed under EUPL v1.2.

Some sections of the code are adaptations of [py_literal](https://github.com/jturner314/py_literal/releases/tag/0.4.0),
which is licensed under Apache and MIT, for which license files are provided
in [core/src/py_literal](fandango/src/py_literal).
