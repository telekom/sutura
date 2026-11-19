//! A column's authored collation reaches the domain column, and a bad one is a refusal.

use sutura_domain::catalog::Collation;

use super::{Description, InvalidModelDocument, ModelDoc};

fn model(collation: &str) -> ModelDoc {
    serde_norway::from_str(&format!(
        "kind: model\nname: notes\nsource: data\ntable: notes\ncolumns:\n  - name: body\n    type: TEXT\n{collation}"
    ))
    .expect("a model document")
}

#[test]
fn a_column_declares_binary_or_a_named_collation_and_says_nothing_otherwise() {
    let read = |line: &str| {
        let model = model(line).into_domain(Description::default()).expect("the column is valid");
        model.columns().next().expect("the declared column").collation().clone()
    };
    assert_eq!(read("    collation: binary\n"), Collation::Binary);
    assert_eq!(read("    collation: BINARY\n"), Collation::Binary);
    assert_eq!(read("    collation: und:ci\n"), Collation::named("und:ci"));
    assert_eq!(read(""), Collation::Unknown);
    assert!(!read("").is_binary());
}

#[test]
fn a_collation_that_is_not_a_name_refuses_the_document() {
    let refused = model("    collation: \"two words\"\n")
        .into_domain(Description::default())
        .expect_err("a collation with a space is no name");
    assert!(matches!(refused, InvalidModelDocument::Collation { .. }), "{refused:?}");
}
