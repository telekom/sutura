//! Does the API reference still generate, and can mkdocs follow every link on it?
//!
//! `docs/api/*.md` is GENERATED and committed nowhere. The docs workflow builds it from
//! `checks.api-docs`, which leaves the pages it judged in `$out`, on `main` and on a `v*` tag, and
//! `just api` writes it for a reader who wants it locally. Rustdoc JSON is nightly-only, and the whole tree already builds on the single
//! pinned nightly, so generating it needs nothing beyond that same toolchain.
//!
//! A page nobody commits cannot go stale, so there is nothing left to compare against. What a
//! pull request can still prove - and what this gate fails on - is that the publish run WOULD
//! succeed: rustdoc accepts every doc comment, the generator renders every library crate, and no
//! link on a rendered page is one `mkdocs build --strict` would abort on or publish dead.
//!
//! HOW: produce the rustdoc JSON, run the generator into a temporary directory, require a page for
//! every library crate, and hand that directory to [`crate::api_links`]. Two properties are why it
//! is this shape rather than a cleverer one:
//!
//!   * IT JUDGES THE DOC LINKS, and not by looking at them. `broken_intra_doc_links` is forbidden in
//!     the workspace lint table, so the `cargo doc` line below fails on a link rustdoc cannot
//!     resolve - 15 of those were warnings behind exit 0 until `github.com/telekom/sutura#360`.
//!     **This is the only venue that enforces it**: the doctest lane does not, measured. This
//!     check also documents each binary-only target, without rendering API pages for it; `lints`
//!     states that scope. A lint also cannot say whether it was ARMED, so that submodule asserts
//!     the table exists and that every workspace member inherits it, before a single crate is
//!     documented. The test that holds the precondition is over THIS function rather than over that
//!     module - see `check_refuses_before_documenting_anything_when_the_lint_is_not_armed`, which
//!     exists because deleting the call and handing `check` a literal left every test in `lints`
//!     green. **And a lint cannot say what it was pointed AT**: rustdoc runs no link-resolution pass
//!     over an item it is not documenting, so the same armed `forbid` was exit 0 over 33
//!     unresolvable links inside private modules - `github.com/telekom/sutura#327`.
//!     `--document-private-items` is what points it at them, `writer` is what keeps `just api`
//!     passing the identical flag, and the visibility filter in the generator is why no private item
//!     reaches a page.
//!   * IT IS THE SAME CODE PATH. The `cargo doc` line and the generator script are the ones the
//!     publish run uses. A gate that reimplemented the rendering could pass where the publish run
//!     fails. Never inline the rendering here.
//!
//! **The limit, next to the claim.** This proves the pages GENERATE and carry no dead destination.
//! It does not prove `mkdocs build --strict` accepts them - pixi cannot run in the nix sandbox this
//! gate runs in - so a duplicate anchor or a malformed table can still fail the publish job after
//! merge, and no pull request shows what a page looks like.
//!
//! RUSTDOC JSON NEEDS NIGHTLY. `--output-format json` is an unstable rustdoc option, and the whole
//! toolchain is nightly - the shell's bare cargo and every gate run the pinned nightly
//! (devco/rust-toolchain-nightly.toml). `flake.nix` hands the check a nightly `$CARGO` and a
//! `CARGO_TARGET_DIR` of its own so the docs never share artifacts with another run's, and
//! `SUTURA_API_DOCS_PROFILE` so its compile is at opt-level 0. It is `Kind::Standalone` because
//! `cargo xtask hygiene` is a cheap sweep that should run on hosts with no Rust nightly at all.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::{Verdict, api_links, repo};

/// Whether the rustdoc run below is armed to judge doc links at all.
mod lints;

/// Whether `just api` hands rustdoc the same flags this gate does.
mod writer;

/// Whether a generated page has crept back into git.
pub(crate) mod owner;

/// Where `just api` writes the pages, and where the publish run copies the ones it built, relative to
/// the repo root.
///
/// The scan names a finding by this path, because it is where a reader finds the page on the
/// site and under `just api`; the scan itself reads a temporary directory.
pub(crate) const PAGES_DIR: &str = "docs/api";

/// The generator, relative to the repo root.
///
/// Run as a script rather than reimplemented, so this gate and the publish run cannot disagree
/// about what a page should contain.
///
/// Shared with [`crate::api_links`], which reads the same file for a different reason: the two hold
/// one list of real URL schemes between them, in two languages, and that scan compares them.
pub(crate) const GENERATOR: &str = "docs/.tools/rustdoc_to_markdown.py";

/// The first line of the header every generated page carries.
///
/// Shared with [`crate::api_links`], where it is the FLOOR: a scan of the generator's output
/// finding no generated page at all is a failure rather than a clean sweep.
pub(crate) const GENERATED_MARKER: &str = "<!-- GENERATED FILE - do not edit.";

/// Names an interpreter to run the generator with, bypassing pixi.
///
/// For the one caller that cannot use pixi: a Nix build sandbox has no network, so it cannot
/// materialise a pixi environment. See [`python_command`].
const PYTHON_ENV: &str = "SUTURA_API_DOCS_PYTHON";

/// Names a directory to leave the generated pages in, once the run has judged them clean.
///
/// For the one caller that has to publish them: the nix check, whose `$out` the docs workflow
/// copies into `docs/api`. Building the pages in that sandbox is what keeps the dependency closure's
/// build scripts away from the job that holds a write token. Unset means nothing is kept.
const KEEP_ENV: &str = "SUTURA_API_DOCS_KEEP";

/// Names the cargo profile the `cargo doc` child compiles under.
///
/// For the same caller as [`PYTHON_ENV`]: the Nix check. Unset means "say nothing", which leaves
/// cargo on its default `dev` - the right answer in a dev shell, where `target/` is already warm
/// under that profile and a second one would be a second full closure on the developer's disk.
///
/// Set, it is worth a lot. The DEFAULT is not cheap here: Cargo.toml carries `opt-level = 3` in
/// both `[profile.dev.package."*"]` and `[profile.dev.build-override]`, and `debug = true` over
/// the whole closure, so `dev` asks for optimised, debuginfo-carrying builds of every dependency
/// and every build script on the way to a JSON file. Documenting a crate needs its dependencies
/// as `rmeta`, not as codegen, and `ci` is the profile this repo already keeps for exactly that
/// bargain.
///
/// A profile NAME and not a boolean: naming it here and in `flake.nix` is what lets the gate and
/// the `just api` writer compile into the same place under the same settings, and the two
/// disagreeing is the failure this module's header is about.
const PROFILE_ENV: &str = "SUTURA_API_DOCS_PROFILE";

/// `xtask check-api-docs` - generate the API pages into a temporary directory and scan them.
pub(crate) fn run(args: &[String]) -> Verdict {
    if !args.is_empty() {
        eprintln!("xtask check-api-docs: takes no arguments");
        return Verdict::Usage;
    }
    let Some(root) = repo::root() else {
        eprintln!("xtask check-api-docs: could not determine the repo root");
        return Verdict::Fail;
    };

    match check(&root) {
        Err(error) => {
            eprintln!("xtask check-api-docs: {error}");
            Verdict::Fail
        }
        Ok(outcome) if outcome.problems.is_empty() => {
            println!("xtask check-api-docs: ok - the API reference generates, and mkdocs can follow every link on it");
            if let Some(read) = &outcome.links {
                println!(
                    "  {} page(s) generated and read, {} link destination(s), {} URL scheme(s) agreed with the generator",
                    read.pages, read.destinations, read.schemes
                );
            }
            println!(
                "  doc links: `broken_intra_doc_links` = {}, {} member(s) declared in Cargo.toml, \
                 {} resolved by cargo metadata, {} inheriting",
                outcome.arming.level, outcome.arming.declared, outcome.arming.resolved, outcome.arming.inheriting
            );
            println!(
                "  {} binary-only target(s) documented for links; API pages remain library-only",
                outcome.binaries
            );
            println!("  rustdoc flags, agreed with `just api`'s writer: {:?}", outcome.flags);
            Verdict::Pass
        }
        Ok(outcome) => {
            report(&outcome.problems);
            Verdict::Fail
        }
    }
}

/// What one run of this gate found, and what it was armed with while finding it.
///
/// The arming travels back with the problems rather than being printed where it is read: a
/// verdict that says the pages generate without saying the rustdoc run behind them judged its links
/// is the shape this pair exists to make impossible.
#[derive(Debug)]
struct Outcome {
    /// A library crate with no generated page, or a page mkdocs cannot follow a link on.
    problems: Vec<String>,
    /// What the link scan read, when it found nothing.
    links: Option<api_links::Read>,
    /// The doc-link lint's level, and the three counts that say it reached every member.
    arming: lints::Arming,
    /// Binary-only targets the workspace run documented for links, not generated pages.
    binaries: usize,
    /// The rustdoc flags `just api` and this gate agree on - see [`writer`].
    flags: Vec<String>,
}

/// Generate every library crate's page into a temporary directory and collect what is wrong.
///
/// `Err` is reserved for "the check could not be performed" - a missing generator, a rustdoc
/// that refused to run. That is deliberately NOT reported as a clean repo: a gate that cannot
/// run has found nothing, and reporting nothing as `ok` is how a pipeline goes green over an
/// unchecked tree.
fn check(root: &Path) -> Result<Outcome, String> {
    let metadata = crate::cargo_metadata(&["--no-deps"])?;
    // FIRST, and it is an `Err` rather than a problem: an unarmed rustdoc run cannot tell a
    // resolvable doc link from an unresolvable one, so generating pages from it and reporting
    // them clean would be a green verdict over a question nobody asked. See `lints`.
    let arming = lints::check(root, &metadata)?;
    // SECOND precondition, and an `Err` for the same reason: `just api` uses the writer, and a writer
    // passing other flags produces pages from a rustdoc run whose question was not the one asked
    // here. The publish run does not use it - it copies what this check kept. See `writer`.
    let flags = writer::check(root)?;
    let packages = library_packages(&metadata)?;
    let binaries = binary_targets(&metadata)?;
    let target_dir = target_directory(root, &metadata);

    if let Some((package, binary)) = colliding_json(&packages, &binaries) {
        return Err(format!(
            "binary target `{package}/{binary}` would write {}, which is a library crate's page \
             input - rename the binary target. One `cargo doc --workspace` documents the library \
             and the binary as two units of the SAME run, so a shared file name is a race rather \
             than an overwrite: whichever unit finishes last decides what the generator reads.",
            json_file_name(binary)
        ));
    }

    let generator = root.join(GENERATOR);
    if !generator.is_file() {
        return Err(format!(
            "{GENERATOR} does not exist - there is nothing to generate the pages with"
        ));
    }
    selftest_renderer(root, &generator)?;

    let cargo = cargo_bin();
    // ONE cargo invocation for every member, and the serial loop it replaces was the whole cost
    // of this gate: 19 of them (16 libraries, 3 binary-only targets) took 216 s of the 225 s
    // buildPhase - 96.0% of it - inside a 254 s CI step whose next-largest sibling was 49 s, on
    // a 16-core host 25.9% busy on average (run 34646880506). Paired and load-matched in the nix
    // sandbox on one host, that buildPhase is 188 s before this change and 50 s after.
    // The larger half is not the lost parallelism. `-p <one> --all-features` is a DIFFERENT
    // feature resolution from the `--workspace --all-features` that `sutura-deps` is built
    // under, so every feature-gated dependency fell outside the warm artifacts and was
    // recompiled - the defect `flake.nix` records against `checks.nextest` at 67 third-party
    // crates, and this gate was the last consumer still asking a question the closure could not
    // answer. Asking the closure's own question is what makes the artifacts reusable here.
    rustdoc_json(&cargo, root)?;
    let mut inputs = Vec::new();
    for package in &packages {
        let json = target_dir.join("doc").join(json_file_name(package));
        if !json.is_file() {
            return Err(format!(
                "rustdoc reported success but wrote no {}.\n  \
                 The JSON file name is derived from the crate's Rust identifier; if rustdoc \
                 changed where it writes, this is the line to fix.",
                json.display()
            ));
        }
        inputs.push(json);
    }

    let scratch = scratch_dir()?;
    if let Err(error) = generate(root, &generator, &inputs, &scratch) {
        drop(std::fs::remove_dir_all(&scratch));
        return Err(error);
    }
    let keep_in = std::env::var(KEEP_ENV).ok();
    let mut problems = missing_pages(&scratch, &packages);
    let links = match api_links::check(root, &scratch) {
        Ok(read) => Some(read),
        Err(found) => {
            problems.extend(found.iter().map(|finding| format!("  {finding}")));
            None
        }
    };

    let kept = if problems.is_empty() {
        keep(&scratch, keep_in.as_deref())
    } else {
        Ok(())
    };
    // Best-effort: a scratch directory left behind is untidy, not a verdict.
    drop(std::fs::remove_dir_all(&scratch));
    kept?;
    Ok(Outcome {
        problems,
        links,
        arming,
        binaries: binaries.len(),
        flags,
    })
}

/// Binary-only package and target names selected from metadata.
type BinaryTargets = BTreeSet<(String, String)>;

/// Explicit binary targets in packages without a library or proc-macro target.
///
/// Cargo's target name can differ from its package, and one package can have several binaries.
/// Missing target metadata must not silently turn either case into an unjudged package.
fn binary_targets(metadata: &serde_json::Value) -> Result<BinaryTargets, String> {
    let packages = metadata
        .get("packages")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| String::from("cargo metadata had no `packages` array"))?;
    let mut binaries = BTreeSet::new();
    for package in packages {
        let name = package
            .get("name")
            .and_then(serde_json::Value::as_str)
            .filter(|name| !name.is_empty())
            .ok_or_else(|| String::from("cargo metadata had an unnamed package"))?;
        let targets = package
            .get("targets")
            .and_then(serde_json::Value::as_array)
            .filter(|targets| !targets.is_empty())
            .ok_or_else(|| format!("cargo metadata had no targets for `{name}`"))?;
        let mut has_library = false;
        for target in targets {
            let kinds = target
                .get("kind")
                .and_then(serde_json::Value::as_array)
                .filter(|kinds| !kinds.is_empty())
                .ok_or_else(|| format!("cargo metadata had no target kind for `{name}`"))?;
            if kinds.iter().any(|kind| kind.as_str().is_none()) {
                return Err(format!("cargo metadata had a non-text target kind for `{name}`"));
            }
            has_library |= kinds
                .iter()
                .any(|kind| matches!(kind.as_str(), Some("lib" | "rlib" | "proc-macro")));
        }
        if has_library {
            continue;
        }
        for target in targets {
            if !target
                .get("kind")
                .and_then(serde_json::Value::as_array)
                .is_some_and(|kinds| kinds.iter().any(|kind| kind.as_str() == Some("bin")))
            {
                continue;
            }
            let binary = target
                .get("name")
                .and_then(serde_json::Value::as_str)
                .filter(|name| !name.is_empty())
                .ok_or_else(|| format!("cargo metadata had an unnamed binary target for `{name}`"))?;
            binaries.insert((String::from(name), String::from(binary)));
        }
    }
    Ok(binaries)
}

/// Every workspace package with a library target.
///
/// DERIVED, not named. A second library crate must not be able to arrive with no page and no
/// failure, and a hardcoded crate name is exactly how that happens. A binary-only crate has no
/// public surface to render, so its absence here is correct rather than a gap.
fn library_packages(metadata: &serde_json::Value) -> Result<BTreeSet<String>, String> {
    let packages = metadata
        .get("packages")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| String::from("cargo metadata had no `packages` array"))?;

    let mut names = BTreeSet::new();
    for package in packages {
        let Some(name) = package.get("name").and_then(serde_json::Value::as_str) else {
            continue;
        };
        let has_lib = package
            .get("targets")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|targets| targets.iter().any(is_lib_target));
        if has_lib {
            names.insert(String::from(name));
        }
    }

    // An empty set would make this gate pass having compared nothing - the failure mode a
    // derived-from-metadata check is most prone to, and the one worth an explicit error.
    if names.is_empty() {
        return Err(String::from(
            "cargo metadata reported no workspace package with a `lib` target.\n  \
             The scan is broken, not the repo: this gate would otherwise pass having \
             compared nothing.",
        ));
    }
    Ok(names)
}

/// Is this cargo target a library whose public surface rustdoc JSON can describe?
///
/// `kind` is a LIST because one target is often several things at once (`lib` and `rlib`). A
/// proc-macro is excluded: it is a compiler plugin rather than a surface a caller programs
/// against, and rustdoc JSON for one describes macros that have no signature to render.
fn is_lib_target(target: &serde_json::Value) -> bool {
    let Some(kinds) = target.get("kind").and_then(serde_json::Value::as_array) else {
        return false;
    };
    let kinds: Vec<&str> = kinds.iter().filter_map(serde_json::Value::as_str).collect();
    (kinds.contains(&"lib") || kinds.contains(&"rlib")) && !kinds.contains(&"proc-macro")
}

/// Where cargo writes build output. Asked rather than assumed, because `CARGO_TARGET_DIR` is
/// set in this repo's dev shell - the gates get their own directory so two toolchains do not
/// invalidate each other's artifacts - and `target/doc` would be the wrong place there.
fn target_directory(root: &Path, metadata: &serde_json::Value) -> PathBuf {
    metadata
        .get("target_directory")
        .and_then(serde_json::Value::as_str)
        .map_or_else(|| root.join("target"), PathBuf::from)
}

/// rustdoc names its JSON after the crate's Rust identifier, so `sutura-domain` becomes
/// `sutura_domain.json`. The page keeps the PACKAGE name, which is what a reader looks for in
/// the nav and in Cargo.toml.
fn json_file_name(package: &str) -> String {
    let mut name = package.replace('-', "_");
    name.push_str(".json");
    name
}

/// A binary target whose rustdoc JSON would overwrite a library crate's, if any.
///
/// Package names are unique, but a `[[bin]] name` is not bound to its package's - so a binary
/// CAN be named after a library crate, and rustdoc names its JSON after the Rust identifier
/// rather than the package. Nothing else in this module would notice: the generator would render
/// the binary's surface onto the library's page, which no later step could tell from the real one.
/// It is a refusal and not a workaround: the two are units of one cargo invocation, so there is
/// no ordering left to impose that would make a shared file name safe.
fn colliding_json<'a>(packages: &BTreeSet<String>, binaries: &'a BinaryTargets) -> Option<(&'a str, &'a str)> {
    let pages: BTreeSet<String> = packages.iter().map(|package| json_file_name(package)).collect();
    binaries
        .iter()
        .find(|(_, binary)| pages.contains(&json_file_name(binary)))
        .map(|(package, binary)| (package.as_str(), binary.as_str()))
}

/// The cargo to invoke.
///
/// The environment variable rather than `env!("CARGO")`: the compile-time form bakes cargo's
/// absolute store path into the binary, which puts the whole cargo closure into the shipped
/// package's runtime closure. Cargo sets this whenever it invokes us.
fn cargo_bin() -> String {
    std::env::var("CARGO").unwrap_or_else(|_| String::from("cargo"))
}

/// Produce every workspace member's rustdoc JSON, in one cargo invocation.
///
/// The arguments are the ones the `api` recipe's writer uses, in the same order, for the
/// same-code-path reason in this module's header.
///
/// `cargo doc` and not `cargo rustdoc`, and the two differences are the point. It documents MANY
/// units, so cargo schedules them across the host instead of this function serialising them; and
/// it resolves features ONCE over the whole workspace, which is the resolution `sutura-deps` was
/// built under, so the warm artifacts are reusable rather than partially invalid. It takes no
/// trailing rustdoc arguments in exchange - there is no single unit for them to belong to - so the
/// flags travel in [`writer::RUSTDOCFLAGS`], which is the copy `writer` compares.
///
/// `--no-deps` is load-bearing: without it `--workspace` documents the entire dependency closure
/// rather than the members, which is a different and far larger job.
///
/// THE ONE STEP THAT NEEDS NIGHTLY, and it is a CHILD PROCESS. `cargo` here is whichever cargo
/// the caller named - `$CARGO`, which the Nix check sets to the nightly. This binary itself is
/// compiled on the same nightly toolchain every gate uses, which is why the check can share their
/// dependency closure.
fn rustdoc_json(cargo: &str, root: &Path) -> Result<(), String> {
    let profile = std::env::var(PROFILE_ENV).ok();
    let status = std::process::Command::new(cargo)
        .current_dir(root)
        // The cranelift backend is INHERITED from the dev shell, and it cannot build this tree:
        // `utoipa-swagger-ui`'s build script unzips its vendored asset bundle, and the CRC32 in
        // `zip` uses `llvm.x86.pclmulqdq.256`, which cranelift does not implement - so the build
        // script aborts with SIGABRT and this gate fails on a crate whose docs are fine. Removing
        // the two variables rather than setting a backend leaves the profile at its default, which
        // is what every other non-dev-shell caller of cargo already gets.
        .env_remove("CARGO_PROFILE_DEV_CODEGEN_BACKEND")
        .env_remove("CARGO_UNSTABLE_CODEGEN_BACKEND")
        .env(writer::RUSTDOCFLAGS, writer::RUSTDOC_ARGS.join(" "))
        .args(["doc", "-q", "--no-deps", "--workspace", "--all-features"])
        .args(profile_args(profile.as_deref()))
        .status()
        .map_err(|error| format!("could not run `{cargo} doc`: {error}"))?;
    if status.success() {
        return Ok(());
    }
    Err(String::from(
        "`cargo doc --workspace --no-deps ... --output-format json` failed, and rustdoc's own \
         output above names the crate and says which of two things happened.\n  \
         A DOC LINK it could not resolve: `broken_intra_doc_links` is forbidden in the workspace \
         lint table, so that is an error here rather than a warning behind exit 0. Fix the link - the \
         `crate::`-qualified inline form `[`x`](crate::path::x)` resolves without an import and \
         the page keeps the code span.\n  \
         Or the TOOLCHAIN: `--output-format json` is unstable, so this gate needs the nightly pin \
         (devco/rust-toolchain-nightly.toml), which is what the shell's bare `cargo` IS.",
    ))
}

/// `--profile <name>`, or nothing at all.
///
/// A PARAMETER and not a read of the environment, for the same reason as [`python_command`]: a
/// function that reads a process-global is a function the tests can only exercise by mutating one,
/// and these tests run in parallel threads.
///
/// An empty value counts as unset. Not defensive decoration - a caller that expands an unset shell
/// variable would otherwise hand cargo a bare `--profile` and get "expected a value", which reads
/// as a cargo problem rather than as the plumbing mistake it is.
fn profile_args(profile: Option<&str>) -> Vec<String> {
    match profile {
        Some(name) if !name.is_empty() => vec![String::from("--profile"), String::from(name)],
        _ => Vec::new(),
    }
}

/// Run the generator's own behavioral fixture, once, before any page is generated.
///
/// A generation that succeeds proves the pages RENDER - and, as `github.com/telekom/sutura#470`
/// measured, does NOT prove the renderer INCLUDES the documentation an item carries. A `pub use` re-export carries its public
/// name and its docs on the TARGET (`inner.use.name`, `inner.use.id`), not on the re-export item
/// itself, and the renderer used to print a literal `use None` for it with the gate green. The
/// generator's `--self-test` renders a hand-built re-export fixture and asserts the caller-visible
/// name and the target's documentation appear, so a future renderer regression fails here even if
/// no crate's documentation happened to exercise it. It is the same script and the same pixi/python
/// interpreter as [`generate`], because the gate and the publish run must exercise the same code path.
///
/// **The fixture asserts more than one property, so the refusal below may not name the one that
/// failed.** It also holds that an enum variant's prose is published in FULL: the renderer used to
/// emit only the first paragraph, with the gate green, which is the same shape as `use None` - a
/// page that reads as complete over documentation the renderer dropped. A refusal naming only the
/// re-export half would send a reader looking in the wrong place, which is why it names neither
/// and points at the assertion instead.
fn selftest_renderer(root: &Path, generator: &Path) -> Result<(), String> {
    let override_path = std::env::var(PYTHON_ENV).ok();
    let (program, mut argv) = python_command(override_path.as_deref());
    argv.push(argument(generator)?);
    argv.push(String::from("--self-test"));

    let status = std::process::Command::new(&program)
        .current_dir(root)
        .args(&argv)
        .status()
        .map_err(|error| format!("could not run the generator self-test: {error}"))?;
    if status.success() {
        return Ok(());
    }
    Err(format!(
        "the generator's self-test failed. It holds two properties: a public re-export renders \
         its caller-visible name and its TARGET's documentation rather than `use None`, and an \
         enum variant's prose is published in full rather than truncated to its first paragraph. \
         Run `{GENERATOR} --self-test` the way this gate does - the assertion says which one."
    ))
}

/// How to run the generator, as a program and its leading arguments.
///
/// `pixi run --frozen python` by default, which is what the justfile recipe and the page header
/// both say. pixi owns every Python in this repo on purpose - one resolver per language - so the
/// dev shell puts no interpreter on PATH at all and a bare `python3` would work on a laptop and
/// be missing where it matters.
///
/// An override exists for the one caller that cannot use pixi: a Nix build sandbox has no
/// network, so it cannot materialise a pixi environment. The SCRIPT is identical either way and
/// uses the standard library only, so the interpreter is the only thing that differs and the
/// output cannot.
fn python_command(interpreter: Option<&str>) -> (String, Vec<String>) {
    interpreter.map_or_else(
        || {
            (
                String::from("pixi"),
                vec![String::from("run"), String::from("--frozen"), String::from("python")],
            )
        },
        |path| (String::from(path), Vec::new()),
    )
}

/// Render every JSON input into `out`, using the generator script.
fn generate(root: &Path, generator: &Path, inputs: &[PathBuf], out: &Path) -> Result<(), String> {
    let override_path = std::env::var(PYTHON_ENV).ok();
    let (program, mut argv) = python_command(override_path.as_deref());
    argv.push(argument(generator)?);
    for input in inputs {
        argv.push(argument(input)?);
    }
    // The script takes the output directory as an optional last argument and defaults to
    // `docs/api`. Passing one is what keeps this gate from writing into the working tree.
    argv.push(argument(out)?);

    let status = std::process::Command::new(&program)
        .current_dir(root)
        .args(&argv)
        .status()
        .map_err(|error| format!("could not run `{program}`: {error}"))?;
    if status.success() {
        return Ok(());
    }
    Err(format!(
        "the generator failed. Run it the way `just api` does to see its own output;\n  \
         a `format_version` mismatch there means the nightly moved and \
         {GENERATOR} needs updating."
    ))
}

/// A path as a command-line argument. Non-UTF-8 is an error rather than a lossy guess: a
/// mangled path would produce a confusing failure several steps later.
fn argument(path: &Path) -> Result<String, String> {
    path.to_str()
        .map(String::from)
        .ok_or_else(|| format!("path is not valid UTF-8: {}", path.display()))
}

/// A scratch directory OUTSIDE the repo.
///
/// Outside deliberately. A generated tree under the repo root gets judged by `text-hygiene` and
/// `max-lines` like a source file, and scratch directories under the root have already blocked
/// commits in this repo more than once.
fn scratch_dir() -> Result<PathBuf, String> {
    let dir = std::env::temp_dir().join(format!("sutura-check-api-docs-{}", std::process::id()));
    if dir.exists() {
        std::fs::remove_dir_all(&dir).map_err(|error| format!("could not clear {}: {error}", dir.display()))?;
    }
    std::fs::create_dir_all(&dir).map_err(|error| format!("could not create {}: {error}", dir.display()))?;
    Ok(dir)
}

/// Copy every generated page into `destination`, when the caller named one.
///
/// A PARAMETER and not a read of the environment, for the reason [`profile_args`] gives. An empty
/// name counts as unset, for the same reason: an unexpanded shell variable is a plumbing mistake.
fn keep(generated: &Path, destination: Option<&str>) -> Result<(), String> {
    let Some(destination) = destination.filter(|name| !name.is_empty()) else {
        return Ok(());
    };
    let destination = Path::new(destination);
    std::fs::create_dir_all(destination).map_err(|error| format!("could not create {}: {error}", destination.display()))?;
    let entries = std::fs::read_dir(generated).map_err(|error| format!("could not read {}: {error}", generated.display()))?;
    for entry in entries {
        let entry = entry.map_err(|error| format!("could not list {}: {error}", generated.display()))?;
        let to = destination.join(entry.file_name());
        std::fs::copy(entry.path(), &to).map_err(|error| format!("could not keep {}: {error}", to.display()))?;
    }
    Ok(())
}

/// Every library crate the generator wrote no page for.
///
/// It read the rustdoc JSON and produced nothing, which is a generator bug. Named per crate so the
/// finding says which input to look at, and checked against the set derived from `cargo metadata`
/// so a second library crate cannot arrive with no page and no failure.
fn missing_pages(generated: &Path, packages: &BTreeSet<String>) -> Vec<String> {
    packages
        .iter()
        .filter(|package| !generated.join(format!("{package}.md")).is_file())
        .map(|package| format!("  {package}: the generator wrote no page for this crate"))
        .collect()
}

fn report(problems: &[String]) {
    eprintln!("xtask check-api-docs: the API reference does not generate cleanly\n");
    for problem in problems {
        eprintln!("{problem}");
    }
    eprintln!();
    eprintln!("Run `just api` to write the pages under {PAGES_DIR}/ and see the generator's own output.");
    eprintln!("The text comes from the doc comments in the crate sources. Edit those, never a page:");
    eprintln!("nothing commits a page, so the next generation discards the edit.");
}

#[cfg(test)]
mod tests {
    use super::{
        colliding_json, is_lib_target, json_file_name, keep, library_packages, missing_pages, profile_args, python_command,
    };
    use std::collections::BTreeSet;

    #[test]
    fn a_binary_named_after_a_library_crate_is_refused() {
        // The binary and library rustdoc runs share one target directory - that sharing is what
        // stops the binary phase rebuilding the closure - so a binary whose Rust identifier
        // matches a library crate's would overwrite that crate's page input after it was
        // collected. Nothing downstream could tell that from the real page.
        let packages = BTreeSet::from([String::from("sutura-domain"), String::from("sutura-app")]);
        let collides = BTreeSet::from([(String::from("sutura-cli"), String::from("sutura_domain"))]);
        assert_eq!(
            colliding_json(&packages, &collides),
            Some(("sutura-cli", "sutura_domain")),
            "the refusal has to name the binary, because renaming it is the fix"
        );

        // And this workspace's own shape is not a collision: no binary target is named after a
        // library crate, so the gate still runs.
        let clear = BTreeSet::from([
            (String::from("sutura-cli"), String::from("sutura")),
            (String::from("xtask"), String::from("xtask")),
        ]);
        assert_eq!(colliding_json(&packages, &clear), None);
    }

    #[test]
    fn check_refuses_before_documenting_anything_when_the_lint_is_not_armed() {
        // THE COMPOSITION, and it is tested here rather than in `lints` for a measured reason:
        // every test in that module calls `arm` or `workspace_level` directly, so replacing the
        // call in `check` with a literal `Arming` left all of them green - measured on this
        // branch as `just test` exit 0, 2432 passed, with this gate printing its witness line
        // character for character while reading no manifest at all. This drives `check` itself
        // against a root that arms nothing, so the only error it can honestly return is the one
        // `lints::check` produces, and the second assertion is what says it came FIRST.
        let root = std::env::temp_dir().join(format!("sutura-api-docs-unarmed-{}", std::process::id()));
        drop(std::fs::remove_dir_all(&root));
        std::fs::create_dir_all(&root).expect("fixture root");
        std::fs::write(root.join("Cargo.toml"), "[workspace]\nmembers = []\n").expect("fixture manifest");

        let outcome = super::check(&root);
        drop(std::fs::remove_dir_all(&root));

        let error = match outcome {
            Ok(found) => panic!("an unarmed tree must not be documented, got {found:?}"),
            Err(error) => error,
        };
        assert!(
            error.contains("broken_intra_doc_links"),
            "the refusal has to be the LINT's - anything else means the precondition was skipped: {error}"
        );
        assert!(
            !error.contains(super::GENERATOR),
            "and it has to come BEFORE the generator is even looked for, or it is not first: {error}"
        );
    }

    #[test]
    fn a_library_crate_the_generator_wrote_no_page_for_is_named() {
        let tree = crate::scratch_tree::Tree::of(
            "api-docs-missing",
            &[("sutura-domain.md", b"page\n"), ("index.md", b"index\n")],
        );
        let packages = BTreeSet::from([String::from("sutura-domain"), String::from("sutura-app")]);
        let missing = missing_pages(tree.root(), &packages);
        assert_eq!(missing.len(), 1, "{missing:?}");
        assert!(
            missing[0].contains("sutura-app"),
            "the crate is what the reader looks for: {missing:?}"
        );
        assert_eq!(
            missing_pages(tree.root(), &BTreeSet::from([String::from("sutura-domain")])),
            Vec::<String>::new()
        );
    }

    #[test]
    fn pages_are_kept_only_where_a_destination_is_named() {
        let tree = crate::scratch_tree::Tree::of("api-docs-keep", &[("out/a.md", b"a\n"), ("out/index.md", b"i\n")]);
        let out = tree.root().join("out");
        let kept = tree.root().join("kept");
        keep(&out, None).expect("nothing to do");
        keep(&out, Some("")).expect("an empty name is unset");
        assert!(!kept.exists(), "no destination named, so nothing was written");
        keep(&out, kept.to_str()).expect("keep");
        assert_eq!(std::fs::read(kept.join("a.md")).expect("a page"), b"a\n");
        assert_eq!(std::fs::read(kept.join("index.md")).expect("the index"), b"i\n");
    }

    #[test]
    fn rustdoc_names_its_json_after_the_rust_identifier() {
        assert_eq!(json_file_name("sutura-domain"), "sutura_domain.json");
        assert_eq!(json_file_name("xtask"), "xtask.json");
    }

    #[test]
    fn a_lib_target_is_told_from_a_binary_and_a_proc_macro() {
        let lib = serde_json::json!({ "kind": ["lib"], "name": "sutura-domain" });
        let rlib = serde_json::json!({ "kind": ["lib", "rlib"], "name": "sutura-domain" });
        let bin = serde_json::json!({ "kind": ["bin"], "name": "sutura" });
        let macros = serde_json::json!({ "kind": ["proc-macro"], "name": "derive" });
        assert!(is_lib_target(&lib));
        assert!(is_lib_target(&rlib));
        assert!(!is_lib_target(&bin));
        assert!(!is_lib_target(&macros), "a proc-macro has no surface to render");
        assert!(!is_lib_target(&serde_json::json!({ "name": "no kind at all" })));
    }

    #[test]
    fn library_packages_are_derived_and_binaries_are_not_included() {
        let metadata = serde_json::json!({
            "packages": [
                { "name": "sutura-domain", "targets": [{ "kind": ["lib"] }] },
                { "name": "sutura-cli", "targets": [{ "kind": ["bin"] }] },
                { "name": "xtask", "targets": [{ "kind": ["bin"] }] },
            ]
        });
        let libs = library_packages(&metadata).expect("one lib target exists");
        assert_eq!(libs.len(), 1);
        assert!(libs.contains("sutura-domain"));
    }

    #[test]
    fn a_workspace_with_no_library_is_an_error_not_a_pass() {
        // The vacuous-pass case: if the metadata scan ever stops recognising a lib target,
        // this gate must say so rather than report `ok` having compared nothing.
        let metadata = serde_json::json!({
            "packages": [{ "name": "sutura-cli", "targets": [{ "kind": ["bin"] }] }]
        });
        let error = library_packages(&metadata).expect_err("no lib target must be an error");
        assert!(error.contains("no workspace package with a `lib` target"), "{error}");
    }

    #[test]
    fn a_profile_is_passed_only_when_one_is_named() {
        // Unset is the dev-shell answer: cargo's default `dev` keeps the developer's `target/`
        // warm. The Nix check names `ci`, and it must arrive as a FLAG - `CARGO_PROFILE` is
        // crane's convention and a spawned child sees nothing that turns it into one.
        assert!(
            profile_args(None).is_empty(),
            "the dev-shell default passes no --profile flags"
        );
        assert!(
            profile_args(Some("")).is_empty(),
            "an empty value gives cargo a bare --profile"
        );
        assert_eq!(profile_args(Some("ci")), ["--profile", "ci"]);
    }

    #[test]
    fn the_interpreter_defaults_to_pixi_and_can_be_overridden() {
        // pixi by default because the dev shell puts no interpreter on PATH: one resolver owns
        // Python here. The override exists for the Nix sandbox, which has no network.
        let (program, args) = python_command(None);
        assert_eq!(program, "pixi");
        assert_eq!(args, ["run", "--frozen", "python"]);

        let (program, args) = python_command(Some("/nix/store/x/bin/python3"));
        assert_eq!(program, "/nix/store/x/bin/python3");
        assert!(args.is_empty(), "an explicit interpreter takes no pixi arguments");
    }
}
