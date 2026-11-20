use std::num::NonZeroU64;

use sutura_domain::catalog::Collation;

use super::{Assembly, Row};
use crate::{ColumnMetadata, DictionaryBounds};

fn row(column: &str) -> Row {
    Row {
        environment: Some(String::from("prod")),
        schema_name: Some(String::from("sales")),
        table_name: Some(String::from("orders")),
        column_name: Some(String::from(column)),
        is_primary_key: Some(false),
        ..Row::default()
    }
}

#[test]
fn a_collation_is_kept_with_a_column_that_has_no_type_or_prose_and_unknown_is_kept_as_nothing() {
    let bound = |value| NonZeroU64::new(value).expect("a nonzero test bound");
    let mut assembly = Assembly::new("prod", DictionaryBounds::new(bound(10), bound(1_000)));
    let mut exact = row("exact");
    exact.column_collation = Collation::Binary;
    let mut folded = row("folded");
    folded.column_collation = Collation::named("und:ci");
    for pushed in [exact, folded, row("silent")] {
        assembly.push(pushed).expect("a well-formed row");
    }
    let dictionary = assembly.finish();
    let [orders] = dictionary.tables() else {
        panic!("one table: {dictionary:?}")
    };
    let read = |column| orders.column_metadata(column).map(ColumnMetadata::collation);
    assert_eq!(read("exact"), Some(&Collation::Binary));
    assert_eq!(read("folded"), Some(&Collation::named("und:ci")));
    assert_eq!(read("silent"), None, "a column with no evidence at all carries no metadata");
}
