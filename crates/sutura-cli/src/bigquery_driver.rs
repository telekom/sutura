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

/// Who `source`'s jobs bill, as the adapter holds it. The service account project is parsed a
/// second time by the transport's own type, because that is the value written into a request.
pub(crate) fn job_billing(
    source: &sutura_domain::model::SourceName,
    billing: &sutura_config::sources::billing::SourceBilling,
) -> Result<sutura_exec_bigquery::JobBilling, String> {
    use sutura_config::sources::billing::SourceBilling;
    Ok(match *billing {
        SourceBilling::CallerRun { caller_may_name, .. } => sutura_exec_bigquery::JobBilling::CallerRun { caller_may_name },
        SourceBilling::ServiceAccount { ref project } => sutura_exec_bigquery::JobBilling::ServiceAccount(
            sutura_exec_bigquery::transport::ProjectId::parse(project.as_str()).map_err(|cause| {
                format!("`sources.{source}.billing.service_account_project` is not a usable project id: {cause}")
            })?,
        ),
    })
}

/// Both command roots share the same endpoints and native-driver TLS material.
#[cfg(feature = "bigquery")]
pub(crate) fn endpoints(
    configured: &sutura_config::google_endpoints::GoogleEndpoints,
    outbound: Option<&sutura_tls::Declared>,
) -> Result<sutura_exec_bigquery::adbc::GoogleEndpoints, String> {
    let endpoints = sutura_exec_bigquery::adbc::GoogleEndpoints::new(
        configured.bigquery().as_str(),
        configured.bigquery_storage().as_str(),
        configured.iam_credentials().as_str(),
        configured.oauth_token().as_str(),
        configured.sts().as_str(),
    );
    let path = |path: &std::path::Path| {
        path.to_str()
            .map(str::to_owned)
            .ok_or_else(|| String::from("BigQuery TLS material needs a UTF-8 file path"))
    };
    let Some(outbound) = outbound else {
        return Ok(endpoints);
    };
    let ca = match outbound.anchors() {
        sutura_tls::Anchors::System => None,
        sutura_tls::Anchors::Bundle(file) => Some(path(file)?),
    };
    let identity = outbound
        .identity()
        .map(|identity| Ok::<_, String>((path(identity.certificate())?, path(identity.key())?)))
        .transpose()?;
    Ok(endpoints.with_trust(ca, identity))
}

#[cfg(test)]
mod endpoint_cells {
    #[test]
    #[cfg(all(unix, feature = "bigquery"))]
    fn native_google_tls_refuses_non_utf8_ca_certificate_and_key_paths() {
        use std::os::unix::ffi::OsStringExt as _;
        let invalid = std::path::PathBuf::from(std::ffi::OsString::from_vec(vec![b'/', 0xff]));
        let declarations = [
            sutura_tls::Declared::new(sutura_tls::Anchors::Bundle(invalid.clone()), None),
            sutura_tls::Declared::new(
                sutura_tls::Anchors::System,
                Some(sutura_tls::Identity::new(invalid.clone(), "/etc/sutura/client.key".into())),
            ),
            sutura_tls::Declared::new(
                sutura_tls::Anchors::System,
                Some(sutura_tls::Identity::new("/etc/sutura/client.pem".into(), invalid)),
            ),
        ];
        for declaration in declarations {
            let result = super::endpoints(
                &sutura_config::google_endpoints::GoogleEndpoints::default(),
                Some(&declaration),
            );
            let error = result.expect_err("native TLS material with an unusable file path is refused before opening a driver");
            assert_eq!(error, "BigQuery TLS material needs a UTF-8 file path");
        }
    }
}
