//! Where a machine-shared filesystem root is TAKEN, and whose key reaches the path it builds.
//!
//! **The unit is a TAKING, not a line and not a file**, for `crate::warm_start::pairing`'s reason:
//! the transferable claim is about the act - acquiring a root every checkout on the machine can
//! reach - and two takings written on one line are two.
//!
//! # Why this lexes rather than greps
//!
//! `sutura/gates` records three gates that counted where they should have lexed. Rust here is read
//! through [`crate::serde_parse::scan`], which is this workspace's one answer to *what is code in
//! a Rust file*: comments go, and so does the interior of any string that SPANS LINES. Both halves
//! are load-bearing.
//!
//! * A doc comment is not a taking. `dev/src/discovery.rs` and `dev/src/provisioned.rs` each carry
//!   a doctest that hands the shared root to `Scope::from_root` **to prove nothing is provisioned
//!   there**; those are prose to this gate, which is correct, and it is also the limit - a doctest
//!   that really did write to a shared path is invisible here.
//! * A multi-line string is not code, which is what makes this module's own fixtures invisible to
//!   it. Every fixture below is therefore written as a MULTI-LINE literal on purpose, and the two
//!   root spellings are assembled with `concat!` for the same reason: a single-line literal IS a
//!   live anchor - `sutura/gates` states that limit for all three of this workspace's lexers - so a
//!   gate naming its own needle in one would report itself.
//!
//! # The two classes, and why only one of them can be judged from a line
//!
//! A **root taking** is unambiguous: `std::env::temp_dir()` acquires the machine's shared temporary
//! directory, so whatever it is narrowed by is the whole of what separates two checkouts.
//!
//! A **literal taking** - a path spelled `/tmp/...` - is not, and the difference was measured
//! before this gate was written: every one of the rooted literals in this workspace is a FIXTURE
//! VALUE that never reaches a filesystem (`Isolated::for_a_wiring_test(Path::new("/tmp/root"), ..)`,
//! `CredentialFile::at(..)`, two `Principals::of(..)` arguments, and this module's own root list).
//! The gate's own verdict is the count, so no number is written here - `cargo xtask
//! check-worktree-state` prints it beside the other holders. A rule that reddened all of them would
//! redden correct work, and a gate that reddens correct work gets disabled. So
//! the literal class is READ rather than assumed: [`Keyed::Unwritten`] is the answer for a literal
//! whose statement names no filesystem mutation, and it is a reading of the statement rather than a
//! declaration somebody wrote. What it does not reach is a mutation laundered into a helper, which
//! is stated at that variant.

use std::collections::BTreeMap;

/// Everything below a taking's root, as far as this gate reads it, capped so a missing terminator
/// cannot walk to the end of the file.
///
/// A statement in this workspace fits inside this; the cap exists because an unbalanced delimiter
/// must be a bounded read rather than a whole-file one.
const STATEMENT_LINES: usize = 40;

/// A language this gate lexes, and the whole scope decision.
///
/// An enum rather than a `bool` or an extension string, so [`Language::roots`] and the dispatch in
/// [`takings`] both have to answer for every language rather than inherit a default - the shape
/// `telekom/sutura#405` names as property 5.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Language {
    /// First-party Rust: `crates/`, `xtask/` and `dev/`.
    Rust,
    /// A gate script that runs on the developer's own machine: `nix/*.sh`.
    Shell,
}

impl Language {
    /// The word the verdict prints.
    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::Shell => "shell",
        }
    }
}

/// Whose key reaches the path a taking builds.
///
/// **Closed, and every decision over it is an exhaustive `match` with no wildcard arm** - which is
/// the mechanism `telekom/sutura#405` asks for by name: a sibling gate reopened its own measured
/// hole because a `matches!` inherited `false` for a variant added later, and it compiled at exit
/// 0. A fifth answer here does not compile until [`Keyed::is_shared`] and [`Keyed::holder`] both
/// say what it means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Keyed {
    /// This worktree's own key reaches it: the segment names the scope derivation - a digest, a
    /// `Scope`, a state directory or a scratch path.
    Worktree,
    /// Only this process can reach it: the segment names the process id, or a `mktemp`-shaped
    /// allocation. Unstable across runs and therefore useless as a cache, which is why it is a
    /// second answer rather than the only one.
    Process,
    /// A machine-shared path in a statement that touches no filesystem - a fixture value, an
    /// assertion, a constant this gate reads its own needles out of.
    ///
    /// **The limit, next to the claim:** the read is over the STATEMENT, so a mutation laundered
    /// into a helper called with this value is invisible here. It is the honest half of a question
    /// a line scan cannot answer, and the alternative measured worse - see the module header.
    Unwritten,
    /// Nothing keys it. Two worktrees of this repository both reach this path.
    Shared,
}

impl Keyed {
    /// Two checkouts on one machine reach this path.
    pub(super) const fn is_shared(self) -> bool {
        match self {
            Self::Shared => true,
            Self::Worktree | Self::Process | Self::Unwritten => false,
        }
    }

    /// What the verdict calls the holder, one word per answer.
    pub(super) const fn holder(self) -> &'static str {
        match self {
            Self::Worktree => "worktree",
            Self::Process => "process",
            Self::Unwritten => "unwritten",
            Self::Shared => "nothing",
        }
    }
}

/// One acquisition of a machine-shared filesystem root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Taking {
    /// Repo-relative file.
    pub(super) path: String,
    /// 1-based line, so a verdict names the line a reader opens.
    pub(super) line: usize,
    /// The root that was taken, as written.
    pub(super) root: String,
    /// Everything below the root that this gate could read: the first path segment for a root
    /// taking, the whole literal for a literal one. Empty when a taking narrows to nothing at all.
    pub(super) segment: String,
}

/// Which language's rules a repo-relative path is subject to, or `None` for out of scope.
///
/// **Derived from the path, and the scope is argued rather than assumed.** `crates/`, `xtask/` and
/// `dev/` are where this repository's test and gate code lives - the two things
/// `telekom/sutura#405` names. `nix/*.sh` is in scope because those scripts run on the developer's
/// own machine rather than inside a nix sandbox, so a path one of them writes is exactly as shared
/// as a Rust one; a `.nix` file is NOT, and that is the gate's largest stated limit - see the
/// module header of `super`.
pub(super) fn language_of(rel: &str) -> Option<Language> {
    // Extension by suffix, deliberately case-SENSITIVE: `.RS` is not a file this repository writes,
    // and a case-folding comparison would put a scope decision at the mercy of the filesystem the
    // path came off - which is the mistake `Scope::from_canonical` records for the digest.
    let extension = std::path::Path::new(rel).extension().and_then(|raw| raw.to_str());
    match extension {
        Some("rs") if ["crates/", "xtask/", "dev/"].iter().any(|dir| rel.starts_with(dir)) => Some(Language::Rust),
        Some("sh") if rel.starts_with("nix/") => Some(Language::Shell),
        _ => None,
    }
}

/// Assembled from parts so this gate's own source does not carry its own needle as a live anchor.
fn rust_root() -> String {
    format!("{}{}", "temp_", concat!("dir", "()"))
}

/// Every machine-shared root a path can be spelled from, longest first so `/var/tmp` is not read
/// as `/tmp` with a prefix.
const SHARED_ROOTS: &[&str] = &["/private/tmp", "/var/tmp", "/tmp"];

/// The shell spellings of the same root.
fn shell_roots() -> Vec<String> {
    let mut roots = vec![String::from("$TMPDIR"), String::from("${TMPDIR")];
    roots.extend(SHARED_ROOTS.iter().map(|root| (*root).to_owned()));
    roots
}

/// A call that changes the filesystem. Used only to decide [`Keyed::Unwritten`].
///
/// Receivers are not resolvable from a line - `sutura/gates` records that for
/// `check-bounded-wait`'s `.status()` - so this list is deliberately the SPELLINGS that name a
/// filesystem operation and nothing that could plausibly be something else.
const MUTATIONS: &[&str] = &[
    "create_dir",
    "create_new",
    "File::create",
    "fs::write",
    "fs::rename",
    "fs::copy",
    "fs::remove",
    "remove_dir",
    "remove_file",
    "OpenOptions",
    "write_all",
    "symlink",
];

/// How many times a shared root is taken in `text`, counted by a predicate the loop does not use.
///
/// **The other side of the conservation law**, and it is a different expression on purpose:
/// `sutura/gates` records a gate whose only floor was computed off the same filter its loop used,
/// where `.take(1)` satisfied both. This counts OCCURRENCES of a root spelling in the code half of
/// the file and knows nothing about narrowing, adjudication or statements.
pub(super) fn offered(language: Language, text: &str) -> usize {
    let code = code_of(language, text);
    let mut count = 0_usize;
    for root in roots_of(language) {
        count = count.saturating_add(code.matches(root.as_str()).count());
    }
    if language == Language::Rust {
        count = count.saturating_add(rooted_literals(text).len());
    }
    count
}

/// Every taking in `text`, with the answer to *whose key reaches it*.
pub(super) fn takings(path: &str, language: Language, text: &str) -> Vec<(Taking, Keyed)> {
    let code = code_of(language, text);
    let bindings = bindings_of(language, &code);
    let mut found = Vec::new();

    for root in roots_of(language) {
        for at in occurrences(&code, root.as_str()) {
            let after = code.get(at.saturating_add(root.len())..).unwrap_or_default();
            let segment = match language {
                Language::Rust => joined_segment(after),
                Language::Shell => word_tail(after),
            };
            let keyed = adjudicate(language, &segment, at, &bindings);
            found.push((
                Taking {
                    path: String::from(path),
                    line: line_of(&code, at),
                    root: root.clone(),
                    segment,
                },
                keyed,
            ));
        }
    }

    if language == Language::Rust {
        for literal in rooted_literals(text) {
            let statement = statement_at_line(&code, literal.line);
            let keyed = if MUTATIONS.iter().any(|call| statement.contains(call)) {
                Keyed::Shared
            } else {
                Keyed::Unwritten
            };
            found.push((
                Taking {
                    path: String::from(path),
                    line: literal.line,
                    root: String::from("a path literal"),
                    segment: literal.body,
                },
                keyed,
            ));
        }
    }

    found.sort_by_key(|(taking, _)| taking.line);
    found
}

/// The code half of a file: comments gone, and whatever else a language's lexer removes.
fn code_of(language: Language, text: &str) -> String {
    match language {
        Language::Rust => crate::serde_parse::scan::code_lines(text).join("\n"),
        Language::Shell => text
            .lines()
            .map(|line| match line.find('#') {
                Some(at) if line.get(..at).is_some_and(|before| before.trim().is_empty()) => String::new(),
                _ => String::from(line),
            })
            .collect::<Vec<String>>()
            .join("\n"),
    }
}

/// The root spellings a language can take.
fn roots_of(language: Language) -> Vec<String> {
    match language {
        Language::Rust => vec![rust_root()],
        Language::Shell => shell_roots(),
    }
}

/// Every byte offset at which `needle` occurs in `text`.
fn occurrences(text: &str, needle: &str) -> Vec<usize> {
    text.match_indices(needle).map(|(at, _)| at).collect()
}

/// The 1-based line an offset sits on.
fn line_of(text: &str, at: usize) -> usize {
    text.get(..at)
        .map_or(1, |before| before.matches('\n').count().saturating_add(1))
}

/// The argument of the FIRST `.join(..)` applied to a root, or an empty string when the root is
/// narrowed by nothing in this expression.
///
/// **The first segment is the whole of the question**, because a path is separated by its topmost
/// component: `<shared>/sutura-conformance/<table>.csv` is one directory for every checkout
/// whatever the leaf carries, which is exactly the defect this gate was written for - and that
/// corpus DID carry the process id, in the staged name it renamed away from.
fn joined_segment(after: &str) -> String {
    let rest = after.trim_start();
    let Some(inside) = rest.strip_prefix(".join(") else {
        return String::new();
    };
    balanced(inside).unwrap_or_default()
}

/// The rest of a shell WORD after a root spelling: everything up to whitespace or a quote.
fn word_tail(after: &str) -> String {
    after
        .chars()
        .take_while(|c| !c.is_whitespace() && *c != '"' && *c != '\'' && *c != ';')
        .collect()
}

/// The text up to the paren that closes the one this starts inside.
fn balanced(inside: &str) -> Option<String> {
    let mut depth = 1_i32;
    let mut out = String::new();
    for character in inside.chars() {
        match character {
            '(' => depth = depth.saturating_add(1),
            ')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(out);
                }
            }
            _ => {}
        }
        out.push(character);
    }
    None
}

/// How far a name is followed to the value that keys it.
///
/// **Bounded, and the bound is what the tree needs plus one.** The Rust shape is one hop -
/// `let unique = format!("..{}", process::id()); temp_dir().join(unique)` - and the shell shape is
/// two, because a tier derives `key` from `root` and `root` from `pwd -P`. A chain longer than this
/// reads as UNKEYED, which is the safe direction: it asks for the key to be moved nearer the taking
/// rather than reporting a green over a path nothing visibly narrows.
const MAX_HOPS: usize = 4;

/// Whose key a segment names, following a name to its binding up to [`MAX_HOPS`] times.
///
/// A visited set as well as a bound, because `a=$b; b=$a` is a cycle a text scan can be handed and
/// a gate that loops on it is a gate that hangs - which `sutura/gates` records as the most
/// expensive failure mode a check has.
fn adjudicate(language: Language, segment: &str, at: usize, bindings: &Bindings) -> Keyed {
    if segment.trim().is_empty() {
        return Keyed::Shared;
    }
    let mut current = String::from(segment);
    let mut seen: Vec<String> = Vec::new();
    for _ in 0..MAX_HOPS {
        if let Some(direct) = keyed_by(&current) {
            return direct;
        }
        let Some(name) = referenced_name(language, &current) else {
            return Keyed::Shared;
        };
        if seen.contains(&name) {
            return Keyed::Shared;
        }
        let Some(value) = bindings.resolve(&name, at) else {
            return Keyed::Shared;
        };
        seen.push(name);
        current = value;
    }
    Keyed::Shared
}

/// What a piece of text names outright, with no binding to follow.
fn keyed_by(text: &str) -> Option<Keyed> {
    /// The spellings that name the worktree's own key, in either language. `Scope` and its two
    /// derivations plus the digest for Rust - deliberately NOT the bare word `scope`, which appears
    /// in `sutura-scope-<pid>` and would have reported a process key as a worktree one - and, for
    /// shell, the two ways a script can learn where it is: `pwd -P` and `git rev-parse
    /// --show-toplevel`. Those two are what the tiers already derive their key from.
    const WORKTREE: &[&str] = &[
        "digest",
        "Scope::",
        "state_dir",
        "scratch",
        "STATE_DIR",
        "pwd -P",
        "show-toplevel",
    ];
    /// The spellings that name this process. `mktemp` and a scoped temporary directory allocate
    /// rather than derive, which is a stronger answer than a key and reads as this one.
    const PROCESS: &[&str] = &["process::id", "mktemp", "TempDir", "tempdir", "$$"];
    // PROCESS FIRST, because it is the more specific reading: a name carrying both `sutura-scope`
    // and the process id is keyed by the process, and reporting the wrong holder in a verdict is
    // the kind of confident wrong answer this whole gate is about.
    if PROCESS.iter().any(|needle| text.contains(needle)) {
        return Some(Keyed::Process);
    }
    if WORKTREE.iter().any(|needle| text.contains(needle)) {
        return Some(Keyed::Worktree);
    }
    None
}

/// The single name a segment refers to, when a segment is nothing but a reference.
///
/// Rust: a bare identifier, with `&` and `.clone()` tolerated. Shell: the first `$name` in the
/// word. Anything else - a literal, a call, an expression - names nothing to follow.
fn referenced_name(language: Language, segment: &str) -> Option<String> {
    match language {
        Language::Rust => {
            let bare = segment
                .trim()
                .trim_start_matches('&')
                .trim_end_matches("()")
                .trim_end_matches(".clone")
                .trim();
            let ident = bare.trim_start_matches("String::from(").trim_end_matches(')').trim();
            let is_ident = !ident.is_empty()
                && ident.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                && !ident.starts_with(|c: char| c.is_ascii_digit());
            is_ident.then(|| String::from(ident))
        }
        Language::Shell => {
            // THE FIRST `$` IS NOT ALWAYS A VARIABLE, and getting that wrong is what made the real
            // tier's shape read as unkeyed: `key="$(printf '%s' "$root" | cksum ..)"` opens with a
            // command substitution, so `$(` yielded an empty name and the chain stopped one hop
            // short of `pwd -P`. Every `$` is tried, in order, and the first that names something
            // wins.
            segment.match_indices('$').find_map(|(at, _)| {
                let rest = segment.get(at.saturating_add(1)..)?;
                let name: String = rest
                    .trim_start_matches('{')
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .collect();
                (!name.is_empty()).then_some(name)
            })
        }
    }
}

/// One binding: where it was written, and what it was bound to.
type Bound = (usize, String);

/// Every `name = value` this gate can follow, with where it was written.
#[derive(Debug, Default)]
struct Bindings {
    /// Name to every binding of it, in source order.
    by_name: BTreeMap<String, Vec<Bound>>,
}

impl Bindings {
    /// The value bound to `name` most recently BEFORE `at`.
    ///
    /// Most recently before, rather than any binding of that name anywhere: a later binding cannot
    /// have keyed an earlier taking, and *any binding keyed it* would be the permissive answer.
    fn resolve(&self, name: &str, at: usize) -> Option<String> {
        self.by_name
            .get(name)?
            .iter()
            .rev()
            .find(|(offset, _)| *offset < at)
            .map(|(_, value)| value.clone())
    }
}

/// Every binding in the code half of a file.
fn bindings_of(language: Language, code: &str) -> Bindings {
    let mut bindings = Bindings::default();
    match language {
        Language::Rust => {
            for (at, _) in code.match_indices("let ") {
                let rest = code.get(at.saturating_add(4)..).unwrap_or_default();
                let declared = rest.trim_start().trim_start_matches("mut ").trim_start();
                let name: String = declared
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .collect();
                if name.is_empty() {
                    continue;
                }
                let Some((_, value)) = declared.split_once('=') else {
                    continue;
                };
                bindings.by_name.entry(name).or_default().push((at, truncated(value)));
            }
        }
        Language::Shell => {
            let mut offset = 0_usize;
            for line in code.lines() {
                if let Some((left, value)) = line.split_once('=') {
                    let name = left.trim().trim_start_matches("export ").trim();
                    if !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                        bindings
                            .by_name
                            .entry(String::from(name))
                            .or_default()
                            .push((offset, String::from(value)));
                    }
                }
                offset = offset.saturating_add(line.len()).saturating_add(1);
            }
        }
    }
    bindings
}

/// A binding's value, bounded to the statement it opens.
fn truncated(value: &str) -> String {
    let mut depth = 0_i32;
    let mut out = String::new();
    for character in value.chars() {
        match character {
            '(' | '[' | '{' => depth = depth.saturating_add(1),
            ')' | ']' | '}' => depth = depth.saturating_sub(1),
            ';' if depth <= 0 => return out,
            _ => {}
        }
        out.push(character);
    }
    out
}

/// The lines around a 1-based line, as one string, bounded by [`STATEMENT_LINES`].
///
/// A statement rather than a line, because a call is routinely broken over several by the
/// formatter - and bounded, because an unbalanced delimiter must not turn a statement read into a
/// whole-file one.
fn statement_at_line(code: &str, line: usize) -> String {
    let mut depth = 0_i32;
    let mut out: Vec<&str> = Vec::new();
    for text in code.lines().skip(line.saturating_sub(1)).take(STATEMENT_LINES) {
        out.push(text);
        let mut ends_here = false;
        for character in text.chars() {
            match character {
                '(' | '[' => depth = depth.saturating_add(1),
                ')' | ']' => depth = depth.saturating_sub(1),
                ';' if depth <= 0 => ends_here = true,
                _ => {}
            }
        }
        if ends_here {
            break;
        }
    }
    out.join("\n")
}

/// A string literal whose VALUE is rooted at a machine-shared temporary directory.
struct RootedLiteral {
    line: usize,
    body: String,
}

/// Every such literal in a Rust file, read through the workspace's own literal lexer.
///
/// **The lexer answers the nesting question this gate would otherwise get wrong.**
/// `xtask/src/examples.rs` carries a fixture whose OUTER literal contains an escaped
/// `\"/tmp/s/..\"`; there is no inner literal for the compiler and there is none here either, so
/// that line is not a taking - which is right, and which a `contains` would have got wrong.
fn rooted_literals(text: &str) -> Vec<RootedLiteral> {
    crate::serde_parse::scan::string_literals(text)
        .into_iter()
        .filter(|literal| {
            SHARED_ROOTS
                .iter()
                .any(|root| literal.body == *root || literal.body.starts_with(&format!("{root}/")))
        })
        .map(|literal| RootedLiteral {
            line: literal.line,
            body: literal.body,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{Keyed, Language, language_of, offered, takings};

    /// The one answer for a fixture with exactly one taking.
    ///
    /// It asserts the two sides of the per-taking conservation law agree on the fixture as well,
    /// so a fixture that the loop and the count read differently is a failure here rather than a
    /// surprise in the verdict.
    fn only(language: Language, text: &str) -> Keyed {
        let found = takings("crates/x/src/lib.rs", language, text);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(offered(language, text), 1, "the two sides disagree: {found:?}");
        found.first().map(|(_, keyed)| *keyed).expect("one taking")
    }

    /// EVERY FIXTURE BELOW IS ONE MULTI-LINE RAW STRING, AND THAT IS LOAD-BEARING RATHER THAN
    /// STYLE. `crate::serde_parse::scan::code_lines` blanks the interior of a string that spans
    /// lines, so a fixture written this way is invisible to the gate scanning its own source -
    /// which is what lets this module hold its own needle without reporting itself. Measured on the
    /// way in: with these fixtures written as arrays of single-line literals, the gate reported 8
    /// violations in this file, because `sutura/gates` states for all three of this workspace's
    /// lexers that a needle inside a SINGLE-line string on a live line IS a live anchor.
    #[test]
    fn a_purpose_with_no_key_is_the_defect_this_gate_was_written_for() {
        // `telekom/sutura#405`'s instance 1, verbatim in shape: a directory named for what it holds
        // and for nothing that says WHOSE it is. Reproduced with two worktrees on 2026-09-07 - see
        // `super`'s header.
        let text = r#"
fn materialise() -> PathBuf {
    let dir = std::env::temp_dir().join("sutura-conformance");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
"#;
        assert_eq!(only(Language::Rust, text), Keyed::Shared);
    }

    #[test]
    fn a_key_in_the_leaf_does_not_key_the_directory() {
        // THE HALF THAT WOULD HAVE MISSED THE INSTANCE. `materialise` DID carry the process id - in
        // the staged file it renamed away from - so a rule that asked *is a key anywhere near this*
        // would have passed the very defect. The first segment below the root is the whole question,
        // because that segment is one directory for every checkout on the machine.
        let text = r#"
fn materialise() -> PathBuf {
    let dir = std::env::temp_dir().join("sutura-conformance");
    let staged = dir.join(format!("rows.{}.csv", std::process::id()));
    std::fs::write(&staged, ROWS).unwrap();
    staged
}
"#;
        assert_eq!(only(Language::Rust, text), Keyed::Shared);
    }

    #[test]
    fn a_taking_narrowed_by_nothing_is_shared() {
        // `let at = std::env::temp_dir();` with the `join` ten lines down, which is the shape
        // `xtask/src/compose/docker/bounded.rs` had. The gate does not guess: it reports the taking
        // and the remedy says to put the key in the same statement.
        let text = r#"
fn new() -> Self {
    let at = std::env::temp_dir();
    Self { stdout: at.join(format!("{unique}.out")) }
}
"#;
        assert_eq!(only(Language::Rust, text), Keyed::Shared);
    }

    #[test]
    fn the_process_id_in_the_first_segment_keys_it() {
        let text = r#"
fn scratch() -> PathBuf {
    std::env::temp_dir().join(format!("sutura-x-{}", std::process::id()))
}
"#;
        assert_eq!(only(Language::Rust, text), Keyed::Process);
    }

    #[test]
    fn the_worktree_digest_in_the_first_segment_keys_it() {
        let text = r#"
pub fn scratch(&self, purpose: &str) -> PathBuf {
    std::env::temp_dir().join(format!("sutura-{}-{purpose}", self.digest))
}
"#;
        assert_eq!(only(Language::Rust, text), Keyed::Worktree);
    }

    #[test]
    fn a_key_bound_through_a_chain_is_followed_as_far_as_the_bound_and_no_further() {
        // One hop is the Rust shape - `let unique = format!(..id()); ..join(unique)` - and two is
        // the shell one, so the bound is what the tree needs plus one. Both directions, because a
        // resolver with no bound is a resolver that can be handed a cycle.
        let one_hop = r#"
fn new() -> Self {
    let unique = format!("sutura-compose-{}", std::process::id());
    let at = std::env::temp_dir().join(unique);
    Self { at }
}
"#;
        assert_eq!(only(Language::Rust, one_hop), Keyed::Process);

        // Past the bound: five names between the taking and the key.
        let too_far = r#"
fn new() -> Self {
    let e = format!("sutura-{}", std::process::id());
    let d = e;
    let c = d;
    let b = c;
    let a = b;
    let at = std::env::temp_dir().join(a);
    Self { at }
}
"#;
        assert_eq!(only(Language::Rust, too_far), Keyed::Shared);
    }

    #[test]
    fn a_cyclic_binding_answers_rather_than_looping() {
        // A text scan can be handed `a = b; b = a`. A gate that loops on it hangs, which
        // `sutura/gates` records as the most expensive failure mode a check has - so the resolver
        // carries a visited set as well as a bound, and this is the cell that would hang without it.
        // A plain string rather than a raw one, because it holds no quote - and still MULTI-LINE,
        // which is the property that keeps it invisible to the gate scanning this file.
        let text = "
fn new() -> Self {
    let a = b;
    let b = a;
    let at = std::env::temp_dir().join(a);
    Self { at }
}
";
        assert_eq!(only(Language::Rust, text), Keyed::Shared);
    }

    #[test]
    fn a_binding_written_after_the_taking_does_not_key_it() {
        // `Bindings::resolve` takes the most recent binding BEFORE the taking, because a later one
        // cannot have keyed an earlier path - and *any binding of that name anywhere* is the
        // permissive answer that would pass this fixture.
        let text = r#"
fn new() -> Self {
    let at = std::env::temp_dir().join(unique);
    let unique = format!("sutura-{}", std::process::id());
    Self { at }
}
"#;
        assert_eq!(only(Language::Rust, text), Keyed::Shared);
    }

    #[test]
    fn a_comment_is_not_a_taking() {
        // `dev/src/discovery.rs` and `dev/src/provisioned.rs` each hand the shared root to
        // `Scope::from_root` inside a DOCTEST, to prove nothing is provisioned there. Those are
        // prose, and this is the assertion that they are - it is also the limit, stated at the
        // module header: a doctest that really wrote to a shared path is invisible here.
        let text = r#"
/// let dir = std::env::temp_dir().join("whatever");
fn nothing() {}
"#;
        assert!(takings("dev/src/discovery.rs", Language::Rust, text).is_empty());
        assert_eq!(offered(Language::Rust, text), 0);
    }

    #[test]
    fn a_path_literal_in_a_statement_that_writes_is_shared_and_one_that_does_not_is_read() {
        // THE MEASURED SPLIT. Every rooted literal in this workspace is a fixture value that never
        // reaches a filesystem, so a rule reddening all of them would redden correct work - and a
        // gate that reddens correct work gets disabled. Both directions here, because either alone
        // is satisfied by a gate that answers one way always.
        let written = r#"
fn go() {
    std::fs::create_dir_all("/tmp/sutura-shared").unwrap();
}
"#;
        assert_eq!(only(Language::Rust, written), Keyed::Shared);

        let read = r#"
fn go() {
    assert_eq!(one.dir(), Path::new("/tmp/tree"));
}
"#;
        assert_eq!(only(Language::Rust, read), Keyed::Unwritten);
    }

    #[test]
    fn an_escaped_literal_inside_a_literal_is_not_a_taking() {
        // `xtask/src/examples.rs` carries exactly this: an outer fixture string whose body contains
        // an escaped inner path. There is no inner literal for the compiler and there is none here
        // either - which a `contains` would have got wrong, and which the workspace's own literal
        // lexer gets right.
        let text = "fn go() {\n    let fixture = \"let p = \\\"/tmp/s/x.json\\\";\";\n}\n";
        assert!(takings("xtask/src/examples.rs", Language::Rust, text).is_empty());
        assert_eq!(offered(Language::Rust, text), 0);
    }

    #[test]
    fn a_shell_path_with_no_key_is_shared_and_one_derived_from_the_worktree_is_not() {
        // `nix/*.sh` runs on the developer's own machine, so a path one of them writes is exactly as
        // shared as a Rust one. `telekom/sutura#405`'s instance 2 is a log path of this shape.
        let bare = r#"
#!/usr/bin/env bash
log="/tmp/shipcheck.log"
echo hi >"$log"
"#;
        assert_eq!(only(Language::Shell, bare), Keyed::Shared);

        // The real tier's shape: two hops from the path to `pwd -P`. A word that names the root
        // TWICE - `${TMPDIR:-/tmp}`, the variable and its default - is two takings, because the unit
        // is an ACT and that word performs the acquisition two ways; both are adjudicated, and the
        // conservation law counts acts rather than lines for exactly this reason.
        let keyed = r#"
root="$(pwd -P)"
key="$(printf '%s' "$root" | cksum | cut -d' ' -f1)"
pg="${TMPDIR:-/tmp}/sutura-pg-$key"
"#;
        let found = takings("nix/postgres.sh", Language::Shell, keyed);
        assert_eq!(found.len(), 2, "{found:?}");
        assert_eq!(offered(Language::Shell, keyed), 2);
        assert!(
            found.iter().all(|(_, answer)| !answer.is_shared()),
            "a worktree-derived key must not read as shared: {found:?}"
        );
        assert!(found.iter().any(|(_, answer)| *answer == Keyed::Worktree), "{found:?}");

        let by_process = r#"
scratch="$TMPDIR/sutura-$$"
"#;
        assert_eq!(only(Language::Shell, by_process), Keyed::Process);
    }

    #[test]
    fn a_commented_out_shell_line_is_not_a_taking() {
        let text = r#"
# log="/tmp/shipcheck.log"
echo hi
"#;
        assert!(takings("nix/run-gate.sh", Language::Shell, text).is_empty());
        assert_eq!(offered(Language::Shell, text), 0);
    }

    #[test]
    fn two_takings_on_one_line_are_two() {
        // The unit is a TAKING, so a line holding two is two - `crate::warm_start::pairing`'s rule
        // one directory over, and the reason the conservation law counts occurrences rather than
        // lines.
        let text = r#"
fn go() {
    let pair = (std::env::temp_dir().join("a"), std::env::temp_dir().join("b"));
}
"#;
        let found = takings("crates/x/src/lib.rs", Language::Rust, text);
        assert_eq!(found.len(), 2, "{found:?}");
        assert_eq!(offered(Language::Rust, text), 2);
    }

    #[test]
    fn the_scope_is_first_party_rust_and_the_shell_that_gates_it() {
        assert_eq!(language_of("crates/sutura-conformance/src/corpus.rs"), Some(Language::Rust));
        assert_eq!(language_of("xtask/src/repo.rs"), Some(Language::Rust));
        assert_eq!(language_of("dev/src/scope.rs"), Some(Language::Rust));
        assert_eq!(language_of("nix/with-tier.sh"), Some(Language::Shell));
        // OUT of scope, each for its own reason: a `.nix` file cannot be told apart from a
        // derivation's private temporary directory by text (the gate's stated limit), a script
        // outside `nix/` is not a gate, and a page is prose.
        assert_eq!(language_of("nix/postgres-tier.nix"), None);
        assert_eq!(language_of("docs/publish.sh"), None);
        assert_eq!(language_of("AGENTS.md"), None);
    }
}
