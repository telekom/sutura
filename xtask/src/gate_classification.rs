//! The `hygiene` sweep's classification, held against the page that argues from it.
//!
//! `.github/workflows/docs.yml` skips the `hygiene` build for a diff of `docs/*.md` and
//! `mkdocs.yml` alone, and the argument for that skip is not the sweep's SIZE - it is that every
//! gate in it either reads nothing such a diff can touch, or reads prose and therefore defers a
//! verdict the page has to name. So the number in that page was never the claim; the two groups
//! were.
//!
//! **Why this is a gate over the page rather than a count in it.** The page said the sweep was
//! thirteen gates while it collected twenty-two, so nine were unaccounted for. A gated count
//! would have caught THAT, and would then have gone green the moment a tenth code-reading gate
//! landed - a number that looks maintained while the sentence it serves rots. What the argument
//! needs is that the groups cover the set, and that is what this checks. The classification
//! itself is not here: [`crate::Reads`] is a payload on `Kind::Hygiene`, so the compiler makes a
//! new gate declare a side and no check has to notice a missing one.
//!
//! **The limit, next to the claim.** This compares two NAMES lists - the page's against the
//! registry's. It does not read a gate's code, so a gate whose inputs grow to include a
//! `docs/*.md` file while its `Reads` still says `Code` passes here, exactly as `check-crap` did
//! for as long as the page named it in the wrong group. That case is caught by review or not at
//! all; what is mechanised is that no gate is missing from the argument and none is claimed twice.
//!
//! **Fails closed, and it has to.** A missing table, a missing separator row, a cell with no
//! backticked name and an empty group are all failures rather than a pass over silence: this gate
//! exists because a claim went unchecked, and a scan that finds nothing must not be the way it
//! goes unchecked again.

use std::collections::{BTreeMap, BTreeSet};

use crate::{Reads, Verdict, hygiene_gates, repo};

/// The page whose two tables ARE the classification the workflow's skip argues from.
const PAGE: &str = "docs/implementation-plan-identity-and-services.md";

/// The header row of the group a `docs/*.md`-only diff cannot reach.
const CODE_TABLE: &str = "| Gate | What it reads |";

/// The header row of the group it can.
const PROSE_TABLE: &str = "| Gate | On a prose-only pull request |";

/// Every gate named in the first column of the table under `header`.
///
/// The header is matched as a whole trimmed line, so the tables can stay indented inside the
/// numbered list they live in, and a table that appears twice is a failure rather than a choice
/// between two candidates.
fn table_gates(text: &str, header: &str) -> Result<BTreeSet<String>, String> {
    let mut lines = text.lines().map(str::trim).peekable();
    let mut seen_header = false;
    let mut gates = BTreeSet::new();

    while let Some(line) = lines.next() {
        if line != header {
            continue;
        }
        if seen_header {
            return Err(format!("{PAGE} has two `{header}` tables - which one is the claim?"));
        }
        seen_header = true;

        match lines.next() {
            // A table with no separator row is not a table, and markdown would render the
            // rows as one paragraph - so the page would read as prose while this gate read
            // an empty group.
            Some(separator) if separator.starts_with('|') && separator.contains("---") => {}
            _ => return Err(format!("{PAGE}: `{header}` is not followed by a separator row")),
        }

        while let Some(row) = lines.next_if(|l| l.starts_with('|')) {
            let cell = row.trim_start_matches('|').split('|').next().unwrap_or_default();
            let named = backticked(cell);
            if named.is_empty() {
                return Err(format!("{PAGE}: a row under `{header}` names no gate: {row}"));
            }
            for name in named {
                gates.insert(name);
            }
        }
    }

    if !seen_header {
        return Err(format!("{PAGE} has no `{header}` table - the classification is not stated"));
    }
    if gates.is_empty() {
        return Err(format!("{PAGE}: the `{header}` table has no rows"));
    }
    Ok(gates)
}

/// Every backtick-delimited span in `cell`. A row may name more than one gate - two gates whose
/// deferred verdict is the same sentence are one row on the page.
fn backticked(cell: &str) -> Vec<String> {
    cell.split('`')
        .skip(1)
        .step_by(2)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect()
}

/// Every way the page and the registry can disagree, as one message each.
fn disagreements(code: &BTreeSet<String>, prose: &BTreeSet<String>) -> Vec<String> {
    let mut problems = Vec::new();
    let registry: BTreeMap<&str, Reads> = hygiene_gates().collect();

    for both in code.intersection(prose) {
        problems.push(format!("{PAGE} lists `{both}` in both groups - it can only defer or not"));
    }

    for (name, reads) in &registry {
        let stated = match (code.contains(*name), prose.contains(*name)) {
            (true, _) => Some(Reads::Code),
            (_, true) => Some(Reads::Prose),
            (false, false) => None,
        };
        match stated {
            Some(stated) if stated == *reads => {}
            Some(stated) => problems.push(format!(
                "`{name}` declares Reads::{reads:?} and {PAGE} lists it as reading {}",
                stated.label()
            )),
            None => problems.push(format!(
                "the hygiene sweep collects `{name}` and {PAGE} classifies it in neither group - it reads {}",
                reads.label()
            )),
        }
    }

    for named in code.union(prose) {
        if !registry.contains_key(named.as_str()) {
            problems.push(format!(
                "{PAGE} classifies `{named}`, which the hygiene sweep does not collect"
            ));
        }
    }

    problems
}

pub(crate) fn run(_args: &[String]) -> Verdict {
    let Some(root) = repo::root() else {
        eprintln!("xtask check-gate-classification: could not locate the repo root");
        return Verdict::Fail;
    };

    let text = match std::fs::read_to_string(root.join(PAGE)) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("xtask check-gate-classification: could not read {PAGE}: {error}");
            eprintln!("  That page is where the prose-only skip in .github/workflows/docs.yml is argued.");
            return Verdict::Fail;
        }
    };

    let (code, prose) = match (table_gates(&text, CODE_TABLE), table_gates(&text, PROSE_TABLE)) {
        (Ok(code), Ok(prose)) => (code, prose),
        (Err(reason), _) | (_, Err(reason)) => {
            eprintln!("xtask check-gate-classification: FAILED - {reason}");
            return Verdict::Fail;
        }
    };

    let problems = disagreements(&code, &prose);
    if problems.is_empty() {
        println!(
            "xtask check-gate-classification: ok - {} gate(s) reading code, {} reading prose, as {PAGE} states",
            code.len(),
            prose.len()
        );
        return Verdict::Pass;
    }

    eprintln!("xtask check-gate-classification: FAILED");
    for problem in &problems {
        eprintln!("  {problem}");
    }
    eprintln!();
    eprintln!("A hygiene gate declares what it reads on `Kind::Hygiene` in its xtask/src/task_table/ area,");
    eprintln!("{PAGE} argues from that classification:");
    eprintln!("a diff of docs/*.md and mkdocs.yml skips the sweep, which is only safe while every");
    eprintln!("gate reading prose has a row saying which verdict that defers. Add the row, or move it.");
    Verdict::Fail
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{CODE_TABLE, PAGE, PROSE_TABLE, disagreements, table_gates};

    fn set(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|n| String::from(*n)).collect()
    }

    /// A page with both tables, indented the way the real one is inside its numbered list.
    const PAGE_TEXT: &str = "\
   **Reads code.**

   | Gate | What it reads |
   | --- | --- |
   | `check-pins` | flake.nix and pixi.toml |
   | `check-scope` | the justfile |

   **Reads prose.**

   | Gate | On a prose-only pull request |
   | --- | --- |
   | `text-hygiene`, `line-endings` | Deferred |

   Prose after the table.
";

    #[test]
    fn both_tables_are_read_through_their_indentation() {
        assert_eq!(
            table_gates(PAGE_TEXT, CODE_TABLE).expect("the code table parses"),
            set(&["check-pins", "check-scope"])
        );
        // Two gates in one cell are two gates: the page groups them by the sentence they share.
        assert_eq!(
            table_gates(PAGE_TEXT, PROSE_TABLE).expect("the prose table parses"),
            set(&["text-hygiene", "line-endings"])
        );
    }

    #[test]
    fn a_missing_table_is_a_failure_and_not_an_empty_group() {
        // The failure this whole gate exists to prevent, arrived at from the other direction: a
        // scan that finds nothing must not report that nothing is wrong.
        let error = table_gates("no tables here\n", CODE_TABLE).expect_err("a missing table fails");
        assert!(error.contains("has no"), "{error}");
    }

    #[test]
    fn a_table_with_no_separator_row_fails() {
        let text = format!("{CODE_TABLE}\n| `check-pins` | flake.nix |\n");
        let error = table_gates(&text, CODE_TABLE).expect_err("a separator-less table fails");
        assert!(error.contains("separator"), "{error}");
    }

    #[test]
    fn a_row_naming_no_gate_fails() {
        let text = format!("{CODE_TABLE}\n| --- | --- |\n| check-pins | no backticks |\n");
        let error = table_gates(&text, CODE_TABLE).expect_err("an unbackticked row fails");
        assert!(error.contains("names no gate"), "{error}");
    }

    #[test]
    fn a_table_stated_twice_fails() {
        let text =
            format!("{CODE_TABLE}\n| --- | --- |\n| `check-pins` | x |\n\n{CODE_TABLE}\n| --- | --- |\n| `check-scope` | y |\n");
        let error = table_gates(&text, CODE_TABLE).expect_err("two tables fail");
        assert!(error.contains("two"), "{error}");
    }

    #[test]
    fn an_unclassified_gate_is_reported_with_the_side_it_declares() {
        // Nothing classified at all: every gate the sweep collects has to be reported, which is
        // the omission a count could not see.
        let problems = disagreements(&BTreeSet::new(), &set(&["check-guidance"]));
        assert!(
            problems
                .iter()
                .any(|p| p.contains("`check-pins`") && p.contains("neither group")),
            "{problems:?}"
        );
    }

    #[test]
    fn the_wrong_side_is_reported_rather_than_ignored() {
        let problems = disagreements(&set(&["check-guidance"]), &BTreeSet::new());
        assert!(
            problems
                .iter()
                .any(|p| p.contains("`check-guidance` declares Reads::Prose") && p.contains("reading code")),
            "{problems:?}"
        );
    }

    #[test]
    fn a_gate_in_both_groups_fails() {
        let problems = disagreements(&set(&["check-pins"]), &set(&["check-pins"]));
        assert!(problems.iter().any(|p| p.contains("both groups")), "{problems:?}");
    }

    #[test]
    fn a_name_the_sweep_does_not_collect_fails() {
        let problems = disagreements(&set(&["check-nothing"]), &BTreeSet::new());
        assert!(
            problems
                .iter()
                .any(|p| p.contains("`check-nothing`") && p.contains("does not collect")),
            "{problems:?}"
        );
    }

    #[test]
    fn this_repos_own_page_classifies_every_hygiene_gate() {
        // The whole point, over the real tree rather than a fixture: the page's two tables and
        // the registry's two groups are the same two groups.
        let root = crate::repo::root().expect("the repo root is discoverable");
        let text = std::fs::read_to_string(root.join(PAGE)).expect("the page is readable");
        let code = table_gates(&text, CODE_TABLE).expect("the code table parses");
        let prose = table_gates(&text, PROSE_TABLE).expect("the prose table parses");
        let problems = disagreements(&code, &prose);
        assert!(problems.is_empty(), "{problems:?}");
    }
}
