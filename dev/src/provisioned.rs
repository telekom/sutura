//! The consumption half: how a test, an example or a demo reaches a service this worktree brought up.
//!
//! [`crate::discovery`] is the door that *can* be opened; this module is the one a caller should
//! actually use, and the difference is two things neither a test nor a reader should have to write
//! twice.
//!
//! # 1. The diagnostic, which is most of the value here
//!
//! Without it the failure a developer sees is a connection refused, thirty seconds into a test,
//! attributed to the adapter under test rather than to a tier that was never started. Every path
//! out of [`here`] and [`in_worktree`] carries [`Absent`] instead, which names the worktree, the
//! file it looked in, what the file said, and the task to run. The whole point of allocating a port
//! per worktree is that nobody has to know the port; the cost of that is that nobody can guess it
//! either, so the message has to close the gap.
//!
//! # 2. The skip-or-fail decision, made once
//!
//! [`here`] applies [`crate::requirement`]: a missing tier skips loudly on a developer machine and
//! fails where the tier is required. A test that made that decision for itself would make it
//! differently from the next test, and one of them would make it silently.
//!
//! # What is deliberately not here
//!
//! **No fallback port, at any level.** Not a default, not a "try the container port", not an
//! environment variable a caller could set to a constant. A fallback connects to whatever else
//! holds that port, and on a machine running two worktrees of this repository that is the
//! neighbour's fixture - a test that passes against the wrong data and says nothing about it. That
//! is the exact failure the per-worktree design removes, so re-introducing it as a convenience
//! would remove the design.
//!
//! **No knowledge of docker**, for the same reason the rest of this crate has none: bringing
//! services up is `xtask`'s job. This module reads a file.

use std::path::{Path, PathBuf};

use crate::discovery::{DiscoveryError, Endpoint, Endpoints, path_for};
use crate::requirement::{FORCE, Requirement};
use crate::scope::{Scope, ScopeError};

/// What a harness gets when it asks for a provisioned service.
///
/// Two variants and no third, because the fail direction does not return: see [`here`].
#[derive(Debug)]
pub enum Provisioned {
    /// It is up, and this is where. Read from the discovery file, which is the only place a host
    /// port for this worktree exists.
    At(Endpoint),
    /// Nothing to connect to, on a machine class where that is not a failure.
    ///
    /// **The notice has already been written to stderr** by the time this is returned. A skip a
    /// reader cannot see is a green run that tested nothing, which is worse than a red one.
    Skipped(Absent),
}

impl Provisioned {
    /// The endpoint, where there is one.
    ///
    /// For a caller that wants `let Some(endpoint) = .. else { return }` rather than a match. The
    /// skip has already been reported either way, so discarding the [`Absent`] loses nothing.
    #[must_use]
    pub const fn endpoint(&self) -> Option<&Endpoint> {
        match *self {
            Self::At(ref endpoint) => Some(endpoint),
            Self::Skipped(_) => None,
        }
    }
}

/// Nothing to connect to, and what to do about it.
///
/// The typed fields are the contract and the `Display` form is the message; a caller that wants to
/// branch reads [`Absent::reason`] rather than the prose.
///
/// Plain backticks on `Display` rather than a rustdoc link to `std::fmt::Display`, and that is the
/// rule `AGENTS.md` states rather than a preference: the api-docs generator copies a link to another
/// crate's path through verbatim, and `mkdocs build --strict` then aborts on an unrecognized
/// relative link - which every nix check passes over, because none of them builds the site.
///
/// **Boxed, and it is a lint that says so rather than taste.** The diagnostic is four fields wide
/// and one of them is another error, which puts the whole thing past `result_large_err`: every
/// `Ok(Endpoint)` on the way back would carry room for it. This is the cold path and it can afford
/// one allocation, so the box is here and not at the call sites - a public
/// `Result<Endpoint, Box<Absent>>` would push it onto everybody instead.
#[derive(Debug)]
pub struct Absent(Box<Details>);

/// The fields of an [`Absent`]. Private, so the box is an implementation detail rather than an API.
#[derive(Debug)]
struct Details {
    /// The service that was asked for.
    service: String,
    /// The worktree this looked in, once it knew which one that was.
    worktree: Option<PathBuf>,
    /// The discovery file it looked for, once it could name one.
    discovery: Option<PathBuf>,
    /// What stopped it.
    reason: Reason,
}

/// Which half failed. A variant rather than a sentence, because "run `just dev-up`" is the wrong
/// advice for two of these and a caller matching on prose has no contract.
#[derive(Debug)]
pub enum Reason {
    /// The directory given is not inside a checkout of this repository, so there is no worktree
    /// whose discovery file could be read.
    NoWorktree {
        /// Where the walk upwards started.
        from: PathBuf,
    },
    /// A worktree root that could not become a [`Scope`].
    NoScope(ScopeError),
    /// There is a worktree, and its discovery file does not answer.
    NotDiscovered(DiscoveryError),
}

/// Which venue answers for a service, so a remedy cites THAT venue's task rather than assuming one.
///
/// There are two and they are not interchangeable: the compose tier is the demo a person brings up
/// on their own machine, and a nix-native tier is what `checks.nextest` and `just test` provision
/// where there is no docker socket and no network. `compose.services.yaml` declares which one
/// answers per service, and the gate holding that declaration holds it against the existence of the
/// module - so the MODULE is the ground truth, and reading it here derives the same answer instead
/// of adding a second declaration to keep true.
#[derive(Debug)]
enum Venue {
    /// A nix-native tier. Nothing to enable: it is started, or it is not.
    NixTier {
        /// The module, repo-relative, so the remedy names a file a reader can open.
        module: String,
    },
    /// A compose service behind a profile, which is off unless it was asked for.
    Profile(&'static str),
    /// A compose service with no profile, so a provision that ran would have started it.
    ComposeDefault,
    /// Nothing in this worktree declares it under either venue.
    Undeclared,
}

/// The venue that answers for `service`, as far as this worktree can be asked.
///
/// **The tier wins over the compose row, and that order is the fix rather than an accident.** A
/// service that moved to a tier KEEPS its compose block, because the block is the demo - so a
/// lookup that stopped at the row would hand a reader the venue that is no longer the one CI and
/// `just test` run, which is the same wrong-venue advice the comment in [`Absent::remedy`] records.
///
/// `None` for the worktree is the case where the walk upwards found no root, so no module can be
/// looked for; the compose row is still readable, because it is compiled in.
fn venue(worktree: Option<&Path>, service: &str) -> Venue {
    let module = format!("nix/{service}-tier.nix");
    if worktree.is_some_and(|root| root.join(&module).is_file()) {
        return Venue::NixTier { module };
    }
    crate::scope::SERVICES
        .iter()
        .find(|declared| declared.name() == service)
        .map_or(Venue::Undeclared, |declared| {
            declared.profile().map_or(Venue::ComposeDefault, Venue::Profile)
        })
}

impl Absent {
    /// The one place an [`Absent`] is built, so every path carries the same four fields.
    fn new(service: &str, worktree: Option<PathBuf>, discovery: Option<PathBuf>, reason: Reason) -> Self {
        Self(Box::new(Details {
            service: String::from(service),
            worktree,
            discovery,
            reason,
        }))
    }

    /// The service that was asked for.
    #[must_use]
    pub fn service(&self) -> &str {
        &self.0.service
    }

    /// What stopped it. Branch on this, never on the message.
    #[must_use]
    pub const fn reason(&self) -> &Reason {
        &self.0.reason
    }

    /// What to run, chosen from the reason rather than printed unconditionally.
    ///
    /// A message that says "run `just dev-up`" when the tier is already up and the service is
    /// behind a profile sends somebody to re-run something that will not help.
    fn remedy(&self) -> Vec<String> {
        match self.0.reason {
            Reason::NoWorktree { .. } => vec![String::from(
                "run        this from inside a checkout of this repository - the walk upwards found no root",
            )],
            Reason::NoScope(_) => vec![String::from(
                "run        nothing yet - the worktree root did not resolve, which is a filesystem problem",
            )],
            Reason::NotDiscovered(ref problem) => match *problem {
                DiscoveryError::NotProvisioned { .. } => vec![
                    String::from("run        `just dev-up` - it starts this worktree's services and writes that file"),
                    format!(
                        "             `just dev-endpoint {}` then prints where it landed",
                        self.0.service
                    ),
                ],
                DiscoveryError::UnknownService { .. } => vec![
                    String::from("run        `just dev-endpoints` to see what this worktree actually has, and"),
                    // BOTH HALVES ARE DERIVED, and each of them was wrong once.
                    //
                    // The profile used to be named - `just dev-up-identity`, the only one there was
                    // - so the day a second arrived it became advice that starts the wrong stack: a
                    // reader whose missing service was `datahub` was told to bring up Keycloak. The
                    // replacement listed every profile and cited `xtask dev-up --with <profile>`,
                    // which is a raw command line where all four sibling remedies here cite a `just`
                    // task - the one form `AGENTS.md` says a reader is never told to type, enforced
                    // by `check-guidance` over prose and therefore by nothing over a string literal.
                    // It also assumed the compose venue, which stops being true for a service the
                    // day it gets a nix-native tier.
                    //
                    // So [`venue`] keeps the venue true and
                    // `every_command_this_remedy_names_is_a_just_task_that_exists` keeps the task
                    // name true. A remedy nothing checks is prose, and prose rots silently.
                    match venue(self.0.worktree.as_deref(), &self.0.service) {
                        Venue::NixTier { ref module } => format!(
                            "             `just test` provisions it - `{}` is a nix-native tier ({module}) and not a \
                             compose profile, so there is no profile to enable",
                            self.0.service
                        ),
                        Venue::Profile(profile) => format!(
                            "             `just dev-up-{profile}` - `{}` is behind the `{profile}` compose profile, \
                             which is off unless it is asked for",
                            self.0.service
                        ),
                        Venue::ComposeDefault => format!(
                            "             `just dev-up` - `{}` is a compose service with no profile, so a provision \
                             that finished would have started it",
                            self.0.service
                        ),
                        Venue::Undeclared => format!(
                            "             nothing declares `{}` - a service is a row in `compose.services.yaml` or a \
                             `nix/<service>-tier.nix` module, and this worktree has neither for it",
                            self.0.service
                        ),
                    },
                ],
                DiscoveryError::Unreadable { .. }
                | DiscoveryError::Malformed { .. }
                | DiscoveryError::UnreadablePublishedAddress { .. }
                | DiscoveryError::Unwritable { .. } => vec![String::from(
                    "run        `just dev-down` then `just dev-up` - the discovery file is not readable as one",
                )],
            },
        }
    }
}

impl std::fmt::Display for Absent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "`{}` is not reachable: nothing provisioned answers for it", self.0.service)?;
        if let Some(ref worktree) = self.0.worktree {
            writeln!(f, "  worktree   {}", worktree.display())?;
        }
        if let Some(ref discovery) = self.0.discovery {
            writeln!(f, "  discovery  {}", discovery.display())?;
        }
        writeln!(f, "  because    {}", self.0.reason)?;
        for line in self.remedy() {
            writeln!(f, "  {line}")?;
        }
        write!(
            f,
            "  There is no default port to fall back to, deliberately. A fallback would connect to\n  \
             whatever else holds that port, and on a machine running two worktrees of this repository\n  \
             that is the neighbour's fixture - a test passing against the wrong data and saying nothing."
        )
    }
}

impl std::fmt::Display for Reason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            Self::NoWorktree { ref from } => {
                write!(f, "no worktree root above {}", from.display())
            }
            Self::NoScope(ref problem) => write!(f, "{problem}"),
            Self::NotDiscovered(ref problem) => write!(f, "{problem}"),
        }
    }
}

impl std::error::Error for Absent {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self.0.reason {
            Reason::NoWorktree { .. } => None,
            Reason::NoScope(ref problem) => Some(problem),
            Reason::NotDiscovered(ref problem) => Some(problem),
        }
    }
}

/// Where one service in ONE named worktree is listening.
///
/// The half with no environment and no printing in it, so a caller that already knows which
/// worktree it means - a `just` task, a test over a fixture directory - can drive it directly.
///
/// ```
/// use sutura_dev::provisioned;
///
/// // A temporary directory is a real directory and nothing has provisioned it, so this is the
/// // diagnostic path rather than an endpoint. Note what it is NOT: a default port.
/// let problem = provisioned::in_worktree(&std::env::temp_dir(), "postgres")
///     .expect_err("nothing is provisioned in a temporary directory");
/// assert_eq!(problem.service(), "postgres");
/// assert!(problem.to_string().contains("just dev-up"), "{problem}");
/// ```
pub fn in_worktree(root: &Path, service: &str) -> Result<Endpoint, Absent> {
    let scope = Scope::from_root(root)
        .map_err(|problem| Absent::new(service, Some(root.to_path_buf()), None, Reason::NoScope(problem)))?;
    let absent = |problem: DiscoveryError| {
        Absent::new(
            service,
            Some(scope.root().to_path_buf()),
            Some(path_for(&scope)),
            Reason::NotDiscovered(problem),
        )
    };
    let endpoints = Endpoints::discover(&scope).map_err(absent)?;
    endpoints.endpoint(service).cloned().map_err(absent)
}

/// Where one service in THIS worktree is listening, with the skip-or-fail decision applied.
///
/// `inside` is any directory in the worktree; an integration test passes
/// `Path::new(env!("CARGO_MANIFEST_DIR"))`, which is the one thing a test reliably knows about
/// where it is. The worktree root is found by walking upwards - see [`worktree_root`].
///
/// # Panics
///
/// In the [`Requirement::Required`] direction, and only there. The caller is a test, a panic is how
/// a test fails, and returning [`Provisioned::Skipped`] there would be the silent green run this
/// whole tier exists to prevent. On a developer machine the direction is
/// [`Requirement::Optional`], the notice goes to stderr, and nothing panics.
#[expect(
    clippy::panic,
    reason = "the required direction has to END the run: a harness that returned Skipped where the \
              tier is mandatory would report green having connected to nothing, which is the one \
              outcome this module exists to prevent. A test is the caller and a panic is how a test \
              fails."
)]
pub fn here(inside: &Path, service: &str) -> Provisioned {
    let found = worktree_root(inside)
        .ok_or_else(|| {
            Absent::new(
                service,
                None,
                None,
                Reason::NoWorktree {
                    from: inside.to_path_buf(),
                },
            )
        })
        .and_then(|root| in_worktree(&root, service));

    let absent = match found {
        Ok(endpoint) => return Provisioned::At(endpoint),
        Err(absent) => absent,
    };
    match Requirement::from_env() {
        Requirement::Required => panic!("{}", required(&absent)),
        Requirement::Optional => {
            eprintln!("{}", skipped(&absent));
            Provisioned::Skipped(absent)
        }
    }
}

/// The skip notice. Written where a reader will look for it, and it says what did NOT run.
///
/// "SKIPPED" in the first column on purpose: it is what `xtask dev-up` prints in the same
/// situation, so one grep finds both halves of the tier declining to run.
fn skipped(absent: &Absent) -> String {
    format!(
        "SKIPPED - {absent}\n  \
         Nothing was tested against a real service. This skips on a developer machine and FAILS\n  \
         where the tier is required - set {FORCE}=1 to get that direction here."
    )
}

/// The failure. Same diagnostic, opposite verdict, and it says which way the flag points.
fn required(absent: &Absent) -> String {
    format!(
        "{absent}\n  \
         This run requires a provisioned service tier, so an absent one is a failure rather than a\n  \
         skip: a green run that connected to nothing certifies an adapter nothing exercised. Set\n  \
         {FORCE}=0 to skip instead, and mean it."
    )
}

/// The worktree root at or above `inside`, or `None` if there is not one.
///
/// **Both markers, not either**, and this is borrowed from `xtask`'s own root walk because the same
/// two mistakes are available: `flake.nix` alone appears in unrelated directories, and `Cargo.toml`
/// alone matches every crate on the way up - which would stop the walk at a workspace MEMBER and
/// derive a scope for a directory no provisioning ever used.
///
/// A walk rather than `git rev-parse`, deliberately. A harness runs where a `.git` directory may not
/// be - a nix sandbox copies the tree without one - and shelling out to git from a test is a
/// subprocess in the way of an assertion.
#[must_use]
pub fn worktree_root(inside: &Path) -> Option<PathBuf> {
    let mut dir = std::fs::canonicalize(inside).ok()?;
    loop {
        if dir.join("flake.nix").is_file() && dir.join("Cargo.toml").is_file() {
            return Some(dir);
        }
        if !dir.pop() {
            return None;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::path::{Path, PathBuf};

    use super::{Absent, Reason, in_worktree, worktree_root};
    use crate::discovery::{DiscoveryError, Malformed, publish};
    use crate::scope::{SERVICES, Scope, ScopeError, Service};

    /// A real directory to hang a scope off, because `Scope::from_root` canonicalises.
    fn temp_worktree(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sutura-provisioned-{}-{tag}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dirs are creatable");
        dir
    }

    /// A service every default provision brings up, read from the declaration rather than typed.
    fn a_default_service() -> &'static Service {
        SERVICES
            .iter()
            .find(|service| service.is_default())
            .expect("the tier declares at least one default service")
    }

    #[test]
    fn the_harness_returns_the_port_this_worktree_published_and_not_the_containers_own() {
        // THE assertion this module exists for. A harness that read the declaration would get the
        // container's port - 5432 for postgres - and on a machine with one instance listening
        // there it would pass by accident. The published port is a different number, and it is the
        // one a connection has to use.
        let service = a_default_service();
        let dir = temp_worktree("published");
        let scope = Scope::from_root(&dir).expect("the directory exists");

        // A port docker could plausibly have allocated, and deliberately not the container's own.
        let published = 47_213_u16;
        assert_ne!(
            published,
            service.container_port(),
            "the fixture has to differ to prove anything"
        );
        publish(&scope, &[(service.name(), format!("0.0.0.0:{published}"))]).expect("writes");

        let found = in_worktree(&dir, service.name()).expect("provisioned");
        assert_eq!(found.port(), published, "the harness did not read the published port");
        assert_ne!(
            found.port(),
            service.container_port(),
            "the harness handed back the constant a test might have hardcoded"
        );
        assert_eq!(found.host(), "127.0.0.1", "a bind address is not a connect address");
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn an_unprovisioned_worktree_names_the_file_and_the_task_rather_than_failing_to_connect() {
        // The diagnostic IS the deliverable: without it the symptom is a connection refused inside
        // whatever test was running, attributed to the code under test.
        let dir = temp_worktree("diagnostic");
        let problem = in_worktree(&dir, "postgres").expect_err("nothing is provisioned there");

        assert!(matches!(
            *problem.reason(),
            Reason::NotDiscovered(DiscoveryError::NotProvisioned { .. })
        ));
        let message = problem.to_string();
        assert!(message.contains("endpoints.json"), "{message}");
        assert!(message.contains("just dev-up"), "{message}");
        assert!(message.contains("postgres"), "{message}");
        assert!(message.contains("no default port"), "{message}");
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn a_service_this_worktree_did_not_start_gets_the_task_for_its_own_profile() {
        // Telling somebody to run `just dev-up` when the tier is already up and the service is
        // behind a profile sends them to re-run something that cannot help.
        let dir = temp_worktree("profile");
        let scope = Scope::from_root(&dir).expect("the directory exists");
        let started = a_default_service();
        publish(&scope, &[(started.name(), String::from("0.0.0.0:47214"))]).expect("writes");

        let profiled = SERVICES
            .iter()
            .find(|service| service.profile().is_some())
            .expect("the tier declares a profiled service");
        let profile = profiled.profile().expect("that is what it was found by");
        let problem = in_worktree(&dir, profiled.name()).expect_err("it was not started");

        assert!(matches!(
            *problem.reason(),
            Reason::NotDiscovered(DiscoveryError::UnknownService { .. })
        ));
        let message = problem.to_string();
        assert!(!message.contains("`just dev-up` - it starts"), "wrong advice: {message}");
        // THE PROFILE THE MISSING SERVICE DECLARES, and the task that enables it. Two wrong shapes
        // came first, in order: `just dev-up-identity` unconditionally, which was correct while
        // `identity` was the only profile and became advice that starts the wrong stack the day a
        // second arrived - a reader whose missing service was `datahub` was told to bring up
        // Keycloak; then a derived list of EVERY profile, which is never wrong and never says which
        // one to type. The service knows its own profile, so the advice is one task.
        assert!(message.contains(&format!("`just dev-up-{profile}`")), "{message}");
        for other in super::super::scope::profiles() {
            assert!(
                other == profile || !message.contains(&format!("dev-up-{other}")),
                "`{other}` is another service's profile and this advice names it: {message}"
            );
        }
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    /// Every recipe name in the justfile - what `just --summary` prints, read from the file so an
    /// assertion needs no subprocess. A recipe header is an unindented line carrying a `:` and no
    /// `:=`, and its name is the first word: the same hand-parse `xtask`'s own justfile gate uses,
    /// for the reason that gate records - the shape being read is a file this repository controls.
    fn recipes(justfile: &str) -> BTreeSet<&str> {
        justfile
            .lines()
            .filter(|line| !line.starts_with([' ', '\t', '#']) && !line.contains(":="))
            .filter_map(|line| line.split_once(':'))
            .filter_map(|(head, _)| head.split_whitespace().next())
            .map(|name| name.trim_start_matches('@'))
            .filter(|name| !name.is_empty())
            .collect()
    }

    /// Every backtick span in one line. Fields 1, 3, 5 ... of a backtick split are the spans and the
    /// bound drops an unterminated trailing backtick - the walk both citation checkers in `xtask`
    /// use, so this reads a remedy the way the prose gates read a page.
    fn spans(line: &str) -> Vec<&str> {
        let parts: Vec<&str> = line.split('`').collect();
        // `take` before `skip` is what drops an unterminated trailing backtick: the last field of
        // the split follows no closing one, so it is not a span.
        let closed = parts.len().saturating_sub(1);
        parts.iter().take(closed).skip(1).step_by(2).copied().collect()
    }

    /// A label per reason, with NO wildcard arm - so a new variant fails to compile HERE rather than
    /// escaping the scan below with its remedy unread.
    fn label(reason: &Reason) -> &'static str {
        match *reason {
            Reason::NoWorktree { .. } => "NoWorktree",
            Reason::NoScope(ScopeError::NotResolvable { .. }) => "NoScope",
            Reason::NotDiscovered(ref problem) => match *problem {
                DiscoveryError::NotProvisioned { .. } => "NotProvisioned",
                DiscoveryError::Unreadable { .. } => "Unreadable",
                DiscoveryError::Malformed { .. } => "Malformed",
                DiscoveryError::UnknownService { .. } => "UnknownService",
                DiscoveryError::UnreadablePublishedAddress { .. } => "UnreadablePublishedAddress",
                DiscoveryError::Unwritable { .. } => "Unwritable",
            },
        }
    }

    /// One of each reason, constructed rather than provoked: a remedy is chosen from the reason
    /// alone, so this reaches every arm without making a filesystem fail in six ways.
    fn every_reason(service: &str) -> Vec<Reason> {
        let path = PathBuf::from("endpoints.json");
        let refused = || std::io::Error::from(std::io::ErrorKind::PermissionDenied);
        vec![
            Reason::NoWorktree {
                from: PathBuf::from("/nowhere"),
            },
            Reason::NoScope(ScopeError::NotResolvable {
                given: PathBuf::from("/nowhere"),
                cause: refused(),
            }),
            Reason::NotDiscovered(DiscoveryError::NotProvisioned { path: path.clone() }),
            Reason::NotDiscovered(DiscoveryError::Unreadable {
                path: path.clone(),
                cause: refused(),
            }),
            Reason::NotDiscovered(DiscoveryError::Malformed {
                path: path.clone(),
                what: Malformed::NotJson,
            }),
            Reason::NotDiscovered(DiscoveryError::UnknownService {
                service: String::from(service),
                known: Vec::new(),
            }),
            Reason::NotDiscovered(DiscoveryError::UnreadablePublishedAddress {
                service: String::from(service),
                reported: String::from("not an address"),
            }),
            Reason::NotDiscovered(DiscoveryError::Unwritable { path, cause: refused() }),
        ]
    }

    #[test]
    fn every_command_this_remedy_names_is_a_just_task_that_exists() {
        // THE MECHANISM for `AGENTS.md`'s "cite a `just` task, never a raw command line", held where
        // a reader is actually TOLD rather than where a reader is documented to. `check-guidance`
        // filters to prose - `.md`, `.nix`, `.yml`, `.yaml`, `.toml`, `.sh` - and the justfile gate
        // reads the justfile, so a citation inside a Rust string literal is outside every citation
        // checker in this repository. This remedy said `xtask dev-up --with <profile>` while its
        // four siblings cited `just` tasks, and nothing was red.
        //
        // Two halves, and the second is the one that fires on a rename: a multi-word backtick span
        // in a remedy IS an invocation, so it has to begin `just `; and the task it names has to be
        // a recipe that exists.
        //
        // WHAT IT DOES NOT REACH: a single-word span, deliberately. `` `datahub` ``, `` `identity` ``
        // and `` `endpoints.json` `` are single-word spans these messages need, so a bare
        // `` `docker` `` would not be read as an invocation. Nor does it reach any other module -
        // this is one function's output and nothing else.
        let root = worktree_root(Path::new(env!("CARGO_MANIFEST_DIR"))).expect("this crate is inside a checkout");
        let justfile = std::fs::read_to_string(root.join("justfile")).expect("the justfile is readable");
        let tasks = recipes(&justfile);
        for expected in ["test", "dev-up", "dev-endpoint", "dev-endpoints", "dev-down"] {
            assert!(
                tasks.contains(expected),
                "the recipe scan is broken rather than the justfile: no `{expected}` among {tasks:?}"
            );
        }

        // Every service a remedy can be asked about, under BOTH venues, because the venue is what
        // decides the task. A row in `SERVICES` is the compose side; a `nix/<name>-tier.nix` is the
        // other one - and Postgres is ONLY ever the second, so a scan over `SERVICES` alone would
        // miss the service this harness is asked for most.
        let mut services: BTreeSet<String> = SERVICES.iter().map(|service| String::from(service.name())).collect();
        for entry in std::fs::read_dir(root.join("nix")).expect("nix/ is readable") {
            let name = entry.expect("a readable directory entry").file_name();
            if let Some(service) = name.to_string_lossy().strip_suffix("-tier.nix") {
                services.insert(String::from(service));
            }
        }
        assert!(
            services.len() > SERVICES.len(),
            "this scan found {services:?} and no nix tier, so it would not cover the second venue"
        );

        let mut reasons: BTreeSet<&str> = BTreeSet::new();
        let mut cited: usize = 0;
        for service in &services {
            for reason in every_reason(service) {
                reasons.insert(label(&reason));
                let absent = Absent::new(service, Some(root.clone()), None, reason);
                for line in absent.remedy() {
                    for span in spans(&line) {
                        if !span.contains(' ') {
                            continue;
                        }
                        assert!(
                            span.starts_with("just "),
                            "`{span}` is a command line in the remedy for `{service}`, and this \
                             repository cites a `just` task instead: {line}"
                        );
                        let name = span.split_whitespace().nth(1).unwrap_or_default();
                        assert!(
                            tasks.contains(name),
                            "`just {name}` is cited in the remedy for `{service}` and names no recipe \
                             - it was renamed, deleted, or never existed: {line}"
                        );
                        cited = cited.saturating_add(1);
                    }
                }
            }
        }

        // FAIL CLOSED in the two directions a text scan fails silently: a reason whose remedy this
        // never rendered, and a scan that found no citation at all and would therefore pass a remedy
        // that cited nothing.
        assert_eq!(
            reasons,
            BTreeSet::from([
                "Malformed",
                "NoScope",
                "NoWorktree",
                "NotProvisioned",
                "UnknownService",
                "Unreadable",
                "UnreadablePublishedAddress",
                "Unwritable",
            ]),
            "a reason was added to this module and not to `every_reason`, so its remedy is unscanned"
        );
        assert!(
            cited >= services.len(),
            "the scan found {cited} citation(s) across {} service(s): it is broken, not clean",
            services.len()
        );
    }

    #[test]
    fn a_service_whose_venue_is_a_nix_tier_is_not_told_to_enable_a_compose_profile() {
        // The staleness that arrives with the second nix-native tier. A service that moves from a
        // compose profile to a tier KEEPS its compose block - the block is the demo - so a remedy
        // reading the profile row would tell a reader to enable a profile when what is absent is a
        // TIER NOBODY STARTED. Same class as the failure `remedy`'s own comment records: advice
        // naming the wrong venue reads as correct.
        //
        // The fixture supplies the module, so the branch is proven against THIS worktree's state
        // rather than waiting for a second tier to exist.
        let profiled = SERVICES
            .iter()
            .find(|service| service.profile().is_some())
            .expect("the tier declares a profiled service");
        let profile = profiled.profile().expect("that is what it was found by");
        let dir = temp_worktree("venue");
        std::fs::create_dir_all(dir.join("nix")).expect("temp dirs are creatable");
        let module = format!("nix/{}-tier.nix", profiled.name());
        std::fs::write(dir.join(&module), "# a tier module, as far as this test is concerned\n").expect("writes");

        let scope = Scope::from_root(&dir).expect("the directory exists");
        publish(&scope, &[(a_default_service().name(), String::from("0.0.0.0:47215"))]).expect("writes");
        let problem = in_worktree(&dir, profiled.name()).expect_err("the tier was not started");
        let message = problem.to_string();
        std::fs::remove_dir_all(&dir).expect("cleanup");

        assert!(message.contains("nix-native tier"), "{message}");
        assert!(message.contains(&module), "the advice does not name the module: {message}");
        assert!(message.contains("`just test`"), "the advice names no task: {message}");
        // The venue that is no longer this service's, in both spellings this message ever had.
        assert!(
            !message.contains("dev-up"),
            "a tier is not brought up by the compose tier: {message}"
        );
        assert!(
            !message.contains(&format!("`{profile}` compose profile")),
            "the profile row won over the tier: {message}"
        );
    }

    #[test]
    fn the_harness_has_no_fallback_port_to_offer() {
        // The mechanism that stops this eroding one pull request at a time. `Endpoint`'s private
        // fields are what make a constant unconstructible; this reads THIS module for the softer
        // failure - a helper that "helpfully" reached for the declaration when discovery came up
        // empty. Neither half is the claim alone.
        let source = include_str!("provisioned.rs");
        let body = source
            .split("mod tests")
            .next()
            .expect("the module has a body before its tests");

        assert!(
            !body.contains("container_port"),
            "this module reached for the container's own port, which is the fallback it must not have"
        );
        for line in body.lines() {
            let trimmed = line.trim();
            let is_const_item = trimmed.starts_with("pub const ") && !trimmed.starts_with("pub const fn ");
            assert!(
                !(is_const_item || trimmed.starts_with("pub static ")),
                "a public constant here is how a port gets memorised again: {trimmed}"
            );
        }
    }

    #[test]
    fn the_walk_upwards_stops_at_the_workspace_and_not_at_a_member() {
        // Both markers matter: `Cargo.toml` alone would stop at `dev/`, and a scope derived from
        // `dev/` is a different digest from the one provisioning used - so every lookup would come
        // back NotProvisioned while the tier was up.
        let here = worktree_root(std::path::Path::new(env!("CARGO_MANIFEST_DIR"))).expect("this crate is inside a checkout");
        assert!(here.join("flake.nix").is_file(), "{}", here.display());
        assert!(here.join("compose.services.yaml").is_file(), "{}", here.display());
        assert!(
            !here.ends_with("dev"),
            "the walk stopped at the crate rather than the workspace: {}",
            here.display()
        );
    }

    #[test]
    fn a_directory_outside_any_checkout_says_so_rather_than_reporting_nothing_provisioned() {
        // Distinguished on purpose: "run `just dev-up`" is useless advice to somebody whose harness
        // is not inside the repository at all.
        assert!(
            worktree_root(std::path::Path::new("/")).is_none(),
            "the filesystem root is not a worktree of this repository"
        );
    }
}
