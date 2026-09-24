//! Which `BigQuery` ADBC driver this process will use, decided once for all three callers.
//!
//! **Its own module because there are three, and they used to be two copies plus a diagnostic that
//! disagreed with both.** `serve::bigquery` and `sources::bigquery` are separate composition roots
//! dispatching the same `SourceKind`, and `doctor` reports what a deployment would get; a rule
//! about where the driver comes from that each spelled for itself is a rule that drifts.
//!
//! **The order is not a preference, it is the security decision.** A release artefact carries its
//! own driver, pinned by the flake lock to the revision `nix/bigquery-adbc.nix` built - so a
//! mounted path must not be able to displace it. `telekom/sutura#929`'s sixth finding is that the
//! path was the ONLY route; this makes it the fallback that a source build needs and a published
//! artefact never takes.
//!
//! **The limit next to that:** this decides the driver, not the deployment's trust in it. A source
//! build that mounts a `.so` is trusting whatever that file is - nothing here verifies a signature,
//! and `sutura_exec_bigquery::adbc::AdbcBigQuery::probe` only establishes that it is this ABI and
//! that its runtime started.

use sutura_exec_bigquery::adbc::DriverLocation;

/// The variable a source build names a mounted driver with.
///
/// **Not a settings key, and that is unchanged**: which driver file a host carries is a property of
/// the host rather than of the semantic deployment, and a release artefact needs no answer at all.
/// A settings key would have to be optional in a tree where every other key is required to mean
/// something.
pub(crate) const MOUNTED: &str = "SUTURA_BIGQUERY_ADBC_DRIVER";

/// The driver this process will open, or the refusal an operator has to act on.
///
/// `subject` is what the refusal names - `sources.<name>` for a composition root, and what the
/// command is for a diagnostic. A `&str` and not the source name itself, because the two callers
/// locate the fix in different files and a refusal that named the wrong one would send an operator
/// to edit a settings key a diagnostic never read.
///
/// # Errors
///
/// A sentence naming `subject`, where this build carries no driver and no usable path was named -
/// the two cases being nothing set at all and a path
/// [`DriverLocation::parse`](sutura_exec_bigquery::adbc::DriverLocation::parse) refused.
pub(crate) fn resolve(subject: &str) -> Result<DriverLocation, String> {
    if let Some(carried) = DriverLocation::linked_in() {
        return Ok(carried);
    }
    let named = std::env::var(MOUNTED).map_err(|_absent| {
        format!(
            "this build carries no BigQuery ADBC driver and `{MOUNTED}` is not set, so {subject} \
             has no driver to open. A published sutura artefact carries its own; a build from source \
             points that variable at an absolute libadbc_driver_bigquery.so"
        )
    })?;
    DriverLocation::parse(&named)
        .map_err(|cause| format!("`{MOUNTED}` does not name a driver this process can open for {subject}: {cause}"))
}

/// Refuses a `security.outbound` declaration the `BigQuery` transport would silently not honour.
///
/// **The ADBC driver dials with its own trust store and presents no client certificate** - nothing
/// in this build hands it a bundle or an identity. The deleted HTTP transport did read both: a
/// declared bundle REPLACED the compiled-in roots, which narrows trust. So serving a `bigquery` source
/// under a declared bundle or identity would widen trust behind a declaration that says the
/// opposite, and a declaration that silently does nothing is what `docs/adr/0010` forbids.
/// `Anchors::System` with no identity promises nothing the driver's own store does not already
/// stand for, and is accepted - which is the limit: nothing here shows the driver's store is the
/// same set of roots.
///
/// # Errors
///
/// A sentence naming `sources.<source>` and what was declared, when `outbound` carries a bundle or
/// a client identity.
pub(crate) fn refuse_undeliverable_outbound(
    source: &sutura_domain::model::SourceName,
    outbound: Option<&sutura_tls::Declared>,
) -> Result<(), String> {
    let Some(declared) = outbound else {
        return Ok(());
    };
    let bundle = matches!(declared.anchors(), sutura_tls::Anchors::Bundle(_));
    let identity = declared.identity().is_some();
    if !bundle && !identity {
        return Ok(());
    }
    let what = match (bundle, identity) {
        (true, true) => "a trust-anchor bundle and a client identity",
        (true, false) => "a trust-anchor bundle",
        _ => "a client identity",
    };
    Err(format!(
        "`security.outbound` declares {what}, and `sources.{source}` is `kind: bigquery`: the ADBC \
         BigQuery driver dials with its own trust store and presents no client certificate, so this \
         declaration would reach nothing for that source. Remove the bundle and identity from \
         `security.outbound`, or serve `{source}` from a deployment that does not declare them"
    ))
}
