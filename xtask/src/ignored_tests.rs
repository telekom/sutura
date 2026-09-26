//! `cargo xtask check-ignored-tests` - no `#[ignore]` on a test without a line in
//! `devco/ignored-tests`.
//!
//! An `#[ignore]` silences a test from the default suite, and a change that adds one to a live cell
//! turned it off without anything saying so. This gate reads every first-party `.rs` file under
//! `crates/` and `xtask/`, names each `#[ignore]`d test through the causality gate's own attribute
//! vocabulary ([`attributes::cells`]), and compares the set against the committed baseline. It is a
//! ratchet in both directions: an ignored test the baseline does not list is refused, and so is a
//! baseline line naming a test that is no longer ignored - a stale line would let the same ignore
//! come back unnoticed.
//!
//! **One vocabulary, not two.** What makes a test ignored is already decided in
//! `causality::attributes`, whose header says why a second copy is a second thing to keep true; this
//! gate asks that module rather than re-reading attributes itself, so a wrapped `#[ignore = ".."]`,
//! an `#[ignore]` on either side of `#[test]` and a commented-out test are answered the same way
//! here as there.
//!
//! # What it does NOT hold
//!
//! * **A test switched off another way.** `#[cfg_attr(.., ignore)]`, a `#[cfg(..)]` over the cell
//!   or over the module holding it, and a feature-gated test target are not a literal `#[ignore]`,
//!   and `attributes::cells` reports the first two as undecidable rather than ignored. This gate
//!   reads the literal attribute only.
//! * **Which venue runs an ignored test.** A baseline line says the ignore is deliberate; whether a
//!   task runs the cell is `check-venues`' question.
//! * **A test declared from a macro.** A `#[test]` a `macro_rules!` body expands is not in the
//!   text, so an `#[ignore]` written inside that body is not seen.

use std::collections::BTreeSet;

use crate::Verdict;
use crate::causality::attributes;
use crate::repo;
use crate::serde_parse::scan::code_lines;

/// The committed baseline: one `<path>::<fn_name>` per ignored test, `#` comments allowed.
const BASELINE_FILE: &str = "devco/ignored-tests";

/// First-party Rust under `crates/` and `xtask/`.
fn in_scope(rel: &str) -> bool {
    let is_rs = std::path::Path::new(rel)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("rs"));
    is_rs && (rel.starts_with("crates/") || rel.starts_with("xtask/"))
}

/// What one file says: its ignored tests as `<rel>::<fn_name>`, and every declaration the attribute
/// scan could not resolve - a caller that cannot see a test cannot say whether it is ignored.
struct Found {
    ignored: Vec<String>,
    unresolved: Vec<String>,
}

fn ignored_in(rel: &str, text: &str) -> Found {
    let cells = attributes::cells(text, &code_lines(text).join("\n"));
    let ignored = cells.ignored().iter().map(|name| format!("{rel}::{name}")).collect();
    let unresolved = cells.unresolved().iter().map(|why| format!("{rel}: {why}")).collect();
    Found { ignored, unresolved }
}

/// Every baseline line, comments and blank lines skipped.
fn parse_baseline(text: &str) -> BTreeSet<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(String::from)
        .collect()
}

/// The ignored tests the baseline does not list - the refusal this gate exists for.
fn new_ignores<'a>(live: &'a BTreeSet<String>, baselined: &'a BTreeSet<String>) -> Vec<&'a String> {
    live.difference(baselined).collect()
}

pub(crate) fn run(_args: &[String]) -> Verdict {
    let Some(root) = repo::root() else {
        eprintln!("xtask check-ignored-tests: FAILED - could not determine the repo root");
        return Verdict::Fail;
    };
    let baselined = match std::fs::read_to_string(root.join(BASELINE_FILE)) {
        Ok(text) => parse_baseline(&text),
        Err(why) => {
            eprintln!("xtask check-ignored-tests: FAILED - could not read {BASELINE_FILE}: {why}");
            return Verdict::Fail;
        }
    };
    let census = match repo::all_files() {
        Ok(census) => census,
        Err(why) => {
            eprintln!("xtask check-ignored-tests: FAILED - {}", why.describe());
            return Verdict::Fail;
        }
    };

    let mut live = BTreeSet::new();
    let mut unresolved = Vec::new();
    let scope: repo::Scope = in_scope;
    // Anchored on the binary root, as `check-expect-thresholds` is: a scope that stopped matching
    // it would be judging nothing.
    let inspected = match census.inspect(&["xtask/src/main.rs"], scope, |rel, bytes| {
        let found = ignored_in(rel, &String::from_utf8_lossy(bytes));
        live.extend(found.ignored);
        unresolved.extend(found.unresolved);
    }) {
        Ok(inspected) => inspected,
        Err(why) => {
            eprintln!("xtask check-ignored-tests: FAILED - {}", why.describe());
            return Verdict::Fail;
        }
    };

    let added = new_ignores(&live, &baselined);
    let stale: Vec<&String> = baselined.difference(&live).collect();
    if added.is_empty() && stale.is_empty() && unresolved.is_empty() {
        println!(
            "xtask check-ignored-tests: ok - {} ignored test(s), each listed in {BASELINE_FILE}; {}",
            live.len(),
            inspected.verdict()
        );
        return Verdict::Pass;
    }
    for one in &added {
        eprintln!("xtask check-ignored-tests: FAILED - `{one}` is #[ignore]d and not listed in {BASELINE_FILE}");
    }
    for one in &stale {
        eprintln!("xtask check-ignored-tests: FAILED - {BASELINE_FILE} lists `{one}`, which is not an #[ignore]d test");
    }
    for one in &unresolved {
        eprintln!("xtask check-ignored-tests: FAILED - {one}");
    }
    eprintln!();
    eprintln!("An #[ignore] takes a test out of the default suite. List a deliberate one in {BASELINE_FILE}");
    eprintln!("as `<path>::<fn_name>`, or drop the attribute; delete the line of an ignore that is gone.");
    Verdict::Fail
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{ignored_in, new_ignores, parse_baseline};

    const REL: &str = "crates/x/tests/t.rs";

    /// The live set of one fixture file.
    fn live(text: &str) -> BTreeSet<String> {
        let found = ignored_in(REL, text);
        assert_eq!(found.unresolved, Vec::<String>::new());
        found.ignored.into_iter().collect()
    }

    #[test]
    fn a_new_ignore_not_in_the_baseline_is_refused() {
        let live = live("#[test]\n#[ignore = \"needs a tier\"]\nfn needs_a_deployment() {}\n");
        let baselined = BTreeSet::new();
        assert_eq!(
            new_ignores(&live, &baselined),
            vec!["crates/x/tests/t.rs::needs_a_deployment"],
            "an ignore the baseline does not list is refused"
        );
    }

    #[test]
    fn a_baselined_ignore_passes() {
        let live = live("#[ignore]\n#[test]\nfn already_ignored() {}\n");
        let baselined = parse_baseline("# header\n\ncrates/x/tests/t.rs::already_ignored\n");
        assert_eq!(new_ignores(&live, &baselined), Vec::<&String>::new());
    }

    #[test]
    fn a_running_test_and_a_comment_naming_ignore_are_not_ignored() {
        let live = live("/// Not `#[ignore]`d.\n#[test]\nfn runs() {}\n// #[ignore]\n#[test]\nfn also_runs() {}\n");
        assert_eq!(live, BTreeSet::new());
    }

    #[test]
    fn a_wrapped_ignore_over_a_wrapped_test_attribute_is_found() {
        let live = live(concat!(
            "#[ignore = \"needs `just dev-up`; run that task \\\n",
            "            instead\"]\n",
            "#[tokio::test(\n",
            "    flavor = \"multi_thread\"\n",
            ")]\n",
            "async fn the_provisioned_surface() {}\n",
        ));
        assert_eq!(
            live,
            BTreeSet::from([String::from("crates/x/tests/t.rs::the_provisioned_surface")])
        );
    }
}
