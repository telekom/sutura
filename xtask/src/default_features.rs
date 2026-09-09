//! Every package that SHIPS compiles and lints at the feature set it ships with.
//!
//! **The lane no other gate can see, and it was found by shipping a red branch through it.** Every
//! compiling gate in this repository passes `--all-features`: `just lint`, `just test`,
//! `just check-changed` and the `clippy` and `nextest` nix checks alike. `nix/shipped.nix`, though,
//! builds each published binary with cargo's DEFAULT features - `--package` and `--target`, no
//! `--features` - because a non-optional networked adapter would cross-compile `ureq`, rustls and
//! `ring` for two musl triples for a binary that links none of them. So a `#[cfg(feature = "...")]`
//! that a developer only ever compiled with the feature ON can be a hard error in the exact
//! configuration a release publishes, and every gate a developer runs before pushing is green.
//!
//! That happened: `sutura-serve`'s boot pre-flight landed with three items reachable only from a
//! `bigquery` arm, `dead_code = "deny"` made all three errors with the feature off, and it reached
//! review as a green branch. The four `cross` link checks WOULD have caught it - they build
//! `.#sutura-serve-<triple>-ci` at the default set on every pull request - but they are
//! `needs: [ci]`, and `ci` had failed on something else, so they never ran. That is a sequencing
//! fact rather than coverage, and a gate a developer can run is the fix for it. The same review then
//! measured the LINT half of the same lane still red on a pre-existing `doc_markdown` in a
//! `#[cfg(not(feature = "bigquery"))]` doc comment, which is why this runs clippy as well as check:
//! the two commands see different code, and neither is `just lint`'s.
//!
//! **NOT a hygiene gate, for `check-attribution-current`'s reason.** It invokes cargo, so it needs a
//! resolvable registry and a target directory; the nix sandbox `hygiene` runs in has neither. So it
//! lives in `just gates`, which is where every gate that shells out to cargo lives.
//!
//! **CI runs it as `nix run .#default-features`, inside the one required job.** Before that step
//! existed, what CI had for this lane was the four `cross` link builds for the COMPILE half - and
//! they are `needs: [ci]`, so a `ci` failure skips them, which is exactly how the branch above
//! reached review - and nothing at all for the LINT half. An app rather than a check for the reason
//! the paragraph above gives. `tests::both_lanes_still_invoke_this_gate` is what holds the
//! wiring, in both venues, by reading the step rather than the file.
//!
//! **The profile is DERIVED from the target directory and not passed as a flag** -
//! [`crate::warm_start::profile_for`], shared with this lane's other half. Cargo keys artifacts per
//! profile, so compiling inside the warmed directory at anything but the profile those artifacts
//! carry reuses none of them: it rebuilds the closure, passes, and nobody attributes the minutes to
//! it. An earlier revision threaded `--profile <name>` through instead and put the flag where cargo
//! cannot read it - past the `--` on the app's own line, which selects a profile for the xtask
//! binary and none for the build - while `check-warm-start` still printed `ok`. A derivation has no
//! wrong side of a separator to be written on. `just gates` derives `None`, which is the
//! developer's default profile and no second dependency build.
//!
//! **THE REUSE IS PARTIAL, and read that before costing this step.** The closure holds dependency
//! units at the workspace-wide feature union; this gate deliberately asks the narrow question
//! instead - one shipped package, no feature flags - so the v2 resolver gives much of the graph a
//! narrower feature set, a fresh `-C metadata`, and a recompile. MEASURED in this gate's own CI job,
//! 2026-09-03: the `cargo check` pass compiled 104 units in 38.27 s for `sutura-cli` and 89 in
//! 41.00 s for `sutura-serve`, against `cargo tree --edges normal,build` graphs of 261 and 301
//! packages - about a third of each, and the EXPENSIVE third, because the whole
//! arrow/parquet/datafusion stack misses: 27 s of that first 38 s. The two clippy passes were
//! 3.96 s and 4.46 s only because cargo runs clippy-driver on the primary package alone, so they
//! consume what the check pass beside them just produced. Matching the closure would mean asking
//! about the whole workspace at once, which is the cross-member feature unification this gate
//! exists to see past - so the recompile is the price of the question and not a defect in it.
//!
//! **That cost is SHARED now, so the figure above will not reproduce alone.** It was taken before
//! `check-default-feature-tests` existed, and that gate compiles the same narrow configuration -
//! whichever of the two `ci` steps runs first pays the recompile and the second reuses it. They are
//! adjacent in `ci.yml` for that reason. Cost the pair, never this step by itself.
//!
//! **The package list is DERIVED and not written here**, which is the single-owner rule: it is every
//! `package = "..."` inside `nix/shipped.nix`'s `binaries` list, the same declaration
//! `check-shipped-binaries` compares the release literals against. A binary added there is covered by
//! this gate without anybody remembering to add it, and a package renamed in one place fails rather
//! than silently dropping out. FAIL CLOSED on parsing none, for that gate's own stated reason: a
//! parser that silently sees half a file is worse than no parser.
//!
//! Before compilation, a resolve-only preflight rejects `sutura-domain/agreement` in each shipped
//! root's normal target dependencies, for every target declared by the cross list and host mapping.
//! Build dependencies, proc-macro hosts and dev dependencies are outside that projection. This is
//! resolved feature selection, not inspection of emitted code or proof of runtime data disclosure.
//!
//! **What it does NOT do**, stated because a green run invites the wider reading: it compiles the
//! host's default set only. A feature declared and never compiled by anything is still uncovered here - the
//! `--all-features` gates are what reach those - and this says nothing about a package that does not
//! ship. Nor does it link: `cargo check` and `cargo clippy` both stop at metadata, which is what
//! keeps it affordable and is also why the `cross` builds stay the authority on a musl link.
//!
//! **And it RUNS nothing, which for a whole category of test meant nobody did.** Stopping at
//! metadata compiles a `#[cfg(not(feature = "..."))]` test and never executes it, while every venue
//! that does run a test passes `--all-features`, where that cfg is false. `check-default-feature-tests`
//! is this lane's other half and its module header carries the measurement.

use std::path::Path;

use crate::Verdict;
use crate::repo;
use crate::warm_start::profile_for;

/// The declaration the package list is read out of.
pub(crate) const SOURCE: &str = "nix/shipped.nix";

/// Every `package = "..."` inside `nix/shipped.nix`'s `binaries = [ ... ]`, in declaration order.
///
/// Scoped to that list rather than grepping the file, and found anywhere on the line rather than at
/// its start - both for the reasons `shipped::declared` states at length beside its own parser: the
/// key appears elsewhere in that file, and `{ bin = "sutura"; package = "sutura-cli"; }` is one legal
/// record on one line. Duplicates are dropped, keeping first appearance, so two binaries out of one
/// package are one compile rather than two.
///
/// `pub(crate)` for exactly one other reader: `check-default-feature-tests` runs the same packages'
/// tests at the same feature set, and a second parser over one declaration is how two gates come to
/// disagree about which packages ship.
pub(crate) fn shipped_packages(text: &str) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    let mut indent: Option<usize> = None;
    for line in text.lines() {
        let trimmed = line.trim();
        let Some(open) = indent else {
            if trimmed.starts_with("binaries = [") {
                indent = Some(line.len().saturating_sub(line.trim_start().len()));
            }
            continue;
        };
        if trimmed == "];" && line.len().saturating_sub(line.trim_start().len()) == open {
            break;
        }
        for found in packages_in(line) {
            let owned = String::from(found);
            if !names.contains(&owned) {
                names.push(owned);
            }
        }
    }
    names
}

/// Every `package = "..."` value on one line, left to right.
///
/// The character before the key must not be part of a name, so a hypothetical `subPackage = "x"` is
/// not read as one. An unterminated quote yields nothing rather than the rest of the file.
fn packages_in(line: &str) -> impl Iterator<Item = &str> {
    const KEY: &str = "package = \"";
    let mut rest = line;
    core::iter::from_fn(move || {
        loop {
            let at = rest.find(KEY)?;
            let is_key = rest
                .get(..at)
                .and_then(|s| s.chars().next_back())
                .is_none_or(|c| !c.is_alphanumeric() && c != '_' && c != '-');
            let tail = rest.get(at.saturating_add(KEY.len())..)?;
            let end = tail.find('"')?;
            let value = tail.get(..end)?;
            rest = tail.get(end.saturating_add(1)..)?;
            if is_key && !value.is_empty() {
                return Some(value);
            }
        }
    })
}

/// One cargo invocation, named for the question it answers.
struct Pass {
    /// What this pass is called in the output.
    what: &'static str,
    /// The cargo subcommand and its arguments, before `--package`.
    lead: &'static [&'static str],
    /// Arguments after `--package`, which is where a lint level goes.
    tail: &'static [&'static str],
}

/// The two passes, and they are two because they see different code.
///
/// `check` answers *does the shipped configuration compile*; clippy answers *is it clean under this
/// workspace's lint set*, which includes the whole `restriction` category and `-D warnings`. A
/// `doc_markdown` on a `#[cfg(not(feature = ...))]` item is invisible to the first and to every
/// `--all-features` run, which is the measured case that put both here.
const PASSES: &[Pass] = &[
    Pass {
        what: "check",
        lead: &["check", "--all-targets"],
        tail: &[],
    },
    Pass {
        what: "clippy",
        lead: &["clippy", "--all-targets"],
        tail: &["--", "-D", "warnings"],
    },
];

/// The words one pass hands cargo, for one package, at one profile.
///
/// The profile travels with the SUBCOMMAND and never in [`Pass::tail`]: clippy's tail opens `--`,
/// and everything past that separator belongs to the lint driver rather than to cargo - so a
/// `--profile` appended there would select no profile, warm nothing, and not fail either. That is
/// the same confusion `check-warm-start` reads out of the flake app's own line, one level up, and
/// the reason this gate takes no flag at all.
fn invocation<'a>(pass: &Pass, package: &'a str, profile: Option<&'a str>) -> Vec<&'a str> {
    let mut words: Vec<&str> = pass.lead.to_vec();
    if let Some(name) = profile {
        words.extend(["--profile", name]);
    }
    words.extend(["--package", package]);
    words.extend(pass.tail.iter().copied());
    words
}

/// What a gate needs before it can compile anything the declaration names.
///
/// A struct rather than a tuple, and clippy asked for it: the two fields are a path and a list of
/// strings, which is exactly the pair a positional return gets wrong silently.
pub(crate) struct Shipped {
    /// The repo root, so a `Command` can set the working directory cargo resolves paths against.
    pub(crate) root: std::path::PathBuf,
    /// The packages `nix/shipped.nix` publishes, in declaration order.
    pub(crate) packages: Vec<String>,
}

/// The shipped package list, or the verdict to return instead.
///
/// **One owner for the fail-closed policy**, and that is the whole reason this is a function. The
/// rule - *a list this gate reads as empty checks nothing and passes, which is the one failure it
/// must not have* - was stated twice in two paraphrases once a second gate read the same declaration,
/// and a policy stated twice is a policy that drifts. The per-gate prologue is a house pattern here
/// (`shipped.rs` has a third instance against the same file), so what is shared is the part with no
/// precedent for duplication: this one, whose two readers also share the PARSER.
///
/// `gate` names the caller in every message, because a reader of a failure needs to know which gate
/// could not read the declaration.
pub(crate) fn shipped_or_fail(gate: &str) -> Result<Shipped, Verdict> {
    let Some(root) = repo::root() else {
        eprintln!("xtask {gate}: could not determine the repo root");
        return Err(Verdict::Fail);
    };
    let path = root.join(SOURCE);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("xtask {gate}: could not read {}: {error}", path.display());
            return Err(Verdict::Fail);
        }
    };
    let packages = shipped_packages(&text);
    if packages.is_empty() {
        eprintln!("xtask {gate}: FAILED - parsed no package out of {SOURCE}");
        eprintln!("  A list this gate reads as empty checks nothing and passes, which is the one");
        eprintln!("  failure it must not have. `binaries = [` and `package = \"...\";` are the two");
        eprintln!("  shapes it looks for.");
        return Err(Verdict::Fail);
    }
    Ok(Shipped { root, packages })
}

/// A cursor over just the literal list/map declarations this gate consumes, not a Nix evaluator.
struct TargetLiteral<'a> {
    rest: &'a str,
}

impl<'a> TargetLiteral<'a> {
    fn whitespace(&mut self) -> Option<()> {
        loop {
            self.rest = self.rest.trim_start();
            if self.rest.starts_with('#') {
                self.rest = self.rest.split_once('\n').map_or("", |(_, tail)| tail);
            } else if let Some(comment) = self.rest.strip_prefix("/*") {
                self.rest = comment.split_once("*/")?.1;
            } else {
                return Some(());
            }
        }
    }

    fn take(&mut self, token: &str) -> Option<()> {
        self.whitespace()?;
        self.rest = self.rest.strip_prefix(token)?;
        Some(())
    }

    fn quoted(&mut self) -> Option<&'a str> {
        self.take("\"")?;
        let (value, tail) = self.rest.split_once('"')?;
        if !package_name(value) {
            return None;
        }
        self.rest = tail;
        Some(value)
    }
}

/// Locate one live assignment; duplicate names and nonliteral bodies are refusals, not subsets.
fn target_literal<'a>(text: &'a str, name: &str) -> Option<TargetLiteral<'a>> {
    let mut found = None;
    let mut offset = 0;
    for (raw, line) in text.split_inclusive('\n').zip(crate::workflows::code_lines(text)) {
        if line.code.split_once('=').is_some_and(|(key, _)| key.trim() == name) {
            if found.is_some() {
                return None;
            }
            let (raw_key, _) = raw.split_once('=')?;
            if raw_key.trim() != name {
                return None;
            }
            found = Some(TargetLiteral {
                rest: text.get(offset + raw_key.len() + 1..)?,
            });
        }
        offset += raw.len();
    }
    found
}

/// Literal targets from both declarations, including host mappings other than the current host.
/// Expressions, interpolation, malformed records and unclosed/empty declarations fail closed.
fn artifact_targets(text: &str) -> Option<Vec<String>> {
    let mut targets = Vec::new();
    let mut cross = target_literal(text, "crossTargets")?;
    cross.take("[")?;
    loop {
        cross.whitespace()?;
        if cross.rest.starts_with(']') {
            break;
        }
        let target = cross.quoted()?;
        if target.split('-').count() < 3 {
            return None;
        }
        if !targets.iter().any(|known| known == target) {
            targets.push(String::from(target));
        }
    }
    cross.take("]")?;
    cross.take(";")?;
    if targets.is_empty() {
        return None;
    }
    let mut host = target_literal(text, "hostRustTarget")?;
    let mut keys = Vec::new();
    host.take("{")?;
    loop {
        host.whitespace()?;
        if host.rest.starts_with('}') {
            break;
        }
        let key = host.quoted()?;
        if keys.contains(&key) {
            return None;
        }
        keys.push(key);
        host.take("=")?;
        let target = host.quoted()?;
        host.take(";")?;
        if target.split('-').count() < 3 {
            return None;
        }
        if !targets.iter().any(|known| known == target) {
            targets.push(String::from(target));
        }
    }
    host.take("}")?;
    host.take(".${system}")?;
    host.take("or null;")?;
    (!keys.is_empty()).then_some(targets)
}

fn package_name(name: &str) -> bool {
    name.chars().next().is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
}

fn cargo_version(version: &str) -> bool {
    let Some(version) = version.strip_prefix('v') else {
        return false;
    };
    let (release, build) = version
        .split_once('+')
        .map_or((version, None), |(release, build)| (release, Some(build)));
    let (core, prerelease) = release
        .split_once('-')
        .map_or((release, None), |(core, pre)| (core, Some(pre)));
    core.split('.').count() == 3
        && core
            .split('.')
            .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()) && (part == "0" || !part.starts_with('0')))
        && prerelease.is_none_or(|pre| version_identifiers(pre, true))
        && build.is_none_or(|build| version_identifiers(build, false))
}

fn version_identifiers(text: &str, prerelease: bool) -> bool {
    text.split('.').all(|part| {
        !part.is_empty()
            && part.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
            && (!prerelease || !part.chars().all(|c| c.is_ascii_digit()) || part == "0" || !part.starts_with('0'))
    })
}

/// Cargo's marker for a package whose subtree was already printed.
const REPEAT: &str = " (*)";

struct FeatureRow<'a> {
    package: &'a str,
    features: &'a str,
}

/// Cargo's controlled package/feature columns, with an optional source locator.
/// No arbitrary suffix or malformed row may disappear while another row supplies the subject.
///
/// Deduplicated output marks a repeat with a trailing ` (*)` and elides its subtree. Only that exact
/// suffix is accepted, so every other unexpected trailing text is still a malformed row: the marker is
/// stripped rather than parsed. Eliding a repeat costs the walk nothing, because Cargo prints a
/// package's first occurrence in full - so every package's feature set is still read at least once,
/// which is all `inspect_features` asks of it.
fn feature_row(line: &str) -> Option<FeatureRow<'_>> {
    let line = line.strip_suffix(REPEAT).unwrap_or(line);
    let (identity, features) = line.split_once('|')?;
    let (package, rest) = identity.split_once(' ')?;
    let (version, source) = rest
        .split_once(' ')
        .map_or((rest, None), |(version, source)| (version, Some(source)));
    if !package_name(package) || !cargo_version(version) {
        return None;
    }
    if let Some(source) = source {
        let locator = source.strip_prefix('(')?.strip_suffix(')')?;
        let url = locator.split_once("://").is_some_and(|(scheme, address)| {
            !scheme.is_empty()
                && scheme
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
                && !address.is_empty()
                && !address.chars().any(char::is_whitespace)
        });
        if locator.chars().any(char::is_control) || (!Path::new(locator).is_absolute() && !url) {
            return None;
        }
    }
    if !features.is_empty()
        && !features.split(',').all(|feature| {
            !feature.is_empty()
                && feature
                    .chars()
                    .all(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '+' | '.'))
        })
    {
        return None;
    }
    Some(FeatureRow { package, features })
}

/// Missing root/subject rows are an incomplete answer, even when every present feature is allowed.
fn inspect_features(text: &str, root: &str) -> Result<(), &'static str> {
    let mut found_root = false;
    let mut found_domain = false;
    for line in text.lines() {
        let row = feature_row(line).ok_or("malformed Cargo package/feature row")?;
        found_root |= row.package == root;
        if row.package == "sutura-domain" {
            found_domain = true;
            if row.features.split(',').any(|feature| feature == "agreement") {
                return Err("sutura-domain/agreement is enabled in normal target dependencies");
            }
        }
    }
    if !found_root || !found_domain {
        return Err("Cargo output omitted the selected root or enrolled sutura-domain subject");
    }
    Ok(())
}

fn feature_preflight(root: &Path, packages: &[String]) -> Result<(), String> {
    let text = std::fs::read_to_string(root.join(SOURCE)).map_err(|error| format!("cannot read {SOURCE}: {error}"))?;
    let targets = artifact_targets(&text).ok_or("cannot read complete literal crossTargets and hostRustTarget declarations")?;
    let mut inspected = 0;
    for package in packages {
        for target in &targets {
            let output = std::process::Command::new("cargo")
                .current_dir(root)
                .args([
                    "tree",
                    "--offline",
                    "--locked",
                    "--package",
                    package,
                    "--target",
                    target,
                    "--edges",
                    "normal,no-proc-macro",
                    "--prefix",
                    "none",
                    "--format",
                    "{p}|{f}",
                    "--color",
                    "never",
                ])
                .output()
                .map_err(|error| format!("{package} / {target}: cannot run Cargo: {error}"))?;
            if !output.status.success() {
                return Err(format!(
                    "{package} / {target}: Cargo tree failed ({}): {}",
                    output.status,
                    String::from_utf8_lossy(&output.stderr)
                ));
            }
            let answer = String::from_utf8(output.stdout)
                .map_err(|error| format!("{package} / {target}: non-UTF-8 Cargo answer: {error}"))?;
            inspect_features(&answer, package).map_err(|error| format!("{package} / {target}: {error}"))?;
            inspected += 1;
        }
    }
    println!(
        "  resolved-feature admission: {inspected} root/target pair(s), targets: {}",
        targets.join(", ")
    );
    Ok(())
}

/// `cargo xtask check-default-features` - the shipped feature set compiles and lints.
pub(crate) fn run(args: &[String]) -> Verdict {
    if !args.is_empty() {
        eprintln!("usage: check-default-features - it takes no arguments");
        eprintln!("  The cargo profile is derived from the target directory rather than passed in.");
        return Verdict::Usage;
    }
    let Shipped { root, packages } = match shipped_or_fail("check-default-features") {
        Ok(read) => read,
        Err(verdict) => return verdict,
    };
    if let Err(error) = feature_preflight(&root, &packages) {
        eprintln!("xtask check-default-features: FAILED - {error}");
        return Verdict::Fail;
    }
    let target_dir = std::env::var_os("CARGO_TARGET_DIR");
    let profile = profile_for(target_dir.as_deref().map(Path::new));
    println!(
        "xtask check-default-features: {} shipped package(s) from {SOURCE}: {}",
        packages.len(),
        packages.join(", ")
    );
    println!("  cargo's DEFAULT feature set - the one `nix/shipped.nix` publishes and no other gate compiles.");
    if let Some(name) = profile {
        println!(
            "  profile `{name}` - the warmed artifacts' own, so the units that match this narrow feature set are reused. Not most of them: this module's header has the measurement."
        );
    }
    let mut failed: Vec<String> = Vec::new();
    for package in &packages {
        for pass in PASSES {
            println!("\n=== {} {package} ===", pass.what);
            let mut command = std::process::Command::new("cargo");
            command.current_dir(&root).args(invocation(pass, package, profile));
            match command.status() {
                Ok(status) if status.success() => {}
                Ok(_) => failed.push(format!("{} {package}", pass.what)),
                Err(error) => {
                    eprintln!("xtask check-default-features: could not run cargo: {error}");
                    return Verdict::Fail;
                }
            }
        }
    }
    if failed.is_empty() {
        println!(
            "\nxtask check-default-features: ok - {} package(s) compile and lint at their default features",
            packages.len()
        );
        return Verdict::Pass;
    }
    eprintln!(
        "\nxtask check-default-features: FAILED - {}: {}",
        failed.len(),
        failed.join(", ")
    );
    eprintln!("  `just lint` and `just test` pass --all-features and cannot see this, and neither can");
    eprintln!("  `just check-changed`. What ships is what this compiled.");
    Verdict::Fail
}

#[cfg(test)]
mod tests {
    use super::{PASSES, artifact_targets, feature_row, inspect_features, invocation, shipped_packages};

    /// The flake output CI reaches this gate through.
    const APP: &str = "default-features";

    /// The name `main.rs` registers this gate under.
    const TASK: &str = "check-default-features";

    /// The recipe that is the developer's lane.
    const RECIPE: &str = "gates";

    /// The workflow the CI lane lives in.
    const WORKFLOW: &str = ".github/workflows/ci.yml";

    /// The job it has to be in, which is the one required context.
    const JOB: &str = "ci";

    /// The condition every rust step in that job is gated on.
    const CLASSIFIED: &str = "steps.classify.outputs.rust == 'true'";

    #[test]
    fn target_declarations_are_complete_literal_inputs_not_a_host_subset() {
        let nix = concat!(
            "crossTargets = [\n \"aarch64-unknown-linux-gnu\" # ignored\n];\n",
            "hostRustTarget = {\n",
            " \"aarch64-linux\" = \"aarch64-unknown-linux-gnu\";\n",
            " \"x86_64-linux\" = \"x86_64-unknown-linux-gnu\";\n",
            " /* ignored */ \"aarch64-darwin\" = \"aarch64-apple-darwin\";\n",
            "}.${system} or null;\n",
        );
        assert_eq!(
            artifact_targets(nix),
            Some(vec![
                String::from("aarch64-unknown-linux-gnu"),
                String::from("x86_64-unknown-linux-gnu"),
                String::from("aarch64-apple-darwin"),
            ])
        );
        for broken in [
            nix.replace(
                "crossTargets = [\n \"aarch64-unknown-linux-gnu\" # ignored\n];",
                "/* = [\"x86_64-unknown-linux-gnu\"]; */ crossTargets = [\"x86_64-unknown-linux-musl\"];",
            ),
            nix.replace("crossTargets =", "renamed ="),
            nix.replace("hostRustTarget =", "renamed ="),
            nix.replace("];", "] ++ other;"),
            nix.replace("}.${system} or null;", ""),
            nix.replace("\"x86_64-unknown-linux-gnu\"", "computedTarget"),
            nix.replace("\"aarch64-darwin\"", "\"x86_64-linux\""),
            format!("{nix}\ncrossTargets = [];\n"),
            String::from("crossTargets = [];\nhostRustTarget = {}.${system} or null;\n"),
        ] {
            assert!(
                artifact_targets(&broken).is_none(),
                "an incomplete target declaration was admitted: {broken}"
            );
        }
    }

    #[test]
    fn cargo_rows_keep_empty_features_but_refuse_incomplete_identities() {
        for row in [
            "sutura-domain v0.1.0|",
            "other v1.2.3-alpha.1+build (https://example.com/source#commit)|default",
        ] {
            assert!(feature_row(row).is_some(), "valid controlled row: {row}");
        }
        for row in [
            "sutura-domain|agreement",
            "sutura-domain v1.0|",
            "sutura-domain v1.0.0+|",
            "sutura-domain v1.0.0-01|",
            "sutura-domain v01.0.0|",
            "sutura-domain v1.0.0 ()|",
            "sutura-domain v1.0.0 (proc-macro)|",
            "sutura-domain v1.0.0 extra|",
            "sutura-domain v1.0.0|default,,agreement",
            "sutura-domain v1.0.0|default|agreement",
        ] {
            assert!(feature_row(row).is_none(), "malformed controlled row: {row}");
        }
    }

    /// Deduplicated output is what the walk reads now that `--no-dedupe` is gone, so the repeat
    /// marker has to be accepted - and *only* that marker, or the refusal of a malformed row would
    /// have been widened into accepting arbitrary trailing text.
    #[test]
    fn only_cargos_exact_repeat_marker_survives_the_row_parser() {
        for row in [
            "sutura-domain v0.1.0| (*)",
            "sutura-domain v0.1.0|default,agreement (*)",
            "other v1.2.3 (https://example.com/s#c)|default (*)",
        ] {
            let parsed = feature_row(row).unwrap_or_else(|| panic!("a repeat row is a row: {row}"));
            assert!(!parsed.features.contains('*'), "the marker is stripped, not parsed: {row}");
        }
        for row in [
            "sutura-domain v0.1.0|(*)",
            "sutura-domain v0.1.0| (**)",
            "sutura-domain v0.1.0| (*) trailing",
            "sutura-domain v0.1.0 (*)|",
            "sutura-domain v1.0|default (*)",
        ] {
            assert!(feature_row(row).is_none(), "not Cargo's marker, so still malformed: {row}");
        }
    }

    /// The elision a repeat marker stands for must not hide the enrolled subject: Cargo prints a
    /// package's FIRST occurrence in full, so one complete row is enough for the agreement refusal.
    #[test]
    fn a_deduplicated_walk_still_refuses_agreement_on_the_first_occurrence() {
        let deduped = "root v0.1.0|\nsutura-domain v0.1.0|default,agreement\nsutura-domain v0.1.0|default,agreement (*)\n";
        assert_eq!(
            inspect_features(deduped, "root"),
            Err("sutura-domain/agreement is enabled in normal target dependencies")
        );
        let repeat_only = "root v0.1.0|\nsutura-domain v0.1.0| (*)\n";
        assert_eq!(inspect_features(repeat_only, "root"), Ok(()));
    }

    #[test]
    fn every_domain_row_is_inspected_and_feature_names_are_exact_tokens() {
        let clean = "root v0.1.0|\nsutura-domain v0.1.0|\nsutura-domain-extra v0.1.0|agreement\nsutura-domain v0.1.0|agreement-extra,default\n";
        inspect_features(clean, "root").expect("complete clean feature rows");
        assert!(inspect_features(&format!("{clean}sutura-domain v0.1.0|default,agreement\n"), "root").is_err());
        assert!(inspect_features(&format!("{clean}unparseable\n"), "root").is_err());
        for missing in ["", "root v0.1.0|\n", "sutura-domain v0.1.0|\n"] {
            assert!(inspect_features(missing, "root").is_err());
        }
    }

    #[test]
    fn every_package_in_the_binaries_list_is_read_in_declaration_order() {
        let nix = concat!(
            "  binaries = [\n",
            "    {\n",
            "      bin = \"sutura\";\n",
            "      package = \"sutura-cli\";\n",
            "    }\n",
            "    {\n",
            "      bin = \"sutura-serve\";\n",
            "      package = \"sutura-serve\";\n",
            "    }\n",
            "  ];\n",
        );
        assert_eq!(shipped_packages(nix), vec!["sutura-cli", "sutura-serve"]);
    }

    #[test]
    fn a_record_written_on_one_line_is_still_a_record() {
        // The failure `shipped::declared`'s own tests forced: `nixpkgs-fmt`'s shape is not the only
        // legal one, and a parser that reads fewer packages than are declared passes by checking
        // less - which is the one failure mode this gate may not have.
        let nix = "  binaries = [\n    { bin = \"sutura\"; package = \"sutura-cli\"; }\n  ];\n";
        assert_eq!(shipped_packages(nix), vec!["sutura-cli"]);
    }

    #[test]
    fn a_longer_key_ending_in_package_is_not_a_package() {
        let nix = "  binaries = [\n    { subPackage = \"decoy\"; package = \"sutura-cli\"; }\n  ];\n";
        assert_eq!(shipped_packages(nix), vec!["sutura-cli"]);
    }

    #[test]
    fn a_package_outside_the_list_is_not_a_declaration() {
        // The scoping reason: this key appears elsewhere in that file, and a whole-file grep would
        // compile packages nothing publishes.
        let nix = concat!(
            "  someOther = { package = \"not-shipped\"; };\n",
            "  binaries = [\n",
            "    { package = \"sutura-cli\"; }\n",
            "  ];\n",
            "  after = { package = \"also-not-shipped\"; };\n",
        );
        assert_eq!(shipped_packages(nix), vec!["sutura-cli"]);
    }

    #[test]
    fn two_binaries_out_of_one_package_are_one_compile() {
        let nix = concat!(
            "  binaries = [\n",
            "    { bin = \"one\"; package = \"sutura-cli\"; }\n",
            "    { bin = \"two\"; package = \"sutura-cli\"; }\n",
            "  ];\n",
        );
        assert_eq!(shipped_packages(nix), vec!["sutura-cli"]);
    }

    #[test]
    fn every_pass_puts_the_profile_where_cargo_reads_it_and_not_past_the_lint_separator() {
        // The failure this exists for: clippy's tail opens `--`, so a profile appended to the end
        // of the line goes to the lint driver instead of to cargo. Nothing errors - the run just
        // compiles at the default profile, reuses none of the warmed artifacts and passes, so the
        // only symptom is a CI step quietly paying for a whole dependency build.
        for pass in PASSES {
            let words = invocation(pass, "sutura-serve", Some("ci"));
            let at = words
                .windows(2)
                .position(|pair| pair == ["--profile", "ci"])
                .unwrap_or_else(|| panic!("{} must pass `--profile ci`, got {words:?}", pass.what));
            assert!(at > 0, "{} must keep the subcommand first, got {words:?}", pass.what);
            if let Some(separator) = words.iter().position(|word| *word == "--") {
                assert!(
                    at < separator,
                    "{} puts the profile past `--`, where cargo never sees it: {words:?}",
                    pass.what
                );
            }
        }
    }

    #[test]
    fn the_lint_level_is_still_the_last_thing_clippy_is_told() {
        // The other half of that seam: threading a profile in must not reorder `-D warnings` out of
        // the driver's arguments, which would turn the lint half into a warning-only run.
        let clippy = PASSES
            .iter()
            .find(|pass| pass.what == "clippy")
            .expect("a clippy pass is declared");
        let words = invocation(clippy, "sutura-cli", Some("ci"));
        assert_eq!(
            words.get(words.len().saturating_sub(3)..),
            Some(&["--", "-D", "warnings"][..])
        );
    }

    #[test]
    fn no_profile_argument_leaves_cargo_on_the_developers_default() {
        // `just gates` passes nothing and must not be pushed into a second profile: locally that is
        // a second dependency build in the same target directory for an identical verdict.
        for pass in PASSES {
            let words = invocation(pass, "sutura-cli", None);
            assert!(!words.contains(&"--profile"), "{} named a profile: {words:?}", pass.what);
            assert!(words.windows(2).any(|pair| pair == ["--package", "sutura-cli"]));
        }
    }

    #[test]
    fn both_lanes_still_invoke_this_gate() {
        // A gate reachable from neither lane is a module, and this lane's whole history is a check
        // that existed while nothing ran it. Two readers, therefore: the developer's `just gates`
        // and the required CI job. `check-workflows` holds the other direction, that the app this
        // names is declared in flake.nix.
        //
        // NEITHER READER IS `contains` OVER RAW TEXT, which is the point of this test rather than
        // a detail of it. `#     cargo run -q -p xtask -- check-default-features` in the recipe and
        // `# run: nix run .#default-features` in the workflow each satisfy a substring while no
        // lane invokes anything - the dead-check shape, in the test that says the check is wired.
        // So a comment line of the recipe body is dropped, and the workflow is read through
        // `workflows::step`, whose reader skips a `#` line.
        let root = crate::repo::root().expect("the repo root");
        assert!(
            crate::TASKS.iter().any(|task| task.name == TASK),
            "{TASK} is not a registered task"
        );
        let body = crate::tasks::recipe_body(&root, RECIPE).expect("a `gates` recipe in the justfile");
        assert!(
            body.iter()
                .any(|line| !line.trim_start().starts_with('#') && line.contains(TASK)),
            "`just {RECIPE}` no longer runs {TASK} on a line that is not a comment"
        );
        let workflow = std::fs::read_to_string(root.join(WORKFLOW)).expect("ci.yml");
        let step = crate::workflows::step::app_step(&workflow, JOB, APP)
            .unwrap_or_else(|| panic!("a live `nix run .#{APP}` step inside {WORKFLOW}'s `{JOB}` job"));
        let declared = step.join("\n");
        // The two ways the step stays in the file and stops being a gate. A job under a NEW name
        // would be the third and is not reachable from here: which contexts are required is a
        // branch-ruleset setting no file in this tree states, which is why the step is in `{JOB}`.
        assert!(
            !declared.contains("continue-on-error"),
            "the {APP} step tolerates its own failure, so the CI half reports rather than gates:\n{declared}"
        );
        assert!(
            declared.contains(CLASSIFIED),
            "the {APP} step is not gated on `{CLASSIFIED}`, which is the condition the rust steps around it use:\n{declared}"
        );
    }

    #[test]
    fn a_list_this_parser_cannot_find_reads_as_empty_so_the_gate_can_fail_closed() {
        // `run` turns this into a FAILURE rather than a pass, which is the whole of why the parser
        // is allowed to answer nothing.
        assert!(
            shipped_packages("nothing that looks like a binaries list").is_empty(),
            "an unparseable list yields no shipped packages"
        );
    }
}
