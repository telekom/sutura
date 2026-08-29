//! The deployment postures this service refuses to start as.
//!
//! A file of its own for the reason [`crate::settings::inbound`] is: `settings.rs` reached the
//! thousand-line limit `cargo xtask max-lines` enforces, and this is the most separable thing in it.
//! The seam is the one [`crate::settings`]'s own module documentation already names - a parse turns a
//! raw tree into typed values, and this is the *other* half, the combinations of individually valid
//! values that are not fit to serve. Nothing here parses anything.
//!
//! `Settings::refusals` is the only producer, and it stays beside `Settings` because it reads that
//! struct's private fields.

use crate::security::DeploymentIdentity;

/// A deployment this service refuses to start as.
///
/// **These are the security posture, and each one is a refusal rather than a warning on purpose.**
/// The thing being guarded against is not an operator who ignores a log line - it is an operator
/// who never sees one, because the line was emitted in a format nothing was collecting, on a
/// process that went on to serve traffic. A process that does not start is noticed.
///
/// Every variant names the key to change, because a refusal that does not say what to do is a
/// support request.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NotFitToServe {
    /// The bind address is reachable from other hosts and nobody said what protects the path to
    /// it.
    ///
    /// **This is not a refusal of the bind, and the previous version of this row was.** A
    /// plaintext listener on a pod network with an ingress controller or a sidecar terminating TLS
    /// in front of it is the normal arrangement, and refusing it would refuse the deployment this
    /// service is built for. What is refused is *silence*: the bearer token crosses whatever sits
    /// between the terminator and this process in cleartext, and how far that reaches - a loopback
    /// hop inside a pod, or the pod network - is a fact about the deployment that only the operator
    /// knows. Naming it makes it a stated fact that the startup log can print, and a declaration
    /// cannot be satisfied by agreeing that off-host was intended.
    #[error(
        "server.host is {bind}, which is reachable from other hosts, and security.tls_termination \
         is `none`. Say where TLS is terminated - one of: sidecar, ingress, in-process - or bind \
         127.0.0.1. The declaration does not encrypt anything: it records which cleartext hop \
         this bearer token crosses, which is a fact only this deployment knows"
    )]
    TlsTerminationUndeclared { bind: String },
    /// Something is reachable off-host, or this is production, and there is no token.
    ///
    /// Not authentication - see [`crate::security`] - but the difference between a bearer secret
    /// and nothing at all is the difference between a configured reader and anyone who can route
    /// a packet.
    #[error(
        "{because}, so security.access_token must be set. It authenticates the DEPLOYMENT and not \
         the caller: sutura has no per-caller identity, so every query still runs with whatever \
         access this process already had"
    )]
    AccessTokenRequired { because: &'static str },
    /// Production with the limiter switched off.
    #[error(
        "rate_limit.enabled is false in production. A question here is an aggregate over up to ten \
         years of history, so an unbounded caller is an unbounded load on the data system"
    )]
    RateLimitingDisabledInProduction,
    /// Production asking the kernel to choose the port.
    #[error(
        "server.port is 0 in production, which asks the kernel for an ephemeral port. Nothing can \
         then be configured to reach this service; port 0 is for a test that reads the port back"
    )]
    EphemeralPortInProduction,
    /// A forwarded header would be believed with nobody named as the hop it may come from.
    ///
    /// **The one refusal in this list that exists because the permissive branch is worse than the
    /// restrictive one in both directions.** With no trusted hop, `X-Forwarded-For` is a value any
    /// caller writes, so every bucket becomes the caller's to choose - a limiter that reports a
    /// configured limit and bounds nothing at all, which is strictly worse than the one shared
    /// bucket that peer keying gives behind a proxy.
    #[error(
        "rate_limit.client_address is `forwarded` and rate_limit.trusted_proxies is empty. A \
         forwarded header is a value any caller can write, so with no hop named it would let every \
         caller pick their own rate-limit bucket. List the proxy addresses or blocks, or set \
         client_address: peer"
    )]
    ForwardedWithoutTrustedProxies,
    /// Trusted proxies were listed and nothing reads them.
    ///
    /// Refused rather than ignored, for the reason every unknown key here is an error: a list that
    /// does nothing reads as a control that is in place.
    #[error(
        "rate_limit.trusted_proxies names {count} hop(s) and rate_limit.client_address is `peer`, \
         which reads no header - so the list has no effect. Set client_address: forwarded, or \
         remove the list"
    )]
    TrustedProxiesWithoutForwarding { count: usize },
    /// TLS termination was declared as in-process and no certificate and key were given.
    #[error(
        "security.tls_termination is `in-process` and no server.tls_certificate and server.tls_key \
         are set. This process cannot terminate TLS without them, and it will not fall back to \
         plaintext on a port that was configured to be encrypted"
    )]
    InProcessTlsWithoutMaterial,
    /// A certificate and key were given and nothing will use them.
    #[error(
        "server.tls_certificate and server.tls_key are set and security.tls_termination is \
         `{declared}`, so this process serves plaintext and the material is never read. Set \
         tls_termination: in-process, or remove the paths"
    )]
    TlsMaterialWithoutInProcessTermination { declared: &'static str },
    /// TLS termination was declared as in-process and this binary cannot do it.
    ///
    /// **The loud failure the requirement asks for.** A binary built without the `tls` feature has
    /// no TLS implementation linked in at all, so the alternative to refusing is serving plaintext
    /// on a port an operator configured to be encrypted - which is the one failure mode that must
    /// never be quiet.
    #[error(
        "security.tls_termination is `in-process` and this binary was built without the `tls` \
         feature, so it has no TLS implementation linked in. Rebuild with `--features tls`, or \
         terminate TLS in front of this process and declare `sidecar` or `ingress`"
    )]
    InProcessTlsNotCompiledIn,
    /// Sources are configured and nobody said which kind of deployment this is.
    ///
    /// **The mode has no default, and this is the refusal that makes that true.** It is keyed on a
    /// source being configured rather than raised unconditionally, because a deployment with no source
    /// cannot answer anything and the composition root refuses it on the catalog naming a source with
    /// no declaration - so every deployment that can serve a question reaches this check.
    #[error(
        "{count} source(s) are configured and {} is not set. Say which kind of deployment this is - \
         one of: {}. It decides where a shared source's acknowledgement has to be written, and no \
         combination of source postures may answer it on your behalf: a multi-tenant deployment whose \
         sources are all shared is exactly the case a derived mode would exempt from the check it most \
         needs",
        DeploymentIdentity::KEY,
        DeploymentIdentity::NAMES.join(", ")
    )]
    DeploymentIdentityUndeclared { count: usize },
    /// A source is served to every caller as one identity in a multi-user deployment, and no operator
    /// wrote that down on that source's own entry.
    ///
    /// **This is the check the shared posture exists to be caught by.** The wrong outcome here is not a
    /// failure - it is an answer, computed from rows a caller's own permissions never filtered. Sutura
    /// declares no data sensitivity, so it cannot see whether that was fine; what it can do is make the
    /// posture impossible to arrive at by accident and impossible to arrive at in silence.
    ///
    /// Per source and never global: a deployment cannot acknowledge one source and inherit it for the
    /// next. In single-user mode the mode's own declaration supplies the witness, because there the one
    /// identity is the one user's own.
    #[error(
        "source `{alias}` is `shared-service-user`, {} is `multi-user`, and \
         `sources.{alias}.acknowledged_because` is not set. Every caller would read that source as one \
         identity that is not theirs. Write why that is intended on this entry - there is no global \
         acknowledgement, and no acknowledgement is inherited from another source",
        DeploymentIdentity::KEY
    )]
    SharedSourceNotAcknowledged { alias: String },
    /// Two different credentials configured to arrive in one header.
    ///
    /// **A collision found by building leg 1 rather than by reading the record**, and it is worth
    /// stating because `docs/adr/0014` says the deployment token and leg 1 both survive and answer
    /// different questions. They do - in the `behind-gateway` mode, where the proof arrives in a
    /// header of the component's own and `Authorization` stays the deployment token's.
    ///
    /// In the `direct` mode they cannot. RFC 6750 puts an access token in `Authorization: Bearer` and
    /// an OAuth 2.1 client has no option to put it elsewhere, so a deployment that is its own resource
    /// server owns that header. Configuring both is configuring a request that has to carry two
    /// values in one field, and every alternative to refusing it is worse: sniffing whether the value
    /// looks like a JWT is a guess, and checking one and then the other makes the *weaker* credential
    /// sufficient.
    ///
    /// So the direct mode replaces the deployment token rather than joining it - which is why
    /// [`Self::AccessTokenRequired`] does not fire when an inbound identity is configured. That is
    /// not a weakening: a validated, audience-bound, expiring token per caller is strictly more than
    /// a shared secret every caller holds.
    #[error(
        "security.access_token is set and security.inbound.mode is `direct`, and both are read from \
         `authorization: Bearer`. A request cannot carry two credentials in one header. In the \
         direct mode this deployment IS the resource server, so the caller's own token is what \
         authenticates the request - remove security.access_token. To keep a deployment-wide \
         perimeter as well, put the caller's identity behind a component and declare \
         `behind-gateway`, whose proof arrives in a header of its own"
    )]
    DeploymentTokenSharesTheHeader,
}
