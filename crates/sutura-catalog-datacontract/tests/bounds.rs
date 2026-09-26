#![forbid(unsafe_code)]
//! The byte bound on data-contract catalog intake, held against the PUBLIC API only.
//!
//! **Why an integration test and not a `#[cfg(test)]` unit test.** `cargo xtask test-causality`
//! proves red-before-green by restoring the production file(s) a diff touched to `origin/main` and
//! re-running the new tests against that restored build. This crate's unit tests live in
//! `src/tests.rs`, wired into the build by `mod tests;` in `src/lib.rs` - the exact line the byte
//! bound's diff also touches. Restoring `lib.rs` to base reverts that line, so the split-out file
//! never gets referenced by any `mod` statement in the restored build and silently drops out of
//! it - a compiling, passing, but EMPTY measurement. This is the same trap `sutura-catalog-okf`'s
//! own `tests/bounds.rs` documents for that crate's two bounds. A file under `tests/` has no such
//! wire: cargo discovers and compiles every file here on its own, so it stays part of the build
//! whichever half of a diff a causality pass restores.
//!
//! **Why the refusal is asserted by rendered message rather than matching a variant.**
//! `DataContractError::TooLarge` does not exist on `origin/main` - matching it by name would refuse
//! to COMPILE there, not fail at runtime, which is not red-before-green evidence. The fixture is
//! instead a complete, valid, otherwise-unbounded catalog: on `origin/main`, where the bound does not
//! exist, the same directory loads successfully. `DataContractCatalog::load` is called through
//! [`sutura_domain::pinned::SemanticCatalog`], its own driving port, so this measures exactly what
//! a real deployment's boot path calls.

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use sutura_catalog_datacontract::DataContractCatalog;
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
        let dir = std::env::temp_dir().join(format!("sutura-catalog-datacontract-bounds-{name}-{}", std::process::id()));
        drop(std::fs::remove_dir_all(&dir));
        std::fs::create_dir_all(&dir).expect("a scratch directory is creatable");
        dir
    }

    /// A complete, valid, uniquely-named v3.1.0 contract - a real catalog element on its own, so a
    /// directory of these is a real catalog rather than a fixture that only looks like one to the
    /// one check under test.
    fn contract(unique: &str) -> String {
        format!(
            "version: 1.0.0\nkind: DataContract\nid: {unique}\nstatus: active\napiVersion: v3.1.0\nschema:\n  - name: {unique}\n    properties:\n      - name: id\n        primaryKey: true\n"
        )
    }

    /// A complete, valid contract whose TOTAL byte length on disk is exactly `total_len` - padded
    /// with a YAML comment at the top of the document. `deny_unknown_fields` refuses a key this
    /// adapter does not read, and the columns are parsed through validated name newtypes, so the
    /// only safe padding is a comment - which the YAML parser ignores and never surfaces as a field.
    fn padded_contract(unique: &str, total_len: usize) -> String {
        let body = contract(unique);
        let prefix = "# ";
        let suffix = "\n";
        let padding_len = total_len
            .checked_sub(body.len() + prefix.len() + suffix.len())
            .expect("total_len must be large enough to hold the contract and its comment around the padding");
        format!("{prefix}{}{suffix}{body}", "a".repeat(padding_len))
    }

    /// The aggregate is a SUM, not a per-file limit, and the bound is a startup refusal naming
    /// bytes - the same property `sutura-catalog-okf`'s own bound test pins for that crate. Two
    /// documents, each well under the cap on its own, whose total is over it by one: a mutation
    /// that checked each file's own size against the cap (rather than a running total) would never
    /// refuse this directory - neither file is individually oversized - so this is the cell that
    /// catches exactly that weakening.
    #[test]
    fn a_directory_whose_documents_sum_past_the_byte_bound_is_refused() {
        const CAP: usize = 16 * 1024 * 1024;
        const A_LEN: usize = 1024;
        const B_LEN: usize = CAP + 1 - A_LEN;
        let root = scratch("too-large-aggregate");
        std::fs::write(root.join("a.yaml"), padded_contract("a", A_LEN)).expect("a contract is writable");
        std::fs::write(root.join("b.yaml"), padded_contract("b", B_LEN)).expect("a contract is writable");

        let catalog = DataContractCatalog::new(test_name(), root.clone(), version());
        let err = catalog.load().expect_err(
            "on origin/main these two contracts load in full regardless of size - refusing \
             the directory is exactly the behaviour this test exists to require",
        );
        let message = err.to_string();
        assert!(
            message.contains(&CAP.to_string()),
            "the refusal should name the byte limit itself ({CAP}), not just the word 'bytes': {message}"
        );
        assert!(
            message.contains("b.yaml"),
            "the refusal should name the document whose read stopped it, not only the catalog root: {message}"
        );

        drop(std::fs::remove_dir_all(&root));
    }
}
