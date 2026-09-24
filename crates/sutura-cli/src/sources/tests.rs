//! What `super`'s composition root does with a declaration, asserted against the example bundle.
//!
//! **Its own file rather than a `mod tests` inside the parent**, and the reason is mechanical: the
//! 1000-line cap under `crates/` is unexemptable, and the parent crossed it when the fourth
//! `SourceKind` arrived. `crate::serve` already carries its suite this way
//! (`crates/sutura-cli/src/serve/tests.rs`), so this is the shape the sibling root uses rather than
//! a new one - and `#[cfg(test)] mod tests;` at the declaration is what keeps the split a TEST
//! partition, which a bare `mod tests;` would not.

use std::path::{Path, PathBuf};

use sutura_domain::model::{MetricName, TableName};

use sutura_exec_datafusion::DataFusionWarehouse;

use super::{
    BUILT_IN_SOURCE, Opened, OpenedWith, bundle_naming, bundle_over, declaring, declaring_bigquery, open_engine, overlay_remedy,
    runtime, timeout, unservable,
};

/// The files registry `open_engine` produced, or a failure saying which arm it took instead.
///
/// An exhaustive match rather than an `if let`, so a third linked adapter is a compile error in
/// this suite too - the same property the two commands' own matches carry.
fn files_of(opened: Opened) -> OpenedWith<DataFusionWarehouse> {
    match opened {
        Opened::Files(opened) => Some(opened),
        #[cfg(feature = "bigquery")]
        Opened::BigQuery(_) => None,
        #[cfg(feature = "postgres")]
        Opened::Postgres(_) => None,
        #[cfg(feature = "clickhouse")]
        Opened::ClickHouse(_) => None,
        #[cfg(feature = "oracle")]
        Opened::Oracle(_) => None,
    }
    .expect("this fixture declares a files source")
}

fn example() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/single-player")
}

/// The example's own data directory, as an absolute path.
///
/// Absolute because `sources.<alias>.data_dir` is parsed absolute - a service's working directory
/// is whatever its supervisor chose - so a relative one is a settings refusal rather than a
/// declaration this suite could make.
fn data_dir() -> String {
    example()
        .join("data")
        .canonicalize()
        .expect("the example's data directory is there")
        .to_string_lossy()
        .into_owned()
}

/// A files entry over the example's data, for one alias.
fn declaring_files(alias: &str) -> sutura_config::SourceRegistry {
    declaring(
        alias,
        &format!("    kind: files\n    data_dir: \"{}\"", data_dir()),
        "shared-service-user",
    )
}

/// An empty registry: the ordinary command-line case, where nothing was configured.
fn nothing_declared() -> sutura_config::SourceRegistry {
    sutura_config::SourceRegistry::default()
}

#[test]
fn a_files_source_the_deployment_declared_is_opened_under_its_own_name() {
    // THE OUTCOME issue 121 asks for: a catalog whose models name `warehouse` is answered,
    // because the deployment said what `warehouse` is. Before this, this function compared the
    // declared source against one constant and refused everything else, so the only catalog the
    // published binary could open was one that happened to call its data system `local`.
    let opened = files_of(
        open_engine(
            &bundle_naming("warehouse"),
            &declaring_files("warehouse"),
            runtime(),
            timeout(),
            None,
            None,
        )
        .expect("a declared files source opens"),
    );
    assert_eq!(
        opened
            .engines
            .postures()
            .map(|(name, posture)| (name.as_str(), posture.as_str()))
            .collect::<Vec<(&str, &str)>>(),
        vec![("warehouse", "shared-service-user")],
        "the engine answers to the DECLARED name, under the identity that declaration carries"
    );
    assert!(
        opened.attached.is_some(),
        "a files source attaches, so there is a table set to compare a re-load against"
    );
    // The broker has to have come from the same decision, or the leg would carry an
    // acknowledgement no adapter was opened with. One entry, for the one declared source.
    assert_eq!(opened.broker.count(), 1, "the deployment's own tree is what mints for it");
}

#[test]
fn a_source_the_deployment_never_declared_gets_the_built_in_files_source_only_under_its_own_name() {
    // THE BUG THE OLD NAME COMPARISON EXISTED FOR, kept: with nothing declared there is no
    // statement anywhere that `production_warehouse` is a directory of files, so an engine
    // wearing that name over the caller's own CSVs would answer that catalog's certified metric
    // out of them, stamped with the real bundle's version and digest - because
    // `warehouses.get(plan.source())` is a LOOKUP by the name the catalog declared, and an engine
    // registered under that name makes it succeed. See this module's head, which states why the
    // guard an earlier version of this comment named does not exist.
    //
    // `dim_customer.csv` is in the example's data directory, so nothing else fails either.
    let error = open_engine(
        &bundle_naming("production_warehouse"),
        &nothing_declared(),
        runtime(),
        timeout(),
        Some(&example().join("data")),
        None,
    )
    .map(|_| ())
    .expect_err("an undeclared source must not get the built-in declaration's engine");
    assert!(
        error.contains("sources.production_warehouse"),
        "the refusal must name the entry to write: {error}"
    );
    assert!(
        error.contains(sutura_config::CONFIG_DIR_VARIABLE),
        "the refusal must say where that entry goes: {error}"
    );
}

#[test]
fn the_built_in_declaration_still_opens_the_documented_example() {
    // The other half: a gate is worth nothing if it also refuses the catalog the quickstart tells
    // a reader to run, with no configuration at all. This is the path
    // `crates/sutura-cli/tests/example.rs` and `docs/getting-started.md` both take.
    let pinned = crate::commands::load(&example().join("catalog")).expect("the example catalog loads");
    let opened = files_of(
        open_engine(
            &pinned,
            &nothing_declared(),
            runtime(),
            timeout(),
            Some(&example().join("data")),
            None,
        )
        .expect("the example catalog opens with nothing declared"),
    );
    assert_eq!(
        opened
            .engines
            .postures()
            .map(|(name, posture)| (name.as_str(), posture.as_str()))
            .collect::<Vec<(&str, &str)>>(),
        vec![(BUILT_IN_SOURCE, "shared-service-user")],
        "the built-in declaration keeps its own name and states the identity it reads under"
    );
    assert_eq!(
        opened
            .attached
            .as_ref()
            .map(|tables| tables.iter().map(TableName::as_str).collect::<Vec<&str>>()),
        Some(
            sutura_app::preflight::served_tables(&pinned)
                .iter()
                .map(TableName::as_str)
                .collect::<Vec<&str>>()
        ),
        "the attached set is what the served bundle names"
    );
}

#[test]
fn a_question_is_answered_through_a_declared_source_under_the_witness_that_entry_carries() {
    // **THE OUTCOME issue 121 asks for, end to end - and the test review proved was missing.**
    // Everything else here builds an `Opened` and stops. This one verifies the bundle against the
    // engine the REGISTRY arm opened and answers a certified question through it, which is the
    // only thing that exercises the seam `OpenedWith`'s doc comment claims: the broker mints for
    // the same decision the engines came from.
    //
    // Review demonstrated the gap with a mutation - minting the built-in acknowledgement while
    // the engine carries the registry entry's - and all 34 committed tests passed, because the
    // only broker assertion was `count() == 1` and both constructors give 1 on a one-source
    // registry. `Presented::agrees_with` is what actually catches that, at answer time, so an
    // answer is what has to be asked for.
    //
    // The EXAMPLE catalog rather than a hand-built bundle, deliberately: its models say
    // `source: local`, so declaring `sources.local` sends it through the registry arm with real
    // CSVs behind it. A fixture bundle cannot answer - `bundle_naming` names a `signed_at` column
    // `dim_customer.csv` does not have, which review also measured.
    let pinned = crate::commands::load(&example().join("catalog")).expect("the example catalog loads");
    let opened = files_of(
        open_engine(&pinned, &declaring_files(BUILT_IN_SOURCE), runtime(), timeout(), None, None)
            .expect("the example catalog opens through a declared files source"),
    );
    let validated = sutura_app::verify_and_validate(pinned, &opened.engines).expect("every anchor reproduces");
    // The example's own certified window, which is the range `examples/single-player` documents
    // and the one its anchor covers.
    let question = sutura_domain::query::Query::new(
        MetricName::parse("recurring_revenue").expect("a test metric is a metric"),
        sutura_domain::model::Grain::Month,
        sutura_domain::calendar::TimeRange::new(
            sutura_domain::calendar::Date::parse("2026-01-01").expect("a test date is a date"),
            sutura_domain::calendar::Date::parse("2026-07-01").expect("a test date is a date"),
        )
        .expect("a test range is not empty"),
        Vec::new(),
        Vec::new(),
    );
    let outcome = sutura_app::answer(
        &validated,
        &question,
        &sutura_domain::identity::RequestContext::of(sutura_domain::identity::PrincipalChain::of(
            sutura_domain::identity::Subject::TheDeploymentItself,
        )),
        &opened.broker,
        &opened.engines,
        &sutura_domain::plan::RefusingCombiner,
        1 << 30,
        sutura_domain::warehouse::deadline::Deadline::opened_at(std::time::Instant::now(), timeout().budget()),
        &sutura_app::SpendLedger::no_budget(),
        sutura_domain::plan::RowCeiling::DEFAULT,
    )
    .expect("a declared source answers rather than failing")
    .into_outcome();
    // An ANSWER and not a refusal: a broker minting a witness the adapter was not opened with is
    // a `SurfaceFailure`, and a posture disagreement is what `Presented::agrees_with` returns.
    assert!(
        matches!(outcome, sutura_domain::query::ToolOutcome::Answer { .. }),
        "a certified question through a declared source must be answered: {outcome:?}"
    );
}

#[test]
fn a_data_directory_offered_to_a_dataset_is_refused_as_an_argument_that_selects_nothing() {
    // NOT "two answers to one question", which is what the single pre-match check called it: a
    // dataset has no directory, so the argument is not a competing answer - it is an argument
    // that means nothing for that kind. Review reproduced the old message's remedy leading to a
    // second refusal about the feature, which is a different problem than the one it named.
    let error = open_engine(
        &bundle_naming("warehouse"),
        &declaring_bigquery("shared-service-user", ""),
        runtime(),
        timeout(),
        Some(&example().join("data")),
        None,
    )
    .map(|_| ())
    .expect_err("a data directory for a dataset source is refused");
    assert!(error.contains("a dataset has none"), "{error}");
    assert!(
        !error.contains("two answers to one question"),
        "a dataset's directory is not a competing answer: {error}"
    );
}

#[test]
fn the_built_in_declaration_with_no_directory_says_what_is_missing() {
    // The arm the argument being OPTIONAL created: `[data-dir]` may be absent because a fully
    // declared deployment needs none, so the case where it is absent AND nothing is declared has
    // to say which of the two to supply rather than failing on a path built from nothing.
    let error = open_engine(
        &bundle_naming(BUILT_IN_SOURCE),
        &nothing_declared(),
        runtime(),
        timeout(),
        None,
        None,
    )
    .map(|_| ())
    .expect_err("no declaration and no directory is nothing to read");
    assert!(error.contains("no data directory was given"), "{error}");
}

#[test]
fn a_catalog_spanning_two_data_systems_gets_no_engine() {
    // The arm nothing else proves. This command answers one question against one data system, so
    // a bundle whose models name two gets no engine at all rather than one over whichever half
    // happens to be local.
    //
    // The built-in source is deliberately ONE OF THE PAIR, and its table has a real file in the
    // example's data directory. That is what makes this discriminate: an arm that took the first
    // source instead of refusing would find it, find `dim_customer.csv`, and hand back a working
    // engine serving half a catalog under the whole bundle's digest.
    let error = open_engine(
        &bundle_over(&[
            ("customers", BUILT_IN_SOURCE, "dim_customer"),
            ("products", "production_warehouse", "dim_product"),
        ]),
        &nothing_declared(),
        runtime(),
        timeout(),
        Some(&example().join("data")),
        None,
    )
    .map(|_| ())
    .expect_err("a catalog spanning two data systems must not get an engine");
    assert!(
        error.contains("spans 2 data systems"),
        "the refusal must say how many it found: {error}"
    );
    // **The remedy it must offer, and this assertion is the inverse of the one it replaces.**
    // It used to assert the ABSENCE of "over http", because at the time every adapter a release
    // linked declared `EXECUTES_LEGS = false` and the served surface refused the same question
    // as `FederationNotExecutable` - so naming that surface sent an operator on a round trip.
    // `sutura-exec-datafusion` declares the constant now, and
    // `crates/sutura-cli/tests/served.rs` asks the composed binary a two-source question and
    // gets rows, so the surface is a real remedy and withholding it is the defect.
    assert!(
        error.to_lowercase().contains("over http"),
        "the refusal must name the surface that does answer a two-source question: {error}"
    );
    // And the claim that inverted with it, asserted as an absence because it is now FALSE: a
    // release does link an adapter that executes a leg.
    assert!(
        !error.contains("executes no leg"),
        "the refusal still claims no shipped adapter executes a leg: {error}"
    );
    // NOT the neighbouring arm, and this is the half that stops the test passing on the wrong
    // branch: `production_warehouse` is also undeclared, so a test that only checked for *a*
    // refusal would be green with the multi-source arm gone.
    assert!(
        !error.contains("sources.production_warehouse"),
        "this is the multi-source arm, not the undeclared-source one: {error}"
    );
}

#[test]
fn a_catalog_declaring_no_models_gets_no_engine() {
    // The empty bundle. An engine opened over nothing would open successfully, because the attach
    // loop has nothing to iterate, and then answer every question as an unknown metric - which
    // reads as a question problem rather than as a catalog directory that holds no models.
    let error = open_engine(
        &bundle_over(&[]),
        &nothing_declared(),
        runtime(),
        timeout(),
        Some(&example().join("data")),
        None,
    )
    .map(|_| ())
    .expect_err("a catalog with no models opens nothing");
    assert!(error.contains("declares no models"), "{error}");
}

#[test]
fn the_remedy_names_the_overlay_variables_this_process_has() {
    // **`github.com/telekom/sutura#386`.** The remedy named `SUTURA_CONFIG_DIR` and
    // `SUTURA_ENVIRONMENT` and nothing else, so a reader whose `SUTURA__SERVER__HOST` caused the
    // refusal was pointed at two variables that were neither set nor able to fix it - the same
    // class as #366's refusal citing a cargo feature that does not exist. The names are the
    // process's own, so the message stops guessing.
    let set = overlay_remedy(&[String::from("SUTURA__SERVER__HOST")]);
    assert!(set.contains("SUTURA__SERVER__HOST"), "{set}");
    assert!(set.contains(" 1 set"), "the count and the list agree: {set}");

    // The empty case says so rather than staying silent: what rules the overlay out for a
    // reader is being told it is empty.
    let none = overlay_remedy(&[]);
    assert!(none.contains("none set"), "{none}");
    assert!(!none.contains("SUTURA__SERVER__HOST"), "{none}");

    // And the sentence reaches the message an operator sees. Not a second rendering of the same
    // words: `unservable` is what `configured` maps its error through, and a helper tested alone
    // would say nothing about whether anything calls it. `SettingsLoadError::new` is private to
    // `sutura_config`, so a refused load is the only way the command is handed one - the same
    // route the loader's own tests take.
    let refused = sutura_config::Settings::load(
        &sutura_config::Sources::defaults(sutura_config::Environment::Development)
            .with_overlay("security:\n  inbound:\n    resource: \"https://sutura.example.com\"\n"),
    )
    .expect_err("an inbound block with no mode is refused");
    let refusal = unservable(&refused);
    assert!(refusal.contains(sutura_config::CONFIG_DIR_VARIABLE), "{refusal}");
    assert!(
        refusal.contains("-prefixed environment variable"),
        "the overlay is the third thing the remedy names: {refusal}"
    );
}
