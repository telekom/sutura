//! The ONE `ClickHouse` composition, shared by both of this crate's composition roots.
//!
//! `sutura serve` and `sutura query`/`mcp` each dispatch the same `sutura_config::SourceKind`
//! vocabulary through their own exhaustive match, and for `bigquery` and `postgres` each root also
//! carries its own copy of the per-source BUILD - which
//! `crates/sutura-cli/src/sources/postgres.rs` calls out by name as what issue 121 asks for and
//! does not have. This kind arrives with it: `build` below is the only place a declared `clickhouse`
//! entry becomes an open `sutura_domain::warehouse::Warehouse`, so the posture cross-check, the
//! secret read, the channel resolution and the refusal WORDING cannot differ between the two roots.
//!
//! What stays per root is the feature-off refusal, because the two are genuinely different
//! sentences: `serve`'s names every declared source at once, and the command's names the one it was
//! asked about.
//!
//! # The generic is the seam, and it is kept
//!
//! `ClickHouseSource` pins the ADBC warehouse **here, at the composition root**. The fixture and
//! conformance targets retain the HTTP-backed generic warehouse while their table loader is being
//! migrated; a served source always uses the ADBC path.
//!
//! # Per-subject execution
//!
//! An `impersonation-at-source` source keeps ONE transport under the declared service user; each
//! statement switches with `EXECUTE AS` to the user named by the caller's verified subject.
//! `deployment_user` hands `serve::broker` the service user, the one name no caller is run as.
//!
//! The WHOLE module is behind `#[cfg(feature = "clickhouse")]` at its declaration in `main.rs`,
//! which is what lets everything here name an adapter type unconditionally - `serve::broker` and
//! `serve::agent` take the same shape for the same reason.

use sutura_domain::model::SourceName;
use sutura_domain::warehouse::Warehouse as _;
use sutura_exec_clickhouse::adbc::{AdbcClickHouseWarehouse, TlsOptions};

use crate::commands::render;

/// A `ClickHouse` source as this binary composes it: the adapter, over its ADBC driver.
///
/// Named once for the reason `crate::serve::BigQuerySource` is: it appears in a registry type, a
/// `Warehouse` bound and a constructor's return, and the two layers ARE the composition.
pub(crate) type ClickHouseSource = AdbcClickHouseWarehouse;

type OpenChannel = (String, Option<TlsOptions>);

/// Builds one `ClickHouse` adapter from a declared entry, after checking this build can deliver the
/// source's posture.
///
/// **Everything that can fail before a question is asked is done here**: the posture cross-check,
/// the password file, the TLS material and the endpoint's own construction - so a deployment whose
/// channel or credential is misdeclared refuses at composition rather than on the first question.
/// Nothing is attached and nothing is registered: the tables live in the database.
///
/// # The limit, next to the claim
///
/// The TLS material is read ONCE and the `ureq::Agent` fixes its configuration at construction, so
/// a replaced anchor bundle is adopted on the next process start and not on the next request -
/// unlike the `bigquery` wire and the `postgres` channel, which both take a rotating handle. That is
/// the transport's shape rather than a choice made here (`sutura_exec_clickhouse::transport::Http`'s
/// own documentation states it), and it is why this root starts no rotation poll for this kind.
///
/// # Errors
///
/// A placement the dispatcher should have sent elsewhere; a source with no declared identity; a
/// password file that cannot be read or is empty; and declared TLS material that is not usable.
pub(crate) fn build(
    source: &SourceName,
    configured: &sutura_config::ConfiguredSource,
    working_set: sutura_exec_datafusion::WorkingSet,
) -> Result<ClickHouseSource, String> {
    // Matched rather than read off accessors every kind would have to have, for the reason
    // `crate::serve::bigquery::build_bigquery` gives: the dispatcher has already decided this is the
    // ClickHouse arm, and a second openable kind should arrive as a compile error at this line too.
    let sutura_config::SourcePlacement::ClickHouse {
        ref host,
        port,
        ref user,
        ref password_file,
        ref transport,
        ..
    } = *configured.placement()
    else {
        return Err(format!(
            "`sources.{source}` reached the ClickHouse connect step with a placement no ClickHouse \
             adapter reads, which the dispatcher should have sent elsewhere"
        ));
    };
    let identity = configured
        .identity()
        .ok_or_else(|| format!("`sources.{source}` declares no identity a query could run under"))?;
    // Against this adapter's OWN constant, which is the point of the cross-check being per adapter.
    identity
        .posture()
        .deliverable_by(ClickHouseSource::IMPERSONATION, source)
        .map_err(|cause| render(&cause))?;
    let password = crate::password_file::read(source, password_file)?;
    let driver = sutura_exec_clickhouse::adbc::driver_location().map_err(|cause| cause.to_string())?;
    let (endpoint, tls) = open_channel(source, host, port, transport)?;
    Ok(AdbcClickHouseWarehouse::new(
        source.clone(),
        identity.posture().clone(),
        driver,
        endpoint,
        user.clone(),
        password,
        tls,
        working_set.result_budget(),
    ))
}

/// The hops a `clickhouse` source exchanges each caller's token through before that caller signs
/// in; empty for a source that switches with `EXECUTE AS`, and for any other kind.
pub(crate) fn declared_delegation(
    configured: &sutura_config::ConfiguredSource,
) -> &[sutura_config::sources::workload_identity::DelegationDeclared] {
    let sutura_config::SourcePlacement::ClickHouse { ref delegation, .. } = *configured.placement() else {
        return &[];
    };
    delegation
}

/// The broker's half of an `impersonation-at-source` source: the service user its transport signs in
/// as, which `DeclaredPrincipalBroker::switching` never runs a caller as. `None` for any other source.
pub(crate) fn deployment_user(
    source: &SourceName,
    configured: &sutura_config::ConfiguredSource,
) -> Result<Option<sutura_domain::identity::PrincipalName>, String> {
    let sutura_config::SourcePlacement::ClickHouse { ref user, .. } = *configured.placement() else {
        return Ok(None);
    };
    if !matches!(
        configured.posture(),
        Some(sutura_domain::source::SourcePosture::ImpersonationAtSource)
    ) {
        return Ok(None);
    }
    sutura_domain::identity::PrincipalName::parse(user)
        .map(Some)
        .map_err(|cause| format!("`sources.{source}.user` is not a principal: {cause}"))
}

/// The transport for the channel this source declared: plaintext, or TLS over the declared store.
///
/// **The scheme follows the declaration and is never guessed.** `Endpoint::plaintext` for
/// `transport_mode: plaintext` and `Endpoint::tls` for the two TLS modes, so a source declared
/// `verified` cannot end up dialled over `http://` - which is the failure a single endpoint
/// constructor plus an optional TLS config would admit.
fn open_channel(
    source: &SourceName,
    host: &sutura_config::sources::placement::HostName,
    port: u16,
    declared: &sutura_config::sources::transport::SourceTransport,
) -> Result<OpenChannel, String> {
    use sutura_config::sources::transport::{SourceTransport, TrustAnchors};
    use sutura_exec_clickhouse::tls::{TlsAnchors, TlsIdentity};

    let (anchors, client) = match *declared {
        SourceTransport::Plaintext => {
            return Ok((format!("http://{}:{port}", host.as_str()), None));
        }
        SourceTransport::Verified { ref anchors } => (anchors, None),
        SourceTransport::Mutual {
            ref anchors,
            ref identity,
        } => (
            anchors,
            Some(TlsIdentity::new(identity.certificate().clone(), identity.key().clone())),
        ),
    };
    let anchors = match *anchors {
        TrustAnchors::System => TlsAnchors::System,
        TrustAnchors::File(ref path) => TlsAnchors::Bundle(path.clone()),
    };
    let (root, chain, key) = sutura_exec_clickhouse::tls::adbc_options(&anchors, client.as_ref()).map_err(|cause| {
        format!(
            "`sources.{source}` is declared TLS and its material is not usable: {}",
            render(&cause)
        )
    })?;
    Ok((
        format!("https://{}:{port}", host.as_str()),
        Some(TlsOptions::new(root, chain, key)),
    ))
}

#[cfg(test)]
mod tests {
    use sutura_config::sources::placement::HostName;
    use sutura_config::sources::transport::{SourceTransport, TrustAnchors};
    use sutura_domain::model::SourceName;

    /// **The scheme follows the declaration**, observed on the transport this root built: a source
    /// declared `verified` is dialled over `https`, and only `plaintext` over `http`. The anchor
    /// bundle is a freshly issued certificate, so the TLS config really builds rather than the
    /// cell stopping at an unreadable file.
    #[test]
    fn a_source_declared_verified_is_dialled_over_https_and_only_plaintext_over_http() {
        let directory = std::env::temp_dir().join(format!("sutura-cli-ch-scheme-{}", std::process::id()));
        std::fs::create_dir_all(&directory).expect("a scratch directory is creatable");
        let bundle = directory.join("anchors.pem");
        let issued = rcgen::generate_simple_self_signed([String::from("localhost")]).expect("a self-signed pair generates");
        std::fs::write(&bundle, issued.cert.pem()).expect("a certificate writes");

        let source = SourceName::parse("warehouse").expect("a test source is a source");
        let host = HostName::parse("ch.example.com").expect("a test host is a host");
        let verified = SourceTransport::Verified {
            anchors: TrustAnchors::File(bundle),
        };
        let secured = super::open_channel(&source, &host, 8443, &verified).expect("the declared bundle is usable");
        let plain = super::open_channel(&source, &host, 8123, &SourceTransport::Plaintext)
            .expect("a plaintext transport needs no material");
        let _ignored = std::fs::remove_dir_all(&directory);

        assert!(
            secured.0.starts_with("https://"),
            "a verified source must dial https: {:?}",
            secured.0
        );
        assert!(plain.0.starts_with("http://"), "a plaintext source dials http: {:?}", plain.0);
    }
}
