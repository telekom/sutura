//! `sources.<alias>.split`: how a source's lookup legs may be read in partitions.
//!
//! The nodes that read the source read the partitions (`github.com/telekom/sutura#828`). Written
//! after the entry itself parsed, as `inbound_token_at_pool` is, so the refusals can read the
//! entry's kind and posture.

use sutura_domain::model::SourceName;
use sutura_domain::source::SourcePosture;

use super::{InvalidSourceRegistry, SourceKind, SourceRegistry};

/// How a source's lookup legs may be split.
///
/// One value and no boolean, so the key reads as how it splits and `false` cannot be written. An
/// entry without the key reads every leg whole.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceSplit {
    /// An `oracle` source read as its shared service user: every partition reads `AS OF` the one
    /// system change number taken when the leg is split. The service user holds the `FLASHBACK`
    /// object privilege on each table it reads, never `FLASHBACK ANY TABLE`: the source's boot probe
    /// refuses the system privilege however it is granted, directly or through any role.
    Flashback,
}

impl SourceSplit {
    /// The key under `sources.<alias>`.
    pub const KEY: &'static str = "split";

    /// The exact word `flashback`; anything else is refused, not read as absent.
    ///
    /// # Errors
    ///
    /// [`SplitRefused::Unknown`] for any other value.
    pub fn parse(raw: &str) -> Result<Self, SplitRefused> {
        match raw {
            "flashback" => Ok(Self::Flashback),
            _ => Err(SplitRefused::Unknown),
        }
    }
}

/// Why a written `split` is refused. No variant carries the written value: it is operator text, and
/// the one accepted word is the whole remedy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SplitRefused {
    #[error("the only value is `flashback`")]
    Unknown,
    /// Only an Oracle source reads a snapshot by its system change number.
    #[error("`flashback` reads an Oracle snapshot, and this source is `kind: {}`", kind.as_str())]
    NotOracle { kind: SourceKind },
    /// A partition is read as the shared service user, and this source reads as each caller.
    #[error("a partition is read as the shared service user, and this source is `impersonation-at-source`")]
    Impersonating,
}

/// Records on `registry` the `split` written on the entry `written`, after the entry itself parsed.
pub(crate) fn declare(registry: &mut SourceRegistry, written: &str, value: &str) -> Result<(), InvalidSourceRegistry> {
    let alias = SourceName::parse(written).map_err(|cause| InvalidSourceRegistry::Alias {
        written: String::from(written),
        cause,
    })?;
    let Some(source) = registry.by_alias.get_mut(&alias) else {
        return Ok(());
    };
    let refused = |cause| InvalidSourceRegistry::Split {
        alias: alias.clone(),
        cause,
    };
    let split = SourceSplit::parse(value).map_err(refused)?;
    if source.kind() != SourceKind::Oracle {
        return Err(refused(SplitRefused::NotOracle { kind: source.kind() }));
    }
    if matches!(source.posture(), Some(SourcePosture::ImpersonationAtSource)) {
        return Err(refused(SplitRefused::Impersonating));
    }
    source.split = Some(split);
    Ok(())
}

#[cfg(test)]
mod tests;
