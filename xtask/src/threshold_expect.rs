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
//!
//! The gate's second rule is needle-side. A substring assertion whose needle is a PROPER
//! SUBSTRING of another `.contains(..)` needle in the same function body can inflate a count:
//! when the longer needle matches, its shorter suffix matches for the same reason, so both
//! assertions firing is one fact reported twice. Non-literal needles (a `const`, `.to_string()`)
//! are invisible to the raw scan by design; grouping is per-fn, per-file, and a cross-file or
//! cross-fn pair is never compared. `#429` drives the shape and its measured cost.

use crate::Verdict;
use crate::repo;
use crate::rust_source;

/// The count-threshold lints an `#[expect]` may not name. Bare names, because the gate's own
/// message builds the `clippy::` prefix so this source cannot report itself.
const FORBIDDEN: &[&str] = &["too_many_lines", "too_many_arguments", "cognitive_complexity"];

/// Rust source, and nothing else. A [`repo::Scope`]: a bare `fn`, so it cannot count subjects and
/// is not handed the content - an ordinal narrowing has nowhere to keep its counter. It is NOT
/// sealed against a predicate that opens the file itself; [`repo::Scope`] measures that fail-open
/// and says which of the five printed numbers it can and cannot move.
fn rust_source(rel: &str) -> bool {
    std::path::Path::new(rel)
        .extension()
        .and_then(std::ffi::OsStr::to_str)
        .is_some_and(|ext| ext == "rs")
}

/// One banned attribute, located for the report.
struct Violation {
    /// 1-based line of the `#[expect(`.
    line: usize,
    /// The banned lint, without the `clippy::` prefix.
    lint: &'static str,
}

pub(crate) fn run(_args: &[String]) -> Verdict {
    let census = match repo::all_files() {
        Ok(census) => census,
        Err(why) => {
            eprintln!("xtask check-expect-thresholds: FAILED - {}", why.describe());
            return Verdict::Fail;
        }
    };

    let mut violations: Vec<(String, Violation)> = Vec::new();
    let mut needle_violations: Vec<(String, NeedleViolation)> = Vec::new();
    // The loop lives inside `inspect`, so this gate never holds the listing and `.take(n)` has
    // nowhere to be written. `rs_files` is gone with it: the count in the success line is now the
    // census's own length rather than a local this loop increments.
    //
    // **And the read lives there too.** This closure used to open the file and then classify what
    // happened, which put all three instruments behind one arm it wrote itself: answering `Judged`
    // for a file it could not open discharged the anchor, moved the numerator and printed a
    // verdict byte-identical to a clean tree's at exit 0. It cannot make that claim now - it is
    // handed the bytes of a subject the census opened, and it returns nothing.
    //
    // `must_judge` replaces the `rs_files == 0` floor, and is strictly stronger than it. A floor
    // over a count is satisfiable by reading almost anything; naming a file the gate cannot have a
    // verdict without survives a scope predicate that stopped matching. This one is the crate's
    // `[[bin]]` root: every `.rs` scan in the tree can name it, and a binary cannot lose its root
    // file, so it will not vanish. It held the task table until `#610` moved that to
    // `task_table.rs`; the anchor never depended on the table, only on the file existing.
    // Named with its type at the call site: a [`repo::Scope`] is a bare `fn` pointer, so a
    // closure - the only place an ordinal counter could live - does not compile here.
    let scope: repo::Scope = rust_source;
    let inspected = match census.inspect(&["xtask/src/main.rs"], scope, |rel, bytes| {
        // Lossy rather than `read_to_string`, which used to turn a `.rs` file that is not valid
        // UTF-8 into an `Unreachable` - a file that WAS reached, and that rustc would reject on its
        // own. A subject the census opened is judged; how well is this gate's business.
        let text = String::from_utf8_lossy(bytes);
        let mut found = Vec::new();
        scan(&text, &mut found);
        for v in found {
            violations.push((String::from(rel), v));
        }
        // The needle rule runs in the same closure and census, so neither rule can go green by
        // the other not running - and nothing is derived outside this loop.
        for nv in needle_lints(&text) {
            needle_violations.push((String::from(rel), nv));
        }
    }) {
        Ok(inspected) => inspected,
        Err(why) => {
            eprintln!("xtask check-expect-thresholds: FAILED - {}", why.describe());
            return Verdict::Fail;
        }
    };

    if violations.is_empty() && needle_violations.is_empty() {
        println!(
            "xtask check-expect-thresholds: ok - no threshold-lint #[expect(] and no fn-local \
             needle substring - {}",
            inspected.verdict()
        );
        return Verdict::Pass;
    }

    if !violations.is_empty() {
        eprintln!("xtask check-expect-thresholds: FAILED - #[expect] on a count-threshold lint");
        for (path, v) in &violations {
            eprintln!("  {path}:{}: an #[expect] names clippy::{}", v.line, v.lint);
        }
        eprintln!();
        eprintln!("A threshold lint's cause is a number, so an unrelated branch can remove it and the");
        eprintln!("merge fires an #[expect] that no longer holds. Split the function, or raise the");
        eprintln!("threshold in clippy.toml deliberately for the whole workspace - either beats a");
        eprintln!("suppression whose correctness is a property of the merge.");
    }
    if !needle_violations.is_empty() {
        eprintln!("xtask check-expect-thresholds: FAILED - fn-local substring needle");
        for (path, v) in &needle_violations {
            eprintln!(
                "  {path}:{}: needle {:?} is a proper substring of {:?} in the same fn",
                v.line, v.needle, v.into
            );
        }
        eprintln!();
        eprintln!("Two needles in one fn where one is a proper substring of the other can inflate a");
        eprintln!("count: the longer match is already counted when the shorter fires. Drop or widen");
        eprintln!("the shorter needle, or record the pair in ALLOWED with a reason it is safe.");
    }
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
    let blanked = rust_source::blank_comments(code);
    for (at, _) in blanked.match_indices("#[expect(") {
        // The generalised `call_end` needs the `(` itself; this attribute places it at the last
        // byte of the `#[expect(` prefix.
        let open = at + "#[expect(".len() - 1;
        let Some(close) = rust_source::call_end(&blanked, open) else {
            continue;
        };
        let Some(body) = blanked.get(at..close) else {
            continue;
        };
        for lint in FORBIDDEN {
            if body_matches(body, lint) {
                out.push(Violation {
                    line: rust_source::line_of(&blanked, at),
                    lint,
                });
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

/// Fn-local needle collisions that are reviewed exemptions rather than fixes, keyed by the
/// colliding (shorter) needle's text. The liveness pass over the tree on 2026-09-08 enumerated
/// today's real fn-local collisions; the two truly redundant pairs were fixed at the source and
/// the rest are each recorded here with the reason the collision is not a count inflation, so
/// the rule is honest over the tree rather than failing blinkered. A needle's text is how it is
/// keyed, so an entry exempts that needle wherever it appears.
const ALLOWED: &[&str] = &[
    // Different variables, opposite verdicts: `with` must contain the full sentence while
    // `without` must NOT contain the bare operation name. Not the same event counted twice.
    "Call `catalog`",
    // One differential side checks the one-source refusal for the full phrase while the
    // two-source side only needs the word; the two variables are different sides.
    "finite",
    // The bare subject's record is asserted to LACK `acting=` while the agent's record (the
    // next line) is asserted to HAVE it; the prefix needle is the absence check.
    "acting=",
    // The two-subject leg's `grace` needle is documented beside the assertion as safe by
    // arithmetic (4 x 64^-5); its longer sibling `for:grace@example.com` is the material half.
    "grace",
    // The structured half and the rendered text half say different things about the same
    // sentence: flat JSON must OMIT it, while the text half must carry the markdown-`>`
    // blockquoted spelling. Absence vs presence, not inflation.
    "Revenue, in minor units.",
    // A hostile cell keeps the arbitrary word `region` while the structured `Sales region.`
    // description must be omitted: presence of the word vs absence of the description.
    "region",
    // The quoted rendering must carry the `> # SYSTEM` form while the omitted half must lack
    // both spellings: positive quoted check vs negative omitted check.
    "# SYSTEM",
    // Same two halves as "# SYSTEM": present blockquoted on the rendered text, absent flat on
    // the omitted half.
    "definitions: v99",
    // Dialect arms are mutually exclusive: one default must spell `NULLS LAST`, the three that
    // collapse it must not spell `NULLS` at all. Never both true for one dialect.
    "NULLS",
    // Mutually exclusive venue arms: one arm asserts the remedy does NOT send the reader to
    // `dev-up`, another asserts the correct command IS `just dev-up`.
    "dev-up",
    // Opposite verdicts over different runs: nothing-to-revert must lack `not reverted`, the
    // full report names what `not reverted: crates/x/Cargo.toml`.
    "not reverted",
    // Opposite verdicts: with tests in scope the run must NOT say `proves nothing`, the plain
    // run must still say `proves nothing here`.
    "proves nothing",
    // Two spellings of one recipe name must not be confused: the name is `dev-endpoint`, never
    // the `@dev-endpoint` form; presence of one implies absence of the other.
    "dev-endpoint",
];

/// One fn-local needle collision, located for the report.
struct NeedleViolation {
    /// 1-based line of the colliding (shorter) needle.
    line: usize,
    /// The shorter needle, which is a proper substring of a longer one in the same fn.
    needle: String,
    /// The longer needle it is a proper substring of.
    into: String,
}

/// One collected `.contains(..)` needle literal.
struct Needle {
    /// The argument's inner bytes, quotes stripped, not unescaped (raw byte-substring).
    text: String,
    /// 1-based line of the `.contains(`.
    line: usize,
    /// The enclosing fn-body block. Shared by every needle in one function body; a needle with
    /// no enclosing fn gets a fresh id, so two such needles are never compared.
    group: u32,
}

/// Every needle collision in `code`: one needle a proper substring of another in the same fn.
///
/// The scan is raw: it blanks comments (so a `.contains(..)` in prose never counts), then walks
/// the bytes once, skipping whole string/char/raw-string literals so a brace or a `.contains(..)`
/// inside one is not read as code, tracks brace depth to tell which `fn` body owns each
/// `.contains(..)` ([`rust_source::call_end`] delimits the argument), and keeps only a whole
/// quoted literal argument - a `const`, a method call or a bare ident is skipped by design (#429).
fn needle_lints(code: &str) -> Vec<NeedleViolation> {
    let blanked = rust_source::blank_comments(code);
    let bytes = blanked.as_bytes();
    let mut needles: Vec<Needle> = Vec::new();
    let mut depth = 0usize;
    let mut next_group = 1u32;
    // Open fn bodies as (group id, the brace depth the body opened at).
    let mut active: Vec<(u32, usize)> = Vec::new();
    let mut fn_header = false;
    let mut i = 0usize;

    while i < bytes.len() {
        match bytes.get(i) {
            // Skip whole literals so their braces and any `.contains(` inside are not code.
            Some(b'"') => {
                i = skip_string(bytes, i);
                continue;
            }
            Some(b'\'') => {
                i = skip_tick(bytes, i);
                continue;
            }
            Some(b'r') if matches!(bytes.get(i + 1).copied(), Some(b'#' | b'"')) => {
                i = skip_raw(bytes, i);
                continue;
            }
            Some(b'{') => {
                depth = depth.saturating_add(1);
                if fn_header {
                    // The first `{` after a `fn` keyword is the function body.
                    active.push((next_group, depth));
                    next_group += 1;
                    fn_header = false;
                }
                i += 1;
                continue;
            }
            Some(b'}') => {
                if let Some(&(_, open)) = active.last() {
                    // The body's own `}` brings depth back to the level it opened at.
                    if depth == open {
                        active.pop();
                    }
                }
                depth = depth.saturating_sub(1);
                i += 1;
                continue;
            }
            _ => {}
        }
        // `fn a();` or `fn a = ..` is a signature or item, not a body: no `{` follows.
        if fn_header && matches!(bytes.get(i).copied(), Some(b';' | b'=')) {
            fn_header = false;
        }
        if is_fn_word(bytes, i) {
            fn_header = true;
            i += 2;
            continue;
        }
        if blanked.get(i..).is_some_and(|rest| rest.starts_with(".contains(")) {
            let open = i + ".contains".len();
            if let Some(close) = rust_source::call_end(&blanked, open)
                && let Some(arg) = blanked.get(open + 1..close - 1)
                && let Some(text) = strip_literal(arg)
                && !ALLOWED.contains(&text)
            {
                let group = active.last().map_or_else(
                    || {
                        let g = next_group;
                        next_group += 1;
                        g
                    },
                    |(id, _)| *id,
                );
                needles.push(Needle {
                    text: text.to_owned(),
                    line: rust_source::line_of(&blanked, i),
                    group,
                });
            }
            i += ".contains(".len();
            continue;
        }
        i += 1;
    }

    collisions(&needles)
}

/// Advance past a `"..."` string, `\` escapes respected. Returns the index past the closing `"`.
fn skip_string(bytes: &[u8], at: usize) -> usize {
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
fn skip_raw(bytes: &[u8], at: usize) -> usize {
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
fn skip_tick(bytes: &[u8], at: usize) -> usize {
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

/// Is `fn` at `i` a keyword, not the suffix of an identifier?
fn is_fn_word(bytes: &[u8], i: usize) -> bool {
    if bytes.get(i) != Some(&b'f') || bytes.get(i + 1) != Some(&b'n') {
        return false;
    }
    let before = bytes.get(i.wrapping_sub(1)).copied().unwrap_or(b' ');
    let after = bytes.get(i + 2).copied().unwrap_or(b' ');
    !is_ident(before) && !is_ident(after)
}

const fn is_ident(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// If `arg` is exactly one quoted literal with nothing after it, return its inner bytes (raw);
/// otherwise `None` - a `const`, a method call or a bare ident are never compared (#429).
fn strip_literal(arg: &str) -> Option<&str> {
    let bytes = arg.as_bytes();
    let quote = bytes.first().copied()?;
    if quote != b'"' && quote != b'\'' {
        return None;
    }
    let mut j = 1;
    while j < bytes.len() {
        match bytes.get(j).copied() {
            Some(b'\\') => j = (j + 2).min(bytes.len()),
            Some(c) if c == quote => {
                // The literal must be the WHOLE argument: nothing but whitespace after the close.
                if arg.get(j + 1..).is_some_and(|tail| tail.trim().is_empty()) {
                    return arg.get(1..j);
                }
                return None;
            }
            _ => j += 1,
        }
    }
    None
}

/// The rule: needle A is flagged when it is a PROPER SUBSTRING of a longer needle B in the same
/// fn-body group. Deduplicated so a repeated needle text is reported once per line.
fn collisions(needles: &[Needle]) -> Vec<NeedleViolation> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for (ai, a) in needles.iter().enumerate() {
        let mut into: Option<&Needle> = None;
        for (bi, b) in needles.iter().enumerate() {
            if bi == ai {
                continue;
            }
            if a.group == b.group
                && a.text.len() < b.text.len()
                && b.text.contains(&a.text)
                && into.is_none_or(|cur| cur.text.len() < b.text.len())
            {
                into = Some(b);
            }
        }
        let Some(b) = into else { continue };
        if seen.insert((a.line, a.text.clone())) {
            out.push(NeedleViolation {
                line: a.line,
                needle: a.text.clone(),
                into: b.text.clone(),
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{FORBIDDEN, Violation, scan};

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
        assert!(
            lints("#[expect(clippy::float_arithmetic, reason = \"by design\")]").is_empty(),
            "a categorical expect is not a lint to forbid"
        );
    }

    #[test]
    fn an_allow_is_not_caught() {
        assert!(
            lints(&format!("#[allow(clippy::{})]", FORBIDDEN[0])).is_empty(),
            "an allow is not an expect to forbid"
        );
    }

    #[test]
    fn a_comment_naming_the_class_is_not_caught() {
        // The real confound: prose that merely names the attribute must not report.
        let code = format!(
            "// No `#[expect(clippy::{})]` any more, and that is worth a line\nfn f() {{}}\n",
            FORBIDDEN[0]
        );
        assert!(lints(&code).is_empty(), "a comment naming the class is not a lint to catch");
    }

    #[test]
    fn a_block_comment_naming_the_class_is_not_caught() {
        let code = format!("/* Never: #[expect(clippy::{})] */\nfn f() {{}}\n", FORBIDDEN[0]);
        assert!(
            lints(&code).is_empty(),
            "a block comment naming the class is not a lint to catch"
        );
    }

    #[test]
    fn a_multiline_expect_is_caught_on_its_real_line() {
        let a = format!(
            "#[expect(\n    clippy::{},\n    reason = \"enough\"\n)]\nfn f() {{}}",
            FORBIDDEN[1]
        );
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
        assert!(lints(&code).is_empty(), "a commented-out attribute is not a live lint");
    }

    #[test]
    fn call_end_respects_nesting() {
        // `call_end` takes the byte offset of the opening `(`; the `(` in `#[expect(..)`
        // sits at offset 8.
        use crate::rust_source::call_end;
        assert_eq!(call_end("#[expect()]", 8), Some(10));
        assert_eq!(call_end("#[expect(a, f(b))]", 8), Some(17));
        assert!(call_end("#[expect(unclosed", 8).is_none());
    }

    /// The `(line, needle, into)` triples the needle rule reports.
    type NeedleHit = (usize, String, String);

    /// Run the needle rule and return the triples it flags.
    fn needles(haystack: &str) -> Vec<NeedleHit> {
        super::needle_lints(haystack)
            .into_iter()
            .map(|v| (v.line, v.needle, v.into))
            .collect()
    }

    #[test]
    fn a_shorter_needle_that_is_a_substring_of_a_longer_sibling_is_flagged() {
        let code = concat!(
            "fn f(s: &str, t: &str) {\n",             // 1
            "    assert!(s.contains(\"queen\"));\n",  // 2
            "    assert!(t.contains(\"queens\"));\n", // 3
            "}\n",
        );
        assert_eq!(needles(code), vec![(2, "queen".into(), "queens".into())]);
    }

    #[test]
    fn needs_that_have_no_longer_sibling_are_not_flagged() {
        let code = concat!(
            "fn f(s: &str) {\n",                         // 1
            "    assert!(s.contains(\"kid\"));\n",       // 2
            "    assert!(s.contains(\"grace\"));\n",     // 3
            "    assert!(s.contains(i.to_string()));\n", // 4
            "    assert!(s.contains(PORT));\n",          // 5
            "    assert!(s.contains(\"grace\"));\n",     // 6
            "    assert!(s.contains(k));\n",             // 7
            "}\n",
        );
        assert!(needles(code).is_empty(), "needles with no longer sibling are not flagged");
    }

    #[test]
    fn a_char_needle_can_collide_with_a_longer_string_sibling() {
        let code = concat!(
            "fn f(s: &str, t: &str) {\n",         // 1
            "    assert!(s.contains('a'));\n",    // 2
            "    assert!(t.contains(\"ab\"));\n", // 3
            "}\n",
        );
        assert_eq!(needles(code), vec![(2, "a".into(), "ab".into())]);
    }

    #[test]
    fn a_collision_across_two_fns_is_not_flagged() {
        let code = concat!(
            "fn a(s: &str) {\n",                     // 1
            "    assert!(s.contains(\"grace\"));\n", // 2
            "}\n",
            "fn b(t: &str) {\n",                        // 4
            "    assert!(t.contains(\"graceful\"));\n", // 5
            "}\n",
        );
        assert!(needles(code).is_empty(), "a collision across separate fns is not a needle");
    }

    #[test]
    fn nested_blocks_are_still_one_function_body() {
        let code = concat!(
            "fn f(s: &str, t: &str, a: bool) {\n",       // 1
            "    if a {\n",                              // 2
            "        assert!(s.contains(\"queen\"));\n", // 3
            "    }\n",
            "    loop {\n",                               // 5
            "        assert!(t.contains(\"queens\"));\n", // 6
            "    }\n",
            "}\n",
        );
        assert_eq!(needles(code), vec![(3, "queen".into(), "queens".into())]);
    }

    #[test]
    fn a_commented_out_call_is_not_a_needle() {
        let code = concat!(
            "fn f(s: &str, t: &str) {\n",
            "    // assert!(s.contains(\"grace\"));\n",
            "    assert!(t.contains(\"graceful\"));\n",
            "}\n",
        );
        assert!(needles(code).is_empty(), "a commented-out call yields no needle");
    }

    #[test]
    fn a_brace_inside_a_literal_does_not_break_ownership() {
        // `format!(..)` puts a `{` in a string used as a needle's neighbour; it must not
        // unbalance the brace count and move the needle into the wrong "fn".
        let code = concat!(
            "fn f(s: &str, t: &str) {\n",                       // 1
            "    assert!(s.contains(format!(\"{{}}\", x)));\n", // 2
            "    assert!(t.contains(\"graceful\"));\n",         // 3
            "}\n",
        );
        assert!(needles(code).is_empty(), "a brace inside a literal spawns no phantom needle");
    }
}
