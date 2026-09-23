//! Reading one `sources:` entry's placement keys: what belongs to its kind, what must be written,
//! and what a key that means nothing for that kind is refused as.
//!
//! Split out of `sources` when a fifth kind pushed that file past the line cap - it is the per-entry
//! reader, while the parent keeps the registry, the kinds and the refusal set it draws on.

use std::path::PathBuf;

use sutura_domain::model::SourceName;

use super::placement::{BillingProject, DatasetId, SourcePlacement};
use super::{InvalidSourceRegistry, RawSourceEntry, SourceKind, clickhouse, oracle};

/// Reads the fields that belong to this entry's kind, and refuses the ones that do not.
///
/// **Both directions are refused, and the second one is the reason this is a function rather than two
/// lines at the call site.** A `bigquery` entry with no `billing_project` cannot be served, so it is
/// refused - that direction is obvious. A `files` entry that also carries a `billing_project` is
/// refused too, because the key would otherwise sit in the file doing nothing: an operator who wrote
/// it believes it is in effect, and a deployment that reads past it has a configuration nobody can
/// see. Fail-closed on a key that means nothing is the same argument `deny_unknown_fields` makes one
/// level up, applied to a key that IS known and is known to the wrong kind.
pub(super) fn parse_placement(
    alias: &SourceName,
    kind: SourceKind,
    entry: &RawSourceEntry<'_>,
) -> Result<SourcePlacement, InvalidSourceRegistry> {
    // Read as "was anything meaningful written", so an empty string is the same as an absent key -
    // which is what the rest of this module already does with operator-written text.
    let written = |value: Option<&str>| value.is_some_and(|text| !text.trim().is_empty());
    match kind {
        SourceKind::Files => {
            refuse_foreign_keys(
                alias,
                kind,
                [
                    ("billing_project", written(entry.billing_project)),
                    ("dataset", written(entry.dataset)),
                    ("credential_file", written(entry.credential_file)),
                    ("max_bytes_billed", entry.max_bytes_billed.is_some()),
                ]
                .into_iter()
                .chain(dialled_source_keys(entry, written)),
            )?;
            Ok(SourcePlacement::Files {
                data_dir: parse_data_dir(alias, entry.data_dir)?,
            })
        }
        SourceKind::BigQuery => {
            // The `postgres`-only keys are refused here too, for the same reason as on `files`: a
            // `bigquery` entry carrying `transport_mode: verified` and `transport_anchors` would
            // otherwise load and verify against `ureq`'s compiled-in roots regardless, which is
            // exactly the limit this deployment's own transport declaration would claim to close.
            refuse_foreign_keys(
                alias,
                kind,
                std::iter::once(("data_dir", written(entry.data_dir))).chain(dialled_source_keys(entry, written)),
            )?;
            let billing_project = BillingProject::parse(required(alias, kind, "billing_project", entry.billing_project)?)
                .map_err(|cause| InvalidSourceRegistry::ResourceName {
                    alias: alias.clone(),
                    key: "billing_project",
                    cause,
                })?;
            let dataset = DatasetId::parse(required(alias, kind, "dataset", entry.dataset)?).map_err(|cause| {
                InvalidSourceRegistry::ResourceName {
                    alias: alias.clone(),
                    key: "dataset",
                    cause,
                }
            })?;
            let credential_file = parse_absolute(
                alias,
                "credential_file",
                required(alias, kind, "credential_file", entry.credential_file)?,
            )?;
            // Required, and the refusal is `MissingForKind` like the three keys above it - so an
            // operator who left it out is told the same thing about the same kind rather than being
            // handed a range error about a zero nobody wrote. The RANGE is the adapter's, checked
            // where the source is opened; see `SourcePlacement::BigQuery::max_bytes_billed`.
            let max_bytes_billed = entry.max_bytes_billed.ok_or_else(|| InvalidSourceRegistry::MissingForKind {
                alias: alias.clone(),
                kind,
                key: "max_bytes_billed",
            })?;
            Ok(SourcePlacement::BigQuery {
                billing_project,
                dataset,
                credential_file,
                max_bytes_billed,
            })
        }
        SourceKind::Postgres => {
            // The reference values the kind refuses when a foreign key sits on it. `files` keys are
            // refused on a `postgres` entry too - a directory would be an adapter that reads it - so
            // the set is the union of every key that belongs to a DIFFERENT kind.
            refuse_foreign_keys(
                alias,
                kind,
                [
                    ("data_dir", written(entry.data_dir)),
                    ("billing_project", written(entry.billing_project)),
                    ("dataset", written(entry.dataset)),
                    ("credential_file", written(entry.credential_file)),
                    ("max_bytes_billed", entry.max_bytes_billed.is_some()),
                    ("service_name", written(entry.service_name)),
                ],
            )?;
            // Exactly one of `host` or `unix_socket`. Both, or neither, is a declaration the adapter
            // cannot dial - a source has to say whether it is reached over the network or a socket.
            // Built directly into `PostgresDial` rather than into two `Option` fields: the states a
            // struct of options would still admit - `(None, None)`, `(Some, Some)` - are exactly the
            // two this match refuses, so there is no representable state left for a composition
            // root to re-refuse. See `sources::placement::PostgresDial`.
            let has_host = written(entry.host);
            let has_unix_socket = written(entry.unix_socket);
            let port = entry.port.ok_or_else(|| InvalidSourceRegistry::MissingForKind {
                alias: alias.clone(),
                kind,
                key: "port",
            })?;
            let dial = match (has_host, has_unix_socket) {
                (true, true) => {
                    return Err(InvalidSourceRegistry::KeyNotForKind {
                        alias: alias.clone(),
                        kind,
                        key: "host_and_unix_socket",
                    });
                }
                (false, false) => {
                    return Err(InvalidSourceRegistry::MissingForKind {
                        alias: alias.clone(),
                        kind,
                        key: "host_or_unix_socket",
                    });
                }
                (true, false) => {
                    let text = entry.host.map(str::trim).unwrap_or_default();
                    let host = crate::sources::placement::HostName::parse(text).map_err(|cause| InvalidSourceRegistry::Host {
                        alias: alias.clone(),
                        cause,
                    })?;
                    crate::sources::placement::PostgresDial::Tcp { host, port }
                }
                (false, true) => {
                    let text = entry.unix_socket.map(str::trim).unwrap_or_default();
                    let directory = PathBuf::from(text);
                    if directory.is_relative() {
                        return Err(InvalidSourceRegistry::RelativePath {
                            alias: alias.clone(),
                            key: "unix_socket",
                            path: directory,
                        });
                    }
                    crate::sources::placement::PostgresDial::UnixSocket { directory, port }
                }
            };
            let database = required(alias, kind, "database", entry.database)?.to_owned();
            let user = required(alias, kind, "user", entry.user)?.to_owned();
            let password_file = parse_absolute(
                alias,
                "password_file",
                required(alias, kind, "password_file", entry.password_file)?,
            )?;
            // The channel is a declared decision, read from its FLAT keys. See `crate::sources::transport`.
            let transport = crate::sources::transport::parse(
                alias,
                required(alias, kind, "transport_mode", entry.transport_mode)?,
                entry.transport_anchors,
                entry.client_certificate,
                entry.client_key,
            )
            .map_err(|cause| InvalidSourceRegistry::Transport {
                alias: alias.clone(),
                cause,
            })?;
            // Issue 124's fail-closed rule, and it is why `plaintext` is a word an operator writes:
            // a unix socket or a loopback host may say it, and a host a network can reach may not.
            // Shared with the `clickhouse` arm through [`refuse_remote_plaintext`], which is where
            // the reasoning is; a unix-socket dial reaches no host and so is not asked.
            if let crate::sources::placement::PostgresDial::Tcp { ref host, .. } = dial {
                refuse_remote_plaintext(alias, host, &transport)?;
            }
            // The other direction issue 125 asks for: TLS over a unix socket has no handshake to
            // perform, so a `verified`/`mutual` declaration on that dial is refused HERE, naming both
            // keys, rather than reaching `PostgresWarehouse::connect_secured` and failing at connect
            // time with an error that names neither.
            if transport.anchors().is_some()
                && let crate::sources::placement::PostgresDial::UnixSocket { .. } = dial
            {
                return Err(InvalidSourceRegistry::TlsOverUnixSocket {
                    alias: alias.clone(),
                    mode: transport.describe(),
                });
            }
            Ok(SourcePlacement::Postgres {
                dial,
                database,
                user,
                password_file,
                transport,
            })
        }
        SourceKind::ClickHouse => clickhouse::parse_placement(alias, kind, entry, written),
        SourceKind::Oracle => oracle::parse_placement(alias, kind, entry, written),
    }
}

/// Issue 124's fail-closed rule, for every kind that dials a HOST.
///
/// **One function rather than the same three lines per kind**, because a rule copied per arm is a
/// rule that can be narrowed in one of them - `github.com/telekom/sutura#877`'s lesson about
/// `trust_into`, applied before the second copy exists rather than after it drifted.
/// `anchors().is_none()` IS "no transport security" (both TLS variants name a store), and the
/// refusal names `transport_mode`, which is the key the remedy is written under.
///
/// **The limit, next to the claim:** loopback is decided by `sutura_domain::source::
/// host_is_loopback`, which reads a literal ADDRESS - so `localhost` is not loopback here and a
/// name that resolves to one is refused. That is the fail-closed direction on purpose; this
/// function adds no resolution of its own.
pub(super) fn refuse_remote_plaintext(
    alias: &SourceName,
    host: &crate::sources::placement::HostName,
    transport: &crate::sources::transport::SourceTransport,
) -> Result<(), InvalidSourceRegistry> {
    if transport.anchors().is_none() && !crate::sources::transport::host_is_loopback(host.as_str()) {
        return Err(InvalidSourceRegistry::RemoteWithoutTls {
            alias: alias.clone(),
            host: host.as_str().to_owned(),
        });
    }
    Ok(())
}

/// The eleven keys that mean something only to a source this deployment DIALS - `postgres`,
/// `clickhouse` or `oracle` - paired with whether this entry wrote each one.
///
/// Shared by the `Files` and `BigQuery` foreign-key checks in [`parse_placement`]: a key that means
/// nothing to a kind is refused on that kind, and these eleven mean nothing to either of those two.
///
/// **Not every dialled kind reads all eleven.** Each dialled kind's own list refuses the ones it
/// reads past: `unix_socket`, `database` and `service_name` on `clickhouse`; `service_name` on
/// `postgres`; `unix_socket` and `database` on `oracle`.
pub(super) fn dialled_source_keys(
    entry: &RawSourceEntry<'_>,
    written: impl Fn(Option<&str>) -> bool,
) -> [(&'static str, bool); 11] {
    [
        ("host", written(entry.host)),
        ("unix_socket", written(entry.unix_socket)),
        ("port", entry.port.is_some()),
        ("database", written(entry.database)),
        ("service_name", written(entry.service_name)),
        ("user", written(entry.user)),
        ("password_file", written(entry.password_file)),
        ("transport_mode", written(entry.transport_mode)),
        ("transport_anchors", written(entry.transport_anchors)),
        ("client_certificate", written(entry.client_certificate)),
        ("client_key", written(entry.client_key)),
    ]
}

/// Refuses the first key in `keys` this entry wrote, naming it and the kind it does not belong to.
///
/// `Ok(())` when nothing in `keys` was written - the fail-closed rule `parse_placement`'s own doc
/// states, mechanised in one place for the three kinds that all read the same union.
pub(super) fn refuse_foreign_keys(
    alias: &SourceName,
    kind: SourceKind,
    keys: impl IntoIterator<Item = (&'static str, bool)>,
) -> Result<(), InvalidSourceRegistry> {
    for (key, present) in keys {
        if present {
            return Err(InvalidSourceRegistry::KeyNotForKind {
                alias: alias.clone(),
                kind,
                key,
            });
        }
    }
    Ok(())
}

/// One kind-specific key that has to be there, trimmed, or the refusal that says it is not.
///
/// It returns the TEXT rather than taking the newtype's `parse` as an argument, and that is a
/// deliberate retreat from a tidier shape: the `parse` functions here are generic over
/// `impl AsRef<str>`, so passing one as a `FnOnce(&str)` needs a higher-ranked bound the fn item does
/// not satisfy. Two steps at the call site read better than a `for<'a>` bound whose only job is to
/// make a one-line helper accept a generic function.
pub(super) fn required<'raw>(
    alias: &SourceName,
    kind: SourceKind,
    key: &'static str,
    written: Option<&'raw str>,
) -> Result<&'raw str, InvalidSourceRegistry> {
    written
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .ok_or_else(|| InvalidSourceRegistry::MissingForKind {
            alias: alias.clone(),
            kind,
            key,
        })
}

/// Reads a kind-specific path that has to be absolute, naming the key it refuses.
///
/// Separate from [`parse_data_dir`] rather than shared with it, because the two differ in what an
/// ABSENCE means: `data_dir` has its own refusal for that, and every other path arrives already
/// required by [`required`]. What is shared is the check that matters, and it is one line.
pub(super) fn parse_absolute(alias: &SourceName, key: &'static str, written: &str) -> Result<PathBuf, InvalidSourceRegistry> {
    let path = PathBuf::from(written);
    if path.is_relative() {
        return Err(InvalidSourceRegistry::RelativePath {
            alias: alias.clone(),
            key,
            path,
        });
    }
    Ok(path)
}

/// Reads one entry's file location.
pub(super) fn parse_data_dir(alias: &SourceName, written: Option<&str>) -> Result<PathBuf, InvalidSourceRegistry> {
    let Some(raw) = written.map(str::trim).filter(|value| !value.is_empty()) else {
        return Err(InvalidSourceRegistry::NoDataDirectory { alias: alias.clone() });
    };
    let path = PathBuf::from(raw);
    if path.is_relative() {
        return Err(InvalidSourceRegistry::RelativeDataDirectory {
            alias: alias.clone(),
            path,
        });
    }
    Ok(path)
}
