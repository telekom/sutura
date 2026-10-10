use std::num::NonZeroU64;

use sutura_domain::identity::Secret;

use super::{DriverError, OracleLogin, OracleReader, assemble, decode, flag, statement};
use crate::postgres_reader::{DEFAULT_MAX_DICTIONARY_BYTES, DEFAULT_MAX_DICTIONARY_ROWS, InvalidReaderConfig, RowPredicate};
use crate::{ColumnMetadata, Dictionary, DictionaryBounds, DictionaryReader as _, RdbmsError, Table, TableAddress};

fn login() -> OracleLogin {
    login_at("127.0.0.1")
}

fn login_at(host: &str) -> OracleLogin {
    OracleLogin::new(
        String::from(host),
        1521,
        String::from("FREEPDB1"),
        String::from("reader"),
        Secret::new("a-test-password"),
    )
}

fn equals(column: &str) -> RowPredicate {
    RowPredicate::Equals {
        column: String::from(column),
        value: String::from("live'; DROP TABLE t; --"),
    }
}

#[test]
fn the_oracle_reader_refuses_a_schema_or_predicate_column_that_is_not_an_identifier() {
    let reader = |schema: &str, predicate: RowPredicate| {
        OracleReader::new(login(), schema, String::from("prod"), predicate, None, None).map(drop)
    };
    assert_eq!(
        reader("dictionary\"; DROP USER app; --", RowPredicate::None),
        Err(InvalidReaderConfig::Schema)
    );
    assert_eq!(
        reader("dictionary", equals("state\" OR 1=1 --")),
        Err(InvalidReaderConfig::PredicateColumn)
    );
    assert_eq!(reader("dictionary", equals("state")), Ok(()));
}

#[test]
fn the_oracle_statement_quotes_folded_identifiers_and_binds_every_value() {
    let sql = statement("sutura_dictionary", &equals("state"));
    assert!(sql.contains("FROM \"SUTURA_DICTIONARY\".\"COLUMNS\""), "{sql}");
    assert!(
        sql.contains("WHERE is_deleted = 0 AND environment = :1 AND \"STATE\" = :2"),
        "{sql}"
    );
    assert!(
        !sql.contains("DROP"),
        "the equals value is bound, never written into the statement: {sql}"
    );
    assert!(
        statement("d", &RowPredicate::IsNull(String::from("gone"))).contains("= :1 AND \"GONE\" IS NULL ORDER BY"),
        "an is-null predicate binds nothing"
    );
}

#[test]
fn a_primary_key_flag_is_evidence_only_as_zero_or_one() {
    assert_eq!(flag(Some(1)), Some(true));
    assert_eq!(flag(Some(0)), Some(false));
    assert_eq!(flag(Some(2)), None);
    assert_eq!(flag(None), None);
}

#[test]
fn the_reader_binds_the_environment_and_the_equals_value_in_placeholder_order() {
    let reader = |predicate: RowPredicate| {
        OracleReader::new(login(), "dictionary", String::from("prod"), predicate, None, None).expect("a valid reader")
    };
    assert_eq!(reader(equals("state")).binds(), ["prod", "live'; DROP TABLE t; --"]);
    assert_eq!(reader(RowPredicate::IsNotNull(String::from("state"))).binds(), ["prod"]);
}

#[test]
fn an_ipv6_login_is_bracketed_in_the_connect_string() {
    assert_eq!(login_at("::1").address(), "[::1]:1521/FREEPDB1");
    assert_eq!(login_at("127.0.0.1").address(), "127.0.0.1:1521/FREEPDB1");
}

fn cap(value: u64) -> NonZeroU64 {
    NonZeroU64::new(value).expect("a nonzero test cap")
}

#[test]
fn the_oracle_reader_holds_the_declared_caps_or_the_documented_defaults() {
    let reader = |rows, bytes| {
        OracleReader::new(login(), "dictionary", String::from("prod"), RowPredicate::None, rows, bytes).expect("a valid reader")
    };
    assert_eq!(
        reader(Some(cap(500)), Some(cap(64_000))).bounds(),
        DictionaryBounds::new(cap(500), cap(64_000))
    );
    assert_eq!(
        reader(None, None).bounds(),
        DictionaryBounds::new(cap(DEFAULT_MAX_DICTIONARY_ROWS), cap(DEFAULT_MAX_DICTIONARY_BYTES))
    );
}

/// One row as the driver yields it, in [`statement`]'s `SELECT` order; the tenth cell is the
/// `NUMBER(1)` key flag.
type DriverRow = [Option<&'static str>; 10];

const ROWS: [DriverRow; 3] = [
    [
        Some("prod"),
        Some("FREEPDB1"),
        Some("SALES"),
        Some("ORDERS"),
        Some("orders"),
        Some("One row per order"),
        Some("ID"),
        Some("NUMBER(10)"),
        Some("The order key"),
        Some("1"),
    ],
    [
        Some("prod"),
        Some("FREEPDB1"),
        Some("SALES"),
        Some("ORDERS"),
        Some("orders"),
        Some("One row per order"),
        Some("TOTAL"),
        Some("NUMBER(12,2)"),
        None,
        Some("0"),
    ],
    [
        Some("prod"),
        None,
        Some("SALES"),
        Some("CUSTOMERS"),
        None,
        None,
        Some("ID"),
        None,
        None,
        Some("1"),
    ],
];

fn read(rows: &[DriverRow], bounds: DictionaryBounds) -> Result<Dictionary, RdbmsError> {
    assemble(
        "prod",
        bounds,
        rows.iter().map(|cells| {
            decode(
                |index| Ok(cells[index].map(String::from)),
                |index| Ok(cells[index].map(|cell| cell.parse().expect("a NUMBER cell"))),
            )
        }),
    )
}

#[test]
fn the_oracle_decoder_assembles_positional_values_into_this_dictionary() {
    let text = |value: &str| String::from(value);
    let customers = Table::new(
        text("CUSTOMERS"),
        TableAddress::new(None, text("SALES"), text("CUSTOMERS")),
        vec![text("ID")],
        None,
    )
    .with_primary_key(vec![text("ID")]);
    let orders = Table::new(
        text("orders"),
        TableAddress::new(Some(text("FREEPDB1")), text("SALES"), text("ORDERS")),
        vec![text("ID"), text("TOTAL")],
        Some(text("One row per order")),
    )
    .with_column_metadata([
        (
            text("ID"),
            ColumnMetadata::new(Some(text("NUMBER(10)")), Some(text("The order key"))),
        ),
        (text("TOTAL"), ColumnMetadata::new(Some(text("NUMBER(12,2)")), None)),
    ])
    .with_primary_key(vec![text("ID")]);
    assert_eq!(
        read(&ROWS, DictionaryBounds::new(cap(10), cap(10_000))).expect("the rows assemble"),
        Dictionary::new(vec![customers, orders], Vec::new())
    );
}

#[test]
fn the_oracle_assembly_refuses_the_row_past_the_declared_row_cap() {
    read(&ROWS[..2], DictionaryBounds::new(cap(2), cap(10_000))).expect("the row at the cap is admitted");
    let refused = read(&ROWS, DictionaryBounds::new(cap(2), cap(10_000))).expect_err("the third row crosses the cap");
    assert_eq!(
        std::error::Error::source(&refused).map(ToString::to_string).as_deref(),
        Some("the dictionary stream reached the declared maximum of 2 rows")
    );
}

#[test]
fn the_oracle_assembly_stops_at_the_first_row_the_driver_fails() {
    let pulled = std::cell::Cell::new(0);
    let rows = ROWS.iter().enumerate().map(|(at, cells)| {
        pulled.set(pulled.get() + 1);
        decode(
            |index| {
                if at == 1 && index == 8 {
                    Err(oracledb::Error::from(std::io::Error::other("a driver fault")))
                } else {
                    Ok(cells[index].map(String::from))
                }
            },
            |index| Ok(cells[index].map(|cell| cell.parse().expect("a NUMBER cell"))),
        )
    });
    let refused =
        assemble("prod", DictionaryBounds::new(cap(10), cap(10_000)), rows).expect_err("the driver's fault refuses the read");
    assert!(
        matches!(std::error::Error::source(&refused), Some(cause) if cause.is::<DriverError>()),
        "{refused:?}"
    );
    assert_eq!(pulled.get(), 2, "the cursor is abandoned at the refused row");
}

/// A listener that answers the login with a redirect is refused, typed, and the address it named is
/// never dialled - so no login reaches it.
#[test]
fn the_oracle_reader_refuses_a_listener_redirect_before_authentication() {
    let listener = sutura_dev::tns_listener::RedirectingListener::start().expect("the fake listener binds");
    let login = OracleLogin::new(
        String::from("127.0.0.1"),
        listener.port(),
        String::from("FREEPDB1"),
        String::from("reader"),
        Secret::new("a-test-password"),
    );
    let reader = OracleReader::new(login, "dictionary", String::from("prod"), RowPredicate::None, None, None)
        .expect("a valid reader config");
    let error = reader.read_dictionary().expect_err("a redirecting listener is refused");
    assert!(matches!(error, RdbmsError::RedirectRefused), "{error:?}");
    assert!(
        !listener.target_was_dialled(std::time::Duration::from_secs(2)),
        "the address the redirect named was dialled"
    );
}
