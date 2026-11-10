//! The Foundry half of `sutura serve`: one declared Ontology SQL source becomes a warehouse.

/// Opens one Foundry adapter per declared source.
#[cfg(feature = "foundry")]
pub(crate) fn open_foundry(
    declared: &[&sutura_domain::model::SourceName],
    registry: &sutura_config::SourceRegistry,
    outbound: Option<&sutura_tls::Declared>,
    runtime: sutura_config::RuntimeSettings,
) -> Result<super::OpenedSources, String> {
    let mut engines: Option<sutura_app::Warehouses<super::FoundrySource>> = None;
    for source in declared {
        let configured = super::configured_source(source, registry)?;
        let engine = crate::foundry::build(source, configured, outbound, crate::sources::working_set(runtime))?;
        engines = Some(match engines {
            None => sutura_app::Warehouses::of(engine),
            Some(open) => open.and(engine).map_err(super::flatten)?,
        });
    }
    engines
        .map(super::OpenedSources::Foundry)
        .ok_or_else(|| String::from("this catalog declares no models, so there is nothing to open"))
}

/// The refusal for a build that did not link the Foundry SQL adapter.
#[cfg(not(feature = "foundry"))]
pub(crate) fn open_foundry(
    declared: &[&sutura_domain::model::SourceName],
    _registry: &sutura_config::SourceRegistry,
    _outbound: Option<&sutura_tls::Declared>,
    _runtime: sutura_config::RuntimeSettings,
) -> Result<super::OpenedSources, String> {
    let named = declared
        .iter()
        .map(|source| source.as_str())
        .collect::<Vec<&str>>()
        .join(", ");
    Err(format!(
        "[{named}] declares `kind: foundry`, and this binary was built without the `foundry` feature - so it links no Foundry SQL adapter. Build `sutura-cli` with `--features foundry`, or declare a `files` source"
    ))
}

#[cfg(all(test, not(feature = "foundry")))]
mod tests {
    use crate::serve::open_engine;
    use crate::serve::tests::{bundle_over, default_timeout, one_worker, refusal, registry};

    #[test]
    fn a_foundry_source_is_refused_by_a_build_that_did_not_link_the_adapter() {
        let entry = "  warehouse:\n    kind: foundry\n    endpoint: https://foundry.example.com\n    ontology: sales\n    token_file: /nonexistent/sutura-foundry-token\n    posture: shared-service-user\n";
        let error = refusal(
            open_engine(
                &bundle_over(&[("customers", "warehouse", "customers")]),
                &registry(entry),
                one_worker(),
                default_timeout(),
                None,
            ),
            "a build without Foundry must refuse the source",
        );
        assert!(error.contains("warehouse"), "{error}");
        assert!(error.contains("`kind: foundry`"), "{error}");
        assert!(error.contains("without the `foundry` feature"), "{error}");
    }
}
