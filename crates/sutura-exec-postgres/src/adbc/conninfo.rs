//! The libpq connection string one declared source is dialled with - built here from the
//! declaration's parts, and never accepted from anyone.
//!
//! **No connection string is taken, so none can arrive carrying a weakening knob.** Refusing a
//! caller's `sslmode` would mean parsing conninfo with libpq's quoting rules; building it removes
//! the case. Every value is single-quoted with `\` and `'` escaped (`conninfo_parse`), so a declared
//! value cannot open a second key.
//!
//! **Every one of libpq 18.6's 50 keywords is classified in `KEYWORDS`**: written, or left to
//! libpq for a stated reason. libpq fills an unwritten key from the
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
//! A [`Kerberos`] sign-in is refused to a socket, where a server offers no GSSAPI; while
//! `KRB5CCNAME` names no credential cache, because MIT krb5 then signs in as whatever a default
//! cache holds and reads a client keytab only where that cache does not exist; and with GSSAPI
//! encryption beside TLS, which libpq tries first and which verifies none of the declared anchors. An [`OAuth`] sign-in is refused over
//! plaintext, where its bearer would be readable on the path, and with a bearer libpq cannot carry
//! (empty, or holding a NUL). Declared material is read once here too, through the same
//! [`client_config`](crate::tls::client_config) that path boots with, so an unreadable bundle or
//! client pair is refused before a driver loads rather than at the first connect.
//!
//! **The limits.** A Kerberos sign-in is the PROCESS's: libpq takes no keytab or cache per
//! connection, so every Kerberos source signs in as the one principal the named cache holds, and the
//! server maps it to the declared `user`. The string pins what the ticket is for (`krbsrvname`) and
//! that the credential is never forwarded (`gssdelegation`), not who holds it. Which server
//! principal GSSAPI authenticates is the `<krbsrvname>/<host>` that `krb5.conf` makes of the declared
//! host (`qualify_shortname`, `dns_canonicalize_hostname`), not anything the declared anchors hold. An OAuth bearer is
//! never in the string: libpq has no keyword for one, so `Conninfo::bearer` holds it for the
//! linked libpq's hook, and only the linked driver can be handed it. Its issuer and client are
//! pinned; when the token expires, and who it was minted for, are the declaration's, not libpq's. libpq re-reads the files at every connect, so what was checked here is not
//! what is presented later. libpq refuses a client key readable by group or others; this does not
//! check that, so such a key passes here and fails at connect. The string names keys libpq 16 to 18
//! added (`sslcertmode`, `require_auth`, `sslkeylogfile`), so a mounted driver over an older libpq
//! refuses it at connect. `require_auth` admits `none`, which a `mutual` source signing in by
//! certificate needs, so a server that asks for no password is accepted; and it admits cleartext
//! `password` - both as `tokio-postgres` does. OpenSSL still reads its compiled-in default
//! configuration file when `OPENSSL_CONF` is unset; in the nix build that file is in the store, and
//! what the static musl artefact's copy holds is unmeasured. `PGTZ`, `PGDATESTYLE` and `PGGEQO` have
//! no keyword, so the string cannot pin them: each reaches the server as a session setting. The
//! environment (`OPENSSL_CONF`, `KRB5CCNAME`) is read through a lookup a cell supplies; that the
//! constructors hand it the process's own `std::env::var_os` is the one line no cell drives. And no cell here observes a handshake: the strings below are what
//! libpq is told, read against its source, not what it did.

use core::ffi::CStr;
use std::ffi::OsString;

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
    (
        "passfile",
        Held::Harmless(
            "read only where no password is written: a password sign-in refuses an empty one, and \
             `require_auth='gss'` sends nothing a password file holds",
        ),
    ),
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
        Held::Written("the declared method; otherwise SSPI or OAuth could sign in as an ambient identity"),
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
        Held::Written("`require` only where declared; otherwise GSSAPI could replace the declared TLS"),
    ),
    (
        "krbsrvname",
        Held::Written("on Kerberos; `PGKRBSRVNAME` would aim the ticket at another service"),
    ),
    (
        "gsslib",
        Held::Harmless("chooses SSPI or GSSAPI on Windows, which no release targets"),
    ),
    (
        "gssdelegation",
        Held::Written("`0` on Kerberos; `PGGSSDELEGATION=1` would hand the server this process's credential"),
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
        Held::Written("on OAuth; a service file's would pick which server's issuer the bearer is for"),
    ),
    (
        "oauth_client_id",
        Held::Written("on OAuth; libpq refuses OAuth without it, and a service file's would be someone else's"),
    ),
    (
        "oauth_client_secret",
        Held::Harmless("read only by libpq's device flow, which the linked libpq is built without"),
    ),
    (
        "oauth_scope",
        Held::Harmless("only handed to the hook, which answers with the held bearer whatever it names"),
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

/// How the declared user signs in.
#[derive(Debug, Clone, Copy)]
enum SignIn<'a> {
    Password(&'a Secret),
    Kerberos(&'a Kerberos),
    OAuth(&'a OAuth, &'a Secret),
}

/// A Kerberos sign-in through GSSAPI, as the declaration names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Kerberos {
    service: KerberosService,
    encryption: GssEncryption,
}

impl Kerberos {
    /// The sign-in a declaration names. The credential is never delegated to the server.
    #[must_use]
    pub const fn new(service: KerberosService, encryption: GssEncryption) -> Self {
        Self { service, encryption }
    }
}

/// The service half of the server's principal, `<service>/<host>` - libpq's `krbsrvname`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KerberosService(String);

/// A declared Kerberos service name that is not one.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvalidKerberosService {
    #[error("a Kerberos service name is empty")]
    Empty,
    #[error("a Kerberos service name holds {found:?}; it is `A-Z`, `a-z`, `0-9`, `-`, `_` and `.` alone")]
    Character { found: char },
}

impl KerberosService {
    /// Parses a service name: one or more ASCII letters, digits, `-`, `_` or `.`, so a declared one
    /// cannot name a realm (`@`) or a second component (`/`).
    ///
    /// # Errors
    ///
    /// [`InvalidKerberosService`] for an empty name or any other character.
    pub fn parse(name: &str) -> Result<Self, InvalidKerberosService> {
        if name.is_empty() {
            return Err(InvalidKerberosService::Empty);
        }
        let usable = |character: char| character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.');
        if let Some(found) = name.chars().find(|&character| !usable(character)) {
            return Err(InvalidKerberosService::Character { found });
        }
        Ok(Self(String::from(name)))
    }
}

/// Whether GSSAPI encrypts the channel - libpq's `gssencmode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GssEncryption {
    /// Required, over [`Channel::Plaintext`]: GSSAPI is the channel, TLS is off.
    Required,
    /// Off: GSSAPI signs in, and the channel is the declared one.
    Off,
}

/// An OAuth sign-in, as the declaration names the issuer and the client the bearer is for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuth {
    issuer: String,
    client_id: String,
}

/// A declared OAuth issuer or client that libpq could not hold to.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvalidOAuth {
    #[error("an OAuth issuer is an `https://` URL")]
    IssuerNotHttps,
    #[error("an OAuth client id is empty")]
    EmptyClientId,
    #[error("an OAuth issuer or client id holds a control character")]
    ControlCharacter,
}

impl OAuth {
    /// The issuer libpq holds the server's discovery document to, and the client id it requires.
    ///
    /// # Errors
    ///
    /// [`InvalidOAuth`] for an issuer that is not `https://`, an empty client id, or a control
    /// character in either.
    pub fn new(issuer: &str, client_id: &str) -> Result<Self, InvalidOAuth> {
        if issuer.strip_prefix("https://").is_none_or(str::is_empty) {
            return Err(InvalidOAuth::IssuerNotHttps);
        }
        if client_id.is_empty() {
            return Err(InvalidOAuth::EmptyClientId);
        }
        if issuer.chars().chain(client_id.chars()).any(char::is_control) {
            return Err(InvalidOAuth::ControlCharacter);
        }
        Ok(Self {
            issuer: String::from(issuer),
            client_id: String::from(client_id),
        })
    }
}

/// What [`Conninfo`] reads from the process environment, handed in so a cell can set it.
#[derive(Debug, Clone, Copy)]
struct Process {
    openssl_configured: bool,
    kerberos_cache_named: bool,
}

impl Process {
    /// The process's own environment - the one line no cell drives, because setting a variable is
    /// `unsafe`; [`Process::read_with`] is what the cells hold.
    fn read() -> Self {
        Self::read_with(|name| std::env::var_os(name))
    }

    /// The environment `var` answers for. Only `KRB5CCNAME` names the Kerberos credential: with no
    /// cache named, MIT krb5 signs in as whatever the default cache holds and reads the client keytab
    /// only when that cache does not exist, so `KRB5_CLIENT_KTNAME` alone pins nobody.
    fn read_with(var: impl Fn(&str) -> Option<OsString>) -> Self {
        Self {
            openssl_configured: var("OPENSSL_CONF").is_some(),
            kerberos_cache_named: var("KRB5CCNAME").is_some_and(|value| !value.is_empty()),
        }
    }
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
    #[error(
        "`sources.{alias}` declares Kerberos to `{target}`, which libpq dials as a unix socket, where \
         a server offers no GSSAPI. Declare a TCP host for Kerberos"
    )]
    KerberosOverASocket { alias: SourceName, target: String },
    #[error(
        "`sources.{alias}` declares Kerberos and `KRB5CCNAME` names no credential cache, \
         so libpq would sign in as whichever principal a default cache holds, a keytab notwithstanding. \
         Name the cache - a keytab in `KRB5_CLIENT_KTNAME` fills it"
    )]
    NoKerberosCache { alias: SourceName },
    #[error(
        "`sources.{alias}` declares GSSAPI encryption beside TLS; libpq would take GSSAPI first and \
         verify none of the declared anchors. Declare one of the two"
    )]
    GssEncryptionBesideTls { alias: SourceName },
    #[error(
        "`sources.{alias}` declares an OAuth bearer over plaintext, where anyone on the path could \
         read it and sign in with it. Declare TLS for OAuth"
    )]
    BearerOverPlaintext { alias: SourceName },
    #[error("`sources.{alias}` declares an OAuth bearer that is empty or holds a NUL, which libpq cannot carry")]
    UnusableBearer { alias: SourceName },
    #[error("`sources.{alias}` declares TLS material that cannot be used")]
    Material {
        alias: SourceName,
        #[source]
        cause: PostgresError,
    },
}

/// The connection string for one source, and the OAuth bearer its dials sign in with, if any.
///
/// Only [`Conninfo::new`], [`Conninfo::kerberos`] and [`Conninfo::oauth`] make one, and its `Debug`
/// is the [`Secret`]s', so neither the password nor the bearer is ever printed.
#[derive(Debug)]
pub struct Conninfo {
    text: Secret,
    /// NUL-terminated, so libpq reads it in place: no copy outlives the zeroizing [`Secret`].
    bearer: Option<Secret>,
}

impl Conninfo {
    /// Builds the connection string for `source` over `channel`, signing in with `password`.
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
        let sign_in = SignIn::Password(password);
        Self::under(Process::read(), source, target, port, database, user, sign_in, channel)
    }

    /// Builds the connection string for `source` over `channel`, signing in with Kerberos as the
    /// principal the credential cache `KRB5CCNAME` names holds - filled from `KRB5_CLIENT_KTNAME`'s
    /// keytab where one is named.
    ///
    /// # Errors
    ///
    /// [`Conninfo::new`]'s, less the password's, and [`UnusableChannel`] for a target libpq reads as
    /// a unix socket, for an environment naming no Kerberos credential, and for GSSAPI encryption
    /// declared beside TLS.
    pub fn kerberos(
        source: &SourceName,
        target: ConnectionTarget<'_>,
        port: u16,
        database: &str,
        user: &str,
        kerberos: &Kerberos,
        channel: Channel<'_>,
    ) -> Result<Self, UnusableChannel> {
        let sign_in = SignIn::Kerberos(kerberos);
        Self::under(Process::read(), source, target, port, database, user, sign_in, channel)
    }

    /// Builds the connection string for `source` over `channel`, signing in with OAuth: libpq asks
    /// the server for its issuer, refuses one other than `oauth`'s, and is handed `bearer` by the
    /// linked libpq's hook (`sutura_adbc::with_postgres_bearer`) - no keyword carries it.
    ///
    /// # Errors
    ///
    /// [`Conninfo::new`]'s, less the password's, and [`UnusableChannel`] for a plaintext channel
    /// and for a bearer that is empty or holds a NUL.
    pub fn oauth(
        source: &SourceName,
        target: ConnectionTarget<'_>,
        port: u16,
        database: &str,
        user: &str,
        oauth: &OAuth,
        bearer: &Secret,
        channel: Channel<'_>,
    ) -> Result<Self, UnusableChannel> {
        let sign_in = SignIn::OAuth(oauth, bearer);
        Self::under(Process::read(), source, target, port, database, user, sign_in, channel)
    }

    fn under(
        process: Process,
        source: &SourceName,
        target: ConnectionTarget<'_>,
        port: u16,
        database: &str,
        user: &str,
        sign_in: SignIn<'_>,
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
        let (require_auth, gssencmode) = match sign_in {
            SignIn::Password(_) => ("password,md5,scram-sha-256,none", "disable"),
            SignIn::Kerberos(kerberos) => {
                if socket {
                    return Err(UnusableChannel::KerberosOverASocket {
                        alias: source.clone(),
                        target: host,
                    });
                }
                if !process.kerberos_cache_named {
                    return Err(UnusableChannel::NoKerberosCache { alias: source.clone() });
                }
                match (kerberos.encryption, channel) {
                    (GssEncryption::Off, _) => ("gss", "disable"),
                    (GssEncryption::Required, Channel::Plaintext) => ("gss", "require"),
                    (GssEncryption::Required, Channel::Verified(_) | Channel::Mutual(..)) => {
                        return Err(UnusableChannel::GssEncryptionBesideTls { alias: source.clone() });
                    }
                }
            }
            SignIn::OAuth(..) => {
                if matches!(channel, Channel::Plaintext) {
                    return Err(UnusableChannel::BearerOverPlaintext { alias: source.clone() });
                }
                ("oauth", "disable")
            }
        };
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
            ("require_auth", require_auth),
            ("gssencmode", gssencmode),
            ("sslkeylogfile", ""),
            ("ssl_min_protocol_version", "TLSv1.2"),
        ] {
            pair(&mut text, key, value);
        }
        let (anchors, identity) = match channel {
            Channel::Plaintext => {
                pair(&mut text, "sslmode", "disable");
                return signed_in(text, sign_in, source);
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
        if process.openssl_configured {
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
        signed_in(text, sign_in, source)
    }

    /// The string, for the one caller that hands it to the driver.
    pub(crate) const fn secret(&self) -> &Secret {
        &self.text
    }

    /// The bearer, as the C string libpq's hook hands over, where this string signs in with OAuth.
    pub(crate) fn bearer(&self) -> Option<&CStr> {
        #[expect(
            clippy::disallowed_methods,
            reason = "the bearer's destination is libpq's hook, and handing it over is its purpose"
        )]
        let bearer = self.bearer.as_ref()?.expose_secret();
        CStr::from_bytes_with_nul(bearer.as_bytes()).ok()
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

/// Appends what the sign-in writes and seals the string: the password; the Kerberos service and
/// delegation; or the OAuth issuer and client, with the bearer held beside the string.
fn signed_in(mut text: String, sign_in: SignIn<'_>, source: &SourceName) -> Result<Conninfo, UnusableChannel> {
    let bearer = match sign_in {
        SignIn::Password(password) => {
            text = with_password(text, password, source)?;
            None
        }
        SignIn::Kerberos(kerberos) => {
            pair(&mut text, "krbsrvname", &kerberos.service.0);
            pair(&mut text, "gssdelegation", "0");
            None
        }
        SignIn::OAuth(oauth, bearer) => {
            pair(&mut text, "oauth_issuer", &oauth.issuer);
            pair(&mut text, "oauth_client_id", &oauth.client_id);
            #[expect(
                clippy::disallowed_methods,
                reason = "the bearer is re-sealed NUL-terminated for libpq's hook, its one destination"
            )]
            let bearer = bearer.expose_secret();
            if bearer.is_empty() || bearer.contains('\0') {
                return Err(UnusableChannel::UnusableBearer { alias: source.clone() });
            }
            Some(Secret::new(format!("{bearer}\0")))
        }
    };
    Ok(Conninfo {
        text: Secret::new(text),
        bearer,
    })
}

/// Appends the password, refusing an empty one: libpq reads `password=''` as unset and looks it up
/// in `PGPASSFILE` or `~/.pgpass`.
fn with_password(mut text: String, password: &Secret, source: &SourceName) -> Result<String, UnusableChannel> {
    #[expect(
        clippy::disallowed_methods,
        reason = "the libpq connection string is the password's destination, and building it is its purpose"
    )]
    let password = password.expose_secret();
    if password.is_empty() {
        return Err(UnusableChannel::EmptyPassword { alias: source.clone() });
    }
    pair(&mut text, "password", password);
    Ok(text)
}

#[cfg(test)]
mod tests;
