//! Client-side TLS configuration for the HTTP transport.
//!
//! When the crate is built with a crypto-provider feature (`rustls-tls-aws-lc` or
//! `rustls-tls-ring`, which enable the internal `tls-mtls` feature), the driver can verify the
//! ClickHouse server against a caller-supplied PEM root bundle (replacing the built-in roots)
//! and/or present a client identity (PEM certificate chain + private key) for mutual TLS.
//!
//! The transport is assembled into a custom [`rustls::ClientConfig`] that is handed to the
//! underlying `clickhouse` client through [`clickhouse::Client::with_http_client`], which accepts
//! any `hyper-util` legacy client whose connector is a [`hyper_rustls::HttpsConnector`].

use adbc_core::error::{Error, Status};
#[cfg(feature = "tls-mtls")]
use rustls::pki_types::pem::{Error as PemError, PemObject as _};

use crate::options::{MTLS_CERT_CHAIN, MTLS_PRIVATE_KEY, TLS_ROOT_CERTS};

/// The TLS trust and identity options for the HTTP transport.
///
/// Holds the raw PEM strings set through the driver options. Each PEM value is validated as soon
/// as it is set; whether a client identity is fully specified (both halves) is only checked when
/// the transport is built, so a caller may set the two halves across separate option-setting calls.
#[derive(Clone, Default)]
pub(crate) struct TlsOptions {
    /// PEM root certificates that *replace* the built-in roots when set.
    pub(crate) root_certs: Option<String>,
    /// PEM client certificate chain for mutual TLS.
    pub(crate) client_cert_chain: Option<String>,
    /// PEM client private key for mutual TLS.
    pub(crate) client_private_key: Option<String>,
}

// The private key is a secret, so `Debug` reports only whether it is set.
impl std::fmt::Debug for TlsOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TlsOptions")
            .field("root_certs", &self.root_certs)
            .field("client_cert_chain", &self.client_cert_chain)
            .field(
                "client_private_key",
                &self.client_private_key.as_ref().map(|_| "<redacted>"),
            )
            .finish()
    }
}

/// Whether `key` is one of this module's TLS options.
pub(crate) fn is_tls_option(key: &str) -> bool {
    matches!(key, TLS_ROOT_CERTS | MTLS_CERT_CHAIN | MTLS_PRIVATE_KEY)
}

impl TlsOptions {
    pub(crate) fn is_empty(&self) -> bool {
        self.declared().is_empty()
    }

    /// The names of the options that are set.
    fn declared(&self) -> Vec<&'static str> {
        [
            (TLS_ROOT_CERTS, self.root_certs.is_some()),
            (MTLS_CERT_CHAIN, self.client_cert_chain.is_some()),
            (MTLS_PRIVATE_KEY, self.client_private_key.is_some()),
        ]
        .into_iter()
        .filter_map(|(name, set)| set.then_some(name))
        .collect()
    }

    /// Refuse a declared TLS setting on a URI that is not `https`.
    ///
    /// `scheme` is the scheme of the configured URI, or `None` where none is configured and the
    /// client's default (`http`) applies.
    pub(crate) fn require_https(&self, scheme: Option<&str>) -> Result<(), Error> {
        let declared = self.declared();
        if declared.is_empty() || scheme == Some("https") {
            return Ok(());
        }
        Err(invalid_arguments(format!(
            "a declared TLS setting requires an https URI: {declared:?} set, URI scheme is {:?}",
            scheme.unwrap_or("http"),
        )))
    }

    /// Validate and store a single option value.
    ///
    /// Malformed PEM is refused with [`Status::InvalidArguments`] immediately. The mutual-TLS
    /// pairing rule (certificate chain without key, or key without chain) is enforced later, at
    /// transport-build time, via [`Self::check_identity`]. Without a crypto-provider feature this
    /// refuses loudly rather than silently ignoring the option.
    pub(crate) fn try_set(&mut self, key: &str, value: String) -> Result<(), Error> {
        #[cfg(feature = "tls-mtls")]
        {
            match key {
                TLS_ROOT_CERTS => {
                    ensure_certificates(&value, key)?;
                    self.root_certs = Some(value);
                }
                MTLS_CERT_CHAIN => {
                    ensure_certificates(&value, key)?;
                    self.client_cert_chain = Some(value);
                }
                MTLS_PRIVATE_KEY => {
                    parse_private_key(&value, key)?;
                    self.client_private_key = Some(value);
                }
                other => return Err(unknown_option(other)),
            }
            Ok(())
        }

        #[cfg(not(feature = "tls-mtls"))]
        {
            let _ = (&key, &value);
            Err(Error::with_message_and_status(
                format!(
                    "option {key:?} requires this crate to be built with a \
                     `rustls-tls-aws-lc` or `rustls-tls-ring` feature"
                ),
                Status::InvalidArguments,
            ))
        }
    }

    /// Refuse a client identity specified with only one half (chain without key, or key without
    /// chain).
    pub(crate) fn check_identity(&self) -> Result<(), Error> {
        if self.client_cert_chain.is_some() != self.client_private_key.is_some() {
            return Err(Error::with_message_and_status(
                format!(
                    "client identity must set both {MTLS_CERT_CHAIN:?} and {MTLS_PRIVATE_KEY:?}; \
                     got certificate chain: {}, private key: {}",
                    self.client_cert_chain.is_some(),
                    self.client_private_key.is_some(),
                ),
                Status::InvalidArguments,
            ));
        }
        Ok(())
    }

    /// Read back an option's stored value.
    ///
    /// The private key is treated as a secret and its read-back is refused with
    /// [`Status::NotFound`] (mirroring the driver's handling of other sensitive options).
    pub(crate) fn read_back(&self, key: &str) -> Result<String, Error> {
        match key {
            MTLS_PRIVATE_KEY => Err(Error::with_message_and_status(
                "read-back of the private key has been omitted to avoid leaking a secret",
                Status::NotFound,
            )),
            TLS_ROOT_CERTS => Ok(self.root_certs.clone().unwrap_or_default()),
            MTLS_CERT_CHAIN => Ok(self.client_cert_chain.clone().unwrap_or_default()),
            other => Err(unknown_option(other)),
        }
    }

    /// Build the `hyper-rustls` HTTPS connector carrying this configuration.
    ///
    /// Only called when `!self.is_empty()`, i.e. the caller wants to customize the transport.
    #[cfg(feature = "tls-mtls")]
    pub(crate) fn build_https_connector(
        &self,
    ) -> Result<
        hyper_rustls::HttpsConnector<hyper_util::client::legacy::connect::HttpConnector>,
        Error,
    > {
        use hyper_rustls::ConfigBuilderExt;
        use hyper_rustls::HttpsConnectorBuilder;
        use hyper_util::client::legacy::connect::HttpConnector;
        use rustls::{ClientConfig, RootCertStore};

        self.check_identity()?;

        // Mirror the `clickhouse` crate: the crypto provider follows the enabled feature.
        #[cfg(feature = "rustls-tls-aws-lc")]
        let provider = rustls::crypto::aws_lc_rs::default_provider();
        #[cfg(all(feature = "rustls-tls-ring", not(feature = "rustls-tls-aws-lc")))]
        let provider = rustls::crypto::ring::default_provider();

        let builder = ClientConfig::builder_with_provider(std::sync::Arc::new(provider))
            .with_safe_default_protocol_versions()
            .map_err(transport_error)?;

        // A caller-supplied bundle *replaces* (rather than extends) the built-in root store.
        let builder = if let Some(pem) = &self.root_certs {
            let mut store = RootCertStore::empty();
            store.add_parsable_certificates(parse_certificates(pem, TLS_ROOT_CERTS)?);
            builder.with_root_certificates(store)
        } else {
            #[cfg(feature = "rustls-tls-native-roots")]
            {
                builder.with_native_roots().map_err(transport_error)?
            }
            #[cfg(all(
                feature = "rustls-tls-webpki-roots",
                not(feature = "rustls-tls-native-roots")
            ))]
            {
                builder.with_webpki_roots()
            }
        };

        let config = if let (Some(certs), Some(key)) = (
            self.client_cert_chain.as_deref(),
            self.client_private_key.as_deref(),
        ) {
            builder
                .with_client_auth_cert(
                    parse_certificates(certs, MTLS_CERT_CHAIN)?,
                    parse_private_key(key, MTLS_PRIVATE_KEY)?,
                )
                .map_err(|e| {
                    invalid_arguments(format!("error configuring client certificate: {e}"))
                })?
        } else {
            builder.with_no_client_auth()
        };

        let http = HttpConnector::new();
        Ok(HttpsConnectorBuilder::new()
            .with_tls_config(config)
            .https_or_http()
            .enable_http1()
            .wrap_connector(http))
    }
}

#[cfg(feature = "tls-mtls")]
fn parse_certificates(
    pem: &str,
    key: &str,
) -> Result<Vec<rustls::pki_types::CertificateDer<'static>>, Error> {
    rustls::pki_types::CertificateDer::pem_slice_iter(pem.as_bytes())
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| invalid_arguments(format!("malformed PEM in option {key:?}: {e}")))
}

#[cfg(feature = "tls-mtls")]
fn ensure_certificates(pem: &str, key: &str) -> Result<(), Error> {
    let certs = parse_certificates(pem, key)?;
    if certs.is_empty() {
        return Err(invalid_arguments(format!(
            "no certificates found in option {key:?}"
        )));
    }
    Ok(())
}

#[cfg(feature = "tls-mtls")]
fn parse_private_key(
    pem: &str,
    key: &str,
) -> Result<rustls::pki_types::PrivateKeyDer<'static>, Error> {
    rustls::pki_types::PrivateKeyDer::from_pem_slice(pem.as_bytes()).map_err(|e| match e {
        PemError::NoItemsFound => invalid_arguments(format!("no private key found in option {key:?}")),
        other => invalid_arguments(format!("malformed PEM in option {key:?}: {other}")),
    })
}

fn invalid_arguments(message: impl Into<String>) -> Error {
    Error::with_message_and_status(message, Status::InvalidArguments)
}

fn transport_error(e: impl std::fmt::Display) -> Error {
    Error::with_message_and_status(
        format!("error building TLS configuration: {e}"),
        Status::Internal,
    )
}

fn unknown_option(key: &str) -> Error {
    Error::with_message_and_status(
        format!("unknown TLS option {key:?}"),
        Status::InvalidArguments,
    )
}

#[cfg(all(test, feature = "tls-mtls"))]
mod tests {
    use super::*;
    use adbc_core::error::Status;

    fn identity() -> (String, String) {
        let pair = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
        (pair.cert.pem(), pair.signing_key.serialize_pem())
    }

    #[test]
    fn tls_option_names_use_repo_style() {
        assert_eq!(TLS_ROOT_CERTS, "clickhouse.client.tls_root_certs");
        assert_eq!(MTLS_CERT_CHAIN, "clickhouse.client.mtls_cert_chain");
        assert_eq!(MTLS_PRIVATE_KEY, "clickhouse.client.mtls_private_key");
        for key in [TLS_ROOT_CERTS, MTLS_CERT_CHAIN, MTLS_PRIVATE_KEY] {
            assert!(is_tls_option(key));
            assert!(key.starts_with("clickhouse.client."));
        }
        assert!(!is_tls_option("clickhouse.client.product_info"));
    }

    #[test]
    fn refuses_malformed_pem() {
        let mut opts = TlsOptions::default();
        let err = opts
            .try_set(TLS_ROOT_CERTS, "not a certificate".into())
            .unwrap_err();
        assert_eq!(err.status, Status::InvalidArguments);

        let err = opts
            .try_set(
                MTLS_CERT_CHAIN,
                "-----BEGIN CERTIFICATE-----\n!!!!!!!!\n-----END CERTIFICATE-----".into(),
            )
            .unwrap_err();
        assert_eq!(err.status, Status::InvalidArguments);

        let err = opts
            .try_set(MTLS_PRIVATE_KEY, "not a key".into())
            .unwrap_err();
        assert_eq!(err.status, Status::InvalidArguments);
    }

    #[test]
    fn accepts_valid_pem() {
        let (certificate, private_key) = identity();
        let mut opts = TlsOptions::default();
        opts.try_set(TLS_ROOT_CERTS, certificate.clone()).unwrap();
        opts.try_set(MTLS_CERT_CHAIN, certificate.clone())
            .unwrap();
        opts.try_set(MTLS_PRIVATE_KEY, private_key.clone())
            .unwrap();
        assert!(!opts.is_empty());
        opts.check_identity().unwrap();
    }

    #[test]
    fn declared_tls_requires_an_https_uri() {
        let (certificate, private_key) = identity();
        let mut opts = TlsOptions::default();
        assert!(opts.require_https(None).is_ok(), "nothing declared, nothing to refuse");
        opts.try_set(TLS_ROOT_CERTS, certificate).unwrap();
        assert!(opts.require_https(Some("https")).is_ok());
        for scheme in [None, Some("http")] {
            let refused = opts.require_https(scheme).unwrap_err();
            assert_eq!(refused.status, Status::InvalidArguments);
            assert!(refused.message.contains(TLS_ROOT_CERTS), "{refused:?}");
        }
        let mut identity_only = TlsOptions::default();
        identity_only.try_set(MTLS_PRIVATE_KEY, private_key).unwrap();
        assert!(identity_only.require_https(Some("http")).is_err());
    }

    #[test]
    fn refuses_half_identity() {
        let (certificate, private_key) = identity();
        let mut opts = TlsOptions::default();
        opts.try_set(MTLS_CERT_CHAIN, certificate.clone())
            .unwrap();
        // Certificate chain without a private key is refused.
        let err = opts.check_identity().unwrap_err();
        assert_eq!(err.status, Status::InvalidArguments);

        let (certificate, private_key) = identity();
        let mut opts = TlsOptions::default();
        opts.try_set(MTLS_PRIVATE_KEY, private_key.clone())
            .unwrap();
        // Private key without a certificate chain is refused.
        let err = opts.check_identity().unwrap_err();
        assert_eq!(err.status, Status::InvalidArguments);
    }

    #[test]
    fn builds_https_connector() {
        let (certificate, private_key) = identity();
        let mut opts = TlsOptions::default();
        opts.try_set(TLS_ROOT_CERTS, certificate.clone()).unwrap();
        opts.try_set(MTLS_CERT_CHAIN, certificate.clone())
            .unwrap();
        opts.try_set(MTLS_PRIVATE_KEY, private_key.clone())
            .unwrap();
        opts.build_https_connector().unwrap();
    }
}
