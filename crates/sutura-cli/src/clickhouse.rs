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
//! `ClickHouseSource` pins `T = transport::Http` **here, at the composition root** - the adapter
//! itself stays generic over `sutura_exec_clickhouse::transport::ClickHouseTransport`, the same
//! way `crate::serve::BigQuerySource` pins a wire while `BigQueryWarehouse` stays generic. That
//! generic is what a per-subject `ClickHouse` identity would arrive through: a transport constructed
//! per request rather than one holding a configured identity. Collapsing the adapter to a concrete
//! type to simplify this file would close that door, so it is not done.
//!
//! The WHOLE module is behind `#[cfg(feature = "clickhouse")]` at its declaration in `main.rs`,
//! which is what lets everything here name an adapter type unconditionally - `serve::broker` and
//! `serve::agent` take the same shape for the same reason.

use sutura_domain::model::SourceName;
use sutura_domain::warehouse::Warehouse as _;
use sutura_exec_clickhouse::ClickHouseWarehouse;
use sutura_exec_clickhouse::transport::{BasicAuth, Endpoint, Http};

use crate::commands::render;

/// A `ClickHouse` source as this binary composes it: the adapter, over its HTTP transport.
///
/// Named once for the reason `crate::serve::BigQuerySource` is: it appears in a registry type, a
/// `Warehouse` bound and a constructor's return, and the two layers ARE the composition.
pub(crate) type ClickHouseSource = ClickHouseWarehouse<Http>;

/// Why an `impersonation-at-source` `clickhouse` entry is refused, in terms of what is missing
/// rather than of what to go and build.
///
/// **The mechanism is `SourcePosture::deliverable_by`, unchanged and unnarrowed** - it compares the
/// declared posture against this adapter's own `IMPERSONATION` constant and flips on its own the day
/// that constant changes. What this sentence adds is the one thing the domain refusal cannot know:
/// its generic remedy says *deploy a build whose adapter for that source can impersonate*, and for
/// `ClickHouse` there is no such build to deploy. Per-subject identity here is wanted and blocked on
/// work outside this repository, so an operator who reads only the generic remedy would go looking
/// for a feature flag that does not exist.
///
/// It is appended to the domain refusal rather than replacing it, and it becomes unreachable - not
/// wrong - the day the adapter declares `PerSubjectCredential`: `deliverable_by` returns `Ok` then
/// and this branch is never taken. That is the *one constant plus one arm* shape, kept on purpose.
///
/// **It deliberately names no cargo flag, and `cargo xtask check-feature-remedies` is why.** That
/// gate resolves every feature a refusal directs a reader at against the crate's own `[features]`
/// table, and it caught an earlier wording of this sentence for spelling `--features`: there is no
/// value for that flag that makes this source openable, so naming it would have been the
/// unactionable remedy that gate exists to refuse. What is left says what does not exist - which is
/// the honest answer, and the shape the gate's own doc names as earning no remedy.
const IMPERSONATION_DEFERRED: &str = "this adapter presents one HTTP Basic credential the deployment declared, and ClickHouse has \
     no per-subject path in this repository yet - so no build of sutura opens an \
     `impersonation-at-source` ClickHouse source today, whatever it was compiled with. Declare \
     `shared-service-user` with an acknowledgement instead";

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
/// A placement the dispatcher should have sent elsewhere; a source with no declared identity; the
/// `impersonation-at-source` posture, which this adapter has nowhere to put; a password file that
/// cannot be read or is empty; or declared TLS material that is not usable.
pub(crate) fn build(source: &SourceName, configured: &sutura_config::ConfiguredSource) -> Result<ClickHouseSource, String> {
    // Matched rather than read off accessors every kind would have to have, for the reason
    // `crate::serve::bigquery::build_bigquery` gives: the dispatcher has already decided this is the
    // ClickHouse arm, and a second openable kind should arrive as a compile error at this line too.
    let sutura_config::SourcePlacement::ClickHouse {
        ref host,
        port,
        ref user,
        ref password_file,
        ref transport,
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
    // See `IMPERSONATION_DEFERRED` for why its remedy is widened rather than the comparison narrowed.
    identity
        .posture()
        .deliverable_by(ClickHouseSource::IMPERSONATION, source)
        .map_err(|cause| format!("{}\n{IMPERSONATION_DEFERRED}", render(&cause)))?;
    let password = crate::password_file::read(source, password_file)?;
    let auth = Some(BasicAuth::new(user.clone(), password));
    let transport = open_transport(source, host, port, auth, transport)?;
    Ok(ClickHouseWarehouse::of(source.clone(), identity.posture().clone(), transport))
}

/// The transport for the channel this source declared: plaintext, or TLS over the declared store.
///
/// **The scheme follows the declaration and is never guessed.** `Endpoint::plaintext` for
/// `transport_mode: plaintext` and `Endpoint::tls` for the two TLS modes, so a source declared
/// `verified` cannot end up dialled over `http://` - which is the failure a single endpoint
/// constructor plus an optional TLS config would admit.
fn open_transport(
    source: &SourceName,
    host: &sutura_config::sources::placement::HostName,
    port: u16,
    auth: Option<BasicAuth>,
    declared: &sutura_config::sources::transport::SourceTransport,
) -> Result<Http, String> {
    use sutura_config::sources::transport::{SourceTransport, TrustAnchors};
    use sutura_exec_clickhouse::tls::{TlsAnchors, TlsIdentity};

    let (anchors, client) = match *declared {
        SourceTransport::Plaintext => {
            return Ok(Http::connect(Endpoint::plaintext(host.as_str(), port), auth));
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
    let tls = sutura_exec_clickhouse::tls::config(&anchors, client.as_ref()).map_err(|cause| {
        format!(
            "`sources.{source}` is declared TLS and its material is not usable: {}",
            render(&cause)
        )
    })?;
    Ok(Http::connect_secured(Endpoint::tls(host.as_str(), port), auth, tls))
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
        let secured = super::open_transport(&source, &host, 8443, None, &verified).expect("the declared bundle is usable");
        let plain = super::open_transport(&source, &host, 8123, None, &SourceTransport::Plaintext)
            .expect("a plaintext transport needs no material");
        let _ignored = std::fs::remove_dir_all(&directory);

        let secured = format!("{secured:?}");
        assert!(
            secured.contains("scheme: \"https\""),
            "a verified source must dial https: {secured}"
        );
        let plain = format!("{plain:?}");
        assert!(plain.contains("scheme: \"http\""), "a plaintext source dials http: {plain}");
    }
}
