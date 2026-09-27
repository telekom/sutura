//! The entry cap on the walk, over a tree of non-documents the document cap never sees.

use std::path::PathBuf;

use super::WalkError;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("sutura-bounded-read-entry-cap-{name}-{}", std::process::id()));
    drop(std::fs::remove_dir_all(&dir));
    std::fs::create_dir_all(&dir).expect("a scratch directory is creatable");
    dir
}

/// One entry over the cap, none of them documents, is refused rather than walked to its end.
#[test]
fn a_tree_with_more_entries_than_the_cap_is_refused() {
    let root = scratch("too-many-entries");
    for i in 0..=super::MAX_CATALOG_ENTRIES {
        std::fs::write(root.join(format!("entry-{i}.txt")), "not a document").expect("a scratch file is writable");
    }
    let err = super::walk(&root, &["yaml"], super::MAX_CATALOG_DOCUMENTS).expect_err("an over-cap tree refuses");
    assert!(
        matches!(err, WalkError::TooManyEntries { found, limit, .. } if found == super::MAX_CATALOG_ENTRIES + 1 && limit == super::MAX_CATALOG_ENTRIES),
        "expected TooManyEntries with found={} and limit={}, got {err:?}",
        super::MAX_CATALOG_ENTRIES + 1,
        super::MAX_CATALOG_ENTRIES,
    );
    drop(std::fs::remove_dir_all(&root));
}

/// Exactly the cap is walked: the refusal starts one entry past it.
#[test]
fn a_tree_at_exactly_the_cap_is_walked() {
    let root = scratch("at-cap");
    for i in 0..super::MAX_CATALOG_ENTRIES {
        std::fs::write(root.join(format!("entry-{i}.txt")), "not a document").expect("a scratch file is writable");
    }
    let err = super::walk(&root, &["yaml"], super::MAX_CATALOG_DOCUMENTS).expect_err("no documents, but not too many entries");
    assert!(
        matches!(err, WalkError::Empty { .. }),
        "at exactly the cap the walk should return Empty, not TooManyEntries: {err:?}"
    );
    drop(std::fs::remove_dir_all(&root));
}
