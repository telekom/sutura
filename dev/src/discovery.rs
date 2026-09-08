//! The discovery file: the only way to learn where a provisioned service is listening.
//!
//! Ports are allocated rather than derived - published ephemerally, so docker and the operating
//! system pick them and there is no window between a check and a bind for a neighbour to lose a
//! race in. What that costs is exactly one property: a fixed port somebody could memorise between
//! runs. This module is what replaces it, with a value that is correct rather than remembered.
//!
//! # Why the reader has one door
//!
//! A test that reads a constant port works alone, fails in parallel, and **passes review easily** -
//! which is why the mechanism has to be the *only* way to learn an endpoint rather than the
//! encouraged one. So:
//!
//! * [`Endpoint`]'s fields are private and it has no public constructor, no `Default` and no
//!   `parse`. A struct literal for it does not compile, which is asserted by a `compile_fail`
//!   doctest with a compiling twin.
//! * [`Endpoints::discover`] is the only public function that returns an `Endpoints`. It reads the
//!   file this module writes, in this worktree, and there is nothing else to call.
//! * [`publish`] - what provisioning calls - returns the path it wrote and **not** an `Endpoints`,
//!   so even the writer has to go through the reader's door to look at what it published.
//!
//! **The limit, stated with the claim:** the mint inside [`publish`] parses what a container runtime
//! reported. A caller that fabricated that text would get an endpoint it made up - but that is
//! lying about docker's output, which is a different and much louder thing than reading a constant,
//! and no test can do it by accident.
//!
//! # Why the WRITER only ever touches its own entries
//!
//! `github.com/telekom/sutura#317`. This file has two writers - [`publish`] here, and
//! `nix/tier-endpoints.nix` for every nix-native tier - and [`publish`] used to serialise the whole
//! document from the docker services it had just read. So a `dev-up` after `just keycloak-tier`
//! dropped the entry that tier had merged, and the server it named went on running unnamed: a
//! truthful file about half a tier, which this repository treats as worse than a crash.
//!
//! So a provisioner reads and writes **only its own entry**. [`publish`] merges, [`forget`]
//! withdraws what THIS provisioner published and leaves the rest, and the last entry out takes the
//! file with it - byte for byte what `nix/tier-endpoints.nix` does on the other side, because the
//! file's EXISTENCE is what discovery reads as *something is provisioned here*.
//!
//! That needs the document to say which provisioner an entry came from, and **that is why
//! [`Provisioner`] hangs off [`Endpoint`] rather than off [`Endpoints`]**: one field on the
//! document cannot answer the question once two provisioners contribute to it, and a field that
//! answers *nix* for a docker entry is exactly the confident wrong answer being removed. The
//! decision `#317` asked for, as a type rather than a paragraph.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::scope::Scope;

/// The file, inside the worktree's state directory, that provisioning writes and a harness reads.
const FILE: &str = "endpoints.json";

/// The discovery document, as JSON, with the keys neither writer is allowed to interpret still on
/// it. Both writers merge into one file, so a whole-document value is what a merge reads and writes.
type Document = serde_json::Map<String, serde_json::Value>;

/// The address a caller connects to.
///
/// `0.0.0.0` and `::` are BIND addresses and are not connectable on every host, so [`publish`]
/// rewrites either to loopback. A test that got `0.0.0.0` back would fail intermittently and for a
/// reason nobody would look for here.
const LOOPBACK: &str = "127.0.0.1";

/// What brought one service up.
///
/// **A property of the ENTRY, not of the document.** `.sutura-dev/endpoints.json` has two writers -
/// `xtask dev-up` through [`publish`], and `nix/tier-endpoints.nix` for every nix-native tier - and
/// both merge into one file, so *what provisioned this* has as many answers as the file has
/// entries. A single document-level field could only be the last writer's opinion about somebody
/// else's service.
///
/// It is an enum and not a string because the one caller that matters is [`forget`], which asks *is
/// this entry mine to withdraw*. A `&str` comparison there is a decision to destroy another
/// provisioner's state spelled as a typo away from wrong.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Provisioner {
    /// `xtask dev-up`: a compose project, an ephemeral host port read back off docker.
    Docker,
    /// A nix-native tier (`nix/postgres-tier.nix`, `nix/keycloak-tier.nix`), merged by
    /// `nix/tier-endpoints.nix`.
    Nix,
}

impl std::fmt::Display for Provisioner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match *self {
            Self::Docker => DOCKER,
            Self::Nix => NIX,
        })
    }
}

/// How [`Provisioner::Docker`] is spelled in the file. `nix/tier-endpoints.nix` writes [`NIX`].
const DOCKER: &str = "docker";

/// How [`Provisioner::Nix`] is spelled in the file, by `nix/tier-endpoints.nix`.
const NIX: &str = "nix";

/// The provisioner this crate publishes as. Not a parameter: nothing in Rust provisions a nix
/// tier, so a `Provisioner` argument on [`publish`] or [`forget`] would be a way to withdraw
/// somebody else's entry and nothing more.
const OURS: Provisioner = Provisioner::Docker;

/// The written spelling, read back. `None` for anything else, which the reader refuses rather than
/// guesses: an entry whose provisioner cannot be named is one [`forget`] cannot decide about.
fn provisioner_of(named: &str) -> Option<Provisioner> {
    match named {
        DOCKER => Some(Provisioner::Docker),
        NIX => Some(Provisioner::Nix),
        _ => None,
    }
}

/// Where one provisioned service is listening, on this host, right now.
///
/// There is no way to construct one except by reading the discovery file. That is the point:
///
/// ```compile_fail
/// use sutura_dev::discovery::Endpoint;
/// // A constant endpoint is exactly what this type refuses to be: the fields are private, so
/// // there is no literal to write.
/// let _ = Endpoint { host: String::from("127.0.0.1"), port: 5432 };
/// ```
///
/// The compiling twin - the one door, which fails at runtime because nothing is provisioned in a
/// temporary directory, rather than at compile time because the door is missing:
///
/// ```
/// use sutura_dev::discovery::Endpoints;
/// use sutura_dev::scope::Scope;
///
/// let Ok(scope) = Scope::from_root(&std::env::temp_dir()) else { return };
/// assert!(Endpoints::discover(&scope).is_err(), "nothing is provisioned there");
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    /// Host to connect to. Loopback (`127.0.0.1`) for the docker tier, or the unix-socket
    /// directory for the nix tier (`nix/postgres-tier.nix`) - what the driver treats a
    /// `/`-prefixed host as. See [`LOOPBACK`].
    host: String,
    /// The host port docker allocated.
    port: u16,
    /// What brought this service up. Carried per entry because the document has two writers - see
    /// [`Provisioner`].
    provisioner: Provisioner,
}

impl Endpoint {
    /// Host to connect to.
    #[must_use]
    pub fn host(&self) -> &str {
        &self.host
    }

    /// The host port docker allocated for this run.
    #[must_use]
    pub const fn port(&self) -> u16 {
        self.port
    }

    /// What brought THIS service up.
    ///
    /// Per entry, because the document has two writers and they merge into one file. A reader that
    /// wants to know whether it is looking at the docker tier asks the service it is about to
    /// connect to, which is the only question the file can answer once both have contributed.
    #[must_use]
    pub const fn provisioner(&self) -> Provisioner {
        self.provisioner
    }
}

impl std::fmt::Display for Endpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.host, self.port)
    }
}

/// Every endpoint one worktree's provisioning bound, plus the compose project they belong to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoints {
    /// The compose project these came from. Carried so a harness can say which worktree answered.
    project: String,
    /// Service name to endpoint. Ordered, so output and any digest over it are stable.
    services: BTreeMap<String, Endpoint>,
}

impl Endpoints {
    /// Read this worktree's discovery file.
    ///
    /// **The reader's only door.** Nothing else in this crate returns an `Endpoints`.
    pub fn discover(scope: &Scope) -> Result<Self, DiscoveryError> {
        let path = path_for(scope);
        let text = std::fs::read_to_string(&path).map_err(|cause| {
            if cause.kind() == std::io::ErrorKind::NotFound {
                DiscoveryError::NotProvisioned { path: path.clone() }
            } else {
                DiscoveryError::Unreadable {
                    path: path.clone(),
                    cause,
                }
            }
        })?;
        parse(&path, &text)
    }

    /// The compose project these endpoints came from.
    #[must_use]
    pub fn project(&self) -> &str {
        &self.project
    }

    /// Where `service` is listening.
    pub fn endpoint(&self, service: &str) -> Result<&Endpoint, DiscoveryError> {
        self.services.get(service).ok_or_else(|| DiscoveryError::UnknownService {
            service: String::from(service),
            known: self.services.keys().cloned().collect(),
        })
    }

    /// Every service and where it is listening, in name order.
    pub fn services(&self) -> impl Iterator<Item = (&str, &Endpoint)> {
        self.services.iter().map(|(name, endpoint)| (name.as_str(), endpoint))
    }
}

/// Why an endpoint could not be learned, or could not be recorded.
#[derive(Debug)]
pub enum DiscoveryError {
    /// No discovery file. Nothing has provisioned this worktree, or teardown removed it.
    ///
    /// **It names no task, and that is the fix rather than an omission.** This variant carries a
    /// path and nothing else - no service, no worktree - so it cannot ask which venue answers. The
    /// remedy belongs to `provisioned::Absent`, which derives one; why this line used to cite
    /// `just dev-up` too is recorded at `provisioned::Venue::advice`.
    NotProvisioned {
        /// Where the file would have been.
        path: PathBuf,
    },
    /// The file exists and could not be read.
    Unreadable {
        /// The file.
        path: PathBuf,
        /// What the filesystem said.
        cause: std::io::Error,
    },
    /// The file is not the shape this module writes.
    Malformed {
        /// The file.
        path: PathBuf,
        /// Which part, named rather than described, so a caller can branch.
        what: Malformed,
    },
    /// A service nobody provisioned.
    UnknownService {
        /// What was asked for.
        service: String,
        /// What is actually in the file.
        known: Vec<String>,
    },
    /// A published address the container runtime reported that this module cannot read.
    UnreadablePublishedAddress {
        /// The service it was reported for.
        service: String,
        /// What the runtime printed.
        reported: String,
    },
    /// The discovery file could not be written.
    Unwritable {
        /// Where it was going.
        path: PathBuf,
        /// What the filesystem said.
        cause: std::io::Error,
    },
}

/// Which part of the discovery file is wrong. A variant rather than a sentence, because a caller
/// that has to match on prose has no contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Malformed {
    /// Not JSON at all.
    NotJson,
    /// No `project` string.
    NoProject,
    /// No `services` object.
    NoServices,
    /// A service entry without a readable `host` and `port`.
    ServiceEntry,
    /// A service host that is neither loopback nor a `/`-prefixed socket directory.
    ///
    /// The docker tier connects on loopback, the nix tier on a socket path. Anything else would
    /// let a discovery file hand a harness an arbitrary host, so it is refused rather than trusted.
    HostNeitherLoopbackNorSocket,
    /// An entry no provisioner can be attributed to.
    ///
    /// Refused rather than defaulted, and the reason is [`forget`]: a provisioner withdraws its own
    /// entries and leaves every other one alone, so an entry it cannot attribute is one it would
    /// have to guess about - and both guesses are wrong. Leaving it would strand a claim over a
    /// dead server; taking it would delete a live tier's address.
    ServiceProvisioner,
}

impl std::fmt::Display for DiscoveryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            Self::NotProvisioned { ref path } => write!(
                f,
                "no services are provisioned for this worktree ({} is absent)",
                path.display()
            ),
            Self::Unreadable { ref path, .. } => write!(f, "could not read {}", path.display()),
            Self::Malformed { ref path, what } => {
                write!(f, "{} is not a discovery file ({what:?})", path.display())
            }
            Self::UnknownService { ref service, ref known } => {
                write!(f, "`{service}` was not provisioned; this worktree has {}", known.join(", "))
            }
            Self::UnreadablePublishedAddress {
                ref service,
                ref reported,
            } => write!(f, "the runtime reported `{reported}` as {service}'s published address"),
            Self::Unwritable { ref path, .. } => write!(f, "could not write {}", path.display()),
        }
    }
}

impl std::error::Error for DiscoveryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match *self {
            Self::Unreadable { ref cause, .. } | Self::Unwritable { ref cause, .. } => Some(cause),
            Self::NotProvisioned { .. }
            | Self::Malformed { .. }
            | Self::UnknownService { .. }
            | Self::UnreadablePublishedAddress { .. } => None,
        }
    }
}

/// Where this worktree's discovery file lives. Under the worktree, so a neighbour cannot read it.
#[must_use]
pub fn path_for(scope: &Scope) -> PathBuf {
    scope.state_dir().join(FILE)
}

/// Record what provisioning actually bound, and return the path written.
///
/// `reported` pairs a service name with the line a container runtime printed for its published
/// address - `0.0.0.0:32768`, `[::]:32768`, `127.0.0.1:32768`. Parsing it here is what keeps the
/// mint in one place: nothing else in this crate turns a number into an [`Endpoint`].
///
/// It returns the PATH and not an [`Endpoints`], deliberately. Even the writer reads its own work
/// back through [`Endpoints::discover`], so there is exactly one door and no second shape of it.
///
/// **It MERGES.** Every entry it writes is marked [`Provisioner::Docker`] and every other entry in
/// the document is left byte for byte as it was, keys this writer does not understand included -
/// `github.com/telekom/sutura#317`. `project` and `root` are the exception, because they are facts
/// about the worktree rather than about a provisioner and both writers run in one tree.
pub fn publish(scope: &Scope, reported: &[(&str, String)]) -> Result<PathBuf, DiscoveryError> {
    let path = path_for(scope);
    let mut document = read_document(&path)?;
    let mut services = document
        .get("services")
        .and_then(serde_json::Value::as_object)
        .cloned()
        .unwrap_or_default();

    for &(service, ref line) in reported {
        let endpoint = mint(service, line)?;
        services.insert(
            String::from(service),
            serde_json::json!({
                "host": endpoint.host,
                "port": endpoint.port,
                "provisioner": OURS.to_string(),
            }),
        );
    }

    // `project` and `root` are facts about the WORKTREE rather than about a provisioner, so this
    // one is entitled to set them; every other key the document carries is left exactly as it was,
    // because a key this writer does not understand belongs to whoever wrote it.
    document.insert(String::from("project"), serde_json::Value::String(scope.project()));
    document.insert(
        String::from("root"),
        serde_json::Value::String(scope.root().to_string_lossy().into_owned()),
    );
    document.insert(String::from("services"), serde_json::Value::Object(services));

    write_document(&path, &document)?;
    Ok(path)
}

/// Withdraw every entry THIS provisioner published, and remove the file if nothing is left.
///
/// Teardown's half of the contract: endpoints that no longer exist must not be readable, because a
/// stale file is the one way discovery could hand back a wrong answer instead of an error.
///
/// **It used to `remove_file`, and that was the other half of `github.com/telekom/sutura#317`.** A
/// nix-native tier merges its entry into this same document, so removing the file withdrew a claim
/// over a server that was still running - `just dev-down` did it deliberately, and every failing
/// path through `with_endpoints_forgotten` did it by accident. Fail-closed is the right posture
/// about *our* entries and is somebody else's data when applied to theirs.
///
/// The last entry out still takes the file with it, because the file's EXISTENCE is what discovery
/// reads as *something is provisioned here* - the rule `nix/tier-endpoints.nix`'s `withdraw` holds
/// on the other side.
///
/// A document this module cannot read is **refused rather than removed**: it publishes nothing a
/// harness can use either way, and destroying state that cannot be attributed is the failure this
/// function was changed to stop.
pub fn forget(scope: &Scope) -> Result<(), DiscoveryError> {
    let path = path_for(scope);
    if !path.exists() {
        return Ok(());
    }
    // Parsed rather than edited blind, so an entry is attributed before it is withdrawn.
    let surviving: Vec<String> = Endpoints::discover(scope)?
        .services()
        .filter(|&(_name, endpoint)| endpoint.provisioner() != OURS)
        .map(|(name, _endpoint)| String::from(name))
        .collect();

    if surviving.is_empty() {
        return match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(cause) if cause.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(cause) => Err(DiscoveryError::Unwritable { path, cause }),
        };
    }

    let mut document = read_document(&path)?;
    let mut services = document
        .get("services")
        .and_then(serde_json::Value::as_object)
        .cloned()
        .unwrap_or_default();
    services.retain(|name, _entry| surviving.iter().any(|kept| kept == name));
    document.insert(String::from("services"), serde_json::Value::Object(services));
    write_document(&path, &document)
}

/// The document as it stands, or an empty object where there is no file yet.
///
/// It goes through [`parse`] first and throws the result away, deliberately: a merge into a
/// document this module cannot read would write back a shape it did not understand, and the entry
/// that vanished would be the other provisioner's.
fn read_document(path: &Path) -> Result<Document, DiscoveryError> {
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
fn write_document(path: &Path, document: &Document) -> Result<(), DiscoveryError> {
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
    let staged = path.with_extension("json.new");
    std::fs::write(&staged, text).map_err(unwritable)?;
    std::fs::rename(&staged, path).map_err(unwritable)
}

/// The one place a host port becomes an [`Endpoint`].
///
/// A bind address is not a connect address: docker publishes on `0.0.0.0` or `::` by default, and
/// neither is reliably connectable. Both become loopback here rather than at every call site.
fn mint(service: &str, reported: &str) -> Result<Endpoint, DiscoveryError> {
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
fn parse(path: &Path, text: &str) -> Result<Endpoints, DiscoveryError> {
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

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{DiscoveryError, Endpoints, Malformed, Provisioner, mint, parse, publish};
    use crate::scope::Scope;

    /// A real directory to hang a scope off, because `Scope::from_root` canonicalises.
    fn temp_worktree(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sutura-discovery-{}-{tag}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dirs are creatable");
        dir
    }

    #[test]
    fn an_endpoint_is_read_from_the_discovery_file_and_never_from_a_constant() {
        let dir = temp_worktree("roundtrip");
        let scope = Scope::from_root(&dir).expect("the directory exists");

        // Before provisioning there is nothing to read, and that is an error rather than a
        // fallback: a fallback is how a constant gets back in.
        assert!(matches!(
            Endpoints::discover(&scope),
            Err(DiscoveryError::NotProvisioned { .. })
        ));

        publish(&scope, &[("postgres", String::from("0.0.0.0:51234"))]).expect("writes");
        let found = Endpoints::discover(&scope).expect("reads back");
        let endpoint = found.endpoint("postgres").expect("provisioned");

        assert_eq!(endpoint.port(), 51234);
        assert_eq!(found.project(), scope.project());
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn the_harness_exposes_no_way_to_read_a_constant_endpoint() {
        // The test that stops the mechanism eroding one pull request at a time. It reads THIS
        // module's own text, and the limit is worth stating: it can only see doors declared here.
        // What makes a door elsewhere impossible is that `Endpoint`'s fields are private - the
        // `compile_fail` doctest on the type is the other half, and neither alone is the claim.
        //
        // `SURFACE` is the WHOLE public surface of this module, pinned by name. Adding a function
        // is adding a door, and a door has to arrive in a diff that changes this list on purpose -
        // which is the only form of this check a well-meaning pull request cannot walk past.
        // `discover` is the only entry that produces endpoints; the rest read one they were given,
        // name the file, or write it.
        const SURFACE: &[&str] = &[
            "host",
            "port",
            "discover",
            "project",
            "provisioner",
            "endpoint",
            "services",
            "path_for",
            "publish",
            "forget",
        ];

        let source = include_str!("discovery.rs");
        let body = source
            .split("mod tests")
            .next()
            .expect("the module has a body before its tests");

        assert!(
            !body.contains("impl Default for Endpoint"),
            "a `Default` endpoint is a constant with a nicer name"
        );
        assert!(
            !body.contains("pub const fn new") && !body.contains("pub fn parse"),
            "a public mint would be a second door"
        );
        for line in body.lines() {
            let trimmed = line.trim();
            // `pub const fn` is a function and is covered by the surface list below; a `pub const`
            // ITEM is the thing this module exists to prevent.
            let is_const_item = trimmed.starts_with("pub const ") && !trimmed.starts_with("pub const fn ");
            assert!(
                !(is_const_item || trimmed.starts_with("pub static ")),
                "a public constant in this module is the thing it exists to prevent: {trimmed}"
            );
        }

        let mut found: Vec<&str> = body
            .lines()
            .map(str::trim)
            .filter_map(|line| {
                let tail = line.strip_prefix("pub fn ").or_else(|| line.strip_prefix("pub const fn "))?;
                tail.split('(').next()
            })
            .collect();
        found.sort_unstable();
        let mut expected: Vec<&str> = SURFACE.to_vec();
        expected.sort_unstable();
        assert_eq!(found, expected, "the public surface of the discovery module moved");
    }

    #[test]
    fn a_bind_address_becomes_a_connectable_one() {
        // `0.0.0.0` is what docker prints and is not reliably connectable. A test that got it back
        // would fail on some hosts only, for a reason nobody would look for in a discovery file.
        assert_eq!(mint("postgres", "0.0.0.0:32768").expect("minted").host(), "127.0.0.1");
        assert_eq!(mint("postgres", "[::]:32768").expect("minted").host(), "127.0.0.1");
        assert_eq!(mint("postgres", "127.0.0.1:32768").expect("minted").host(), "127.0.0.1");
    }

    #[test]
    fn an_unreadable_published_address_is_refused_rather_than_guessed() {
        for reported in ["", "32768", "0.0.0.0:", "0.0.0.0:not-a-port", "0.0.0.0:0", "0.0.0.0:99999"] {
            assert!(
                matches!(
                    mint("postgres", reported),
                    Err(DiscoveryError::UnreadablePublishedAddress { .. })
                ),
                "`{reported}` was accepted"
            );
        }
    }

    #[test]
    fn a_malformed_discovery_file_names_which_part_is_wrong() {
        let path = Path::new("/somewhere/endpoints.json");
        let cases = [
            ("not json at all", Malformed::NotJson),
            (r#"{"services":{}}"#, Malformed::NoProject),
            (r#"{"project":"p"}"#, Malformed::NoServices),
            (
                r#"{"project":"p","services":{"pg":{"host":"127.0.0.1"}}}"#,
                Malformed::ServiceEntry,
            ),
            (
                r#"{"project":"p","services":{"pg":{"host":"127.0.0.1","port":0}}}"#,
                Malformed::ServiceEntry,
            ),
            (
                // A host that is neither loopback nor a socket path is refused, so a discovery
                // file cannot hand the harness an arbitrary remote host.
                r#"{"project":"p","services":{"pg":{"host":"db.internal","port":5432}}}"#,
                Malformed::HostNeitherLoopbackNorSocket,
            ),
            (
                // An entry no provisioner can be attributed to. `forget` would have to guess
                // whether to withdraw it, and both guesses destroy or strand something.
                r#"{"project":"p","services":{"pg":{"host":"127.0.0.1","port":5432}}}"#,
                Malformed::ServiceProvisioner,
            ),
            (
                // A named provisioner this reader does not know is the same answer, not a
                // default: `Provisioner` is the closed set `forget` decides over.
                r#"{"project":"p","provisioner":"podman","services":{"pg":{"host":"127.0.0.1","port":5432}}}"#,
                Malformed::ServiceProvisioner,
            ),
        ];
        for (text, expected) in cases {
            match parse(path, text) {
                Err(DiscoveryError::Malformed { what, .. }) => assert_eq!(what, expected, "{text}"),
                other => panic!("{text} gave {other:?}"),
            }
        }
    }

    #[test]
    fn a_socket_directory_host_is_kept_as_an_address_the_driver_treats_as_unix() {
        let path = Path::new("/somewhere/endpoints.json");
        let text = r#"{"project":"p","services":{"pg":{"host":"/build/sutura-pg","port":5432,"provisioner":"nix"}}}"#;
        let parsed = parse(path, text).expect("a socket-directory host is valid");
        let endpoint = parsed.endpoint("pg").expect("decodes");
        assert_eq!(endpoint.host(), "/build/sutura-pg");
    }

    #[test]
    fn asking_for_a_service_nobody_provisioned_lists_what_there_is() {
        let dir = temp_worktree("unknown");
        let scope = Scope::from_root(&dir).expect("the directory exists");
        publish(&scope, &[("postgres", String::from("0.0.0.0:51235"))]).expect("writes");
        let found = Endpoints::discover(&scope).expect("reads back");
        match found.endpoint("clickhouse") {
            Err(DiscoveryError::UnknownService { known, .. }) => {
                assert_eq!(known, vec![String::from("postgres")]);
            }
            other => panic!("{other:?}"),
        }
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    /// What `nix/tier-endpoints.nix` leaves in the file when a nix-native tier merges its entry.
    ///
    /// Written as TEXT, byte-shaped like the other writer's `jq` output, because the point of these
    /// two tests is a document this crate did not produce. Minting one through `publish` would
    /// prove the merge preserves something `publish` had just written.
    fn a_nix_tier_has_published(scope: &Scope, service: &str) {
        let path = super::path_for(scope);
        std::fs::create_dir_all(path.parent().expect("the state dir has a parent")).expect("creatable");
        let document = format!(
            r#"{{"project":"sutura","services":{{"{service}":{{"host":"/tmp/sutura-pg-1","port":5432,"provisioner":"nix"}}}}}}"#
        );
        std::fs::write(&path, document).expect("the fixture is writable");
    }

    #[test]
    fn a_second_provisioners_entry_survives_a_publish() {
        // `github.com/telekom/sutura#317`. `publish` serialised the whole document from the docker
        // services it had just read, so a `dev-up` after `just keycloak-tier` dropped the entry
        // that tier had merged - and the server it named went on running unnamed, which is a
        // truthful file about half a tier rather than a failure anybody sees.
        let dir = temp_worktree("merge");
        let scope = Scope::from_root(&dir).expect("the directory exists");
        a_nix_tier_has_published(&scope, "postgres");

        publish(&scope, &[("clickhouse", String::from("0.0.0.0:60660"))]).expect("writes");

        let found = Endpoints::discover(&scope).expect("reads back");
        let neighbour = found.endpoint("postgres").expect("the nix tier's entry survived a dev-up");
        assert_eq!(neighbour.provisioner(), Provisioner::Nix);
        assert_eq!(neighbour.host(), "/tmp/sutura-pg-1", "and survived unaltered");
        let ours = found.endpoint("clickhouse").expect("published");
        assert_eq!(ours.provisioner(), Provisioner::Docker);
        assert_eq!(ours.port(), 60660);
        // The worktree's own facts ARE this writer's to set: both provisioners run in one tree.
        assert_eq!(found.project(), scope.project());
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn teardown_withdraws_this_provisioners_entries_and_leaves_every_other_one() {
        // The other half of #317: `forget` was a `remove_file`, so `dev-down` - and every failing
        // path through `xtask::compose::tier::with_endpoints_forgotten` - withdrew a claim over a
        // nix tier that was still running. Fail-closed is the right posture about OUR entries and
        // is somebody else's data when applied to theirs.
        let dir = temp_worktree("withdraw");
        let scope = Scope::from_root(&dir).expect("the directory exists");
        a_nix_tier_has_published(&scope, "postgres");
        publish(&scope, &[("clickhouse", String::from("0.0.0.0:60661"))]).expect("writes");

        super::forget(&scope).expect("withdraws");

        let found = Endpoints::discover(&scope).expect("the file is still there for the nix tier");
        assert_eq!(
            found.services().map(|(name, _)| name).collect::<Vec<_>>(),
            vec!["postgres"],
            "teardown took an entry it did not publish, or left one it did"
        );

        // And the LAST entry out still takes the file with it, because the file's existence is what
        // discovery reads as "something is provisioned here" - so a nix-only document is untouched
        // by our teardown, and a docker-only one disappears.
        publish(&scope, &[("clickhouse", String::from("0.0.0.0:60662"))]).expect("writes");
        super::forget(&scope).expect("withdraws");
        assert!(super::path_for(&scope).is_file(), "the nix entry kept the file alive");
        std::fs::remove_file(super::path_for(&scope)).expect("the fixture is removable");
        publish(&scope, &[("clickhouse", String::from("0.0.0.0:60663"))]).expect("writes");
        super::forget(&scope).expect("withdraws");
        assert!(matches!(
            Endpoints::discover(&scope),
            Err(DiscoveryError::NotProvisioned { .. })
        ));
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn a_document_from_before_the_per_entry_marker_is_attributed_and_not_stranded() {
        // The migration, held rather than hoped for. A file written by the code this replaces
        // carries ONE document-level `provisioner` and no per-entry one; if that read as
        // unattributable, `forget` would refuse and every `dev-up` in a dev shell that had already
        // published would be wedged by a document it wrote itself.
        let dir = temp_worktree("legacy");
        let scope = Scope::from_root(&dir).expect("the directory exists");
        let path = super::path_for(&scope);
        std::fs::create_dir_all(path.parent().expect("the state dir has a parent")).expect("creatable");
        std::fs::write(
            &path,
            r#"{"project":"p","provisioner":"docker","services":{"clickhouse":{"host":"127.0.0.1","port":60664}}}"#,
        )
        .expect("the fixture is writable");

        let found = Endpoints::discover(&scope).expect("reads back");
        assert_eq!(
            found.endpoint("clickhouse").expect("provisioned").provisioner(),
            Provisioner::Docker,
            "an entry written before the per-entry marker must still be attributable"
        );
        super::forget(&scope).expect("withdraws");
        assert!(matches!(
            Endpoints::discover(&scope),
            Err(DiscoveryError::NotProvisioned { .. })
        ));
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn teardown_makes_a_stale_endpoint_unreadable() {
        let dir = temp_worktree("forget");
        let scope = Scope::from_root(&dir).expect("the directory exists");
        publish(&scope, &[("postgres", String::from("0.0.0.0:51236"))]).expect("writes");
        super::forget(&scope).expect("removes");
        assert!(matches!(
            Endpoints::discover(&scope),
            Err(DiscoveryError::NotProvisioned { .. })
        ));
        // Idempotent: a second teardown is not an error, because a teardown that fails on already
        // being torn down is one people stop running.
        super::forget(&scope).expect("idempotent");
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }
}
