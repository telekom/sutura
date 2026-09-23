//! The declared user's password, read from the file a `clickhouse` or `oracle` entry names.
//!
//! **One copy for the kinds whose build lives in one shared module** (`crate::clickhouse`,
//! `crate::oracle`), so the trim, the empty-file refusal and the wording cannot differ between them.
//! The two `postgres` roots still carry their own reads - issue 121's unshared build, not this file's.

use sutura_domain::model::SourceName;

/// The declared user's password, read at BOOT rather than on the first question.
///
/// The same argument `crate::serve::bigquery`'s credential read makes: a password file that is
/// missing, unreadable or empty has to stop the composition, not become a deployment that answers
/// every question with an authentication failure while its startup log says it opened a database.
///
/// Trimmed, because a file written by `echo` carries a newline the server would reject; empty after
/// trimming is refused rather than sent, so a truncated secret file is a refusal naming the key
/// instead of an authentication failure on the first question.
pub(crate) fn read(source: &SourceName, password_file: &std::path::Path) -> Result<sutura_domain::identity::Secret, String> {
    let raw = std::fs::read_to_string(password_file)
        .map_err(|cause| format!("`sources.{source}.password_file` could not be read: {cause}"))?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(format!(
            "`sources.{source}.password_file` is empty, and an empty password is not a credential \
             this deployment can present"
        ));
    }
    Ok(sutura_domain::identity::Secret::new(trimmed))
}

#[cfg(test)]
mod tests {
    use sutura_domain::model::SourceName;

    /// A file holding only whitespace - a truncated secret, or `echo > file` - is refused naming the
    /// key, never trimmed down to an empty password and sent.
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
