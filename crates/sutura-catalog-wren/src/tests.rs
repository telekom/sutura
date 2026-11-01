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

/// A destination that cannot be written into is `Write` naming a path under it. The destination is
/// a symlink to a directory that does not exist: the emptiness check reads it as absent, and the
/// first `create_dir_all` under it fails because the link is not a directory. A permission bit would
/// not do - root writes through a read-only mode, and this cell must not pass by never refusing.
#[cfg(unix)]
#[test]
fn an_unwritable_destination_is_a_write_error_naming_the_path() {
    let source = scratch("unwritable");
    let destination = source.join("out");
    std::fs::write(
        source.join("manifest.json"),
        r#"{"catalog":"test","schema":"public","models":[{"name":"orders","columns":[{"name":"id","type":"BIGINT"}]}]}"#,
    )
    .expect("the scratch source is writable");
    std::os::unix::fs::symlink(source.join("absent"), &destination).expect("a dangling symlink is creatable");

    let err = import(source.as_path(), destination.as_path()).expect_err("an unwritable destination is not importable");

    drop(std::fs::remove_dir_all(&source));
    assert!(
        matches!(err, ImportError::Write { ref path, .. } if path.starts_with(&destination)),
        "an unwritable destination is Write naming the path, got {err:?}"
    );
}

fn refused_kind(source: &std::path::Path) -> Option<std::io::ErrorKind> {
    match import(source, &source.join("out")) {
        Err(ImportError::Read { source, .. }) => Some(source.kind()),
        _ => None,
    }
}

/// A manifest past the 16 MiB bound is refused as too large, never read whole and handed to the parser.
#[test]
fn a_manifest_past_the_bound_is_refused_unread() {
    let source = scratch("large");
    std::fs::File::create(source.join("manifest.json"))
        .and_then(|created| created.set_len(16 * 1024 * 1024 + 1))
        .expect("the manifest is sized");
    let refused = refused_kind(&source);
    drop(std::fs::remove_dir_all(&source));
    assert_eq!(refused, Some(std::io::ErrorKind::FileTooLarge));
}

/// A manifest path that is not a regular file is refused before it is opened, so a FIFO cannot
/// block the import.
#[test]
fn a_manifest_that_is_not_a_regular_file_is_refused_before_it_is_opened() {
    let source = scratch("directory");
    std::fs::create_dir_all(source.join("manifest.json")).expect("a directory where the manifest goes");
    let refused = refused_kind(&source);
    drop(std::fs::remove_dir_all(&source));
    assert_eq!(refused, Some(std::io::ErrorKind::InvalidInput));
}
