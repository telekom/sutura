//! The ONE Oracle composition, shared by both of this crate's composition roots.
//!
//! `crate::clickhouse`'s shape, for its reason: `build` below is the only place a declared `oracle`
//! entry becomes an open `sutura_domain::warehouse::Warehouse`, so the posture cross-check, the
//! secret read and the refusal WORDING cannot differ between `sutura serve` and `sutura
//! query`/`mcp`. What stays per root is the feature-off refusal, because the two are different
//! sentences - one names every declared source, the other the one it was asked about.
//!
//! **The channel is the placement's `OracleChannel`**: verified TLS or explicitly acknowledged
//! native encryption, whose unauthenticated DH does not verify the server. For TLS this module
//! reads at boot and hands to the adapter as the ONLY anchors. A
//! listener's redirect is refused before authentication, so the connection stays on the declared
//! address - `crate::serve::oracle`'s `a_listener_redirect_to_an_address_nobody_declared_is_refused`
//! holds it.
//!
//! **An `impersonation-at-source` source opens each question's session with the token its
//! caller's own token is exchanged for**, through the `delegation` hops it declares:
//! `declared_delegation` hands them to `serve::broker`, and `build` connects the boot session under
//! the declared user as for any other posture.
//!
//! The WHOLE module is behind `#[cfg(feature = "oracle")]` at its declaration in `main.rs`, so
//! everything here may name an adapter type unconditionally.

use sutura_domain::model::SourceName;
use sutura_domain::warehouse::Warehouse as _;
use sutura_exec_oracle::{Channel, Dial, OracleWarehouse};

use crate::commands::render;

/// Builds one Oracle adapter from a declared entry, after checking this build can deliver the
/// source's posture.
///
/// **Everything that can fail before a question is asked is done here**: the posture cross-check,
/// the password file, and the connection itself - the driver dials and authenticates in
/// `OracleWarehouse::connect`, so an unreachable listener or a refused login stops the composition
/// rather than the first question.
///
/// # The limit, next to the claim
///
/// The TCP connect is bounded by `sutura_exec_oracle::DIAL_DEADLINE`; the handshake after it is
/// not, so a listener that accepts and then never answers holds the boot.
///
/// **A source that declares `split: flashback` is probed here too**, over every table `pinned`'s
/// models on it name: `OracleWarehouse::with_flashback` refuses a service user holding
/// `FLASHBACK ANY TABLE` and one that cannot read each of those tables at a snapshot. The probe
/// reads the models' own tables, not the tables their joins reach.
///
/// # Errors
///
/// A placement the dispatcher should have sent elsewhere; a source with no declared identity; a
/// posture this adapter cannot deliver; a password file that cannot be read or is empty; an anchor
/// bundle that cannot be read or holds no usable certificate; a connection the listener or the
/// database refused; or a declared flashback split whose probe failed.
pub(crate) fn build(
    source: &SourceName,
    configured: &sutura_config::ConfiguredSource,
    pinned: &sutura_domain::pinned::PinnedDefinitions,
    working_set: sutura_exec_datafusion::WorkingSet,
) -> Result<OracleWarehouse, String> {
    // Matched rather than read off accessors every kind would have to have: the dispatcher has
    // already decided this is the Oracle arm, and a second openable kind should arrive as a
    // compile error at this line too.
    let sutura_config::SourcePlacement::Oracle {
        ref host,
        port,
        ref service_name,
        ref user,
        ref password_file,
        channel: Some(ref channel),
        ..
    } = *configured.placement()
    else {
        return Err(format!(
            "`sources.{source}` reached the Oracle connect step with a placement no Oracle adapter \
             reads, which the dispatcher should have sent elsewhere"
        ));
    };
    let identity = configured
        .identity()
        .ok_or_else(|| format!("`sources.{source}` declares no identity a query could run under"))?;
    identity
        .posture()
        .deliverable_by(OracleWarehouse::IMPERSONATION, source)
        .map_err(|cause| render(&cause))?;
    let password = crate::password_file::read(source, password_file)?;
    #[expect(
        clippy::disallowed_methods,
        reason = "the password's destination is the connection handshake, which is the one place the \
                  value itself is the payload"
    )]
    let exposed = password.expose_secret();
    let anchors_pem = match channel.transport() {
        sutura_config::sources::placement::OracleTransport::Verified { anchors, .. } => {
            Some(read_anchors(&format!("sources.{source}.transport_anchors"), anchors)?)
        }
        sutura_config::sources::placement::OracleTransport::NativeEncryption { .. } => None,
    };
    let transport = anchors_pem
        .as_deref()
        .map_or(Channel::NativeEncryption, |anchors_pem| Channel::Verified { anchors_pem });
    let dial = Dial::new(host.as_str(), port, service_name.as_str(), transport);
    let dial = if channel.legacy_password_verifier().is_some() {
        dial.allow_legacy_verifiers()
    } else {
        dial
    };
    let opened = OracleWarehouse::connect(
        source.clone(),
        identity.posture().clone(),
        dial,
        user,
        exposed,
        working_set.result_budget(),
    )
    .map_err(|cause| format!("`sources.{source}` did not open: {}", render(&cause)))?;
    match configured.split() {
        None => Ok(opened),
        Some(sutura_config::sources::split::SourceSplit::Flashback) => {
            opened.with_flashback(&tables_on(pinned, source)).map_err(|cause| {
                format!(
                    "`sources.{source}` declares `split: flashback`, and its boot probe refused: {}",
                    render(&cause)
                )
            })
        }
    }
}

/// The tables `pinned`'s models on `source` sit behind.
fn tables_on(
    pinned: &sutura_domain::pinned::PinnedDefinitions,
    source: &SourceName,
) -> std::collections::BTreeSet<sutura_domain::model::QualifiedTable> {
    pinned
        .definitions()
        .models()
        .values()
        .filter(|model| model.source() == source)
        .map(|model| model.table().clone())
        .collect()
}

/// The PEM bundle at `anchors`, which `key` declared; the refusal names both.
pub(crate) fn read_anchors(key: &str, anchors: &std::path::Path) -> Result<String, String> {
    std::fs::read_to_string(anchors).map_err(|cause| format!("`{key}` ({}) could not be read: {cause}", anchors.display()))
}

/// The hops an `impersonation-at-source` source exchanges each caller's token through, in order;
/// empty for a source that is not impersonating.
pub(crate) fn declared_delegation(
    configured: &sutura_config::ConfiguredSource,
) -> &[sutura_config::sources::workload_identity::DelegationDeclared] {
    let sutura_config::SourcePlacement::Oracle { ref delegation, .. } = *configured.placement() else {
        return &[];
    };
    delegation
}

/// The dictionary's own encrypted login, never borrowed from a data source.
#[cfg(feature = "rdbms")]
pub(crate) fn dictionary_login(
    catalog: &SourceName,
    connection: &sutura_config::OracleCatalogConnection,
    password: sutura_domain::identity::Secret,
) -> Result<sutura_catalog_rdbms::oracle_reader::EncryptedOracleLogin, String> {
    let login = sutura_catalog_rdbms::oracle_reader::OracleLogin::new(
        String::from(connection.host().as_str()),
        connection.port(),
        String::from(connection.service_name().as_str()),
        String::from(connection.user()),
        password,
    );
    let login = match connection.channel().transport() {
        sutura_config::sources::placement::OracleTransport::Verified { anchors, .. } => login.verified(read_anchors(
            &format!("catalogs.{}.connection.transport_anchors", catalog),
            anchors,
        )?),
        sutura_config::sources::placement::OracleTransport::NativeEncryption { acknowledged_because } => {
            tracing::warn!(catalog = %catalog, channel = "native-encryption", reason = %acknowledged_because, "Oracle native encryption uses unauthenticated DH; server identity is not verified; catalog reads as one shared user");
            login.native_encryption()
        }
    };
    let login = if let Some(reason) = connection.channel().legacy_password_verifier() {
        tracing::warn!(catalog = %catalog, reason = %reason, "Oracle legacy password verifiers enabled on the declared encrypted channel");
        login.allow_legacy_verifiers()
    } else {
        login
    };
    Ok(login)
}
