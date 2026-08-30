//! What the wire decides, exercised without a network.
//!
//! **Read the limit before the tests, because it decides what any green run here means.** Nothing in
//! this file opens a socket, and nothing in it is an acceptance test. What it asserts is two halves of
//! a claim that is genuinely smaller than *`BigQuery` accepts this*:
//!
//! - the request this adapter builds is the document it says it builds - asserted on the SERIALIZED
//!   body, so it is the bytes and not a struct;
//! - the answer this adapter reads is read the way it says it is - asserted over response documents
//!   written here.
//!
//! **Those documents are not the service's**, which is the whole gap `docs/adr/0017` refuses to paper
//! over: an emulator would give a green suite over statements the real service may reject, and a
//! response document written by the same person who wrote the decoder is the same fallacy one size
//! smaller. So these are tests of *this code against its own stated contract*, and the contract came
//! from the endpoint's published reference rather than from a running service.
//!
//! **And one thing here cannot be tested at all, deliberately:** the HTTP exchange itself. [`HOST`] is
//! a compile-time constant and the agent is built `https_only`, so there is no way to point this at a
//! loopback listener - which is a security property (no deployment can redirect the credential) paid
//! for with a coverage hole (no local test reaches the socket). The trade is stated rather than
//! resolved: making the host configurable to test it would remove the property the test would be
//! checking around.
//!
//! `crates/sutura-exec-bigquery/tests/acceptance.rs` is the leg that would close it, it needs a
//! project a developer names in their own environment, and **it has not been run.**

use sutura_domain::calendar::Date;
use sutura_domain::identity::{Expiry, Secret};
use sutura_domain::warehouse::ParamValue;

use crate::transport::{Cell, DatasetId, FieldType, JobRequest, JobTransport as _, ProjectId};
use crate::wire::credential::{AccessTokens, Bearer};
use crate::wire::{BigQueryWire, DryRun, HOST, WireError, accepted, agent, body, cells, columns, complete, reason, refusal, url};

// ------------------------------------------------------------------- the fixtures ----

/// An error nothing returns, for the pure functions that are generic in one.
#[derive(Debug, thiserror::Error)]
#[error("this credential source cannot fail")]
struct CannotFail;

/// A source that hands back whatever a test gave it.
struct Fixed(Bearer);

impl AccessTokens for Fixed {
    type Error = CannotFail;

    fn bearer(&self, _now_unix_seconds: u64) -> Result<Bearer, Self::Error> {
        Ok(self.0.clone())
    }
}

/// A source with nothing to hand back.
#[derive(Debug, thiserror::Error)]
#[error("there is no credential on this machine")]
struct NoCredential;

struct Missing;

impl AccessTokens for Missing {
    type Error = NoCredential;

    fn bearer(&self, _now_unix_seconds: u64) -> Result<Bearer, Self::Error> {
        Err(NoCredential)
    }
}

fn project() -> ProjectId {
    ProjectId::parse("a-payer").expect("a plain project id parses")
}

fn dataset() -> DatasetId {
    DatasetId::parse("Warehouse").expect("a plain dataset id parses")
}

/// The answer document, parsed the way the transport parses one.
fn answer(document: &str) -> crate::wire::QueryAnswer {
    serde_json::from_str(document).expect("the fixture is a query response")
}

// --------------------------------------------------------------- the request built ----

#[test]
fn the_request_carries_the_statement_and_its_values_in_separate_fields() {
    // The no-injection invariant at the one boundary where this adapter could break it. Asserted on
    // the serialized document rather than on the struct, because the struct is not what is sent.
    let params = [
        ParamValue::Text(String::from("active")),
        ParamValue::Date(Date::parse("2026-01-31").expect("an ISO date parses")),
    ];
    let project = project();
    let dataset = dataset();
    let request = JobRequest::new("SELECT `x` FROM `t` WHERE `s` = ? AND `d` <= ?", &params, &project, &dataset);
    let sent = serde_json::to_value(body(&request, DryRun::No)).expect("the body serializes");

    assert_eq!(sent["query"], "SELECT `x` FROM `t` WHERE `s` = ? AND `d` <= ?");
    assert_eq!(sent["parameterMode"], "POSITIONAL");
    assert_eq!(sent["defaultDataset"]["datasetId"], "Warehouse");
    // Neither value appears in the statement, and both appear as parameters in the plan's order.
    assert!(!sent["query"].to_string().contains("active"), "{sent}");
    assert!(!sent["query"].to_string().contains("2026-01-31"), "{sent}");
    assert_eq!(sent["queryParameters"][0]["parameterValue"]["value"], "active");
    assert_eq!(sent["queryParameters"][1]["parameterValue"]["value"], "2026-01-31");
}

#[test]
fn a_positional_parameter_carries_no_name_at_all() {
    // `POSITIONAL` means the nth value pairs with the nth `?`, and the endpoint takes one parameter
    // form or the other and not both - so an entry that grew a `name` would change the query's
    // meaning rather than merely add a field. Asserted as the ABSENCE of the key, because a `null`
    // name and no name are different documents.
    let params = [ParamValue::Text(String::from("active"))];
    let project = project();
    let dataset = dataset();
    let request = JobRequest::new("SELECT 1 FROM `t` WHERE `s` = ?", &params, &project, &dataset);
    let sent = serde_json::to_value(body(&request, DryRun::No)).expect("the body serializes");

    let entry = sent["queryParameters"][0].as_object().expect("a parameter is an object");
    assert!(!entry.contains_key("name"), "a positional parameter carried a name: {sent}");
    assert_eq!(entry.len(), 2, "a parameter is exactly a type and a value: {sent}");
}

#[test]
fn a_date_parameter_is_declared_as_a_date_and_travels_as_its_iso_text() {
    // The typed half of the binding. `DuckDB` gets a day count and this endpoint gets ISO text, which
    // is the sentence `ParamValue`'s own documentation makes: the driver decides how a date is
    // written on the wire.
    let params = [ParamValue::Date(Date::parse("2026-08-30").expect("an ISO date parses"))];
    let project = project();
    let dataset = dataset();
    let request = JobRequest::new("SELECT 1 FROM `t` WHERE `d` = ?", &params, &project, &dataset);
    let sent = serde_json::to_value(body(&request, DryRun::No)).expect("the body serializes");

    assert_eq!(sent["queryParameters"][0]["parameterType"]["type"], "DATE");
    assert_eq!(sent["queryParameters"][0]["parameterValue"]["value"], "2026-08-30");

    let text = [ParamValue::Text(String::from("500"))];
    let other = JobRequest::new("SELECT 1 FROM `t` WHERE `s` = ?", &text, &project, &dataset);
    let sent = serde_json::to_value(body(&other, DryRun::No)).expect("the body serializes");
    assert_eq!(sent["queryParameters"][0]["parameterType"]["type"], "STRING");
}

#[test]
fn the_request_writes_the_two_flags_whose_endpoint_defaults_are_the_wrong_ones() {
    // Both are DECISIONS rather than omissions, and both default the other way at the endpoint:
    // `useLegacySql` defaults to true, and the result cache is on. Pinned by value, because forgetting
    // either is silent - a legacy-SQL request fails on the first backtick, and a cached answer is a
    // number that reproduced the cache.
    let project = project();
    let dataset = dataset();
    let request = JobRequest::new("SELECT 1 FROM `t`", &[], &project, &dataset);
    let sent = serde_json::to_value(body(&request, DryRun::No)).expect("the body serializes");

    assert_eq!(sent["useLegacySql"], false);
    assert_eq!(sent["useQueryCache"], false);
    assert_eq!(sent["timeoutMs"], crate::wire::JOB_TIMEOUT_MS);
}

#[test]
fn a_dry_run_and_a_real_run_differ_by_that_one_field() {
    // What makes `dry_run` answering `PreFlight::Accepted` honest is that the two submissions are the
    // same request. If anything else differed, a validated statement and an executed one would not be
    // the same statement.
    let params = [ParamValue::Text(String::from("active"))];
    let project = project();
    let dataset = dataset();
    let request = JobRequest::new("SELECT 1 FROM `t` WHERE `s` = ?", &params, &project, &dataset);

    let mut validated = serde_json::to_value(body(&request, DryRun::Yes)).expect("the body serializes");
    let executed = serde_json::to_value(body(&request, DryRun::No)).expect("the body serializes");

    assert_eq!(validated["dryRun"], true);
    assert_eq!(executed["dryRun"], false);
    validated["dryRun"] = serde_json::Value::Bool(false);
    assert_eq!(validated, executed, "a dry run differs from a real one by more than the flag");
}

#[test]
fn the_url_names_the_billing_project_against_a_host_no_deployment_chooses() {
    let project = project();
    let dataset = dataset();
    let request = JobRequest::new("SELECT 1 FROM `t`", &[], &project, &dataset);

    assert_eq!(url(&request), format!("{HOST}/bigquery/v2/projects/a-payer/queries"));
    // The host is a constant, and this is the assertion that says so: a configurable one would be a
    // key that could point a bearer token somewhere else.
    assert!(HOST.starts_with("https://"), "{HOST}");
}

// ---------------------------------------------------------------- the answer read ----

#[test]
fn an_incomplete_job_is_refused_rather_than_polled() {
    // The refusal that keeps a partial answer from reading, to `answer()`, as *under the cap, not
    // truncated*. Polling is the alternative and it needs a `location` this deployment does not
    // declare - see the module header.
    let refused = complete::<CannotFail>(answer(r#"{"jobComplete": false}"#));
    assert!(matches!(refused, Err(WireError::NotComplete)), "{refused:?}");
}

#[test]
fn a_page_of_a_larger_result_is_refused() {
    let document = r#"{"jobComplete": true, "totalRows": "2", "pageToken": "next",
        "schema": {"fields": [{"name": "n", "type": "INTEGER"}]},
        "rows": [{"f": [{"v": "1"}]}, {"f": [{"v": "2"}]}]}"#;
    let refused = complete::<CannotFail>(answer(document));
    assert!(matches!(refused, Err(WireError::MoreThanOnePage)), "{refused:?}");
}

#[test]
fn a_complete_job_that_states_no_total_is_refused_rather_than_read_as_empty() {
    // Reading an absent total as zero would make a delivered-nothing answer and a stated-nothing
    // answer the same value, and only one of those is a result this adapter may certify.
    let refused = complete::<CannotFail>(answer(r#"{"jobComplete": true}"#));
    assert!(matches!(refused, Err(WireError::NoTotal)), "{refused:?}");
}

#[test]
fn a_total_that_is_not_a_number_is_refused() {
    // It arrives as TEXT, because the endpoint writes 64-bit integers as JSON strings - so "not a
    // number" is a reachable document rather than a type error.
    let refused = complete::<CannotFail>(answer(r#"{"jobComplete": true, "totalRows": "lots"}"#));
    assert!(matches!(refused, Err(WireError::NotATotal { .. })), "{refused:?}");
}

#[test]
fn the_reported_total_travels_beside_the_rows_so_the_adapter_can_compare_them() {
    // This is the half the seam owns. The COMPARISON is `BigQueryWarehouse::rows`, which refuses
    // `Incomplete`; what the wire has to get right is carrying the endpoint's own number rather than
    // the length of what it happened to send.
    let document = r#"{"jobComplete": true, "totalRows": "7",
        "schema": {"fields": [{"name": "n", "type": "INTEGER"}]},
        "rows": [{"f": [{"v": "1"}]}]}"#;
    let rows = complete::<CannotFail>(answer(document)).expect("a complete page decodes");
    assert_eq!(rows.total_rows(), 7);
    assert_eq!(rows.rows().len(), 1);
    assert_eq!(rows.fields().len(), 1);
    assert_eq!(*rows.fields()[0].kind(), FieldType::Int64);
}

#[test]
fn a_cell_is_text_or_null_and_anything_else_is_refused_naming_its_position() {
    // Every scalar the endpoint returns is JSON text whatever its declared type, so an array, an
    // object, a bare number or a bare bool is a column outside `FieldType`'s vocabulary. Refused
    // naming the POSITION and not the value, because the value is a row.
    let good = r#"{"jobComplete": true, "totalRows": "1",
        "schema": {"fields": [{"name": "n", "type": "INTEGER"}, {"name": "s", "type": "STRING"}]},
        "rows": [{"f": [{"v": "250"}, {"v": null}]}]}"#;
    let rows = complete::<CannotFail>(answer(good)).expect("text and null decode");
    assert_eq!(rows.rows()[0][0], Cell::Text(String::from("250")));
    assert_eq!(rows.rows()[0][1], Cell::Null);

    for hostile in [r#"{"v": [1, 2]}"#, r#"{"v": {"a": 1}}"#, r#"{"v": 250}"#, r#"{"v": true}"#] {
        let document = format!(
            r#"{{"jobComplete": true, "totalRows": "1",
               "schema": {{"fields": [{{"name": "n", "type": "INTEGER"}}, {{"name": "s", "type": "STRING"}}]}},
               "rows": [{{"f": [{{"v": "1"}}, {hostile}]}}]}}"#
        );
        let refused = complete::<CannotFail>(answer(&document));
        assert!(
            matches!(refused, Err(WireError::NotAScalar { row: 0, column: 1 })),
            "{hostile} was accepted as a cell: {refused:?}"
        );
    }
}

#[test]
fn rows_with_no_schema_are_refused_and_no_rows_with_no_schema_are_an_empty_result() {
    // Two documents that differ only in whether anything was delivered, because the schema's absence
    // means two different things: nothing to read the cells against, and nothing to read.
    let refused = columns::<CannotFail>(None, 3);
    assert!(matches!(refused, Err(WireError::NoSchema { rows: 3 })), "{refused:?}");
    assert!(
        columns::<CannotFail>(None, 0)
            .expect("an empty result has no columns")
            .is_empty()
    );
}

#[test]
fn a_job_the_endpoint_accepted_and_reported_an_error_against_is_a_failure() {
    // A `200` carrying `errors` is a job that ran and did not work, which is a different fact from a
    // refused statement - and it is checked where both the dry run and the real run inherit it.
    let document = r#"{"jobComplete": true, "totalRows": "0", "errors": [{"reason": "resourcesExceeded"}]}"#;
    let refused = accepted::<CannotFail>(answer(document));
    match refused {
        Err(WireError::Failed { named }) => assert_eq!(named, "resourcesExceeded"),
        other => panic!("a reported error was read as an answer: {other:?}"),
    }
    // And the same document without the errors is an answer.
    accepted::<CannotFail>(answer(r#"{"jobComplete": true, "totalRows": "0"}"#)).expect("no errors is an answer");
}

// ------------------------------------------------------------ the refusal it maps ----

#[test]
fn a_refusal_keeps_the_endpoints_reason_and_never_its_message() {
    // The endpoint's `reason` is a fixed vocabulary an operator acts on; the `message` beside it is
    // free text from another service, and it quotes the resource that was refused. So the message is
    // not a field on this type at all - what is not read cannot be logged by accident.
    let document = r#"{"error": {"code": 403, "status": "PERMISSION_DENIED", "message":
        "Access Denied: Table a-payer:Warehouse.ledger: User does not have permission",
        "errors": [{"reason": "accessDenied", "domain": "global", "message":
        "Access Denied: Table a-payer:Warehouse.ledger"}]}}"#;
    let mapped: WireError<CannotFail> = refusal(403, document);
    match mapped {
        WireError::Refused { status, ref named } => {
            assert_eq!(status, 403);
            assert_eq!(*named, "accessDenied");
        }
        ref other => panic!("a refusal was mapped to {other:?}"),
    }
    let shown = mapped.to_string();
    assert!(!shown.contains("ledger"), "the refusal quoted the resource: {shown}");
    assert!(!shown.contains("a-payer"), "the refusal quoted the project: {shown}");
}

#[test]
fn a_refusal_whose_body_is_not_the_envelope_still_reports_the_status() {
    // The status is the guaranteed half. A gateway, a proxy or an outage answers with something that
    // is not this document, and a mapping that failed there would turn a refusal into a parse error.
    for document in ["", "<html>502 Bad Gateway</html>", "{}", r#"{"error": {}}"#] {
        let mapped: WireError<CannotFail> = refusal(502, document);
        match mapped {
            WireError::Refused { status, ref named } => {
                assert_eq!(status, 502);
                assert!(named.is_empty(), "{document:?} produced a reason: {named}");
            }
            ref other => panic!("{document:?} was mapped to {other:?}"),
        }
    }
}

#[test]
fn a_reason_from_the_endpoint_is_bounded_and_filtered() {
    // Foreign text heading for a log. Bounded by characters rather than bytes, because a byte slice of
    // somebody else's UTF-8 can land inside a character - which is also why `clippy::string_slice` is
    // denied here.
    assert_eq!(reason(Some(String::from("accessDenied"))), "accessDenied");
    assert_eq!(reason(None), "");
    assert_eq!(
        reason(Some(String::from("bad\nreason\r\"quoted\" \u{1b}"))),
        "badreasonquoted"
    );
    let long = reason(Some("a".repeat(4096)));
    assert_eq!(long.len(), crate::wire::MAX_REASON_CHARS);
}

// ----------------------------------------------------- the credential, before a send ----

#[test]
fn an_expired_credential_is_refused_before_anything_is_sent() {
    // The one guard in `submit` a test can reach with no socket, because it runs BEFORE the request is
    // built. A source handing back an expired token has a clock problem, and presenting it anyway
    // would turn that into a 401 an operator reads as a permissions fault.
    let wire = BigQueryWire::new(agent(), Fixed(Bearer::of(Secret::new("t"), Expiry::At { unix_seconds: 1 })));
    let project = project();
    let dataset = dataset();
    let request = JobRequest::new("SELECT 1 FROM `t`", &[], &project, &dataset);

    let refused = wire.run(&request);
    assert!(matches!(refused, Err(WireError::Expired { at: 1, .. })), "{refused:?}");
    // Both port methods, because a check on one of two credential-taking calls is the shape this
    // repository already refused once in this crate's `deliverable`.
    let refused = wire.validate(&request);
    assert!(matches!(refused, Err(WireError::Expired { at: 1, .. })), "{refused:?}");
}

#[test]
fn a_credential_source_that_cannot_answer_stops_before_anything_is_sent() {
    // And it keeps the source's own error on the chain, which is what the generic in `WireError`
    // exists for: a caller that knows which source is installed can tell a missing file from a
    // refused refresh.
    let wire = BigQueryWire::new(agent(), Missing);
    let project = project();
    let dataset = dataset();
    let request = JobRequest::new("SELECT 1 FROM `t`", &[], &project, &dataset);

    let refused = wire.run(&request);
    match refused {
        Err(WireError::Credential { ref cause }) => assert_eq!(cause.to_string(), "there is no credential on this machine"),
        ref other => panic!("a missing credential was mapped to {other:?}"),
    }
    assert!(
        core::error::Error::source(&refused.expect_err("it refused")).is_some(),
        "the cause did not survive #[source]"
    );
}

#[test]
fn a_bearer_never_shows_its_token() {
    // `Secret` is the mechanism and this is the assertion that it survives being wrapped: a `Bearer`
    // is what the transport holds, and a derived `Debug` on it would print the token.
    let bearer = Bearer::of(Secret::new("ya29-do-not-log-me"), Expiry::NothingExpires);
    assert!(!format!("{bearer:?}").contains("ya29"), "{bearer:?}");
    assert_eq!(bearer.token().expose(), "ya29-do-not-log-me");
}

#[test]
fn the_cells_helper_names_the_row_as_well_as_the_column() {
    // The position is two numbers, and a refusal that only named the column would send a reader to
    // the wrong row of a result they cannot see.
    let document = r#"{"jobComplete": true, "totalRows": "2",
        "schema": {"fields": [{"name": "n", "type": "INTEGER"}]},
        "rows": [{"f": [{"v": "1"}]}, {"f": [{"v": 2}]}]}"#;
    let refused = cells::<CannotFail>(answer(document).rows);
    assert!(
        matches!(refused, Err(WireError::NotAScalar { row: 1, column: 0 })),
        "{refused:?}"
    );
}
