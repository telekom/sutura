//! A platform predicate read off `stdenv` itself, which nixpkgs deprecated and still evaluates.
//!
//! `stdenv.isDarwin` and its siblings are aliases nixpkgs kept for compatibility. They work, they
//! emit `stdenv.isDarwin is deprecated, use stdenv.hostPlatform.isDarwin instead` on every
//! evaluation, and one of them survived in this tree while every other site had already moved -
//! which is the whole argument for a gate rather than a sentence. A warning nobody acts on is a
//! warning readers learn to scroll past, and the next real one scrolls past with it.
//!
//! # What it holds
//!
//! Every `.nix` file this repository tracks carries no `stdenv.is<Platform>` spelling. The scan is
//! over the whole file text rather than line by line, so a spelling the formatter wrapped across a
//! newline - `stdenv` on one line and `.isDarwin` on the next - is found; a line-based needle
//! cannot prove that absence, and `sutura/gates` records that shape twice.
//!
//! # What it does NOT hold, stated because an overstated gate is itself the defect
//!
//! * **It does not know nixpkgs' deprecation list.** It enforces one lexical rule - the `stdenv.is*`
//!   family moves to `stdenv.hostPlatform.is*` - and says nothing about any other alias nixpkgs
//!   deprecates or will deprecate. A second deprecated spelling arrives unseen, and the remedy for
//!   that is another rule here rather than a wider claim from this one.
//! * **It does not evaluate nix.** A predicate reached through a `let` binding or a function
//!   argument - `let s = pkgs.stdenv; in s.isDarwin` - is not this spelling and passes. That is a
//!   text scan's boundary and not a thing to pretend about; what closes it is nix's own warning,
//!   which is what found the instance this gate was written for.
//! * **A COMMENT IS NOT EXEMPT, deliberately.** There is no comment lexer here and there is not
//!   meant to be one: a `#` is where an evasion would hide, `#` also opens no comment inside a
//!   string, and no nix file has a reason to write the deprecated spelling at all - so the
//!   fail-closed direction costs nothing and needs no second lexer that can disagree with the
//!   first. The remedy is to write the supported spelling, in a comment as much as in code.
//! * **The floor is the census's, not this gate's.** A tree with `.nix` files and no violation is a
//!   legitimate pass, so there is nothing here to count a violation against;
//!   `crate::repo::Census::inspect` is what refuses an empty discovery, an unreadable in-scope file
//!   and a `flake.nix` the walk never judged. What makes this gate's own refusal reachable is
//!   `crate::falsifier`'s seeded violation, which is the distinction `telekom/sutura#371` asks for.

use crate::Verdict;
use crate::repo;

/// The attribute prefix that moved: `stdenv.isDarwin` -> `stdenv.hostPlatform.isDarwin`.
const OWNER: &str = "stdenv";

/// Where the predicate lives now.
const MOVED_TO: &str = "hostPlatform";

/// One deprecated spelling, with the line a reader opens.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Deprecated {
    /// 1-based line of the `stdenv` the predicate hangs off.
    pub(crate) line: usize,
    /// The predicate's own name, `isDarwin`.
    pub(crate) attribute: String,
}

/// Is this the last character of an identifier, so a `stdenv` after it is not this `stdenv`?
///
/// `mystdenv.isDarwin` names somebody else's attribute set and this gate has nothing to say about
/// it. A `.` or a space before it is the real thing - `pkgs.stdenv.isDarwin`, `stdenv.isDarwin`.
const fn continues_a_name(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-' || byte == b'\''
}

/// Every `stdenv.is<Platform>` in `text`.
///
/// Whitespace-tolerant between the three parts, because the formatter may put the `.` or the
/// attribute on the next line and a line-based scan would then report the absence of something
/// that is present. The attribute has to start `is` followed by any ASCII-alphabetic letter:
/// nixpkgs' real deprecation list (`pkgs/stdenv/generic/default.nix`) reaches all the way down to
/// `isx86_64`, `isi686` and `isx86_32`, so no letter after `is` is safe, and no correct code writes
/// `stdenv.is*` at all - failing closed costs nothing.
pub(crate) fn deprecated(text: &str) -> Vec<Deprecated> {
    let bytes = text.as_bytes();
    let mut found = Vec::new();
    for (at, _) in text.match_indices(OWNER) {
        if at > 0 && bytes.get(at.saturating_sub(1)).copied().is_some_and(continues_a_name) {
            continue;
        }
        let mut cursor = at.saturating_add(OWNER.len());
        cursor = past_space(bytes, cursor);
        if bytes.get(cursor).copied() != Some(b'.') {
            continue;
        }
        cursor = past_space(bytes, cursor.saturating_add(1));
        let Some(rest) = text.get(cursor..) else {
            continue;
        };
        let Some(after_is) = rest.strip_prefix("is") else {
            continue;
        };
        if !after_is.starts_with(|c: char| c.is_ascii_alphabetic()) {
            continue;
        }
        let attribute: String = rest.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
        found.push(Deprecated {
            // The line of the OWNER, not of the attribute: that is where the edit goes, and it is
            // the same line in every unwrapped case.
            line: text
                .get(..at)
                .map_or(1, |before| before.matches('\n').count().saturating_add(1)),
            attribute,
        });
    }
    found
}

/// The first index at or after `from` that is not ASCII whitespace.
fn past_space(bytes: &[u8], from: usize) -> usize {
    let mut cursor = from;
    while bytes.get(cursor).copied().is_some_and(|b| b.is_ascii_whitespace()) {
        cursor = cursor.saturating_add(1);
    }
    cursor
}

/// The files this gate judges: nix expressions this repository authors.
///
/// A [`repo::Scope`], so it is a bare `fn` with nothing captured - it cannot count its subjects and
/// it is not handed the content. `vendor/` is out: a vendored tree's spelling is upstream's to fix
/// and `VENDOR.md` is where that is recorded.
fn is_nix_expression(rel: &str) -> bool {
    // Case-insensitive on the extension, matching `serde_parse::in_scope`'s answer for `.rs`: one
    // house pattern rather than two that can disagree, and a `FOO.NIX` nix imports is still a nix
    // file. `vendor/` is out - a vendored tree's spelling is upstream's to fix, recorded in
    // `VENDOR.md`.
    std::path::Path::new(rel)
        .extension()
        .and_then(std::ffi::OsStr::to_str)
        .is_some_and(|ext| ext.eq_ignore_ascii_case("nix"))
        && !rel.starts_with("vendor/")
}

/// The gate.
pub(crate) fn run(_args: &[String]) -> Verdict {
    let census = match repo::all_files() {
        Ok(census) => census,
        Err(refusal) => {
            eprintln!("xtask check-nix-platform: FAILED - {}", refusal.describe());
            return Verdict::Fail;
        }
    };

    let mut problems: Vec<String> = Vec::new();
    let mut read = 0_usize;
    let scope: repo::Scope = is_nix_expression;
    // `flake.nix` as the anchor: it is the one nix file this repository cannot be without, so a
    // walk that judged every other one and not that one is a broken scan wearing a plausible file
    // count. It exists in `crate::falsifier`'s tree as well, which is what keeps the refusal there
    // coming from THIS GATE'S OWN RULE rather than from a missing input - the distinction
    // `telekom/sutura#371` measures, and one only three of the registered gates managed.
    let counted = census.inspect(&["flake.nix"], scope, |rel, bytes| {
        // Lossy rather than a UTF-8 read: a file the census opened is a file this gate judges, and
        // turning a decode failure into an unread file is the silent drop `telekom/sutura#412` is
        // about. A byte sequence that is not UTF-8 cannot spell the needle either way.
        let text = String::from_utf8_lossy(bytes);
        read = read.saturating_add(1);
        for one in deprecated(&text) {
            problems.push(format!(
                "{rel}:{}: `{OWNER}.{}` is deprecated - write `{OWNER}.{MOVED_TO}.{}`",
                one.line, one.attribute, one.attribute
            ));
        }
    });
    let counted = match counted {
        Ok(counted) => counted,
        Err(refusal) => {
            eprintln!("xtask check-nix-platform: FAILED - {}", refusal.describe());
            return Verdict::Fail;
        }
    };

    if problems.is_empty() {
        println!(
            "xtask check-nix-platform: ok - {read} nix file(s) carry no deprecated platform predicate; {}",
            counted.verdict()
        );
        return Verdict::Pass;
    }

    eprintln!("xtask check-nix-platform: FAILED - a deprecated platform predicate:");
    for problem in &problems {
        eprintln!("  {problem}");
    }
    eprintln!();
    eprintln!("nixpkgs kept `{OWNER}.is<Platform>` as an alias and warns on every evaluation that");
    eprintln!("reaches it. It still evaluates, which is why one survived here while every other");
    eprintln!("site had moved: nothing failed. A warning nobody acts on teaches readers to scroll");
    eprintln!("past warnings, and the next real one scrolls past with it. Write");
    eprintln!("`{OWNER}.{MOVED_TO}.is<Platform>`, which is the spelling the rest of this tree uses.");
    Verdict::Fail
}

#[cfg(test)]
mod tests {
    use super::{Deprecated, deprecated, is_nix_expression};

    /// The one spelling this gate exists for, as it stood in `nix/jscpd.nix`.
    #[test]
    fn the_spelling_this_gate_was_written_for_is_found() {
        let found = deprecated("    buildInputs = pkgs.lib.optionals pkgs.stdenv.isDarwin [ pkgs.libiconv ];\n");
        assert_eq!(
            found,
            vec![Deprecated {
                line: 1,
                attribute: String::from("isDarwin")
            }]
        );
    }

    #[test]
    fn the_supported_spelling_is_not_a_finding() {
        // The whole tree is written this way, so a rule that flagged it would fail correct code -
        // and `engineering/rust` records that a gate which fails correct code gets disabled.
        for line in [
            "buildInputs = pkgs.lib.optionals pkgs.stdenv.hostPlatform.isDarwin [ pkgs.libiconv ];",
            "nativeBuildInputs = pkgs.lib.optionals pkgs.stdenv.hostPlatform.isLinux [ ];",
            "isAarch64 = targetPkgs.stdenv.hostPlatform.isAarch64;",
        ] {
            assert_eq!(deprecated(line), Vec::new(), "flagged the supported spelling: {line}");
        }
    }

    #[test]
    fn a_spelling_the_formatter_wrapped_is_still_found() {
        // WHY THE SCAN IS NOT LINE-BASED. A line-based needle reports the absence of something that
        // is present, which `sutura/gates` records as its own defect shape - and the line reported
        // is the owner's, because that is where the edit goes.
        let wrapped = "buildInputs = lib.optionals\n  pkgs.stdenv\n    .isDarwin\n  [ libiconv ];\n";
        assert_eq!(
            deprecated(wrapped),
            vec![Deprecated {
                line: 2,
                attribute: String::from("isDarwin")
            }]
        );
    }

    #[test]
    fn somebody_elses_attribute_set_is_not_this_stdenv() {
        // `continues_a_name`'s reason: a name ENDING in `stdenv` is a different set, and flagging it
        // would be this gate reddening a file it has nothing to say about.
        assert_eq!(deprecated("mystdenv.isDarwin"), Vec::new());
        assert_eq!(deprecated("crossStdenv.isDarwin"), Vec::new());
        // And the real thing after a dot or at the start of an expression still is.
        assert_eq!(deprecated("pkgs.stdenv.isMusl").len(), 1);
        assert_eq!(deprecated("stdenv.isLinux").len(), 1);
    }

    #[test]
    fn an_attribute_that_is_not_a_platform_predicate_is_left_alone() {
        // With the whole `stdenv.is*` family refused, the only spelling left alone is one with no
        // `is` at all - `stdenv.mkDerivation`. `stdenv.island` was once the counter-example that
        // justified an uppercase-only check, but no nixpkgs predicate is spelt that way, so `is`
        // followed by any alphabetic letter is now the family and `island` is a finding too.
        assert_eq!(deprecated("stdenv.mkDerivation"), Vec::new());
        assert_eq!(deprecated("stdenv.island").len(), 1);
        assert_eq!(deprecated("stdenv.isCrossCompiling").len(), 1);
    }

    #[test]
    fn the_lowercase_second_letter_family_is_found() {
        // nixpkgs' real deprecation list has three spellings whose letter after `is` is LOWERCASE -
        // `isx86_64`, `isi686`, `isx86_32` - and the old uppercase-only scan let all three through
        // at exit 0. Any ASCII-alphabetic letter after `is` is the family.
        for line in [
            "pkgs.stdenv.isx86_64",
            "buildInputs = pkgs.stdenv.isi686;",
            "pkgs.stdenv.isx86_32",
        ] {
            assert_eq!(deprecated(line).len(), 1, "missed the deprecated spelling: {line}");
        }
    }

    #[test]
    fn a_lowercase_second_letter_spelling_is_refused_not_accepted() {
        // Both directions: the deprecated `stdenv.isx86_64` is a finding, and the supported
        // `stdenv.hostPlatform.isx86_64` is not.
        assert_eq!(deprecated("stdenv.isx86_64").len(), 1);
        assert_eq!(deprecated("stdenv.hostPlatform.isx86_64"), Vec::new());
    }

    #[test]
    fn a_comment_is_not_exempt_and_that_is_the_stated_direction() {
        // THE ARM A READER WOULD ASSUME GOES THE OTHER WAY. There is no comment lexer here, so a
        // `#` cannot switch the rule off - which is the shape `telekom/sutura#447` is about one
        // gate over, where a `#` in front of a licence check left the rule answering `true`.
        assert_eq!(deprecated("  # was: pkgs.stdenv.isDarwin\n").len(), 1);
    }

    #[test]
    fn the_scope_is_this_repositorys_own_nix_and_not_a_vendored_tree() {
        assert!(is_nix_expression("nix/jscpd.nix"));
        assert!(is_nix_expression("flake.nix"));
        assert!(!is_nix_expression("vendor/somewhere/default.nix"));
        // Case-insensitive, so a spelling the filesystem accepts is not a file this gate ignores.
        assert!(is_nix_expression("nix/Shell.NIX"));
        assert!(!is_nix_expression("xtask/src/nix_platform.rs"));
        assert!(!is_nix_expression("docs/architecture.md"));
    }
}
