//! `top`: the engine's own sort-and-limit, mirroring `sutura_sql::generate::top` because this
//! adapter renders no SQL for either of them to share.
//!
//! **Why this exists at all, separately from the renderer.** This engine is the one leg-executing
//! adapter a shipped binary links (`Warehouse::EXECUTES_LEGS`), and it builds a
//! [`LogicalPlan`](datafusion::logical_expr::LogicalPlan) directly rather than rendering a
//! statement - so `sutura_sql::generate`'s `ORDER BY`/`LIMIT` never reaches it. A plan carrying
//! [`sutura_domain::query::Top`] answers wrong here unless this module reads the field too: without it, `top` renders
//! correctly on every SQL-speaking adapter and does nothing on the one this repository ships by
//! default.

use datafusion::common::Column;
use datafusion::logical_expr::{Expr, SortExpr};
use sutura_domain::query::{Top, TopBy, TopDirection};

/// The sort list and the row limit for one plan's output, given its already-built tie-break
/// (the plain grouped ordering [`crate::collect::outputs`] returns).
///
/// Without `top`: every tie-break column, ascending, nulls last - the same call
/// [`LogicalPlanBuilder::sort_by`](datafusion::logical_expr::LogicalPlanBuilder::sort_by) already
/// made, restated here as explicit [`SortExpr`]s so both arms return the same shape. With `top`:
/// the caller's own key first, in the direction asked, nulls last regardless of direction - a null
/// there means there was nothing to rank, which sorts after every value either way - then the same
/// tie-break, unchanged.
pub(crate) fn sort_and_limit(
    top: Option<Top>,
    labels: &[String],
    group_count: usize,
    tiebreak: Vec<Expr>,
    max_rows: u32,
) -> (Vec<SortExpr>, usize) {
    let Some(top) = top else {
        return (
            tiebreak.into_iter().map(|e| e.sort(true, false)).collect(),
            usize::try_from(max_rows.saturating_add(1)).unwrap_or(usize::MAX),
        );
    };
    // The measure is always the last label (`QueryPlan::result_labels`); the bucket is the last of
    // the grouped ones, at `group_count - 1` - both read off the SAME label list the tie-break was
    // built from, so this cannot name a column the projection does not carry.
    let primary_label = match top.by() {
        TopBy::Metric => labels.last().map(String::as_str).unwrap_or_default(),
        TopBy::Period => labels
            .get(group_count.saturating_sub(1))
            .map(String::as_str)
            .unwrap_or_default(),
    };
    let ascending = matches!(top.direction(), TopDirection::Asc);
    let primary = Expr::Column(Column::new_unqualified(primary_label)).sort(ascending, false);
    let mut sorts = Vec::with_capacity(tiebreak.len().saturating_add(1));
    sorts.push(primary);
    sorts.extend(tiebreak.into_iter().map(|e| e.sort(true, false)));
    (sorts, usize::try_from(top.n().get()).unwrap_or(usize::MAX))
}

#[cfg(test)]
mod tests {
    use datafusion::common::Column;
    use datafusion::logical_expr::Expr;
    use sutura_domain::query::{Top, TopBy, TopDirection, TopN};

    fn labels() -> Vec<String> {
        vec![String::from("region"), String::from("period"), String::from("revenue")]
    }

    fn tiebreak() -> Vec<Expr> {
        vec![
            Expr::Column(Column::new_unqualified("region")),
            Expr::Column(Column::new_unqualified("period")),
        ]
    }

    #[test]
    fn no_top_sorts_by_the_tiebreak_alone_and_limits_one_past_the_cap() {
        let (sorts, limit) = super::sort_and_limit(None, &labels(), 2, tiebreak(), 10_000);
        assert_eq!(sorts.len(), 2, "{sorts:?}");
        assert_eq!(limit, 10_001);
    }

    #[test]
    fn top_by_metric_ranks_the_measure_first_and_limits_to_its_own_n() {
        let top = Top::new(
            TopN::parse(3).expect("three is a row count"),
            TopBy::Metric,
            TopDirection::Desc,
        );
        let (sorts, limit) = super::sort_and_limit(Some(top), &labels(), 2, tiebreak(), 10_000);
        assert_eq!(limit, 3);
        assert_eq!(sorts.len(), 3, "{sorts:?}");
        assert_eq!(sorts[0].expr, Expr::Column(Column::new_unqualified("revenue")));
        assert!(!sorts[0].asc, "descending asks for the largest first");
    }

    #[test]
    fn top_by_period_ranks_the_bucket_rather_than_the_measure() {
        let top = Top::new(TopN::parse(5).expect("five is a row count"), TopBy::Period, TopDirection::Asc);
        let (sorts, _) = super::sort_and_limit(Some(top), &labels(), 2, tiebreak(), 10_000);
        assert_eq!(sorts[0].expr, Expr::Column(Column::new_unqualified("period")));
        assert!(sorts[0].asc);
    }
}
