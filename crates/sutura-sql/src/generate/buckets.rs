//! One leg, restricted to one hash bucket of its integer keys - one partition of a leg read in
//! several by a source with no page range to cut (`github.com/telekom/sutura#828`).
//!
//! **Why the integer keys, and why that is enough.** A row's bucket is a function of key values its
//! `GROUP BY` also groups by, so every row of one group lands in the same bucket, and the buckets
//! together are the whole leg with each group exactly once - whichever subset of the keys is hashed.
//! Only an integer key is hashed because only for an integer is the source's equality the same as
//! its bytes: a collation that groups `'a'` with `'A'`, or `0.0` with `-0.0`, would hash one group
//! into two buckets, and the combine would read its link twice.
//!
//! **When "each group exactly once" holds.** Only when every bucket reads one snapshot and every
//! bucket's reader hashes alike (a `DuckDB` `HASH` is not promised to be stable across versions).
//! Nothing here holds either. Under a concurrent write a group can fall in no bucket, and the lookup
//! then joins as no row: a silent NULL link, not a refusal. It can also fall in two, which the
//! combine refuses as an ambiguous link.
//!
//! **"Integer" is the source's type, not the driver's Arrow type.** The vendored `ClickHouse` driver
//! describes `Date` as `UInt16`, `DateTime` and `IPv4` as `UInt32` and an `Enum` as `Int8` or
//! `Int16`, and records the column's own type in its field metadata. A `ClickHouse` key counts as
//! an integer only when that type is one (`Int8`..`UInt64`, under any `Nullable` or
//! `LowCardinality`); a described field with no such metadata is read by its Arrow type alone. An
//! Oracle column is an Arrow integer only as `NUMBER(p, 0)` with `p` up to 18 (the vendored driver's
//! `fetch.rs`), and a `DuckDB` one only as an integer type, so neither needs the metadata.

use arrow_schema::{Field, Schema};
use polyglot_sql::builder;
use sutura_domain::plan::LegPlan;

use super::{GenerateError, key_expr, leg_within};
use crate::GeneratedQuery;
use crate::dialect::Dialect;

/// The field metadata the vendored `ClickHouse` driver records a column's own type under.
const CLICKHOUSE_TYPE: &str = "CLICKHOUSE:type";

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
    #[error(transparent)]
    Generate(#[from] GenerateError),
}

/// [`super::generate_leg`], keeping only the rows in `bucket`: those whose integer keys hash to its
/// index, a NULL key hashed as `0`.
///
/// `described` is the leg's columns as its source described them; a key is integer when its
/// column there is, by the source's own type where it states one (see the module doc). `ClickHouse`
/// hashes every integer key as `cityHash64(k1, k2, …) % count`, Oracle its first integer key as
/// `ORA_HASH(k, count - 1)` (it hashes one value, and any subset of the keys is exact), and `DuckDB`
/// as `HASH(k1, k2, …) % count`. Each key is `COALESCE(k, 0)` inside the hash, so a row whose key is
/// NULL lands in exactly one bucket whatever the dialect's hash makes of a NULL.
///
/// **Limits:** a NULL key and a `0` share a bucket, and a key of `Decimal(p, 0)` is not an integer
/// here. Each bucket is a full scan at the source, and nothing holds the buckets to one snapshot:
/// see the module doc for what that costs.
///
/// # Errors
///
/// [`BucketsRefused`]: a key not described, then no integer key, then the dialect, then the leg's
/// own rendering.
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
        if is_integer_key(field, dialect) {
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
            hashed.truncate(1);
            hashed.push(builder::lit(count.saturating_sub(1)));
            builder::func("ORA_HASH", hashed).eq(index)
        }
        Dialect::Postgres | Dialect::BigQuery => return Err(BucketsRefused::Unsupported { dialect }),
    };
    leg_within(leg, dialect, Some(within)).map_err(BucketsRefused::Generate)
}

/// Whether `field`, as its source described it, is an integer in the source's own type.
fn is_integer_key(field: &Field, dialect: Dialect) -> bool {
    field.data_type().is_integer()
        && match dialect {
            Dialect::ClickHouse => field
                .metadata()
                .get(CLICKHOUSE_TYPE)
                .is_none_or(|declared| is_clickhouse_integer(declared)),
            _ => true,
        }
}

/// A `ClickHouse` type name that is an integer, under any `Nullable` or `LowCardinality`.
fn is_clickhouse_integer(declared: &str) -> bool {
    let mut declared = declared;
    while let Some(inner) = ["Nullable(", "LowCardinality("]
        .iter()
        .find_map(|wrapper| declared.strip_prefix(wrapper))
        .and_then(|rest| rest.strip_suffix(')'))
    {
        declared = inner;
    }
    matches!(
        declared,
        "Int8" | "Int16" | "Int32" | "Int64" | "UInt8" | "UInt16" | "UInt32" | "UInt64"
    )
}

#[cfg(test)]
mod tests;
