//! Are the committed API reference pages still what the generator produces?
//!
//! `docs/api/*.md` are GENERATED files that are also COMMITTED. Committed because rustdoc JSON
//! is nightly-only while the docs site is built by a job that has stable and nothing else, so
//! the pages have to exist as files rather than be produced on the way to publishing.
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
//!   * IT IS THE SAME CODE PATH. The `cargo rustdoc` line and the generator script are the ones
//!     the `api` recipe in the justfile runs. A gate that reimplemented the rendering could
//!     disagree with `just api`, and then the fix its own message asks for would not make it
//!     pass. Never inline the rendering here.
//!
//! WHY IT IS SPECIAL-CASED ONTO NIGHTLY: `--output-format json` is an unstable rustdoc option,
//! so stable rejects `-Z` outright. Every other gate is run with `nix/stable-env.sh` sourced
//! first, because clippy's lint set differs between channels and this workspace gates on the
//! whole `restriction` category. This one must NOT be wrapped that way - wrapping it is the one
//! thing that breaks it. It is `Kind::Standalone` for the same reason: `cargo xtask hygiene` is
//! a cheap sweep that runs on hosts with no Rust nightly at all.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::Verdict;
use crate::repo;

/// Where the committed pages live, relative to the repo root.
const PAGES_DIR: &str = "docs/api";

/// The generator, relative to the repo root.
///
/// Run as a script rather than reimplemented, so this gate and `just api` cannot disagree
/// about what a page should contain.
const GENERATOR: &str = "docs/.tools/rustdoc_to_markdown.py";

/// The first line of the header every generated page carries.
///
/// Used to tell a generated page from a hand-written one, which is how a page left behind by a
/// renamed crate is found. `docs/api/index.md` is hand-written and carries no such header.
const GENERATED_MARKER: &str = "<!-- GENERATED FILE - do not edit.";

/// Names an interpreter to run the generator with, bypassing pixi.
///
/// For the one caller that cannot use pixi: a Nix build sandbox has no network, so it cannot
/// materialise a pixi environment. See [`python_command`].
const PYTHON_ENV: &str = "SUTURA_API_DOCS_PYTHON";

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
        Ok(problems) if problems.is_empty() => {
            println!("xtask check-api-docs: ok - the committed pages match a fresh generation");
            Verdict::Pass
        }
        Ok(problems) => {
            report(&problems);
            Verdict::Fail
        }
    }
}

/// Regenerate every library crate's page and collect what disagrees.
///
/// `Err` is reserved for "the check could not be performed" - a missing generator, a rustdoc
/// that refused to run. That is deliberately NOT reported as a clean repo: a gate that cannot
/// run has found nothing, and reporting nothing as `ok` is how a pipeline goes green over an
/// unchecked tree.
fn check(root: &Path) -> Result<Vec<String>, String> {
    let metadata = crate::cargo_metadata(&["--no-deps"])?;
    let packages = library_packages(&metadata)?;
    let target_dir = target_directory(root, &metadata);

    let generator = root.join(GENERATOR);
    if !generator.is_file() {
        return Err(format!("{GENERATOR} does not exist - there is nothing to compare against"));
    }

    let cargo = cargo_bin();
    let mut inputs = Vec::new();
    for package in &packages {
        rustdoc_json(&cargo, root, package)?;
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
    Ok(problems)
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
/// The arguments are the ones in the justfile's `api` recipe, in the same order, for the
/// same-code-path reason in this module's header.
fn rustdoc_json(cargo: &str, root: &Path, package: &str) -> Result<(), String> {
    let status = std::process::Command::new(cargo)
        .current_dir(root)
        .args(["rustdoc", "-q", "-p", package, "--all-features"])
        .args(["--", "-Z", "unstable-options", "--output-format", "json"])
        .status()
        .map_err(|error| format!("could not run `{cargo} rustdoc`: {error}"))?;
    if status.success() {
        return Ok(());
    }
    Err(format!(
        "`cargo rustdoc -p {package} ... --output-format json` failed.\n  \
         That option is unstable, so this gate needs the NIGHTLY toolchain \
         (devco/rust-toolchain-nightly.toml).\n  \
         Every other gate sources nix/stable-env.sh; this one must not, because stable \
         rejects `-Z` outright."
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
    use super::{excerpt, first_difference, is_lib_target, json_file_name, library_packages, python_command};

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
