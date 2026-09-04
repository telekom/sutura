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

use std::collections::BTreeSet;

use crate::transport::{DatasetAddress, HeldTables, ListingTotal};
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
    /// How many tables the service says the dataset holds, and it is read as raw JSON on purpose.
    ///
    /// **The one field here that must not be able to refuse a document, which is stronger than
    /// tolerating its absence.** `Option<u64>` would already read a missing field as [`None`]; what
    /// it would ALSO do is fail the whole decode on a value spelled some other way - and a failed
    /// decode of this listing is a pre-flight that stops verifying, silently, in the warning half.
    /// So a field nothing yet decides on could turn a working boot check off. This crate already
    /// knows the service spells one count as a JSON number and another as a string:
    /// `document::QueryAnswer::total_rows` is `Option<String>` because `totalRows` is a `uint64`.
    ///
    /// **Measured rather than assumed:** in the endpoint's own discovery document, read on
    /// 2026-09-04 at revision `20260811`, `TableList.totalItems` is
    /// `{"format": "int32", "type": "integer"}` - a bare JSON number - described as *"The total
    /// number of tables in the dataset"*, against the neighbouring `etag`'s *"A hash of this page of
    /// results"*. So it is the DATASET's number and not the page's, which is what makes comparing it
    /// against a whole finished listing the right comparison. **A real answer does populate it**,
    /// measured in the `bigquery-acceptance` job on 2026-09-04 - one dataset at one moment, which is
    /// the whole of what that establishes.
    #[serde(default)]
    total_items: Option<serde_json::Value>,
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

/// What a finished listing's own `totalItems` says, against the entries the document carried.
///
/// **Infallible by construction, and that is the property rather than an implementation detail.**
/// Every way the field can arrive that this crate cannot read as a count lands on
/// [`ListingTotal::Unreadable`] - a float, a negative, an object, a number past `u64`, a string that
/// is not a number - so the field can neither refuse a listing nor be mistaken for one of the two
/// answers that mean something. [`Listing::total_items`] argues why that matters and why a quoted
/// count is read too.
///
/// `carried` is the count of entries the whole finished listing carried - every page, before any id
/// this crate cannot match was dropped. [`ListingTotal`] says why it may not be the named set.
fn reported_total(field: Option<&serde_json::Value>, carried: usize) -> ListingTotal {
    let reported = match field {
        None | Some(serde_json::Value::Null) => return ListingTotal::Unreported,
        Some(serde_json::Value::Number(number)) => number.as_u64(),
        Some(serde_json::Value::String(text)) => text.parse::<u64>().ok(),
        Some(_) => None,
    };
    // The loop below reads at most `MAX_PAGES * PAGE_SIZE` entries, so the fallback is unreachable -
    // and it is the direction that cannot invent a shape change out of a conversion.
    let carried = u64::try_from(carried).unwrap_or(u64::MAX);
    match reported {
        None => ListingTotal::Unreadable,
        Some(reported) if reported > carried => ListingTotal::Short { reported, carried },
        Some(reported) => ListingTotal::Accounted { reported },
    }
}

/// A listing being read, one page at a time.
///
/// **A type rather than three locals inside [`list`], because the decisions worth pinning are the
/// ones that span pages** - which page's total counts, and that the entry count is the whole
/// listing's rather than the last page's. As locals in a loop only a socket can drive, both would be
/// claims in a doc comment; here `super::tests` folds pages in by hand and asserts them.
#[derive(Debug, Default)]
struct Accumulating {
    /// The ids read so far, after every id this crate cannot match was dropped.
    named: BTreeSet<String>,
    /// Every entry the document carried, BEFORE any of that dropping. See [`ListingTotal`].
    carried: usize,
    /// The first page's `totalItems`, still as the service spelled it.
    total: Option<serde_json::Value>,
    /// How many pages have been folded in, which is the only way [`Self::absorb`] knows it is first.
    pages: usize,
}

impl Accumulating {
    /// Folds one page in and answers the page token it carried, if it carried one.
    fn absorb(&mut self, page: Listing) -> Option<String> {
        // **The FIRST page's total, and the choice matters in one direction only.** The service
        // documents the number as the DATASET's rather than the page's, so every page should repeat
        // it; where they disagree, the page that decides is the one whose emptiness is the whole
        // ambiguity - a listing whose first page carries no entries carries no page token either, so
        // it is also the only page there is.
        if self.pages == 0 {
            self.total = page.total_items;
        }
        self.pages += 1;
        self.carried += page.tables.len();
        self.named.extend(
            page.tables
                .into_iter()
                .filter_map(|entry| entry.table_reference?.table_id)
                .filter(|id| usable_table_id(id)),
        );
        page.next_page_token
    }

    /// The answer, once the service has said there is no more of the listing.
    ///
    /// Only reached where the listing FINISHED: a run out of pages or of budget is an `Err`, so a
    /// total is never compared against a count this transport knows is short.
    fn finish(self) -> HeldTables {
        HeldTables::of(self.named, reported_total(self.total.as_ref(), self.carried))
    }
}

/// Every table id one dataset holds, read one page at a time.
///
/// **One absolute deadline for the whole listing and not one per page**, which is `submit`'s
/// correction applied to a call that can make several requests: a dataset that pages slowly shortens
/// the pages after it rather than each getting a full budget. A budget spent before the next page is
/// [`WireError::DeadlineSpent`], not a partial listing, because a partial listing is a wrong answer
/// rather than a slow one.
///
/// The quota-project header is asked of the CREDENTIAL exactly as `submit` asks it, and the project
/// it names is the SOURCE's declared billing project - never the one the dataset lives in. **This
/// sentence read the other way round until review corrected it a second time:** the code below was
/// already right and the doc still stated the rule the fix had reversed, which is the cheapest
/// available way to reintroduce the bug. The dataset's own project goes in the request PATH, and
/// [`DatasetAddress`] names its two accessors after their roles for exactly this reason.
pub(super) fn list<C>(wire: &BigQueryWire<C>, at: &DatasetAddress) -> Wired<HeldTables, C::Error>
where
    C: AccessTokens,
{
    let call = CallDeadline::opened(wire.agent.bounds().deadline());
    let now = BigQueryWire::<C>::now()?;
    let bearer = wire.source_bearer(now, call)?;
    let mut listing = Accumulating::default();
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
        match listing.absorb(page) {
            None => return Ok(listing.finish()),
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
/// A `401` means the identity this source was opened with could not authenticate at all, and a `403`
/// USUALLY means it may not list the dataset - which fails identically on every boot, and which one
/// IAM grant fixes. Everything else is `false`, exhaustively rather than through a wildcard: an
/// unreachable host, an unreadable body, a listing that would not decode, a page token this transport
/// will not send, a dataset that did not finish listing. Each of those is a condition that can pass,
/// so telling a composition root to stop would refuse a deployment that would have worked.
///
/// **A `404` is deliberately NOT this.** A dataset that is not there is indistinguishable from one
/// whose name a deployment is about to fix, and the endpoint answers `404` for a project the caller
/// cannot see either - so it is left to the warning, which is where an ambiguous status belongs.
///
/// **Nor is every `403`, which is a review finding and the reason this reads `named` at all.** The
/// status alone was the whole decision, and [`crate::wire::document::refusal`] fills `named` from the
/// per-error `reason` precisely so 403s can be told apart. `BigQuery` documents six reasons at that
/// status, and two of them are not a grant: `rateLimitExceeded` - *"your project exceeds a short-term
/// rate limit by sending too many requests too quickly"*, whose own remedy is *"slow down the request
/// rate"* - and `quotaExceeded`, a project or custom quota rather than a permission. So three
/// replicas restarting together could be told to add a grant they already hold, and refuse, where a
/// `503` in the same position warned and served. Both go with the `404`: ambiguous belongs in the
/// warning half.
///
/// **Documentation rather than measurement, said out loud because the two spellings are load-bearing
/// and nothing here can provoke them.** They are `BigQuery`'s own published reason vocabulary,
/// re-read against `cloud.google.com/bigquery/docs/error-messages` on 2026-09-02, where
/// `rateLimitExceeded` is the only 403 that document calls retryable. `quotaExceeded` is documented
/// as needing intervention rather than as transient, and it is in the warning half anyway on the
/// argument this predicate actually makes: what a refusal here tells an operator is *grant
/// `bigquery.tables.list`*, and a quota is not that - a quota also resets, so the next boot can
/// succeed, which is exactly the *condition that can pass* test every `false` arm below meets.
///
/// **What is deliberately left in the refusal half, so the judgement is visible rather than
/// implied:** `blocked` (*"temporarily denylisted ... contact support"*), `billingNotEnabled` and
/// `responseTooLarge`. The first reads transient and its remedy is not self-service; the other two
/// fail identically on every boot. None of the three is fixed by the grant a refusal here names, so
/// refusing on them costs an operator a startup message pointing at the wrong permission - which is
/// the limit of this split rather than a case it answers.
pub(super) fn was_refused<C>(error: &WireError<C>) -> bool
where
    C: core::error::Error + 'static,
{
    match *error {
        WireError::Refused { status, ref named, .. } => {
            (status == 401 || status == 403) && !matches!(named.as_str(), "rateLimitExceeded" | "quotaExceeded")
        }
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
        Accumulating, Listing, MAX_PAGE_TOKEN_BYTES, MAX_PAGES, MAX_TABLE_ID_BYTES, PAGE_SIZE, WireError, page_url,
        usable_table_id, usable_token, was_refused,
    };
    use crate::transport::{DatasetAddress, DatasetId, HeldTables, ListingTotal, ProjectId};

    /// One page of a listing, read the way [`super::list`] reads one - and nothing more.
    ///
    /// A helper rather than a copy of the loop: what it leaves out is the socket, the deadline and
    /// the page token, which is the whole of what a document cannot say. Pages are folded in in
    /// order, so a multi-page listing is written as a slice of documents.
    fn read(pages: &[&str]) -> HeldTables {
        let mut listing = Accumulating::default();
        for body in pages {
            let page: Listing = serde_json::from_str(body).expect("a listing document decodes");
            listing.absorb(page);
        }
        listing.finish()
    }

    /// A CROSS-PROJECT address, and the two projects differ on purpose.
    ///
    /// **They used to be the same value in both roles, which is what review found:** the fix that
    /// made [`list`] send `billed_to` in the quota header and `project` in the path was then
    /// unobservable here, because every fixture read the same string whichever accessor it went
    /// through. A source declared `billing_project: acme-analytics` reading `partner-data.Warehouse.*`
    /// is the case the third field exists for, so it is the case the fixture is.
    fn at() -> DatasetAddress {
        DatasetAddress::of(
            ProjectId::parse("acme-analytics").expect("the source's billing project parses"),
            ProjectId::parse("partner-data").expect("the dataset's own project parses"),
            DatasetId::parse("Warehouse").expect("a dataset id parses"),
        )
    }

    #[test]
    fn a_listing_url_names_the_dataset_and_asks_for_a_whole_page() {
        assert_eq!(
            page_url(&at(), None),
            format!(
                "https://bigquery.googleapis.com/bigquery/v2/projects/partner-data/datasets/Warehouse/tables?maxResults={PAGE_SIZE}"
            ),
            "the path carries the DATASET's own project - never the one the read is billed to - and \
             the page size is stated rather than defaulted"
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
        // An empty `tables` really is what the service sends for an empty dataset. What tells the two
        // apart is the SAME document's own total, and that is the test below rather than a sentence
        // here - these three documents are the ones where it cannot help, because none of them
        // reports a total above zero.
        for body in ["{}", r#"{"kind":"bigquery#tableList","totalItems":0}"#, r#"{"tables":[]}"#] {
            let page: Listing = serde_json::from_str(body).expect("a document with no tables still decodes");
            assert!(page.tables.is_empty(), "{body}");
            assert!(page.next_page_token.is_none(), "{body}");
        }
    }

    #[test]
    fn a_listing_carrying_no_entries_beside_a_non_zero_total_is_not_an_empty_dataset() {
        // **THE cross-check, and the reason it is worth a decoder change:** an empty listing means
        // *every table is absent* one port up, and until the total was read that answer was the same
        // value whether the dataset was empty or the document had stopped being a document this crate
        // can read. The three cases are three different things a caller may conclude, so they are
        // three variants rather than a boolean.
        assert_eq!(
            read(&[r#"{"kind":"bigquery#tableList","totalItems":7,"tables":[]}"#]).total(),
            ListingTotal::Short { reported: 7, carried: 0 },
            "a dataset that answers with no entries while claiming seven tables is a shape change"
        );
        assert_eq!(
            read(&[r#"{"kind":"bigquery#tableList","totalItems":0,"tables":[]}"#]).total(),
            ListingTotal::Accounted { reported: 0 },
            "the service saying zero twice is an empty dataset, which is the case the shape change \
             was indistinguishable from"
        );
        assert_eq!(
            read(&["{}"]).total(),
            ListingTotal::Unreported,
            "a document that reports no total leaves the two indistinguishable, and says so"
        );
    }

    #[test]
    fn a_total_the_document_never_sent_is_told_from_one_this_crate_could_not_read() {
        // **Two absences a `#[serde(default)]` decode is famous for merging**, and merging them here
        // would cost the finding: *the service stopped sending the field* and *the service sent
        // something else* are different shape changes, and only the second says the document is being
        // generated differently.
        assert_eq!(read(&[r#"{"tables":[]}"#]).total(), ListingTotal::Unreported);
        assert_eq!(
            read(&[r#"{"totalItems":null,"tables":[]}"#]).total(),
            ListingTotal::Unreported
        );
        for spelling in [
            r#"{"totalItems":-1,"tables":[]}"#,
            r#"{"totalItems":1.5,"tables":[]}"#,
            r#"{"totalItems":true,"tables":[]}"#,
            r#"{"totalItems":{"count":2},"tables":[]}"#,
            r#"{"totalItems":[2],"tables":[]}"#,
            r#"{"totalItems":"","tables":[]}"#,
            r#"{"totalItems":"two","tables":[]}"#,
        ] {
            assert_eq!(read(&[spelling]).total(), ListingTotal::Unreadable, "{spelling}");
        }
        // THE control, without which the loop above is a function that answers `Unreadable` to
        // everything: a count spelled as a string is read. `Listing::total_items` says why one can
        // arrive that way.
        assert_eq!(
            read(&[r#"{"totalItems":"12","tables":[]}"#]).total(),
            ListingTotal::Short {
                reported: 12,
                carried: 0
            }
        );
    }

    #[test]
    fn a_total_this_crate_cannot_read_never_costs_the_listing_that_carried_it() {
        // **The property the field is decoded as raw JSON for** - `Listing::total_items` argues the
        // direction. The mutation that shows this test is not free: the field as an `Option<u64>`
        // fails here with `invalid type: map, expected u64`.
        for hostile in [
            r#"{"totalItems":{"count":2},"tables":[{"tableReference":{"tableId":"dim_customer"}}]}"#,
            r#"{"totalItems":"lots","tables":[{"tableReference":{"tableId":"dim_customer"}}]}"#,
            r#"{"totalItems":-2,"tables":[{"tableReference":{"tableId":"dim_customer"}}]}"#,
        ] {
            let held = read(&[hostile]);
            assert!(held.holds("dim_customer"), "the ids survived the unreadable total: {hostile}");
            assert_eq!(held.total(), ListingTotal::Unreadable, "{hostile}");
        }
    }

    #[test]
    fn a_table_id_this_crate_dropped_does_not_read_as_a_listing_short_of_its_own_total() {
        // **The wrong claim this cross-check would otherwise make, and it is why the comparison is
        // against the entries a document CARRIED rather than the ids it named.** `BigQuery` permits a
        // table id outside `usable_table_id`'s set, and such an id is dropped from the listing - so a
        // dataset holding one names fewer ids than its total claims while nothing whatever is wrong.
        // Comparing against the named set would report it as a shape change: an overstated finding
        // replacing an unsettled question, which is the defect this whole change is about.
        let held = read(&[r#"{"totalItems":3,"tables":[
            {"tableReference":{"tableId":"dim_customer"}},
            {"tableReference":{"tableId":"dim-region"}},
            {"tableReference":{"tableId":"fct_orders"}}
        ]}"#]);
        assert_eq!(
            held.named().len(),
            2,
            "the hyphenated id is dropped, which is the case that makes this test necessary"
        );
        assert_eq!(
            held.total(),
            ListingTotal::Accounted { reported: 3 },
            "three entries against a total of three is accounted for, however many ids survived"
        );
    }

    #[test]
    fn the_total_that_counts_is_the_first_pages_and_the_entries_are_every_pages() {
        // **The two decisions that span pages**, which as locals in a loop only a socket can drive
        // were a doc comment and nothing else. A dataset reporting three tables over two pages is
        // accounted for; the same first page followed by one carrying nothing is short of it.
        let first = r#"{"totalItems":3,"tables":[
            {"tableReference":{"tableId":"dim_customer"}},
            {"tableReference":{"tableId":"fct_orders"}}
        ],"nextPageToken":"more"}"#;
        assert_eq!(
            read(&[
                first,
                r#"{"totalItems":3,"tables":[{"tableReference":{"tableId":"dim_plan"}}]}"#
            ])
            .total(),
            ListingTotal::Accounted { reported: 3 },
            "the entry count is the whole listing's, so a paged dataset is not short of its own total"
        );
        assert_eq!(
            read(&[first, r#"{"totalItems":3,"tables":[]}"#]).total(),
            ListingTotal::Short { reported: 3, carried: 2 },
            "a page that carried nothing is still a page that carried nothing"
        );
        // And the FIRST page decides, so a later page disagreeing with itself cannot change the
        // number the comparison is made against.
        assert_eq!(
            read(&[
                first,
                r#"{"totalItems":99,"tables":[{"tableReference":{"tableId":"dim_plan"}}]}"#
            ])
            .total(),
            ListingTotal::Accounted { reported: 3 },
            "the total is read once, from the page whose emptiness would be the ambiguity"
        );
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
    fn a_403_the_service_documents_as_a_rate_limit_is_not_read_as_a_missing_grant() {
        // **The status alone was the whole decision until review, and this is the case that broke
        // it:** three replicas restart together, `tables.list` answers `403 rateLimitExceeded` on
        // two, and both refuse to start with a message telling the operator to add a grant they
        // already hold - where a `503` in the same position warned and served.
        //
        // One assertion per reason rather than a loop, so a failure names which reason regressed.
        assert!(
            !was_refused(&refused(403, "rateLimitExceeded")),
            "the service's own documented retryable 403 warns rather than refusing"
        );
        assert!(
            !was_refused(&refused(403, "quotaExceeded")),
            "a quota is not the `bigquery.tables.list` grant a refusal here names, and it resets"
        );
        // **THE control, and it is what stops the fix from being a hole.** Reading `named` at all
        // only pays if the ordinary permission refusal still refuses.
        assert!(
            was_refused(&refused(403, "accessDenied")),
            "a plain permission refusal is still a refusal"
        );
        // A body that is not the envelope leaves `named` empty - `document::refusal` says so - and an
        // unnamed 403 has to stay a refusal, or a shape change at the service would silently turn the
        // whole split off.
        assert!(
            was_refused(&crate::wire::document::refusal::<FakeCause>(403, "not the envelope")),
            "a 403 this crate could name no reason for is still a refusal"
        );
        // And the two reasons are not a status-blind allowlist: the same word at 503 was already a
        // warning and stays one, so the arm reads the PAIR rather than the reason alone.
        assert!(!was_refused(&refused(503, "rateLimitExceeded")), "still not a refusal");
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
