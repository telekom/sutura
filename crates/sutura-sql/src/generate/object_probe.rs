//! One table's reference probe: a row count over it, for a data system's parse-only mode.

use polyglot_sql::builder;
use sutura_domain::model::{QualifiedTable, SourceName};

use super::{GenerateError, aliased, assert_placeholder_count, named_objects, render, table_path};
use crate::GeneratedQuery;
use crate::dialect::Dialect;

/// The label the count is read under.
pub const OBJECT_PROBE_LABEL: &str = "rows";

/// Renders `SELECT COUNT(*) AS "rows" FROM <table>` in `dialect`.
///
/// **It exists to be parsed, not answered**: it asks whether the data system resolves the table
/// reference this crate renders - for Foundry, a dotted object's one quoted identifier - and carries
/// no column, no filter and no parameter. A count projects no row value if it ever runs.
///
/// # Errors
///
/// [`GenerateError`] for a table `dialect` cannot name.
pub fn generate_object_probe(
    source: &SourceName,
    table: &QualifiedTable,
    dialect: Dialect,
) -> Result<GeneratedQuery, GenerateError> {
    let ast = builder::select([aliased(builder::count_star(), OBJECT_PROBE_LABEL)?])
        .from(&table_path(table, dialect)?)
        .build();
    let ast = named_objects(ast, std::iter::once(table), dialect)?;
    let params = Vec::new();
    let sql = render(&ast, dialect)?;
    assert_placeholder_count(&ast, dialect, &params)?;
    Ok(GeneratedQuery::new(source.clone(), sql, params))
}

#[cfg(test)]
mod tests {
    use sutura_domain::model::{ObjectPath, QualifiedTable, SourceName, TableName};

    use super::generate_object_probe;
    use crate::dialect::Dialect;

    fn source() -> SourceName {
        SourceName::parse("ontology").expect("a test source is a source")
    }

    #[test]
    fn a_dotted_object_is_probed_as_one_quoted_identifier_aliased_to_its_local_name() {
        let table = QualifiedTable::object(
            ObjectPath::parse("com.palantir.system.Evolution")
                .expect("dotted names parse")
                .expect("a dotted name is an object path"),
        )
        .expect("the local name is a name");
        let query = generate_object_probe(&source(), &table, Dialect::Foundry).expect("a dotted object renders for Foundry");
        assert_eq!(
            query.sql(),
            "SELECT COUNT(*) AS `rows` FROM `com.palantir.system.Evolution` AS `com_palantir_system_Evolution`"
        );
        assert_eq!(query.params(), []);
        assert!(
            generate_object_probe(&source(), &table, Dialect::Postgres).is_err(),
            "only Foundry names a dotted object"
        );
    }

    #[test]
    fn a_plain_object_is_probed_by_its_name() {
        let table = QualifiedTable::from(TableName::parse("suturaCustomer").expect("a name"));
        let query = generate_object_probe(&source(), &table, Dialect::Foundry).expect("a plain object renders");
        assert_eq!(query.sql(), "SELECT COUNT(*) AS `rows` FROM `suturaCustomer`");
    }
}
