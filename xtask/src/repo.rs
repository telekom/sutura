//! Shared plumbing for the repo gates: where the repo root is, which files to look at,
//! and how an ignore pattern is matched.
//!
//! It lives in its own module because two gates need it and a copy in each would be two
//! things to keep in step.

use std::path::{Path, PathBuf};

/// Directories no gate ever descends into: build output, VCS internals, tool caches.
const SKIP_DIRS: &[&str] = &[
    ".git",
    // A nested checkout of THIS repo. `git worktree` and agent tooling both put sibling working
    // trees under `.claude/worktrees/`, and a gate that walks into one judges another checkout's
    // files as if they were this one's: `max-lines` failed on vendored C inside a worktree, where
    // `devco/max-lines-ignore`'s `vendor/**` cannot reach because the path is prefixed. Matched by
    // NAME like every entry here, so a bare `worktrees/` is covered too.
    "worktrees",
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
    // The rendered docs site. Gitignored, so `git ls-files` never lists it - but the walk
    // fallback does, and mkdocs-material vendors a 6708-line lunr bundle that fails
    // `max-lines`. A developer who ran `just docs` could not then pass the gates, which is a
    // gate punishing someone for building the thing the gate exists to protect.
    "site",
];

/// Strip the git environment variables that would point a subprocess at another repository.
///
/// A gate that shells out to git is often invoked BY git - from a hook, or from inside a command
/// that set up its own index - and those variables outlive the process that set them. Two gates
/// need it, so it lives here rather than in either of them: `test-causality` builds a worktree and
/// `clean-branches` decides what may be deleted, and neither may read another checkout's state.
pub(crate) fn strip_git_env(command: &mut std::process::Command) {
    for name in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_INDEX_FILE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_COMMON_DIR",
    ] {
        command.env_remove(name);
    }
}

/// The workspace root, derived from this crate's manifest rather than from the current
/// directory - so a gate behaves the same whether it is invoked by a hook, by CI, or by
/// hand from a subdirectory.
pub(crate) fn root() -> Option<PathBuf> {
    // Runtime discovery FIRST, because the compile-time path is wrong for a binary that was
    // not run from where it was built - and it fails silently rather than loudly. `nix run
    // .#xtask -- text-hygiene` reported "ok - 0 text file(s) checked" and exit 0: the store
    // binary's `CARGO_MANIFEST_DIR` points into a build sandbox that no longer exists, the
    // tree walk found nothing, and a gate that checked nothing announced success. A false pass
    // is the worst outcome a gate has, so the path it depends on is now established by looking.
    //
    // Both markers, not either: `flake.nix` alone appears in unrelated directories and
    // `Cargo.toml` alone matches every crate on the way up. Together they identify this repo's
    // root and stop the walk at the workspace rather than at a member.
    if let Ok(mut dir) = std::env::current_dir() {
        loop {
            if dir.join("flake.nix").is_file() && dir.join("Cargo.toml").is_file() {
                return Some(dir);
            }
            if !dir.pop() {
                break;
            }
        }
    }

    // Fallback: the compile-time path, which is correct for `cargo run` from anywhere in the
    // workspace and for the Nix build sandbox, where the source IS the manifest's parent.
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
    // TRACKED plus UNTRACKED-BUT-NOT-IGNORED, which is what git would publish.
    //
    // Tracked-only was wrong in a way that is hard to see: a brand-new file is invisible until
    // it is staged, so a gate reports `ok` having checked less than it appears to. The only
    // tell was the file count in the verdict line. A hook path is unaffected - pre-commit
    // stages first - but `cargo xtask hygiene` on new work checked nothing of it.
    //
    // Two calls because they answer different questions. `--stage` carries the mode, which is
    // needed to skip a symlink: its content is a target path, so there are no line endings to
    // police and adding a final newline would break the link, and on a checkout without
    // symlink support only the index can say what it is. `--others --exclude-standard` has no
    // mode, but an untracked symlink is rare enough that the walk's own symlink check covers
    // it in the fallback path.
    let tracked = std::process::Command::new("git")
        .args(["ls-files", "--stage", "-z"])
        .current_dir(&root)
        .output();
    let untracked = std::process::Command::new("git")
        .args(["ls-files", "--others", "--exclude-standard", "-z"])
        .current_dir(&root)
        .output();

    if let Ok(tracked) = tracked
        && tracked.status.success()
    {
        let mut files: Vec<String> = tracked
            .stdout
            .split(|b| *b == 0)
            .filter(|raw| !raw.is_empty())
            .filter_map(|raw| staged_path(&String::from_utf8_lossy(raw)))
            .collect();

        if let Ok(untracked) = untracked
            && untracked.status.success()
        {
            files.extend(
                untracked
                    .stdout
                    .split(|b| *b == 0)
                    .filter(|raw| !raw.is_empty())
                    .map(|raw| String::from(String::from_utf8_lossy(raw))),
            );
        }

        files.sort_unstable();
        files.dedup();
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
    // A DIRECTORY THIS WALK CANNOT OPEN IS EMITTED AS A PATH, not skipped in silence.
    //
    // The `else { return; }` this replaces was `github.com/telekom/sutura#412`'s own defect one
    // level up, and it was measured in both directions: `chmod 000` on a directory is caught on
    // the git path, because git's index does not need to read it - and was INVISIBLE here, where
    // the listing simply lost the subtree. `flake.nix` gives `checks.hygiene` the whole tree with
    // no `.git`, so this fallback is the path the merge verdict comes from: the gate was strong in
    // the inner loop and blind exactly where it counted.
    //
    // Emitting the path rather than returning a `Result` is what keeps this a three-line fix:
    // `RepoFiles` is destructured by fourteen gates, several of them owned by other branches, so a
    // new field would be a mechanical edit across files this change has no business touching. A
    // consumer that reads its entries - `text-hygiene`, `line-endings` - now finds an in-scope path
    // it cannot read and says so; one that filters by extension never sees it.
    let Ok(entries) = std::fs::read_dir(dir) else {
        if let Some(rel) = relative(root, dir) {
            out.push(rel);
        }
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

/// The path from one `git ls-files --stage` entry, or `None` for a symlink.
///
/// The format is `<mode> <sha> <stage>\t<path>`. Mode 120000 is a symlink: skipped, because a
/// symlink has no content of its own to check.
fn staged_path(entry: &str) -> Option<String> {
    let (meta, path) = entry.split_once('\t')?;
    let mode = meta.split_whitespace().next()?;
    if mode == "120000" {
        return None;
    }
    Some(String::from(path))
}

/// Every text file under `dir`, as repo-relative paths.
///
/// The content-based sibling of [`collect_files`], for gates that should judge every text file
/// rather than a named set of extensions.
pub(crate) fn collect_text_files(root: &Path, dir: &Path, out: &mut Vec<String>) {
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
                collect_text_files(root, &path, out);
            }
            continue;
        }
        if is_text_file(&path)
            && let Some(rel) = relative(root, &path)
        {
            out.push(rel);
        }
    }
}

/// Paths that are SYMLINKS in the index but may be pointer files on disk.
///
/// git stores mode 120000 for these. On a platform without symlink support - or with
/// `core.symlinks=false` - it writes a small text file containing the target path instead, with
/// no trailing newline, because a symlink target has none.
///
/// Where `.git` is available a gate skips them by reading the index. Where it is NOT - the Nix
/// build sandbox, and Docker's `COPY . .` - a gate walks the tree, sees a 17-byte regular file
/// and fails it for a missing final newline. That is why the Dockerfile's `build` target could
/// never succeed from a Windows checkout, and why `checks.hygiene` failed in the sandbox.
///
/// Skipping by path rather than by content: "a tiny file whose text happens to be a relative
/// path" is not a shape worth guessing at, and `cargo xtask check-skills` still verifies these
/// are mode 120000 wherever git can be asked - so the invariant keeps a mechanism rather than
/// becoming an exemption.
pub(crate) const INDEX_SYMLINKS: &[&str] = &[".claude/skills", ".codex/skills", ".opencode/skills"];

/// Is this path one of the index symlinks above?
pub(crate) fn is_index_symlink(path: &str) -> bool {
    INDEX_SYMLINKS.contains(&path)
}

/// How much of a file to inspect before deciding whether it is text.
///
/// A NUL in the first few KiB is the standard binary heuristic and is what git itself uses.
/// Bounded so a large file costs a single read rather than a full decode.
const SNIFF_BYTES: usize = 8 * 1024;

/// What a look at a file's first bytes concluded.
///
/// **Three answers rather than two, and the third is the whole point.** `is_text_file` collapses
/// *this is not text* and *I could not look* into one `false`, so a gate that skips the first
/// silently skips the second as well - and then reports a count one lower with nothing to compare
/// it against. `github.com/telekom/sutura#412` is that defect in `text-hygiene`, and the trap the
/// fix has to avoid is the one `check-shipped-binaries` hit: a PNG under a documentation glob is
/// OUT OF SCOPE, not unreadable, and a gate that reddens on it gets switched off. Separating the
/// two is what lets a caller refuse the second while still ignoring the first.
#[derive(Debug)]
pub(crate) enum TextProbe {
    /// No NUL byte in the sniffed prefix, and that prefix decodes as UTF-8.
    Text,
    /// Read, and not text. Out of scope for a text gate rather than a problem with it.
    Binary,
    /// In scope and unanswerable: the file could not be opened, or the read failed. The caller
    /// decides what that means - `std::io::ErrorKind::NotFound` is a listing that disagrees with
    /// the working tree, which is not the same as a file that is there and will not be read.
    Unreadable(std::io::Error),
}

/// Is this file text?
///
/// Decided by CONTENT, not by an extension list. Three gates each carried their own list of 14,
/// 14 and 11 extensions, and a file with no extension and no leading dot was invisible to all
/// three - which meant `justfile` and `Dockerfile`, the two files most likely to reintroduce
/// the CRLF-in-a-shell-string bug the line-endings gate exists for, were exactly the two it
/// could not see. Content-based detection has no list to forget: a new file type is covered
/// the day it appears.
///
/// Text means no NUL byte in the first [`SNIFF_BYTES`] and that prefix decodes as UTF-8.
///
/// **The boolean loses which of the two other answers it gave**, so a caller that must not drop an
/// unreadable file in silence wants [`probe_text`] instead. This form stays for the callers whose
/// verdict genuinely does not depend on the difference.
pub(crate) fn is_text_file(path: &Path) -> bool {
    matches!(probe_text(path), TextProbe::Text)
}

/// [`is_text_file`], with *could not look* kept apart from *not text*.
///
/// Two steps, because they answer different questions: the read either happened or it did not,
/// and only if it did is there anything to classify. Splitting them is what makes the OS error
/// available to the caller instead of collapsing into a `false`.
pub(crate) fn probe_text(path: &Path) -> TextProbe {
    match sniff(path) {
        Ok(head) => classify(&head),
        Err(cause) => TextProbe::Unreadable(cause),
    }
}

/// The first [`SNIFF_BYTES`] of a file, or why they could not be had.
fn sniff(path: &Path) -> std::io::Result<Vec<u8>> {
    use std::io::Read as _;

    let mut file = std::fs::File::open(path)?;
    let mut head = vec![0_u8; SNIFF_BYTES];
    let read = file.read(&mut head)?;
    head.truncate(read);
    Ok(head)
}

/// Text or binary, from bytes that were read.
fn classify(head: &[u8]) -> TextProbe {
    if head.contains(&0) {
        return TextProbe::Binary;
    }
    // A multi-byte character can straddle the cutoff, so an incomplete tail is not evidence of
    // binary. Only an error before the last few bytes is.
    match std::str::from_utf8(head) {
        Ok(_) => TextProbe::Text,
        Err(e) if e.valid_up_to() + 4 >= head.len() => TextProbe::Text,
        Err(_) => TextProbe::Binary,
    }
}

/// A repo-relative path with forward slashes, so patterns are written once and match on
/// every platform.
pub(crate) fn relative(root: &Path, path: &Path) -> Option<String> {
    path.strip_prefix(root)
        .ok()
        .map(|rel| rel.to_string_lossy().replace(std::path::MAIN_SEPARATOR, "/"))
}

/// Does any of `patterns` match `path`?
///
/// Here rather than in each caller because every path-scoped gate wants the plural form and three
/// of them had grown their own name for it.
pub(crate) fn matches_any(patterns: &[&str], path: &str) -> bool {
    patterns.iter().any(|pattern| matches(pattern, path))
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
    fn a_symlink_entry_is_skipped_but_a_regular_file_is_not() {
        use super::staged_path;
        // A symlink's content is its target: no line endings to police, and adding a final
        // newline would break the link. The working tree cannot show this on a checkout
        // without symlink support, so the index mode is the only source.
        assert_eq!(staged_path("120000 abc123 0\t.claude/skills"), None);
        assert_eq!(staged_path("100644 abc123 0\tjustfile").as_deref(), Some("justfile"));
        // An executable file is still a file.
        assert_eq!(
            staged_path("100755 abc123 0\thooks/thing.sh").as_deref(),
            Some("hooks/thing.sh")
        );
        assert_eq!(staged_path("malformed"), None);
    }

    #[test]
    fn text_detection_is_by_content_not_extension() {
        use std::io::Write as _;

        let dir = std::env::temp_dir().join("sutura-is-text-test");
        drop(std::fs::create_dir_all(&dir));

        // No extension at all - the case three extension lists all missed.
        let extensionless = dir.join("justfile");
        let mut f = std::fs::File::create(&extensionless).expect("create");
        f.write_all(b"default:\n    echo hi\n").expect("write");
        assert!(super::is_text_file(&extensionless));

        // A NUL byte makes it binary whatever it is called.
        let fake_text = dir.join("looks-like.md");
        let mut f = std::fs::File::create(&fake_text).expect("create");
        f.write_all(b"header\x00\x01\x02binary").expect("write");
        assert!(!super::is_text_file(&fake_text));

        // Multi-byte UTF-8 is text.
        let utf8 = dir.join("utf8.txt");
        let mut f = std::fs::File::create(&utf8).expect("create");
        f.write_all("a non-ASCII character: \u{00e4}\n".as_bytes()).expect("write");
        assert!(super::is_text_file(&utf8));

        // A path that does not exist is not text: nothing can be said about it.
        assert!(!super::is_text_file(&dir.join("absent")));

        drop(std::fs::remove_dir_all(&dir));
    }

    #[test]
    fn a_probe_keeps_could_not_look_apart_from_not_text() {
        use std::io::Write as _;

        use super::TextProbe;

        // THE distinction `github.com/telekom/sutura#412` is about. `is_text_file` answers `false`
        // to both, so a gate that skips a binary file also skips one it could not open - and says
        // nothing about either. Keyed on the process id, and removed first, for the reason
        // `crate::falsifier` gives at its own constructor: a pid is reusable.
        let dir = std::env::temp_dir().join(format!("sutura-probe-{}", std::process::id()));
        drop(std::fs::remove_dir_all(&dir));
        std::fs::create_dir_all(&dir).expect("a scratch directory");

        let text = dir.join("prose.md");
        std::fs::write(&text, "a line\n").expect("write");
        assert!(matches!(super::probe_text(&text), TextProbe::Text));

        // Out of scope, and it must STAY out of scope: `check-shipped-binaries` reddened a correct
        // tree by treating a PNG under a documentation glob as a failure to read.
        let binary = dir.join("favicon.png");
        let mut f = std::fs::File::create(&binary).expect("create");
        f.write_all(b"\x89PNG\r\n\x1a\n\x00\x00").expect("write");
        assert!(matches!(super::probe_text(&binary), TextProbe::Binary));

        // In scope and unanswerable. A DIRECTORY rather than a mode-000 file: `open` succeeds on
        // unix and the read fails with `EISDIR`, which no privilege level changes - where mode bits
        // deny nobody when the process is root, so a permission fixture would assert nothing there.
        let opaque = dir.join("a-directory");
        std::fs::create_dir_all(&opaque).expect("a directory in the path of a file read");
        assert!(
            matches!(super::probe_text(&opaque), TextProbe::Unreadable(_)),
            "a path that cannot be read as a file is neither text nor binary"
        );

        // Absent is Unreadable too, and the KIND is what a caller separates on: a listing that
        // names a file the working tree does not have is git's business, not a text gate's.
        match super::probe_text(&dir.join("absent")) {
            TextProbe::Unreadable(cause) => assert_eq!(cause.kind(), std::io::ErrorKind::NotFound),
            other => panic!("an absent path must be unreadable, not {other:?}"),
        }

        drop(std::fs::remove_dir_all(&dir));
    }

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

    #[cfg(unix)]
    #[test]
    fn a_directory_the_walk_cannot_open_is_emitted_rather_than_dropped() {
        // THE RESIDUAL `github.com/telekom/sutura#412` left open, closed. `collect_all` used to
        // `return` on an unreadable directory, so the whole subtree left the listing with nothing
        // said - and because `checks.hygiene` runs this fallback (whole tree, no `.git`), that was
        // the blind spot on the path the merge verdict comes from. Measured before the fix: a
        // mode-000 directory gave `ok - 4 text file(s) checked; 4 of 4 listed path(s) accounted
        // for`, exit 0, with a real trailing-whitespace finding inside it hidden.
        //
        // Mode bits ARE the fixture here, unlike the gates' own tests, because there is no
        // portable way to make `read_dir` fail on a directory otherwise - so this asserts only
        // that the walk did not lose the path, and skips when the mode denies nobody (a root
        // process), which is stated rather than silently passing.
        let root = std::env::temp_dir().join(format!("sutura-opaque-dir-{}", std::process::id()));
        let _cleanup = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("open")).expect("a readable directory");
        std::fs::write(root.join("open/kept.md"), "a line\n").expect("a file inside it");
        let shut = root.join("shut");
        std::fs::create_dir_all(&shut).expect("a directory to close");
        std::fs::write(shut.join("hidden.md"), "a line\n").expect("a file inside it");

        std::fs::set_permissions(&shut, std::os::unix::fs::PermissionsExt::from_mode(0o000)).expect("close it");
        let denied = std::fs::read_dir(&shut).is_err();
        let mut found = Vec::new();
        super::collect_all(&root, &root, &mut found);
        std::fs::set_permissions(&shut, std::os::unix::fs::PermissionsExt::from_mode(0o755)).expect("reopen it");
        let _swept = std::fs::remove_dir_all(&root);

        if !denied {
            // A privileged process is not denied by a mode bit, so there is nothing to observe.
            // Said out loud rather than passed quietly, because a fixture that stops reproducing
            // is a test that has become decoration.
            return;
        }
        assert!(found.contains(&String::from("open/kept.md")), "{found:?}");
        assert!(
            found.contains(&String::from("shut")),
            "the unreadable directory has to reach the listing, or the subtree leaves it in \
             silence and the count is the only tell: {found:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_nested_worktree_is_not_walked_into() {
        // THE case this closes: `git worktree` and agent tooling put sibling checkouts of this
        // repo under `.claude/worktrees/`, and the walk judged their files as this checkout's.
        // `max-lines` failed on vendored mimalloc C inside one, which `devco/max-lines-ignore`
        // exempts as `vendor/**` - a pattern that cannot match `.claude/worktrees/x/vendor/...`.
        let root = std::env::temp_dir().join(format!("sutura-walk-{}", std::process::id()));
        let _cleanup = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("crates/thing/src")).expect("a scratch tree");
        std::fs::create_dir_all(root.join(".claude/worktrees/agent-x/crates/thing/src")).expect("a nested checkout");
        std::fs::write(root.join("crates/thing/src/lib.rs"), "// ours\n").expect("our file");
        std::fs::write(
            root.join(".claude/worktrees/agent-x/crates/thing/src/lib.rs"),
            "// another checkout's\n",
        )
        .expect("their file");

        let mut found = Vec::new();
        super::collect_files(&root, &root, &["rs"], &mut found);

        let _swept = std::fs::remove_dir_all(&root);
        assert_eq!(found, vec![String::from("crates/thing/src/lib.rs")], "{found:?}");
    }
}
