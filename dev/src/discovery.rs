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

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::scope::Scope;

/// The file, inside the worktree's state directory, that provisioning writes and a harness reads.
const FILE: &str = "endpoints.json";

/// The address a caller connects to.
///
/// `0.0.0.0` and `::` are BIND addresses and are not connectable on every host, so [`publish`]
/// rewrites either to loopback. A test that got `0.0.0.0` back would fail intermittently and for a
/// reason nobody would look for here.
const LOOPBACK: &str = "127.0.0.1";

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
    /// What wrote the file - `docker` (`xtask dev-up`) or `nix-sandbox` (the nix check). A fact in
    /// the file rather than an inference, so a reader need not guess docker from the project name.
    provisioner: Option<String>,
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

    /// What provisioned this tier, where the file says.
    ///
    /// `docker` for `xtask dev-up`, `nix-sandbox` for the nix check. `None` when an older file (or
    /// a hand-written one) carried no marker - a reader must not assume docker from the absence.
    #[must_use]
    pub fn provisioner(&self) -> Option<&str> {
        self.provisioner.as_deref()
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
}

impl std::fmt::Display for DiscoveryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            Self::NotProvisioned { ref path } => write!(
                f,
                "no services are provisioned for this worktree ({} is absent) - run `just dev-up`",
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
pub fn publish(scope: &Scope, reported: &[(&str, String)]) -> Result<PathBuf, DiscoveryError> {
    let mut services = serde_json::Map::new();
    for &(service, ref line) in reported {
        let endpoint = mint(service, line)?;
        services.insert(
            String::from(service),
            serde_json::json!({ "host": endpoint.host, "port": endpoint.port }),
        );
    }

    let document = serde_json::json!({
        "project": scope.project(),
        "provisioner": "docker",
        "root": scope.root().to_string_lossy(),
        "services": serde_json::Value::Object(services),
    });

    let path = path_for(scope);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|cause| DiscoveryError::Unwritable {
            path: path.clone(),
            cause,
        })?;
    }
    let Ok(mut text) = serde_json::to_string_pretty(&document) else {
        return Err(DiscoveryError::Unwritable {
            path,
            cause: std::io::Error::other("the discovery document did not serialize"),
        });
    };
    text.push('\n');
    std::fs::write(&path, text).map_err(|cause| DiscoveryError::Unwritable {
        path: path.clone(),
        cause,
    })?;
    Ok(path)
}

/// Remove this worktree's discovery file, if there is one.
///
/// Teardown's half of the contract: endpoints that no longer exist must not be readable, because a
/// stale file is the one way discovery could hand back a wrong answer instead of an error.
pub fn forget(scope: &Scope) -> Result<(), DiscoveryError> {
    let path = path_for(scope);
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(cause) if cause.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(cause) => Err(DiscoveryError::Unwritable { path, cause }),
    }
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
    Ok(Endpoint { host, port })
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
    let provisioner = document
        .get("provisioner")
        .and_then(serde_json::Value::as_str)
        .map(String::from);
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
        services.insert(
            name.clone(),
            Endpoint {
                host: String::from(host),
                port,
            },
        );
    }

    Ok(Endpoints {
        project: String::from(project),
        provisioner,
        services,
    })
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{DiscoveryError, Endpoints, Malformed, mint, parse, publish};
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
            "host", "port", "discover", "project", "provisioner", "endpoint", "services", "path_for", "publish", "forget",
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
            (r#"{"project":"p","services":{"pg":{"host":"127.0.0.1"}}}"#, Malformed::ServiceEntry),
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
        let text = r#"{"project":"p","services":{"pg":{"host":"/build/sutura-pg","port":5432}}}"#;
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
