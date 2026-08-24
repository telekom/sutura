//! Shared plumbing for the repo gates: where the repo root is, which files to look at,
//! and how an ignore pattern is matched.
//!
//! It lives in its own module because two gates need it and a copy in each would be two
//! things to keep in step.

use std::path::{Path, PathBuf};

/// Directories no gate ever descends into: build output, VCS internals, tool caches.
const SKIP_DIRS: &[&str] = &[
    ".git",
    // Written by crane inside the Nix build sandbox, not by us. The walk fallback would
    // otherwise judge a generated cargo config as if it were a repo file - which failed CI
    // once, since git never listed it and only the fallback can see it.
    ".cargo-home",
    ".devenv",
    ".direnv",
    ".pixi",
    "target",
    "result",
    "node_modules",
    "__pycache__",
];

/// The workspace root, derived from this crate's manifest rather than from the current
/// directory - so a gate behaves the same whether it is invoked by a hook, by CI, or by
/// hand from a subdirectory.
pub(crate) fn root() -> Option<PathBuf> {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().map(Path::to_path_buf)
}

/// Every file under `dir` whose extension is in `extensions`, as repo-relative paths with
/// `/` separators. Symlinks are skipped: following them can leave the repo or loop.
pub(crate) fn collect_files(root: &Path, dir: &Path, extensions: &[&str], out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_symlink() {
            continue;
        }
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if file_type.is_dir() {
            if !SKIP_DIRS.contains(&name.as_str()) {
                collect_files(root, &path, extensions, out);
            }
            continue;
        }
        let matches_extension = path
            .extension()
            .and_then(std::ffi::OsStr::to_str)
            .is_some_and(|ext| extensions.contains(&ext));
        if matches_extension && let Some(rel) = relative(root, &path) {
            out.push(rel);
        }
    }
}

/// Every file in the repo, as repo-relative paths, paired with the root to join them to.
///
/// Prefers `git ls-files` because tracked-only is the set a gate should judge: a build
/// artefact somebody left lying around is not a repo problem. Falls back to walking the
/// tree when git is unavailable or this is not a git checkout - which is exactly the case
/// inside the Nix build sandbox, where the flake source is present but `.git` is not.
///
/// The fallback still scans EVERYTHING, so it cannot turn a gate into a no-op; it can only
/// be more inclusive than the git listing. Returning an empty list on a missing git would
/// have made every gate pass vacuously in the sandbox, which is the failure mode a gate
/// exists to prevent.
pub(crate) struct RepoFiles {
    /// Absolute repo root; join it to a `files` entry to read one.
    pub(crate) root: PathBuf,
    /// Repo-relative paths with `/` separators.
    pub(crate) files: Vec<String>,
}

pub(crate) fn all_files() -> Option<RepoFiles> {
    let root = root()?;
    if let Ok(out) = std::process::Command::new("git")
        .args(["ls-files", "-z"])
        .current_dir(&root)
        .output()
        && out.status.success()
    {
        let files: Vec<String> = out
            .stdout
            .split(|b| *b == 0)
            .filter(|raw| !raw.is_empty())
            .map(|raw| String::from(String::from_utf8_lossy(raw)))
            .collect();
        if !files.is_empty() {
            return Some(RepoFiles { root, files });
        }
    }
    let mut files = Vec::new();
    collect_all(&root, &root, &mut files);
    Some(RepoFiles { root, files })
}

/// The extension-agnostic sibling of [`collect_files`], for gates that decide what is text
/// by their own rules rather than by a fixed extension list.
fn collect_all(root: &Path, dir: &Path, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_symlink() {
            continue;
        }
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if file_type.is_dir() {
            if !SKIP_DIRS.contains(&name.as_str()) {
                collect_all(root, &path, out);
            }
            continue;
        }
        if let Some(rel) = relative(root, &path) {
            out.push(rel);
        }
    }
}

/// A repo-relative path with forward slashes, so patterns are written once and match on
/// every platform.
pub(crate) fn relative(root: &Path, path: &Path) -> Option<String> {
    path.strip_prefix(root)
        .ok()
        .map(|rel| rel.to_string_lossy().replace(std::path::MAIN_SEPARATOR, "/"))
}

/// Does `path` match `pattern`?
///
/// `*` matches within one path segment, `**` matches across segments, `?` matches one
/// character. A pattern with no `/` also matches a bare file name at any depth, so
/// `Cargo.lock` covers the nested ones too.
pub(crate) fn matches(pattern: &str, path: &str) -> bool {
    let pattern_chars: Vec<char> = pattern.chars().collect();
    let path_chars: Vec<char> = path.chars().collect();
    if glob(&pattern_chars, &path_chars) {
        return true;
    }
    if pattern.contains('/') {
        return false;
    }
    let base: Vec<char> = path.rsplit('/').next().unwrap_or(path).chars().collect();
    glob(&pattern_chars, &base)
}

/// Glob match over char slices. Slices are walked with `split_first`, never indexed, so
/// this compiles under the workspace's `indexing_slicing` ban.
fn glob(pattern: &[char], path: &[char]) -> bool {
    match pattern.split_first() {
        None => path.is_empty(),
        Some((&'*', rest)) => match rest.split_first() {
            Some((&'*', after)) => glob_double_star(pattern, after, path),
            _ => glob_star(pattern, rest, path),
        },
        Some((&'?', rest)) => match path.split_first() {
            Some((&c, tail)) if c != '/' => glob(rest, tail),
            _ => false,
        },
        Some((&expected, rest)) => match path.split_first() {
            Some((&c, tail)) if c == expected => glob(rest, tail),
            _ => false,
        },
    }
}

/// `**` - consumes any number of characters, path separators included. `**/rest` also
/// matches `rest` with no leading directory, which is what makes `**/x.json` cover a
/// root-level `x.json`.
fn glob_double_star(pattern: &[char], after: &[char], path: &[char]) -> bool {
    if let Some((&'/', tail)) = after.split_first()
        && glob(tail, path)
    {
        return true;
    }
    if glob(after, path) {
        return true;
    }
    match path.split_first() {
        Some((_, tail)) => glob(pattern, tail),
        None => false,
    }
}

/// `*` - consumes any number of characters except a path separator.
fn glob_star(pattern: &[char], rest: &[char], path: &[char]) -> bool {
    if glob(rest, path) {
        return true;
    }
    match path.split_first() {
        Some((&c, tail)) if c != '/' => glob(pattern, tail),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::matches;

    #[test]
    fn literal_and_basename() {
        assert!(matches("Cargo.lock", "Cargo.lock"));
        assert!(matches("Cargo.lock", "crates/x/Cargo.lock"));
        assert!(!matches("docs/Cargo.lock", "crates/x/Cargo.lock"));
    }

    #[test]
    fn single_star_stays_inside_one_segment() {
        assert!(matches("docs/*.md", "docs/a.md"));
        assert!(!matches("docs/*.md", "docs/nested/a.md"));
    }

    #[test]
    fn double_star_crosses_segments() {
        assert!(matches("docs/**/*.json", "docs/a/b/c.json"));
        assert!(matches("**/generated/*", "docs/generated/schema.json"));
        assert!(matches("**/x.json", "x.json"));
    }

    #[test]
    fn question_mark_matches_one_char() {
        assert!(matches("a?.md", "ab.md"));
        assert!(!matches("a?.md", "abc.md"));
        assert!(!matches("a?.md", "a/.md"));
    }

    #[test]
    fn trailing_star_covers_a_directory() {
        assert!(matches("docs/generated/*", "docs/generated/openapi.json"));
        assert!(!matches("docs/generated/*", "docs/adr/0001.md"));
    }
}
