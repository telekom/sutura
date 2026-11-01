use std::io::ErrorKind;
use std::path::PathBuf;

use super::{ImportError, MAX_MANIFEST_BYTES, import};

/// A project directory of this test's own, removed by the caller.
fn project(name: &str) -> PathBuf {
    let directory = std::env::temp_dir().join(format!("sutura-wren-manifest-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&directory).expect("a scratch directory is creatable");
    directory
}

fn refused_kind(source: &std::path::Path) -> Option<ErrorKind> {
    match import(source, &source.join("out")) {
        Err(ImportError::Read { source, .. }) => Some(source.kind()),
        _ => None,
    }
}

/// A manifest past the bound is refused as too large, never read whole and handed to the parser.
#[test]
fn a_manifest_past_the_bound_is_refused_unread() {
    let directory = project("large");
    std::fs::File::create(directory.join("manifest.json"))
        .and_then(|created| created.set_len(MAX_MANIFEST_BYTES + 1))
        .expect("the manifest is sized");
    let refused = refused_kind(&directory);
    let _ignored = std::fs::remove_dir_all(&directory);
    assert_eq!(refused, Some(ErrorKind::FileTooLarge));
}

/// A manifest path that is not a regular file is refused before it is opened, so a FIFO cannot
/// block the import.
#[test]
fn a_manifest_that_is_not_a_regular_file_is_refused_before_it_is_opened() {
    let directory = project("directory");
    std::fs::create_dir_all(directory.join("manifest.json")).expect("a directory where the manifest goes");
    let refused = refused_kind(&directory);
    let _ignored = std::fs::remove_dir_all(&directory);
    assert_eq!(refused, Some(ErrorKind::InvalidInput));
}
