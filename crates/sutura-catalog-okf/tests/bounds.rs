#![forbid(unsafe_code)]
//! The byte bound on OKF catalog intake, held against the PUBLIC API only.
//!
//! **Why an integration test and not a `#[cfg(test)]` unit test.** `cargo xtask test-causality`
//! proves red-before-green by restoring the production file(s) a diff touched to `origin/main` and
//! re-running the new tests against that restored build. This crate's unit tests live in
//! `src/tests.rs`, wired into the build by `mod tests;` in `src/lib.rs` - the exact line the byte
//! bound's diff also touches. Restoring `lib.rs` to base reverts that line, so the split-out file
//! never gets referenced by any `mod` statement in the restored build and silently drops out of
//! it - a compiling, passing, but EMPTY measurement. `telekom/sutura#657` is the same trap in the
//! inbound key-refresh path, and `crates/sutura-catalog-local/tests/bounds.rs` documents it for
//! that crate's own two bounds. A file under `tests/` has no such wire: cargo discovers and
//! compiles every file here on its own, so it stays part of the build whichever half of the diff a
//! causality pass restores.
//!
//! **Why the refusal is asserted by rendered message rather than matching a variant.**
//! `OkfCatalogError::TooLarge` does not exist on `origin/main` - matching it by name would refuse
//! to COMPILE there, not fail at runtime, which is not red-before-green evidence (a compile error
//! confounds "this is refused" with "this cannot even be asked"). The fixture is instead a
//! complete, valid, otherwise-unbounded catalog: on `origin/main`, where the bound does not exist,
//! the same directory loads successfully. `OkfCatalog::load` is called through
//! [`sutura_domain::pinned::SemanticCatalog`], its own driving port, so this measures exactly what
//! a real deployment's boot path calls.

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use sutura_catalog_okf::OkfCatalog;
    use sutura_domain::model::SourceName;
    use sutura_domain::pinned::{DefinitionVersion, SemanticCatalog as _};

    fn test_name() -> SourceName {
        SourceName::parse("test").expect("a test name is a name")
    }

    fn version() -> DefinitionVersion {
        DefinitionVersion::parse("test-1").expect("a test version is a version")
    }

    /// A scratch directory of this test's own, cleared on the way in - matching the pattern this
    /// crate's own unit tests already use (`tempfile` is not a dependency of this workspace).
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sutura-catalog-okf-bounds-{name}-{}", std::process::id()));
        drop(std::fs::remove_dir_all(&dir));
        std::fs::create_dir_all(&dir).expect("a scratch directory is creatable");
        dir
    }

    /// A complete, valid, uniquely-named Table Schema descriptor - a real catalog element on its
    /// own, so a directory of these is a real catalog rather than a fixture that only looks like
    /// one to the one check under test.
    fn descriptor(unique: &str) -> String {
        format!("description: Bounds fixture model {unique}, one row per unit.\nfields:\n  - name: id\n")
    }

    /// A complete, valid descriptor whose TOTAL byte length on disk is exactly `total_len` -
    /// padded with a YAML comment at the top of the document. `deny_unknown_fields` refuses a key
    /// this adapter does not read, and the columns are parsed through validated name newtypes, so
    /// the only safe padding is a comment - which the YAML parser ignores and never surfaces as a
    /// field, so no per-field check fires for an unrelated reason.
    fn padded_descriptor(unique: &str, total_len: usize) -> String {
        let body = descriptor(unique);
        let prefix = "# ";
        let suffix = "\n";
        let padding_len = total_len
            .checked_sub(body.len() + prefix.len() + suffix.len())
            .expect("total_len must be large enough to hold the descriptor and its comment around the padding");
        format!("{prefix}{}{suffix}{body}", "a".repeat(padding_len))
    }

    /// The aggregate is a SUM, not a per-file limit, and the bound is a startup refusal naming
    /// bytes - the same two properties `sutura-catalog-local`'s own bound test pins for that
    /// crate. Two documents, each well under the cap on its own, whose total is over it by one:
    /// a mutation that checked each file's own size against the cap (rather than a running total)
    /// would never refuse this directory - neither file is individually oversized - so this is the
    /// cell that catches exactly that weakening, and it costs `MAX_CATALOG_BYTES + 1` bytes on disk
    /// rather than a 20 MiB file.
    #[test]
    fn a_directory_whose_documents_sum_past_the_byte_bound_is_refused() {
        const CAP: usize = 16 * 1024 * 1024;
        // Each file is almost but not quite the whole cap; their sum is CAP + 1, one byte over. The
        // `a` descriptor at the start is the tiny, valid one; `b_oversized` (no hyphen: the file
        // stem is the table name, and a hyphen is not a name) carries the bulk.
        const A_LEN: usize = 1024;
        const B_LEN: usize = CAP + 1 - A_LEN;
        let root = scratch("too-large-aggregate");
        std::fs::write(root.join("a.yaml"), padded_descriptor("a", A_LEN)).expect("a descriptor is writable");
        std::fs::write(root.join("b_oversized.yaml"), padded_descriptor("b", B_LEN)).expect("a descriptor is writable");

        let catalog = OkfCatalog::new(test_name(), root.clone(), version());
        let err = catalog.load().expect_err(
            "on origin/main these two descriptors load in full regardless of size - refusing \
             the directory is exactly the behaviour this test exists to require",
        );
        let message = err.to_string();
        assert!(
            message.contains(&CAP.to_string()),
            "the refusal should name the byte limit itself ({CAP}), not just the word 'bytes': {message}"
        );
        assert!(
            message.contains("b_oversized"),
            "the refusal should name the document whose read stopped it, not only the catalog root: {message}"
        );

        drop(std::fs::remove_dir_all(&root));
    }
}
