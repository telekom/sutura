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
use sutura_domain::model::TableName;

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

/// The request timeout the embedded defaults ship, also what a `BigQuery` job's deadline is filled from.
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
        #[cfg(feature = "postgres")]
        Ok(OpenedSources::Postgres(_)) => panic!("expected the file engine, got the Postgres arm"),
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
    sutura_config::Settings::load(
        &sutura_config::Sources::defaults(sutura_config::Environment::Development).with_overlay(overlay),
    )
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

mod support;

use support::{bundle_with_an_anchor, engine_declared};

// Re-exported so `crate::serve::tests::bundle_over` still resolves - `crate::serve::boot`'s own
// tests use that path, and moving the definition must not move the address a caller outside this
// file depends on.
pub(crate) use support::bundle_over;

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
            None,
        ),
        "a catalog reading an undeclared source must not get an engine",
    );
    assert!(
        error.contains("no `sources.production_warehouse` entry"),
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
        None,
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
        None,
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
    let error = sutura_config::Settings::load(
        &sutura_config::Sources::defaults(sutura_config::Environment::Development).with_overlay(overlay),
    )
    .expect_err("no build of this repository has a Snowflake adapter, so the kind is not a kind");
    let rendered = super::flatten(error);
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
        None,
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
        None,
    )
}

/// The `postgres` source's boot-refusal cell - `postgres_entry`, `opened_postgres`, and
/// `a_postgres_source_configured_to_impersonate_refuses_at_boot` - moved to its own file by the
/// same file-length gate `serve/boot.rs`'s own header names: this file crossed 1000 lines, and
/// `cargo xtask max-lines` fails rather than warning. `#[cfg(feature = "postgres")]` on the
/// declaration, not just on its contents, so an unused import in that file is not what a build
/// without the feature discovers.
#[cfg(feature = "postgres")]
mod postgres;

#[cfg(not(feature = "agent"))]
mod agent;

// The `bigquery`-kind cells: the settings-tree refusal, the no-adapter refusal, and (on a build that
// linked it) the credential/ceiling/anchor/boot-line cells. Moved out as a pure relocation to keep
// this file under the 1000-line cap.
#[cfg(test)]
mod bigquery;

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
            None,
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
            None,
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
            None,
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
            None,
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
            None,
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
            None,
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
        open_engine(&bundle_over(&[]), &engine_declared(), one_worker(), default_timeout(), None),
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
            None,
        ),
        "a model with no file behind it must not open",
    );
    assert!(error.contains("fct_order.csv"), "the CSV path is missing: {error}");
    assert!(error.contains("fct_order.parquet"), "the Parquet path is missing: {error}");
    assert!(error.contains("table fct_order"), "the table is not named: {error}");
}

#[test]
#[cfg(not(feature = "datahub"))]
fn a_declared_catalog_kind_this_build_cannot_open_is_a_boot_refusal_naming_it() {
    // `SourceKind::BigQuery`'s property on the metadata side, and the reason it is a property of
    // the BUILD rather than of the file: `sutura-config` can and must not see which catalog
    // adapter a binary linked, so the refusal lives in the composition root that would have to
    // open the kind. **`cfg(not(feature = "datahub"))`, and that guard is the point of this test
    // now** - since issue #202's reader, `datahub` IS a kind a `--features datahub` build of this
    // binary links; this cell is what a build WITHOUT that feature still gets.
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
    let err = super::catalog::open_catalog(&catalogs).expect_err("this build does not link the datahub feature");
    assert!(err.contains("catalog.kind: datahub"), "{err}");
    assert!(
        err.contains("--features datahub"),
        "unlike the pre-#202 refusal, THIS one has a real feature to name: {err}"
    );
    let markdown = CatalogSettings::parse(
        name,
        CatalogKind::Markdown,
        PathBuf::from("/nowhere/catalog"),
        PathBuf::from("/nowhere/data"),
        version,
    )
    .expect("a directory and a version are a settings");
    let catalogs = Catalogs::parse(vec![markdown]).expect("one declared catalog is a registry");
    super::catalog::open_catalog(&catalogs).expect("markdown is the kind every build links");
}

// `the_datahub_refusal_offers_no_rebuild_this_binary_has_no_feature_for`
// (`github.com/telekom/sutura#366`) is RETIRED rather than kept: it asserted the refusal must not
// name a feature, because `sutura-serve` declared none that provided `datahub`. Since issue #202's
// reader, `sutura-serve` DOES declare a `datahub` feature, so naming it is now the actionable
// remedy - the same shape `open_bigquery`'s own not-linked refusal already holds for `bigquery`.
// `a_declared_catalog_kind_this_build_cannot_open_is_a_boot_refusal_naming_it` above (now
// `cfg(not(feature = "datahub"))`) asserts the new, opposite rule for a build without the feature.

#[test]
fn catalogs_of_more_than_one_kind_in_one_deployment_are_refused() {
    // `sutura_app::Surface::start_composed` is generic in ONE catalog type per call, and
    // `SemanticCatalog::KIND`/`capabilities()` are per-TYPE associated items with no instance to
    // dispatch on - `crate::catalog`'s module header explains why that rules out a single enum
    // faithfully wrapping both a markdown and a datahub catalog. So a deployment naming both kinds
    // is refused here, by name, rather than silently opening only one of them.
    use sutura_config::{CatalogKind, CatalogSettings, Catalogs};
    use sutura_domain::model::SourceName;
    use sutura_domain::pinned::DefinitionVersion;
    let version = DefinitionVersion::parse("test-1").expect("a test version is a version");
    let markdown = CatalogSettings::parse(
        SourceName::parse("prose").expect("a test name is a name"),
        CatalogKind::Markdown,
        PathBuf::from("/nowhere/catalog"),
        PathBuf::from("/nowhere/data"),
        version.clone(),
    )
    .expect("a directory and a version are a settings");
    let datahub = CatalogSettings::parse(
        SourceName::parse("metrics").expect("a test name is a name"),
        CatalogKind::Datahub,
        PathBuf::from("/nowhere/catalog"),
        PathBuf::from("/nowhere/data"),
        version,
    )
    .expect("a directory and a version are a settings");
    let catalogs = Catalogs::parse(vec![markdown, datahub]).expect("two distinctly-named catalogs are a registry");
    let err = super::catalog::open_catalog(&catalogs).expect_err("a mix of catalog kinds is refused");
    assert!(err.contains("markdown"), "{err}");
    assert!(err.contains("datahub"), "{err}");
}

#[cfg(feature = "datahub")]
mod datahub_served;

// `OpenedCatalogs::Datahub` is `#[cfg(feature = "datahub")]`, so at the default (no-datahub)
// build the `Markdown` pattern below is the enum's ONLY arm and the `else` is IRREFUTABLE - a
// rustc error under `-D irrefutable-let-patterns` that the `--all-features` build never sees
// (there the `Datahub` variant makes it refutable, which is what the `else` is for). The narrow
// cfg-scoped allowance is the honest way to run the same cell in both builds.
#[cfg_attr(
    not(feature = "datahub"),
    expect(
        irrefutable_let_patterns,
        reason = "at the default build the Datahub arm is cfg'd out, so the Markdown pattern and its else are irrefutable; the all-features build has both arms and needs the else"
    )
)]
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
    let super::catalog::OpenedCatalogs::Markdown(opens) = opened else {
        panic!("a markdown-only deployment opens the markdown vector");
    };
    assert_eq!(opens.len(), 2, "one opened catalog per declared entry");
    assert_eq!(opens[0].name().as_str(), "structure");
    assert_eq!(opens[1].name().as_str(), "metrics");
}
