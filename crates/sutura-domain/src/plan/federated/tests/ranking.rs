//! Case 2's own ranking cells - `github.com/telekom/sutura#777`. Split out of `super` for that
//! file's own `max-lines` reason, not for thematic tidiness, so `use super::*` reaches every
//! fixture (`metric`) exactly as these tests read it before the move.

use super::*;
use crate::catalog::TIME_BUCKET_LABEL;
use crate::query::{Top, TopBy, TopDirection, TopN};
use crate::warehouse::{RowSet, Value};

/// One combined answer, ranked and truncated - `github.com/telekom/sutura#777`'s case 2. Built
/// directly rather than through a combiner, because `rank`'s own contract is about the ORDER an
/// already-combined answer comes back in, not about the join - which is why `rank` stayed in the
/// domain when `docs/adr/0039` step 3 moved the combine out: it reads the answer's own column
/// order and no engine.
fn ranked(top: Top) -> RowSet {
    let combined = RowSet::new(
        vec![
            String::from("product_family"),
            String::from("region"),
            String::from(TIME_BUCKET_LABEL),
            String::from(ResultLabel::measure(&metric("revenue")).as_str()),
        ],
        vec![
            vec![
                Value::Text("A".into()),
                Value::Text("north".into()),
                Value::Text("2026-06".into()),
                Value::Integer(100),
            ],
            vec![
                Value::Text("B".into()),
                Value::Text("south".into()),
                Value::Text("2026-06".into()),
                Value::Integer(300),
            ],
            // Two null-measured groups, in this order - a group with nothing to rank, twice.
            vec![
                Value::Text("C".into()),
                Value::Text("east".into()),
                Value::Text("2026-06".into()),
                Value::Null,
            ],
            vec![
                Value::Text("D".into()),
                Value::Text("west".into()),
                Value::Text("2026-06".into()),
                Value::Null,
            ],
        ],
    )
    .expect("a hand-built combined answer is well formed");
    FederatedPlan::rank(&combined, top).expect("ranking four already well-formed rows cannot become malformed")
}

fn families(rows: &RowSet) -> Vec<&str> {
    let at = rows.column_index("product_family").expect("product_family is projected");
    rows.rows()
        .iter()
        .map(|row| match row.get(at) {
            Some(Value::Text(text)) => text.as_str(),
            other => panic!("product_family cell is not text: {other:?}"),
        })
        .collect()
}

#[test]
fn ranking_descending_puts_the_largest_measure_first_and_nulls_last_regardless() {
    let top = Top::new(
        TopN::parse(4).expect("four is a row count"),
        TopBy::Metric,
        TopDirection::Desc,
    );
    assert_eq!(
        families(&ranked(top)),
        vec!["B", "A", "C", "D"],
        "300 outranks 100, and both nulls sort after either - stable in their own input order"
    );
}

#[test]
fn ranking_ascending_still_puts_nulls_last_rather_than_first() {
    // The negative control for the cell above: reversing `desc` reverses A and B, and a
    // mutation that let `desc` also flip null placement would move `C`/`D` to the front here -
    // this is what tells "nulls sort last" apart from "nulls sort last only when descending".
    let top = Top::new(TopN::parse(4).expect("four is a row count"), TopBy::Metric, TopDirection::Asc);
    assert_eq!(
        families(&ranked(top)),
        vec!["A", "B", "C", "D"],
        "100 outranks 300 ascending, and nulls still sort after both - not first"
    );
}

#[test]
fn ranking_truncates_to_top_n_after_ordering() {
    let top = Top::new(TopN::parse(1).expect("one is a row count"), TopBy::Metric, TopDirection::Desc);
    assert_eq!(families(&ranked(top)), vec!["B"], "top 1 keeps only the largest measure");
}
