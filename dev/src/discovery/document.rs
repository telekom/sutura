//! The discovery file's own JSON shape: reading it, writing it back without tearing a concurrent
//! reader, and the one place a runtime-reported address becomes a typed [`super::Endpoint`].
//!
//! Split out of `discovery.rs` at the line cap. Every function here is `pub(super)`: the door
//! stays [`super::Endpoints::discover`] and [`super::publish`], this module is the wire format
//! behind them and reaches no further than its own parent.

use std::collections::BTreeMap;
use std::path::Path;

use super::{DiscoveryError, Document, Endpoint, Endpoints, LOOPBACK, Malformed, OURS, provisioner_of};

/// Lift the `services` object OUT of the document, ready to be merged into and put back.
///
/// `remove` rather than `get(..).cloned()`, which is a clone to escape the borrow checker and the
/// one `AGENTS.md` names: both callers own the map, mutate it and insert it again, so nothing
/// needed a second copy of it. An entry that is not an object is the same case as no entry at all -
/// [`parse`] has already refused every document a reader could reach, so this arm is only for the
/// empty document [`read_document`] returns when there is no file.
pub(super) fn take_services(document: &mut Document) -> Document {
    match document.remove("services") {
        Some(serde_json::Value::Object(services)) => services,
        _ignored => Document::new(),
    }
}

/// The document as it stands, or an empty object where there is no file yet.
///
/// It goes through [`parse`] first and throws the result away, deliberately: a merge into a
/// document this module cannot read would write back a shape it did not understand, and the entry
/// that vanished would be the other provisioner's.
pub(super) fn read_document(path: &Path) -> Result<Document, DiscoveryError> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(cause) if cause.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Document::new());
        }
        Err(cause) => {
            return Err(DiscoveryError::Unreadable {
                path: path.to_path_buf(),
                cause,
            });
        }
    };
    parse(path, &text)?;
    serde_json::from_str(&text).map_err(|_ignored| DiscoveryError::Malformed {
        path: path.to_path_buf(),
        what: Malformed::NotJson,
    })
}

/// Write the document where a reader can be part-way through the old one.
///
/// A temporary file in the same directory and a rename, for the reason `nix/tier-endpoints.nix`
/// gives for its own `mv`: a harness can be reading while a tier is starting, and half a JSON
/// document is a malformed-file error attributed to whatever ran next.
///
/// **The stage path carries the writer**, so the two writers of one file cannot stage over each
/// other: this one writes `endpoints.json.docker.new`, `nix/tier-endpoints.nix` writes
/// `endpoints.json.new`. Sharing it - which is what a plain `.new` did - is one process renaming
/// the other's half-written bytes onto the real file, or renaming a path the other has already
/// renamed away and getting a not-found for it.
///
/// **The limit, stated with the claim: a suffix is not a lock.** Two concurrent runs of THIS writer
/// still share one stage path, and the final rename is last-writer-wins across writers either way,
/// so a `just dev-up` racing a nix tier's `start` can still lose an entry - only now it loses it to
/// a merge that read the file a moment too early rather than to a torn write. `nix/with-tier.sh`
/// is documented as not a lock and this does not make it one.
pub(super) fn write_document(path: &Path, document: &Document) -> Result<(), DiscoveryError> {
    let unwritable = |cause: std::io::Error| DiscoveryError::Unwritable {
        path: path.to_path_buf(),
        cause,
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(unwritable)?;
    }
    let Ok(mut text) = serde_json::to_string_pretty(document) else {
        return Err(unwritable(std::io::Error::other("the discovery document did not serialize")));
    };
    text.push('\n');
    // `{OURS}` and not the literal `docker`: the stage path is owned by the provisioner this
    // crate publishes as, so a rename of that constant renames the file it stages through.
    let staged = path.with_extension(format!("json.{OURS}.new"));
    std::fs::write(&staged, text).map_err(unwritable)?;
    std::fs::rename(&staged, path).map_err(unwritable)
}

/// The one place a host port becomes an [`super::Endpoint`].
///
/// A bind address is not a connect address: docker publishes on `0.0.0.0` or `::` by default, and
/// neither is reliably connectable. Both become loopback here rather than at every call site.
pub(super) fn mint(service: &str, reported: &str) -> Result<Endpoint, DiscoveryError> {
    let trimmed = reported.trim();
    let (host, port) = trimmed
        .rsplit_once(':')
        .ok_or_else(|| DiscoveryError::UnreadablePublishedAddress {
            service: String::from(service),
            reported: String::from(trimmed),
        })?;
    let port: u16 = port.parse().map_err(|_ignored| DiscoveryError::UnreadablePublishedAddress {
        service: String::from(service),
        reported: String::from(trimmed),
    })?;
    if port == 0 {
        return Err(DiscoveryError::UnreadablePublishedAddress {
            service: String::from(service),
            reported: String::from(trimmed),
        });
    }
    let host = match host.trim_matches(['[', ']']) {
        "" | "0.0.0.0" | "::" | "*" => String::from(LOOPBACK),
        connectable => String::from(connectable),
    };
    Ok(Endpoint {
        host,
        port,
        provisioner: OURS,
    })
}

/// The discovery file, read back into the type.
pub(super) fn parse(path: &Path, text: &str) -> Result<Endpoints, DiscoveryError> {
    let bad = |what: Malformed| DiscoveryError::Malformed {
        path: path.to_path_buf(),
        what,
    };

    let document: serde_json::Value = serde_json::from_str(text).map_err(|_ignored| bad(Malformed::NotJson))?;
    let project = document
        .get("project")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| bad(Malformed::NoProject))?;
    // The document-level field this module no longer writes, kept as the fallback for an entry
    // that predates the per-entry one. Both writers always set it - `docker` here, `nix` in
    // `nix/tier-endpoints.nix` - so a file from before `github.com/telekom/sutura#317` attributes
    // correctly and a dev shell mid-transition is not wedged by a document it wrote itself. An
    // entry with NEITHER is refused; see `Malformed::ServiceProvisioner`.
    let document_wide = document.get("provisioner").and_then(serde_json::Value::as_str);
    let entries = document
        .get("services")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| bad(Malformed::NoServices))?;

    let mut services = BTreeMap::new();
    for (name, value) in entries {
        let host = value
            .get("host")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| bad(Malformed::ServiceEntry))?;
        let valid_host = host == "127.0.0.1" || host == "::1" || host.starts_with('/');
        if !valid_host {
            return Err(bad(Malformed::HostNeitherLoopbackNorSocket));
        }
        let port = value
            .get("port")
            .and_then(serde_json::Value::as_u64)
            .and_then(|n| u16::try_from(n).ok())
            .filter(|&n| n != 0)
            .ok_or_else(|| bad(Malformed::ServiceEntry))?;
        let provisioner = value
            .get("provisioner")
            .and_then(serde_json::Value::as_str)
            .or(document_wide)
            .and_then(provisioner_of)
            .ok_or_else(|| bad(Malformed::ServiceProvisioner))?;
        services.insert(
            name.clone(),
            Endpoint {
                host: String::from(host),
                port,
                provisioner,
            },
        );
    }

    Ok(Endpoints {
        project: String::from(project),
        services,
    })
}
