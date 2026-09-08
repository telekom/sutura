//! Reading Rust source as text: the comment blanker, the line locator and the call terminator.
//!
//! Its own module because more than one gate needs them, which is the rule [`crate::repo`]
//! states for the same reason - a copy in each would be two things to keep in step.
//! `check-expect-thresholds` declared `blank_comments` and `line_of` privately first, then the
//! needle rule needed `call_end`; the move rather than a second private copy is the point.
//!
//! None of these parses Rust. They exist so a gate that searches source TEXT does not report
//! prose that merely names the shape it looks for - which is not a nicety: every assertion gate
//! has to write the shape down somewhere to explain itself, and the shape is what it finds.

/// Blank out line and block comments with spaces, preserving every newline.
///
/// Width-preserving per character (a multibyte char becomes one space) so byte offsets no
/// longer line up with the input for multibyte text - but newline POSITIONS are preserved, and
/// every search here works in the blanked string's own space, so nothing goes out of step.
pub(crate) fn blank_comments(code: &str) -> String {
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

/// Byte offset past the `)` that closes the call whose opening `(` is at byte offset `open`.
///
/// Depth-counts the parentheses from `open`, so a caller can delimit a whole argument list -
/// the `#[expect(..)` body, or the argument of a `.contains(..)` - without parsing Rust.
pub(crate) fn call_end(blanked: &str, open: usize) -> Option<usize> {
    let after = blanked.get(open.checked_add(1)?..)?;
    let mut depth = 1usize;
    for (i, b) in after.as_bytes().iter().enumerate() {
        match b {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(open + i + 2);
                }
            }
            _ => {}
        }
    }
    None
}

/// The 1-based line holding byte offset `at`, from the blanked string's newlines.
pub(crate) fn line_of(blanked: &str, at: usize) -> usize {
    blanked.match_indices('\n').take_while(|(i, _)| *i < at).count() + 1
}
