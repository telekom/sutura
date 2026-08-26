//! The service: what happens between a question arriving and an answer leaving.
//!
//! Generic over the ports and holding no framework types, so it can be exercised against a fake
//! warehouse in a test and a real one in production without either of them knowing. The composition
//! root decides which; this crate never names an adapter.
//!
//! Two entry points, and the order between them is the point:
//!
//! [`verify_anchors`] re-executes every metric that declares a certified number and reports whether
//! it still produces it. [`answer`] takes a [`Validated`] bundle, which is the only thing
//! [`sutura_domain::pinned::Validated::new`] will produce from that report, so **a bundle whose
//! anchors were never checked cannot reach the query path.** Not by discipline: there is no other
//! constructor.

use sutura_domain::catalog::Anchor;
use sutura_domain::model::{Grain, MetricName, SourceName};
use sutura_domain::pinned::{AnchorCheck, AnchorReport, NotExecutedReason, PinnedDefinitions, Validated};
use sutura_domain::query::{Query, RefusalReason, ToolOutcome};
use sutura_domain::warehouse::{RowSet, Warehouse};
use sutura_semantic::{BundleInconsistent, Compiled, compile};

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
    // Prepared before it is run. It costs a round trip and it means a statement that would be
    // rejected is rejected before any data is read, which is the difference between a failed query
    // and a partial one.
    warehouse.dry_run(&plan).map_err(|cause| ServiceError::Warehouse { cause })?;
    let rows = warehouse.execute(&plan).map_err(|cause| ServiceError::Warehouse { cause })?;
    Ok(ToolOutcome::Answer {
        provenance: pinned.provenance(),
        rows,
    })
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
    use sutura_domain::catalog::{Anchor, Definitions, Metric, Model};
    use sutura_domain::definitions::DefinitionDigest;
    use sutura_domain::measure::{AggregatedColumn, Measure, Term};
    use sutura_domain::model::{Aggregate, ColumnName, Grain, ModelName, SourceName, TableName};
    use sutura_domain::pinned::DefinitionVersion;
    use sutura_domain::plan::QueryPlan;

    use super::{AnchorCheck, MetricName, NotExecutedReason, PinnedDefinitions, RowSet, Warehouse, verify_anchors};

    const DIGEST: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

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
            String::new(),
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
            String::new(),
        );
        let definitions = Definitions::assemble(vec![model], vec![], vec![revenue]).expect("the test bundle is consistent");
        PinnedDefinitions::new(
            DefinitionVersion::parse("test-1").expect("a test version is a version"),
            DefinitionDigest::parse(DIGEST).expect("a real digest is a digest"),
            definitions,
        )
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
}
