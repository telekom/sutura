//! A reader's collation evidence reaches the pinned schema, and only as far as it was given.

use sutura_domain::catalog::Collation;
use sutura_domain::model::{ModelName, SourceName};
use sutura_domain::pinned::{DefinitionVersion, SemanticCatalog as _};
use sutura_domain::warehouse::schema::{PinnedSchemas, TypeDialect};

use crate::{ColumnMetadata, Dictionary, DictionaryReader, RdbmsCatalog, RdbmsError, Table, TableAddress};

struct Recorded(Dictionary);

impl DictionaryReader for Recorded {
    fn read_dictionary(&self) -> Result<Dictionary, RdbmsError> {
        Ok(self.0.clone())
    }
}

fn text(collation: Collation) -> ColumnMetadata {
    ColumnMetadata::new(Some(String::from("text")), None).with_collation(collation)
}

#[test]
fn a_dictionary_collation_reaches_the_pinned_field_and_a_silent_column_pins_unknown() {
    let source = SourceName::parse("local").expect("a source");
    let table = Table::new(
        String::from("notes"),
        TableAddress::new(None, String::from("public"), String::from("notes")),
        vec![String::from("exact"), String::from("folded"), String::from("silent")],
        None,
    )
    .with_column_metadata([
        (String::from("exact"), text(Collation::Binary)),
        (String::from("folded"), text(Collation::named("public.und_ci"))),
        (String::from("silent"), ColumnMetadata::new(Some(String::from("text")), None)),
    ]);
    let version = DefinitionVersion::parse("test").expect("a version");
    let pinned = RdbmsCatalog::new(source.clone(), version, Recorded(Dictionary::new(vec![table], Vec::new())))
        .load()
        .expect("the dictionary loads");
    let schemas = PinnedSchemas::pin(
        pinned.definitions(),
        &std::collections::BTreeMap::from([(source, TypeDialect::Postgres)]),
    )
    .expect("the bundle pins");
    let schema = schemas
        .of(&ModelName::parse("notes").expect("a model"))
        .expect("the model is pinned");
    let pinned_collation = |column: &str| Collation::of_field(schema.field_with_name(column).expect("a pinned column"));
    assert_eq!(pinned_collation("exact"), Collation::Binary);
    assert_eq!(pinned_collation("folded"), Collation::named("public.und_ci"));
    assert_eq!(pinned_collation("silent"), Collation::Unknown);
}
