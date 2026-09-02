//! The refusals this root makes by READING the bundle, gathered in one file.
//!
//! **Moved out of `main.rs` by the file-length gate, and the cut is at a seam rather than at a line
//! count.** Everything here answers one shape of question - *does the bundle this process is about to
//! serve hold together against what the deployment declared and against what the data systems
//! actually have* - and none of it opens a socket, builds a runtime or touches the settings tree
//! beyond the source registry it is handed. `main.rs` keeps the ORDER these run in, which is the part
//! a reader of a composition root is looking for.
//!
//! The three that were already here refuse a bundle whose anchors cannot be verified and a bundle
//! that changed between two loads. [`refuse_absent_tables`] is the new one, and it closes an
//! asymmetry rather than adding a rule: see its own documentation.

use std::collections::{BTreeMap, BTreeSet};

use sutura_app::Warehouses;
use sutura_domain::model::{ModelName, QualifiedTable, SourceName, TableName};
use sutura_domain::pinned::PinnedDefinitions;
use sutura_domain::warehouse::Warehouse;

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
/// **Ordered after the credential and before the listener, and the type is what pins the first
/// half:** this takes an OPEN registry, which only `open_engine` can produce, and opening a
/// networked source reads its credential. So an operator whose credential file is wrong is told
/// about the credential file - not about a table, which they would then go and not fix.
///
/// **A data system that could not be ASKED is a WARNING and not a refusal**, which is the one
/// deliberate soft edge in this file and is the alternative to a flag. A deployment that cannot grant
/// a listing has to be servable, and the choice was between a documented *could not verify* line and
/// a `skip_preflight` key - a key that would be set in the deployment that most needs the check.
/// `sutura_runtime::banner::announce_token_class` is the precedent for a limit printed at `WARN` on
/// its own line.
///
/// **Two limits.** What a pre-flight establishes is that a table EXISTS: not that the model's columns
/// are on it, and not that a question's identity may read it - an anchor is what covers both, for the
/// metrics that have one. And it reads the bundle loaded FIRST, so a model added to the catalog
/// directory between this root's two loads is caught on a `files` deployment by
/// [`refuse_unattached`] and is not caught here.
pub(crate) fn refuse_absent_tables<W>(pinned: &PinnedDefinitions, engines: &Warehouses<W>) -> Result<(), String>
where
    W: Warehouse,
{
    for (source, engine) in engines.each() {
        let behind = models_by_table(pinned, source);
        if behind.is_empty() {
            continue;
        }
        let asked: BTreeSet<QualifiedTable> = behind.keys().cloned().collect();
        let present = match engine.preflight(&asked) {
            Ok(present) => present,
            Err(cause) => {
                // The WARNING this function's documentation argues for, on its own line, naming the
                // source and the reason. Not `Err`: a deployment that cannot grant a listing is a
                // deployment that still has to serve.
                tracing::warn!(
                    source = %source,
                    tables = asked.len(),
                    reason = %flatten(cause),
                    "could not verify that this data system holds the tables the catalog names - \
                     serving anyway, so a mistyped table name will fail the first question against it"
                );
                continue;
            }
        };
        if let Some(missing) = present.absent() {
            return Err(format!(
                "{source} does not hold {}. Refusing to serve a model whose questions would fail at \
                 query time - fix the catalog's `table:`, or create the table",
                unmatched(missing.named(), &behind)
            ));
        }
        if present.was_asked() {
            tracing::info!(
                source = %source,
                tables = asked.len(),
                "every table the catalog names is in this data system"
            );
        }
    }
    Ok(())
}

/// Which models sit behind each table one source's part of the bundle names.
///
/// **Keyed by the table and carrying the models, because that is the direction the refusal reads
/// in:** the data system answers about a table, and the operator has to edit a model. Two models on
/// one table is ordinary - a bundle may declare several over one fact table - so the value is a set.
fn models_by_table(pinned: &PinnedDefinitions, source: &SourceName) -> BTreeMap<QualifiedTable, BTreeSet<ModelName>> {
    let mut behind: BTreeMap<QualifiedTable, BTreeSet<ModelName>> = BTreeMap::new();
    for model in pinned.definitions().models().values() {
        if model.source() == source {
            behind.entry(model.table().clone()).or_default().insert(model.name().clone());
        }
    }
    behind
}

/// The absent tables, each with the models that named it, as one line of a refusal.
///
/// **It names the model AND the table**, because the two things an operator can do are in the same
/// file and one of them is a typo: the table path is what the data system disagreed with, and the
/// model is what they have to open to change it.
fn unmatched(missing: &BTreeSet<QualifiedTable>, behind: &BTreeMap<QualifiedTable, BTreeSet<ModelName>>) -> String {
    missing
        .iter()
        .map(|table| {
            let models = behind
                .get(table)
                .map(|models| models.iter().map(ModelName::as_str).collect::<Vec<&str>>().join(", "))
                .unwrap_or_default();
            format!("table {table}, named by model(s) [{models}]")
        })
        .collect::<Vec<String>>()
        .join("; ")
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

#[cfg(test)]
mod tests {
    use core::cell::RefCell;
    use std::collections::BTreeSet;

    use sutura_app::Warehouses;
    use sutura_domain::identity::Presented;
    use sutura_domain::model::{QualifiedTable, SourceName};
    use sutura_domain::plan::{AnchorPlan, Executable};
    use sutura_domain::source::{ImpersonationCapability, SourcePosture};
    use sutura_domain::warehouse::{AnchorRows, RowSet, TablesPresent, Warehouse};

    use super::refuse_absent_tables;

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
    }

    /// One open registry over one fake, under the alias the example bundle's models declare.
    fn opened(answer: Answering) -> Warehouses<Answers> {
        Warehouses::of(Answers {
            source: SourceName::parse("warehouse").expect("a test source is a source"),
            answer,
            asked: RefCell::new(Vec::new()),
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
        assert!(error.contains("orders"), "the refusal must name the model: {error}");
        assert!(
            !error.contains("dim_customer"),
            "the refusal must not name a table the data system holds: {error}"
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
    fn a_data_system_that_could_not_be_asked_still_boots() {
        // **The one deliberate soft edge**, and the alternative to a `skip_preflight` flag that would
        // be set in the deployment that most needs the check. It is a WARN line rather than a silent
        // pass; what a test can assert here is that it is not a refusal.
        let engines = opened(|_asked| Err(CouldNotAsk));
        refuse_absent_tables(&bundle(), &engines)
            .expect("a deployment that cannot grant a listing is a deployment that still has to serve");
    }

    #[test]
    fn an_adapter_that_takes_the_ports_default_still_boots() {
        // The engine, and any adapter written before this port existed: `NotAsked` is not a claim
        // that anything was verified, and it is not a refusal either.
        let engines = opened(|_asked| Ok(TablesPresent::NotAsked));
        refuse_absent_tables(&bundle(), &engines).expect("an adapter that did not look refuses nothing");
    }
}
