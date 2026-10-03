//! What libpq is told for each declared channel, and each refusal arriving instead of a string.

use std::path::{Path, PathBuf};

use sutura_domain::identity::Secret;
use sutura_domain::model::SourceName;

use super::{
    Channel, Conninfo, GssEncryption, Held, InvalidKerberosService, InvalidOAuth, KEYWORDS, Kerberos, KerberosService, OAuth,
    Process, SignIn, UnusableChannel,
};
use crate::connection::ConnectionTarget;
use crate::tls::{TlsAnchors, TlsIdentity};

/// Every channel's shared prefix: the declared target, then the keys the environment may not fill.
const PINNED: &str = "host='db.example' port='5432' dbname='sales' user='reader' hostaddr='' \
                      options='' client_encoding='UTF8' require_auth='password,md5,scram-sha-256,none' \
                      gssencmode='disable' sslkeylogfile='' ssl_min_protocol_version='TLSv1.2' ";

fn source() -> SourceName {
    SourceName::parse("pg").expect("a test source is a source")
}

fn built(target: ConnectionTarget<'_>, password: &str, channel: Channel<'_>) -> Result<String, UnusableChannel> {
    let conninfo = Conninfo::new(&source(), target, 5432, "sales", "reader", &Secret::new(password), channel)?;
    #[expect(
        clippy::disallowed_methods,
        reason = "the cell asserts the exact string libpq is handed, password included"
    )]
    let text = conninfo.secret().expose_secret().to_owned();
    Ok(text)
}

/// The string a Kerberos sign-in builds, in an environment that names a credential or none.
fn kerberized(
    named: bool,
    target: ConnectionTarget<'_>,
    kerberos: &Kerberos,
    channel: Channel<'_>,
) -> Result<String, UnusableChannel> {
    let process = Process {
        openssl_configured: false,
        kerberos_cache_named: named,
    };
    let sign_in = SignIn::Kerberos(kerberos);
    let conninfo = Conninfo::under(process, &source(), target, 5432, "sales", "reader", sign_in, channel)?;
    #[expect(clippy::disallowed_methods, reason = "the cell asserts the exact string libpq is handed")]
    let text = conninfo.secret().expose_secret().to_owned();
    Ok(text)
}

/// The string an OAuth sign-in builds, and the bearer bytes it holds beside it.
type Authorised = (String, Option<Vec<u8>>);

fn oauth() -> OAuth {
    OAuth::new("https://issuer.example", "sutura").expect("a test declaration is one")
}

/// The string and bearer an OAuth sign-in builds.
fn authorised(bearer: &str, channel: Channel<'_>) -> Result<Authorised, UnusableChannel> {
    let process = Process {
        openssl_configured: false,
        kerberos_cache_named: false,
    };
    let oauth = oauth();
    let bearer = Secret::new(bearer);
    let sign_in = SignIn::OAuth(&oauth, &bearer);
    let conninfo = Conninfo::under(process, &source(), HOST, 5432, "sales", "reader", sign_in, channel)?;
    #[expect(clippy::disallowed_methods, reason = "the cell asserts the exact string libpq is handed")]
    let text = conninfo.secret().expose_secret().to_owned();
    Ok((text, conninfo.bearer().map(|bearer| bearer.to_bytes().to_vec())))
}

fn kerberos(encryption: GssEncryption) -> Kerberos {
    let service = KerberosService::parse("postgres").expect("a test service is a service");
    Kerberos::new(service, encryption)
}

/// A self-signed certificate and its key, written where the cell can name them.
fn material(case: &str) -> (PathBuf, PathBuf) {
    let generated = rcgen::generate_simple_self_signed(vec![String::from("db.example")]).expect("rcgen signs");
    let dir = std::env::temp_dir().join(format!("sutura-conninfo-{case}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a temp dir is creatable");
    let (certificate, key) = (dir.join("ca.pem"), dir.join("client.key"));
    std::fs::write(&certificate, generated.cert.pem()).expect("the certificate is writable");
    std::fs::write(&key, generated.signing_key.serialize_pem()).expect("the key is writable");
    (certificate, key)
}

const HOST: ConnectionTarget<'static> = ConnectionTarget::Host("db.example");

#[test]
fn a_verified_channel_is_verify_full_against_the_declared_bundle_and_presents_nothing() {
    let (bundle, _) = material("verified");
    let anchors = TlsAnchors::Bundle(bundle.clone());
    let text = built(HOST, "p", Channel::Verified(&anchors)).expect("a readable bundle builds");
    assert_eq!(
        text,
        format!(
            "{PINNED}sslmode='verify-full' sslrootcert='{}' sslcertmode='disable' password='p' ",
            bundle.display()
        )
    );
}

#[test]
fn a_mutual_channel_presents_exactly_the_declared_pair() {
    let (certificate, key) = material("mutual");
    let anchors = TlsAnchors::Bundle(certificate.clone());
    let identity = TlsIdentity::new(certificate.clone(), key.clone());
    let text = built(HOST, "p", Channel::Mutual(&anchors, &identity)).expect("a matching pair builds");
    assert_eq!(
        text,
        format!(
            "{PINNED}sslmode='verify-full' sslrootcert='{c}' sslcertmode='allow' sslcert='{c}' sslkey='{}' password='p' ",
            key.display(),
            c = certificate.display()
        )
    );
}

#[test]
fn a_plaintext_channel_disables_tls_rather_than_leaving_libpq_to_prefer_it() {
    let text = built(HOST, "p", Channel::Plaintext).expect("plaintext builds");
    assert_eq!(text, format!("{PINNED}sslmode='disable' password='p' "));
}

#[test]
fn a_quote_in_a_declared_value_cannot_open_a_second_key() {
    let text = built(HOST, r"x' sslmode='disable \", Channel::Plaintext).expect("plaintext builds");
    assert!(text.ends_with(r"password='x\' sslmode=\'disable \\' "), "{text}");
}

#[test]
fn tls_over_a_unix_socket_is_refused_by_name() {
    let (bundle, _) = material("socket");
    let anchors = TlsAnchors::Bundle(bundle);
    let refused = built(
        ConnectionTarget::UnixSocket(Path::new("/run/postgresql")),
        "p",
        Channel::Verified(&anchors),
    )
    .expect_err("libpq drops TLS on a socket");
    assert!(
        matches!(refused, UnusableChannel::TlsOverASocket { ref alias, ref target } if alias.as_str() == "pg" && target == "/run/postgresql"),
        "{refused:?}"
    );
}

#[test]
fn tls_to_a_host_libpq_reads_as_an_abstract_socket_is_refused() {
    let (bundle, _) = material("abstract");
    let anchors = TlsAnchors::Bundle(bundle);
    let refused = built(ConnectionTarget::Host("@pg"), "p", Channel::Verified(&anchors)).expect_err("`@` is a socket");
    assert!(matches!(refused, UnusableChannel::TlsOverASocket { .. }), "{refused:?}");
}

#[test]
fn a_declared_system_store_is_refused_by_name() {
    let refused = built(HOST, "p", Channel::Verified(&TlsAnchors::System)).expect_err("libpq's system is not sutura's");
    assert!(
        matches!(refused, UnusableChannel::HostStore { ref alias } if alias.as_str() == "pg"),
        "{refused:?}"
    );
}

#[test]
fn an_unreadable_bundle_is_refused_before_any_driver_loads() {
    let anchors = TlsAnchors::Bundle(PathBuf::from("/nonexistent/sutura-ca.pem"));
    let refused = built(HOST, "p", Channel::Verified(&anchors)).expect_err("no bundle lives there");
    assert!(
        matches!(
            refused,
            UnusableChannel::Material {
                cause: crate::PostgresError::AnchorsRead { .. },
                ..
            }
        ),
        "{refused:?}"
    );
}

#[test]
fn a_target_that_is_not_one_host_is_refused_on_every_channel() {
    let (bundle, _) = material("targets");
    let anchors = TlsAnchors::Bundle(bundle);
    for (target, channel) in [
        (ConnectionTarget::Host("db.example,"), Channel::Verified(&anchors)),
        (ConnectionTarget::Host("db.example,@pg"), Channel::Verified(&anchors)),
        (ConnectionTarget::Host(""), Channel::Verified(&anchors)),
        (ConnectionTarget::Host("127.0.0.1,"), Channel::Plaintext),
        (
            ConnectionTarget::UnixSocket(Path::new("/run/pg,db.example")),
            Channel::Plaintext,
        ),
        (ConnectionTarget::UnixSocket(Path::new("")), Channel::Plaintext),
    ] {
        let refused = built(target, "p", channel).expect_err("one host, or none");
        assert!(
            matches!(refused, UnusableChannel::NotOneTarget { ref alias, .. } if alias.as_str() == "pg"),
            "{refused:?}"
        );
    }
}

#[test]
fn an_empty_password_is_refused_rather_than_looked_up() {
    let (bundle, _) = material("password");
    let anchors = TlsAnchors::Bundle(bundle);
    for channel in [Channel::Plaintext, Channel::Verified(&anchors)] {
        let refused = built(HOST, "", channel).expect_err("an empty password is not sent");
        assert!(
            matches!(refused, UnusableChannel::EmptyPassword { ref alias } if alias.as_str() == "pg"),
            "{refused:?}"
        );
    }
}

#[test]
fn tls_is_refused_while_openssl_conf_is_set_and_plaintext_is_not() {
    let (bundle, _) = material("openssl-conf");
    let anchors = TlsAnchors::Bundle(bundle);
    let password = Secret::new("p");
    let process = Process {
        openssl_configured: true,
        kerberos_cache_named: false,
    };
    let under = |channel| {
        let sign_in = SignIn::Password(&password);
        Conninfo::under(process, &source(), HOST, 5432, "sales", "reader", sign_in, channel)
    };
    let refused = under(Channel::Verified(&anchors)).expect_err("its OpenSSL would read the file");
    assert!(matches!(refused, UnusableChannel::OpensslConfig { .. }), "{refused:?}");
    under(Channel::Plaintext).expect("plaintext never starts OpenSSL");
}

#[test]
fn an_unreadable_client_pair_is_refused_before_any_driver_loads() {
    let (certificate, _) = material("pair");
    let anchors = TlsAnchors::Bundle(certificate.clone());
    let identity = TlsIdentity::new(certificate, PathBuf::from("/nonexistent/sutura-client.key"));
    let refused = built(HOST, "p", Channel::Mutual(&anchors, &identity)).expect_err("no key lives there");
    assert!(
        matches!(
            refused,
            UnusableChannel::Material {
                cause: crate::PostgresError::IdentityRead { .. },
                ..
            }
        ),
        "{refused:?}"
    );
}

/// Every Kerberos string's shared prefix, the password one's with GSSAPI the only method.
const KERBEROS: &str = "host='db.example' port='5432' dbname='sales' user='reader' hostaddr='' \
                        options='' client_encoding='UTF8' require_auth='gss' ";

#[test]
fn a_kerberos_sign_in_requires_gssapi_encryption_names_the_service_and_sends_no_password() {
    let text = kerberized(true, HOST, &kerberos(GssEncryption::Required), Channel::Plaintext).expect("it builds");
    assert_eq!(
        text,
        format!(
            "{KERBEROS}gssencmode='require' sslkeylogfile='' ssl_min_protocol_version='TLSv1.2' \
             sslmode='disable' krbsrvname='postgres' gssdelegation='0' "
        )
    );
}

#[test]
fn a_kerberos_sign_in_inside_tls_keeps_gssapi_encryption_off_and_delegates_nothing() {
    let (bundle, _) = material("kerberos-tls");
    let anchors = TlsAnchors::Bundle(bundle.clone());
    let service = KerberosService::parse("POSTGRES").expect("a test service is a service");
    let inside = Kerberos::new(service, GssEncryption::Off);
    let text = kerberized(true, HOST, &inside, Channel::Verified(&anchors)).expect("it builds");
    assert_eq!(
        text,
        format!(
            "{KERBEROS}gssencmode='disable' sslkeylogfile='' ssl_min_protocol_version='TLSv1.2' \
             sslmode='verify-full' sslrootcert='{}' sslcertmode='disable' krbsrvname='POSTGRES' gssdelegation='0' ",
            bundle.display()
        )
    );
}

#[test]
fn kerberos_with_no_cache_named_is_refused_rather_than_signing_in_as_the_default_cache() {
    let refused = kerberized(false, HOST, &kerberos(GssEncryption::Required), Channel::Plaintext).expect_err("no cache is named");
    assert!(
        matches!(refused, UnusableChannel::NoKerberosCache { ref alias } if alias.as_str() == "pg"),
        "{refused:?}"
    );
}

#[test]
fn only_a_named_cache_names_the_kerberos_credential_and_a_keytab_alone_does_not() {
    let lookup = |set: &'static [(&'static str, &'static str)]| {
        move |name: &str| {
            set.iter()
                .find(|&&(key, _)| key == name)
                .map(|&(_, value)| std::ffi::OsString::from(value))
        }
    };
    let named = |set| Process::read_with(lookup(set)).kerberos_cache_named;
    assert!(!named(&[]), "nothing set");
    assert!(!named(&[("KRB5_CLIENT_KTNAME", "/run/keytab")]), "a keytab pins no principal");
    assert!(!named(&[("KRB5CCNAME", "")]), "an empty name is unset");
    assert!(named(&[("KRB5CCNAME", "MEMORY:sutura")]), "a named cache");
    assert!(named(&[("KRB5CCNAME", "MEMORY:s"), ("KRB5_CLIENT_KTNAME", "/run/keytab")]));
    let openssl = |set| Process::read_with(lookup(set)).openssl_configured;
    assert!(!openssl(&[]));
    assert!(openssl(&[("OPENSSL_CONF", "")]), "libpq's OpenSSL reads even an empty one");
}

#[test]
fn gssapi_encryption_beside_tls_is_refused_by_name() {
    let (bundle, _) = material("kerberos-both");
    let anchors = TlsAnchors::Bundle(bundle);
    let refused = kerberized(true, HOST, &kerberos(GssEncryption::Required), Channel::Verified(&anchors))
        .expect_err("libpq would take GSSAPI and skip the anchors");
    assert!(
        matches!(refused, UnusableChannel::GssEncryptionBesideTls { .. }),
        "{refused:?}"
    );
}

#[test]
fn kerberos_to_a_unix_socket_is_refused_by_name() {
    for target in [
        ConnectionTarget::UnixSocket(Path::new("/run/postgresql")),
        ConnectionTarget::Host("/tmp"),
    ] {
        let refused = kerberized(true, target, &kerberos(GssEncryption::Off), Channel::Plaintext)
            .expect_err("a server offers no GSSAPI on a socket");
        assert!(
            matches!(refused, UnusableChannel::KerberosOverASocket { ref alias, .. } if alias.as_str() == "pg"),
            "{refused:?}"
        );
    }
}

#[test]
fn a_service_name_cannot_carry_a_realm_a_second_component_or_nothing() {
    assert_eq!(KerberosService::parse(""), Err(InvalidKerberosService::Empty));
    for (name, found) in [
        ("postgres@EXAMPLE.TEST", '@'),
        ("postgres/db.example", '/'),
        ("post gres", ' '),
        ("postgres'", '\''),
    ] {
        assert_eq!(
            KerberosService::parse(name),
            Err(InvalidKerberosService::Character { found }),
            "{name:?}"
        );
    }
}

#[test]
fn an_oauth_sign_in_pins_the_issuer_and_client_and_keeps_the_bearer_out_of_the_string() {
    let (bundle, _) = material("oauth");
    let anchors = TlsAnchors::Bundle(bundle.clone());
    let (text, bearer) = authorised("a-bearer", Channel::Verified(&anchors)).expect("it builds");
    assert_eq!(
        text,
        format!(
            "host='db.example' port='5432' dbname='sales' user='reader' hostaddr='' options='' \
             client_encoding='UTF8' require_auth='oauth' gssencmode='disable' sslkeylogfile='' \
             ssl_min_protocol_version='TLSv1.2' sslmode='verify-full' sslrootcert='{}' sslcertmode='disable' \
             oauth_issuer='https://issuer.example' oauth_client_id='sutura' ",
            bundle.display()
        )
    );
    assert_eq!(bearer.as_deref(), Some(&b"a-bearer"[..]));
}

#[test]
fn a_password_or_kerberos_sign_in_holds_no_bearer() {
    let password = Conninfo::new(
        &source(),
        HOST,
        5432,
        "sales",
        "reader",
        &Secret::new("p"),
        Channel::Plaintext,
    );
    assert!(password.expect("it builds").bearer().is_none());
    let process = Process {
        openssl_configured: false,
        kerberos_cache_named: true,
    };
    let kerberos = kerberos(GssEncryption::Required);
    let sign_in = SignIn::Kerberos(&kerberos);
    let kerberized = Conninfo::under(process, &source(), HOST, 5432, "sales", "reader", sign_in, Channel::Plaintext);
    assert!(kerberized.expect("it builds").bearer().is_none());
}

#[test]
fn an_oauth_bearer_over_plaintext_is_refused_by_name() {
    let refused = authorised("a-bearer", Channel::Plaintext).expect_err("a bearer rides TLS or nothing");
    assert!(
        matches!(refused, UnusableChannel::BearerOverPlaintext { ref alias } if alias.as_str() == "pg"),
        "{refused:?}"
    );
}

#[test]
fn an_empty_bearer_or_one_holding_a_nul_is_refused_by_name() {
    let (bundle, _) = material("oauth-bearer");
    let anchors = TlsAnchors::Bundle(bundle);
    for bearer in ["", "a\0b"] {
        let refused = authorised(bearer, Channel::Verified(&anchors)).expect_err("libpq cannot carry it");
        assert!(matches!(refused, UnusableChannel::UnusableBearer { .. }), "{refused:?}");
    }
}

#[test]
fn an_oauth_declaration_needs_an_https_issuer_and_a_client() {
    assert_eq!(OAuth::new("http://issuer.example", "c"), Err(InvalidOAuth::IssuerNotHttps));
    assert_eq!(OAuth::new("https://", "c"), Err(InvalidOAuth::IssuerNotHttps));
    assert_eq!(OAuth::new("https://issuer.example", ""), Err(InvalidOAuth::EmptyClientId));
    assert_eq!(
        OAuth::new("https://issuer.example", "c\n"),
        Err(InvalidOAuth::ControlCharacter)
    );
}

#[test]
fn every_libpq_keyword_is_classified_once_and_exactly_the_written_ones_are_written() {
    let (certificate, key) = material("keywords");
    let anchors = TlsAnchors::Bundle(certificate.clone());
    let identity = TlsIdentity::new(certificate, key);
    let strings = [
        built(HOST, "p", Channel::Plaintext),
        built(HOST, "p", Channel::Verified(&anchors)),
        built(HOST, "p", Channel::Mutual(&anchors, &identity)),
        kerberized(true, HOST, &kerberos(GssEncryption::Required), Channel::Plaintext),
        kerberized(
            true,
            HOST,
            &kerberos(GssEncryption::Off),
            Channel::Mutual(&anchors, &identity),
        ),
        authorised("b", Channel::Verified(&anchors)).map(|(text, _)| text),
    ]
    .map(|text| format!(" {}", text.expect("each channel builds")));
    let mut seen = std::collections::HashSet::new();
    for (keyword, held) in KEYWORDS {
        assert!(seen.insert(keyword), "{keyword} is classified twice");
        let written = strings.iter().any(|text| text.contains(&format!(" {keyword}='")));
        let (Held::Written(reason) | Held::Harmless(reason)) = held;
        assert!(!reason.is_empty(), "{keyword} is classified for no stated reason");
        assert_eq!(written, matches!(held, Held::Written(_)), "{keyword} is {held:?}");
    }
}
