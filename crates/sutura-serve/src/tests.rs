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
use sutura_domain::capabilities::MetadataCapabilities;
use sutura_domain::catalog::{Definitions, Description, Model};
use sutura_domain::knowledge::Knowledge;
use sutura_domain::model::{ColumnName, ModelName, SourceName, TableName};
use sutura_domain::pinned::{Contribution, ContributionManifest, DefinitionVersion, PinnedDefinitions};

use super::boot::refuse_unattached;
use super::{ENGINE_SOURCE, Opened, OpenedSources, open_engine};

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
fn refusal(opened: Result<OpenedSources, String>, expected: &str) -> String {
    opened.map(drop).expect_err(expected)
}

/// The request timeout the embedded defaults ship, which is also what a `BigQuery` job's deadline is
/// filled from - so a test cannot pick a number a deployment would not run with.
fn default_timeout() -> sutura_config::RequestTimeout {
    sutura_config::RequestTimeout::parse(30).expect("thirty seconds is a request timeout")
}

/// The file arm, or a failed test naming which arm came back instead.
///
/// Every assertion below about attached tables is about the in-process engine, because it is the only
/// adapter that ATTACHES anything - so unwrapping the arm here is more honest than an accessor on the
/// enum that would have to invent an answer for the other one.
fn files(opened: Result<OpenedSources, String>) -> Opened {
    match opened.map_err(|error| format!("expected the file engine, got a refusal: {error}")) {
        Ok(OpenedSources::Files(files)) => files,
        #[cfg(feature = "bigquery")]
        Ok(OpenedSources::BigQuery(_)) => panic!("expected the file engine, got the BigQuery arm"),
        Err(message) => panic!("{message}"),
    }
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

/// The `workload_identity` block an `impersonation-at-source` source must now declare.
///
/// Issue 87 made the declaration required (an exchanging broker has to know which provider it hands
/// a subject's token to), and these fixtures thread it through so the test reaches the refusal it is
/// actually about rather than stopping at the settings tree.
fn wif() -> &'static str {
    "    workload_identity:\n      audience: \
     \"//iam.googleapis.com/projects/acme-analytics/locations/global/workloadIdentityPools/analysts/\
     providers/sso\"\n      scope: \"https://www.googleapis.com/auth/bigquery.readonly\"\n"
}

/// The ordinary declaration: the engine source, shared, over the example data.
fn engine_declared() -> sutura_config::SourceRegistry {
    registry(&entry(ENGINE_SOURCE, "shared-service-user", ""))
}

/// One model as a catalog document names it: the model, its data system, its table.
pub(crate) type DeclaredModel<'raw> = (&'raw str, &'raw str, &'raw str);

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
fn bundle_with_an_anchor(source: &str) -> PinnedDefinitions {
    use sutura_domain::calendar::{Date, TimeRange};
    use sutura_domain::catalog::{Anchor, AnchorValue, Metric};
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
        Some(Anchor::new(
            range,
            AnchorValue::parse("7").expect("a test anchor value is a value"),
        )),
        Description::default(),
    )
    .expect("no dimensions to duplicate");
    let definitions = Definitions::assemble(vec![model], vec![], vec![metric]).expect("the test bundle is consistent");
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

#[test]
fn a_catalog_naming_a_source_with_no_declaration_starts_nothing() {
    // **This replaces `a_catalog_spanning_two_data_systems_starts_nothing`, and the replacement is
    // the point of this branch rather than a regression.** That test asserted a constraint the
    // source registry removes: a catalog whose models sit on two DECLARED sources is now a
    // deployment that opens two engines, and a QUESTION whose plan spans both is now ANSWERED on
    // this binary: `sutura-exec-datafusion` declares `Warehouse::EXECUTES_LEGS`, so the splitter's
    // federated plan runs as two legs and the combiner assembles them - `served.rs`'s
    // `a_served_deployment_answers_a_question_spanning_two_sources` asks this deployment one and
    // gets rows. (Three or more sources are still refused at plan time, which is where the blanket
    // bound always belonged.)
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
            default_timeout(),
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
    let opened = files(open_engine(
        &bundle_over(&[
            ("customers", ENGINE_SOURCE, "dim_customer"),
            ("products", ENGINE_SOURCE, "dim_product"),
        ]),
        &two,
        one_worker(),
        default_timeout(),
    ));
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
    let routed = files(open_engine(
        &bundle_over(&[
            ("customers", ENGINE_SOURCE, "dim_customer"),
            ("products", "second", "dim_product"),
        ]),
        &two,
        one_worker(),
        default_timeout(),
    ));
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

    // **The second refusal moved from the KIND to the FEATURE, and the test that asserted it is two
    // tests below** - `a_bigquery_source_is_refused_by_a_build_that_did_not_link_the_adapter` and
    // `a_bigquery_source_reaches_the_credential_the_deployment_declared`. It is not weakened and it
    // is not gone: a `bigquery` source still parses and is still refused by the composition root on a
    // build that linked no adapter, and the refusal still names the source. What changed is that the
    // second thing it has to name is the `bigquery` feature rather than the kind, because on a build
    // that DID link the adapter there is no refusal to make - and one test cannot assert both.

    // And the deployment the old rule refused: a files source under an alias that is not the
    // built-in engine's fixture name. It opens, which is the whole point of the change.
    let opened = files(open_engine(
        &bundle_over(&[("customers", "warehouse", "dim_customer")]),
        &registry(&entry("warehouse", "shared-service-user", "")),
        one_worker(),
        default_timeout(),
    ));
    assert_eq!(opened.attached, tables(&["dim_customer"]));
}

/// One `sources:` entry for a `BigQuery` dataset, with every key that kind is opened with.
///
/// The credential file points at a path that is not there ON PURPOSE, and the tests below say what
/// each of them is proving with it: a refusal naming that key is proof the composition reached the
/// credential layer, which is the furthest a test with no project can get.
fn bigquery_entry(alias: &str, posture: &str, extra: &str) -> String {
    format!(
        "  {alias}:\n    kind: \"bigquery\"\n    billing_project: \"acme-analytics\"\n    dataset: \
         \"warehouse\"\n    credential_file: \"/nonexistent/sutura-test-bigquery.json\"\n    \
         max_bytes_billed: 1073741824\n    posture: \"{posture}\"\n{extra}"
    )
}

/// The startup a `bigquery` source produces, whichever way this binary was built.
fn opened_bigquery(entries: &str) -> Result<OpenedSources, String> {
    open_engine(
        &bundle_over(&[("customers", "warehouse", "dim_customer")]),
        &registry(entries),
        one_worker(),
        default_timeout(),
    )
}

#[test]
fn a_bigquery_source_missing_a_key_that_kind_is_opened_with_does_not_load() {
    // **A settings-tree refusal rather than a boot one, and it belongs here for the reason the
    // unknown-kind half above belongs here:** this is the binary that would otherwise serve it, and
    // it is where a reader looks for the check. `sutura-config` owns the mechanism and tests each key
    // separately; what this asserts is that the refusal survives `Settings::load` with its key
    // attached, which is the only part a composition root depends on.
    //
    // `credential_file` and not `billing_project`, because it is the key this step added and the one
    // whose absence used to be answerable from the environment - see its own note in `sutura-config`.
    let overlay = format!(
        "security:\n  identity: \"single-user\"\n  single_user_because: \"a test\"\nsources:\n{}",
        "  warehouse:\n    kind: \"bigquery\"\n    billing_project: \"acme-analytics\"\n    dataset: \
         \"warehouse\"\n    max_bytes_billed: 1073741824\n    posture: \"shared-service-user\"\n"
    );
    let error =
        sutura_config::Settings::load(&sutura_config::Sources::defaults(crate::Environment::Development).with_overlay(overlay))
            .expect_err("a bigquery source with no credential file is not a source this deployment can open");
    let rendered = crate::flatten(error);
    assert!(
        rendered.contains("credential_file"),
        "the refusal must name the key: {rendered}"
    );
    assert!(rendered.contains("warehouse"), "the refusal must name the entry: {rendered}");
}

#[test]
#[cfg(not(feature = "bigquery"))]
fn a_bigquery_source_is_refused_by_a_build_that_did_not_link_the_adapter() {
    // **The refusal that used to be about the KIND and is now about the FEATURE.** It parses - the
    // vocabulary of kinds is the repository's and the repository has this adapter - and the
    // composition root refuses it, because only this file can see what was linked.
    //
    // It names the feature as well as the source, because the two available actions are in two
    // different files: change the `kind:`, or build with `--features bigquery`. Under
    // `--all-features` this test is not compiled and its twin below is; that split is the honest
    // consequence of a behaviour that differs by build, and one test cannot assert both.
    //
    // **WHICH VENUE RUNS THIS ONE:** `just gates` and the `The shipped feature set runs its tests`
    // step in `ci.yml`, both through `cargo xtask check-default-feature-tests`. Not `just test` and
    // not the `nextest` nix check - they pass `--all-features`, so this cfg is false there. It was
    // compiled by a gate and executed by nothing at all until that task existed.
    let error = refusal(
        opened_bigquery(&bigquery_entry("warehouse", "shared-service-user", "")),
        "a build with no BigQuery adapter must not start against a bigquery source",
    );
    assert!(error.contains("warehouse"), "the refusal must name the source: {error}");
    assert!(
        error.contains("bigquery` feature") || error.contains("--features bigquery"),
        "the refusal must name the feature that would link it: {error}"
    );
}

#[test]
#[cfg(feature = "bigquery")]
fn a_bigquery_source_reaches_the_credential_the_deployment_declared() {
    // **What this proves, and it is deliberately the furthest a test with no project can reach:** the
    // kind DISPATCHED to the BigQuery adapter, the shared posture was accepted against that adapter's
    // own `IMPERSONATION`, both bounds parsed, and the composition asked for the credential file the
    // settings tree named. A refusal about that path is the proof; a refusal about the feature, the
    // kind or the posture would mean it stopped earlier.
    //
    // It cannot go further here by construction: `wire::BigQueryWire`'s host is a `const` and its
    // agent is `https_only`, so there is no loopback to point it at - `docs/adr/0018` states that as a
    // coverage hole paid for with a security property, and `just bigquery-acceptance` is the leg that
    // closes it against a real dataset.
    let error = refusal(
        opened_bigquery(&bigquery_entry("warehouse", "shared-service-user", "")),
        "the declared credential file is not there, so this deployment does not start",
    );
    assert!(
        error.contains("credential_file"),
        "the refusal must name the key that could not be read: {error}"
    );
    assert!(error.contains("warehouse"), "the refusal must name the source: {error}");
    // NOT the neighbouring arms, which is the half that stops this passing on the wrong branch: a
    // build that linked no adapter, or a posture cross-check that fired, would both be green on the
    // two assertions above if they only checked for a refusal.
    assert!(
        !error.contains("--features bigquery"),
        "this build DID link the adapter: {error}"
    );
    assert!(
        !error.contains("no fallback"),
        "the shared posture is deliverable by this adapter: {error}"
    );
    // **And the pre-flight has not run either, which is the ordering half.** An operator told about
    // a table when the credential is unreadable would go and edit the catalog, which was never
    // wrong. A type is what makes this hold rather than this assertion - though not the type this
    // comment first named: `wire::credential::Credential::read` is the only public constructor of a
    // `Credential`, and a `BigQueryWarehouse` cannot exist without one, so no arrangement of `run`
    // can ask a dataset about a table before its credential was read off disk.
    assert!(
        !error.contains("does not hold"),
        "no dataset is asked about a table before its credential is read: {error}"
    );
}

#[test]
#[cfg(feature = "bigquery")]
fn an_impersonating_source_is_opened_and_reads_the_credential_it_declared() {
    // **Issue 87's serve half, and the reversal is the point of the change.** The exchanging broker
    // is now attached in `run()`'s `bigquery` arm, so `open_engine` no longer refuses an
    // `impersonation-at-source` source by name - the adapter declares `PerSubjectCredential`, and a
    // subject's credential is exactly what the attached `WorkloadIdentityBroker` mints. The refusal
    // that used to stand in for "no broker attached" is gone from this path.
    //
    // **What this reaches instead is the furthest a test with no project can: the source is OPENED
    // and reads the credential file it declared** - and when that is missing, the boot names the
    // FILE and not the posture. It cannot check the broker wire here because `open_engine` predates
    // the broker; the attachment lives in `run()`, at the seam this suite cannot reach without a
    // real project.
    let error = refusal(
        opened_bigquery(&bigquery_entry(
            "warehouse",
            "impersonation-at-source",
            &format!("{}    verification_identity: \"sutura_anchor_reader\"\n", wif()),
        )),
        "an impersonating source with a declared workload identity is served, so reaching its credential is the test",
    );
    assert!(error.contains("warehouse"), "the refusal must name the source: {error}");
    assert!(
        error.contains("credential_file"),
        "the source was OPENED and its refusal is about the credential it declared: {error}"
    );
    // The composition gap is closed: these are the two sentences the old refusal said, and neither
    // is true any more - the exchange and the broker are wired, so an entry reaching this far is not
    // read as the deployment's own identity.
    assert!(
        !error.contains("does not attach a broker"),
        "the composition no longer refuses impersonation by name: {error}"
    );
    assert!(
        !error.contains("no fallback"),
        "the posture is no longer a fallback-shaped refusal: {error}"
    );
}

#[test]
#[cfg(feature = "bigquery")]
fn a_bigquery_ceiling_the_adapter_will_not_send_is_a_startup_refusal_naming_the_key() {
    // The other half of leaving `max_bytes_billed` a bare number in the settings tree: the RANGE
    // belongs to `sutura_exec_bigquery::wire::BytesBilledCeiling`, so there is one parse of it and it
    // happens here. What this asserts is that the refusal still names the key an operator has to
    // change - a range error from a newtype with no key attached would be a support request.
    //
    // Zero rather than a value above the cap, because zero is the one an operator reaches by writing
    // a placeholder: it would refuse every question rather than bounding one.
    let entry = bigquery_entry("warehouse", "shared-service-user", "").replace("1073741824", "0");
    let error = refusal(
        opened_bigquery(&entry),
        "a ceiling of zero would refuse every question rather than bounding one",
    );
    assert!(error.contains("max_bytes_billed"), "the refusal must name the key: {error}");
    assert!(error.contains("warehouse"), "the refusal must name the source: {error}");
    assert!(
        !error.contains("credential_file"),
        "the bound is parsed before the credential file is read: {error}"
    );
}

#[test]
#[cfg(feature = "bigquery")]
fn a_request_timeout_that_leaves_no_job_budget_does_not_start() {
    // **The subtle one, and it is a bug this test exists to have caught rather than a range check.**
    // A job's deadline is not `server.request_timeout_seconds`: an answer makes
    // `QueryDeadline::CALLS_PER_ANSWER` calls and each pays a connect margin, so filling the deadline
    // with the whole timeout would produce a job allowed to outlive the request that promised it -
    // green in every test here and an overrun under load. `within_request_timeout` owns that
    // arithmetic, next to the constant it depends on.
    //
    // Ten seconds is the smallest number that makes the point: half of it is five, the connect margin
    // is five, and what is left is nothing - so a deployment whose timeout cannot fit a query is told
    // so at startup rather than being handed a clamped value nobody chose.
    let error = refusal(
        open_engine(
            &bundle_over(&[("customers", "warehouse", "dim_customer")]),
            &registry(&bigquery_entry("warehouse", "shared-service-user", "")),
            one_worker(),
            sutura_config::RequestTimeout::parse(10).expect("ten seconds is a request timeout"),
        ),
        "a request timeout with no room for a job is not a servable deployment",
    );
    assert!(
        error.contains("server.request_timeout_seconds"),
        "the refusal must name the key an operator has to change: {error}"
    );
    assert!(
        !error.contains("credential_file"),
        "the budget is worked out before the credential file is read: {error}"
    );
}

#[test]
fn an_anchor_on_a_bigquery_source_is_held_to_the_same_verification_rule() {
    // **The anchor check reads a DECLARATION and not a kind, so registering a second adapter must not
    // have moved it - and this is what says so rather than leaving it to be assumed.** It runs before
    // anything is opened, so it fires on a `bigquery` source exactly as it fires on a `files` one: a
    // metric that certifies a number, on a source declared `impersonation-at-source` with nobody named
    // to re-run it as, is a bundle this deployment cannot verify.
    let error = refusal(
        open_engine(
            &bundle_with_an_anchor("warehouse"),
            &registry(&bigquery_entry("warehouse", "impersonation-at-source", wif())),
            one_worker(),
            default_timeout(),
        ),
        "an anchor on an impersonating source with no verification identity must not start",
    );
    assert!(
        error.contains("verification_identity"),
        "the refusal must name the key that would declare one: {error}"
    );
    assert!(error.contains("warehouse"), "the refusal must name the source: {error}");

    // And a SHARED `bigquery` source's anchor is a complete claim, so this is not a check that fires
    // on every anchored bundle: the verification identity IS the shared identity, one service account
    // reaching the dataset for everybody, so the number the anchor certifies is the number every
    // caller gets. It still does not START - the credential file is not there on this machine, and on
    // a build with no adapter the feature is missing - but whatever stops it is not this check.
    let later = refusal(
        open_engine(
            &bundle_with_an_anchor("warehouse"),
            &registry(&bigquery_entry("warehouse", "shared-service-user", "")),
            one_worker(),
            default_timeout(),
        ),
        "the credential is still not there, so the deployment still does not start",
    );
    assert!(
        !later.contains("verification_identity"),
        "a shared source's anchors run as the shared identity: {later}"
    );
}

#[test]
fn a_catalog_reading_two_kinds_of_source_does_not_start() {
    // **The limit `sutura_app::Warehouses` documents, made a startup refusal instead of a surprise.**
    // That registry is generic in one adapter type, so this process holds two file sources or two
    // datasets and cannot hold one of each; federating across two kinds needs a closed enum over the
    // adapter types or dynamic dispatch, which that module records as an architecture decision.
    //
    // The alternative is what makes this worth a refusal rather than a comment: whichever kind lost
    // would be a source nothing opened, and the first question against it would answer
    // `SourceUnavailable` - a refusal that reads as "nobody configured that" about a source the
    // operator configured.
    let both = format!(
        "{}{}",
        entry(ENGINE_SOURCE, "shared-service-user", ""),
        bigquery_entry("warehouse", "shared-service-user", "")
    );
    let error = refusal(
        open_engine(
            &bundle_over(&[
                ("customers", ENGINE_SOURCE, "dim_customer"),
                ("products", "warehouse", "dim_product"),
            ]),
            &registry(&both),
            one_worker(),
            default_timeout(),
        ),
        "one process opens one kind of data system at a time",
    );
    assert!(
        error.contains(ENGINE_SOURCE) && error.contains("warehouse"),
        "the refusal must name both entries: {error}"
    );
    assert!(
        error.contains("files") && error.contains("bigquery"),
        "the refusal must name both kinds: {error}"
    );
    assert!(
        error.contains("one kind of data system at a time"),
        "the refusal must say what the limit is: {error}"
    );
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
            &registry(&entry(ENGINE_SOURCE, "impersonation-at-source", wif())),
            one_worker(),
            default_timeout(),
        ),
        "an impersonating posture on an adapter that cannot impersonate must not start",
    );
    assert!(error.contains(ENGINE_SOURCE), "the refusal must name the source: {error}");
    // `per-subject credential` is a substring of the fuller spelling, so the OR was one fact
    // stated twice; the shorter arm alone is equivalent and the needle the rule trusts.
    assert!(
        error.contains("per-subject credential"),
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
            default_timeout(),
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
            &registry(&entry(ENGINE_SOURCE, "impersonation-at-source", wif())),
            one_worker(),
            default_timeout(),
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
                &format!("{}    verification_identity: \"sutura_anchor_reader\"\n", wif()),
            )),
            one_worker(),
            default_timeout(),
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
        open_engine(
            &bundle_with_an_anchor(ENGINE_SOURCE),
            &engine_declared(),
            one_worker(),
            default_timeout(),
        )
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
        open_engine(&bundle_over(&[]), &engine_declared(), one_worker(), default_timeout()),
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
            default_timeout(),
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

#[test]
fn a_declared_catalog_kind_this_build_cannot_open_is_a_boot_refusal_naming_it() {
    // `SourceKind::BigQuery`'s property on the metadata side, and the reason it is a property of
    // the BUILD rather than of the file: `sutura-config` can and must not see which catalog
    // adapter a binary linked, so the refusal lives in the composition root that would have to
    // open the kind. `datahub` is the vocabulary's one kind no binary here links.
    use sutura_config::{CatalogKind, CatalogSettings, Catalogs};
    use sutura_domain::model::SourceName;
    use sutura_domain::pinned::DefinitionVersion;
    let version = DefinitionVersion::parse("test-1").expect("a test version is a version");
    let name = SourceName::parse("model").expect("a test name is a name");
    let datahub = CatalogSettings::parse(
        name.clone(),
        CatalogKind::Datahub,
        PathBuf::from("/nowhere/catalog"),
        PathBuf::from("/nowhere/data"),
        version.clone(),
    )
    .expect("a directory and a version are a settings");
    let catalogs = Catalogs::parse(vec![datahub]).expect("one declared catalog is a registry");
    let err = super::catalog::open_catalog(&catalogs).expect_err("datahub cannot be opened by this build");
    assert!(err.contains("datahub"), "{err}");
    assert!(err.contains("markdown"), "{err}");
    let markdown = CatalogSettings::parse(
        name,
        CatalogKind::Markdown,
        PathBuf::from("/nowhere/catalog"),
        PathBuf::from("/nowhere/data"),
        version,
    )
    .expect("a directory and a version are a settings");
    let catalogs = Catalogs::parse(vec![markdown]).expect("one declared catalog is a registry");
    super::catalog::open_catalog(&catalogs).expect("markdown is the kind this build links");
}

#[test]
fn the_datahub_refusal_offers_no_rebuild_this_binary_has_no_feature_for() {
    // `github.com/telekom/sutura#366`. The refusal used to tell an operator to "build the binary
    // with the feature that provides it". `sutura-serve` declares `tls` and `bigquery` and nothing
    // else, and does not depend on the DataHub adapter in any form - so no `--features` value
    // satisfied that sentence and the remedy was unactionable.
    //
    // What is asserted is the ABSENCE of the instruction rather than a wording: a refusal here may
    // say what this binary cannot do, and must not send a reader after a feature that does not
    // exist. `cargo xtask check-feature-remedies` is the same rule over every crate's messages.
    use sutura_config::{CatalogKind, CatalogSettings, Catalogs};
    use sutura_domain::model::SourceName;
    use sutura_domain::pinned::DefinitionVersion;
    let datahub = CatalogSettings::parse(
        SourceName::parse("model").expect("a test name is a name"),
        CatalogKind::Datahub,
        PathBuf::from("/nowhere/catalog"),
        PathBuf::from("/nowhere/data"),
        DefinitionVersion::parse("test-1").expect("a test version is a version"),
    )
    .expect("a directory and a version are a settings");
    let catalogs = Catalogs::parse(vec![datahub]).expect("one declared catalog is a registry");
    let err = super::catalog::open_catalog(&catalogs).expect_err("datahub cannot be opened by this build");
    assert!(
        !err.contains("feature"),
        "the refusal may not send an operator after a feature this crate does not declare: {err}"
    );
    // The reachable kind is still offered, so the message stays actionable.
    assert!(err.contains("markdown"), "{err}");
}

#[test]
fn a_deployment_with_more_than_one_catalog_opens_one_per_declared_entry() {
    // Step 4 of the issue: the settings DECLARE several metadata sources and the composition root
    // opens one adapter per entry, so there is no longer anything here to refuse - the refusal a
    // second source could earn lives in the metadata assembler, where content that does not compose
    // is refused naming both sources. What this test pins is the composition root's half: each
    // declared markdown catalog opens, and `catalog::load` composes them.
    use sutura_config::{CatalogKind, CatalogSettings, Catalogs};
    use sutura_domain::model::SourceName;
    use sutura_domain::pinned::DefinitionVersion;
    let version = DefinitionVersion::parse("test-1").expect("a test version is a version");
    let entry = |raw: &str| {
        CatalogSettings::parse(
            SourceName::parse(raw).expect("a test name is a name"),
            CatalogKind::Markdown,
            PathBuf::from("/nowhere/catalog"),
            PathBuf::from("/nowhere/data"),
            version.clone(),
        )
        .expect("a directory and a version are a settings")
    };
    let catalogs = Catalogs::parse(vec![entry("structure"), entry("metrics")]).expect("two names are a registry");
    let opened = super::catalog::open_catalog(&catalogs).expect("two markdown catalogs open");
    assert_eq!(opened.len(), 2, "one opened catalog per declared entry");
    assert_eq!(opened[0].name().as_str(), "structure");
    assert_eq!(opened[1].name().as_str(), "metrics");
}
