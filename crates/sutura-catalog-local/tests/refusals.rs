#![forbid(unsafe_code)]
//! Every reachable `LocalCatalogError` refusal no other test provokes, each driven through the
//! public [`SemanticCatalog::load`] port over a scratch directory of real documents and asserted by
//! variant, the contract a caller matches on.
//!
//! Not provoked: `Digest`, which no input reaches. The `Io` cell is Unix-only and holds for an
//! unprivileged runner only - a root process reads a mode-000 directory.
//!
//! Wrapped in `#[cfg(test)] mod tests` so `allow-expect-in-tests` reaches the helpers, the shape
//! `tests/bounds.rs` uses.

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use sutura_catalog_local::document::{InvalidMetricDocument, InvalidModelDocument};
    use sutura_catalog_local::{LocalCatalog, LocalCatalogError};
    use sutura_domain::model::SourceName;
    use sutura_domain::pinned::{DefinitionVersion, SemanticCatalog as _};

    const MODEL: &str =
        "---\nkind: model\nname: orders\nsource: local\ntable: fct_order\ncolumns: [amount_cents, order_date]\n---\nOrders.\n";

    fn load(root: &Path) -> Option<LocalCatalogError> {
        LocalCatalog::new(
            SourceName::parse("test").expect("a test name is a name"),
            root.to_path_buf(),
            DefinitionVersion::parse("test-1").expect("a test version is a version"),
        )
        .load()
        .err()
    }

    /// A scratch directory of this test's own, cleared on the way in.
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sutura-catalog-local-refusals-{name}-{}", std::process::id()));
        drop(std::fs::remove_dir_all(&dir));
        std::fs::create_dir_all(&dir).expect("a scratch directory is creatable");
        dir
    }

    /// Loads a directory holding `documents` and returns its refusal, removing the directory.
    fn refused(name: &str, documents: &[(&str, &str)]) -> Option<LocalCatalogError> {
        let root = scratch(name);
        for &(file, document) in documents {
            std::fs::write(root.join(file), document).expect("a document is writable");
        }
        let outcome = load(&root);
        drop(std::fs::remove_dir_all(&root));
        outcome
    }

    const SUM: &str = "measure:\n  simple: { aggregate: sum, column: amount_cents }\n";

    fn metric(body: &str) -> String {
        format!("---\nkind: metric\nname: revenue\n{body}time_column: order_date\ngrains: [month]\n---\nNet revenue.\n")
    }

    #[test]
    fn a_file_where_a_catalog_root_should_be_is_refused_as_not_a_directory() {
        let root = scratch("file-root");
        let file = root.join("orders.md");
        std::fs::write(&file, MODEL).expect("a document is writable");
        let outcome = load(&file);
        drop(std::fs::remove_dir_all(&root));
        assert!(
            matches!(outcome, Some(LocalCatalogError::NotADirectory { ref path }) if *path == file),
            "{outcome:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn an_unreadable_subdirectory_is_refused_as_io() {
        use std::os::unix::fs::PermissionsExt as _;
        let root = scratch("unreadable");
        let subdir = root.join("subdir");
        std::fs::create_dir_all(&subdir).expect("a subdirectory is creatable");
        std::fs::set_permissions(&subdir, std::fs::Permissions::from_mode(0o000)).expect("permissions are settable");
        let outcome = load(&root);
        drop(std::fs::set_permissions(&subdir, std::fs::Permissions::from_mode(0o755)));
        drop(std::fs::remove_dir_all(&root));
        assert!(
            matches!(outcome, Some(LocalCatalogError::Io { ref path, .. }) if *path == subdir),
            "{outcome:?}"
        );
    }

    #[test]
    fn a_typed_frontmatter_field_with_the_wrong_type_is_refused_as_frontmatter() {
        let model = MODEL.replace("[amount_cents, order_date]", "\"not a list\"");
        let outcome = refused("wrong-type", &[("orders.md", &model)]);
        assert!(
            matches!(outcome, Some(LocalCatalogError::Frontmatter { kind: "model", .. })),
            "{outcome:?}"
        );
    }

    #[test]
    fn a_metric_naming_an_undeclared_model_is_refused_as_inconsistent() {
        let metric = metric(&format!("model: nonexistent\n{SUM}audience: open\n"));
        let outcome = refused("unknown-model", &[("orders.md", MODEL), ("revenue.md", &metric)]);
        assert!(matches!(outcome, Some(LocalCatalogError::Inconsistent { .. })), "{outcome:?}");
    }

    #[test]
    fn an_empty_directory_is_refused_as_empty() {
        let outcome = refused("empty", &[]);
        assert!(matches!(outcome, Some(LocalCatalogError::Empty { .. })), "{outcome:?}");
    }

    #[test]
    fn a_metric_with_no_computation_is_refused_as_metric_through_load() {
        let outcome = refused(
            "no-computation",
            &[
                ("orders.md", MODEL),
                ("revenue.md", &metric("model: orders\naudience: open\n")),
            ],
        );
        assert!(
            matches!(
                outcome,
                Some(LocalCatalogError::Metric {
                    cause: InvalidMetricDocument::Computation(_),
                    ..
                })
            ),
            "{outcome:?}"
        );
    }

    #[test]
    fn a_model_with_an_undeclared_primary_key_is_refused_as_model_through_load() {
        let model = MODEL.replace("order_date]\n", "order_date]\nprimary_key: [order_id]\n");
        let outcome = refused("undeclared-key", &[("orders.md", &model)]);
        assert!(
            matches!(
                outcome,
                Some(LocalCatalogError::Model {
                    cause: InvalidModelDocument::PrimaryKey(_),
                    ..
                })
            ),
            "{outcome:?}"
        );
    }

    #[test]
    fn a_column_description_with_an_invisible_character_is_refused_as_column_description() {
        // U+202E, a right-to-left override: YAML keeps it, `Description::parse` refuses it.
        let model = MODEL.replace(
            "[amount_cents, order_date]",
            "\n  - order_date\n  - name: amount_cents\n    description: \"Net revenue where status = 'act\u{202E}ive'.\"",
        );
        let outcome = refused("invisible", &[("orders.md", &model)]);
        assert!(
            matches!(
                outcome,
                Some(LocalCatalogError::Model { cause: InvalidModelDocument::ColumnDescription { ref column, .. }, .. })
                    if column.as_str() == "amount_cents"
            ),
            "{outcome:?}"
        );
    }

    #[test]
    fn a_metric_audience_with_an_unusable_identifier_is_refused_as_audience() {
        let metric = metric(&format!(
            "model: orders\n{SUM}audience:\n  restricted: [\"not an identifier!\"]\n"
        ));
        let outcome = refused("audience", &[("orders.md", MODEL), ("revenue.md", &metric)]);
        assert!(
            matches!(
                outcome,
                Some(LocalCatalogError::Metric {
                    cause: InvalidMetricDocument::Audience { .. },
                    ..
                })
            ),
            "{outcome:?}"
        );
    }
}
