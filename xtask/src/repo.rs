//! Shared plumbing for the repo gates: where the repo root is, which files to look at,
//! and how an ignore pattern is matched.
//!
//! It lives in its own module because two gates need it and a copy in each would be two
//! things to keep in step.

use std::path::{Path, PathBuf};

mod census;

pub(crate) use census::{Census, Looked, Refusal, Unmigrated};

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

/// What a walk is looking for. One enum rather than three copies of the walk: the swallowed
/// `read_dir` error below was written three times, at `:88`, `:194` and `:236`, and a defect with
/// three homes is a defect that gets fixed in two of them.
enum Wanted<'a> {
    /// An extension in this list. The `collect_files` door.
    Extensions(&'a [&'a str]),
    /// Every file. The `all_files` walk fallback.
    Everything,
    /// Text, decided by content. The `collect_text_files` door.
    ///
    /// **A limit, marked rather than fixed here:** [`is_text_file`] returns `false` for a file it
    /// cannot open, so an unreadable file is dropped as *not text* rather than recorded as
    /// unreachable. That is one root cause behind three gates and it changes all three at once, so
    /// it is the next-but-one PR in `github.com/telekom/sutura#414`'s stack, not this one.
    Text,
}

impl Wanted<'_> {
    fn holds(&self, path: &Path) -> bool {
        match self {
            Self::Extensions(extensions) => path
                .extension()
                .and_then(std::ffi::OsStr::to_str)
                .is_some_and(|ext| extensions.contains(&ext)),
            Self::Everything => true,
            Self::Text => is_text_file(path),
        }
    }
}

/// Every file under `dir` whose extension is in `extensions`, as a [`Census`].
///
/// Symlinks are skipped: following them can leave the repo or loop.
pub(crate) fn collect_files(root: &Path, dir: &Path, extensions: &[&str]) -> Census {
    gather(root, dir, &Wanted::Extensions(extensions))
}

/// Every text file under `dir`, as a [`Census`].
///
/// The content-based sibling of [`collect_files`], for gates that should judge every text file
/// rather than a named set of extensions.
pub(crate) fn collect_text_files(root: &Path, dir: &Path) -> Census {
    gather(root, dir, &Wanted::Text)
}

/// Walk, and hand back what was found together with what could not be reached.
fn gather(root: &Path, dir: &Path, wanted: &Wanted<'_>) -> Census {
    let mut found = Vec::new();
    let mut unreachable = Vec::new();
    walk(root, dir, wanted, &mut found, &mut unreachable);
    Census::found(root.to_path_buf(), found, unreachable)
}

/// The one tree walk, and the one place an `fs` error on the way down is recorded.
///
/// **Every error here used to be dropped** - `let Ok(entries) = read_dir(dir) else { return; }`
/// followed by `entries.flatten()` and `let Ok(file_type) = .. else { continue; }`, three times
/// over. Dropping one takes a subtree out of the DENOMINATOR as well as out of the scan, so every
/// count a gate prints afterwards agrees with itself over a tree it never looked at: measured,
/// `chmod 000 .github/actions` produced `ok - 2 literal(s) across 8 file(s)` at exit 0.
///
/// `NotFound` versus anything else, the split [`is_text_file`]'s neighbour `read_to_string` calls
/// already make: a directory that is absent was never a subject, while a directory that exists and
/// will not be read is a subject this walk was meant to reach and could not.
fn walk(root: &Path, dir: &Path, wanted: &Wanted<'_>, found: &mut Vec<String>, unreachable: &mut Vec<String>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        // Absent is not unreachable. A door pointed at a directory this tree does not have -
        // `docs/` in a checkout without it - discovers nothing, and the census's own empty-set
        // refusal is what decides whether that is acceptable for the gate asking.
        Err(why) if why.kind() == std::io::ErrorKind::NotFound => return,
        Err(why) => {
            unreachable.push(format!("{}: {why}", shown(root, dir)));
            return;
        }
    };
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            // The `.flatten()` that used to be here. A `DirEntry` error means this directory holds
            // something the walk cannot name, so the listing is short by an unknown amount.
            Err(why) => {
                unreachable.push(format!("{}: {why}", shown(root, dir)));
                continue;
            }
        };
        let path = entry.path();
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(why) => {
                unreachable.push(format!("{}: {why}", shown(root, &path)));
                continue;
            }
        };
        if file_type.is_symlink() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if file_type.is_dir() {
            if !SKIP_DIRS.contains(&name.as_str()) {
                walk(root, &path, wanted, found, unreachable);
            }
            continue;
        }
        if wanted.holds(&path)
            && let Some(rel) = relative(root, &path)
        {
            found.push(rel);
        }
    }
}

/// A path as a reader should see it: repo-relative where that is possible, absolute otherwise.
fn shown(root: &Path, path: &Path) -> String {
    relative(root, path).unwrap_or_else(|| path.display().to_string())
}

/// Every file in the repo, as a [`Census`] carrying the root to join a path to.
///
/// Prefers `git ls-files` because tracked-only is the set a gate should judge: a build
/// artefact somebody left lying around is not a repo problem. Falls back to walking the
/// tree when git is unavailable or this is not a git checkout - which is exactly the case
/// inside the Nix build sandbox, where the flake source is present but `.git` is not.
///
/// The fallback still scans EVERYTHING, so it cannot turn a gate into a no-op; it can only
/// be more inclusive than the git listing. Returning an empty list on a missing git would
/// have made every gate pass vacuously in the sandbox, which is the failure mode a gate
/// exists to prevent - and [`Refusal::Empty`] is what now says so out loud instead.
///
/// **Where the unreachable-subject finding comes from on this path, stated because it reads
/// stronger than it is.** `git ls-files` reads the INDEX, so it lists a file inside a directory
/// nothing can open and the walk's own finding is empty. On a git checkout the finding therefore
/// arrives from the gate's own read - `Looked::Unreachable`, refused by [`Census::inspect`] - and
/// the walk's finding covers the sandbox fallback and the two directory-scoped doors above.
pub(crate) fn all_files() -> Result<Census, Refusal> {
    let Some(root) = root() else {
        return Err(Refusal::NoRoot);
    };
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
            return Ok(Census::found(root, files, Vec::new()));
        }
    }
    Ok(gather(&root, &root, &Wanted::Everything))
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
/// **A file that cannot be read answers `false` here, and that is a known fail-open** - it drops
/// out of the walk as *not text* rather than being recorded as unreachable. Three gates share that
/// root cause and change together; see [`Wanted::Text`].
pub(crate) fn is_text_file(path: &Path) -> bool {
    use std::io::Read as _;

    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    let mut head = vec![0_u8; SNIFF_BYTES];
    let Ok(read) = file.read(&mut head) else {
        return false;
    };
    head.truncate(read);

    if head.contains(&0) {
        return false;
    }
    // A multi-byte character can straddle the cutoff, so an incomplete tail is not evidence of
    // binary. Only an error before the last few bytes is.
    match std::str::from_utf8(&head) {
        Ok(_) => true,
        Err(e) => e.valid_up_to() + 4 >= head.len(),
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

    #[test]
    fn a_directory_the_walk_cannot_open_is_a_refusal_rather_than_a_shorter_list() {
        // THE defect, and deterministic on every platform: `read_dir` on a regular file fails with
        // something other than `NotFound`, which is exactly the class `chmod 000` produces. It used
        // to `return` in silence, so the subtree left the DENOMINATOR as well as the scan and every
        // count printed afterwards agreed with itself over a tree the walk never looked at.
        let root = std::env::temp_dir().join(format!("sutura-unreachable-{}", std::process::id()));
        let _cleanup = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("a scratch tree");
        let not_a_directory = root.join("regular.txt");
        std::fs::write(&not_a_directory, "not a directory\n").expect("a regular file");

        let refused = super::collect_files(&root, &not_a_directory, &["rs"]).inspect(&[], |_| super::Looked::Judged);

        let _swept = std::fs::remove_dir_all(&root);
        let Err(why) = refused else {
            panic!("a directory that cannot be opened produced a verdict");
        };
        assert!(
            why.describe().contains("regular.txt"),
            "the refusal has to name what it could not reach: {}",
            why.describe()
        );
    }

    #[cfg(unix)]
    #[test]
    fn an_unreadable_subtree_refuses_instead_of_shrinking_the_walk() {
        // The measured instance, seeded the way it was measured. **Self-skips where mode bits are
        // ignored** - uid 0 reads a 0000 directory - so the deterministic test above is the one
        // that holds this arm in every venue, and this one holds the SHAPE the issue reported.
        use std::os::unix::fs::PermissionsExt as _;

        let root = std::env::temp_dir().join(format!("sutura-chmod-{}", std::process::id()));
        let _cleanup = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("open/inner")).expect("a scratch tree");
        std::fs::create_dir_all(root.join("shut")).expect("a subtree to close");
        std::fs::write(root.join("open/inner/a.rs"), "// reachable\n").expect("a reachable file");
        std::fs::write(root.join("shut/b.rs"), "// unreachable\n").expect("a file behind it");
        std::fs::set_permissions(root.join("shut"), std::fs::Permissions::from_mode(0o000)).expect("chmod");

        let took_effect = std::fs::read_dir(root.join("shut")).is_err();
        let refused = super::collect_files(&root, &root, &["rs"]).inspect(&[], |_| super::Looked::Judged);

        drop(std::fs::set_permissions(root.join("shut"), std::fs::Permissions::from_mode(0o755)));
        let _swept = std::fs::remove_dir_all(&root);

        if !took_effect {
            // Running as a user the mode cannot stop. Nothing to assert, and saying so beats an
            // assertion that would be vacuous.
            return;
        }
        let Err(why) = refused else {
            panic!("an unreadable subtree produced a verdict over the rest of the tree");
        };
        assert!(why.describe().contains("shut"), "{}", why.describe());
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
        super::collect_files(&root, &root, &["rs"])
            .inspect(&[], |rel| {
                found.push(String::from(rel));
                super::Looked::Judged
            })
            .expect("the scratch tree holds one .rs file and nothing unreadable");

        let _swept = std::fs::remove_dir_all(&root);
        assert_eq!(found, vec![String::from("crates/thing/src/lib.rs")], "{found:?}");
    }
}
