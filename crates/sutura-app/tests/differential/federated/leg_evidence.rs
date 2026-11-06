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
/// owes a venue that ran one. A cell rather than a sentence, so
/// registering a further leg-executing adapter REDDENS here and the diff that enrols it arrives
/// beside the registration. `sutura-conformance`'s binding holds the per-adapter agreement between
/// the tag and the constant; what this holds is the SET, and now the evidence each member claims.
///
/// **`LegEvidence` is typed rather than prose, and `github.com/telekom/sutura#919`'s reasoning in
/// `crates/sutura-app/tests/golden/dialects.rs` is why.** What it replaces was the sentence *every
/// entry in `LEG_EXECUTING` owes this file one [pass]* beside a list of bare names: true for the
/// first two entries, and a claim no mechanism could keep true once a third arrived whose venue is
/// in another crate. An entry declaring execution that never runs is exactly
/// `github.com/telekom/sutura#920`'s defect, so the venue is a file this cell holds the tree to.
///
/// **A render-only arm was here until `oracle` acquired a venue**, and it was held by asserting
/// `available()` was `false`. It had no member left, so it went rather than stayed as a variant
/// nothing constructs; the next adapter that declares execution before any venue runs it needs it
/// back, with that assertion.
#[derive(Debug, Clone, Copy)]
enum LegEvidence {
    /// A venue in this workspace ran a leg on this adapter, in the file named - asserted to EXIST
    /// and to be non-empty, so a moved or emptied venue reddens instead of leaving a citation.
    ///
    /// **Does not reach whether that venue ran HERE**, nor whether it is a real data system:
    /// `datafusion` and `duckdb` are this file's own two passes, `postgres` and `clickhouse`
    /// conformance bindings against the tiers `nix/postgres-tier.nix` and `nix/clickhouse-tier.nix`
    /// provision, `bigquery` the canned transport its pack binds - a fake standing in for a
    /// dataset, whose own header says so - and `oracle` the two-fact walk in the `oracle-tier` job
    /// alone. Asserting anything about `available()` here would redden a host whose venue comes up,
    /// which is not a defect.
    Executed {
        /// The file whose cells execute a leg on this adapter.
        at: &'static str,
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
            at: "crates/sutura-app/tests/differential/federated/harness.rs",
        },
    ),
    (
        "postgres",
        LegEvidence::Executed {
            at: "crates/sutura-exec-postgres/tests/conformance.rs",
        },
    ),
    (
        "clickhouse",
        LegEvidence::Executed {
            at: "crates/sutura-exec-clickhouse/tests/conformance.rs",
        },
    ),
    // The two-fact walk, in the `oracle-tier` job alone: `SUTURA_ORACLE_TIER` brings the compose
    // `oracle` profile in, and the walk prints `oracle` among the entries it compared.
    (
        "oracle",
        LegEvidence::Executed {
            at: "crates/sutura-app/tests/differential/federated/two_fact.rs",
        },
    ),
    // The canned pack, because it is what every gate runs. A `bigquery` leg also executes live:
    // `federated/two_fact.rs`'s, on the dataset the `bigquery-conformance` job provisions, in that
    // job alone, under its one shared CI identity, with both sources on that one dataset.
    (
        "bigquery",
        LegEvidence::Executed {
            at: "crates/sutura-exec-bigquery/tests/conformance.rs",
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

/// Holds one declaration against the tree, and says whether its venue answers HERE.
///
/// **`available` is reported and never asserted**: a host whose venue comes up is not a defect, and
/// the line is what shows a run's reader which entries' evidence this run could have exercised.
fn hold(name: &str, evidence: LegEvidence, available: bool) {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let LegEvidence::Executed { at: cited } = evidence;
    let size = std::fs::metadata(root.join(cited))
        .unwrap_or_else(|cause| panic!("{name} cites {cited} as its leg evidence and it did not read: {cause}"))
        .len();
    assert!(size > 0, "{name} cites {cited} as its leg evidence and that file is empty");
    let venue = if available { "answers" } else { "does not answer" };
    eprintln!("leg evidence: {name} cites {cited}; its venue {venue} here");
}

/// One cell over the registry rather than one per entry, so the assertion sits in the cell's own fn:
/// a failure panics at the `registered!` line below, inside it, which is where a claim's killing
/// mutation is read.
#[test]
fn whether_it_can_run_a_leg_is_what_this_differential_can_use_it_for() {
    use crate::adapters::DataSystemUnderTest;
    use sutura_domain::warehouse::Warehouse;

    macro_rules! leg_capability {
        ($name:ident, $adapter:ty) => {{
            let name = <$adapter as DataSystemUnderTest>::NAME;
            let declared = leg_evidence(name);
            assert_eq!(
                <$adapter as Warehouse>::EXECUTES_LEGS,
                declared.is_some(),
                "{name} changed its leg capability; \
                 crates/sutura-app/tests/differential/federated/leg_evidence.rs is where a \
                 leg-executing adapter declares where its executed leg evidence is, and an \
                 adapter that declares the constant owes an entry"
            );
            if let Some(evidence) = declared {
                hold(name, evidence, <$adapter as DataSystemUnderTest>::available());
            }
        }};
    }
    crate::adapters::registered!(data_systems: leg_capability);
}
