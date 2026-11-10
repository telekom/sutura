//! One partition of a [`super::SplitLeg`] as bytes another node can hand back to
//! [`AdbcPostgres::read_partition`](super::super::AdbcPostgres::read_partition): the exported
//! snapshot and the page range, never the statement - the reading side regenerates that from the
//! leg it already holds.
//!
//! Version 1, in order: the version byte `1`; the snapshot id's length in one byte; the id's ASCII
//! bytes; `from_page` as four big-endian bytes; one byte, `0` for a range open to the end of the
//! table and `1` for a closed one; and `to_page` as four big-endian bytes after a `1` alone. Every
//! field's width is fixed or declared ahead of it, so every proper prefix of a descriptor is refused
//! as cut short - dropping the closing page cannot turn a closed range into an open one.
//!
//! **The limits**: nothing here authenticates the bytes, so a well-formed descriptor nobody minted
//! parses. Their integrity on the wire is the task grant's - `github.com/telekom/sutura#828` - and
//! what a forged one can reach is stated on `read_partition`. The snapshot id is kept out of this
//! type's `Debug`, not out of every message: a failed import is the server's error, and its text -
//! `snapshot "<id>" does not exist`, `invalid snapshot identifier: "<id>"` - quotes the id into the
//! source chain of the `PostgresError` that `read_partition` returns.

use core::fmt;

use super::super::session::{MOST_SNAPSHOT_ID, SnapshotId};
use super::Pages;

const VERSION: u8 = 1;
const OPEN: u8 = 0;
const CLOSED: u8 = 1;

/// A partition's exported snapshot and `[from, to)` page range, minted by
/// [`Partition::descriptor`](super::Partition::descriptor) and parsed from bytes by
/// `TryFrom<&[u8]>`.
///
/// There is no descriptor without a snapshot id the session would import: a split read whole
/// because it holds no snapshot has none to hand out, and bytes whose id the session refuses are
/// [`InvalidDescriptor::Snapshot`].
pub struct PartitionDescriptor {
    snapshot: SnapshotId,
    pages: Pages,
}

/// Why bytes are not a [`PartitionDescriptor`]. No variant carries the bytes themselves.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum InvalidDescriptor {
    /// A version this build does not read.
    #[error("a partition descriptor of version {found}, where this build reads version 1")]
    Version { found: u8 },
    /// The bytes end before the field they declare does.
    #[error("a partition descriptor cut short")]
    Truncated,
    /// Bytes past the last field.
    #[error("a partition descriptor with {extra} bytes past its last field")]
    Trailing { extra: usize },
    /// A snapshot id that is not one the session would import.
    #[error("a partition descriptor whose snapshot id is not one the session would import")]
    Snapshot,
    /// The byte saying whether the range closes is neither `0` nor `1`.
    #[error("a partition descriptor whose range end is marked {found}, neither open nor closed")]
    End { found: u8 },
    /// A closed range that holds no page.
    #[error("a partition descriptor whose range closes at page {to}, not after it opens at page {from}")]
    EmptyRange { from: u32, to: u32 },
}

impl PartitionDescriptor {
    /// A range `cut` produced, under the snapshot its split exported - not re-checked here.
    pub(super) const fn new(snapshot: SnapshotId, pages: Pages) -> Self {
        Self { snapshot, pages }
    }

    pub(super) const fn snapshot(&self) -> &SnapshotId {
        &self.snapshot
    }

    pub(super) const fn pages(&self) -> Pages {
        self.pages
    }

    /// The bytes `TryFrom<&[u8]>` parses back into this descriptor.
    #[must_use]
    #[expect(
        clippy::big_endian_bytes,
        reason = "a descriptor crosses hosts, so its pages are in network byte order"
    )]
    pub fn to_bytes(&self) -> Vec<u8> {
        const _: () = assert!(
            MOST_SNAPSHOT_ID <= u8::MAX as usize,
            "an admitted id's length fits its one byte"
        );
        let id = self.snapshot.as_str().as_bytes();
        let (from, to) = self.pages;
        let mut bytes = Vec::with_capacity(id.len() + 11);
        bytes.push(VERSION);
        bytes.push(u8::try_from(id.len()).unwrap_or(u8::MAX));
        bytes.extend_from_slice(id);
        bytes.extend_from_slice(&from.to_be_bytes());
        match to {
            None => bytes.push(OPEN),
            Some(to) => {
                bytes.push(CLOSED);
                bytes.extend_from_slice(&to.to_be_bytes());
            }
        }
        bytes
    }
}

impl TryFrom<&[u8]> for PartitionDescriptor {
    type Error = InvalidDescriptor;

    fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
        let (&version, rest) = bytes.split_first().ok_or(InvalidDescriptor::Truncated)?;
        if version != VERSION {
            return Err(InvalidDescriptor::Version { found: version });
        }
        let (&length, rest) = rest.split_first().ok_or(InvalidDescriptor::Truncated)?;
        let (id, rest) = rest
            .split_at_checked(usize::from(length))
            .ok_or(InvalidDescriptor::Truncated)?;
        let snapshot = core::str::from_utf8(id)
            .ok()
            .and_then(SnapshotId::parse)
            .ok_or(InvalidDescriptor::Snapshot)?;
        let (from, rest) = page(rest)?;
        let (&end, rest) = rest.split_first().ok_or(InvalidDescriptor::Truncated)?;
        let (to, rest) = match end {
            OPEN => (None, rest),
            CLOSED => page(rest).map(|(to, rest)| (Some(to), rest))?,
            found => return Err(InvalidDescriptor::End { found }),
        };
        if !rest.is_empty() {
            return Err(InvalidDescriptor::Trailing { extra: rest.len() });
        }
        if let Some(to) = to
            && to <= from
        {
            return Err(InvalidDescriptor::EmptyRange { from, to });
        }
        Ok(Self {
            snapshot,
            pages: (from, to),
        })
    }
}

/// A page number and the bytes after it.
type Page<'bytes> = Result<(u32, &'bytes [u8]), InvalidDescriptor>;

#[expect(
    clippy::big_endian_bytes,
    reason = "a descriptor crosses hosts, so its pages are in network byte order"
)]
fn page(bytes: &[u8]) -> Page<'_> {
    let (page, rest) = bytes.split_first_chunk::<4>().ok_or(InvalidDescriptor::Truncated)?;
    Ok((u32::from_be_bytes(*page), rest))
}

impl fmt::Debug for PartitionDescriptor {
    /// The page range alone. The snapshot id is a handle on another session's open transaction for
    /// as long as it lives, and `Debug` is what a log line or a panic message reaches for first.
    /// That covers this type only: an import the server refuses quotes the id in its own message,
    /// which the returned `PostgresError`'s source chain carries.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PartitionDescriptor")
            .field("from_page", &self.pages.0)
            .field("to_page", &self.pages.1)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests;
