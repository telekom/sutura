//! One leg, restricted to one hash bucket of its integer keys - one partition of a leg read in
//! several by a source with no page range to cut (`github.com/telekom/sutura#828`).
//!
//! **Why the integer keys, and why that is enough.** A row's bucket is a function of key values its
//! `GROUP BY` also groups by, so every row of one group lands in the same bucket, and the buckets
//! together are the whole leg with each group exactly once - whichever subset of the keys is hashed.
//! Only an integer key is hashed because only for an integer is the source's equality the same as
//! its bytes: a collation that groups `'a'` with `'A'`, or `0.0` with `-0.0`, would hash one group
//! into two buckets, and the combine would read its link twice.

use arrow_schema::Schema;
use polyglot_sql::builder;
use sutura_domain::plan::LegPlan;

use super::{GenerateError, key_expr, leg_within};
use crate::GeneratedQuery;
use crate::dialect::Dialect;

/// Bucket `index` of `count`: at least two buckets, and an index below the count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bucket {
    index: u32,
    count: u32,
}

/// Why a [`Bucket`] could not be built.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum InvalidBucket {
    #[error("a leg splits into two buckets or more, not {count}")]
    TooFew { count: u32 },
    #[error("bucket {index} is past the last of {count}")]
    PastTheLast { index: u32, count: u32 },
}

impl Bucket {
    /// Bucket `index` of `count`.
    ///
    /// # Errors
    ///
    /// [`InvalidBucket::TooFew`] below two buckets, then [`InvalidBucket::PastTheLast`] for an
    /// `index` at or past `count`.
    pub const fn new(index: u32, count: u32) -> Result<Self, InvalidBucket> {
        if count < 2 {
            return Err(InvalidBucket::TooFew { count });
        }
        if index >= count {
            return Err(InvalidBucket::PastTheLast { index, count });
        }
        Ok(Self { index, count })
    }

    /// Which bucket, from `0`.
    #[inline]
    #[must_use]
    pub const fn index(self) -> u32 {
        self.index
    }

    /// How many buckets the leg is split into.
    #[inline]
    #[must_use]
    pub const fn count(self) -> u32 {
        self.count
    }
}

/// Why a leg was not split into hash buckets.
#[derive(Debug, thiserror::Error)]
pub enum BucketsRefused {
    /// No hash here for this dialect: `Postgres` cuts page ranges and `BigQuery` read streams.
    #[error("a leg is not split into hash buckets for {dialect}")]
    Unsupported { dialect: Dialect },
    /// A key of the leg its source did not describe, so its type is unknown.
    #[error("the leg's key {label:?} is not among the columns its source described")]
    KeyNotDescribed { label: String },
    /// No key of the leg is an integer, so no hash agrees with how the source groups it.
    #[error("no key of the leg is an integer, so it has no key to hash into buckets")]
    NoIntegerKey,
    /// More than one integer key, for a dialect whose hash takes one value.
    #[error("{dialect} hashes one value, and the leg has {keys} integer keys")]
    CompoundKey { dialect: Dialect, keys: usize },
    #[error(transparent)]
    Generate(#[from] GenerateError),
}

/// [`super::generate_leg`], keeping only the rows in `bucket`: those whose integer keys hash to its
/// index, a NULL key hashed as `0`.
///
/// `described` is the leg's columns as its source described them; a key is integer when its
/// column there is. `ClickHouse` hashes every integer key as `cityHash64(k1, k2, …) % count`, Oracle
/// its one integer key as `ORA_HASH(k, count - 1)`, and `DuckDB` as `HASH(k1, k2, …) % count`. Each
/// key is `COALESCE(k, 0)` inside the hash, so a row whose key is NULL lands in exactly one bucket
/// whatever the dialect's hash makes of a NULL.
///
/// **Limits:** a NULL key and a `0` share a bucket, and a key of `Decimal(p, 0)` is not an integer
/// here. Each bucket is a full scan at the source, and nothing holds the buckets to one snapshot.
///
/// # Errors
///
/// [`BucketsRefused`]: a key not described, then no integer key, then the dialect or two integer
/// keys for Oracle, then the leg's own rendering.
pub fn generate_leg_buckets(
    leg: &LegPlan,
    described: &Schema,
    bucket: Bucket,
    dialect: Dialect,
) -> Result<GeneratedQuery, BucketsRefused> {
    let mut hashed = Vec::with_capacity(leg.keys().len());
    for key in leg.keys() {
        let field = described
            .field_with_name(key.label())
            .map_err(|_absent| BucketsRefused::KeyNotDescribed {
                label: String::from(key.label()),
            })?;
        if field.data_type().is_integer() {
            hashed.push(builder::coalesce([key_expr(key, dialect), builder::lit(0_i64)]));
        }
    }
    if hashed.is_empty() {
        return Err(BucketsRefused::NoIntegerKey);
    }
    let index = builder::lit(i64::from(bucket.index()));
    let count = i64::from(bucket.count());
    let within = match dialect {
        Dialect::ClickHouse => builder::func("cityHash64", hashed).modulo(builder::lit(count)).eq(index),
        Dialect::DuckDb => builder::func("HASH", hashed).modulo(builder::lit(count)).eq(index),
        Dialect::Oracle => {
            if hashed.len() > 1 {
                return Err(BucketsRefused::CompoundKey {
                    dialect,
                    keys: hashed.len(),
                });
            }
            hashed.push(builder::lit(count.saturating_sub(1)));
            builder::func("ORA_HASH", hashed).eq(index)
        }
        Dialect::Postgres | Dialect::BigQuery => return Err(BucketsRefused::Unsupported { dialect }),
    };
    leg_within(leg, dialect, Some(within)).map_err(BucketsRefused::Generate)
}

#[cfg(test)]
mod tests;
