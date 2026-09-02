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
fn usable_token(token: &str) -> bool {
    !token.is_empty()
        && token
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '~' | '='))
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
                sending = sending.header(QUOTA_PROJECT_HEADER, at.project().as_str());
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
        held.extend(page.tables.into_iter().filter_map(|entry| entry.table_reference?.table_id));
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

#[cfg(test)]
mod tests {
    use super::{PAGE_SIZE, page_url, usable_token};
    use crate::transport::{DatasetAddress, DatasetId, ProjectId};

    fn at() -> DatasetAddress {
        DatasetAddress::of(
            ProjectId::parse("acme-analytics").expect("a project id parses"),
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
}
