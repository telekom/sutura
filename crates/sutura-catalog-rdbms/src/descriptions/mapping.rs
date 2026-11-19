//! A fixed projection over a separately declared documentation table.

use crate::postgres_reader::RowPredicate;
use sutura_domain::model::{ColumnName, QualifiedTable};

#[derive(Debug, Clone)]
pub struct MappedDescriptions {
    table: QualifiedTable,
    catalog: Option<ColumnName>,
    schema: ColumnName,
    table_key: ColumnName,
    column: ColumnName,
    description: ColumnName,
    predicate: RowPredicate,
}

impl MappedDescriptions {
    pub const fn new(
        table: QualifiedTable,
        catalog: Option<ColumnName>,
        schema: ColumnName,
        table_key: ColumnName,
        column: ColumnName,
        description: ColumnName,
        predicate: RowPredicate,
    ) -> Self {
        Self {
            table,
            catalog,
            schema,
            table_key,
            column,
            description,
            predicate,
        }
    }

    pub(crate) const fn predicate(&self) -> &RowPredicate {
        &self.predicate
    }

    pub(crate) fn binds(&self) -> Vec<String> {
        match &self.predicate {
            RowPredicate::Equals { value, .. } => vec![value.clone()],
            RowPredicate::None | RowPredicate::IsNull(_) | RowPredicate::IsNotNull(_) => Vec::new(),
        }
    }

    pub(crate) fn statement(&self, oracle: bool, limit: u64) -> String {
        let quote = |value: &str| format!("\"{value}\"");
        let table = self.table.to_string().split('.').map(quote).collect::<Vec<_>>().join(".");
        let text = |column: &ColumnName| {
            let column = quote(column.as_str());
            if oracle { column } else { format!("{column}::text") }
        };
        let catalog = self.catalog.as_ref().map_or_else(
            || {
                if oracle {
                    String::from("CAST(NULL AS VARCHAR2(128))")
                } else {
                    String::from("NULL::text")
                }
            },
            text,
        );
        let schema = text(&self.schema);
        let table_key = text(&self.table_key);
        let column = text(&self.column);
        let description = text(&self.description);
        let empty = if oracle { "CAST(NULL AS VARCHAR2(1))" } else { "NULL::text" };
        let flag = if oracle { "0" } else { "false" };
        let placeholder = if oracle { ":1" } else { "$1" };
        let predicate = match &self.predicate {
            RowPredicate::None => String::new(),
            RowPredicate::IsNull(column) => format!(" WHERE {} IS NULL", quote(column)),
            RowPredicate::IsNotNull(column) => format!(" WHERE {} IS NOT NULL", quote(column)),
            RowPredicate::Equals { column, .. } => format!(" WHERE {} = {placeholder}", quote(column)),
        };
        let bound = if oracle {
            format!("FETCH FIRST {limit} ROWS ONLY")
        } else {
            format!("LIMIT {limit}")
        };
        format!(
            "SELECT 'descriptions' AS environment, {catalog} AS catalog_name, {schema} AS schema_name, \
            {table_key} AS table_name, {table_key} AS model_name, {empty} AS table_description, \
            {column} AS column_name, {empty} AS column_type, {description} AS column_description, {flag} AS is_primary_key \
            FROM {table}{predicate} ORDER BY 2, 3, 4, 7 {bound}"
        )
    }
}
