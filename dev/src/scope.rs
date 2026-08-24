//! Per-worktree isolation: the part that has to be right.
//!
//! Several worktrees of this repo are open at once - that is the point of stacked branches -
//! and each needs its own Postgres, `ClickHouse` and an identity provider. Two worktrees sharing a
//! container is the worst outcome available: a test passes because the *other* branch's
//! migration ran, and the failure appears in whichever branch is unlucky.
//!
//! So everything scoped to a worktree derives from one value: a short digest of its absolute
//! path.
//!
//! * ports come from the digest, deterministically, inside a private range
//! * the compose project name comes from the digest, so containers, networks and volumes are
//!   namespaced
//! * state lives under the worktree, never in a shared directory
//!
//! Deterministic rather than "find a free port": a port that moves between runs cannot be put
//! in a config file, a bookmark, or a bug report. If it collides, the override is explicit.

use std::path::Path;

use sha2::Digest as _;

/// Ports live here. Above the registered range, below the ephemeral range Linux hands out for
/// outbound connections, so a scoped port cannot collide with one the kernel assigned.
const RANGE_START: u16 = 21_000;
const RANGE_END: u16 = 31_000;

/// A dev service that gets its own port per worktree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Service {
    /// Name used in the compose file and in output.
    pub(crate) name: &'static str,
    /// Environment variable that overrides the derived port.
    pub(crate) port_env: &'static str,
    /// Distinguishes this service's port from another's within the same worktree.
    pub(crate) salt: &'static str,
}

/// The services a worktree may run. Adding one is a row here; nothing else changes.
///
/// They are declared before they are implemented on purpose: the port scheme has to be right
/// from the start, because changing it later moves every developer's ports at once.
pub(crate) const SERVICES: &[Service] = &[
    Service {
        name: "postgres",
        port_env: "SUTURA_DEV_POSTGRES_PORT",
        salt: "postgres",
    },
    Service {
        name: "clickhouse",
        port_env: "SUTURA_DEV_CLICKHOUSE_PORT",
        salt: "clickhouse",
    },
    Service {
        name: "keycloak",
        port_env: "SUTURA_DEV_KEYCLOAK_PORT",
        salt: "keycloak",
    },
];

/// Everything derived from one worktree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Scope {
    /// Absolute worktree path, as the digest saw it.
    pub(crate) root: String,
    /// Short digest of `root`.
    pub(crate) digest: String,
}

impl Scope {
    /// Derive a scope from a worktree root.
    ///
    /// The path is lowercased before hashing: Windows reaches the same directory through
    /// `C:\` and `c:\`, and two spellings of one worktree must not get two sets of ports.
    #[must_use]
    pub(crate) fn from_root(root: &Path) -> Self {
        let text = root.to_string_lossy().to_lowercase().replace('\\', "/");
        let digest = sha2::Sha256::digest(text.as_bytes());
        let mut short = String::with_capacity(8);
        for byte in digest.iter().take(4) {
            use std::fmt::Write as _;
            if write!(short, "{byte:02x}").is_err() {
                break;
            }
        }
        Self {
            root: text,
            digest: short,
        }
    }

    /// Compose project name. Lowercase alphanumeric and dashes only, which is all docker
    /// compose accepts, and prefixed so a stray container is identifiable as ours.
    #[must_use]
    pub(crate) fn project(&self) -> String {
        format!("sutura-dev-{}", self.digest)
    }

    /// The port for `service`, from its override if set, otherwise derived.
    #[must_use]
    pub(crate) fn port(&self, service: &Service) -> u16 {
        std::env::var(service.port_env)
            .ok()
            .and_then(|v| v.parse::<u16>().ok())
            .unwrap_or_else(|| derive_port(&self.digest, service.salt))
    }
}

/// A port in `[RANGE_START, RANGE_END)` from a digest and a salt.
///
/// Separate from `Scope` so it is testable without a filesystem.
#[must_use]
pub(crate) fn derive_port(digest: &str, salt: &str) -> u16 {
    let mut hasher = sha2::Sha256::new();
    hasher.update(digest.as_bytes());
    hasher.update(b":");
    hasher.update(salt.as_bytes());
    let out = hasher.finalize();

    // Two bytes is enough for a 10,000-wide range and keeps the arithmetic obvious.
    let high = u16::from(out.first().copied().unwrap_or(0));
    let low = u16::from(out.get(1).copied().unwrap_or(0));
    let span = RANGE_END - RANGE_START;
    // `rem_euclid` rather than `%`: the restriction lint wants the intent spelled out, and
    // for unsigned values this is the same operation with a name.
    RANGE_START + high.wrapping_mul(256).wrapping_add(low).rem_euclid(span)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{RANGE_END, RANGE_START, SERVICES, Scope, derive_port};

    #[test]
    fn the_same_worktree_always_gets_the_same_ports() {
        let a = Scope::from_root(Path::new("/home/x/sutura"));
        let b = Scope::from_root(Path::new("/home/x/sutura"));
        assert_eq!(a, b);
        for service in SERVICES {
            assert_eq!(a.port(service), b.port(service));
        }
    }

    #[test]
    fn different_worktrees_get_different_scopes() {
        let a = Scope::from_root(Path::new("/home/x/sutura"));
        let b = Scope::from_root(Path::new("/home/x/sutura-feature"));
        assert_ne!(a.digest, b.digest);
        assert_ne!(a.project(), b.project(), "compose projects must not collide");
    }

    #[test]
    fn case_and_separator_differences_are_the_same_worktree() {
        // Windows reaches one directory by several spellings; two spellings must not mean two
        // sets of containers.
        let a = Scope::from_root(Path::new(r"C:\Users\x\sutura"));
        let b = Scope::from_root(Path::new("c:/Users/x/sutura"));
        assert_eq!(a, b);
    }

    #[test]
    fn services_within_a_worktree_do_not_share_a_port() {
        let scope = Scope::from_root(Path::new("/home/x/sutura"));
        let mut ports: Vec<u16> = SERVICES.iter().map(|s| scope.port(s)).collect();
        let count = ports.len();
        ports.sort_unstable();
        ports.dedup();
        assert_eq!(ports.len(), count, "two services derived the same port");
    }

    #[test]
    fn ports_stay_inside_the_private_range() {
        // Below the ephemeral range, so the kernel cannot have already handed one out.
        for seed in ["0000", "ffff", "1a2b", "dead"] {
            for salt in ["postgres", "clickhouse", "keycloak", "future-service"] {
                let port = derive_port(seed, salt);
                assert!((RANGE_START..RANGE_END).contains(&port), "{port} out of range");
            }
        }
    }

    #[test]
    fn a_compose_project_name_is_valid_for_docker() {
        let scope = Scope::from_root(Path::new("/home/x/sutura"));
        let project = scope.project();
        assert!(
            project
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
            "docker compose rejects anything else: {project}"
        );
        assert!(project.starts_with("sutura-dev-"), "a stray container must be identifiable");
    }
}
