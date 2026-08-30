//! A refused question, on the wire: the status it comes back as, the code a client branches on, and
//! the sentence a person reads.
//!
//! # A refusal is not a `200`
//!
//! It was, and the argument for that is worth restating before it is taken apart, because it was not
//! a silly one: `ToolOutcome::Refusal` is a domain *result* rather than an `Err`, and a `4xx` was
//! said to invite a client library to retry - retrying a governance decision until it succeeds being
//! exactly the behaviour a refusal exists to prevent. The invariant is real and is untouched here.
//! The status conclusion drawn from it was wrong twice over.
//!
//! **The retry premise does not hold.** Nothing mainstream retries a `4xx` by default; the statuses
//! retried by convention are `429` and `408`, and no refusal maps to either. Checked against the
//! current documentation rather than asserted:
//!
//! * `urllib3.util.Retry` - what `requests` mounts through its `HTTPAdapter` - drives status-based
//!   retries from `status_forcelist`, "a set of integer HTTP status codes that we should force a
//!   retry on", and documents its default as "By default, this is disabled with `None`." So no
//!   status is retried until somebody names one.
//! * `reqwest` 0.13's `retry` module documents its default policy as "to only retry requests where
//!   an error or low-level protocol NACK is encountered that is known to be safe to retry" - a
//!   transport condition, not a response status.
//! * `axios` retries nothing on its own. `axios-retry`, the plugin that adds it, defaults
//!   `retryCondition` to `isNetworkOrIdempotentRequestError`, documented as: "By default, it retries
//!   if it is a network error or a 5xx error on an idempotent request (GET, HEAD, OPTIONS, PUT or
//!   DELETE)."
//!
//! Go's `net/http` reference documents no status-driven retry anywhere in `Client`, `Transport` or
//! `RoundTripper`. And `422`, which four refusals below map to, is documented the other way round
//! from the premise: "Clients that receive a `422` response should expect that repeating the request
//! without modification will fail with the same error."
//!
//! **And the `200` cost something the argument never priced.** A governance refusal that comes back
//! `200` is indistinguishable from an answer to everything that reads a status and not a body: an
//! ingress log, a dashboard, an error-rate alert, a client's `raise_for_status()`, a generated
//! client whose success branch is `2xx`. A deployment refusing every question looks perfectly
//! healthy. The refusal was legible only to code written against this specific envelope, which is
//! the one reader that did not need convincing.
//!
//! So a refusal now carries all three: a status, the machine-readable `code` it always had, and the
//! sentence. The body is unchanged apart from the status being repeated inside it.
//!
//! # The statuses, and why each one
//!
//! One exhaustive match, no wildcard arm. A refusal variant added to the domain fails to compile
//! here until somebody decides what it is on the wire - the same mechanism that already stops a new
//! governance outcome from reaching a caller as an unnamed one.
//!
//! Three statuses are shared by more than one variant, and that is deliberate rather than a
//! shortage: the status is what a monitor counts and the `code` is what a client branches on, so the
//! grouping is by *what the caller should do*, not one number per variant. [`crate::problem`]
//! already takes the same position where `unavailable` and `at_capacity` share `503`.

use axum::http::StatusCode;
use sutura_domain::query::{RefusalReason, ResultBound};

use super::RefusalBody;

/// The status, the code and the sentence for one refusal.
///
/// **The match is exhaustive with no wildcard arm, deliberately**, and it decides all three at once
/// rather than in three matches that could drift apart. A refusal variant added to the domain fails
/// to compile here until it is given a status, a code and a sentence.
#[expect(
    clippy::too_many_lines,
    reason = "one exhaustive match over every refusal, deciding status, code and detail together - so \
              it grows by one arm per domain variant and splitting it would need a wildcard arm, which \
              is exactly the gap the exhaustiveness exists to close"
)]
pub(crate) fn refused(reason: &RefusalReason) -> (StatusCode, RefusalBody) {
    let (status, code, detail) = match *reason {
        // 404. The name does not resolve in this snapshot, which is the plainest thing a status can
        // say. `definition_version` on an answer is what makes "in this snapshot" the honest
        // qualifier: the same name against a later bundle is a different question.
        RefusalReason::MetricUnknown { ref metric } => (
            StatusCode::NOT_FOUND,
            "metric_unknown",
            format!("this catalog defines no metric called `{metric}`"),
        ),
        // 422. The metric exists, the request is well formed, and the grain asked for is one nobody
        // rendered - so the content is understood and cannot be processed, which is what 422 is for.
        // Not 404: the metric IS there, and a caller told the name was not found would go looking
        // for the wrong mistake.
        RefusalReason::GrainNotSupported { ref metric, grain } => (
            StatusCode::UNPROCESSABLE_ENTITY,
            "grain_not_supported",
            format!("`{metric}` is not defined at `{grain}` grain"),
        ),
        // 403 for all three dimension refusals. Each is the catalog's answer to "may this be asked
        // of this metric" - the declared dimension set, the filterable flag, the value allowlist -
        // so 403's "understood the request but refuses to fulfill it" is the right sense, and
        // grouping them is what lets a monitor count attempts to ask outside the catalog as one
        // number.
        //
        // **The 403 is NOT a statement about the caller's credential**, and this surface has no
        // per-caller identity to make one with - see the crate documentation. No token widens a
        // metric's dimension set, which is why each sentence below names the dimension and the
        // metric: a caller must not read this as "go and get a better token".
        //
        // `DimensionNotPermitted` is the one of the three that could be argued to 422 instead - the
        // domain calls it "a name that does not resolve" - and it stays here because what it is
        // checked against is the metric's DECLARED dimension set, which is the same catalog
        // statement the other two read at finer grain. `docs/adr/0005` records the argument.
        RefusalReason::DimensionNotPermitted {
            ref metric,
            ref dimension,
        } => (
            StatusCode::FORBIDDEN,
            "dimension_not_permitted",
            format!("`{metric}` does not declare a dimension called `{dimension}`"),
        ),
        RefusalReason::DimensionNotFilterable {
            ref metric,
            ref dimension,
        } => (
            StatusCode::FORBIDDEN,
            "dimension_not_filterable",
            format!("`{dimension}` can be grouped by on `{metric}` but not filtered on"),
        ),
        RefusalReason::DimensionValueNotAllowed {
            ref metric,
            ref dimension,
        } => (
            StatusCode::FORBIDDEN,
            "dimension_value_not_allowed",
            // The value is deliberately absent. See [`RefusalBody`].
            format!("that value is not one `{metric}` declares for `{dimension}`"),
        ),
        // 422. Well formed, and not a question: the caller believes something about the second
        // occurrence that we do not, which is why the domain refuses rather than deduplicating.
        RefusalReason::DuplicateDimension { ref dimension } => (
            StatusCode::UNPROCESSABLE_ENTITY,
            "duplicate_dimension",
            format!("`{dimension}` appears more than once"),
        ),
        // 422, and the caller can act on it without a second request to discover the number: the
        // sentence carries what they asked for and the bound.
        RefusalReason::TooManyDimensions { requested, limit } => (
            StatusCode::UNPROCESSABLE_ENTITY,
            "too_many_dimensions",
            format!("{requested} group-by keys were asked for and the maximum is {limit}"),
        ),
        // 422. The range parsed and both endpoints are real dates, so this is not a `400`; it is the
        // availability boundary, and the answer to a well formed question is no.
        RefusalReason::TimeRangeTooLong { days, limit } => (
            StatusCode::UNPROCESSABLE_ENTITY,
            "time_range_too_long",
            format!("the period spans {days} days and the maximum is {limit}"),
        ),
        // 413, and this is the one status here that is arguably wrong by the letter of the spec: 413
        // is defined over the *request* content - "the request entity was larger than limits defined
        // by server" - and what is too large here is the answer. It is used anyway, and the
        // objection is recorded rather than hidden: 413 is the status a person reading a dashboard
        // reads as "too large", which is exactly what this refusal has to be unmistakable about.
        // `code` is what disambiguates it from the body-limit `413` on this same route, and the two
        // bodies differ in shape as well - this one carries `outcome`. `docs/adr/0005` has the rest.
        //
        // No row count in the sentence, because there is none to give: the plan asks for one row
        // past the cap and stops, so what is known is "more than this". What the sentence carries
        // instead is that nothing was truncated to fit - a partial total under a certified name is
        // the failure this refusal exists to prevent - the cap itself, and the two things a caller
        // can narrow.
        //
        // **One status and one code for both bounds.** They are what a client branches on, and *too
        // much data* is one thing to branch on: the remedy is the same narrowing whichever side
        // measured it, and a second code would make an operator configure a dashboard for two
        // answers to one question. What differs is what can honestly be SAID, and that is
        // [`too_much_data`]'s own exhaustive match.
        //
        // **Never `503`.** The volume bound is the defect this arm was widened for: a result over
        // the data system's reply bound used to arrive as `ServiceError::Warehouse` and leave as
        // `503`, which is what a dead data system looks like - so a caller was told to retry against
        // a bound that returns the same reply.
        RefusalReason::ResultTooLarge { bound } => (StatusCode::PAYLOAD_TOO_LARGE, "result_too_large", too_much_data(bound)),
        // 422, and choosing it is the whole point of this variant existing. Exhaustion used to reach
        // a caller as `503 unavailable` out of `ServiceError::Warehouse` - the same status a data
        // system that is down produces - so a caller was told to retry against a configured bound
        // that fires again in exactly the same place. 422 is the status whose own definition says
        // otherwise: "Clients that receive a 422 response should expect that repeating the request
        // without modification will fail with the same error", which is precisely true here.
        //
        // Not 503: the service is well, and the number that refused is one an operator wrote down.
        // Not 413, which this route already uses for a body over the limit and for `result_too_large`
        // - and what was too large here is neither the request nor the answer, it is the memory the
        // engine would have had to reserve on the way to one. Not 507 `Insufficient Storage`, which
        // is a 5xx and reads as the server's fault to every client that branches on the class.
        //
        // The sentence names the ceiling because that is a configured number and safe to hand back,
        // and names nothing about what the question demanded - see the domain variant for why.
        RefusalReason::ResourcesExhausted { ceiling_bytes } => (
            StatusCode::UNPROCESSABLE_ENTITY,
            "resources_exhausted",
            format!(
                "answering this needed more working memory than this deployment's ceiling of \
                 {ceiling_bytes} bytes, and it was refused rather than allowed to exhaust the \
                 process; narrow the period, group by fewer dimensions or add a filter and ask \
                 again. Retrying it unchanged returns this same refusal"
            ),
        ),
        // 409. The question is answerable in principle and this deployment will not answer it: a
        // second data system is a second identity to satisfy, and a plan that runs partly as
        // somebody else is the failure the whole design is arranged against. That is a conflict
        // between what was asked and how this deployment is arranged, which is what 409 says and
        // what no status about the request's own content would.
        RefusalReason::PlanSpansTwoSources { sources } => (
            StatusCode::CONFLICT,
            "plan_spans_two_sources",
            format!("answering this would read from {sources} data systems, and a plan runs against one"),
        ),
        // 409, and the same reasoning `PlanSpansTwoSources` above carries: the question is well formed,
        // the metric permits it, and this deployment cannot express it as one statement. That is a
        // conflict between what was asked and how the tables it reads are named, which is what 409 says
        // and what no status about the request's own content would.
        //
        // The sentence names the identifier and neither of the two paths. The identifier is the thing a
        // person can act on; a path carries the project and dataset a deployment reads, which is not
        // the asker's business. It says what else to try, because unlike the two-source refusal there
        // usually IS another question: a dimension that needs no join is still answered.
        RefusalReason::PlanTablesShareAnIdentifier { ref table } => (
            StatusCode::CONFLICT,
            "plan_tables_share_an_identifier",
            format!(
                "answering this would read two different tables that are both called `{table}`, and one \
                 statement cannot tell them apart; ask for a dimension that does not need that join, or \
                 report it to a person"
            ),
        ),
        // 503, and the only refusal where retrying is a reasonable thing for a caller to do. It is
        // the variant an identity failure will use, and today it is raised by a name comparison -
        // the plan's data system against the adapter this process opened - so today's cause is a
        // deployment wired wrong rather than one that is briefly unwell. The status is still the
        // honest one for the variant's meaning, and the sentence is what an operator reads.
        //
        // **No `Retry-After`.** Nothing here knows when a data system comes back, and
        // `Failure::retry_after` already sets the rule for this surface: a number that is already
        // known, or no header, because a guess is a promise. `Failure::Unavailable` - the same
        // situation reached from the failure side - carries none for the same reason, and a refusal
        // that invented one would make the two disagree.
        RefusalReason::SourceUnavailable { ref source } => (
            StatusCode::SERVICE_UNAVAILABLE,
            "source_unavailable",
            format!("`{source}` could not be reached as the calling subject"),
        ),
        // 403, and this is the refusal that AMENDED `docs/adr/0005`'s note - "the 403s are not a
        // statement about a credential" - stop being true. It is one, so the sentence must not send
        // the caller looking for a better token FOR THIS SERVICE: what is missing is a grant at the
        // data system, and presenting a different bearer here changes nothing. Retrying is pointless
        // and the sentence says so, because the alternative reading of a 403 is "authenticate
        // harder".
        //
        // It names the source and nothing about which grant. Which permission a subject lacks is the
        // data system's to say; guessing it here would be this deployment holding a second opinion
        // about somebody else's authorization, and it would tell a caller something about a data
        // system they were just refused access to.
        RefusalReason::CredentialUnavailable { ref source } => (
            StatusCode::FORBIDDEN,
            "credential_unavailable",
            format!(
                "you have no credential at the data system `{source}`, so this deployment will not \
                 read it on your behalf - and it will not read it as itself instead. This is not \
                 about the token you presented to this service: the missing grant is at that data \
                 system. Asking the same question again returns this same refusal"
            ),
        ),
    };
    (
        status,
        RefusalBody {
            code,
            status: status.as_u16(),
            detail,
        },
    )
}

/// The sentence for a result that was too much data, per bound.
///
/// **A function of its own because the two bounds share a status and a code and cannot share a
/// sentence.** One of them has a number an operator configured and the other has no number at all, so
/// a single string would either invent a cap for the volume case or drop the cap from the row case.
///
/// Exhaustive with no wildcard arm: a third bound has to be answered here rather than inheriting the
/// row cap's wording, which would be a certified-looking figure for a bound nobody measured.
fn too_much_data(bound: ResultBound) -> String {
    match bound {
        ResultBound::Rows { limit } => format!(
            "the answer exceeded this service's cap of {limit} rows and was NOT truncated to fit; \
             narrow the period or group by fewer dimensions and ask again"
        ),
        // No figure, because there is none this deployment was told - see `ResultBound::Volume`,
        // which says at length why inventing one would be worse than leaving it out. So the sentence
        // says which side the bound belongs to, that nothing was truncated to fit, and that a retry
        // is not the remedy.
        ResultBound::Volume => String::from(
            "the answer was more data than the data system would return at once and was NOT \
             truncated to fit; the bound is the data system's own and this service is not told what \
             it is, so narrow the period or group by fewer dimensions and ask again. Retrying it \
             unchanged returns this same refusal",
        ),
    }
}

#[cfg(test)]
mod tests {
    use axum::http::StatusCode;
    use sutura_domain::model::{DimensionName, Grain, MetricName, TableName};
    use sutura_domain::query::{RefusalReason, ResultBound};

    use super::refused;

    fn metric() -> MetricName {
        MetricName::parse("revenue").expect("a test metric is a metric")
    }

    fn dimension() -> DimensionName {
        DimensionName::parse("region").expect("a test dimension is a dimension")
    }

    /// One refusal reason, the status it is on the wire, and its code.
    ///
    /// Named rather than written out at the signature: `type_complexity` is a fair reading
    /// complaint about the tuple, and these three are exactly what a refused caller is given.
    type Expected = (RefusalReason, StatusCode, &'static str);

    /// Every variant the domain has, with the status it is on the wire.
    ///
    /// A list rather than one test per variant, and it is the same list the exhaustive match above
    /// is checked against: a variant added to `RefusalReason` breaks the compile in `refused`, and
    /// this is where somebody then writes down what they decided.
    fn every_reason() -> Vec<Expected> {
        vec![
            (
                RefusalReason::MetricUnknown { metric: metric() },
                StatusCode::NOT_FOUND,
                "metric_unknown",
            ),
            (
                RefusalReason::GrainNotSupported {
                    metric: metric(),
                    grain: Grain::Year,
                },
                StatusCode::UNPROCESSABLE_ENTITY,
                "grain_not_supported",
            ),
            (
                RefusalReason::DimensionNotPermitted {
                    metric: metric(),
                    dimension: dimension(),
                },
                StatusCode::FORBIDDEN,
                "dimension_not_permitted",
            ),
            (
                RefusalReason::DimensionNotFilterable {
                    metric: metric(),
                    dimension: dimension(),
                },
                StatusCode::FORBIDDEN,
                "dimension_not_filterable",
            ),
            (
                RefusalReason::DimensionValueNotAllowed {
                    metric: metric(),
                    dimension: dimension(),
                },
                StatusCode::FORBIDDEN,
                "dimension_value_not_allowed",
            ),
            (
                RefusalReason::DuplicateDimension { dimension: dimension() },
                StatusCode::UNPROCESSABLE_ENTITY,
                "duplicate_dimension",
            ),
            (
                RefusalReason::TooManyDimensions { requested: 5, limit: 4 },
                StatusCode::UNPROCESSABLE_ENTITY,
                "too_many_dimensions",
            ),
            (
                RefusalReason::TimeRangeTooLong { days: 9000, limit: 3653 },
                StatusCode::UNPROCESSABLE_ENTITY,
                "time_range_too_long",
            ),
            (
                RefusalReason::ResultTooLarge {
                    bound: ResultBound::Rows { limit: 10_000 },
                },
                StatusCode::PAYLOAD_TOO_LARGE,
                "result_too_large",
            ),
            (
                RefusalReason::ResourcesExhausted {
                    ceiling_bytes: 1024 * 1024 * 1024,
                },
                StatusCode::UNPROCESSABLE_ENTITY,
                "resources_exhausted",
            ),
            (
                RefusalReason::PlanSpansTwoSources { sources: 2 },
                StatusCode::CONFLICT,
                "plan_spans_two_sources",
            ),
            (
                RefusalReason::PlanTablesShareAnIdentifier {
                    table: TableName::parse("orders").expect("a test table is a table"),
                },
                StatusCode::CONFLICT,
                "plan_tables_share_an_identifier",
            ),
            (
                RefusalReason::SourceUnavailable {
                    source: sutura_domain::model::SourceName::parse("local").expect("a test source is a source"),
                },
                StatusCode::SERVICE_UNAVAILABLE,
                "source_unavailable",
            ),
            (
                RefusalReason::CredentialUnavailable {
                    source: sutura_domain::model::SourceName::parse("warehouse").expect("a test source is a source"),
                },
                StatusCode::FORBIDDEN,
                "credential_unavailable",
            ),
        ]
    }

    #[test]
    fn every_refusal_carries_a_status_a_code_and_a_sentence() {
        // The whole contract of this module in one assertion, per variant. All three, because a
        // caller needs all three: the status for anything that reads a status alone, the code to
        // branch on, and the sentence for a person.
        for (reason, expected, code) in every_reason() {
            let (status, body) = refused(&reason);
            assert_eq!(status, expected, "{reason:?}");
            assert_eq!(body.code(), code, "{reason:?}");
            assert_eq!(
                body.status(),
                expected.as_u16(),
                "the body's status disagrees with the response's for {reason:?}"
            );
            assert!(!body.detail().is_empty(), "{reason:?} refused with no sentence");
        }
    }

    #[test]
    fn no_refusal_comes_back_as_a_success() {
        // The point of the change, stated as the property rather than as eleven numbers. A `200`
        // makes a governance refusal indistinguishable from an answer to an ingress log, a
        // dashboard, an error-rate alert or a generated client whose success branch is `2xx`.
        for (reason, _, _) in every_reason() {
            let (status, _) = refused(&reason);
            assert!(
                status.is_client_error() || status.is_server_error(),
                "{reason:?} came back as {status}"
            );
        }
    }

    #[test]
    fn every_refusal_has_a_distinct_code() {
        // The status is shared on purpose - four variants are `422` - so the code is what a client
        // has to be able to branch on, and two variants sharing one would make that impossible.
        let mut codes: Vec<&str> = every_reason().into_iter().map(|(_, _, code)| code).collect();
        let count = codes.len();
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(codes.len(), count, "two refusals share a code");
    }

    #[test]
    fn the_row_cap_refusal_says_what_happened_and_what_to_do_about_it() {
        // The case the change was asked for. A caller whose answer was declined for being too large
        // must be able to tell that from the sentence alone: what happened, that nothing was
        // silently cut down to fit, the cap, and which two things they can narrow.
        let (status, body) = refused(&RefusalReason::ResultTooLarge {
            bound: ResultBound::Rows { limit: 10_000 },
        });
        assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
        let detail = body.detail();
        assert!(detail.contains("10000"), "the sentence does not name the cap: {detail}");
        assert!(detail.contains("NOT truncated"), "{detail}");
        assert!(detail.contains("narrow"), "{detail}");
    }

    #[test]
    fn a_result_the_data_system_would_not_return_at_once_is_not_a_dead_data_system() {
        // THE defect this bound was added for, in the shape the exhaustion test above already has:
        // a result INSIDE the row cap that the data system will not hand back in one piece used to
        // arrive as `ServiceError::Warehouse` and leave as `503 unavailable` - the same status
        // `source_unavailable` is, the one refusal on this surface where retrying is reasonable. So
        // the caller was told to retry against a bound that returns the same reply.
        //
        // Both halves are asserted, because either alone passes on the wrong grouping: the status is
        // not 503, and the code is the row cap's own - one answer, one thing to branch on.
        let (status, body) = refused(&RefusalReason::ResultTooLarge {
            bound: ResultBound::Volume,
        });
        assert_ne!(status, StatusCode::SERVICE_UNAVAILABLE, "a retry returns the same reply");
        assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(body.code(), "result_too_large");
        let detail = body.detail();
        // And the sentence must NOT invent a bound. `ResultBound::Volume` carries no number on
        // purpose, so a digit here would be a figure this deployment was never told - which is the
        // one way this arm could be worse than the `503` it replaced.
        assert!(
            !detail.chars().any(char::is_numeric),
            "the sentence names a bound nobody measured: {detail}"
        );
        assert!(detail.contains("NOT truncated"), "{detail}");
        assert!(detail.contains("narrow"), "{detail}");
        assert!(detail.contains("unchanged"), "{detail}");
    }

    #[test]
    fn exhaustion_is_not_the_status_a_dead_data_system_comes_back_as() {
        // The defect the variant was added for. Before it existed, an engine operator that could not
        // reserve memory arrived as `DataFusionError::Execute` and left as `503 unavailable` - which
        // is what `source_unavailable` is, the one refusal on this surface where retrying is
        // reasonable. So the caller was told to retry against a configured bound that fires again in
        // the same place. Both halves are asserted, because either alone would pass on the wrong
        // grouping: the status is not 503, and the code is not the retryable one.
        let (status, body) = refused(&RefusalReason::ResourcesExhausted {
            ceiling_bytes: 1024 * 1024 * 1024,
        });
        assert_ne!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(body.code(), "resources_exhausted");
        // The ceiling is a configured number, so the sentence may carry it - and an operator reading
        // a log needs it to know which key to change.
        let detail = body.detail();
        assert!(
            detail.contains("1073741824"),
            "the sentence does not name the ceiling: {detail}"
        );
        assert!(detail.contains("unchanged"), "{detail}");
    }

    #[test]
    fn a_missing_credential_at_a_source_is_not_a_request_to_authenticate_again() {
        // `docs/adr/0005` says the 403s on this surface are not a statement about a credential, and
        // this variant is the one that makes that stop being true - so the sentence has one job
        // beyond naming the source: it must not send the caller back to authenticate. A 401 would say
        // "present a credential", and a client that re-authenticates gets the same token and the same
        // refusal forever.
        let (status, body) = refused(&RefusalReason::CredentialUnavailable {
            source: sutura_domain::model::SourceName::parse("warehouse").expect("a test source is a source"),
        });
        assert_ne!(status, StatusCode::UNAUTHORIZED, "re-authenticating changes nothing here");
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body.code(), "credential_unavailable");
        let detail = body.detail();
        assert!(detail.contains("warehouse"), "the sentence names the source: {detail}");
        assert!(
            detail.contains("not about the token you presented"),
            "the sentence has to say the deployment token is not what is missing: {detail}"
        );
        // And it says the thing this whole port exists for: no fallback. The refusal is not "we could
        // not reach it", it is "we will not read it as somebody else".
        assert!(detail.contains("not read it as itself"), "{detail}");
    }

    #[test]
    fn a_rejected_filter_value_is_not_echoed_into_the_response() {
        // The domain refuses to carry the value in its refusal reason, and this is the assertion
        // that the wire shape does not put it back: a rejected value reflected into a response
        // reaches a log, a UI and an agent's context.
        let (status, body) = refused(&RefusalReason::DimensionValueNotAllowed {
            metric: metric(),
            dimension: dimension(),
        });
        assert_eq!(status, StatusCode::FORBIDDEN);
        let rendered = serde_json::to_string(&body).expect("the refusal serializes");
        assert!(rendered.contains("region"), "{rendered}");
        assert!(!rendered.contains("north"), "{rendered}");
    }
}
