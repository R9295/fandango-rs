//! Target for the Alpenglow vote grammar in `grammars/alpenglow.fan`.
//!
//! Each generated scenario has ten validators with 10% stake each. Validators `v0` and
//! `v1` are Byzantine (20% total), while `v2` and `v3` are potentially absent (20%
//! total). Active validators can emit one or more votes; independently generated list
//! elements deliberately permit duplicate votes.

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
