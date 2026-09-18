//! Boot-time anchor verification, and the bundle introspection a composition root needs beside it.
//!
//! Split out of `lib.rs` when that file reached the unexemptable 1000-line gate, along the seam
//! the module doc there already named: [`verify_anchors`] re-executes every declared anchor
//! against a `Warehouse`, with no caller and no credential, and everything below it in this file
//! exists only to make that check possible or to describe the bundle it checked.

use sutura_domain::catalog::Anchor;
use sutura_domain::model::{Grain, MetricName, SourceName};
use sutura_domain::pinned::view::ScopedView;
use sutura_domain::pinned::{AnchorCheck, AnchorReport, NotExecutedReason, PinnedDefinitions};
use sutura_domain::plan::{AnchorPlan, RowCeiling};
use sutura_domain::query::Query;
use sutura_domain::warehouse::{RowSet, Warehouse};
use sutura_semantic::{Compiled, compile};

use crate::Warehouses;

/// Re-executes every declared anchor and reports what each produced.
///
/// Returns a report rather than a `Result`, because "this one metric no longer computes its number"
/// and "the data system is down" are both outcomes worth recording per metric. Collapsing either into
/// a single error would lose which metric, and the whole point is to name it.
pub fn verify_anchors<W>(pinned: &PinnedDefinitions, warehouses: &Warehouses<W>) -> AnchorReport
where
    W: Warehouse,
{
    let mut report = AnchorReport::new();
    for (name, anchor) in pinned.anchored_metrics() {
        report.record(name.clone(), check_one(pinned, warehouses, name, anchor));
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
pub(crate) fn flatten(error: &dyn core::error::Error) -> (String, Vec<String>) {
    (error.to_string(), causes(error))
}

/// Checks one anchor.
fn check_one<W>(pinned: &PinnedDefinitions, warehouses: &Warehouses<W>, metric: &MetricName, anchor: &Anchor) -> AnchorCheck
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

    // No dimensions and no filters: an anchor is the metric's own number, not a slice of it. No
    // `top` either, so the compiled default row ceiling is exact rather than a stand-in.
    let question = Query::new(metric.clone(), grain, anchor.range(), Vec::new(), Vec::new());
    let compiled = match compile(&question, &ScopedView::everything(pinned), RowCeiling::DEFAULT) {
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
        // An anchor is asked with no dimensions, so it can only ever be one source. Reaching this
        // arm is a defect here rather than anything about the data.
        Compiled::Federated { .. } => {
            return not_executed(NotExecutedReason::ResolvedToTwoSources);
        }
        Compiled::Planned { plan } => plan,
    };
    // A governance condition, not prose in a report field: the plan names a data system this process
    // did not open, which is the same thing `answer` refuses a question for. The plan SELECTS its
    // warehouse - it is not compared against one - so this arm is "nobody configured that source"
    // rather than "the one adapter we hold is called something else".
    let Some(warehouse) = warehouses.get(plan.source()) else {
        return not_executed(NotExecutedReason::SourceNotConfigured {
            plan: plan.source().clone(),
        });
    };
    // The plan, checked against the bundle as this anchor's own before anything executes it. Every
    // fact `AnchorPlan::of` compares - that the metric is defined, that it declares an anchor, the
    // range that anchor certifies, and the coarsest grain it is asked at - is read off `pinned` rather
    // than handed in, which is what makes the check a check on THIS function rather than on its
    // arguments. It is a self-check and not an authority: `AnchorPlan`'s own documentation says so,
    // and what keeps the credential-free method to this one call site is the `clippy.toml` ban below.
    // Reaching the `Err` arm means this function compiled something other than the anchor's own
    // question, so it is a defect here rather than a governance outcome, and it is reported as one:
    // `NotExecutedReason::NotAnAnchor` names the metric's report entry rather than failing the boot
    // for every other anchor in the bundle.
    let anchor_plan = match AnchorPlan::of(&plan, pinned, metric) {
        Ok(anchor_plan) => anchor_plan,
        // D10: carried typed now - `flatten` used to erase `NotAnAnchorsPlan`'s own variant into a
        // string, though it is this crate's own type and never needed the boundary that justifies
        // `flatten` for the other two `NotExecutedReason` arms.
        Err(cause) => return not_executed(NotExecutedReason::NotAnAnchor { cause }),
    };
    // `verify_anchor` and not `execute`, and the difference is the identity rather than the method
    // name. There is no caller at boot, so there is no credential in scope and nothing here could
    // pass one - which is what stops this path from being the door the service-identity fallback
    // comes back through. What it runs as is whatever the deployment configured this adapter with,
    // and `docs/adr/0008` part 1 is why that is the only honest answer available: under row-level
    // security a per-subject anchor is a function rather than a number.
    //
    // THE SINGLE EXPECTATION for the `clippy.toml` ban on this method, and it is the mechanism that
    // makes the credential-free path boot-only: a second call site anywhere in the workspace is an
    // error under `-D warnings` until somebody writes a second `#[expect]` a reviewer sees in the
    // diff. `AnchorPlan` cannot carry that on its own - every value its constructor reads is publicly
    // constructible, and Rust has no cross-crate friend visibility.
    #[expect(
        clippy::disallowed_methods,
        reason = "the boot path is the one caller of the method that executes with no credential; the \
                  ban exists so that this is the only place it is called from"
    )]
    let rows = match warehouse.verify_anchor(anchor_plan) {
        Ok(rows) => rows,
        Err(cause) => {
            let (message, chain) = flatten(&cause);
            return not_executed(NotExecutedReason::Failed { message, chain });
        }
    };
    match measure_of(rows.verified_at_boot(), metric) {
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

/// The data system one metric's own model sits on.
///
/// Exposed for the composition root's anchor check: an anchor is asked with no dimensions, so it
/// resolves to the metric's own model and therefore to that model's source - which is the source whose
/// declared verification identity would have to run it.
///
/// **Narrower than "every source this metric's plan could read", deliberately.** A question WITH
/// dimensions can reach a joined model, and the plan stage refuses one that spans two sources - so for
/// a plan that compiles at all this is the only source there is. What it is not is a general answer for
/// a federated plan, and it stops being the right function the moment one exists.
pub fn source_of<'bundle>(pinned: &'bundle PinnedDefinitions, metric: &MetricName) -> Option<&'bundle SourceName> {
    let definitions = pinned.definitions();
    let model = definitions.metric(metric)?.model();
    Some(definitions.model(model)?.source())
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
