//! The `ORDER BY` list for a `top` question: the caller's own key first, the plan's existing
//! tie-break after it.
//!
//! **The tie-break is not decoration.** It is the same group-key ordering
//! [`super::generate()`] already emits for a question with no `top`, so two rows tied on the
//! caller's own key still come back in the one order every dialect agrees on - without it, which
//! row lands at position `n` would be a fact about the executor rather than about the question.

use polyglot_sql::builder::{self, Expr};
use sutura_domain::federation::Ranking;
use sutura_domain::measure::ZeroDenominator;
use sutura_domain::plan::LegTerm;
use sutura_domain::query::{Top, TopBy, TopDirection};

use super::GenerateError;
use crate::dialect::Dialect;

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

/// A case-1 `top`'s ranking, as one expression - `github.com/telekom/sutura#777`.
///
/// [`Ranking::Term`] reads the leg's own already-built term expression at that position, the same
/// [`super::term_expression`] every plain leaf column renders through; [`Ranking::Quotient`]
/// divides two of those exactly as [`super::measure_expression`]'s own `Ratio` arm does,
/// `NULLIF`-guarded the same way. The two functions cannot drift on how a division renders,
/// because both call [`super::term_expression`] and nothing else for a leaf.
pub(super) fn ranking_expression(ranking: &Ranking, terms: &[LegTerm], dialect: Dialect) -> Result<Expr, GenerateError> {
    match *ranking {
        Ranking::Term(position) => terms
            .get(position)
            .map(|term| super::term_expression(term.term(), dialect))
            .ok_or(GenerateError::RankingExceedsTerms {
                position,
                terms: terms.len(),
            }),
        Ranking::Quotient {
            ref numerator,
            ref denominator,
            zero_denominator,
        } => {
            let top = ranking_expression(numerator, terms, dialect)?.cast("DOUBLE");
            let bottom = ranking_expression(denominator, terms, dialect)?;
            let bottom = match zero_denominator {
                ZeroDenominator::Null => builder::null_if(bottom, builder::lit(0)),
                ZeroDenominator::Fail => bottom,
            };
            Ok(top.div(bottom))
        }
    }
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

    fn a_term(column: &str) -> sutura_domain::plan::LegTerm {
        sutura_domain::plan::LegTerm::new(
            sutura_domain::plan::PlanTerm::Aggregate {
                aggregate: sutura_domain::model::Aggregate::Sum,
                column: sutura_domain::plan::PlanColumn::new(
                    sutura_domain::model::TableName::parse("t").expect("a test table is a table"),
                    sutura_domain::model::ColumnName::parse(column).expect("a test column is a column"),
                ),
            },
            sutura_domain::plan::ResultLabel::internal(sutura_domain::plan::InternalLabel::Leaf(0)),
        )
    }

    #[test]
    fn a_term_ranking_reads_the_leg_s_own_term_at_that_position() {
        let terms = vec![a_term("revenue_cents")];
        let expr = super::ranking_expression(&sutura_domain::federation::Ranking::Term(0), &terms, Dialect::DuckDb)
            .expect("position 0 is this leg's only term");
        let sql = super::super::render(&builder::select(vec![expr]).from("t").build(), Dialect::DuckDb)
            .expect("a ranking expression renders");
        assert!(sql.contains("SUM"), "{sql}");
        assert!(sql.contains("revenue_cents"), "{sql}");
    }

    #[test]
    fn a_quotient_ranking_divides_two_terms_with_the_same_guard_a_ratio_measure_uses() {
        let terms = vec![a_term("numerator_cents"), a_term("denominator_count")];
        let ranking = sutura_domain::federation::Ranking::Quotient {
            numerator: Box::new(sutura_domain::federation::Ranking::Term(0)),
            denominator: Box::new(sutura_domain::federation::Ranking::Term(1)),
            zero_denominator: sutura_domain::measure::ZeroDenominator::Null,
        };
        let expr = super::ranking_expression(&ranking, &terms, Dialect::DuckDb).expect("both positions exist");
        let sql = super::super::render(&builder::select(vec![expr]).from("t").build(), Dialect::DuckDb)
            .expect("a ranking expression renders");
        assert!(
            sql.contains("NULLIF"),
            "the null-guard must match a ratio measure's own: {sql}"
        );
        assert!(sql.contains('/'), "{sql}");
    }

    /// The refusal, not merely the predicate: a position past this leg's own terms is refused
    /// rather than panicking or reading an out-of-bounds cell as null.
    #[test]
    fn a_ranking_position_past_this_legs_terms_is_refused() {
        let terms = vec![a_term("revenue_cents")];
        let error = super::ranking_expression(&sutura_domain::federation::Ranking::Term(1), &terms, Dialect::DuckDb)
            .expect_err("this leg carries one term, at position 0");
        assert!(
            matches!(error, super::GenerateError::RankingExceedsTerms { position: 1, terms: 1 }),
            "{error:?}"
        );
    }
}
