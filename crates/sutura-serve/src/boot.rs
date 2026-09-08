//! The refusals this root makes by READING the bundle, gathered in one file.
//!
//! **Moved out of `main.rs` by the file-length gate, and the cut is at a seam rather than at a line
//! count.** Everything here answers one shape of question - *does the bundle this process is about to
//! serve hold together against what the deployment declared and against what the data systems
//! actually have* - and none of it opens a socket, builds a runtime or touches the settings tree
//! beyond the source registry it is handed. `main.rs` keeps the ORDER these run in, which is the part
//! a reader of a composition root is looking for.
//!
//! The two that were already here refuse a bundle whose anchors cannot be verified and a bundle
//! that changed between two loads. `refuse_absent_tables` is the third, and it closes an asymmetry
//! rather than adding a rule: see its own documentation.
//!
//! **What it no longer holds is the DECISION, and review is why.** Asking each data system, mapping
//! the port's three answers plus the two shapes of failure, and rendering the absent tables with the
//! models behind them all lived here and were copied verbatim into `sutura-cli`'s own root - review
//! measured `unmatched` as byte-identical and `models_by_table` as identical modulo how the signature
//! wraps. They are `sutura_app::preflight` now, which both roots already depend on for `Warehouses`
//! itself, so the shared home points inward and adds no root-to-root edge. What stays here is the
//! part that is genuinely this root's: the sentence an operator reads and the `tracing` sink a
//! server delivers it through.
//!
//! **`refuse_absent_tables` is behind `#[cfg(feature = "bigquery")]`, and the reason is a build that
//! ships rather than tidiness.** Its only non-test caller is the `bigquery` arm of `main.rs`'s
//! dispatch, so with that default-off feature absent `dead_code = "deny"` makes it a hard error -
//! and `nix/shipped.nix` builds this package with cargo's DEFAULT features, so the published binary,
//! its image and all four cross triples are exactly the configuration that would not compile.
//!
//! **Which gate would have caught it, stated precisely, because the first version of this paragraph
//! said *nothing in this repository* and review measured otherwise - in both directions.** The four
//! `cross` jobs build `.#sutura-serve-<triple>-ci` for every shipped triple on every pull request,
//! and `nix/shipped.nix` passes `--package` and `--target` with no `--features`, i.e. cargo's
//! default set: they WOULD have caught it, and they did not run, because they are `needs: [ci]` and
//! `ci` had already failed. That is a sequencing fact rather than an absence of coverage. And
//! `just validate` would not have caught it either: the checks it runs build no package, and their
//! clippy and nextest both pass `--all-features`. So the honest sentence is *the gates a developer
//! runs before pushing cannot see this lane; the four `cross` link checks can, and they run only
//! after `ci` passes, which is why it reached review.*
//!
//! **The developer half of that lane is a gate now, which is what changes the paragraph above from
//! advice into history.** `just gates` runs `check-default-features`, which reads the shipped package
//! list out of `nix/shipped.nix` and both COMPILES and LINTS each one at cargo's default features -
//! two things `just lint` and `just check-changed` cannot do, because both pass `--all-features`. It
//! is in `gates` rather than `just hygiene` for `check-attribution-current`'s reason: it shells out
//! to cargo, and the sandbox `hygiene` runs in has no registry. **It runs in CI as well now**, as
//! `nix run .#default-features` in the one required job, sharing the warmed target directory the way
//! `apps.causality` does - so the lint half of this lane is a required check and not a developer
//! courtesy. A step is deleted more easily than a job, and `ci` is the only required context here,
//! so the step's own presence is asserted by
//! `default_features::tests::both_lanes_still_invoke_this_gate` rather than left to review.

use std::collections::BTreeSet;

#[cfg(feature = "bigquery")]
use sutura_app::Warehouses;
#[cfg(feature = "bigquery")]
use sutura_app::preflight::Verdict;
use sutura_domain::model::TableName;
use sutura_domain::pinned::PinnedDefinitions;
#[cfg(feature = "bigquery")]
use sutura_domain::warehouse::Warehouse;

#[cfg(feature = "bigquery")]
use crate::flatten;

/// Every table the served bundle's models sit behind.
pub(crate) fn served_tables(served: &PinnedDefinitions) -> BTreeSet<TableName> {
    served
        .definitions()
        .models()
        .values()
        .map(|model| model.table_name().clone())
        .collect()
}

/// The tables the served bundle names, against the tables the engine actually holds.
///
/// Two sets rather than a bundle and a set, so the comparison is unit-testable without a digest, a
/// knowledge declaration and an engine - [`served_tables`] is the other half and is one map over a
/// public accessor.
///
/// Both directions are refused, and the second is not pedantry: a table attached for a model the
/// served bundle no longer names means the catalog directory changed between two loads seconds
/// apart, and whatever else moved with it is the part nobody has looked at.
pub(crate) fn refuse_unattached(serving: &BTreeSet<TableName>, attached: &BTreeSet<TableName>) -> Result<(), String> {
    let missing = names(serving.difference(attached));
    let extra = names(attached.difference(serving));
    if missing.is_empty() && extra.is_empty() {
        return Ok(());
    }
    Err(format!(
        "the catalog changed while this process was starting: the engine was opened for the bundle \
         loaded first, and the bundle being served names different tables. Served with no table \
         attached: [{missing}]. Attached and no longer served: [{extra}]. Refusing to serve a model \
         whose questions would fail at query time"
    ))
}

/// One line of table names, for a message an operator has to act on.
fn names<'table>(tables: impl Iterator<Item = &'table TableName>) -> String {
    tables.map(TableName::as_str).collect::<Vec<&str>>().join(", ")
}

/// Refuses a bundle naming a table the data system behind it does not hold.
///
/// **This is issue 120's asymmetry, closed at the one place that can see both halves.** A `files`
/// deployment is refused already, and not by a check: the engine is GIVEN one file per model, so a
/// model with nothing behind it never opens. A dataset has no attach step - the tables live there -
/// so before this the same mistyped table name produced a green startup, a healthy liveness probe,
/// and a `SurfaceFailure` for whoever asked that metric first. The operator found out from a user.
///
/// **Ordered after the credential and before the listener, and a type pins the first half - though
/// not the type this comment used to name.** It said *this takes an OPEN registry, and only
/// `open_engine` can produce one*, and review disproved that in one line: `Warehouses::of` and
/// `::and` are both `pub`, and this file's own tests build a registry from a fake two hundred lines
/// below. What actually holds the order is `main.rs`'s
/// `type BigQuerySource = BigQueryWarehouse<BigQueryWire<Credential>>`, whose credential-shaped
/// parameter has one implementor - `sutura_exec_bigquery::wire::credential::Credential`, whose only
/// public constructor is `Credential::read`. **The ALIAS and not the warehouse, which is review
/// correcting this same sentence a second time:** `BigQueryWarehouse<T>` is generic in its transport
/// and that adapter's own suite builds twenty-four of them over fakes with no credential in sight -
/// `grep -c '= open(' crates/sutura-exec-bigquery/src/tests.rs`, measured 2026-09-02 - so *a
/// `BigQueryWarehouse` cannot exist without a credential read off disk* was false of the type it
/// named. **Naming the right type matters more than the property:** a later
/// `Credential::from_token` would spend the guarantee while a sentence about registries still read
/// true, which is the third time this repository has caught a doc crediting a type with a property
/// something else holds.
///
/// **A data system whose own listing did not account for itself is a REFUSAL, and it is the third
/// one rather than a shade of the second.** A listing that reports a total and then names fewer
/// tables than the total claims has a gap, and a table the bundle names that is missing from what it
/// DID name may be sitting in it - so this root refuses without saying the table is absent, because
/// the catalog may be entirely right. `telekom/sutura#275` and `docs/adr/0018` carry why it is not
/// the `WARN`: an `Err` out of the port is the warning half, so the one shape the cross-check exists
/// to catch would have ended in a deployment that serves. **The limit next to it:** what it
/// establishes is that the check did not complete, never why - a document whose shape changed and a
/// table created or dropped mid-listing leave the same gap.
///
/// **A data system that could not be ASKED is a WARNING and not a refusal, and that is now narrower
/// than it was.** Review found the earlier version indefensible: a `403` on `tables.list` - a grant
/// an operator can add - and a `503` from a dead endpoint were the same permanent `WARN`, in the
/// deployment least likely to notice, which is a stronger objection than the `skip_preflight` key
/// this function's design argues against. So the two are split by
/// [`Warehouse::preflight_was_refused`](sutura_domain::warehouse::Warehouse::preflight_was_refused),
/// a defaulted predicate on the `result_did_not_fit` pattern: an AUTHORIZATION failure is a refusal
/// naming the grant, and a transport failure is the `WARN`. A deployment that genuinely cannot grant
/// `bigquery.tables.list` is still servable - it just has to say so by not declaring the source, or
/// by granting the one permission this check needs. `banner::announce_token_class` is the precedent
/// for a limit printed at `WARN` on its own line.
///
/// **Two limits.** What a pre-flight establishes is that a table EXISTS: not that the model's columns
/// are on it, and not that a question's identity may read it - an anchor is what covers both, for the
/// metrics that have one. And it reads the bundle loaded FIRST, so a model added to the catalog
/// directory between this root's two loads is caught on a `files` deployment by
/// [`refuse_unattached`] and is not caught here.
///
/// The decision itself is `sutura_app::preflight::ask`; what this function is, is the words and the
/// sink. Its limits are stated there too, because a caller reading the port's answer needs them
/// whichever root it is in.
#[cfg(feature = "bigquery")]
pub(crate) fn refuse_absent_tables<W>(pinned: &PinnedDefinitions, engines: &Warehouses<W>) -> Result<(), String>
where
    W: Warehouse,
{
    for asked in sutura_app::preflight::ask(pinned, engines) {
        let source = asked.source();
        match asked.into_verdict() {
            // **The split review asked for.** An authorization failure is a REFUSAL, because the fix
            // is one IAM grant and a permanent `WARN` hides it in the deployment least likely to
            // read a startup log.
            Verdict::Refused { cause } => {
                return Err(format!(
                    "{source} refused to list the tables the catalog names, so this deployment cannot \
                     tell a mistyped `table:` from a table that is there. Grant the identity this \
                     source is opened with `bigquery.tables.list` on the dataset. The data system \
                     said: {}",
                    flatten(cause)
                ));
            }
            // Everything else that failed - an endpoint that did not answer, a dataset that is not
            // there - is the `WARN`, because a deployment that cannot reach a data system at boot
            // still has to be able to serve when it comes back.
            Verdict::Unverified { asked: tables, cause } => tracing::warn!(
                source = %source,
                tables,
                reason = %flatten(cause),
                "could not verify that this data system holds the tables the catalog names - \
                 serving anyway, so a mistyped table name will fail the first question against it"
            ),
            Verdict::Absent(absent) => {
                return Err(format!(
                    "{source} does not hold {absent}. Refusing to serve a model whose questions would \
                     fail at query time - fix the catalog's `table:`, or create the table"
                ));
            }
            // **A REFUSAL, and deliberately not the `Absent` sentence above.** The data system said
            // it holds more tables than it went on to name, so these are tables it did not answer
            // about rather than tables it said it does not have - and the catalog may be entirely
            // right. Sending an operator to fix a `table:` here is the defect `telekom/sutura#275`
            // is about; the WARN half is the worse direction still, because the one shape the
            // cross-check exists to catch would end in a deployment that serves.
            //
            // **Three numbers, and the third is a clamp rather than a repeat of the first.** The gap
            // BOUNDS how many of these tables it can explain - a shortfall of one beside three
            // unnamed tables means two of them really are missing - so a sentence carrying the set
            // and the gap as one quantity contradicts itself. Review reproduced that, and then
            // reproduced the other direction: a shortfall counts tables missing from the whole
            // dataset while this set is only the part the bundle names, so printing the gap where
            // the bound belongs says *at most 9 of the 2*. `explained_by` is the clamp, in the
            // domain, because two roots each remembering a `min` is a rule held by recall.
            Verdict::Unaccounted { tables, shortfall } => {
                return Err(format!(
                    "{source} did not account for {shortfall} of the table(s) it says it holds, so at most \
                     {explained} of the {count} table(s) the catalog names here may be sitting in that gap \
                     rather than missing: {tables}. Refusing to serve, and NOT reporting them absent - the \
                     catalog may be right and the listing was not whole. Start again; if it persists, this \
                     deployment is not reading the data system's listing the way the data system is \
                     writing it",
                    explained = tables.explained_by(shortfall),
                    count = tables.len()
                ));
            }
            Verdict::Present { asked: tables } => tracing::info!(
                source = %source,
                tables,
                "every table the catalog names is in this data system"
            ),
            // **`NotReported` gets a line of its own, and review is why.** It used to be silent, which
            // made the one outcome meaning *nothing verified this* the only one an operator could not
            // see - and indistinguishable from a source the bundle names no models for. The `files`
            // path never reaches here, so this cannot become a spurious line on the shipped engine.
            Verdict::NotReported { asked: tables } => tracing::info!(
                source = %source,
                tables,
                "this adapter does not report which tables it holds, so nothing here verified them"
            ),
        }
    }
    Ok(())
}

/// Refuses a bundle that declares an anchor on a source with no identity to re-run it under.
///
/// **Not skipped, not warned about, and not treated as a passing anchor** - the three ways this would
/// otherwise become a mode nobody chose. A deployment that genuinely wants an impersonating source with
/// no verification identity gets it by authoring no anchors on that source's metrics, which is a catalog
/// fact a reviewer can see rather than a runtime behaviour they have to infer.
///
/// It names the metric AND the source, because the fix is in one of two different files: either the
/// catalog stops certifying that number, or the source's entry declares the identity that would.
///
/// The identity the check reads is the one the port does not take yet. When `Warehouse` gains a method
/// that runs an anchor under a `VerificationIdentity`, this check stays where it is and stops being the
/// only thing between a declared identity and the one that ran.
pub(crate) fn refuse_unverifiable_anchors(
    pinned: &PinnedDefinitions,
    registry: &sutura_config::SourceRegistry,
) -> Result<(), String> {
    for (metric, _anchor) in pinned.anchored_metrics() {
        let Some(source) = sutura_app::source_of(pinned, metric) else {
            continue;
        };
        let Some(identity) = registry.get(source).and_then(sutura_config::ConfiguredSource::identity) else {
            // Already refused by `configured_source` for every source the catalog names, so there is
            // nothing left to say here and nothing to skip: a source with no declaration never gets
            // this far.
            continue;
        };
        match identity.anchors_run_as() {
            sutura_domain::source::AnchorIdentity::TheSharedIdentity { .. }
            | sutura_domain::source::AnchorIdentity::Declared { .. } => {}
            sutura_domain::source::AnchorIdentity::NoneDeclared => {
                return Err(format!(
                    "metric {metric} declares an anchor and reads from {source}, which is \
                     `impersonation-at-source` with no `sources.{source}.verification_identity`. There \
                     is no identity to re-run that certified number as. Declare one, or remove the \
                     anchor from {metric}"
                ));
            }
        }
    }
    Ok(())
}

/// The pre-flight's own tests.
#[cfg(test)]
mod tests {
    /// Everything that needs the `bigquery` feature, which is everything in here.
    ///
    /// **A nested module rather than a gate on the parent, and neither obvious form works.** With
    /// the feature off there is no `refuse_absent_tables` to call, so a bare `#[cfg(test)]` fails to
    /// compile on the DEFAULT feature set - one configuration over from the defect review caught.
    /// Writing `#[cfg(all(test, feature = "bigquery"))]` on the parent is what a reader reaches for
    /// and it makes every test in here a lint error, because `clippy::tests_outside_test_module` and
    /// `clippy::expect_used` both key on the literal `#[cfg(test)]` attribute; an inner `#![cfg(..)]`
    /// beside a doc comment is `clippy::mixed_attributes_style`. This nests, and trips neither.
    #[cfg(feature = "bigquery")]
    mod preflight {

        use core::cell::RefCell;
        use core::num::NonZeroU64;
        use std::collections::BTreeSet;

        use sutura_app::Warehouses;
        use sutura_domain::identity::Presented;
        use sutura_domain::model::{QualifiedTable, SourceName};
        use sutura_domain::plan::{AnchorPlan, Executable};
        use sutura_domain::source::{ImpersonationCapability, SourcePosture};
        use sutura_domain::warehouse::preflight::{TablesPresent, UnaccountedTables};
        use sutura_domain::warehouse::{AnchorRows, RowSet, Warehouse};

        use crate::boot::refuse_absent_tables;

        /// A data system that answers the pre-flight from what a test handed it, and records the asking.
        ///
        /// **A fake above the port and not a fake transport**, which is what makes every outcome of the
        /// new check reachable here: a real `BigQuery` adapter's host is a `const` and its agent is
        /// `https_only`, so no test in this repository can point one at a loopback. What is under test is
        /// this root's DECISION about each answer, and that is exactly what a `Warehouse` fake exercises.
        struct Answers {
            source: SourceName,
            answer: Answering,
            asked: RefCell<Vec<usize>>,
            /// What this fake says about its own failure - the `preflight_was_refused` half.
            ///
            /// A field rather than a second fake type, because what the root has to get right is that it
            /// asks: two fakes whose `Err` is the same value and which answer this differently is the
            /// only shape that shows the delegation happening.
            refused: bool,
        }

        /// What a test hands the fake to answer a pre-flight with.
        ///
        /// A named function pointer, because the spelled-out type is over the `type_complexity`
        /// threshold this workspace tightened - the same reason `sutura_exec_bigquery`'s `Mapped` exists.
        type Answering = fn(&BTreeSet<QualifiedTable>) -> Result<TablesPresent, CouldNotAsk>;

        /// The one failure this fake can report: the data system could not be asked.
        ///
        /// Written by hand rather than derived, because `sutura-serve` declares no `thiserror`
        /// dependency and `unused-deps` would be the next thing to complain if it did. A binary's test
        /// fake is exactly the case where two impls beat a dependency.
        #[derive(Debug)]
        struct CouldNotAsk;

        impl core::fmt::Display for CouldNotAsk {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                f.write_str("the dataset could not be listed")
            }
        }

        impl core::error::Error for CouldNotAsk {}

        impl Warehouse for Answers {
            type Error = CouldNotAsk;

            const IMPERSONATION: ImpersonationCapability = ImpersonationCapability::NoPlaceForASubject;

            fn source(&self) -> &SourceName {
                &self.source
            }

            fn posture(&self) -> &SourcePosture {
                &SourcePosture::ImpersonationAtSource
            }

            fn execute(&self, _executable: Executable<'_>, _presented: &Presented) -> Result<RowSet, Self::Error> {
                Err(CouldNotAsk)
            }

            fn verify_anchor(&self, _plan: AnchorPlan<'_>) -> Result<AnchorRows, Self::Error> {
                Err(CouldNotAsk)
            }

            fn preflight(&self, tables: &BTreeSet<QualifiedTable>) -> Result<TablesPresent, Self::Error> {
                self.asked.borrow_mut().push(tables.len());
                (self.answer)(tables)
            }

            fn preflight_was_refused(&self, _error: &Self::Error) -> bool {
                self.refused
            }
        }

        /// One open registry over one fake, under the alias the example bundle's models declare.
        fn opened(answer: Answering) -> Warehouses<Answers> {
            registry(answer, false)
        }

        /// The same, over a data system that says its failure was a REFUSAL rather than an outage.
        fn refusing(answer: Answering) -> Warehouses<Answers> {
            registry(answer, true)
        }

        fn registry(answer: Answering, refused: bool) -> Warehouses<Answers> {
            Warehouses::of(Answers {
                source: SourceName::parse("warehouse").expect("a test source is a source"),
                answer,
                asked: RefCell::new(Vec::new()),
                refused,
            })
        }

        /// Two models on one source, one of which the data system will be said not to hold.
        fn bundle() -> sutura_domain::pinned::PinnedDefinitions {
            crate::tests::bundle_over(&[
                ("customers", "warehouse", "dim_customer"),
                ("orders", "warehouse", "fct_orders"),
            ])
        }

        #[test]
        fn a_bundle_naming_a_table_the_data_system_does_not_hold_does_not_start() {
            // THE parity issue 120 is about: a `files` deployment in this state does not start, because
            // the engine is given a file per model. A networked one used to start and fail the first
            // question against that model.
            let engines = opened(|asked| {
                Ok(TablesPresent::of(
                    asked
                        .iter()
                        .filter(|table| table.name().as_str() == "fct_orders")
                        .cloned()
                        .collect(),
                ))
            });
            let error = refuse_absent_tables(&bundle(), &engines).expect_err("a table that is not there stops the process");
            assert!(error.contains("fct_orders"), "the refusal must name the table: {error}");
            // **The whole clause and not the bare model name, which is a review correction to a test
            // that could not fail.** `"orders"` is a substring of `"fct_orders"`, so an assertion on
            // it was entailed by the line above and stayed green even with the model set dropped
            // entirely - coverage for a property nothing checked.
            assert!(
                error.contains("named by model(s) [orders]"),
                "the refusal must name the model: {error}"
            );
            assert!(
                !error.contains("dim_customer"),
                "the refusal must not name a table the data system holds: {error}"
            );
        }

        #[test]
        fn a_data_system_that_did_not_account_for_its_own_tables_does_not_boot_and_names_no_model() {
            // **`telekom/sutura#275` at the root that renders it.** The data system said it holds four
            // tables it then did not name, so the bundle's table may be one of them - and the two
            // things this root must not do are the two it did before: report the table absent (which
            // sends an operator to a `table:` that may be perfectly right) and serve with a `WARN`
            // (which is where an `Err` out of the port would have landed it, and is the worse
            // direction of the two).
            let engines = opened(|asked| {
                Ok(TablesPresent::Unaccounted {
                    tables: UnaccountedTables::parse(asked.clone()).expect("the bundle names two tables"),
                    shortfall: NonZeroU64::new(1).expect("one is not zero"),
                })
            });
            let error = refuse_absent_tables(&bundle(), &engines)
                .expect_err("a data system that cannot account for its own tables has not verified this bundle");
            assert!(error.contains("warehouse"), "the refusal must name the source: {error}");
            assert!(
                error.contains("fct_orders") && error.contains("dim_customer"),
                "and both tables nothing was said about: {error}"
            );
            // **The two numbers, and the bound between them.** A gap of one over two unnamed tables
            // means one of them really is missing; a sentence that read the set and the gap as one
            // quantity would say two tables are unaccounted for beside a gap of one, which is a
            // review finding on the first version of this arm.
            assert!(
                error.contains("did not account for 1 of the table(s)") && error.contains("at most 1 of the 2 table(s)"),
                "the refusal states the gap and what it can explain, and they are different numbers: {error}"
            );
            // The two sentences this refusal must NOT be, asserted rather than assumed from the arm
            // it was written in: `does not hold` is the absent refusal an operator fixes a `table:`
            // over, and `named by model(s)` is the half of it that names the file to open.
            assert!(
                !error.contains("does not hold"),
                "nothing here established that the table is missing: {error}"
            );
            assert!(
                !error.contains("named by model(s)"),
                "and no model is at fault, so none is named: {error}"
            );
        }

        #[test]
        fn a_gap_bigger_than_the_bundle_does_not_claim_to_explain_more_tables_than_there_are() {
            // **The other direction of the bound, reproduced by review at this root.** A shortfall
            // counts tables the data system did not account for across the whole dataset, and the
            // set here is only the part the bundle names - so the gap can be the larger number, and
            // on the shape this check exists for it always is: an identified count of zero makes the
            // shortfall the dataset's entire table count. Printed raw, the refusal read *at most 9
            // of the 2 table(s)*.
            let engines = opened(|asked| {
                Ok(TablesPresent::Unaccounted {
                    tables: UnaccountedTables::parse(asked.clone()).expect("the bundle names two tables"),
                    shortfall: NonZeroU64::new(9).expect("nine is not zero"),
                })
            });
            let error = refuse_absent_tables(&bundle(), &engines).expect_err("the deployment is still not verified");
            assert!(
                error.contains("did not account for 9 of the table(s)"),
                "the gap the data system left is still stated as it is: {error}"
            );
            assert!(
                error.contains("at most 2 of the 2 table(s)"),
                "but what it can explain is bounded by how many tables there are: {error}"
            );
            assert!(
                !error.contains("at most 9"),
                "a gap cannot explain more tables than the answer named: {error}"
            );
        }

        #[test]
        fn a_bundle_whose_tables_are_all_there_starts() {
            // The control, and it asks the data system ONCE with the whole set rather than once per
            // model - which is the cost argument the port is shaped around.
            let engines = opened(|_asked| Ok(TablesPresent::All));
            refuse_absent_tables(&bundle(), &engines).expect("a bundle whose tables are all there serves");
            let engine = engines
                .get(&SourceName::parse("warehouse").expect("a test source is a source"))
                .expect("the fake is registered under that alias");
            assert_eq!(
                *engine.asked.borrow(),
                vec![2],
                "one call carrying both tables, not one call per model"
            );
        }

        #[test]
        fn a_data_system_that_could_not_be_reached_still_boots() {
            // **The soft edge, and it is narrower than it was.** A data system that did not ANSWER is a
            // condition that passes, so the deployment serves with a `WARN`; what a test can assert here
            // is that it is not a refusal. Its twin below is the half review added.
            let engines = opened(|_asked| Err(CouldNotAsk));
            refuse_absent_tables(&bundle(), &engines)
                .expect("an endpoint that did not answer at boot is a deployment that still has to serve");
        }

        #[test]
        fn a_data_system_that_refused_the_listing_does_not_boot() {
            // **The split review asked for, and the control above is what makes it mean something:** the
            // same `Err` value, the same variant, and the only difference is what the data system says
            // about its own failure. A `403` on a listing is one IAM grant and fails identically on every
            // boot, so a permanent `WARN` would hide the check being off in the deployment least likely
            // to read a startup log.
            let engines = refusing(|_asked| Err(CouldNotAsk));
            let error = refuse_absent_tables(&bundle(), &engines)
                .expect_err("a data system that refuses to be asked is a deployment that cannot verify itself");
            assert!(error.contains("warehouse"), "the refusal must name the source: {error}");
            assert!(
                error.contains("bigquery.tables.list"),
                "the refusal must name the grant an operator has to add: {error}"
            );
        }

        #[test]
        fn an_adapter_that_reports_nothing_still_boots() {
            // The engine, and any adapter written before this port existed: `NotAsked` is not a claim
            // that anything was verified, and it is not a refusal either.
            //
            // **Renamed from `..._takes_the_ports_default_...`, because it did not:** this fake overrides
            // `preflight` and returns `Ok(NotAsked)` by hand. What exercises the real DEFAULT is
            // `sutura_domain::warehouse::preflight`'s own test, whose fake omits the method entirely. A
            // test named for a property it does not exercise is the kind of coverage this repository
            // deletes.
            let engines = opened(|_asked| Ok(TablesPresent::NotAsked));
            refuse_absent_tables(&bundle(), &engines).expect("an adapter that did not look refuses nothing");
        }
    }
}
