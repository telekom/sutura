//! The justfile as the files `just` reads: the root `justfile` and each `import "<path>"` line in it.
//!
//! Flat on purpose: an import is a path relative to the repo root and an imported file imports
//! nothing. Everything this reader cannot follow is refused rather than skipped, because a scan that
//! reads nothing passes everything: an import it cannot open, a `mod` (its recipes are
//! `module::name`, which no `just <task>` citation spells), an `import?` (a file that may be absent
//! is a scan that may read nothing) and an import inside an imported file.
//!
//! It walks no git listing, so it reads the same set in the nix sandbox. Limits: it reads text and
//! does not run `just`, so a recipe `just` would reject still parses; a symlinked import is followed.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

const ROOT: &str = "justfile";

/// One file of the set, `path` relative to the repo root.
#[derive(Debug)]
pub(crate) struct Source {
    pub(crate) path: PathBuf,
    pub(crate) text: String,
}

/// An import path that stays inside the repo: relative, with no `.` or `..` component.
struct ImportPath(PathBuf);

impl ImportPath {
    fn parse(literal: &str) -> Option<Self> {
        let inside = literal.split('/').all(|part| !matches!(part, "" | "." | ".."));
        inside.then(|| Self(PathBuf::from(literal)))
    }
}

/// Why the set could not be read, with the file and 1-based line where it applies.
#[derive(Debug)]
pub(crate) enum Refused {
    Unreadable { path: PathBuf, source: io::Error },
    Module { file: PathBuf, line: usize },
    OptionalImport { file: PathBuf, line: usize },
    NestedImport { file: PathBuf, line: usize },
    Malformed { file: PathBuf, line: usize },
    Outside { file: PathBuf, line: usize },
}

impl fmt::Display for Refused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreadable { path, source } => write!(
                f,
                "{} cannot be read ({source}); an import the scan cannot open would read nothing",
                path.display()
            ),
            Self::Module { file, line } => write!(
                f,
                "{}:{line}: `mod` is not read: its recipes are `module::name`; use a flat `import`",
                file.display()
            ),
            Self::OptionalImport { file, line } => write!(
                f,
                "{}:{line}: `import?` is not read: a file that may be absent may be read as nothing; use `import`",
                file.display()
            ),
            Self::NestedImport { file, line } => write!(
                f,
                "{}:{line}: an imported file imports nothing; list the import in the root `justfile`",
                file.display()
            ),
            Self::Malformed { file, line } => write!(
                f,
                "{}:{line}: an `import` takes one quoted path with no backslash and nothing after it but a `#` comment",
                file.display()
            ),
            Self::Outside { file, line } => write!(
                f,
                "{}:{line}: an `import` path is relative to the repo root with no `.` or `..` component",
                file.display()
            ),
        }
    }
}

impl std::error::Error for Refused {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Unreadable { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// The root `justfile`, then each file it imports, in import order.
pub(crate) fn read(root: &Path) -> Result<Vec<Source>, Refused> {
    let top = load(root, PathBuf::from(ROOT))?;
    let imports = directives(&top.path, &top.text)?;
    let mut set = vec![top];
    for import in imports {
        let file = load(root, import.0)?;
        if let Some(found) = held_by_the_root(&file) {
            return Err(found);
        }
        set.push(file);
    }
    Ok(set)
}

fn load(root: &Path, path: PathBuf) -> Result<Source, Refused> {
    match std::fs::read_to_string(root.join(&path)) {
        Ok(text) => Ok(Source { path, text }),
        Err(source) => Err(Refused::Unreadable { path, source }),
    }
}

/// The set as one text, for a reader that searches rather than parses: `\n` between files, so no
/// file's last line runs into the next file's first.
pub(crate) fn text(root: &Path) -> Result<String, Refused> {
    Ok(read(root)?
        .into_iter()
        .map(|source| source.text)
        .collect::<Vec<_>>()
        .join("\n"))
}

/// The first `import` or `mod` in an imported file: only the root declares either.
fn held_by_the_root(imported: &Source) -> Option<Refused> {
    imported.text.lines().enumerate().find_map(|(index, raw)| {
        let (file, line) = (imported.path.clone(), index.saturating_add(1));
        match keyword(raw) {
            ("import", true) => Some(Refused::OptionalImport { file, line }),
            ("import", false) => Some(Refused::NestedImport { file, line }),
            ("mod", _) => Some(Refused::Module { file, line }),
            _ => None,
        }
    })
}

/// `import` or `mod` when the line opens with one, and whether a `?` follows it; `("", false)` for
/// any other line.
///
/// The word is cut at the first character a recipe name cannot hold, because `just` reads
/// `import"x"`, `import?"x"` and `mod?x` as directives, and a recipe named `important` or
/// `module-x` is not one. An indented or comment line starts with no such character, so no word.
fn keyword(line: &str) -> (&str, bool) {
    let cut = line
        .find(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '_' | '-')))
        .unwrap_or(line.len());
    let (word, rest) = line.split_at(cut);
    let optional = rest.starts_with('?');
    let spaced = rest.is_empty() || rest.starts_with([' ', '\t', '"', '\'']);
    if matches!(word, "import" | "mod") && (optional || spaced) {
        (word, optional)
    } else {
        ("", false)
    }
}

/// The imports of the root, refusing a `mod`, an `import?` and a bad path.
fn directives(file: &Path, text: &str) -> Result<Vec<ImportPath>, Refused> {
    let mut imports = Vec::new();
    for (index, raw) in text.lines().enumerate() {
        let line = index.saturating_add(1);
        let file = file.to_path_buf();
        match keyword(raw) {
            ("mod", _) => return Err(Refused::Module { file, line }),
            ("import", true) => return Err(Refused::OptionalImport { file, line }),
            ("import", false) => {
                let literal = raw
                    .strip_prefix("import")
                    .and_then(quoted)
                    .ok_or_else(|| Refused::Malformed {
                        file: file.clone(),
                        line,
                    })?;
                imports.push(ImportPath::parse(literal).ok_or(Refused::Outside { file, line })?);
            }
            _ => {}
        }
    }
    Ok(imports)
}

/// The text of the one quoted literal in `rest`, when only whitespace or a `#` comment follows it.
fn quoted(rest: &str) -> Option<&str> {
    let rest = rest.trim();
    let quote = rest.chars().next().filter(|quote| matches!(quote, '"' | '\''))?;
    let (literal, after) = rest.get(1..)?.split_once(quote)?;
    let after = after.trim();
    (!literal.contains('\\') && (after.is_empty() || after.starts_with('#'))).then_some(literal)
}

#[cfg(test)]
mod tests {
    use super::{Refused, Source, read};
    use crate::scratch_tree::Tree;

    fn tree(tag: &str, files: &[(&str, &str)]) -> Tree {
        let files: Vec<_> = files.iter().map(|&(path, text)| (path, text.as_bytes())).collect();
        Tree::of(&format!("justfiles-{tag}"), &files)
    }

    fn attempt(tag: &str, files: &[(&str, &str)]) -> Result<Vec<Source>, Refused> {
        read(tree(tag, files).root())
    }

    #[test]
    fn a_justfile_with_no_import_reads_as_that_file_alone() {
        let set = read(tree("alone", &[("justfile", "a:\n    echo a\n")]).root()).expect("one file");
        assert_eq!(set.len(), 1);
        assert_eq!(set[0].path.to_str(), Some("justfile"));
        assert_eq!(set[0].text, "a:\n    echo a\n");
    }

    #[test]
    fn the_root_and_each_import_are_returned_flat_in_order() {
        let root = "import \"just/gates.just\"\nimport'dev.just' # the rest\na:\n    echo import \"x\"\n";
        let set = read(
            tree(
                "flat",
                &[("justfile", root), ("just/gates.just", "g:\n"), ("dev.just", "d:\n")],
            )
            .root(),
        )
        .expect("three files");
        let paths: Vec<_> = set.iter().filter_map(|source| source.path.to_str()).collect();
        assert_eq!(paths, ["justfile", "just/gates.just", "dev.just"]);
        assert_eq!(set[1].text, "g:\n");
    }

    #[test]
    fn the_text_of_the_set_joins_its_files_with_a_newline() {
        let tree = tree(
            "text",
            &[("justfile", "import \"just/gates.just\"\na:\n"), ("just/gates.just", "g:\n")],
        );
        assert_eq!(
            super::text(tree.root()).expect("two files"),
            "import \"just/gates.just\"\na:\n\ng:\n"
        );
    }

    #[test]
    fn an_import_it_cannot_open_is_refused() {
        let found = attempt("unopenable", &[("justfile", "import \"gone.just\"\n")]).unwrap_err();
        assert!(
            matches!(&found, Refused::Unreadable { path, .. } if path.to_str() == Some("gone.just")),
            "{found}"
        );
    }

    #[test]
    fn a_missing_root_is_refused() {
        let found = attempt("no-root", &[("other", "")]).unwrap_err();
        assert!(
            matches!(&found, Refused::Unreadable { path, .. } if path.to_str() == Some("justfile")),
            "{found}"
        );
    }

    #[test]
    fn a_mod_line_is_refused() {
        for line in ["mod x", "mod? x", "mod?x"] {
            let found = attempt("mod", &[("justfile", &format!("a:\n{line}\n")), ("x.just", "")]).unwrap_err();
            assert!(
                matches!(&found, Refused::Module { file, line: 2 } if file.to_str() == Some("justfile")),
                "{line}: {found}"
            );
        }
    }

    #[test]
    fn an_optional_import_is_refused() {
        for line in ["import? \"x.just\"", "import?\"x.just\""] {
            let found = attempt("optional", &[("justfile", &format!("{line}\n")), ("x.just", "")]).unwrap_err();
            assert!(matches!(&found, Refused::OptionalImport { line: 1, .. }), "{line}: {found}");
        }
    }

    #[test]
    fn an_import_inside_an_imported_file_is_refused() {
        for line in ["import \"y.just\"", "import\"y.just\"", "import? \"y.just\""] {
            let found = attempt(
                "nested",
                &[
                    ("justfile", "import \"x.just\"\n"),
                    ("x.just", &format!("a:\n{line}\n")),
                    ("y.just", ""),
                ],
            )
            .unwrap_err();
            assert!(
                matches!(&found, Refused::NestedImport { file, line: 2 } | Refused::OptionalImport { file, line: 2 }
                    if file.to_str() == Some("x.just")),
                "{line}: {found}"
            );
        }
    }

    #[test]
    fn a_mod_inside_an_imported_file_is_refused() {
        let found = attempt("nested-mod", &[("justfile", "import \"x.just\"\n"), ("x.just", "mod y\n")]).unwrap_err();
        assert!(
            matches!(&found, Refused::Module { file, line: 1 } if file.to_str() == Some("x.just")),
            "{found}"
        );
    }

    #[test]
    fn an_import_outside_the_root_is_refused() {
        for path in ["../x.just", "/etc/x.just", "./x.just", "just/./x.just", "just//x.just", ""] {
            let found = attempt("outside", &[("justfile", &format!("import \"{path}\"\n"))]).unwrap_err();
            assert!(matches!(found, Refused::Outside { line: 1, .. }), "{path}: {found}");
        }
    }

    #[test]
    fn an_import_that_is_not_one_quoted_literal_is_refused() {
        for rest in ["x.just", "\"x.just", "\"a\\b\"", "\"x.just\" y", ""] {
            let found = attempt("malformed", &[("justfile", &format!("import {rest}\n"))]).unwrap_err();
            assert!(matches!(found, Refused::Malformed { line: 1, .. }), "{rest}: {found}");
        }
    }

    #[test]
    fn an_indented_import_and_a_recipe_named_like_one_are_not_directives() {
        let root = "important:\n    import \"nope\"\nmodule-x:\n    mod y\n# import \"z\"\n";
        let set = read(tree("not-directives", &[("justfile", root)]).root()).expect("no directive");
        assert_eq!(set.len(), 1);
    }
}
