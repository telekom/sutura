//! No `#[expect]` on a count-threshold lint.
//!
//! A threshold lint's cause is not local to the code the attribute sits on. `too_many_lines`,
//! `too_many_arguments` and `cognitive_complexity` all have a NUMBER as their cause, and the
//! number is a property of the surrounding function - so two branches can each move that
//! number correctly and only their merge is wrong: one side lengthens the function and
//! suppresses it honestly, the other lifts it under the threshold, and the merged
//! suppression no longer fires. Under `-D warnings` an `#[expect]` that stops firing is an
//! error, so the merge fails on code where neither side did anything wrong. Five instances of
//! the shape landed in one day; three were found only after a merge.
//!
//! The categorical lints are different - `cast_precision_loss`, `float_cmp`, and the rest have
//! a cause that is the code the attribute sits on, which an unrelated refactor cannot remove.
//! This gate leaves those alone: `#[expect]` over `#[allow]` remains the right default for them.
//!
//! The honest alternatives for a threshold lint are to split the function, or to raise the
//! threshold in `clippy.toml` deliberately for the whole workspace - and the gate says so in
//! its message rather than merely banning the attribute.

use crate::Verdict;
use crate::repo;

/// The count-threshold lints an `#[expect]` may not name. Bare names, because the gate's own
/// message builds the `clippy::` prefix so this source cannot report itself.
const FORBIDDEN: &[&str] = &["too_many_lines", "too_many_arguments", "cognitive_complexity"];

/// One banned attribute, located for the report.
struct Violation {
    /// 1-based line of the `#[expect(`.
    line: usize,
    /// The banned lint, without the `clippy::` prefix.
    lint: &'static str,
}

pub(crate) fn run(_args: &[String]) -> Verdict {
    let Some(repo::RepoFiles { root, files }) = repo::all_files() else {
        eprintln!("xtask check-expect-thresholds: could not determine the repo root");
        return Verdict::Fail;
    };

    let mut violations: Vec<(String, Violation)> = Vec::new();
    let mut rs_files = 0usize;
    for rel in files {
        if std::path::Path::new(&rel).extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        rs_files += 1;
        let Ok(code) = std::fs::read_to_string(root.join(&rel)) else {
            continue;
        };
        let mut found = Vec::new();
        scan(&code, &mut found);
        for v in found {
            violations.push((rel.clone(), v));
        }
    }

    if violations.is_empty() {
        println!("xtask check-expect-thresholds: ok - no threshold-lint #[expect(] in {rs_files} file(s)");
        return Verdict::Pass;
    }

    eprintln!("xtask check-expect-thresholds: FAILED - #[expect] on a count-threshold lint");
    for (path, v) in &violations {
        eprintln!("  {path}:{}: an #[expect] names clippy::{}", v.line, v.lint);
    }
    eprintln!();
    eprintln!("A threshold lint's cause is a number, so an unrelated branch can remove it and the");
    eprintln!("merge fires an #[expect] that no longer holds. Split the function, or raise the");
    eprintln!("threshold in clippy.toml deliberately for the whole workspace - either beats a");
    eprintln!("suppression whose correctness is a property of the merge.");
    Verdict::Fail
}

/// Collect every banned `#[expect(...)]` in `code`, ignoring comments.
///
/// Doc comments (`///`, `//!`) and `/* */` are comments, so a sentence that merely names the
/// class is not a violation. Only a real attribute is: the check requires the literal
/// `#[expect(` and one of the banned `clippy::<lint>` paths inside its argument list. String
/// literals are left alone, which is fine because a string never carries the `clippy::<lint>`
/// path shape an attribute would - and this gate's own tests build that shape from parts so
/// this source cannot report itself.
fn scan(code: &str, out: &mut Vec<Violation>) {
    let blanked = blank_comments(code);
    for (at, _) in blanked.match_indices("#[expect(") {
        let Some(close) = attr_end(&blanked, at) else {
            continue;
        };
        let Some(body) = blanked.get(at..close) else {
            continue;
        };
        for lint in FORBIDDEN {
            if body_matches(body, lint) {
                out.push(Violation { line: line_of(&blanked, at), lint });
            }
        }
    }
}

/// Is `clippy::<lint>` present in `body`? Built from pieces so this source never contains the
/// literal the gate looks for.
fn body_matches(body: &str, lint: &str) -> bool {
    let needle = format!("clippy::{lint}");
    body.contains(&needle)
}

/// Blank out line and block comments with spaces, preserving every newline.
///
/// Width-preserving per character (a multibyte char becomes one space) so byte offsets no
/// longer line up with the input for multibyte text - but newline POSITIONS are preserved, and
/// every search here works in the blanked string's own space, so nothing goes out of step.
fn blank_comments(code: &str) -> String {
    let mut out = String::with_capacity(code.len());
    let mut chars = code.char_indices().peekable();
    while let Some((_, c)) = chars.next() {
        let next = chars.peek().map(|(_, n)| *n);
        match (c, next) {
            // `//` to end of line.
            ('/', Some('/')) => {
                chars.next();
                out.push_str("  ");
                for (_, c2) in chars.by_ref() {
                    if c2 == '\n' {
                        out.push('\n');
                        break;
                    }
                    out.push(' ');
                }
            }
            // `/*` to the matching `*/`, nesting counted.
            ('/', Some('*')) => {
                chars.next();
                out.push_str("  ");
                let mut depth = 1usize;
                loop {
                    let a = chars.peek().map(|(_, n)| *n);
                    let b = chars.clone().nth(1).map(|(_, n)| n);
                    match (a, b) {
                        (Some('*'), Some('/')) => {
                            chars.next();
                            chars.next();
                            out.push_str("   ");
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        (Some('/'), Some('*')) => {
                            chars.next();
                            chars.next();
                            out.push_str("   ");
                            depth += 1;
                        }
                        (Some(c2), _) => {
                            chars.next();
                            if c2 == '\n' {
                                out.push('\n');
                            } else {
                                out.push(' ');
                            }
                        }
                        (None, _) => break,
                    }
                }
            }
            _ => out.push(c),
        }
    }
    out
}

/// Byte offset past the `)` that closes the `#[expect(` at `at`.
///
/// Depth-counts the parentheses. A `)` inside a reason string would end the count early, but a
/// reason string never carries a banned lint, so the narrowed body still contains one if it is
/// there.
fn attr_end(blanked: &str, at: usize) -> Option<usize> {
    let open = at + "#[expect(".len();
    let after = blanked.get(open..)?;
    let mut depth = 1usize;
    for (i, b) in after.as_bytes().iter().enumerate() {
        match b {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(open + i + 1);
                }
            }
            _ => {}
        }
    }
    None
}

/// The 1-based line holding byte offset `at`, from the blanked string's newlines.
fn line_of(blanked: &str, at: usize) -> usize {
    blanked.match_indices('\n').take_while(|(i, _)| *i < at).count() + 1
}

#[cfg(test)]
mod tests {
    use super::{FORBIDDEN, attr_end, scan, Violation};

    fn lints(haystack: &str) -> Vec<(&'static str, usize)> {
        let mut found = Vec::new();
        scan(haystack, &mut found);
        found.sort_by_key(|v| v.line);
        found.into_iter().map(|v: Violation| (v.lint, v.line)).collect()
    }

    /// Build an attribute for `lint` without the source ever containing `clippy::<lint>`
    /// verbatim, so this module's own tests cannot be reported by the gate they test.
    fn expect(lint: &str) -> String {
        format!("#[expect(clippy::{lint})]")
    }

    #[test]
    fn every_banned_lint_is_caught() {
        for lint in FORBIDDEN {
            assert_eq!(lints(&expect(lint)), vec![(*lint, 1)]);
        }
    }

    #[test]
    fn a_banned_lint_with_others_in_the_same_attribute_is_caught() {
        let a = format!("#[expect(clippy::float_cmp, clippy::{}, reason = \"x\")]", FORBIDDEN[0]);
        assert_eq!(lints(&a), vec![(FORBIDDEN[0], 1)]);
    }

    #[test]
    fn a_categorical_expect_is_not_caught() {
        assert!(lints("#[expect(clippy::float_arithmetic, reason = \"by design\")]").is_empty());
    }

    #[test]
    fn an_allow_is_not_caught() {
        assert!(lints(&format!("#[allow(clippy::{})]", FORBIDDEN[0])).is_empty());
    }

    #[test]
    fn a_comment_naming_the_class_is_not_caught() {
        // The real confound: prose that merely names the attribute must not report.
        let code = format!("// No `#[expect(clippy::{})]` any more, and that is worth a line\nfn f() {{}}\n", FORBIDDEN[0]);
        assert!(lints(&code).is_empty());
    }

    #[test]
    fn a_block_comment_naming_the_class_is_not_caught() {
        let code = format!("/* Never: #[expect(clippy::{})] */\nfn f() {{}}\n", FORBIDDEN[0]);
        assert!(lints(&code).is_empty());
    }

    #[test]
    fn a_multiline_expect_is_caught_on_its_real_line() {
        let a = format!("#[expect(\n    clippy::{},\n    reason = \"enough\"\n)]\nfn f() {{}}", FORBIDDEN[1]);
        assert_eq!(lints(&a), vec![(FORBIDDEN[1], 1)]);
    }

    #[test]
    fn line_numbers_count_real_code() {
        let code = format!("mod a {{\n    {}\n}}\n", expect(FORBIDDEN[1]));
        assert_eq!(lints(&code), vec![(FORBIDDEN[1], 2)]);
    }

    #[test]
    fn a_commented_out_attribute_on_a_later_line_is_not_caught() {
        let code = format!("fn f() {{}}\n// {}\n", expect(FORBIDDEN[2]));
        assert!(lints(&code).is_empty());
    }

    #[test]
    fn attr_end_respects_nesting() {
        assert_eq!(attr_end("#[expect()]", 0), Some(10));
        assert_eq!(attr_end("#[expect(a, f(b))]", 0), Some(17));
        assert!(attr_end("#[expect(unclosed", 0).is_none());
    }
}
