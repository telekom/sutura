//! Following a name to the value that keys a path, and deciding what that value NAMES.
//!
//! **Its own module because `super` hit the 1000-line cap `cargo xtask max-lines` holds** - the
//! same pressure `crate::falsifier` and `crate::registry` each record - and the seam is by task
//! rather than by size: everything here answers *whose key reaches this*, and nothing here knows
//! what a taking is. Every `#[test]` stayed in `super`, which is what the causality gate requires:
//! it never reverts a file that adds a test, so a moved assertion orphans silently and reads as
//! *green against base*. What this file adds instead are DIRECT unit tests of the two things
//! `super`'s fixtures could only reach through a whole scan.

use std::collections::BTreeMap;

use super::{Keyed, Language};

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
pub(super) fn adjudicate(language: Language, segment: &str, at: usize, bindings: &Bindings) -> Keyed {
    if segment.trim().is_empty() {
        return Keyed::Shared;
    }
    let mut current = String::from(segment);
    let mut seen: Vec<String> = Vec::new();
    for _ in 0..MAX_HOPS {
        if let Some(direct) = keyed_by(language, &current) {
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
fn keyed_by(language: Language, text: &str) -> Option<Keyed> {
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
    // **A NEEDLE HAS TO BE CODE, AND THIS IS THE DEFECT REVIEW FOUND.** Matched against the raw
    // segment, `temp_dir().join("sutura-scratch")` passed - the word `scratch` was in the BASENAME,
    // not in a derivation - and so did `"shared-digest-cache"` and `"my-tempdir"`. Three written,
    // machine-shared paths at exit 0, which is the second of the three shapes this gate claims to
    // refuse. **The sharp part: `scratch` and `state_dir` are the two words `super::explain()`
    // tells a developer to use**, so following the printed remedy produced a path the gate then
    // accepted. It is the comment-versus-code split `check-newtype-leaks` already states, one level
    // down: blank the non-code half before matching anything.
    let code = match language {
        Language::Rust => outside_literals(text),
        // Shell has no such split and blanking one would be wrong: a `"$( .. )"` still interpolates
        // and executes, so `pwd -P` inside `key="$(printf '%s' "$root" | cksum ..)"` IS the
        // derivation the tiers key from.
        Language::Shell => String::from(text),
    };
    // PROCESS FIRST, because it is the more specific reading: a name carrying both `sutura-scope`
    // and the process id is keyed by the process, and reporting the wrong holder in a verdict is
    // the kind of confident wrong answer this whole gate is about.
    if PROCESS.iter().any(|needle| code.contains(needle)) {
        return Some(Keyed::Process);
    }
    if WORKTREE.iter().any(|needle| code.contains(needle)) {
        return Some(Keyed::Worktree);
    }
    None
}

/// A Rust expression with every string literal's TEXT blanked, keeping what a `{..}` placeholder
/// names.
///
/// **The placeholders are kept because they are code**: `format!("sutura-{digest}")` captures a
/// binding, so blanking the whole literal would read a correctly keyed path as unkeyed - a gate that
/// reddens correct work gets disabled. Everything else inside the quotes goes, which is what closes
/// the hole: a BASENAME containing the word `scratch` is text, not a derivation.
///
/// A fragment lexer rather than [`crate::serde_parse::scan`]'s, because what arrives here is one
/// `.join(..)` argument or one binding's value, not a file. Raw strings are blanked wholesale - the
/// safe direction, and a raw string in a path expression carries no placeholder to lose.
fn outside_literals(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_string = false;
    let mut in_placeholder = false;
    let mut escaped = false;
    for character in text.chars() {
        if !in_string {
            in_string = character == '"';
            out.push(if in_string { ' ' } else { character });
            continue;
        }
        if escaped {
            escaped = false;
            out.push(' ');
            continue;
        }
        match character {
            '\\' => {
                escaped = true;
                out.push(' ');
            }
            '"' => {
                in_string = false;
                in_placeholder = false;
                out.push(' ');
            }
            '{' => {
                in_placeholder = true;
                out.push(' ');
            }
            '}' => {
                in_placeholder = false;
                out.push(' ');
            }
            _ => out.push(if in_placeholder { character } else { ' ' }),
        }
    }
    out
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
pub(super) struct Bindings {
    /// Name to every binding of it, in source order.
    by_name: BTreeMap<String, Vec<Bound>>,
}

impl Bindings {
    /// The value bound to `name` most recently BEFORE `at`.
    ///
    /// Most recently before, rather than any binding of that name anywhere: a later binding cannot
    /// have keyed an earlier taking, and *any binding keyed it* would be the permissive answer.
    pub(super) fn resolve(&self, name: &str, at: usize) -> Option<String> {
        self.by_name
            .get(name)?
            .iter()
            .rev()
            .find(|(offset, _)| *offset < at)
            .map(|(_, value)| value.clone())
    }
}

/// Every binding in the code half of a file.
pub(super) fn bindings_of(language: Language, code: &str) -> Bindings {
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

#[cfg(test)]
mod tests {
    use super::{Keyed, Language, keyed_by, outside_literals};

    #[test]
    fn a_needle_inside_a_rust_literal_is_blanked_and_a_placeholder_is_not() {
        // THE TWO HALVES OF THE FIX REVIEW ASKED FOR, tested where they live rather than through a
        // whole scan. `super`'s fixtures reach this only via `takings`, so a change here that broke
        // one direction and not the other could look like one failing integration test.
        assert!(!outside_literals(r#"join("sutura-scratch")"#).contains("scratch"));
        assert!(!outside_literals(r#"join("shared-digest-cache")"#).contains("digest"));
        assert!(!outside_literals(r#"join("my-tempdir")"#).contains("tempdir"));
        // A placeholder is code: `format!("sutura-{digest}")` captures a binding.
        assert!(outside_literals(r#"format!("sutura-{digest}")"#).contains("digest"));
        // And an argument outside the quotes is untouched.
        assert!(outside_literals(r#"format!("sutura-{}", std::process::id())"#).contains("process::id"));
    }

    #[test]
    fn an_escape_inside_a_literal_cannot_end_it_early() {
        // `"a\"scratch"` is ONE literal whose text contains a quote. A blanker that treated the
        // escaped quote as the end would leave `scratch` outside a literal and reading as a key.
        let blanked = outside_literals("join(\"a\\\"scratch\")");
        assert!(!blanked.contains("scratch"), "{blanked}");
    }

    #[test]
    fn a_needle_is_matched_in_rust_code_and_in_shell_text() {
        // Language-scoped, both directions. Shell has no code/literal split - a `"$( .. )"` still
        // interpolates - so blanking there would have reddened both tier scripts.
        assert_eq!(keyed_by(Language::Rust, r#""sutura-scratch""#), None);
        assert_eq!(keyed_by(Language::Rust, "self.digest"), Some(Keyed::Worktree));
        assert_eq!(keyed_by(Language::Rust, "std::process::id()"), Some(Keyed::Process));
        assert_eq!(keyed_by(Language::Shell, r#""$(pwd -P)""#), Some(Keyed::Worktree));
        assert_eq!(keyed_by(Language::Shell, "sutura-$$"), Some(Keyed::Process));
        assert_eq!(keyed_by(Language::Shell, "sutura-shared"), None);
    }

    #[test]
    fn a_binding_resolves_to_the_most_recent_one_before_the_taking() {
        // `Bindings::resolve`'s own rule, direct: a later binding cannot have keyed an earlier
        // path, and *any binding of that name anywhere* is the permissive answer.
        let code = "let unique = a;\nlet unique = b;\n";
        let bindings = super::bindings_of(Language::Rust, code);
        assert_eq!(bindings.resolve("unique", 0), None, "nothing is bound before the first `let`");
        assert_eq!(bindings.resolve("unique", code.len()).as_deref(), Some(" b"));
        assert_eq!(bindings.resolve("nothing", code.len()), None);
    }
}
