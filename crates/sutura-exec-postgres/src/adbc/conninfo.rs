//! The libpq connection string one declared source is dialled with - built here from the
//! declaration's parts, and never accepted from anyone.
//!
//! **No connection string is taken, so none can arrive carrying a weakening knob.** Refusing a
//! caller's `sslmode` would mean parsing conninfo with libpq's quoting rules; building it removes
//! the case. Every value is single-quoted with `\` and `'` escaped (`conninfo_parse`), so a declared
//! value cannot open a second key.
//!
//! **Every one of libpq 18.6's 50 keywords is classified in `KEYWORDS`**: written, read only in
//! a case refused here, or left to libpq for a stated reason. libpq fills an unwritten key from the
//! process environment (`PG*`) and then from a `PGSERVICE` file, which sets only keys still unset
//! (`parseServiceFile`, `fe-connect.c:6126`) - and `tokio-postgres` reads neither. The
//! `adbc-driver-postgresql` check compares that table's keywords with the `PQconninfoOptions` of the
//! libpq the drivers are built against, so a libpq bump that adds one fails `just validate` until it
//! is classified.
//!
//! **Refused here, by name, is a declaration libpq cannot hold to** - [`UnusableChannel`]. libpq
//! reads `host` as a list, so a target holding a list separator, or nothing, is refused. libpq
//! drops TLS on a unix socket whatever `sslmode` says, and reads its `system` store as OpenSSL's
//! compiled-in default, a build-host path in the static build rather than the host store
//! `rustls-native-certs` reads for the `tokio-postgres` path. An empty password is refused, because libpq reads one as unset and looks it
//! up in a password file. TLS is refused while `OPENSSL_CONF` is set, because libpq has no cipher
//! knob and that file can lower the protocol ceiling and the cipher list below the declared channel.
//! Declared material is read once here too, through the same
//! [`client_config`](crate::tls::client_config) that path boots with, so an unreadable bundle or
//! client pair is refused before a driver loads rather than at the first connect.
//!
//! **The limits.** libpq re-reads the files at every connect, so what was checked here is not
//! what is presented later. libpq refuses a client key readable by group or others; this does not
//! check that, so such a key passes here and fails at connect. The string names keys libpq 16 to 18
//! added (`sslcertmode`, `require_auth`, `sslkeylogfile`), so a mounted driver over an older libpq
//! refuses it at connect. `require_auth` admits `none`, which a `mutual` source signing in by
//! certificate needs, so a server that asks for no password is accepted; and it admits cleartext
//! `password` - both as `tokio-postgres` does. OpenSSL still reads its compiled-in default
//! configuration file when `OPENSSL_CONF` is unset; in the nix build that file is in the store, and
//! what the static musl artefact's copy holds is unmeasured. `PGTZ`, `PGDATESTYLE` and `PGGEQO` have
//! no keyword, so the string cannot pin them: each reaches the server as a session setting. The
//! `OPENSSL_CONF` cell hands the variable's state in; that [`Conninfo::new`] reads the process's own
//! is not held by a cell here. And no cell here observes a handshake: the strings below are what
//! libpq is told, read against its source, not what it did.

use sutura_domain::identity::Secret;
use sutura_domain::model::SourceName;

use crate::PostgresError;
use crate::connection::ConnectionTarget;
use crate::tls::{TlsAnchors, TlsIdentity};

/// How one libpq keyword is kept from widening the channel or changing who signs in.
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Held {
    /// Written on every string that libpq reads it for, and what leaving it unwritten would allow.
    Written(&'static str),
    /// Read only in the case named, which [`Conninfo::new`] refuses.
    Refused(&'static str),
    /// Left to libpq, for the reason given.
    Harmless(&'static str),
}

/// libpq 18.6's `PQconninfoOptions`, in its order, each classified. A cell holds the `Written` rows
/// to the strings built; the `adbc-driver-postgresql` check holds the keywords to libpq's own.
#[cfg(test)]
const KEYWORDS: [(&str, Held); 50] = [
    (
        "service",
        Held::Harmless("a service file fills only unset keys, each harmless by its own row"),
    ),
    ("user", Held::Written("`PGUSER` would sign in as another role")),
    ("password", Held::Written("`PGPASSWORD` would replace the declared one")),
    ("passfile", Held::Refused("read only for an empty password")),
    (
        "channel_binding",
        Held::Harmless("verify-full already authenticates the server; plaintext has no channel to bind"),
    ),
    ("connect_timeout", Held::Harmless("how long a dial waits, not where it goes")),
    ("dbname", Held::Written("`PGDATABASE` would open another database")),
    ("host", Held::Written("`PGHOST` would dial another host")),
    (
        "hostaddr",
        Held::Written("empty; `PGHOSTADDR` would dial another address under the declared name"),
    ),
    ("port", Held::Written("`PGPORT` would dial another port")),
    (
        "client_encoding",
        Held::Written("`PGCLIENTENCODING` would re-encode text; `tokio-postgres` sends `UTF8` too"),
    ),
    (
        "options",
        Held::Written("empty; `PGOPTIONS` would set session parameters, `search_path` among them"),
    ),
    ("application_name", Held::Harmless("a label in the server's activity view")),
    (
        "fallback_application_name",
        Held::Harmless("a label in the server's activity view"),
    ),
    ("keepalives", Held::Harmless("TCP liveness tuning")),
    ("keepalives_idle", Held::Harmless("TCP liveness tuning")),
    ("keepalives_interval", Held::Harmless("TCP liveness tuning")),
    ("keepalives_count", Held::Harmless("TCP liveness tuning")),
    ("tcp_user_timeout", Held::Harmless("TCP liveness tuning")),
    (
        "sslmode",
        Held::Written("`PGSSLMODE=require` would be TLS that verifies nothing"),
    ),
    (
        "sslnegotiation",
        Held::Harmless("changes how TLS starts, not what it verifies"),
    ),
    (
        "sslcompression",
        Held::Harmless("libpq reads it, but the static OpenSSL has no zlib and servers since 14 refuse it"),
    ),
    (
        "sslcert",
        Held::Written("on mutual; otherwise a certificate found on disk could be presented"),
    ),
    (
        "sslkey",
        Held::Written("on mutual; otherwise a key found on disk could be presented"),
    ),
    (
        "sslcertmode",
        Held::Written("`disable` on verified presents nothing, whatever is on disk"),
    ),
    ("sslpassword", Held::Harmless("only decrypts the declared key")),
    (
        "sslrootcert",
        Held::Written("`PGSSLROOTCERT` or `~/.postgresql/root.crt` would be trusted instead"),
    ),
    ("sslcrl", Held::Harmless("only narrows what verifies")),
    ("sslcrldir", Held::Harmless("only narrows what verifies")),
    ("sslsni", Held::Harmless("verify-full checks the name with or without SNI")),
    ("requirepeer", Held::Harmless("only narrows, and only on a socket")),
    (
        "require_auth",
        Held::Written("Kerberos, SSPI or OAuth would sign in as an ambient identity"),
    ),
    (
        "min_protocol_version",
        Held::Harmless("the wire protocol version, not the channel's"),
    ),
    (
        "max_protocol_version",
        Held::Harmless("the wire protocol version, not the channel's"),
    ),
    (
        "ssl_min_protocol_version",
        Held::Written("`PGSSLMINPROTOCOLVERSION=TLSv1` would lower the floor"),
    ),
    (
        "ssl_max_protocol_version",
        Held::Harmless("a ceiling; below the written floor libpq refuses the range"),
    ),
    (
        "gssencmode",
        Held::Written("GSSAPI encryption would replace TLS, verified by none of the declared anchors"),
    ),
    (
        "krbsrvname",
        Held::Harmless("read only for GSSAPI, which gssencmode and require_auth rule out"),
    ),
    (
        "gsslib",
        Held::Harmless("read only for GSSAPI, which gssencmode and require_auth rule out"),
    ),
    (
        "gssdelegation",
        Held::Harmless("read only for GSSAPI, which gssencmode and require_auth rule out"),
    ),
    (
        "replication",
        Held::Harmless("a session mode for the declared user, not a channel or an identity"),
    ),
    (
        "target_session_attrs",
        Held::Harmless("with one host it can only refuse a server"),
    ),
    ("load_balance_hosts", Held::Harmless("one host, so nothing to balance")),
    (
        "scram_client_key",
        Held::Harmless("the declared user still signs in, and a key not theirs fails"),
    ),
    (
        "scram_server_key",
        Held::Harmless("the declared user still signs in, and a key not theirs fails"),
    ),
    (
        "oauth_issuer",
        Held::Harmless("read only for OAuth, which require_auth rules out"),
    ),
    (
        "oauth_client_id",
        Held::Harmless("read only for OAuth, which require_auth rules out"),
    ),
    (
        "oauth_client_secret",
        Held::Harmless("read only for OAuth, which require_auth rules out"),
    ),
    (
        "oauth_scope",
        Held::Harmless("read only for OAuth, which require_auth rules out"),
    ),
    (
        "sslkeylogfile",
        Held::Written("empty; a service file could write every session key to disk"),
    ),
];

/// How the channel to the source is secured, as the composition root resolved the declaration.
#[derive(Debug, Clone, Copy)]
pub enum Channel<'a> {
    /// No transport security.
    Plaintext,
    /// TLS, verified against `anchors`, presenting nothing.
    Verified(&'a TlsAnchors),
    /// TLS, verified against the anchors, presenting the identity.
    Mutual(&'a TlsAnchors, &'a TlsIdentity),
}

/// A declared connection the ADBC transport cannot hold to, refused before anything dials.
#[derive(Debug, thiserror::Error)]
pub enum UnusableChannel {
    #[error(
        "`sources.{alias}` is reached at `{target}`, which libpq does not read as exactly one host or \
         socket directory. Declare one"
    )]
    NotOneTarget { alias: SourceName, target: String },
    #[error(
        "`sources.{alias}` declares TLS to `{target}`, which libpq dials as a unix socket - and \
         libpq never negotiates TLS on one, whatever `sslmode` says. Declare a TCP host for TLS"
    )]
    TlsOverASocket { alias: SourceName, target: String },
    #[error(
        "`sources.{alias}` declares `transport_anchors: system`, and libpq's `system` store is \
         OpenSSL's compiled-in default rather than the host store sutura reads, so the ADBC \
         transport cannot verify against what was declared. Name a PEM bundle"
    )]
    HostStore { alias: SourceName },
    #[error(
        "`sources.{alias}` declares TLS and `OPENSSL_CONF` is set; the ADBC transport's OpenSSL \
         would read it and it can weaken the channel below what was declared. Unset it"
    )]
    OpensslConfig { alias: SourceName },
    #[error("`sources.{alias}` declares an empty password, which libpq would look up elsewhere instead")]
    EmptyPassword { alias: SourceName },
    #[error("`sources.{alias}` declares TLS material that cannot be used")]
    Material {
        alias: SourceName,
        #[source]
        cause: PostgresError,
    },
}

/// The connection string for one source. Only [`Conninfo::new`] makes one, and its `Debug` is the
/// [`Secret`]'s, so the password it carries is never printed.
#[derive(Debug)]
pub struct Conninfo(Secret);

impl Conninfo {
    /// Builds the connection string for `source` over `channel`.
    ///
    /// # Errors
    ///
    /// [`UnusableChannel`] for a target that is not exactly one host or directory, for an empty
    /// password, for a TLS channel over a unix socket (a [`ConnectionTarget::UnixSocket`], or a host
    /// libpq reads as one: a leading `/` or `@`), for `system` anchors, for TLS while `OPENSSL_CONF`
    /// is set, and for declared material [`client_config`](crate::tls::client_config) refuses.
    pub fn new(
        source: &SourceName,
        target: ConnectionTarget<'_>,
        port: u16,
        database: &str,
        user: &str,
        password: &Secret,
        channel: Channel<'_>,
    ) -> Result<Self, UnusableChannel> {
        let openssl_configured = std::env::var_os("OPENSSL_CONF").is_some();
        Self::under(openssl_configured, source, target, port, database, user, password, channel)
    }

    fn under(
        openssl_configured: bool,
        source: &SourceName,
        target: ConnectionTarget<'_>,
        port: u16,
        database: &str,
        user: &str,
        password: &Secret,
        channel: Channel<'_>,
    ) -> Result<Self, UnusableChannel> {
        let host = match target {
            ConnectionTarget::Host(host) => String::from(host),
            ConnectionTarget::UnixSocket(directory) => directory.display().to_string(),
        };
        if host.is_empty() || host.contains(',') {
            return Err(UnusableChannel::NotOneTarget {
                alias: source.clone(),
                target: host,
            });
        }
        let socket = matches!(target, ConnectionTarget::UnixSocket(_)) || host.starts_with(['/', '@']);
        let port = port.to_string();
        let mut text = String::new();
        for (key, value) in [
            ("host", host.as_str()),
            ("port", port.as_str()),
            ("dbname", database),
            ("user", user),
            ("hostaddr", ""),
            ("options", ""),
            ("client_encoding", "UTF8"),
            ("require_auth", "password,md5,scram-sha-256,none"),
            ("gssencmode", "disable"),
            ("sslkeylogfile", ""),
            ("ssl_min_protocol_version", "TLSv1.2"),
        ] {
            pair(&mut text, key, value);
        }
        let (anchors, identity) = match channel {
            Channel::Plaintext => {
                pair(&mut text, "sslmode", "disable");
                return with_password(text, password, source).map(Self);
            }
            Channel::Verified(anchors) => (anchors, None),
            Channel::Mutual(anchors, identity) => (anchors, Some(identity)),
        };
        if socket {
            return Err(UnusableChannel::TlsOverASocket {
                alias: source.clone(),
                target: host,
            });
        }
        let TlsAnchors::Bundle(ref bundle) = *anchors else {
            return Err(UnusableChannel::HostStore { alias: source.clone() });
        };
        if openssl_configured {
            return Err(UnusableChannel::OpensslConfig { alias: source.clone() });
        }
        crate::tls::client_config(anchors, identity).map_err(|cause| UnusableChannel::Material {
            alias: source.clone(),
            cause,
        })?;
        pair(&mut text, "sslmode", "verify-full");
        pair(&mut text, "sslrootcert", &bundle.display().to_string());
        match identity {
            None => pair(&mut text, "sslcertmode", "disable"),
            Some(identity) => {
                pair(&mut text, "sslcertmode", "allow");
                pair(&mut text, "sslcert", &identity.certificate().display().to_string());
                pair(&mut text, "sslkey", &identity.key().display().to_string());
            }
        }
        with_password(text, password, source).map(Self)
    }

    /// The string, for the one caller that hands it to the driver.
    pub(crate) const fn secret(&self) -> &Secret {
        &self.0
    }
}

/// Appends `key='value' `, with `\` and `'` escaped as libpq's `conninfo_parse` reads them.
fn pair(text: &mut String, key: &str, value: &str) {
    text.push_str(key);
    text.push_str("='");
    for character in value.chars() {
        if matches!(character, '\\' | '\'') {
            text.push('\\');
        }
        text.push(character);
    }
    text.push_str("' ");
}

/// Appends the password and seals the string, refusing an empty one: libpq reads `password=''` as
/// unset and looks it up in `PGPASSFILE` or `~/.pgpass`.
fn with_password(mut text: String, password: &Secret, source: &SourceName) -> Result<Secret, UnusableChannel> {
    #[expect(
        clippy::disallowed_methods,
        reason = "the libpq connection string is the password's destination, and building it is its purpose"
    )]
    let password = password.expose_secret();
    if password.is_empty() {
        return Err(UnusableChannel::EmptyPassword { alias: source.clone() });
    }
    pair(&mut text, "password", password);
    Ok(Secret::new(text))
}

#[cfg(test)]
mod tests {
    //! What libpq is told for each declared channel, and each refusal arriving instead of a string.

    use std::path::{Path, PathBuf};

    use sutura_domain::identity::Secret;
    use sutura_domain::model::SourceName;

    use super::{Channel, Conninfo, Held, KEYWORDS, UnusableChannel};
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
        let under = |channel| Conninfo::under(true, &source(), HOST, 5432, "sales", "reader", &Secret::new("p"), channel);
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

    #[test]
    fn every_libpq_keyword_is_classified_once_and_exactly_the_written_ones_are_written() {
        let (certificate, key) = material("keywords");
        let anchors = TlsAnchors::Bundle(certificate.clone());
        let identity = TlsIdentity::new(certificate, key);
        let strings = [
            built(HOST, "p", Channel::Plaintext),
            built(HOST, "p", Channel::Verified(&anchors)),
            built(HOST, "p", Channel::Mutual(&anchors, &identity)),
        ]
        .map(|text| format!(" {}", text.expect("each channel builds")));
        let mut seen = std::collections::HashSet::new();
        for (keyword, held) in KEYWORDS {
            assert!(seen.insert(keyword), "{keyword} is classified twice");
            let written = strings.iter().any(|text| text.contains(&format!(" {keyword}='")));
            let (Held::Written(reason) | Held::Refused(reason) | Held::Harmless(reason)) = held;
            assert!(!reason.is_empty(), "{keyword} is classified for no stated reason");
            assert_eq!(written, matches!(held, Held::Written(_)), "{keyword} is {held:?}");
        }
    }
}
