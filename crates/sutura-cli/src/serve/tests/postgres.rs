//! The `postgres` source's own boot-refusal cell, split out of the parent by the file-length gate.
//!
//! **Moved here rather than shortened**, the same choice `main.rs`'s own module header names for
//! `boot.rs`: `cargo xtask max-lines` fails rather than warning past 1000, `devco/max-lines-ignore`
//! cannot exempt anything under `crates/`, and the answer to a long file is to split it.

use super::{OpenedSources, bundle_over, default_timeout, one_worker, open_engine, refusal, registry, wif};

/// One `sources:` entry for a `Postgres` database, with every key that kind is opened with.
///
/// `127.0.0.1` with `transport_mode: "plaintext"` is the one combination issue 124's non-loopback
/// fail-closed still parses - a remote host declared plaintext is a settings-tree refusal, tested in
/// `sutura-config`, and would stop these cells before they reached the composition root's own cross-
/// check. The password file points at a path that is not there, for the reason `bigquery_entry`'s
/// credential file does: a refusal naming that key is proof the composition reached the connection
/// layer, which is the furthest a test with no server can get.
///
/// Gated on `postgres` itself: the only caller today is the impersonation cross-check below, which
/// is gated the same way - unlike `bigquery_entry`, nothing here is exercised on a build without the
/// feature, so leaving it unconditional would be dead code there.
#[cfg(feature = "postgres")]
fn postgres_entry(alias: &str, posture: &str, extra: &str) -> String {
    format!(
        "  {alias}:\n    kind: \"postgres\"\n    host: \"127.0.0.1\"\n    port: 5432\n    database: \
         \"warehouse\"\n    user: \"sutura\"\n    password_file: \"/nonexistent/sutura-test-postgres-password\"\n    \
         transport_mode: \"plaintext\"\n    posture: \"{posture}\"\n{extra}"
    )
}

/// The startup a `postgres` source produces, whichever way this binary was built.
#[cfg(feature = "postgres")]
fn opened_postgres(entries: &str) -> Result<OpenedSources, String> {
    open_engine(
        &bundle_over(&[("customers", "warehouse", "dim_customer")]),
        &registry(entries),
        one_worker(),
        default_timeout(),
        None,
    )
}

#[test]
#[cfg(feature = "postgres")]
fn a_postgres_source_configured_to_impersonate_refuses_at_boot() {
    // **The Postgres half of the cross-check the neighbouring
    // `a_source_configured_to_impersonate_on_an_adapter_that_cannot_refuses_at_boot` proves over the
    // in-process engine.** `PostgresWarehouse::IMPERSONATION` is `NoPlaceForASubject` - one
    // connection under the deployment's declared identity, with nowhere for a subject's own
    // credential to arrive - and `build_postgres` runs this check BEFORE it reads `password_file` or
    // dials anything, so the refusal is reachable with no server listening and no password file on
    // disk.
    //
    // Until this cell existed, that ordering was proven for the engine and for BigQuery and asserted
    // nowhere for this adapter - a Postgres entry declared `impersonation-at-source` had never been
    // opened by a test at all.
    let error = refusal(
        opened_postgres(&postgres_entry("warehouse", "impersonation-at-source", wif())),
        "an impersonating posture on an adapter with nowhere for a subject's credential to arrive must not start",
    );
    assert!(error.contains("warehouse"), "the refusal must name the source: {error}");
    assert!(
        error.contains("per-subject credential"),
        "the refusal must say what the adapter cannot do: {error}"
    );
    assert!(
        error.contains("no fallback"),
        "the refusal must say there is no fallback: {error}"
    );
    // NOT the neighbouring arms: the entry is declared, the build DOES link the adapter, and the
    // check fires before the connection step would name the unreadable password file.
    assert!(
        !error.contains("--features postgres"),
        "this build DID link the adapter: {error}"
    );
    assert!(
        !error.contains("password_file"),
        "the capability cross-check fires before the password file is read: {error}"
    );
}
