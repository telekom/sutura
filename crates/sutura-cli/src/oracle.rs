//! The ONE Oracle composition, shared by both of this crate's composition roots.
//!
//! `crate::clickhouse`'s shape, for its reason: `build` below is the only place a declared `oracle`
//! entry becomes an open `sutura_domain::warehouse::Warehouse`, so the posture cross-check, the
//! secret read and the refusal WORDING cannot differ between `sutura serve` and `sutura
//! query`/`mcp`. What stays per root is the feature-off refusal, because the two are different
//! sentences - one names every declared source, the other the one it was asked about.
//!
//! **No channel resolution, and that absence is the placement's.** `sutura_config`'s
//! `SourcePlacement::Oracle` carries no transport because the parse accepts only `plaintext` on a
//! loopback host - the driver cannot be handed a declared trust store (see that variant's doc) - so
//! this module dials `host:port/service_name` in the clear and nothing else. **That is the first
//! dial only:** the driver follows a listener's TNS REDIRECT to any address it names, unchecked and
//! still in the clear, and authenticates there - `crate::serve::oracle`'s
//! `a_listener_redirect_is_followed_to_an_address_nobody_declared` holds that limit. The adapter's own
//! `connect_secured` stays unwired until a declared store can reach it.
//!
//! The WHOLE module is behind `#[cfg(feature = "oracle")]` at its declaration in `main.rs`, so
//! everything here may name an adapter type unconditionally.

use sutura_domain::model::SourceName;
use sutura_domain::warehouse::Warehouse as _;
use sutura_exec_oracle::OracleWarehouse;

use crate::commands::render;

/// Why an `impersonation-at-source` `oracle` entry is refused, in terms of what is missing rather
/// than of what to go and build.
///
/// `crate::clickhouse::IMPERSONATION_DEFERRED`'s shape and reason: `SourcePosture::deliverable_by`
/// is the mechanism, unchanged, and its generic remedy - *deploy a build whose adapter for that
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
/// The dial has no timeout of its own: the pinned driver connects with a bare
/// `TcpStream::connect`. The parse confines the DECLARED host to a loopback literal, which narrows
/// that first dial to a local listener that accepts and then stalls - and does not reach a
/// redirect's dial at all, which is a second bare `TcpStream::connect` to whatever address the
/// listener names.
///
/// # Errors
///
/// A placement the dispatcher should have sent elsewhere; a source with no declared identity; the
/// `impersonation-at-source` posture, which this adapter has nowhere to put; a password file that
/// cannot be read or is empty; or a connection the listener or the database refused.
pub(crate) fn build(source: &SourceName, configured: &sutura_config::ConfiguredSource) -> Result<OracleWarehouse, String> {
    // Matched rather than read off accessors every kind would have to have, for the reason
    // `crate::clickhouse::build` gives.
    let sutura_config::SourcePlacement::Oracle {
        ref host,
        port,
        ref service_name,
        ref user,
        ref password_file,
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
    OracleWarehouse::connect(
        source.clone(),
        identity.posture().clone(),
        host.as_str(),
        port,
        service_name.as_str(),
        user,
        exposed,
    )
    .map_err(|cause| format!("`sources.{source}` did not open: {}", render(&cause)))
}
