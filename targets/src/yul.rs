//! Target for the Yul grammar stored in `grammars/yul.fan`.
//!
//! [Yul](https://docs.soliditylang.org/en/latest/yul.html) is Ethereum's intermediate
//! language. The grammar is a context-free grammar over Yul *source text*, translated
//! from the official Yul grammar; repetition and optionals are expressed with right
//! recursion and empty productions.
//!
//! Like the EVM target, this grammar carries no semantic constraints, so there is no
//! `ConstraintVisitor`/`ConstraintFixer`; the derived grammar definitions are all that
//! is needed to generate and measure inputs.

#[cfg(not(feature = "static_defs"))]
mod defs {
    use core::convert::Infallible;
    use fandango::Fandango;

    /// Base for the Yul grammar stored in yul.fan.
    #[derive(Fandango)]
    #[fandango(grammar = "grammars/yul.fan", parse = false, dynamic = true)]
    pub struct Yul(Infallible);
}

#[cfg(feature = "static_defs")]
mod defs {
    use core::convert::Infallible;
    use fandango::Fandango;

    /// Base for the Yul grammar stored in yul.fan.
    #[derive(Fandango)]
    #[fandango(grammar = "grammars/yul.fan", parse = false)]
    pub struct Yul(Infallible);
}

pub use defs::*;
