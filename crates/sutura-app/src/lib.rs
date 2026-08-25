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
use sutura_domain::pinned::{AnchorCheck, AnchorReport, PinnedDefinitions, Validated};
use sutura_domain::query::{Query, RefusalReason, ToolOutcome};
use sutura_domain::warehouse::{RowSet, Warehouse};
use sutura_semantic::{CompileError, Compiled, Dialect, compile};

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
        cause: CompileError,
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
pub fn answer<W>(definitions: &Validated<PinnedDefinitions>, query: &Query, warehouse: &W, dialect: Dialect) -> Answered<W>
where
    W: Warehouse,
{
    let pinned = definitions.get();
    let compiled = compile(query, pinned, dialect).map_err(|cause| ServiceError::Compile { cause })?;
    let generated = match compiled {
        Compiled::Refused { reason } => return Ok(ToolOutcome::Refusal { reason }),
        // The plan is not needed to answer: it exists so a golden can pin what we decided.
        Compiled::Statement { query, .. } => query,
    };
    if generated.source() != warehouse.source() {
        return Ok(ToolOutcome::Refusal {
            reason: RefusalReason::SourceUnavailable {
                source: generated.source().clone(),
            },
        });
    }
    // Prepared before it is run. It costs a round trip and it means a statement that would be
    // rejected is rejected before any data is read, which is the difference between a failed query
    // and a partial one.
    warehouse
        .dry_run(&generated)
        .map_err(|cause| ServiceError::Warehouse { cause })?;
    let rows = warehouse
        .execute(&generated)
        .map_err(|cause| ServiceError::Warehouse { cause })?;
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
pub fn verify_anchors<W>(pinned: &PinnedDefinitions, warehouse: &W, dialect: Dialect) -> AnchorReport
where
    W: Warehouse,
{
    let mut report = AnchorReport::new();
    for (name, anchor) in pinned.anchored_metrics() {
        report.record(name.clone(), check_one(pinned, warehouse, dialect, name, anchor));
    }
    report
}

/// Checks one anchor.
fn check_one<W>(pinned: &PinnedDefinitions, warehouse: &W, dialect: Dialect, metric: &MetricName, anchor: &Anchor) -> AnchorCheck
where
    W: Warehouse,
{
    let not_executed = |reason: String| AnchorCheck::NotExecuted { reason };

    let Some(definition) = pinned.definitions().metric(metric) else {
        return not_executed(String::from("the bundle does not hold this metric"));
    };
    // The coarsest grain the metric declares, so that an anchor range covering one period yields one
    // row. A finer grain would return several, and there is no single number to compare against.
    let Some(grain) = definition.grains().iter().copied().max() else {
        return not_executed(String::from("the metric declares no grain"));
    };

    // No dimensions and no filters: an anchor is the metric's own number, not a slice of it.
    let question = Query::new(metric.clone(), grain, anchor.range(), Vec::new(), Vec::new());
    let compiled = match compile(&question, pinned, dialect) {
        Ok(compiled) => compiled,
        Err(cause) => return not_executed(format!("the anchor query could not be compiled: {cause}")),
    };
    let generated = match compiled {
        Compiled::Refused { reason } => {
            return not_executed(format!("the anchor query was refused: {reason:?}"));
        }
        Compiled::Statement { query, .. } => query,
    };
    if generated.source() != warehouse.source() {
        return not_executed(format!(
            "the metric reads from {}, and this data system is {}",
            generated.source().as_str(),
            warehouse.source().as_str()
        ));
    }
    let rows = match warehouse.execute(&generated) {
        Ok(rows) => rows,
        Err(cause) => return not_executed(format!("the anchor query failed: {cause}")),
    };
    match measure_of(&rows, metric) {
        Ok(actual) if actual == anchor.value() => AnchorCheck::Matched,
        Ok(actual) => AnchorCheck::Mismatch {
            expected: String::from(anchor.value()),
            actual,
        },
        Err(reason) => not_executed(reason.to_string()),
    }
}

/// Why an anchor result could not be reduced to one number.
///
/// A typed enum rather than prose. The boundary gate fails a string-typed error in a library crate,
/// and it is right to: each of these sends a reader somewhere different, and only the variant says
/// which. (Written without quoting the banned signature, because that gate is a line scan and a
/// doc comment naming the shape trips it - which is a fair trade for a check that cannot be fooled
/// by a rename.)
#[derive(Debug, thiserror::Error)]
enum NotOneNumber {
    /// The declared range covers more than one period at the metric's coarsest grain.
    #[error(
        "the anchor query returned {rows} rows, and an anchor is one number: the declared range \
         covers more than one period at the metric's coarsest grain"
    )]
    WrongRowCount { rows: usize },
    #[error("the result has no single column labelled {label:?}, so there is nothing to compare")]
    NoMeasureColumn { label: String },
    #[error("the result set was not the shape it reported")]
    ShapeMismatch,
}

/// The measure column of a single-row anchor result.
///
/// Insists on exactly one row. An anchor range that covers two periods at the metric's coarsest
/// grain comes back as two rows, and reading the first would compare a certified total against one
/// period of it: a mismatch that reads like a broken definition and is a mis-declared range.
fn measure_of(rows: &RowSet, metric: &MetricName) -> Result<String, NotOneNumber> {
    if rows.rows().len() != 1 {
        return Err(NotOneNumber::WrongRowCount { rows: rows.rows().len() });
    }
    let label = metric.as_str();
    let index = rows.column_index(label).ok_or_else(|| NotOneNumber::NoMeasureColumn {
        label: String::from(label),
    })?;
    rows.cell(0, index)
        .map(sutura_domain::warehouse::Value::render)
        .ok_or(NotOneNumber::ShapeMismatch)
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
