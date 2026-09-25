//! **One relationship, two key sets, one real database.** The mono half of the compound-join
//! evidence: `federated.rs` proves the two-source topology; this proves the single-source one, over
//! a real embedded `DuckDB`, that a one-column key over a daily-fact-to-monthly-snapshot join is not
//! `many_to_one` the data holds up, while the compound key that also truncates the origin to a
//! month is.
//!
//! # Why this is "over-counts", not "is refused"
//!
//! The single-column relationship cannot be watched answering a doubled number, because
//! `sutura_app::verify_and_validate` refuses a `many_to_one` the target side is not unique under
//! BEFORE anything is served - `crate::federated::a_violated_cardinality_declaration_is_refused_by_both_topologies`
//! is that mechanism's own differential. That refusal is the measurement: the fixture below puts one
//! subscription in the monthly snapshot three times (April, May and June), so a probe over
//! `subscription_key` alone counts three rows and one distinct value - the same shape a rendered
//! `JOIN` on that column alone would multiply June's usage by. The counts this test reads off the
//! refusal are the real evidence that the naive join over-counts by exactly that factor; the compound
//! key is what turns the declaration true, validates, and answers the correct total.
//!
//! # What this does not measure
//!
//! Whether the compound join renders correctly for every dialect - `sutura_sql::generate::tests` and
//! the `fact-compound-join` entry in `tests/golden/legs.rs` are that evidence, and the shared corpus's
//! `usage_subscription` relationship plus `voice-minutes-by-product-family.yaml` is what
//! `tests/differential.rs`'s `agrees_with_the_engine_on_every_question` runs across every registered
//! data system. This file is the one place a wrong declaration and a right one meet over the same
//! real duplication.

use std::collections::BTreeSet;
use std::path::PathBuf;

use sutura_app::{SpendLedger, Warehouses, answer, verify_and_validate};
use sutura_domain::capabilities::MetadataCapabilities;
use sutura_domain::catalog::{
    Audience, Definitions, Description, Dimension, JoinKey, JoinKeys, Metric, Model, Relationship, ViaChain,
};
use sutura_domain::knowledge::Knowledge;
use sutura_domain::measure::{AggregatedColumn, Measure, Term};
use sutura_domain::model::{
    Aggregate, ColumnName, DimensionName, Grain, JoinType, MetricName, ModelName, RelationshipName, SourceName, TableName,
};
use sutura_domain::pinned::{Contribution, ContributionManifest, DefinitionVersion, NotValidated, PinnedDefinitions};
use sutura_domain::plan::RowCeiling;
use sutura_domain::query::Query;
use sutura_domain::warehouse::{Real, Value};

use crate::adapters::{a_caller, deadline, shared_credential};

fn column(raw: &str) -> ColumnName {
    ColumnName::parse(raw).expect("a test column is a column")
}

fn model_name(raw: &str) -> ModelName {
    ModelName::parse(raw).expect("a test model is a model")
}

fn metric_name() -> MetricName {
    MetricName::parse("voice_minutes_fixture").expect("a test metric is a metric")
}

fn relationship_name() -> RelationshipName {
    RelationshipName::parse("usage_snapshot_fixture").expect("a test relationship is a relationship")
}

fn source() -> SourceName {
    SourceName::parse("local").expect("a test source is a source")
}

/// The directory this file's two CSVs live in, written once per process.
///
/// One subscription, three monthly snapshot rows - April, May and June - which is the whole of the
/// duplication the declared-key probe measures. Usage carries two June rows, `7.5` and `2.5`
/// minutes, so the correct total is exactly `10.0` and a naive `subscription_key`-only join would
/// have produced `30.0` had anything let it run.
fn fixture_dir() -> PathBuf {
    let root = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("compound-join-fixture-{}", std::process::id()));
    if !root.exists() {
        std::fs::create_dir_all(&root).unwrap_or_else(|e| panic!("could not create {}: {e}", root.display()));
        std::fs::write(
            root.join("usage.csv"),
            "usage_date,subscription_key,voice_min\n2026-06-01,1,7.5\n2026-06-15,1,2.5\n",
        )
        .expect("the usage fixture writes");
        std::fs::write(
            root.join("snapshot.csv"),
            "month,subscription_key,product_family\n2026-04-01,1,mobile\n2026-05-01,1,mobile\n2026-06-01,1,mobile\n",
        )
        .expect("the snapshot fixture writes");
    }
    root
}

fn usage_model() -> Model {
    Model::new(
        model_name("usage_fixture"),
        source(),
        TableName::parse("usage_fixture").expect("a test table is a table"),
        BTreeSet::from([column("usage_date"), column("subscription_key"), column("voice_min")]),
        Description::default(),
    )
}

fn snapshot_model() -> Model {
    Model::new(
        model_name("snapshot_fixture"),
        source(),
        TableName::parse("snapshot_fixture").expect("a test table is a table"),
        BTreeSet::from([column("month"), column("subscription_key"), column("product_family")]),
        Description::default(),
    )
}

/// The relationship, either shape - `keys` is the whole of what differs between the two bundles.
fn relationship(keys: JoinKeys) -> Relationship {
    Relationship::new(
        relationship_name(),
        model_name("usage_fixture"),
        model_name("snapshot_fixture"),
        JoinType::ManyToOne,
        keys,
    )
}

fn metric() -> Metric {
    Metric::new(
        metric_name(),
        model_name("usage_fixture"),
        Measure::Simple(Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("voice_min")))),
        Vec::new(),
        column("usage_date"),
        BTreeSet::from([Grain::Month]),
        vec![Dimension::new(
            DimensionName::parse("product_family").expect("a test dimension is a dimension"),
            column("product_family"),
            Some(ViaChain::of(vec![relationship_name()]).expect("one hop is a chain")),
            None,
            Description::default(),
        )],
        None,
        Description::default(),
        Audience::Open,
    )
    .expect("one dimension, no duplicate")
}

fn bundle(keys: JoinKeys) -> PinnedDefinitions {
    let definitions = Definitions::assemble(
        vec![usage_model(), snapshot_model()],
        vec![relationship(keys)],
        vec![metric()],
    )
    .expect("the fixture models, relationship and metric hold together");
    PinnedDefinitions::pin(
        DefinitionVersion::parse("compound-join-fixture-1").expect("a test version is a version"),
        definitions,
        Knowledge::none(),
        ContributionManifest::single(source(), Contribution::of(MetadataCapabilities::nothing())),
    )
    .expect("the fixture definitions hash")
}

fn naive_bundle() -> PinnedDefinitions {
    bundle(JoinKeys::single(JoinKey::Equal {
        origin: column("subscription_key"),
        target: column("subscription_key"),
    }))
}

fn compound_bundle() -> PinnedDefinitions {
    bundle(
        JoinKeys::of(vec![
            JoinKey::Equal {
                origin: column("subscription_key"),
                target: column("subscription_key"),
            },
            JoinKey::TruncatedEqual {
                origin: column("usage_date"),
                grain: Grain::Month,
                target: column("month"),
            },
        ])
        .expect("two keys is a non-empty set"),
    )
}

fn warehouse() -> sutura_exec_duckdb::DuckDbWarehouse {
    let dir = fixture_dir();
    let warehouse = sutura_exec_duckdb::DuckDbWarehouse::in_memory(source(), crate::adapters::posture())
        .expect("an in-memory database opens");
    warehouse
        .attach_csv(
            &TableName::parse("usage_fixture").expect("a test table is a table"),
            &dir.join("usage.csv"),
        )
        .expect("the usage fixture attaches");
    warehouse
        .attach_csv(
            &TableName::parse("snapshot_fixture").expect("a test table is a table"),
            &dir.join("snapshot.csv"),
        )
        .expect("the snapshot fixture attaches");
    warehouse
}

fn question() -> Query {
    serde_norway::from_str(
        "metrics: [voice_minutes_fixture]\ngrain: month\nrange: { start: 2026-06-01, end: 2026-07-01 }\n\
         dimensions: [product_family]\n",
    )
    .expect("the fixture question is a question")
}

/// **The measurement.** A one-column key over this real duplication is not a declaration the data
/// holds up - refused before anything is served, naming exactly the fan-out the fixture built - and
/// the compound key that also matches the month is, and answers the correct, un-multiplied total.
#[test]
fn a_compound_key_validates_the_real_duplication_a_single_column_key_does_not() {
    let warehouses = Warehouses::of(warehouse());

    let refused = verify_and_validate(naive_bundle(), &warehouses)
        .expect_err("subscription_key alone repeats three times in the snapshot, which is not many_to_one");
    let NotValidated::DeclaredKeyNotUnique(ref violation) = refused else {
        panic!("a duplicated target key is refused as one, not as {refused:?}");
    };
    assert_eq!(violation.relationship(), &relationship_name());
    // Three snapshot rows, one distinct subscription: the same 3-to-1 fan-out a rendered `JOIN` on
    // `subscription_key` alone would have multiplied June's two usage rows by, had this bundle ever
    // been allowed to answer with it.
    assert_eq!(violation.counts().rows(), 3);
    assert_eq!(violation.counts().distinct(), 1);
    assert_eq!(violation.counts().duplicated(), 2);

    let validated =
        verify_and_validate(compound_bundle(), &warehouses).expect("subscription_key AND month together identify one row");

    let combiner = sutura_exec_datafusion::DataFusionCombiner::new().expect("a combiner builds");
    let outcome = answer(
        &validated,
        &question(),
        &a_caller(),
        &shared_credential(),
        &warehouses,
        &combiner,
        1 << 30,
        deadline(),
        &SpendLedger::no_budget(),
        RowCeiling::DEFAULT,
    )
    .expect("the compound bundle answers")
    .into_outcome();

    let sutura_domain::query::ToolOutcome::Answer { rows, .. } = outcome else {
        panic!("the compound bundle must answer, not refuse: {outcome:?}");
    };
    assert_eq!(
        rows.rows().len(),
        1,
        "one subscription, one month, one product family: one row"
    );
    let measure = rows
        .column_index(metric_name().as_str())
        .expect("the answer projects the measure under the metric's own name");
    // `7.5 + 2.5`, June's two usage rows and no others - not `30.0`, which is what three joined
    // snapshot months would have produced. This is the number the naive bundle above never gets to
    // compute, because it is refused first.
    assert_eq!(
        rows.cell(0, measure),
        Some(&Value::Real(Real::parse(10.0).expect("ten is finite"))),
        "the compound join must not multiply June's usage by the snapshot's other two months: {rows:?}"
    );
}
