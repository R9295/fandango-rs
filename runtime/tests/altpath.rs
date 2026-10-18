//! Coverage and selection checks using concrete derivation trees.

#![allow(missing_docs)] // Generated fixture types.

extern crate alloc;

use core::num::NonZeroUsize;
use fandango::Fandango;
use fandango::typing::Structured;
use fandango::visitor::Visitor;
use fandango::visitor::altpath::{AltPathUpdate, AltPaths};
use fandango_runtime::evolvers::basic::BasicIndividual;
use fandango_runtime::evolvers::multi::{AltPathDiversityHook, Nsga2Hook};
use fandango_runtime::population::Individual;

#[derive(Fandango)]
#[fandango(grammar = "tests/grammars/altpath.fan")]
struct Grammar;

fn table(k: usize) -> AltPaths {
    AltPaths::new::<Type<'_>>(
        NonZeroUsize::new(k).unwrap(),
        nonterminal_start::ROOT.inner(),
    )
}

#[test]
fn exact_coverage_ignores_unreachable_rules_and_duplicate_occurrences() {
    let mut paths = table(1);
    // Two branches each for items and item. Unreachable x/y are excluded.
    assert_eq!(paths.alt_paths(), (4, 4));
    let a = Grammar::extract("aaaa").unwrap();
    assert_eq!(paths.paths_in(&a).len(), 3);
    assert_eq!(paths.cover(&a), 3);
    assert_eq!(paths.cover(&a), 0);
    assert_eq!(paths.alt_paths(), (1, 4));
    assert!(paths.lookup().values().all(|&count| count <= 1));
    assert_eq!(paths.cover(&Grammar::extract("b").unwrap()), 1);
    assert_eq!(paths.alt_paths(), (0, 4));
    paths.clear();
    assert_eq!(paths.alt_paths(), (4, 4));
}

#[test]
fn occurrence_updates_can_be_reversed() {
    let mut paths = table(2);
    let input = Grammar::extract("aaba").unwrap();
    let total = paths.lookup().len();
    let distinct = paths.paths_in(&input).len();
    assert!(distinct > 4);
    AltPathUpdate::inserting(&mut paths)
        .visit(&input, 0)
        .unwrap();
    assert_eq!(paths.alt_paths(), (total - distinct, total));
    assert!(paths.lookup().values().any(|&count| count > 1));
    AltPathUpdate::removing(&mut paths)
        .visit(&input, 0)
        .unwrap();
    assert_eq!(paths.alt_paths(), (total, total));
}

fn sort(
    hook: &mut AltPathDiversityHook<()>,
    inputs: &[&str],
) -> alloc::vec::Vec<nonterminal_start> {
    let mut front = inputs
        .iter()
        .map(|input| BasicIndividual::new(Grammar::extract(input).unwrap(), 0usize))
        .collect::<alloc::vec::Vec<_>>();
    <AltPathDiversityHook<()> as Nsga2Hook<_, (), ()>>::diversity_sort(hook, &mut front);
    front
        .into_iter()
        .map(|individual| individual.node().clone())
        .collect()
}

#[test]
fn diversity_includes_terminals_and_refreshes_per_input_frequencies() {
    let mut hook = AltPathDiversityHook::new((), NonZeroUsize::new(1).unwrap());
    // b is present in only one input, despite occurring four times there.
    let sorted = sort(&mut hook, &["a", "aa", "bbbb"]);
    assert_eq!(sorted[0], Grammar::extract("bbbb").unwrap());
    // Reusing the same hook must refresh frequencies, not retain old rarity.
    let sorted = sort(&mut hook, &["b", "bb", "aaaa"]);
    assert_eq!(sorted[0], Grammar::extract("aaaa").unwrap());
    assert_eq!(sort(&mut hook, &[]).len(), 0);
    assert_eq!(sort(&mut hook, &[""]).len(), 1);
}

mod positions {
    use core::num::NonZeroUsize;
    use fandango::Fandango;
    use fandango::typing::Structured;
    use fandango::visitor::altpath::AltPaths;

    #[derive(Fandango)]
    #[fandango(grammar = "tests/grammars/altpath-positions.fan")]
    struct Positions;

    #[test]
    fn distinguishes_references_to_the_same_nonterminal() {
        let mut paths = AltPaths::new::<Type<'_>>(
            NonZeroUsize::new(2).unwrap(),
            nonterminal_start::ROOT.inner(),
        );
        // Four one-choice paths, plus four combinations of pair/item choices:
        // pair -> left a/b and pair -> right a/b.
        assert_eq!(paths.alt_paths(), (8, 8));
        let ab = Positions::extract("ab").unwrap();
        let ba = Positions::extract("ba").unwrap();
        assert_eq!(paths.paths_in(&ab).len(), 5);
        assert_eq!(paths.cover(&ab), 5);
        assert_eq!(paths.cover(&ba), 2);
        assert_eq!(paths.cover(&Positions::extract("x").unwrap()), 1);
        assert_eq!(paths.alt_paths(), (0, 8));
    }
}
