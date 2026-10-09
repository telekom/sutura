//! The cursor accounting both catalog readers' paged reads share.
//!
//! A reader asks a page, reads its envelope and hands [`Pager::advance`] what the page said about
//! the rest. The pager answers "fetch [`Pager::cursor`] next" or "complete", or refuses the read.
//! A read is therefore whole or refused: it is never cut short and never ends on a partial list.

use std::collections::HashSet;

/// The recommended page size: how many entities one request asks for.
pub const DEFAULT_PAGE_SIZE: usize = 1000;

/// The bound on the entities one entity kind may return across all its pages.
///
/// A catalog of 100,000 tables is the size a deployment is built for; this is that, so a read above
/// it is refused ([`PagingRefusal::TooManyEntities`]) rather than held in memory.
pub const DEFAULT_MAX_ENTITIES: usize = 100_000;

/// Why a declared limit is not usable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum InvalidPageLimits {
    /// Zero would refuse every read rather than bounding one.
    #[error("a {what} of zero would refuse every read rather than bounding one")]
    Zero { what: &'static str },
}

/// What one entity kind's paged read may ask for and may collect.
///
/// Constants by default, a code-level parameter and never a settings key: a deployment's request
/// timeout and byte cap are settings, the size of what a catalog may hold is not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageLimits {
    page_size: usize,
    max_entities: usize,
}

impl PageLimits {
    /// [`DEFAULT_PAGE_SIZE`] and [`DEFAULT_MAX_ENTITIES`]; a `const` so a `const fn` constructor can hold it.
    pub const DEFAULT: Self = Self {
        page_size: DEFAULT_PAGE_SIZE,
        max_entities: DEFAULT_MAX_ENTITIES,
    };

    /// Parses a page size and an entity bound, refusing either at zero.
    pub const fn parse(page_size: usize, max_entities: usize) -> Result<Self, InvalidPageLimits> {
        if page_size == 0 {
            return Err(InvalidPageLimits::Zero { what: "page size" });
        }
        if max_entities == 0 {
            return Err(InvalidPageLimits::Zero { what: "entity bound" });
        }
        Ok(Self { page_size, max_entities })
    }

    #[inline]
    #[must_use]
    pub const fn page_size(self) -> usize {
        self.page_size
    }

    #[inline]
    #[must_use]
    pub const fn max_entities(self) -> usize {
        self.max_entities
    }
}

impl Default for PageLimits {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Why a paged read was refused. Never carries a cursor: it is endpoint-owned text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PagingRefusal {
    /// The service handed back a cursor it had already handed back, so following it never ends.
    #[error("the service repeated a cursor it had already given")]
    RepeatedCursor,
    /// A page held no entity and still reported more, so following it never ends.
    #[error("a page held no entity while it reported more")]
    NoProgress,
    /// The entities read passed the bound on one entity kind.
    #[error("more than the {max}-entity bound on one entity kind was read")]
    TooManyEntities { max: usize },
    /// The last page ended the list short of the total the service reported.
    #[error("the list ended after {read} entities, short of the {total} the service reported")]
    ShortOfTotal { read: usize, total: u64 },
}

/// What one page said about the rest of its list.
#[derive(Debug, Clone, Copy)]
pub struct PageReport<'page> {
    returned: usize,
    next: Option<&'page str>,
    total: Option<u64>,
}

impl<'page> PageReport<'page> {
    /// `returned` is how many entities the page held, `next` the cursor of the next page (`None` on
    /// the last) and `total` the size the service reports for the whole list, when it reports one.
    #[must_use]
    pub const fn new(returned: usize, next: Option<&'page str>, total: Option<u64>) -> Self {
        Self { returned, next, total }
    }
}

/// One entity kind's read in progress: the cursor to ask for, the cursors already followed and how
/// many entities have arrived.
#[derive(Debug)]
pub struct Pager {
    limits: PageLimits,
    cursor: Option<String>,
    followed: HashSet<String>,
    read: usize,
}

impl Pager {
    #[must_use]
    pub fn new(limits: PageLimits) -> Self {
        Self {
            limits,
            cursor: None,
            followed: HashSet::new(),
            read: 0,
        }
    }

    /// The cursor the next request carries; `None` for the first page.
    #[must_use]
    pub fn cursor(&self) -> Option<&str> {
        self.cursor.as_deref()
    }

    #[must_use]
    pub const fn page_size(&self) -> usize {
        self.limits.page_size
    }

    /// Accounts for one page. `Ok(true)` asks for [`Self::cursor`] next, `Ok(false)` ends the list.
    ///
    /// # Errors
    ///
    /// The read can no longer be whole: see [`PagingRefusal`].
    pub fn advance(&mut self, page: PageReport<'_>) -> Result<bool, PagingRefusal> {
        self.read = self.read.saturating_add(page.returned);
        if self.read > self.limits.max_entities {
            return Err(PagingRefusal::TooManyEntities {
                max: self.limits.max_entities,
            });
        }
        let Some(next) = page.next else {
            return match page.total {
                Some(total) if (self.read as u64) < total => Err(PagingRefusal::ShortOfTotal { read: self.read, total }),
                _ => Ok(false),
            };
        };
        if page.returned == 0 {
            return Err(PagingRefusal::NoProgress);
        }
        if self.cursor.as_deref() == Some(next) || self.followed.contains(next) {
            return Err(PagingRefusal::RepeatedCursor);
        }
        if let Some(spent) = self.cursor.replace(next.to_owned()) {
            self.followed.insert(spent);
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::{InvalidPageLimits, PageLimits, PageReport, Pager, PagingRefusal};

    fn limits(page_size: usize, max_entities: usize) -> PageLimits {
        PageLimits::parse(page_size, max_entities).expect("a nonzero page size and bound parse")
    }

    fn page(returned: usize, next: Option<&str>, total: Option<u64>) -> PageReport<'_> {
        PageReport::new(returned, next, total)
    }

    #[test]
    fn the_defaults_hold_a_catalog_of_one_hundred_thousand() {
        assert!(PageLimits::default().max_entities() >= 100_000);
    }

    #[test]
    fn a_zero_limit_is_refused() {
        assert_eq!(PageLimits::parse(0, 10), Err(InvalidPageLimits::Zero { what: "page size" }));
        assert_eq!(
            PageLimits::parse(10, 0),
            Err(InvalidPageLimits::Zero { what: "entity bound" })
        );
    }

    #[test]
    fn each_cursor_is_followed_until_a_page_names_none() {
        let mut pager = Pager::new(limits(2, 10));
        assert_eq!(pager.cursor(), None);
        assert_eq!(pager.advance(page(2, Some("a"), Some(5))), Ok(true));
        assert_eq!(pager.cursor(), Some("a"));
        assert_eq!(pager.advance(page(2, Some("b"), Some(5))), Ok(true));
        assert_eq!(pager.cursor(), Some("b"));
        assert_eq!(pager.advance(page(1, None, Some(5))), Ok(false));
    }

    #[test]
    fn a_repeated_cursor_is_refused_whether_it_is_the_last_or_an_earlier_one() {
        let mut last = Pager::new(limits(2, 10));
        assert_eq!(last.advance(page(2, Some("a"), None)), Ok(true));
        assert_eq!(last.advance(page(2, Some("a"), None)), Err(PagingRefusal::RepeatedCursor));

        let mut earlier = Pager::new(limits(2, 10));
        assert_eq!(earlier.advance(page(2, Some("a"), None)), Ok(true));
        assert_eq!(earlier.advance(page(2, Some("b"), None)), Ok(true));
        assert_eq!(earlier.advance(page(2, Some("a"), None)), Err(PagingRefusal::RepeatedCursor));
    }

    #[test]
    fn an_empty_page_that_reports_more_is_refused_and_an_empty_last_page_is_not() {
        let mut more = Pager::new(limits(2, 10));
        assert_eq!(more.advance(page(0, Some("a"), None)), Err(PagingRefusal::NoProgress));

        let mut last = Pager::new(limits(2, 10));
        assert_eq!(last.advance(page(2, Some("a"), None)), Ok(true));
        assert_eq!(last.advance(page(0, None, None)), Ok(false));
    }

    #[test]
    fn the_bound_admits_exactly_its_count_and_refuses_one_more() {
        let mut at = Pager::new(limits(2, 4));
        assert_eq!(at.advance(page(2, Some("a"), None)), Ok(true));
        assert_eq!(at.advance(page(2, None, None)), Ok(false));

        let mut over = Pager::new(limits(2, 4));
        assert_eq!(over.advance(page(2, Some("a"), None)), Ok(true));
        assert_eq!(over.advance(page(2, Some("b"), None)), Ok(true));
        assert_eq!(
            over.advance(page(1, None, None)),
            Err(PagingRefusal::TooManyEntities { max: 4 })
        );
    }

    #[test]
    fn a_last_page_short_of_the_reported_total_is_refused() {
        let mut pager = Pager::new(limits(2, 10));
        assert_eq!(
            pager.advance(page(2, None, Some(3))),
            Err(PagingRefusal::ShortOfTotal { read: 2, total: 3 })
        );
    }
}
