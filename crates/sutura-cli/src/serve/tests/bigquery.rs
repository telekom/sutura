//! `bigquery`-kind sources: the settings-tree refusal for a missing key, the composition root's own
//! refusal for a build that did not link the adapter, and - on a build that did - the furthest a
//! fixture with no project can reach: the credential layer, the billed-bytes ceiling, the anchor's
//! verification rule, and the boot line for the credential cache's default. Moved out of `tests.rs`
//! as a pure relocation to keep that file under the 1000-line cap with room to spare.

use super::support::bundle_with_an_anchor;
use super::{bigquery_entry, default_timeout, one_worker, open_engine, opened_bigquery, refusal, registry, wif};

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
    let error = sutura_config::Settings::load(
        &sutura_config::Sources::defaults(sutura_config::Environment::Development).with_overlay(overlay),
    )
    .expect_err("a bigquery source with no credential file is not a source this deployment can open");
    let rendered = super::super::flatten(error);
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
fn a_bigquery_source_refuses_for_a_missing_driver_not_the_credential_file() {
    // **What this proves, and it is the whole seam a test with no driver can reach:** the kind
    // DISPATCHED to the BigQuery adapter, the shared posture was accepted against that adapter's
    // OWN `IMPERSONATION`, both bounds parsed, and the composition then demanded the on-disk ADBC
    // driver - refused here because no `SUTURA_BIGQUERY_ADBC_DRIVER` points at one. The driver
    // authenticates ambiently, so the `credential_file` a `bigquery_entry` declares is deliberately
    // NOT read: matching the driver refusal is exactly how this proves the file never is.
    let error = refusal(
        opened_bigquery(&bigquery_entry("warehouse", "shared-service-user", "")),
        "the ADBC driver is not configured, so this deployment does not start",
    );
    assert!(
        error.contains("SUTURA_BIGQUERY_ADBC_DRIVER"),
        "the refusal must name the driver variable an operator has to set: {error}"
    );
    assert!(error.contains("warehouse"), "the refusal must name the source: {error}");
    assert!(
        !error.contains("credential_file"),
        "the driver authenticates ambiently - a credential file must not be read: {error}"
    );
    assert!(
        !error.contains("--features bigquery"),
        "this build DID link the adapter: {error}"
    );
}

#[test]
#[cfg(feature = "bigquery")]
fn an_impersonating_source_is_opened_and_reaches_the_driver_refusal() {
    // **Issue 87's serve half, and the reversal is the point of the change.** The adapter declares
    // `PerSubjectCredential`, so the serve open fn's capability cross-check ACCEPTS an
    // `impersonation-at-source` entry - it is not refused here by name. Whether a subject's own
    // identity reaches the driver is `run()`'s refusal (the ADBC transport refuses a
    // `subject_bearer`), which this boot seam cannot reach without a driver. What a test with no
    // driver CAN reach is the demand for the on-disk driver itself; matching that instead of an
    // `impersonation-at-source` refusal is what shows the capability half still accepts the posture.
    let error = refusal(
        opened_bigquery(&bigquery_entry(
            "warehouse",
            "impersonation-at-source",
            &format!("{}    verification_identity: \"sutura_anchor_reader\"\n", wif()),
        )),
        "an impersonating source passes the capability check, so its refusal is the missing driver",
    );
    assert!(error.contains("warehouse"), "the refusal must name the source: {error}");
    assert!(
        error.contains("SUTURA_BIGQUERY_ADBC_DRIVER"),
        "the source was OPENED past the posture cross-check and its refusal names the driver: {error}"
    );
    assert!(
        !error.contains("does not attach a broker"),
        "the serve open fn still accepts the posture - impersonation is not refused by name here: {error}"
    );
}

// The ceiling test `a_bigquery_ceiling_the_adapter_will_not_send_is_a_startup_refusal_naming_the_key`
// lived here. It held the `BytesBilledCeiling` range parse the removed `wire` transport applied to
// `max_bytes_billed`; the ADBC driver reads no such ceiling, so the refusal it asserted is gone with
// the wire - deleted rather than adapted, because there is no boot-time number left to test.

// `a_request_timeout_that_leaves_no_job_budget_does_not_start` lived here: a ten-second
// `server.request_timeout_seconds` used to refuse a `bigquery` deployment at boot, because
// `QueryDeadline::within_request_timeout` divided that number by the two calls one answer makes and
// found nothing left. `docs/adr/0029` retired that arithmetic - a request-time job now derives
// `timeoutMs`/`jobTimeoutMs` from the port's own `Deadline`, which the transport opens from the SAME
// key without dividing it, so a ten-second `server.request_timeout_seconds` is a usable (if narrow)
// budget rather than an unservable one. The refusal this test held is gone with the arithmetic that
// produced it; deleted rather than adapted, because there is no boot-time number left to test.

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
            None,
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
            None,
        ),
        "the credential is still not there, so the deployment still does not start",
    );
    assert!(
        !later.contains("verification_identity"),
        "a shared source's anchors run as the shared identity: {later}"
    );
}
