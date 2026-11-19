use std::num::NonZeroU64;

use super::{Assembly, Row};
use crate::DictionaryBounds;

fn bounds(rows: u64, bytes: u64) -> DictionaryBounds {
    DictionaryBounds::new(
        NonZeroU64::new(rows).expect("a nonzero test bound"),
        NonZeroU64::new(bytes).expect("a nonzero test bound"),
    )
}

fn row(table: &str, column: &str, key: bool) -> Row {
    Row {
        environment: Some(String::from("prod")),
        schema_name: Some(String::from("sales")),
        table_name: Some(String::from(table)),
        column_name: Some(String::from(column)),
        is_primary_key: Some(key),
        ..Row::default()
    }
}

#[track_caller]
fn refusal(result: Result<(), crate::RdbmsError>) -> String {
    let error = result.expect_err("the row is refused");
    std::error::Error::source(&error).map_or_else(String::new, ToString::to_string)
}

#[test]
#[cfg(feature = "live-oracle")]
fn an_assembly_refuses_the_row_that_crosses_either_declared_cap() {
    let mut rows = Assembly::new("prod", bounds(1, 1_000));
    rows.admit(10).expect("the first row is within both caps");
    assert_eq!(
        refusal(rows.admit(10)),
        "the dictionary stream reached the declared maximum of 1 rows"
    );

    let mut bytes = Assembly::new("prod", bounds(10, 25));
    bytes.admit(20).expect("20 of 25 bytes is within the cap");
    assert_eq!(
        refusal(bytes.admit(6)),
        "the dictionary stream reached the declared maximum of 25 bytes"
    );
}

#[test]
fn an_assembly_refuses_a_row_that_breaks_the_views_contract() {
    let mut other = row("orders", "id", true);
    other.environment = Some(String::from("staging"));
    let mut unkeyed = row("orders", "id", true);
    unkeyed.is_primary_key = None;
    let mut renamed = row("orders", "total", false);
    renamed.model_name = Some(String::from("sales_orders"));
    let mut redescribed = row("orders", "total", false);
    redescribed.table_description = Some(String::from("another story"));
    let mut unplaced = row("orders", "total", false);
    unplaced.schema_name = None;
    let mut untabled = row("orders", "total", false);
    untabled.table_name = Some(String::new());
    for (bad, words) in [
        (other, "environment did not match"),
        (unkeyed, "no primary-key evidence"),
        (row("orders", "id", false), "repeated column id"),
        (renamed, "disagree about one table's model"),
        (redescribed, "disagree about one table's model or description"),
        (unplaced, "no schema and table identity"),
        (untabled, "no schema and table identity"),
    ] {
        let mut assembly = Assembly::new("prod", bounds(10, 1_000));
        assembly
            .push(row("orders", "id", true))
            .expect("the first row is well-formed");
        let refused = refusal(assembly.push(bad));
        assert!(refused.contains(words), "expected `{words}` in: {refused}");
    }
}

#[test]
fn an_assembly_gathers_rows_into_keyed_tables() {
    let mut assembly = Assembly::new("prod", bounds(10, 1_000));
    for (table, column, key) in [("orders", "id", true), ("orders", "total", false), ("customers", "id", true)] {
        assembly.push(row(table, column, key)).expect("a well-formed row");
    }
    let dictionary = assembly.finish();
    let orders = dictionary
        .tables()
        .iter()
        .find(|table| table.model() == "orders")
        .expect("orders is read");
    assert_eq!(orders.columns(), ["id", "total"]);
    assert_eq!(orders.primary_key(), ["id"]);
    assert_eq!(dictionary.tables().len(), 2);
    assert_eq!(dictionary.relationships().len(), 0, "no foreign keys are read");
}

#[test]
#[cfg(feature = "live-oracle")]
fn a_rows_decoded_length_counts_its_text_and_is_never_zero() {
    assert_eq!(Row::default().decoded_len(), 1);
    assert_eq!(
        row("orders", "id", true).decoded_len(),
        18,
        "prod + sales + orders + id, plus one"
    );
}

#[test]
fn a_collation_is_kept_with_a_column_that_has_no_type_or_prose_and_unknown_is_kept_as_nothing() {
    use sutura_domain::catalog::Collation;

    let mut assembly = Assembly::new("prod", bounds(10, 1_000));
    let mut exact = row("orders", "exact", false);
    exact.column_collation = Collation::Binary;
    let mut folded = row("orders", "folded", false);
    folded.column_collation = Collation::named("und:ci");
    for pushed in [exact, folded, row("orders", "silent", false)] {
        assembly.push(pushed).expect("a well-formed row");
    }
    let dictionary = assembly.finish();
    let [orders] = dictionary.tables() else {
        panic!("one table: {dictionary:?}")
    };
    let read = |column| orders.column_metadata(column).map(crate::ColumnMetadata::collation);
    assert_eq!(read("exact"), Some(&Collation::Binary));
    assert_eq!(read("folded"), Some(&Collation::named("und:ci")));
    assert_eq!(read("silent"), None, "a column with no evidence at all carries no metadata");
}
