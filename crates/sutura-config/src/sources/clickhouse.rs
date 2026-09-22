//! The `clickhouse` entry's own keys: which of them this kind is opened with, and which are
//! refused for meaning nothing to it.
//!
//! **Split out of the parent for the reason the parent's module declaration states** - the 1000-line
//! cap is unexemptable under `crates/`, and a fourth kind's key reading does not fit beside the
//! other three. What lives here is exactly this kind's half of
//! [`super::parse_placement`](super::parse_placement); the shared helpers - the foreign-key refusal,
//! the required-key read, the absolute-path read and the remote-plaintext rule - stay in the parent
//! and are reached through `super::`, so there is one copy of each and this file cannot narrow one.

use std::path::PathBuf;

use super::{InvalidSourceRegistry, RawSourceEntry, SourceKind};
use crate::sources::placement::{HostName, SourcePlacement};
use sutura_domain::model::SourceName;

/// Reads a `kind: clickhouse` entry into its placement.
///
/// **Every value the transport needs is declared and nothing else is accepted.** The host and port
/// are the HTTP interface's address, the user and password file are the HTTP Basic credential, and
/// the transport is the channel - the same five declarations a `postgres` entry makes minus the
/// two that mean nothing here.
///
/// # The two keys refused for being absent from the adapter rather than from the file
///
/// `unix_socket` and `database` are in the refused set beside the `files` and `bigquery` keys.
/// Neither is a gap waiting to be filled: `ClickHouse`'s HTTP interface is dialled over TCP, and
/// `sutura_exec_clickhouse::transport::Http` sends no `database` field on the one request it makes,
/// so a `database:` written here would be a key an operator believes is in effect while the
/// deployment reads past it. That is the state this module's own foreign-key rule exists to refuse,
/// and refusing it on this kind too is the rule applied rather than an exception to it.
///
/// # Errors
///
/// A key that belongs to another kind; a missing `host`, `port`, `user`, `password_file` or
/// `transport_mode`; a `host` that is not a usable host; a relative `password_file`; a transport
/// declaration this build cannot use; and a non-loopback host declared `plaintext`.
pub(super) fn parse_placement(
    alias: &SourceName,
    kind: SourceKind,
    entry: &RawSourceEntry<'_>,
    written: impl Fn(Option<&str>) -> bool,
) -> Result<SourcePlacement, InvalidSourceRegistry> {
    super::refuse_foreign_keys(alias, kind, foreign_keys(entry, written))?;
    let host =
        HostName::parse(super::required(alias, kind, "host", entry.host)?).map_err(|cause| InvalidSourceRegistry::Host {
            alias: alias.clone(),
            cause,
        })?;
    let port = entry.port.ok_or_else(|| InvalidSourceRegistry::MissingForKind {
        alias: alias.clone(),
        kind,
        key: "port",
    })?;
    let user = super::required(alias, kind, "user", entry.user)?.to_owned();
    let password_file: PathBuf = super::parse_absolute(
        alias,
        "password_file",
        super::required(alias, kind, "password_file", entry.password_file)?,
    )?;
    // The channel is a declared decision, read from its FLAT keys - the same call the `postgres`
    // arm makes, so a mode that discards a key it would not read is refused identically on both.
    let transport = crate::sources::transport::parse(
        alias,
        super::required(alias, kind, "transport_mode", entry.transport_mode)?,
        entry.transport_anchors,
        entry.client_certificate,
        entry.client_key,
    )
    .map_err(|cause| InvalidSourceRegistry::Transport {
        alias: alias.clone(),
        cause,
    })?;
    // Issue 124's fail-closed rule, through the parent's one copy of it: a password over HTTP Basic
    // and every row would otherwise cross the network in clear text.
    super::refuse_remote_plaintext(alias, &host, &transport)?;
    Ok(SourcePlacement::ClickHouse {
        host,
        port,
        user,
        password_file,
        transport,
    })
}

/// The seven keys a `clickhouse` entry has no use for, paired with whether this entry wrote each.
///
/// The `files` key, the four `bigquery` keys, and the two dialled keys this kind does not read -
/// see this module's own header for why those last two are an absence in the adapter rather than a
/// pending feature.
fn foreign_keys(entry: &RawSourceEntry<'_>, written: impl Fn(Option<&str>) -> bool) -> [(&'static str, bool); 7] {
    [
        ("data_dir", written(entry.data_dir)),
        ("billing_project", written(entry.billing_project)),
        ("dataset", written(entry.dataset)),
        ("credential_file", written(entry.credential_file)),
        ("max_bytes_billed", entry.max_bytes_billed.is_some()),
        ("unix_socket", written(entry.unix_socket)),
        ("database", written(entry.database)),
    ]
}
