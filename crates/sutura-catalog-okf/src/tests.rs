//! Tests for [`super`].
//!
//! A file rather than an inline `mod tests`, which is this repository's usual shape over the
//! thousand-line gate. The scratch-directory helpers follow `sutura-catalog-local`'s: each test
//! writes real Table Schema descriptors to a per-process temp directory and loads through
//! [`super::OkfCatalog::load`], so a cell asserts on what the adapter reads, not on source text.

use std::io::Read as _;
use std::path::{Path, PathBuf};

use sutura_domain::capabilities::MetadataCapabilities;
use sutura_domain::model::SourceName;
use sutura_domain::pinned::{DefinitionVersion, PinnedDefinitions, SemanticCatalog};

use crate::{OkfCatalog, OkfCatalogError};

fn test_name() -> SourceName {
    SourceName::parse("test").expect("a test name is a name")
}

fn catalog(root: PathBuf) -> OkfCatalog {
    OkfCatalog::new(
        test_name(),
        root,
        DefinitionVersion::parse("test-1").expect("a test version is a version"),
    )
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("sutura-catalog-okf-{name}-{}", std::process::id()));
    drop(std::fs::remove_dir_all(&dir));
    std::fs::create_dir_all(&dir).expect("a scratch directory is creatable");
    dir
}

/// Writes the given `(file, document)` pairs into a fresh scratch directory and opens a catalog over it.
///
/// Split from [`outcome_of`] so the file-writes (which may panic on a broken temp dir) stay in a
/// function that returns no `Result`, leaving the `Result`-returning helper to contain only the
/// catalog's own verdict.
fn catalog_of(name: &str, documents: &[(&str, &str)]) -> (PathBuf, OkfCatalog) {
    let root = scratch(name);
    for &(file, document) in documents {
        std::fs::write(root.join(file), document).expect("a descriptor is writable");
    }
    (root.clone(), catalog(root))
}

/// Loads a catalog built over the given descriptors, cleaning up the scratch directory on the way out.
fn outcome_of(name: &str, documents: &[(&str, &str)]) -> Result<PinnedDefinitions, OkfCatalogError> {
    let (root, catalog) = catalog_of(name, documents);
    let outcome = catalog.load();
    drop(std::fs::remove_dir_all(&root));
    outcome
}

fn error_for(name: &str, document: &str) -> OkfCatalogError {
    outcome_of(name, &[("model.yaml", document)]).expect_err("the descriptor must be refused")
}

const ORDERS: &str =
    "description: Orders, one row per order.\nfields:\n  - name: id\n    type: integer\n  - name: total\n    type: number\n";

const CUSTOMERS: &str = "title: Customers\ndescription: The people who place orders.\nfields:\n  - name: id\n    type: integer\n  - name: region\n    type: string\n";

#[test]
fn a_bundle_from_this_vocabulary_loads_pins_and_validates() {
    let pinned = outcome_of("loads", &[("orders.yaml", ORDERS), ("customers.yaml", CUSTOMERS)])
        .expect("a directory of Table Schema descriptors loads");
    let models = pinned.definitions().models();
    assert_eq!(models.len(), 2, "two descriptors load as two models");
    let orders_name = sutura_domain::model::ModelName::parse("orders").expect("a fixture name is a name");
    let orders = models.get(&orders_name).expect("orders is a model");
    assert_eq!(orders.columns().len(), 2, "orders carries its two columns");
    assert!(!orders.description().is_empty(), "orders carries its description");
}

/// `it_provides_exactly_what_it_declares`, lived in the adapter's own crate too: the bundle the
/// adapter produces and the declaration it publishes agree in both directions.
#[test]
fn it_declares_what_it_cannot_supply_and_the_bundle_agrees() {
    let pinned = outcome_of("fidelity", &[("orders.yaml", ORDERS)]).expect("the descriptor loads");
    let produced = MetadataCapabilities::produced(pinned.definitions(), pinned.knowledge());
    assert_eq!(
        <OkfCatalog as SemanticCatalog>::capabilities().checked_against(&produced),
        Ok(()),
        "the OKF catalog's declaration and its bundle disagree"
    );
    // And the honest half of "declares what it cannot supply": a numeric column type is a data type,
    // NOT a measure, so a bundle with a `type: number` column still carries no metric and the
    // declaration says so.
    assert!(
        pinned.definitions().metrics().is_empty(),
        "a Table Schema column type is not a measure"
    );
}

const ORDERS_WITH_COLUMN_METADATA: &str = "description: Orders, one row per order.\nprimaryKey: id\nfields:\n  - name: id\n    type: integer\n    description: The order's unique identifier.\n  - name: total\n    type: number\n";

#[test]
fn a_field_s_type_and_description_arrive_on_the_model_s_column() {
    let pinned = outcome_of("column-metadata", &[("orders.yaml", ORDERS_WITH_COLUMN_METADATA)])
        .expect("a descriptor with typed, described fields loads");
    let orders_name = sutura_domain::model::ModelName::parse("orders").expect("a fixture name is a name");
    let orders = pinned.definitions().models().get(&orders_name).expect("orders is a model");
    let id = sutura_domain::model::ColumnName::parse("id").expect("a fixture column is a column");
    let column = orders.column(&id).expect("id is a declared column");
    assert_eq!(
        column.data_type().map(sutura_domain::catalog::ColumnType::as_str),
        Some("integer")
    );
    assert_eq!(column.description(), "The order's unique identifier.");
    // `total` carries a type and no description - the two kinds are independent, over one column.
    let total = sutura_domain::model::ColumnName::parse("total").expect("a fixture column is a column");
    assert_eq!(orders.column(&total).expect("total is declared").description(), "");
    // `primaryKey: id` is evidence only: it arrives on the model and licenses nothing else.
    assert_eq!(orders.primary_key(), &std::collections::BTreeSet::from([id]));
}

/// A field's `title` describes it when there is no `description` - the same fallback the model's
/// own description already had, extended to the field level.
#[test]
fn a_field_s_title_describes_it_when_there_is_no_description() {
    let yaml = "description: Orders, one row per order.\nfields:\n  - name: id\n    title: The order's unique identifier.\n";
    let pinned = outcome_of("field-title-fallback", &[("orders.yaml", yaml)]).expect("a title-only field loads");
    let orders_name = sutura_domain::model::ModelName::parse("orders").expect("a fixture name is a name");
    let orders = pinned.definitions().models().get(&orders_name).expect("orders is a model");
    let id = sutura_domain::model::ColumnName::parse("id").expect("a fixture column is a column");
    assert_eq!(
        orders.column(&id).expect("id is declared").description(),
        "The order's unique identifier."
    );
}

/// A type this crate cannot represent - here, one over `MAX_COLUMN_TYPE_CHARS` - is dropped, not
/// refused: the load still succeeds and the column simply carries no type. `ColumnType`'s own doc
/// carries the argument; this is the load-level proof that a whole catalog does not go down for
/// one long type spelling, the defect found in review over a real `DataHub` field.
#[test]
fn a_column_type_this_crate_cannot_represent_is_dropped_rather_than_refusing_the_load() {
    let long_type = format!("STRUCT<{}last STRING>", "field STRING, ".repeat(80));
    assert!(
        long_type.len() > sutura_domain::catalog::MAX_COLUMN_TYPE_CHARS,
        "the fixture must actually exceed the bound to prove anything"
    );
    let yaml = format!("description: Orders, one row per order.\nfields:\n  - name: id\n    type: \"{long_type}\"\n");
    let pinned = outcome_of("long-column-type", &[("orders.yaml", &yaml)]).expect("a long column type still loads");
    let orders_name = sutura_domain::model::ModelName::parse("orders").expect("a fixture name is a name");
    let orders = pinned.definitions().models().get(&orders_name).expect("orders is a model");
    let id = sutura_domain::model::ColumnName::parse("id").expect("a fixture column is a column");
    assert_eq!(orders.column(&id).expect("id is declared").data_type(), None);
}

#[test]
fn a_primary_key_naming_an_undeclared_column_fails_the_load() {
    let err = error_for(
        "badkey",
        "description: A descriptor whose primaryKey is not one of its own fields.\nprimaryKey: nope\nfields:\n  - name: id\n",
    );
    assert!(
        matches!(err, OkfCatalogError::Inconsistent { .. }),
        "a primaryKey naming an unknown column is a cross-reference failure: {err}"
    );
}

#[test]
fn an_unusable_primary_key_shape_is_refused() {
    let err = error_for(
        "listkey",
        "description: A descriptor whose primaryKey is a number.\nprimaryKey: 1\nfields:\n  - name: id\n",
    );
    assert!(
        matches!(err, OkfCatalogError::InvalidPrimaryKey { .. }),
        "a primaryKey that is neither a string nor a list of strings is refused: {err}"
    );
}

#[test]
fn a_document_field_it_does_not_declare_fails_the_load() {
    let err = error_for(
        "unknown",
        "description: A descriptor with a stray key.\nfields:\n  - name: id\nunknown_thing: 1\n",
    );
    assert!(
        matches!(err, OkfCatalogError::Malformed { .. }),
        "an unknown top-level key is refused: {err}"
    );
}

#[test]
fn a_descriptor_without_a_description_is_refused() {
    let err = error_for("nodescription", "fields:\n  - name: id\n");
    assert!(
        matches!(err, OkfCatalogError::MissingDescription { .. }),
        "a descriptor that would leave Descriptions unproduced is refused: {err}"
    );
}

#[test]
fn duplicate_column_names_are_refused() {
    let err = error_for(
        "duplicate",
        "description: A descriptor listing the same column twice.\nfields:\n  - name: id\n  - name: id\n",
    );
    assert!(
        matches!(err, OkfCatalogError::DuplicateColumn { .. }),
        "a doubled column is refused rather than silently deduplicated: {err}"
    );
}

#[test]
fn an_empty_directory_is_an_error() {
    let root = scratch("empty");
    let err = catalog(root.clone()).load();
    drop(std::fs::remove_dir_all(&root));
    assert!(
        matches!(err, Err(OkfCatalogError::Empty { .. })),
        "an empty root loads no catalog: {err:?}"
    );
}

#[test]
fn the_example_corpus_loads_and_digests_stably() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/okf");
    let first = catalog(root.clone()).load().expect("the example corpus loads");
    let second = catalog(root).load().expect("the example corpus loads twice");
    assert_eq!(first.digest(), second.digest(), "the same corpus pins to the same digest");
    assert!(!first.definitions().models().is_empty(), "the example corpus has models");
}

/// The `NotARegularFile` refusal is on the OPENED handle, not on a separately-stated path.
///
/// `/dev/null` is a character device: `File::open` succeeds on it, and the `fstat` on that handle
/// shows a non-regular file, so [`super::read_document`] refuses it before reading a byte - the
/// exact device half of the post-walk swap this refusal exists to close. Deleting the `is_file`
/// check makes this cell red.
#[test]
fn a_non_regular_file_is_refused_on_the_opened_handle() {
    let file = std::fs::File::open("/dev/null").expect("a character device is openable");
    let metadata = file.metadata().expect("the opened handle has an fstat");
    let root = PathBuf::from("/unreached"); // names the catalog only in TooLarge text; unreached here
    let err = super::read_document(file, &metadata, Path::new("document.yaml"), &root, 0)
        .expect_err("a device is refused as not a regular file");
    assert!(
        matches!(err, OkfCatalogError::NotARegularFile { .. }),
        "the refusal is NotARegularFile, not {err:?}"
    );
}

/// A reader that DELIVERS more than the remaining budget is refused by the post-read re-check,
/// and the `take` cap is what keeps the read from pulling more than `remaining + 1` bytes.
///
/// The declared length is a real file well under the budget, so the fast path passes; the
/// wrapper below yields more than `remaining`, which only the post-read re-check
/// ([`super::read_document`]) refuses. The wrapper also counts how many bytes it was actually
/// pulled for - the bound that matters for allocation. However many bytes the lying reader COULD
/// yield, no more than `remaining + 1` are pulled (that is `Read::take`'s cap). Deleting the
/// `take` makes the count assertion red; deleting the re-check makes this cell's refusal red.
#[test]
fn a_reader_that_delivers_more_than_the_remaining_budget_is_refused() {
    let root = scratch("under-declared");
    std::fs::write(root.join("document.yaml"), "description: t\nfields:\n  - name: id\n").expect("a descriptor is writable");
    let file = std::fs::File::open(root.join("document.yaml")).expect("the descriptor is openable");
    let metadata = file.metadata().expect("the opened handle has an fstat");
    assert!(
        metadata.len() < super::MAX_CATALOG_BYTES,
        "the fixture is well under the aggregate budget"
    );
    let path = root.join("document.yaml");
    // Declared at the small length above, but yields more than `remaining`: only the re-check
    // (on top of the `take`) refuses it. The wrapper counts bytes pulled so the `take` cap is
    // itself tested, not just assumed. Finite (MAX + 100 at most) so the `take`-deleted mutation
    // fails the count red instead of reading forever; unbounded on its own, so what bounds the
    // read is `read_document`'s own `take`.
    let mut lying = CountingReader::bounded(super::MAX_CATALOG_BYTES + 100);
    let err =
        super::read_document(&mut lying, &metadata, &path, &root, 0).expect_err("yielding past the remaining budget is refused");
    assert!(matches!(err, OkfCatalogError::TooLarge { .. }), "the refusal is TooLarge");
    // The `take(remaining + 1)` cap: a reader that would happily yield MAX + 100 bytes must be
    // asked for at most remaining + 1 of them. With `read_document`'s `take` deleted, this count
    // is MAX + 2 and the assertion fails - the mutation this cell holds.
    assert!(
        lying.pulled() <= super::MAX_CATALOG_BYTES + 1,
        "the take cap must bound the read, pulled {} bytes",
        lying.pulled()
    );
    drop(std::fs::remove_dir_all(&root));
}

/// A [`std::io::Repeat`] that counts how many bytes were pulled, so a test can assert the read
/// was bounded. `Read` is implemented for `&mut Self`, so the counter is readable after the read
/// hands the borrow back.
struct CountingReader {
    inner: std::io::Take<std::io::Repeat>,
    pulled: u64,
}

impl CountingReader {
    fn bounded(bytes: u64) -> Self {
        Self {
            inner: std::io::repeat(b'a').take(bytes),
            pulled: 0,
        }
    }

    fn pulled(&self) -> u64 {
        self.pulled
    }
}

impl std::io::Read for &mut CountingReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.pulled += n as u64;
        Ok(n)
    }
}
