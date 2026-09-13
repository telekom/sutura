//! The `Postgres` half of this composition root: one declared source becomes an open `Warehouse`.
//!
//! **Split out of `main.rs` when this adapter's composition pushed that file past the unexemptable
//! 1000-line cap**, beside `bigquery`'s half and for the same reason. Both `open_postgres`
//! definitions are here - the one that opens the connection and the refusal for a build that linked
//! no adapter - because the compiler picks between them at the one call site.
//!
//! **Opening is where a misdeclared channel fails, and it fails before the listener binds**: the
//! posture cross-check, the password file, the TLS material and the connection itself are all read
//! here, so a deployment cannot come up believing a channel is secured when it is not.

/// Opens one `PostgreSQL` connection per declared source, secured as the source declares.
///
/// Attaches nothing (the tables live in the database), which is the whole difference from the files
/// arm. What opening DOES is everything that can fail before a question is asked against a real
/// server: the posture cross-check, the TLS resolution (which reads the declared trust anchors and
/// an optional client identity and refuses what a closed type must refuse), and the connection
/// itself - so a deployment whose channel is misdeclared fails to START rather than answering every
/// question over a channel it believes is secured.
///
/// **The composition is `sutura-serve`'s single Postgres composition, and it is thin on purpose:**
/// the TLS reading and the connection live in `sutura-exec-postgres` (`tls::client_config` and
/// `connect_secured`), so a fix to either lands once and both composition roots get it. What this
/// function owns is the mapping from the declared `SourcePlacement` to that adapter's resolved
/// material - the same boundary `build_bigquery` draws.
#[cfg(feature = "postgres")]
pub(crate) fn open_postgres(
    declared: &[&sutura_domain::model::SourceName],
    registry: &sutura_config::SourceRegistry,
) -> Result<super::OpenedSources, String> {
    let mut engines: Option<sutura_app::Warehouses<super::PostgresSource>> = None;
    for source in declared {
        let configured = super::configured_source(source, registry)?;
        let engine = build_postgres(source, configured)?;
        engines = Some(match engines {
            None => sutura_app::Warehouses::of(engine),
            Some(open) => open.and(engine).map_err(super::flatten)?,
        });
    }
    engines
        .map(super::OpenedSources::Postgres)
        .ok_or_else(|| String::from("this catalog declares no models, so there is nothing to open"))
}

/// The refusal for a build that did not link the `Postgres` adapter.
///
/// **Two definitions of one signature rather than a `cfg` inside one body**, for the reason
/// `open_bigquery` gives at the same shape: the dispatcher has exactly one call and the compiler
/// decides which of these it reaches, keeping both signatures identical under `dead_code = "deny"`.
#[cfg(not(feature = "postgres"))]
pub(crate) fn open_postgres(
    declared: &[&sutura_domain::model::SourceName],
    _registry: &sutura_config::SourceRegistry,
) -> Result<super::OpenedSources, String> {
    let named = declared
        .iter()
        .map(|source| source.as_str())
        .collect::<Vec<&str>>()
        .join(", ");
    Err(format!(
        "[{named}] declares `kind: postgres`, and this binary was built without the `postgres` \
         feature - so it links no Postgres adapter. Build `sutura-serve` with `--features postgres`, \
         or declare a `files` source"
    ))
}

/// Builds one `Postgres` adapter, after checking this build can deliver the source's posture and
/// that the declared channel is one this binary can actually open.
///
/// **Every value the connection needs is declared on the entry**: the host or unix socket, the
/// port, the role, the database and the password file are the entry's. The password file is read
/// HERE - the same justification `build_bigquery` gives for its credential file - so a password
/// that is missing, unreadable or empty stops the process rather than producing a deployment that
/// answers every question with an authentication failure while claiming to be open.
///
/// **The non-loopback fail-closed is configuration's, and this function inherits it:** config
/// refuses a non-loopback `host` declared `plaintext` at load (issue 124). What opening adds is the
/// TLS resolution for a `verified`/`mutual` source - reading the anchor bundle and optional client
/// identity through `sutura_exec_postgres::tls::client_config`, which reads an explicitly selected
/// `system` store and refuses one it cannot read completely, anchors that parse to nothing, and an
/// incomplete identity.
#[cfg(feature = "postgres")]
fn build_postgres(
    source: &sutura_domain::model::SourceName,
    configured: &sutura_config::ConfiguredSource,
) -> Result<super::PostgresSource, String> {
    use sutura_exec_postgres::PostgresWarehouse;
    use sutura_exec_postgres::connection::{ConnectionTarget, config};
    use sutura_exec_postgres::tls::{TlsAnchors, TlsIdentity, client_config};

    let identity = configured
        .identity()
        .ok_or_else(|| format!("`sources.{source}` declares no identity a query could run under"))?;
    identity
        .posture()
        .deliverable_by(
            <super::PostgresSource as sutura_domain::warehouse::Warehouse>::IMPERSONATION,
            source,
        )
        .map_err(super::flatten)?;
    // Matched rather than read off accessors every kind would have to have, for the reason
    // `build_bigquery` gives: `one_kind` has already decided this is the Postgres arm, and a second
    // openable kind should arrive as a compile error at this line too.
    let sutura_config::SourcePlacement::Postgres {
        ref dial,
        ref database,
        ref user,
        ref password_file,
        ref transport,
    } = *configured.placement()
    else {
        return Err(format!(
            "`sources.{source}` reached the Postgres connect step with a placement no Postgres \
             adapter reads, which `one_kind` should have dispatched elsewhere"
        ));
    };

    // Exactly one of these, by construction - `sutura_config::sources::placement::PostgresDial`
    // is the reason there is no third arm here for a state `parse_placement` already refused.
    let (target, port) = match dial {
        sutura_config::sources::placement::PostgresDial::Tcp { host, port } => (ConnectionTarget::Host(host.as_str()), *port),
        sutura_config::sources::placement::PostgresDial::UnixSocket { directory, port } => {
            (ConnectionTarget::UnixSocket(directory), *port)
        }
    };
    let config = config(target, port, database, user, password_file)
        .map_err(|cause| format!("`sources.{source}.password_file` could not be read: {cause}"))?;
    let tls = match transport {
        sutura_config::sources::transport::SourceTransport::Plaintext => None,
        sutura_config::sources::transport::SourceTransport::Verified { anchors } => {
            let anchors = match anchors {
                sutura_config::sources::transport::TrustAnchors::System => TlsAnchors::System,
                sutura_config::sources::transport::TrustAnchors::File(path) => TlsAnchors::Bundle(path.clone()),
            };
            Some(client_config(&anchors, None).map_err(|cause| {
                format!(
                    "`sources.{source}` is declared TLS and its material is not usable: {}",
                    super::flatten(&cause)
                )
            })?)
        }
        sutura_config::sources::transport::SourceTransport::Mutual { anchors, identity } => {
            let anchors = match anchors {
                sutura_config::sources::transport::TrustAnchors::System => TlsAnchors::System,
                sutura_config::sources::transport::TrustAnchors::File(path) => TlsAnchors::Bundle(path.clone()),
            };
            let identity = TlsIdentity::new(identity.certificate().clone(), identity.key().clone());
            Some(client_config(&anchors, Some(&identity)).map_err(|cause| {
                format!(
                    "`sources.{source}` is declared mTLS and its material is not usable: {}",
                    super::flatten(&cause)
                )
            })?)
        }
    };
    PostgresWarehouse::connect_secured(source.clone(), identity.posture().clone(), &config, tls).map_err(super::flatten)
}
