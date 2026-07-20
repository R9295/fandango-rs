//! Target for the Alpenglow vote grammar in `grammars/alpenglow.fan`.
//!
//! Each generated scenario has twenty validators with 5% stake each. Validators `v0`
//! through `v3` are Byzantine (20% total), while `v4` through `v7` are potentially absent
//! (20% total). Active validators can emit one or more votes; independently generated
//! list elements deliberately permit duplicate votes. [`CertificateConstraintVisitor`]
//! checks that the distinct voting stake produces at least one Alpenglow certificate.

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
    use alloc::collections::VecDeque;
    use alloc::vec;
    use alloc::vec::Vec;
    use core::convert::Infallible;
    use core::fmt;
    use core::ops::ControlFlow;
    use fandango::Fandango;
    use fandango::typing::{AsNodeRef, Downcast, Node, Opaque};
    use fandango::visitor::write::WriteVisitor;
    use fandango::visitor::{VisitResult, VisitableChildren, Visitor};
    use fandango_runtime::measurement::Violations;
    use fandango_runtime::operators::Checker;
    use num_rational::Ratio;

    /// Base for the Alpenglow grammar stored in `alpenglow.fan`.
    #[derive(Fandango)]
    #[fandango(grammar = "grammars/alpenglow.fan", parse = false)]
    pub struct Alpenglow(Infallible);

    /// One of the two competing blocks in slot `s0`.
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub enum Block {
        /// Block `b1`.
        B1,
        /// Block `b2`.
        B2,
    }

    impl fmt::Display for Block {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                Self::B1 => f.write_str("b1"),
                Self::B2 => f.write_str("b2"),
            }
        }
    }

    /// A certificate derivable from the validators' distinct signed votes.
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub enum CertificateKind {
        /// At least 60% stake cast `notarize` for the block.
        Notarize(Block),
        /// At least 60% stake cast `notarize` or `notarize-fallback` for the block.
        NotarizeFallback(Block),
        /// At least 80% stake cast `notarize` for the block.
        FastFinalize(Block),
        /// At least 60% stake cast `skip` or `skip-fallback` for slot `s0`.
        Skip,
    }

    impl fmt::Display for CertificateKind {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                Self::Notarize(block) => write!(f, "NotarizeCert({block})"),
                Self::NotarizeFallback(block) => {
                    write!(f, "NotarizeFallbackCert({block})")
                }
                Self::FastFinalize(block) => write!(f, "FastFinalizeCert({block})"),
                Self::Skip => f.write_str("SkipCert(s0)"),
            }
        }
    }

    /// A generated certificate and the distinct stake supporting it.
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub struct Certificate {
        kind: CertificateKind,
        stake_percent: usize,
    }

    impl Certificate {
        /// The certificate type and its block or slot.
        #[must_use]
        pub const fn kind(&self) -> CertificateKind {
            self.kind
        }

        /// Supporting distinct stake as an integer percentage.
        #[must_use]
        pub const fn stake_percent(&self) -> usize {
            self.stake_percent
        }
    }

    #[derive(Clone, Copy, Debug, Default)]
    struct ValidatorVotes {
        notarize: [bool; 2],
        notarize_fallback: [bool; 2],
        skip: bool,
        skip_fallback: bool,
    }

    const VALIDATOR_COUNT: usize = 20;
    const STAKE_PERCENT: usize = 5;

    fn certificates_for(votes: &[ValidatorVotes; VALIDATOR_COUNT]) -> Vec<Certificate> {
        let mut certificates = Vec::new();
        for (block_index, block) in [(0, Block::B1), (1, Block::B2)] {
            let notarize = votes
                .iter()
                .filter(|validator| validator.notarize[block_index])
                .count()
                * STAKE_PERCENT;
            let notarize_fallback = votes
                .iter()
                .filter(|validator| {
                    validator.notarize[block_index] || validator.notarize_fallback[block_index]
                })
                .count()
                * STAKE_PERCENT;

            if notarize >= 60 {
                certificates.push(Certificate {
                    kind: CertificateKind::Notarize(block),
                    stake_percent: notarize,
                });
            }
            if notarize_fallback >= 60 {
                certificates.push(Certificate {
                    kind: CertificateKind::NotarizeFallback(block),
                    stake_percent: notarize_fallback,
                });
            }
            if notarize >= 80 {
                certificates.push(Certificate {
                    kind: CertificateKind::FastFinalize(block),
                    stake_percent: notarize,
                });
            }
        }

        let skip = votes
            .iter()
            .filter(|validator| validator.skip || validator.skip_fallback)
            .count()
            * STAKE_PERCENT;
        if skip >= 60 {
            certificates.push(Certificate {
                kind: CertificateKind::Skip,
                stake_percent: skip,
            });
        }
        certificates
    }

    /// Semantic constraint requiring at least one certificate in a generated scenario.
    ///
    /// Duplicate votes from one validator count only once. A `SkipCert` combines `skip`
    /// and `skip-fallback` votes; there is no separate skip-fallback certificate.
    #[derive(Clone, Debug, Default)]
    pub struct CertificateConstraintVisitor {
        votes: [ValidatorVotes; VALIDATOR_COUNT],
        current_validator: Option<usize>,
    }

    impl CertificateConstraintVisitor {
        /// Return every certificate supported by the votes seen so far.
        #[must_use]
        pub fn certificates(&self) -> Vec<Certificate> {
            certificates_for(&self.votes)
        }
    }

    impl Checker for CertificateConstraintVisitor {
        fn violations(self) -> Violations {
            let passed = !self.certificates().is_empty();
            let violations = if passed {
                Vec::new()
            } else {
                vec![VecDeque::new()]
            };
            Violations::new(Ratio::new(usize::from(passed), 1), violations)
        }
    }

    impl<T> Visitor<T> for CertificateConstraintVisitor
    where
        T: VisitableChildren<T>
            + AsNodeRef<nonterminal_byzantine_v0>
            + AsNodeRef<nonterminal_byzantine_v1>
            + AsNodeRef<nonterminal_byzantine_v2>
            + AsNodeRef<nonterminal_byzantine_v3>
            + AsNodeRef<nonterminal_potentially_absent_v4>
            + AsNodeRef<nonterminal_potentially_absent_v5>
            + AsNodeRef<nonterminal_potentially_absent_v6>
            + AsNodeRef<nonterminal_potentially_absent_v7>
            + AsNodeRef<nonterminal_honest_v8>
            + AsNodeRef<nonterminal_honest_v9>
            + AsNodeRef<nonterminal_honest_v10>
            + AsNodeRef<nonterminal_honest_v11>
            + AsNodeRef<nonterminal_honest_v12>
            + AsNodeRef<nonterminal_honest_v13>
            + AsNodeRef<nonterminal_honest_v14>
            + AsNodeRef<nonterminal_honest_v15>
            + AsNodeRef<nonterminal_honest_v16>
            + AsNodeRef<nonterminal_honest_v17>
            + AsNodeRef<nonterminal_honest_v18>
            + AsNodeRef<nonterminal_honest_v19>
            + AsNodeRef<nonterminal_notarize>
            + AsNodeRef<nonterminal_notarize_fallback>
            + AsNodeRef<nonterminal_skip>
            + AsNodeRef<nonterminal_skip_fallback>,
    {
        type Continue = Self;
        type Break = Infallible;
        type Error = Infallible;

        fn visit<'program, N>(mut self, node: &'program N, _idx: usize) -> VisitResult<Self, T>
        where
            N: Node<Type<'program> = T>,
            T: From<&'program N> + AsNodeRef<N>,
        {
            let visited = node.opaque();
            let previous_validator = self.current_validator;
            self.current_validator = if visited.downcast::<nonterminal_byzantine_v0>().is_some() {
                Some(0)
            } else if visited.downcast::<nonterminal_byzantine_v1>().is_some() {
                Some(1)
            } else if visited.downcast::<nonterminal_byzantine_v2>().is_some() {
                Some(2)
            } else if visited.downcast::<nonterminal_byzantine_v3>().is_some() {
                Some(3)
            } else if visited
                .downcast::<nonterminal_potentially_absent_v4>()
                .is_some()
            {
                Some(4)
            } else if visited
                .downcast::<nonterminal_potentially_absent_v5>()
                .is_some()
            {
                Some(5)
            } else if visited
                .downcast::<nonterminal_potentially_absent_v6>()
                .is_some()
            {
                Some(6)
            } else if visited
                .downcast::<nonterminal_potentially_absent_v7>()
                .is_some()
            {
                Some(7)
            } else if visited.downcast::<nonterminal_honest_v8>().is_some() {
                Some(8)
            } else if visited.downcast::<nonterminal_honest_v9>().is_some() {
                Some(9)
            } else if visited.downcast::<nonterminal_honest_v10>().is_some() {
                Some(10)
            } else if visited.downcast::<nonterminal_honest_v11>().is_some() {
                Some(11)
            } else if visited.downcast::<nonterminal_honest_v12>().is_some() {
                Some(12)
            } else if visited.downcast::<nonterminal_honest_v13>().is_some() {
                Some(13)
            } else if visited.downcast::<nonterminal_honest_v14>().is_some() {
                Some(14)
            } else if visited.downcast::<nonterminal_honest_v15>().is_some() {
                Some(15)
            } else if visited.downcast::<nonterminal_honest_v16>().is_some() {
                Some(16)
            } else if visited.downcast::<nonterminal_honest_v17>().is_some() {
                Some(17)
            } else if visited.downcast::<nonterminal_honest_v18>().is_some() {
                Some(18)
            } else if visited.downcast::<nonterminal_honest_v19>().is_some() {
                Some(19)
            } else {
                self.current_validator
            };

            if let Some(validator) = self.current_validator {
                if visited.downcast::<nonterminal_skip>().is_some() {
                    self.votes[validator].skip = true;
                } else if visited.downcast::<nonterminal_skip_fallback>().is_some() {
                    self.votes[validator].skip_fallback = true;
                } else if let Some(vote) = visited.downcast::<nonterminal_notarize>() {
                    let bytes = WriteVisitor::new(Vec::new())
                        .visit(vote, 0)?
                        .continue_value()
                        .unwrap()
                        .output();
                    let block = usize::from(bytes.as_slice() == b"notarize(b2)");
                    self.votes[validator].notarize[block] = true;
                } else if let Some(vote) = visited.downcast::<nonterminal_notarize_fallback>() {
                    let bytes = WriteVisitor::new(Vec::new())
                        .visit(vote, 0)?
                        .continue_value()
                        .unwrap()
                        .output();
                    let block = usize::from(bytes.as_slice() == b"notarize-fallback(b2)");
                    self.votes[validator].notarize_fallback[block] = true;
                }
            }

            let Ok(ControlFlow::Continue(mut visitor)) = visited.visit_each(self);
            visitor.current_validator = previous_validator;
            Ok(ControlFlow::Continue(visitor))
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn certificate_thresholds_count_distinct_validators() {
            let mut votes = [ValidatorVotes::default(); VALIDATOR_COUNT];
            for validator in &mut votes[..11] {
                validator.notarize[0] = true;
            }
            assert!(certificates_for(&votes).is_empty());

            votes[11].notarize[0] = true;
            let certificates = certificates_for(&votes);
            assert!(certificates.iter().any(|certificate| {
                certificate.kind == CertificateKind::Notarize(Block::B1)
                    && certificate.stake_percent == 60
            }));

            for validator in &mut votes[12..16] {
                validator.notarize[0] = true;
            }
            assert!(certificates_for(&votes).iter().any(|certificate| {
                certificate.kind == CertificateKind::FastFinalize(Block::B1)
                    && certificate.stake_percent == 80
            }));
        }

        #[test]
        fn fallback_vote_types_combine_into_their_certificates() {
            let mut votes = [ValidatorVotes::default(); VALIDATOR_COUNT];
            for validator in &mut votes[..6] {
                validator.notarize_fallback[1] = true;
                validator.skip = true;
            }
            for validator in &mut votes[6..12] {
                validator.notarize[1] = true;
                validator.skip_fallback = true;
            }

            let certificates = certificates_for(&votes);
            assert!(certificates.iter().any(|certificate| {
                certificate.kind == CertificateKind::NotarizeFallback(Block::B2)
                    && certificate.stake_percent == 60
            }));
            assert!(certificates.iter().any(|certificate| {
                certificate.kind == CertificateKind::Skip && certificate.stake_percent == 60
            }));
        }
    }
}

pub use defs::*;
