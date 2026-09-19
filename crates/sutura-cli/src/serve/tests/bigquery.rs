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
