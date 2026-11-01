//! The raw tool asked to span sources with the `sql` feature OFF: this binary links no SQL parser,
//! so the switch, or a second listed source, is a startup refusal. Only `check-default-feature-tests`
//! runs this file.

#[test]
fn federation_allowed_is_refused_by_a_build_that_did_not_link_the_sql_frontend() {
    let error = sutura_config::Settings::load(
        &sutura_config::Sources::defaults(sutura_config::Environment::Development)
            .with_overlay("tools:\n  run_sql:\n    federation_allowed: true\n"),
    )
    .expect_err("a build with no SQL parser does not serve the federation switch");
    let sutura_config::SettingsError::NotFitToServe { ref refusals } = *error.reason() else {
        panic!("expected a posture refusal, got {error:?}");
    };
    assert_eq!(*refusals, vec![sutura_config::NotFitToServe::RunSqlFederationNotCompiledIn]);
    let rendered = refusals.first().expect("one refusal").to_string();
    assert!(
        rendered.contains("tools.run_sql.federation_allowed"),
        "the refusal must name the key: {rendered}"
    );
    assert!(
        rendered.contains("--features sql"),
        "the refusal must say what to build: {rendered}"
    );
}

#[test]
fn a_second_listed_source_is_refused_by_a_build_that_did_not_link_the_sql_frontend() {
    let error = sutura_config::Settings::load(
        &sutura_config::Sources::defaults(sutura_config::Environment::Development)
            .with_overlay("tools:\n  run_sql:\n    sources: [\"orders\", \"crm\"]\n"),
    )
    .expect_err("a build with no SQL parser runs one statement at one source");
    let sutura_config::SettingsError::NotFitToServe { ref refusals } = *error.reason() else {
        panic!("expected a posture refusal, got {error:?}");
    };
    assert_eq!(*refusals, vec![sutura_config::NotFitToServe::RunSqlFederationNotCompiledIn]);
    let rendered = refusals.first().expect("one refusal").to_string();
    assert!(rendered.contains("tools.run_sql.sources"), "{rendered}");
}
