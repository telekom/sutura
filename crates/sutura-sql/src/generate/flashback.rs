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

use core::num::NonZeroU64;

use polyglot_sql::builder::{self, Expr};
use polyglot_sql::expressions::{Expression, Literal, Paren, Var, Version};
use sutura_domain::model::{QualifiedTable, SourceName};
use sutura_domain::plan::LegPlan;

use super::leg::{finish, leg_statement};
use super::{GenerateError, key_expr, render, table_path};
use crate::GeneratedQuery;
use crate::dialect::Dialect;

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

    /// The number as a literal, for a statement or a function argument.
    fn literal(self) -> Expression {
        Expression::Literal(Box::new(Literal::Number(self.get().to_string())))
    }

    /// `AS OF SCN <scn>` on a table reference.
    fn version(self) -> Version {
        Version {
            this: Box::new(Expression::Var(Box::new(Var {
                this: String::from("SCN"),
            }))),
            kind: String::from("AS OF"),
            expression: Some(Box::new(self.literal())),
        }
    }
}

/// Bucket `index` of `count`, of the leg's key at position `key`: at least two buckets, and an
/// index below the count.
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
    /// # Errors
    ///
    /// [`InvalidKeyBucket::TooFew`] below two buckets, then [`InvalidKeyBucket::PastTheLast`] for an
    /// `index` at or past `count`.
    pub const fn new(key: u16, index: u32, count: u32) -> Result<Self, InvalidKeyBucket> {
        if count < 2 {
            return Err(InvalidKeyBucket::TooFew { count });
        }
        if index >= count {
            return Err(InvalidKeyBucket::PastTheLast { index, count });
        }
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

/// [`super::generate_leg`] at [`Dialect::Oracle`], every table read `AS OF SCN` `scn`, keeping only
/// the rows of `bucket` where there is one.
///
/// Bucket 0 is `(k IS NULL OR ORA_HASH(k, count - 1) = 0)`, and bucket `i` past it
/// `(k IS NOT NULL AND ORA_HASH(k, count - 1) = i)`, joined by `AND` after the leg's own filters, so their
/// parameter order holds.
///
/// **Limits:** the buckets are exact for a lookup leg only - a fact leg's partial terms from two
/// buckets are not combined here. `ORA_HASH` buckets are not uniform, and every NULL key lands in
/// bucket 0. Each bucket reads the whole root table.
///
/// # Errors
///
/// [`AsOfRefused::NoKey`] for a bucket past the leg's keys, then the leg's own rendering.
pub fn generate_leg_as_of(leg: &LegPlan, scn: Scn, bucket: Option<KeyBucket>) -> Result<GeneratedQuery, AsOfRefused> {
    let within = bucket.map(|bucket| within(leg, bucket)).transpose()?;
    let mut ast = leg_statement(leg, Dialect::Oracle, within)?;
    let tables = leg.tables().joins().len().saturating_add(1);
    let stamped = stamp(&mut ast, scn);
    if stamped != tables {
        return Err(AsOfRefused::Unstamped { stamped, tables });
    }
    Ok(finish(leg, Dialect::Oracle, &ast)?)
}

/// `SELECT 1 FROM <table> AS OF SCN <scn> WHERE 1 = 0`: reads no row, and is refused where the
/// session may not read `table` at a snapshot.
///
/// That Oracle refuses this zero-row read without a `FLASHBACK` grant is read from its
/// documentation, not measured by any cell here.
///
/// # Errors
///
/// [`AsOfRefused::Generate`] for a table path Oracle does not resolve.
pub fn generate_table_as_of(source: &SourceName, table: &QualifiedTable, scn: Scn) -> Result<GeneratedQuery, AsOfRefused> {
    let mut ast = builder::select([builder::lit(1_i64)])
        .from(&table_path(table, Dialect::Oracle)?)
        .where_(builder::lit(1_i64).eq(builder::lit(0_i64)))
        .build();
    let stamped = stamp(&mut ast, scn);
    if stamped != 1 {
        return Err(AsOfRefused::Unstamped { stamped, tables: 1 });
    }
    let sql = render(&ast, Dialect::Oracle)?;
    Ok(GeneratedQuery::literal(source.clone(), sql))
}

/// The predicate keeping `bucket`'s rows of `leg`.
fn within(leg: &LegPlan, bucket: KeyBucket) -> Result<Expr, AsOfRefused> {
    let key = leg.keys().get(usize::from(bucket.key())).ok_or_else(|| AsOfRefused::NoKey {
        position: bucket.key(),
        keys: leg.keys().len(),
    })?;
    let value = || key_expr(key, Dialect::Oracle);
    let last = builder::lit(i64::from(bucket.count().saturating_sub(1)));
    let hashed = builder::func("ORA_HASH", [value(), last]).eq(builder::lit(i64::from(bucket.index())));
    let kept = if bucket.index() == 0 {
        value().is_null().or(hashed)
    } else {
        value().is_not_null().and(hashed)
    };
    Ok(Expr(Expression::Paren(Box::new(Paren {
        this: kept.into_inner(),
        trailing_comments: Vec::new(),
    }))))
}

/// Every table reference of `ast`'s `FROM` and `JOIN`s read `AS OF SCN` `scn`; answers how many.
fn stamp(ast: &mut Expression, scn: Scn) -> usize {
    let Expression::Select(ref mut select) = *ast else {
        return 0;
    };
    let select = &mut **select;
    let from = select.from.iter_mut().flat_map(|from| from.expressions.iter_mut());
    let joins = select.joins.iter_mut().map(|join| &mut join.this);
    let mut stamped = 0_usize;
    for reference in from.chain(joins) {
        if let Expression::Table(ref mut table) = *reference {
            table.version = Some(Box::new(scn.version()));
            stamped = stamped.saturating_add(1);
        }
    }
    stamped
}

#[cfg(test)]
mod tests;
