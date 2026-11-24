//! `catalogs[].max_definitions_bytes` through the whole open path - `github.com/telekom/sutura#2071`,
//! `#2073`'s review blocking 2.
//!
//! **Why this is not inside `catalog.rs`'s own `mod tests`.** That module builds a
//! [`sutura_catalog_local::LocalCatalog`] by hand and calls `load_each` on it directly, so no
//! `max_definitions_bytes` an operator declared ever enters that cell - it could not have caught
//! the composer bug (`#2073` review blocking 1): `sutura_app::assemble::{assemble, assemble_linked}`
//! used to check the merged bundle against the bound-less `Definitions::assemble`
//! (`DefinitionsByteBound::default()`, 1 MiB) no matter what any contributor declared, and a test
//! that never constructs a declared bound cannot see it ignored. This file goes through
//! `sutura_config::Settings::load` and `super::{open_catalog, load}` - the actual composition-root
//! path a deployment runs - so a regression that reintroduces the bug (by, say, dropping the
//! `bound` parameter again) fails here.
//!
//! Both cells load the `examples/single-player` catalog from a scratch copy, so neither has to
//! hand-author a tree that satisfies every declared `DefinitionKind` from scratch.

use std::path::{Path, PathBuf};

use sutura_config::{Environment, Settings, Sources};

/// A scratch copy of the example catalog, with one model's description grown past `extra_bytes`
/// beyond its own, so a caller can push the bundle over an arbitrary byte count.
///
/// Copying rather than hand-authoring: `UnfaithfulDeclaration::Unprovided` refuses a markdown
/// tree that declares a `DefinitionKind` (every one of them, since `LocalCatalog::capabilities`
/// claims everything) and supplies none of it, and the example catalog is already a tree that
/// supplies every kind this adapter can produce - the same fixture `tests/example.rs` loads.
fn scratch_catalog(name: &str, extra_bytes: usize) -> PathBuf {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let source = repo_root.join("examples/single-player/catalog");
    let root = std::env::temp_dir().join(format!("sutura-cli-definitions-bound-{name}-{}", std::process::id()));
    drop(std::fs::remove_dir_all(&root));
    copy_dir(&source, &root);
    if extra_bytes > 0 {
        let target = root.join("models/customers.md");
        let mut content = std::fs::read_to_string(&target).expect("the copied model is readable");
        content.push_str(&"x".repeat(extra_bytes));
        std::fs::write(&target, content).expect("the grown model is writable");
    }
    root
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("a scratch directory is creatable");
    for entry in std::fs::read_dir(from).expect("the source directory is readable") {
        let entry = entry.expect("a directory entry is readable");
        let dest = to.join(entry.file_name());
        if entry.file_type().expect("a file type is readable").is_dir() {
            copy_dir(&entry.path(), &dest);
        } else {
            std::fs::copy(entry.path(), &dest).expect("a file is copyable");
        }
    }
}

/// Settings declaring exactly one markdown catalog at `dir`, with the given
/// `max_definitions_bytes` (omitted when `None`, which resolves to the 1 MiB default).
fn settings_for(dir: &Path, max_definitions_bytes: Option<usize>) -> Settings {
    let bound = max_definitions_bytes.map_or_else(String::new, |bytes| format!("\n    max_definitions_bytes: {bytes}"));
    let overlay = format!(
        "catalogs:\n  - name: \"model\"\n    kind: \"markdown\"\n    dir: \"{}\"\n    data_dir: \"{}\"\n    \
         version: \"test-1\"{bound}\n",
        dir.display(),
        dir.display(),
    );
    let sources = Sources::defaults(Environment::Development).with_overlay(overlay);
    Settings::load(&sources).expect("a declared markdown catalog with a well-formed bound loads")
}

/// A catalog whose authored content is pushed past the 1 MiB default composes once its own
/// `max_definitions_bytes` is raised - the composer bug `#2073`'s review caught: before the fix,
/// `sutura_app::assemble::{assemble, assemble_linked}` re-checked the merged bundle against the
/// bound-less `Definitions::assemble`, which is `DefinitionsByteBound::default()` (1 MiB)
/// regardless of what this catalog declared.
#[test]
fn a_catalog_over_the_default_bound_loads_once_its_own_bound_is_raised() {
    let root = scratch_catalog("raised", 2 * 1024 * 1024);

    let default_settings = settings_for(&root, None);
    let default_catalogs = default_settings.catalogs();
    let opened = super::open_catalog(default_catalogs, None).expect("opening does not read file content");
    let refused = super::load(&opened).expect_err("over 1 MiB of authored content is refused at the default bound");
    assert!(
        refused.contains("authored bytes"),
        "expected the definitions-too-large refusal, got: {refused}"
    );

    let raised_settings = settings_for(&root, Some(4 * 1024 * 1024));
    let raised_catalogs = raised_settings.catalogs();
    let opened = super::open_catalog(raised_catalogs, None).expect("opening does not read file content");
    let pinned = super::load(&opened).expect("a raised max_definitions_bytes admits the same content");
    assert!(
        !pinned.definitions().models().is_empty(),
        "the bundle still carries its models"
    );

    drop(std::fs::remove_dir_all(&root));
}

/// A markdown catalog well under the 1 MiB default is still refused once its OWN declared bound is
/// set below what it authors - the per-catalog bound is read and enforced, not just defaulted.
#[test]
fn a_markdown_catalog_with_a_low_declared_bound_is_refused() {
    let root = scratch_catalog("low", 0);
    let settings = settings_for(&root, Some(1000));
    let catalogs = settings.catalogs();
    let opened = super::open_catalog(catalogs, None).expect("opening does not read file content");
    let refused = super::load(&opened).expect_err("the example catalog's authored content exceeds 1000 bytes");
    assert!(
        refused.contains("authored bytes") && refused.contains("limit is 1000"),
        "expected a definitions-too-large refusal naming the declared bound, got: {refused}"
    );
    drop(std::fs::remove_dir_all(&root));
}
