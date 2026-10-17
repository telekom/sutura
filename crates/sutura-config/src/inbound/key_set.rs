//! Where the inbound signing keys come from.

use crate::inbound::primitive::KeySetFile;

/// Where the signing keys are read from.
///
/// **An enum, so a second source arrives as a variant every reader has to match** rather than as a
/// second optional field a reader can forget. The gate picks its key source out of this value, and
/// the cache it feeds is the same whatever the source is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeySetLocation {
    /// A key set on disk at a [`KeySetFile`] path.
    File(KeySetFile),
}
