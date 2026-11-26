//! A shared "include whole items up to a byte budget, name the rest" selector.
//!
//! The same skip-and-continue shape is needed in four places that do not share a crate: the MCP
//! system prompt's physical-schema section (`prompt::physical_schema`), `sutura-mcp`'s
//! `describe_catalog` result (both its metrics and its physical models), and `sutura-http`'s
//! `CatalogBody`. One definition here, rather than four hand-rolled loops that drift apart.

/// `fill_to_budget`'s result.
///
/// The items it kept whole, a descriptor of the first `max_omitted` it left out (a plain name, or -
/// where a caller needs more than a name to read an item back - a small struct of its own), and the
/// true count left out (which may exceed that list's length).
pub type Budgeted<T, N = String> = (Vec<T>, Vec<N>, usize);

/// Packs `items` into `budget` bytes of `cost`, in iteration order.
///
/// The first item is always included, even alone over budget - a budget no single item fits under
/// must never yield an empty result, which would read as "this deployment defines nothing" rather
/// than "ask for one by name". Every item after that is included only while the running total still
/// fits; an item that does not fit is never truncated - it is named, by `name`, in the second
/// returned list. Only the first `max_omitted` such descriptors are kept, so the omission list is
/// itself bounded; the third return value is the TRUE number omitted, which can exceed that list's
/// length once that cap is reached.
///
/// `name` is generic in its return type rather than fixed to `String`: a metric's own omission entry
/// needs its model riding along to stay reachable by `describe_catalog(metric: ...)` without a
/// second lookup, so a caller filling a metrics pool returns a small `{metric, model}` struct here
/// instead of a pre-formatted string. Whatever `name` returns, [`omission_reserve`] must be handed
/// the SAME per-item `entry_cost` this function would charge an omitted item, so the budget handed
/// in already set enough aside for a full omission list of that shape - this function itself does
/// not know or check that.
pub fn fill_to_budget<T, N>(
    items: impl IntoIterator<Item = T>,
    budget: usize,
    max_omitted: usize,
    mut cost: impl FnMut(&T) -> usize,
    mut name: impl FnMut(&T) -> N,
) -> Budgeted<T, N> {
    let mut included = Vec::new();
    let mut omitted_names = Vec::new();
    let mut omitted_count = 0_usize;
    let mut used = 0_usize;
    for item in items {
        let size = cost(&item);
        if !included.is_empty() && used.saturating_add(size) > budget {
            omitted_count += 1;
            if omitted_names.len() < max_omitted {
                omitted_names.push(name(&item));
            }
            continue;
        }
        used = used.saturating_add(size);
        included.push(item);
    }
    (included, omitted_names, omitted_count)
}

/// The byte budget a catalog listing - the system prompt's physical schema section, or either
/// transport's `describe_catalog`/`CatalogBody` metrics and physical models - holds its items to.
///
/// **Measured, not guessed.** The shipped `examples/single-player` catalog (11 metrics, two models,
/// the glossary/caveat/example knowledge the README ships) renders to 67253 bytes through this same
/// code path with no cap in effect, measured directly against `crates/sutura-app/src/prompt.rs`'s
/// `render` at `github.com/telekom/sutura#2071`. An earlier version of this bound was 64 KiB,
/// borrowed by analogy from an unrelated 240538-byte planner measurement
/// (`github.com/telekom/sutura#2071`'s second ask) rather than from this document's own fixed cost;
/// at 64 KiB the example's own system prompt silently dropped one metric's whole section (keeping
/// only its name in the omission list), which is exactly the failure this bound exists to prevent
/// for an ordinarily authored catalog. 128 KiB is roughly 1.9x the measured 67253 bytes - enough
/// headroom that a normal catalog growing by a few more metrics still reaches the agent complete -
/// while staying far short of the 1 MiB planner bound, so a listing that still crosses this one is
/// the deployment-scale case this budget is for, not an ordinarily sized one.
///
/// **One shared budget per whole agent-facing result, spent in sequence - never one per pool.**
/// Each call site first measures its FIXED parts (the notice, the knowledge section, the operator's
/// instructions, the provenance trailer - on `sutura-mcp` counted once per half the result carries,
/// since the structured and the text half both pay for them independently), subtracts that from
/// `MAX_CATALOG_LISTING_BYTES`, then hands what remains to the metrics pool's own `fill_to_budget`
/// call, then hands what metrics did not spend to the physical-models pool's call. No interleaving
/// is needed for this: the pools are still walked one after another, only the budget each is handed
/// is `remaining_after_the_previous_pool` rather than a fresh budget every time, so the whole result
/// - fixed parts plus every pool - never exceeds this one bound.
///
/// **The limit:** the spend order is fixed (metrics always get first claim on what is left after
/// the fixed parts, models get what metrics did not spend), so a result with an enormous metric
/// vocabulary and a small physical schema can still omit models it would have carried whole had the
/// pools been budgeted the other way around. `model`/`metric` narrows the metrics pool to what one
/// name needs and sidesteps this ordering entirely.
pub const MAX_CATALOG_LISTING_BYTES: usize = 128 * 1024;

/// How many omitted names a listing keeps before it stops naming them and states the count instead.
pub const MAX_OMITTED_NAMES: usize = 200;

/// Fixed bytes an omission list's own scaffolding costs, independent of how many entries end up in
/// it - the lead-in sentence, the byte bound quoted back into it, the trailing remedy sentence, or
/// (structured) the `omitted_*_more` field's own key and digits - plus whatever OTHER fixed bytes a
/// call site's own rendering adds around a pool that this module never sees and so cannot measure:
/// a structured half's own unchanging JSON keys (`"metrics":`, `"omitted_metrics":`, the braces and
/// commas around them) and a text half's trailing sentence are both paid out of this one reserve
/// too, not a reserve of their own. One generous constant rather than a measurement of each call
/// site's own wording: that wording is free to change without this reserve drifting out from under
/// it, and every call site's actual total - header, keys and trailer together - is comfortably
/// under it, which the over-budget total-size tests in each call site's module hold to directly.
const OMISSION_HEADER_RESERVE: usize = 512;

/// Sizes an omission-list reserve from the REAL candidates that might end up in it, never from a
/// worst-case identifier length.
///
/// The previous version reserved as though every one of up to [`MAX_OMITTED_NAMES`] entries were
/// [`sutura_domain::model::MAX_IDENTIFIER_LEN`] bytes long, win or lose - 32112 bytes for a
/// two-identifier metric entry, held back on EVERY call regardless of how many metrics a catalog
/// actually defines or how long their names are, doubled again on `sutura-mcp`'s two halves. A
/// deployment the size of `examples/single-player` (11 metrics) could lose most of its listing to
/// that reserve alone before a single metric ever crossed the real bound
/// (`github.com/telekom/sutura#2081`'s review).
///
/// `entry_cost(item)` must be the SAME cost a later omission note charges for naming `item` -
/// the text line, the structured object, or (on a transport that renders both) their sum - never
/// `item`'s own full content cost. Only the `min(MAX_OMITTED_NAMES, candidates.len() - 1)` LARGEST
/// such costs are summed: at most `candidates.len() - 1` of `candidates` can ever be omitted (the
/// first item is always included, even alone over budget - see `fill_to_budget`), and the list
/// itself never names more than [`MAX_OMITTED_NAMES`], so reserving for more than that covers an
/// omission that can never happen.
///
/// **Reserved from a pool's budget BEFORE `fill_to_budget` is called, never after.**
/// `fill_to_budget` only learns an omission list's real size once it has decided what did not fit,
/// so a budget that did not set this aside up front could let included content plus the note
/// describing what got left out cross [`MAX_CATALOG_LISTING_BYTES`] together, even with the
/// included content alone safely under it - which is exactly how a result could overshoot this
/// bound by tens of kilobytes before this reserve existed (`github.com/telekom/sutura#2071`).
pub fn omission_reserve<T>(candidates: &[T], mut entry_cost: impl FnMut(&T) -> usize) -> usize {
    let max_omittable = candidates.len().saturating_sub(1).min(MAX_OMITTED_NAMES);
    if max_omittable == 0 {
        return OMISSION_HEADER_RESERVE;
    }
    let mut costs: Vec<usize> = candidates.iter().map(&mut entry_cost).collect();
    costs.sort_unstable_by(|left, right| right.cmp(left));
    OMISSION_HEADER_RESERVE + costs.into_iter().take(max_omittable).sum::<usize>()
}

/// A page of names plus the cursor to resume from, `None` once the index is exhausted - see
/// [`paged_names`].
pub type PagedNames<T> = (Vec<T>, Option<String>);

/// Pages a caller-visible, sorted name set behind a `names`-only index.
///
/// The mechanism that keeps every item reachable once a listing's own `MAX_OMITTED_NAMES`-capped
/// omission list cannot name all of them (`github.com/telekom/sutura#2083`).
///
/// `items` is walked in order (the caller's own sort - typically a `BTreeMap`'s, so the same `after`
/// cursor means the same resume point across calls), filtered to names strictly greater than
/// `after` when given and, separately, to names starting with `prefix` when given, then packed into
/// `budget` bytes by `cost` the same way [`fill_to_budget`] packs any pool - the first candidate is
/// always included even alone over budget, so a budget too small for one item still yields that one
/// item rather than an empty page.
///
/// Returns the page plus a cursor: `Some(last included name)` once a candidate did not fit (there is
/// more to page through - call again with `after` set to it), `None` once every candidate that
/// matched `after`/`prefix` fit (the index is exhausted for that prefix). Unlike `fill_to_budget`'s
/// own omission list, no cap applies to how many PAGES this takes to exhaust an index - only to how
/// much one page may cost - so this closes the gap `fill_to_budget` alone leaves: an omission list
/// over `MAX_OMITTED_NAMES` names long still cannot name the rest, but a caller that pages with
/// `after` reaches every one of them.
pub fn paged_names<T>(
    items: impl Iterator<Item = T>,
    after: Option<&str>,
    prefix: Option<&str>,
    budget: usize,
    cost: impl Fn(&T) -> usize,
    name_of: impl Fn(&T) -> String,
) -> PagedNames<T> {
    let candidates: Vec<T> = items
        .filter(|item| prefix.is_none_or(|prefix| name_of(item).starts_with(prefix)))
        .filter(|item| after.is_none_or(|after| name_of(item).as_str() > after))
        .collect();
    if candidates.is_empty() {
        return (Vec::new(), None);
    }
    // `max_omitted: 0` - this function never names what did not fit, only reports that something
    // did not, so the omitted list `fill_to_budget` would otherwise build is pure waste here.
    let (included, _never_named, cut_off) = fill_to_budget(candidates, budget, 0, |item| cost(item), |_| ());
    let cursor = (cut_off > 0).then(|| included.last().map(&name_of)).flatten();
    (included, cursor)
}

/// A `names: true` listing's single paging cursor - ONE sequential walk, models then metrics, never
/// two independent ones.
///
/// `github.com/telekom/sutura#2083`'s review, round 3: a model-cursor-only and a metric-cursor-only
/// `after` kept producing their own edge bugs (a caller that only ever follows the metric cursor
/// could make the model half look finished when it was simply never walked; the metric half's
/// "first candidate always admitted" rule could fire on every call rather than once, overshooting
/// the bound) because the two halves could be addressed, and so paged, independently of one
/// another. One cursor naming which half AND where in it removes that independence: a caller only
/// ever holds the one cursor the previous page actually returned, there is no second `after` to
/// desynchronise from it, and "first item always admitted" (`fill_to_budget`'s own rule, shared with
/// every other caller of it in this module) can only ever fire at the one true transition from
/// models into metrics, not on a repeated call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NamesCursor {
    /// Resume the model half strictly after this model name.
    Model(String),
    /// The model half is exhausted; resume the metric half strictly after this metric name.
    Metric(String),
}

/// `after` did not parse as a [`NamesCursor`].
///
/// Neither transport invents its own prefix scheme, so a caller handed one of these needs no
/// mechanism beyond `Display`/`parse` to round-trip it, and a hand-typed or truncated value is
/// refused rather than silently read as "the beginning".
#[derive(Debug, thiserror::Error)]
#[error("`after` must be a cursor this index itself returned (`model:<name>` or `metric:<name>`)")]
pub struct MalformedNamesCursor;

impl NamesCursor {
    pub fn parse(raw: &str) -> Result<Self, MalformedNamesCursor> {
        raw.strip_prefix("model:").map_or_else(
            || {
                raw.strip_prefix("metric:")
                    .map_or(Err(MalformedNamesCursor), |name| Ok(Self::Metric(name.to_owned())))
            },
            |name| Ok(Self::Model(name.to_owned())),
        )
    }
}

impl std::fmt::Display for NamesCursor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Model(name) => write!(f, "model:{name}"),
            Self::Metric(name) => write!(f, "metric:{name}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{fill_to_budget, paged_names};

    #[test]
    fn the_first_item_is_never_omitted_even_alone_over_budget() {
        let (included, omitted, omitted_count) = fill_to_budget(vec![100_usize], 10, 10, |item| *item, usize::to_string);
        assert_eq!(included, vec![100]);
        assert_eq!(omitted, [] as [String; 0]);
        assert_eq!(omitted_count, 0);
    }

    #[test]
    fn items_past_the_budget_are_named_not_truncated() {
        let (included, omitted, omitted_count) = fill_to_budget(vec![5_usize, 5, 5], 8, 10, |item| *item, usize::to_string);
        assert_eq!(included, vec![5]);
        assert_eq!(omitted, vec!["5", "5"]);
        assert_eq!(omitted_count, 2);
    }

    #[test]
    fn the_omission_list_itself_is_capped_but_the_count_is_not() {
        let (included, omitted, omitted_count) = fill_to_budget(vec![5_usize, 1, 1, 1], 5, 1, |item| *item, usize::to_string);
        assert_eq!(included, vec![5]);
        assert_eq!(omitted, vec!["1"]);
        assert_eq!(omitted_count, 3);
    }

    fn names(count: usize) -> Vec<String> {
        (0..count).map(|index| format!("model_{index:04}")).collect()
    }

    #[test]
    fn paged_names_returns_a_cursor_only_while_more_remain() {
        let (page, cursor) = paged_names(names(5).into_iter(), None, None, 1000, |_| 10, String::clone);
        assert_eq!(page, names(5));
        assert_eq!(cursor, None, "every candidate fit - there is nothing left to page to");
    }

    #[test]
    fn paged_names_resumes_strictly_after_its_own_cursor() {
        let all = names(5);
        let (first, cursor) = paged_names(all.clone().into_iter(), None, None, 25, |_| 10, String::clone);
        assert_eq!(first, vec!["model_0000", "model_0001"]);
        let cursor = cursor.expect("two of five did not fit - a cursor must be returned");
        let (second, cursor) = paged_names(all.into_iter(), Some(&cursor), None, 25, |_| 10, String::clone);
        assert_eq!(second, vec!["model_0002", "model_0003"]);
        assert!(cursor.is_some(), "one name is still left past this page");
    }

    #[test]
    fn paged_names_walks_every_item_to_completion() {
        let all = names(450);
        let mut seen = Vec::new();
        let mut after: Option<String> = None;
        loop {
            let (page, cursor) = paged_names(all.clone().into_iter(), after.as_deref(), None, 200, |_| 10, String::clone);
            assert!(!page.is_empty(), "a page must never be empty while candidates remain");
            seen.extend(page);
            match cursor {
                Some(next) => after = Some(next),
                None => break,
            }
        }
        assert_eq!(seen, all, "paging to exhaustion must visit every item exactly once, in order");
    }

    #[test]
    fn paged_names_honours_a_prefix_filter() {
        let items = vec!["orders_a".to_owned(), "customers_a".to_owned(), "orders_b".to_owned()];
        let (page, cursor) = paged_names(items.into_iter(), None, Some("orders_"), 1000, |_| 10, String::clone);
        assert_eq!(page, vec!["orders_a", "orders_b"]);
        assert_eq!(cursor, None);
    }

    use super::NamesCursor;

    #[test]
    fn a_names_cursor_round_trips_through_its_own_display_and_parse() {
        for cursor in [
            NamesCursor::Model("orders".to_owned()),
            NamesCursor::Metric("revenue".to_owned()),
        ] {
            let rendered = cursor.to_string();
            assert_eq!(NamesCursor::parse(&rendered).expect("its own rendering parses"), cursor);
        }
    }

    #[test]
    fn a_cursor_with_neither_prefix_is_refused() {
        NamesCursor::parse("orders").unwrap_err();
        NamesCursor::parse("").unwrap_err();
    }
}
