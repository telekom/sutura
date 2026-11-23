//! A fixed projection over a separately declared documentation table, optionally reached through a
//! declared parent table (`key.via`, `telekom/sutura#2040`), with any number of closed-form
//! live-row predicates each naming the table it filters.

use crate::postgres_reader::RowPredicate;
use sutura_domain::model::{ColumnName, QualifiedTable};

/// Which physical table a [`MappedPredicate`] filters: the mapped table, or its `key.via` parent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MappedTable {
    Main,
    Via,
}

/// One closed-form predicate, naming the table it filters.
#[derive(Debug, Clone)]
pub struct MappedPredicate {
    table: MappedTable,
    predicate: RowPredicate,
}

impl MappedPredicate {
    pub const fn new(table: MappedTable, predicate: RowPredicate) -> Self {
        Self { table, predicate }
    }
}

/// A mapped key's schema and physical table name: either columns on the mapped table, or columns
/// on a declared parent table joined in on declared columns.
#[derive(Debug, Clone)]
pub enum MappedKeySource {
    Direct { schema: ColumnName, table: ColumnName },
    Via(MappedVia),
}

/// The parent table a mapped key is resolved through.
#[derive(Debug, Clone)]
pub struct MappedVia {
    table: QualifiedTable,
    on: Vec<(ColumnName, ColumnName)>,
    schema: ColumnName,
    table_name: ColumnName,
}

impl MappedVia {
    pub const fn new(
        table: QualifiedTable,
        on: Vec<(ColumnName, ColumnName)>,
        schema: ColumnName,
        table_name: ColumnName,
    ) -> Self {
        Self {
            table,
            on,
            schema,
            table_name,
        }
    }
}

#[derive(Debug, Clone)]
pub struct MappedDescriptions {
    table: QualifiedTable,
    catalog: Option<ColumnName>,
    key: MappedKeySource,
    column: ColumnName,
    description: ColumnName,
    predicates: Vec<MappedPredicate>,
}

impl MappedDescriptions {
    pub const fn new(
        table: QualifiedTable,
        catalog: Option<ColumnName>,
        key: MappedKeySource,
        column: ColumnName,
        description: ColumnName,
        predicates: Vec<MappedPredicate>,
    ) -> Self {
        Self {
            table,
            catalog,
            key,
            column,
            description,
            predicates,
        }
    }

    /// Every predicate column, main or via, so a reader can check each is a safe identifier before
    /// any SQL text exists.
    pub(crate) fn predicate_columns(&self) -> impl Iterator<Item = &str> {
        self.predicates.iter().filter_map(|mapped| match &mapped.predicate {
            RowPredicate::None => None,
            RowPredicate::IsNull(column) | RowPredicate::IsNotNull(column) | RowPredicate::Equals { column, .. } => {
                Some(column.as_str())
            }
        })
    }

    /// Bound values, in the same left-to-right order their placeholders appear in [`Self::statement`].
    pub(crate) fn binds(&self) -> Vec<String> {
        self.predicates
            .iter()
            .filter_map(|mapped| match &mapped.predicate {
                RowPredicate::Equals { value, .. } => Some(value.clone()),
                RowPredicate::None | RowPredicate::IsNull(_) | RowPredicate::IsNotNull(_) => None,
            })
            .collect()
    }

    pub(crate) fn statement(&self, oracle: bool, limit: u64) -> String {
        let quote = |value: &str| format!("\"{value}\"");
        let qualified_table = |table: &QualifiedTable| table.to_string().split('.').map(quote).collect::<Vec<_>>().join(".");
        let text = |column: &str| {
            let column = quote(column);
            if oracle { column } else { format!("{column}::text") }
        };
        let empty = if oracle { "CAST(NULL AS VARCHAR2(1))" } else { "NULL::text" };
        let flag = if oracle { "0" } else { "false" };
        let bound = if oracle {
            format!("FETCH FIRST {limit} ROWS ONLY")
        } else {
            format!("LIMIT {limit}")
        };

        let mut placeholder = 0usize;
        let mut next_placeholder = || {
            placeholder += 1;
            if oracle {
                format!(":{placeholder}")
            } else {
                format!("${placeholder}")
            }
        };

        match &self.key {
            MappedKeySource::Direct { schema, table } => {
                let catalog = self.catalog.as_ref().map_or_else(
                    || {
                        if oracle {
                            String::from("CAST(NULL AS VARCHAR2(128))")
                        } else {
                            String::from("NULL::text")
                        }
                    },
                    |column| text(column.as_str()),
                );
                let schema = text(schema.as_str());
                let table_key = text(table.as_str());
                let column = text(self.column.as_str());
                let description = text(self.description.as_str());
                let from = qualified_table(&self.table);
                let predicate = where_clause(&self.predicates, None, &mut next_placeholder, quote);
                format!(
                    "SELECT 'descriptions' AS environment, {catalog} AS catalog_name, {schema} AS schema_name, \
                    {table_key} AS table_name, {table_key} AS model_name, {empty} AS table_description, \
                    {column} AS column_name, {empty} AS column_type, {description} AS column_description, {flag} AS is_primary_key \
                    FROM {from}{predicate} ORDER BY 2, 3, 4, 7 {bound}"
                )
            }
            MappedKeySource::Via(via) => {
                let main_alias = "d0";
                let via_alias = "d1";
                let aliased = |alias: &str, column: &str| format!("{alias}.{}", quote(column));
                let text_aliased = |alias: &str, column: &str| {
                    let aliased = aliased(alias, column);
                    if oracle { aliased } else { format!("{aliased}::text") }
                };
                let catalog = self.catalog.as_ref().map_or_else(
                    || {
                        if oracle {
                            String::from("CAST(NULL AS VARCHAR2(128))")
                        } else {
                            String::from("NULL::text")
                        }
                    },
                    |column| text_aliased(main_alias, column.as_str()),
                );
                let schema = text_aliased(via_alias, via.schema.as_str());
                let table_key = text_aliased(via_alias, via.table_name.as_str());
                let column = text_aliased(main_alias, self.column.as_str());
                let description = text_aliased(main_alias, self.description.as_str());
                let main_table = format!("{} {main_alias}", qualified_table(&self.table));
                let via_table = format!("{} {via_alias}", qualified_table(&via.table));
                let on = via
                    .on
                    .iter()
                    .map(|(main, parent)| {
                        format!(
                            "{} = {}",
                            aliased(main_alias, main.as_str()),
                            aliased(via_alias, parent.as_str())
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(" AND ");
                let predicate = where_clause(&self.predicates, Some((main_alias, via_alias)), &mut next_placeholder, quote);
                format!(
                    "SELECT 'descriptions' AS environment, {catalog} AS catalog_name, {schema} AS schema_name, \
                    {table_key} AS table_name, {table_key} AS model_name, {empty} AS table_description, \
                    {column} AS column_name, {empty} AS column_type, {description} AS column_description, {flag} AS is_primary_key \
                    FROM {main_table} JOIN {via_table} ON {on}{predicate} ORDER BY 2, 3, 4, 7 {bound}"
                )
            }
        }
    }
}

fn where_clause(
    predicates: &[MappedPredicate],
    aliases: Option<(&str, &str)>,
    next_placeholder: &mut impl FnMut() -> String,
    quote: impl Fn(&str) -> String,
) -> String {
    let column_ref = |mapped: &MappedPredicate, column: &str| match aliases {
        Some((main, via)) => {
            let alias = match mapped.table {
                MappedTable::Main => main,
                MappedTable::Via => via,
            };
            format!("{alias}.{}", quote(column))
        }
        None => quote(column),
    };
    let clauses: Vec<String> = predicates
        .iter()
        .filter_map(|mapped| match &mapped.predicate {
            RowPredicate::None => None,
            RowPredicate::IsNull(column) => Some(format!("{} IS NULL", column_ref(mapped, column))),
            RowPredicate::IsNotNull(column) => Some(format!("{} IS NOT NULL", column_ref(mapped, column))),
            RowPredicate::Equals { column, .. } => Some(format!("{} = {}", column_ref(mapped, column), next_placeholder())),
        })
        .collect();
    if clauses.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", clauses.join(" AND "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn column(raw: &str) -> ColumnName {
        ColumnName::parse(raw).unwrap()
    }

    fn table(raw: &str) -> QualifiedTable {
        QualifiedTable::parse(raw).unwrap()
    }

    fn direct() -> MappedDescriptions {
        MappedDescriptions::new(
            table("inventory.column_docs"),
            None,
            MappedKeySource::Direct {
                schema: column("owner"),
                table: column("object_name"),
            },
            column("field_name"),
            column("prose"),
            Vec::new(),
        )
    }

    fn via() -> MappedDescriptions {
        MappedDescriptions::new(
            table("inventory.column_docs"),
            None,
            MappedKeySource::Via(MappedVia::new(
                table("inventory.object_docs"),
                vec![(column("obj_id"), column("id"))],
                column("schema_name"),
                column("object_name"),
            )),
            column("field_name"),
            column("prose"),
            Vec::new(),
        )
    }

    #[test]
    fn a_direct_key_source_keeps_the_pre_via_flat_sql_unchanged() {
        let mapped = direct();
        let postgres = mapped.statement(false, 1000);
        assert!(postgres.contains("FROM \"inventory\".\"column_docs\""));
        assert!(!postgres.contains("JOIN"));
        assert!(!postgres.contains(" WHERE "));
        let oracle = mapped.statement(true, 1000);
        assert!(oracle.contains("FETCH FIRST 1000 ROWS ONLY"));
        assert!(!oracle.contains("JOIN"));
    }

    #[test]
    fn a_via_key_source_joins_the_declared_parent_table_on_declared_columns() {
        let mapped = via();
        let postgres = mapped.statement(false, 1000);
        assert!(postgres.contains("FROM \"inventory\".\"column_docs\" d0 JOIN \"inventory\".\"object_docs\" d1"));
        assert!(postgres.contains("d0.\"obj_id\" = d1.\"id\""));
        assert!(postgres.contains("d1.\"schema_name\"::text AS schema_name"));
        assert!(postgres.contains("d1.\"object_name\"::text AS table_name"));
    }

    #[test]
    fn predicates_on_main_and_via_tables_are_aliased_and_bound_in_declared_order() {
        let mapped = MappedDescriptions::new(
            table("inventory.column_docs"),
            None,
            MappedKeySource::Via(MappedVia::new(
                table("inventory.object_docs"),
                vec![(column("obj_id"), column("id"))],
                column("schema_name"),
                column("object_name"),
            )),
            column("field_name"),
            column("prose"),
            vec![
                MappedPredicate::new(
                    MappedTable::Main,
                    RowPredicate::Equals {
                        column: column("deleted").to_string(),
                        value: String::from("N"),
                    },
                ),
                MappedPredicate::new(MappedTable::Via, RowPredicate::IsNull(column("retired_at").to_string())),
            ],
        );
        let postgres = mapped.statement(false, 1000);
        assert!(postgres.contains("WHERE d0.\"deleted\" = $1 AND d1.\"retired_at\" IS NULL"));
        assert_eq!(mapped.binds(), vec![String::from("N")]);
        let oracle = mapped.statement(true, 1000);
        assert!(oracle.contains("WHERE d0.\"deleted\" = :1 AND d1.\"retired_at\" IS NULL"));
    }

    #[test]
    fn predicate_columns_reports_every_predicate_column_main_and_via() {
        let mapped = MappedDescriptions::new(
            table("inventory.column_docs"),
            None,
            MappedKeySource::Direct {
                schema: column("owner"),
                table: column("object_name"),
            },
            column("field_name"),
            column("prose"),
            vec![MappedPredicate::new(
                MappedTable::Main,
                RowPredicate::IsNotNull(column("deleted").to_string()),
            )],
        );
        assert_eq!(mapped.predicate_columns().collect::<Vec<_>>(), vec!["deleted"]);
    }
}
