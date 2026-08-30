//! What this composition root refuses to start with.
//!
//! Every test here drives `open_engine` or one of the checks beside it, because those are the refusals
//! only a composition root can make: whether the LINKED adapter can carry a per-subject credential,
//! and whether the BUNDLE declares an anchor on a source with no identity to re-run it under.
//! `sutura-config` owns the ones a file alone decides and tests them there.
//!
//! The `sources:` trees these tests read are built through `sutura_config::Settings::load`, not through
//! `SourceRegistry::parse` - that constructor is crate-private to `sutura-config` on purpose, so a root
//! test reaching past it would exercise a path no deployment takes.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use sutura_config::EngineWorkers;
use sutura_domain::catalog::{Definitions, Description, Model};
use sutura_domain::knowledge::Knowledge;
use sutura_domain::model::{ColumnName, ModelName, SourceName, TableName};
use sutura_domain::pinned::{DefinitionVersion, PinnedDefinitions};

use super::{ENGINE_SOURCE, Opened, open_engine, refuse_unattached};

fn tables(names: &[&str]) -> BTreeSet<TableName> {
    names
        .iter()
        .map(|raw| TableName::parse(raw).expect("a test table is a table"))
        .collect()
}

/// The example deployment's data directory, which is the one the quickstart points at.
fn data() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/single-player/data")
}

/// The narrowest runtime the engine will take, so a refusal that fires before it is built costs
/// nothing and the one that fires after builds a single-threaded engine.
///
/// It became the whole settings group rather than a worker count when the working-set ceiling
/// arrived: `open_engine` needs the ceiling to build the memory pool, and passing the group is
/// what stops a test choosing a different ceiling from the one a deployment would run with.
/// Every other value is the embedded default.
fn one_worker() -> sutura_config::RuntimeSettings {
    use sutura_config::{AdmissionTimeout, QueryConcurrency, ShutdownGrace, WorkingSetCeiling};

    sutura_config::RuntimeSettings::new(
        QueryConcurrency::parse(1).expect("one query at a time is a concurrency"),
        AdmissionTimeout::parse(1).expect("a second is an admission timeout"),
        EngineWorkers::parse(Some(1)).expect("one worker is a worker count"),
        // The ceiling a deployment would run with, and `None` for the machine's memory because a
        // test must not refuse on the host it happens to run on.
        WorkingSetCeiling::parse(WorkingSetCeiling::DEFAULT_BYTES, None).expect("the default ceiling parses"),
        ShutdownGrace::parse(1).expect("a second is a grace period"),
    )
}

/// The refusal a startup produced, or a failed test.
///
/// `expect_err` wants a `Debug` on the success type, and `Opened` holds a live engine and a table
/// set. Dropping the success value here rather than deriving `Debug` on it keeps a test's
/// convenience out of the composition root's types, and the message the caller passes is what
/// says which arm was expected to fire.
fn refusal(opened: Result<Opened, String>, expected: &str) -> String {
    opened.map(drop).expect_err(expected)
}

/// A `sources:` tree, built through the REAL settings loader.
///
/// **Not `SourceRegistry::parse`, which is crate-private to `sutura-config` on purpose** - the raw
/// shapes are private there, so `Settings::parse` is the only door and the refusals cannot be
/// skipped. A composition-root test that reached past it would be exercising a path no deployment
/// takes; going through an overlay means these tests read the same tree a `base.yaml` would produce.
///
/// The mode is `single-user`, so a shared source needs no per-source acknowledgement here - the
/// acknowledgement refusal is `sutura-config`'s own test, and repeating it would be asserting the
/// same mechanism in the wrong crate.
fn registry(entries: &str) -> sutura_config::SourceRegistry {
    let overlay = format!(
        "security:\n  identity: \"single-user\"\n  single_user_because: \"the test deployment reads its own fixture \
             files\"\nsources:\n{entries}"
    );
    sutura_config::Settings::load(&sutura_config::Sources::defaults(crate::Environment::Development).with_overlay(overlay))
        .expect("the test settings load")
        .sources()
        .clone()
}

/// One `sources:` entry over the example data directory.
///
/// The path is absolute because a relative one is refused at parse - `sutura-config` owns that
/// refusal and tests it - and because a service's working directory is whatever its supervisor
/// chose.
fn entry(alias: &str, posture: &str, extra: &str) -> String {
    format!(
        "  {alias}:\n    kind: \"files\"\n    data_dir: \"{}\"\n    posture: \"{posture}\"\n{extra}",
        data().display()
    )
}

/// The ordinary declaration: the engine source, shared, over the example data.
fn engine_declared() -> sutura_config::SourceRegistry {
    registry(&entry(ENGINE_SOURCE, "shared-service-user", ""))
}

/// One model as a catalog document names it: the model, its data system, its table.
type DeclaredModel<'raw> = (&'raw str, &'raw str, &'raw str);

/// A pinned bundle over exactly the models given, and no metrics.
///
/// Models are all `open_engine` reads: [`sutura_app::sources`] maps over them and `attach` is
/// called once per model, so a metric would add nothing any arm of that function looks at.
/// Leaving them out is what lets one helper stand behind every arm below.
fn bundle_over(models: &[DeclaredModel<'_>]) -> PinnedDefinitions {
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
    )
    .expect("the test definitions hash")
}

/// A bundle whose one metric declares an anchor, on `source`.
///
/// The anchor's NUMBER is irrelevant here and nothing executes it: what the boot check reads is
/// that an anchor exists and which source the metric's model sits on. The model is
/// `dim_customer`, so the table behind it is a real file - which keeps a refusal about identity
/// from being satisfied by a missing CSV.
fn bundle_with_an_anchor(source: &str) -> PinnedDefinitions {
    use std::collections::BTreeMap;

    use sutura_domain::calendar::{Date, TimeRange};
    use sutura_domain::catalog::{Anchor, Metric};
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
        BTreeMap::new(),
        Some(Anchor::new(range, String::from("7"))),
        Description::default(),
    );
    let definitions = Definitions::assemble(vec![model], vec![], vec![metric]).expect("the test bundle is consistent");
    PinnedDefinitions::pin(
        DefinitionVersion::parse("test-1").expect("a test version is a version"),
        definitions,
        Knowledge::none(),
    )
    .expect("the test definitions hash")
}

#[test]
fn a_catalog_naming_a_source_with_no_declaration_starts_nothing() {
    // **This replaces `a_catalog_spanning_two_data_systems_starts_nothing`, and the replacement is
    // the point of this branch rather than a regression.** That test asserted a constraint the
    // source registry removes: a catalog whose models sit on two DECLARED sources is now a
    // deployment that opens two engines, and only a QUESTION whose plan spans both is refused -
    // once the splitter produces a federated plan it is refused by `answer` as
    // `FederationNotExecutable` while no adapter executes a leg (three or more sources are refused
    // at plan time, which is where the blanket bound always belonged).
    //
    // What survives, and what this asserts, is the half that is still a misconfiguration: a source
    // the catalog reads and the deployment never declared. There is nothing to open it as, no
    // location for its files and no posture for its queries, and an arm that defaulted any of
    // those would be a deployment serving data under a configuration nobody wrote.
    //
    // `local` is deliberately one of the pair and its table has a real file in the example's data
    // directory, which is what makes this discriminate: an arm that opened the sources it
    // recognised and skipped the rest would find `local`, find `dim_customer.csv`, and serve half a
    // catalog under the whole bundle's digest.
    let error = refusal(
        open_engine(
            &bundle_over(&[
                ("customers", ENGINE_SOURCE, "dim_customer"),
                ("products", "production_warehouse", "dim_product"),
            ]),
            &engine_declared(),
            one_worker(),
        ),
        "a catalog reading an undeclared source must not get an engine",
    );
    assert!(
        error.contains("production_warehouse") && error.contains("no `sources.production_warehouse` entry"),
        "the refusal must name the source with no entry: {error}"
    );
    // NOT a neighbouring arm, which is the half that stops this passing on the wrong branch.
    // `local` IS declared and opens fine, so an arm that refused the whole start for a reason
    // about `local` would be green with the undeclared-source arm gone.
    assert!(
        !error.contains("no fallback") && !error.contains("declares no models"),
        "this is the undeclared arm, not the capability or empty one: {error}"
    );
}

#[test]
fn two_declared_sources_both_open_and_each_carries_its_own_posture() {
    // The Done-when of this branch, in the binary that serves: two sources configured, each saying
    // what it is. Both are `local`-shaped here because the only adapter this build links is the
    // in-process engine - so the second entry is a second engine over the same directory, which is
    // enough to prove the registry routes by name and hands each adapter its own declaration.
    //
    // Both models name a real file in the example's data directory, which is what keeps this test
    // about routing rather than about a missing CSV.
    let two = registry(&format!(
        "{}{}",
        entry(ENGINE_SOURCE, "shared-service-user", ""),
        entry("second", "shared-service-user", "")
    ));
    let opened = open_engine(
        &bundle_over(&[
            ("customers", ENGINE_SOURCE, "dim_customer"),
            ("products", ENGINE_SOURCE, "dim_product"),
        ]),
        &two,
        one_worker(),
    )
    .expect("two declared sources open");
    assert_eq!(
        opened
            .engines
            .postures()
            .map(|(name, posture)| (String::from(name.as_str()), posture.as_str()))
            .collect::<Vec<(String, &str)>>(),
        vec![(String::from(ENGINE_SOURCE), "shared-service-user")],
        "an engine is opened for each source the CATALOG names, not for every entry configured - a \
             declared source no model reads is a configuration a deployment may hold without paying for \
             a runtime"
    );
    assert_eq!(opened.attached, tables(&["dim_customer", "dim_product"]));

    // And the routing itself: a model on the SECOND source gets the second engine, and its table is
    // attached from that source's own directory. A loop that attached every model to every engine
    // would answer a question about one source out of another's files.
    let routed = open_engine(
        &bundle_over(&[
            ("customers", ENGINE_SOURCE, "dim_customer"),
            ("products", "second", "dim_product"),
        ]),
        &two,
        one_worker(),
    )
    .expect("a catalog spanning two DECLARED sources is servable");
    assert_eq!(
        routed
            .engines
            .postures()
            .map(|(name, posture)| (String::from(name.as_str()), posture.as_str()))
            .collect::<Vec<(String, &str)>>(),
        vec![
            (String::from(ENGINE_SOURCE), "shared-service-user"),
            (String::from("second"), "shared-service-user")
        ],
        "two sources the catalog reads are two engines, each with its own declaration"
    );
}

#[test]
fn a_source_of_a_kind_this_build_cannot_open_cannot_even_be_configured() {
    // **This replaces `a_catalog_naming_another_data_system_starts_nothing`, and the replacement is
    // narrower AND stronger.** That test asserted a comparison against a hard-coded source NAME:
    // any source not called `local` got no engine. It caught the right failure for the wrong
    // reason, and it refused a legitimate deployment - an operator who holds a warehouse extract
    // as a directory of files and calls that source `warehouse` was told this build had no adapter
    // for it, on the strength of the alias.
    //
    // What decides it now is the declared KIND, and the vocabulary of kinds is the vocabulary of
    // adapters - so a source this build cannot open is a word the settings tree refuses, before a
    // registry exists to hand to `open_engine`. This test is in the composition root's own module
    // rather than only in `sutura-config` because this is the binary that would otherwise serve it,
    // and because it is where a reader goes looking for the check that used to be here.
    // **A fourth dialect split this into TWO refusals, and this test now asserts both** - the word
    // it used to use, `bigquery`, is a kind this repository has an adapter for, so keeping it here
    // would have been a fixture asserting the wrong mechanism. The two are not interchangeable, and
    // an operator sent to the wrong one goes looking for a typo in a word that is spelled right:
    //
    // 1. a word no build of this repository has an adapter for is refused by the SETTINGS TREE;
    // 2. a kind this repository has and this BINARY did not link parses fine and is refused by the
    //    composition root, which is the only place that can know what was linked.
    let overlay = format!(
        "security:\n  identity: \"single-user\"\n  single_user_because: \"a test\"\nsources:\n  \
             production_warehouse:\n    kind: \"snowflake\"\n    data_dir: \"{}\"\n    posture: \
             \"shared-service-user\"\n",
        data().display()
    );
    let error =
        sutura_config::Settings::load(&sutura_config::Sources::defaults(crate::Environment::Development).with_overlay(overlay))
            .expect_err("no build of this repository has a Snowflake adapter, so the kind is not a kind");
    let rendered = crate::flatten(error);
    assert!(
        rendered.contains("sources.production_warehouse.kind"),
        "the refusal must name the key: {rendered}"
    );
    assert!(
        rendered.contains("files"),
        "the refusal must list what this build can open: {rendered}"
    );

    // The second refusal: a declared kind this binary links no adapter for. It PARSES - the settings
    // tree accepts it, because the repository does have that adapter - and the composition root
    // refuses it, naming the source so an operator knows which entry to change.
    let declared = "  production_warehouse:\n    kind: \"bigquery\"\n    billing_project: \"acme-analytics\"\n    \
                    dataset: \"warehouse\"\n    posture: \"shared-service-user\"\n";
    let error = refusal(
        open_engine(
            &bundle_over(&[("customers", "production_warehouse", "dim_customer")]),
            &registry(declared),
            one_worker(),
        ),
        "a kind this binary links no adapter for must not start",
    );
    assert!(
        error.contains("production_warehouse"),
        "the refusal must name the source: {error}"
    );
    assert!(
        error.contains("bigquery"),
        "the refusal must name the kind it cannot open: {error}"
    );

    // And the deployment the old rule refused: a files source under an alias that is not the
    // built-in engine's fixture name. It opens, which is the whole point of the change.
    let opened = open_engine(
        &bundle_over(&[("customers", "warehouse", "dim_customer")]),
        &registry(&entry("warehouse", "shared-service-user", "")),
        one_worker(),
    )
    .expect("a declared files source opens under whatever alias it was given");
    assert_eq!(opened.attached, tables(&["dim_customer"]));
}

#[test]
fn a_source_configured_to_impersonate_on_an_adapter_that_cannot_refuses_at_boot() {
    // **The cross-check, and it is HERE rather than in `sutura-config` because half of it is a
    // property of the build.** Configuration says which posture the deployment is asking for; only
    // the composition root can see which adapter was linked, and the in-process engine over local
    // files has nowhere for a subject's credential to arrive. There is no fallback - the alternative
    // to refusing is a deployment that believes it impersonates and reads every file as this
    // process.
    //
    // The engine source is declared `impersonation-at-source`, which parses fine and is a
    // configuration a different build could honour. That is what makes this a BOOT refusal rather
    // than a parse one.
    let error = refusal(
        open_engine(
            &bundle_over(&[("customers", ENGINE_SOURCE, "dim_customer")]),
            &registry(&entry(ENGINE_SOURCE, "impersonation-at-source", "")),
            one_worker(),
        ),
        "an impersonating posture on an adapter that cannot impersonate must not start",
    );
    assert!(error.contains(ENGINE_SOURCE), "the refusal must name the source: {error}");
    assert!(
        error.contains("cannot carry a per-subject credential") || error.contains("per-subject credential"),
        "the refusal must say what the adapter cannot do: {error}"
    );
    assert!(
        error.contains("no fallback"),
        "the refusal must say there is no fallback: {error}"
    );
    // NOT the neighbouring arms: the source IS declared and the build DOES have an adapter for it.
    assert!(!error.contains("no `sources."), "this is the capability arm: {error}");
    assert!(
        !error.contains("verification_identity"),
        "this is the capability arm: {error}"
    );

    // And the shared posture on the same adapter starts, so this is not a check that refuses every
    // declaration.
    drop(
        open_engine(
            &bundle_over(&[("customers", ENGINE_SOURCE, "dim_customer")]),
            &engine_declared(),
            one_worker(),
        )
        .expect("a shared source on a file engine is the ordinary case"),
    );
}

#[test]
fn an_anchor_on_a_source_with_no_declared_verification_identity_does_not_boot() {
    // Not skipped, not warned about, and not treated as a passing anchor - the three ways this
    // would otherwise become a mode nobody chose. The refusal names the metric AND the source,
    // because the fix is in one of two different files: either the catalog stops certifying that
    // number, or the source's entry declares the identity that would re-run it.
    //
    // **It fires before the engines are built, which is what makes it reachable at all here.** The
    // only adapter this build links cannot impersonate, so an `impersonation-at-source` entry is
    // also a capability refusal - and if the cross-check ran first this arm would be unreachable
    // through `open_engine` on this build and could only be tested as a bare function. Ordering the
    // cheap check first is what keeps the more actionable message in front of the operator.
    let error = refusal(
        open_engine(
            &bundle_with_an_anchor(ENGINE_SOURCE),
            &registry(&entry(ENGINE_SOURCE, "impersonation-at-source", "")),
            one_worker(),
        ),
        "an anchor with no identity to re-run it as must not boot",
    );
    assert!(
        error.contains("recurring_revenue"),
        "the refusal must name the metric: {error}"
    );
    assert!(error.contains(ENGINE_SOURCE), "the refusal must name the source: {error}");
    assert!(
        error.contains("verification_identity"),
        "the refusal must name the key to write: {error}"
    );

    // Two arms that must NOT refuse, so this is not a check that fires on every anchored bundle.
    //
    // A declared identity satisfies it - and then the capability cross-check is what stops this
    // build, which is the neighbouring refusal and a different sentence.
    let declared = refusal(
        open_engine(
            &bundle_with_an_anchor(ENGINE_SOURCE),
            &registry(&entry(
                ENGINE_SOURCE,
                "impersonation-at-source",
                "    verification_identity: \"sutura_anchor_reader\"\n",
            )),
            one_worker(),
        ),
        "the capability cross-check still stops this build",
    );
    assert!(
        !declared.contains("verification_identity"),
        "a declared identity is not an anchor refusal: {declared}"
    );
    assert!(declared.contains("no fallback"), "it is the capability arm now: {declared}");

    // And a SHARED source needs nothing declared at all: there the verification identity IS the
    // shared identity, and an anchor is a complete claim - every caller reads that source as that
    // one identity, so the number the anchor certifies is the number every caller gets.
    drop(
        open_engine(&bundle_with_an_anchor(ENGINE_SOURCE), &engine_declared(), one_worker())
            .expect("a shared source's anchors run as the shared identity"),
    );
}

#[test]
fn a_catalog_declaring_no_models_starts_nothing() {
    // The empty bundle, and the claim the tests below this one make in a comment - that an empty
    // catalog is "already refused earlier, by `open_engine`" - which nothing asserted in either
    // binary. Without the arm this opens cleanly: the attach loop has nothing to iterate, so the
    // service starts and refuses every question as an unknown metric, which reads as a question
    // problem rather than as a catalog directory holding no models.
    let error = refusal(
        open_engine(&bundle_over(&[]), &engine_declared(), one_worker()),
        "a catalog with no models opens nothing",
    );
    assert!(error.contains("declares no models"), "{error}");
    assert!(
        !error.contains("no `sources.") && !error.contains("no fallback"),
        "this is the empty arm, not the undeclared or capability one: {error}"
    );
}

#[test]
fn a_model_with_no_file_behind_it_starts_nothing() {
    // `attach` runs per model AFTER the source name is accepted, so this arm is reachable only by
    // a catalog this build can otherwise open - which is why it names the engine source. It is
    // also what makes `Opened::attached` evidence rather than bookkeeping: the set is collected
    // from the successful call, so a startup that got here does not proceed with a table missing.
    let error = refusal(
        open_engine(
            &bundle_over(&[("orders", ENGINE_SOURCE, "fct_order")]),
            &engine_declared(),
            one_worker(),
        ),
        "a model with no file behind it must not open",
    );
    assert!(error.contains("fct_order.csv"), "the CSV path is missing: {error}");
    assert!(error.contains("fct_order.parquet"), "the Parquet path is missing: {error}");
    assert!(error.contains("table fct_order"), "the table is not named: {error}");
}

#[test]
fn a_model_the_engine_has_no_table_for_stops_the_process() {
    // The startup sequence loads the catalog TWICE - the engine is opened for the first bundle
    // and the service validates and serves the second - so a model added to the catalog
    // directory between the two calls was served with nothing attached behind it. `answer`
    // cannot catch that: its only check on the engine is that the source NAME matches, so the
    // first question about the new metric came back as an error from the engine rather than as a
    // refusal at startup.
    let err = refuse_unattached(
        &tables(&["fact_subscription", "dim_customer"]),
        &tables(&["fact_subscription"]),
    )
    .expect_err("a served model with no attached table does not serve");
    assert!(err.contains("Served with no table attached: [dim_customer]"), "{err}");
    assert!(err.contains("Attached and no longer served: []"), "{err}");
    assert!(err.contains("the catalog changed while this process was starting"), "{err}");
}

#[test]
fn a_table_attached_for_a_model_no_longer_served_stops_it_too() {
    // The other direction, and not pedantry: it means the catalog directory changed between two
    // loads seconds apart. This one would answer every question correctly, which is exactly why
    // it has to be loud - whatever else moved in that edit is the part nobody has looked at.
    let err = refuse_unattached(
        &tables(&["fact_subscription"]),
        &tables(&["fact_subscription", "dim_customer"]),
    )
    .expect_err("an attached table for nothing served does not serve");
    assert!(err.contains("Served with no table attached: []"), "{err}");
    assert!(err.contains("Attached and no longer served: [dim_customer]"), "{err}");
}

#[test]
fn the_two_bundles_agreeing_is_the_ordinary_case_and_starts() {
    // The check has to be silent when nothing changed, which is every start. An empty catalog is
    // already refused earlier, by `open_engine`, so the empty pair is not a case this decides.
    refuse_unattached(&tables(&["fact_subscription"]), &tables(&["fact_subscription"])).expect("two bundles that agree start");
    assert!(
        refuse_unattached(
            &tables(&["dim_customer", "fact_subscription"]),
            &tables(&["fact_subscription", "dim_customer"])
        )
        .is_ok(),
        "the comparison is over sets, so declaration order is not a difference"
    );
}
