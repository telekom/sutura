//! The recorded fixtures and the fake reader that serves them.
//!
//! `fixture/example/` is a synthetic metadata registry: its document, its mapping and the answers
//! it gives, keyed by request target. Written for this adapter, not recorded from a running service.
//! `fixture/foundry-sample/` is a sample of the configured mode for one vendor's ontology API: a
//! document subset transcribed from public reference pages and a mapping over it, with no answers.

use std::collections::BTreeMap;
use std::time::Instant;

use sutura_domain::model::SourceName;
use sutura_domain::pinned::DefinitionVersion;

use crate::{AnswerReader, Contract, ContractError, OpenApiCatalog, Target};

/// The example registry's document.
pub const DOCUMENT: &str = include_str!("fixture/example/openapi.json");
/// The example registry's mapping.
pub const MAPPING: &str = include_str!("fixture/example/mapping.yaml");
/// The example registry's answers, by request target.
pub const ANSWERS: &str = include_str!("fixture/example/answers.json");
/// The vendor sample's document subset.
pub const SAMPLE_DOCUMENT: &str = include_str!("fixture/foundry-sample/openapi.json");
/// The vendor sample's mapping.
pub const SAMPLE_MAPPING: &str = include_str!("fixture/foundry-sample/mapping.yaml");

/// Why the fixture could not be opened or answer.
#[derive(Debug, thiserror::Error)]
pub enum FixtureError {
    #[error("the recorded answers are not a JSON object of answers")]
    Answers(#[source] serde_json::Error),
    #[error("the fixture's document and mapping do not make a contract")]
    Contract(#[source] ContractError),
    #[error("no answer is recorded for {target}")]
    Unrecorded { target: String },
}

/// The fake [`AnswerReader`]: it answers each recorded target and refuses every other one.
#[derive(Debug, Clone)]
pub struct FixtureReader(BTreeMap<String, serde_json::Value>);

impl FixtureReader {
    /// A reader over answers keyed by request target.
    #[must_use]
    pub const fn new(answers: BTreeMap<String, serde_json::Value>) -> Self {
        Self(answers)
    }

    /// A reader over [`ANSWERS`].
    pub fn recorded() -> Result<Self, FixtureError> {
        serde_json::from_str(ANSWERS).map(Self).map_err(FixtureError::Answers)
    }
}

impl AnswerReader for FixtureReader {
    type Error = FixtureError;

    fn get(&self, target: &Target, _started: Instant) -> Result<serde_json::Value, Self::Error> {
        self.0.get(target.as_str()).cloned().ok_or_else(|| FixtureError::Unrecorded {
            target: target.as_str().to_owned(),
        })
    }
}

/// An [`OpenApiCatalog`] over the example registry, every model on `name`. The golden registry
/// opens it.
pub fn over_fixture_source(name: SourceName, version: DefinitionVersion) -> Result<OpenApiCatalog<FixtureReader>, FixtureError> {
    let contract = Contract::parse(DOCUMENT, MAPPING).map_err(FixtureError::Contract)?;
    Ok(OpenApiCatalog::new(
        name.clone(),
        version,
        name,
        contract,
        FixtureReader::recorded()?,
    ))
}
