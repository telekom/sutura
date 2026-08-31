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
//! `crates/sutura-exec-bigquery/tests/acceptance.rs` is the leg that closes part of it, and **it has
//! been run**: three tests green against a real dataset on 2026-08-30. What it closed is *one
//! statement is accepted*; what stays open is everything the corpus would exercise, and the HTTP
//! exchange, which no local test can reach for the reason above.

use sutura_domain::calendar::Date;
use sutura_domain::identity::{Expiry, Secret};
use sutura_domain::warehouse::ParamValue;

use core::time::Duration;

use crate::transport::{Cell, DatasetId, FieldType, JobRequest, JobTransport as _, ProjectId};
use crate::wire::credential::{AccessTokens, Bearer, QuotaProject};
use crate::wire::document::{body, cells, columns, complete, refusal, reported, url};
use crate::wire::{
    BigQueryWire, BytesBilledCeiling, CallDeadline, DryRun, HOST, JobBounds, QueryDeadline, UnusableBound, WireAgent, WireError,
    bounded,
};

// ------------------------------------------------------------------- the fixtures ----

/// An error nothing returns, for the pure functions that are generic in one.
#[derive(Debug, thiserror::Error)]
#[error("this credential source cannot fail")]
struct CannotFail;

/// A source that hands back whatever a test gave it, and records the budget it was handed.
struct Fixed {
    bearer: Bearer,
    /// What `remaining()` said when this source was asked. `None` before it is asked at all.
    ///
    /// A `Cell` rather than a plain field because the port takes `&self` - which is the property that
    /// makes an adapter shareable across tasks and is not something a fake may relax.
    handed: core::cell::Cell<Option<Duration>>,
}

impl Fixed {
    fn holding(bearer: Bearer) -> Self {
        Self {
            bearer,
            handed: core::cell::Cell::new(None),
        }
    }
}

impl AccessTokens for Fixed {
    type Error = CannotFail;

    fn quota_project(&self) -> QuotaProject {
        QuotaProject::Required
    }

    fn bearer(&self, _now_unix_seconds: u64, within: CallDeadline) -> Result<Bearer, Self::Error> {
        self.handed.set(within.remaining());
        Ok(self.bearer.clone())
    }
}

/// A source that spends the whole call before it answers.
///
/// **The only way to reach the spent-budget refusal without sleeping through a real deadline** on the
/// path a caller takes, because the budget is opened INSIDE `submit` and nothing outside can inject
/// one. It sleeps rather than lying about the clock, which is what makes it a test of the wiring rather
/// than of the arithmetic - `CallDeadline`'s own test covers that separately.
struct Slow(Bearer);

impl AccessTokens for Slow {
    type Error = CannotFail;

    fn quota_project(&self) -> QuotaProject {
        QuotaProject::Required
    }

    fn bearer(&self, _now_unix_seconds: u64, _within: CallDeadline) -> Result<Bearer, Self::Error> {
        std::thread::sleep(Duration::from_millis(1_200));
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

    fn quota_project(&self) -> QuotaProject {
        QuotaProject::Required
    }

    fn bearer(&self, _now_unix_seconds: u64, _within: CallDeadline) -> Result<Bearer, Self::Error> {
        Err(NoCredential)
    }
}

/// The bounds every request in this suite is built with.
///
/// **30 seconds because that is `server.request_timeout_seconds`' shipped default**, which is the
/// number a composition root would fill this from - written here so the test says where it comes from
/// rather than picking a round one.
fn bounds() -> JobBounds {
    JobBounds::of(
        QueryDeadline::parse(30).expect("30 seconds is a deadline"),
        BytesBilledCeiling::parse(64 * 1024 * 1024).expect("64 mebibytes is a ceiling"),
    )
}

fn pinned() -> WireAgent {
    WireAgent::pinned(bounds())
}

/// The whole budget, for a request built with none of it spent yet.
///
/// **A fixture rather than a real remaining time**, because `body` now takes what is LEFT of the call's
/// budget and a real one would make the two timeout fields a different number on every run. What this
/// suite asserts is the document a given window produces; that the window IS the remaining budget is
/// `submit`'s job and has tests of its own.
fn window() -> Duration {
    bounds().deadline().budget()
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
    let request = JobRequest::new(
        "SELECT `x` FROM `t` WHERE `s` = ? AND `d` <= ?",
        &params,
        &project,
        &dataset,
        None,
    );
    let sent = serde_json::to_value(body(&request, DryRun::No, bounds(), window())).expect("the body serializes");

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
    let request = JobRequest::new("SELECT 1 FROM `t` WHERE `s` = ?", &params, &project, &dataset, None);
    let sent = serde_json::to_value(body(&request, DryRun::No, bounds(), window())).expect("the body serializes");

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
    let request = JobRequest::new("SELECT 1 FROM `t` WHERE `d` = ?", &params, &project, &dataset, None);
    let sent = serde_json::to_value(body(&request, DryRun::No, bounds(), window())).expect("the body serializes");

    assert_eq!(sent["queryParameters"][0]["parameterType"]["type"], "DATE");
    assert_eq!(sent["queryParameters"][0]["parameterValue"]["value"], "2026-08-30");

    let text = [ParamValue::Text(String::from("500"))];
    let other = JobRequest::new("SELECT 1 FROM `t` WHERE `s` = ?", &text, &project, &dataset, None);
    let sent = serde_json::to_value(body(&other, DryRun::No, bounds(), window())).expect("the body serializes");
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
    let request = JobRequest::new("SELECT 1 FROM `t`", &[], &project, &dataset, None);
    let sent = serde_json::to_value(body(&request, DryRun::No, bounds(), window())).expect("the body serializes");

    assert_eq!(sent["useLegacySql"], false);
    assert_eq!(sent["useQueryCache"], false);
}

#[test]
fn a_dry_run_and_a_real_run_differ_by_that_one_field() {
    // What makes `dry_run` answering `PreFlight::Accepted` honest is that the two submissions are the
    // same request. If anything else differed, a validated statement and an executed one would not be
    // the same statement.
    let params = [ParamValue::Text(String::from("active"))];
    let project = project();
    let dataset = dataset();
    let request = JobRequest::new("SELECT 1 FROM `t` WHERE `s` = ?", &params, &project, &dataset, None);

    let mut validated = serde_json::to_value(body(&request, DryRun::Yes, bounds(), window())).expect("the body serializes");
    let executed = serde_json::to_value(body(&request, DryRun::No, bounds(), window())).expect("the body serializes");

    assert_eq!(validated["dryRun"], true);
    assert_eq!(executed["dryRun"], false);
    validated["dryRun"] = serde_json::Value::Bool(false);
    assert_eq!(validated, executed, "a dry run differs from a real one by more than the flag");
}

#[test]
fn the_url_names_the_billing_project_against_a_host_no_deployment_chooses() {
    let project = project();
    let dataset = dataset();
    let request = JobRequest::new("SELECT 1 FROM `t`", &[], &project, &dataset, None);

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
    assert!(matches!(refused, Err(WireError::NotComplete { .. })), "{refused:?}");
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
fn a_page_of_a_larger_result_is_a_size_bound_and_every_other_failure_is_not() {
    // The predicate the transport port asks, and the reason the refusal above stops reaching a caller
    // as `503`. A page token is the endpoint saying *there is more of this than fits one reply*, which
    // is a governance outcome above the domain port - `ResultTooLarge` carrying `ResultBound::Volume`
    // - and a retry returns the same page.
    // Any wire will do: this predicate reads the error and nothing else, which is what makes it
    // answerable with no socket in reach.
    let wire = BigQueryWire::new(
        pinned(),
        Fixed::holding(Bearer::of(Secret::new("t"), Expiry::At { unix_seconds: 1 })),
    );
    assert!(
        wire.result_did_not_fit(&WireError::<CannotFail>::MoreThanOnePage),
        "a page of a larger result is a size bound"
    );
    // The control, and it is the half that matters more: every other failure has to stay a failure,
    // because telling a caller not to retry a data system that is briefly unwell is the mistake the
    // port's own documentation says costs more. `NotComplete` is the sharpest of them - a job that ran
    // out of time may well finish on a retry.
    for failure in [
        WireError::<CannotFail>::NotComplete { named: String::new() },
        WireError::NoTotal { named: String::new() },
        WireError::NoSchema { rows: 1 },
        WireError::NotAScalar { row: 0, column: 0 },
        WireError::Refused {
            status: 400,
            named: String::new(),
            detail: String::new(),
        },
    ] {
        assert!(
            !wire.result_did_not_fit(&failure),
            "{failure:?} was reported as a result too large"
        );
    }
}

#[test]
fn a_complete_job_that_states_no_total_is_refused_rather_than_read_as_empty() {
    // Reading an absent total as zero would make a delivered-nothing answer and a stated-nothing
    // answer the same value, and only one of those is a result this adapter may certify.
    let refused = complete::<CannotFail>(answer(r#"{"jobComplete": true}"#));
    assert!(matches!(refused, Err(WireError::NoTotal { .. })), "{refused:?}");
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
fn a_complete_result_that_carries_a_warning_is_answered_rather_than_refused() {
    // **The reversal, and it is a correctness fix rather than a preference.** The endpoint documents
    // `errors` as "the first errors or warnings encountered" and says entries "do not necessarily
    // mean that the job has completed or was unsuccessful" - so the previous version of this module,
    // which refused on a non-empty array, declined successful queries that merely warned and answered
    // a caller `503` for a result the service had produced.
    let document = r#"{"jobComplete": true, "totalRows": "1", "errors": [{"reason": "warning"}],
        "schema": {"fields": [{"name": "n", "type": "INTEGER"}]},
        "rows": [{"f": [{"v": "1"}]}]}"#;
    let rows = complete::<CannotFail>(answer(document)).expect("a complete result is answered");
    assert_eq!(rows.total_rows(), 1);
    assert_eq!(rows.rows().len(), 1);
}

#[test]
fn a_failed_job_is_caught_by_its_shape_and_carries_the_reason_the_endpoint_gave() {
    // What replaced the `errors` check: the SHAPE decides, and the reported reason is folded into
    // whichever shape check fires - which is where a failed job actually lands, because the endpoint
    // reports one as complete with no total.
    let document = r#"{"jobComplete": true, "errors": [{"reason": "resourcesExceeded"}]}"#;
    match complete::<CannotFail>(answer(document)) {
        Err(WireError::NoTotal { ref named }) => assert_eq!(*named, "resourcesExceeded"),
        other => panic!("a failed job was mapped to {other:?}"),
    }

    // And an incomplete one, which is the other shape a reason attaches to.
    let document = r#"{"jobComplete": false, "errors": [{"reason": "timeout"}]}"#;
    match complete::<CannotFail>(answer(document)) {
        Err(WireError::NotComplete { ref named }) => assert_eq!(*named, "timeout"),
        other => panic!("an incomplete job was mapped to {other:?}"),
    }
}

#[test]
fn a_reported_reason_is_a_diagnostic_and_is_bounded_like_every_other_foreign_string() {
    // It reaches an error and therefore a log, so it goes through the same one function every other
    // foreign short token in this crate goes through.
    let document = r#"{"jobComplete": true, "errors": [{"reason": "bad\nreason [31m"}]}"#;
    assert_eq!(reported(&answer(document)), "badreason31m");
    assert_eq!(reported(&answer(r#"{"jobComplete": true}"#)), "");
}

// ------------------------------------------------------------ the refusal it maps ----

#[test]
fn a_refusal_keeps_the_endpoints_reason_and_a_bounded_message() {
    // **This test was `..._and_never_its_message`, and the reversal is the point.** The message was
    // deliberately not a field, on the argument that what is not read cannot be logged by accident -
    // and then the first live submission came back `400 invalidQuery` and there was no way to tell
    // WHICH construct the service disliked. A status and a reason code that together say *your SQL is
    // wrong* are not actionable, and `docs/adr/0017` had already measured that this dialect's risk is
    // exactly the construct a parse check cannot see.
    //
    // So the message is carried and BOUNDED. What the bound has to make impossible is a service's
    // answer forging a log line, which is why the fixture below carries a newline, a carriage return
    // and an escape sequence inside the message: the sentence survives and those do not.
    let document = "{\"error\": {\"code\": 403, \"status\": \"PERMISSION_DENIED\", \"message\": \
        \"Access Denied:\\n\\u001b[31mTable\\r ledger: User does not have permission\", \
        \"errors\": [{\"reason\": \"accessDenied\", \"domain\": \"global\"}]}}";
    let mapped: WireError<CannotFail> = refusal(403, document);
    match mapped {
        WireError::Refused {
            status,
            ref named,
            ref detail,
        } => {
            assert_eq!(status, 403);
            assert_eq!(*named, "accessDenied");
            assert!(detail.contains("Access Denied"), "the detail lost the sentence: {detail}");
            assert!(
                detail.contains("does not have permission"),
                "the detail was truncated: {detail}"
            );
            assert!(!detail.contains('\n'), "the detail carried a newline: {detail:?}");
            assert!(!detail.contains('\r'), "the detail carried a carriage return: {detail:?}");
            assert!(
                !detail.contains('\u{1b}'),
                "the detail carried an escape sequence: {detail:?}"
            );
        }
        ref other => panic!("a refusal was mapped to {other:?}"),
    }
}

#[test]
fn a_message_from_the_endpoint_cannot_be_longer_than_a_log_line() {
    // The other half of the bound. A service that answered with a megabyte of prose would otherwise
    // put a megabyte into a log, which is the concern that got the field dropped in the first place -
    // answered here by a cap rather than by an absence.
    let long = "z".repeat(4096);
    let document = format!("{{\"error\": {{\"message\": \"{long}\"}}}}");
    let mapped: WireError<CannotFail> = refusal(500, &document);
    match mapped {
        WireError::Refused { ref detail, .. } => assert_eq!(detail.len(), 400),
        ref other => panic!("a long message was mapped to {other:?}"),
    }
}

#[test]
fn a_refusal_whose_body_is_not_the_envelope_still_reports_the_status() {
    // The status is the guaranteed half. A gateway, a proxy or an outage answers with something that
    // is not this document, and a mapping that failed there would turn a refusal into a parse error.
    for document in ["", "<html>502 Bad Gateway</html>", "{}", r#"{"error": {}}"#] {
        let mapped: WireError<CannotFail> = refusal(502, document);
        match mapped {
            WireError::Refused {
                status,
                ref named,
                ref detail,
            } => {
                assert_eq!(status, 502);
                assert!(named.is_empty(), "{document:?} produced a reason: {named}");
                assert!(detail.is_empty(), "{document:?} produced a detail: {detail}");
            }
            ref other => panic!("{document:?} was mapped to {other:?}"),
        }
    }
}

#[test]
fn a_short_token_another_service_sent_is_bounded_and_filtered() {
    // Foreign text heading for a log. Bounded by characters rather than bytes, because a byte slice of
    // somebody else's UTF-8 can land inside a character - which is also why `clippy::string_slice` is
    // denied here.
    assert_eq!(bounded(Some(String::from("accessDenied"))), "accessDenied");
    assert_eq!(bounded(None), "");
    assert_eq!(
        bounded(Some(String::from("bad\nreason\r\"quoted\" \u{1b}"))),
        "badreasonquoted"
    );
    let long = bounded(Some("a".repeat(4096)));
    assert_eq!(long.len(), 64);
}

// ----------------------------------------------------- the credential, before a send ----

#[test]
fn an_expired_credential_is_refused_before_anything_is_sent() {
    // The one guard in `submit` a test can reach with no socket, because it runs BEFORE the request is
    // built. A source handing back an expired token has a clock problem, and presenting it anyway
    // would turn that into a 401 an operator reads as a permissions fault.
    let wire = BigQueryWire::new(
        pinned(),
        Fixed::holding(Bearer::of(Secret::new("t"), Expiry::At { unix_seconds: 1 })),
    );
    let project = project();
    let dataset = dataset();
    let request = JobRequest::new("SELECT 1 FROM `t`", &[], &project, &dataset, None);

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
    let wire = BigQueryWire::new(pinned(), Missing);
    let project = project();
    let dataset = dataset();
    let request = JobRequest::new("SELECT 1 FROM `t`", &[], &project, &dataset, None);

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
fn a_job_carries_both_bounds_and_the_two_timeout_fields_agree() {
    // **The bound that was missing entirely, and the one that bounded nothing.** `maximumBytesBilled`
    // is what stops a question scanning a partitioned table end to end - neither `LIMIT 10001` nor the
    // one-page refusal nor the response cap does. And `timeoutMs` alone does NOT cancel a job: an
    // expired one returns `jobComplete: false` and the job keeps running and billing, which is why
    // `jobTimeoutMs` is here and why the two are the same number.
    let project = project();
    let dataset = dataset();
    let request = JobRequest::new("SELECT 1 FROM `t`", &[], &project, &dataset, None);
    let sent = serde_json::to_value(body(&request, DryRun::No, bounds(), window())).expect("the body serializes");

    assert_eq!(sent["jobTimeoutMs"], 30_000);
    assert_eq!(sent["timeoutMs"], 30_000);
    assert_eq!(
        sent["timeoutMs"], sent["jobTimeoutMs"],
        "the client would stop waiting at a different instant than the service cancels: {sent}"
    );
    // Text, because the endpoint reads 64-bit integers as JSON strings - a number here is silently a
    // double at the far end.
    assert_eq!(sent["maximumBytesBilled"], "67108864");
    assert!(sent["maximumBytesBilled"].is_string(), "{sent}");
}

#[test]
fn a_bound_that_would_bound_nothing_is_refused_at_construction() {
    // A newtype PARSES: if an instance exists the bound is usable. A zero deadline would refuse every
    // question rather than bounding one, and a zero ceiling the same.
    QueryDeadline::parse(0).expect_err("a zero deadline is refused");
    BytesBilledCeiling::parse(0).expect_err("a zero ceiling is refused");
    // And a bound so large it is indistinguishable from no bound.
    QueryDeadline::parse(60 * 60 * 24).expect_err("a day is above the endpoint's own ceiling");
    BytesBilledCeiling::parse(u64::MAX).expect_err("a ceiling nothing could exceed is not a ceiling");
    // The socket outlives the job by connection setup and no more, which is what keeps a pool thread
    // from outliving the request it was answering.
    let deadline = QueryDeadline::parse(30).expect("30 seconds is a deadline");
    assert_eq!(deadline.milliseconds(), 30_000);
    assert_eq!(deadline.socket().as_secs(), 35);
}

#[test]
fn the_pinned_client_is_the_only_kind_either_half_of_this_module_accepts() {
    // **The newtype IS the mechanism, and this test is what says so.** The four settings used to live
    // in a free function returning a bare `ureq::Agent`, and both the transport and the credential
    // source took any agent - so a composition root writing `ureq::Agent::new_with_defaults()` got
    // redirects on, plaintext allowed and no timeout, while every test passed because the tests all
    // called the right builder.
    //
    // What holds it now is the type: `WireAgent` has a private field and `pinned` is its only
    // constructor, so there is no `ureq::Agent` a caller could substitute. That is not assertable at
    // run time - it is a compile error, and the `compile_fail` doctest on
    // `credential::ApplicationDefault::read`, with its compiling twin, is where that is pinned. What
    // IS assertable here is that the bounds travel with the agent, so the
    // socket timeout and the request body cannot disagree.
    let agent = pinned();
    assert_eq!(agent.bounds(), bounds());
    assert_eq!(agent.bounds().deadline().socket().as_secs(), 35);
    // And a clone carries the pins, which is what lets the credential source share one pool.
    let shared = agent.clone();
    assert_eq!(shared.bounds(), agent.bounds());
}

#[test]
fn a_bearer_never_shows_its_token() {
    // `Secret` is the mechanism and this is the assertion that it survives being wrapped: a `Bearer`
    // is what the transport holds, and a derived `Debug` on it would print the token.
    let bearer = Bearer::of(Secret::new("ya29-do-not-log-me"), Expiry::NothingExpires);
    assert!(!format!("{bearer:?}").contains("ya29"), "{bearer:?}");
    assert_eq!(bearer.token().expose_secret(), "ya29-do-not-log-me");
}

// -------------------------------------------------------------- one deadline per call ----

#[test]
fn one_call_has_one_deadline_and_the_credential_exchange_spends_part_of_it() {
    // **The shape review measured as wrong.** `timeout_global` used to live on the agent alone, so
    // every request through it got the whole budget INDEPENDENTLY: an exchange and then a job, each
    // allowed `deadline + CONNECT_MARGIN` of its own, twice per answer because `answer` calls
    // `dry_run` and then `execute`. The claim in this module's header - a five-second overrun - was
    // false against a thirty-second request timeout.
    //
    // What is asserted here is that the credential port is HANDED the call's budget, which is the
    // wiring the fix needed: the fake records what `remaining()` said when it was asked.
    let source = Fixed::holding(Bearer::of(Secret::new("t"), Expiry::At { unix_seconds: 1 }));
    let wire = BigQueryWire::new(pinned(), source);
    let project = project();
    let dataset = dataset();
    let request = JobRequest::new("SELECT 1 FROM `t`", &[], &project, &dataset, None);

    // It refuses on the expired token, which is fine: what matters is that the exchange was asked
    // first, and with a budget.
    wire.run(&request)
        .expect_err("the fixture's token is expired, so the call refuses after the exchange");
    let handed = wire.credentials.handed.get().expect("the exchange was handed a budget");
    assert!(
        handed <= bounds().deadline().budget(),
        "the exchange was handed more than the call's whole budget: {handed:?}"
    );
    assert!(!handed.is_zero(), "the exchange was handed nothing to work with");
}

#[test]
fn a_call_whose_budget_the_exchange_spent_refuses_rather_than_submitting_a_job() {
    // The consequence of one budget rather than two: a slow exchange does not get to be followed by a
    // job with a full budget of its own. Submitting anyway would mean either an unbounded wait or a
    // job the service keeps running after the client has stopped waiting - the pair this shape rules
    // out. Reached with a real sleep against a one-second budget, because the budget is opened inside
    // `submit` and nothing outside can inject a spent one.
    let bounds = JobBounds::of(
        QueryDeadline::parse(1).expect("one second is a deadline"),
        BytesBilledCeiling::parse(1024).expect("a kibibyte is a ceiling"),
    );
    let wire = BigQueryWire::new(
        WireAgent::pinned(bounds),
        Slow(Bearer::of(Secret::new("t"), Expiry::NothingExpires)),
    );
    let project = project();
    let dataset = dataset();
    let request = JobRequest::new("SELECT 1 FROM `t`", &[], &project, &dataset, None);

    // No socket is opened, which is the point: `HOST` is unreachable from a test, so any other error
    // here would mean the request had been sent.
    match wire.run(&request) {
        Err(WireError::DeadlineSpent { budget_seconds }) => assert_eq!(budget_seconds, 1),
        other => panic!("a spent budget was mapped to {other:?}"),
    }
}

#[test]
fn what_is_left_of_the_budget_is_what_the_request_asks_the_service_to_hold_the_job_for() {
    // The other half of one-budget-per-call: the two timeout fields read the REMAINING window rather
    // than the whole deadline, so the service cancels at the instant the client stops waiting even
    // when the exchange spent part of it first. Asserted at two windows, because a single one cannot
    // tell "it reads the window" from "it reads the constant".
    let project = project();
    let dataset = dataset();
    let request = JobRequest::new("SELECT 1 FROM `t`", &[], &project, &dataset, None);

    let whole = serde_json::to_value(body(&request, DryRun::No, bounds(), Duration::from_secs(30))).expect("the body serializes");
    assert_eq!(whole["timeoutMs"], 30_000);
    assert_eq!(whole["jobTimeoutMs"], 30_000);

    let shortened =
        serde_json::to_value(body(&request, DryRun::No, bounds(), Duration::from_millis(21_500))).expect("the body serializes");
    assert_eq!(shortened["timeoutMs"], 21_500);
    assert_eq!(
        shortened["timeoutMs"], shortened["jobTimeoutMs"],
        "the client would stop waiting at a different instant than the service cancels: {shortened}"
    );
    // The money bound is NOT a function of the window, which is the pair of decisions `JobBounds`
    // carries: time is what is left, money is the whole ceiling for this job.
    assert_eq!(shortened["maximumBytesBilled"], whole["maximumBytesBilled"]);
}

#[test]
fn a_budget_is_spent_by_elapsed_time_and_a_spent_one_is_no_timeout_rather_than_zero() {
    // The arithmetic, pinned purely - which is why `opened_at` is public. Zero would mean *no timeout*
    // to the client underneath, so a spent budget has to be an absence rather than a duration.
    let deadline = QueryDeadline::parse(30).expect("30 seconds is a deadline");
    let now = std::time::Instant::now();

    let fresh = CallDeadline::opened_at(now, deadline);
    let left = fresh.remaining().expect("a budget opened now has time left");
    assert!(left <= Duration::from_secs(30) && left > Duration::from_secs(29), "{left:?}");

    let spent = CallDeadline::opened_at(
        now.checked_sub(Duration::from_secs(60)).expect("an instant a minute ago"),
        deadline,
    );
    assert_eq!(spent.remaining(), None, "a budget opened a minute ago is not still running");

    // And the socket gets connection setup on top of whatever is left, never on top of the whole
    // deadline - which is what stops a pool thread outliving the caller it was answering.
    assert_eq!(CallDeadline::socket(Duration::from_secs(10)), Duration::from_secs(15));
}

#[test]
fn the_deadline_a_composition_root_gets_already_accounts_for_the_calls_one_answer_makes() {
    // **The arithmetic that was left to whoever wired this, and would have been got wrong.** One
    // answer calls the port `CALLS_PER_ANSWER` times and each call pays `CONNECT_MARGIN` on top of its
    // own budget, so the number a root wants is not `server.request_timeout_seconds` - it is that
    // number's share. Thirty seconds shipped, two calls, five seconds of setup each: ten.
    let from_the_shipped_default = QueryDeadline::within_request_timeout(30).expect("30 seconds leaves a budget");
    assert_eq!(
        from_the_shipped_default,
        QueryDeadline::parse(10).expect("ten seconds is a deadline")
    );
    // Which is the arithmetic holding: two calls of ten plus five is the thirty a caller was promised.
    assert_eq!(
        from_the_shipped_default.socket().as_secs() * QueryDeadline::CALLS_PER_ANSWER,
        30
    );

    // A request timeout too short to leave anything is named rather than clamped, because a deployment
    // whose timeout cannot fit a query wants to hear so at startup.
    assert_eq!(
        QueryDeadline::within_request_timeout(10),
        Err(UnusableBound::NoBudget { given: 10, calls: 2 })
    );
    assert!(matches!(
        QueryDeadline::within_request_timeout(0),
        Err(UnusableBound::NoBudget { .. })
    ));
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
