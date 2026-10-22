//! A leg as it crosses from one node to another, and the parse that decides what the receiver runs.
//!
//! `docs/adr/20261017032054-leg-transport-between-sutura-nodes.md` decides the shape: a typed leg
//! crosses, never SQL text, and the receiving node parses it against **its own** certified
//! definitions. [`to_wire`] is the sender's half and [`LegWire::parse`] the receiver's.
//!
//! **What the document names, and what it never carries.** It names a source, a table path, the
//! relationships a statement joins through, columns, a grain and the metric a fact leg measures,
//! with that metric's digest. It carries no join key, join type, joined table or statement: the
//! receiver writes each join from its own relationship
//! ([`PlanJoin::hop`](crate::plan::PlanJoin::hop)), so a sender cannot make the receiver join on a
//! pair of columns the receiver never declared. Parameter values are not in it either - they
//! travel beside it as the Arrow batch the ADR decides - so `parse` takes them as an argument and
//! [`PlanBindings::parse`](crate::plan::PlanBindings::parse) holds the pair coherent.
//!
//! **The digest is per metric.** A fact leg names its metric as one [`MetricDigest`], so a leg with
//! no digest does not deserialize, and the receiver compares only that digest
//! ([`PinnedDefinitions::agrees_on`]): a bundle digest that differs is not read.
//!
//! **Limits, stated where the parse is.** The digest detects a version gap between two honest
//! nodes and is no control against a hostile sender. The parse does not compare a result label with
//! the dimension it names, an aggregate with the metric's own, or a column's type with the
//! sender's. It certifies that every name is one this node declares and that the metric is defined
//! as the sender defined it. For a fact leg it also certifies that the bucket is the metric's time
//! column on the table this node buckets it on, that every term reads a column the measure reads on
//! the leg's own root table, that a leg rooted on the metric's own table carries this node's
//! required filters, and that the leg is bounded by its own range on that bucket column, at most
//! [`MAX_RANGE_DAYS`](crate::query::MAX_RANGE_DAYS) days long: this node holds the time bound
//! itself, as its own planner does. A lookup leg names no metric and carries no range.

use crate::calendar::TimeRange;
use crate::definitions::NotDigestible;
use crate::model::{DimensionName, Grain, QualifiedTable, RelationshipName, SourceName};
use crate::pinned::{MetricDigest, PinnedDefinitions};
use crate::plan::{InternalLabel, KeyValue, LegPlan, PlanColumn, PlanFilter, PlanKey, PlanTerm, ResultLabel};
use crate::warehouse::ParamValue;

mod certify;

use certify::certify;
pub use certify::{LegNotCertified, LegPart};

/// What a result column is called, as it travels: an internal label or a dimension's name.
///
/// There is no `Deserialize` on [`ResultLabel`], by design, so the wire holds the two things a leg's
/// column can be called and the receiver builds the label from the one it parsed.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum WireLabel {
    Internal(InternalLabel),
    Dimension(DimensionName),
}

/// Why a label on the wire is neither of the two things a leg's column can be called.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{text:?} names neither an internal label nor a dimension")]
pub struct InvalidWireLabel {
    text: String,
}

impl WireLabel {
    /// The canonical constructor: [`TryFrom<String>`] and `Deserialize` route here.
    pub fn parse(text: &str) -> Result<Self, InvalidWireLabel> {
        InternalLabel::parse(text)
            .map(Self::Internal)
            .or_else(|| DimensionName::parse(text).ok().map(Self::Dimension))
            .ok_or_else(|| InvalidWireLabel {
                text: String::from(text),
            })
    }

    fn into_label(self) -> ResultLabel {
        match self {
            Self::Internal(label) => ResultLabel::internal(label),
            Self::Dimension(name) => ResultLabel::dimension(&name),
        }
    }
}

impl TryFrom<String> for WireLabel {
    type Error = InvalidWireLabel;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        Self::parse(&text)
    }
}

impl From<WireLabel> for String {
    fn from(label: WireLabel) -> Self {
        match label {
            WireLabel::Internal(label) => label.label(),
            WireLabel::Dimension(name) => name.as_str().to_owned(),
        }
    }
}

/// A group-by column and what it is projected under.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireKey {
    label: WireLabel,
    column: PlanColumn,
}

/// An aggregated term and what it is projected under.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireTerm {
    label: WireLabel,
    term: PlanTerm,
}

/// The time bucket a fact leg groups by: its grain and the column it truncates.
///
/// No label: a bucket is always projected under the one label the catalog reserves for it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireBucket {
    grain: Grain,
    column: PlanColumn,
}

/// One leg as it crosses between nodes.
///
/// Every field refuses an undeclared sibling (`deny_unknown_fields`, the shape `Query` has), so a
/// field the receiver does not read is an error that names it rather than a value that rides along.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "leg", rename_all = "snake_case", deny_unknown_fields)]
pub enum LegWire {
    /// One fact table's share of an answer, with the metric and its digest as one value.
    Fact {
        source: SourceName,
        metric: MetricDigest,
        table: QualifiedTable,
        joins: Vec<RelationshipName>,
        bucket: WireBucket,
        keys: Vec<WireKey>,
        terms: Vec<WireTerm>,
        filters: Vec<PlanFilter>,
        range: TimeRange,
    },
    /// One remote dimension's share: its key and the columns the answer groups by.
    Lookup {
        source: SourceName,
        table: QualifiedTable,
        joins: Vec<RelationshipName>,
        keys: Vec<WireKey>,
        filters: Vec<PlanFilter>,
    },
}

/// Why a leg cannot be written for the wire.
#[derive(Debug, thiserror::Error)]
pub enum NotSendable {
    /// The metric the leg measures has no digest in the sender's own pin.
    #[error(transparent)]
    Digest(#[from] NotDigestible),
    /// A result label the receiver could not read back.
    #[error(transparent)]
    Label(#[from] InvalidWireLabel),
    /// A key truncated to a grain: the wire carries a key's column and not its grain.
    #[error("the key `{label}` is truncated to a grain, which a leg on the wire does not carry")]
    KeyTruncated { label: String },
}

/// The leg as the sender writes it, with the digest of its metric read off the sender's own pin.
///
/// The digest is [`PinnedDefinitions::metric_digest`], never a value the caller supplies, so what
/// a leg claims about its metric is what the sender planned under.
///
/// A key truncated to a grain is refused as [`NotSendable::KeyTruncated`]: a wire key carries its
/// column and not its grain, so the receiver would group by the raw column and answer wrongly.
pub fn to_wire(leg: &LegPlan, pinned: &PinnedDefinitions) -> Result<LegWire, NotSendable> {
    let keys = |keys: &[PlanKey]| -> Result<Vec<WireKey>, NotSendable> {
        keys.iter()
            .map(|key| {
                let KeyValue::Column(column) = key.value() else {
                    return Err(NotSendable::KeyTruncated {
                        label: key.label().to_owned(),
                    });
                };
                Ok(WireKey {
                    label: WireLabel::parse(key.label())?,
                    column: column.clone(),
                })
            })
            .collect()
    };
    let joins = leg.tables().joins().iter().map(|join| join.relationship().clone()).collect();
    let table = leg.table().clone();
    let source = leg.source().clone();
    let filters = leg.filters().to_vec();
    Ok(match *leg {
        LegPlan::Fact {
            ref metric,
            ref bucket,
            ref terms,
            range,
            ..
        } => LegWire::Fact {
            source,
            metric: pinned.metric_digest(metric)?,
            table,
            joins,
            bucket: WireBucket {
                grain: bucket.grain(),
                column: bucket.column().clone(),
            },
            keys: keys(leg.keys())?,
            terms: terms
                .iter()
                .map(|term| {
                    Ok(WireTerm {
                        label: WireLabel::parse(term.label())?,
                        term: term.term().clone(),
                    })
                })
                .collect::<Result<_, InvalidWireLabel>>()?,
            filters,
            range,
        },
        LegPlan::Lookup { .. } => LegWire::Lookup {
            source,
            table,
            joins,
            keys: keys(leg.keys())?,
            filters,
        },
    })
}

impl LegWire {
    /// The leg this node runs, or the reason it does not.
    ///
    /// `params` are the values the leg's predicates bind, in placeholder order: the transport reads
    /// them off the Arrow batch beside the document. `pinned` is this node's own pin, and the only
    /// source of what a name means.
    pub fn parse(self, pinned: &PinnedDefinitions, params: Vec<ParamValue>) -> Result<LegPlan, LegNotCertified> {
        certify(self, pinned, params)
    }
}

#[cfg(test)]
mod tests;
