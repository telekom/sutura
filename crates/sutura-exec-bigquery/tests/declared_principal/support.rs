/// One environment value this hosted leg needs, or a panic naming the variable and never its value.
///
/// **A panic and not a skip**: a hosted venue whose environment is half-configured must be RED. A
/// project id and an account address are what a public workflow log should not learn from a failure.
pub(super) fn live_env(name: &str) -> String {
    match std::env::var(name) {
        Ok(value) if !value.trim().is_empty() => value,
        _ => panic!("{name} is unset or empty, and this leg proves nothing without it"),
    }
}
