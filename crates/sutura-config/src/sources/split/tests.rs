//! `split`'s value, and the entries it is refused on, read through the whole settings load.

use sutura_domain::model::SourceName;

use super::{SourceSplit, SplitRefused};
use crate::environment::Environment;
use crate::settings::{Settings, SettingsError, Sources};
use crate::sources::{InvalidSourceRegistry, SourceKind};

const SINGLE_USER: &str = "security:\n  identity: \"single-user\"\n  single_user_because: \"a test\"\n";

/// A shared `oracle` source named `warehouse`, with `extra` under it.
fn oracle(extra: &str) -> Sources {
    Sources::defaults(Environment::Development).with_overlay(format!(
        "{SINGLE_USER}sources:\n  warehouse:\n    kind: \"oracle\"\n    host: \"db.example.com\"\n    port: 2484\n    \
         service_name: \"orcl.example.com\"\n    user: \"sutura\"\n    password_file: \"/run/secrets/oracle-password\"\n    \
         transport_mode: \"verified\"\n    transport_anchors: \"/run/secrets/oracle-ca.pem\"\n    \
         posture: \"shared-service-user\"\n{extra}"
    ))
}

/// What the loaded `warehouse` entry declares.
fn declared(sources: &Sources) -> Option<SourceSplit> {
    let settings = Settings::load(sources).expect("the deployment loads");
    let warehouse = SourceName::parse("warehouse").expect("a legal identifier");
    settings
        .sources()
        .each()
        .find(|&(alias, _)| *alias == warehouse)
        .map_or_else(|| panic!("the warehouse entry is declared"), |(_, source)| source.split())
}

fn refusal(sources: &Sources) -> SplitRefused {
    let error = Settings::load(sources).expect_err("the split is refused at load");
    match *error.reason() {
        SettingsError::Sources {
            cause: InvalidSourceRegistry::Split { ref alias, cause },
        } => {
            assert_eq!(alias.as_str(), "warehouse");
            cause
        }
        ref other => panic!("expected a split refusal, got {other:?}"),
    }
}

/// A shared Oracle source declares `flashback`, and one that writes nothing reads every leg whole.
#[test]
fn a_shared_oracle_source_declares_a_flashback_split_and_no_key_declares_none() {
    assert_eq!(declared(&oracle("    split: \"flashback\"\n")), Some(SourceSplit::Flashback));
    assert_eq!(declared(&oracle("")), None);
}

/// **Each refusal, by name**: a word that is not `flashback`, any kind but `oracle`, and an Oracle
/// source that reads as each caller.
#[test]
fn a_split_is_refused_unless_a_shared_oracle_source_writes_flashback() {
    assert_eq!(refusal(&oracle("    split: \"buckets\"\n")), SplitRefused::Unknown);

    let files = Sources::defaults(Environment::Development).with_overlay(format!(
        "{SINGLE_USER}sources:\n  warehouse:\n    kind: \"files\"\n    data_dir: \"/srv/sutura/data\"\n    \
         posture: \"shared-service-user\"\n    split: \"flashback\"\n"
    ));
    assert_eq!(refusal(&files), SplitRefused::NotOracle { kind: SourceKind::Files });

    let impersonating = Sources::defaults(Environment::Development).with_overlay(format!(
        "{SINGLE_USER}sources:\n  warehouse:\n    kind: \"oracle\"\n    host: \"db.example.com\"\n    port: 2484\n    \
         service_name: \"orcl.example.com\"\n    user: \"sutura\"\n    password_file: \"/run/secrets/oracle-password\"\n    \
         transport_mode: \"verified\"\n    transport_anchors: \"/run/secrets/oracle-ca.pem\"\n    \
         posture: \"impersonation-at-source\"\n    delegation:\n      - token_endpoint: \"https://idp.example.com/token\"\n        \
         client_id: \"sutura\"\n        client_secret_file: \"/run/secrets/idp-client\"\n        grant: \"on-behalf-of\"\n        \
         audience: \"database-client-id\"\n    split: \"flashback\"\n"
    ));
    assert_eq!(refusal(&impersonating), SplitRefused::Impersonating);
}
