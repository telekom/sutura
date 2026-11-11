//! Oracle spelling over the existing unparser's AST; no statement text is reparsed or replaced.

use std::ops::ControlFlow;

use datafusion::common::plan_datafusion_err;
use datafusion::error::{DataFusionError, Result};
use datafusion::logical_expr::LogicalPlan;
use datafusion::sql::sqlparser::ast::{Fetch, LimitClause, OffsetRows, Query, TableFactor, VisitMut as _, VisitorMut};
use datafusion::sql::unparser::Unparser;
use datafusion::sql::unparser::dialect::{CharacterLengthStyle, CustomDialectBuilder, DateFieldExtractStyle};

pub(super) fn unparsed(plan: &LogicalPlan) -> Result<String> {
    let spelling = CustomDialectBuilder::new()
        .with_identifier_quote_style('"')
        .with_supports_column_alias_in_table_alias(false)
        .with_date_field_extract_style(DateFieldExtractStyle::Extract)
        .with_character_length_style(CharacterLengthStyle::Length)
        .build();
    let mut statement = Unparser::new(&spelling).plan_to_sql(plan)?;
    if let ControlFlow::Break(error) = statement.visit(&mut Oracle) {
        return Err(error);
    }
    Ok(statement.to_string())
}

struct Oracle;

impl VisitorMut for Oracle {
    type Break = DataFusionError;

    fn post_visit_query(&mut self, query: &mut Query) -> ControlFlow<Self::Break> {
        let Some(clause) = query.limit_clause.take() else {
            return ControlFlow::Continue(());
        };
        let LimitClause::LimitOffset {
            limit,
            mut offset,
            limit_by,
        } = clause
        else {
            return ControlFlow::Break(plan_datafusion_err!("the Oracle part has an unsupported limit"));
        };
        if !limit_by.is_empty() || query.fetch.is_some() {
            return ControlFlow::Break(plan_datafusion_err!("the Oracle part has conflicting limits"));
        }
        if let Some(quantity) = limit {
            query.fetch = Some(Fetch {
                with_ties: false,
                percent: false,
                quantity: Some(quantity),
            });
        }
        if let Some(offset) = offset.as_mut() {
            offset.rows = OffsetRows::Rows;
        }
        query.limit_clause = offset.map(|offset| LimitClause::LimitOffset {
            limit: None,
            offset: Some(offset),
            limit_by,
        });
        ControlFlow::Continue(())
    }

    fn post_visit_table_factor(&mut self, table: &mut TableFactor) -> ControlFlow<Self::Break> {
        match table {
            TableFactor::Table { alias, .. } | TableFactor::Derived { alias, .. } => {
                if let Some(alias) = alias {
                    alias.explicit = false;
                }
                ControlFlow::Continue(())
            }
            _ => ControlFlow::Break(plan_datafusion_err!("the Oracle part has an unsupported table factor")),
        }
    }
}
