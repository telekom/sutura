use super::{CellName, create_user, drop_user, grant};
use crate::fixture::FixtureError;

#[test]
fn a_cell_is_named_after_its_process_and_its_ordinal() {
    let cell = CellName::of(7, 3);
    assert_eq!(cell.as_str(), "CELL_7_3");
    assert_eq!(cell.to_string(), "CELL_7_3");
    assert_ne!(cell, CellName::of(7, 4));
    assert_ne!(cell, CellName::of(8, 3));
}

#[test]
fn the_cell_user_is_a_quoted_identifier_holding_the_privileges_a_load_needs_and_no_more() {
    let cell = CellName::of(7, 3);
    assert_eq!(drop_user(&cell), "DROP USER \"CELL_7_3\" CASCADE");
    assert_eq!(
        create_user(&cell, "p4ss").expect("a quotable password"),
        "CREATE USER \"CELL_7_3\" IDENTIFIED BY \"p4ss\""
    );
    assert_eq!(
        grant(&cell),
        "GRANT CREATE SESSION, CREATE TABLE, UNLIMITED TABLESPACE TO \"CELL_7_3\""
    );
}

#[test]
fn a_password_that_holds_the_closing_quote_is_refused_before_a_statement_is_built() {
    let refused = create_user(&CellName::of(7, 3), "a\"; DROP USER SYSTEM; --");
    assert!(matches!(refused, Err(FixtureError::PasswordNotQuotable)), "{refused:?}");
}
