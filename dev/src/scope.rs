//! Per-worktree isolation: the part that has to be right.
//!
//! Several worktrees of this repo are open at once - that is the point of stacked branches -
//! and each needs its own services. Two worktrees sharing a
//! container is the worst outcome available: a test passes because the *other* branch's
//! migration ran, and the failure appears in whichever branch is unlucky.
//!
//! Every containerised service in the compose tier is scoped to a worktree (Postgres is not here:
//! it is nix-native, provisioned by `nix/postgres-tier.nix` and run by `checks.nextest` and by
//! `just test`, over a unix socket in a short per-worktree directory under `$TMPDIR` - see that
//! module).
//!
//! So everything NAMED is scoped to a worktree, and it all derives from one value: a short digest
//! of the worktree's CANONICAL path.
//!
//! * the compose project name comes from the digest, so containers, networks and volumes are
//!   namespaced
//! * state lives under the worktree, never in a shared directory
//!
//! # Naming is derived; ports are NOT
//!
//! An earlier version of this module derived the published ports from the same digest, and that
//! design is withdrawn. Two defects, and the second is the worse one:
//!
//! * **A hash into a port range cannot guarantee disjoint blocks.** It is a total function from an
//!   unbounded set of paths into a finite set of blocks, so collisions exist by construction. A
//!   corpus of sample paths can only fail to find one, which is not the same claim.
//! * **Check-then-bind is a race.** "Refuse if the port is already bound" leaves the whole window
//!   between the check and docker's bind open to anything else on the host - including the
//!   neighbouring worktree running the same check at the same time. It reads as a guarantee and
//!   delivers a probability.
//!
//! Ports are therefore allocated by the thing that owns them: published ephemerally, and read back
//! after the container is up. See [`crate::discovery`], which is the only way to learn one.
//!
//! **Naming stays derived, because naming has no allocator** - and the asymmetry is the whole
//! reason one of the two moved and the other did not. A hash collision in a NAME is a startup
//! error somebody reads; a hash collision in a PORT is a test that passes against the wrong
//! fixture.

use std::path::{Path, PathBuf};

use sha2::Digest as _;

/// A dev service that gets its own container per worktree.
///
/// No port field, derived or otherwise: what a service publishes on the host is allocated at
/// provision time and read back, so a port here would be a second answer to a question this type
/// is not allowed to answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Service {
    /// Name used in the compose file, in the discovery file and in output.
    name: &'static str,
    /// The port the container listens on. Not a host port: the compose file publishes this one
    /// ephemerally, and `xtask` asks docker which host port it landed on.
    container_port: u16,
    /// The compose profile that turns this service on, or `None` for one that is always started.
    ///
    /// A profile is how "this service is off unless somebody needs it" becomes a mechanism rather
    /// than a paragraph. `keycloak` carries one because nothing in this repository can use an
    /// identity provider yet, and a tier that starts it on every run pays for it on every run.
    profile: Option<&'static str>,
}

impl Service {
    /// Name used in the compose file, in the discovery file and in output.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        self.name
    }

    /// The port INSIDE the container. Provisioning publishes it ephemerally and reads back the
    /// host port docker chose.
    #[must_use]
    pub const fn container_port(&self) -> u16 {
        self.container_port
    }

    /// The compose profile that turns this service on, or `None` for one always started.
    #[must_use]
    pub const fn profile(&self) -> Option<&'static str> {
        self.profile
    }

    /// Is this service started when no profile was asked for?
    #[must_use]
    pub const fn is_default(&self) -> bool {
        self.profile.is_none()
    }
}

/// Every profile any service declares, in declaration order and without repeats.
///
/// Teardown enables all of them, and that is the reason this exists: `docker compose down` only
/// considers services in ACTIVE profiles, so a destroy that forgot one would leave that service's
/// container and named volume behind **while reporting success** - the same silent-success failure
/// the teardown contract below is about. Derived rather than listed, so adding a profile does not
/// need a second edit somewhere else to stay correct.
#[must_use]
pub fn profiles() -> Vec<&'static str> {
    let mut found: Vec<&'static str> = Vec::new();
    for service in SERVICES {
        if let Some(profile) = service.profile
            && !found.contains(&profile)
        {
            found.push(profile);
        }
    }
    found
}

/// The DISCOVERABLE services assigned to one profile, and nothing else.
///
/// **Not [`profiles`], and the difference is the point.** [`crate::scope::profiles`] names every
/// profile any discoverable service declares, so a teardown can activate all of them and leave
/// nothing behind. This selection is narrower still: helper containers that publish no endpoint
/// are deliberately absent from [`SERVICES`], so this is NOT an inventory of every Compose block.
/// The exclusive lifecycle accepts only the self-contained `demo` profile and starts it with
/// `--no-deps`; widening that policy requires a source of truth for those helper containers first.
///
/// Derived from [`SERVICES`] rather than listed, so a service added to a profile joins its
/// lifecycle without a second edit. A name no service declares selects nothing, which the caller
/// refuses rather than treating as an empty success.
#[must_use]
pub fn discoverable_services_of(profile: &str) -> Vec<&'static str> {
    SERVICES
        .iter()
        .filter(|service| service.profile == Some(profile))
        .map(Service::name)
        .collect()
}

/// The services a worktree may run. Adding one is a row here plus a block in
/// `compose.services.yaml` - which is NOT `compose.dev.yaml`, the dev-container wrapper.
///
/// # The teardown contract provisioning inherits
///
/// **Written here because this list is what provisioning reads, and every rule below is a lesson
/// somebody already paid for.** `xtask/src/compose.rs` is what honours them; this is the statement
/// of the rules, and none of them may be cited as an invariant - what enforces each one is named
/// beside it in that module.
///
/// 1. **Destructive cleanup is dry-runnable.** A command that removes containers, networks, volumes
///    or state directories can say what it *would* remove and exit without removing it. The reason is
///    not caution in the abstract: the selection logic is the part that goes wrong, and a dry run is
///    the only way to inspect the selection without living with it. A destroy whose only mode is
///    "do it" is a destroy nobody can review.
///
/// 2. **Eligibility is re-checked at destroy time, under a lock held across the destroy, and what is
///    deliberately spared is reported as its own category.** Deciding a container is stale and then
///    removing it are two moments, and another worktree can start between them - so the check that
///    said "nobody is using this" has to be re-run inside the lock that the removal happens under,
///    not before it. The lock is held for the whole destroy rather than taken per item, because the
///    window is what is being closed.
///
///    And the candidates that survived the re-check are **printed as a category of their own** -
///    "in use, left alone" - never omitted. Silence there is indistinguishable from "there was
///    nothing to consider", which is precisely the case where a reader needs to know the safety
///    mechanism fired. **A spared item is a success of the check and has to read as one.**
///
/// 3. **One function supplies the compose project name to both start and stop, and it supplies it
///    the same way.** [`Scope::project`] is that function. The trap is specific: passing the project
///    by command-line flag alone does NOT populate the variable an override file interpolates, so a
///    compose file that interpolates the project name into a network, a volume or a container name
///    resolves it from an unset variable at destroy time - and the destroy then targets the wrong
///    network, or nothing at all, while reporting success. Whatever start relies on, stop has to be
///    given identically: the flag AND the environment, from one call site.
///
/// # Signalling a process
///
/// **A PID is signalled only if its working directory is under this repository.** "Whatever is
/// listening on a port I expected" is not an identity, and neither is "whatever holds a PID a stale
/// file names": a PID is reused. The check is the process's own working directory, resolved and
/// compared against this repository's root, because that is the one property a colliding stranger
/// cannot accidentally have.
pub const SERVICES: &[Service] = &[
    Service {
        name: "clickhouse",
        container_port: 8123,
        profile: None,
    },
    Service {
        // OFF unless asked for. The reasoning lives beside the service in `compose.services.yaml`:
        // it is only needed for the two-subject test and nothing here can use it yet. It used to be
        // the slowest thing in this tier and no longer is - the `DataHub` stack below is - which
        // changes nothing about the profile, because readiness was never the argument for it.
        name: "keycloak",
        container_port: 8080,
        profile: Some("identity"),
    },
    Service {
        // The metadata platform, and the FIRST row here whose compose block is a stack rather than a
        // container: five containers, of which this is the only one anything discovers. The other
        // four - Kafka, `MySQL`, `OpenSearch` and the migration job - are its stores, nothing in
        // this repository speaks to them, and a row here would promise an endpoint no test wants.
        //
        // Waiting on this one waits on all five, and that is a property of the compose file rather
        // than of this list: `datahub` is healthy only after the migration job COMPLETED
        // SUCCESSFULLY, and that job is gated on all three stores being healthy. So the dependency
        // chain is the readiness gate, and adding the four here would only make provisioning
        // re-derive what it already waits for.
        //
        // OFF unless asked for, and unlike `keycloak` the reason is cost rather than absence: there
        // IS a reader - `sutura-catalog-datahub` - so this is not a service with nothing to test.
        // It is three JVMs and a reindexing migration, where `clickhouse` is one alpine container
        // ready in seconds.
        name: "datahub",
        container_port: 8080,
        profile: Some("datahub"),
    },
    Service {
        // The local chat demo, and the ONE service whose container holds two processes: the
        // `sutura-serve` binary against the example corpus, and the chat client that calls it.
        //
        // OFF unless asked for, and the reason is not cost but SCOPE: it needs a language model -
        // hosted with an operator's key, or local - which no other service in this tier needs, so
        // it can only come up on a machine where somebody has configured one. `just demo` is what
        // asks for the profile; `just dev-up` never does.
        //
        // `container_port` is the CHAT CLIENT's, because that is the endpoint a person opens in a
        // browser. The server the client calls is loopback inside the same container and is never
        // published, so it is deliberately not a row here.
        name: "demo",
        container_port: 8080,
        profile: Some("demo"),
    },
];

/// The directory, under the worktree, where provisioning keeps its state. Gitignored.
///
/// **The DEFAULT answer to "where does this worktree's state live", and the reason it is a
/// directory under the tree rather than a keyed one under `$TMPDIR`:** a path inside the worktree
/// needs no key, because the worktree IS the key. Nothing can collide with it that is not in the
/// same tree. [`Scope::scratch`] is the exception and says why one is needed.
const STATE_DIR: &str = ".sutura-dev";

/// The prefix every keyed directory under the machine-shared temporary root carries.
///
/// Prefixed so a stray directory is identifiable as ours, the way [`Scope::project`] is.
const SCRATCH_PREFIX: &str = "sutura-";

/// Why a worktree root could not become a scope.
#[derive(Debug)]
pub enum ScopeError {
    /// The path could not be canonicalised - it does not exist, or a component is not readable.
    ///
    /// Canonicalisation is not a nicety here: it resolves symlinks and returns the on-disk
    /// spelling, which is what makes one directory reached two ways one worktree rather than two.
    /// A scope over an unresolved path would namespace containers by how somebody typed a path.
    NotResolvable {
        /// The path as given.
        given: PathBuf,
        /// What the filesystem said.
        cause: std::io::Error,
    },
}

impl std::fmt::Display for ScopeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            Self::NotResolvable { ref given, .. } => {
                write!(f, "`{}` could not be resolved to a real directory", given.display())
            }
        }
    }
}

impl std::error::Error for ScopeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match *self {
            Self::NotResolvable { ref cause, .. } => Some(cause),
        }
    }
}

/// Everything derived from one worktree.
///
/// If an instance exists, its root is canonical: [`Scope::from_root`] is the only public
/// constructor and it canonicalises first, so no caller has to wonder which spelling of a path a
/// scope was built from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scope {
    /// Canonical worktree path, as the digest saw it.
    root: PathBuf,
    /// Short digest of `root`.
    digest: String,
}

impl Scope {
    /// Derive a scope from a worktree root.
    ///
    /// The canonical constructor. It resolves the path first - symlinks included - so two spellings
    /// of one directory are one worktree, and two genuinely different directories are two. On a
    /// case-folding filesystem that is what makes a case-only difference one worktree, and on a
    /// case-sensitive one it is what stops two real directories being folded into one. Neither
    /// property comes from lowercasing the string, which an earlier version did and which was wrong
    /// on exactly one of those two platforms.
    pub fn from_root(root: &Path) -> Result<Self, ScopeError> {
        let resolved = std::fs::canonicalize(root).map_err(|cause| ScopeError::NotResolvable {
            given: root.to_path_buf(),
            cause,
        })?;
        Ok(Self::from_canonical(&resolved))
    }

    /// The digest and the naming, over a path the caller has already resolved.
    ///
    /// Separate from [`Scope::from_root`] so the naming is testable without a filesystem, and
    /// `pub(crate)` because a caller that has not canonicalised would get a scope whose whole
    /// guarantee is missing.
    pub(crate) fn from_canonical(root: &Path) -> Self {
        let digest = sha2::Sha256::digest(root.to_string_lossy().as_bytes());
        let mut short = String::with_capacity(8);
        for byte in digest.iter().take(4) {
            use std::fmt::Write as _;
            if write!(short, "{byte:02x}").is_err() {
                break;
            }
        }
        Self {
            root: root.to_path_buf(),
            digest: short,
        }
    }

    /// The canonical worktree root.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Short digest of the canonical root. Printed so a stray container can be traced back.
    #[must_use]
    pub fn digest(&self) -> &str {
        &self.digest
    }

    /// Compose project name. Lowercase alphanumeric and dashes only, which is all docker
    /// compose accepts, and prefixed so a stray container is identifiable as ours.
    ///
    /// **The one function that supplies this name**, to start and to stop alike. Rule 3 of the
    /// teardown contract on [`SERVICES`] is about the two of them agreeing.
    #[must_use]
    pub fn project(&self) -> String {
        format!("sutura-dev-{}", self.digest)
    }

    /// Where provisioning keeps this worktree's state. Under the worktree, never shared.
    #[must_use]
    pub fn state_dir(&self) -> PathBuf {
        self.root.join(STATE_DIR)
    }

    /// This worktree's own subtree of the machine-shared temporary root, for one `purpose`.
    ///
    /// **The one derivation of a keyed path under a shared root**, and the only reason it exists
    /// beside [`Scope::state_dir`] is LENGTH: a unix socket path caps around 100 bytes on macOS, so
    /// a server cannot sit under an arbitrarily deep worktree. Everything that does not have that
    /// constraint belongs under the worktree, where the tree is the key and there is nothing to
    /// derive.
    ///
    /// `purpose` namespaces two writers in one worktree from each other; the digest namespaces two
    /// worktrees from one another. Both are needed and neither substitutes: a purpose alone is the
    /// defect `telekom/sutura#405` collects - `<temp_dir>/sutura-conformance/<table>.csv` carried a
    /// purpose and no key, so a second checkout of this repository was a second WRITER of it.
    ///
    /// **What this does NOT give you.** It is a NAME, not an allocation - the same asymmetry this
    /// module's header states for ports. Two worktrees whose canonical paths collide in four bytes
    /// of SHA-256 get one directory, which is a startup error somebody reads rather than a test
    /// that passes against the wrong fixture. And it makes no directory: a caller creates it, so a
    /// path this returns is not evidence that anything is there.
    #[must_use]
    pub fn scratch(&self, purpose: &str) -> PathBuf {
        std::env::temp_dir().join(format!("{SCRATCH_PREFIX}{}-{purpose}", self.digest))
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{SERVICES, Scope};

    #[test]
    fn the_project_name_is_derived_from_the_worktree_path_and_is_stable() {
        // Stability is what makes teardown scoped: stop has to name what start named.
        let a = Scope::from_canonical(Path::new("/home/x/sutura"));
        let b = Scope::from_canonical(Path::new("/home/x/sutura"));
        assert_eq!(a, b);
        assert_eq!(a.project(), b.project());
        assert!(a.project().starts_with("sutura-dev-"));
    }

    #[test]
    fn two_worktree_paths_produce_different_project_names() {
        let a = Scope::from_canonical(Path::new("/home/x/sutura"));
        let b = Scope::from_canonical(Path::new("/home/x/sutura-feature"));
        assert_ne!(a.digest(), b.digest());
        assert_ne!(a.project(), b.project(), "compose projects must not collide");
    }

    #[test]
    fn a_case_only_path_difference_is_two_strings_and_the_filesystem_decides() {
        // The digest does NOT fold case - an earlier version lowercased the path, which made two
        // real directories on a case-sensitive filesystem one worktree. Case folding, where it
        // happens at all, is the filesystem's answer and arrives through canonicalisation; the
        // test below asks a real directory rather than a string.
        let upper = Scope::from_canonical(Path::new("/home/x/Sutura"));
        let lower = Scope::from_canonical(Path::new("/home/x/sutura"));
        assert_ne!(upper.project(), lower.project());
    }

    #[test]
    fn a_case_only_path_difference_is_one_worktree_where_the_filesystem_folds_case() {
        // Both directions are asserted, because a case-folding decision with only one side tested
        // is half a decision - and which side holds is a property of the host, not of this code.
        let base = std::env::temp_dir().join(format!("sutura-scope-{}", std::process::id()));
        let real = base.join("Worktree");
        std::fs::create_dir_all(&real).expect("temp dirs are creatable");

        let folded = base.join("worktree");
        let host_folds_case = folded.is_dir();

        let canonical = Scope::from_root(&real).expect("the directory exists");
        let reached_the_other_way = Scope::from_root(&folded);

        if host_folds_case {
            let other = reached_the_other_way.expect("a folding filesystem resolves both spellings");
            assert_eq!(
                canonical.project(),
                other.project(),
                "one directory reached two ways must be one worktree"
            );
        } else {
            assert!(
                matches!(reached_the_other_way, Err(super::ScopeError::NotResolvable { .. })),
                "on a case-sensitive filesystem the other spelling is not a directory at all"
            );
        }
        std::fs::remove_dir_all(&base).expect("cleanup");
    }

    #[test]
    fn an_unresolvable_root_is_refused_rather_than_hashed() {
        // A scope over a path that does not exist would namespace containers by a spelling nothing
        // resolved, which is the whole failure canonicalisation removes.
        let missing = std::env::temp_dir().join(format!("sutura-scope-not-here-{}", std::process::id()));
        assert!(matches!(
            Scope::from_root(&missing),
            Err(super::ScopeError::NotResolvable { .. })
        ));
    }

    #[test]
    fn a_compose_project_name_is_valid_for_docker() {
        let scope = Scope::from_canonical(Path::new("/home/x/sutura"));
        let project = scope.project();
        assert!(
            project
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
            "docker compose rejects anything else: {project}"
        );
        assert!(project.starts_with("sutura-dev-"), "a stray container must be identifiable");
    }

    #[test]
    fn a_service_declares_no_host_port() {
        // The shape of the guarantee: there is no field on `Service` a host port could hide in, so
        // nothing can read one from the declaration instead of from discovery.
        let source = include_str!("scope.rs");
        let declaration = source
            .split("pub struct Service {")
            .nth(1)
            .and_then(|tail| tail.split('}').next())
            .expect("the declaration is in this file");
        // The FIELDS, not the prose about them: a doc comment explaining why there is no host port
        // is not a host port, and a check that could not tell the two apart would fail on the
        // explanation of itself.
        let fields: Vec<&str> = declaration
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with("//"))
            .collect();
        for field in &fields {
            assert!(
                !field.contains("host") && !field.contains("published"),
                "`Service` grew a host-port field: {field}"
            );
        }
        assert_eq!(fields.len(), 3, "an unreviewed field on `Service`: {fields:?}");
        for service in SERVICES {
            assert!(service.container_port() > 0, "{} has no container port", service.name());
        }
    }

    #[test]
    fn the_expensive_services_are_off_unless_they_are_asked_for() {
        // The reviewer's question - "for what cases do we use keycloak? can we mock it in CI?" -
        // answered as a mechanism rather than a paragraph, and now answered for TWO services whose
        // reasons are NOT the same. Pinned by value, because the whole point is that a service
        // gaining or losing a profile is a decision somebody reads in a diff.
        //
        //   * `keycloak` is opt-in because nothing here can use an identity provider yet, so a tier
        //     that started it on every run would pay for it on every run and test nothing.
        //   * `datahub` is opt-in because it COSTS - five containers, three of them JVMs, and a
        //     reindexing migration. There IS a reader for it, which is precisely why the argument
        //     had to be restated rather than reused: "nothing reads it" does not apply.
        //   * `demo` is opt-in because it is the only service that needs a LANGUAGE MODEL, hosted
        //     with an operator's key or run locally - so it can only come up where somebody has
        //     configured one, and a `dev-up` that started it would fail for want of a model on
        //     every developer's machine.
        let opt_in: Vec<&str> = SERVICES
            .iter()
            .filter(|service| !service.is_default())
            .map(super::Service::name)
            .collect();
        assert_eq!(opt_in, vec!["keycloak", "datahub", "demo"]);

        // And a CHEAP data source is not behind a profile: that is what an adapter is tested
        // against, so making it opt-in would be the tier failing at its own job. `clickhouse` is
        // named rather than derived, because "every service that is not one of the two above" would
        // make this test agree with whatever the list said.
        let started_by_default: Vec<&str> = SERVICES
            .iter()
            .filter(|service| service.is_default())
            .map(super::Service::name)
            .collect();
        assert_eq!(started_by_default, vec!["clickhouse"]);
    }

    #[test]
    fn a_stack_contributes_one_row_and_not_one_row_per_container() {
        // `datahub` is five containers in `compose.services.yaml` and ONE row here, because the four
        // it depends on publish no port and nothing in this repository speaks to them. A row per
        // container would make provisioning read back a host port for a service that publishes none,
        // which fails the provision over containers that are behaving correctly.
        //
        // What keeps that safe is in the compose file rather than here - GMS is healthy only after
        // the migration job completed and that job is gated on all three stores - so this asserts
        // the SHAPE: one registered name, and no row for a store.
        let names: Vec<&str> = SERVICES.iter().map(super::Service::name).collect();
        assert!(
            names.contains(&"datahub"),
            "the platform is registered under its GMS service name"
        );
        for store in [
            "datahub-kafka",
            "datahub-mysql",
            "datahub-opensearch",
            "datahub-system-update",
        ] {
            assert!(
                !names.contains(&store),
                "`{store}` is infrastructure with no published port - registering it would make \
                 provisioning read back a port that does not exist"
            );
        }
    }

    #[test]
    fn a_profile_selection_is_that_profiles_discoverable_services() {
        // The demo lifecycle's isolation, asserted on the selection rather than by running docker
        // and looking at what survived. `discoverable_services_of` is safe as a lifecycle selector
        // only for the self-contained demo, which is why `--only` accepts that profile alone.
        assert_eq!(super::discoverable_services_of("demo"), vec!["demo"]);
        for unrelated in ["clickhouse", "keycloak", "datahub"] {
            assert!(
                !super::discoverable_services_of("demo").contains(&unrelated),
                "`{unrelated}` is selected by the demo profile's teardown"
            );
        }
        // A name nothing declares selects nothing, which the caller refuses rather than removing
        // nothing and reporting success.
        assert_eq!(super::discoverable_services_of("nonesuch"), Vec::<&str>::new());
    }

    #[test]
    fn every_declared_profile_is_discoverable_without_a_second_list() {
        // Teardown enables every profile, because `docker compose down` only considers services in
        // ACTIVE profiles - so a profile missing from this walk leaves a container and a named
        // volume behind while the destroy reports success. Derived from `SERVICES`, so adding a
        // profile cannot forget to update it.
        assert_eq!(super::profiles(), vec!["identity", "datahub", "demo"]);
        for service in SERVICES {
            if let Some(profile) = service.profile() {
                assert!(
                    super::profiles().contains(&profile),
                    "{} declares `{profile}`, which the walk does not report",
                    service.name()
                );
            }
        }
    }

    #[test]
    fn state_lives_under_the_worktree() {
        let scope = Scope::from_canonical(Path::new("/home/x/sutura"));
        assert!(scope.state_dir().starts_with(scope.root()));
    }

    #[test]
    fn a_scratch_path_under_the_shared_root_carries_this_worktree_s_key() {
        // THE PROPERTY `telekom/sutura#405` IS ABOUT. A path under the machine's temporary root is
        // reachable from every checkout on the machine, so the only thing that makes one this
        // worktree's own is the key in it. `<temp_dir>/sutura-conformance/<table>.csv` carried a
        // purpose and no key, and two worktrees were two writers of it.
        let one = Scope::from_canonical(Path::new("/home/x/sutura"));
        let other = Scope::from_canonical(Path::new("/home/x/sutura-feature"));
        assert!(
            one.scratch("pg").to_string_lossy().contains(one.digest()),
            "{}",
            one.scratch("pg").display()
        );
        assert_ne!(
            one.scratch("pg"),
            other.scratch("pg"),
            "two worktrees asking for one purpose must not get one directory"
        );
        // ONE SHARED ROOT, TWO SUBTREES - asserted as a shared parent rather than by naming the
        // platform's temporary directory here. Acquiring that root in a test would be a taking
        // `cargo xtask check-worktree-state` reports, and it is not needed: what the derivation
        // claims is that two worktrees sit under one root under different names, and that is
        // exactly this pair of assertions.
        assert_eq!(one.scratch("pg").parent(), other.scratch("pg").parent());
    }

    #[test]
    fn two_purposes_in_one_worktree_are_two_directories() {
        // The other half, and it is not symmetry: the digest separates trees, the purpose separates
        // writers. A derivation carrying only the digest would put two writers in one worktree into
        // one directory, which is the same collision one level down.
        let scope = Scope::from_canonical(Path::new("/home/x/sutura"));
        assert_ne!(scope.scratch("pg"), scope.scratch("keycloak"));
    }

    #[test]
    fn a_scratch_path_is_a_name_and_not_a_directory() {
        // Stated as a test because the doc states it as a limit: the derivation creates nothing, so
        // a caller that treated the return value as evidence of a directory would be wrong.
        let scope = Scope::from_canonical(Path::new("/home/x/sutura"));
        assert!(!scope.scratch("nothing-makes-this").exists());
    }
}
