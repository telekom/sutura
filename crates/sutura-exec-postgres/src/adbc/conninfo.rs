//! The libpq connection string one declared source is dialled with - built here from the
//! declaration's parts, and never accepted from anyone.
//!
//! **No connection string is taken, so none can arrive carrying a weakening knob.** Refusing a
//! caller's `sslmode` would mean parsing conninfo with libpq's quoting rules; building it removes
//! the case. Every value is single-quoted with `\` and `'` escaped (`conninfo_parse`), so a declared
//! value cannot open a second key.
//!
//! **Every key below is written, because libpq fills an unwritten one from the process environment
//! (`PG*`) and then from a `PGSERVICE` file**, which sets only keys still unset (`fe-connect.c`,
//! libpq 18.6) - and `tokio-postgres` reads neither. Pinned, against that file's option table:
//!
//! | key | written | what leaving it unwritten allows |
//! | --- | --- | --- |
//! | `sslmode` | `disable`, or `verify-full` for TLS | `PGSSLMODE=require`: TLS that verifies nothing |
//! | `sslrootcert` | the declared bundle | `PGSSLROOTCERT`, or `~/.postgresql/root.crt` |
//! | `sslcert`, `sslkey` | the declared pair, on `mutual` | a certificate found on disk, presented |
//! | `sslcertmode` | `disable`, on `verified` | the same: `disable` presents nothing whatever is on disk |
//! | `ssl_min_protocol_version` | `TLSv1.2` | `PGSSLMINPROTOCOLVERSION=TLSv1` |
//! | `sslkeylogfile` | empty | a service file writing every session key to disk |
//! | `gssencmode` | `disable` | GSSAPI encryption in place of TLS, where the declared anchors verify nothing |
//! | `require_auth` | `password,md5,scram-sha-256,none` | Kerberos, SSPI or OAuth sign-in as an ambient identity, not the declared user |
//! | `hostaddr` | empty | `PGHOSTADDR` dialling another address under the declared name |
//! | `options` | empty | `PGOPTIONS` setting session parameters, `search_path` among them |
//! | `client_encoding` | `UTF8` | `PGCLIENTENCODING` re-encoding text; `tokio-postgres` sends `UTF8` too |
//!
//! Not pinned, because none widens the channel or changes who signs in: `sslcrl`/`sslcrldir`
//! (they only narrow), `sslsni`, `sslnegotiation`, `sslpassword`, `channel_binding`,
//! `connect_timeout`, `application_name`, the `keepalives*` family, `target_session_attrs` and
//! `load_balance_hosts`. Each still reaches libpq from the environment.
//!
//! **Refused here, by name, is a declaration libpq cannot hold to** - [`UnusableChannel`]. libpq
//! drops TLS on a unix socket whatever `sslmode` says, and reads its `system` store as OpenSSL's
//! compiled-in default, a build-host path in the static build rather than the host store
//! `rustls-native-certs` reads for the `tokio-postgres` path. Declared material is read once here
//! too, through the same [`client_config`](crate::tls::client_config) that path boots with, so an
//! unreadable or mismatched pair is refused before a driver loads rather than at the first connect.
//!
//! **The limits.** libpq re-reads the files at every connect, so what was checked here is not
//! what is presented later. libpq refuses a client key readable by group or others; this does not
//! check that, so such a key passes here and fails at connect. The string names keys libpq 16 to 18
//! added (`sslcertmode`, `require_auth`, `sslkeylogfile`), so a mounted driver over an older libpq
//! refuses it at connect. And no cell here observes a handshake: the strings below are what libpq is
//! told, read against its source, not what it did.

use sutura_domain::identity::Secret;
use sutura_domain::model::SourceName;

use crate::PostgresError;
use crate::connection::ConnectionTarget;
use crate::tls::{TlsAnchors, TlsIdentity};

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

/// A declared channel the ADBC transport cannot hold to, refused before anything dials.
#[derive(Debug, thiserror::Error)]
pub enum UnusableChannel {
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
    /// [`UnusableChannel`] for a TLS channel over a unix socket (a [`ConnectionTarget::UnixSocket`],
    /// or a host libpq reads as one: a leading `/` or `@`), for `system` anchors, and for declared
    /// material [`client_config`](crate::tls::client_config) refuses.
    pub fn new(
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
                return Ok(Self(with_password(text, password)));
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
        crate::tls::client_config(anchors, identity).map_err(|cause| UnusableChannel::Material {
            alias: source.clone(),
            cause,
        })?;
        pair(&mut text, "sslmode", "verify-full");
        pair(&mut text, "sslrootcert", &bundle.display().to_string());
        match identity {
            None => pair(&mut text, "sslcertmode", "disable"),
            Some(identity) => {
                pair(&mut text, "sslcert", &identity.certificate().display().to_string());
                pair(&mut text, "sslkey", &identity.key().display().to_string());
            }
        }
        Ok(Self(with_password(text, password)))
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

/// Appends the password and seals the string.
fn with_password(mut text: String, password: &Secret) -> Secret {
    #[expect(
        clippy::disallowed_methods,
        reason = "the libpq connection string is the password's destination, and building it is its purpose"
    )]
    pair(&mut text, "password", password.expose_secret());
    Secret::new(text)
}

#[cfg(test)]
mod tests {
    //! What libpq is told for each declared channel, and each refusal arriving instead of a string.

    use std::path::{Path, PathBuf};

    use sutura_domain::identity::Secret;
    use sutura_domain::model::SourceName;

    use super::{Channel, Conninfo, UnusableChannel};
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
                "{PINNED}sslmode='verify-full' sslrootcert='{c}' sslcert='{c}' sslkey='{}' password='p' ",
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
}
