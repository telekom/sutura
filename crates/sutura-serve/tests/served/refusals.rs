//! The four startup refusals the served binary makes, each on the REAL composition root: the
//! settings type refuses the declaration, and this module asserts the PROCESS refuses it too -
//! exit 1, nothing bound, and the operator's own sentence.
//!
//! Split out of `served.rs`'s `mod tests` by the same `max-lines` 1000-line cap that moved
//! `harness`, `keycloak_test`, `datahub`, `agent` and `e2e` into their own files; this module is
//! `#[path = "served/refusals.rs"]` from `served.rs`, so it shares the one `harness` module and
//! every fixture the served suite uses.

use sutura_config::{Environment, NotFitToServe, Settings, SettingsError, SettingsLoadError, Sources, TokenRequiredBy};

use crate::harness::{LOOPBACK, SINGLE_USER, TOKEN, deployment, example_root, refused_to_start, written};
// ------------------------------------------------------- the four startup refusals ---
//
// `github.com/telekom/sutura#302`. Each of the four is already asserted over `Settings::load` in
// `crates/sutura-config/src/settings/tests.rs`, and that is a claim about the settings TYPE. What
// an operator gets is a PROCESS, and *`load` returned `Err`* and *the binary exited without
// binding a port* are two different statements - a service that starts and answers `401` to
// everything is a different outcome from one that refuses to exist. Only the first had a test.
//
// The three assertions each case makes, and why each is separate:
//
// 1. **Exit `1`**, in `harness::refused_to_start`, and exactly 1 rather than non-zero - see its
//    own note on why an abort is not a refusal.
// 2. **Nothing bound**, in `stopped_before_binding`.
// 3. **The operator's own sentence**, DERIVED from `sutura_config` rather than quoted here - see
//    `stopped_before_binding`. Nothing in this repository compared anything to the `not fit to serve`
//    wording before, so the header an operator greps for was reachable by no test at all.

/// Starts the composed binary on a deployment it must refuse, and takes what it said.
///
/// **The "never binds" half, and it is the security-relevant one.** `sutura_http::server::serve`
/// reads the port back off the socket and logs it, which is the same event the whole harness
/// waits for - so its ABSENCE is this suite's own evidence that no listener was opened, on the
/// same channel every other case trusts.
///
/// **The limit, stated rather than implied:** this reads the process's own log. A root that bound
/// a socket and did not report it would be invisible here, and what makes that narrow rather than
/// hollow is that the event is emitted by the bind itself and nothing else in this file could
/// pass without it.
fn stopped_before_binding(environment: Environment, case: &str, settings: &str) -> (SettingsLoadError, Vec<String>) {
    let directory = written(case, settings);
    let layer = directory.join("base.yaml").display().to_string();
    // Read once from the same directory the child consumes. Keep the Result until the child
    // guard has cleaned up, so an unexpected successful load cannot leave the directory behind.
    let expected = Settings::load(&Sources::defaults(environment).with_directory(directory.clone()));
    let told = refused_to_start(environment, directory);
    let refused = expected.expect_err("this case's fixture is a deployment the settings type refuses");
    assert!(
        !told.iter().any(|line| line.contains(r#""msg":"listening""#)),
        "a deployment that must not start opened a listener:\n{}",
        told.join("\n")
    );
    assert!(
        told.iter().any(|line| line.contains(&layer)),
        "the startup refusal did not name the configuration file it read, {layer}:\n{}",
        told.join("\n")
    );
    said_every_line_of(&told, &refused.to_string());
    (refused, told)
}

/// Asserts the process wrote every line of `message`.
///
/// **Line by line rather than as one string, and the channel is the reason.** `harness::forward`
/// feeds standard output and standard error into ONE channel from two threads, so a banner line
/// the process wrote before the refusal can be scheduled between two lines of it. Each stream is
/// ordered against itself and the two are not ordered against each other, so a `contains` over
/// the joined log would be a test that passes on a scheduling accident.
///
/// `contains` per line rather than equality, because the first line of a refusal reaches
/// standard error behind `main`'s own `sutura-serve: ` prefix.
fn said_every_line_of(told: &[String], message: &str) {
    for expected in message.lines() {
        assert!(
            told.iter().any(|line| line.contains(expected)),
            "the deployment did not tell the operator `{expected}`:\n{}",
            told.join("\n")
        );
    }
}

#[test]
fn a_non_loopback_bind_with_no_declared_terminator_stops_the_process() {
    // The single change that turns a local tool into a network service, refused on the binary.
    // The deployment is otherwise the one every serving case in this file starts: the same
    // catalog, the same source, the same deployment token - so the wildcard bind is the only
    // thing that can account for the refusal.
    let settings = deployment(
        &example_root(),
        "  host: \"0.0.0.0\"\n  port: 0\n",
        &format!("{SINGLE_USER}  access_token: \"{TOKEN}\"\n"),
    );
    let (refused, told) = stopped_before_binding(Environment::Development, "undeclared-bind", &settings);
    let SettingsError::NotFitToServe { ref refusals } = *refused.reason() else {
        panic!("this fixture is meant to be a posture refusal and is {refused:?}");
    };
    // The WHOLE set, not a `contains`: every refusal in it is pinned by value, so the case cannot
    // be green because the process stopped for a reason nobody named. The second is the metrics
    // credential - this deployment is reachable off-host and gates `/metrics` with nothing of its
    // own, which `docs/adr/0015` Decision 1 makes a refusal at this bind. The third is the
    // limiter, keyed on `off_host` the same way: this fixture sets no `rate_limit.enabled` and
    // development defaults it off.
    assert_eq!(refusals.len(), 3, "{refusals:?}");
    let NotFitToServe::TlsTerminationUndeclared { bind, origin } = &refusals[0] else {
        panic!("expected the undeclared-bind refusal, got {refusals:?}");
    };
    assert_eq!(bind.to_string(), "0.0.0.0:0");
    assert_eq!(
        refusals[1],
        NotFitToServe::MetricsTokenRequired {
            because: TokenRequiredBy::OffHost
        }
    );
    assert_eq!(
        refusals[2],
        NotFitToServe::RateLimitingDisabled {
            because: TokenRequiredBy::OffHost
        }
    );
    // The bind came from the fixture's own `base.yaml`, so the refusal must name that file -
    // the same one the harness already asserted appears in the log. (The precise equality with
    // that path, through the canonical spelling the `config` crate leaves relative, is asserted
    // in `sutura-config` where the file still exists; here the file is already reclaimed, so
    // the suffix is the whole of what can be asked of a reclaimed path.)
    assert!(origin.ends_with("base.yaml"), "the refusal did not name the file: {origin}");
    said_every_line_of(&told, &refused.reason().to_string());
}

#[test]
fn a_production_deployment_with_no_credential_stops_the_process() {
    // The only case here that is about the ENVIRONMENT rather than about a key: this same file
    // starts on a laptop and refuses in production, which is why `harness::command` takes an
    // `Environment` at all.
    //
    // The port is the embedded default rather than `0`, and that is load-bearing: `server.port: 0`
    // in production is `EphemeralPortInProduction`, so a fixture that kept the harness's
    // kernel-chosen port would refuse for two reasons and this case could not say which one
    // stopped the process. Safe because the assertion below is that nothing was ever bound.
    let settings = deployment(&example_root(), "  host: \"127.0.0.1\"\n", SINGLE_USER);
    let (refused, told) = stopped_before_binding(Environment::Production, "production-no-token", &settings);
    let SettingsError::NotFitToServe { ref refusals } = *refused.reason() else {
        panic!("this fixture is meant to be a posture refusal and is {refused:?}");
    };
    // Both refusals, by value: the API surface has no credential and `/metrics` has none of its
    // own, and production is where both are required. Pinning the whole set is what keeps this
    // case from passing because the process stopped for a third, unnamed reason.
    assert_eq!(
        *refusals,
        vec![
            NotFitToServe::AccessTokenRequired {
                because: TokenRequiredBy::Production
            },
            NotFitToServe::MetricsTokenRequired {
                because: TokenRequiredBy::Production
            },
        ]
    );
    said_every_line_of(&told, &refused.reason().to_string());
}

#[test]
fn a_configured_source_with_no_declared_mode_stops_the_process() {
    // `security.identity` has no default and no derivation, and this is the refusal on the
    // binary that makes that true of the artefact rather than of the type. The fixture leaves
    // out exactly `SINGLE_USER` and keeps everything else, including the deployment token - so
    // the missing mode is the whole difference from a deployment that serves.
    let settings = deployment(&example_root(), LOOPBACK, &format!("  access_token: \"{TOKEN}\"\n"));
    let (refused, told) = stopped_before_binding(Environment::Development, "undeclared-mode", &settings);
    let SettingsError::NotFitToServe { ref refusals } = *refused.reason() else {
        panic!("this fixture is meant to be a posture refusal and is {refused:?}");
    };
    // **This line restates `crates/sutura-config/src/settings/tests.rs`'s own assertion, and it is
    // here anyway.** Its job in this file is not coverage of the refusal - that test has it - but
    // to stop the derived expectation and the binary moving TOGETHER: a `refusals()` that answered a
    // different refusal for this deployment would change the expected sentence and the printed
    // one alike, and the case would stay green over a process refused for the wrong reason.
    // Measured on the neighbouring case: with `refusals()` pushing `InProcessTlsWithoutMaterial`
    // for the non-loopback bind and its equivalent line deleted, `just serve-e2e` is `40 passed`
    // at exit 0.
    assert_eq!(*refusals, vec![NotFitToServe::DeploymentIdentityUndeclared { count: 1 }]);
    said_every_line_of(&told, &refused.reason().to_string());
}

#[test]
fn a_misspelled_key_stops_the_process_and_the_operator_is_told_which_key() {
    // The fourth refusal, and the only one that is not a posture: `deny_unknown_fields` makes an
    // unknown key a deserialization error, so this exercises the OTHER arm of the startup path -
    // an error with a `#[source]` chain under it.
    //
    // Which makes it the one case that holds `main::flatten`, and nothing did. The outer sentence
    // adds the file layers above "the configuration sources could not be read"; the key to
    // fix is in the cause, and a root that printed `error.to_string()` alone would leave them
    // reading a deployment's whole settings tree looking for a typo. The three assertions are
    // therefore the outer sentence, the chain marker, and the key itself.
    let settings = deployment(
        &example_root(),
        &format!("{LOOPBACK}  prot: 9000\n"),
        &format!("{SINGLE_USER}  access_token: \"{TOKEN}\"\n"),
    );
    let (refused, told) = stopped_before_binding(Environment::Development, "misspelled-key", &settings);
    assert!(
        matches!(refused.reason(), SettingsError::Source { .. }),
        "a misspelled key is meant to be a source error and is {refused:?}"
    );
    said_every_line_of(&told, &refused.reason().to_string());
    // **One line has to carry both, and that is a correction rather than a tightening for its own
    // sake.** Written as two independent `any`, `contains("prot")` was satisfied by the key
    // appearing ANYWHERE the process wrote - and `flatten` is exactly the code that decides
    // whether the cause reaches standard error at all, so a chain dropped while the key survived
    // elsewhere would have passed both. Anchoring the key to the line carrying `caused by:` makes
    // it one claim: the cause was walked, and the walked cause names the key.
    //
    // `caused by:` is `main::flatten`'s own wording, in this crate rather than across a
    // boundary, so this pins the walk in the file that performs it.
    assert!(
        told.iter().any(|line| line.contains("caused by:") && line.contains("prot")),
        "the operator was not shown a cause naming the key they misspelled:\n{}",
        told.join("\n")
    );
}
