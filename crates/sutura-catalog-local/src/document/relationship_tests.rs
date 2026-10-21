//! Tests for the relationship document, split out of `tests.rs` for the thousand-line cap.

use super::{InvalidRelationshipDocument, RelationshipDoc};
use sutura_domain::catalog::{InvalidComparison, InvalidJoinKeys};
use sutura_domain::plan::LinkKind;

fn relationship_doc(yaml: &str) -> Result<RelationshipDoc, serde_norway::Error> {
    serde_norway::from_str(yaml)
}

// A compound relationship: a `keys:` list and no `column` on either endpoint. The single-pair
// form keeps `origin: { model, column }` / `target: { model, column }` and no `keys:` - the two
// are mutually exclusive, which `RELATIONSHIP_BOTH` and `RELATIONSHIP_NEITHER` below test.
const COMPOUND_RELATIONSHIP: &str = "
kind: relationship
name: usage_subscription
origin: { model: daily_usage }
target: { model: subscriptions }
join_type: many_to_one
keys:
  - { origin: subscription_key, target: subscription_key }
  - { origin: usage_date, grain: month, target: month }
";

// The single-pair form: one `origin.column = target.column` and no `keys:`.
const SINGLE_PAIR_RELATIONSHIP: &str = "
kind: relationship
name: orders_customer
origin: { model: orders, column: customer_id }
target: { model: customers, column: customer_id }
join_type: many_to_one
";

// A document that declares `keys:` AND a `column` on the ORIGIN endpoint - the conflict shape.
const RELATIONSHIP_BOTH: &str = "
kind: relationship
name: usage_subscription
origin: { model: daily_usage, column: subscription_key }
target: { model: subscriptions }
join_type: many_to_one
keys:
  - { origin: subscription_key, target: subscription_key }
  - { origin: usage_date, grain: month, target: month }
";

// The other half of the conflict shape: `keys:` AND a `column` on the TARGET endpoint instead -
// `RelationshipDoc::into_domain`'s match has a separate alternative for this
// (`(Some(_), _, Some(_))`), which `RELATIONSHIP_BOTH` above never exercises because its target
// carries no column.
const RELATIONSHIP_BOTH_TARGET_COLUMN: &str = "
kind: relationship
name: usage_subscription
origin: { model: daily_usage }
target: { model: subscriptions, column: subscription_key }
join_type: many_to_one
keys:
  - { origin: subscription_key, target: subscription_key }
  - { origin: usage_date, grain: month, target: month }
";

// A document that declares neither a column pair nor `keys:`.
const RELATIONSHIP_NEITHER: &str = "
kind: relationship
name: usage_subscription
origin: { model: daily_usage }
target: { model: subscriptions }
join_type: many_to_one
";

// A document with an empty `keys:` list, which cannot name a join that links anything.
const RELATIONSHIP_EMPTY_KEYS: &str = "
kind: relationship
name: usage_subscription
origin: { model: daily_usage }
target: { model: subscriptions }
join_type: many_to_one
keys: []
";

/// A compound join's keys parse into the typed `TruncatedEqual`/`Equal` pair, not the legacy
/// `origin`/`target` pair alone.
#[test]
fn a_compound_relationships_keys_parse() {
    let doc = relationship_doc(COMPOUND_RELATIONSHIP).expect("two well-formed keys are well-formed");
    let relationship = doc.into_domain().expect("two keys is a non-empty set");
    assert_eq!(relationship.keys().len(), 2);
    assert_eq!(
        relationship.origin_model().as_str(),
        "daily_usage",
        "the origin MODEL still lands on the relationship when the columns live in `keys:`"
    );
}

/// The single-pair form - `origin.column = target.column` and no `keys:` - still loads.
#[test]
fn a_single_pair_relationship_still_parses_to_one_equality() {
    let doc = relationship_doc(SINGLE_PAIR_RELATIONSHIP).expect("a column pair is well-formed");
    let relationship = doc.into_domain().expect("one column pair is a non-empty set");
    assert_eq!(relationship.keys().len(), 1);
}

/// A document that declares both a column pair and a `keys:` list is refused, naming the conflict.
#[test]
fn a_relationship_declaring_both_columns_and_keys_is_refused() {
    let doc = relationship_doc(RELATIONSHIP_BOTH).expect("the shape parses; the refusal is `into_domain`'s");
    assert_eq!(
        doc.into_domain().unwrap_err(),
        InvalidRelationshipDocument::Both,
        "a document choosing both join forms must refuse rather than silently prefer one"
    );
}

/// The `Both` refusal's other alternative: `keys:` plus a column on the TARGET endpoint, with the
/// origin endpoint bare. `RELATIONSHIP_BOTH` above only ever puts the column on origin, so this is
/// the twin that exercises `(Some(_), _, Some(_))` rather than `(Some(_), Some(_), _)`.
#[test]
fn a_relationship_declaring_keys_and_a_target_column_is_also_refused() {
    let doc = relationship_doc(RELATIONSHIP_BOTH_TARGET_COLUMN).expect("the shape parses; the refusal is `into_domain`'s");
    assert_eq!(
        doc.into_domain().unwrap_err(),
        InvalidRelationshipDocument::Both,
        "a `keys:` list plus a column on either endpoint alone is still the same conflict"
    );
}

/// A relationship with neither a column pair nor a `keys:` list joins nothing and is refused.
#[test]
fn a_relationship_declaring_neither_form_is_refused() {
    let doc = relationship_doc(RELATIONSHIP_NEITHER).expect("the shape parses; the refusal is `into_domain`'s");
    assert_eq!(
        doc.into_domain().unwrap_err(),
        InvalidRelationshipDocument::Neither,
        "a relationship must name at least the single pair or `keys:`, never neither"
    );
}

/// An empty `keys:` list is refused at `into_domain`, surfacing the domain's empty-set refusal.
#[test]
fn an_empty_keys_list_is_refused() {
    let doc = relationship_doc(RELATIONSHIP_EMPTY_KEYS).expect("`keys: []` is well-formed YAML");
    assert!(
        matches!(
            doc.into_domain().unwrap_err(),
            InvalidRelationshipDocument::EmptyKeys(InvalidJoinKeys::Empty)
        ),
        "an empty keys list must reach the domain's own empty-set refusal"
    );
}

/// `compare_as:` is the author's word for the kind the two keys compare as, and each of the five
/// words reaches the domain relationship as its own kind; a document without it declares nothing.
#[test]
fn a_markdown_compare_as_reaches_the_relationship() {
    for (word, kind) in [
        ("exact_integer", LinkKind::ExactInteger),
        ("exact_decimal", LinkKind::ExactDecimal),
        ("text", LinkKind::Text),
        ("date", LinkKind::Date),
        ("boolean", LinkKind::Boolean),
    ] {
        let relationship = relationship_doc(&format!("{SINGLE_PAIR_RELATIONSHIP}compare_as: {word}\n"))
            .expect("a known word parses")
            .into_domain()
            .expect("one key takes a comparison");
        assert_eq!(relationship.comparison(), Some(kind), "`compare_as: {word}`");
    }
    let silent = relationship_doc(SINGLE_PAIR_RELATIONSHIP)
        .expect("a column pair is well-formed")
        .into_domain()
        .expect("one column pair is a non-empty set");
    assert_eq!(silent.comparison(), None, "an absent `compare_as:` is equality of kind");
}

/// A word that is not a kind is a parse error at the document edge, naming the word, and a
/// misspelt key stays refused by `deny_unknown_fields`.
#[test]
fn an_unknown_compare_as_word_is_refused() {
    let error =
        relationship_doc(&format!("{SINGLE_PAIR_RELATIONSHIP}compare_as: float\n")).expect_err("`float` is not a link kind");
    assert!(error.to_string().contains("float"), "the error must name the word: {error}");
    relationship_doc(&format!("{SINGLE_PAIR_RELATIONSHIP}compare_a: text\n"))
        .expect_err("a misspelt `compare_as` is an unknown field");
}

/// A comparison applies to one key pair, so a compound relationship that declares one is refused.
#[test]
fn a_compare_as_on_a_compound_relationship_is_refused() {
    let declared = format!("{COMPOUND_RELATIONSHIP}compare_as: text\n");
    assert_eq!(
        relationship_doc(&declared)
            .expect("the shape parses; the refusal is `into_domain`'s")
            .into_domain()
            .unwrap_err(),
        InvalidRelationshipDocument::Comparison(InvalidComparison::CompoundKey { keys: 2 }),
    );
}

/// **The measurement `#[serde(deny_unknown_fields)]` on `JoinKeyDoc` exists for.** Before it, a
/// typo'd `grian:` in place of `grain:` parsed silently - serde drops a field it does not
/// recognise unless the struct demands otherwise - and the entry fell through to
/// `JoinKeyDoc::into_domain`'s `None` arm, becoming a plain `JoinKey::Equal { origin: usage_date,
/// target: month }` instead of the truncated key the document meant. That key is still a legal
/// declaration - `usage_date = month` just never matches - so nothing downstream would have
/// caught it: no parse error, no consistency-check refusal, only a relationship that silently
/// answers nothing for the dimension it was meant to unlock. Red before `deny_unknown_fields` was
/// added: this parse used to be `Ok`.
#[test]
fn a_typo_in_a_compound_keys_grain_field_is_refused_rather_than_silently_dropped() {
    let yaml = COMPOUND_RELATIONSHIP.replace("grain: month", "grian: month");
    assert_ne!(
        yaml, COMPOUND_RELATIONSHIP,
        "the replacement must have found the field it means to typo"
    );
    let error = relationship_doc(&yaml).expect_err("an unknown field inside a compound key must not parse");
    assert!(
        error.to_string().contains("grian"),
        "the parse error must name the field it could not place: {error}"
    );
}
