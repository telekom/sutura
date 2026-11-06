//! One leg read at one Oracle snapshot, and one hash bucket of it - a partition of an Oracle lookup
//! leg (`github.com/telekom/sutura#828`).
//!
//! Every table reference of the statement, the root and each join, carries `AS OF SCN <scn>`, so
//! the statement reads one snapshot whatever session runs it. The SCN, the bucket index and the
//! bucket count are rendered from integers, never from a caller's text.
//!
//! **Why a bucket is exact for a lookup leg:** the hashed key is one of the leg's `GROUP BY` keys,
//! so every row of one group hashes alike and every group lands in one bucket. A row whose key is
//! NULL is kept by bucket 0 alone, whatever `ORA_HASH` makes of a NULL - Oracle's reference does not
//! say.
//!
//! **Not built yet:** every statement here is refused as [`AsOfRefused::Unstamped`], and a
//! [`KeyBucket`] is not checked against its count.

use core::num::NonZeroU64;

use sutura_domain::model::{QualifiedTable, SourceName};
use sutura_domain::plan::LegPlan;

use super::GenerateError;
use crate::GeneratedQuery;

/// An Oracle system change number. Never zero: no snapshot is taken at it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Scn(NonZeroU64);

impl Scn {
    /// `scn`, or `None` for zero.
    #[must_use]
    #[inline]
    pub const fn parse(scn: u64) -> Option<Self> {
        match NonZeroU64::new(scn) {
            Some(scn) => Some(Self(scn)),
            None => None,
        }
    }

    /// The number.
    #[must_use]
    #[inline]
    pub const fn get(self) -> u64 {
        self.0.get()
    }
}

/// Bucket `index` of `count`, of the leg's key at position `key`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyBucket {
    key: u16,
    index: u32,
    count: u32,
}

/// Why a [`KeyBucket`] could not be built.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum InvalidKeyBucket {
    #[error("a leg splits into two buckets or more, not {count}")]
    TooFew { count: u32 },
    #[error("bucket {index} is past the last of {count}")]
    PastTheLast { index: u32, count: u32 },
}

impl KeyBucket {
    /// Bucket `index` of `count` of the key at position `key`.
    ///
    /// **Not built yet:** the count and the index are not checked.
    ///
    /// # Errors
    ///
    /// None yet; [`InvalidKeyBucket`] names the refusals the check will make.
    #[expect(
        clippy::unnecessary_wraps,
        reason = "the bounds check is what refuses, and it lands with the partition read"
    )]
    pub const fn new(key: u16, index: u32, count: u32) -> Result<Self, InvalidKeyBucket> {
        Ok(Self { key, index, count })
    }

    /// The hashed key's position among the leg's keys.
    #[must_use]
    #[inline]
    pub const fn key(self) -> u16 {
        self.key
    }

    /// Which bucket, from `0`.
    #[must_use]
    #[inline]
    pub const fn index(self) -> u32 {
        self.index
    }

    /// How many buckets the leg is split into.
    #[must_use]
    #[inline]
    pub const fn count(self) -> u32 {
        self.count
    }
}

/// Why a leg was not rendered at a snapshot.
#[derive(Debug, thiserror::Error)]
pub enum AsOfRefused {
    /// The bucket names a key position the leg has no key at.
    #[error("the bucket hashes the key at position {position}, and the leg has {keys} keys")]
    NoKey { position: u16, keys: usize },
    /// A table reference the statement holds did not take the snapshot, so it would read the
    /// table as it is now. A defect in this module, never anything about the leg.
    #[error("{stamped} of the statement's {tables} table references carry the snapshot")]
    Unstamped { stamped: usize, tables: usize },
    #[error(transparent)]
    Generate(#[from] GenerateError),
}

/// [`super::generate_leg`] at Oracle, every table read `AS OF SCN` `scn`, keeping only the rows of
/// `bucket` where there is one.
///
/// **Not built yet:** no table reference takes the snapshot, so every leg is refused.
///
/// # Errors
///
/// [`AsOfRefused::Unstamped`], for every leg.
pub fn generate_leg_as_of(leg: &LegPlan, _scn: Scn, _bucket: Option<KeyBucket>) -> Result<GeneratedQuery, AsOfRefused> {
    Err(AsOfRefused::Unstamped {
        stamped: 0,
        tables: leg.tables().joins().len().saturating_add(1),
    })
}

/// `SELECT 1 FROM <table> AS OF SCN <scn> WHERE 1 = 0`: reads no row, and is refused where the
/// session may not read `table` at a snapshot.
///
/// **Not built yet:** the table reference does not take the snapshot, so every probe is refused.
///
/// # Errors
///
/// [`AsOfRefused::Unstamped`], for every table.
pub const fn generate_table_as_of(
    _source: &SourceName,
    _table: &QualifiedTable,
    _scn: Scn,
) -> Result<GeneratedQuery, AsOfRefused> {
    Err(AsOfRefused::Unstamped { stamped: 0, tables: 1 })
}

#[cfg(test)]
mod tests;
