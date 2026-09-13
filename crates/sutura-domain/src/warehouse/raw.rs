//! [`Warehouse::execute_raw`]'s own types - split out of `warehouse.rs` because that file hit the
//! thousand-line limit `cargo xtask max-lines` enforces.

use super::Value;

/// What [`Warehouse::execute_raw`] answers: `None` where the adapter does not accept raw text at
/// all, otherwise the same result [`execute`](Warehouse::execute) would have carried.
///
/// Named so the port's own signature reads as one type rather than as a shape a reader has to
/// re-derive at the call site.
pub type RawExecution<E> = Option<Result<RawRows, E>>;

/// What a raw statement's execution produced, before the application layer turns it into a
/// [`crate::raw::RawOutcome`].
///
/// A plain pair rather than a [`RowSet`]: `RowSet::new` refuses a ragged result, which is a
/// certified-answer guarantee about a plan the compiler shaped, and a raw statement's own adapter is
/// the only thing that has already checked its rows are rectangular - the driver's own row type
/// carries one value per declared column by construction. Building a `RowSet` here would ask that
/// type's constructor to re-verify a shape only the adapter could have gotten wrong.
#[derive(Debug, Clone, PartialEq)]
pub struct RawRows {
    columns: Vec<String>,
    rows: Vec<Vec<Value>>,
}

impl RawRows {
    /// The only constructor. Infallible: nothing here promises the two vectors agree in width, the
    /// way `RowSet::new` does for a certified answer - this is what
    /// `crate::raw::RawOutcome`'s builder reads that promise from before rendering; a raw statement's
    /// own adapter is what already produced a rectangular result.
    #[must_use]
    pub const fn of(columns: Vec<String>, rows: Vec<Vec<Value>>) -> Self {
        Self { columns, rows }
    }

    #[must_use]
    pub fn columns(&self) -> &[String] {
        &self.columns
    }

    #[must_use]
    pub fn rows(&self) -> &[Vec<Value>] {
        &self.rows
    }

    /// The rows, owned - for a caller that is about to render them and drop the rest.
    #[must_use]
    pub fn into_parts(self) -> RawColumnsAndRows {
        (self.columns, self.rows)
    }
}

/// The two halves [`RawRows::into_parts`] hands back: labels, then cells.
pub type RawColumnsAndRows = (Vec<String>, Vec<Vec<Value>>);
