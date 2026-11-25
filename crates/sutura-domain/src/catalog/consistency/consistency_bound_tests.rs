//! `assemble_bounded`'s caller-declared bound (`github.com/telekom/sutura#2071`): a new file, not a
//! `#[cfg(test)] mod` inline in `consistency.rs`, because that file's own implementation change
//! (gating `assemble`, adding `assemble_bounded`) makes it `impl_only` under the causality gate -
//! reverted at base like any implementation file. Declared from there, this file has no base
//! counterpart: at base, `super::consistency_bound_tests` does not exist and these tests are not
//! run at all, rather than failing to compile against a signature that is not there yet. Every test
//! here is listed by name in `devco/causality-no-base-exemptions`.
//!
//! The exhaustive column/description/metric-prose breakdown cases are here rather than beside
//! `catalog/tests/column_metadata.rs`'s own cases, which stayed on the base-arity `Definitions::
//! assemble` and are genuinely measured red-on-base/green-on-head
//! (`a_catalog_between_the_former_fixed_bound_and_the_new_default_loads_under_the_default`).

use std::collections::BTreeSet;

use super::{Definitions, DefinitionsByteBound, InconsistentDefinitions, Model, ModelName, SourceName, TableName};
use crate::catalog::{Column, ColumnType, Description, MAX_COLUMN_TYPE_CHARS, MAX_DESCRIPTION_BYTES, Metric};
use crate::measure::{AggregatedColumn, Measure, Term};
use crate::model::{Aggregate, ColumnName, Grain, MetricName};

fn column(raw: &str) -> ColumnName {
    ColumnName::parse(raw).expect("a test column is a column")
}

fn model_name(raw: &str) -> ModelName {
    ModelName::parse(raw).expect("a test model is a model")
}

fn metric_name(raw: &str) -> MetricName {
    MetricName::parse(raw).expect("a test metric is a metric")
}

#[test]
fn a_bundle_over_its_caller_declared_bound_is_refused_naming_the_limit() {
    let model = Model::new(
        model_name("m"),
        SourceName::parse("s").expect("a test source name is a name"),
        TableName::parse("t").expect("a test table name is a name"),
        Vec::<ColumnName>::new(),
        Description::parse("some authored prose").expect("under the per-description cap"),
    );
    let error = Definitions::assemble_bounded(vec![model], vec![], vec![], DefinitionsByteBound::parse(1).unwrap())
        .expect_err("one description already exceeds a bound of 1 byte");
    let InconsistentDefinitions::DefinitionsTooLarge { limit, .. } = error else {
        panic!("over-bound is the too-large variant, not {error:?}");
    };
    assert_eq!(limit, 1);
}

/// `#2071`: the bound is per-catalog, not only raised globally - a deployment may declare a lower
/// `max_definitions_bytes` than the default and have it refuse the same bundle a bare-default load
/// would admit.
#[test]
fn an_explicitly_configured_lower_bound_refuses_a_catalog_the_default_would_load() {
    let bound = DefinitionsByteBound::parse(128 * 1024).expect("a positive bound inside the ceiling parses");
    let mut columns = Vec::new();
    for index in 0..40_u32 {
        let filler = Description::parse("y".repeat(MAX_DESCRIPTION_BYTES)).expect("exactly the description cap is a description");
        columns.push(Column::new(column(&format!("c{index}")), None, filler, None));
    }
    let heavy = Model::new(
        model_name("orders"),
        SourceName::parse("local").expect("a test source is a source"),
        TableName::parse("orders").expect("a test table is a table"),
        columns,
        Description::default(),
    );
    match Definitions::assemble_bounded(vec![heavy], vec![], vec![], bound).unwrap_err() {
        InconsistentDefinitions::DefinitionsTooLarge { bytes, limit, .. } => {
            assert_eq!(limit, bound.get());
            assert!(bytes > limit, "{bytes} must exceed {limit}");
        }
        other => panic!("a declared lower bound must be what refuses this: {other:?}"),
    }
}

/// `#2071`'s second ask: the refusal names which part is large, so an operator can see what to
/// trim rather than guess. Three scenarios, each sized to cross the bound in exactly one bucket.
#[test]
fn the_refusal_breakdown_names_the_bucket_that_is_actually_large() {
    let bound = DefinitionsByteBound::parse(128 * 1024).expect("a positive bound inside the ceiling parses");

    // Column types alone: 300 columns at the type cap, no description, no metric.
    let type_filler = ColumnType::parse("x".repeat(MAX_COLUMN_TYPE_CHARS)).expect("exactly the type cap is a type");
    let typed_columns: Vec<Column> = (0..300_u32)
        .map(|index| {
            Column::new(
                column(&format!("c{index}")),
                Some(type_filler.clone()),
                Description::default(),
                None,
            )
        })
        .collect();
    let typed_orders = Model::new(
        model_name("orders"),
        SourceName::parse("local").expect("a test source is a source"),
        TableName::parse("orders").expect("a test table is a table"),
        typed_columns,
        Description::default(),
    );
    match Definitions::assemble_bounded(vec![typed_orders], vec![], vec![], bound).unwrap_err() {
        InconsistentDefinitions::DefinitionsTooLarge {
            columns,
            descriptions,
            metric_prose,
            ..
        } => {
            assert_eq!(descriptions, 0, "no description was declared");
            assert_eq!(metric_prose, 0, "no metric was declared");
            assert!(
                columns > bound.get(),
                "column types alone must be what crosses the bound: {columns}"
            );
        }
        other => panic!("expected DefinitionsTooLarge, got {other:?}"),
    }

    // Descriptions alone: 40 columns at the description cap, no type, no metric.
    let mut described_columns = Vec::new();
    for index in 0..40_u32 {
        let filler = Description::parse("y".repeat(MAX_DESCRIPTION_BYTES)).expect("exactly the description cap is a description");
        described_columns.push(Column::new(column(&format!("c{index}")), None, filler, None));
    }
    let described_orders = Model::new(
        model_name("orders"),
        SourceName::parse("local").expect("a test source is a source"),
        TableName::parse("orders").expect("a test table is a table"),
        described_columns,
        Description::default(),
    );
    match Definitions::assemble_bounded(vec![described_orders], vec![], vec![], bound).unwrap_err() {
        InconsistentDefinitions::DefinitionsTooLarge {
            columns,
            descriptions,
            metric_prose,
            ..
        } => {
            assert_eq!(metric_prose, 0, "no metric was declared");
            assert!(
                columns < descriptions,
                "descriptions alone must dominate columns: {columns} vs {descriptions}"
            );
            assert!(
                descriptions > bound.get(),
                "descriptions alone must be what crosses the bound: {descriptions}"
            );
        }
        other => panic!("expected DefinitionsTooLarge, got {other:?}"),
    }

    // Metric prose alone: a bare model, and 40 metrics at the description cap.
    let description_filler =
        Description::parse("y".repeat(MAX_DESCRIPTION_BYTES)).expect("exactly the description cap is a description");
    let metrics: Vec<Metric> = (0..40_u32)
        .map(|index| {
            Metric::new(
                metric_name(&format!("metric_{index}")),
                model_name("orders"),
                Measure::Simple(Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("amount_cents")))),
                Vec::new(),
                column("order_date"),
                BTreeSet::from([Grain::Month]),
                Vec::new(),
                None,
                description_filler.clone(),
                crate::catalog::Audience::Open,
            )
            .expect("no dimensions to duplicate")
        })
        .collect();
    let bare_orders = Model::new(
        model_name("orders"),
        SourceName::parse("local").expect("a test source is a source"),
        TableName::parse("orders").expect("a test table is a table"),
        vec![column("amount_cents"), column("order_date")],
        Description::default(),
    );
    match Definitions::assemble_bounded(vec![bare_orders], vec![], metrics, bound).unwrap_err() {
        InconsistentDefinitions::DefinitionsTooLarge {
            columns,
            descriptions,
            metric_prose,
            ..
        } => {
            assert_eq!(descriptions, 0, "no model or column description was declared");
            assert!(
                metric_prose > columns,
                "metric prose alone must dominate the small column-identifier bytes: {metric_prose} vs {columns}"
            );
            assert!(
                metric_prose > bound.get(),
                "metric prose alone must be what crosses the bound: {metric_prose}"
            );
        }
        other => panic!("expected DefinitionsTooLarge, got {other:?}"),
    }
}
