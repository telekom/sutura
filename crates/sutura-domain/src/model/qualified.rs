//! Where a table lives, when that is more than its own name.
//!
//! A multi-project estate names tables outside the one dataset a connection defaults to, and the
//! failure mode of not being able to is not a clean refusal: a table of the same name present in the
//! default dataset is read instead, and a plausible number comes back under a certified metric with
//! its definition digest unmoved. `docs/adr/0019` is the decision; issue #83 is the report.
//!
//! # This is a COMPOSITION of parsed names, and never a string containing dots
//!
//! **Relaxing [`TableName`] to admit a `.` would have been one line and would have silently
//! invalidated the reasoning two invariants rest on.** `AGENTS.md`'s *no identifier reaches the
//! statement unquoted* and *no value from a question reaches the statement as text* are both asserted
//! over the golden corpus by **stripping quoted spans** with a single toggle, and that stripping is
//! sound only because [`crate::model::parse_identifier`] rejects every character outside
//! `[A-Za-z0-9_]` - so no `"`, `'` or `` ` `` can occur inside a name and the toggle cannot land
//! mid-name. A dotted string would put a whole path where the corpus assertions expect one name, and
//! nothing would have failed.
//!
//! So every part is its own parsed name, stored separately, and the generator quotes **per part**.
//! [`QualifiedTable::parse`] exists for the convenience of whoever writes a catalog document -
//! `table: analytics_prod.sales.orders` - and it is a *splitter in front of the canonical
//! constructor*, not a second parser: it splits on `.` and hands each piece to the name parser it
//! belongs to. Nothing here stores the text it was given.
//!
//! # The one name shape in this workspace that admits a hyphen
//!
//! [`ProjectName`] does, and [`DatasetName`] does not. That asymmetry is not a preference: a
//! `BigQuery` **project id** is lowercase letters, digits and hyphens - the credential this
//! repository's acceptance leg runs under names one with a hyphen in it, which is how the asymmetry
//! was found rather than assumed - while a **dataset id** is letters, digits and underscores only,
//! exactly [`crate::model::parse_identifier`]'s set. [`crate::model::Hyphens`] carries what widening
//! that flag is and is not licensed to do.
//!
//! **A legacy domain-scoped project - `example.com:project` - is refused rather than supported.** A
//! `:` or a second `.` inside one part is the defect this module exists to prevent, so accepting the
//! spelling that carries them would be reintroducing it in the one place it was designed out.
//!
//! # What this module deliberately does not decide
//!
//! **How deep a qualifier a given data system resolves.** [`Qualification`] is the vocabulary and
//! `sutura_sql::Dialect::qualification` is the declaration, matched exhaustively there so a fifth
//! dialect cannot compile without answering. The domain names the shape; a dialect answers for
//! itself.
//!
//! **Whether reaching two projects is one source or two.** It is **one**: a source is a credential
//! plus a billing project, not a project, and `sutura_semantic::plan` says so where a source count
//! decides between one statement, a split and `PlanSpansTooManySources`. Two projects read by one
//! credential in one statement is a native
//! join the data system pushes down, and routing it through a splitter and a client-side combiner
//! would replace that with something slower which discards the pushdown.

use crate::model::{Hyphens, InvalidIdentifier, TableName, identifier_newtype, parse_name};

identifier_newtype! {
    /// The name of a dataset, or of a schema - the qualifier immediately above a table.
    ///
    /// One type for both words because it is one position in a path. `dataset` is what `BigQuery`
    /// calls it and `schema` is what the standard calls it; a dataset id is letters, digits and
    /// underscore, which is exactly what every other name in this module accepts, so there is nothing
    /// to parameterise.
    DatasetName
}

/// The name of a project, or of a catalog - the topmost qualifier a table path may carry.
///
/// **Hand-written rather than a seventh [`identifier_newtype`], and the hyphen is the whole reason.**
/// A `BigQuery` project id is `[a-z][a-z0-9-]{4,28}[a-z0-9]`, so a hyphen is not an edge case there,
/// it is the norm - and `parse_identifier` rejects one as an [`InvalidIdentifier::IllegalCharacter`].
/// It still routes through one body: [`parse_name`] under [`Hyphens::Allowed`].
///
/// **Not named `CatalogName`, deliberately.** `catalog` is the standard's word for this position and
/// it is also this workspace's word for the *metadata* catalog - `crate::catalog`, `SemanticCatalog`,
/// a catalog document. A type called `CatalogName` next to those would be read wrongly by every
/// reader once.
///
/// **What it accepts is a union rather than one target's rules, and the reason is stated so nobody
/// reads it as sloppiness.** This one position stands for a `BigQuery` project and for a standard
/// catalog, whose names are ordinary identifiers with uppercase and underscore. Refusing what either
/// target accepts would make a model unauthorable for the other. What a *particular* data system
/// then does with a name it does not recognise is refuse the query by name, loudly, which is the
/// failure this module is not trying to pre-empt. What is NOT a union is the character set: a quote,
/// a dot, a colon and whitespace are refused here, and that is the property the golden stripping
/// rests on.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "String")]
pub struct ProjectName(String);

impl ProjectName {
    /// Parses a project name, rejecting anything that is not one.
    ///
    /// The canonical constructor. [`TryFrom<String>`] and the `Deserialize` derive both route here.
    pub fn parse(raw: impl AsRef<str>) -> Result<Self, InvalidIdentifier> {
        parse_name(raw.as_ref(), Hyphens::Allowed).map(Self)
    }

    #[inline]
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for ProjectName {
    type Error = InvalidIdentifier;

    fn try_from(raw: String) -> Result<Self, Self::Error> {
        Self::parse(raw)
    }
}

impl core::fmt::Display for ProjectName {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

/// How much of a path sits above a table.
///
/// Read two ways, and one type rather than two so the two can be **compared**: a
/// [`QualifiedTable`] reports how deep it is, `sutura_sql::Dialect::qualification` declares how deep
/// a data system resolves, and rendering refuses when the first is deeper than the second. Two types
/// would have made that comparison a hand-written match somebody has to keep in step.
///
/// **The variant order is load-bearing and is asserted rather than assumed.** The derived [`Ord`] on
/// an enum is declaration order, so `TableOnly < Dataset < ProjectAndDataset` is what makes
/// `name.qualification() <= dialect.qualification()` mean *shallow enough*. Reordering the
/// declaration would invert every such comparison silently, which is why
/// `deeper_is_greater_because_the_comparison_is_what_decides_a_refusal` exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Qualification {
    /// The table's own name and nothing above it.
    TableOnly,
    /// `dataset.table`, or `schema.table`.
    Dataset,
    /// `project.dataset.table`, or `catalog.schema.table`.
    ProjectAndDataset,
}

impl Qualification {
    #[inline]
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::TableOnly => "table_only",
            Self::Dataset => "dataset",
            Self::ProjectAndDataset => "project_and_dataset",
        }
    }
}

impl core::fmt::Display for Qualification {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What sits above a table: a dataset, and optionally a project above that.
///
/// **`dataset` is not optional, and that is the shape doing the work.** `project..table` is not a
/// thing any data system names, so a project without a dataset is *unrepresentable* here rather than
/// refused by a check somebody has to remember to run. There is no constructor that takes a project
/// alone and no field a caller could leave out.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TableQualifier {
    /// Above the dataset. Absent for a two-part name.
    project: Option<ProjectName>,
    /// Immediately above the table. Always present - see the type's own note.
    dataset: DatasetName,
}

impl TableQualifier {
    /// A dataset, and optionally a project above it.
    ///
    /// The canonical constructor, and the only one: [`Self::in_dataset`] and [`Self::in_project`] are
    /// spellings of it that read better at a call site than `None` and `Some` do.
    #[inline]
    #[must_use]
    pub const fn new(project: Option<ProjectName>, dataset: DatasetName) -> Self {
        Self { project, dataset }
    }

    /// `dataset.table`.
    #[inline]
    #[must_use]
    pub const fn in_dataset(dataset: DatasetName) -> Self {
        Self::new(None, dataset)
    }

    /// `project.dataset.table`.
    #[inline]
    #[must_use]
    pub const fn in_project(project: ProjectName, dataset: DatasetName) -> Self {
        Self::new(Some(project), dataset)
    }

    #[inline]
    #[must_use]
    pub const fn project(&self) -> Option<&ProjectName> {
        self.project.as_ref()
    }

    #[inline]
    #[must_use]
    pub const fn dataset(&self) -> &DatasetName {
        &self.dataset
    }

    #[inline]
    #[must_use]
    const fn qualification(&self) -> Qualification {
        match self.project {
            None => Qualification::Dataset,
            Some(_) => Qualification::ProjectAndDataset,
        }
    }
}

/// Why a table path was not one.
///
/// One variant per position, each carrying the whole path it came from and the position's own parse
/// failure on the `#[source]` chain - so a refusal says *which part* of
/// `analytics_prod.sales.orders` was wrong as well as why. A single `Part { index }` variant was the
/// alternative and it makes every reader count dots.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum InvalidQualifiedTable {
    /// More dots than a path has positions.
    #[error("{value:?} names {parts} parts, and a table path names at most {limit}")]
    TooManyParts { value: String, parts: usize, limit: usize },
    /// The topmost part is not a project name.
    #[error("the project part of {value:?} is not a name")]
    Project {
        value: String,
        #[source]
        cause: InvalidIdentifier,
    },
    /// The middle part is not a dataset name.
    #[error("the dataset part of {value:?} is not a name")]
    Dataset {
        value: String,
        #[source]
        cause: InvalidIdentifier,
    },
    /// The last part is not a table name. Also what an empty path is reported as.
    #[error("the table part of {value:?} is not a name")]
    Table {
        value: String,
        #[source]
        cause: InvalidIdentifier,
    },
}

/// The most parts a table path may name.
const MAX_PARTS: usize = 3;

/// A table, and where it lives when that is more than the default.
///
/// **A model that names only a table keeps working, byte for byte**, and that is the compatibility
/// property rather than a hope: [`Self::parse`] of a bare name yields no qualifier,
/// [`Display`](core::fmt::Display) writes the bare name back, and [`serde::Serialize`] writes that
/// same text - so a catalog document, a serialized plan and a definition digest over an unqualified
/// model are unchanged by this type existing.
///
/// See this module's header for why it is a composition and not a string.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct QualifiedTable {
    /// Above the table. Absent for a bare name.
    ///
    /// Declared **before** `table`, which is what the derived [`Ord`] sorts on first. A qualified
    /// name and a bare one of the same table therefore do not compare equal and do not sort
    /// adjacently by table, which is the ordering an operator reading a list of served tables wants:
    /// grouped by where they live.
    qualifier: Option<TableQualifier>,
    table: TableName,
}

impl QualifiedTable {
    /// A table, with whatever sits above it.
    ///
    /// **The canonical constructor.** [`Self::parse`], [`From<TableName>`] and the `Deserialize`
    /// derive all route here, and none of them repeats a check.
    #[inline]
    #[must_use]
    pub const fn new(qualifier: Option<TableQualifier>, table: TableName) -> Self {
        Self { qualifier, table }
    }

    /// Parses a dotted path: `table`, `dataset.table`, or `project.dataset.table`.
    ///
    /// **A splitter in front of [`Self::new`] and not a second parser.** It splits on `.` - which no
    /// part can contain, because each is parsed by a parser that rejects a dot - and hands each piece
    /// to the name type for that position. The text it was handed is not stored.
    ///
    /// This is the shape a catalog document writes, so this is where a document's `table:` value
    /// arrives; `sutura_catalog_local` needs no parsing of its own.
    pub fn parse(raw: impl AsRef<str>) -> Result<Self, InvalidQualifiedTable> {
        let raw = raw.as_ref();
        let trimmed = raw.trim();
        let parts: Vec<&str> = trimmed.split('.').collect();
        let value = || String::from(trimmed);
        // Longest first, so the two-part and three-part arms read in the order a path is written.
        match *parts.as_slice() {
            [project, dataset, table] => Ok(Self::new(
                Some(TableQualifier::in_project(
                    ProjectName::parse(project).map_err(|cause| InvalidQualifiedTable::Project { value: value(), cause })?,
                    DatasetName::parse(dataset).map_err(|cause| InvalidQualifiedTable::Dataset { value: value(), cause })?,
                )),
                TableName::parse(table).map_err(|cause| InvalidQualifiedTable::Table { value: value(), cause })?,
            )),
            [dataset, table] => Ok(Self::new(
                Some(TableQualifier::in_dataset(
                    DatasetName::parse(dataset).map_err(|cause| InvalidQualifiedTable::Dataset { value: value(), cause })?,
                )),
                TableName::parse(table).map_err(|cause| InvalidQualifiedTable::Table { value: value(), cause })?,
            )),
            [table] => Ok(Self::new(
                None,
                TableName::parse(table).map_err(|cause| InvalidQualifiedTable::Table { value: value(), cause })?,
            )),
            // Four parts or more. Nothing names a table that deep, and truncating would pick a table
            // the author did not write.
            _ => Err(InvalidQualifiedTable::TooManyParts {
                value: value(),
                parts: parts.len(),
                limit: MAX_PARTS,
            }),
        }
    }

    /// The table's own name: the last part of the path.
    ///
    /// **This is the name a column is qualified by**, and the reason is that in every dialect this
    /// workspace renders for, `FROM a.b.c` gives the reference an implicit alias of `c`. It is also
    /// the name a file-registering engine registers under. So the two readings of "the table" are
    /// both real and both needed, and they are two accessors rather than one that guesses:
    /// [`Self::name`] is the last part, [`Display`](core::fmt::Display) is the whole path.
    #[inline]
    #[must_use]
    pub const fn name(&self) -> &TableName {
        &self.table
    }

    /// What sits above the table, if anything.
    #[inline]
    #[must_use]
    pub const fn qualifier(&self) -> Option<&TableQualifier> {
        self.qualifier.as_ref()
    }

    /// How deep this path is.
    ///
    /// Compared against `sutura_sql::Dialect::qualification` at render time. See [`Qualification`]
    /// for why the ordering of that comparison is asserted rather than assumed.
    #[inline]
    #[must_use]
    pub const fn qualification(&self) -> Qualification {
        match self.qualifier {
            None => Qualification::TableOnly,
            Some(ref qualifier) => qualifier.qualification(),
        }
    }
}

/// The compatibility hinge, and every unqualified call site in this workspace goes through it.
///
/// `Model::new`, `QueryPlan::new`, `PlanJoin::new` and `LegPlan`'s constructors all take
/// `impl Into<QualifiedTable>`, so a fixture or an adapter that hands over a [`TableName`] compiles
/// unchanged and means exactly what it used to.
impl From<TableName> for QualifiedTable {
    fn from(table: TableName) -> Self {
        Self::new(None, table)
    }
}

impl TryFrom<String> for QualifiedTable {
    type Error = InvalidQualifiedTable;

    fn try_from(raw: String) -> Result<Self, Self::Error> {
        Self::parse(raw)
    }
}

/// The dotted path, which is also the serialized form and the form a document writes.
impl core::fmt::Display for QualifiedTable {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        if let Some(qualifier) = self.qualifier.as_ref() {
            if let Some(project) = qualifier.project.as_ref() {
                write!(f, "{project}.")?;
            }
            write!(f, "{}.", qualifier.dataset)?;
        }
        write!(f, "{}", self.table)
    }
}

/// **Hand-written, and the asymmetry it closes has shipped as a bug in this workspace before.**
///
/// `#[serde(try_from = "String")]` affects `Deserialize` alone, so a *derived* `Serialize` here would
/// write `{qualifier: {...}, table: ...}` - a shape this type's own `Deserialize` refuses. `Date`
/// shipped exactly that, and it mattered because the definition digest is taken over the serialized
/// form: it would have covered a field layout that appears in no catalog file rather than the text an
/// author wrote. `a_serialized_path_deserializes_back` is the test, and it is written as a round trip
/// rather than against an expected string so it cannot pass while both halves are wrong together.
impl serde::Serialize for QualifiedTable {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.collect_str(self)
    }
}

impl<'de> serde::Deserialize<'de> for QualifiedTable {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = <String as serde::Deserialize<'de>>::deserialize(deserializer)?;
        Self::parse(&raw).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests;
