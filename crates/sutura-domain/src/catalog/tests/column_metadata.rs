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
    // The two fields `MAX_DEFINITIONS_BYTES` did not bound before this type existed. That they move
    // the DIGEST too is `pinned::tests::a_column_s_type_or_description_arriving_moves_the_digest`;
    // this asserts the narrower claim, over the byte count `assemble` actually caps.
    let bare = model("orders", "local", &["amount_cents"]);
    assert!(
        Definitions::assemble(vec![bare], vec![], vec![]).is_ok(),
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
    match Definitions::assemble(vec![heavy], vec![], vec![]).unwrap_err() {
        InconsistentDefinitions::DefinitionsTooLarge { bytes, limit } => {
            assert_eq!(limit, MAX_DEFINITIONS_BYTES);
            assert!(bytes > MAX_DEFINITIONS_BYTES, "{bytes} must exceed {MAX_DEFINITIONS_BYTES}");
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
        Definitions::assemble(vec![bare_orders], vec![], metrics.clone()).is_ok(),
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
    match Definitions::assemble(vec![typed_orders], vec![], metrics).unwrap_err() {
        InconsistentDefinitions::DefinitionsTooLarge { bytes, limit } => {
            assert_eq!(limit, MAX_DEFINITIONS_BYTES);
            assert!(bytes > MAX_DEFINITIONS_BYTES, "{bytes} must exceed {MAX_DEFINITIONS_BYTES}");
        }
        other => panic!("column type bytes must be what refuses this: {other:?}"),
    }
}

#[test]
fn many_empty_models_cannot_make_an_unbounded_schema_listing() {
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
            Definitions::assemble(models, vec![], vec![]),
            Err(InconsistentDefinitions::DefinitionsTooLarge { .. })
        ),
        "model identifiers count toward the existing definition cap"
    );
}
