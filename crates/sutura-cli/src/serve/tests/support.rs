//! The catalog-bundle fixtures shared by `super`'s cells, split out when that file crossed
//! the unexemptable 1000-line cap. This file adds no cell; `crate::tests::bundle_over` still
//! resolves, through the re-export `super` keeps. `bundle_with_an_anchor` and its unanchored twin
//! share `bundle_with_a_metric`, whose manifest declares what the bundle produces so
//! `LocalService::start` accepts it. The `bigquery`-gated items at the foot are the leg-1 token and
//! fixed-catalog fixtures `agent_identity` and `delegation_served` both compose, moved out of
//! `agent_identity` with one change: the catalog's error type is `core::convert::Infallible`.

use std::collections::BTreeSet;

use sutura_domain::capabilities::MetadataCapabilities;
use sutura_domain::catalog::{Definitions, Description, Model};
use sutura_domain::knowledge::Knowledge;
use sutura_domain::model::{ColumnName, ModelName, SourceName, TableName};
use sutura_domain::pinned::{Contribution, ContributionManifest, DefinitionVersion, PinnedDefinitions};

use super::{ENGINE_SOURCE, entry, registry};

/// The ordinary declaration: the engine source, shared, over the example data.
pub(super) fn engine_declared() -> sutura_config::SourceRegistry {
    registry(&entry(ENGINE_SOURCE, "shared-service-user", ""))
}

/// One model as a catalog document names it: the model, its data system, its table.
pub(super) type DeclaredModel<'raw> = (&'raw str, &'raw str, &'raw str);

/// A pinned bundle over exactly the models given, and no metrics.
///
/// Models are all `open_engine` reads: [`sutura_app::sources`] maps over them and `attach` is
/// called once per model, so a metric would add nothing any arm of that function looks at.
/// Leaving them out is what lets one helper stand behind every arm below.
///
/// `pub(crate)` so `crate::boot`'s own tests build their bundles the same way rather than growing a
/// second copy of this that could drift from what a document really produces. Both modules are
/// `#[cfg(test)]`, so nothing compiled into the binary can reach it.
pub(crate) fn bundle_over(models: &[DeclaredModel<'_>]) -> PinnedDefinitions {
    let declared: Vec<Model> = models
        .iter()
        .map(|&(model, source, table)| {
            Model::new(
                ModelName::parse(model).expect("a test model is a model"),
                SourceName::parse(source).expect("a test source is a source"),
                TableName::parse(table).expect("a test table is a table"),
                BTreeSet::from([ColumnName::parse("customer_key").expect("a test column is a column")]),
                Description::default(),
            )
        })
        .collect();
    let definitions = Definitions::assemble(declared, vec![], vec![]).expect("the test bundle is consistent");
    PinnedDefinitions::pin(
        DefinitionVersion::parse("test-1").expect("a test version is a version"),
        definitions,
        Knowledge::none(),
        ContributionManifest::single(
            SourceName::parse("local").expect("a test source is a source"),
            Contribution::of(MetadataCapabilities::nothing()),
        ),
    )
    .expect("the test definitions hash")
}

/// A bundle whose one metric declares an anchor, on `source`.
///
/// The anchor's NUMBER is irrelevant here and nothing executes it: what the boot check reads is
/// that an anchor exists and which source the metric's model sits on. The model is
/// `dim_customer`, so the table behind it is a real file - which keeps a refusal about identity
/// from being satisfied by a missing CSV.
pub(super) fn bundle_with_an_anchor(source: &str) -> PinnedDefinitions {
    bundle_with_a_metric(source, true)
}

/// [`bundle_with_an_anchor`]'s metric with no anchor, so a service starts over it without asking
/// the source anything at boot.
#[cfg(feature = "bigquery")]
pub(super) fn bundle_with_an_unanchored_metric(source: &str) -> PinnedDefinitions {
    bundle_with_a_metric(source, false)
}

fn bundle_with_a_metric(source: &str, anchored: bool) -> PinnedDefinitions {
    use sutura_domain::calendar::{Date, TimeRange};
    use sutura_domain::catalog::{Anchor, AnchorValue, Audience, Metric};
    use sutura_domain::measure::{AggregatedColumn, Measure, Term};
    use sutura_domain::model::{Aggregate, Grain, MetricName};

    let column = |raw: &str| ColumnName::parse(raw).expect("a test column is a column");
    let model = Model::new(
        ModelName::parse("customers").expect("a test model is a model"),
        SourceName::parse(source).expect("a test source is a source"),
        TableName::parse("dim_customer").expect("a test table is a table"),
        BTreeSet::from([column("customer_key"), column("signed_up_on")]),
        Description::default(),
    );
    let range = TimeRange::new(
        Date::parse("2026-06-01").expect("a test date is a date"),
        Date::parse("2026-07-01").expect("a test date is a date"),
    )
    .expect("June is a range");
    let metric = Metric::new(
        MetricName::parse("recurring_revenue").expect("a test metric is a metric"),
        ModelName::parse("customers").expect("a test model is a model"),
        Measure::Simple(Term::Aggregate(AggregatedColumn::new(
            Aggregate::Count,
            column("customer_key"),
        ))),
        Vec::new(),
        column("signed_up_on"),
        BTreeSet::from([Grain::Month]),
        Vec::new(),
        anchored.then(|| Anchor::new(range, AnchorValue::parse("7").expect("a test anchor value is a value"))),
        Description::default(),
        Audience::Open,
    )
    .expect("no dimensions to duplicate");
    let definitions = Definitions::assemble(vec![model], vec![], vec![metric]).expect("the test bundle is consistent");
    // What the bundle really carries, so `LocalService::start`'s faithfulness check accepts it too.
    let produced = MetadataCapabilities::produced(&definitions, &Knowledge::none());
    PinnedDefinitions::pin(
        DefinitionVersion::parse("test-1").expect("a test version is a version"),
        definitions,
        Knowledge::none(),
        ContributionManifest::single(
            SourceName::parse("local").expect("a test source is a source"),
            Contribution::of(produced),
        ),
    )
    .expect("the test definitions hash")
}

/// Every scope this surface has, space-delimited per RFC 6749.
#[cfg(feature = "bigquery")]
fn every_scope() -> String {
    sutura_app::Capability::every()
        .map(sutura_app::Capability::scope)
        .collect::<Vec<&str>>()
        .join(" ")
}

/// A token this deployment would accept, granting every capability the surface has.
#[cfg(feature = "bigquery")]
pub(super) fn accepted_by(subject: &str) -> sutura_dev::issuer::Token {
    sutura_dev::issuer::Token::for_subject(subject).granting(&every_scope())
}

/// A `direct` inbound declaration for `issuer`, reading its key set at `key_set_path`.
#[cfg(feature = "bigquery")]
pub(super) fn direct_overlay(issuer: &sutura_dev::issuer::MockIssuer, key_set_path: &str) -> String {
    format!(
        "security:\n  inbound:\n    mode: \"direct\"\n    resource: \"{}\"\n    \
         authorization_server: \"{}\"\n    key_set_file: \"{key_set_path}\"\n    algorithms: [\"ES256\"]\n",
        issuer.audience(),
        issuer.issuer(),
    )
}

/// A catalog port that hands back a bundle somebody else built - the pass-through
/// `crate::catalog` composes deployments over, so `LocalService::start` validates it.
#[cfg(feature = "bigquery")]
pub(super) struct FixedCatalog {
    bundle: sutura_domain::pinned::PinnedDefinitions,
}

#[cfg(feature = "bigquery")]
impl sutura_domain::pinned::SemanticCatalog for FixedCatalog {
    type Error = core::convert::Infallible;

    const KIND: sutura_domain::pinned::CatalogKind = sutura_domain::pinned::CatalogKind::Golden;

    fn capabilities() -> sutura_domain::capabilities::MetadataCapabilities {
        sutura_domain::capabilities::MetadataCapabilities::everything()
    }

    fn load(&self) -> Result<sutura_domain::pinned::PinnedDefinitions, Self::Error> {
        Ok(self.bundle.clone())
    }
}

#[cfg(feature = "bigquery")]
pub(super) fn catalog_of(bundle: sutura_domain::pinned::PinnedDefinitions) -> FixedCatalog {
    FixedCatalog { bundle }
}
