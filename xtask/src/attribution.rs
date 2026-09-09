//! The attribution document: every third-party crate this workspace resolves, and its licence.
//!
//! WHY THIS EXISTS AND `deny.toml` IS NOT IT. `deny.toml` holds an exact allowlist of SPDX
//! identifiers with `unused-allowed-license = "deny"`, and that is a POLICY check: it answers "is
//! this identifier one we accept". It says nothing a distributor can hand on. Several of the
//! identifiers on that allowlist - Apache-2.0 and MIT among them - oblige whoever redistributes a
//! binary to carry the licence and the copyright notice of what is inside it. `docs/adr/0021` said
//! the adjacent half out loud: *"There is no attribution document, and `NOTICE` still says nothing
//! about third-party crates."* This is the mechanism for that sentence.
//!
//! # It is GENERATED AND NOT COMMITTED, and that is the decision this module records
//!
//! It used to be a committed file with two gates over it, and the cost of that arrangement was not
//! theoretical: **every Dependabot cargo bump was red on arrival.** A bump moves `Cargo.lock`, the
//! document is derived from `Cargo.lock`, and the bot runs with a read-only token - so it could not
//! regenerate the artefact its own change invalidated, and no bump could go green without a human
//! pushing to the bot's branch. `github.com/telekom/sutura#431` measured that on two open bumps;
//! `#462` established that the ARTEFACT is not redundant and that its COMMITTED FORM is what costs.
//!
//! So the generator is the artefact's only owner. There is no committed copy to fall behind
//! `Cargo.lock`, therefore no staleness for a gate to catch, therefore nothing for a bot bump to
//! trip over. `just attribution` writes it under `/target` for a human to read; the release
//! workflow generates it into the release's own asset directory, where it is hashed, signed and
//! published exactly as before.
//!
//! **The licence obligation is discharged in the same place it always was: on the release.** The
//! Apache-2.0 §4(d), MIT and BSD requirement is that notices accompany a *distribution*, and what
//! this project distributes is a tagged release - `sutura-attribution.md`, with its `.sha256` and
//! its Sigstore bundle. A file on `main` never discharged that obligation; it was the *input* to
//! the asset that does. Generating the asset from the tagged tree's own `Cargo.lock` discharges it
//! from a shorter chain, because the copy step that could hand on a stale document is gone.
//!
//! **What was LOST, stated next to the claim.** A new dependency's declared licence no longer
//! appears in a pull-request diff. That was a real review-time property and nothing here restores
//! it. Two things bound the loss and neither is a substitute: `cargo deny check` still *refuses* a
//! licence that is not on the allowlist, and `check-attribution` below now *refuses* a crate that
//! declares no licence at all - which the generator previously only printed to stderr while
//! writing `NOT DECLARED` into the document and passing. The compensation is deliberate: the
//! obligation lost a reviewer and gained a refusal.
//!
//! # Three tasks, because generating, deriving and forbidding need different inputs
//!
//! * `cargo xtask attribution [dest]` WRITES the document. It shells out to `cargo metadata`,
//!   which is where a crate's declared licence expression lives - `Cargo.lock` does not carry one.
//!   `just attribution` is the way to call it, and `.github/workflows/release.yml` is the other
//!   caller.
//! * `cargo xtask check-attribution` GENERATES ONE AND REFUSES AN INCOMPLETE RESULT. It cannot
//!   byte-compare against anything, because there is nothing committed to compare with - so it
//!   asserts the properties the artefact has to have, from two independent inputs: the package SET
//!   comes from `Cargo.lock` parsed as text, and the LICENCES come from `cargo metadata`'s
//!   resolution. A generator that dropped rows, or a keying bug that matched no metadata entry, is
//!   caught by the disagreement between the two. `Kind::Standalone`, because `cargo metadata` needs
//!   a resolvable registry the nix sandbox has not got - `check-api-docs` is the same shape for the
//!   same kind of reason.
//! * `cargo xtask check-attribution-owner` FORBIDS A SECOND OWNER, offline, in the hygiene sweep.
//!   Absence is the whole point of the change above, and **an absence that nothing witnesses
//!   silently returns**: a committed `ATTRIBUTION.md` could be re-added in any commit, would begin
//!   to go stale immediately, and the next tag would sign it. So the gate refuses the path's
//!   existence, and refuses a release workflow that does not generate.
//!
//! # Which packages belong in it, and why `source` is not the test
//!
//! **A `source` entry in `Cargo.lock` means "from a registry or a git remote". It does not mean
//! "third-party", and the first version of this module used it as if it did.** `mimalloc` and
//! `libmimalloc-sys` are vendored under `vendor/` and declared as PATH dependencies, so they have
//! no `source` - and `sutura-cli` LINKS `mimalloc` on Linux. Filtering on `source` therefore left
//! two shipped third-party crates out of the released attribution asset, which is the exact
//! failure `docs/adr/0021` says this document exists to prevent. A review caught it.
//!
//! So the test is **workspace membership**, read off the root `Cargo.toml`'s `members` list and
//! each member's own `name`. A source-less package that is not a member is a vendored path
//! dependency and belongs in the document. `VENDOR.md` and `REUSE.toml` remain the provenance
//! record for those trees - they are useful and they put no rows in the asset a consumer downloads.
//!
//! It reads files rather than asking cargo, because the membership half has to stay cheap, and it
//! fails closed - an unreadable manifest or an empty members list is a non-zero exit, since a
//! members set that came back empty would silently attribute every one of our own crates to
//! somebody else.
//!
//! # What is claimed, and what is not
//!
//! Claimed: the generated document names every third-party package in `Cargo.lock` at the resolved
//! version, names nothing else, and carries a non-empty declared licence for each one.
//!
//! Not claimed, and each is real:
//!
//! * **Not that the licence expression is TRUE of the crate's source.** It is what the manifest
//!   declares, copied through. Checking it against the licence FILES in a crate's tree is a source
//!   scan this repository does not perform.
//! * **Not that it is the list a given binary LINKS.** `Cargo.lock` records what cargo resolved.
//!   `docs/adr/0021` argues that at length against generating an SBOM this way and is right - and
//!   for an attribution document the error points the other way: naming a crate that did not ship
//!   discharges an obligation nobody had, while omitting one that did ship is the failure. So this
//!   errs by overstating, on purpose.
//! * **Not the notice text of each dependency.** An Apache-2.0 dependency's own `NOTICE` file is
//!   in its source tree and not in its metadata, so nothing here can render one.
//! * **Not that the release actually published it.** That is the arrival assertion in
//!   `release.yml`, which is where the bytes are. The owner gate below can see that the workflow
//!   *invokes* the generator; it cannot run a tagged release.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::Verdict;
use crate::repo;

/// The path a committed copy WOULD live at. **Nothing writes it.** `check-attribution-owner`
/// refuses its presence, which is the mechanism that keeps the generator the only owner.
const COMMITTED: &str = "ATTRIBUTION.md";

/// Where the generator writes when no destination is named. Under `/target`, which `.gitignore`
/// already excludes wholesale - so a local generation cannot become a committed copy by accident,
/// and the owner gate cannot be tripped by a developer having run `just attribution`.
const DEFAULT_OUTPUT: &str = "target/attribution/sutura-attribution.md";

/// The release workflow, which is the caller that turns this document into a distributed asset.
const RELEASE: &str = ".github/workflows/release.yml";

/// The substring the owner gate requires `RELEASE` to carry: the release generates its own copy.
///
/// Matched on the task name rather than the whole command line, because the destination path and
/// the `nix run` prefix are the workflow's business and this only has to see that the generator is
/// the thing being called.
const GENERATE_IN_RELEASE: &str = "xtask -- attribution ";

/// The shape the release must NOT carry: a copy of a committed document.
///
/// This is the arrangement `#431` measured and `#462` decided against. Without this half the gate
/// above is satisfied by a workflow that generates a document and then copies a stale committed one
/// over the top of it.
const COPY_IN_RELEASE: &str = "cp ATTRIBUTION.md";

/// The lock file, read as text. The same choice `arrow_major` and `shared_client` make.
const LOCK: &str = "Cargo.lock";

/// The workspace root manifest, which is where the member list lives.
const MANIFEST: &str = "Cargo.toml";

/// The task that writes the document, named in every failure message so the fix is the message.
const REGENERATE: &str = "just attribution";

/// One package: the key the gate compares on, and the licence the generator writes.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Package {
    /// Sorted by name first, then version, which is the order the document is written in.
    name: String,
    version: String,
}

/// A generated document, and the two sets a verdict is drawn from.
///
/// The text and the sets travel together so the check cannot answer about a document the writer
/// would not have produced - one owner for the bytes, which is the same reason `document` renders
/// once and both callers use it.
struct Generated {
    /// The document itself.
    text: String,
    /// The lock-derived third-party set: the packages the document MUST name.
    wanted: BTreeSet<Package>,
    /// The packages `cargo metadata` declares no licence for.
    undeclared: BTreeSet<Package>,
}

/// Strip the surrounding quotes from a lock-file value, or `None` if it is not quoted.
fn unquote(value: &str) -> Option<&str> {
    value.strip_prefix('"')?.strip_suffix('"')
}

/// Crate names declared as workspace members, read off the root manifest and each member's own.
///
/// **This is the discriminator, and `source` is not**: see the module header. A source-less stanza
/// in `Cargo.lock` is either one of our crates or a vendored path dependency, and only the member
/// list tells the two apart.
///
/// Two levels of file read rather than one, because a member is a PATH and the directory name is
/// not the crate name - `dev` holds `sutura-dev`, so assuming otherwise would put `sutura-dev` in
/// the attribution document.
///
/// `None` where the manifest declares no members or a member's name cannot be read, so the caller
/// fails rather than proceeding with an empty set - which would attribute all of our own crates to
/// somebody else.
pub(crate) fn workspace_members(root: &Path, manifest: &str) -> Option<BTreeSet<String>> {
    let start = manifest.find("members = [")?;
    let rest = manifest.get(start..)?;
    let end = rest.find("\n]")?;
    let block = rest.get(..end)?;

    let mut names = BTreeSet::new();
    for raw in block.split('\n').skip(1) {
        let line = raw.trim();
        // Comment lines are skipped, and it is not tidiness: that members list carries prose about
        // the architecture, and the prose quotes crate names - so a scan for quoted strings that
        // did not skip comments would read `polyglot-sql` out of a comment and call it a member.
        if line.starts_with('#') {
            continue;
        }
        let Some(path) = line.strip_prefix('"').and_then(|l| l.split('"').next()) else {
            continue;
        };
        let text = std::fs::read_to_string(root.join(path).join(MANIFEST)).ok()?;
        names.insert(String::from(package_name(&text)?));
    }
    if names.is_empty() { None } else { Some(names) }
}

/// The `name` a manifest's `[package]` table declares.
fn package_name(manifest: &str) -> Option<&str> {
    manifest
        .lines()
        .find_map(|line| unquote(line.trim().strip_prefix("name = ")?))
}

/// Every THIRD-PARTY package in the lock file.
///
/// Two shapes qualify, and the second is a review's correction: a stanza WITH a `source` came from
/// a registry or a git remote, and a stanza WITHOUT one that is not a workspace member is a
/// vendored path dependency - third-party code carried in this repository, which `sutura-cli`
/// links on Linux. Only our own crates are excluded.
///
/// The shape this relies on is `cargo`'s own output: within a stanza, `name` precedes `version`,
/// and both are `key = "value"` on their own line. Parsed as text rather than as TOML for
/// `arrow_major`'s reason - `xtask` reads two fields out of this file in three gates now, and a
/// TOML dependency for that is a poor trade.
fn third_party(lock: &str, members: &BTreeSet<String>) -> Vec<Package> {
    let mut found = Vec::new();
    let mut name: Option<&str> = None;
    let mut version: Option<&str> = None;
    let mut flush = |name: &mut Option<&str>, version: &mut Option<&str>, sourced: bool| {
        // The OR is the review's correction: a stanza with a `source` is a registry or git
        // dependency, and a source-less stanza that is not a workspace member is a vendored path
        // dependency. Both are somebody else's code; only our own crates are excluded.
        if let (Some(n), Some(v)) = (name.take(), version.take())
            && (sourced || !members.contains(n))
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

/// Every package row a document carries, keyed so a duplicated row is visible as one entry - which
/// the count comparison in [`refuse`] then catches.
fn rows(document: &str) -> BTreeMap<Package, String> {
    document.lines().filter_map(row).collect()
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
/// marker is load-bearing and not decoration - it survives here because the release asset is still
/// a markdown file somebody may run a formatter over.
fn header(packages: usize) -> String {
    format!(
        "<!-- GENERATED by `{REGENERATE}`: do not edit, and do not commit. There is no committed copy; `cargo xtask check-attribution-owner` refuses one. -->\n\
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
- **The vendored trees ARE in it.** `mimalloc` and `libmimalloc-sys` live under `vendor/` and are\n\
  declared as path dependencies, so they carry no registry source - and `sutura-cli` links the\n\
  allocator on Linux, so they are rows below like anything else somebody else wrote. What selects a\n\
  row is not being a workspace member, never the presence of a source. `VENDOR.md` records their\n\
  upstream, commit and local changes and `REUSE.toml` records their licence per file; both are\n\
  provenance, and neither puts a row in this file.\n\
- **Not an advisory statement.** Whether any of these has a vulnerability against it is\n\
  `cargo deny check` against the RustSec database, whose verdict is a run rather than a document.\n\
\n\
## Packages\n\
\n\
| Package | Version | Licence |\n\
| --- | --- | --- |\n"
    )
}

/// The document the generator would write, together with the sets a verdict is drawn from.
///
/// One owner for the bytes, so the writer and the check cannot disagree about what a correct
/// document is - which they would if each rendered its own.
///
/// `--all-features`, because a feature-gated dependency is still something a build of this
/// workspace can pull in, and `--locked`, because the document has to describe the committed
/// resolution rather than whatever cargo would resolve today.
fn generate(root: &Path) -> Option<Generated> {
    let mut command = std::process::Command::new("cargo");
    command
        .current_dir(root)
        .args(["metadata", "--format-version", "1", "--all-features", "--locked"]);
    let output = match command.output() {
        Ok(output) => output,
        Err(error) => {
            eprintln!("xtask attribution: could not run `cargo metadata`: {error}");
            return None;
        }
    };
    if !output.status.success() {
        eprintln!("xtask attribution: `cargo metadata` failed");
        eprintln!("{}", String::from_utf8_lossy(&output.stderr));
        return None;
    }

    let metadata: serde_json::Value = match serde_json::from_slice(&output.stdout) {
        Ok(value) => value,
        Err(error) => {
            eprintln!("xtask attribution: `cargo metadata` output did not parse: {error}");
            return None;
        }
    };

    // The lock is the authority on WHICH packages belong in the document, and `cargo metadata` is
    // the authority on their licences. **Two independent inputs, on purpose**: that is what lets
    // `check-attribution` have an oracle at all now that there is no committed file to compare
    // against. A generator that decided the set for itself would be checking itself.
    let lock = read(root, LOCK)?;
    let members = workspace_members(root, &read(root, MANIFEST)?)?;
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

    let wanted: BTreeSet<Package> = third_party(&lock, &members).into_iter().collect();
    let mut lines: Vec<String> = Vec::new();
    let mut undeclared = BTreeSet::new();
    for package in &wanted {
        let licence = licences.get(package).map_or("", String::as_str);
        if licence.is_empty() {
            undeclared.insert(package.clone());
        }
        // Collected and joined rather than written into a `String`: `clippy::format_push_string`
        // rules out `push_str(&format!(..))` and `clippy::expect_used` rules out unwrapping the
        // `write!` that replaces it, so the shape with no error to discard is the one left.
        let declared = if licence.is_empty() { "NOT DECLARED" } else { licence };
        lines.push(format!("| `{}` | `{}` | {declared} |", package.name, package.version));
    }

    let count = lines.len();
    let table = lines.join("\n");
    Some(Generated {
        text: format!("{}{table}\n", header(count)),
        wanted,
        undeclared,
    })
}

/// The properties a generated document has to have, or the reasons it does not.
///
/// `Ok` carries the row count so the caller's pass line names what it read. Three refusals, and
/// each one is a failure this repository has actually had or would sign:
///
/// 1. **An empty third-party set** - fail closed. A lock that resolves nothing is a parse that
///    broke, and a well formed document naming nothing is the silent case the release's own count
///    floor was written for.
/// 2. **A package in the lock with no row** - the generator dropped it. Omitting a crate that
///    shipped is the failure the whole document exists to prevent.
/// 3. **A package that declares no licence** - previously printed to stderr and written into the
///    document as `NOT DECLARED`, with a passing exit. It is a refusal now, and that is the
///    deliberate compensation for the review-time visibility a committed file used to give: the
///    obligation lost a reviewer and gained a gate.
fn refuse(generated: &Generated) -> Result<usize, String> {
    let mut failures: Vec<String> = Vec::new();

    if generated.wanted.is_empty() {
        failures.push(format!(
            "{LOCK} resolves no third-party package. That cannot be right, and a well formed\n  document naming nothing is exactly the silent failure this refuses."
        ));
    }

    let named = rows(&generated.text);
    let missing: Vec<&Package> = generated
        .wanted
        .iter()
        .filter(|package| !named.contains_key(*package))
        .collect();
    if !missing.is_empty() {
        let mut lines = vec![format!(
            "{} package(s) in {LOCK} have no row in the generated document:",
            missing.len()
        )];
        for package in missing {
            lines.push(format!("    {} {}", package.name, package.version));
        }
        failures.push(lines.join("\n  "));
    }

    if !generated.undeclared.is_empty() {
        let mut lines = vec![format!(
            "{} package(s) declare no licence at all:",
            generated.undeclared.len()
        )];
        for package in &generated.undeclared {
            lines.push(format!("    {} {}", package.name, package.version));
        }
        lines.push(String::from(
            "  A licence question somebody has to answer, not a row to write quietly. `cargo deny",
        ));
        lines.push(String::from(
            "  check` decides whether it may ship; this refuses to publish a notice that omits it.",
        ));
        failures.push(lines.join("\n  "));
    }

    if failures.is_empty() {
        Ok(named.len())
    } else {
        Err(failures.join("\n  "))
    }
}

/// `cargo xtask attribution [dest]` - write the document from `cargo metadata`.
///
/// The destination is an argument because there are two callers with different needs and neither
/// wants a committed file: `just attribution` takes the default under `/target` so a human can read
/// it, and `.github/workflows/release.yml` names the release's own asset directory. **A default
/// inside `/target` rather than at the repo root is load-bearing** - it is what stops a local
/// generation from becoming the committed copy `check-attribution-owner` refuses.
///
/// It refuses before it writes, on [`refuse`]'s three grounds. Writing an incomplete document and
/// reporting it is what the previous version did for an undeclared licence, and the release would
/// have signed the result.
pub(crate) fn run_generate(args: &[String]) -> Verdict {
    let Some(root) = repo::root() else {
        eprintln!("xtask attribution: could not determine the repo root");
        return Verdict::Fail;
    };
    let destination: PathBuf = match args {
        [] => root.join(DEFAULT_OUTPUT),
        [dest] => PathBuf::from(dest),
        _ => return Verdict::Usage,
    };
    let Some(generated) = generate(&root) else {
        return Verdict::Fail;
    };
    let count = match refuse(&generated) {
        Ok(count) => count,
        Err(reasons) => {
            eprintln!("xtask attribution: FAILED - refusing to write an incomplete document");
            eprintln!("  {reasons}");
            return Verdict::Fail;
        }
    };
    if let Some(parent) = destination.parent()
        && let Err(error) = std::fs::create_dir_all(parent)
    {
        eprintln!("xtask attribution: could not create {}: {error}", parent.display());
        return Verdict::Fail;
    }
    if let Err(error) = std::fs::write(&destination, &generated.text) {
        eprintln!("xtask attribution: could not write {}: {error}", destination.display());
        return Verdict::Fail;
    }
    println!(
        "xtask attribution: wrote {} - {count} third-party packages",
        destination.display()
    );
    Verdict::Pass
}

/// `cargo xtask check-attribution` - a generation succeeds and is complete.
///
/// It generates and throws the bytes away. **There is nothing to byte-compare against**, which is
/// the consequence of not committing the artefact, so the oracle is the disagreement between two
/// independent inputs instead: `Cargo.lock` decides the package set and `cargo metadata` supplies
/// the licences. What that reaches, and what it does not, is in [`refuse`]'s own documentation.
///
/// `Kind::Standalone` for `check-api-docs`' reason rather than its own: it needs an input the nix
/// sandbox has not got - a compiler there, a resolvable registry here.
pub(crate) fn run_check(_args: &[String]) -> Verdict {
    let Some(root) = repo::root() else {
        eprintln!("xtask check-attribution: could not determine the repo root");
        return Verdict::Fail;
    };
    let Some(generated) = generate(&root) else {
        return Verdict::Fail;
    };
    match refuse(&generated) {
        Ok(count) => {
            println!(
                "xtask check-attribution: ok - a generation names all {count} third-party packages in {LOCK}, each with a declared licence"
            );
            Verdict::Pass
        }
        Err(reasons) => {
            eprintln!("xtask check-attribution: FAILED");
            eprintln!("  {reasons}");
            eprintln!();
            eprintln!("  The document is GENERATED and NOT COMMITTED, so there is nothing to");
            eprintln!("  refresh: this is a defect in the generator or in the dependency set.");
            Verdict::Fail
        }
    }
}

/// `cargo xtask check-attribution-owner` - the generator is the artefact's only owner.
///
/// **This is an absence gate, and an absence that nothing witnesses silently returns.** The whole
/// value of `#462`'s decision is that no copy of this document is committed; a commit that re-added
/// one would restore the staleness the change removed, turn every bot bump red again, and the next
/// tag would sign whatever it said. `xtask/src/workflows/sast.rs` is the local precedent for
/// holding a deliberate absence with a check rather than with a sentence.
///
/// Offline, so it is `Kind::Hygiene(Reads::Code)` and runs everywhere a developer commits. It reads
/// two files: the forbidden path, and the release workflow that has to generate. The second half
/// exists because the first half alone is satisfied by a release that still copies - which would
/// then fail at the one moment no gate is watching, on a tag.
pub(crate) fn run_check_owner(_args: &[String]) -> Verdict {
    let Some(root) = repo::root() else {
        eprintln!("xtask check-attribution-owner: could not determine the repo root");
        return Verdict::Fail;
    };
    let failures = owner_failures(&root);
    if failures.is_empty() {
        println!("xtask check-attribution-owner: ok - no committed {COMMITTED}, and {RELEASE} generates the asset");
        Verdict::Pass
    } else {
        eprintln!("xtask check-attribution-owner: FAILED");
        for failure in &failures {
            eprintln!("  {failure}");
        }
        Verdict::Fail
    }
}

/// Every reason the generator is not the artefact's only owner, as the messages the gate prints.
///
/// Separated from [`run_check_owner`] so **the refusals themselves are under test** rather than
/// only the predicates they read. That distinction is not pedantic: a refusal neutralised while
/// every field it reads stays live is invisible to `dead_code`, and a test that asserted the tree
/// property directly - `ATTRIBUTION.md` is absent - would still pass while this function had
/// stopped being able to say so.
fn owner_failures(root: &Path) -> Vec<String> {
    let mut failures: Vec<String> = Vec::new();

    if root.join(COMMITTED).exists() {
        failures.push(format!(
            "{COMMITTED} is back in the tree. It is a GENERATED artefact with one owner - the\n  generator - and a committed copy falls behind {LOCK} the moment a dependency moves.\n  That is what made every Dependabot bump red on arrival, and the next tag would sign the\n  stale copy. Delete it; `{REGENERATE}` writes one under {DEFAULT_OUTPUT}."
        ));
    }

    match read(root, RELEASE) {
        None => failures.push(format!(
            "{RELEASE} could not be read, so nothing here can say the release still generates the\n  attribution asset. An unreadable input is a fault, not a pass."
        )),
        Some(workflow) => {
            if !workflow.contains(GENERATE_IN_RELEASE) {
                failures.push(format!(
                    "{RELEASE} does not invoke `{}`. The attribution asset is what discharges the\n  Apache-2.0 §4(d), MIT and BSD notice obligation on a distribution, and with no committed\n  copy the release is the ONLY place it is produced. A release that does not generate it\n  publishes binaries with no notice.",
                    GENERATE_IN_RELEASE.trim()
                ));
            }
            if workflow.contains(COPY_IN_RELEASE) {
                failures.push(format!(
                    "{RELEASE} still carries `{COPY_IN_RELEASE}`. Copying a committed document is the\n  arrangement `#462` decided against: it is the step that can hand a stale notice to the\n  signer, and it is satisfied even when a generation ran first."
                ));
            }
        }
    }

    failures
}

#[cfg(test)]
mod tests;
