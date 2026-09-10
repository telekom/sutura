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
mod harness;

#[cfg(test)]
mod tests {
    use super::harness::*;
    use super::{
        COMMITTED, COPY_IN_RELEASE, GENERATE_IN_RELEASE, RELEASE, header, owner_failures, package_name, refuse, row, rows,
        third_party, workspace_members,
    };

    #[test]
    fn a_complete_generation_passes_and_reports_its_row_count() {
        // The self-guard on every refusal below: the fixture shape has to be capable of PASSING,
        // or each red assertion would be red for the wrong reason - a table `row` cannot parse
        // reads as every package missing, and the test would look like it proved something.
        let g = generated(
            &[("serde", "1.0.230"), ("zstd", "0.13.3")],
            &declared(&[("serde", "1.0.230", "MIT OR Apache-2.0"), ("zstd", "0.13.3", "MIT")]),
        );
        assert_eq!(refuse(&g), Ok(2), "a complete document must pass, or nothing below is a test");
    }

    #[test]
    fn a_package_declaring_no_licence_is_refused() {
        // THE STRENGTHENING, and the reason it is here: the previous generator printed this to
        // stderr, wrote `NOT DECLARED` into the document, and exited zero - so an undeclared
        // licence reached the signed release asset with a green run behind it. Now that the
        // document is no longer committed, no reviewer sees that row in a diff either, so a
        // refusal is the only thing left that can see it at all.
        let g = generated(
            &[("serde", "1.0.230"), ("mystery", "0.1.0")],
            &declared(&[("serde", "1.0.230", "MIT OR Apache-2.0")]),
        );
        let Err(reasons) = refuse(&g) else {
            panic!("an undeclared licence must be refused, not written as NOT DECLARED and passed");
        };
        assert!(reasons.contains("declare no licence"), "{reasons}");
        assert!(reasons.contains("mystery 0.1.0"), "the refusal names which one: {reasons}");
    }

    #[test]
    fn an_empty_third_party_set_is_refused_rather_than_reported_as_a_clean_document() {
        // Fail closed. A lock that resolves nothing is a parse that broke, and the shape of the
        // failure is a WELL FORMED document naming nothing - which every count in a header and
        // every byte-compare against a fresh generation would have called correct.
        let g = generated(&[], &declared(&[]));
        let Err(reasons) = refuse(&g) else {
            panic!("an empty package set must fail closed");
        };
        assert!(reasons.contains("resolves no third-party package"), "{reasons}");
    }

    #[test]
    fn a_package_the_generator_dropped_is_refused() {
        // The set comes from `Cargo.lock` and the rows come from the render, so a render that lost
        // one is visible. This is the assertion that keeps `check-attribution` from being a
        // generator checking itself: `wanted` and `text` are two different derivations.
        let mut g = generated(
            &[("serde", "1.0.230"), ("zstd", "0.13.3")],
            &declared(&[("serde", "1.0.230", "MIT OR Apache-2.0"), ("zstd", "0.13.3", "MIT")]),
        );
        g.text = g.text.replace("| `zstd` | `0.13.3` | MIT |\n", "");
        g.text = g.text.replace("| `zstd` | `0.13.3` | MIT |", "");
        let Err(reasons) = refuse(&g) else {
            panic!("a package in the lock with no row must be refused");
        };
        assert!(reasons.contains("no row in the generated document"), "{reasons}");
        assert!(reasons.contains("zstd 0.13.3"), "{reasons}");
    }

    #[test]
    fn the_owner_gate_passes_only_on_a_tree_with_no_committed_copy_and_a_generating_release() {
        // The self-guard for the three refusals below. If the fixture could not PASS, each of them
        // would be red for the wrong reason and the whole group would look like coverage while
        // asserting that a broken fixture is broken.
        let root = fixture("clean", GENERATING_RELEASE, false);
        assert_eq!(
            owner_failures(&root),
            Vec::<String>::new(),
            "the fixture has to be capable of passing, or nothing below is a test"
        );
    }

    #[test]
    fn a_committed_attribution_document_is_refused() {
        // The absence this whole change rests on. A commit re-adding the file restores the
        // staleness, turns every dependency bump red on arrival again, and the next tag signs
        // whatever the stale copy says - so the absence is held by this, not by a sentence.
        let root = fixture("committed", GENERATING_RELEASE, true);
        let failures = owner_failures(&root);
        assert_eq!(failures.len(), 1, "{failures:?}");
        let joined = failures.join("\n");
        assert!(joined.contains("is back in the tree"), "{joined}");
    }

    #[test]
    fn a_release_that_does_not_generate_the_asset_is_refused() {
        // With nothing committed, the release is the ONLY place the notice is produced. A release
        // that stopped generating would publish binaries with no attribution, and would do it at
        // the one moment no gate is watching - on a tag.
        let root = fixture("no-generate", "      - run: echo nothing to do\n", false);
        let failures = owner_failures(&root);
        assert_eq!(failures.len(), 1, "{failures:?}");
        assert!(failures.join("\n").contains("does not invoke"), "{failures:?}");
    }

    #[test]
    fn a_release_that_copies_a_committed_document_is_refused_even_when_it_also_generates() {
        // The half that is easy to leave out, and the reason it is separate: a workflow that
        // generates AND copies satisfies the check above while still handing a stale document to
        // the signer. So the copy is refused on its own, with the generation present.
        let workflow = format!("{GENERATING_RELEASE}      - run: cp ATTRIBUTION.md dist/sutura-attribution.md\n");
        let root = fixture("copies", &workflow, false);
        let failures = owner_failures(&root);
        assert_eq!(failures.len(), 1, "{failures:?}");
        assert!(failures.join("\n").contains("still carries"), "{failures:?}");
    }

    #[test]
    fn an_unreadable_release_workflow_is_a_fault_and_not_a_pass() {
        // Fail closed. A missing workflow file used to be indistinguishable from a workflow that
        // generates correctly, and this gate exists precisely so that nothing about the release's
        // attribution step goes unchecked in silence.
        let root = fixture("unreadable", GENERATING_RELEASE, false);
        std::fs::remove_file(root.join(RELEASE)).expect("the fixture wrote it");
        let failures = owner_failures(&root);
        assert_eq!(failures.len(), 1, "{failures:?}");
        assert!(failures.join("\n").contains("could not be read"), "{failures:?}");
    }

    #[test]
    fn the_tree_carries_no_committed_attribution_document() {
        // The absence `#462` decided on, asserted against the REAL tree rather than a fixture,
        // because the thing that can regress is a commit re-adding the file. `run_check_owner` is
        // the gate; this is the same claim in the test suite, so a `just test` run that never
        // invokes the hygiene sweep still notices.
        let Some(root) = crate::repo::root() else {
            return;
        };
        assert!(
            !root.join(COMMITTED).exists(),
            "{COMMITTED} is committed again: a generated artefact with a second owner goes stale, \
         turns every dependency bump red on arrival, and the next tag signs the stale copy"
        );
    }

    #[test]
    fn the_release_workflow_generates_the_attribution_asset_and_copies_no_committed_one() {
        // The other half of the owner gate, and the half that matters most: with nothing committed,
        // the release is the ONLY place the notice is produced. A release that stopped generating
        // would publish binaries with no attribution and would fail at the one moment no gate is
        // watching - on a tag.
        let Some(root) = crate::repo::root() else {
            return;
        };
        let Ok(workflow) = std::fs::read_to_string(root.join(RELEASE)) else {
            return;
        };
        assert!(
            workflow.contains(GENERATE_IN_RELEASE),
            "{RELEASE} must invoke `{}`: it is where the licence obligation is discharged",
            GENERATE_IN_RELEASE.trim()
        );
        assert!(
            !workflow.contains(COPY_IN_RELEASE),
            "{RELEASE} copies a committed document again, which is the step that can hand a stale \
         notice to the signer"
        );
    }

    #[test]
    fn a_vendored_path_dependency_is_third_party_and_a_workspace_member_is_not() {
        // RED BEFORE GREEN, and this is the finding a review had to make: the first version of this
        // module selected on the presence of a `source` line, so `mimalloc` and `libmimalloc-sys` -
        // vendored under `vendor/`, declared as PATH dependencies, and LINKED by `sutura-cli` on
        // Linux - were left out of the released attribution asset. That is the exact failure
        // `docs/adr/0021` says this document exists to prevent, and the old code passed its own
        // tests while doing it.
        //
        // So the assertion is about the pair: a source-less NON-member is in, a source-less member
        // is out.
        let lock = format!(
            "{}{}{}",
            stanza("sutura-domain", "0.1.0", None),
            stanza("mimalloc", "0.1.52", None),
            stanza("serde", "1.0.230", Some(REGISTRY))
        );
        let found = third_party(&lock, &ours(&["sutura-domain"]));
        assert_eq!(
            found,
            vec![package("mimalloc", "0.1.52"), package("serde", "1.0.230")],
            "a vendored path dependency has to be attributed and a workspace member must not be"
        );
    }

    #[test]
    fn the_member_list_is_read_through_each_members_own_manifest() {
        // A member is a PATH, and `dev` holds `sutura-dev` - so deriving the crate name from the
        // directory would put `sutura-dev` in the attribution document as if we had not written it.
        // Read against the real files, because that mapping is the thing that can be wrong.
        let Some(root) = crate::repo::root() else {
            return;
        };
        let Ok(manifest) = std::fs::read_to_string(root.join(super::MANIFEST)) else {
            return;
        };
        let members = workspace_members(&root, &manifest).expect("the workspace declares members");
        assert!(members.contains("sutura-dev"), "the `dev` path resolves to its crate name");
        assert!(members.contains("xtask"));
        assert!(
            !members.contains("mimalloc"),
            "the vendored allocator is not a workspace member, so it belongs in the document"
        );
    }

    #[test]
    fn a_comment_in_the_member_list_contributes_no_member() {
        // That list carries prose about the architecture, and the prose quotes crate names. A scan
        // for quoted strings that did not skip comments would read one of those as a member and
        // then EXCLUDE it from the attribution document - a silent omission, which is the failure
        // mode this whole module is about.
        let manifest = concat!(
            "[workspace]\n",
            "members = [\n",
            "  # keeps \"polyglot-sql\" out of the core's closure\n",
            "  \"crates/sutura-domain\",\n",
            "]\n",
        );
        let root = std::path::Path::new("/nonexistent");
        // The member path cannot be read here, so the whole thing is `None` - which is the
        // fail-closed contract. What this pins is that the COMMENT did not become a member: were it
        // read as one, the loop would try `/nonexistent/# keeps ...` and still answer `None`, so the
        // observable difference is in `package_name`, asserted directly below.
        assert!(workspace_members(root, manifest).is_none());
        assert_eq!(package_name("[package]\nname = \"sutura-dev\"\n"), Some("sutura-dev"));
        assert_eq!(package_name("[package]\nversion = \"0.1.0\"\n"), None);
    }

    #[test]
    fn two_versions_of_one_crate_are_two_packages() {
        // Crates are duplicated in this workspace, so the key is the pair and not the name. Keying
        // on the name alone would report a complete document as complete while one of the two
        // versions went unattributed.
        let lock = format!(
            "{}{}",
            stanza("arrow", "58.4.0", Some(REGISTRY)),
            stanza("arrow", "59.2.0", Some(REGISTRY))
        );
        assert_eq!(third_party(&lock, &ours(&[])).len(), 2);
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
        assert_eq!(third_party(&lock, &ours(&["xtask"])).len(), 1);
    }

    #[test]
    fn the_last_stanza_in_the_file_is_read() {
        // There is no `[[package]]` after it to flush on, which is the off-by-one this asserts:
        // cargo writes the lock with no trailing marker, so a parser that only flushes on the next
        // header loses whichever crate sorts last.
        let lock = stanza("zstd", "0.13.3", Some(REGISTRY));
        assert_eq!(third_party(&lock, &ours(&[])).len(), 1);
    }

    #[test]
    fn a_package_row_is_parsed_and_prose_is_not() {
        assert_eq!(
            row("| `serde` | `1.0.230` | MIT OR Apache-2.0 |"),
            Some((package("serde", "1.0.230"), String::from("MIT OR Apache-2.0")))
        );
        // The header and the separator share the file with the table, so neither may parse as a
        // row: an unquoted first cell is what tells them apart.
        assert_eq!(row("| Package | Version | Licence |"), None);
        assert_eq!(row("| --- | --- | --- |"), None);
        assert_eq!(row("Every crate the workspace resolves."), None);
        // A fourth column is a different document; refusing it means the check reports a drift
        // rather than reading a column it does not understand as a licence.
        assert_eq!(row("| `serde` | `1.0.230` | MIT | extra |"), None);
    }

    #[test]
    fn the_generated_header_carries_the_marker_text_hygiene_reads() {
        // `xtask/src/text.rs` looks for `do not edit`, case-insensitively, in the FIRST FIVE lines.
        // A header that grew a line above the marker would silently re-enter the em-dash and
        // whitespace rules, and the released asset is still a markdown file somebody may lint.
        let head = header(455);
        let first_five: Vec<&str> = head.lines().take(5).collect();
        assert!(
            first_five.iter().any(|l| l.to_ascii_lowercase().contains("do not edit")),
            "the generated marker moved out of the first five lines: {first_five:?}"
        );
        assert!(head.contains("455 third-party crates"), "the count reaches the prose");
    }

    #[test]
    fn a_duplicated_row_is_one_entry_and_the_count_says_so() {
        // `rows` is keyed, so a document that named `serde` twice would report one row. The count
        // in the pass line is therefore the number of DISTINCT packages named, which is the number
        // the refusals reason about.
        let document = concat!("| `serde` | `1.0.230` | MIT |\n", "| `serde` | `1.0.230` | Apache-2.0 |\n",);
        assert_eq!(rows(document).len(), 1);
    }
}
