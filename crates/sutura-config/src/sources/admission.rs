//! Source admission declarations; startup checks resolve their configured verifier names.

use super::SourceRegistry;
use crate::inbound::InboundVerifier;
use std::collections::BTreeSet;
use sutura_domain::identity::{InvalidVerifierId, VerifierId};
use sutura_domain::model::{InvalidIdentifier, SourceName};

#[derive(Debug, thiserror::Error)]
pub enum InvalidSourceAdmission {
    #[error("`sources.{written}` is not a source name")]
    Alias {
        written: String,
        #[source]
        cause: InvalidIdentifier,
    },
    #[error("`sources.{alias}.admitted_by[{index}]` is not a verifier id")]
    Verifier {
        alias: SourceName,
        index: usize,
        #[source]
        cause: InvalidVerifierId,
    },
}

pub(super) fn declare_admitted_by(
    registry: &mut SourceRegistry,
    written: &str,
    raw: Option<&[String]>,
    inbound: &[InboundVerifier],
) -> Result<(), InvalidSourceAdmission> {
    let alias = SourceName::parse(written).map_err(|cause| InvalidSourceAdmission::Alias {
        written: written.to_owned(),
        cause,
    })?;
    let admitted = match raw {
        Some(names) => Some(
            names
                .iter()
                .enumerate()
                .map(|(index, name)| {
                    VerifierId::parse(name).map_err(|cause| InvalidSourceAdmission::Verifier {
                        alias: alias.clone(),
                        index,
                        cause,
                    })
                })
                .collect::<Result<BTreeSet<_>, _>>()?,
        ),
        None => match inbound {
            [verifier] => Some(BTreeSet::from([verifier.id().clone()])),
            _ => None,
        },
    };
    if let Some(source) = registry.by_alias.get_mut(&alias) {
        source.admitted_by = admitted;
    }
    Ok(())
}
