//! Direct Google mode binds the caller's token and the jobs.query route together.
use super::*;
#[test]
fn a_direct_google_job_carries_only_its_access_token_and_the_readonly_jobs_query_route() {
    let token = Secret::new("opaque-google-fixture");
    let options = authenticate(
        JobIdentity::AsSubject { assertion: &token },
        &Impersonation::CallerGoogleToken,
        None,
    )
    .expect("caller job options")
    .options;
    let shown: Vec<_> = options
        .into_iter()
        .map(|(key, value)| (format!("{key:?}"), format!("{value:?}")))
        .collect();
    for (key, value) in [
        (AUTH_TYPE, AUTH_TYPE_ACCESS_TOKEN),
        (AUTH_ACCESS_TOKEN, "opaque-google-fixture"),
        (QUERY_THROUGH_JOBS_QUERY, "true"),
    ] {
        assert!(
            shown
                .iter()
                .any(|(found, text)| found.contains(key) && text == &format!("String({value:?})")),
            "{key}: {shown:?}"
        );
    }
    assert_eq!(shown.len(), 3, "no refresh credential or impersonating account is added");
}
