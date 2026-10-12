/// One environment value this hosted leg needs, or a panic naming the variable and never its value.
///
/// **A panic and not a skip**: a hosted venue whose environment is half-configured must be RED. A
/// project id and an account address are what a public workflow log should not learn from a failure.
///
/// **Every read is also refused outside CI**: this leg opens a real project, so a developer's shell
/// never runs it. **The limit:** `GITHUB_ACTIONS` is an environment value, so a shell that exports
/// `true` passes.
pub(super) fn live_env(name: &str) -> String {
    assert!(
        std::env::var("GITHUB_ACTIONS").is_ok_and(|ci| ci == "true"),
        "a live bigquery cell runs only in CI (GITHUB_ACTIONS=true); this host names a bigquery venue outside CI"
    );
    match std::env::var(name) {
        Ok(value) if !value.trim().is_empty() => value,
        _ => panic!("{name} is unset or empty, and this leg proves nothing without it"),
    }
}
