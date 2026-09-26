//! The bounded, single-open read of one catalog document.
//!
//! The open is INSIDE this function and everything about the file is decided from the handle that
//! is actually read: `O_NOFOLLOW` makes a final-component symlink a refusal (`ELOOP`) at open, a
//! swapped FIFO `ENXIO` at open rather than a boot that never returns, and the `fstat` here is of
//! the same handle the read runs on - so a document swapped between a stat and a separately named
//! read is not reachable at all. The byte budget is enforced on the READ itself, not on a `stat`
//! taken separately from it: the `fstat`'d size against the remaining budget is the fast path that
//! refuses a legitimately-oversized file before anything is read, and the read is done in bounded
//! chunks capped at the bytes the aggregate had left, with a post-read length recheck - so a file
//! that lies about its size, or grows while it is being read, is refused rather than allocated.
//! There is no `unsafe`: the descriptor is borrowed for the read and closed by rustix's ownership.
//!
//! The refusal paths and their exact reach are [`read_document`]'s contract; a caller maps its
//! [`ReadError`] into its own error enum, keeping its variants and rendered messages.

use std::io;
use std::path::{Path, PathBuf};

/// The most bytes a catalog root's documents may sum to.
///
/// A startup bound, not a request-time one: a served catalog directory is operator-mounted, and an
/// unbounded aggregate read is a startup cost nobody asked to pay. 16 MiB is a round number, and a
/// generous one, shared by every catalog adapter that uses this crate.
pub const MAX_CATALOG_BYTES: u64 = 16 * 1024 * 1024;

/// Why a bounded document read could not complete.
///
/// Every variant carries the path (a catalog is many files, and a message that names no file sends
/// a reader to read all of them), and, for [`Self::TooLarge`], the whole of the refusal's context,
/// so a caller can name the same things its own refusal did before this crate existed.
#[derive(Debug, thiserror::Error)]
pub enum ReadError {
    /// An open or `fstat` of a descriptor failed in a way the OS described but [`Self::Io`]'s
    /// wording cannot: a swapped symlink refuses with `ELOOP` and a swapped FIFO with `ENXIO`, and
    /// neither is "could not read".
    #[error("could not open {path}: {cause}")]
    Open {
        path: PathBuf,
        #[source]
        cause: rustix::io::Errno,
    },
    /// The document opened is not a regular file - a device node, for one, is refused by the
    /// regular-file check on the handle that was opened, not by the walk, which only saw the entry
    /// that was there before the swap.
    #[error("the document {path} is not a regular file - refused rather than read as one")]
    NotARegularFile { path: PathBuf },
    /// A read from the handle, or the conversion of the read bytes to UTF-8, failed.
    #[error("could not read {path}")]
    Io {
        path: PathBuf,
        #[source]
        cause: io::Error,
    },
    /// Reading this document pushed the running total over the aggregate byte budget.
    ///
    /// `root` is the catalog root, `document` is the one whose bytes crossed the budget, `found`
    /// is the running total the refusal saw, and `limit` is the aggregate cap. The size came from
    /// the handle's own `fstat` before the read, or from what the capped read actually delivered
    /// if a file grew in between - whichever refusal fired.
    #[error("the catalog at {root} holds more than {limit} bytes of documents (the read stopped at {document}, {found} found)")]
    TooLarge {
        root: PathBuf,
        document: PathBuf,
        found: u64,
        limit: u64,
    },
}

/// Reads one catalog document whole and against the aggregate byte budget.
///
/// `root` is the catalog root (carried only so the [`ReadError::TooLarge`] text can name it the way
/// each caller's own refusal does) and `total_bytes` is the running total of bytes already read, so
/// the remaining budget is the cap minus it. Returns the document's text, which is what the caller
/// parses next.
///
/// # Errors
///
/// [`ReadError::Open`] for an open or `fstat` that the OS refused (a swapped symlink `ELOOP`, a
/// swapped FIFO `ENXIO`); [`ReadError::NotARegularFile`] for a handle that is not a regular file;
/// [`ReadError::TooLarge`] when the document is oversized against the aggregate budget, whether its
/// declared size already is, it grew past it while being read, or the read delivered more than the
/// budget left; [`ReadError::Io`] for the read itself or a failed UTF-8 conversion.
pub fn read_document(root: &Path, path: &Path, total_bytes: u64) -> Result<String, ReadError> {
    let flags = rustix::fs::OFlags::RDONLY
        .union(rustix::fs::OFlags::NOFOLLOW)
        .union(rustix::fs::OFlags::NONBLOCK)
        .union(rustix::fs::OFlags::CLOEXEC);
    let fd = rustix::fs::open(path, flags, rustix::fs::Mode::empty()).map_err(|cause| ReadError::Open {
        path: path.to_path_buf(),
        cause,
    })?;
    // `fstat` on the descriptor we are about to read from: this is the file actually being read,
    // not a separately-named path. A non-regular file that opens anyway - a device, for one - is
    // refused here rather than read; a swapped symlink and a swapped FIFO are refused at the open
    // itself (`O_NOFOLLOW` / `O_NONBLOCK`), before this check runs.
    let stat = rustix::fs::fstat(&fd).map_err(|cause| ReadError::Open {
        path: path.to_path_buf(),
        cause,
    })?;
    if rustix::fs::FileType::from_raw_mode(stat.st_mode) != rustix::fs::FileType::RegularFile {
        return Err(ReadError::NotARegularFile {
            path: path.to_path_buf(),
        });
    }
    let remaining = MAX_CATALOG_BYTES - total_bytes.min(MAX_CATALOG_BYTES);
    if stat.st_size.cast_unsigned() > remaining {
        let found = total_bytes.saturating_add(stat.st_size.cast_unsigned());
        return Err(ReadError::TooLarge {
            root: root.to_path_buf(),
            document: path.to_path_buf(),
            found,
            limit: MAX_CATALOG_BYTES,
        });
    }
    // The read is on the descriptor itself, in bounded chunks: a document that grows while it is
    // being read cannot allocate past the bytes the aggregate had left, in any one chunk.
    let mut buf = Vec::new();
    let mut chunk = [0u8; 16 * 1024];
    loop {
        let n = rustix::io::read(&fd, &mut chunk).map_err(|cause| ReadError::Io {
            path: path.to_path_buf(),
            cause: cause.into(),
        })?;
        if n == 0 {
            break;
        }
        if buf.len() as u64 + n as u64 > remaining.saturating_add(1) {
            let found = total_bytes.saturating_add(remaining.saturating_add(1));
            return Err(ReadError::TooLarge {
                root: root.to_path_buf(),
                document: path.to_path_buf(),
                found,
                limit: MAX_CATALOG_BYTES,
            });
        }
        match chunk.get(..n) {
            Some(read) => buf.extend_from_slice(read),
            // `read` never reports more bytes than the chunk held; the bound is the one guard if
            // it ever did.
            None => {
                return Err(ReadError::TooLarge {
                    root: root.to_path_buf(),
                    document: path.to_path_buf(),
                    found: total_bytes.saturating_add(u64::try_from(n).unwrap_or(u64::MAX)),
                    limit: MAX_CATALOG_BYTES,
                });
            }
        }
    }
    let text = String::from_utf8(buf).map_err(|cause| ReadError::Io {
        path: path.to_path_buf(),
        cause: io::Error::new(io::ErrorKind::InvalidData, cause),
    })?;
    // The capped read above bounds what arrives; if the file actually held more than `remaining`
    // bytes, the length of what was READ re-checks it - closing the case of a file that grew
    // between the `fstat` and the read.
    if text.len() as u64 > remaining {
        let found = total_bytes.saturating_add(text.len() as u64);
        return Err(ReadError::TooLarge {
            root: root.to_path_buf(),
            document: path.to_path_buf(),
            found,
            limit: MAX_CATALOG_BYTES,
        });
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{MAX_CATALOG_BYTES, ReadError};

    /// A scratch directory of this test's own, cleared on the way in - `tempfile` is not a
    /// dependency of this workspace, matching the pattern the catalog adapters' own tests use.
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sutura-bounded-read-{name}-{}", std::process::id()));
        drop(std::fs::remove_dir_all(&dir));
        std::fs::create_dir_all(&dir).expect("a scratch directory is creatable");
        dir
    }

    #[test]
    fn a_regular_file_reads_whole_within_the_budget() {
        let root = scratch("regular");
        let body = "a small document";
        let path = root.join("doc.yaml");
        std::fs::write(&path, body).expect("a document is writable");
        let text = super::read_document(&root, &path, 0).expect("an in-budget regular file reads");
        assert_eq!(text, body);
        drop(std::fs::remove_dir_all(&root));
    }

    #[test]
    fn a_file_whose_size_is_past_the_budget_is_refused_as_too_large() {
        let root = scratch("oversized");
        // A byte paste of NULs: no parser runs here, this crate only checks sizes, and a regular
        // file is all the reader needs to exercise the byte bound.
        let bytes = vec![0u8; usize::try_from(MAX_CATALOG_BYTES + 1).expect("fits usize on a 64-bit test target")];
        let path = root.join("doc.yaml");
        std::fs::write(&path, &bytes).expect("a document is writable");
        let err = super::read_document(&root, &path, 0).expect_err("an oversized file refuses");
        assert!(matches!(err, ReadError::TooLarge { .. }), "{err:?}");
        drop(std::fs::remove_dir_all(&root));
    }

    #[test]
    fn the_budget_is_aggregate_not_per_file() {
        let root = scratch("aggregate");
        // `total_bytes` already at the cap: nothing more may be read, whatever any one file says.
        let body = "tiny";
        let path = root.join("doc.yaml");
        std::fs::write(&path, body).expect("a document is writable");
        let err =
            super::read_document(&root, &path, MAX_CATALOG_BYTES).expect_err("a file after the budget is exhausted refuses");
        assert!(matches!(err, ReadError::TooLarge { .. }), "{err:?}");
        drop(std::fs::remove_dir_all(&root));
    }
}
