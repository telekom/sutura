//! The service: what happens between a question arriving and an answer leaving.
//!
//! Generic over the ports and holding no framework types, so it can be exercised against a fake
//! warehouse in a test and a real one in production without either of them knowing. The composition
//! root decides which; this crate never names an adapter.
//!
//! Two entry points, and the order between them is the point:
//!
//! [`verify_and_validate`] re-executes every metric that declares a certified number, against the
//! `Warehouse` it is handed, and hands back the bundle as a [`Validated`] one only if every anchor
//! reproduced its number. [`answer`] takes nothing else. So **a bundle whose anchors were never
//! checked cannot reach the query path** - not by discipline, and not because a caller was asked to
//! call the two in order: [`Validated`] has no other constructor, and the one it has takes a
//! warehouse and calls it.
//!
//! That is the correction to what this crate used to claim. The proof used to be
//! `sutura_domain::pinned::Validated::new(pinned, &report)`, and `AnchorReport::new`,
//! `AnchorReport::record` and `AnchorCheck::Matched` are all public - so any caller could enumerate
//! the bundle's anchors, record `Matched` for each without opening a data system, and get a bundle
//! the service would serve. The golden suite did exactly that. The wrapper attested to the caller's
//! own assertion and read like proof, which is worse than no wrapper. [`verify_anchors`] survives
//! because a report is worth rendering to an operator; what it cannot do any more is mint the proof.
//!
//! [`surface`] is those same two entry points with the ports' generic parameters erased, for a
//! transport whose request handler is a concrete function. It is a *driving* port and it lives here
//! rather than in a transport crate, which is a correction: it used to be `sutura-http`'s, and a
//! second transport would have had to depend on the first to reach it. Nothing in that module names
//! a framework type, so this crate still holds none.

use sutura_domain::catalog::Anchor;
use sutura_domain::model::{Grain, MetricName, SourceName};
use sutura_domain::pinned::{AnchorCheck, AnchorReport, NotExecutedReason, PinnedDefinitions};
use sutura_domain::query::{Query, RefusalReason, ToolOutcome};
use sutura_domain::warehouse::{RowSet, Warehouse};
use sutura_semantic::{BundleInconsistent, Compiled, compile};

// The application-facing interface a transport consumes, with the ports' generics erased: a
// DRIVING port and its one implementor. The argument for it being here rather than in `sutura-http`
// is the module's own documentation - a plain comment here rather than a doc comment, because
// rustdoc resolves the links in a merged module doc against the file the `mod` line is in, and
// every name in that argument lives in the other file.
pub mod surface;

// The agent-facing system prompt, derived from the tool surface and the pinned bundle. Here for the
// same reason `sources` and `grains_coarsest_first` below are: a shape the application can offer
// for free, that every one of its callers needs, belongs with the application rather than with one
// transport. It adds no dependency to this crate's manifest, which is what keeps `cargo tree -p
// sutura-app -e normal` at `sutura-domain`, `sutura-semantic` and `thiserror` - the fact `AGENTS.md`
// cites as holding up the rule that a driving port is not owned by one of its callers.
pub mod prompt;

pub use crate::proof::{Validated, verify_and_validate};

/// The proof, and the only operation that can mint it.
///
/// A module rather than two items in `lib.rs`, and a PRIVATE one, because that is the mechanism: the
/// field of [`Validated`] and its tuple constructor are visible exactly here, so
/// [`verify_and_validate`] is the only safe code anywhere that can produce one. Moving either item
/// out of this module, or adding a second `pub fn` to it that does not call a `Warehouse`, is what a
/// reviewer has to notice - and it is a one-item diff in one place rather than a property of every
/// call site.
mod proof {
    use sutura_domain::pinned::{NotValidated, PinnedDefinitions};
    use sutura_domain::warehouse::Warehouse;

    /// A `T` that has been shown to hold up.
    ///
    /// **The service accepts only this, so an unvalidated bundle is unrepresentable rather than
    /// merely refused.** The field is private to the module this type is declared in, and
    /// [`verify_and_validate`] is the only thing in that module which builds one.
    ///
    /// Generic in the type it wraps, but obtainable only for [`PinnedDefinitions`], and that
    /// asymmetry is the point: validating means re-running every anchor the bundle declares, so
    /// whatever mints this has to be able to enumerate them and to execute them. A blanket
    /// constructor for any `T` would be a wrapper that proves nothing, which is worse than no
    /// wrapper because it reads like proof.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Validated<T>(T);

    impl<T> Validated<T> {
        #[inline]
        pub const fn get(&self) -> &T {
            &self.0
        }

        #[inline]
        pub fn into_inner(self) -> T {
            self.0
        }
    }

    /// Re-runs every anchor against `warehouse`, and returns the bundle only if all of them held.
    ///
    /// The one operation that produces a [`Validated`] bundle. It takes the `Warehouse` and calls
    /// it, which is the whole of what the type is now allowed to claim: not "somebody asserted these
    /// anchors match", but "these statements were executed against this data system and reproduced
    /// the numbers their author certified".
    ///
    /// **What it still does not claim.** `W` is a port, so a caller may pass a fake - and a fake is
    /// exactly what the golden suite passes, deliberately, because the alternative is a test suite
    /// that needs a database to check a refusal. What the type proves is that a warehouse was
    /// called; that the warehouse was the one holding the business's data is a composition-root
    /// decision no signature can make. `answer` narrows it a little further by refusing a plan whose
    /// source is not the adapter's own.
    ///
    /// The forgery this closes does not compile:
    ///
    /// ```compile_fail
    /// use sutura_app::Validated;
    /// use sutura_domain::model::MetricName;
    /// use sutura_domain::pinned::{AnchorCheck, AnchorReport, PinnedDefinitions};
    ///
    /// // Enumerate the anchors, claim each one matched, hand the claim to the validator.
    /// // No data system is opened and no statement is executed.
    /// fn _forge(pinned: PinnedDefinitions) -> Validated<PinnedDefinitions> {
    ///     let names: Vec<MetricName> = pinned.anchored_metrics().map(|(name, _)| name.clone()).collect();
    ///     let mut report = AnchorReport::new();
    ///     for name in names {
    ///         report.record(name, AnchorCheck::Matched);
    ///     }
    ///     // Neither the constructor that was here nor the tuple constructor is reachable.
    ///     Validated::new(pinned, &report).unwrap()
    /// }
    ///
    /// fn _wrap(pinned: PinnedDefinitions) -> Validated<PinnedDefinitions> {
    ///     Validated(pinned)
    /// }
    /// ```
    ///
    /// The twin of that block, which pins the names so a rename cannot make it pass vacuously:
    ///
    /// ```
    /// use sutura_app::{Validated, verify_and_validate};
    /// use sutura_domain::pinned::{NotValidated, PinnedDefinitions};
    /// use sutura_domain::warehouse::Warehouse;
    ///
    /// fn _served(_bundle: &Validated<PinnedDefinitions>) {}
    ///
    /// fn _mint<W: Warehouse>(
    ///     pinned: PinnedDefinitions,
    ///     warehouse: &W,
    /// ) -> Result<Validated<PinnedDefinitions>, NotValidated> {
    ///     verify_and_validate(pinned, warehouse)
    /// }
    /// ```
    pub fn verify_and_validate<W>(pinned: PinnedDefinitions, warehouse: &W) -> Result<Validated<PinnedDefinitions>, NotValidated>
    where
        W: Warehouse,
    {
        let report = super::verify_anchors(&pinned, warehouse);
        report.verdict(&pinned)?;
        Ok(Validated(pinned))
    }
}

/// Why the service could not produce an outcome.
///
/// Neither variant is a refusal. A refusal is something the caller asked for and may not have; these
/// are the data system being unreachable and our own bundle or generator being wrong, and offering
/// either as a refusal would invite a caller to retry a different question forever.
///
/// Generic in the warehouse error rather than boxing it, so the adapter that failed keeps its own
/// typed error all the way out. A `Box<dyn Error>` here would be the same loss of information the
/// boundary gate bans `anyhow` for, arrived at by a different route.
#[derive(Debug, thiserror::Error)]
pub enum ServiceError<E> {
    #[error("the question could not be compiled")]
    Compile {
        #[source]
        cause: BundleInconsistent,
    },
    #[error("the data system did not answer")]
    Warehouse {
        #[source]
        cause: E,
    },
}

/// What answering produced, or why it could not.
///
/// A named alias because the inline form is over the complexity threshold in `clippy.toml`, and
/// naming it is the better half of that trade: the generic parameter is a warehouse, not a result.
pub type Answered<W> = Result<ToolOutcome, ServiceError<<W as Warehouse>::Error>>;

/// Answers one question, or says why it will not.
///
/// The source check is not a formality. A plan names exactly one data system, and running it against
/// a different one would answer a question about other data under the same provenance. It is a
/// refusal rather than an error because it is a governance outcome: this caller cannot have this
/// question answered here.
pub fn answer<W>(definitions: &Validated<PinnedDefinitions>, query: &Query, warehouse: &W) -> Answered<W>
where
    W: Warehouse,
{
    let pinned = definitions.get();
    let compiled = compile(query, pinned).map_err(|cause| ServiceError::Compile { cause })?;
    // The PLAN is what the port takes now, not a rendered statement: an adapter that executes
    // without generating SQL is a first-class implementation of it. A SQL-speaking adapter renders
    // the plan itself, for its own dialect.
    let plan = match compiled {
        Compiled::Refused { reason } => return Ok(ToolOutcome::Refusal { reason }),
        Compiled::Planned { plan } => plan,
    };
    if plan.source() != warehouse.source() {
        return Ok(ToolOutcome::Refusal {
            reason: RefusalReason::SourceUnavailable {
                source: plan.source().clone(),
            },
        });
    }
    // Prepared before it is run, WHERE THAT IS CHEAPER THAN RUNNING IT. For an adapter across a
    // network it is: a statement that would be rejected is rejected before any data is read, which
    // is the difference between a failed query and a partial one. For the in-process engine it is
    // not - checking builds the logical plan and runs the analyzer and the optimizer, and then
    // execution does all of it again, so the guarantee was bought at the price of two full planning
    // passes per question. `Warehouse::dry_run` is defaulted for that reason: an adapter that cannot
    // make checking cheaper answers this by doing nothing, and says so by not implementing it.
    warehouse.dry_run(&plan).map_err(|cause| ServiceError::Warehouse { cause })?;
    // The working-set ceiling, on its way out as a refusal rather than as an error. Exhaustion is a
    // governance outcome - the question is well formed and this deployment will not spend more than
    // a configured number of bytes on it - and it used to leave here as `ServiceError::Warehouse`,
    // which the transport answers `503`. That is what a data system being down looks like, so a
    // caller was told to retry against a bound that fires again in the same place.
    //
    // The adapter is asked rather than inspected: `Self::Error` is its own type and nothing here can
    // read it, which is why `working_set_exhausted` is on the port. An adapter with no bounded pool
    // answers `None` by default and this arm never runs for it.
    //
    // `dry_run` above is deliberately not given the same treatment: the port's contract is that a
    // check reads no data, so there is no reservation for a ceiling to refuse.
    let rows = match warehouse.execute(&plan) {
        Ok(rows) => rows,
        Err(cause) => {
            if let Some(ceiling_bytes) = warehouse.working_set_exhausted(&cause) {
                return Ok(ToolOutcome::Refusal {
                    reason: RefusalReason::ResourcesExhausted { ceiling_bytes },
                });
            }
            // Anything else is a failure rather than a refusal, and the typed cause travels with it.
            return Err(ServiceError::Warehouse { cause });
        }
    };
    // The row cap, enforced rather than merely requested. The plan asked for one row more than
    // `plan.max_rows()`, so more than that many coming back means the result was cut short - and a
    // truncated result is a wrong total under a certified name, with provenance attached and nothing
    // saying it is partial. Refused, because "this question is too wide to certify" is an answer the
    // caller can act on and a silent partial one is not.
    if exceeds_row_cap(rows.rows().len(), plan.max_rows()) {
        return Ok(ToolOutcome::Refusal {
            reason: RefusalReason::ResultTooLarge { limit: plan.max_rows() },
        });
    }
    Ok(ToolOutcome::Answer {
        provenance: pinned.provenance(),
        rows,
    })
}

/// Whether a result set came back with more rows than its plan capped it at.
///
/// **A governance control, so the direction it fails in is the whole of what this function is for.**
/// The comparison used to be written inline as
/// `rows.len() > usize::try_from(plan.max_rows()).unwrap_or(usize::MAX)`, which reads as a cap and
/// is a cap being lifted: a conversion that came back `Err` produced `usize::MAX`, and no result set
/// is longer than that, so the one refusal that stops a TRUNCATED total from being certified would
/// have been skipped. Unreachable on any target with 32-bit pointers or wider, and still the wrong
/// direction to have written down.
///
/// It compares in `u64` instead, where the plan's `u32` cap widens with `From` and cannot fail at
/// all. The count still needs a conversion, because neither direction between these two types is
/// infallible - `From<usize> for u64` does not exist, since a target with pointers wider than 64
/// bits would lose a count, and `From<u32> for usize` does not either, since a 16-bit target could
/// not hold the cap. What changed is which way the unreachable case falls: a count that does not fit
/// a `u64` is a count larger than any `u32` cap, so `u64::MAX` here is not a fallback that guesses,
/// it is the answer. The control refuses rather than opening.
///
/// Named rather than inline so the boundary is testable without a data system: the case that decides
/// a certification is one row over the cap, and reaching it through [`answer`] means fabricating ten
/// thousand rows through a validated bundle.
fn exceeds_row_cap(returned: usize, max_rows: u32) -> bool {
    u64::try_from(returned).unwrap_or(u64::MAX) > u64::from(max_rows)
}

/// Re-executes every declared anchor and reports what each produced.
///
/// Returns a report rather than a `Result`, because "this one metric no longer computes its number"
/// and "the data system is down" are both outcomes worth recording per metric. Collapsing either into
/// a single error would lose which metric, and the whole point is to name it.
pub fn verify_anchors<W>(pinned: &PinnedDefinitions, warehouse: &W) -> AnchorReport
where
    W: Warehouse,
{
    let mut report = AnchorReport::new();
    for (name, anchor) in pinned.anchored_metrics() {
        report.record(name.clone(), check_one(pinned, warehouse, name, anchor));
    }
    report
}

/// Every cause beneath an error, outermost first.
///
/// **This is where the anchor failure's cause used to be thrown away.** `Display` on a `thiserror`
/// enum prints the outermost message and stops, so formatting an adapter error into a sentence
/// discarded the driver's own complaint - the part that names the table, the column or the file. The
/// chain cannot be kept as a typed cause either: the port's error is a generic parameter, and the
/// domain must not hold one. Walking it to text here is the lossless option at that boundary, and
/// this is the only place where the typed error is still in scope.
fn causes(error: &dyn core::error::Error) -> Vec<String> {
    let mut chain = Vec::new();
    let mut cursor = error.source();
    while let Some(cause) = cursor {
        chain.push(cause.to_string());
        cursor = cause.source();
    }
    chain
}

/// A typed error, flattened for a [`NotExecutedReason`] variant that cannot name it.
fn flatten(error: &dyn core::error::Error) -> (String, Vec<String>) {
    (error.to_string(), causes(error))
}

/// Checks one anchor.
fn check_one<W>(pinned: &PinnedDefinitions, warehouse: &W, metric: &MetricName, anchor: &Anchor) -> AnchorCheck
where
    W: Warehouse,
{
    let not_executed = |reason: NotExecutedReason| AnchorCheck::NotExecuted { reason };

    let Some(definition) = pinned.definitions().metric(metric) else {
        return not_executed(NotExecutedReason::BundleMissingMetric);
    };
    // The coarsest grain the metric declares, so that an anchor range covering one period yields one
    // row. A finer grain would return several, and there is no single number to compare against.
    let Some(grain) = definition.grains().iter().copied().max() else {
        return not_executed(NotExecutedReason::NoGrain);
    };

    // No dimensions and no filters: an anchor is the metric's own number, not a slice of it.
    let question = Query::new(metric.clone(), grain, anchor.range(), Vec::new(), Vec::new());
    let compiled = match compile(&question, pinned) {
        Ok(compiled) => compiled,
        Err(cause) => {
            let (message, chain) = flatten(&cause);
            return not_executed(NotExecutedReason::NotCompiled { message, chain });
        }
    };
    let plan = match compiled {
        Compiled::Refused { reason } => {
            return not_executed(NotExecutedReason::Refused { reason });
        }
        Compiled::Planned { plan } => plan,
    };
    // A governance condition, not prose in a report field: the plan names a data system this process
    // did not open, which is the same thing `answer` refuses a question for.
    if plan.source() != warehouse.source() {
        return not_executed(NotExecutedReason::SourceMismatch {
            plan: plan.source().clone(),
            warehouse: warehouse.source().clone(),
        });
    }
    let rows = match warehouse.execute(&plan) {
        Ok(rows) => rows,
        Err(cause) => {
            let (message, chain) = flatten(&cause);
            return not_executed(NotExecutedReason::Failed { message, chain });
        }
    };
    match measure_of(&rows, metric) {
        Ok(actual) if actual == anchor.value() => AnchorCheck::Matched,
        Ok(actual) => AnchorCheck::Mismatch {
            expected: String::from(anchor.value()),
            actual,
        },
        Err(reason) => not_executed(reason),
    }
}

/// The measure column of a single-row anchor result.
///
/// Insists on exactly one row. An anchor range that covers two periods at the metric's coarsest
/// grain comes back as two rows, and reading the first would compare a certified total against one
/// period of it: a mismatch that reads like a broken definition and is a mis-declared range.
///
/// The three ways this fails are variants of [`NotExecutedReason`] rather than a second enum
/// declared here. They are all about the shape of a [`RowSet`], which is a domain type, and one
/// enum whose variants match the branches of the check is what lets the report be read without a
/// translation step in the middle that could lose one.
fn measure_of(rows: &RowSet, metric: &MetricName) -> Result<String, NotExecutedReason> {
    if rows.rows().len() != 1 {
        return Err(NotExecutedReason::NotOneNumber { rows: rows.rows().len() });
    }
    let label = metric.as_str();
    let index = rows.column_index(label).ok_or_else(|| NotExecutedReason::NoMeasureColumn {
        label: String::from(label),
    })?;
    rows.cell(0, index)
        .map(sutura_domain::warehouse::Value::render)
        .ok_or(NotExecutedReason::ResultShapeMismatch)
}

/// The data systems a bundle reads from.
///
/// Exposed because a composition root has to decide which adapters to open before it can answer
/// anything, and reading it off the bundle beats being told twice.
pub fn sources(pinned: &PinnedDefinitions) -> Vec<&SourceName> {
    let mut out: Vec<&SourceName> = pinned
        .definitions()
        .models()
        .values()
        .map(sutura_domain::catalog::Model::source)
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// The grains a metric declares, coarsest first.
///
/// A small helper the composition root uses to describe a metric, kept here so the ordering is the
/// same one [`verify_anchors`] picks a grain by.
pub fn grains_coarsest_first(pinned: &PinnedDefinitions, metric: &MetricName) -> Vec<Grain> {
    pinned
        .definitions()
        .metric(metric)
        .map(|definition| {
            let mut grains: Vec<Grain> = definition.grains().iter().copied().collect();
            grains.sort_unstable_by(|a, b| b.cmp(a));
            grains
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use sutura_domain::calendar::{Date, TimeRange};
    use sutura_domain::catalog::{Anchor, Definitions, Description, Metric, Model};
    use sutura_domain::knowledge::Knowledge;
    use sutura_domain::measure::{AggregatedColumn, Measure, Term};
    use sutura_domain::model::{Aggregate, ColumnName, Grain, ModelName, SourceName, TableName};
    use sutura_domain::pinned::{DefinitionVersion, NotValidated};
    use sutura_domain::plan::{MAX_ROWS, QueryPlan};

    use super::{
        AnchorCheck, MetricName, NotExecutedReason, PinnedDefinitions, RowSet, Warehouse, exceeds_row_cap, verify_anchors,
        verify_and_validate,
    };

    fn metric() -> MetricName {
        MetricName::parse("revenue").expect("a test metric name is a name")
    }

    fn source() -> SourceName {
        SourceName::parse("local").expect("a test source is a source")
    }

    /// A one-metric bundle whose metric declares an anchor, so there is exactly one check to make.
    fn bundle() -> PinnedDefinitions {
        let column = |raw: &str| ColumnName::parse(raw).expect("a test column is a column");
        let model = Model::new(
            ModelName::parse("orders").expect("a test model is a model"),
            source(),
            TableName::parse("orders").expect("a test table is a table"),
            BTreeSet::from([column("amount_cents"), column("order_date")]),
            Description::default(),
        );
        let range = TimeRange::new(
            Date::parse("2026-06-01").expect("a test date is a date"),
            Date::parse("2026-07-01").expect("a test date is a date"),
        )
        .expect("June is a range");
        let revenue = Metric::new(
            metric(),
            ModelName::parse("orders").expect("a test model is a model"),
            Measure::Simple(Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("amount_cents")))),
            Vec::new(),
            column("order_date"),
            BTreeSet::from([Grain::Month]),
            BTreeMap::new(),
            Some(Anchor::new(range, String::from("197122"))),
            Description::default(),
        );
        let definitions = Definitions::assemble(vec![model], vec![], vec![revenue]).expect("the test bundle is consistent");
        // The real hasher, from the catalog adapter that owns the canonical form. `pin` applies it to
        // the definitions being pinned, so there is no digest here for the bundle not to describe.
        PinnedDefinitions::pin(
            DefinitionVersion::parse("test-1").expect("a test version is a version"),
            definitions,
            Knowledge::none(),
        )
        .expect("the test definitions hash")
    }

    /// The driver's own complaint, one level below the adapter's.
    #[derive(Debug, thiserror::Error)]
    #[error("no such file: orders.csv")]
    struct DriverFailure;

    /// What an adapter returns: its own message, with the driver's underneath it.
    #[derive(Debug, thiserror::Error)]
    #[error("the data system rejected the statement")]
    struct AdapterFailure {
        #[source]
        cause: DriverFailure,
    }

    /// A data system that fails every statement, with a cause worth reading.
    struct BrokenWarehouse {
        source: SourceName,
    }

    impl Warehouse for BrokenWarehouse {
        type Error = AdapterFailure;

        fn source(&self) -> &SourceName {
            &self.source
        }

        fn dry_run(&self, _plan: &QueryPlan) -> Result<(), Self::Error> {
            Err(AdapterFailure { cause: DriverFailure })
        }

        fn execute(&self, _plan: &QueryPlan) -> Result<RowSet, Self::Error> {
            Err(AdapterFailure { cause: DriverFailure })
        }
    }

    #[test]
    fn a_failed_anchor_check_keeps_the_adapters_own_cause() {
        // THE BUG THIS EXISTS FOR. The failure used to be recorded as one formatted sentence, and
        // `Display` on a `thiserror` enum prints only the outermost message - so the driver's own
        // complaint, the half that names a table, a column or a file, was gone before the report was
        // built. Anchor verification is the readiness gate, so that sentence was the whole of what an
        // operator got when a deployment refused to serve.
        //
        // Asserted over the chain rather than over the message alone: the message was never the part
        // that went missing.
        let pinned = bundle();
        let report = verify_anchors(&pinned, &BrokenWarehouse { source: source() });
        let check = report.checks().get(&metric()).expect("the anchored metric was checked");
        let AnchorCheck::NotExecuted {
            reason: NotExecutedReason::Failed { ref message, ref chain },
        } = *check
        else {
            panic!("a data system that fails every statement is a failed check, not {check:?}");
        };
        assert_eq!(message, "the data system rejected the statement");
        assert_eq!(chain, &vec![String::from("no such file: orders.csv")]);
    }

    #[test]
    fn a_check_against_the_wrong_data_system_is_a_source_mismatch() {
        // It was prose in a report field, and it is a governance condition: the plan names a data
        // system this process did not open. Typed, an operator can tell it apart from an outage
        // without reading a sentence, which is the difference that decides who gets paged.
        let pinned = bundle();
        let elsewhere = BrokenWarehouse {
            source: SourceName::parse("somewhere_else").expect("a test source is a source"),
        };
        let report = verify_anchors(&pinned, &elsewhere);
        let check = report.checks().get(&metric()).expect("the anchored metric was checked");
        let AnchorCheck::NotExecuted {
            reason: NotExecutedReason::SourceMismatch { ref plan, ref warehouse },
        } = *check
        else {
            panic!("a plan for another data system is a source mismatch, not {check:?}");
        };
        assert_eq!(plan.as_str(), "local");
        assert_eq!(warehouse.as_str(), "somewhere_else");
    }

    #[test]
    fn a_bundle_whose_anchor_could_not_run_does_not_come_back_validated() {
        // The other half of the invariant, and the half a report could not carry: the ONLY way to a
        // `Validated` bundle runs the anchors, so a data system that answers nothing yields no
        // bundle at all. Before this operation existed, the same situation was a report a caller was
        // free to ignore - and `Validated::new` was happy to be handed a different one.
        let error = verify_and_validate(bundle(), &BrokenWarehouse { source: source() })
            .expect_err("a data system that fails every statement cannot validate a bundle");
        let NotValidated::AnchorNotExecuted { ref metric, .. } = error else {
            panic!("a failed anchor check is a not-executed verdict, not {error:?}");
        };
        assert_eq!(metric, &self::metric());
    }

    #[test]
    fn the_row_cap_refuses_at_one_row_over_and_cannot_be_lifted_by_a_failed_conversion() {
        // The plan asks a data system for one row MORE than it will certify, so a result carrying
        // more than the cap is a result that was cut short - a wrong total under a certified name.
        // The boundary is the whole control: exactly the cap answers, one row over refuses.
        assert_eq!(MAX_ROWS, 10_000, "the boundary below is written in terms of the cap");
        assert!(!exceeds_row_cap(0, MAX_ROWS), "an empty result is not a truncated one");
        assert!(!exceeds_row_cap(9_999, MAX_ROWS), "under the cap answers");
        assert!(!exceeds_row_cap(10_000, MAX_ROWS), "exactly the cap answers");
        assert!(exceeds_row_cap(10_001, MAX_ROWS), "one row over the cap is a truncated total");
        // THE DIRECTION THIS FUNCTION EXISTS FOR. The comparison used to narrow the CAP to a
        // `usize` with `unwrap_or(usize::MAX)`, so a conversion that failed meant no cap at all and
        // the largest result set there is would have been certified as complete. A count that
        // cannot be carried is a count over every cap, and this asserts it refuses.
        assert!(
            exceeds_row_cap(usize::MAX, MAX_ROWS),
            "the largest count there is exceeds any cap"
        );
        assert!(
            exceeds_row_cap(usize::MAX, u32::MAX),
            "including against the widest cap a plan could carry"
        );
        // And a zero cap is a cap, not an absence: `QueryPlan::new` always sets `MAX_ROWS`, and this
        // function is not allowed to read a small number as permission.
        assert!(exceeds_row_cap(1, 0), "one row over a cap of zero is over the cap");
        assert!(!exceeds_row_cap(0, 0), "no rows is not over a cap of none");
        // The other end, so the comparison is not narrowing by accident: a cap no result could reach
        // admits an ordinary result.
        assert!(!exceeds_row_cap(1, u32::MAX), "one row is not over four billion");
    }
}
