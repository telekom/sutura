//! Which registered data system may hold a leg, expanded over the registry itself.
//!
//! Its own module because the census is its own concept - a fact about the REGISTRY, where the rest
//! of `federated.rs` is a comparison of two answers - and because that file sat at 999 lines of the
//! unexemptable 1000-line cap, one line from refusing the next measurement anyone adds to it. A
//! file at the cap is a missing abstraction rather than a file to halve.
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

macro_rules! leg_capability {
    ($name:ident, $adapter:ty) => {
        mod $name {
            use crate::adapters::DataSystemUnderTest;
            use sutura_domain::warehouse::Warehouse;

            #[test]
            fn whether_it_can_run_a_leg_is_what_this_differential_can_use_it_for() {
                let name = <$adapter as DataSystemUnderTest>::NAME;
                assert_eq!(
                    <$adapter as Warehouse>::EXECUTES_LEGS,
                    super::LEG_EXECUTING.contains(&name),
                    "{name} changed its leg capability; \
                     crates/sutura-app/tests/differential/federated.rs is where a leg-executing \
                     adapter gets enrolled in a two-source pass, and every entry in \
                     `LEG_EXECUTING` owes this file one"
                );
            }
        }
    };
}

crate::adapters::registered!(data_systems: leg_capability);
