#![forbid(unsafe_code)]
//! The two startup bounds on local catalog intake, held against the PUBLIC API only.
//!
//! **Why an integration test and not a `#[cfg(test)]` unit test.** `cargo xtask test-causality`
//! proves red-before-green by restoring the production file(s) a diff touched to `origin/main` and
//! re-running the new tests against that restored build. A unit-test module that lives in its own
//! file (`src/tests.rs`, split out of `src/lib.rs` to stay under `cargo xtask max-lines`) is wired
//! in by one line - `mod tests;` - that only exists in `src/lib.rs` itself. Restoring `lib.rs` to
//! base reverts that line along with everything else, so the split-out file is never referenced by
//! any `mod` statement in the restored build and silently drops out of it - a compiling, passing,
//! but EMPTY measurement that reads as "green against base" for the wrong reason. `telekom/sutura#657`
//! is the same trap in the inbound key-refresh path. A file under `tests/` has no such wire: cargo
//! discovers and compiles every file here on its own, with no `mod` declaration anywhere in `src/`
//! for a restore to break - so it stays part of the build whichever half of the diff a causality
//! pass restores.
//!
//! **Why every assertion below reads a rendered message rather than matching a refusal variant.**
//! `LocalCatalogError::TooManyDocuments` and `::TooLarge` do not exist on `origin/main` - matching
//! either by name would refuse to COMPILE there, not fail at runtime, which is not red-before-green
//! evidence (a compile error confounds "this is refused" with "this cannot even be asked"). Every
//! fixture below is instead a complete, valid, otherwise-unbounded catalog: on `origin/main`, where
//! neither bound exists, the same directory loads successfully. `LocalCatalog::load` is called
//! through [`sutura_domain::pinned::SemanticCatalog`], its own driving port, so this measures
//! exactly what a real deployment's boot path calls - not a private helper this crate could rename
//! freely.

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use sutura_catalog_local::LocalCatalog;
    use sutura_domain::model::SourceName;
    use sutura_domain::pinned::{DefinitionVersion, SemanticCatalog as _};

    fn test_name() -> SourceName {
        SourceName::parse("test").expect("a test name is a name")
    }

    fn version() -> DefinitionVersion {
        DefinitionVersion::parse("test-1").expect("a test version is a version")
    }

    /// A scratch directory of this test's own, cleared on the way in - `tempfile` is not a
    /// dependency of this workspace, matching the pattern this crate's own unit tests already use.
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sutura-catalog-local-bounds-{name}-{}", std::process::id()));
        drop(std::fs::remove_dir_all(&dir));
        std::fs::create_dir_all(&dir).expect("a scratch directory is creatable");
        dir
    }

    /// A complete, valid, uniquely-named model document - a real catalog element on its own, so a
    /// directory of many of these is a real (if pointless) catalog rather than a fixture that only
    /// looks like one to the one check under test.
    fn model_document(unique: &str) -> String {
        format!(
            "---\nkind: model\nname: bound_{unique}\nsource: local\ntable: bound_{unique}\ncolumns: [id]\n---\nA fixture model, used only to make this directory large.\n"
        )
    }

    /// A complete, valid model document whose TOTAL byte length is exactly `total_len` - padded
    /// with a YAML comment inside its frontmatter, which contributes to the file's size on disk
    /// without being a value any parser reads (so no per-field length cap - a description, a note
    /// body - can fire for an unrelated reason; see the module doc's second "Why").
    fn padded_model_document(unique: &str, total_len: usize) -> String {
        let prefix = format!("---\nkind: model\nname: bound_{unique}\nsource: local\ntable: bound_{unique}\ncolumns: [id]\n# ");
        let suffix = "\n---\nA fixture model, used only to make this directory large.\n";
        let padding_len = total_len
            .checked_sub(prefix.len() + suffix.len())
            .expect("total_len must be large enough to hold the frontmatter and body around the padding");
        format!("{prefix}{}{suffix}", "a".repeat(padding_len))
    }

    #[test]
    fn a_directory_with_too_many_documents_is_refused_by_name() {
        // Comfortably over the production cap (1,000 as of this writing) - the assertion is on the
        // REFUSAL, not the exact count, so raising the constant later does not need this number
        // raised to match, only to stay above it.
        const DOCUMENT_COUNT: usize = 1_100;
        let root = scratch("too-many-documents");
        for n in 0..DOCUMENT_COUNT {
            let unique = format!("{n:05}");
            std::fs::write(root.join(format!("model-{unique}.md")), model_document(&unique)).expect("a document is writable");
        }

        let catalog = LocalCatalog::new(test_name(), root.clone(), version());
        let err = catalog.load().expect_err(
            "on origin/main this directory of independent, valid models loads successfully - \
             refusing it is exactly the behaviour this test exists to require",
        );
        let message = err.to_string();
        assert!(
            message.contains("documents"),
            "the refusal should name what there were too many of: {message}"
        );

        drop(std::fs::remove_dir_all(&root));
    }

    #[test]
    fn a_directory_whose_documents_sum_past_the_byte_bound_is_refused_by_name() {
        // Two valid models. The second carries a YAML comment inside its frontmatter, padding the
        // FILE on disk past the production aggregate cap (16 MiB as of this writing) without
        // padding anything a parser reads as a field - the comment is not a value, so no per-field
        // length cap (a description, a note body) fires first for an unrelated reason, and both
        // documents remain exactly as parseable as the small one in the first test. Two documents
        // is far under the document-count bound above, so that check does not fire first either.
        const OVERSIZED_LEN: usize = 20 * 1024 * 1024;
        let root = scratch("too-large-aggregate");
        std::fs::write(root.join("model-a.md"), model_document("a")).expect("a document is writable");
        std::fs::write(root.join("model-b-oversized.md"), padded_model_document("b", OVERSIZED_LEN))
            .expect("a document is writable");

        let catalog = LocalCatalog::new(test_name(), root.clone(), version());
        let err = catalog.load().expect_err(
            "on origin/main these two documents load in full regardless of size - refusing the \
             directory is exactly the behaviour this test exists to require",
        );
        let message = err.to_string();
        assert!(
            message.contains("bytes"),
            "the refusal should name what was measured in bytes: {message}"
        );

        drop(std::fs::remove_dir_all(&root));
    }

    /// The document-count bound at exactly the cap. Neither red-on-base cell above names the
    /// boundary VALUE - both use a count and a size comfortably over it - so a weakened check that
    /// still refuses somewhere past the true cap (`>` mutated to `>=`, which moves the effective
    /// cap to one less) had nowhere to be caught. This cell is that boundary: exactly
    /// `MAX_CATALOG_DOCUMENTS` (1,000 as of this writing) must load, not be refused for size. Green
    /// on base is fine here - `origin/main` has no such bound at all - the file already carries the
    /// two red-on-base cells above, and the causality gate measures the file, not each cell.
    #[test]
    fn a_directory_at_exactly_the_document_cap_loads() {
        const DOCUMENT_COUNT: usize = 1_000;
        let root = scratch("at-the-document-cap");
        for n in 0..DOCUMENT_COUNT {
            let unique = format!("{n:05}");
            std::fs::write(root.join(format!("model-{unique}.md")), model_document(&unique)).expect("a document is writable");
        }

        let catalog = LocalCatalog::new(test_name(), root.clone(), version());
        catalog
            .load()
            .expect("exactly the cap must load, not be refused for a count that stays at it");

        drop(std::fs::remove_dir_all(&root));
    }

    /// The aggregate-byte bound at exactly the cap - the twin of the document-count boundary above,
    /// same reasoning: a `>` mutated to `>=` moves the effective cap down by one byte, and nothing
    /// else in this file would catch it.
    #[test]
    fn two_documents_summing_to_exactly_the_aggregate_cap_load() {
        // Mirrors the production constant rather than importing it (private to `src/lib.rs`, and
        // this file compiles against the public API only) - the second document is padded to
        // whatever length makes the total hit this number exactly, so the two cannot drift apart.
        const AGGREGATE_CAP: u64 = 16 * 1024 * 1024;
        let root = scratch("at-the-aggregate-cap");
        let first = model_document("a");
        let first_len = u64::try_from(first.len()).expect("a fixture document's length fits a u64");
        std::fs::write(root.join("model-a.md"), &first).expect("a document is writable");
        let second_len = usize::try_from(AGGREGATE_CAP - first_len).expect("the remainder fits a usize");
        let second = padded_model_document("b", second_len);
        assert_eq!(
            second.len(),
            second_len,
            "the padding helper must hit the requested length exactly"
        );
        std::fs::write(root.join("model-b.md"), &second).expect("a document is writable");

        let catalog = LocalCatalog::new(test_name(), root.clone(), version());
        catalog
            .load()
            .expect("exactly the aggregate cap must load, not be refused for a total that stays at it");

        drop(std::fs::remove_dir_all(&root));
    }

    /// The aggregate is a SUM, not a per-file limit: two documents, each well UNDER the aggregate
    /// cap on its own, whose total is over it. A mutation that checked each file's own size against
    /// the cap (rather than a running total) would never refuse this directory - neither file is
    /// individually oversized - so this is the cell that catches that specific weakening, which the
    /// two red-on-base cells above do not (both put all the size in one file).
    #[test]
    fn two_documents_each_under_the_cap_whose_sum_is_over_it_are_refused_by_name() {
        const EACH_LEN: usize = 9 * 1024 * 1024;
        let root = scratch("aggregate-not-per-file");
        std::fs::write(root.join("model-a.md"), padded_model_document("a", EACH_LEN)).expect("a document is writable");
        std::fs::write(root.join("model-b.md"), padded_model_document("b", EACH_LEN)).expect("a document is writable");

        let catalog = LocalCatalog::new(test_name(), root.clone(), version());
        let err = catalog.load().expect_err(
            "on origin/main these two documents load in full regardless of size - refusing the \
             directory is exactly the behaviour this test exists to require",
        );
        let message = err.to_string();
        assert!(
            message.contains("bytes"),
            "the refusal should name what was measured in bytes: {message}"
        );

        drop(std::fs::remove_dir_all(&root));
    }
}
