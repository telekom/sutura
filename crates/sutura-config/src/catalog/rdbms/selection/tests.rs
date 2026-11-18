use super::{RawCatalog, native_selection};

#[test]
fn a_native_column_rule_carries_its_raw_prefilter_acknowledgement() {
    let catalog: RawCatalog = serde_norway::from_str(
        r#"
kind: rdbms
name: dictionary
version: "1"
dictionary_source: native_dictionary
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
