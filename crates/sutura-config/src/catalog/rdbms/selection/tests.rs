use super::{InvalidRdbmsCatalog, RawCatalog, native_selection};
use sutura_domain::catalog::{EmptyTablePolicy, UnsupportedColumnPolicy};

#[test]
fn a_native_column_rule_carries_its_raw_prefilter_acknowledgement() {
    let catalog: RawCatalog = serde_norway::from_str(
        r#"
kind: rdbms
name: dictionary
version: "1"
dictionary_source: native_dictionary
include: [{schema: '*', table: notes}]
column_rules:
  - column: '*.notes.body'
    redact: [{pattern: secret, replace: '[masked]'}]
    raw_prefilter:
      acknowledged_because: this source accepts secret-dependent search timing
"#,
    )
    .expect("a native rule may acknowledge raw prefilter timing");
    let selection = native_selection(&catalog).expect("the column rule is valid");
    let [(_, policy)] = selection.text_rules() else {
        panic!("one policy must remain");
    };
    let encoded = serde_norway::to_value(policy).expect("the policy serializes");
    assert_eq!(
        encoded["raw_prefilter"]["acknowledged_because"],
        "this source accepts secret-dependent search timing"
    );
}

#[test]
fn an_undeclared_native_selection_keeps_refuse_as_its_policy_for_both_opt_ins() {
    let catalog: RawCatalog = serde_norway::from_str(
        r#"
kind: rdbms
name: dictionary
version: "1"
dictionary_source: native_dictionary
include: [{schema: '*', table: notes}]
"#,
    )
    .expect("a minimal native selection parses");
    let selection = native_selection(&catalog).expect("no opt-in key still parses");
    assert_eq!(selection.on_unsupported_column(), UnsupportedColumnPolicy::Refuse);
    assert_eq!(selection.on_empty_table(), EmptyTablePolicy::Refuse);
}

#[test]
fn on_unsupported_column_and_on_empty_table_parse_their_omit_spelling() {
    let catalog: RawCatalog = serde_norway::from_str(
        r#"
kind: rdbms
name: dictionary
version: "1"
dictionary_source: native_dictionary
include: [{schema: '*', table: notes}]
on_unsupported_column: omit
on_empty_table: omit
"#,
    )
    .expect("the omit spelling parses");
    let selection = native_selection(&catalog).expect("omit is a valid policy for both keys");
    assert_eq!(selection.on_unsupported_column(), UnsupportedColumnPolicy::Omit);
    assert_eq!(selection.on_empty_table(), EmptyTablePolicy::Omit);
}

#[test]
fn an_unknown_on_unsupported_column_spelling_is_refused() {
    let catalog: RawCatalog = serde_norway::from_str(
        r#"
kind: rdbms
name: dictionary
version: "1"
dictionary_source: native_dictionary
include: [{schema: '*', table: notes}]
on_unsupported_column: sometimes
"#,
    )
    .expect("the catalog itself still deserializes");
    let error = native_selection(&catalog).expect_err("an unknown spelling is refused");
    assert!(matches!(error, InvalidRdbmsCatalog::UnsupportedColumnPolicy), "{error:?}");
}

#[test]
fn an_unknown_on_empty_table_spelling_is_refused() {
    let catalog: RawCatalog = serde_norway::from_str(
        r#"
kind: rdbms
name: dictionary
version: "1"
dictionary_source: native_dictionary
include: [{schema: '*', table: notes}]
on_empty_table: sometimes
"#,
    )
    .expect("the catalog itself still deserializes");
    let error = native_selection(&catalog).expect_err("an unknown spelling is refused");
    assert!(matches!(error, InvalidRdbmsCatalog::EmptyTablePolicy), "{error:?}");
}
