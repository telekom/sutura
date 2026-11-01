//! A declared secret read from its file: a `clickhouse`, `oracle` or `postgres` entry's password,
//! an `rdbms` catalog's, or a `bigquery` delegation exchange's client secret.
//!
//! **One copy for every root that reads one** (`crate::clickhouse`, `crate::oracle`,
//! `crate::postgres`, `crate::catalog`, `crate::serve::broker`), so the trim, the empty-file
//! refusal and the wording cannot differ between them.

/// The declared user's password, read at BOOT rather than on the first question.
///
/// The same argument `crate::serve::bigquery`'s credential read makes: a password file that is
/// missing, unreadable or empty has to stop the composition, not become a deployment that answers
/// every question with an authentication failure while its startup log says it opened a database.
///
/// Trimmed, because a file written by `echo` carries a newline the server would reject; empty after
/// trimming is refused rather than sent, so a truncated secret file is a refusal naming the key
/// instead of an authentication failure on the first question.
#[cfg(any(feature = "clickhouse", feature = "oracle", feature = "postgres"))]
pub(crate) fn read(
    source: &sutura_domain::model::SourceName,
    password_file: &std::path::Path,
) -> Result<sutura_domain::identity::Secret, String> {
    read_key(&format!("sources.{source}.password_file"), password_file)
}

/// The most bytes a secret file may hold - far above any password or client secret, so a larger file
/// is a mistyped path, not a credential.
const MAX_SECRET_BYTES: usize = 64 * 1024;

/// `read` for any declared secret file; `key` is the full settings key the refusal names, field
/// included.
///
/// Only a regular file is read, through a symlink as a mounted Kubernetes Secret is one, so a FIFO
/// or a device refuses instead of blocking boot. The untrimmed copy is read into a buffer sized once
/// and never grown, which is wiped on drop. A file swapped for a FIFO between the check and the open
/// still blocks; the path is the operator's own.
pub(crate) fn read_key(key: &str, password_file: &std::path::Path) -> Result<sutura_domain::identity::Secret, String> {
    use std::io::Read as _;
    let unreadable = |cause: std::io::Error| format!("`{key}` could not be read at {}: {cause}", password_file.display());
    if !std::fs::metadata(password_file).map_err(unreadable)?.is_file() {
        return Err(format!("`{key}` at {} is not a regular file", password_file.display()));
    }
    let mut raw = secrecy::zeroize::Zeroizing::new(String::with_capacity(MAX_SECRET_BYTES + 1));
    std::fs::File::open(password_file)
        .and_then(|file| file.take(MAX_SECRET_BYTES as u64 + 1).read_to_string(&mut raw))
        .map_err(unreadable)?;
    if raw.len() > MAX_SECRET_BYTES {
        return Err(format!(
            "`{key}` at {} holds more than {MAX_SECRET_BYTES} bytes, which no secret this deployment reads does",
            password_file.display()
        ));
    }
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(format!(
            "`{key}` is empty, and an empty secret is not a credential \
             this deployment can present"
        ));
    }
    Ok(sutura_domain::identity::Secret::new(trimmed))
}

#[cfg(test)]
mod bounded;

#[cfg(test)]
#[cfg(any(feature = "clickhouse", feature = "oracle"))]
mod tests {
    use sutura_domain::model::SourceName;

    /// A file holding only whitespace - a truncated secret, or `echo > file` - is refused naming the
    /// key, never trimmed down to an empty secret and sent.
    #[test]
    fn a_whitespace_only_password_file_is_refused_naming_the_key() {
        let directory = std::env::temp_dir().join(format!("sutura-cli-password-file-{}", std::process::id()));
        std::fs::create_dir_all(&directory).expect("a scratch directory is creatable");
        let file = directory.join("password");
        std::fs::write(&file, " \n\t\n").expect("the file writes");
        let refusal = super::read(&SourceName::parse("warehouse").expect("a source name"), &file).map(drop);
        let _ignored = std::fs::remove_dir_all(&directory);
        let error = refusal.expect_err("a whitespace-only password file is not a credential");
        assert!(error.contains("`sources.warehouse.password_file` is empty"), "{error}");
    }
}
