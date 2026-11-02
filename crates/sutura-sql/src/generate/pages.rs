//! One leg, restricted to a range of its root table's heap pages - one partition of a leg read in
//! several (`github.com/telekom/sutura#828`).
//!
//! **Why here and not in the adapter:** the range has to be a conjunct INSIDE the leg's statement,
//! ahead of its `GROUP BY`, and a leg's filters cannot carry it - each is a typed predicate over a
//! declared column with a bound parameter, and a parameter arrives as `text`, which Postgres has no
//! `tid` comparison for. Splicing text into a rendered statement is the thing this crate exists to
//! make unnecessary.

use polyglot_sql::builder;
use sutura_domain::plan::LegPlan;

use super::{GenerateError, leg_within};
use crate::GeneratedQuery;
use crate::dialect::Dialect;

/// [`super::generate_leg`] at [`Dialect::Postgres`], keeping only the rows on heap pages
/// `from_page` up to `to_page`, or to the end of the table where `to_page` is `None`.
///
/// The range is `ctid >= '(from_page,0)' AND ctid < '(to_page,0)'`, each half left out where it would
/// hold every row, so `(0, None)` renders exactly [`super::generate_leg`]'s statement. The literals are
/// rendered from two integers, never from a caller's text; a bound parameter is not an option, for the
/// reason the module header gives. Postgres 14 and later can answer the range with a TID range scan
/// rather than a full one, and an older server filters a full scan to the same rows; which scan the
/// planner picks is not observed by any test.
///
/// **The limit**: this is the predicate on the leg's root table alone. A row of a joined table is
/// kept or dropped with the root row it joins to, and a lookup leg's distinct key set is distinct
/// per range, so a key that sits on pages of two ranges comes back once from each.
pub fn generate_leg_pages(leg: &LegPlan, from_page: u32, to_page: Option<u32>) -> Result<GeneratedQuery, GenerateError> {
    let ctid = || builder::col(&format!("{}.ctid", leg.table_name().as_str()));
    let from = (from_page > 0).then(|| ctid().gte(builder::lit(format!("({from_page},0)"))));
    let to = to_page.map(|page| ctid().lt(builder::lit(format!("({page},0)"))));
    let range = match (from, to) {
        (Some(from), Some(to)) => Some(from.and(to)),
        (from, to) => from.or(to),
    };
    leg_within(leg, Dialect::Postgres, range)
}
