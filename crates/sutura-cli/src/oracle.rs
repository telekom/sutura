//! The ONE Oracle composition, shared by both of this crate's composition roots.
//!
//! `crate::clickhouse`'s shape, for its reason: `build` below is the only place a declared `oracle`
//! entry becomes an open `sutura_domain::warehouse::Warehouse`, so the posture cross-check, the
//! secret read and the refusal WORDING cannot differ between `sutura serve` and `sutura
//! query`/`mcp`. What stays per root is the feature-off refusal, because the two are different
//! sentences - one names every declared source, the other the one it was asked about.
//!
//! **The channel is the placement's `OracleChannel`**: `plaintext` to a loopback host, or `verified`
//! against a PEM bundle this module reads at boot and hands to the adapter as the ONLY anchors. A
//! listener's redirect is refused before authentication, so the connection stays on the declared
//! address - `crate::serve::oracle`'s `a_listener_redirect_to_an_address_nobody_declared_is_refused`
//! holds it.
//!
//! The WHOLE module is behind `#[cfg(feature = "oracle")]` at its declaration in `main.rs`, so
//! everything here may name an adapter type unconditionally.

use sutura_config::sources::placement::OracleChannel;
use sutura_domain::model::SourceName;
use sutura_domain::warehouse::Warehouse as _;
use sutura_exec_oracle::{Channel, Dial, OracleWarehouse};

use crate::commands::render;

/// Why an `impersonation-at-source` `oracle` entry is refused, in terms of what is missing rather
/// than of what to go and build.
///
/// `SourcePosture::deliverable_by` is the mechanism, unchanged, and its generic remedy - *deploy a build whose adapter for that
/// source can impersonate* - names a build that does not exist for this kind. Appended to the domain
/// refusal, and unreachable rather than wrong the day the adapter declares `PerSubjectCredential`.
/// It names no cargo flag, because no value of one makes this source impersonate
/// (`cargo xtask check-feature-remedies`).
const IMPERSONATION_DEFERRED: &str = "this adapter is one connection under the user the deployment declared, and Oracle has no \
     per-subject path in this repository yet - so no build of sutura opens an \
     `impersonation-at-source` Oracle source today, whatever it was compiled with. Declare \
     `shared-service-user` with an acknowledgement instead";

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
/// # Errors
///
/// A placement the dispatcher should have sent elsewhere; a source with no declared identity; the
/// `impersonation-at-source` posture, which this adapter has nowhere to put; a password file that
/// cannot be read or is empty; an anchor bundle that cannot be read or holds no usable certificate;
/// or a connection the listener or the database refused.
pub(crate) fn build(
    source: &SourceName,
    configured: &sutura_config::ConfiguredSource,
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
        ref channel,
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
        .map_err(|cause| format!("{}\n{IMPERSONATION_DEFERRED}", render(&cause)))?;
    let password = crate::password_file::read(source, password_file)?;
    #[expect(
        clippy::disallowed_methods,
        reason = "the password's destination is the connection handshake, which is the one place the \
                  value itself is the payload"
    )]
    let exposed = password.expose_secret();
    let (dialled, anchors_pem) = match *channel {
        OracleChannel::Plaintext => (host.as_str(), None),
        OracleChannel::Verified {
            ref anchors,
            ref server_name,
        } => (
            server_name.as_str(),
            Some(std::fs::read_to_string(anchors).map_err(|cause| {
                format!(
                    "`sources.{source}.transport_anchors` ({}) could not be read: {cause}",
                    anchors.display()
                )
            })?),
        ),
    };
    let channel = anchors_pem
        .as_deref()
        .map_or(Channel::Plaintext, |anchors_pem| Channel::Verified { anchors_pem });
    let dial = Dial::new(dialled, port, service_name.as_str(), channel);
    OracleWarehouse::connect(
        source.clone(),
        identity.posture().clone(),
        dial,
        user,
        exposed,
        working_set.result_budget(),
    )
    .map_err(|cause| format!("`sources.{source}` did not open: {}", render(&cause)))
}
