//! The one metadata read this transport makes: which tables a dataset holds.
//!
//! **A module of its own for `wire.rs`'s reason and one more.** The reason is the line count that
//! split `document.rs` out. The one more is that this is the only call in the crate that is not a
//! job: it is a `GET`, it is paged, it is billed for nothing, and it reads a document with a
//! different shape - so keeping it beside the query body would put two request shapes in one file
//! and invite a reader to assume the second follows the first's rules.
//!
//! Everything the surrounding module decides still applies and is not re-argued here: the host is a
//! `const`, the agent is the pinned one, redirects are refused, and the answer is read under a byte
//! cap. What IS decided here is paging, and it is the interesting half - see [`list`].

use crate::transport::{DatasetAddress, HeldTables};
use crate::wire::credential::{AccessTokens, QuotaProject};
use crate::wire::{BigQueryWire, CallDeadline, HOST, MAX_ANSWER_BYTES, QUOTA_PROJECT_HEADER, WireError, Wired, bounded};

/// How many tables one page asks for.
///
/// The endpoint's own default is far smaller, so stating this is what keeps an ordinary dataset to
/// ONE call - which is the whole affordability argument for a boot check that reads a set rather than
/// a model. It is not a bound on the dataset: a larger one pages, under [`MAX_PAGES`].
const PAGE_SIZE: u32 = 1000;

/// How many pages this will read before it refuses to conclude anything.
///
/// **A bound rather than a loop, and it has to fail rather than truncate.** The answer this call
/// feeds is *these tables are absent*, so a listing cut short would report a table that is there as
/// missing and refuse a correct deployment at boot. [`MAX_PAGES`] pages of [`PAGE_SIZE`] is 64,000
/// tables, which is past what a dataset holds; a dataset past it is one where this check says *could
/// not verify* rather than one where it guesses.
const MAX_PAGES: usize = 64;

/// The listing document, as `tables.list` answers it.
///
/// No `deny_unknown_fields`, for `document::QueryAnswer`'s reason: the service defines this document
/// and it already carries fields nothing here reads.
#[derive(Debug, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Listing {
    #[serde(default)]
    tables: Vec<Listed>,
    #[serde(default)]
    next_page_token: Option<String>,
}

/// One entry in a listing.
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Listed {
    #[serde(default)]
    table_reference: Option<Reference>,
}

/// Where one listed table lives.
///
/// Only `tableId` is read. The project and the dataset are in the request's own path, so reading
/// them back would be comparing the answer against the question rather than learning anything - and
/// an entry whose reference is absent is skipped rather than refused, because a listing entry this
/// crate cannot name is an entry no model can match anyway.
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Reference {
    #[serde(default)]
    table_id: Option<String>,
}

/// The URL one page of a dataset's table listing is read from.
///
/// **Interpolated with no escaping, and `document::url` carries the argument for why that is not an
/// injection:** `ProjectId::parse` accepts `[a-z0-9-]` and `DatasetId::parse` accepts `[A-Za-z0-9_]`,
/// so neither can hold a `/`, `?`, `#`, `%`, whitespace or a non-ASCII character. The page token is
/// the one part of this URL that came from somewhere else, and [`usable_token`] is why it cannot
/// carry one either.
fn page_url(at: &DatasetAddress, token: Option<&str>) -> String {
    let base = format!(
        "{HOST}/bigquery/v2/projects/{}/datasets/{}/tables?maxResults={PAGE_SIZE}",
        at.project().as_str(),
        at.dataset().as_str()
    );
    match token {
        None => base,
        Some(token) => format!("{base}&pageToken={token}"),
    }
}

/// Whether a page token the service handed back can be written into the next request's query string.
///
/// **Validated and never filtered, which is the decision worth reading.** A token is opaque: it means
/// nothing to this crate, so a character removed from it does not make it safe, it makes it a
/// DIFFERENT token - and the next page would be whatever that addresses. Filtering here would turn a
/// character this crate did not expect into a silently wrong listing, and a wrong listing is a table
/// reported absent that is there.
///
/// So the accepted set is the URL-unreserved characters plus `=`, which is what a base64url token
/// with padding is spelled in, and anything else refuses the whole call as
/// [`WireError::UnusablePageToken`]. That is fail-closed in the direction that matters: a boot check
/// that says *could not verify* is a warning an operator reads, where a truncated listing is a
/// deployment refused for a table it holds.
///
/// **A LENGTH as well as a character set, which review had to point out.** *Validated* named only
/// the alphabet: the token is interpolated into a URL, and its only bound was the 32 MiB answer cap -
/// so a 32 MiB token built a 32 MiB URL. Nothing reaches a log, so the cost was allocation rather
/// than injection, but `AGENTS.md` claims every foreign string in this crate is bounded and
/// character-filtered, and a claim is either true or deleted.
fn usable_token(token: &str) -> bool {
    !token.is_empty()
        && token.len() <= MAX_PAGE_TOKEN_BYTES
        && token
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '~' | '='))
}

/// How long a page token may be.
///
/// Generous against what the service sends - the real ones are tens of characters - and far below
/// what any HTTP stack will put in a request line. It is a bound rather than a measurement, which is
/// the honest description: nothing here knows the service's own limit.
const MAX_PAGE_TOKEN_BYTES: usize = 4096;

/// How long a table id may be, and which characters it may hold.
///
/// `BigQuery`'s documented maximum is 1024 characters; the accepted set is what a table id is spelled
/// in. **An id outside either is DROPPED from the listing rather than refusing the call**, and that
/// is the safe direction here: an id this crate cannot match is one no model in the bundle can name,
/// because a model's table name is parsed by `sutura_domain::model::TableName`, whose accepted set is
/// narrower. Refusing on it would let one exotic table elsewhere in the dataset stop a deployment
/// that has nothing to do with it.
fn usable_table_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= MAX_TABLE_ID_BYTES && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// `BigQuery`'s own documented maximum table-id length, in characters.
const MAX_TABLE_ID_BYTES: usize = 1024;

/// Every table id one dataset holds, read one page at a time.
///
/// **One absolute deadline for the whole listing and not one per page**, which is `submit`'s
/// correction applied to a call that can make several requests: a dataset that pages slowly shortens
/// the pages after it rather than each getting a full budget. A budget spent before the next page is
/// [`WireError::DeadlineSpent`], not a partial listing, because a partial listing is a wrong answer
/// rather than a slow one.
///
/// The quota-project header is asked of the CREDENTIAL exactly as `submit` asks it, and the project
/// it names is the one whose dataset is being listed.
pub(super) fn list<C>(wire: &BigQueryWire<C>, at: &DatasetAddress) -> Wired<HeldTables, C::Error>
where
    C: AccessTokens,
{
    let call = CallDeadline::opened(wire.agent.bounds().deadline());
    let now = BigQueryWire::<C>::now()?;
    let bearer = wire.source_bearer(now, call)?;
    let mut held = HeldTables::new();
    let mut token: Option<String> = None;
    for _ in 0..MAX_PAGES {
        let left = call.remaining().ok_or(WireError::DeadlineSpent {
            budget_seconds: wire.agent.bounds().deadline().seconds,
        })?;
        let mut sending = wire
            .agent
            .agent()
            .get(page_url(at, token.as_deref()))
            .config()
            .timeout_global(Some(CallDeadline::socket(left)))
            .build()
            .header("authorization", &bearer);
        match wire.credentials.quota_project() {
            QuotaProject::Required => {
                // **`billed_to` and NOT `project`, which is a review correction and a bug that only
                // showed up on a cross-project model.** This header names the project whose quota the
                // read is attributed to, and the caller has to hold `serviceusage.services.use` on
                // it; the dataset's own project is not that. `submit` sends the source's declared
                // billing project for the same reason - `docs/adr/0018` states the rule - and sending
                // the dataset's would 403 a listing whose QUERY works.
                sending = sending.header(QUOTA_PROJECT_HEADER, at.billed_to().as_str());
            }
            QuotaProject::FromTheCredential => {}
        }
        let mut answer = sending
            .call()
            .map_err(|cause| WireError::Unreachable { cause: Box::new(cause) })?;
        let status = answer.status();
        let text = answer
            .body_mut()
            .with_config()
            .limit(MAX_ANSWER_BYTES)
            .read_to_string()
            .map_err(|cause| WireError::Unreadable { cause: Box::new(cause) })?;
        if !status.is_success() {
            return Err(crate::wire::document::refusal(status.as_u16(), &text));
        }
        let page: Listing = serde_json::from_str(&text).map_err(|cause| WireError::NotAListing { cause })?;
        held.extend(
            page.tables
                .into_iter()
                .filter_map(|entry| entry.table_reference?.table_id)
                .filter(|id| usable_table_id(id)),
        );
        match page.next_page_token {
            None => return Ok(held),
            Some(next) if usable_token(&next) => token = Some(next),
            Some(next) => {
                return Err(WireError::UnusablePageToken {
                    named: bounded(Some(next)),
                });
            }
        }
    }
    Err(WireError::ListingDidNotFinish { pages: MAX_PAGES })
}

/// Whether a listing failure was the endpoint REFUSING, rather than failing to answer.
///
/// **A free function here rather than a method in `wire.rs`, for the reason `document.rs` exists:**
/// that file was at the length gate, and everything about the listing belongs together anyway.
///
/// A `401` or a `403` on `tables.list` means the identity this source was opened with may not list
/// the dataset - which fails identically on every boot, and which one IAM grant fixes. Everything
/// else is `false`, exhaustively rather than through a wildcard: an unreachable host, an unreadable
/// body, a listing that would not decode, a page token this transport will not send, a dataset that
/// did not finish listing. Each of those is a condition that can pass, so telling a composition root
/// to stop would refuse a deployment that would have worked.
///
/// **A `404` is deliberately NOT this.** A dataset that is not there is indistinguishable from one
/// whose name a deployment is about to fix, and the endpoint answers `404` for a project the caller
/// cannot see either - so it is left to the warning, which is where an ambiguous status belongs.
pub(super) const fn was_refused<C>(error: &WireError<C>) -> bool
where
    C: core::error::Error + 'static,
{
    match *error {
        WireError::Refused { status, .. } => status == 401 || status == 403,
        WireError::Credential { .. }
        | WireError::Expired { .. }
        | WireError::NoClock { .. }
        | WireError::DeadlineSpent { .. }
        | WireError::RequestNotSerializable { .. }
        | WireError::Unreachable { .. }
        | WireError::Unreadable { .. }
        | WireError::NotADocument { .. }
        | WireError::NotComplete { .. }
        | WireError::MoreThanOnePage
        | WireError::NoTotal { .. }
        | WireError::NotATotal { .. }
        | WireError::NoSchema { .. }
        | WireError::NotAScalar { .. }
        | WireError::NotAListing { .. }
        | WireError::UnusablePageToken { .. }
        | WireError::ListingDidNotFinish { .. } => false,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Listing, MAX_PAGE_TOKEN_BYTES, MAX_PAGES, MAX_TABLE_ID_BYTES, PAGE_SIZE, WireError, page_url, usable_table_id,
        usable_token, was_refused,
    };
    use crate::transport::{DatasetAddress, DatasetId, ProjectId};

    fn at() -> DatasetAddress {
        let project = ProjectId::parse("acme-analytics").expect("a project id parses");
        DatasetAddress::of(
            project.clone(),
            project,
            DatasetId::parse("Warehouse").expect("a dataset id parses"),
        )
    }

    #[test]
    fn a_listing_url_names_the_dataset_and_asks_for_a_whole_page() {
        assert_eq!(
            page_url(&at(), None),
            format!(
                "https://bigquery.googleapis.com/bigquery/v2/projects/acme-analytics/datasets/Warehouse/tables?maxResults={PAGE_SIZE}"
            ),
            "the path carries the project and the dataset, and the page size is stated rather than defaulted"
        );
    }

    #[test]
    fn a_second_page_is_asked_for_by_token_and_nothing_else_changes() {
        let first = page_url(&at(), None);
        assert_eq!(
            page_url(&at(), Some("tables-page-2_of_3==")),
            format!("{first}&pageToken=tables-page-2_of_3=="),
            "a page token is appended and the rest of the request is the same request"
        );
    }

    #[test]
    fn a_page_token_that_could_leave_the_query_string_is_not_accepted() {
        // The reason this is a validation and not a filter: a token is opaque, so a stripped
        // character makes it a different token rather than a safe one - and the page that answers
        // would be a listing this crate then compares a bundle against.
        for hostile in [
            "next&maxResults=1",
            "next#fragment",
            "next?alt=json",
            "next/../../datasets",
            "next%2f",
            "with space",
            "",
        ] {
            assert!(!usable_token(hostile), "{hostile:?} was accepted as a page token");
        }
        // The accepted set, spelled out by a value that uses all of it. A real token is opaque
        // base64url with padding; this one is written to be obviously not a credential, because the
        // leak scanner reads a high-entropy literal as one and is right to.
        assert!(
            usable_token("tables-page-2_of_3=="),
            "base64url characters with padding are what the service sends"
        );
    }

    /// The endpoint's own error envelope, as it answers a listing the caller may not make.
    fn refusal_body(reason: &str) -> String {
        format!(
            r#"{{"error":{{"code":403,"message":"Access Denied","status":"PERMISSION_DENIED","errors":[{{"reason":"{reason}"}}]}}}}"#
        )
    }

    /// The transport error a status and that body become. `FakeCause` stands in for the credential
    /// source's error, which this decision never reads.
    #[derive(Debug, thiserror::Error)]
    #[error("the credential source cannot fail in this test")]
    struct FakeCause;

    fn refused(status: u16, reason: &str) -> WireError<FakeCause> {
        crate::wire::document::refusal(status, &refusal_body(reason))
    }

    #[test]
    fn a_listing_document_decodes_to_the_ids_the_service_named() {
        // **A hand-written response document, which is this crate's established precedent** - the
        // wire cannot be pointed at a loopback, so what a test can prove is that this adapter reads
        // the document it says it reads.
        let body = r#"{
          "kind": "bigquery#tableList",
          "etag": "abc",
          "totalItems": 2,
          "tables": [
            {"kind":"bigquery#table","id":"acme:warehouse.dim_customer",
             "tableReference":{"projectId":"acme","datasetId":"warehouse","tableId":"dim_customer"},
             "type":"TABLE"},
            {"kind":"bigquery#table","id":"acme:warehouse.fct_orders",
             "tableReference":{"projectId":"acme","datasetId":"warehouse","tableId":"fct_orders"},
             "type":"TABLE"}
          ]
        }"#;
        let page: Listing = serde_json::from_str(body).expect("a real listing document decodes");
        let ids: Vec<String> = page
            .tables
            .into_iter()
            .filter_map(|entry| entry.table_reference?.table_id)
            .collect();
        assert_eq!(ids, vec![String::from("dim_customer"), String::from("fct_orders")]);
        assert!(page.next_page_token.is_none(), "one page carries no token");
    }

    #[test]
    fn a_document_whose_shape_this_crate_does_not_recognise_decodes_to_nothing() {
        // **The failure mode worth pinning rather than the happy path.** Every field here is
        // `#[serde(default)]`, so a document this crate cannot read decodes to an EMPTY listing - and
        // the pre-flight reads an empty listing as *every table is absent*, which is the one wrong
        // answer the whole feature exists to prevent. What stops that being a wrong answer to a
        // caller is the direction it fails in: an empty listing refuses the deployment rather than
        // serving it, and `AGENTS.md` records that as a stated limit rather than a guarantee.
        //
        // An empty `tables` really is what the service sends for an empty dataset, so this cannot be
        // told from a shape change here - which is exactly why it is written down.
        for body in ["{}", r#"{"kind":"bigquery#tableList","totalItems":0}"#, r#"{"tables":[]}"#] {
            let page: Listing = serde_json::from_str(body).expect("a document with no tables still decodes");
            assert!(page.tables.is_empty(), "{body}");
            assert!(page.next_page_token.is_none(), "{body}");
        }
    }

    #[test]
    fn an_entry_with_no_reference_is_skipped_rather_than_refusing_the_listing() {
        let body = r#"{"tables":[{"kind":"bigquery#table"},{"tableReference":{"tableId":"dim_customer"}}]}"#;
        let page: Listing = serde_json::from_str(body).expect("a partial entry still decodes");
        let ids: Vec<String> = page
            .tables
            .into_iter()
            .filter_map(|entry| entry.table_reference?.table_id)
            .collect();
        assert_eq!(ids, vec![String::from("dim_customer")]);
    }

    #[test]
    fn an_authorization_failure_is_a_refusal_and_an_outage_is_not() {
        // THE split, and it is the finding that made this predicate exist: a `403` on `tables.list`
        // is a grant an operator adds and will fail identically forever, where a `503` passes. Before
        // the split both were the same permanent WARN in the deployment least likely to notice.
        assert!(was_refused(&refused(403, "accessDenied")), "a 403 is a refusal");
        assert!(was_refused(&refused(401, "unauthorized")), "a 401 is a refusal");
        // The controls, and they are what stop this being a predicate that says yes to everything.
        assert!(!was_refused(&refused(503, "backendError")), "an outage is not a refusal");
        assert!(!was_refused(&refused(500, "internalError")), "an outage is not a refusal");
        // A dataset that is not there is deliberately left to the warning: it cannot be told from a
        // name a deployment is about to fix, and the endpoint answers it for an invisible project too.
        assert!(!was_refused(&refused(404, "notFound")), "a 404 is ambiguous, so it warns");
        assert!(
            !was_refused(&WireError::<FakeCause>::ListingDidNotFinish { pages: MAX_PAGES }),
            "a dataset that did not finish listing can finish on the next boot"
        );
    }

    #[test]
    fn a_page_token_is_bounded_as_well_as_filtered() {
        // *Validated* named only the alphabet until review: the token goes into a URL, and its only
        // bound was the 32 MiB answer cap.
        let long: String = core::iter::repeat_n('a', MAX_PAGE_TOKEN_BYTES + 1).collect();
        assert!(!usable_token(&long), "a token past the bound is refused");
        let at_the_bound: String = core::iter::repeat_n('a', MAX_PAGE_TOKEN_BYTES).collect();
        assert!(usable_token(&at_the_bound), "the bound itself is usable");
    }

    #[test]
    fn a_table_id_this_crate_could_not_match_is_dropped_from_the_listing() {
        // Dropped rather than refusing the call: an id outside `TableName`'s own accepted set is one
        // no model in the bundle can name, so one exotic table elsewhere in the dataset must not stop
        // a deployment that has nothing to do with it.
        assert!(usable_table_id("dim_customer"));
        assert!(!usable_table_id(""), "an empty id matches nothing");
        assert!(!usable_table_id("dim-customer"), "a hyphen is not in a table id");
        assert!(!usable_table_id("dim.customer"), "a dot would look like a path");
        let long: String = core::iter::repeat_n('a', MAX_TABLE_ID_BYTES + 1).collect();
        assert!(!usable_table_id(&long), "past BigQuery's own maximum");
    }
}
