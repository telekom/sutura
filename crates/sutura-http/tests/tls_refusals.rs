//! One `TlsNotUsable` variant, provoked outside `src/tls.rs`'s own test module.
//!
//! That module's tests already name the other six `TlsNotUsable` variants, and
//! `xtask/src/refusals.rs`'s `name_evidence` discards a file naming EVERY variant of an enum as a
//! census - which is what one file naming all seven would read as, even split honestly across
//! seven separate tests. Keeping this one here is what keeps the enrolment real evidence rather
//! than a name the gate throws away.

#![cfg(feature = "tls")]

#[cfg(test)]
mod tests {
    use sutura_http::tls::TlsNotUsable;
    use tokio_rustls::rustls::ServerConfig;
    use tokio_rustls::rustls::crypto::CryptoProvider;
    use tokio_rustls::rustls::crypto::ring::default_provider;

    #[test]
    fn a_provider_with_no_usable_cipher_suite_cannot_configure_the_server() {
        // `Termination::prepare` always builds `default_provider()`, which this variant's own doc
        // calls "unreachable from a pair that got this far" - no material this crate can build
        // reaches it through `prepare` itself. So this reaches the same rustls call directly, with
        // the one provider shape that call refuses, rather than naming the variant without
        // exercising the branch that constructs it.
        let broken = CryptoProvider {
            cipher_suites: Vec::new(),
            ..default_provider()
        };
        let cause = ServerConfig::builder_with_provider(std::sync::Arc::new(broken))
            .with_safe_default_protocol_versions()
            .expect_err("a provider with no cipher suites has no usable one");
        let error = TlsNotUsable::NotConfigurable { cause };
        assert!(format!("{error}").contains("would not build"), "{error}");
    }
}
