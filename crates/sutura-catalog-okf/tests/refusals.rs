#![forbid(unsafe_code)]
//! Every reachable `OkfCatalogError` refusal `src/tests.rs` does not provoke, each driven through
//! the public [`SemanticCatalog::load`] port over a scratch directory of real Table Schema
//! descriptors and asserted by variant, the contract a caller matches on.
//!
//! Not provoked: `Unnamed` (a file name that is not UTF-8, which not every filesystem can hold),
//! `UncheckableKnowledge` (this adapter supplies `KnowledgeInput::none()`) and `Digest`. `Io` is
//! provoked on its read arm only; its walk arm is a `read_dir` failure.
//!
//! Wrapped in `#[cfg(test)] mod tests` so `allow-expect-in-tests` reaches the helpers, the shape
//! `tests/bounds.rs` uses.

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use sutura_catalog_okf::{InvalidPrimaryKeyShape, OkfCatalog, OkfCatalogError};
    use sutura_domain::model::SourceName;
    use sutura_domain::pinned::{DefinitionVersion, SemanticCatalog as _};

    const VALID: &str = "description: A model.\nfields:\n  - name: id\n";

    fn load(root: &Path) -> Option<OkfCatalogError> {
        OkfCatalog::new(
            SourceName::parse("test").expect("a test name is a name"),
            root.to_path_buf(),
            DefinitionVersion::parse("test-1").expect("a test version is a version"),
        )
        .load()
        .err()
    }

    /// A scratch directory of this test's own, cleared on the way in.
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sutura-catalog-okf-refusals-{name}-{}", std::process::id()));
        drop(std::fs::remove_dir_all(&dir));
        std::fs::create_dir_all(&dir).expect("a scratch directory is creatable");
        dir
    }

    /// Loads a directory holding one `document` as `file` and returns its refusal, removing the
    /// directory.
    fn refused(name: &str, file: &str, document: &[u8]) -> Option<OkfCatalogError> {
        let root = scratch(name);
        std::fs::write(root.join(file), document).expect("a descriptor is writable");
        let outcome = load(&root);
        drop(std::fs::remove_dir_all(&root));
        outcome
    }

    #[test]
    fn a_non_directory_root_is_refused() {
        let root = scratch("file-root");
        let file = root.join("orders.yaml");
        std::fs::write(&file, VALID).expect("a descriptor is writable");
        let outcome = load(&file);
        drop(std::fs::remove_dir_all(&root));
        assert!(matches!(outcome, Some(OkfCatalogError::NotADirectory { .. })), "{outcome:?}");
    }

    #[test]
    fn a_document_that_is_not_utf8_is_refused_as_io() {
        let outcome = refused("non-utf8", "orders.yaml", b"\xff\xfe not utf8");
        assert!(matches!(outcome, Some(OkfCatalogError::Io { .. })), "{outcome:?}");
    }

    #[test]
    fn a_file_stem_that_is_not_a_table_name_is_refused() {
        let outcome = refused("bad-stem", "1orders.yaml", VALID.as_bytes());
        assert!(matches!(outcome, Some(OkfCatalogError::InvalidName { .. })), "{outcome:?}");
    }

    #[test]
    fn a_field_name_that_is_not_a_column_name_is_refused() {
        let outcome = refused(
            "bad-column",
            "orders.yaml",
            b"description: A model.\nfields:\n  - name: '1bad'\n",
        );
        assert!(matches!(outcome, Some(OkfCatalogError::InvalidColumn { .. })), "{outcome:?}");
    }

    #[test]
    fn an_okf_model_description_with_a_control_character_is_refused() {
        let outcome = refused(
            "ctrl-model",
            "orders.yaml",
            b"description: \"A model.\\rmore\"\nfields:\n  - name: id\n",
        );
        assert!(
            matches!(outcome, Some(OkfCatalogError::InvalidDescription { .. })),
            "{outcome:?}"
        );
    }

    #[test]
    fn a_field_description_with_a_control_character_is_refused() {
        let descriptor = b"description: A model.\nfields:\n  - name: id\n    description: \"A field.\\rmore\"\n";
        let outcome = refused("ctrl-column", "orders.yaml", descriptor);
        assert!(
            matches!(outcome, Some(OkfCatalogError::InvalidColumnDescription { ref column, .. }) if column.as_str() == "id"),
            "{outcome:?}"
        );
    }

    #[test]
    fn a_directory_with_more_than_max_documents_is_refused() {
        let root = scratch("too-many");
        for i in 0..=sutura_bounded_read::MAX_CATALOG_DOCUMENTS {
            std::fs::write(root.join(format!("m{i}.yaml")), VALID).expect("a descriptor is writable");
        }
        let outcome = load(&root);
        drop(std::fs::remove_dir_all(&root));
        assert!(
            matches!(outcome, Some(OkfCatalogError::TooManyDocuments { .. })),
            "{outcome:?}"
        );
    }

    /// A string `primaryKey` passes the shape check and fails the column-name parse -
    /// `src/tests.rs` provokes only the `NotAStringOrList` arm.
    #[test]
    fn a_primary_key_string_that_is_not_a_column_name_is_refused() {
        let outcome = refused(
            "bad-pk-name",
            "orders.yaml",
            b"description: A model.\nprimaryKey: '1bad'\nfields:\n  - name: id\n",
        );
        assert!(
            matches!(
                outcome,
                Some(OkfCatalogError::InvalidPrimaryKey {
                    cause: InvalidPrimaryKeyShape::Column { .. },
                    ..
                })
            ),
            "{outcome:?}"
        );
    }
}
