//! The one `#[ignore]`d cell that needs a real Keycloak tier, split out of `served.rs`'s own `mod
//! tests` by `cargo xtask max-lines`'s 1000-line cap - the same reason `served/harness.rs` is a
//! separate file rather than inline. Declared at the top level, a sibling of `mod tests` rather
//! than nested inside it, for the reason `served.rs`'s own header gives for `mod harness`: a
//! `#[path]` on a module declared INSIDE an inline module resolves against this file's own
//! directory conventions, not the nesting - `mod harness`'s comment states the same trap by name.
//!
//! `#[cfg(unix)]` and `#[cfg(test)]` as two attributes, not `cfg(all(test, unix))`: clippy's
//! `allow-expect-in-tests` and `tests_outside_test_module` both look for a literal `#[cfg(test)]`
//! on an ancestor and do not see through an `all(..)`.

// `RECORD`, `RESOURCE`, `keycloak_settings`, `recurring_revenue_june`, `start_configured` and `v1`
// are the same `pub(crate)` items `mod tests` imports from `crate::harness` - this module reaches
// them the same way, as a sibling rather than a descendant.
use crate::harness::{RECORD, RESOURCE, keycloak_settings, keycloak_subject_of, recurring_revenue_june, start_configured, v1};

// The `subject` field of one bunyan record line - `RECORD`'s own shape, read rather than
// assumed. The audit record masks each `sub` to its first character plus `***`, so the field
// alone cannot distinguish two subjects whose UUIDs share a first hex character (1 in 16); the
// cell below ties each record to ITS OWN token's full `sub` claim instead - see the assertion
// comment where that comparison is made.
fn subject_field(line: &str) -> &str {
    let after = line
        .split_once(r#""subject":""#)
        .map_or_else(|| panic!("no `subject` field in the record: {line}"), |(_, rest)| rest);
    after
        .split_once('"')
        .map_or_else(|| panic!("the `subject` field is not terminated: {line}"), |(value, _)| value)
}

#[test]
#[ignore = "needs the keycloak tier; run via `just keycloak-served-test`, which brings it up first"]
fn a_real_keycloak_issued_token_is_verified_by_the_composed_binary_and_a_wrong_audience_is_refused() {
    // Leg 1 over a REAL issuer, on the composed binary, filling the one gap
    // `docs/where-identity-is-proven.md` names beside the mock: the mock can generate no RSA
    // key, so nothing before this cell exercised a real provider's own signature, JWKS document
    // or discovery document through this deployment. It does NOT answer issue #105's own
    // question - see the plan for why a password grant against this tier's one client is not a
    // third party's audience in that issue's sense.
    let fixture = keycloak_settings("keycloak-leg-one");
    let served = start_configured("keycloak-leg-one", &fixture.settings_naming(&fixture.resource));

    let reply = served.post(
        &v1(sutura_http::constants::base_paths::QUERY),
        Some(&fixture.subject_a_token),
        &recurring_revenue_june(),
    );
    assert_eq!(reply.status, 200, "{}", reply.body);
    assert_eq!(
        reply.json()["rows"],
        serde_json::json!([["2026-06-01", "202121"]]),
        "{}",
        reply.body
    );
    let after_a = served.awaiting(RECORD);
    let record_a = after_a
        .iter()
        .rev()
        .find(|line| line.contains(RECORD))
        .expect("`awaiting` returns only once a line carries it");
    assert!(
        record_a.contains(r#""subject_established":"verified""#),
        "the record for a real token does not say a caller was verified:\n{record_a}"
    );

    // The OTHER provisioned subject, over the same deployment: a different `sub`, so the audit
    // record has to name a different subject too - the two-subject property
    // `nix/keycloak-tier.nix` provisions for and `docs/adr/0008` draws.
    let reply = served.post(
        &v1(sutura_http::constants::base_paths::QUERY),
        Some(&fixture.subject_b_token),
        &recurring_revenue_june(),
    );
    assert_eq!(reply.status, 200, "{}", reply.body);
    let after_b = served.awaiting(RECORD);
    let record_b = after_b
        .iter()
        .rev()
        .find(|line| line.contains(RECORD))
        .expect("`awaiting` returns only once a line carries it");
    // **Not a comparison of the two MASKED subjects.** The audit record masks each `sub` to its
    // first character plus `***` (`mask_principal_into` in `sutura-domain`'s `principal`), and
    // Keycloak subjects are UUIDs, so two distinct users' masked forms collide 1 in 16 - a run
    // would pass on the shared hex prefix alone. The property rests instead on the full `sub` each
    // token's OWN payload mints (the harness decoded it), and each record is tied to its own
    // token's mask via `SubjectId::parse` - the same mask the deployment wrote - so the assertion
    // would fail if the harness minted the same user twice, whatever the prefix.
    let sub_a = keycloak_subject_of(&fixture.subject_a_token);
    let sub_b = keycloak_subject_of(&fixture.subject_b_token);
    assert_ne!(sub_a, sub_b, "two provisioned subjects minted the same `sub` claim");
    let mask = |sub: &str| {
        sutura_domain::identity::SubjectId::parse(sub)
            .expect("a Keycloak UUID parses as a subject")
            .to_string()
    };
    assert_eq!(
        subject_field(record_a),
        mask(&sub_a),
        "the record does not carry the mask of ITS OWN token's subject:\n{record_a}"
    );
    assert_eq!(
        subject_field(record_b),
        mask(&sub_b),
        "the record does not carry the mask of ITS OWN token's subject:\n{record_b}"
    );

    // The negative: the SAME valid, correctly-signed token against a deployment declaring a
    // DIFFERENT resource - a real issuer's audience mismatch, refused the same way a mock's is.
    let wrong = start_configured("keycloak-wrong-audience", &fixture.settings_naming(RESOURCE));
    let reply = wrong.post(
        &v1(sutura_http::constants::base_paths::QUERY),
        Some(&fixture.subject_a_token),
        &recurring_revenue_june(),
    );
    assert_eq!(
        reply.status, 401,
        "a real token minted for a different audience was accepted: {}",
        reply.body
    );
    assert_eq!(reply.json()["code"], "unauthorized", "{}", reply.body);
}
