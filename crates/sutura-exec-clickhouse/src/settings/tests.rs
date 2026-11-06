use super::{Lifted, with_clause};

#[test]
fn a_bare_select_takes_the_clause_once_at_its_end() {
    assert_eq!(
        with_clause("SELECT \"t\".\"x\" FROM \"t\" GROUP BY toDate(\"t\".\"day\")", 30),
        Ok(String::from(
            "SELECT \"t\".\"x\" FROM \"t\" GROUP BY toDate(\"t\".\"day\") SETTINGS max_execution_time = 30, \
             join_use_nulls = 1, output_format_json_quote_denormals = 1, output_format_json_quote_64bit_integers = 1, \
             output_format_json_quote_decimals = 1, output_format_decimal_trailing_zeros = 1, \
             timeout_overflow_mode = 'throw'"
        ))
    );
    let stacked = "SELECT \"k\" FROM (SELECT 1 AS \"k\" UNION ALL SELECT 2 AS \"k\") AS \"stacked\"";
    assert!(
        with_clause(stacked, 30).is_ok(),
        "a set operation inside parentheses is not the top level"
    );
    let quoted = "SELECT 'UNION' AS \"SETTINGS\", `FORMAT` FROM \"t\"";
    assert!(with_clause(quoted, 30).is_ok(), "a quoted keyword is not one");
}

#[test]
fn a_statement_whose_clause_would_be_lifted_past_the_switch_is_refused() {
    for (select, keyword) in [
        ("SELECT 1 SETTINGS join_use_nulls = 0", "SETTINGS"),
        ("SELECT 1 FORMAT TSV", "FORMAT"),
        ("(SELECT 1) UNION ALL (SELECT 2)", "UNION"),
        ("select 1 intersect select 2", "INTERSECT"),
        ("SELECT 1 EXCEPT SELECT 2", "EXCEPT"),
    ] {
        assert_eq!(with_clause(select, 30), Err(Lifted(keyword)), "{select}");
    }
}
