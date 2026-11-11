//! Internal receipt bounds and typed-value validation; no remote honesty claim.

use serde_json::json;

use super::{decode, encode};
use crate::calendar::Date;
use crate::model::SourceName;
use crate::warehouse::ParamValue;
use crate::warehouse::execution::{Execution, PlanStage};
use crate::warehouse::schema::TypeDialect;

const LIMIT: usize = 1 << 16;

fn source() -> SourceName {
    SourceName::parse("sales").expect("a source")
}

#[test]
fn execution_receipts_preserve_order_duplicates_and_engine_binding_types() {
    let params = [
        ParamValue::Text(String::from("north")),
        ParamValue::Date(Date::parse("2026-01-02").expect("a date")),
    ];
    let mut records: Vec<_> = [
        TypeDialect::DuckDb,
        TypeDialect::Postgres,
        TypeDialect::ClickHouse,
        TypeDialect::BigQuery,
        TypeDialect::Oracle,
    ]
    .into_iter()
    .map(|dialect| Execution::sql(source(), dialect, String::from("SELECT bound_values"), &params))
    .collect();
    records.extend([
        Execution::DataFusion {
            source: Some(source()),
            stage: PlanStage::Files,
            logical_plan: String::from("TableScan: orders"),
        },
        Execution::DataFusion {
            source: None,
            stage: PlanStage::Combine,
            logical_plan: String::from("Aggregate: SUM(amount)"),
        },
        Execution::Cube {
            source: source(),
            query: json!({"query":{"measures":["orders.total"]}}),
        },
        Execution::bigquery_read(source(), b"opaque descriptor"),
        Execution::bigquery_read(source(), b"opaque descriptor"),
    ]);
    let encoded = encode(&records, LIMIT).expect("records fit");
    assert_eq!(decode(&encoded, LIMIT).expect("typed records parse"), records);
    assert!(
        !String::from_utf8(encoded)
            .expect("JSON is UTF-8")
            .contains("opaque descriptor")
    );
    assert_eq!(
        decode(&encode(&[], LIMIT).expect("explicit empty completion"), LIMIT).expect("empty parses"),
        Vec::new()
    );
}

#[test]
fn execution_receipts_reject_unknown_fields_identifiers_binding_shapes_and_dates() {
    let valid = json!({"kind":"sql","source":"sales","dialect":"duckdb","sql":"SELECT ?","parameters":[{"binding":{"kind":"position","value":0},"value":{"type":"utf8","value":"north"}}]});
    let mut invalid = Vec::new();
    for (path, value) in [
        ("/source", json!("")),
        ("/sql", json!("")),
        ("/dialect", json!("unknown")),
        ("/parameters/0/binding/value", json!(1)),
        ("/parameters/0/binding", json!({"kind":"named","value":"p0"})),
        ("/parameters/0/value", json!({"type":"oracle_date","value":"2026-02-30"})),
        ("/parameters/0/value", json!({"type":"oracle_string","value":"north"})),
    ] {
        let mut changed = valid.clone();
        *changed.pointer_mut(path).expect("a fixture field") = value;
        invalid.push(changed);
    }
    let mut unknown = valid.clone();
    unknown["extra"] = json!(true);
    invalid.push(unknown);
    let mut clickhouse = valid.clone();
    clickhouse["dialect"] = json!("clickhouse");
    invalid.push(clickhouse);
    let mut oracle = valid;
    oracle["dialect"] = json!("oracle");
    invalid.push(oracle);
    invalid.extend([
        json!({"kind":"datafusion","stage":"combine","logical_plan":"EmptyRelation"}),
        json!({"kind":"datafusion","source":null,"stage":"files","logical_plan":"TableScan"}),
        json!({"kind":"datafusion","source":"sales","stage":"combine","logical_plan":"Aggregate"}),
        json!({"kind":"datafusion","source":null,"stage":"combine","logical_plan":""}),
    ]);
    for record in invalid {
        let envelope = serde_json::to_vec(&json!({"version":1,"executions":[record]})).expect("JSON");
        assert!(decode(&envelope, LIMIT).is_err(), "{record}");
    }
    for envelope in [
        json!({"version":2,"executions":[]}),
        json!({"version":1,"executions":[],"extra":true}),
    ] {
        decode(&serde_json::to_vec(&envelope).expect("JSON"), LIMIT).expect_err("invalid envelope");
    }
}

#[test]
fn execution_receipts_bound_encoded_bytes_depth_and_record_count() {
    let records = [Execution::Cube {
        source: source(),
        query: json!({"escaped":"\"".repeat(300)}),
    }];
    assert!(encode(&records, 512).is_err(), "JSON expansion counts toward the wire bound");
    let encoded = encode(&records, LIMIT).expect("a larger bound fits");
    assert!(
        decode(&encoded, encoded.len() - 1).is_err(),
        "raw bytes checked before parsing"
    );
    let nested = format!(
        r#"{{"version":1,"executions":[{{"kind":"cube","source":"s","query":{}0{}}}]}}"#,
        "[".repeat(62),
        "]".repeat(62)
    );
    decode(nested.as_bytes(), LIMIT).expect_err("depth bound");
    let count = format!(
        r#"{{"version":1,"executions":[{}]}}"#,
        std::iter::repeat_n(r#"{"kind":"cube","source":"s","query":0}"#, 17)
            .collect::<Vec<_>>()
            .join(",")
    );
    assert!(count.len() < 1024, "this exercises count independently of byte length");
    decode(count.as_bytes(), 1024).expect_err("record count bound");
    decode(b"", LIMIT).expect_err("missing envelope");
    decode(br#"{"version":1,"executions":[]"#, LIMIT).expect_err("incomplete envelope");
}

#[test]
fn native_stream_receipts_hash_the_actual_descriptor_bytes() {
    let expected = [
        0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea, 0x41, 0x41, 0x40, 0xde, 0x5d, 0xae, 0x22, 0x23, 0xb0, 0x03, 0x61, 0xa3,
        0x96, 0x17, 0x7a, 0x9c, 0xb4, 0x10, 0xff, 0x61, 0xf2, 0x00, 0x15, 0xad,
    ];
    assert_eq!(
        Execution::bigquery_read(source(), b"abc"),
        Execution::BigQueryRead {
            source: source(),
            descriptor_sha256: expected
        }
    );
    assert_ne!(
        Execution::bigquery_read(source(), b"abc"),
        Execution::bigquery_read(source(), b"abcd")
    );
}
