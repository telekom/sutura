use std::sync::Arc;

use arrow_array::{ArrayRef, Int64Array, RecordBatch};
use arrow_ipc::writer::StreamWriter;
use arrow_schema::{DataType, Field, Schema};
use sutura_domain::identity::Secret;
use sutura_domain::model::SourceName;
use sutura_domain::source::SourcePosture;
use sutura_domain::warehouse::arrow::ResultBudget;

use super::{FoundryTransport, FoundryWarehouse, OntologyIdentifier, Parameters, SqlParameter, SqlRequest};

#[derive(Debug, thiserror::Error)]
#[error("the test transport is not callable")]
struct TestError;

#[derive(Debug)]
struct TestTransport;

impl FoundryTransport for TestTransport {
    type Error = TestError;

    fn execute(
        &self,
        _request: &SqlRequest,
        _token: &Secret,
        _deadline: sutura_domain::warehouse::deadline::Deadline,
    ) -> Result<Vec<u8>, Self::Error> {
        Err(TestError)
    }

    fn source_refused(&self, _error: &Self::Error) -> bool {
        false
    }

    fn deadline_exceeded(&self, _error: &Self::Error) -> bool {
        false
    }
}

#[test]
fn request_keeps_native_parameters_out_of_sql_text() {
    let request = SqlRequest {
        query: String::from("SELECT * FROM orders WHERE created_at = ?"),
        ontology_identifier: String::from("sales_ontology"),
        row_limit: 42,
        parameters: Some(Parameters {
            kind: "unnamedParameterValues",
            values: vec![SqlParameter::String {
                value: String::from("2026-11-10"),
            }],
        }),
        dry_run: false,
    };
    let wire = serde_json::to_value(request).expect("the request is serializable");
    assert_eq!(wire["query"], "SELECT * FROM orders WHERE created_at = ?");
    assert_eq!(wire["parameters"]["type"], "unnamedParameterValues");
    assert_eq!(wire["parameters"]["values"][0]["type"], "string");
    assert_eq!(wire["parameters"]["values"][0]["value"], "2026-11-10");
}

#[test]
fn ontology_identifier_matches_the_catalog_identifier_grammar() {
    OntologyIdentifier::parse("sales_ontology.v1").expect("a valid ontology identifier");
    for invalid in ["", "/ontology", " ontology", "a?b", "a\\b"] {
        assert!(OntologyIdentifier::parse(invalid).is_err(), "{invalid:?} must be refused");
    }
}

#[test]
fn arrow_ipc_is_decoded_through_the_domain_schema_guard() {
    let schema = Arc::new(Schema::new(vec![Field::new("count", DataType::Int64, false)]));
    let values: ArrayRef = Arc::new(Int64Array::from(vec![7_i64]));
    let batch = RecordBatch::try_new(Arc::clone(&schema), vec![values]).expect("the test batch is rectangular");
    let mut bytes = Vec::new();
    {
        let mut writer = StreamWriter::try_new(&mut bytes, &schema).expect("the IPC writer opens");
        writer.write(&batch).expect("the IPC batch writes");
        writer.finish().expect("the IPC stream finishes");
    }
    let warehouse = FoundryWarehouse::new(
        SourceName::parse("warehouse").expect("the test source is a name"),
        SourcePosture::SharedServiceUser {
            declared: sutura_domain::source::SharedIdentityDeclared::of(
                sutura_domain::source::AcknowledgementReason::parse("a deployment service account").expect("reason"),
            ),
        },
        OntologyIdentifier::parse("sales_ontology").expect("the ontology is valid"),
        Secret::new("test-token"),
        TestTransport,
        ResultBudget::of_bytes(core::num::NonZeroUsize::new(1024 * 1024).expect("the test budget is non-zero")),
    );
    let result = warehouse
        .decode(bytes, super::FOUNDRY_MAX_ROW_LIMIT - 1, super::boot_deadline())
        .expect("the Arrow stream is readable");
    assert_eq!(result.rows(), 1);
    assert_eq!(result.schema().fields().len(), 1);
}
