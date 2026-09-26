//! The harness `super::tests` builds its fixture trees with - moved out of `one_bound.rs` at the
//! 1000-line cap. A file with no `#[test]` is the part a test-bearing file may shed, and every
//! assertion stays where it is.

use std::collections::{BTreeMap, BTreeSet};

use super::{Scan, scan};

/// A fixture tree: the paths the gate lists, and what each one holds.
///
/// Named rather than a tuple, because `type_complexity` is tightened in this workspace.
pub(super) struct Tree {
    pub(super) paths: Vec<String>,
    pub(super) contents: BTreeMap<String, String>,
}

/// A tree of paths to contents, read the way the gate reads the working tree.
pub(super) fn tree(files: &[(&str, &str)]) -> Tree {
    Tree {
        paths: files.iter().map(|&(path, _)| String::from(path)).collect(),
        contents: files
            .iter()
            .map(|&(path, text)| (String::from(path), String::from(text)))
            .collect(),
    }
}

/// The scan over such a tree.
pub(super) fn scanned(files: &[(&str, &str)]) -> Scan {
    let Tree { paths, contents } = tree(files);
    scan(&paths, &|path| contents.get(path).cloned()).expect("a fixture tree is readable")
}

pub(super) fn named(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|&name| String::from(name)).collect()
}

/// A root that builds one bound and hands it to every taker this gate keys on.
///
/// All three, which no real root does - `serve.rs` composes the HTTP state and `mcp.rs`
/// the agent surface. It has to be all three here so that renaming ONE of them below leaves the
/// other two matched, and the failure therefore names the needle under test rather than
/// whichever happens to sort first.
pub(super) const GOOD_ROOT: &str = "\
fn run() -> Result<(), String> {
    let admission = Admission::from_settings(settings.runtime());
    let state = ServiceState::new(service, Arc::new(settings), admission.clone());
    let agent = AgentSurface::new(service, permitted, prose, admission.clone(), reply);
    block_on(sutura_mcp::serve_stdio(service, permitted, prose, admission, reply))
}
";
