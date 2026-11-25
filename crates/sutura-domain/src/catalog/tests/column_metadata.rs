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
#[expect(
    clippy::integer_division,
    clippy::integer_division_remainder_used,
    reason = "a fixture count derived from two byte constants, not a measurement - truncation is \
              the point: it is how many whole items of the divisor's size fit the dividend"
)]
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
    // proves for a metric's own description. Sized from the constants, not a hand-counted literal,
    // so it exceeds the bound on either tree's value for `MAX_DEFINITIONS_BYTES`.
    let column_count = MAX_DEFINITIONS_BYTES / MAX_DESCRIPTION_BYTES + 1;
    let column_count = u32::try_from(column_count).expect("well under u32::MAX");
    let mut columns = Vec::new();
    for index in 0..column_count {
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
        InconsistentDefinitions::DefinitionsTooLarge { bytes, limit, .. } => {
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
#[expect(
    clippy::integer_division,
    clippy::integer_division_remainder_used,
    reason = "a fixture count derived from byte constants, not a measurement - truncation is the \
              point: it is how many whole items of the divisor's size fit the dividend"
)]
fn a_column_s_type_alone_can_push_the_bundle_over_the_aggregate_byte_cap() {
    let description_filler =
        Description::parse("y".repeat(MAX_DESCRIPTION_BYTES)).expect("exactly the description cap is a description");
    // One metric description short of the aggregate cap's own count: comfortably under the bound on
    // either tree's value for `MAX_DEFINITIONS_BYTES` -
    // `enough_conforming_metrics_to_exceed_the_aggregate_cap_do_not_load` already proves this half
    // alone loads.
    let metrics_count = MAX_DEFINITIONS_BYTES / MAX_DESCRIPTION_BYTES - 1;
    let metrics_count_u32 = u32::try_from(metrics_count).expect("well under u32::MAX");
    let metrics: Vec<Metric> = (0..metrics_count_u32)
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
        "one short of the cap's own count of max descriptions, with no column types, stays under the cap"
    );

    // The remaining headroom - the cap minus the metric descriptions already counted above -
    // cannot absorb enough columns at the type cap plus their own names, so adding only TYPES, with
    // every description still empty, is what has to cross it. The `+ 2` is slack for the column
    // names' own bytes and the model's own name/table bytes, neither counted in `headroom` itself.
    let headroom = MAX_DEFINITIONS_BYTES - metrics_count * MAX_DESCRIPTION_BYTES;
    let typed_column_count = headroom / MAX_COLUMN_TYPE_CHARS + 2;
    let typed_column_count = u32::try_from(typed_column_count).expect("well under u32::MAX");
    let type_filler = ColumnType::parse("x".repeat(MAX_COLUMN_TYPE_CHARS)).expect("exactly the type cap is a type");
    let mut typed_columns: Vec<Column> = vec![Column::from(column("amount_cents")), Column::from(column("order_date"))];
    typed_columns.extend((0..typed_column_count).map(|index| {
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
        InconsistentDefinitions::DefinitionsTooLarge { bytes, limit, .. } => {
            assert_eq!(limit, MAX_DEFINITIONS_BYTES);
            assert!(bytes > MAX_DEFINITIONS_BYTES, "{bytes} must exceed {MAX_DEFINITIONS_BYTES}");
        }
        other => panic!("column type bytes must be what refuses this: {other:?}"),
    }
}

#[test]
#[expect(
    clippy::integer_division,
    clippy::integer_division_remainder_used,
    reason = "a fixture count derived from two byte constants, not a measurement - truncation is \
              the point: it is how many whole items of the divisor's size fit the dividend"
)]
fn many_empty_models_cannot_make_an_unbounded_schema_listing() {
    // Sized from `MAX_DEFINITIONS_BYTES` rather than a hand-counted literal, so it exceeds the bound
    // on either tree's value. A fixed-width, zero-padded index keeps every name the same length
    // (well under `MAX_IDENTIFIER_LEN`) regardless of how many models that takes.
    let name_len = format!("m_{:06}_{}", 0_u32, "x".repeat(48)).len();
    let model_count = MAX_DEFINITIONS_BYTES / name_len + 2;
    let model_count = u32::try_from(model_count).expect("well under u32::MAX");
    let models = (0..model_count)
        .map(|index| {
            Model::new(
                model_name(&format!("m_{index:06}_{}", "x".repeat(48))),
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
