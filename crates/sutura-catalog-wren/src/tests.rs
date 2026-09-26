use super::{ImportError, import};

/// A scratch directory of this test's own, cleared on the way in - `tempfile` is not a
/// dependency of this workspace and one test is not the argument for adding one; the crate's
/// sibling tests under `sutura-catalog-local` use the same pattern. A fresh dir per call, because
/// this repo has hit shared-path races before.
fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "sutura-catalog-wren-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("the clock is after 1970")
            .as_nanos()
    ));
    drop(std::fs::remove_dir_all(&dir));
    std::fs::create_dir_all(&dir).expect("a scratch directory is creatable");
    dir
}

/// A missing `manifest.json` is `Read` naming the path, not `NotAManifest`: the read runs before
/// the parse, and a file that is not there is not handed to `serde_json` at all.
#[test]
fn a_missing_manifest_json_is_read_naming_the_path() {
    let source = scratch("missing");
    let destination = scratch("missing-out");

    let err = import(source.as_path(), destination.as_path()).expect_err("a directory with no manifest.json is not importable");

    assert!(
        matches!(err, ImportError::Read { ref path, .. } if path == &source.join("manifest.json")),
        "a missing manifest.json is Read naming the path, got {err:?}"
    );
}

/// A `manifest.json` that is not JSON is `NotAManifest` naming the path: the read succeeds, the
/// parse fails, and the `serde_json` error is carried with the path rather than reported as a read.
#[test]
fn a_manifest_that_is_not_a_wren_mdl_object_is_not_a_manifest() {
    let source = scratch("not-a-manifest");
    let destination = scratch("not-a-manifest-out");
    std::fs::write(source.join("manifest.json"), "not a manifest").expect("the scratch source is writable");

    let err = import(source.as_path(), destination.as_path()).expect_err("a manifest that is not JSON is not importable");

    assert!(
        matches!(err, ImportError::NotAManifest { ref path, .. } if path == &source.join("manifest.json")),
        "a non-JSON manifest is NotAManifest naming the path, got {err:?}"
    );
}
