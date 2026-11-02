//! What a node is for: serving callers, or running another node's stages.

/// What a node is for.
///
/// `Serving` is everything `sutura serve` did before there was a choice: it answers callers, reads
/// sources and coordinates its questions. The two worker roles run stages for the serving nodes
/// and answer no caller.
///
/// - `Worker`, spelled `worker`, only combines. `Settings::refusals` refuses one whose
///   configuration declares a source or a catalog, so it holds no source credential.
/// - `ReadingWorker`, spelled `reading-worker`, declares the sources it reads and the catalog it
///   certifies a leg against, and `Settings::refusals` refuses one that declares no source or no
///   catalog. **The role is the boot-time half only:** the process opens neither, and its worker is
///   a combining worker's, which refuses every stage whose leaf reads a source. Reading a partition
///   needs a task grant that names it, and the grant does not carry one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerRole {
    Serving,
    Worker,
    ReadingWorker,
}

/// The string named no role.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("`{found}` is not a server role - use one of: {}", ServerRole::NAMES.join(", "))]
pub struct UnknownServerRole {
    found: String,
}

impl ServerRole {
    /// The canonical spellings.
    pub const NAMES: &'static [&'static str] = &["serving", "worker", "reading-worker"];

    /// Reads a role name. No fallback: a misspelt `worker` must not start a serving node.
    pub fn parse(raw: impl AsRef<str>) -> Result<Self, UnknownServerRole> {
        let raw = raw.as_ref().trim();
        match raw.to_ascii_lowercase().as_str() {
            "serving" => Ok(Self::Serving),
            "worker" => Ok(Self::Worker),
            "reading-worker" => Ok(Self::ReadingWorker),
            _ => Err(UnknownServerRole {
                found: String::from(raw),
            }),
        }
    }
}
