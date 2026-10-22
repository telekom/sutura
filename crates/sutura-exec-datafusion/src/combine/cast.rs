//! The cast a relationship's declared key comparison asks of one link column.
//!
//! **A value that does not cast refuses the answer.** `cast_with_options` is called with `safe:
//! false`: the default (`safe: true`) turns a failed value into NULL, and a NULL key drops its row
//! under an inner join and lands in the unmatched bucket under a left one - the silent non-match
//! `telekom/sutura#138` closed. Arrow's own message names the value, so the cause is dropped and the
//! refusal carries the two kinds only.
//!
//! **Two casts, because [`sutura_domain::plan::LinkKind::reaches`] admits two.** Text is read as an
//! `Int64`, never as the anchor's own type: a text cast to a zero-scale decimal goes through the
//! rounding path. An integer is widened to the anchor's decimal scale at the widest precision its
//! width holds, so the join compares like with like and a value past that precision is refused
//! rather than wrapped.
//!
//! The cast leg is a second copy of one column's batches, held outside the memory pool exactly as
//! the registered legs are, and bounded by that leg's own rows.

use std::sync::Arc;

use datafusion::arrow::array::RecordBatch;
use datafusion::arrow::compute::{CastOptions, cast_with_options};
use datafusion::arrow::datatypes::{DataType, Schema, SchemaRef};
use sutura_domain::plan::LinkKind;
use sutura_domain::warehouse::ResultBatches;

use super::CombineError;
use super::schema::LegSchema;

/// One link column's cast: what it holds, what the relationship declares, and the Arrow type between.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Cast {
    from: LinkKind,
    to: LinkKind,
    target: DataType,
}

/// A leg's schema and batches once its link column is cast.
pub(super) type CastLeg = (SchemaRef, Vec<RecordBatch>);

/// What each leg joined on the first link is cast to, `None` for a leg already of the declared kind.
#[derive(Debug, Default)]
pub(super) struct LinkCasts {
    pub(super) fact: Option<Cast>,
    pub(super) lookup: Option<Cast>,
    pub(super) second_fact: Option<Cast>,
}

/// The Arrow type a key of kind `own` is cast to under `declared`, anchored on the other leg's type.
///
/// `None` is a pair [`LinkKind::reaches`] does not admit, so a caller that has already seen
/// [`LinkKind::joins`] pass never gets it for a leg that is not of the declared kind.
const fn target(declared: LinkKind, own: LinkKind, anchor: &DataType) -> Option<DataType> {
    match (declared, own) {
        (LinkKind::ExactInteger, LinkKind::Text) => Some(DataType::Int64),
        (LinkKind::ExactDecimal, LinkKind::ExactInteger) => match *anchor {
            DataType::Decimal128(_, scale) => Some(DataType::Decimal128(38, scale)),
            DataType::Decimal256(_, scale) => Some(DataType::Decimal256(76, scale)),
            _ => None,
        },
        _ => None,
    }
}

impl LinkCasts {
    /// The casts a declared comparison asks of the legs joined on `label`.
    ///
    /// The second fact leg is a fact leg beside the first, so it is cast as the first is: to the
    /// declared kind, against the lookup leg's anchor type. Its own relationship's declaration is
    /// not read.
    ///
    /// # Errors
    ///
    /// [`CombineError::LinkKindsNotJoined`] for a leg not of the declared kind that has no cast to it.
    pub(super) fn of(
        declared: Option<LinkKind>,
        label: &str,
        fact: &LegSchema,
        lookup: &LegSchema,
        second_fact: Option<&LegSchema>,
    ) -> Result<Self, CombineError> {
        let Some(declared) = declared else {
            return Ok(Self::default());
        };
        let (fact_kind, lookup_kind) = (fact.link_kind(label)?, lookup.link_kind(label)?);
        let (fact_type, lookup_type) = (fact.kind_of(label)?, lookup.kind_of(label)?);
        let cast_of = |own: LinkKind, anchor: &DataType| {
            if own == declared {
                return Ok(None);
            }
            target(declared, own, anchor)
                .map(|target| {
                    Some(Cast {
                        from: own,
                        to: declared,
                        target,
                    })
                })
                // Unreachable behind `agreeing_link`, which only passes a pair `reaches` admits. Kept
                // closed rather than dropped: a leg left uncast would meet the engine's own coercion.
                .ok_or(CombineError::LinkKindsNotJoined {
                    declared: declared.word(),
                    fact: fact_kind.word(),
                    lookup: lookup_kind.word(),
                })
        };
        Ok(Self {
            fact: cast_of(fact_kind, lookup_type)?,
            lookup: cast_of(lookup_kind, fact_type)?,
            second_fact: second_fact
                .map(|second| cast_of(second.link_kind(label)?, lookup_type))
                .transpose()?
                .flatten(),
        })
    }
}

impl Cast {
    /// `batches` with the link column `label` cast, under its schema with that field retyped.
    ///
    /// # Errors
    ///
    /// [`CombineError::LinkValueNotCast`] if any value does not cast, carrying the two kinds and no
    /// value; [`CombineError::MissingColumn`] if the leg has no such column.
    pub(super) fn apply(&self, side: &'static str, batches: &ResultBatches, label: &str) -> Result<CastLeg, CombineError> {
        let at = batches
            .schema()
            .index_of(label)
            .map_err(|_unknown| CombineError::MissingColumn {
                side,
                label: String::from(label),
            })?;
        let fields: Vec<_> = batches
            .schema()
            .fields()
            .iter()
            .enumerate()
            .map(|(position, field)| {
                if position == at {
                    Arc::new(field.as_ref().clone().with_data_type(self.target.clone()))
                } else {
                    Arc::clone(field)
                }
            })
            .collect();
        let schema = Arc::new(Schema::new_with_metadata(fields, batches.schema().metadata().clone()));
        let options = CastOptions {
            safe: false,
            ..CastOptions::default()
        };
        let cast = batches
            .batches()
            .iter()
            .map(|batch| {
                let columns = batch
                    .columns()
                    .iter()
                    .enumerate()
                    .map(|(position, column)| {
                        if position == at {
                            cast_with_options(column, &self.target, &options).map_err(|_names_the_value| {
                                CombineError::LinkValueNotCast {
                                    side,
                                    from: self.from.word(),
                                    to: self.to.word(),
                                }
                            })
                        } else {
                            Ok(Arc::clone(column))
                        }
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                RecordBatch::try_new(Arc::clone(&schema), columns).map_err(|cause| CombineError::Register {
                    side,
                    cause: cause.into(),
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok((schema, cast))
    }
}
