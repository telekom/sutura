//! What the probe's vocabulary claims, held against itself.

use std::collections::BTreeSet;

use super::{
    CountsNotRead, DISTINCT_LABEL, DeclaredKey, ImpossibleCounts, KeyCounts, KeyNotUnique, KeyUniqueness, NoDeclaredKey,
    ROWS_LABEL,
};
use crate::catalog::{Definitions, Description, Model, Relationship};
use crate::model::{ColumnName, JoinType, ModelName, RelationshipName, SourceName, TableName};
use crate::warehouse::{RowSet, Value};

fn column(raw: &str) -> ColumnName {
    ColumnName::parse(raw).expect("a test column is a column")
}

fn model_name(raw: &str) -> ModelName {
    ModelName::parse(raw).expect("a test model is a model")
}

fn model(name: &str, columns: &[&str]) -> Model {
    Model::new(
        model_name(name),
        SourceName::parse("local").expect("a test source is a source"),
        TableName::parse(name).expect("a test table is a table"),
        columns.iter().map(|c| column(c)).collect::<BTreeSet<_>>(),
        Description::default(),
    )
}

fn joined(target_model: &str, target_column: &str, join_type: JoinType) -> Relationship {
    Relationship::new(
        RelationshipName::parse("orders_customer").expect("a test relationship is a relationship"),
        model_name("orders"),
        column("customer_key"),
        model_name(target_model),
        column(target_column),
        join_type,
    )
}

/// The two models the relationships below join, with no metric over them.
fn definitions() -> Definitions {
    Definitions::assemble(
        vec![
            model("orders", &["amount_cents", "order_date", "customer_key"]),
            model("customers", &["customer_key", "region"]),
        ],
        Vec::new(),
        Vec::new(),
    )
    .expect("two models and no joins are consistent")
}

/// A `RowSet` in the shape the probe's statement projects.
fn counted(rows: Value, distinct: Value) -> RowSet {
    RowSet::new(
        vec![String::from(ROWS_LABEL), String::from(DISTINCT_LABEL)],
        vec![vec![rows, distinct]],
    )
    .expect("two labels and one two-cell row are a result set")
}

#[test]
fn both_join_types_that_promise_a_unique_target_yield_a_probe() {
    let definitions = definitions();
    for join_type in [JoinType::ManyToOne, JoinType::OneToOne] {
        let relationship = joined("customers", "customer_key", join_type);
        let key = DeclaredKey::promised_by(&relationship, &definitions).expect("this join type promises a unique target");
        assert_eq!(key.model().as_str(), "customers");
        assert_eq!(key.column().as_str(), "customer_key");
        assert_eq!(key.source().as_str(), "local");
        assert_eq!(key.table().to_string(), "customers");
        assert_eq!(key.relationship().as_str(), "orders_customer");
    }
}

/// The one join type that promises nothing, refused as *nothing to ask* rather than as a violation.
///
/// A probe over a `one_to_many` target would refuse a bundle for holding exactly the shape it
/// declared, which is why the refusal is in the constructor rather than in whatever reads the counts.
#[test]
fn a_one_to_many_target_promises_nothing_and_yields_no_probe() {
    let definitions = definitions();
    let relationship = joined("customers", "customer_key", JoinType::OneToMany);
    assert_eq!(
        DeclaredKey::promised_by(&relationship, &definitions),
        Err(NoDeclaredKey::MayDuplicateRows {
            join_type: JoinType::OneToMany
        })
    );
}

#[test]
fn a_target_model_or_column_the_definitions_do_not_carry_yields_no_probe() {
    let definitions = definitions();
    assert_eq!(
        DeclaredKey::promised_by(&joined("elsewhere", "customer_key", JoinType::ManyToOne), &definitions),
        Err(NoDeclaredKey::ModelUndefined {
            model: model_name("elsewhere")
        })
    );
    assert_eq!(
        DeclaredKey::promised_by(&joined("customers", "absent", JoinType::ManyToOne), &definitions),
        Err(NoDeclaredKey::ColumnNotOnModel {
            model: model_name("customers"),
            column: column("absent"),
        })
    );
}

#[test]
fn counts_that_agree_are_unique_and_counts_that_do_not_carry_the_surplus() {
    let clean = KeyCounts::parse(40, 40).expect("forty rows and forty keys are possible");
    assert!(clean.is_unique());
    assert_eq!(clean.duplicated(), 0);

    let violated = KeyCounts::parse(41, 40).expect("forty-one rows and forty keys are possible");
    assert!(!violated.is_unique());
    assert_eq!(violated.duplicated(), 1);
    assert_eq!(violated.rows(), 41);
    assert_eq!(violated.distinct(), 40);
}

/// The two pairs a single column of a single table cannot produce, refused by the constructor.
///
/// **The second was accepted until review found it.** `parse(41, 0)` answered `Ok`, and the
/// violation it licensed described forty-one rows under no key at all - a table that cannot exist,
/// printed into a boot refusal. An EMPTY column is not the same shape and stays legal: a dimension
/// table whose key column is entirely null counts `0` over `0`, which is unique and true.
#[test]
fn more_distinct_values_than_values_is_not_a_pair_of_counts() {
    assert_eq!(
        KeyCounts::parse(40, 41),
        Err(ImpossibleCounts::MoreDistinctThanRows { rows: 40, distinct: 41 })
    );
    assert_eq!(KeyCounts::parse(41, 0), Err(ImpossibleCounts::NoDistinctValue { rows: 41 }));
    let empty = KeyCounts::parse(0, 0).expect("a column of nothing but nulls counts zero over zero");
    assert!(empty.is_unique());
    assert_eq!(empty.duplicated(), 0);
}

/// **The constructor that decides whether the boot path refuses, asked directly.**
///
/// It exists because a mutation proved the suite blind: forcing [`KeyNotUnique::found`] to answer
/// `Some` unconditionally left `cargo nextest -p sutura-domain -E 'test(cardinality)'` at exit 0.
/// Every other cell here reaches `found` through nothing at all - the boot decision is
/// `sutura-app`'s - so the one function that turns two counts into a refusal had no test of its own
/// and read as covered.
#[test]
fn a_violation_is_found_only_where_the_counts_show_one() {
    let definitions = definitions();
    let relationship = joined("customers", "customer_key", JoinType::ManyToOne);
    let key = DeclaredKey::promised_by(&relationship, &definitions).expect("a many-to-one promises a unique target");

    let clean = KeyCounts::parse(40, 40).expect("forty over forty is possible");
    assert_eq!(
        KeyNotUnique::found(&key, clean),
        None,
        "counts that hold the declaration up must not produce a refusal"
    );

    let violated = KeyCounts::parse(41, 40).expect("forty-one over forty is possible");
    let found = KeyNotUnique::found(&key, violated).expect("forty-one rows under forty keys is a violation");
    assert_eq!(found.relationship().as_str(), "orders_customer");
    assert_eq!(found.model().as_str(), "customers");
    assert_eq!(found.column().as_str(), "customer_key");
    assert_eq!(found.counts(), violated);

    // **What the refusal says, asserted on the rendering rather than on the fields**, because the
    // fields are what a caller reads and the sentence is what an operator reads. Every part of it is
    // a parsed name or a count - there is no cell of the dimension table in it, and there is no
    // field on this type that could carry one.
    let said = found.to_string();
    for part in ["orders_customer", "customers", "customer_key", "41", "40"] {
        assert!(said.contains(part), "the refusal must name {part}: {said}");
    }
}

#[test]
fn a_probe_result_reads_back_as_the_counts_it_projected() {
    let read = KeyUniqueness::read(&counted(Value::Integer(41), Value::Integer(40))).expect("this is the projected shape");
    assert!(read.was_asked());
    assert_eq!(
        read,
        KeyUniqueness::Counted(KeyCounts::parse(41, 40).expect("forty-one over forty is possible"))
    );
}

/// **Not asked is not counted**, which is the whole reason the two are different values.
#[test]
fn the_default_answer_says_nobody_counted() {
    assert!(!KeyUniqueness::NotAsked.was_asked());
}

/// Every way a probe's result set can fail to be two counts, each named rather than collapsed.
#[test]
fn a_result_that_is_not_two_counts_is_refused_by_the_half_that_is_wrong() {
    let no_rows = RowSet::new(vec![String::from(ROWS_LABEL), String::from(DISTINCT_LABEL)], Vec::new())
        .expect("an empty result set is a result set");
    assert_eq!(KeyUniqueness::read(&no_rows), Err(CountsNotRead::NotOneRow { rows: 0 }));

    let mislabelled = RowSet::new(
        vec![String::from("rows"), String::from(DISTINCT_LABEL)],
        vec![vec![Value::Integer(1), Value::Integer(1)]],
    )
    .expect("two labels and one two-cell row are a result set");
    assert_eq!(
        KeyUniqueness::read(&mislabelled),
        Err(CountsNotRead::NoColumn { label: ROWS_LABEL })
    );

    assert_eq!(
        KeyUniqueness::read(&counted(Value::Text(String::from("41")), Value::Integer(40))),
        Err(CountsNotRead::NotACount {
            label: ROWS_LABEL,
            value: Value::Text(String::from("41")),
        })
    );
    assert_eq!(
        KeyUniqueness::read(&counted(Value::Integer(-1), Value::Integer(0))),
        Err(CountsNotRead::NegativeCount {
            label: ROWS_LABEL,
            value: -1
        })
    );
    assert_eq!(
        KeyUniqueness::read(&counted(Value::Integer(40), Value::Integer(41))),
        Err(CountsNotRead::Impossible {
            cause: ImpossibleCounts::MoreDistinctThanRows { rows: 40, distinct: 41 }
        })
    );
}
