//! `server.nodes` through `Settings::load`: declared without a node authority it is refused, an
//! address that is not `https` is refused naming it, a leg size above which a question goes
//! across rides with the nodes, never as zero, and so does a bounded wait for other nodes, and the
//! family of address a peer declared by a host is dialled at. A `clickhouse` source's hash-bucket
//! reason rides with that source.
//!
//! The refusal cells read the refusal only through what the base tree already exports - its
//! variant and `Display` - so they build there and fail on its behaviour.

use core::num::NonZeroU64;

use crate::server::TlsMaterial;
use crate::settings::{Environment, NotFitToServe, Settings, SettingsError, Sources};

const PAIR: &str = "server:\n  tls_certificate: \"/tls/chain.pem\"\n  tls_key: \"/tls/key.pem\"\n";

fn refusal(overlay: String) -> String {
    let error =
        Settings::load(&Sources::defaults(Environment::Development).with_overlay(overlay)).expect_err("these nodes are refused");
    let SettingsError::TlsMaterial { ref cause } = *error.reason() else {
        panic!("expected a TLS material refusal, got {error:?}");
    };
    cause.to_string()
}

/// `#828`: a peer entry under `others` is now a map naming its `url`, `role` and the source
/// aliases it reads - never a bare string.
const WORKER_PEER: &str = "    others:\n      - url: \"https://b.example:8443\"\n        role: \"worker\"\n";

#[test]
fn nodes_declared_without_a_node_authority_are_refused() {
    let nodes = format!("  nodes:\n    own: \"https://a.example:8443\"\n{WORKER_PEER}");
    for overlay in [format!("{PAIR}{nodes}"), format!("server:\n{nodes}")] {
        assert_eq!(refusal(overlay), "server.nodes is set and server.tls_node_authority is not");
    }
}

#[test]
fn a_node_address_that_is_not_https_is_refused_naming_it() {
    let overlay = format!(
        "{PAIR}  tls_node_authority: \"/tls/nodes.pem\"\n  nodes:\n    own: \"https://a.example:8443\"\n    \
         others:\n      - url: \"http://b.example:8443\"\n        role: \"worker\"\n"
    );
    assert_eq!(
        refusal(overlay),
        "`http://b.example:8443` in server.nodes is not an https URL with a host"
    );
}

#[test]
fn a_leg_size_declared_with_the_nodes_rides_with_them_and_zero_is_refused() {
    let nodes = |above: u64| {
        Sources::defaults(Environment::Development).with_overlay(format!(
            "{PAIR}  tls_node_authority: \"/tls/nodes.pem\"\n  nodes:\n    own: \"https://a.example:8443\"\n\
             {WORKER_PEER}    across_above_bytes: {above}\n\
             security:\n  tls_termination: \"in-process\"\n"
        ))
    };
    let settings = Settings::load(&nodes(4096)).expect("a declared leg size loads");
    let declared = settings
        .server()
        .tls()
        .and_then(TlsMaterial::nodes)
        .expect("the nodes are declared");
    assert_eq!(declared.across_above().map(NonZeroU64::get), Some(4096));
    let error = Settings::load(&nodes(0)).expect_err("a leg size of zero is refused");
    assert!(matches!(*error.reason(), SettingsError::Source { .. }), "{error:?}");
}

#[test]
fn a_wait_for_workers_declared_with_the_nodes_loads() {
    let overlay = format!(
        "{PAIR}  tls_node_authority: \"/tls/nodes.pem\"\n  nodes:\n    own: \"https://a.example:8443\"\n\
         {WORKER_PEER}    wait_for_workers_seconds: 5\n\
         security:\n  tls_termination: \"in-process\"\n"
    );
    let settings =
        Settings::load(&Sources::defaults(Environment::Development).with_overlay(overlay)).expect("a declared wait loads");
    assert!(
        settings.server().tls().and_then(TlsMaterial::nodes).is_some(),
        "the nodes are declared"
    );
}

/// `#828`: a declared peer's source alias this node's own `sources:` does not declare is a posture
/// refusal, not a parse-time one - `TlsMaterial::with_nodes` cannot see `sources:` to check it.
#[test]
fn a_declared_node_reading_an_undeclared_source_is_not_fit_to_serve() {
    let overlay = format!(
        "{PAIR}  tls_node_authority: \"/tls/nodes.pem\"\n  nodes:\n    own: \"https://a.example:8443\"\n    \
         others:\n      - url: \"https://b.example:8443\"\n        role: \"reading-worker\"\n        \
         sources:\n          - \"missing\"\nsecurity:\n  tls_termination: \"in-process\"\n"
    );
    let error = Settings::load(&Sources::defaults(Environment::Development).with_overlay(overlay))
        .expect_err("a peer reading a source this node never declares is refused");
    let SettingsError::NotFitToServe { ref refusals } = *error.reason() else {
        panic!("expected a posture refusal naming the undeclared source, got {error:?}");
    };
    assert_eq!(
        refusals
            .iter()
            .filter(|&refusal| *refusal != NotFitToServe::InProcessTlsNotCompiledIn)
            .cloned()
            .collect::<Vec<_>>(),
        vec![NotFitToServe::DeclaredNodeReadsUndeclaredSource {
            url: String::from("https://b.example:8443/"),
            alias: String::from("missing"),
        }]
    );
}

/// `server.role` is a key a served tree may name, beside the nodes a worker serves.
#[test]
fn a_served_tree_naming_its_role_loads() {
    let sources = Sources::defaults(Environment::Development).with_overlay("server:\n  role: \"serving\"\n");
    Settings::load(&sources).expect("a tree naming the serving role loads");
}

/// A serving node, its source `local`, and one peer: a `reading-worker` reading `local`, or a
/// `worker` reading nothing - with `security` after it.
fn peered(reading: bool, security: &str) -> String {
    let peer = if reading {
        "      - url: \"https://b.example:8443\"\n        role: \"reading-worker\"\n        sources:\n          - \"local\"\n"
    } else {
        "      - url: \"https://b.example:8443\"\n        role: \"worker\"\n"
    };
    format!(
        "{PAIR}  tls_node_authority: \"/tls/nodes.pem\"\n  nodes:\n    own: \"https://a.example:8443\"\n    others:\n{peer}\
         sources:\n  local:\n    kind: \"files\"\n    data_dir: \"/srv/sutura/local\"\n    posture: \"shared-service-user\"\n    \
         acknowledged_because: \"a directory of CSVs this deployment owns\"\n\
         security:\n  tls_termination: \"in-process\"\n{security}"
    )
}

/// What `overlay` is refused with, or nothing when it loads.
fn refused_with(overlay: String) -> Vec<NotFitToServe> {
    match Settings::load(&Sources::defaults(Environment::Development).with_overlay(overlay)) {
        Ok(_) => Vec::new(),
        Err(error) => match *error.reason() {
            SettingsError::NotFitToServe { ref refusals } => refusals.clone(),
            ref other => panic!("expected the deployment to load or be refused at its posture, got {other:?}"),
        },
    }
}

fn reading_without_inbound() -> NotFitToServe {
    NotFitToServe::ReadingNodeWithoutInboundIdentity {
        url: String::from("https://b.example:8443/"),
        alias: String::from("local"),
    }
}

/// `#828`: a lookup leg read on a reading node is granted for a verified caller, so a deployment that
/// verifies none and declares one is refused before it serves, naming the remedy.
#[test]
fn a_reading_node_on_a_deployment_that_verifies_no_caller_is_not_fit_to_serve() {
    let refusals = refused_with(peered(true, ""));
    assert!(refusals.contains(&reading_without_inbound()), "{refusals:?}");
    let rendered = reading_without_inbound().to_string();
    assert!(rendered.contains("Declare security.inbound"), "{rendered}");
}

#[test]
fn a_reading_node_on_a_deployment_that_verifies_its_callers_is_not_refused_for_it() {
    let refusals = refused_with(peered(true, super::inbound::DIRECT_INBOUND));
    assert!(!refusals.contains(&reading_without_inbound()), "{refusals:?}");
}

#[test]
fn declared_nodes_with_no_reading_node_and_no_inbound_identity_still_serve() {
    let refusals = refused_with(peered(false, ""));
    assert!(
        !refusals
            .iter()
            .any(|refusal| matches!(refusal, NotFitToServe::ReadingNodeWithoutInboundIdentity { .. })),
        "{refusals:?}"
    );
    let overlay = format!(
        "{PAIR}  tls_node_authority: \"/tls/nodes.pem\"\n  nodes:\n    own: \"https://a.example:8443\"\n\
         {WORKER_PEER}security:\n  tls_termination: \"in-process\"\n"
    );
    Settings::load(&Sources::defaults(Environment::Development).with_overlay(overlay))
        .expect("nodes with no reading node and no inbound identity still load");
}

/// `server.nodes` over `others`, each a worker, with `extra` appended under `nodes`.
fn nodes_over(others: &[&str], extra: &str) -> String {
    let peers = others
        .iter()
        .map(|url| format!("      - url: \"{url}\"\n        role: \"worker\"\n"))
        .collect::<Vec<_>>()
        .concat();
    format!(
        "{PAIR}  tls_node_authority: \"/tls/nodes.pem\"\n  nodes:\n    own: \"https://a.example:8443\"\n    others:\n{peers}{extra}\
         security:\n  tls_termination: \"in-process\"\n"
    )
}

/// The nodes `sources` declare, as `Debug` renders them: the family is read off the one public view
/// the base tree already has, so these cells build there and fail on its refusal of the key.
fn declared_nodes(sources: &Sources) -> String {
    let settings = Settings::load(sources).expect("these nodes load");
    format!(
        "{:?}",
        settings
            .server()
            .tls()
            .and_then(TlsMaterial::nodes)
            .expect("the nodes are declared")
    )
}

fn loaded(overlay: String) -> String {
    declared_nodes(&Sources::defaults(Environment::Development).with_overlay(overlay))
}

#[test]
fn each_address_family_loads_and_rides_with_the_nodes() {
    for (written, read) in [("auto", "Auto"), ("ipv4", "Ipv4"), ("ipv6", "Ipv6")] {
        let rendered = loaded(nodes_over(
            &["https://b.example:8443"],
            &format!("    address_family: \"{written}\"\n"),
        ));
        assert!(rendered.contains(&format!("address_family: {read}")), "{written}: {rendered}");
    }
}

#[test]
fn a_node_with_no_address_family_dials_either() {
    let rendered = loaded(nodes_over(&["https://b.example:8443"], ""));
    assert!(rendered.contains("address_family: Auto"), "{rendered}");
}

#[test]
fn the_address_family_can_be_set_by_a_variable() {
    let sources = Sources::defaults(Environment::Development)
        .with_overlay(nodes_over(&["https://b.example:8443"], ""))
        .with_variables(super::variables(&[("SUTURA__SERVER__NODES__ADDRESS_FAMILY", "ipv6")]));
    assert!(declared_nodes(&sources).contains("address_family: Ipv6"));
}

#[test]
fn an_unknown_address_family_is_refused_naming_the_value_and_the_choices() {
    let overlay = nodes_over(&["https://b.example:8443"], "    address_family: \"ipv5\"\n");
    let error = Settings::load(&Sources::defaults(Environment::Development).with_overlay(overlay))
        .expect_err("a family that is not auto, ipv4 or ipv6 is refused");
    assert!(matches!(*error.reason(), SettingsError::Source { .. }), "{error:?}");
    let rendered = format!("{error:?}");
    assert!(
        rendered.contains("`ipv5` is not an address family - one of: auto, ipv4, ipv6")
            && rendered.contains("server.nodes.address_family"),
        "{rendered}"
    );
}

/// The refusal as `Display` renders it, through the variant the base tree already has.
fn declared_node_refusal(overlay: String) -> String {
    let error = Settings::load(&Sources::defaults(Environment::Development).with_overlay(overlay))
        .expect_err("a peer of the other family is refused");
    let SettingsError::InvalidDeclaredNode { ref cause } = *error.reason() else {
        panic!("expected a refused peer, got {error:?}");
    };
    cause.to_string()
}

#[test]
fn a_peer_given_as_an_ip_literal_of_the_other_family_is_refused_at_load() {
    for (family, url) in [
        ("ipv6", "https://192.0.2.7:8443"),
        ("ipv4", "https://[2001:db8::7]:8443"),
        ("ipv6", "https://192.0.2.7:8443#192.0.2.7"),
        ("ipv4", "https://b.example:8443#2001:db8::7"),
    ] {
        let overlay = nodes_over(&[url], &format!("    address_family: \"{family}\"\n"));
        let written = url.split_once('#').map_or(url, |(origin, _)| origin);
        let refused = declared_node_refusal(overlay);
        assert!(
            refused.contains(&format!("`{written}")) && refused.contains(&format!("address_family `{family}`")),
            "{family} {url}: {refused}"
        );
    }
}

#[test]
fn a_peer_of_the_chosen_family_and_a_host_name_load_under_each_family() {
    for (family, read, peers) in [
        ("auto", "Auto", ["https://192.0.2.7:8443", "https://[2001:db8::7]:8443"]),
        ("ipv4", "Ipv4", ["https://192.0.2.7:8443", "https://b.example:8443"]),
        ("ipv6", "Ipv6", ["https://[2001:db8::7]:8443", "https://b.example:8443"]),
    ] {
        let rendered = loaded(nodes_over(&peers, &format!("    address_family: \"{family}\"\n")));
        assert!(rendered.contains(&format!("address_family: {read}")), "{family}: {rendered}");
    }
}

/// A serving node that verifies its callers, over two loopback `clickhouse` sources, with `warehouse`
/// declared `posture`, carrying `extra`, and read by one reading node.
fn hash_bucketed_deployment(posture: &str, extra: &str) -> Sources {
    let entry = |alias: &str, posture: &str| {
        format!(
            "  {alias}:\n    kind: \"clickhouse\"\n    host: \"127.0.0.1\"\n    port: 8123\n    user: \"sutura\"\n    \
             password_file: \"/run/secrets/ch-password\"\n    transport_mode: \"plaintext\"\n    posture: \"{posture}\"\n"
        )
    };
    Sources::defaults(Environment::Development).with_overlay(format!(
        "{PAIR}  tls_node_authority: \"/tls/nodes.pem\"\n  nodes:\n    own: \"https://a.example:8443\"\n    others:\n      \
         - url: \"https://b.example:8443\"\n        role: \"reading-worker\"\n        sources:\n          - \"warehouse\"\n\
         sources:\n{}{extra}{}security:\n  identity: \"single-user\"\n  single_user_because: \"a test\"\n  \
         tls_termination: \"in-process\"\n{}",
        entry("warehouse", posture),
        entry("orders", "shared-service-user"),
        super::inbound::DIRECT_INBOUND,
    ))
}

/// `#828`: a `clickhouse` source lets its lookup leg be read in hash buckets with the operator's
/// reason, shared or switched to each caller, and the reason is carried on that source alone.
#[test]
fn a_clickhouse_source_opting_into_hash_buckets_loads_carrying_the_reason() {
    let alias = |name: &str| sutura_domain::model::SourceName::parse(name).expect("a legal identifier");
    for posture in ["shared-service-user", "impersonation-at-source"] {
        let without = Settings::load(&hash_bucketed_deployment(posture, "")).expect("the deployment without the key loads");
        let with = Settings::load(&hash_bucketed_deployment(
            posture,
            "    hash_buckets_acknowledged_because: \"each key's rows are read together; the buckets share no snapshot\"\n",
        ))
        .expect("the deployment with the key loads");
        assert_ne!(
            with.sources().get(&alias("warehouse")),
            without.sources().get(&alias("warehouse")),
            "{posture}: the reason is carried"
        );
        // The operator's own words, not a reason of some other text: `Debug` is what base exports.
        let carried = format!("{:?}", with.sources().get(&alias("warehouse")));
        assert!(
            carried.contains("each key's rows are read together; the buckets share no snapshot"),
            "{posture}: {carried}"
        );
        assert_eq!(
            with.sources().get(&alias("orders")),
            without.sources().get(&alias("orders")),
            "{posture}"
        );
    }
}

/// A hash-bucket reason, on the entry it follows.
const BUCKET_REASON: &str = "    hash_buckets_acknowledged_because: \"each key's rows are read together\"\n";

/// A loopback, shared `clickhouse` source under `alias`, carrying `extra`.
fn bucketed_clickhouse(alias: &str, extra: &str) -> String {
    format!(
        "  {alias}:\n    kind: \"clickhouse\"\n    host: \"127.0.0.1\"\n    port: 8123\n    user: \"sutura\"\n    \
         password_file: \"/run/secrets/ch-password\"\n    transport_mode: \"plaintext\"\n    \
         posture: \"shared-service-user\"\n{extra}"
    )
}

/// A serving node that verifies its callers, with one reading node that reads `read`, over `sources`.
fn bucket_readers(read: &str, sources: &str) -> String {
    format!(
        "{PAIR}  tls_node_authority: \"/tls/nodes.pem\"\n  nodes:\n    own: \"https://a.example:8443\"\n    others:\n      \
         - url: \"https://b.example:8443\"\n        role: \"reading-worker\"\n        sources:\n          - \"{read}\"\n\
         sources:\n{sources}security:\n  identity: \"single-user\"\n  single_user_because: \"a test\"\n  \
         tls_termination: \"in-process\"\n{}",
        super::inbound::DIRECT_INBOUND,
    )
}

/// What `overlay` is refused with at its posture, less the build's own TLS answer.
fn bucket_refusals(overlay: String) -> Vec<NotFitToServe> {
    refused_with(overlay)
        .into_iter()
        .filter(|refusal| *refusal != NotFitToServe::InProcessTlsNotCompiledIn)
        .collect()
}

fn warehouse() -> sutura_domain::model::SourceName {
    sutura_domain::model::SourceName::parse("warehouse").expect("a legal identifier")
}

/// `#828`: only a lookup leg of a federated answer is read in hash buckets, so an opt-in on the only
/// source is refused before the deployment serves, naming the key.
#[test]
fn a_hash_bucket_opt_in_on_the_only_source_is_not_fit_to_serve() {
    let refusals = bucket_refusals(bucket_readers("warehouse", &bucketed_clickhouse("warehouse", BUCKET_REASON)));
    assert_eq!(
        refusals,
        vec![NotFitToServe::HashBucketsOnTheOnlySource { alias: warehouse() }]
    );
    let rendered = refusals.first().expect("one refusal").to_string();
    assert!(
        rendered.contains("sources.warehouse.hash_buckets_acknowledged_because"),
        "{rendered}"
    );
}

/// `#828`: a hash bucket is read on a reading node, so an opt-in no declared peer reads is refused
/// before the deployment serves - with no `server.nodes` at all, and with a peer reading another
/// source.
#[test]
fn a_hash_bucket_opt_in_that_no_node_reads_is_not_fit_to_serve() {
    let two = format!(
        "{}{}",
        bucketed_clickhouse("warehouse", BUCKET_REASON),
        bucketed_clickhouse("orders", "")
    );
    let refused = vec![NotFitToServe::HashBucketsReadByNoNode { alias: warehouse() }];
    assert_eq!(
        bucket_refusals(format!(
            "security:\n  identity: \"single-user\"\n  single_user_because: \"a test\"\nsources:\n{two}"
        )),
        refused
    );
    assert_eq!(bucket_refusals(bucket_readers("orders", &two)), refused);
    // The CONTROL: the same sources, with the peer reading `warehouse`, serve.
    assert_eq!(bucket_refusals(bucket_readers("warehouse", &two)), Vec::new());
}

/// A shared `oracle` source under `alias`, carrying `extra`.
fn oracle(alias: &str, extra: &str) -> String {
    format!(
        "  {alias}:\n    kind: \"oracle\"\n    host: \"db.example.com\"\n    port: 2484\n    \
         service_name: \"orcl.example.com\"\n    user: \"sutura\"\n    password_file: \"/run/secrets/oracle-password\"\n    \
         transport_mode: \"verified\"\n    transport_anchors: \"/run/secrets/oracle-ca.pem\"\n    \
         posture: \"shared-service-user\"\n{extra}"
    )
}

/// The flashback split opt-in, on the entry it follows.
const FLASHBACK: &str = "    split: \"flashback\"\n";

/// `#828`: only a lookup leg of a federated answer is read in flashback buckets, so an opt-in on
/// the only source is refused before the deployment serves, naming the key.
#[test]
fn a_flashback_split_on_the_only_source_is_not_fit_to_serve() {
    let refusals = bucket_refusals(bucket_readers("warehouse", &oracle("warehouse", FLASHBACK)));
    assert_eq!(refusals, vec![NotFitToServe::FlashbackOnTheOnlySource { alias: warehouse() }]);
    let rendered = refusals.first().expect("one refusal").to_string();
    assert!(rendered.contains("sources.warehouse.split"), "{rendered}");
}

/// `#828`: a flashback bucket is read on a reading node, so an opt-in no declared peer reads is
/// refused before the deployment serves - with no `server.nodes` at all, and with a peer reading
/// another source.
#[test]
fn a_flashback_split_that_no_node_reads_is_not_fit_to_serve() {
    let two = format!("{}{}", oracle("warehouse", FLASHBACK), oracle("orders", ""));
    let refused = vec![NotFitToServe::FlashbackReadByNoNode { alias: warehouse() }];
    assert_eq!(
        bucket_refusals(format!(
            "security:\n  identity: \"single-user\"\n  single_user_because: \"a test\"\nsources:\n{two}"
        )),
        refused
    );
    assert_eq!(bucket_refusals(bucket_readers("orders", &two)), refused);
    // The CONTROL: the same sources, with the peer reading `warehouse`, serve.
    assert_eq!(bucket_refusals(bucket_readers("warehouse", &two)), Vec::new());
}
