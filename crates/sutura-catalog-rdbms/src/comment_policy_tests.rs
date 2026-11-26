use sutura_domain::model::{ColumnName, ModelName};
use sutura_domain::pinned::{DefinitionVersion, SemanticCatalog as _};

use super::descriptions::InvalidDescriptionPolicy;
use super::{ColumnMetadata, Dictionary, DictionaryReader, RdbmsCatalog, RdbmsError, SourceName, Table, TableAddress};

fn name() -> SourceName {
    SourceName::parse("local").expect("a test name is a name")
}

fn version() -> DefinitionVersion {
    DefinitionVersion::parse("test").expect("a test version is a version")
}

fn table(model: &str, columns: Vec<String>, description: Option<String>) -> Table {
    Table::new(
        model.to_owned(),
        TableAddress::in_schema("public".to_owned(), model.to_owned()),
        columns,
        description,
    )
}

/// A reader that hands its recorded dictionary back unchanged.
struct SparseReader(Dictionary);
impl DictionaryReader for SparseReader {
    fn read_dictionary(&self) -> Result<Dictionary, RdbmsError> {
        Ok(self.0.clone())
    }
}

macro_rules! orders {
    ($pinned:expr) => {
        $pinned
            .definitions()
            .models()
            .get(&ModelName::parse("orders").expect("a test model is a model"))
            .expect("orders is a model")
    };
}

#[test]
fn on_invalid_comment_omit_drops_an_unusable_native_column_comment_and_serves_the_column_without_prose() {
    let comment = Some("The order's id.\r\nOne per row.".to_owned());
    let orders_table = table("orders", vec!["order_id".to_owned()], None)
        .with_column_metadata([("order_id".to_owned(), ColumnMetadata::new(None, comment))]);
    let crlf = SparseReader(Dictionary::new(vec![orders_table], Vec::new()));
    let pinned = RdbmsCatalog::new(name(), version(), crlf)
        .with_invalid_comment_policy(InvalidDescriptionPolicy::Omit)
        .load()
        .expect("omit serves the column without prose rather than refusing");
    let orders = orders!(pinned);
    let order_id = ColumnName::parse("order_id").expect("a test column is a column");
    assert_eq!(orders.column(&order_id).expect("order_id is declared").description(), "");
}

#[test]
fn on_invalid_comment_omit_drops_an_unusable_native_table_comment_and_serves_the_table_without_prose() {
    let crlf = SparseReader(Dictionary::new(
        vec![table(
            "orders",
            vec!["order_id".to_owned()],
            Some("Orders placed by customers.\r\nOne row per order.".to_owned()),
        )],
        Vec::new(),
    ));
    let pinned = RdbmsCatalog::new(name(), version(), crlf)
        .with_invalid_comment_policy(InvalidDescriptionPolicy::Omit)
        .load()
        .expect("omit serves the table without prose rather than refusing");
    let orders = orders!(pinned);
    assert_eq!(orders.description(), "");
}

#[test]
fn control_character_sanitization_lets_a_c1_control_comment_through_replaced() {
    let comment = Some("The order\u{0096}s id.".to_owned());
    let orders_table = table("orders", vec!["order_id".to_owned()], None)
        .with_column_metadata([("order_id".to_owned(), ColumnMetadata::new(None, comment))]);
    let c1 = SparseReader(Dictionary::new(vec![orders_table], Vec::new()));
    let pinned = RdbmsCatalog::new(name(), version(), c1)
        .with_control_character_sanitization(true)
        .load()
        .expect("a sanitized C1 control is a loadable description");
    let orders = orders!(pinned);
    let order_id = ColumnName::parse("order_id").expect("a test column is a column");
    assert_eq!(
        orders.column(&order_id).expect("order_id is declared").description(),
        "The order\u{FFFD}s id."
    );
}

/// An `on_invalid_description: Omit`-dropped mapping row whose fallback - the dictionary's own
/// native comment - is itself unusable no longer refuses the whole load; it follows
/// `on_invalid_comment` instead.
#[test]
fn omit_policy_s_native_fallback_being_itself_invalid_is_served_without_prose_not_refused() {
    use super::descriptions::DescriptionOverlay;

    fn dictionary_with(prose: &str) -> Dictionary {
        let table = table("orders", vec!["id".to_owned()], None)
            .with_column_metadata([("id".to_owned(), ColumnMetadata::new(None, Some(prose.to_owned())))]);
        Dictionary::new(vec![table], Vec::new())
    }

    struct Merged(Dictionary);
    impl DictionaryReader for Merged {
        fn read_dictionary(&self) -> Result<Dictionary, RdbmsError> {
            Ok(self.0.clone())
        }
    }

    let bounds = super::DictionaryBounds::new(super::NonZeroU64::new(100).unwrap(), super::NonZeroU64::new(10000).unwrap());
    let overlay_source = dictionary_with("Bad\u{96}prose");
    let overlay = DescriptionOverlay::from_dictionary(&overlay_source, bounds, false, InvalidDescriptionPolicy::Omit)
        .expect("omit never refuses at construction");
    let native = dictionary_with("Also bad\u{96}prose");
    let (merged, ignored) = native
        .overlay_descriptions(&[overlay])
        .expect("an omitted row falls back to the native comment, invalid or not");
    assert_eq!(ignored, 0);
    let pinned = RdbmsCatalog::new(name(), version(), Merged(merged))
        .with_invalid_comment_policy(InvalidDescriptionPolicy::Omit)
        .load()
        .expect("the invalid native fallback must be omitted, not refuse the whole catalog");
    let orders = orders!(pinned);
    let id = ColumnName::parse("id").expect("a test column is a column");
    assert_eq!(orders.column(&id).expect("id is declared").description(), "");
}

#[test]
fn control_character_sanitization_lets_a_c1_control_table_comment_through_replaced() {
    let crlf = SparseReader(Dictionary::new(
        vec![table(
            "orders",
            vec!["order_id".to_owned()],
            Some("Orders\u{0096}placed by customers.".to_owned()),
        )],
        Vec::new(),
    ));
    let pinned = RdbmsCatalog::new(name(), version(), crlf)
        .with_control_character_sanitization(true)
        .load()
        .expect("a sanitized C1 control table comment is a loadable description");
    let orders = orders!(pinned);
    assert_eq!(orders.description(), "Orders\u{FFFD}placed by customers.");
}

#[test]
fn control_character_sanitization_still_refuses_a_comment_left_invalid_after_replacement() {
    let comment = Some("The order\u{0096}s id.\r\nStill a bare CR.".to_owned());
    let orders_table = table("orders", vec!["order_id".to_owned()], None)
        .with_column_metadata([("order_id".to_owned(), ColumnMetadata::new(None, comment))]);
    let still_bad = SparseReader(Dictionary::new(vec![orders_table], Vec::new()));
    RdbmsCatalog::new(name(), version(), still_bad)
        .with_control_character_sanitization(true)
        .load()
        .expect_err("sanitizing C1 does not also fix an unrelated bare CR");
}

#[test]
fn control_character_sanitization_off_still_refuses_a_c1_control_comment() {
    let comment = Some("The order\u{0096}s id.".to_owned());
    let orders_table = table("orders", vec!["order_id".to_owned()], None)
        .with_column_metadata([("order_id".to_owned(), ColumnMetadata::new(None, comment))]);
    let c1 = SparseReader(Dictionary::new(vec![orders_table], Vec::new()));
    RdbmsCatalog::new(name(), version(), c1)
        .load()
        .expect_err("without sanitize_control_characters, a C1 control still refuses");
}
