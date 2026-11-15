//! Oracle spelling over the existing unparser's AST; no statement text is reparsed or replaced.

use std::ops::ControlFlow;

use datafusion::common::plan_datafusion_err;
use datafusion::error::{DataFusionError, Result};
use datafusion::logical_expr::LogicalPlan;
use datafusion::sql::sqlparser::ast::{
    CastKind, DataType, Expr, Fetch, LimitClause, OffsetRows, Query, TableFactor, TimezoneInfo, TypedString, Value,
    VisitMut as _, VisitorMut,
};
use datafusion::sql::unparser::Unparser;
use datafusion::sql::unparser::dialect::{CharacterLengthStyle, CustomDialectBuilder, DateFieldExtractStyle};

pub(super) fn unparsed(plan: &LogicalPlan) -> Result<String> {
    let spelling = CustomDialectBuilder::new()
        .with_identifier_quote_style('"')
        .with_use_timestamp_for_date64(true)
        .with_int64_cast_dtype(DataType::Integer(None))
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

    fn post_visit_expr(&mut self, expression: &mut Expr) -> ControlFlow<Self::Break> {
        if let Expr::Cast {
            kind: CastKind::Cast,
            expr,
            data_type,
            array: false,
            format: None,
        } = expression
        {
            let literal_type = match data_type {
                DataType::Date => DataType::Date,
                DataType::Timestamp(None, TimezoneInfo::None) => DataType::Timestamp(None, TimezoneInfo::None),
                _ => return ControlFlow::Continue(()),
            };
            if let Expr::Value(value) = expr.as_mut()
                && matches!(value.value, Value::SingleQuotedString(_))
            {
                // The optimizer's temporal constants arrive as string casts. An ANSI literal
                // keeps their ISO value independent of the source session's NLS settings.
                let value = std::mem::replace(value, Value::Null.into());
                *expression = Expr::TypedString(TypedString {
                    data_type: literal_type,
                    value,
                    uses_odbc_syntax: false,
                });
            }
        }
        ControlFlow::Continue(())
    }

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
