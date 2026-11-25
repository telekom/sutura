//! A shared "include whole items up to a byte budget, name the rest" selector.
//!
//! The same skip-and-continue shape is needed in four places that do not share a crate: the MCP
//! system prompt's physical-schema section (`prompt::physical_schema`), `sutura-mcp`'s
//! `describe_catalog` result (both its metrics and its physical models), and `sutura-http`'s
//! `CatalogBody`. One definition here, rather than four hand-rolled loops that drift apart.

/// `fill_to_budget`'s result: the items it kept whole, the names of the first `max_omitted` it left
/// out, and the true count left out (which may exceed the name list's length).
pub type Budgeted<T> = (Vec<T>, Vec<String>, usize);

/// Packs `items` into `budget` bytes of `cost`, in iteration order.
///
/// The first item is always included, even alone over budget - a budget no single item fits under
/// must never yield an empty result, which would read as "this deployment defines nothing" rather
/// than "ask for one by name". Every item after that is included only while the running total still
/// fits; an item that does not fit is never truncated - it is named, by `name`, in the second
/// returned list. Only the first `max_omitted` such names are kept, so the omission list is itself
/// bounded; the third return value is the TRUE number omitted, which can exceed the name list's
/// length once that cap is reached.
pub fn fill_to_budget<T>(
    items: impl IntoIterator<Item = T>,
    budget: usize,
    max_omitted: usize,
    mut cost: impl FnMut(&T) -> usize,
    mut name: impl FnMut(&T) -> String,
) -> Budgeted<T> {
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
/// `github.com/telekom/sutura#2071`'s second ask measured 240538 bytes of authored model
/// definitions as small enough for the planner and not small enough for an agent reading it as one
/// prompt section or one tool result. 64 KiB is comfortably under that measured case (about 3.7x)
/// without being so tight that an ordinarily authored catalog ever brushes it - a listing that size
/// is itself the signal to read it one item at a time rather than in full.
///
/// **The limit:** each of the call sites above spends this budget on its OWN pool of items
/// (metrics, or physical models), not on one shared total across every section of one result -
/// combining them into a single running budget would need the metrics and the models to be walked
/// in one interleaved order, which none of these renderers do. A result whose metrics alone and
/// whose models alone each stay under 64 KiB can therefore still exceed 128 KiB combined; narrowing
/// to one model keeps both pools small in the ordinary case.
pub const MAX_CATALOG_LISTING_BYTES: usize = 64 * 1024;

/// How many omitted names a listing keeps before it stops naming them and states the count instead.
pub const MAX_OMITTED_NAMES: usize = 200;

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
