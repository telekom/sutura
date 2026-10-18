use std::collections::{BTreeMap, BTreeSet};

use super::{Anchor, Audience, Description, Dimension, InconsistentDefinitions, Metric};
use crate::expression::Computation;
use crate::measure::{Measure, RequiredFilter};
use crate::model::{ColumnName, DimensionName, Grain, IdentifierCase, MetricName, ModelName};

impl Metric {
    /// A certified metric, or a refusal if two of its dimensions answer to one label.
    ///
    /// **Takes a `Vec<Dimension>` and returns a `Result`, and the argument for that is already
    /// written one level up.** [`Definitions::assemble`](super::Definitions::assemble): *"Takes vectors rather than maps so the
    /// duplicate checks are ours: a caller that built a map first has already silently dropped one
    /// of a duplicated pair."* This constructor took a map, so the check was not ours, and the two
    /// shipped adapters had answered the question differently - `sutura_catalog_local` refused a
    /// duplicate and `sutura_catalog_datahub` collected into a map and kept the last. One content,
    /// two [`Definitions`](super::Definitions). The module header above says two adapters reading the same content must
    /// produce the same one or one of them is wrong, and the golden suite could not see it because
    /// no fixture declares a duplicate.
    ///
    /// **A vector makes the bypass a compile error rather than a rule**, which is why the signature
    /// changed instead of a check being added beside the old one: an adapter cannot collapse the
    /// pair before this point any more, because there is nowhere earlier for it to collapse it. The
    /// field stays a [`BTreeMap`] - the digest is taken over the serialized form and every reader
    /// looks a dimension up by name - so the difference between the parameter and the field is the
    /// whole mechanism.
    ///
    /// **One scan and two refusals, because a duplicate and a folded pair are one rule.** The
    /// comparison is [`IdentifierCase::COARSEST`], which is true of two identical spellings too, so
    /// an exact repeat is the special case and is named as one:
    /// [`InconsistentDefinitions::DuplicateDimension`] says *declares dimension `region` twice*,
    /// which is what an author needs to read, and
    /// [`InconsistentDefinitions::TwoDimensionsOneLabel`] carries the pair. Asking it here rather
    /// than in [`Definitions::assemble`](super::Definitions::assemble) is what makes this a parse: after `Ok`, no two of a
    /// metric's dimensions name one label and nothing downstream re-asks. `assemble` could not have
    /// asked - by the time a [`Metric`] reaches it the map has collapsed an exact pair - and the
    /// DECLARED order is here and nowhere later, so the refusal names the two spellings in the
    /// order the file wrote them. Same shape, and the same argument, as
    /// [`StatementTables::parse`](crate::plan::StatementTables::parse).
    ///
    /// **What a folded pair costs was measured rather than argued.** The pinned `DuckDB`
    /// (`v1.5.5 Variegata d8cdaa33fd`), whose `sutura_sql::Dialect::identifier_case` declares
    /// [`IdentifierCase::InsensitiveAscii`]:
    /// `SELECT "Region" FROM (SELECT 1 AS region, 2 AS "Region")` returns **1** - the `region`
    /// column's value - in a result column named `region`, and raises no ambiguity error.
    /// `SELECT *` over the same subquery projects `region, Region_1`, so the second label a caller
    /// was told to expect is not in the result at all. A wrong number and a missing column, from a
    /// catalog that loaded. Folded under `COARSEST` and not under the serving target's rule for the
    /// reason that constant carries: a bundle is dialect-agnostic, so the coarsest rule is the only
    /// one that cannot be wrong in the direction that returns a number.
    ///
    /// **Quadratic, and nothing caps how many dimensions a metric may declare**, so the limit is
    /// stated rather than implied: there is a cap on a dimension's VALUES
    /// ([`MAX_VALUES_PER_DIMENSION`](super::MAX_VALUES_PER_DIMENSION)) and on the group-by keys one question may ask for
    /// (`crate::query::MAX_DIMENSIONS`), and neither is this. What makes it affordable anyway is
    /// position rather than size - it runs once per metric while a document that was read whole is
    /// being converted, and `Definitions`'s own `check_labels_against_table` is already the same shape
    /// over the same list. A cap on declared dimensions is worth having on its own merits and is not
    /// this constructor's to add.
    ///
    /// The refusal is an [`InconsistentDefinitions`] rather than an error of this constructor's own,
    /// so both adapters map it through the variant they already have for that type and neither
    /// grows a second one.
    pub fn new(
        name: MetricName,
        model: ModelName,
        computation: impl Into<Computation>,
        required_filters: Vec<RequiredFilter>,
        time_column: ColumnName,
        grains: BTreeSet<Grain>,
        dimensions: Vec<Dimension>,
        anchor: Option<Anchor>,
        description: Description,
        audience: Audience,
    ) -> Result<Self, InconsistentDefinitions> {
        let mut declared: BTreeMap<DimensionName, Dimension> = BTreeMap::new();
        for dimension in dimensions {
            let collision = declared
                .keys()
                .find(|earlier| IdentifierCase::COARSEST.names_one_thing(earlier.as_str(), dimension.name.as_str()))
                .cloned();
            if let Some(first) = collision {
                let second = dimension.name;
                return Err(if first == second {
                    InconsistentDefinitions::DuplicateDimension {
                        metric: name,
                        dimension: second,
                    }
                } else {
                    InconsistentDefinitions::TwoDimensionsOneLabel {
                        metric: name,
                        first,
                        second,
                    }
                });
            }
            drop(declared.insert(dimension.name.clone(), dimension));
        }
        Ok(Self {
            name,
            model,
            computation: computation.into(),
            required_filters,
            time_column,
            grains,
            dimensions: declared,
            anchor,
            description,
            audience,
            shared_calendar: None,
        })
    }

    #[inline]
    pub const fn name(&self) -> &MetricName {
        &self.name
    }

    /// Who may see this metric - `docs/adr/0028`.
    #[inline]
    pub const fn audience(&self) -> &Audience {
        &self.audience
    }

    #[inline]
    pub const fn model(&self) -> &ModelName {
        &self.model
    }

    #[inline]
    pub const fn computation(&self) -> &Computation {
        &self.computation
    }

    /// The closed measure, if this metric does not use catalog-authored SQL.
    #[inline]
    pub const fn measure(&self) -> Option<&Measure> {
        self.computation.measure()
    }

    /// The predicates every question about this metric carries, whether the caller asked or not.
    #[inline]
    pub fn required_filters(&self) -> &[RequiredFilter] {
        &self.required_filters
    }

    #[inline]
    pub const fn time_column(&self) -> &ColumnName {
        &self.time_column
    }

    #[inline]
    pub const fn grains(&self) -> &BTreeSet<Grain> {
        &self.grains
    }

    #[inline]
    pub const fn dimensions(&self) -> &BTreeMap<DimensionName, Dimension> {
        &self.dimensions
    }

    #[inline]
    pub const fn anchor(&self) -> Option<&Anchor> {
        self.anchor.as_ref()
    }

    #[inline]
    pub fn description(&self) -> &str {
        self.description.as_str()
    }

    #[inline]
    pub fn supports_grain(&self, grain: Grain) -> bool {
        self.grains.contains(&grain)
    }

    #[inline]
    pub fn dimension(&self, name: &DimensionName) -> Option<&Dimension> {
        self.dimensions.get(name)
    }

    /// Declares the conformed calendar model both fact models of a cross-model ratio reach
    /// through a `via` (`telekom/sutura#780`). A builder rather than a constructor argument,
    /// so every existing caller of [`Metric::new`] compiles unchanged - a one-model metric has
    /// no shared calendar, and its on-disk shape and digest do not move the day this field ships.
    ///
    /// The name is checked at load by [`Definitions::assemble`](super::Definitions::assemble)
    /// ([`InconsistentDefinitions::SharedCalendarNotReachable`]): the calendar model must exist,
    /// and both the metric's own model and every cross-model ratio term's model must reach it
    /// through a declared relationship. The builder does not check, because the relationships
    /// are cross-references only the assembled [`Definitions`](super::Definitions) can see.
    #[inline]
    #[must_use]
    pub fn with_shared_calendar(mut self, calendar: ModelName) -> Self {
        self.shared_calendar = Some(calendar);
        self
    }

    /// The conformed calendar model both fact models of a cross-model ratio reach through a
    /// `via`, or `None` for a one-model metric that buckets through its own `time_column`.
    #[inline]
    pub const fn shared_calendar(&self) -> Option<&ModelName> {
        self.shared_calendar.as_ref()
    }
}
