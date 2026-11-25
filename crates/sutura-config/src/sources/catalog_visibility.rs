//! `sources.<alias>.catalog_visibility`: whether each caller's catalog view is narrowed.
//!
//! Narrowed to what the source lists under that caller's own credential -
//! `docs/adr/20261125083027-catalog-visibility-per-caller.md`. Written after the entry itself parsed, as `split` is, so the refusal can read the entry's caller
//! identity. Off unless written: an entry without the key shows every caller the same models.

use sutura_domain::model::SourceName;

use super::caller_identity::CallerIdentity;
use super::{ConfiguredSource, SourceRegistry};

/// How a source narrows the catalog. One value and no boolean, so `false` cannot be written; the
/// absent key is the deployment-scoped catalog every caller sees.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogVisibility {
    /// Each caller sees only the models whose object type the source lists under that caller's own
    /// brokered token. Narrows what is SEEN; the source's refusal under the same token is the data
    /// control.
    PerCaller,
}

impl CatalogVisibility {
    /// The key under `sources.<alias>`.
    pub const KEY: &'static str = "catalog_visibility";

    /// What `source` declared, or `None` for the default every caller shares.
    #[must_use]
    pub const fn of(source: &ConfiguredSource) -> Option<Self> {
        source.catalog_visibility
    }

    /// The exact word `per-caller`; anything else is refused, not read as absent.
    ///
    /// # Errors
    ///
    /// [`CatalogVisibilityRefused::Unknown`] for any other value.
    pub fn parse(raw: &str) -> Result<Self, CatalogVisibilityRefused> {
        match raw {
            "per-caller" => Ok(Self::PerCaller),
            _ => Err(CatalogVisibilityRefused::Unknown),
        }
    }
}

/// Why a written `catalog_visibility` is refused. No variant carries the written value: it is
/// operator text, and the remedy is fixed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum CatalogVisibilityRefused {
    #[error("the only value is `per-caller`")]
    Unknown,
    /// Listing as the caller needs the caller's own token at the source, and only `foundry_login`
    /// brokers one for a catalog listing today.
    #[error(
        "`per-caller` lists object types with the caller's own token, and this source declares no `foundry_login` to get one"
    )]
    NoCallerLogin,
}

/// A written `catalog_visibility` is not usable on the entry it is written on.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("`sources.{alias}.catalog_visibility` is not usable: {cause}")]
pub struct CatalogVisibilityNotUsable {
    alias: SourceName,
    cause: CatalogVisibilityRefused,
}

impl CatalogVisibilityNotUsable {
    #[must_use]
    pub const fn alias(&self) -> &SourceName {
        &self.alias
    }

    #[must_use]
    pub const fn refused(&self) -> CatalogVisibilityRefused {
        self.cause
    }
}

/// Records on `registry` the `catalog_visibility` written on the entry `alias`.
///
/// An alias the registry does not hold was already refused by the entry's own parse.
pub(crate) fn declare(registry: &mut SourceRegistry, alias: &SourceName, value: &str) -> Result<(), CatalogVisibilityNotUsable> {
    let Some(source) = registry.by_alias.get_mut(alias) else {
        return Ok(());
    };
    let refused = |cause| CatalogVisibilityNotUsable {
        alias: alias.clone(),
        cause,
    };
    let visibility = CatalogVisibility::parse(value).map_err(refused)?;
    if !matches!(source.caller_identity(), Some(CallerIdentity::FoundryLogin(_))) {
        return Err(refused(CatalogVisibilityRefused::NoCallerLogin));
    }
    source.catalog_visibility = Some(visibility);
    Ok(())
}

#[cfg(test)]
mod tests;
