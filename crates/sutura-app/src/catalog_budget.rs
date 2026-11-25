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
/// instead of a pre-formatted string. Whatever `name` returns, [`omission_reserve`] must be told how
/// many identifiers worth of bytes that shape costs, so the budget handed in already set enough
/// aside for a full omission list of that shape - this function itself does not know or check that.
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
/// (structured) the `omitted_*_more` field's own key and digits. One generous constant rather than a
/// measurement of each call site's own wording: that wording is free to change without this reserve
/// drifting out from under it, and every call site's actual header is comfortably under it, which
/// the over-budget total-size tests in each call site's module hold to directly.
const OMISSION_HEADER_RESERVE: usize = 512;

/// Fixed bytes one omitted entry costs beyond its name(s): a separator in a text list (`", "`), or -
/// structured - the braces, quotes and field-name keys around an object's identifiers.
const OMISSION_ENTRY_OVERHEAD: usize = 32;

/// Worst-case total bytes a full omission list can cost.
///
/// [`MAX_OMITTED_NAMES`] entries, each up to `names_per_entry` identifiers of
/// [`sutura_domain::model::MAX_IDENTIFIER_LEN`] bytes - 2 for a metric entry that also carries its
/// model, 1 for a model named alone - plus the list's own fixed scaffolding.
///
/// **Reserved from a pool's budget BEFORE `fill_to_budget` is called, never after.**
/// `fill_to_budget` only learns an omission list's real size once it has decided what did not fit,
/// so a budget that did not set this aside up front could let included content plus the note
/// describing what got left out cross [`MAX_CATALOG_LISTING_BYTES`] together, even with the
/// included content alone safely under it - which is exactly how a result could overshoot this
/// bound by tens of kilobytes before this reserve existed (`github.com/telekom/sutura#2071`).
#[must_use]
pub const fn omission_reserve(names_per_entry: usize) -> usize {
    OMISSION_HEADER_RESERVE
        + MAX_OMITTED_NAMES * (names_per_entry * sutura_domain::model::MAX_IDENTIFIER_LEN + OMISSION_ENTRY_OVERHEAD)
}

#[cfg(test)]
mod tests {
    use super::fill_to_budget;

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
}
