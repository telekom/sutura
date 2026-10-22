//! The justfile and the files it imports as one text, for a cell that asserts on a recipe.
//!
//! `xtask` owns the reader that parses the set and refuses what it cannot follow (`mod`, `import?`,
//! a nested import); this crate cannot depend on that reader's module, and the two cells that call
//! this (here and in `sutura-app`) only need the text. So it follows each unindented
//! `import "<path>"` line, and an import it cannot open, or whose path leaves the root, is an
//! error rather than a skip: a scan that reads nothing passes everything.
//!
//! Limit: nothing else is refused here. `cargo xtask check-scope` is what reads a `mod` or an
//! `import?` and fails on it.

use std::fmt;
use std::io;
use std::path::{Component, Path, PathBuf};

/// Why the justfile set could not be read.
#[derive(Debug)]
pub enum Refused {
    /// A file of the set could not be opened.
    Unreadable {
        /// The path, relative to the root.
        path: PathBuf,
        /// What the operating system said.
        source: io::Error,
    },
    /// An `import` names an absolute path or one with a `.` or `..` component.
    Outside {
        /// The path as written.
        path: PathBuf,
    },
}

impl fmt::Display for Refused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreadable { path, source } => write!(f, "{} cannot be read: {source}", path.display()),
            Self::Outside { path } => write!(f, "`import \"{}\"` leaves the repo root", path.display()),
        }
    }
}

impl std::error::Error for Refused {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Unreadable { source, .. } => Some(source),
            Self::Outside { .. } => None,
        }
    }
}

/// The root `justfile`, then each file it imports, joined by a newline.
///
/// # Errors
///
/// A file that cannot be read, or an import path outside the root.
pub fn text(root: &Path) -> Result<String, Refused> {
    let mut whole = read(root, PathBuf::from("justfile"))?;
    let imports: Vec<PathBuf> = whole
        .lines()
        .filter_map(|line| line.strip_prefix("import ")?.split(['"', '\'']).nth(1))
        .map(PathBuf::from)
        .collect();
    for path in imports {
        if !path.components().all(|part| matches!(part, Component::Normal(_))) {
            return Err(Refused::Outside { path });
        }
        let imported = read(root, path)?;
        whole.push('\n');
        whole.push_str(&imported);
    }
    Ok(whole)
}

fn read(root: &Path, path: PathBuf) -> Result<String, Refused> {
    std::fs::read_to_string(root.join(&path)).map_err(|source| Refused::Unreadable { path, source })
}

#[cfg(test)]
mod tests {
    use super::{Refused, text};
    use std::path::PathBuf;

    fn tree(tag: &str, files: &[(&str, &str)]) -> PathBuf {
        let root = std::env::temp_dir().join(format!("sutura-dev-justfile-{}-{tag}", std::process::id()));
        drop(std::fs::remove_dir_all(&root));
        for (path, body) in files {
            let at = root.join(path);
            std::fs::create_dir_all(at.parent().expect("a parent")).expect("a fixture directory");
            std::fs::write(at, body).expect("a fixture file");
        }
        root
    }

    #[test]
    fn the_text_is_the_root_then_each_imported_file() {
        let root = tree(
            "text",
            &[("justfile", "import \"just/a.just\"\nx:\n"), ("just/a.just", "a:\n")],
        );
        assert_eq!(text(&root).expect("the set reads"), "import \"just/a.just\"\nx:\n\na:\n");
    }

    #[test]
    fn an_import_it_cannot_open_is_an_error_not_a_skip() {
        let root = tree("missing", &[("justfile", "import \"gone.just\"\n")]);
        assert!(matches!(text(&root), Err(Refused::Unreadable { path, .. }) if path.to_str() == Some("gone.just")));
    }

    #[test]
    fn an_import_that_leaves_the_root_is_refused() {
        let root = tree("outside", &[("justfile", "import \"../x.just\"\n")]);
        assert!(matches!(text(&root), Err(Refused::Outside { .. })));
    }
}
