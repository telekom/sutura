//! The outbound HTTP client is shared with a build-dependency, and that is a MEASUREMENT rather than
//! a property - so it gets a gate.
//!
//! `docs/adr/0018` decides that `sutura-exec-bigquery`'s wire calls `jobs.query` over `ureq`, and the
//! number that decided it is **zero new packages in `Cargo.lock`**: the same version with the same two
//! features is already resolved as a build-dependency of `libduckdb-sys`, whose downloader never runs
//! here. That record's own last consequence said the obvious thing about it:
//!
//! > A future `just update` that moves `ureq` to a version whose feature set no longer matches what
//! > `libduckdb-sys` resolves would turn the +0 into a real number. Nothing gates that.
//!
//! `AGENTS.md` is unambiguous about what to do with a sentence like that - *put deterministic
//! requirements in a task, a hook, a lint or a generated contract, never in prose a human is expected
//! to remember. A rule with no mechanism is a wish.* This is the mechanism.
//!
//! # Two rules, and the second is the one that would rot silently
//!
//! * **One `ureq` in the lock.** If a `just update` ever resolves two, the +0 is a real number and the
//!   licence and supply-chain arguments in 0018 are measuring the wrong graph. This is the cheap,
//!   loud half.
//! * **`libduckdb-sys` still depends on `ureq`.** This is the PREMISE of the +0, and it can stop being
//!   true without anything else breaking: upstream could drop the downloader, or the `DuckDB` adapter
//!   could stop being a dev-dependency. Nothing would fail - `ureq` would simply become a first-party
//!   dependency with a first-party cost - and 0018 would go on claiming a measurement whose reason had
//!   evaporated. Failing here forces the record to be re-taken rather than quietly inherited.
//!
//! # What it deliberately does NOT check
//!
//! **The feature sets.** 0018's stronger claim - *same version, same features* - is not checkable from
//! `Cargo.lock`, which records resolved packages and their dependency names and not the feature
//! selection that produced them. Checking it needs `cargo metadata`'s resolve graph, which is a
//! process invocation and a JSON parser in a crate that has one dependency. So the gate checks the two
//! facts a text scan can establish exactly, and this paragraph is why the third is absent - which is
//! better than a gate that reads as if it covered it.
//!
//! `deny.toml`'s `multiple-versions = "warn"` does not cover this: it warns, deliberately, because
//! denying it needs a skip list of twenty-seven entries that rots on every update. This gate is the
//! narrow version aimed at the one crate whose duplication would falsify a written measurement - the
//! same shape, and the same argument, as `check-arrow`.

use crate::Verdict;
use crate::repo;

/// The lock file, read as text. The same choice `arrow_major` makes and for the same reason.
const LOCK: &str = "Cargo.lock";

/// The client whose sharing 0018 measured.
const CLIENT: &str = "ureq";

/// The build-dependency that already resolved it, which is the +0's premise.
const SHARER: &str = "libduckdb-sys";

/// The record that would have to be re-taken if either rule fails.
const RECORD: &str = "docs/adr/0018-what-the-bigquery-wire-is-built-from.md";

/// Every version of `name` the lock holds.
///
/// The shape this relies on is `cargo`'s own output: within a `[[package]]` stanza, `name` precedes
/// `version`, and both are `key = "value"` on their own line. Same parse as `arrow_major::packages`,
/// deliberately not shared with it - that one collects the whole file into a `Vec<Package>` to group
/// by major, and one call site wanting two fields is not enough to justify a common abstraction
/// between two gates that would then have to change together.
fn versions_of(lock: &str, name: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut current: Option<&str> = None;
    for line in lock.lines() {
        if let Some(value) = line.strip_prefix("name = ") {
            current = unquote(value);
        } else if let Some(value) = line.strip_prefix("version = ")
            && let Some(seen) = current.take()
            && seen == name
            && let Some(version) = unquote(value)
        {
            found.push(String::from(version));
        }
    }
    found
}

/// Strip the surrounding quotes from a lock-file value, or `None` if it is not quoted.
fn unquote(value: &str) -> Option<&str> {
    value.strip_prefix('"')?.strip_suffix('"')
}

/// Does `holder`'s stanza list `needed` among its dependencies?
///
/// Scoped to one stanza rather than grepping the file, because `"ureq"` appears in its own stanza and
/// in `ureq-proto`'s, and a whole-file search would answer yes for the wrong reason. A stanza runs from
/// its `name = ` line to the next `[[package]]`.
fn depends_on(lock: &str, holder: &str, needed: &str) -> bool {
    let mut inside = false;
    let quoted = format!("\"{needed}\",");
    for line in lock.lines() {
        if line.starts_with("[[package]]") {
            inside = false;
        } else if let Some(value) = line.strip_prefix("name = ") {
            inside = unquote(value) == Some(holder);
        } else if inside && line.trim() == quoted {
            return true;
        }
    }
    false
}

/// `cargo xtask check-shared-client` - the two facts `docs/adr/0018`'s +0 rests on.
pub(crate) fn run(_args: &[String]) -> Verdict {
    let Some(root) = repo::root() else {
        eprintln!("xtask check-shared-client: could not determine the repo root");
        return Verdict::Fail;
    };
    let path = root.join(LOCK);
    let lock = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("xtask check-shared-client: could not read {}: {error}", path.display());
            return Verdict::Fail;
        }
    };

    let found = versions_of(&lock, CLIENT);
    if found.is_empty() {
        eprintln!("xtask check-shared-client: FAILED - no `{CLIENT}` in {LOCK}");
        eprintln!("  {RECORD} measures the wire's dependency cost against `{CLIENT}` being resolved.");
        eprintln!("  If the client has changed, that record is what has to be re-taken.");
        return Verdict::Fail;
    }
    if found.len() > 1 {
        eprintln!(
            "xtask check-shared-client: FAILED - {} versions of `{CLIENT}` in {LOCK}",
            found.len()
        );
        for version in &found {
            eprintln!("  {CLIENT} {version}");
        }
        eprintln!();
        eprintln!("  {RECORD} states that the wire costs ZERO new packages, because `{SHARER}`");
        eprintln!("  already resolves the same version with the same features. Two versions means the");
        eprintln!("  cost is a real number: re-measure it and rewrite that record, or align the two");
        eprintln!("  requirements so one version resolves again.");
        return Verdict::Fail;
    }

    if !depends_on(&lock, SHARER, CLIENT) {
        eprintln!("xtask check-shared-client: FAILED - `{SHARER}` no longer depends on `{CLIENT}`");
        eprintln!("  That is the PREMISE of the zero-cost measurement in {RECORD}, and nothing else");
        eprintln!("  breaks when it stops holding: `{CLIENT}` simply becomes a first-party dependency");
        eprintln!("  with a first-party cost. Re-measure the closure and rewrite that record's");
        eprintln!("  *Why `ureq` and not `reqwest`* section, which is where the number lives.");
        return Verdict::Fail;
    }

    let version = found.first().map_or("?", String::as_str);
    println!("xtask check-shared-client: ok - one `{CLIENT}` ({version}), still shared with `{SHARER}`");
    Verdict::Pass
}

#[cfg(test)]
mod tests {
    use core::fmt::Write as _;

    use super::{depends_on, versions_of};

    /// A lock stanza, so the tests read like the file they parse.
    fn stanza(name: &str, version: &str, deps: &[&str]) -> String {
        let mut out = format!("[[package]]\nname = \"{name}\"\nversion = \"{version}\"\n");
        if !deps.is_empty() {
            out.push_str("dependencies = [\n");
            for dep in deps {
                // `write!` rather than `push_str(&format!(..))`: `clippy::format_push_string` is
                // denied here, and the reason it is denied is the intermediate allocation this avoids.
                // Writing into a `String` is infallible, and this is a fixture builder in a test.
                writeln!(out, " \"{dep}\",").expect("writing into a String cannot fail");
            }
            out.push_str("]\n");
        }
        out
    }

    #[test]
    fn one_version_of_the_client_is_read_out_of_a_lock_file() {
        let lock = format!("{}{}", stanza("ureq", "3.4.0", &["rustls"]), stanza("serde", "1.0.0", &[]));
        assert_eq!(versions_of(&lock, "ureq"), vec![String::from("3.4.0")]);
        assert!(versions_of(&lock, "reqwest").is_empty());
    }

    #[test]
    fn two_versions_are_both_reported_so_the_message_can_name_them() {
        // The failure this gate exists for: a `just update` that resolves a second one. Both are
        // collected rather than counted, because a message naming neither is a message nobody can act
        // on.
        let lock = format!("{}{}", stanza("ureq", "3.4.0", &[]), stanza("ureq", "4.0.0", &[]));
        assert_eq!(versions_of(&lock, "ureq"), vec![String::from("3.4.0"), String::from("4.0.0")]);
    }

    #[test]
    fn a_version_line_with_no_preceding_name_is_not_a_package() {
        // `name` is taken rather than copied, so a stray `version = ` after one stanza cannot be
        // attributed to the package before it.
        let lock = "name = \"ureq\"\nversion = \"3.4.0\"\nversion = \"9.9.9\"\n";
        assert_eq!(versions_of(lock, "ureq"), vec![String::from("3.4.0")]);
    }

    #[test]
    fn the_dependency_check_is_scoped_to_one_stanza() {
        // **The reason this is not a grep.** `"ureq",` appears inside `ureq-proto`'s own stanza too, so
        // a whole-file search answers yes for the wrong reason - and would keep answering yes after
        // `libduckdb-sys` stopped depending on it, which is exactly the drift this rule watches.
        let lock = format!(
            "{}{}",
            stanza("libduckdb-sys", "1.0.0", &["flate2"]),
            stanza("ureq-proto", "0.6.1", &["ureq"])
        );
        assert!(!depends_on(&lock, "libduckdb-sys", "ureq"));

        let lock = format!(
            "{}{}",
            stanza("libduckdb-sys", "1.0.0", &["flate2", "ureq"]),
            stanza("ureq", "3.4.0", &[])
        );
        assert!(depends_on(&lock, "libduckdb-sys", "ureq"));
    }

    #[test]
    fn a_holder_that_is_not_in_the_lock_depends_on_nothing() {
        let lock = stanza("serde", "1.0.0", &["serde_core"]);
        assert!(!depends_on(&lock, "libduckdb-sys", "ureq"));
    }

    #[test]
    fn the_real_lock_satisfies_both_rules() {
        // The gate against the tree it guards, so a refactor of the parse cannot pass its own fixtures
        // and fail the file. `arrow_major`'s suite does not do this and could; it is cheap here because
        // the lock is committed.
        let Some(root) = crate::repo::root() else {
            return;
        };
        let Ok(lock) = std::fs::read_to_string(root.join("Cargo.lock")) else {
            return;
        };
        assert_eq!(
            versions_of(&lock, "ureq").len(),
            1,
            "the workspace resolves more than one ureq"
        );
        assert!(
            depends_on(&lock, "libduckdb-sys", "ureq"),
            "libduckdb-sys no longer shares ureq, so docs/adr/0018's measurement needs re-taking"
        );
    }
}
