#![forbid(unsafe_code)]
//! `DataContractError` refusals `src/tests.rs` does not provoke, each driven through the public
//! [`SemanticCatalog::load`] port over a scratch directory of real contract files and asserted by
//! variant, the contract a caller matches on.
//!
//! Not provoked, because no input reaches them: `RelationshipName` (the name is fitted to the
//! identifier limit by construction), `UncheckableKnowledge` (this adapter supplies
//! `KnowledgeInput::none()`) and `Digest`. **Reachable but not re-provoked here:** the walk arm of
//! `Io` (a mode-000 subdirectory) and `NotARegularFile` (a FIFO swapped in during the read
//! window). This adapter reaches both through the same `sutura_bounded_read::walk` and
//! `read_document` that `sutura-bounded-read`'s and `sutura-catalog-okf`'s cells provoke; no cell
//! in this crate holds its mapping of them. `Io` is provoked here on its read arm only.
//!
//! Wrapped in `#[cfg(test)] mod tests` so `allow-expect-in-tests` reaches the helpers, the shape
//! `tests/bounds.rs` uses.

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use sutura_catalog_datacontract::{DataContractCatalog, DataContractError};
    use sutura_domain::model::SourceName;
    use sutura_domain::pinned::{DefinitionVersion, SemanticCatalog as _};

    fn catalog(root: PathBuf) -> DataContractCatalog {
        DataContractCatalog::new(
            SourceName::parse("test").expect("a test name is a name"),
            root,
            DefinitionVersion::parse("test-1").expect("a test version is a version"),
        )
    }

    /// A scratch directory of this test's own, cleared on the way in.
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sutura-catalog-datacontract-refusals-{name}-{}", std::process::id()));
        drop(std::fs::remove_dir_all(&dir));
        std::fs::create_dir_all(&dir).expect("a scratch directory is creatable");
        dir
    }

    /// A v3.1.0 contract with identifier `id` whose `schema:` list is `schema`.
    fn contract(id: &str, schema: &str) -> String {
        format!("version: 1.0.0\nkind: DataContract\nid: {id}\nstatus: active\napiVersion: v3.1.0\nschema:\n{schema}")
    }

    /// Loads a directory holding `documents` and returns its refusal, removing the directory.
    fn load(name: &str, documents: &[(&str, &str)]) -> Option<DataContractError> {
        let root = scratch(name);
        for &(file, document) in documents {
            std::fs::write(root.join(file), document).expect("a contract is writable");
        }
        let outcome = catalog(root.clone()).load().err();
        drop(std::fs::remove_dir_all(&root));
        outcome
    }

    fn refused(name: &str, schema: &str) -> Option<DataContractError> {
        load(name, &[("contract.yaml", &contract("x", schema))])
    }

    #[test]
    fn a_file_root_is_refused_as_not_a_directory() {
        let file = std::env::temp_dir().join(format!(
            "sutura-catalog-datacontract-refusals-file-root-{}",
            std::process::id()
        ));
        std::fs::write(&file, "not a directory").expect("a file is writable");
        let outcome = catalog(file.clone()).load();
        drop(std::fs::remove_file(&file));
        assert!(
            matches!(outcome, Err(DataContractError::NotADirectory { .. })),
            "a file root is not a catalog: {outcome:?}"
        );
    }

    #[test]
    fn a_non_utf8_document_is_refused_as_io() {
        let root = scratch("non-utf8");
        std::fs::write(root.join("bad.yaml"), b"\xff\xfe\x00\x01").expect("a document is writable");
        let outcome = catalog(root.clone()).load();
        drop(std::fs::remove_dir_all(&root));
        assert!(
            matches!(outcome, Err(DataContractError::Io { .. })),
            "bytes that are not UTF-8 fail the read, not the parse: {outcome:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn an_unreadable_document_is_refused_at_open() {
        use std::os::unix::fs::PermissionsExt as _;
        let root = scratch("unreadable");
        let document = root.join("secret.yaml");
        std::fs::write(&document, contract("x", "  - name: orders\n")).expect("a contract is writable");
        std::fs::set_permissions(&document, std::fs::Permissions::from_mode(0o000)).expect("permissions are settable");
        let outcome = catalog(root.clone()).load();
        drop(std::fs::remove_dir_all(&root));
        // Holds for an unprivileged runner only: a root user opens a mode-000 file.
        assert!(
            matches!(outcome, Err(DataContractError::Open { .. })),
            "an unreadable document is refused at open: {outcome:?}"
        );
    }

    #[test]
    fn an_unknown_relationship_type_is_refused_by_name() {
        let outcome = refused(
            "unknown-relationship-type",
            "  - name: orders\n    properties:\n      - name: id\n        primaryKey: true\n      - name: customer_id\n    relationships:\n      - type: notNull\n        from: orders.customer_id\n        to: orders.id\n",
        );
        assert!(
            matches!(&outcome, Some(DataContractError::UnknownRelationshipType { found, .. }) if found == "notNull"),
            "a relationship type other than foreignKey is refused naming it: {outcome:?}"
        );
    }

    #[test]
    fn a_composite_key_endpoint_is_refused() {
        let outcome = refused(
            "composite",
            "  - name: orders\n    properties:\n      - name: id\n        primaryKey: true\n    relationships:\n      - type: foreignKey\n        from: [orders.id, orders.tenant]\n        to: orders.id\n",
        );
        assert!(
            matches!(outcome, Some(DataContractError::CompositeKeyUnrepresentable { .. })),
            "a composite endpoint is refused, not silently dropped: {outcome:?}"
        );
    }

    #[test]
    fn a_schema_object_with_an_invalid_name_is_refused() {
        let outcome = refused("model-name", "  - name: '1bad'\n    properties:\n      - name: id\n");
        assert!(
            matches!(outcome, Some(DataContractError::InvalidName { .. })),
            "a model name that is not an identifier is refused: {outcome:?}"
        );
    }

    #[test]
    fn a_column_with_an_invalid_name_is_refused() {
        let outcome = refused("column-name", "  - name: orders\n    properties:\n      - name: 'a b'\n");
        assert!(
            matches!(outcome, Some(DataContractError::InvalidColumn { .. })),
            "a column name that is not an identifier is refused: {outcome:?}"
        );
    }

    // `\x01` is a YAML double-quoted escape the parser decodes into U+0001, so the control character
    // reaches `Description::parse`; a raw byte would fail the YAML parse first, as `Malformed`.
    #[test]
    fn a_model_description_with_a_control_character_is_refused() {
        let outcome = refused(
            "model-description",
            "  - name: orders\n    description: \"bad\\x01text\"\n    properties:\n      - name: id\n",
        );
        assert!(
            matches!(outcome, Some(DataContractError::InvalidDescription { .. })),
            "a model description with a control character is refused: {outcome:?}"
        );
    }

    #[test]
    fn a_column_description_with_a_control_character_is_refused() {
        let outcome = refused(
            "column-description",
            "  - name: orders\n    properties:\n      - name: id\n        description: \"bad\\x01text\"\n",
        );
        assert!(
            matches!(outcome, Some(DataContractError::InvalidColumnDescription { .. })),
            "a column description with a control character is refused: {outcome:?}"
        );
    }

    #[test]
    fn two_contracts_declaring_the_same_model_are_refused() {
        let orders = "  - name: orders\n    properties:\n      - name: id\n";
        let outcome = load(
            "same-model",
            &[("a.yaml", &contract("a", orders)), ("b.yaml", &contract("b", orders))],
        );
        assert!(
            matches!(outcome, Some(DataContractError::Inconsistent { .. })),
            "two contracts declaring one model are an inconsistent catalog: {outcome:?}"
        );
    }

    #[test]
    fn a_directory_with_more_than_one_thousand_documents_is_refused() {
        let root = scratch("too-many");
        for i in 0..1001 {
            let document = contract(
                &format!("c{i}"),
                &format!("  - name: m{i}\n    properties:\n      - name: id\n"),
            );
            std::fs::write(root.join(format!("{i}.yaml")), document).expect("a contract is writable");
        }
        let outcome = catalog(root.clone()).load();
        drop(std::fs::remove_dir_all(&root));
        assert!(
            matches!(
                outcome,
                Err(DataContractError::TooManyDocuments {
                    found: 1001,
                    limit: 1000,
                    ..
                })
            ),
            "a directory past the document cap is refused: {outcome:?}"
        );
    }

    /// The entry cap bounds the tree, not the documents: a wide directory of skipped non-document
    /// files is refused here rather than walked without end. Ten thousand and one `.txt` files -
    /// entries the walk counts but does not collect - trip `MAX_CATALOG_ENTRIES` before the document
    /// cap `TooManyDocuments` ever could, because the entry count is checked on every entry and the
    /// document count only on each document inserted. The one `.yaml` has no role: the walk refuses
    /// on entry 10,001, before its `Empty` check after the loop runs.
    #[test]
    fn a_directory_with_more_than_ten_thousand_entries_is_refused() {
        let root = scratch("too-many-entries");
        for i in 0..=10_000 {
            std::fs::write(root.join(format!("skip-{i}.txt")), "not a document").expect("a scratch file is writable");
        }
        let document = contract("x", "  - name: m\n    properties:\n      - name: id\n");
        std::fs::write(root.join("contract.yaml"), document).expect("a contract is writable");
        let outcome = catalog(root.clone()).load();
        drop(std::fs::remove_dir_all(&root));
        assert!(
            matches!(
                outcome,
                Err(DataContractError::TooManyEntries {
                    found: 10_001,
                    limit: 10_000,
                    ..
                })
            ),
            "a directory past the entry cap is refused: {outcome:?}"
        );
    }
}
