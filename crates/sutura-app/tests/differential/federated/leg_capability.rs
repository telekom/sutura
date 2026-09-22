//! Which registered data system may hold a leg, expanded over the registry itself.
//!
//! Its own module because the census is its own concept - a fact about the REGISTRY, where the rest
//! of `federated.rs` is a comparison of two answers - and because that file reached the 1000-line
//! cap when `telekom/sutura#929`'s leg landed beside the request-deadline measurement. A file at
//! the cap is a missing abstraction rather than a file to halve.
//!
//! The two passes each `true` entry owes are in the parent module:
//! [`a_two_source_answer_is_the_same_answer_as_one_source`](super::a_two_source_answer_is_the_same_answer_as_one_source)
//! and [`two_engines_answer_what_one_engine_answers`](super::two_engines_answer_what_one_engine_answers).

/// **Which registered data systems can run a leg, expanded over the registry itself, and this file
/// has a two-source pass for each of them.**
///
/// An entry that cannot run a leg cannot be either half of a federated answer, so every `true` here
/// owes this file a pass: `DuckDB` and the engine each have
/// one, named in this module's own header. A cell rather than a sentence, so
/// registering a THIRD leg-executing adapter REDDENS here. `sutura-conformance`'s binding holds the
/// per-adapter agreement between the tag and the constant; what this holds is the SET.
///
/// **A list of names rather than one comparison**, which is the correction the second entry
/// earned: written as `name == "duckdb"` the assertion had nowhere for a second adapter to go
/// except a boolean expression that grows.
///
/// [`Warehouse::EXECUTES_LEGS`]: sutura_domain::warehouse::Warehouse::EXECUTES_LEGS
const LEG_EXECUTING: &[&str] = &["datafusion", "duckdb"];

/// Declares the capability and owes this file NO pass, because there is nothing here to EXECUTE it
/// against - `telekom/sutura#929`. `sutura-exec-bigquery` declares `EXECUTES_LEGS` now and is the
/// one adapter whose two-source answer can put a SUBJECT on each half, but `available()` is
/// unconditionally `false` and `crate::adapters::NoLocalTier` answers `NeverAsked`, so a name in
/// [`LEG_EXECUTING`] would promise a pass that cannot exist - the cell below CHECKS that, so an
/// entry acquiring a venue reddens. Its leg path is held in its own crate instead:
/// `crates/sutura-exec-bigquery/tests/conformance.rs` binds it to `a_leg_is_executed`.
const LEG_EXECUTING_WITH_NO_VENUE: &[&str] = &["bigquery"];

macro_rules! leg_capability {
    ($name:ident, $adapter:ty) => {
        mod $name {
            use crate::adapters::DataSystemUnderTest;
            use sutura_domain::warehouse::Warehouse;

            #[test]
            fn whether_it_can_run_a_leg_is_what_this_differential_can_use_it_for() {
                let name = <$adapter as DataSystemUnderTest>::NAME;
                let exempt = super::LEG_EXECUTING_WITH_NO_VENUE.contains(&name);
                assert_eq!(
                    <$adapter as Warehouse>::EXECUTES_LEGS,
                    super::LEG_EXECUTING.contains(&name) || exempt,
                    "{name} changed its leg capability; \
                     crates/sutura-app/tests/differential/federated.rs is where a leg-executing \
                     adapter gets enrolled in a two-source pass, and every entry in \
                     `LEG_EXECUTING` owes this file one"
                );
                assert!(
                    !exempt || !<$adapter as DataSystemUnderTest>::available(),
                    "{name} is exempt because it cannot execute here, and it CAN: move it to \
                     `LEG_EXECUTING` and give it a pass"
                );
            }
        }
    };
}

crate::adapters::registered!(data_systems: leg_capability);
