//! The `ORDER BY` list for a `top` question: the caller's own key first, the plan's existing
//! tie-break after it.
//!
//! **The tie-break is not decoration.** It is the same group-key ordering
//! [`super::generate()`] already emits for a question with no `top`, so two rows tied on the
//! caller's own key still come back in the one order every dialect agrees on - without it, which
//! row lands at position `n` would be a fact about the executor rather than about the question.

use polyglot_sql::builder::Expr;
use sutura_domain::query::{Top, TopBy, TopDirection};

/// Ranks by `top.by()` in `top.direction()`, then appends `tiebreak` unchanged.
///
/// `measure` and `bucket` are the two exprs [`super::generate()`] already built for the projection,
/// passed in rather than rebuilt: [`TopBy::Metric`] takes the one already computed for the measure
/// column, [`TopBy::Period`] the one for the time bucket.
pub(super) fn ordering(top: Top, measure: Expr, bucket: Expr, tiebreak: Vec<Expr>) -> Vec<Expr> {
    let primary = match top.by() {
        TopBy::Metric => measure,
        TopBy::Period => bucket,
    };
    let desc = matches!(top.direction(), TopDirection::Desc);
    let mut ordering = Vec::with_capacity(tiebreak.len().saturating_add(1));
    ordering.push(super::ordered(primary, desc));
    ordering.extend(tiebreak);
    ordering
}

#[cfg(test)]
mod tests {
    use polyglot_sql::builder;
    use sutura_domain::query::{Top, TopBy, TopDirection, TopN};

    use crate::dialect::Dialect;

    /// Renders `ordered` as the whole `ORDER BY` of a minimal statement, so the assertion reads
    /// actual SQL rather than a guess at the layer's internal shape - the same idiom
    /// `super::super::tests::every_order_by_states_nulls_last` uses for the tie-break this key
    /// precedes.
    fn rendered(ordered: Vec<super::Expr>) -> String {
        let ast = builder::select(vec![builder::col("measure"), builder::col("period")])
            .from("t")
            .order_by(ordered)
            .build();
        super::super::render(&ast, Dialect::DuckDb).expect("an ORDER BY renders")
    }

    #[test]
    fn by_metric_ranks_the_measure_first_and_keeps_the_tiebreak_after_it() {
        let top = Top::new(
            TopN::parse(10).expect("ten is a row count"),
            TopBy::Metric,
            TopDirection::Desc,
        );
        let ordered = super::ordering(
            top,
            builder::col("measure"),
            builder::col("period"),
            vec![super::super::ordered_nulls_last(builder::col("key"))],
        );
        let sql = rendered(ordered);
        // The measure ranks first and descending; the tie-break key follows it, unchanged.
        let measure_at = sql.find("measure").expect("the measure column is ordered by");
        let key_at = sql.find("key").expect("the tie-break key follows it");
        assert!(measure_at < key_at, "{sql}");
        assert!(sql.contains("DESC"), "{sql}");
    }

    #[test]
    fn by_period_ranks_the_bucket_rather_than_the_measure() {
        let top = Top::new(TopN::parse(5).expect("five is a row count"), TopBy::Period, TopDirection::Asc);
        let ordered = super::ordering(top, builder::col("measure"), builder::col("period"), Vec::new());
        let sql = rendered(ordered);
        let order_by = sql.split("ORDER BY").nth(1).expect("an ORDER BY clause is present");
        assert!(order_by.contains("period"), "{sql}");
        assert!(!order_by.contains("measure"), "{sql}");
    }
}
