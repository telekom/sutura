//! A per-column acknowledgement of secret-dependent raw-search work.

use super::Collation;
use crate::source::{AcknowledgementReason, InvalidOperatorText};
use crate::warehouse::schema::TypeDialect;

/// The source engine families that can carry the raw necessary condition.
///
/// The one place that decides it: another source may bind a linguistic collation to a column, so an
/// engine is added here only with comparison evidence, and the exhaustive match in [`Self::of`] makes
/// a new dialect a compile error until someone decides.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrefilterCapability {
    DataFusion,
    /// A CLOB column read under a binary session sort.
    Oracle,
    /// A column pinned [`Collation::Binary`](super::Collation::Binary), read from the dictionary (the
    /// deterministic database default, `pg_catalog.C` or `pg_catalog.POSIX`) or authored in a local
    /// catalog document. `LIKE` compares a deterministic collation character for character; a
    /// nondeterministic one is refused by `LIKE` before PostgreSQL 18 and is looser than the
    /// replacement proof from 18 on.
    Postgres,
    /// No column has a collation, and `LIKE` is a plain substring search for the admitted ASCII-alphanumeric
    /// needle; admitted for a column pinned binary.
    ClickHouse,
}

impl PrefilterCapability {
    /// Whether this arm may test a column pinned with `collation` on its stored text.
    ///
    /// `ClickHouse` has no column collation, yet only a column pinned [`Collation::Binary`] is
    /// admitted: an unknown or named collation never is. `DataFusion` fixes byte-wise comparison in
    /// the engine, and `Oracle` checks its session sort at run time instead of a pinned fact.
    #[must_use]
    pub const fn admits(self, collation: &Collation) -> bool {
        match self {
            Self::DataFusion | Self::Oracle | Self::Postgres => true,
            Self::ClickHouse => collation.is_binary(),
        }
    }

    #[must_use]
    pub const fn of(dialect: TypeDialect) -> Option<Self> {
        match dialect {
            TypeDialect::DataFusion => Some(Self::DataFusion),
            TypeDialect::Oracle => Some(Self::Oracle),
            TypeDialect::Postgres => Some(Self::Postgres),
            TypeDialect::ClickHouse => Some(Self::ClickHouse),
            TypeDialect::DuckDb | TypeDialect::Cube | TypeDialect::BigQuery => None,
        }
    }

    /// Whether the pinned column itself must be pinned binary, because the engine binds a
    /// collation to each column and nothing else establishes it.
    #[must_use]
    pub const fn requires_binary_column(self) -> bool {
        matches!(self, Self::Postgres)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields, try_from = "WrittenRawPrefilter")]
pub struct RawPrefilter {
    acknowledged_because: AcknowledgementReason,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct WrittenRawPrefilter {
    acknowledged_because: String,
}

impl TryFrom<WrittenRawPrefilter> for RawPrefilter {
    type Error = InvalidOperatorText;

    fn try_from(written: WrittenRawPrefilter) -> Result<Self, Self::Error> {
        Self::parse(written.acknowledged_because)
    }
}

impl RawPrefilter {
    /// Records why this column may use a raw necessary-condition search.
    ///
    /// This permits timing differences, never a result decision on unredacted text. It also never
    /// lets an error stand for one, so the search applies only beside conjuncts that cannot raise:
    /// a plain column compared with literals of its own type (a string literal only against a column
    /// that carries a text policy).
    pub fn parse(reason: impl AsRef<str>) -> Result<Self, InvalidOperatorText> {
        AcknowledgementReason::written_under("raw_prefilter.acknowledged_because", reason)
            .map(|acknowledged_because| Self { acknowledged_because })
    }

    pub fn acknowledged_because(&self) -> &str {
        self.acknowledged_because.as_str()
    }
}
