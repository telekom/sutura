//! A result set, and the rows one anchor's plan produced at boot.
//!
//! Its own module because the shape of a result is a concept of its own: the ragged-row refusal,
//! the one-cell read an anchor makes, and the wrapper that stops a boot result being handed back as
//! an answer. **What it does NOT hold: any decision about who ran the query** - that belongs to the
//! port in [`crate::warehouse`].
//!
//! `pub mod` for [`cell`](crate::warehouse::cell)'s reason, with the same re-export beside it.

use super::cell::Value;

/// A result set: the column labels, and the rows.
///
/// Labels are `String` rather than [`crate::model::ColumnName`] because a generated projection names
/// things a model did not - the truncated time bucket, and the measure under the metric's own name -
/// so constraining them would mean lying about what they are or refusing to name them.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct RowSet {
    columns: Vec<String>,
    rows: Vec<Vec<Value>>,
}

/// Why a result set could not be built.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum MalformedRowSet {
    /// A row has a different number of cells than there are columns.
    ///
    /// Checked once here rather than trusted: every consumer downstream indexes by column position,
    /// and under the `indexing_slicing` ban each would otherwise need its own fallback for a case
    /// that must not exist.
    #[error("row {row} has {cells} cells, and there are {columns} columns")]
    RowWidth { row: usize, cells: usize, columns: usize },
}

impl RowSet {
    /// Builds a result set, rejecting a ragged one.
    pub fn new(columns: Vec<String>, rows: Vec<Vec<Value>>) -> Result<Self, MalformedRowSet> {
        for (index, row) in rows.iter().enumerate() {
            if row.len() != columns.len() {
                return Err(MalformedRowSet::RowWidth {
                    row: index,
                    cells: row.len(),
                    columns: columns.len(),
                });
            }
        }
        Ok(Self { columns, rows })
    }

    #[inline]
    pub fn columns(&self) -> &[String] {
        &self.columns
    }

    #[inline]
    pub fn rows(&self) -> &[Vec<Value>] {
        &self.rows
    }

    /// Where a column with this label sits, if there is exactly one.
    ///
    /// `None` for a label that appears twice, not the first match: two columns under one label means
    /// the projection is not what we think, and returning either answers with a number from a column
    /// nobody chose. `Definitions::assemble` refuses the catalog shapes that cause it, so this is the
    /// second line rather than the first.
    pub fn column_index(&self, label: &str) -> Option<usize> {
        let mut found = None;
        for (index, name) in self.columns.iter().enumerate() {
            if name == label {
                if found.is_some() {
                    return None;
                }
                found = Some(index);
            }
        }
        found
    }

    /// One cell, by row and column position.
    ///
    /// `Option` rather than indexing: `indexing_slicing` is denied for library crates here, and a
    /// caller holding a position from `column_index` still must not be able to panic on a result set
    /// that came back a different shape.
    pub fn cell(&self, row: usize, column: usize) -> Option<&Value> {
        let cells = self.rows.get(row)?;
        cells.get(column)
    }

    /// The single cell of a single-row, single-column result, which is what an anchor check reads.
    ///
    /// `None` for any other shape rather than a panic or a silent first cell: an anchor query coming
    /// back with three rows means the statement is not the one we thought, and reading its first cell
    /// would turn that into a wrong number.
    pub const fn scalar(&self) -> Option<&Value> {
        match (self.columns.as_slice(), self.rows.as_slice()) {
            ([_], [row]) => match row.as_slice() {
                [cell] => Some(cell),
                _ => None,
            },
            _ => None,
        }
    }

    /// The total bytes every cell would occupy once rendered (via
    /// [`Value::rendered_len`](crate::warehouse::cell::Value::rendered_len), so a wide `Text` cell
    /// is counted rather than cloned) - a proxy for the encoded response size, not the wire size:
    /// the same canonical text an anchor compares against, not the JSON or tab-delimited bytes a
    /// transport wraps it in. Column labels are not counted.
    pub fn rendered_byte_len(&self) -> u64 {
        self.rows.iter().flatten().map(|cell| cell.rendered_len() as u64).sum()
    }
}

/// The rows one anchor's plan produced at boot.
///
/// **A wrapper with a private field, so a boot result cannot be handed back as an answer without a
/// named conversion somebody wrote.** The anchor path and the request path are two ways into a data
/// system running as different identities: `execute` takes the asking subject's credential and
/// cannot be called without one, while
/// [`Warehouse::verify_anchor`](crate::warehouse::Warehouse::verify_anchor) takes none at all and
/// runs as
/// whatever identity the deployment configured - `docs/adr/0008` part 1 decides that for a path with
/// no caller. Two types rather than one so the separation is visible at a call site rather than in a
/// comment, and [`Self::verified_at_boot`] is named to be conspicuous in review and in a grep, the
/// way `crate::identity::Secret::expose_secret` is.
#[derive(Debug, Clone, PartialEq)]
pub struct AnchorRows(RowSet);

impl AnchorRows {
    /// What an adapter returns from a verification run.
    #[inline]
    #[must_use]
    pub const fn of(rows: RowSet) -> Self {
        Self(rows)
    }

    /// The rows, for the boot path that compares them against what an author certified.
    #[inline]
    #[must_use]
    pub const fn verified_at_boot(&self) -> &RowSet {
        &self.0
    }
}
