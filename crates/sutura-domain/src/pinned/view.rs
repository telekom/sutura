//! A caller-scoped read of one pinned bundle - `docs/adr/0028-who-may-see-a-metric.md`.
//!
//! [`crate::pinned::SemanticCatalog::load`] takes no request context, so a per-caller filter
//! cannot live there. [`ScopedView`] borrows the bundle instead. Mapping a claim to a
//! [`GrantedAudiences`] is deployment policy, done above this crate.

use crate::catalog::{GrantedAudiences, Metric};
use crate::model::MetricName;
use crate::pinned::PinnedDefinitions;

/// Everything, or a caller's granted set. See [`ScopedView::everything`] and
/// [`ScopedView::granted_by`] for who may construct each.
#[derive(Debug)]
enum Scope {
    Everything,
    Granted(GrantedAudiences),
}

/// A read of a [`PinnedDefinitions`] narrowed to what one caller may see.
///
/// Both fields are private; no struct literal outside this module compiles:
///
/// ```compile_fail
/// use sutura_domain::pinned::{PinnedDefinitions, view::ScopedView};
///
/// fn _by_hand(pinned: &PinnedDefinitions) -> ScopedView<'_> {
///     ScopedView { pinned, scope: sutura_domain::pinned::view::Scope::Everything }
/// }
/// ```
///
/// The compiling twin:
///
/// ```
/// use sutura_domain::pinned::PinnedDefinitions;
/// use sutura_domain::pinned::view::ScopedView;
///
/// fn _read(pinned: &PinnedDefinitions) -> usize {
///     ScopedView::everything(pinned).metrics().count()
/// }
/// ```
#[derive(Debug)]
pub struct ScopedView<'a> {
    pinned: &'a PinnedDefinitions,
    scope: Scope,
}

impl<'a> ScopedView<'a> {
    /// For the surfaces `docs/adr/0028` names as retaining the whole bundle. Public, so not
    /// sealed against misuse; what it buys is that nothing downstream renders a catalog from a
    /// bare `&PinnedDefinitions`.
    #[inline]
    #[must_use]
    pub const fn everything(pinned: &'a PinnedDefinitions) -> Self {
        Self {
            pinned,
            scope: Scope::Everything,
        }
    }

    #[inline]
    #[must_use]
    pub const fn granted_by(pinned: &'a PinnedDefinitions, granted: GrantedAudiences) -> Self {
        Self {
            pinned,
            scope: Scope::Granted(granted),
        }
    }

    /// For provenance, which is always the whole bundle's digest.
    #[inline]
    #[must_use]
    pub const fn pinned(&self) -> &'a PinnedDefinitions {
        self.pinned
    }

    /// Absent, not undescribed, when this caller may not see it.
    #[must_use]
    pub fn metric(&self, name: &MetricName) -> Option<&'a Metric> {
        let metric = self.pinned.definitions().metric(name)?;
        self.visible(metric).then_some(metric)
    }

    pub fn metrics(&self) -> impl Iterator<Item = &'a Metric> + '_ {
        self.pinned
            .definitions()
            .metrics()
            .values()
            .filter(|metric| self.visible(metric))
    }

    fn visible(&self, metric: &Metric) -> bool {
        match &self.scope {
            Scope::Everything => true,
            Scope::Granted(granted) => metric.audience().visible_to(granted.as_set()),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{GrantedAudiences, ScopedView};
    use crate::capabilities::MetadataCapabilities;
    use crate::catalog::{Audience, AudienceGrant, Definitions, Description, Metric, Model};
    use crate::knowledge::Knowledge;
    use crate::measure::{AggregatedColumn, Measure, Term};
    use crate::model::{Aggregate, AudienceId, ColumnName, Grain, MetricName, ModelName, SourceName, TableName};
    use crate::pinned::{Contribution, ContributionManifest, DefinitionVersion, PinnedDefinitions};

    fn column(raw: &str) -> ColumnName {
        ColumnName::parse(raw).expect("a test column is a column")
    }

    fn audience_id(raw: &str) -> AudienceId {
        AudienceId::parse(raw).expect("a test audience id is one")
    }

    fn model() -> Model {
        Model::new(
            ModelName::parse("orders").expect("a test model is a model"),
            SourceName::parse("local").expect("a test source is a source"),
            TableName::parse("orders").expect("a test table is a table"),
            BTreeSet::from([column("amount"), column("occurred_at")]),
            Description::default(),
        )
    }

    fn metric(name: &str, audience: Audience) -> Metric {
        Metric::new(
            MetricName::parse(name).expect("a test metric is a metric"),
            ModelName::parse("orders").expect("a test model is a model"),
            Measure::Simple(Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("amount")))),
            Vec::new(),
            column("occurred_at"),
            BTreeSet::from([Grain::Month]),
            Vec::new(),
            None,
            Description::default(),
            audience,
        )
        .expect("no dimensions to duplicate")
    }

    fn restricted(id: &str) -> Audience {
        Audience::Restricted(AudienceGrant::parse(BTreeSet::from([audience_id(id)])).expect("one id grants"))
    }

    fn bundle(metrics: Vec<Metric>) -> PinnedDefinitions {
        let definitions = Definitions::assemble(vec![model()], Vec::new(), metrics).expect("the test bundle is consistent");
        PinnedDefinitions::pin(
            DefinitionVersion::parse("test-1").expect("a test version is a version"),
            definitions.clone(),
            Knowledge::none(),
            ContributionManifest::single(
                SourceName::parse("local").expect("a test source is a source"),
                Contribution::of(MetadataCapabilities::produced(&definitions, &Knowledge::none())),
            ),
        )
        .expect("the test bundle hashes")
    }

    #[test]
    fn everything_sees_a_restricted_metric_that_nobody_was_granted() {
        let pinned = bundle(vec![
            metric("open_metric", Audience::Open),
            metric("finance_metric", restricted("finance")),
        ]);
        let view = ScopedView::everything(&pinned);
        assert_eq!(view.metrics().count(), 2);
        assert!(view.metric(&MetricName::parse("finance_metric").expect("a name")).is_some());
    }

    #[test]
    fn a_caller_with_no_grant_sees_only_the_open_metrics() {
        let pinned = bundle(vec![
            metric("open_metric", Audience::Open),
            metric("finance_metric", restricted("finance")),
        ]);
        let view = ScopedView::granted_by(&pinned, GrantedAudiences::none());
        assert_eq!(
            view.metrics().map(|metric| metric.name().as_str()).collect::<Vec<_>>(),
            vec!["open_metric"]
        );
        assert!(view.metric(&MetricName::parse("finance_metric").expect("a name")).is_none());
    }

    #[test]
    fn two_callers_with_different_grants_see_different_catalogs_from_one_bundle() {
        let pinned = bundle(vec![
            metric("open_metric", Audience::Open),
            metric("finance_metric", restricted("finance")),
        ]);
        let outsider = ScopedView::granted_by(&pinned, GrantedAudiences::none());
        let finance = ScopedView::granted_by(&pinned, GrantedAudiences::of(BTreeSet::from([audience_id("finance")])));
        assert_eq!(outsider.metrics().count(), 1);
        assert_eq!(finance.metrics().count(), 2);
        // Same bundle, so the digest a caller reads off provenance is identical for both.
        assert_eq!(outsider.pinned().digest(), finance.pinned().digest());
    }

    /// `docs/adr/0028`: an unmapped group neither grants nor vetoes a mapped one in the same claim.
    #[test]
    fn one_mapped_audience_among_several_granted_is_enough() {
        let pinned = bundle(vec![metric("finance_metric", restricted("finance"))]);
        let view = ScopedView::granted_by(
            &pinned,
            GrantedAudiences::of(BTreeSet::from([audience_id("engineering"), audience_id("finance")])),
        );
        assert_eq!(view.metrics().count(), 1);
    }
}
