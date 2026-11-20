//! The acknowledgement participates in the persisted policy and schema identity.

use std::collections::BTreeMap;

use super::{Column, Definitions, Description, InvalidTextPolicy, Model, PrefilterCapability, RawPrefilter, TextPolicy};
use crate::model::{ColumnName, ModelName, SourceName, TableName};
use crate::warehouse::schema::{PinnedSchemas, TypeDialect};

fn policy(reason: &str) -> TextPolicy {
    serde_json::from_value(serde_json::json!({
        "redact": [{"pattern": "secret", "replace": "[masked]"}],
        "raw_prefilter": {"acknowledged_because": reason}
    }))
    .expect("an acknowledged raw necessary-condition policy must load")
}

#[test]
fn a_raw_prefilter_acknowledgement_is_required_bounded_and_persisted() {
    let accepted = policy("  this source accepts secret-dependent search timing  ");
    let encoded = serde_json::to_value(&accepted).expect("a policy serializes");
    assert_eq!(
        encoded["raw_prefilter"]["acknowledged_because"],
        "this source accepts secret-dependent search timing"
    );
    let reread: TextPolicy = serde_json::from_value(encoded.clone()).expect("the stored shape loads");
    assert_eq!(accepted, reread);
    assert_eq!(
        serde_json::to_string(&accepted).expect("the original policy serializes"),
        serde_json::to_string(&reread).expect("the reread policy serializes"),
        "the definition digest must see the same bytes after reloading the mapping"
    );
    for invalid in [
        serde_json::json!({}),
        serde_json::json!({"acknowledged_because": ""}),
        serde_json::json!({"acknowledged_because": " \t "}),
        serde_json::json!({"acknowledged_because": "line\nbreak"}),
        serde_json::json!({"acknowledged_because": "hidden\u{202e}text"}),
        serde_json::json!({"acknowledged_because": "x".repeat(401)}),
        serde_json::json!({"acknowledged_because": "a reason", "enabled": true}),
    ] {
        let mut written = encoded.clone();
        written["raw_prefilter"] = invalid;
        serde_json::from_value::<TextPolicy>(written).expect_err("an invalid acknowledgement must be refused");
    }
    let disabled: TextPolicy = serde_json::from_value(serde_json::json!({"redact": []})).expect("absence stays supported");
    assert!(
        serde_json::to_value(disabled)
            .expect("a policy serializes")
            .get("raw_prefilter")
            .is_none()
    );
}

#[test]
fn raw_prefilter_acknowledgements_change_schema_identity_and_physical_alias_agreement() {
    let source = SourceName::parse("data").expect("a source");
    let model = |name: &str, text: TextPolicy| {
        Model::new(
            ModelName::parse(name).expect("a model"),
            source.clone(),
            TableName::parse("notes").expect("a table"),
            [
                Column::from_metadata(ColumnName::parse("body").expect("a column"), Some("TEXT"), None, None)
                    .expect("text metadata")
                    .with_text_policy(text),
            ],
            Description::default(),
        )
    };
    let pin = |models| {
        PinnedSchemas::pin(
            &Definitions::assemble(models, vec![], vec![]).expect("a catalog"),
            &BTreeMap::from([(source.clone(), TypeDialect::DataFusion)]),
        )
    };
    let first = policy("first accepted timing reason");
    let second = policy("second accepted timing reason");
    let left = pin(vec![model("notes", first.clone())]).expect("a policy schema pins");
    let right = pin(vec![model("notes", second.clone())]).expect("another policy schema pins");
    assert_ne!(left.digest(), right.digest());
    pin(vec![model("notes", first), model("alias", second)]).expect_err("physical aliases must agree on the acknowledgement");
}

#[test]
fn a_raw_prefilter_without_a_redaction_rule_is_refused() {
    let acknowledged = serde_json::json!({"acknowledged_because": "this source accepts secret-dependent search timing"});
    for redact in [serde_json::json!([]), serde_json::Value::Null] {
        let mut written = serde_json::json!({"max_text_bytes": 128, "raw_prefilter": acknowledged});
        if !redact.is_null() {
            written["redact"] = redact;
        }
        let refused = serde_json::from_value::<TextPolicy>(written).expect_err("nothing to narrow, so nothing to acknowledge");
        assert!(
            refused
                .to_string()
                .contains("raw_prefilter requires at least one redact rule"),
            "{refused}"
        );
    }
    assert_eq!(
        TextPolicy::parse(None, Vec::new(), Some(RawPrefilter::parse("a reason").expect("a reason"))),
        Err(InvalidTextPolicy::PrefilterWithoutRedaction)
    );
}

#[test]
fn only_engines_with_comparison_evidence_carry_the_raw_condition() {
    let capable = [
        (TypeDialect::DataFusion, PrefilterCapability::DataFusion),
        (TypeDialect::Oracle, PrefilterCapability::Oracle),
    ];
    for (dialect, capability) in capable {
        assert_eq!(PrefilterCapability::of(dialect), Some(capability), "{dialect:?}");
    }
    for dialect in [
        TypeDialect::DuckDb,
        TypeDialect::Cube,
        TypeDialect::ClickHouse,
        TypeDialect::BigQuery,
    ] {
        assert_eq!(PrefilterCapability::of(dialect), None, "{dialect:?}");
    }
}
