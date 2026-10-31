//! What a node is for: serving callers, or running another node's combine stages.

/// What a node is for.
///
/// `Serving` is everything `sutura serve` did before there was a choice: it answers callers, reads
/// sources and coordinates its questions. A `Worker` runs combine stages for the serving nodes and
/// nothing else, so it opens no catalog and no source; `Settings::refusals` refuses a worker
/// configuration that declares either.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerRole {
    Serving,
    Worker,
}

/// The string named no role.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("`{found}` is not a server role - use one of: {}", ServerRole::NAMES.join(", "))]
pub struct UnknownServerRole {
    found: String,
}

impl ServerRole {
    /// The canonical spellings.
    pub const NAMES: &'static [&'static str] = &["serving", "worker"];

    /// Reads a role name. No fallback: a misspelt `worker` must not start a serving node.
    pub fn parse(raw: impl AsRef<str>) -> Result<Self, UnknownServerRole> {
        let raw = raw.as_ref().trim();
        match raw.to_ascii_lowercase().as_str() {
            "serving" => Ok(Self::Serving),
            "worker" => Ok(Self::Worker),
            _ => Err(UnknownServerRole {
                found: String::from(raw),
            }),
        }
    }
}
