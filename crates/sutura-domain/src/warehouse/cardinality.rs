//! What a data system answered when it was asked whether a declared join key is really unique.
//!
//! # The defect this vocabulary exists for
//!
//! A relationship declares `many_to_one`, and the whole query path spends that declaration: the
//! whole-answer path renders a `JOIN` on the strength of *a dimension row cannot duplicate a fact
//! row*, and a federated lookup leg renders `GROUP BY` over the columns it projects on the strength
//! of the same sentence. **Neither of them checks it, and the two spend it differently.** Measured
//! over the derived corpus in `crates/sutura-app/tests/differential/federated.rs`, with one extra
//! row for a customer key the dimension table already had:
//!
//! | Topology | June 2026, `recurring_revenue`, business only, north |
//! | --- | --- |
//! | one data system | `29138` - the `JOIN` matched twice and the measure was added twice |
//! | two data systems | `22765` - the lookup leg's `GROUP BY` collapsed the pair first |
//!
//! `29138 - 22765` is that one customer's June revenue, counted a second time. **Neither answer was
//! refused**, and which one a deployment gets depends on where the dimension model sits rather than
//! on the question. [`FederatedFailure::AmbiguousLink`](crate::plan::FederatedFailure::AmbiguousLink)
//! covers half the shape and only on the federated side: it fires when the duplicate rows DISAGREE
//! in a column the question projects, and the `GROUP BY` has already removed them when they agree.
//!
//! # Why the check is here rather than on the answer path
//!
//! A duplicate is invisible inside one answer's statement. What the declaration claims is a property
//! of the **target table**, not of any one question, so the place it can be contradicted is the same
//! place an anchor is: once, at boot, against the data system that holds the table. Refusing there
//! makes both topologies agree - a bundle whose declaration the data contradicts is not validated,
//! so neither of them serves - and the refusal names the model, the table and the column, which no
//! answer-path guard could.
//!
//! # What the probe carries back, and what it deliberately does not
//!
//! Two counts: how many non-null values of the key column the table holds, and how many of them are
//! distinct. **No key value ever leaves the data system**, which is why the counts are the shape:
//! a boot refusal is written to an operator's log, and a duplicated dimension key printed there is
//! source data copied into a sink nobody scoped for it. The counts locate the table; the operator
//! queries it.
//!
//! Nulls are excluded from both counts, and that is a correctness decision rather than a
//! convenience: a null key matches nothing on either side of any join, so two null target rows
//! duplicate no fact row. Counting them would report a violation that cannot change an answer.
//!
//! **The links here are `crate::`-prefixed** for the reason [`preflight`](crate::warehouse::preflight)'s header
//! gives: the API reference pages are generated from these comments verbatim.

use crate::catalog::{Definitions, Relationship};
use crate::model::{ColumnName, JoinType, ModelName, QualifiedTable, RelationshipName, SourceName};
use crate::warehouse::{RowSet, Value};

/// The label the row count is projected under.
///
/// **A leading digit, which is the same namespace trick
/// [`InternalLabel`](crate::plan::InternalLabel) is built on and for the same reason:**
/// `crate::model`'s identifier parser refuses a leading digit as a first character, so no
/// `ColumnName` a catalog can declare collides with it. Here that matters less than it does for a
/// leg - the probe projects nothing but these two - but the two labels are read back by label rather
/// than by position, and a label that no column can shadow is what makes reading by label safe.
pub const ROWS_LABEL: &str = "0_key_rows";

/// The label the distinct count is projected under. See [`ROWS_LABEL`].
pub const DISTINCT_LABEL: &str = "0_key_distinct";

/// One declared join key, resolved to the table and column a data system can be asked about.
///
/// **A newtype that parses, and what it parses away is asking the wrong question.** It is
/// constructible only from a [`Relationship`] whose [`JoinType`] promises that the TARGET column is
/// unique, resolved against the [`Definitions`] that carry the target model - so a probe over a
/// `one_to_many` target, or over a column the model does not declare, is unrepresentable rather than
/// refused later. An adapter that holds one of these knows the question is worth asking.
///
/// It borrows, because every part of it is already owned by the pinned bundle the boot path is
/// holding, and a probe outlives nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeclaredKey<'a> {
    relationship: &'a RelationshipName,
    model: &'a ModelName,
    source: &'a SourceName,
    table: &'a QualifiedTable,
    column: &'a ColumnName,
}

/// Why a declared relationship yields no key to probe.
///
/// Typed, one variant per branch, because two of the three are *nothing to ask* and one is a bundle
/// that would not have loaded - and a caller that collapsed them would report a consistent catalog
/// as an unchecked one.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NoDeclaredKey {
    /// The join type promises nothing about the target column.
    ///
    /// `one_to_many` is the only one: it is the direction that MAY duplicate rows, which is why
    /// `Definitions` already refuses to reach a dimension through one. A probe over it would refuse
    /// a bundle for holding exactly the shape it declared.
    #[error("a `{}` relationship promises nothing about its target column being unique", .join_type.as_str())]
    MayDuplicateRows { join_type: JoinType },
    /// The target model is not in these definitions.
    ///
    /// Unreachable through a loaded bundle - `Definitions` refuses a relationship naming a model it
    /// does not carry - and reported rather than panicked for the reason every self-check in this
    /// domain is: the input is a catalog document, and a panic reachable from one is a defect.
    #[error("the target model `{model}` is not defined")]
    ModelUndefined { model: ModelName },
    /// The target model does not declare the column the relationship joins on. Unreachable for
    /// [`ModelUndefined`](NoDeclaredKey::ModelUndefined)'s reason, and reported for it.
    #[error("the target model `{model}` declares no column `{column}`")]
    ColumnNotOnModel { model: ModelName, column: ColumnName },
}

impl<'a> DeclaredKey<'a> {
    /// The key one relationship promises is unique, or why it promises none.
    ///
    /// **The whole of the join-type decision is here**, so no adapter and no boot path repeats it:
    /// [`JoinType::may_duplicate_rows`] is the one question, and both `one_to_one` and `many_to_one`
    /// answer it the same way - each of them says the target column identifies at most one row.
    /// `one_to_one` promises the origin column does too, and **this does not check that half**; see
    /// the module header's limits.
    pub fn promised_by(relationship: &'a Relationship, definitions: &'a Definitions) -> Result<Self, NoDeclaredKey> {
        let join_type = relationship.join_type();
        if join_type.may_duplicate_rows() {
            return Err(NoDeclaredKey::MayDuplicateRows { join_type });
        }
        let model = relationship.target_model();
        let target = definitions
            .model(model)
            .ok_or_else(|| NoDeclaredKey::ModelUndefined { model: model.clone() })?;
        let column = relationship.target_column();
        if !target.has_column(column) {
            return Err(NoDeclaredKey::ColumnNotOnModel {
                model: model.clone(),
                column: column.clone(),
            });
        }
        Ok(Self {
            relationship: relationship.name(),
            model,
            source: target.source(),
            table: target.table(),
            column,
        })
    }

    /// The relationship whose declaration this probe would contradict.
    #[inline]
    #[must_use]
    pub const fn relationship(&self) -> &'a RelationshipName {
        self.relationship
    }

    /// The model the operator opens to fix it.
    #[inline]
    #[must_use]
    pub const fn model(&self) -> &'a ModelName {
        self.model
    }

    /// The data system that holds the table, which is the adapter this is asked of.
    #[inline]
    #[must_use]
    pub const fn source(&self) -> &'a SourceName {
        self.source
    }

    /// The table to count over.
    #[inline]
    #[must_use]
    pub const fn table(&self) -> &'a QualifiedTable {
        self.table
    }

    /// The column whose values are meant to be distinct.
    #[inline]
    #[must_use]
    pub const fn column(&self) -> &'a ColumnName {
        self.column
    }
}

/// How many non-null key values a table holds, and how many of them are distinct.
///
/// **Parsed rather than validated:** neither `distinct > rows` nor rows-with-no-distinct-value is
/// producible by one column of one table, so a pair in either shape is a defect in an adapter's
/// mapping and not a fact about data. The field pair is private and [`parse`](KeyCounts::parse) is
/// the only way in, which is what makes the subtraction in
/// [`duplicated`](KeyCounts::duplicated) safe: `rows >= distinct` holds for every constructible
/// value. It is written `saturating_sub` anyway, and the reason is the sink rather than the
/// arithmetic - this number is rendered into a boot refusal an operator reads, and a panic reachable
/// from a catalog-driven path is the one outcome worse than a wrong figure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyCounts {
    rows: u64,
    distinct: u64,
}

/// Why two counts are not a [`KeyCounts`].
///
/// **Two variants, because there are two impossible pairs and only one of them was refused at
/// first.** Both are arithmetic about one column of one table rather than anything about data, so
/// both mean a broken adapter mapping; and a pair that reached [`KeyNotUnique`] would print a table
/// that cannot exist into an operator's log.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ImpossibleCounts {
    /// More distinct values than values. One column of one table cannot produce this.
    #[error("a key column reported {distinct} distinct values over {rows} rows, which is impossible")]
    MoreDistinctThanRows { rows: u64, distinct: u64 },
    /// Values, and none of them distinct. A non-empty column has at least one distinct value, so
    /// this is exactly as impossible as the pair above and was exactly as constructible: `parse(41,
    /// 0)` answered `Ok`, and the [`KeyNotUnique`] it licensed described a table with forty-one rows
    /// under no key at all. Review found it; *newtypes that parse* is the rule it broke.
    #[error("a key column reported {rows} rows and no distinct value, which is impossible")]
    NoDistinctValue { rows: u64 },
}

impl KeyCounts {
    /// Parses the pair a data system answered with.
    ///
    /// **Both impossible pairs, not one.** `distinct > rows` is the obvious half; `rows > 0` with no
    /// distinct value is the half this originally accepted, and accepting it is what let a refusal
    /// describe a table that cannot exist. An empty column - `0` over `0` - is neither, and is a
    /// perfectly ordinary answer for a dimension table whose key column is entirely null.
    pub const fn parse(rows: u64, distinct: u64) -> Result<Self, ImpossibleCounts> {
        if distinct > rows {
            return Err(ImpossibleCounts::MoreDistinctThanRows { rows, distinct });
        }
        if rows > 0 && distinct == 0 {
            return Err(ImpossibleCounts::NoDistinctValue { rows });
        }
        Ok(Self { rows, distinct })
    }

    /// How many non-null key values the table holds.
    #[inline]
    #[must_use]
    pub const fn rows(&self) -> u64 {
        self.rows
    }

    /// How many of them are distinct.
    #[inline]
    #[must_use]
    pub const fn distinct(&self) -> u64 {
        self.distinct
    }

    /// Does the data hold the declaration up?
    #[inline]
    #[must_use]
    pub const fn is_unique(&self) -> bool {
        self.rows == self.distinct
    }

    /// How many rows are surplus to the keys they carry.
    ///
    /// Not *how many keys are duplicated* - one key on three rows contributes two - and the
    /// difference is worth the sentence, because this number goes into a refusal an operator reads.
    #[inline]
    #[must_use]
    pub const fn duplicated(&self) -> u64 {
        self.rows.saturating_sub(self.distinct)
    }
}

/// A declared key the data contradicts, and the whole of what a refusal about one says.
///
/// **Constructible only from counts that are NOT unique**, so a refusal describing a table that
/// holds its declaration up is unrepresentable rather than a branch a caller could take by mistake.
/// [`found`](KeyNotUnique::found) is the only way in and it returns `None` for the clean case.
///
/// **A struct rather than six fields on a `NotValidated` variant**, and boxed there:
/// `result_large_err` is deliberately left on in this workspace - *a service whose public surface is
/// `ToolOutcome::Refusal` wants to know when the error half of every `Result` grows* - and six
/// parsed names inline made that `Result` the widest thing the boot path returns.
///
/// **It carries no key value**, which is the module header's decision: this is rendered into an
/// operator's log, and a duplicated dimension key printed there is source data copied into a sink
/// nobody scoped for it. The counts locate the table; the operator queries it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyNotUnique {
    relationship: RelationshipName,
    model: ModelName,
    table: QualifiedTable,
    column: ColumnName,
    counts: KeyCounts,
}

impl KeyNotUnique {
    /// The violation these counts show, or `None` where they hold the declaration up.
    #[must_use]
    pub fn found(key: &DeclaredKey<'_>, counts: KeyCounts) -> Option<Self> {
        (!counts.is_unique()).then(|| Self {
            relationship: key.relationship().clone(),
            model: key.model().clone(),
            table: key.table().clone(),
            column: key.column().clone(),
            counts,
        })
    }

    /// The relationship whose declaration the data contradicts.
    #[inline]
    #[must_use]
    pub const fn relationship(&self) -> &RelationshipName {
        &self.relationship
    }

    /// The model an operator opens to fix the declaration.
    #[inline]
    #[must_use]
    pub const fn model(&self) -> &ModelName {
        &self.model
    }

    /// The table an operator queries to find the duplicates.
    #[inline]
    #[must_use]
    pub const fn table(&self) -> &QualifiedTable {
        &self.table
    }

    /// The column that was meant to identify at most one row.
    #[inline]
    #[must_use]
    pub const fn column(&self) -> &ColumnName {
        &self.column
    }

    /// What the data system counted.
    #[inline]
    #[must_use]
    pub const fn counts(&self) -> KeyCounts {
        self.counts
    }
}

impl core::fmt::Display for KeyNotUnique {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "relationship {} declares that {} identifies at most one row of {}, and {} holds {} \
             non-null values under {} distinct ones",
            self.relationship,
            self.column,
            self.model,
            self.table,
            self.counts.rows(),
            self.counts.distinct()
        )
    }
}

/// A declared key no data system would count, and the whole of what a refusal about one says.
///
/// **The adapter's own error, flattened at the one boundary where it is still typed.** `W::Error` is
/// a generic parameter and this crate must not hold one, so the message and its cause chain arrive
/// as text - the same lossless-at-that-boundary move
/// [`NotExecutedReason::Failed`](crate::pinned::NotExecutedReason::Failed) makes for an anchor's
/// adapter error, and for its reason. The chain is what carries the data system's own complaint, and
/// a permission failure names the grant in it.
///
/// **There is deliberately no *refused* / *unreachable* split here**, and that is a measurement
/// rather than a shortcut: the port's one predicate for that question,
/// [`Warehouse::preflight_was_refused`](crate::warehouse::Warehouse::preflight_was_refused), is
/// overridden by exactly one adapter, and that adapter takes this method's default - so a typed
/// split would be a control that cannot fire on any adapter that counts. What is done instead is to
/// refuse in BOTH cases and print the cause, which is loud for either and honest about neither being
/// told apart. Splitting them wants that predicate implemented by an adapter that can tell a `403`
/// from a timeout, and that is a slice of its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyNotCounted {
    relationship: RelationshipName,
    model: ModelName,
    source: SourceName,
    message: String,
    chain: Vec<String>,
}

impl KeyNotCounted {
    /// The failure one probe met, from the key it was asked about and the adapter's flattened error.
    #[must_use]
    pub fn of(key: &DeclaredKey<'_>, message: String, chain: Vec<String>) -> Self {
        Self {
            relationship: key.relationship().clone(),
            model: key.model().clone(),
            source: key.source().clone(),
            message,
            chain,
        }
    }

    /// The relationship whose declaration went unchecked.
    #[inline]
    #[must_use]
    pub const fn relationship(&self) -> &RelationshipName {
        &self.relationship
    }

    /// The model whose table could not be counted.
    #[inline]
    #[must_use]
    pub const fn model(&self) -> &ModelName {
        &self.model
    }

    /// The data system that would not answer, which is where an operator looks.
    #[inline]
    #[must_use]
    pub const fn source(&self) -> &SourceName {
        &self.source
    }
}

impl core::fmt::Display for KeyNotCounted {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "relationship {} declares a unique key on {}, and {} would not count it: {}",
            self.relationship, self.model, self.source, self.message
        )?;
        for cause in &self.chain {
            write!(f, ": {cause}")?;
        }
        Ok(())
    }
}

/// Why a probe's result set is not a pair of counts.
///
/// Every variant is a defect in an adapter or in the rendering, never anything about the data: the
/// probe projects exactly two aggregates over one table and no group, so one row of two integers is
/// the only shape it can have. Typed rather than one message so that whoever reads a boot log knows
/// which half of the mapping is wrong.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum CountsNotRead {
    /// The result carries no column under the label the probe projects.
    #[error("a key probe's result has no column {label:?}")]
    NoColumn { label: &'static str },
    /// Two aggregates over no group produce one row.
    #[error("a key probe's result has {rows} rows, and two aggregates over no group produce one")]
    NotOneRow { rows: usize },
    /// A count came back as something other than an integer.
    #[error("a key probe's {label:?} came back as {value:?} rather than a count")]
    NotACount { label: &'static str, value: Value },
    /// A count came back negative, which no `COUNT` produces.
    #[error("a key probe's {label:?} came back as {value}, and a count is not negative")]
    NegativeCount { label: &'static str, value: i64 },
    /// The pair is arithmetically impossible.
    #[error("a key probe answered an impossible pair")]
    Impossible {
        #[source]
        cause: ImpossibleCounts,
    },
}

/// What a data system said about a declared key.
///
/// **[`Self::NotAsked`] is not [`Self::Counted`] with a clean pair, and no caller can read it as
/// one.** It is [`TablesPresent::NotAsked`](crate::warehouse::preflight::TablesPresent::NotAsked)'s argument
/// applied to a second boot question: the port's default has to be *nothing to report*, because an
/// adapter with no cheap way to count must not be forced to answer, and the only answers available
/// to one that cannot look are nothing-to-report and a lie. A default claiming uniqueness would be
/// that lie, told at boot, about the one declaration the whole join path spends.
///
/// **The third outcome is an `Err` from the port rather than a variant here**, for the reason
/// [`TablesPresent`](crate::warehouse::preflight::TablesPresent) gives: *could not count* and *the data
/// contradicts the declaration* must not collapse into one message, and the adapter's own error type
/// is where the reason lives in the detail an operator needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyUniqueness {
    /// The adapter did not count. The port's default.
    NotAsked,
    /// The adapter counted, and this is what it found.
    Counted(KeyCounts),
}

impl KeyUniqueness {
    /// Reads the two counts off a probe's result set.
    ///
    /// **One function, in the domain, called by every SQL adapter and by the engine**, so the three
    /// implementations of the port cannot disagree about which column is which - the same argument
    /// [`labels`](crate::plan::labels) makes for the federated legs. It takes a [`RowSet`] because
    /// that is what every adapter already produces; nothing here knows what a statement is.
    pub fn read(rows: &RowSet) -> Result<Self, CountsNotRead> {
        if rows.rows().len() != 1 {
            return Err(CountsNotRead::NotOneRow { rows: rows.rows().len() });
        }
        let counted = |label: &'static str| -> Result<u64, CountsNotRead> {
            let index = rows.column_index(label).ok_or(CountsNotRead::NoColumn { label })?;
            match rows.cell(0, index) {
                Some(&Value::Integer(value)) => {
                    u64::try_from(value).map_err(|_out_of_range| CountsNotRead::NegativeCount { label, value })
                }
                Some(value) => Err(CountsNotRead::NotACount {
                    label,
                    value: value.clone(),
                }),
                None => Err(CountsNotRead::NoColumn { label }),
            }
        };
        let counts = KeyCounts::parse(counted(ROWS_LABEL)?, counted(DISTINCT_LABEL)?)
            .map_err(|cause| CountsNotRead::Impossible { cause })?;
        Ok(Self::Counted(counts))
    }

    /// Whether the adapter looked at all.
    ///
    /// Read where a caller has to tell *nobody counted* from *counted and clean*, which is the whole
    /// reason the two are different values.
    #[inline]
    #[must_use]
    pub const fn was_asked(&self) -> bool {
        matches!(*self, Self::Counted(_))
    }
}

#[cfg(test)]
mod tests;
