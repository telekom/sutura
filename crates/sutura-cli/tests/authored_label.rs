//! `catalog` over `examples/authored-sql/catalog`, spawned, so a rename of the label
//! `commands::computation_label` prints for the authored-SQL case is LOOKED AT rather than merely
//! compiled against the `const` assertion sitting beside it.
//!
//! `just documented` reads `docs/getting-started.md` and `examples/single-player/README.md`
//! only, and neither prints a metric whose `computation` is `AuthoredSql` - which is exactly how
//! the label grew to the width of its own column without a build noticing. This pins the one line
//! that command prints for the one catalog that exercises the other arm.

// `cfg(test)` for the reason `crates/sutura-cli/tests/example.rs` gives: clippy honours
// `allow-expect-in-tests` only inside a `#[cfg(test)]` item, and without it every `expect` below is
// a lint error.
#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::process::Command;

    fn catalog_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/authored-sql/catalog")
    }

    #[test]
    fn catalog_labels_the_authored_metric_with_a_column_the_value_does_not_touch() {
        let output = Command::new(env!("CARGO_BIN_EXE_sutura"))
            .arg("catalog")
            .arg(catalog_root())
            .output()
            .expect("the composed binary runs");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(output.status.success(), "code {:?}\n{stdout}", output.status.code());
        assert!(
            stdout.contains("  authored   authored sql for portable\n"),
            "the authored label or its spacing moved:\n{stdout}"
        );
    }
}
