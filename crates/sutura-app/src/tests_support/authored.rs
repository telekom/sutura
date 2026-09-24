//! A fake that declares [`Warehouse::EXECUTES_AUTHORED_SQL`], and the bundle that exercises it.
//!
//! Split out of `tests_support.rs` for that file's own reason - it was at the `max-lines` cap this
//! record's `deadline` module already sits beside, and this pair (the fake plus its one-metric
//! bundle) is the other self-contained chunk with no callers outside `crate::tests`.

use std::collections::BTreeMap;

use sutura_domain::identity::Presented;
use sutura_domain::model::SourceName;
use sutura_domain::pinned::PinnedDefinitions;
use sutura_domain::plan::Executable;
use sutura_domain::source::{ImpersonationCapability, SourcePosture};
use sutura_domain::warehouse::deadline::Deadline;
use sutura_domain::warehouse::{AnchorRows, ResultBatches, Warehouse};

use super::{AdapterFailure, DriverFailure, FixedWarehouse};

/// A fake whose only additional claim is that it can execute catalog-authored SQL.
///
/// Used to prove the startup gate has both directions. It does not prove an authored query can be
/// planned or answered; the production plan carries no SQL, and no published adapter makes this
/// declaration.
pub(crate) struct AuthoredWarehouse(FixedWarehouse);

impl AuthoredWarehouse {
    pub(crate) const fn new(source: SourceName, posture: SourcePosture) -> Self {
        Self(FixedWarehouse::new(source, posture))
    }
}

impl Warehouse for AuthoredWarehouse {
    type Error = AdapterFailure;

    const IMPERSONATION: ImpersonationCapability = ImpersonationCapability::NoPlaceForASubject;
    const EXECUTES_AUTHORED_SQL: bool = true;

    fn source(&self) -> &SourceName {
        self.0.source()
    }

    fn posture(&self) -> &SourcePosture {
        self.0.posture()
    }

    fn execute(
        &self,
        executable: Executable<'_>,
        presented: &Presented,
        deadline: Deadline,
    ) -> Result<ResultBatches, Self::Error> {
        self.0.execute(executable, presented, deadline)
    }

    fn verify_anchor(&self, _plan: sutura_domain::plan::AnchorPlan<'_>) -> Result<AnchorRows, Self::Error> {
        self.0
            .result
            .clone()
            .map(AnchorRows::of)
            .ok_or(AdapterFailure::Statement { cause: DriverFailure })
    }
}

/// One authored metric over one model, with no anchors or relationships.
pub(crate) fn authored_bundle(metric: sutura_domain::model::MetricName, source: SourceName) -> PinnedDefinitions {
    use std::collections::BTreeSet;

    use sutura_domain::capabilities::{DefinitionCapabilities, DefinitionKind, MetadataCapabilities};
    use sutura_domain::catalog::{Audience, Definitions, Description, Metric, Model};
    use sutura_domain::expression::{AuthoredSql, Computation, DialectTag, SqlFragment};
    use sutura_domain::knowledge::{Knowledge, KnowledgeCapabilities};
    use sutura_domain::model::{ColumnName, Grain, ModelName, TableName};
    use sutura_domain::pinned::{Contribution, ContributionManifest, DefinitionVersion, PinnedDefinitions};

    let column = |raw: &str| ColumnName::parse(raw).expect("a test column is a column");
    let model_name = ModelName::parse("orders").expect("a test model is a model");
    let model = Model::new(
        model_name.clone(),
        source.clone(),
        TableName::parse("orders").expect("a test table is a table"),
        BTreeSet::from([column("amount_cents"), column("order_date")]),
        Description::default(),
    );
    let fragment = SqlFragment::parse("MAX(amount_cents) - MIN(amount_cents)").expect("a test fragment is SQL");
    let authored =
        AuthoredSql::new(BTreeMap::from([(DialectTag::portable(), fragment)])).expect("one portable fragment is authored SQL");
    let authored_metric = Metric::new(
        metric,
        model_name,
        Computation::AuthoredSql(authored),
        Vec::new(),
        column("order_date"),
        BTreeSet::from([Grain::Month]),
        Vec::new(),
        None,
        Description::default(),
        Audience::Open,
    )
    .expect("no dimensions to duplicate");
    let declared = MetadataCapabilities::of(
        DefinitionCapabilities::of([DefinitionKind::Structure, DefinitionKind::Metrics, DefinitionKind::Grains]),
        KnowledgeCapabilities::none(),
    );
    PinnedDefinitions::pin(
        DefinitionVersion::parse("test-1").expect("a test version is a version"),
        Definitions::assemble(vec![model], vec![], vec![authored_metric]).expect("the test bundle is consistent"),
        Knowledge::none(),
        ContributionManifest::single(source, Contribution::of(declared)),
    )
    .expect("the test definitions hash")
}
