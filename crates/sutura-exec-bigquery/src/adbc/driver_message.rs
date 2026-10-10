//! A driver's error text with the console job link cut out, in its own module so that
//! `DriverMessage::of` is the only way `super` can build one.

use adbc_core::error::{Error as CoreError, Status};

/// A driver message with every console job link cut out, so no rendered error says where a job ran,
/// and the status the driver gave it.
///
/// The pinned driver appends `(Query: <link>)` to the message of any error after the job was
/// created (`go/record_reader.go`'s `runQuery`), and the link names the project, location and job.
///
/// **The limit:** this seal is rustc's ordinary privacy, not this repo's `check-newtype-leaks`
/// gate - private fields in a child module, so a `DriverMessage { .. }` in `super` is `E0451`.
/// Only `driver_message.rs` itself can skip `of`.
#[derive(Debug)]
pub struct DriverMessage {
    text: String,
    status: Status,
}
impl DriverMessage {
    pub(crate) fn of(error: &CoreError) -> Self {
        Self {
            text: unlinked(&error.to_string()),
            status: error.status,
        }
    }

    /// Did the data system refuse the identity this call ran as?
    ///
    /// `Unauthorized` is what the pinned driver's `errToAdbcErr` (`go/util.go`) gives a `403`, a
    /// job's `accessDenied`, a gRPC `PermissionDenied` from the Storage Read API and a `401` from
    /// the token exchange - each refused again on every retry until a grant or the pool changes.
    /// Every other status is a failure a retry may answer.
    pub(crate) const fn refused_the_identity(&self) -> bool {
        matches!(self.status, Status::Unauthorized)
    }
}
impl std::fmt::Display for DriverMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.text)
    }
}
fn unlinked(message: &str) -> String {
    const LINK: &str = "https://console.cloud.google.com/";
    let mut out = String::with_capacity(message.len());
    let mut rest = message;
    while let Some((before, after)) = rest.split_once(LINK) {
        out.push_str(before);
        out.push_str("[a job link]");
        rest = after.trim_start_matches(|c: char| !c.is_whitespace() && c != ')');
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adbc::AdbcError;

    #[test]
    fn a_console_link_never_reaches_a_rendered_driver_error() {
        let e = CoreError::with_message_and_status(
            "division by zero (Query: https://console.cloud.google.com/bigquery?project=acme-billing&j=bq:EU:job_x1&page=queryresults)",
            adbc_core::error::Status::Unknown,
        );
        let err = AdbcError::Adbc(DriverMessage::of(&e));
        let rendered = err.to_string();
        assert!(!rendered.contains("console.cloud.google.com"));
        assert!(!rendered.contains("job_x1"));
        assert!(rendered.contains("division by zero (Query: [a job link])"));
        let mut source = std::error::Error::source(&err);
        while let Some(s) = source {
            assert!(!s.to_string().contains("console.cloud.google.com"));
            source = s.source();
        }
    }

    #[test]
    fn two_links_are_cut_and_a_plain_message_is_kept() {
        assert_eq!(
            unlinked("a https://console.cloud.google.com/x b (https://console.cloud.google.com/y)"),
            "a [a job link] b ([a job link])"
        );
        assert_eq!(unlinked("plain"), "plain");
    }
}
