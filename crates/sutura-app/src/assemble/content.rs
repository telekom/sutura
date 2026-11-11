//! Borrow the same declarations from a linked bundle or an unlinked contribution.

use sutura_domain::capabilities::MetadataCapabilities;
use sutura_domain::catalog::{Metric, Model, Relationship, UnlinkedContribution};
use sutura_domain::knowledge::{Absence, Caveat, Example, GlossaryEntry, KnowledgeCapabilities, NoteName, Phrase};
use sutura_domain::model::{MetricName, ModelName, RelationshipName};
use sutura_domain::pinned::PinnedDefinitions;

pub(super) enum Content<'a> {
    Pinned(&'a PinnedDefinitions),
    Unlinked(&'a UnlinkedContribution),
}

enum Elements<'a, K, V> {
    Map(std::collections::btree_map::Values<'a, K, V>),
    Slice(std::slice::Iter<'a, V>),
}

impl<'a, K, V> Iterator for Elements<'a, K, V> {
    type Item = &'a V;
    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Map(values) => values.next(),
            Self::Slice(values) => values.next(),
        }
    }
}

impl Content<'_> {
    pub(super) fn models(&self) -> impl Iterator<Item = &Model> {
        match self {
            Self::Pinned(bundle) => Elements::<ModelName, Model>::Map(bundle.definitions().models().values()),
            Self::Unlinked(input) => Elements::Slice(input.models().iter()),
        }
    }

    pub(super) fn relationships(&self) -> impl Iterator<Item = &Relationship> {
        match self {
            Self::Pinned(bundle) => Elements::<RelationshipName, Relationship>::Map(bundle.definitions().relationships().values()),
            Self::Unlinked(input) => Elements::Slice(input.relationships().iter()),
        }
    }

    pub(super) fn metrics(&self) -> impl Iterator<Item = &Metric> {
        match self {
            Self::Pinned(bundle) => Elements::<MetricName, Metric>::Map(bundle.definitions().metrics().values()),
            Self::Unlinked(input) => Elements::Slice(input.metrics().iter()),
        }
    }

    pub(super) fn glossary(&self) -> impl Iterator<Item = &GlossaryEntry> {
        match self {
            Self::Pinned(bundle) => Elements::<Phrase, GlossaryEntry>::Map(bundle.knowledge().glossary().values()),
            Self::Unlinked(input) => Elements::Slice(input.knowledge().glossary().iter()),
        }
    }

    pub(super) fn caveats(&self) -> impl Iterator<Item = &Caveat> {
        match self {
            Self::Pinned(bundle) => Elements::<NoteName, Caveat>::Map(bundle.knowledge().caveats().values()),
            Self::Unlinked(input) => Elements::Slice(input.knowledge().caveats().iter()),
        }
    }

    pub(super) fn absences(&self) -> impl Iterator<Item = &Absence> {
        match self {
            Self::Pinned(bundle) => Elements::<Phrase, Absence>::Map(bundle.knowledge().absences().values()),
            Self::Unlinked(input) => Elements::Slice(input.knowledge().absences().iter()),
        }
    }

    pub(super) fn examples(&self) -> impl Iterator<Item = &Example> {
        match self {
            Self::Pinned(bundle) => Elements::<NoteName, Example>::Map(bundle.knowledge().examples().values()),
            Self::Unlinked(input) => Elements::Slice(input.knowledge().examples().iter()),
        }
    }

    pub(super) fn declares(&self) -> &KnowledgeCapabilities {
        match self {
            Self::Pinned(bundle) => bundle.knowledge().declares(),
            Self::Unlinked(input) => input.knowledge().declares(),
        }
    }

    pub(super) fn produced(&self) -> MetadataCapabilities {
        match self {
            Self::Pinned(bundle) => MetadataCapabilities::produced(bundle.definitions(), bundle.knowledge()),
            Self::Unlinked(input) => input.produced(),
        }
    }
}
