//! The outbound HTTP client is ONE client every first-party dialler shares, and that is a MEASUREMENT
//! rather than a property - so it gets a gate.
//!
//! `docs/adr/0018` decides that outbound calls go over `ureq`. Its first measurement was **zero new
//! packages in `Cargo.lock`**, because `libduckdb-sys`'s downloader already resolved the same version;
//! that premise left with the `duckdb` crate (`telekom/sutura#913`), this gate's former second rule
//! failed exactly as it was written to, and 0018's nineteenth amendment re-took the number: `ureq`,
//! `ureq-proto` and `utf8-zero` are the three packages only `ureq` brings, and its TLS stack is shared
//! with the rest of the graph.
//!
//! `AGENTS.md` is unambiguous about what to do with a measurement like that - *put deterministic
//! requirements in a task, a hook, a lint or a generated contract, never in prose a human is expected
//! to remember. A rule with no mechanism is a wish.* This is the mechanism.
//!
//! # Two rules, and the second is the one that would rot silently
//!
//! * **One `ureq` in the lock.** If a `just update` ever resolves two, the three-package cost is a
//!   larger number and the licence and supply-chain arguments in 0018 are measuring the wrong graph.
//!   This is the cheap, loud half.
//! * **None of the crates `docs/adr/0023`'s no-client measurement names resolves in the lock.**
//!   That record measures the agent surface's transport and says it pulls no HTTP client because
//!   `server-side-http` names neither `reqwest` nor `oauth2` and neither resolves in
//!   `Cargo.lock`. A future resolve could add one of the two
//!   without anything else breaking, and 0023 would go on claiming a no-client property its own
//!   feature closure no longer has. The list is the ADR's own two names - a gate that refused
//!   other crates would enforce a property no record measures.
//!
//! # What it deliberately does NOT check
//!
//! **The feature sets.** 0018's stronger claim - *same version, same features* - is not checkable from
//! `Cargo.lock`, which records resolved packages and their dependency names and not the feature
//! selection that produced them. Checking it needs `cargo metadata`'s resolve graph, which is a
//! process invocation and a JSON parser in a crate that has one dependency. So the gate checks the
//! facts a text scan can establish exactly - the one about `ureq` and the two names 0023 forbids -
//! and this paragraph is why the feature sets are absent, which is better than a gate that reads as
//! if it covered them.
//!
//! **A client under another name.** The lock records the name `cargo` RESOLVED, so the second rule
//! holds the two names the ADR's measurement used and nothing wider: a fork of a forbidden client
//! published under its own name, or a crate that re-exports one, resolves under a name this scan
//! cannot see. A client arriving under another name is the same second client 0018's *Why `ureq`
//! and not `reqwest`* exists to keep out - that one is a decision with its own record; this rule
//! is its lock-level half.
//!
//! `deny.toml`'s `multiple-versions = "warn"` does not cover this: it warns, deliberately, because
//! denying it needs a skip list of twenty-seven entries that rots on every update. This gate is the
//! narrow version aimed at the crates whose duplication or arrival would falsify a written
//! measurement - the same shape, and the same argument, as `check-arrow`.

use crate::Verdict;
use crate::repo;

/// The lock file, read as text. The same choice `arrow_major` makes and for the same reason.
const LOCK: &str = "Cargo.lock";

/// The client whose cost 0018 measured.
const CLIENT: &str = "ureq";

/// The record that would have to be re-taken if the first rule fails.
const RECORD: &str = "docs/adr/0018-what-the-bigquery-wire-is-built-from.md";

/// The two crates `docs/adr/0023`'s no-client measurement names, and the only two the second rule
/// refuses. That record's claim is about its transport's feature closure, so a gate that refused
/// other names would enforce a property no record measures; a client arriving under another name
/// is a different defect, and the module header says what this scan cannot see about it.
const FORBIDDEN: &[&str] = &["reqwest", "oauth2"];

/// The record whose no-client measurement the second rule protects.
const RECORD_0023: &str = "docs/adr/0023-how-the-agent-surface-learns-who-is-asking.md";

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

/// `cargo xtask check-shared-client` - the one-client fact `docs/adr/0018`'s measurement rests on,
/// and the `docs/adr/0023` no-client fact that was held by review while this gate checked only `ureq`.
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
    check_lock(&lock)
}

/// The two rules against a lock's text: the entry `run` calls after reading the file, split out
/// so a test can drive the gate's real logic on a fixture lock rather than on the helper beneath it.
fn check_lock(lock: &str) -> Verdict {
    let found = versions_of(lock, CLIENT);
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
        eprintln!("  {RECORD} measures `{CLIENT}`'s first-party cost against ONE resolved version. Two");
        eprintln!("  versions means the cost is a larger number: re-measure it in a new amendment to that");
        eprintln!("  record, or align the two requirements so one version resolves again.");
        return Verdict::Fail;
    }

    let present: Vec<String> = FORBIDDEN
        .iter()
        .flat_map(|name| {
            versions_of(lock, name)
                .into_iter()
                .map(move |version| format!("{name} {version}"))
        })
        .collect();
    if !present.is_empty() {
        eprintln!("xtask check-shared-client: FAILED - one of the crates {RECORD_0023} forbids resolves in {LOCK}");
        for resolved in &present {
            eprintln!("  {resolved}");
        }
        eprintln!();
        eprintln!("  That record's no-client property is a measurement of its transport's feature closure");
        eprintln!("  on the lock it names, and a lock that resolves one of the two is no longer that lock.");
        eprintln!("  Re-measure the closure and rewrite that record's *What turning the feature on");
        eprintln!("  costs* section, or align the requirement so the client stops resolving.");
        return Verdict::Fail;
    }

    let version = found.first().map_or("?", String::as_str);
    println!("xtask check-shared-client: ok - one `{CLIENT}` ({version}), and none of the crates {RECORD_0023} forbids resolves");
    Verdict::Pass
}

#[cfg(test)]
mod tests {
    use core::fmt::Write as _;

    use super::{check_lock, versions_of};

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
        assert!(
            versions_of(&lock, "reqwest").is_empty(),
            "a client absent from the lock file has no versions"
        );
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
    fn two_versions_of_the_client_trip_the_gate_through_its_entry() {
        let lock = format!(
            "{}{}",
            stanza("ureq", "3.4.0", &["rustls"]),
            stanza("ureq", "4.0.0", &["rustls"]),
        );
        assert_eq!(
            check_lock(&lock),
            crate::Verdict::Fail,
            "a lock with two `ureq` versions is not the lock 0018 measured"
        );
    }

    #[test]
    fn the_real_lock_resolves_one_client() {
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
    }

    #[test]
    fn a_forbidden_client_reached_from_the_server_side_trips_the_gate_not_just_the_helper() {
        // The red fixture: a lock the ADR did NOT measure, because one of the two crates it
        // forbids resolves - reached from `sutura-http`, the crate whose `server-side-http` feature
        // is the transport 0023 measured. The assertion drives `check_lock`, the entry `run` calls
        // after reading the file, so the gate's own second rule trips the verdict rather than a test
        // that only touches the `versions_of` helper beneath it.
        let lock = format!(
            "{}{}{}{}",
            stanza("ureq", "3.4.0", &["rustls"]),
            stanza("libduckdb-sys", "1.0.0", &["ureq"]),
            stanza("sutura-http", "0.1.0", &["reqwest"]),
            stanza("reqwest", "0.12.0", &["hyper"]),
        );
        assert_eq!(
            check_lock(&lock),
            crate::Verdict::Fail,
            "a lock that resolves `reqwest` is no longer the lock 0023 measured, and the gate's own entry must fail"
        );
    }

    #[test]
    fn the_real_lock_forbids_nothing_the_adr_0023_names() {
        // The second rule against the tree it guards, beside the existing real-lock test: a
        // refactor of the parse cannot pass its fixtures and miss the file.
        let Some(root) = crate::repo::root() else {
            return;
        };
        let Ok(lock) = std::fs::read_to_string(root.join("Cargo.lock")) else {
            return;
        };
        for name in super::FORBIDDEN {
            assert!(
                versions_of(&lock, name).is_empty(),
                "{name} resolves in the lock, so docs/adr/0023's no-client measurement needs re-taking"
            );
        }
    }
}
