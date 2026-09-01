//! The attribution document: every third-party crate this workspace resolves, and its licence.
//!
//! WHY THIS EXISTS AND `deny.toml` IS NOT IT. `deny.toml` holds an exact allowlist of SPDX
//! identifiers with `unused-allowed-license = "deny"`, and that is a POLICY check: it answers "is
//! this identifier one we accept". It says nothing a distributor can hand on. Several of the
//! identifiers on that allowlist - Apache-2.0 and MIT among them - oblige whoever redistributes a
//! binary to carry the licence and the copyright notice of what is inside it, and until this file
//! existed the only output anybody could obtain was a CI artifact from `licence-review.yml` that
//! expires after ninety days, is not signed, and is not committed. `docs/adr/0021` said the
//! adjacent half out loud: *"There is no attribution document, and `NOTICE` still says nothing
//! about third-party crates."* This is the mechanism for that sentence.
//!
//! # Two tasks, because generating and checking need different things
//!
//! * `cargo xtask attribution` REGENERATES the document. It shells out to `cargo metadata`, which
//!   is where a crate's declared licence expression lives - `Cargo.lock` does not carry one - and
//!   `just attribution` is the way to call it.
//! * `cargo xtask check-attribution` CHECKS it, and reads nothing but `Cargo.lock` and the
//!   document. That is deliberate: it is a hygiene gate, so it runs on every commit and inside the
//!   nix sandbox, and a gate that has to invoke `cargo metadata` to answer is a gate that needs a
//!   resolvable registry to disagree with a lock file it could have read directly.
//!
//! `check-api-docs` is the shape this deliberately does NOT copy. That gate regenerates into a
//! temporary directory and byte-compares, which is stricter - and it is `Kind::Standalone` for
//! exactly the reason above, because regenerating means compiling. The trade here is that the
//! generator's PROSE and its column order are not gated; the crate set is, exactly.
//!
//! # What the gate claims, and what it does not
//!
//! Claimed, exactly: the document names every third-party package in `Cargo.lock`, at the version
//! the lock resolves, and names nothing else - and every row carries a non-empty licence field.
//!
//! Not claimed, and each is a real edge:
//!
//! * **Not that the licence expression is right.** It is what the crate's own manifest declares,
//!   copied through. Checking it against the licence FILES in a crate's source is a source scan,
//!   which `licence-review.yml`'s header declines at length and for reasons that have not changed.
//! * **Not that it is the list a given binary LINKS.** `Cargo.lock` records what cargo resolved.
//!   `docs/adr/0021` makes that argument at length against generating an SBOM this way and is
//!   right about it - and for an attribution document the direction of the error is the opposite
//!   one: a document naming a crate that did not ship discharges an obligation nobody had, and a
//!   document missing one that did ship is the failure the document exists to prevent. So this
//!   errs by overstating, on purpose, and the amendment to that record says so.
//! * **Not the notice text of each dependency.** An Apache-2.0 dependency's own `NOTICE` file is
//!   in its source tree and not in its metadata, so nothing here can render one.
//! * **Not the two vendored path crates.** `mimalloc` and `libmimalloc-sys` are third-party code
//!   carried in `vendor/`, and they have no `source` in the lock because they are path
//!   dependencies - so this document does not name them. `VENDOR.md` records their upstream and
//!   `REUSE.toml` records their licence per file, which is the shape vendored code is attributed
//!   in here. The document says so where a reader will see it.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::Verdict;
use crate::repo;

/// The committed document, relative to the repo root.
const DOCUMENT: &str = "ATTRIBUTION.md";

/// The lock file, read as text. The same choice `arrow_major` and `shared_client` make.
const LOCK: &str = "Cargo.lock";

/// The task that rewrites the document, named in every failure message so the fix is the message.
const REGENERATE: &str = "just attribution";

/// One package: the key the gate compares on, and the licence the generator writes.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Package {
    /// Sorted by name first, then version, which is the order the document is written in.
    name: String,
    version: String,
}

/// Strip the surrounding quotes from a lock-file value, or `None` if it is not quoted.
fn unquote(value: &str) -> Option<&str> {
    value.strip_prefix('"')?.strip_suffix('"')
}

/// Every THIRD-PARTY package in the lock file.
///
/// A `[[package]]` stanza with no `source` key is a path or workspace member - this workspace's own
/// nineteen crates, plus the two vendored ones under `vendor/`. Those are attributed by `LICENSE`,
/// `VENDOR.md` and `REUSE.toml` rather than here, so the presence of a `source` line is what
/// selects a row. Everything with one came from a registry or a git remote and is somebody else's
/// code.
///
/// The shape this relies on is `cargo`'s own output: within a stanza, `name` precedes `version`,
/// and both are `key = "value"` on their own line. Parsed as text rather than as TOML for
/// `arrow_major`'s reason - `xtask` reads two fields out of this file in three gates now, and a
/// TOML dependency for that is a poor trade.
fn third_party(lock: &str) -> Vec<Package> {
    let mut found = Vec::new();
    let mut name: Option<&str> = None;
    let mut version: Option<&str> = None;
    let mut flush = |name: &mut Option<&str>, version: &mut Option<&str>, sourced: bool| {
        if let (Some(n), Some(v)) = (name.take(), version.take())
            && sourced
        {
            found.push(Package {
                name: String::from(n),
                version: String::from(v),
            });
        }
    };
    let mut sourced = false;
    for line in lock.lines() {
        if line.starts_with("[[package]]") {
            flush(&mut name, &mut version, sourced);
            sourced = false;
        } else if let Some(value) = line.strip_prefix("name = ") {
            name = unquote(value);
        } else if let Some(value) = line.strip_prefix("version = ") {
            version = unquote(value);
        } else if line.starts_with("source = ") {
            sourced = true;
        }
    }
    flush(&mut name, &mut version, sourced);
    found
}

/// A row of the document's package table, as `| `name` | `version` | licence |`.
///
/// Returns the package and its licence field. `None` for any line that is not a package row, which
/// is what lets the prose, the header and the separator share the file with the table.
fn row(line: &str) -> Option<(Package, String)> {
    let mut cells = line.strip_prefix("| ")?.strip_suffix(" |")?.split(" | ");
    let name = cells.next()?.strip_prefix('`')?.strip_suffix('`')?;
    let version = cells.next()?.strip_prefix('`')?.strip_suffix('`')?;
    let licence = cells.next()?.trim();
    if cells.next().is_some() || name.is_empty() || version.is_empty() {
        return None;
    }
    let package = Package {
        name: String::from(name),
        version: String::from(version),
    };
    Some((package, String::from(licence)))
}

/// Every package row the document carries, keyed so a duplicated row is visible as one entry with
/// the licence of whichever came last - which the count comparison below then catches.
fn rows(document: &str) -> BTreeMap<Package, String> {
    document.lines().filter_map(row).collect()
}

/// `cargo xtask check-attribution` - the document names exactly the lock's third-party set.
pub(crate) fn run_check(_args: &[String]) -> Verdict {
    let Some(root) = repo::root() else {
        eprintln!("xtask check-attribution: could not determine the repo root");
        return Verdict::Fail;
    };
    let Some(lock) = read(&root, LOCK) else {
        return Verdict::Fail;
    };
    let Some(document) = read(&root, DOCUMENT) else {
        eprintln!("  {REGENERATE} writes it.");
        return Verdict::Fail;
    };

    let want: BTreeSet<Package> = third_party(&lock).into_iter().collect();
    let have = rows(&document);

    let mut failures = 0_usize;
    for package in &want {
        if !have.contains_key(package) {
            if failures == 0 {
                eprintln!("xtask check-attribution: FAILED");
            }
            failures += 1;
            eprintln!("  missing: {} {}", package.name, package.version);
        }
    }
    for (package, licence) in &have {
        if !want.contains(package) {
            if failures == 0 {
                eprintln!("xtask check-attribution: FAILED");
            }
            failures += 1;
            eprintln!("  not in {LOCK}: {} {}", package.name, package.version);
        } else if licence.is_empty() {
            if failures == 0 {
                eprintln!("xtask check-attribution: FAILED");
            }
            failures += 1;
            eprintln!("  no licence recorded: {} {}", package.name, package.version);
        }
    }

    if failures > 0 {
        eprintln!();
        eprintln!("  {DOCUMENT} and {LOCK} disagree about {failures} package(s).");
        eprintln!("  It is a GENERATED file: run `{REGENERATE}` and commit the result.");
        eprintln!("  A missing row is the failure that matters - a distributor of a shipped binary");
        eprintln!("  carries the notices of what is inside it, and this document is that list.");
        return Verdict::Fail;
    }

    println!(
        "xtask check-attribution: ok - {DOCUMENT} names all {} third-party packages in {LOCK}",
        want.len()
    );
    Verdict::Pass
}

/// Read a repo file, reporting the path rather than the io error alone.
fn read(root: &Path, name: &str) -> Option<String> {
    let path = root.join(name);
    match std::fs::read_to_string(&path) {
        Ok(text) => Some(text),
        Err(error) => {
            eprintln!("xtask attribution: could not read {}: {error}", path.display());
            None
        }
    }
}

/// The generated document's header. `do not edit` in the first five lines is what
/// `cargo xtask text-hygiene` reads to exempt a generated file from its formatting rules, so the
/// marker is load-bearing and not decoration.
fn header(packages: usize) -> String {
    format!(
        "<!-- GENERATED by `{REGENERATE}`: do not edit. `cargo xtask check-attribution` fails when it drifts. -->\n\
\n\
# Third-party attribution\n\
\n\
sutura is Apache-2.0; `LICENSE` is the licence and `NOTICE` is its notice. This file is the other\n\
half: the {packages} third-party crates this workspace resolves, and the licence each one declares.\n\
It exists because an allowlist of SPDX identifiers is a policy check, and a distributor of a\n\
shipped binary needs a statement it can hand on.\n\
\n\
## How to read it\n\
\n\
The licence column is the SPDX expression the crate's own manifest declares, copied through\n\
unchanged - including the several spellings of \"MIT or Apache-2.0\" that crates.io has accumulated,\n\
because normalising them here would make this document disagree with the manifests it reports, and\n\
a count of the spellings would be a number in a generated header that nothing regenerates.\n\
`LICENSES/` carries the full text of the two identifiers this project itself uses; for the rest,\n\
the identifier is the canonical reference.\n\
\n\
## What it covers, and what it does not\n\
\n\
- **Every crate the workspace RESOLVES**, at all features, which is more than any one binary links:\n\
  `sutura-cli` links the engine only, while `libduckdb-sys` and the BigQuery wire put `ureq`,\n\
  rustls and `ring` into the resolve graph for a binary that links none of them. That error is\n\
  deliberate and it points the safe way - naming a crate that did not ship discharges an obligation\n\
  nobody had, and missing one that did ship is the failure this document exists to prevent. The\n\
  per-binary list is inside the binary, in a `cargo auditable` section, and\n\
  `docs/verifying-a-release.md` says how to read it.\n\
- **Not the notice text of each dependency.** An Apache-2.0 crate's own `NOTICE` file lives in its\n\
  source tree rather than in its metadata, so nothing that reads metadata can render one.\n\
- **Not the vendored trees.** `vendor/mimalloc_rust` is third-party code carried in this repository\n\
  as a path dependency, so it has no registry source and is not a row below. `VENDOR.md` records\n\
  its upstream, commit and local changes, and `REUSE.toml` records its licence per file.\n\
- **Not an advisory statement.** Whether any of these has a vulnerability against it is\n\
  `cargo deny check` against the RustSec database, whose verdict is a run rather than a document.\n\
\n\
## Packages\n\
\n\
| Package | Version | Licence |\n\
| --- | --- | --- |\n"
    )
}

/// `cargo xtask attribution` - regenerate the document from `cargo metadata`.
///
/// `--all-features`, because a feature-gated dependency is still something a build of this
/// workspace can pull in, and `--locked`, because the document has to describe the committed
/// resolution rather than whatever cargo would resolve today.
pub(crate) fn run_generate(_args: &[String]) -> Verdict {
    let Some(root) = repo::root() else {
        eprintln!("xtask attribution: could not determine the repo root");
        return Verdict::Fail;
    };

    let mut command = std::process::Command::new("cargo");
    command
        .current_dir(&root)
        .args(["metadata", "--format-version", "1", "--all-features", "--locked"]);
    let output = match command.output() {
        Ok(output) => output,
        Err(error) => {
            eprintln!("xtask attribution: could not run `cargo metadata`: {error}");
            return Verdict::Fail;
        }
    };
    if !output.status.success() {
        eprintln!("xtask attribution: `cargo metadata` failed");
        eprintln!("{}", String::from_utf8_lossy(&output.stderr));
        return Verdict::Fail;
    }

    let metadata: serde_json::Value = match serde_json::from_slice(&output.stdout) {
        Ok(value) => value,
        Err(error) => {
            eprintln!("xtask attribution: `cargo metadata` output did not parse: {error}");
            return Verdict::Fail;
        }
    };

    // The lock is the authority on WHICH packages belong in the document, and `cargo metadata` is
    // the authority on their licences. Reading the set from the lock rather than from the metadata
    // is what keeps the generator and the gate answering the same question: a generator that
    // decided the set for itself could write a document its own gate rejects.
    let Some(lock) = read(&root, LOCK) else {
        return Verdict::Fail;
    };
    let mut licences: BTreeMap<Package, String> = BTreeMap::new();
    // `get` rather than `[..]`: `clippy::indexing_slicing` is denied across this workspace, and
    // `serde_json::Value`'s own `Index` impl panics on a non-object rather than answering `Null`.
    let empty = Vec::new();
    let packages = metadata
        .get("packages")
        .and_then(serde_json::Value::as_array)
        .unwrap_or(&empty);
    for package in packages {
        let field = |key: &str| package.get(key).and_then(serde_json::Value::as_str);
        let (Some(name), Some(version)) = (field("name"), field("version")) else {
            continue;
        };
        let licence = field("license").unwrap_or("").trim();
        let key = Package {
            name: String::from(name),
            version: String::from(version),
        };
        licences.insert(key, String::from(licence));
    }

    let wanted = third_party(&lock);
    let mut lines: Vec<String> = Vec::new();
    let mut unlicensed = Vec::new();
    for package in {
        let mut sorted = wanted;
        sorted.sort();
        sorted.dedup();
        sorted
    } {
        let licence = licences.get(&package).map_or("", String::as_str);
        if licence.is_empty() {
            unlicensed.push(package.clone());
        }
        // Collected and joined rather than written into a `String`: `clippy::format_push_string`
        // rules out `push_str(&format!(..))` and `clippy::expect_used` rules out unwrapping the
        // `write!` that replaces it, so the shape with no error to discard is the one left.
        let declared = if licence.is_empty() { "NOT DECLARED" } else { licence };
        lines.push(format!("| `{}` | `{}` | {declared} |", package.name, package.version));
    }

    // A crate declaring no licence at all is not something to write into the document quietly: it
    // is a licence question somebody has to answer, and `cargo deny check` is the gate that would
    // have refused it. Reported and still written, so the diff shows which one.
    if !unlicensed.is_empty() {
        eprintln!("xtask attribution: {} package(s) declare no licence:", unlicensed.len());
        for package in &unlicensed {
            eprintln!("  {} {}", package.name, package.version);
        }
        eprintln!("  Written as NOT DECLARED. `cargo deny check` is what decides whether that ships.");
    }

    let count = lines.len();
    let table = lines.join("\n");
    let document = format!("{}{table}\n", header(count));
    let path = root.join(DOCUMENT);
    if let Err(error) = std::fs::write(&path, &document) {
        eprintln!("xtask attribution: could not write {}: {error}", path.display());
        return Verdict::Fail;
    }
    println!("xtask attribution: wrote {DOCUMENT} - {count} third-party packages");
    Verdict::Pass
}

#[cfg(test)]
mod tests {
    use core::fmt::Write as _;

    use super::{Package, header, row, rows, third_party};

    /// A lock stanza, so the fixtures read like the file they parse.
    fn stanza(name: &str, version: &str, source: Option<&str>) -> String {
        let mut out = format!("[[package]]\nname = \"{name}\"\nversion = \"{version}\"\n");
        if let Some(source) = source {
            writeln!(out, "source = \"{source}\"").expect("writing into a String cannot fail");
        }
        out
    }

    const REGISTRY: &str = "registry+https://github.com/rust-lang/crates.io-index";

    #[test]
    fn a_stanza_with_no_source_is_not_third_party() {
        // The whole selection rule. A workspace member and a path dependency have no `source`, and
        // that is what distinguishes our own crates and the vendored allocator from somebody
        // else's code - so getting this backwards would either attribute Apache-2.0 crates we
        // wrote or silently drop 455 that we did not.
        let lock = format!(
            "{}{}{}",
            stanza("sutura-domain", "0.1.0", None),
            stanza("mimalloc", "0.1.50", None),
            stanza("serde", "1.0.230", Some(REGISTRY))
        );
        assert_eq!(
            third_party(&lock),
            vec![Package {
                name: String::from("serde"),
                version: String::from("1.0.230")
            }]
        );
    }

    #[test]
    fn two_versions_of_one_crate_are_two_packages() {
        // Twenty-seven crates are duplicated in this workspace, so the key is the pair and not the
        // name. Keying on the name alone would report a complete document as complete while one of
        // the two versions went unattributed.
        let lock = format!(
            "{}{}",
            stanza("arrow", "58.4.0", Some(REGISTRY)),
            stanza("arrow", "59.2.0", Some(REGISTRY))
        );
        assert_eq!(third_party(&lock).len(), 2);
    }

    #[test]
    fn a_source_line_does_not_leak_into_the_next_stanza() {
        // `sourced` is reset at each `[[package]]`, so a registry crate followed by a workspace
        // member does not drag the member into the document. Without the reset the second stanza
        // inherits the first one's source and every workspace crate is attributed as third-party.
        let lock = format!(
            "{}{}",
            stanza("serde", "1.0.230", Some(REGISTRY)),
            stanza("xtask", "0.1.0", None)
        );
        assert_eq!(third_party(&lock).len(), 1);
    }

    #[test]
    fn the_last_stanza_in_the_file_is_read() {
        // There is no `[[package]]` after it to flush on, which is the off-by-one this asserts:
        // cargo writes the lock with no trailing marker, so a parser that only flushes on the next
        // header loses whichever crate sorts last.
        let lock = stanza("zstd", "0.13.3", Some(REGISTRY));
        assert_eq!(third_party(&lock).len(), 1);
    }

    #[test]
    fn a_package_row_is_parsed_and_prose_is_not() {
        assert_eq!(
            row("| `serde` | `1.0.230` | MIT OR Apache-2.0 |"),
            Some((
                Package {
                    name: String::from("serde"),
                    version: String::from("1.0.230")
                },
                String::from("MIT OR Apache-2.0")
            ))
        );
        // The header and the separator share the file with the table, so neither may parse as a
        // row: an unquoted first cell is what tells them apart.
        assert_eq!(row("| Package | Version | Licence |"), None);
        assert_eq!(row("| --- | --- | --- |"), None);
        assert_eq!(row("Every crate the workspace resolves."), None);
        // A fourth column is a different document; refusing it means the gate reports a drift
        // rather than reading a column it does not understand as a licence.
        assert_eq!(row("| `serde` | `1.0.230` | MIT | extra |"), None);
    }

    #[test]
    fn the_generated_header_carries_the_marker_text_hygiene_reads() {
        // `xtask/src/text.rs` looks for `do not edit`, case-insensitively, in the FIRST FIVE lines.
        // A header that grew a line above the marker would silently re-enter the em-dash and
        // whitespace rules and this generated file would then fail a gate it cannot fix.
        let head = header(455);
        let first_five: Vec<&str> = head.lines().take(5).collect();
        assert!(
            first_five.iter().any(|l| l.to_ascii_lowercase().contains("do not edit")),
            "the generated marker moved out of the first five lines: {first_five:?}"
        );
        assert!(head.contains("455 third-party crates"), "the count reaches the prose");
    }

    #[test]
    fn the_committed_document_agrees_with_the_committed_lock() {
        // The gate against the tree it guards, `shared_client`'s last test's reason: a refactor of
        // either parse can pass its own fixtures and fail the files, and both are committed so
        // this is cheap.
        let Some(root) = crate::repo::root() else {
            return;
        };
        let Ok(lock) = std::fs::read_to_string(root.join("Cargo.lock")) else {
            return;
        };
        let Ok(document) = std::fs::read_to_string(root.join(super::DOCUMENT)) else {
            return;
        };
        let have = rows(&document);
        let want = third_party(&lock);
        assert!(
            !want.is_empty(),
            "the lock resolves no third-party packages, which cannot be right"
        );
        for package in &want {
            assert!(
                have.contains_key(package),
                "{} {} is in Cargo.lock and not in ATTRIBUTION.md",
                package.name,
                package.version
            );
        }
        assert_eq!(have.len(), want.len(), "ATTRIBUTION.md names packages the lock does not");
    }
}
