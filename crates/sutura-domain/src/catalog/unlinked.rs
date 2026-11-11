//! Typed catalog declarations before cross-contributor references are linked.

use crate::capabilities::MetadataCapabilities;
use crate::knowledge::KnowledgeInput;
use crate::model::SourceName;
use crate::pinned::DefinitionVersion;

use super::{Metric, Model, Relationship};

/// A contributor's own declarations. Only assembly may turn these into a pinned bundle.
#[derive(Debug)]
pub struct UnlinkedContribution {
    name: SourceName,
    version: DefinitionVersion,
    declared: MetadataCapabilities,
    models: Vec<Model>,
    relationships: Vec<Relationship>,
    metrics: Vec<Metric>,
    knowledge: KnowledgeInput,
}

impl UnlinkedContribution {
    /// Keeps ownership separate while references to another contributor remain unresolved.
    pub const fn new(
        name: SourceName,
        version: DefinitionVersion,
        declared: MetadataCapabilities,
        models: Vec<Model>,
        relationships: Vec<Relationship>,
        metrics: Vec<Metric>,
        knowledge: KnowledgeInput,
    ) -> Self {
        Self {
            name,
            version,
            declared,
            models,
            relationships,
            metrics,
            knowledge,
        }
    }

    pub const fn name(&self) -> &SourceName {
        &self.name
    }
    pub const fn version(&self) -> &DefinitionVersion {
        &self.version
    }
    pub const fn declared(&self) -> &MetadataCapabilities {
        &self.declared
    }
    pub fn models(&self) -> &[Model] {
        &self.models
    }
    pub fn relationships(&self) -> &[Relationship] {
        &self.relationships
    }
    pub fn metrics(&self) -> &[Metric] {
        &self.metrics
    }
    pub const fn knowledge(&self) -> &KnowledgeInput {
        &self.knowledge
    }

    /// Observes only this contributor, never declarations supplied to resolve its references.
    pub fn produced(&self) -> MetadataCapabilities {
        MetadataCapabilities::produced_input(&self.models, &self.relationships, &self.metrics, &self.knowledge)
    }
}
