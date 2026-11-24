//! A column's type, description and primary-key evidence: issue #966.
//!
//! A concept module rather than a split of [`super`], which was over `cargo xtask
//! max-lines`'s thousand-line cap - the same reason [`super::chain`] is its own file.

use super::*;

#[test]
fn a_column_carries_the_type_and_description_it_was_given() {
    let orders = Model::new(
        model_name("orders"),
        SourceName::parse("local").expect("a test source is a source"),
        TableName::parse("orders").expect("a test table is a table"),
        vec![Column::new(
            column("amount_cents"),
            Some(ColumnType::parse("NUMERIC").expect("a test column type is a column type")),
            Description::parse("the order total, in minor units").expect("a test description is a description"),
            Some(false),
        )],
        Description::default(),
    );
    let carried = orders.column(&column("amount_cents")).expect("the column was declared");
    assert_eq!(carried.data_type().map(ColumnType::as_str), Some("NUMERIC"));
    assert_eq!(carried.description(), "the order total, in minor units");
    assert_eq!(carried.nullable(), Some(false));
}

#[test]
fn a_bare_column_name_still_constructs_a_model_with_no_type_or_description() {
    // What every caller here that has only ever named a column set gets: `From<ColumnName>` for
    // `Column`. If this stopped compiling, every fixture in the workspace would have to change at
    // once.
    let orders = model("orders", "local", &["amount_cents"]);
    let bare = orders.column(&column("amount_cents")).expect("the column was declared");
    assert_eq!(bare.data_type(), None);
    assert_eq!(bare.description(), "");
    assert_eq!(bare.nullable(), None);
}

#[test]
fn a_primary_key_naming_a_column_the_model_does_not_have_is_refused() {
    // Checked at construction (`Model::with_primary_key`), not later in `Definitions::assemble` -
    // a `Model` with a dangling key cannot be built at all.
    let err = model("orders", "local", &["amount_cents", "order_date"])
        .with_primary_key([column("order_id")])
        .unwrap_err();
    assert_eq!(
        err,
        InconsistentDefinitions::UnknownPrimaryKeyColumn {
            model: model_name("orders"),
            column: column("order_id"),
        }
    );
}

#[test]
fn a_primary_key_naming_a_real_column_is_evidence_and_licenses_nothing_else() {
    // "Evidence only": a bundle with a primary key still assembles with no relationship at all, and
    // nothing here reads it to decide a `JoinType`.
    let orders = model("orders", "local", &["amount_cents", "order_date"])
        .with_primary_key([column("order_date")])
        .expect("order_date is one of orders' own columns");
    let definitions = Definitions::assemble(vec![orders], vec![], vec![]).expect("a real primary key column assembles");
    let orders = definitions.model(&model_name("orders")).expect("orders was assembled");
    assert_eq!(orders.primary_key(), &BTreeSet::from([column("order_date")]));
}

#[test]
fn a_column_s_type_and_description_count_toward_the_aggregate_byte_cap() {
    // An explicit bound rather than `DefinitionsByteBound::default()`'s 1 MiB, so this fixture
    // (sized against the former fixed 128 KiB bound) still crosses the cap it exercises.
    let bound = DefinitionsByteBound::parse(128 * 1024).expect("a positive bound inside the ceiling parses");
    // The two fields the bound did not cover before this type existed. That they move the DIGEST
    // too is `pinned::tests::a_column_s_type_or_description_arriving_moves_the_digest`; this
    // asserts the narrower claim, over the byte count `assemble` actually caps.
    let bare = model("orders", "local", &["amount_cents"]);
    assert!(
        Definitions::assemble_bounded(vec![bare], vec![], vec![], bound).is_ok(),
        "a bare column stays well under the cap"
    );
    // One column at the description cap, repeated enough times to cross the aggregate limit -
    // the same shape `enough_conforming_metrics_to_exceed_the_aggregate_cap_do_not_load` already
    // proves for a metric's own description.
    let mut columns = Vec::new();
    for index in 0..33_u32 {
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
        other => panic!("column description bytes must be what refuses this: {other:?}"),
    }
}

/// A review found that the test above proves nothing about `data_type`'s own byte count: every
/// column in it has `data_type: None`, so a mutation dropping the type's contribution entirely
/// (`data_type.as_str().len()` to `0`) left it green. Here every metric description stays under
/// the cap on its own, and only adding column TYPES on top crosses it - isolating the half the
/// other test could not.
#[test]
fn a_column_s_type_alone_can_push_the_bundle_over_the_aggregate_byte_cap() {
    // An explicit bound rather than `DefinitionsByteBound::default()`'s 1 MiB, so this fixture
    // (sized against the former fixed 128 KiB bound) still crosses the cap it exercises.
    let bound = DefinitionsByteBound::parse(128 * 1024).expect("a positive bound inside the ceiling parses");
    let description_filler =
        Description::parse("y".repeat(MAX_DESCRIPTION_BYTES)).expect("exactly the description cap is a description");
    // 31 metric descriptions at the cap: 126976 bytes, comfortably under 131072 -
    // `enough_conforming_metrics_to_exceed_the_aggregate_cap_do_not_load` already proves this half
    // alone loads.
    let metrics: Vec<Metric> = (0..31_u32)
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
                Audience::Open,
            )
            .expect("no dimensions to duplicate")
        })
        .collect();

    // The model with no typed column: descriptions alone, well under the cap.
    let bare_orders = model("orders", "local", &["amount_cents", "order_date"]);
    assert!(
        Definitions::assemble_bounded(vec![bare_orders], vec![], metrics.clone(), bound).is_ok(),
        "31 max descriptions and no column types stay under the cap"
    );

    // The remaining headroom (131072 - 126976 = 4096 bytes) cannot absorb nine columns at the
    // 512-character type cap (4608 bytes) plus their own names - so adding only TYPES, with every
    // description still empty, is what has to cross it.
    let type_filler = ColumnType::parse("x".repeat(MAX_COLUMN_TYPE_CHARS)).expect("exactly the type cap is a type");
    let mut typed_columns: Vec<Column> = vec![Column::from(column("amount_cents")), Column::from(column("order_date"))];
    typed_columns.extend((0..9_u32).map(|index| {
        Column::new(
            column(&format!("c{index}")),
            Some(type_filler.clone()),
            Description::default(),
            None,
        )
    }));
    let typed_orders = Model::new(
        model_name("orders"),
        SourceName::parse("local").expect("a test source is a source"),
        TableName::parse("orders").expect("a test table is a table"),
        typed_columns,
        Description::default(),
    );
    match Definitions::assemble_bounded(vec![typed_orders], vec![], metrics, bound).unwrap_err() {
        InconsistentDefinitions::DefinitionsTooLarge { bytes, limit, .. } => {
            assert_eq!(limit, bound.get());
            assert!(bytes > limit, "{bytes} must exceed {limit}");
        }
        other => panic!("column type bytes must be what refuses this: {other:?}"),
    }
}

/// `#2071`: a dictionary catalog between the former fixed 128 KiB bound and the new 1 MiB default
/// loads under the default - the issue's own complaint, that a real catalog this size refused to
/// load at all. `Definitions::assemble` (no explicit bound) is what every one of the eight catalog
/// adapters calls through `assemble_bounded`'s default, so this is what changes for an operator who
/// declares no `max_definitions_bytes`.
#[test]
fn a_catalog_between_the_former_fixed_bound_and_the_new_default_loads_under_the_default() {
    let mut columns = Vec::new();
    for index in 0..40_u32 {
        let filler = Description::parse("y".repeat(MAX_DESCRIPTION_BYTES)).expect("exactly the description cap is a description");
        columns.push(Column::new(column(&format!("c{index}")), None, filler, None));
    }
    // 40 * 4096 = 163840 bytes: past the former fixed 131072 bound, comfortably under the 1 MiB
    // default.
    let heavy = Model::new(
        model_name("orders"),
        SourceName::parse("local").expect("a test source is a source"),
        TableName::parse("orders").expect("a test table is a table"),
        columns,
        Description::default(),
    );
    assert!(
        Definitions::assemble(vec![heavy], vec![], vec![]).is_ok(),
        "a catalog sized between 128 KiB and 1 MiB loads under the new default"
    );
}

/// `#2071`: the bound is per-catalog, not only raised globally - a deployment may declare a lower
/// `max_definitions_bytes` than the default and have it refuse the same bundle the test above loads.
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
                Audience::Open,
            )
            .expect("no dimensions to duplicate")
        })
        .collect();
    let bare_orders = model("orders", "local", &["amount_cents", "order_date"]);
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

#[test]
fn many_empty_models_cannot_make_an_unbounded_schema_listing() {
    // An explicit bound rather than `DefinitionsByteBound::default()`'s 1 MiB, so this fixture
    // (sized against the former fixed 128 KiB bound) still crosses the cap it exercises.
    let bound = DefinitionsByteBound::parse(128 * 1024).expect("a positive bound inside the ceiling parses");
    let models = (0..2500_u32)
        .map(|index| {
            Model::new(
                model_name(&format!("m_{index:04}_{}", "x".repeat(48))),
                SourceName::parse("local").expect("a test source"),
                TableName::parse("t").expect("a test table"),
                Vec::<Column>::new(),
                Description::default(),
            )
        })
        .collect();
    assert!(
        matches!(
            Definitions::assemble_bounded(models, vec![], vec![], bound),
            Err(InconsistentDefinitions::DefinitionsTooLarge { .. })
        ),
        "model identifiers count toward the existing definition cap"
    );
}
