//! Where a key set comes from, and the bound on the document it hands back.
//!
//! Split off `super` at the thousand-line cap, at the seam the file already had: this module is
//! *the source*, and nothing in it reads a clock, holds a lock or decides when a look happens -
//! that is all [`super::KeySetCache`]. `docs/adr/0014`'s remaining half lands here too: a JWKS
//! endpoint is a second implementor of [`KeySetSource`] and touches nothing else.

use std::path::{Path, PathBuf};

use super::InvalidKeySet;

/// The largest key set document a source may hand back.
///
/// **The bound at the edge**, and the edge is where it has to be: a document is read into memory and
/// then parsed, so an unbounded one is work proportional to whatever happens to be at the path -
/// which is a denial-of-service primitive whatever else it is. Sixty-four kilobytes is two orders of
/// magnitude above a JWK set holding a handful of keys, and small enough that one read and one parse
/// are bounded work.
///
/// **It bounds an implementor and cannot bound the port.** [`KeySetSource::read`] hands back a
/// `String`, so by the time [`super::KeySetCache`] sees a document the allocation has already happened.
/// [`FileKeySet`] checks it; a second implementor carries its own check against this constant, and
/// nothing in this module can make it.
pub const MAX_KEY_SET_BYTES: usize = 64 * 1024;

/// Where a key set is read from.
///
/// One method, so a JWKS endpoint is a second implementor and nothing else in this module moves. See
/// `super`'s module documentation for why the only implementor today reads a file.
///
/// **It returns the document's BYTES rather than a parsed key set**, and that is what lets
/// [`super::KeySetCache::poll_once`] tell "changed" from "unchanged" the way `crate::tls::Renewal` does. A
/// comparison of parsed keys could not: the library's key type implements no equality, so the
/// alternative was comparing key *ids*, which would miss a key whose material rotated under the same
/// id.
///
/// **Synchronous, deliberately**, and it runs on the blocking pool rather than on the executor -
/// see [`super::KeySetCache::look`]. Making the trait `async` would either need a boxed future in the
/// signature or force the file source to pretend; what it would not fix is that a synchronous read
/// has to run somewhere, and where that is is the cache's decision rather than the source's.
///
/// **Bounding the document is the IMPLEMENTOR's job and cannot be the cache's.** This returns an
/// owned `String`, so an unbounded read has already allocated by the time anything above it could
/// object. [`MAX_KEY_SET_BYTES`] is the number to check against, [`FileKeySet`] checks it, and
/// nothing in this module makes a second implementor do the same - a type cannot express it, so this
/// paragraph is the whole of the mechanism.
pub trait KeySetSource: Send + Sync + 'static {
    /// Reads the key set document as it is now, refusing one over [`MAX_KEY_SET_BYTES`].
    fn read(&self) -> Result<String, KeySetUnavailable>;
}

/// The source could not be read, or what it returned is not a key set.
#[derive(Debug, thiserror::Error)]
pub enum KeySetUnavailable {
    #[error("the key set at {path} could not be read")]
    Unreadable {
        path: PathBuf,
        #[source]
        cause: std::io::Error,
    },
    #[error("the key set at {path} is not usable")]
    Invalid {
        path: PathBuf,
        #[source]
        cause: InvalidKeySet,
    },
    /// The document is larger than [`MAX_KEY_SET_BYTES`].
    ///
    /// The limit and not the size, because the size is whatever is at the path and the refusal is
    /// about the limit. **Refused rather than truncated:** half a JWK set is not a JWK set, and a
    /// parse of a truncated document would refuse it with a diagnostic about JSON instead of one
    /// about size.
    #[error("the key set at {path} is larger than the {limit} bytes a key set may be")]
    TooLarge { path: PathBuf, limit: usize },
}

/// A key set on the local filesystem.
#[derive(Debug, Clone)]
pub struct FileKeySet {
    path: PathBuf,
}

impl FileKeySet {
    /// Names the file. Does not read it: [`Self::read`] is the read, and the composition root reads
    /// once before the listener opens so an unreadable key set is a refusal to start.
    #[must_use]
    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// The path, for a startup log line.
    #[inline]
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl KeySetSource for FileKeySet {
    /// Reads the file, refusing one larger than [`MAX_KEY_SET_BYTES`].
    ///
    /// It was `std::fs::read_to_string`, which reads whatever is there. **`Read::take` and not a
    /// `metadata` check first**, because the size a `stat` reports and the size a read returns are
    /// two observations of a file a sidecar may be rewriting, and only the second one bounds the
    /// allocation. One byte past the limit is read, so a document exactly at it is accepted and the
    /// first byte over it is seen without reading the rest of whatever is at the path.
    fn read(&self) -> Result<String, KeySetUnavailable> {
        let unreadable = |cause| KeySetUnavailable::Unreadable {
            path: self.path.clone(),
            cause,
        };
        let file = std::fs::File::open(&self.path).map_err(unreadable)?;
        let mut bounded = std::io::Read::take(file, u64::try_from(MAX_KEY_SET_BYTES).unwrap_or(u64::MAX).saturating_add(1));
        let mut document = String::new();
        let _bytes = std::io::Read::read_to_string(&mut bounded, &mut document).map_err(unreadable)?;
        if document.len() > MAX_KEY_SET_BYTES {
            return Err(KeySetUnavailable::TooLarge {
                path: self.path.clone(),
                limit: MAX_KEY_SET_BYTES,
            });
        }
        Ok(document)
    }
}
