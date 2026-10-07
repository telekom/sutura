//! `server.allowed_hosts`: a written list reaches the typed settings, and an entry that is not a
//! bare host refuses at startup.

use crate::settings::{Environment, Settings, SettingsError, Sources};

#[test]
fn declared_hosts_reach_the_server_settings_normalised() {
    let sources = Sources::defaults(Environment::Development)
        .with_overlay("server:\n  allowed_hosts: [\"Sutura.Example.com\", \"10.0.0.7\"]\n");
    let settings = Settings::load(&sources).expect("a list of hosts loads");
    let hosts: Vec<&str> = settings
        .server()
        .allowed_hosts()
        .iter()
        .map(crate::AllowedHost::as_str)
        .collect();
    assert_eq!(hosts, ["sutura.example.com", "10.0.0.7"]);
}

#[test]
fn no_declared_hosts_is_the_default() {
    let settings = Settings::load(&Sources::defaults(Environment::Development)).expect("the defaults load");
    assert_eq!(settings.server().allowed_hosts(), []);
}

#[test]
fn a_declared_host_that_is_not_a_bare_host_is_refused_at_startup() {
    for written in ["https://sutura.example.com", "sutura.example.com:8080", "*.example.com", ""] {
        let sources =
            Sources::defaults(Environment::Development).with_overlay(format!("server:\n  allowed_hosts: [\"{written}\"]\n"));
        let error = Settings::load(&sources).expect_err(written);
        assert!(
            matches!(error.reason(), SettingsError::AllowedHost { .. }),
            "{written}: {error:?}"
        );
    }
}
