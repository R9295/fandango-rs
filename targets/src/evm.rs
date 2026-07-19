//! Target for the EVM bytecode grammar stored in `grammars/evm.fan`.
//!
//! The grammar is a context-free grammar over raw EVM bytecode: every terminal is a
//! byte literal (`b"\xNN"`), so generated inputs are actual bytecode byte streams.
//! It covers all execution opcodes plus structured precompile-call sequences.
//!
//! This grammar carries no semantic constraints, so — unlike the other targets — there
//! is no `ConstraintVisitor`/`ConstraintFixer`; the derived grammar definitions are all
//! that is needed to generate and measure inputs.

#[cfg(not(feature = "static_defs"))]
mod defs {
    use core::convert::Infallible;
    use fandango::Fandango;

    /// Base for the EVM grammar stored in evm.fan.
    #[derive(Fandango)]
    #[fandango(grammar = "grammars/evm.fan", parse = false, dynamic = true)]
    pub struct Evm(Infallible);
}

#[cfg(feature = "static_defs")]
mod defs {
    use core::convert::Infallible;
    use fandango::Fandango;

    /// Base for the EVM grammar stored in evm.fan.
    #[derive(Fandango)]
    #[fandango(grammar = "grammars/evm.fan", parse = false)]
    pub struct Evm(Infallible);
}

pub use defs::*;
