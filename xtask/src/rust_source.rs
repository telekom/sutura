//! Reading Rust source as text: the comment blanker, the line locator and the call terminator.
//!
//! Its own module because more than one gate needs them, which is the rule [`crate::repo`]
//! states for the same reason - a copy in each would be two things to keep in step.
//! `check-expect-thresholds` declared `blank_comments` and `line_of` privately first, then the
//! needle rule needed `call_end`; the move rather than a second private copy is the point.
//! `skip_string`, `skip_raw` and `skip_tick` moved here from the needle rule for the same reason
//! (#702): the blanker needed them too, so a second copy in `blank_comments` would have been the
//! very duplication this module exists to avoid.
//!
//! None of these parses Rust. They exist so a gate that searches source TEXT does not report
//! prose that merely names the shape it looks for - which is not a nicety: every assertion gate
//! has to write the shape down somewhere to explain itself, and the shape is what it finds.

/// Blank out line and block comments with spaces, preserving every newline.
///
/// String, char and raw-string literals are copied verbatim rather than scanned: a `//` or `/*`
/// inside a literal (a URL, a path) is not a comment starting, so it can no longer blank a real
/// `#[expect(` that follows later on the same physical line (#702).
///
/// Width-preserving per character (a multibyte char becomes one space) so byte offsets no
/// longer line up with the input for multibyte text - but newline POSITIONS are preserved, and
/// every search here works in the blanked string's own space, so nothing goes out of step.
pub(crate) fn blank_comments(code: &str) -> String {
    let bytes = code.as_bytes();
    let mut out = String::with_capacity(code.len());
    let mut i = 0usize;
    while let Some(b) = bytes.get(i).copied() {
        match b {
            b'"' => {
                let end = skip_string(bytes, i);
                push_verbatim(code, i, end, &mut out);
                i = end;
            }
            b'\'' => {
                let end = skip_tick(bytes, i);
                push_verbatim(code, i, end, &mut out);
                i = end;
            }
            b'r' if matches!(bytes.get(i + 1).copied(), Some(b'#' | b'"')) => {
                let end = skip_raw(bytes, i);
                push_verbatim(code, i, end, &mut out);
                i = end;
            }
            // `//` to end of line.
            b'/' if bytes.get(i + 1).copied() == Some(b'/') => {
                out.push_str("  ");
                i += 2;
                while bytes.get(i).copied().is_some_and(|b2| b2 != b'\n') {
                    out.push(' ');
                    i += char_len(code, i);
                }
            }
            // `/*` to the matching `*/`, nesting counted.
            b'/' if bytes.get(i + 1).copied() == Some(b'*') => {
                out.push_str("  ");
                i += 2;
                let mut depth = 1usize;
                while depth > 0 {
                    match (bytes.get(i).copied(), bytes.get(i + 1).copied()) {
                        (Some(b'*'), Some(b'/')) => {
                            out.push_str("  ");
                            i += 2;
                            depth -= 1;
                        }
                        (Some(b'/'), Some(b'*')) => {
                            out.push_str("  ");
                            i += 2;
                            depth += 1;
                        }
                        (Some(b'\n'), _) => {
                            out.push('\n');
                            i += 1;
                        }
                        (Some(_), _) => {
                            out.push(' ');
                            i += char_len(code, i);
                        }
                        (None, _) => break,
                    }
                }
            }
            _ => {
                if let Some(ch) = char_at(code, i) {
                    out.push(ch);
                    i += ch.len_utf8();
                } else {
                    i += 1;
                }
            }
        }
    }
    out
}

/// Copy `code[from..to]` verbatim, or nothing if `from..to` is not a valid byte range - which
/// does not happen here, since every `skip_*` return lands right after an ASCII delimiter.
fn push_verbatim(code: &str, from: usize, to: usize, out: &mut String) {
    if let Some(s) = code.get(from..to) {
        out.push_str(s);
    }
}

/// The char starting at byte offset `i`, or `None` past the end or off a char boundary.
fn char_at(code: &str, i: usize) -> Option<char> {
    code.get(i..)?.chars().next()
}

fn char_len(code: &str, i: usize) -> usize {
    char_at(code, i).map_or(1, char::len_utf8)
}

/// Advance past a `"..."` string, `\` escapes respected. Returns the index past the closing `"`.
pub(crate) fn skip_string(bytes: &[u8], at: usize) -> usize {
    let mut j = at + 1;
    while j < bytes.len() {
        match bytes.get(j).copied() {
            Some(b'\\') => j = (j + 2).min(bytes.len()),
            Some(b'"') => return j + 1,
            _ => j += 1,
        }
    }
    bytes.len()
}

/// Advance past a `r##"..."##` raw string starting at the `r`. Returns the index past its close.
pub(crate) fn skip_raw(bytes: &[u8], at: usize) -> usize {
    let mut j = at + 1;
    let mut hashes = 0usize;
    while bytes.get(j).copied() == Some(b'#') {
        hashes += 1;
        j += 1;
    }
    if bytes.get(j).copied() != Some(b'"') {
        // A raw IDENTIFIER like `r#type` - not a raw string; let it scan as ordinary code.
        return at + 1;
    }
    j += 1;
    while j < bytes.len() {
        if bytes.get(j).copied() == Some(b'"')
            && bytes
                .get(j + 1..)
                .is_some_and(|tail| tail.iter().take(hashes).all(|&b| b == b'#'))
        {
            return j + 1 + hashes;
        }
        j += 1;
    }
    bytes.len()
}

/// Advance past a `'` that opens a char literal (whose escape `\u{..}` holds a brace that must
/// not count) or a lifetime (which holds nothing special and just hands the ident back).
pub(crate) fn skip_tick(bytes: &[u8], at: usize) -> usize {
    match bytes.get(at + 1).copied() {
        // `'\n'`, `'\u{7b}'`: find the closing quote with `\\` respected.
        Some(b'\\') => {
            let mut j = at + 2;
            while j < bytes.len() {
                match bytes.get(j).copied() {
                    Some(b'\\') => j = (j + 2).min(bytes.len()),
                    Some(b'\'') => return j + 1,
                    _ => j += 1,
                }
            }
            bytes.len()
        }
        // `' '`, `'!'`, `''`: a non-ident char literal, closed on the next `'`.
        Some(c) if !is_ident(c) => {
            let mut j = at + 2;
            while j < bytes.len() && bytes.get(j).copied() != Some(b'\'') {
                j += 1;
            }
            (j + 1).min(bytes.len())
        }
        // An ident after the quote: a char `'x'` (closed next byte) or a lifetime `'static`
        // (just the quote - the ident scans as ordinary code).
        Some(_) => {
            if bytes.get(at + 2).copied() == Some(b'\'') {
                at + 3
            } else {
                at + 1
            }
        }
        None => bytes.len(),
    }
}

const fn is_ident(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
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

#[cfg(test)]
mod tests {
    use super::blank_comments;

    /// #702: a `//` inside a string literal is not a comment starting - so a URL survives, and
    /// nothing after it on the line goes dark.
    #[test]
    fn a_double_slash_inside_a_string_is_not_a_comment() {
        let code = "let s = \"http://x\"; // real\n";
        let blanked = blank_comments(code);
        assert!(
            blanked.contains("http://x"),
            "a `//` inside a string must not blank the string: {blanked:?}"
        );
        // The real trailing comment is still blanked.
        assert!(
            !blanked.contains("real"),
            "the real // comment must still be blanked: {blanked:?}"
        );
    }

    #[test]
    fn a_block_comment_marker_inside_a_string_is_not_a_comment() {
        let code = "let s = \"a/*b*/c\";\nlet t = 1;\n";
        let blanked = blank_comments(code);
        assert!(
            blanked.contains("a/*b*/c") && blanked.contains("let t = 1;"),
            "a `/*` inside a string must not start a block comment: {blanked:?}"
        );
    }

    #[test]
    fn nested_block_comments_still_close_together() {
        let code = "/* outer /* inner */ still outer */code();\n";
        let blanked = blank_comments(code);
        assert!(blanked.trim_start().starts_with("code();"), "nesting: {blanked:?}");
    }

    #[test]
    fn newline_positions_are_unchanged() {
        let code = "a();\n// one\nb();\n/* two\nlines */\nc();\n";
        let blanked = blank_comments(code);
        assert_eq!(
            code.matches('\n').count(),
            blanked.matches('\n').count(),
            "blanking must not add or remove newlines: {blanked:?}"
        );
    }

    #[test]
    fn a_char_literal_containing_a_slash_is_not_a_comment() {
        let code = "let c = '/'; // real\n";
        let blanked = blank_comments(code);
        assert!(blanked.contains("'/'"), "a char literal must survive: {blanked:?}");
        assert!(
            !blanked.contains("real"),
            "the trailing comment must still blank: {blanked:?}"
        );
    }

    #[test]
    fn a_raw_string_survives_verbatim() {
        let code = "let s = r\"http://x\"; // real\n";
        let blanked = blank_comments(code);
        assert!(blanked.contains("http://x"), "a raw string must survive: {blanked:?}");
        assert!(
            !blanked.contains("real"),
            "the trailing comment must still blank: {blanked:?}"
        );
    }
}
