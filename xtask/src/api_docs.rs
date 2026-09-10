//! Are the committed API reference pages still what the generator produces?
//!
//! `docs/api/*.md` are GENERATED files that are also COMMITTED. Committed because rustdoc JSON
//! is nightly-only, so the pages have to exist as files rather than be produced on the way to
//! publishing - and the whole tree already builds on the single pinned nightly, so generating
//! them needs nothing beyond that same toolchain.
//!
//! A committed generated file rots silently. A doc comment changes, the page does not, and the
//! stale page reads as current - nothing in the pipeline could tell the difference. This gate
//! is what fails when that happens, and it is the whole difference between "generated docs" and
//! "generated docs you can trust".
//!
//! HOW: produce the rustdoc JSON, run the generator into a temporary directory, byte-compare
//! against what is committed. Two properties fall out of that shape, and both are why it is
//! this shape rather than a cleverer one:
//!
//!   * IT IS SELF-SCOPING. A change that touches only private code regenerates to identical
//!     output and passes. So this gate needs no model of which Rust changes reach the public
//!     surface, and therefore cannot get that model wrong. Resist adding one: a heuristic that
//!     decides a change "cannot affect the docs" is a heuristic that will one day be wrong
//!     silently, which is the exact failure this gate exists to remove.
//!   * IT NOW ALSO JUDGES THE DOC LINKS, and not by looking at them. `broken_intra_doc_links` is
//!     forbidden in the workspace lint table, so the `cargo rustdoc` line below fails on a link
//!     rustdoc cannot resolve - 15 of those were warnings behind exit 0 until
//!     `github.com/telekom/sutura#360`. **This is the only venue that enforces it**: the doctest
//!     lane does not, measured. This check also documents each binary-only target, without
//!     rendering API pages for it; `lints` states that scope. A lint also cannot say
//!     whether it was ARMED, so that submodule asserts the table exists and that every workspace
//!     member inherits it, before a single crate is documented. That is a precondition rather
//!     than a finding: an unarmed run compares pages nobody judged. The test that holds the
//!     precondition is over THIS function rather than over that module - see
//!     `check_refuses_before_documenting_anything_when_the_lint_is_not_armed`, which exists
//!     because deleting the call and handing `check` a literal left every test in `lints` green.
//!   * IT IS THE SAME CODE PATH. The `cargo rustdoc` line and the generator script are the ones
//!     the `api` recipe in the justfile runs. A gate that reimplemented the rendering could
//!     disagree with `just api`, and then the fix its own message asks for would not make it
//!     pass. Never inline the rendering here.
//!
//! RUSTDOC JSON NEEDS NIGHTLY. `--output-format json` is an unstable rustdoc option, and the
//! whole toolchain is nightly now - the shell's bare cargo and every gate run the pinned nightly
//! (devco/rust-toolchain-nightly.toml) - so it is not a special case next to gates that ran on a
//! different channel. `flake.nix` hands the check a nightly `$CARGO` and a `CARGO_TARGET_DIR` of
//! its own so the docs never share artifacts with another run's, and `SUTURA_API_DOCS_PROFILE` so
//! its compile is at opt-level 0.
//!
//! Every gate runs on the nightly toolchain now, so the `cargo rustdoc` line here needs no
//! wrapping or un-wrapping rationale. It is `Kind::Standalone` because `cargo xtask hygiene` is a
//! cheap sweep that should run on hosts with no Rust nightly at all.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::Verdict;
use crate::repo;

/// Whether the rustdoc run below is armed to judge doc links at all.
mod lints;

/// Where the committed pages live, relative to the repo root.
///
/// Shared with `check-api-links`, which reads the same directory. Two spellings of one directory
/// is how a gate ends up scanning somewhere the generator no longer writes.
pub(crate) const PAGES_DIR: &str = "docs/api";

/// The generator, relative to the repo root.
///
/// Run as a script rather than reimplemented, so this gate and `just api` cannot disagree
/// about what a page should contain.
///
/// Shared with `check-api-links`, which reads the same file for a different reason: the two hold
/// one list of real URL schemes between them, in two languages, and that gate compares them.
pub(crate) const GENERATOR: &str = "docs/.tools/rustdoc_to_markdown.py";

/// The first line of the header every generated page carries.
///
/// Used to tell a generated page from a hand-written one, which is how a page left behind by a
/// renamed crate is found. `docs/api/index.md` is hand-written and carries no such header.
///
/// Shared with `check-api-links`, where it is the FLOOR: a scan of that directory finding no
/// generated page at all is a failure rather than a clean sweep.
pub(crate) const GENERATED_MARKER: &str = "<!-- GENERATED FILE - do not edit.";

/// Names an interpreter to run the generator with, bypassing pixi.
///
/// For the one caller that cannot use pixi: a Nix build sandbox has no network, so it cannot
/// materialise a pixi environment. See [`python_command`].
const PYTHON_ENV: &str = "SUTURA_API_DOCS_PYTHON";

/// Names the cargo profile the `cargo rustdoc` child compiles under.
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

/// How much of a differing line to print. Long enough to recognise, short enough that a
/// hundred-column signature does not wrap the verdict into unreadability.
const EXCERPT_CHARS: usize = 90;

/// `xtask check-api-docs` - regenerate the API pages and byte-compare against the committed ones.
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
            println!("xtask check-api-docs: ok - the committed pages match a fresh generation");
            println!(
                "  doc links: `broken_intra_doc_links` = {}, {} member(s) declared in Cargo.toml, \
                 {} resolved by cargo metadata, {} inheriting",
                outcome.arming.level, outcome.arming.declared, outcome.arming.resolved, outcome.arming.inheriting
            );
            println!(
                "  {} binary-only target(s) documented for links; API pages remain library-only",
                outcome.binaries
            );
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
/// verdict that says the pages match without saying the rustdoc run behind them judged its links
/// is the shape this pair exists to make impossible.
#[derive(Debug)]
struct Outcome {
    /// Pages that disagree with a fresh generation, or that nothing accounts for.
    problems: Vec<String>,
    /// The doc-link lint's level, and the three counts that say it reached every member.
    arming: lints::Arming,
    /// Binary-only targets whose rustdoc invocation succeeded, not generated pages.
    binaries: usize,
}

/// Regenerate every library crate's page and collect what disagrees.
///
/// `Err` is reserved for "the check could not be performed" - a missing generator, a rustdoc
/// that refused to run. That is deliberately NOT reported as a clean repo: a gate that cannot
/// run has found nothing, and reporting nothing as `ok` is how a pipeline goes green over an
/// unchecked tree.
fn check(root: &Path) -> Result<Outcome, String> {
    let metadata = crate::cargo_metadata(&["--no-deps"])?;
    // FIRST, and it is an `Err` rather than a problem: an unarmed rustdoc run cannot tell a
    // resolvable doc link from an unresolvable one, so regenerating pages from it and reporting
    // that they match would be a green verdict over a question nobody asked. See `lints`.
    let arming = lints::check(root, &metadata)?;
    let packages = library_packages(&metadata)?;
    let binaries = binary_targets(&metadata)?;
    let target_dir = target_directory(root, &metadata);

    let generator = root.join(GENERATOR);
    if !generator.is_file() {
        return Err(format!("{GENERATOR} does not exist - there is nothing to compare against"));
    }
    selftest_renderer(root, &generator)?;

    let cargo = cargo_bin();
    let mut inputs = Vec::new();
    for package in &packages {
        rustdoc_json(&cargo, root, package, None)?;
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
    // Binary and library Rust identifiers can coincide. Their JSON must not share a directory.
    let binary_target_dir = target_dir.join("binary-api-docs");
    for (package, binary) in &binaries {
        rustdoc_json(&cargo, root, package, Some((binary, &binary_target_dir)))
            .map_err(|error| format!("binary target `{package}/{binary}`: {error}"))?;
    }

    let scratch = scratch_dir()?;
    let generated = generate(root, &generator, &inputs, &scratch);
    let mut problems = Vec::new();
    if let Err(error) = generated {
        drop(std::fs::remove_dir_all(&scratch));
        return Err(error);
    }
    for package in &packages {
        compare(root, &scratch, package, &mut problems);
    }
    problems.extend(orphan_pages(root, &packages));

    // Best-effort: a scratch directory left behind is untidy, not a verdict.
    drop(std::fs::remove_dir_all(&scratch));
    Ok(Outcome {
        problems,
        arming,
        binaries: binaries.len(),
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

/// The cargo to invoke.
///
/// The environment variable rather than `env!("CARGO")`: the compile-time form bakes cargo's
/// absolute store path into the binary, which puts the whole cargo closure into the shipped
/// package's runtime closure. Cargo sets this whenever it invokes us.
fn cargo_bin() -> String {
    std::env::var("CARGO").unwrap_or_else(|_| String::from("cargo"))
}

/// Produce one crate's rustdoc JSON.
///
/// The arguments are the ones the `api` recipe's writer uses, in the same order, for the
/// same-code-path reason in this module's header.
///
/// THE ONE STEP THAT NEEDS NIGHTLY, and it is a CHILD PROCESS. `cargo` here is whichever cargo
/// the caller named - `$CARGO`, which the Nix check sets to the nightly. This binary itself is
/// compiled on the same nightly toolchain every gate uses, which is why the check can share their
/// dependency closure.
fn rustdoc_json(cargo: &str, root: &Path, package: &str, binary: Option<(&str, &Path)>) -> Result<(), String> {
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
        .args(["rustdoc", "-q", "-p", package, "--all-features"])
        .args(binary.into_iter().flat_map(|(name, _)| ["--bin", name]))
        .envs(binary.map(|(_, target)| ("CARGO_TARGET_DIR", target)))
        .args(profile_args(profile.as_deref()))
        .args(["--", "-Z", "unstable-options", "--output-format", "json"])
        .status()
        .map_err(|error| format!("could not run `{cargo} rustdoc`: {error}"))?;
    if status.success() {
        return Ok(());
    }
    Err(format!(
        "`cargo rustdoc -p {package} ... --output-format json` failed, and rustdoc's own output \
         above says which of two things happened.\n  \
         A DOC LINK it could not resolve: `broken_intra_doc_links` is forbidden in the workspace \
         lint table, so that is an error here rather than a warning behind exit 0. Fix the link - the \
         `crate::`-qualified inline form `[`x`](crate::path::x)` resolves without an import and \
         the page keeps the code span.\n  \
         Or the TOOLCHAIN: `--output-format json` is unstable, so this gate needs the nightly pin \
         (devco/rust-toolchain-nightly.toml), which is what the shell's bare `cargo` IS."
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

/// Run the generator's own behavioral re-export fixture, once, before any page is compared.
///
/// `check-api-docs` byte-compares the committed pages against a fresh generation, which proves
/// the pages are CURRENT - and, as `github.com/telekom/sutura#470` measured, does NOT prove the
/// renderer INCLUDES the documentation an item carries. A `pub use` re-export carries its public
/// name and its docs on the TARGET (`inner.use.name`, `inner.use.id`), not on the re-export item
/// itself, and the renderer used to print a literal `use None` for it with the gate green. The
/// generator's `--self-test` renders a hand-built re-export fixture and asserts the caller-visible
/// name and the target's documentation appear, so a future renderer regression fails here even if
/// every crate happened to change. It is the same script and the same pixi/python interpreter as
/// [`generate`], because the gate and the fix must exercise the same code path.
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
        "the generator's re-export self-test failed. A public re-export must render its \
         caller-visible name and its target's documentation, not `use None`; run `{GENERATOR} \
         --self-test` the way this gate does to see the assertion."
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

/// Render every JSON input into `out`, using the committed generator script.
fn generate(root: &Path, generator: &Path, inputs: &[PathBuf], out: &Path) -> Result<(), String> {
    let override_path = std::env::var(PYTHON_ENV).ok();
    let (program, mut argv) = python_command(override_path.as_deref());
    argv.push(argument(generator)?);
    for input in inputs {
        argv.push(argument(input)?);
    }
    // The script takes the output directory as an optional last argument and defaults to
    // `docs/api`. Passing one is what keeps this gate read-only over the committed pages.
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

/// Compare one crate's committed page against the freshly generated one.
fn compare(root: &Path, generated_dir: &Path, package: &str, problems: &mut Vec<String>) {
    let page = root.join(PAGES_DIR).join(format!("{package}.md"));
    let fresh = generated_dir.join(format!("{package}.md"));

    let Ok(generated) = std::fs::read(&fresh) else {
        problems.push(format!(
            "  {package}: the generator wrote no page for this crate. It read the rustdoc \
             JSON and produced nothing, which is a generator bug rather than a stale page."
        ));
        return;
    };
    let Ok(committed) = std::fs::read(&page) else {
        problems.push(format!(
            "  {}: missing. A library crate exists with no committed page.",
            relative(root, &page)
        ));
        return;
    };
    if committed == generated {
        return;
    }

    match first_difference(&committed, &generated) {
        Some(difference) => problems.push(format!(
            "  {}: differs, first at line {}\n      committed: {}\n      generated: {}",
            relative(root, &page),
            difference.line,
            difference.committed,
            difference.generated
        )),
        // Equal line by line but not byte for byte: only the trailing newline can differ.
        None => problems.push(format!(
            "  {}: differs in trailing bytes only - a missing or extra final newline.",
            relative(root, &page)
        )),
    }
}

/// A page under `docs/api/` that carries the generated header but that no library crate
/// accounts for.
///
/// Renaming or deleting a library crate otherwise leaves its page behind, still headed
/// "GENERATED FILE", still in the nav, describing a crate that no longer exists. The
/// byte-compare cannot see that: it only ever looks at pages it expects.
fn orphan_pages(root: &Path, expected: &BTreeSet<String>) -> Vec<String> {
    let dir = root.join(PAGES_DIR);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return vec![format!("  {}: cannot be read", relative(root, &dir))];
    };

    let mut problems = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let is_markdown = path.extension().and_then(std::ffi::OsStr::to_str) == Some("md");
        let stem = path.file_stem().and_then(std::ffi::OsStr::to_str).map(String::from);
        let Some(stem) = stem.filter(|_| is_markdown) else {
            continue;
        };
        if expected.contains(&stem) {
            continue;
        }
        // Hand-written pages live here too - the landing page among them - so the header is
        // what decides, not the file name.
        if std::fs::read_to_string(&path).is_ok_and(|text| text.starts_with(GENERATED_MARKER)) {
            problems.push(format!(
                "  {}: generated, but `{stem}` is not a library crate in this workspace.\n      \
                 A renamed or deleted crate leaves its page behind; delete it and its nav entry.",
                relative(root, &path)
            ));
        }
    }
    problems.sort_unstable();
    problems
}

/// Where two files first disagree, and what each says there.
struct Difference {
    line: usize,
    committed: String,
    generated: String,
}

/// The first line at which two files differ.
///
/// A line number and an excerpt rather than a full diff: the reader's next action is `just api`,
/// not a manual edit, so the message needs to prove the pages disagree and say where - not
/// describe every change.
fn first_difference(committed: &[u8], generated: &[u8]) -> Option<Difference> {
    let committed = String::from_utf8_lossy(committed);
    let generated = String::from_utf8_lossy(generated);
    let mut left = committed.lines();
    let mut right = generated.lines();
    let mut number = 0_usize;
    loop {
        number += 1;
        match (left.next(), right.next()) {
            // Both sides ran out together, so every line agreed.
            (None, None) => return None,
            (was, now) if was == now => {}
            (was, now) => {
                return Some(Difference {
                    line: number,
                    committed: excerpt(was),
                    generated: excerpt(now),
                });
            }
        }
    }
}

/// One line, shortened, or a marker for "this file already ended".
fn excerpt(line: Option<&str>) -> String {
    match line {
        None => String::from("(end of file)"),
        Some(text) if text.chars().count() > EXCERPT_CHARS => {
            let mut out: String = text.chars().take(EXCERPT_CHARS).collect();
            out.push_str("...");
            out
        }
        Some(text) => String::from(text),
    }
}

/// A repo-relative path for a message, falling back to the absolute one.
fn relative(root: &Path, path: &Path) -> String {
    repo::relative(root, path).unwrap_or_else(|| path.display().to_string())
}

fn report(problems: &[String]) {
    eprintln!("xtask check-api-docs: the committed API pages are not what the generator produces\n");
    for problem in problems {
        eprintln!("{problem}");
    }
    eprintln!();
    eprintln!("Run `just api` and commit what it writes.");
    eprintln!("The text comes from the doc comments in the crate sources. Edit those, never a page:");
    eprintln!("every page carries a header saying so, and the next regeneration discards the edit.");
}

#[cfg(test)]
mod tests {
    use super::{excerpt, first_difference, is_lib_target, json_file_name, library_packages, profile_args, python_command};

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
    fn identical_files_have_no_first_difference() {
        let text = b"# a\n\nbody\n";
        assert!(first_difference(text, text).is_none());
    }

    #[test]
    fn a_changed_line_is_located() {
        let committed = b"# a\nold text\ntail\n";
        let generated = b"# a\nnew text\ntail\n";
        let found = first_difference(committed, generated).expect("they differ");
        assert_eq!(found.line, 2, "the line number is what sends the reader to the right place");
        assert_eq!(found.committed, "old text");
        assert_eq!(found.generated, "new text");
    }

    #[test]
    fn a_truncated_file_reports_the_end() {
        let committed = b"one\ntwo\n";
        let generated = b"one\n";
        let found = first_difference(committed, generated).expect("they differ");
        assert_eq!(found.line, 2);
        assert_eq!(found.committed, "two");
        assert_eq!(found.generated, "(end of file)");
    }

    #[test]
    fn a_trailing_newline_difference_has_no_differing_line() {
        // `lines()` cannot see it, which is why `compare` has a branch for `None` rather than
        // treating it as "equal". The byte comparison is the authority.
        assert!(first_difference(b"one\n", b"one").is_none());
    }

    #[test]
    fn a_long_line_is_shortened_but_still_recognisable() {
        let long = "x".repeat(200);
        let shortened = excerpt(Some(&long));
        assert!(shortened.len() < long.len());
        assert!(shortened.ends_with("..."));
        assert!(shortened.starts_with("xxxx"));
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
