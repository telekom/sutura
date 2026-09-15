//! The wave-one example's files, held against what `served/e2e.rs`'s `#[ignore]`d cell asserts
//! (the JVM owning the Keycloak tier, so it cannot run here): the README's task list, the
//! question's anchors, the refusal's shape, the posture every prose page names, and a
//! paragraph-level sweep that round 1's leg-2 overclaim ("`BigQuery` executes AS the asking
//! subject", "impersonation-at-source" - `github.com/telekom/sutura` PR #762 round 1) does not
//! read as fact again: each phrase appears TODAY only inside a "NOT built" paragraph (README:19,98,
//! `base.yaml`:6, showcase:36, all naming `docs/where-identity-is-proven.md`'s `unrun` row), so a
//! paragraph (text between blank lines) carrying one WITHOUT a negation is the overclaim.
//!
//! Split out of `tests/documented.rs` (`xtask max-lines`'s 1000-line cap left no room there) -
//! this file duplicates `page`/`repo_root` rather than share them across two test binaries, which
//! is the same choice every other file under `tests/` already makes.

// `cfg(test)` for the reason `tests/documented.rs` gives: clippy honours `allow-expect-in-tests`
// only inside a `#[cfg(test)]` item, and without it the JSON `.expect` below is a lint error.
#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    fn repo_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    fn page(rel: &str) -> String {
        let path = repo_root().join(rel);
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{} is readable: {e}", path.display()))
    }

    /// Phrases round 1 shipped as fact; heuristic, not a parser - a negation anywhere else in a
    /// large paragraph would satisfy it too.
    const OVERCLAIM_PHRASES: &[&str] = &["impersonation-at-source", "as the asking subject", "executes as the caller"];
    const NEGATIONS: &[&str] = &["not", "never", "n't"];
    const PROSE_FILES: &[&str] = &[
        "docs/showcase-datahub-bigquery.md",
        "examples/wave-one/README.md",
        "examples/wave-one/base.yaml",
    ];

    #[test]
    fn the_wave_one_example_documents_what_the_cell_pins() {
        for (rel, needle) in [
            ("examples/wave-one/README.md", "just e2e-datahub-bigquery"),
            ("examples/wave-one/question.yaml", "metric: revenue"),
            ("examples/wave-one/question.yaml", "start: 2026-06-01"),
            ("examples/wave-one/question.yaml", "end: 2026-07-01"),
        ] {
            let text = page(rel);
            assert!(
                text.contains(needle),
                "{rel} no longer carries {needle:?} - an edit to one side of the wave-one pairing \
                 without the other is caught here, the raw-sql README cell's split"
            );
        }

        // The refusal's SHAPE by FIELD (not the mere presence of a `code`/`detail` key) - the same
        // two `served/e2e.rs` pins.
        let refusal: serde_json::Value =
            serde_json::from_str(&page("examples/wave-one/refusal.json")).expect("refusal.json is valid JSON");
        assert_eq!(
            refusal["reason"]["status"],
            serde_json::json!(404),
            "refusal.json's status drifted"
        );
        assert_eq!(refusal["reason"]["code"], "metric_unknown", "refusal.json's code drifted");

        // The posture the served cell runs under (`e2e.rs:440`'s `executed_as`) - named in every
        // page describing the leg, so a silent swap to another posture is caught here.
        for rel in PROSE_FILES {
            assert!(
                page(rel).to_lowercase().contains("shared-service-user"),
                "{rel} no longer names the shared-service-user posture"
            );
        }

        // The negative sweep: a leg-2 overclaim, reintroduced into any of the three prose files,
        // must FAIL here rather than survive on review alone.
        for rel in PROSE_FILES {
            let text = page(rel).to_lowercase();
            for block in text.split("\n\n") {
                for phrase in OVERCLAIM_PHRASES {
                    if block.contains(phrase) {
                        assert!(
                            NEGATIONS.iter().any(|negation| block.contains(negation)),
                            "{rel} states {phrase:?} in a paragraph with no negation - a leg-2 \
                             overclaim (round 1's finding 2) reads as fact again"
                        );
                    }
                }
            }
        }
    }
}
