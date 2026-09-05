//! The harness three of this gate's test modules were each carrying a copy of.
//!
//! A post-image reader and a line builder, nothing else. It lives in its own file for the reason
//! the gate itself teaches: a file with no `#[test]` is one the gate may revert, so a harness is
//! exactly what may move out of a test-bearing file, and assertions are exactly what may not.

use crate::causality::diff::ChangedFile;
use crate::causality::regions::AddedLine;

/// A post-image reader over a fixed set of files, standing in for the working tree.
pub(crate) fn tree(files: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> + use<> {
    let owned: Vec<(String, String)> = files
        .iter()
        .map(|&(path, text)| (String::from(path), String::from(text)))
        .collect();
    move |wanted: &str| owned.iter().find(|(path, _)| path == wanted).map(|(_, text)| text.clone())
}

/// Added lines numbered consecutively from `first`.
pub(crate) fn added_from(first: usize, texts: &[&str]) -> Vec<AddedLine> {
    texts
        .iter()
        .enumerate()
        .map(|(offset, text)| AddedLine::new(first + offset, *text))
        .collect()
}

/// A manifest declaring one package, as the post-image reader hands it back.
///
/// Three test modules were spelling this three ways, and only the shape `changes::package_name`
/// parses matters - so a fourth spelling drifting is the thing this removes.
pub(crate) fn manifest(name: &str) -> String {
    format!("[package]\nname = \"{name}\"\nversion.workspace = true\n")
}

/// A changed file whose added lines run consecutively from `first`.
pub(crate) fn changed(path: &str, first: usize, texts: &[&str]) -> ChangedFile {
    ChangedFile {
        path: String::from(path),
        added: added_from(first, texts),
    }
}
