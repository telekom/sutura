//! What a refresh did to the served bundle, or why it did not.

use sutura_domain::definitions::DefinitionDigest;
use sutura_domain::pinned::NotValidated;

use super::ErasedCause;

/// What a refresh did to the served bundle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Adopted {
    /// The bundle read and its schema set have the digests already served; nothing changed.
    Unchanged,
    /// A different bundle, or the same one over a different schema set, passed every check and is
    /// what the next question reads. The two digests are equal where only the schema set changed.
    Rotated {
        /// The digest that was served until now.
        previous: DefinitionDigest,
        /// The digest served from now on.
        digest: DefinitionDigest,
    },
}

/// Why a refreshed bundle was not stored. The bundle already served is still the one served.
#[derive(Debug, thiserror::Error)]
pub enum NotAdopted {
    /// The deployment's own pre-flight refused it: a model names a table this process does not hold.
    #[error("the refreshed bundle names tables this deployment cannot serve")]
    Preflight {
        /// The deployment's own refusal, whatever type it is.
        #[source]
        cause: ErasedCause,
    },
    /// A declared key or an anchor did not hold, or could not be run.
    #[error("the refreshed bundle is not fit to serve")]
    NotValidated {
        #[source]
        cause: NotValidated,
    },
}
