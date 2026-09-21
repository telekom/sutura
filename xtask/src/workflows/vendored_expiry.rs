//! Does every copied cargo package declare WHAT ENDS IT? The hermetic half of the vendor-expiry
//! mechanism, and the one rule about a `vendor/` child that is not about attribution.
//!
//! # THE HOLE THIS CLOSES
//!
//! `VENDOR.md`'s actionlint row states it in its own words - *"Temporary until the locked
//! actionlint release contains this commit; nothing detects that release automatically"* - and
//! `devco/arrow-majors-allow`'s duckdb row is the same shape: the fix merged upstream, the newest
//! published release predates it, and the entry waits on a release nobody is watching. For a
//! vendored crate that is structural rather than an oversight: it is a PATH dependency, so
//! `cargo update` cannot see it and neither can a dependency bot, and the signal that normally
//! arrives never does.
//!
//! # WHY IT IS HERE AND NOT IN `check-vendor-expiry`
//!
//! Two rules, two venues, and the split is the point. Comparing against the registry needs egress,
//! so `cargo xtask check-vendor-expiry` is `Kind::Standalone` and `just update` runs it - a gate
//! that reddens whichever branch is open at the moment upstream publishes is worse than the rot it
//! closes. *This* rule reads two local inputs and is therefore hermetic, which is what lets it ride
//! in `check-workflows` beside `super::scorecard`'s rules ONE to THREE: the same child listing,
//! the same fail-closed seam, and the thing that stops the NEXT vendored tree arriving with no
//! expiry declaration at all.
//!
//! # THE SUBJECT IS NARROWER THAN ATTRIBUTION'S, AND THE SENTENCE IS NARROWER TO MATCH
//!
//! A child holding a `Cargo.toml`. A copied loose file or a tree that is not a cargo package has
//! no release to compare, so demanding a row for it would put a crate name and a version in a
//! position where neither exists - `vendor/upstream.rs` in `super::scorecard`'s own test is
//! exactly that shape. **What that costs, rather than hidden:** a non-crate copy is attributed and
//! recorded by rules ONE to THREE and watched for expiry by nothing. Closing it needs a second
//! source kind - a GitHub release tag, which is also what `nix/actionlint.nix` would need - and
//! that is not built. [`crate::vendor_expiry::Row`] is the slot it would be added to.

use std::path::Path;

use crate::vendor_expiry::{DECLARATION, Declared, rows};

/// Every way `devco/vendor-expiry` and the copied trees under `vendor/` can disagree.
///
/// `children` is the immediate-child listing `super::scorecard` already has in hand, so this
/// reads the filesystem only to ask which of them is a cargo package. A `Vec` rather than a
/// verdict, for that module's reason: one gate prints one verdict.
pub(super) fn problems(root: &Path, vendor: &str, children: &[String]) -> Vec<String> {
    let mut problems = Vec::new();
    let crated: Vec<&String> = children
        .iter()
        .filter(|child| root.join(vendor).join(child).join("Cargo.toml").is_file())
        .collect();

    let declaration = match std::fs::read_to_string(root.join(DECLARATION)) {
        Ok(declaration) => declaration,
        // NO VENDORED CARGO PACKAGE is a legitimate state, and only `NotFound` over an empty
        // subject set says so - the same seam the child listing itself draws between *no vendored
        // tree* and *an input I could not read*. Every other error is a fault, and so is an absent
        // declaration over a tree that HAS a copied package.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && crated.is_empty() => {
            return problems;
        }
        Err(error) => {
            problems.push(format!(
                "{DECLARATION} could not be read: {error} - so whether a copied cargo package's reason has expired is unread rather than clean"
            ));
            return problems;
        }
    };

    let Declared { rows, malformed } = rows(&declaration);
    problems.extend(malformed);

    // ONE: every copied cargo package has a row.
    for child in &crated {
        if !rows.iter().any(|row| &&row.child == child) {
            problems.push(format!(
                "{vendor}/{child} is a copied cargo package and {DECLARATION} declares no row for it, so nothing can tell whether the reason it was copied has expired - add `{child} <crate> <version> <ISO date> <what a newer release means>` there"
            ));
        }
    }

    // TWO, and a one-way rule would have left this green: a row naming a child that is gone
    // permits nothing while keeping its dated paragraph, which is the reasoning a reviewer reads
    // to decide the exception is still earned. The same direction `check-arrow` and rules ONE and
    // TWO next door already take on their own annotated lists.
    for row in &rows {
        if !children.iter().any(|child| child == &row.child) {
            let (line, child) = (row.line, &row.child);
            problems.push(format!(
                "{DECLARATION}:{line} names {vendor}/{child} and it does not exist - a row permitting nothing keeps its dated paragraph, which is the reasoning a reviewer reads to decide the exception is still earned"
            ));
        }
    }

    problems
}

#[cfg(test)]
mod tests {
    use super::problems;
    use crate::vendor_expiry::DECLARATION;

    /// A scratch root with one copied tree under `vendor/`, keyed on the process id and removed
    /// first - a pid is reusable, and a tree left by a run that panicked would otherwise seed
    /// files no assertion here is about.
    fn scratch(tag: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!("sutura-vendored-expiry-{tag}-{}", std::process::id()));
        drop(std::fs::remove_dir_all(&root));
        std::fs::create_dir_all(root.join("vendor/tree")).expect("the copied tree");
        std::fs::create_dir_all(root.join("devco")).expect("the devco directory");
        root
    }

    fn write(path: &std::path::Path, text: &str) {
        std::fs::write(path, text).expect("the scratch file");
    }

    /// The row shape the declaration's own header documents, for the one tree `scratch` creates.
    const DECLARED: &str = "tree upstream-crate 1.2.3 2026-09-21 what a newer release means here\n";

    #[test]
    fn a_tree_that_is_not_a_cargo_package_needs_no_row_and_no_declaration() {
        // WHY THE SUBJECT IS NARROWED, asserted rather than argued: `vendor/tree` here holds no
        // `Cargo.toml`, so an absent declaration is a legitimate state and not a fault. This is
        // also what keeps `scorecard`'s own fixtures - a bare directory and a loose `.rs` - silent.
        let root = scratch("not-a-package");
        assert_eq!(problems(&root, "vendor", &[String::from("tree")]), Vec::<String>::new());
        write(&root.join("vendor/upstream.rs"), "fn upstream() {}\n");
        assert_eq!(
            problems(&root, "vendor", &[String::from("tree"), String::from("upstream.rs")]),
            Vec::<String>::new()
        );
        drop(std::fs::remove_dir_all(&root));
    }

    #[test]
    fn a_copied_cargo_package_with_no_row_is_refused() {
        let root = scratch("no-row");
        write(&root.join("vendor/tree/Cargo.toml"), "[package]\nname = \"tree\"\n");
        write(&root.join(DECLARATION), "# a header and no row yet\n");
        let found = problems(&root, "vendor", &[String::from("tree")]);
        assert!(
            found
                .iter()
                .any(|p| p.contains("vendor/tree is a copied cargo package") && p.contains("declares no row")),
            "{found:?}"
        );

        write(&root.join(DECLARATION), DECLARED);
        assert_eq!(problems(&root, "vendor", &[String::from("tree")]), Vec::<String>::new());
        drop(std::fs::remove_dir_all(&root));
    }

    #[test]
    fn a_row_naming_a_child_that_is_gone_is_refused_too() {
        // THE OTHER DIRECTION. One-way, a row whose tree was deleted keeps its paragraph
        // permitting nothing, and the report calling the list clean is what rots.
        let root = scratch("stale-row");
        write(&root.join("vendor/tree/Cargo.toml"), "[package]\nname = \"tree\"\n");
        write(
            &root.join(DECLARATION),
            &format!("{DECLARED}retired other-crate 0.1.0 2026-09-21 why\n"),
        );
        let found = problems(&root, "vendor", &[String::from("tree")]);
        assert!(
            found.iter().any(|p| p.contains("vendor-expiry:2 names vendor/retired")),
            "{found:?}"
        );
        drop(std::fs::remove_dir_all(&root));
    }

    #[test]
    fn a_malformed_row_reaches_this_report_rather_than_counting_as_a_declaration() {
        let root = scratch("malformed");
        write(&root.join("vendor/tree/Cargo.toml"), "[package]\nname = \"tree\"\n");
        write(&root.join(DECLARATION), &format!("{DECLARED}tree only-three 1.0.0\n"));
        let found = problems(&root, "vendor", &[String::from("tree")]);
        assert!(
            found.iter().any(|p| p.contains("not the five the header declares")),
            "{found:?}"
        );
        drop(std::fs::remove_dir_all(&root));
    }

    #[test]
    fn an_absent_declaration_over_a_copied_package_is_a_fault_rather_than_a_smaller_scan() {
        // FAIL CLOSED, the distinction only a few of this repo's gates draw: a reader that finds
        // nothing must not be how the rule goes unchecked again.
        let root = scratch("absent");
        write(&root.join("vendor/tree/Cargo.toml"), "[package]\nname = \"tree\"\n");
        let found = problems(&root, "vendor", &[String::from("tree")]);
        assert!(found.iter().any(|p| p.contains("could not be read")), "{found:?}");
        drop(std::fs::remove_dir_all(&root));
    }
}
