//! A native read against the provisioned Postgres tier records each text column's collation.
//!
//! A unit module and not a cell of `tests/provisioned.rs`, so a base run that lacks the collation
//! API removes it with its declaration instead of failing to compile a kept test file. Where no tier
//! is provisioned `sutura_dev::provisioned::here` writes its `SKIPPED` notice, as every tier cell does.

// A literal `#[cfg(test)]` ancestor in this file is what `clippy.toml`'s `allow-expect-in-tests` honours.
#[cfg(test)]
mod cells {
    use std::collections::BTreeMap;
    use std::path::Path;
    use std::time::{SystemTime, UNIX_EPOCH};

    use adbc_core::{Connection as _, Statement as _};
    use arrow_array::Array as _;
    use arrow_array::cast::AsArray as _;
    use sutura_adbc_postgres::{Channel, ConnectionTarget, Conninfo, PostgresDriver};
    use sutura_dev::provisioned::{self, Provisioned};
    use sutura_domain::catalog::{Collation, DictionaryInclude, DictionaryPattern, DictionarySelection};
    use sutura_domain::identity::Secret;
    use sutura_domain::model::SourceName;

    use crate::DictionaryReader as _;
    use crate::postgres_reader::PostgresReader;

    fn required(variable: &str) -> String {
        let value = std::env::var(variable).expect("the provisioned tier exports its credential");
        assert!(!value.trim().is_empty(), "the provisioned tier exported an empty credential");
        value
    }

    type Refused = Box<dyn std::error::Error>;

    struct Tier {
        driver: PostgresDriver,
        socket: String,
        port: u16,
    }

    impl Tier {
        fn here() -> Option<Self> {
            let endpoint = match provisioned::here(Path::new(env!("CARGO_MANIFEST_DIR")), "postgres") {
                Provisioned::At(endpoint) => endpoint,
                Provisioned::Skipped(_) => return None,
            };
            Some(Self {
                driver: PostgresDriver::from_host().expect("the tier is up, so a driver is named"),
                socket: String::from(endpoint.host()),
                port: endpoint.port(),
            })
        }

        fn conninfo(&self) -> Conninfo {
            let source = SourceName::parse("dictionary").expect("a source");
            let password = Secret::new(required("SUTURA_POSTGRES_TIER_PASSWORD"));
            Conninfo::new(
                &source,
                ConnectionTarget::Host(&self.socket),
                self.port,
                &required("SUTURA_POSTGRES_TIER_DB"),
                &required("SUTURA_POSTGRES_TIER_USER"),
                &password,
                Channel::Plaintext,
            )
            .expect("the tier's plaintext connection string builds")
        }

        /// Runs `statement` committed; the error is the driver's, for a statement the tier may refuse.
        fn try_sql(&self, statement: &str) -> Result<(), Refused> {
            let mut connection = self.driver.connect(&self.conninfo())?;
            let mut fixture = connection.new_statement()?;
            fixture.set_sql_query(statement)?;
            #[expect(
                clippy::disallowed_methods,
                reason = "a fixture install: this file's own DDL, never caller text"
            )]
            let installed = fixture.execute_update();
            installed.map(drop).map_err(Into::into)
        }

        /// The native statement's `column_collation` cell for each column of table `odd`.
        fn raw_collations(&self, schema: &str) -> BTreeMap<String, Option<String>> {
            let statement = crate::native::sql::postgres(1, 100).replace("$1", &format!("'{schema}'"));
            let mut connection = self.driver.connect(&self.conninfo()).expect("the tier opens");
            let mut select = connection.new_statement().expect("a statement allocates");
            select.set_sql_query(statement).expect("the statement is accepted");
            let mut cells = BTreeMap::new();
            for batch in select.execute().expect("the statement runs") {
                let batch = batch.expect("a batch decodes");
                let text = |column: &str| {
                    batch
                        .column_by_name(column)
                        .and_then(|array| array.as_string_opt::<i32>())
                        .expect("a text column")
                        .clone()
                };
                let (tables, columns, collations) = (text("table_name"), text("column_name"), text("column_collation"));
                for row in 0..batch.num_rows() {
                    if tables.value(row) == "odd" {
                        cells.insert(
                            columns.value(row).to_owned(),
                            collations.is_valid(row).then(|| collations.value(row).to_owned()),
                        );
                    }
                }
            }
            cells
        }

        fn run_sql(&self, statement: &str) {
            self.try_sql(statement).expect("the fixture installs");
        }
    }

    #[test]
    fn a_native_read_records_the_collation_of_text_columns_and_nothing_for_other_types() {
        let Some(tier) = Tier::here() else { return };
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("the clock is after the epoch")
            .as_nanos();
        let schema = format!("collation_{}_{nonce}", std::process::id());
        tier.run_sql(&format!(
            "CREATE SCHEMA {schema}; \
             CREATE COLLATION {schema}.\"C\" (provider = libc, locale = 'C'); \
             CREATE DOMAIN {schema}.label AS text; \
             CREATE TABLE {schema}.notes (plain text, varied varchar(20), padded char(4), byte_order text COLLATE \"C\", \
                 posix text COLLATE \"POSIX\", own text COLLATE {schema}.\"C\", amount bigint); \
             CREATE TABLE {schema}.odd (wrapped {schema}.label)"
        ));
        let citext = tier
            .try_sql(&format!(
                "CREATE EXTENSION IF NOT EXISTS citext WITH SCHEMA {schema}; \
                 ALTER TABLE {schema}.odd ADD COLUMN folded {schema}.citext"
            ))
            .is_ok();
        let pattern = |raw: &str| DictionaryPattern::parse(raw).expect("a literal pattern parses");
        let selection = DictionarySelection::parse(
            vec![DictionaryInclude::new(pattern(&schema), pattern("notes"))],
            Vec::new(),
            Vec::new(),
            Vec::new(),
        )
        .expect("one include pair is a selection");
        let reader = PostgresReader::native(tier.driver.clone(), tier.conninfo(), &selection, None, None)
            .expect("the native reader settings parse");
        let dictionary = reader.read_dictionary().expect("the native read succeeds");
        let [notes] = dictionary.tables() else {
            panic!("one selected table: {dictionary:?}")
        };
        let collation = |column| notes.column_metadata(column).map(|metadata| metadata.collation().clone());
        for column in ["plain", "varied", "padded", "byte_order", "posix"] {
            assert_eq!(collation(column), Some(Collation::Binary), "{column}");
        }
        assert_eq!(
            collation("own"),
            Some(Collation::named(&format!("{schema}.C"))),
            "a user schema's collation named C is not the built-in one"
        );
        assert_eq!(collation("amount"), Some(Collation::Unknown), "bigint has no collation");
        // The read refuses a type the dictionary mapping does not list, so the statement's own
        // guard is read from its raw cells.
        let raw = tier.raw_collations(&schema);
        assert_eq!(raw.get("wrapped"), Some(&None), "a domain over text");
        if citext {
            assert_eq!(
                raw.get("folded"),
                Some(&None),
                "citext compares case-insensitively and is not one of the base text types"
            );
        } else {
            eprintln!(
                "SKIPPED - the tier cannot install citext, so a citext column was not read.\n  \
                 The domain-over-text assertion above still ran; install the contrib extension to run this half."
            );
        }
    }
}
