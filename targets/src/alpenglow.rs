//! Target for the Alpenglow votor scenario grammar in `grammars/alpenglow.fan`.
//!
//! Each generated scenario is a JSON input for firedancer's `fuzz_ag_votor` harness
//! (`src/choreo/votor/fuzz_ag_votor.c`): the blocks, the staked nodes (the votor under
//! test signs as the first), and the actions to run.
//!
//! The blocks are hardcoded: sixteen slots, with the root `0` in slot 0 and two competing
//! blocks `Na` and `Nb` in each slot `N` from 1 to 15. They form two forks off the root:
//! `Na` builds on `(N-1)a` and `Nb` on `(N-1)b`. Actions name one of these 30 non-root
//! blocks.
//!
//! The nodes are hardcoded: twenty validators sharing a total stake of 10,000,000.
//! Nodes `0`-`9` are honest (620,000 each, 62% total), nodes `10`-`14` are maybe absent
//! (380,000 each, 19% total) and nodes `15`-`19` are maybe Byzantine (380,000 each, 19%
//! total).
//!
//! This grammar carries no semantic constraints yet, so there is no
//! `ConstraintVisitor`/`ConstraintFixer`; the derived grammar definitions are all that
//! is needed to generate and measure inputs.

#[cfg(not(feature = "static_defs"))]
mod defs {
    use core::convert::Infallible;
    use fandango::Fandango;

    /// Base for the Alpenglow grammar stored in `alpenglow.fan`.
    #[derive(Fandango)]
    #[fandango(grammar = "grammars/alpenglow.fan", parse = false, dynamic = true)]
    pub struct Alpenglow(Infallible);
}

#[cfg(feature = "static_defs")]
mod defs {
    use core::convert::Infallible;
    use fandango::Fandango;

    /// Base for the Alpenglow grammar stored in `alpenglow.fan`.
    #[derive(Fandango)]
    #[fandango(grammar = "grammars/alpenglow.fan", parse = false)]
    pub struct Alpenglow(Infallible);
}

pub use defs::*;
