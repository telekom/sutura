//! Everything `tests/served.rs` needs to spawn a deployment, and not one assertion.
//!
//! **A mechanical split, and it moved the HARNESS rather than any test.** `served.rs` reached the
//! 1000-line cap `cargo xtask max-lines` holds, and the direction of the move is not a style choice:
//! `just causality` reverts a changed file that added no `#[test]` and keeps one that did, so moving
//! *tests* out of `served.rs` would turn it into a revertible file, take this module's declaration
//! with it, and leave the base tree compiling with none of the new tests - which the gate would then
//! report as *green against base behaviour*. Nothing here carries an assertion about the deployment,
//! so nothing here changes what that gate can see.
//!
//! What lives here: the settings builders, the spawn-and-wait pair and its refusing sibling, the two
//! guards that reap a process, the one-connection HTTP client, the log readers, the example's own
//! question fixtures, and the mock issuer's names. What lives in `served.rs`: every `#[test]`.
//!
//! **`#[cfg(test)]` is on this module's own declaration** in `served.rs` and not only on its parent,
//! because clippy looks for a literal `#[cfg(test)]` on an ancestor module to decide whether
//! `allow-expect-in-tests` applies - without it every `expect` below is a lint error under
//! `-D warnings`. That is the same measurement `served.rs` records for spelling its two `cfg`s as two
//! attributes rather than one `all(..)`.

use core::fmt::Write as _;
use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

// `Environment` rather than a string, because it is the type that owns the spelling: `as_str` is
// documented as the canonical name AND the file stem this environment layers, so a case that says
// which deployment environment it is about cannot say it in a word `sutura_config` would refuse.
use sutura_config::Environment;
// `PublishedKeySet` is deliberately absent: the tests own the key set's lifetime, because which
// document is published - and whether it is rotated to an unusable one - is what a case is about.
use sutura_dev::issuer::{MockIssuer, Token};

/// The deployment's own bearer token, which authenticates the DEPLOYMENT and not a caller.
///
/// Thirty-five characters, because `sutura_config::AccessToken::MIN_LENGTH` is thirty-two and a
/// fixture shorter than that would be refused at startup rather than at the gate.
pub(crate) const TOKEN: &str = "e2e-access-token-000000000000000000";

/// The version label the served bundle is stamped with.
///
/// Fixed rather than read from the working tree, for the reason the CLI example gives: the
/// digest does not include it, and a version that moved between runs would churn every
/// assertion that carries provenance.
pub(crate) const VERSION: &str = "serve-e2e";

/// How long a start may take before the test gives up and prints the log it has.
///
/// Generous on purpose. Startup loads the catalog and RE-EXECUTES every anchor against the
/// engine, so this is the cost of validating a bundle rather than of binding a socket, and a
/// shared CI runner is slower than a laptop at both.
pub(crate) const START_BUDGET: Duration = Duration::from_secs(120);

/// How long an audit record may lag the response it describes.
///
/// It lags at all because the record is written from the blocking pool while the response is
/// written by the asking task, so the two are not ordered - see [`Served::awaiting`]. Generous
/// rather than tight: the failure this bounds is a record that never comes, and a shared runner
/// scheduling a pool thread late must not read as one.
pub(crate) const RECORD_BUDGET: Duration = Duration::from_secs(30);

/// How long a `SIGTERM` may take to become an exited process.
///
/// Longer than the `shutdown_grace_seconds` the embedded defaults ship, because the bound is on
/// the grace period and the process still has to unwind and exit after spending it.
pub(crate) const STOP_BUDGET: Duration = Duration::from_secs(45);

/// The example deployment, which is also what a reader is told to run.
///
/// Canonicalised because `sutura_config` refuses a relative `data_dir` - a relative path
/// resolves against whatever working directory the supervisor chose - and because a path
/// carrying `..` in a settings file is harder to read in a failure than the real one.
pub(crate) fn example_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/single-player")
        .canonicalize()
        .expect("the example directory is in this repository")
}

/// The settings file the deployment is started from.
///
/// Everything not written here is the embedded default, which is the point: this is a
/// deployment's own `base.yaml` and not a second copy of the configuration tree. Four things are
/// said, and each of them is what makes one of the tests below possible.
///
/// * `server.port: 0` - the kernel picks, so two checks in one sandbox cannot collide.
/// * `security.access_token` - so the bearer gate is armed and its refusal is reachable.
/// * `telemetry.format: bunyan` - so the harness can read the bound address out of the log.
/// * `catalogs:` and `sources:` over the example directory - the one catalog and the one data
///   system a reader of the quickstart has.
pub(crate) fn settings(example: &Path) -> String {
    settings_crediting(example, &format!("  access_token: \"{TOKEN}\"\n"))
}

/// The same deployment with **leg 1** declared instead of a deployment token.
///
/// Two things are different from [`settings`] and both are load-bearing. The `inbound` block is
/// what makes the composition root read a key set before the listener opens; and
/// `security.access_token` is **gone**, because `mode: direct` reads the caller's own token out of
/// the same `authorization: Bearer` header - a deployment declaring both is refused at startup as
/// `DeploymentTokenSharesTheHeader`, so the two credentials cannot be configured together.
///
/// **`identity` stays `single-user`, and that is the limit rather than an oversight.** Leg 1 says
/// who is asking; it does not make a source execute as them. This deployment reads its fixture
/// files under one identity whoever asks, which is why the answer assertion below pins
/// `executed_as` to `shared-service-user`.
///
/// Built from the issuer's own names rather than from constants, for the reason
/// `sutura_http`'s equivalent helper gives: a deployment configured with the issuer under test's
/// `iss` and its audience cannot verify against an issuer it was not configured with.
pub(crate) fn settings_declaring_inbound(example: &Path, issuer: &MockIssuer, key_set: &Path) -> String {
    settings_crediting(
        example,
        &format!(
            "  inbound:\n    mode: \"direct\"\n    resource: \"{}\"\n    \
             authorization_server: \"{}\"\n    key_set_file: \"{}\"\n    algorithms: [\"ES256\"]\n",
            issuer.audience(),
            issuer.issuer(),
            key_set.display(),
        ),
    )
}

/// The deployment above and below the one thing that changes: what a request presents.
///
/// One function over both shapes rather than two copies of the catalog and the source, so a
/// fixture path that moves moves once. `credential` is the rest of the `security:` block, indented
/// for it.
pub(crate) fn settings_crediting(example: &Path, credential: &str) -> String {
    deployment(example, LOOPBACK, &format!("{SINGLE_USER}{credential}"))
}

/// The bind every deployment that is meant to SERVE uses: loopback, and the kernel picks the port.
///
/// A constant rather than a literal inside [`settings_over`], because a startup-refusal case changes
/// exactly this block and the reader has to be able to see that the serving cases do not.
pub(crate) const LOOPBACK: &str = "  host: \"127.0.0.1\"\n  port: 0\n";

/// The mode declaration every servable deployment in this file makes, as the head of `security:`.
///
/// Split out for the same reason as [`LOOPBACK`]: `security.identity` has no default and a
/// deployment that omits it with a source configured does not start, so the case that omits it is
/// the case that leaves this string out.
pub(crate) const SINGLE_USER: &str = "  identity: \"single-user\"\n  \
     single_user_because: \"an end-to-end test reads its own fixture files as one identity\"\n";

/// The example deployment with the two groups a **startup refusal** turns on written by the caller.
///
/// `server` and `security` are the BODIES of their own groups, indented for them; the catalog and the
/// one source are the example's own. Everything a refusal case wants to change is in those two
/// groups, which is what makes a refusal attributable to the lines the case changed rather than to a
/// second fixture that drifted.
///
/// **A refusing deployment still gets the catalog and the source, and that is deliberate.** Three of
/// the four postures `served.rs` refuses are only reachable on a deployment that could otherwise
/// serve - `security.identity` is checked *because* a source is configured - so a fixture stripped
/// down to the failing key would be a different deployment from the one an operator has.
pub(crate) fn deployment(example: &Path, server: &str, security: &str) -> String {
    let data = example.join("data");
    settings_over(
        &example.join("catalog"),
        &data,
        server,
        security,
        &files_source(LOCAL_SOURCE, &data),
    )
}

/// The one data system every other deployment in this file declares.
pub(crate) const LOCAL_SOURCE: &str = "local";

/// The SECOND data system, which only the two-source deployment declares.
pub(crate) const LOOKUP_SOURCE: &str = "geo";

/// One `sources:` entry, indented for the block.
///
/// A helper rather than a second format string, so a `files` entry's shape is written once and the
/// two-source deployment cannot declare its second source differently from its first - which is the
/// asymmetry that would make a two-source failure read as a federation defect.
pub(crate) fn files_source(name: &str, data: &Path) -> String {
    format!(
        "  {name}:\n    \
           kind: \"files\"\n    \
           data_dir: \"{}\"\n    \
           posture: \"shared-service-user\"\n",
        data.display()
    )
}

/// Every deployment in this file, above the three things that vary: the server group, the security
/// group and the sources.
///
/// Extracted when the two-source case arrived, because that case needs a DERIVED catalog directory
/// and a second `sources:` entry - and a second copy of the server, security and telemetry blocks
/// would have been a settings file that could drift from the one every other test starts. The
/// startup-refusal cases widened it by two: `server` and `security` are whole group BODIES rather
/// than a credential line, because a refusal is a combination of settings and three of the four this
/// file provokes live in one of those two groups.
pub(crate) fn settings_over(catalog: &Path, data: &Path, server: &str, security: &str, sources: &str) -> String {
    format!(
        "server:\n\
         {server}\
         security:\n\
         {security}\
         telemetry:\n  \
           format: \"bunyan\"\n\
         catalogs:\n  \
           - name: \"model\"\n    \
             kind: \"markdown\"\n    \
             dir: \"{}\"\n    \
             data_dir: \"{}\"\n    \
             version: \"{VERSION}\"\n\
         sources:\n\
         {sources}",
        catalog.display(),
        data.display(),
    )
}

/// **The two-source deployment: the example's own catalog with ONE line rewritten, and two `files`
/// entries over the one data directory.**
///
/// The rewrite puts the `customers` model on [`LOOKUP_SOURCE`], which is the whole difference - it
/// is the same one-line derivation `crates/sutura-app/tests/differential/federated.rs` makes, and
/// it is derived rather than committed for that file's reason: a second-source topology is one
/// deployment's, not something a single-source quickstart can state.
///
/// **The two sources share a data directory, and the isolation is real anyway.** `open_files` builds
/// one adapter per declared `files` entry and attaches only that entry's models' tables, so neither
/// engine has the other's table registered and a join across them has to happen above the port or
/// not at all. What is under test is that behaviour of the composition root, so pointing both at
/// one directory removes a variable rather than adding one - two directories would differ in what
/// was copied as well as in what was attached.
///
/// A `Rewrite` that found nothing PANICS: a derivation that silently stopped applying would leave
/// this deployment single-source and the test below green over a whole-plan answer.
pub(crate) fn settings_spanning_two_sources(case: &str) -> String {
    let example = example_root();
    let data = example.join("data");
    let catalog = derived_catalog(case, &example.join("catalog"));
    let sources = format!("{}{}", files_source(LOCAL_SOURCE, &data), files_source(LOOKUP_SOURCE, &data));
    settings_over(
        &catalog,
        &data,
        LOOPBACK,
        &format!("{SINGLE_USER}  access_token: \"{TOKEN}\"\n"),
        &sources,
    )
}

/// The example catalog, copied, with the dimension model moved to the second data system.
///
/// Copied rather than edited in place for the obvious reason and one less obvious: this suite runs
/// beside every other gate in one checkout, so a test that rewrote a committed document would
/// change what a concurrent run reads.
pub(crate) fn derived_catalog(case: &str, from: &Path) -> PathBuf {
    let root = derived_beside(&config_path(case));
    drop(std::fs::remove_dir_all(&root));
    copied(from, &root);
    let model = root.join("models").join("customers.md");
    let text = std::fs::read_to_string(&model).expect("the derived catalog carries the dimension model");
    let moved = text.replace(&format!("source: {LOCAL_SOURCE}"), &format!("source: {LOOKUP_SOURCE}"));
    assert_ne!(
        moved,
        text,
        "{} no longer declares `source: {LOCAL_SOURCE}`, so this deployment is not two-source and \
         the question below would be answered whole",
        model.display()
    );
    std::fs::write(&model, moved).expect("the derived model document is writable");
    root
}

/// One directory tree, copied.
///
/// `std::fs` has no recursive copy and this suite has no dev-dependency that does; the catalog is
/// two levels of markdown, so a six-line walk is cheaper than a crate.
fn copied(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("the derived catalog directory is creatable");
    let entries = std::fs::read_dir(from).unwrap_or_else(|cause| panic!("{} is not readable: {cause}", from.display()));
    for entry in entries {
        let entry = entry.expect("a directory entry is readable");
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copied(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).expect("a catalog document is copyable");
        }
    }
}

/// The one question this file asserts a two-source number for.
///
/// `recurring_revenue` is read off the metric's own model and `region` off the dimension model the
/// derivation moved, so this question spans both data systems by construction. The fixture is READ
/// for [`question`]'s reason, and `region` is checked in it too - a fixture that stopped grouping by
/// a remote attribute would leave this suite asking a single-source question.
pub(crate) fn recurring_revenue_by_region() -> String {
    let stem = "recurring-revenue-by-region";
    let path = example_root().join("questions").join(format!("{stem}.yaml"));
    let fixture = std::fs::read_to_string(&path)
        .unwrap_or_else(|cause| panic!("{} is the example question this body mirrors: {cause}", path.display()));
    for expected in ["recurring_revenue", GRAIN, "2026-06-01", "2026-07-01", "region"] {
        assert!(
            fixture.contains(expected),
            "{} no longer mentions `{expected}`, so this suite asks something the example does \
             not:\n{fixture}",
            path.display()
        );
    }
    String::from(
        r#"{"metric":"recurring_revenue","grain":"month",
        "range":{"start":"2026-06-01","end":"2026-07-01"},"dimensions":["region"]}"#,
    )
}

/// The command, with this shell's own `SUTURA_*` variables removed.
///
/// **Not cosmetic.** `sutura_config` layers one environment variable per key on top of the
/// files, so a developer with `SUTURA__SERVER__PORT` exported would be running a different
/// deployment from CI and the failure would name a setting nobody wrote in this file.
///
/// `environment` is a PARAMETER because three of this file's refusals are the same configuration in
/// two different deployments: `security.access_token` is optional on a development laptop and a
/// refusal in production, and a case that could not say which one it is about would be asserting a
/// posture over the wrong deployment. Its spelling comes from [`Environment::as_str`], which is the
/// same function `sutura_config` parses back - so a case cannot name an environment the binary
/// would reject.
pub(crate) fn command(config_dir: &Path, environment: Environment) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_sutura-serve"));
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("SUTURA") {
            command.env_remove(key);
        }
    }
    command
        .env("SUTURA_ENVIRONMENT", environment.as_str())
        .env("SUTURA_CONFIG_DIR", config_dir)
        .env_remove("RUST_LOG")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

/// One HTTP response, as much of it as an assertion needs.
pub(crate) struct Reply {
    pub(crate) status: u16,
    pub(crate) body: String,
    /// The `www-authenticate` challenge, for a refusal that carries one.
    ///
    /// Read here rather than in one test because it is the only header any assertion in this file
    /// needs, and because what it must NOT contain is the interesting half: a challenge that said
    /// which check failed would tell a caller holding a forgery which half of it to fix.
    pub(crate) challenge: Option<String>,
}

impl Reply {
    /// The body as JSON, or a failure naming what came back instead.
    pub(crate) fn json(&self) -> serde_json::Value {
        serde_json::from_str(&self.body)
            .unwrap_or_else(|cause| panic!("the {} body is not JSON ({cause}): {}", self.status, self.body))
    }
}

/// A running deployment: the process, the address it reported, and its log.
pub(crate) struct Served {
    child: Child,
    address: String,
    /// Every line the process wrote up to and including `listening`.
    pub(crate) startup: Vec<String>,
    /// Everything it writes afterwards.
    lines: Receiver<String>,
    /// The two threads feeding `lines`, kept so [`Served::terminate`] can join them.
    ///
    /// Emptied by that join, which is also what makes a second `terminate` a no-op here.
    readers: Vec<JoinHandle<()>>,
    config_dir: PathBuf,
    reaped: bool,
}

/// Reads one stream line by line into the shared channel.
///
/// Both streams go to ONE channel, and that is what stops the test deadlocking: a piped stream
/// nobody drains fills its buffer and blocks the writer, and the writer here is the process
/// under test. Relative order within a stream survives, which is all the ordering assertion
/// below needs - both lines it compares are written by the subscriber, to standard output, from
/// one thread.
pub(crate) fn forward<Stream>(stream: Stream, into: &Sender<String>)
where
    Stream: std::io::Read,
{
    for line in BufReader::new(stream).lines() {
        let Ok(line) = line else { return };
        if into.send(line).is_err() {
            return;
        }
    }
}

/// Waits until every stream reader has returned.
///
/// **This is the only thing that means "both readers have seen end-of-file", and a comment saying so
/// was what this file had instead** - `github.com/telekom/sutura#387`. A reaped child says the
/// process exited; it says nothing about whether the threads reading its pipes have pushed the last
/// bytes into the channel, and the last thing a refusing deployment writes is the refusal.
/// [`forward`] returns only at end-of-file or on a closed channel, so a join is exactly the
/// statement that nothing is in flight.
///
/// The panic a reader carried is discarded rather than resumed: a reader only panics on something
/// this harness did wrong, and the caller is on its way to an assertion that prints what it read.
///
/// **Unbounded, and that is the limit worth stating rather than hiding.** A reader returns at
/// end-of-file, and a pipe reaches it when every writer is closed - so this is bounded by the reaped
/// child being the only one, which it is because `sutura-serve` spawns no subprocess. A deployment
/// that did fork one would hang here instead of losing a line, and no budget on this path would say
/// so. Losing the line is the failure that was actually happening; a hang is at least loud.
pub(crate) fn joined(readers: Vec<JoinHandle<()>>) {
    for reader in readers {
        drop(reader.join());
    }
}

/// Everything a finished process wrote, collected once its readers have been joined.
///
/// The pair is one function because the order is the whole property: `try_recv` is non-blocking and
/// stops at the first empty channel, so draining BEFORE the join returns the log minus whatever was
/// still in flight - and a test asserting on a refusal's own sentence then fails as *the deployment
/// did not refuse*, which is the one diagnosis nobody should be given wrongly.
pub(crate) fn drained(readers: Vec<JoinHandle<()>>, lines: &Receiver<String>) -> Vec<String> {
    joined(readers);
    let mut said = Vec::new();
    while let Ok(line) = lines.try_recv() {
        said.push(line);
    }
    said
}

/// A fresh configuration directory holding `settings` as this deployment's own `base.yaml`.
///
/// One place that decides where a case's settings live, because two functions spawn the binary
/// now - the one that expects a listener and the one that expects a refusal - and a case whose
/// directory was named differently by each would be a confusing failure rather than a wrong one.
pub(crate) fn written(case: &str, settings: &str) -> PathBuf {
    let config_dir = config_path(case);
    drop(std::fs::remove_dir_all(&config_dir));
    std::fs::create_dir_all(&config_dir).expect("the temporary configuration directory is creatable");
    std::fs::write(config_dir.join("base.yaml"), settings).expect("the settings file is writable");
    config_dir
}

/// Where one case's configuration directory is.
///
/// A function rather than an expression inside [`written`], because a two-source case has to name a
/// SECOND directory derived from the same path - see [`derived_beside`].
pub(crate) fn config_path(case: &str) -> PathBuf {
    std::env::temp_dir().join(format!("sutura-serve-e2e-{case}-{}", std::process::id()))
}

/// Where one case's derived catalog is: a SIBLING of its configuration directory, never inside it.
///
/// Inside is what a reader would write and it does not work: [`written`] clears the configuration
/// directory when it writes the settings file, so a catalog derived under it would be removed
/// between the derivation and the spawn and the deployment would refuse a catalog directory that is
/// not there. Named off the same path so [`Served`]'s `Drop` - the one owner of this suite's
/// cleanup - removes both.
pub(crate) fn derived_beside(config_dir: &Path) -> PathBuf {
    let mut path = config_dir.as_os_str().to_os_string();
    path.push("-catalog");
    PathBuf::from(path)
}

/// Spawns the binary on settings it must **refuse**, and returns what it said before exiting.
///
/// The other half of [`start_configured`], and the reason it exists is the one thing leg 1 buys
/// that cannot be observed on a running deployment: a key set the declaration names and the
/// process cannot use has to stop the process, rather than produce one that starts, logs that it
/// establishes a caller identity, and answers `401` to everybody. That difference is invisible to
/// a harness that only ever waits for a listener.
///
/// Asserts the EXIT CODE here rather than in the caller, so a settings file this suite got wrong -
/// one the binary happily serves - fails as *it started* instead of as a missing line in the log.
///
/// **Exactly `1`, and not merely non-zero, and the reason is MEASURED rather than reasoned.** `main`
/// returns `ExitCode::FAILURE` for every refusal it makes; the failure this separates it from is a
/// process that stopped without deciding to. With step 3 changed to `panic!` on an unservable
/// configuration instead of returning `Err`, the child exits `101`: `just serve-e2e` is
/// `40 passed` at exit 0 under the `!status.success()` this replaced, and `36 passed, 4 failed`
/// against `Some(1)`. A deployment that PANICKED while reading its configuration is not a deployment
/// that declined to serve, and only the exact code separates the two.
///
/// **What this note used to say, corrected rather than deleted:** that `panic = "abort"` leaves no
/// exit code at all. That names the shipped and `ci` profiles - the child this harness spawns is
/// built at `test`, which inherits `dev` and unwinds, so the abort case is real for a release
/// artefact and is not what is exercised here. `code()` is still compared as `Some(1)` rather than
/// by subtraction, because a signal gives `None`.
///
/// `environment` reaches the child through [`command`]: it decides which refusals apply at all, so
/// it is a parameter of the case rather than a constant of the harness.
pub(crate) fn refused_to_start(environment: Environment, case: &str, settings: &str) -> Vec<String> {
    // **Held in a guard from the moment it is spawned, and the failing path is the reason rather
    // than the passing one.** The assertion below fires when the process is STILL RUNNING, which
    // is exactly the defect this function exists to catch - and `std::process::Child` does not
    // kill on drop, so panicking with the child owned by this frame left a serving deployment and
    // its configuration directory behind for the rest of the run. `Served`'s own `Drop` documents
    // the standard this file holds itself to: never a process or a directory left behind, a
    // panicking assertion included.
    let config_dir = written(case, settings);
    let mut spawned = Spawned {
        child: command(&config_dir, environment).spawn().expect("the composed binary starts"),
        config_dir,
        reaped: false,
    };
    let stdout = spawned.child.stdout.take().expect("standard output was piped");
    let stderr = spawned.child.stderr.take().expect("standard error was piped");
    let (sender, lines) = channel();
    let second = sender.clone();
    // KEPT rather than dropped, because the collection below is only sound if these can be joined.
    let readers = vec![
        std::thread::spawn(move || forward(stdout, &sender)),
        std::thread::spawn(move || forward(stderr, &second)),
    ];

    let deadline = Instant::now() + START_BUDGET;
    let status = loop {
        if let Some(status) = spawned.child.try_wait().expect("the child is waitable") {
            spawned.reaped = true;
            break status;
        }
        assert!(
            Instant::now() < deadline,
            "the deployment was still running {}s after being given settings it must refuse",
            START_BUDGET.as_secs()
        );
        std::thread::sleep(Duration::from_millis(25));
    };
    // Joined and then drained - see [`drained`]. This used to be a bare `try_recv` sweep under a
    // comment claiming the readers had finished, which is `github.com/telekom/sutura#387`.
    let said = drained(readers, &lines);
    assert_eq!(
        status.code(),
        Some(1),
        "a deployment given settings it must refuse did not decline to serve:\n{}",
        said.join("\n")
    );
    said
}

/// A spawned process and its configuration directory, both gone when this leaves scope.
///
/// [`Served`] makes the same promise for a deployment that reached a listener; this makes it for
/// one that must not. Two types rather than one because the fields genuinely differ - there is no
/// address here and never will be - and a `Served` whose `address` became an `Option` would push
/// that `None` into every test that does have one.
///
/// Named for the process rather than for what happens to it, because `Reaped` put `reaped` under
/// `clippy::struct_field_names` - a field repeating its struct's name - and the field is the one
/// carrying the state.
pub(crate) struct Spawned {
    child: Child,
    config_dir: PathBuf,
    reaped: bool,
}

impl Drop for Spawned {
    /// `SIGKILL` and not `SIGTERM`: this runs either after the process exited on its own - the
    /// ordinary path, where `reaped` is already set and the kill is skipped - or after an assertion
    /// established that it is serving when it must not be. Neither case is owed a drain.
    fn drop(&mut self) {
        if !self.reaped {
            drop(self.child.kill());
            drop(self.child.wait());
        }
        drop(std::fs::remove_dir_all(&self.config_dir));
        // And the derived catalog a two-source case writes beside it. Unconditional: a case that
        // derived nothing has no such directory and the removal is a no-op, which is cheaper than a
        // flag saying which cases derive.
        drop(std::fs::remove_dir_all(derived_beside(&self.config_dir)));
    }
}

/// Starts the deployment and waits until it says what it bound.
///
/// `case` names the temporary configuration directory, so a failure leaves a directory a reader
/// can identify. `nextest` runs each test in its own process, so the process id in the name is
/// what keeps two tests from sharing one.
pub(crate) fn start(case: &str) -> Served {
    start_configured(case, &settings(&example_root()))
}

/// The same, from a settings file the caller wrote.
///
/// Carved out of [`start`] rather than added beside it, so there is one spawn-and-wait in this
/// file: the leg-1 cases below need a deployment declaring `security.inbound` and everything else
/// about starting it - the environment scrub, the two reader threads, reading the bound address
/// off the `listening` event - is the same composition or it proves nothing.
// `zombie_processes` cannot see the `Drop` impl below, which is where the wait for a failing test
// lives: this function hands the child to a `Served`, and every path out of a `Served` - a clean
// `terminate` or a panicking assertion - reaps it. Reaping here would mean waiting for the service
// to exit before asking it anything.
#[expect(clippy::zombie_processes, reason = "the returned `Served` waits on it in `Drop`")]
pub(crate) fn start_configured(case: &str, settings: &str) -> Served {
    let config_dir = written(case, settings);
    let mut child = command(&config_dir, Environment::Development)
        .spawn()
        .expect("the composed binary starts");
    let stdout = child.stdout.take().expect("standard output was piped");
    let stderr = child.stderr.take().expect("standard error was piped");
    let (sender, lines) = channel();
    let second = sender.clone();
    // KEPT rather than dropped: `Served::terminate` joins them, which is what makes `Served::log`
    // complete after a stop rather than whatever happened to have arrived.
    // `github.com/telekom/sutura#387`.
    let readers = vec![
        std::thread::spawn(move || forward(stdout, &sender)),
        std::thread::spawn(move || forward(stderr, &second)),
    ];

    let mut startup = Vec::new();
    let deadline = Instant::now() + START_BUDGET;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        let line = lines
            .recv_timeout(left)
            .unwrap_or_else(|cause| panic!("the service never reported a listener ({cause}):\n{}", startup.join("\n")));
        let bound = bound_address(&line);
        startup.push(line);
        if let Some(address) = bound {
            return Served {
                child,
                address,
                startup,
                lines,
                readers,
                config_dir,
                reaped: false,
            };
        }
    }
}

/// The address a `listening` event reports, if this line is one.
///
/// Reads the event's own `bound` field rather than matching text: that field is
/// `TcpListener::local_addr` read back off the bound socket, which is the whole reason
/// `server.port: 0` is usable here.
pub(crate) fn bound_address(line: &str) -> Option<String> {
    let event: serde_json::Value = serde_json::from_str(line).ok()?;
    if event.get("msg").and_then(serde_json::Value::as_str)? != "listening" {
        return None;
    }
    Some(String::from(event.get("bound").and_then(serde_json::Value::as_str)?))
}

impl Served {
    /// A GET, with the deployment's token when one is given.
    pub(crate) fn get(&self, path: &str, token: Option<&str>) -> Reply {
        self.send("GET", path, token, None)
    }

    /// A POST of a JSON question.
    pub(crate) fn post(&self, path: &str, token: Option<&str>, body: &str) -> Reply {
        self.send("POST", path, token, Some(body))
    }

    /// One request over one connection.
    ///
    /// **Hand-written rather than a client crate, deliberately.** The alternative is `ureq`,
    /// which arrives with rustls and `ring`; `crane.buildDepsOnly` is unscoped so the four cross
    /// dependency derivations - two of them musl - would compile that closure for a binary that
    /// links none of it, which is the same cost `sutura-serve`'s `bigquery` feature is
    /// default-off to avoid. What is needed here is one plaintext loopback request with a fixed
    /// shape, so this is thirty lines and no dependency.
    ///
    /// `Connection: close` is what makes reading to end-of-file the whole response, and the
    /// chunked assertion in [`parse`] is what stops that quietly mis-parsing if a handler ever
    /// answers without a length.
    fn send(&self, method: &str, path: &str, token: Option<&str>, body: Option<&str>) -> Reply {
        let mut stream = TcpStream::connect(&self.address).expect("the listener accepts a connection");
        stream
            .set_read_timeout(Some(Duration::from_secs(60)))
            .expect("a read timeout is settable");
        let mut request = String::new();
        write!(request, "{method} {path} HTTP/1.1\r\n").expect("writing to a String cannot fail");
        write!(request, "Host: {}\r\n", self.address).expect("writing to a String cannot fail");
        request.push_str("Connection: close\r\n");
        if let Some(token) = token {
            write!(request, "Authorization: Bearer {token}\r\n").expect("writing to a String cannot fail");
        }
        if let Some(body) = body {
            request.push_str("Content-Type: application/json\r\n");
            write!(request, "Content-Length: {}\r\n", body.len()).expect("writing to a String cannot fail");
        }
        request.push_str("\r\n");
        if let Some(body) = body {
            request.push_str(body);
        }
        stream.write_all(request.as_bytes()).expect("the request is writable");
        stream.flush().expect("the request flushes");
        let mut raw = Vec::new();
        let read = stream.read_to_end(&mut raw).expect("the response is readable");
        assert!(read > 0, "the listener closed the connection without answering");
        parse(&String::from_utf8_lossy(&raw))
    }

    /// Stops the process the way an orchestrator does, and returns how it exited.
    pub(crate) fn terminate(&mut self) -> ExitStatus {
        let signalled = Command::new("/bin/sh")
            .arg("-c")
            .arg(format!("kill -TERM {}", self.child.id()))
            .status()
            .expect("a shell is available to send the signal");
        assert!(signalled.success(), "the terminate signal was not delivered");
        let deadline = Instant::now() + STOP_BUDGET;
        loop {
            if let Some(status) = self.child.try_wait().expect("the child is waitable") {
                self.reaped = true;
                // The reaped child's pipes are at end-of-file, so this returns as soon as the
                // readers have pushed the last lines in - which is what makes `log` below complete
                // rather than best-effort. Issue 387 is the same defect on the refusing path.
                joined(std::mem::take(&mut self.readers));
                return status;
            }
            assert!(
                Instant::now() < deadline,
                "the process was still running {}s after SIGTERM",
                STOP_BUDGET.as_secs()
            );
            std::thread::sleep(Duration::from_millis(25));
        }
    }

    /// The startup log, plus everything written since.
    ///
    /// **Complete after [`Served::terminate`] and best-effort before it, and the distinction is the
    /// correction `github.com/telekom/sutura#387` paid for.** This used to say the readers "have
    /// already seen end-of-file" because the process had been reaped, which reaping does not establish: a
    /// `try_recv` sweep stops at the first empty channel, so the lines a process writes as it dies
    /// can still be in flight. `terminate` JOINS the readers, so a call after it cannot miss one.
    ///
    /// Before a `terminate` it stays non-blocking on purpose - joining a running deployment's
    /// readers would never return - and the one caller that reads it that way asserts on two lines
    /// that were already read into `startup` before the deployment was handed over.
    pub(crate) fn log(&self) -> Vec<String> {
        let mut out = self.startup.clone();
        loop {
            match self.lines.try_recv() {
                Ok(line) => out.push(line),
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => return out,
            }
        }
    }

    /// Reads until a line contains `needle`, and hands back every line read on the way to it.
    ///
    /// **Blocking, where [`Served::log`] is not, and the difference is not convenience.** The
    /// audit record for an answered question is written from the **blocking pool** - see
    /// `sutura_runtime::blocking` - so it is not ordered against the HTTP response the asking
    /// thread already has. A `try_recv` sweep straight after a `200` is a race that fails as *the
    /// record was never written*, which is the wrong diagnostic for a record that arrived a
    /// millisecond later.
    ///
    /// It is also why this assertion can only live here: `sutura_http`'s log-capture harness
    /// installs a **thread-scoped** subscriber, so the record from that pool is invisible to it
    /// (two of its own tests say so where they stand in `tower_http`'s response line instead).
    /// This suite reads the process's own streams, which is every thread.
    ///
    /// Panics on the deadline with everything it did read, because a failure here is about what
    /// the deployment wrote rather than about this helper.
    pub(crate) fn awaiting(&self, needle: &str) -> Vec<String> {
        let mut read = self.startup.clone();
        let deadline = Instant::now() + RECORD_BUDGET;
        loop {
            if read.iter().any(|line| line.contains(needle)) {
                return read;
            }
            let left = deadline.saturating_duration_since(Instant::now());
            let line = self.lines.recv_timeout(left).unwrap_or_else(|cause| {
                panic!(
                    "no line carrying `{needle}` inside {}s ({cause}):\n{}",
                    RECORD_BUDGET.as_secs(),
                    read.join("\n")
                )
            });
            read.push(line);
        }
    }
}

impl Drop for Served {
    /// Never leaves a process or a directory behind, including after a panicking assertion.
    ///
    /// `SIGKILL` here and `SIGTERM` in [`Served::terminate`]: this path runs when a test has
    /// already failed, so what it owes is cleanup rather than a drain.
    fn drop(&mut self) {
        if !self.reaped {
            drop(self.child.kill());
            drop(self.child.wait());
        }
        drop(std::fs::remove_dir_all(&self.config_dir));
        // And the derived catalog a two-source case writes beside it. Unconditional: a case that
        // derived nothing has no such directory and the removal is a no-op, which is cheaper than a
        // flag saying which cases derive.
        drop(std::fs::remove_dir_all(derived_beside(&self.config_dir)));
    }
}

/// Splits a response into its status and its body.
pub(crate) fn parse(text: &str) -> Reply {
    let (head, body) = text
        .split_once("\r\n\r\n")
        .unwrap_or_else(|| panic!("not an HTTP response: {text}"));
    assert!(
        !head.to_ascii_lowercase().contains("transfer-encoding: chunked"),
        "this harness reads a length-delimited response and got a chunked one:\n{head}"
    );
    let status_line = head.lines().next().unwrap_or_default();
    let status = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse::<u16>().ok())
        .unwrap_or_else(|| panic!("no status in `{status_line}`"));
    // Field name matched case-insensitively, because HTTP field names are.
    let challenge = head
        .lines()
        .find_map(|line| {
            line.split_once(':')
                .filter(|&(name, _)| name.eq_ignore_ascii_case("www-authenticate"))
        })
        .map(|(_, value)| String::from(value.trim()));
    Reply {
        status,
        body: String::from(body),
        challenge,
    }
}

/// Where an event with exactly this message first appears in the log.
///
/// Matches the bunyan `msg` field WHOLE rather than as a substring, and that was MEASURED rather
/// than preferred. The banner writes `listening on loopback only - reachable from this host and no
/// other` before the catalog is loaded, so a substring search for `listening` found that line and
/// the ordering assertion below went red against a service that was in fact ordered correctly. A
/// search loose enough to match the wrong event reports the wrong thing in both directions.
pub(crate) fn position(log: &[String], message: &str) -> usize {
    let field = format!("\"msg\":\"{message}\"");
    log.iter()
        .position(|line| line.contains(&field))
        .unwrap_or_else(|| panic!("no event says `{message}`:\n{}", log.join("\n")))
}

/// The grain every question in this file asks at.
///
/// One constant rather than a parameter because all three fixtures below ask at it, and it is
/// checked against each of them - so this is a fact about the corpus rather than a default.
pub(crate) const GRAIN: &str = "month";

/// A question as it arrives on the wire, mirrored from the example's own fixture.
///
/// **The fixture is READ rather than cited in a comment, and that is the difference between a
/// claim and a mechanism.** `stem` names the file under `examples/single-player/questions/`
/// that holds this question, and the metric, the grain and both dates have to appear in it - so
/// a fixture renamed, deleted or re-ranged goes red HERE instead of leaving this suite quietly
/// asserting a question the example no longer asks. That is the property #117 wants out of this
/// file: the example a reader is told to run is the example CI runs.
///
/// A substring check and not a parse, deliberately: parsing the YAML would mean a second
/// dev-dependency for four fields, and what is guarded here is a fixture that MOVED rather than
/// one that is subtly mis-shaped - `crates/sutura-cli/tests/example.rs` parses every question in
/// that directory and pins what each one compiles to.
pub(crate) fn question(stem: &str, metric: &str, start: &str, end: &str) -> String {
    let path = example_root().join("questions").join(format!("{stem}.yaml"));
    let fixture = std::fs::read_to_string(&path).unwrap_or_else(|cause| {
        panic!(
            "{} is the example question this body mirrors, and it is not readable: {cause}",
            path.display()
        )
    });
    for expected in [metric, GRAIN, start, end] {
        assert!(
            fixture.contains(expected),
            "{} no longer mentions `{expected}`, so this suite asks something the example does \
             not:\n{fixture}",
            path.display()
        );
    }
    format!(r#"{{"metric":"{metric}","grain":"{GRAIN}","range":{{"start":"{start}","end":"{end}"}}}}"#)
}

/// The one question this file asserts numbers for.
pub(crate) fn recurring_revenue_june() -> String {
    question("recurring-revenue-june", "recurring_revenue", "2026-06-01", "2026-07-01")
}

/// A route inside the version prefix, composed the way the router composes it.
///
/// One helper over both routes rather than one per route: the composition is the part worth
/// having in a single place, and `base_paths` already owns each half.
pub(crate) fn v1(base: &str) -> String {
    format!("{}{base}", sutura_http::constants::API_V1_PREFIX)
}

/// What the mock issuer calls itself, and what its tokens are for.
///
/// The resource identifier is this deployment's own name for itself, so it is what a refused
/// caller's challenge has to carry - a challenge naming something else would send a client to the
/// wrong authorization server.
pub(crate) const ISSUER: &str = "https://issuer.example.com";
pub(crate) const RESOURCE: &str = "https://sutura.example.com";
pub(crate) const KEY_ID: &str = "the-current-key";

/// The bunyan `msg` of the audit record for an answered question.
///
/// `answered` is `sutura_runtime::audit`'s own word - the same one the per-outcome log line that
/// record replaced used, so a filter written against either finds this.
///
/// **The `[REQUEST - EVENT]` prefix is the request span's and is part of the match on purpose.**
/// The record is emitted inside the span the router opens, so that is the whole `msg` field as
/// written; matching only `answered` would be a substring search, which is the mistake
/// [`position`] carries its own warning about - it once matched a banner line and reported the
/// wrong thing in both directions. Written whole, this cannot match a different event.
pub(crate) const RECORD: &str = r#""msg":"[REQUEST - EVENT] answered""#;

/// The issuer this deployment is configured with, generating its own key pair.
pub(crate) fn an_issuer() -> MockIssuer {
    MockIssuer::generating(ISSUER, RESOURCE, KEY_ID).expect("a mock issuer generates a key pair")
}

/// A token this deployment accepts, granting every capability the surface has.
///
/// The scopes are read off `sutura_app::Capability` rather than written out, so a capability added
/// later widens what this token grants instead of leaving one governed route quietly unreachable
/// to it: leg 1 says who is asking and the capability gate decides what they may invoke, so a
/// token with no `scope` claim reaches a handler for nothing.
pub(crate) fn accepted_by(subject: &str) -> Token {
    let scopes = sutura_app::Capability::every()
        .map(sutura_app::Capability::scope)
        .collect::<Vec<&str>>()
        .join(" ");
    Token::for_subject(subject).granting(&scopes)
}
