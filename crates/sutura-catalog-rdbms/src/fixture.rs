//! The recorded dictionary corpus and the fake reader that serves it.
//!
//! This is the only [`crate::DictionaryReader`] implementor today, and it is the **fake** the port is
//! tested against - a recorded dictionary, not mocked SQL (`github.com/telekom/sutura#151`'s thing 4).
//! Until a real reader exists this is what a [`crate::RdbmsCatalog`] reads.
//!
//! The corpus mirrors exactly what the spike's `spike/read-a-dictionary` measured against this
//! worktree's provisioned Postgres: two tables (one fact, one lookup), a column set per table,
//! table and column comments, and one foreign key from the fact table to the lookup. There are **no
//! metrics** - that is the whole point of the narrowest metadata source, and what makes
//! `a_bundle_from_a_dictionary_loads_validates_and_answers_no_certified_question` pass.

use sutura_domain::model::SourceName;
use sutura_domain::pinned::DefinitionVersion;

use crate::{Dictionary, DictionaryReader, RdbmsCatalog, RdbmsError, Relationship, Table};

/// The fake [`DictionaryReader`] that serves the recorded corpus.
#[derive(Debug, Clone)]
pub struct FixtureReader;

impl DictionaryReader for FixtureReader {
    fn read_dictionary(&self) -> Result<Dictionary, RdbmsError> {
        Ok(corpus())
    }
}

/// The recorded dictionary: the two tables the spike named, with their comments, and the one
/// foreign key between them.
pub fn corpus() -> Dictionary {
    Dictionary::new(
        vec![
            Table::new(
                "orders".to_owned(),
                vec![
                    "order_id".to_owned(),
                    "customer_id".to_owned(),
                    "amount_cents".to_owned(),
                    "status".to_owned(),
                ],
                Some("Orders placed by customers. One row per order.".to_owned()),
            ),
            Table::new(
                "customers".to_owned(),
                vec!["customer_id".to_owned(), "segment".to_owned()],
                Some("Customer reference data.".to_owned()),
            ),
        ],
        vec![Relationship::new(
            Some("orders_customer_fk".to_owned()),
            "orders".to_owned(),
            "customer_id".to_owned(),
            "customers".to_owned(),
            "customer_id".to_owned(),
        )],
    )
}

/// A [`crate::RdbmsCatalog`] over the recorded corpus.
///
/// The constructor the conformance registry uses to register the adapter; it is `pub` because an
/// integration suite is a separate crate and cannot reach a `#[cfg(test)]` item.
pub const fn over_fixture_source(name: SourceName, version: DefinitionVersion) -> RdbmsCatalog<FixtureReader> {
    RdbmsCatalog::new(name, version, FixtureReader)
}

#[cfg(test)]
mod tests {
    use super::{FixtureReader, corpus};
    use crate::DictionaryReader as _;

    #[test]
    fn the_fixture_carries_models_and_a_foreign_key_and_no_metric() {
        let dictionary = corpus();
        // The corpus mirrors the spike: tables plus a foreign key, and nothing that would make a
        // metric.
        assert_eq!(dictionary.tables().len(), 2);
        assert_eq!(dictionary.relationships().len(), 1);
        FixtureReader.read_dictionary().unwrap();
    }
}
