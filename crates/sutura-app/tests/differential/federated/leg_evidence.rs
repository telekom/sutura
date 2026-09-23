//! Which registered data systems can run a leg, and where each one's leg evidence is.
//!
//! **Its own file because `federated.rs` reached `cargo xtask max-lines`' thousand-line cap**, and
//! the seam is a concept rather than a page number: everything here is about the leg-capability
//! DECLARATION - the set, each member's evidence and the checks that hold both against the tree -
//! while the file it left executes two-source answers. Nothing in it points outward: it reads
//! `crate::adapters`, the same registry `federated.rs` reads, and `federated.rs` does not call in.

/// **Which registered data systems can run a leg, expanded over the registry itself, and where
/// each one's leg evidence is.**
///
/// An entry that cannot run a leg cannot be either half of a federated answer, so every entry here
/// owes a venue that ran one - or says plainly that it has none. A cell rather than a sentence, so
/// registering a further leg-executing adapter REDDENS here and the diff that enrols it arrives
/// beside the registration. `sutura-conformance`'s binding holds the per-adapter agreement between
/// the tag and the constant; what this holds is the SET, and now the evidence each member claims.
///
/// **`LegEvidence` is typed rather than prose, and `github.com/telekom/sutura#919`'s reasoning in
/// `crates/sutura-app/tests/golden/dialects.rs` is why.** What it replaces was the sentence *every
/// entry in `LEG_EXECUTING` owes this file one [pass]* beside a list of bare names: true for the
/// first two entries, and a claim no mechanism could keep true once a third arrived whose venue is
/// in another crate and a fourth that has no venue at all. An entry declaring execution that never
/// runs is exactly `github.com/telekom/sutura#920`'s defect, so [`Self::RenderedOnly`] is a value
/// here and not a footnote.
#[derive(Debug, Clone, Copy)]
enum LegEvidence {
    /// A venue in this workspace ran a leg on this adapter, in the file named - asserted to EXIST
    /// and to be non-empty, so a moved or emptied venue reddens instead of leaving a citation.
    ///
    /// **Does not reach whether that venue ran HERE.** `datafusion` and `duckdb` are this file's
    /// own two passes; `postgres` is a conformance binding in its own crate against the tier
    /// `nix/postgres-tier.nix` provisions. Nothing here reads the cited file's contents, and
    /// nothing asserts `DataSystemUnderTest::available()` for this arm - a networked adapter's
    /// availability is a property of the machine, so asserting it would redden a developer host
    /// with no tier rather than a defect.
    Executed {
        /// The file whose cells execute a leg on this adapter.
        at: &'static str,
    },
    /// The declaration is a RENDERING and nothing more: no venue any gate reaches can open this
    /// adapter at all, so `DataSystemUnderTest::available()` is `false` unconditionally.
    ///
    /// **That is what this arm is checked against**, which is what makes it expire: an adapter
    /// that acquires a venue starts answering `available()` truthfully and reddens here until its
    /// entry moves to [`Self::Executed`].
    RenderedOnly {
        /// The file whose own header states what this adapter cannot prove. Asserted to exist.
        stated_in: &'static str,
    },
}

/// Every registered data system that declares [`Warehouse::EXECUTES_LEGS`], and its evidence.
///
/// [`Warehouse::EXECUTES_LEGS`]: sutura_domain::warehouse::Warehouse::EXECUTES_LEGS
const LEG_EXECUTING: &[(&str, LegEvidence)] = &[
    (
        "datafusion",
        LegEvidence::Executed {
            at: "crates/sutura-app/tests/differential/federated.rs",
        },
    ),
    (
        "duckdb",
        LegEvidence::Executed {
            at: "crates/sutura-app/tests/differential/federated.rs",
        },
    ),
    (
        "postgres",
        LegEvidence::Executed {
            at: "crates/sutura-exec-postgres/tests/conformance.rs",
        },
    ),
    (
        "oracle",
        LegEvidence::RenderedOnly {
            stated_in: "crates/sutura-exec-oracle/src/lib.rs",
        },
    ),
];

/// What this registry declares about one data system, or `None` where it declares nothing.
fn leg_evidence(name: &str) -> Option<LegEvidence> {
    LEG_EXECUTING
        .iter()
        .find(|(declared, _)| *declared == name)
        .map(|&(_, evidence)| evidence)
}

/// Holds one declaration against the tree.
fn hold(name: &str, evidence: LegEvidence, available: bool) {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let (cited, expect_absent_venue) = match evidence {
        LegEvidence::Executed { at } => (at, false),
        LegEvidence::RenderedOnly { stated_in } => (stated_in, true),
    };
    let path = root.join(cited);
    let size = std::fs::metadata(&path)
        .unwrap_or_else(|cause| panic!("{name} cites {cited} as its leg evidence and it did not read: {cause}"))
        .len();
    assert!(size > 0, "{name} cites {cited} as its leg evidence and that file is empty");
    if expect_absent_venue {
        assert!(
            !available,
            "{name} is declared render-only for a leg, and `DataSystemUnderTest::available()` says a \
             venue answered: move its `LEG_EXECUTING` entry to `LegEvidence::Executed` and name the \
             file whose cells run the leg"
        );
    }
}

macro_rules! leg_capability {
    ($name:ident, $adapter:ty) => {
        mod $name {
            use crate::adapters::DataSystemUnderTest;
            use sutura_domain::warehouse::Warehouse;

            #[test]
            fn whether_it_can_run_a_leg_is_what_this_differential_can_use_it_for() {
                let name = <$adapter as DataSystemUnderTest>::NAME;
                let declared = super::leg_evidence(name);
                assert_eq!(
                    <$adapter as Warehouse>::EXECUTES_LEGS,
                    declared.is_some(),
                    "{name} changed its leg capability; \
                     crates/sutura-app/tests/differential/federated/leg_evidence.rs is where a \
                     leg-executing adapter declares where its executed leg evidence is, and an \
                     adapter that declares the constant owes an entry"
                );
                if let Some(evidence) = declared {
                    super::hold(name, evidence, <$adapter as DataSystemUnderTest>::available());
                }
            }
        }
    };
}

crate::adapters::registered!(data_systems: leg_capability);
