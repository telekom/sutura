//! The column's authored acknowledgement reaches the same pinned policy as native catalogs.

use super::{Description, ModelDoc};

#[test]
fn a_model_column_carries_its_raw_prefilter_acknowledgement() {
    let document: ModelDoc = serde_norway::from_str(
        "
kind: model
name: notes
source: data
table: notes
columns:
  - name: body
    type: TEXT
    redact: [{pattern: secret, replace: '[masked]'}]
    raw_prefilter:
      acknowledged_because: this source accepts secret-dependent search timing
",
    )
    .expect("a model column may acknowledge raw prefilter timing");
    let model = document.into_domain(Description::default()).expect("the column is valid");
    let column = model.columns().next().expect("the declared column");
    let encoded = serde_norway::to_value(column.text_policy()).expect("the policy serializes");
    assert_eq!(
        encoded["raw_prefilter"]["acknowledged_because"],
        "this source accepts secret-dependent search timing"
    );
}
