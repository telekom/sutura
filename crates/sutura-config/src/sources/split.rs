//! `sources.<alias>.split`: how a source's lookup legs may be read in partitions.
//!
//! The nodes that read the source read the partitions (`github.com/telekom/sutura#828`).
//!
//! **Not built yet:** `split` is not a key of a source entry, so a deployment that writes it is
//! refused as an unknown key, and no source declares a split.

use super::SourceKind;

/// How a source's lookup legs may be split.
///
/// One value and no boolean, so the key reads as how it splits and `false` cannot be written. An
/// entry without the key reads every leg whole.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceSplit {
    /// An `oracle` source read as its shared service user: every partition reads `AS OF` the one
    /// system change number taken when the leg is split. The service user holds the `FLASHBACK`
    /// object privilege on each table it reads, never `FLASHBACK ANY TABLE`, and the source's boot
    /// probe refuses one that holds the system privilege.
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

#[cfg(test)]
mod tests;
