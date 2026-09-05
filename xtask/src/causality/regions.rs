//! Which added lines are TEST code, decided by where they sit in the file rather than by what
//! the diff happens to contain.
//!
//! THE DEFECT THIS REPLACED. The classifier read only the added lines and looked for a
//! `#[cfg(test)]` or `mod tests` marker AMONG THEM. A test appended inside a test module that
//! already existed adds no such marker - the marker is unchanged diff context - so the first
//! added line, typically a `use`, read as an implementation change and a tests-only branch was
//! reported as `NOT MECHANICALLY SEPARABLE ... changes behaviour and adds tests in one file`.
//! That verdict is not cosmetic: it sends the author down the stated-evidence path, when the
//! gate's own tests-only branch says something different and asks for something different.
//!
//! So the regions come from the file's POST-IMAGE and every added line carries its line number.
//! Deciding is then an intersection rather than a guess.
//!
//! WHAT THIS MODULE DOES NOT DO, stated here rather than left to be discovered:
//!
//! - It does not parse Rust. Regions are found by matching `#[cfg(test)]` textually and then
//!   counting braces with a scanner ([`Braces`]) that knows about comments, string literals,
//!   raw strings and char literals - so a brace inside any of those is not counted. It does not
//!   know about macro bodies, so a `macro_rules!` arm holding an unbalanced brace would end a
//!   region in the wrong place. No such macro exists in this workspace today.
//! - Only the exact attribute `#[cfg(test)]` counts. `#[cfg(all(test, ..))]`,
//!   `#[cfg_attr(.., cfg(test))]` and `#[cfg( test )]` do not, and an added line under one of
//!   those reads as production code - the conservative direction, which asks a human.
//! - A `#[path = ".."] mod x;` declaration is not followed, so an out-of-line test module
//!   reached that way is not recognised as test code.
//! - A file whose post-image cannot be read gets NO regions, so its added lines read as
//!   production code. Same direction: ask rather than guess.

use std::ops::Range;

/// Reads a file's POST-IMAGE - its content as the change leaves it - by repo-relative path.
///
/// A parameter rather than a direct filesystem call, so classification is testable without a
/// checkout: the gate passes the working tree, the tests pass a fixed map.
pub(crate) type PostImage<'reader> = dyn Fn(&str) -> Option<String> + 'reader;

/// One added line: the text the diff added, and where that line sits in the file afterwards.
///
/// The line number is what the old shape lacked, and lacking it is the whole defect: a set of
/// added lines cannot say whether it landed inside a test module or above one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AddedLine {
    /// 1-based line number in the file's POST-IMAGE - the file as it stands after the change.
    pub(crate) number: usize,
    /// The added text, without the diff's leading `+`.
    pub(crate) text: String,
}

impl AddedLine {
    /// An added line, for a caller that has both halves.
    pub(crate) fn new(number: usize, text: impl Into<String>) -> Self {
        Self {
            number,
            text: text.into(),
        }
    }
}

/// How much of a file is test code.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum TestScope {
    /// Every line of it. A dedicated test target, a file carrying `#![cfg(test)]`, or a module
    /// whose parent declares it under `#[cfg(test)]`.
    WholeFile,
    /// These 1-based, half-open line ranges, each covering one `#[cfg(test)]` item.
    Regions(Vec<Range<usize>>),
}

impl TestScope {
    /// Is the line at `number` test code?
    pub(crate) fn covers(&self, number: usize) -> bool {
        match *self {
            Self::WholeFile => true,
            Self::Regions(ref regions) => regions.iter().any(|region| region.contains(&number)),
        }
    }
}

/// Is this file a dedicated test target, where every line is test code?
///
/// Cargo compiles `tests/` as separate integration binaries, so such a file has no
/// implementation to revert - the whole file is the test. Region detection cannot tell: an
/// integration test has no reason to write `#[cfg(test)]`, so a plain `fn t() {}` there would
/// read as an implementation change. Without this the canonical good case - an impl file plus a
/// separate integration test - was misfiled as inseparable and the gate declined to prove the
/// very shape it exists for.
fn is_dedicated_test_target(path: &str) -> bool {
    path.contains("/tests/") || path.starts_with("tests/")
}

/// What part of `path` is test code, reading the post-image through `read`.
///
/// `read` takes a repo-relative path and returns the file's current content. It is a parameter
/// rather than a direct filesystem call so the classification is testable without a checkout,
/// and so the parent-module lookup below can be exercised the same way.
pub(crate) fn scope(path: &str, read: &PostImage<'_>) -> TestScope {
    if is_dedicated_test_target(path) {
        return TestScope::WholeFile;
    }
    let Some(text) = read(path) else {
        return TestScope::Regions(Vec::new());
    };
    // `#![cfg(test)]` at the top of a file compiles the whole file only under test.
    if text.lines().any(|line| line.trim_start().starts_with("#![cfg(test)]")) {
        return TestScope::WholeFile;
    }
    // An out-of-line test module: `#[cfg(test)] mod tests;` in the parent, content in this file.
    // Common here - `sutura-http`'s `testing`, `sutura-domain`'s `refusal_tests` - and invisible
    // to region detection, because the marker is in another file entirely.
    if declared_under_cfg_test(path, read) {
        return TestScope::WholeFile;
    }
    TestScope::Regions(cfg_test_regions(&text))
}

/// Are there added lines that are plainly not test code?
///
/// Position-based: an added line outside every test region is an implementation change. Blank
/// lines, comments and attributes are exempt wherever they land, which is inherited behaviour
/// and is the one place this stays a heuristic - an added `#[derive(..)]` on a production type
/// does change behaviour and is not counted here.
pub(crate) fn has_non_test_additions(added: &[AddedLine], scope: &TestScope) -> bool {
    added
        .iter()
        .any(|line| !carries_no_behaviour(&line.text) && !scope.covers(line.number))
}

/// Blank, a comment, or an attribute: nothing that changes what the code does.
fn carries_no_behaviour(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.is_empty() || trimmed.starts_with("//") || trimmed.starts_with("#[") || trimmed.starts_with("#!")
}

/// The 1-based, half-open line ranges that `#[cfg(test)]` items cover in `text`.
///
/// Handles more than one such item, and an item that is not the last thing in the file: each
/// region ends where its item's braces balance, and the search resumes after it. A nested
/// `#[cfg(test)]` inside a region is therefore not a second region - it is already inside one.
fn cfg_test_regions(text: &str) -> Vec<Range<usize>> {
    let lines: Vec<&str> = text.lines().collect();
    let mut regions: Vec<Range<usize>> = Vec::new();
    let mut cursor = 0_usize;
    while cursor < lines.len() {
        let Some(offset) = lines
            .iter()
            .skip(cursor)
            .position(|line| line.trim_start().starts_with("#[cfg(test)]"))
        else {
            break;
        };
        let start = cursor + offset;
        let last = item_end(&lines, start);
        regions.push(start + 1..last + 2);
        cursor = last + 1;
    }
    regions
}

/// The index of the last line of the item that begins at `start`.
///
/// Two shapes, because `#[cfg(test)]` covers both. A block item - `mod tests { .. }`, a
/// function - ends where its braces balance. A DECLARATION - `mod tests;`, `use x;` - opens no
/// brace and ends at the first line that closes a statement outside a literal.
fn item_end(lines: &[&str], start: usize) -> usize {
    let mut braces = Braces::default();
    for (index, line) in lines.iter().enumerate().skip(start) {
        braces.feed(line);
        if braces.saw_open {
            if braces.depth == 0 {
                return index;
            }
            continue;
        }
        // `in_code` matters: a `;` inside a string literal closes no statement, and a
        // `const X: &str = "a;` would otherwise end the region on its first line.
        if braces.in_code() && line.trim_end().ends_with(';') {
            return index;
        }
    }
    // Unbalanced to the end of the file. Extending to EOF over-counts test code, which is the
    // wrong direction - but it can only happen on a file that does not compile.
    lines.len().saturating_sub(1)
}

/// Is `path` an out-of-line module its parent declares under `#[cfg(test)]`?
///
/// `crates/x/src/testing.rs` is test code because `crates/x/src/lib.rs` says
/// `#[cfg(test)] mod testing;`, and nothing inside the file itself says so. The DECLARATION is
/// checked rather than the name guessed: a file called `tests.rs` that a parent declares
/// unconditionally is production code, and would be treated as such.
fn declared_under_cfg_test(path: &str, read: &PostImage<'_>) -> bool {
    let Some((dir, file)) = path.rsplit_once('/') else {
        return false;
    };
    let Some(stem) = file.strip_suffix(".rs") else {
        return false;
    };
    // `a/b/mod.rs` IS module `b`, declared one level further up.
    let (name, parent) = if stem == "mod" {
        match dir.rsplit_once('/') {
            Some((up, segment)) => (segment, up),
            None => return false,
        }
    } else {
        (stem, dir)
    };
    // The four files that can declare a child module: the parent module's own file in either
    // spelling, or the crate root in either spelling.
    [
        format!("{parent}.rs"),
        format!("{parent}/mod.rs"),
        format!("{parent}/lib.rs"),
        format!("{parent}/main.rs"),
    ]
    .iter()
    .any(|candidate| candidate != path && read(candidate).is_some_and(|text| declares_test_module(&text, name)))
}

/// Does `text` declare `mod <name>;` under a `#[cfg(test)]`?
fn declares_test_module(text: &str, name: &str) -> bool {
    let lines: Vec<&str> = text.lines().collect();
    for (index, line) in lines.iter().enumerate() {
        if !is_module_declaration(line, name) {
            continue;
        }
        if line.trim_start().starts_with("#[cfg(test)]") {
            return true;
        }
        // Walk back over the attributes and comments attached to the declaration. Blank lines
        // are NOT walked over, so an attribute belonging to an earlier item cannot be borrowed.
        let mut back = index;
        while back > 0 {
            back -= 1;
            let Some(previous) = lines.get(back).map(|earlier| earlier.trim_start()) else {
                break;
            };
            if previous.starts_with("#[cfg(test)]") {
                return true;
            }
            if previous.starts_with("#[") || previous.starts_with("//") {
                continue;
            }
            break;
        }
    }
    false
}

/// Is this line the declaration `mod <name>;`, whatever its visibility?
fn is_module_declaration(line: &str, name: &str) -> bool {
    module_name(line) == Some(name)
}

/// The name in `mod NAME;`, if this line declares an out-of-line module.
///
/// One parser rather than one per caller: `super::scoped` reads the same shape to follow a
/// `#[path]` declaration, and the spellings this tree accepts - a `#[cfg(test)]` in front, any
/// visibility, whitespace around the `;` - are exactly the thing that would drift between two
/// copies.
pub(super) fn module_name(line: &str) -> Option<&str> {
    Some(item_head(line).strip_prefix("mod ")?.trim().strip_suffix(';')?.trim())
}

/// What `line` declares, with a leading `#[cfg(test)]` and any visibility stripped: `mod tests;`
/// for `#[cfg(test)] pub(crate) mod tests;`.
///
/// One owner for the prefixes a declaration may carry, because [`module_name`] and
/// `super::attributes` both have to read past them to reach the keyword and two lists would
/// drift. `pub(in path)` is deliberately absent: no such spelling is in this tree, and missing
/// one reads the item as not a module, which is the direction that asks rather than guesses.
pub(super) fn item_head(line: &str) -> &str {
    let mut rest = line.trim();
    if let Some(after) = rest.strip_prefix("#[cfg(test)]") {
        rest = after.trim_start();
    }
    for visibility in ["pub(crate) ", "pub(super) ", "pub(self) ", "pub "] {
        if let Some(after) = rest.strip_prefix(visibility) {
            rest = after.trim_start();
            break;
        }
    }
    rest
}

/// Where a line break left the scanner. Every one of these can span lines in Rust.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Span {
    #[default]
    Code,
    /// Block comments NEST, so the depth is part of the state.
    BlockComment(usize),
    /// An ordinary string literal.
    Text,
    /// A raw string, closed by a quote followed by this many hashes.
    Raw(usize),
}

/// A brace counter that is not fooled by braces inside comments or literals.
///
/// This is the whole reason region detection is not a `matches('{')` count: test modules here
/// hold JSON and YAML fixtures, and a `"{"` in an error-message assertion would otherwise end a
/// region in the middle of one - which is a false positive for PRODUCTION code, the exact
/// direction of the defect this module exists to fix.
#[derive(Debug, Default)]
struct Braces {
    /// Saturating, so a stray `}` cannot wrap around.
    depth: usize,
    /// Has any `{` been seen since the scanner was created? Distinguishes a block item from a
    /// declaration, and does it correctly for `fn h() { 1 }` on a single line.
    saw_open: bool,
    span: Span,
}

impl Braces {
    /// Is the scanner outside every comment and literal?
    const fn in_code(&self) -> bool {
        matches!(self.span, Span::Code)
    }

    /// Consume one line, updating the depth.
    fn feed(&mut self, line: &str) {
        let mut chars = line.chars().peekable();
        while let Some(c) = chars.next() {
            match self.span {
                Span::BlockComment(depth) => self.in_block_comment(c, &mut chars, depth),
                Span::Text => self.in_text(c, &mut chars),
                Span::Raw(hashes) => self.in_raw(c, &mut chars, hashes),
                Span::Code => {
                    if self.in_code_at(c, &mut chars) {
                        return;
                    }
                }
            }
        }
    }

    fn in_block_comment(&mut self, c: char, chars: &mut Chars<'_>, depth: usize) {
        match c {
            '*' if chars.peek() == Some(&'/') => {
                skip_one(chars);
                self.span = if depth <= 1 {
                    Span::Code
                } else {
                    Span::BlockComment(depth - 1)
                };
            }
            '/' if chars.peek() == Some(&'*') => {
                skip_one(chars);
                self.span = Span::BlockComment(depth + 1);
            }
            _ => {}
        }
    }

    fn in_text(&mut self, c: char, chars: &mut Chars<'_>) {
        match c {
            // An escape hides the next character, a closing quote included.
            '\\' => skip_one(chars),
            '"' => self.span = Span::Code,
            _ => {}
        }
    }

    fn in_raw(&mut self, c: char, chars: &mut Chars<'_>, hashes: usize) {
        if c != '"' {
            return;
        }
        let mut seen = 0_usize;
        while seen < hashes && chars.peek() == Some(&'#') {
            skip_one(chars);
            seen += 1;
        }
        if seen == hashes {
            self.span = Span::Code;
        }
    }

    /// One character of ordinary code. Returns `true` when the rest of the line is a comment.
    fn in_code_at(&mut self, c: char, chars: &mut Chars<'_>) -> bool {
        match c {
            '/' if chars.peek() == Some(&'/') => return true,
            '/' if chars.peek() == Some(&'*') => {
                skip_one(chars);
                self.span = Span::BlockComment(1);
            }
            '"' => self.span = Span::Text,
            'r' | 'b' => self.maybe_literal_prefix(c, chars),
            '\'' => skip_char_literal(chars),
            '{' => {
                self.depth = self.depth.saturating_add(1);
                self.saw_open = true;
            }
            '}' => self.depth = self.depth.saturating_sub(1),
            _ => {}
        }
        false
    }

    /// `r"..."`, `r#"..."#`, `b"..."`, `br#"..."#` - and `r#ident`, which is none of them.
    fn maybe_literal_prefix(&mut self, prefix: char, chars: &mut Chars<'_>) {
        if prefix == 'b' {
            match chars.peek() {
                Some(&'r') => {
                    skip_one(chars);
                    self.maybe_raw(chars);
                }
                Some(&'"') => {
                    skip_one(chars);
                    self.span = Span::Text;
                }
                _ => {}
            }
            return;
        }
        self.maybe_raw(chars);
    }

    fn maybe_raw(&mut self, chars: &mut Chars<'_>) {
        match chars.peek() {
            Some(&'"') => {
                skip_one(chars);
                self.span = Span::Raw(0);
            }
            Some(&'#') => {
                let mut hashes = 0_usize;
                while chars.peek() == Some(&'#') {
                    skip_one(chars);
                    hashes += 1;
                }
                if chars.peek() == Some(&'"') {
                    skip_one(chars);
                    self.span = Span::Raw(hashes);
                }
                // Otherwise it was a raw IDENTIFIER, `r#type`, and nothing consumed held a brace.
            }
            _ => {}
        }
    }
}

/// The peekable char stream the scanner walks. Named so the helper signatures stay readable.
type Chars<'line> = std::iter::Peekable<std::str::Chars<'line>>;

/// Consume one character and throw it away.
///
/// A one-line function because neither idiom is available: `Option<char>` is `Copy`, so
/// `drop(chars.next())` is a no-op the compiler warns about, and `Option` is `#[must_use]`, so a
/// bare `chars.next();` trips `unused_must_use` and `let _ = ..` trips `let_underscore_must_use`.
fn skip_one(chars: &mut Chars<'_>) {
    let _consumed = chars.next();
}

/// Consume a char literal, `'{'` and `'\u{7b}'` included, so its braces are not counted.
///
/// A LIFETIME opens the same way and this deliberately cannot tell them apart. It does not have
/// to: a lifetime name holds no brace, so consuming one character of one changes no count.
fn skip_char_literal(chars: &mut Chars<'_>) {
    match chars.next() {
        Some('\\') => {
            if chars.peek() == Some(&'u') {
                skip_one(chars);
                // `\u{7b}` - the braces belong to the escape, not to a block.
                for c in chars.by_ref() {
                    if c == '}' {
                        break;
                    }
                }
            } else {
                skip_one(chars);
            }
            if chars.peek() == Some(&'\'') {
                skip_one(chars);
            }
        }
        Some(_) if chars.peek() == Some(&'\'') => skip_one(chars),
        Some(_) | None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::{Braces, Range, TestScope, cfg_test_regions, has_non_test_additions, scope};
    use crate::causality::fixtures::{added_from as from, tree};

    /// The expected regions, as `(first, past_last)` pairs. A helper rather than `vec![a..b]`
    /// because a one-element vec of a `Range` is a clippy finding on its own.
    fn spans(pairs: &[(usize, usize)]) -> Vec<Range<usize>> {
        pairs.iter().map(|&(first, past_last)| first..past_last).collect()
    }

    #[test]
    fn a_region_is_found_and_ends_where_its_braces_balance() {
        let text = "fn f() {}\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn t() {}\n}\nfn g() {}\n";
        // Lines 2..=6 are the attribute through the closing brace; `fn g()` on 7 is not.
        assert_eq!(cfg_test_regions(text), spans(&[(2, 7)]));
    }

    #[test]
    fn two_test_modules_are_two_regions_and_neither_swallows_the_file() {
        // A test module that is NOT the last thing in the file, plus a second one after it.
        let text = concat!(
            "fn a() {}\n",            // 1
            "#[cfg(test)]\n",         // 2
            "mod first {\n",          // 3
            "    fn t() {}\n",        // 4
            "}\n",                    // 5
            "fn between() -> u8 {\n", // 6
            "    1\n",                // 7
            "}\n",                    // 8
            "#[cfg(test)]\n",         // 9
            "mod second {\n",         // 10
            "    fn u() {}\n",        // 11
            "}\n",                    // 12
            "fn after() {}\n",        // 13
        );
        assert_eq!(cfg_test_regions(text), spans(&[(2, 6), (9, 13)]));
    }

    #[test]
    fn a_cfg_test_item_that_is_not_a_module_counts_as_test_code() {
        // DELIBERATE: a `#[cfg(test)] fn helper` at file scope is compiled only under `cfg(test)`,
        // so no production caller can reach it and reverting the file is the only way to remove
        // it. Counting it as test code is what keeps a helper added beside a test from reading as
        // an implementation change. A one-line declaration - `#[cfg(test)] use x;` - counts too,
        // and ends at its semicolon rather than running to the next brace.
        let text = concat!(
            "#[cfg(test)]\n",                    // 1
            "use std::fmt::Debug;\n",            // 2
            "\n",                                // 3
            "#[cfg(test)]\n",                    // 4
            "fn helper() -> u8 { 7 }\n",         // 5
            "pub fn production() -> u8 { 1 }\n", // 6
        );
        assert_eq!(cfg_test_regions(text), spans(&[(1, 3), (4, 6)]));
    }

    #[test]
    fn a_nested_module_inside_a_test_module_is_not_a_second_region() {
        let text = concat!(
            "#[cfg(test)]\n",      // 1
            "mod tests {\n",       // 2
            "    #[cfg(test)]\n",  // 3
            "    mod inner {\n",   // 4
            "        fn t() {}\n", // 5
            "    }\n",             // 6
            "}\n",                 // 7
            "fn after() {}\n",     // 8
        );
        // One region covering the outer module, not two overlapping ones.
        assert_eq!(cfg_test_regions(text), spans(&[(1, 8)]));
    }

    #[test]
    fn a_brace_in_a_literal_or_a_comment_does_not_end_a_region() {
        // The reason the scanner exists rather than a `matches('{')` count. Each of these lines
        // would otherwise unbalance the count and end the region early - which reports the rest
        // of the test module as production code.
        let text = concat!(
            "#[cfg(test)]\n",                       // 1
            "mod tests {\n",                        // 2
            "    fn t() {\n",                       // 3
            "        let a = \"{\";\n",             // 4
            "        let b = r#\"} { }\"#;\n",      // 5
            "        let c = '{';\n",               // 6
            "        let d = '\\u{7b}';\n",         // 7
            "        // a lone } in a comment\n",   // 8
            "        /* and { in a block one */\n", // 9
            "    }\n",                              // 10
            "}\n",                                  // 11
            "fn after() {}\n",                      // 12
        );
        assert_eq!(cfg_test_regions(text), spans(&[(1, 12)]));
    }

    #[test]
    fn a_multi_line_raw_string_holding_a_brace_does_not_end_a_region() {
        let text = concat!(
            "#[cfg(test)]\n",                // 1
            "mod tests {\n",                 // 2
            "    const YAML: &str = r#\"\n", // 3
            "metrics: {\n",                  // 4
            "\"#;\n",                        // 5
            "}\n",                           // 6
            "fn after() {}\n",               // 7
        );
        assert_eq!(cfg_test_regions(text), spans(&[(1, 7)]));
    }

    #[test]
    fn a_lifetime_is_not_read_as_a_char_literal() {
        // `'a` opens like a char literal and never closes. Mishandling it would swallow the rest
        // of the line, braces included.
        let text = concat!(
            "#[cfg(test)]\n",                              // 1
            "mod tests {\n",                               // 2
            "    fn t<'a>(x: &'a str) -> &'a str { x }\n", // 3
            "}\n",                                         // 4
            "fn after() {}\n",                             // 5
        );
        assert_eq!(cfg_test_regions(text), spans(&[(1, 5)]));
    }

    #[test]
    fn a_test_appended_to_an_existing_test_module_is_not_a_production_change() {
        // THE DEFECT. PR shape: seven tests appended inside `#[cfg(test)] mod tests` blocks that
        // already existed, no production line touched. The marker is unchanged diff CONTEXT, so
        // it never appears among the added lines - and the old classifier, which read only those,
        // took the first added `use` for an implementation change.
        let post_image = concat!(
            "pub fn open_engine() -> u8 {\n",             // 1
            "    1\n",                                    // 2
            "}\n",                                        // 3
            "\n",                                         // 4
            "#[cfg(test)]\n",                             // 5
            "mod tests {\n",                              // 6
            "    use super::open_engine;\n",              // 7
            "\n",                                         // 8
            "    #[test]\n",                              // 9
            "    fn existing() {\n",                      // 10
            "        assert_eq!(open_engine(), 1);\n",    // 11
            "    }\n",                                    // 12
            "\n",                                         // 13
            "    use sutura_domain::model::TableName;\n", // 14
            "\n",                                         // 15
            "    #[test]\n",                              // 16
            "    fn added() {\n",                         // 17
            "        let _ = TableName::parse(\"t\");\n", // 18
            "    }\n",                                    // 19
            "}\n",                                        // 20
        );
        let read = tree(&[("crates/x/src/commands.rs", post_image)]);
        let scoped = scope("crates/x/src/commands.rs", &read);
        assert_eq!(scoped, TestScope::Regions(spans(&[(5, 21)])));

        let added = from(
            14,
            &[
                "    use sutura_domain::model::TableName;",
                "",
                "    #[test]",
                "    fn added() {",
                "        let _ = TableName::parse(\"t\");",
                "    }",
            ],
        );
        assert!(
            !has_non_test_additions(&added, &scoped),
            "every added line sits inside the existing test module"
        );
    }

    #[test]
    fn a_production_line_and_a_test_line_in_one_file_is_still_caught() {
        // The case the gate EXISTS for. Position-awareness must not lose it.
        let post_image = concat!(
            "pub fn fixed() -> u8 {\n", // 1
            "    2\n",                  // 2
            "}\n",                      // 3
            "#[cfg(test)]\n",           // 4
            "mod tests {\n",            // 5
            "    #[test]\n",            // 6
            "    fn t() {}\n",          // 7
            "}\n",                      // 8
        );
        let read = tree(&[("crates/x/src/a.rs", post_image)]);
        let scoped = scope("crates/x/src/a.rs", &read);
        let mut added = from(2, &["    2"]);
        added.extend(from(6, &["    #[test]", "    fn t() {}"]));
        assert!(has_non_test_additions(&added, &scoped));
    }

    #[test]
    fn a_production_line_below_a_test_module_is_caught() {
        // The ordering the old marker-scan could not see at all: once it had seen a marker among
        // the added lines it stopped looking, so a production line added AFTER a test module
        // read as test code.
        let post_image = concat!(
            "#[cfg(test)]\n",                     // 1
            "mod tests {\n",                      // 2
            "    #[test]\n",                      // 3
            "    fn t() {}\n",                    // 4
            "}\n",                                // 5
            "pub fn added_later() -> u8 { 3 }\n", // 6
        );
        let read = tree(&[("crates/x/src/a.rs", post_image)]);
        let scoped = scope("crates/x/src/a.rs", &read);
        let mut added = from(3, &["    #[test]", "    fn t() {}"]);
        added.extend(from(6, &["pub fn added_later() -> u8 { 3 }"]));
        assert!(has_non_test_additions(&added, &scoped));
    }

    #[test]
    fn an_out_of_line_module_the_parent_declares_under_cfg_test_is_all_test_code() {
        // `crates/x/src/lib.rs` says `#[cfg(test)] mod testing;` and `testing.rs` says nothing
        // about itself. Region detection cannot see the marker - it is in another file.
        let read = tree(&[
            ("crates/x/src/lib.rs", "pub fn f() {}\n\n#[cfg(test)]\nmod testing;\n"),
            ("crates/x/src/testing.rs", "pub fn fake() -> u8 { 1 }\n"),
        ]);
        assert_eq!(scope("crates/x/src/testing.rs", &read), TestScope::WholeFile);

        // Nested one level down, declared by the parent module's own file.
        let nested = tree(&[
            (
                "crates/x/src/knowledge.rs",
                "pub fn f() {}\n#[cfg(test)]\nmod refusal_tests;\n",
            ),
            ("crates/x/src/knowledge/refusal_tests.rs", "fn t() {}\n"),
        ]);
        assert_eq!(
            scope("crates/x/src/knowledge/refusal_tests.rs", &nested),
            TestScope::WholeFile
        );
    }

    #[test]
    fn a_module_the_parent_declares_unconditionally_is_production_code() {
        // The name is not the evidence; the declaration is. A file called `tests.rs` that the
        // parent compiles unconditionally is production code and gets no exemption.
        let read = tree(&[
            ("crates/x/src/lib.rs", "pub fn f() {}\nmod tests;\n"),
            ("crates/x/src/tests.rs", "pub fn thing() -> u8 { 1 }\n"),
        ]);
        assert_eq!(scope("crates/x/src/tests.rs", &read), TestScope::Regions(Vec::new()));
    }

    #[test]
    fn an_attribute_belonging_to_an_earlier_item_is_not_borrowed() {
        // A blank line separates items, and the walk back stops at it - otherwise the
        // `#[cfg(test)]` above `mod other;` would exempt `mod tests;` two lines below.
        let read = tree(&[
            ("crates/x/src/lib.rs", "#[cfg(test)]\nmod other;\n\nmod tests;\n"),
            ("crates/x/src/tests.rs", "pub fn thing() -> u8 { 1 }\n"),
        ]);
        assert_eq!(scope("crates/x/src/tests.rs", &read), TestScope::Regions(Vec::new()));
    }

    #[test]
    fn a_dedicated_test_target_and_an_inner_attribute_are_whole_file() {
        let read = tree(&[
            ("crates/x/tests/t.rs", "#[test]\nfn t() {}\n"),
            ("crates/x/src/whole.rs", "#![cfg(test)]\nfn t() {}\n"),
        ]);
        assert_eq!(scope("crates/x/tests/t.rs", &read), TestScope::WholeFile);
        assert_eq!(scope("crates/x/src/whole.rs", &read), TestScope::WholeFile);
    }

    #[test]
    fn a_post_image_that_cannot_be_read_gets_no_regions() {
        // Ask rather than guess: with no regions every non-trivial added line reads as
        // production code, which is the direction that puts a human in the loop.
        let read = tree(&[]);
        assert_eq!(scope("crates/x/src/gone.rs", &read), TestScope::Regions(Vec::new()));
    }

    #[test]
    fn blank_lines_and_comments_outside_a_region_carry_no_behaviour() {
        let scoped = TestScope::Regions(Vec::new());
        let added = from(1, &["", "// a note", "#[derive(Debug)]"]);
        assert!(!has_non_test_additions(&added, &scoped));
        assert!(has_non_test_additions(&from(1, &["fn f() {}"]), &scoped));
    }

    #[test]
    fn the_scanner_tracks_a_block_comment_across_lines() {
        let mut braces = Braces::default();
        braces.feed("/* opening {");
        assert!(!braces.in_code());
        assert_eq!(braces.depth, 0);
        braces.feed("still } inside */ {");
        assert!(braces.in_code());
        assert_eq!(braces.depth, 1);
    }
}
