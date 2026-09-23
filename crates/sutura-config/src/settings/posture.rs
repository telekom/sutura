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

use sutura_domain::model::SourceName;

use crate::security::DeploymentIdentity;
use crate::server::BindAddress;

/// Which of the two reasons a caller-facing credential, or the limiter, was required.
///
/// Closed rather than a free string: every site below chooses between exactly these two reasons -
/// production, or reachable off-host - never a third, so a match missing an arm is a compile error
/// rather than a refusal nobody wrote. One type shared across [`NotFitToServe::AccessTokenRequired`],
/// [`NotFitToServe::RateLimitingDisabled`] and [`NotFitToServe::MetricsTokenRequired`], because all
/// three ask the identical question `settings.rs`'s `metrics_refusals` asks first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenRequiredBy {
    /// The environment is production, regardless of the bind.
    Production,
    /// The bind is reachable from other hosts, regardless of environment.
    OffHost,
}

impl core::fmt::Display for TokenRequiredBy {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::Production => "this is a production deployment",
            Self::OffHost => "this service is bound where other hosts can reach it",
        })
    }
}

/// A deployment this service refuses to start as.
///
/// **These are the security posture, and each one is a refusal rather than a warning on purpose.**
/// The thing being guarded against is not an operator who ignores a log line - it is an operator
/// who never sees one, because the line was emitted in a format nothing was collecting, on a
/// process that went on to serve traffic. A process that does not start is noticed.
///
/// Every variant names the key to change, because a refusal that does not say what to do is a
/// support request.
///
/// **No `Serialize`, and neither has [`crate::settings::SettingsError`] that carries it.** Both
/// reach a caller only through `#[error]`'s rendered text on stderr, never as a structured value -
/// so typing a field here - [`BindAddress`], [`SourceName`], [`TokenRequiredBy`] - is a compiler
/// check on this crate's own construction sites, and publishes nothing to anyone outside it.
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
        "server.host is {bind} (from {origin}), which is reachable from other hosts, and \
         security.tls_termination is `none`. Say where TLS is terminated - one of: sidecar, \
         ingress, in-process - or bind 127.0.0.1. The declaration does not encrypt anything: it \
         records which cleartext hop this bearer token crosses, which is a fact only this \
         deployment knows"
    )]
    TlsTerminationUndeclared {
        bind: BindAddress,
        /// A provenance SENTENCE, not a value - `self.layers.origin_of("server.host")` at
        /// `settings.rs:459-463` already renders it (a file path, a variable name, or "embedded
        /// defaults"). Typing it further would mean this crate re-parsing its own message.
        origin: String,
    },
    /// Something is reachable off-host, or this is production, and there is no token.
    ///
    /// Not authentication - see [`crate::security`] - but the difference between a bearer secret
    /// and nothing at all is the difference between a configured reader and anyone who can route
    /// a packet.
    #[error(
        "{because}, and no inbound identity is configured, so security.access_token must be set. \
         It authenticates the DEPLOYMENT and not the caller: sutura has no per-caller identity, so \
         every query still runs with whatever access this process already had"
    )]
    AccessTokenRequired { because: TokenRequiredBy },
    /// Reachable off-host, or production, with the limiter switched off.
    ///
    /// **Keyed exactly like [`Self::MetricsTokenRequired`], and that is the fix over the previous
    /// shape.** The old refusal fired only in production, but an unbounded caller is an unbounded
    /// aggregate over the same up-to-ten-years history whether or not the deployment happens to be
    /// labelled `production` - reachability off-host is what makes the load somebody else's to send.
    #[error(
        "{because}, and rate_limit.enabled is false. A question here is an aggregate over up to ten \
         years of history, so an unbounded caller is an unbounded load on the data system"
    )]
    RateLimitingDisabled { because: TokenRequiredBy },
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
    SharedSourceNotAcknowledged { alias: SourceName },
    /// The raw SQL tool is enabled in a deployment that declared it serves more than one subject.
    ///
    /// **The same "same reason, same mechanism" the shared-source check already uses, over a
    /// sharper capability.** `docs/adr/0013` requires the raw tool available only where the source
    /// executes as the asking subject, or the deployment is single-user - and today no linked
    /// adapter accepts a raw statement AND carries a per-subject credential, so this refuses
    /// unconditionally on the declared mode rather than asking a composition root about an adapter
    /// capability that cannot yet make the answer `no`. **The limit, stated with the claim:** this
    /// is a boot-time refusal over a declared mode, not a runtime check that a caller's identity
    /// actually varies - `single-user` is still a word an operator writes.
    #[error(
        "tools.run_sql.enabled is true and {key} is `multi-user`. The raw SQL tool executes under \
         one shared identity for every caller - see docs/adr/0013 - so it may run only where the \
         deployment is single-user or a source executes as the asking subject. Neither holds here: \
         set tools.run_sql.enabled to false, or declare {key} as single-user with a written reason",
        key = DeploymentIdentity::KEY
    )]
    RunSqlEnabledInMultiUserMode,
    /// A per-replica spend ceiling declared over a `BigQuery` source, whose ADBC transport prices
    /// nothing.
    ///
    /// **The ceiling would be a declared control that bounds nothing on the one source it exists
    /// for.** `sutura_app`'s spend ledger charges a question what its dry run priced, and the ADBC
    /// transport declines the dry run, so every `BigQuery` question is charged zero. Refused at boot
    /// rather than served, until that transport prices a statement again. **The limit:** what
    /// bounds one `BigQuery` job's spend without a ceiling is the source's own `max_bytes_billed`,
    /// per job and not per subject.
    ///
    /// **"Prices nothing" was checked against the pinned driver rather than assumed, for
    /// `telekom/sutura#929`'s second review finding, and it is not because the driver has no dry
    /// run.** It does: `bigquery.query.dry_run` is a real statement option
    /// (`adbc-drivers/bigquery` pinned rev `5f1e65dc8a904c39cdf79ef9e19140139c1becb4`,
    /// `go/driver.go`'s `OptionQueryDryRun`), and setting it makes `ExecuteQuery` return the job's
    /// `Statistics.TotalBytesProcessed` as its `rows_affected` out-parameter instead of a row count
    /// (`go/record_reader.go:140-141`'s dry-run branch of `runQuery`, reached through
    /// `go/statement.go`'s `ExecuteQuery` at `executeUpdate = false`). **What has no route is that
    /// value reaching Rust.** The pinned binding's `Statement::execute` - the ADBC call `crate::adbc`
    /// makes - passes `null_mut()` for exactly that out-parameter
    /// (`adbc_driver_manager-0.24.0/src/lib.rs:1195-1205`), discarding it before this crate could
    /// read it. The other two ADBC calls that DO read `rows_affected`,
    /// `execute_update`/`execute_partitions` (`adbc_driver_manager-0.24.0/src/lib.rs:1218-1233`),
    /// are both dead ends of their own: `ExecuteUpdate` takes the DML-affected-rows branch of the
    /// SAME Go function whenever `executeUpdate = true`, ahead of the dry-run branch
    /// (`go/record_reader.go:135-141`), so it can never reach the estimate either; and
    /// `ExecutePartitions` is unconditionally `NotImplemented` for this driver
    /// (`go/statement.go:940-945`). No statement option surfaces it as a value to read back either -
    /// `GetOptionInt` echoes only configuration this crate itself set (`go/statement.go:183-203`),
    /// and the driver's OTHER statistics surface, `ConnectionGetStatistics`
    /// (`go/connection_statistics.go`), reports a TABLE's stored bytes, never a query's estimated
    /// scan - the wrong kind of number even where it is reachable. So the refusal stays: nothing
    /// this crate can call gets a byte estimate out of this driver, and the day the pinned
    /// binding's `execute` reads `rows_affected` this citation is the one to revisit.
    #[error(
        "governance.per_replica_spend_ceiling is set and sources.{alias} is a `bigquery` source,          whose ADBC transport prices nothing - so the ceiling would never charge its questions.          Remove governance.per_replica_spend_ceiling, or remove the source;          sources.{alias}.max_bytes_billed still bounds each job"
    )]
    UnpricedSourceUnderSpendCeiling {
        /// The `bigquery` source the ceiling would not bound.
        alias: SourceName,
    },
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

    /// The metrics endpoint is mounted and nothing protects its own credential.
    ///
    /// `docs/adr/0015` Decision 1: `/metrics` has its own token, never the deployment's, so a
    /// scrape cannot interrogate the business. The endpoint lives on the one listener and is always
    /// mounted, so a deployment reachable off-host or in production with no metrics token has an
    /// unauthenticated way to read the process's counters - the same argument
    /// [`Self::AccessTokenRequired`] makes for the API surface, applied to what a scrape can see.
    #[error(
        "the metrics endpoint is mounted and {because}, but security.metrics_token is not set. The \
         metrics endpoint is gated by its own credential, never the deployment token - a scrape \
         needs to read counters, and giving it the API token would hand the monitoring system the \
         ability to interrogate the business. Set security.metrics_token"
    )]
    MetricsTokenRequired { because: TokenRequiredBy },
    /// The two credentials are the same value, which collapses the separation `docs/adr/0015`
    /// exists for.
    ///
    /// A holder of the metrics token can then ask any question the catalog certifies, which turns
    /// the monitoring credential store into a data-access secret store. Nothing at runtime would
    /// show it, so it is a refusal to start.
    #[error(
        "security.metrics_token is equal to security.access_token. The metrics endpoint must be \
         gated by a credential of its own: a single token behind both surfaces hands the monitoring \
         system every ability a holder of the deployment token has. Use a different value for each"
    )]
    MetricsTokenSharesTheApiToken,
}
