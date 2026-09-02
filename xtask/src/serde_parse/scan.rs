//! The SYNTAX half of the serde gate: what a file declares, read off its text.
//!
//! Split from the rules next door because the two halves fail differently. A rule is wrong when
//! it forbids correct code; a scan is wrong when it misreads a declaration - and the second kind
//! is what the tests here are about, one shape at a time.
//!
//! **It does not parse Rust, and the direction of every shortcut is the same one.** Comments and
//! the interiors of multi-line strings are blanked by [`code_lines`] before anything is read, so a
//! rustdoc example and the gate's own fixtures are invisible; a declaration wrapped in a shape the
//! walk does not expect reads as [`Shape::Other`] and is skipped rather than misreported. Under-
//! claiming is the safe failure for a gate: it lets a violation past, where over-claiming fails
//! correct code and gets the gate disabled.

use std::collections::{BTreeMap, BTreeSet};

/// The field shape of one struct, as much of it as a line scan can honestly see.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Shape {
    /// `struct X(T);` - exactly one unnamed field, whose type is this text.
    Newtype(String),
    /// A named-field body, with these field names in declaration order.
    Named(Vec<String>),
    /// A unit struct, a tuple of two or more, or a declaration this scan could not read.
    Other,
}

/// One struct declaration, with the attribute run that sits above it.
#[derive(Debug)]
pub(super) struct Declared {
    /// The type's name.
    pub(super) name: String,
    /// 1-based line of the `struct` keyword.
    pub(super) line: usize,
    /// Every attribute line above the declaration, joined with spaces, from the RAW text.
    pub(super) attrs: String,
    /// What its fields look like.
    pub(super) shape: Shape,
}

/// Every struct declared in `code`, with the attribute run above each read from `raw`.
///
/// Two views of the same file on purpose. `code` decides WHERE a declaration is, so a struct
/// inside a doc comment or a test fixture is not one; `raw` supplies the attribute TEXT, because
/// blanking a string literal would take `try_from = "String"` with it.
pub(super) fn declarations(code: &[String], raw: &[&str]) -> Vec<Declared> {
    let mut out = Vec::new();
    let mut attrs = String::new();
    let mut unclosed = 0_usize;
    for (index, line) in code.iter().enumerate() {
        let trimmed = line.trim();
        if unclosed > 0 || trimmed.starts_with("#[") || trimmed.starts_with("#![") {
            if let Some(text) = raw.get(index) {
                attrs.push(' ');
                attrs.push_str(text.trim());
            }
            unclosed = unclosed.saturating_add(opened(trimmed)).saturating_sub(closed(trimmed));
            continue;
        }
        // A blank line or a comment between the attributes and the declaration keeps the run:
        // `#[derive(..)]` above a doc comment above the struct is one declaration, not two.
        if trimmed.is_empty() || trimmed.starts_with("//") {
            continue;
        }
        if let Some(name) = struct_name(trimmed) {
            out.push(Declared {
                name: String::from(name),
                line: index.saturating_add(1),
                attrs: attrs.clone(),
                shape: shape_of(code, index),
            });
        }
        attrs.clear();
    }
    out
}

/// Brackets this line opens. Both kinds, because an attribute is `#[..(..)]` and a multi-line
/// one can break inside either.
fn opened(line: &str) -> usize {
    line.chars().filter(|c| *c == '(' || *c == '[').count()
}

/// Brackets this line closes.
fn closed(line: &str) -> usize {
    line.chars().filter(|c| *c == ')' || *c == ']').count()
}

/// The name in `struct Name ..`, with any visibility stripped first.
fn struct_name(trimmed: &str) -> Option<&str> {
    let rest = without_visibility(trimmed).strip_prefix("struct ")?;
    Some(identifier_at(rest)).filter(|name| !name.is_empty())
}

/// The leading identifier of `text`, which is empty when it does not start with one.
fn identifier_at(text: &str) -> &str {
    let end = text
        .find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .unwrap_or(text.len());
    text.get(..end).unwrap_or_default()
}

/// `text` with a leading `pub`, `pub(crate)`, `pub(super)` or `pub(in ..)` removed.
pub(super) fn without_visibility(text: &str) -> &str {
    let Some(rest) = text.strip_prefix("pub") else {
        return text;
    };
    if let Some(scoped) = rest.strip_prefix('(') {
        return scoped
            .find(')')
            .and_then(|at| scoped.get(at.saturating_add(1)..))
            .unwrap_or(rest)
            .trim_start();
    }
    rest.trim_start()
}

/// The field shape of the struct declared at `at`.
pub(super) fn shape_of(code: &[String], at: usize) -> Shape {
    let header = header_at(code, at);
    if let Some(fields) = tuple_body(&header) {
        let mut parts = split_top_level(&fields);
        return match (parts.next(), parts.next()) {
            (Some(only), None) if !only.trim().is_empty() => Shape::Newtype(String::from(without_visibility(only.trim()).trim())),
            _ => Shape::Other,
        };
    }
    if header.contains('{') {
        return Shape::Named(field_names(code, at));
    }
    Shape::Other
}

/// The declaration's own text, from `at` up to the `{` or `;` that ends it.
///
/// Joined across lines because a generic list or a `where` clause pushes the body down, and the
/// tuple-versus-named decision has to be made on the whole declaration.
fn header_at(code: &[String], at: usize) -> String {
    let mut text = String::new();
    for line in code.iter().skip(at) {
        for character in line.chars() {
            text.push(character);
            if character == '{' || character == ';' {
                return text;
            }
        }
        text.push(' ');
    }
    text
}

/// The text between the parentheses of a tuple struct, or `None` when the declaration opens its
/// body with a brace instead.
fn tuple_body(header: &str) -> Option<String> {
    let open = header.find('(')?;
    if header.find('{').is_some_and(|brace| brace < open) {
        return None;
    }
    let after = header.get(open.saturating_add(1)..)?;
    let mut depth = 1_usize;
    let mut body = String::new();
    for character in after.chars() {
        match character {
            '(' => depth = depth.saturating_add(1),
            ')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(body);
                }
            }
            _ => {}
        }
        body.push(character);
    }
    None
}

/// `text` split on commas that are not inside brackets.
fn split_top_level(text: &str) -> impl Iterator<Item = String> {
    let mut parts: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut depth = 0_usize;
    for character in text.chars() {
        match character {
            '<' | '(' | '[' => depth = depth.saturating_add(1),
            '>' | ')' | ']' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                parts.push(current.clone());
                current.clear();
                continue;
            }
            _ => {}
        }
        current.push(character);
    }
    parts.push(current);
    parts.into_iter()
}

/// The named fields of the struct whose declaration starts at `at`, in declaration order.
///
/// Only depth 1 counts: anything deeper belongs to a type in a field's position.
fn field_names(code: &[String], at: usize) -> Vec<String> {
    let mut names = Vec::new();
    let mut depth = 0_usize;
    let mut inside = false;
    for line in code.iter().skip(at) {
        let trimmed = line.trim();
        if inside && depth == 1 {
            let candidate = without_visibility(trimmed);
            if let Some((name, _)) = candidate.split_once(':')
                && !name.is_empty()
                && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            {
                names.push(String::from(name));
            }
        }
        let opens = trimmed.matches('{').count();
        let closes = trimmed.matches('}').count();
        if opens > 0 {
            inside = true;
        }
        depth = depth.saturating_add(opens).saturating_sub(closes);
        if inside && depth == 0 {
            break;
        }
    }
    names
}

/// Type name to the name of a fallible constructor it declares.
///
/// `impl Name` blocks only - never `impl Trait for Name`, because `impl TryFrom<String> for X`
/// declares `try_from`, which returns `Result<Self`, and reading that as the type's own
/// constructor would make every correctly-routed newtype a violation.
pub(super) fn fallible_constructors(code: &[String]) -> BTreeMap<String, String> {
    let mut found = BTreeMap::new();
    let mut current: Option<String> = None;
    let mut depth = 0_usize;
    let mut inside = false;
    for (index, line) in code.iter().enumerate() {
        let trimmed = line.trim();
        if current.is_none() {
            let Some(name) = inherent_impl_target(trimmed) else {
                continue;
            };
            current = Some(String::from(name));
            depth = 0;
            inside = false;
        }
        if let Some(ref name) = current
            && trimmed.contains("fn ")
            && let Some(constructor) = constructor_name(&signature_at(code, index))
        {
            found.entry(name.clone()).or_insert_with(|| String::from(constructor));
        }
        // `inside` rather than a brace on THIS line, because the brace that closes the block sits
        // on a line of its own: without it the walk stayed in the first `impl` for the rest of the
        // file and attributed a later type's constructor to it. Caught by this module's own test.
        let opens = trimmed.matches('{').count();
        let closes = trimmed.matches('}').count();
        if opens > 0 {
            inside = true;
        }
        depth = depth.saturating_add(opens).saturating_sub(closes);
        if inside && depth == 0 {
            current = None;
        }
    }
    found
}

/// The type an `impl Name ..` block is for, or `None` for a trait impl or a non-impl line.
fn inherent_impl_target(trimmed: &str) -> Option<&str> {
    let rest = trimmed.strip_prefix("impl")?;
    if !rest.starts_with('<') && !rest.starts_with(' ') {
        // `implementors_of(..)` starts with the same four letters.
        return None;
    }
    // `impl<'a, T: Into<String>>` - skip the parameter list, to ITS OWN closing bracket. The LAST
    // `>` on the line is a different one: on `impl<T> Holder<T> {` it belongs to the type, and
    // taking it left no identifier to read at all. Caught by this module's own test.
    let rest = if rest.starts_with('<') {
        rest.get(matching_angle(rest)?.saturating_add(1)..)?
    } else {
        rest
    };
    let rest = rest.trim_start();
    if rest.contains(" for ") {
        return None;
    }
    Some(identifier_at(rest)).filter(|name| !name.is_empty())
}

/// The byte offset of the `>` that closes the `<` at the start of `text`.
fn matching_angle(text: &str) -> Option<usize> {
    let mut depth = 0_usize;
    for (at, character) in text.char_indices() {
        match character {
            '<' => depth = depth.saturating_add(1),
            '>' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(at);
                }
            }
            _ => {}
        }
    }
    None
}

/// The joined signature that starts on the line at `at`, up to the `{` or `;` that ends it.
fn signature_at(code: &[String], at: usize) -> String {
    let mut text = String::new();
    let mut parens = 0_usize;
    let mut opened_params = false;
    for line in code.iter().skip(at) {
        for character in line.chars() {
            match character {
                '(' => {
                    parens = parens.saturating_add(1);
                    opened_params = true;
                }
                ')' => parens = parens.saturating_sub(1),
                '{' | ';' if opened_params && parens == 0 => return text,
                _ => {}
            }
            text.push(character);
        }
        text.push(' ');
    }
    text
}

/// The function's name if this signature is a fallible constructor, else `None`.
pub(super) fn constructor_name(signature: &str) -> Option<&str> {
    let after_fn = signature.split_once("fn ")?.1;
    let (name, rest) = after_fn.split_once('(')?;
    let name = name.trim();
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return None;
    }
    let (params, tail) = rest.split_once(')')?;
    if params.contains("self") {
        return None;
    }
    // `Result<Self` rather than any `Result`: an associated function returning some OTHER type's
    // `Result` is not this type's constructor, and reading it as one would fail correct code.
    tail.contains("-> Result<Self").then_some(name)
}

/// Types with a hand-written `impl .. <trait_name> .. for Type`.
pub(super) fn hand_written(code: &[String], trait_name: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    for line in code {
        let trimmed = line.trim();
        if !trimmed.starts_with("impl") || !trimmed.contains(trait_name) {
            continue;
        }
        let Some((_, after)) = trimmed.split_once(" for ") else {
            continue;
        };
        let name = identifier_at(after.trim_start());
        if !name.is_empty() {
            found.insert(String::from(name));
        }
    }
    found
}

/// Does this attribute run carry `derive(.. <trait_name> ..)`?
///
/// The `derive` is required, so a `#[serde(with = "Deserialize")]` argument is not read as one.
/// Each item is compared by SUFFIX, because the path may be written `serde::Deserialize` - and
/// suffix rather than substring, so a `Deserialize` entry does not answer for `Serialize`.
pub(super) fn derives(attrs: &str, trait_name: &str) -> bool {
    let mut rest = attrs;
    while let Some(at) = rest.find("derive(") {
        let tail = rest.get(at.saturating_add("derive(".len())..).unwrap_or_default();
        let Some(close) = tail.find(')') else {
            return false;
        };
        if tail
            .get(..close)
            .is_some_and(|list| split_top_level(list).any(|item| item.trim().ends_with(trait_name)))
        {
            return true;
        }
        rest = tail;
    }
    false
}

/// The value of `#[serde(<key> = "..")]` in this attribute run.
pub(super) fn serde_arg(attrs: &str, key: &str) -> Option<String> {
    let mut rest = attrs;
    while let Some(at) = rest.find("serde(") {
        let tail = rest.get(at.saturating_add("serde(".len())..)?;
        let body = tail.get(..tail.find(')').unwrap_or(tail.len()))?;
        for item in split_top_level(body) {
            let Some((name, value)) = item.split_once('=') else {
                continue;
            };
            if name.trim() == key {
                return Some(String::from(value.trim().trim_matches('"')));
            }
        }
        rest = tail;
    }
    None
}

/// Where the lexer is.
#[derive(Clone, Copy)]
enum Lexeme {
    Code,
    LineComment,
    BlockComment(usize),
    /// Inside a string. `hashes` is 0 for an ordinary one and the `#` count for a raw one.
    Text {
        hashes: usize,
    },
}

/// The lines built so far, plus the string literal currently open.
///
/// The open literal is held rather than written through, because whether it is code depends on
/// something only its END reveals: a one-line string keeps its content and a multi-line one does
/// not. See [`emit_text`].
#[derive(Default)]
struct Sink {
    lines: Vec<String>,
    current: String,
    held: Option<String>,
}

impl Sink {
    /// A character that IS code.
    fn code(&mut self, character: char) {
        if character == '\n' {
            self.newline();
        } else {
            self.current.push(character);
        }
    }

    /// A character that is not code: only its newline matters, and it always does.
    fn skipped(&mut self, character: char) {
        if character == '\n' {
            self.newline();
        }
    }

    fn newline(&mut self) {
        self.lines.push(core::mem::take(&mut self.current));
    }

    fn finish(mut self) -> Vec<String> {
        self.lines.push(self.current);
        self.lines
    }
}

/// Each line of `text` with everything that is not code blanked out.
///
/// Comments go, and so does the interior of any string that SPANS LINES - which is what makes
/// the gate's own fixtures and every rustdoc example invisible to it. A single-line string keeps
/// its content, because `try_from = "String"` is one and the value is the point. Char literals
/// are consumed so that `'"'` cannot open a string, and a lifetime is left alone.
///
/// Newlines are always preserved, so a reported line number is the one a reader will open.
pub(super) fn code_lines(text: &str) -> Vec<String> {
    let mut sink = Sink::default();
    let mut state = Lexeme::Code;
    let mut characters = text.chars();
    while let Some(character) = characters.next() {
        state = step(state, character, &mut characters, &mut sink);
    }
    sink.finish()
}

/// One character, and the state after it.
///
/// A plain `Chars` rather than a `Peekable`, because every lookahead here is a `clone()` on a
/// slice iterator - cheap - and two arms want the peeked value without consuming it.
fn step(state: Lexeme, character: char, characters: &mut core::str::Chars<'_>, sink: &mut Sink) -> Lexeme {
    match state {
        Lexeme::Code => in_code(character, characters, sink),
        Lexeme::LineComment => {
            sink.skipped(character);
            if character == '\n' {
                Lexeme::Code
            } else {
                Lexeme::LineComment
            }
        }
        Lexeme::BlockComment(depth) => {
            sink.skipped(character);
            in_block_comment(character, characters, depth)
        }
        Lexeme::Text { hashes } => in_text(character, characters, sink, hashes),
    }
}

/// One character of ordinary code, which may open a comment, a string or a char literal.
fn in_code(character: char, characters: &mut core::str::Chars<'_>, sink: &mut Sink) -> Lexeme {
    match (character, characters.clone().next()) {
        ('/', Some('/')) => Lexeme::LineComment,
        ('/', Some('*')) => {
            characters.next();
            Lexeme::BlockComment(1)
        }
        ('"', _) => {
            sink.held = Some(String::new());
            Lexeme::Text { hashes: 0 }
        }
        ('\'', _) => {
            let width = char_literal_width(characters);
            if width == 0 {
                // A lifetime, which is ordinary code.
                sink.code(character);
            } else {
                for _ in 0..width {
                    characters.next();
                }
            }
            Lexeme::Code
        }
        ('r', _) => {
            let Some(hashes) = raw_string_hashes(characters) else {
                sink.code(character);
                return Lexeme::Code;
            };
            for _ in 0..=hashes {
                characters.next();
            }
            sink.held = Some(String::new());
            Lexeme::Text { hashes }
        }
        _ => {
            sink.code(character);
            Lexeme::Code
        }
    }
}

/// One character of a block comment, nesting counted.
fn in_block_comment(character: char, characters: &mut core::str::Chars<'_>, depth: usize) -> Lexeme {
    match (character, characters.clone().next()) {
        ('*', Some('/')) => {
            characters.next();
            if depth <= 1 {
                Lexeme::Code
            } else {
                Lexeme::BlockComment(depth.saturating_sub(1))
            }
        }
        ('/', Some('*')) => {
            characters.next();
            Lexeme::BlockComment(depth.saturating_add(1))
        }
        _ => Lexeme::BlockComment(depth),
    }
}

/// One character inside a string literal.
fn in_text(character: char, characters: &mut core::str::Chars<'_>, sink: &mut Sink, hashes: usize) -> Lexeme {
    if let Some(ref mut held) = sink.held {
        held.push(character);
    }
    // Escapes exist in an ordinary string only; in a raw one `\` is just a character.
    if hashes == 0 && character == '\\' {
        if let Some(escaped) = characters.next()
            && let Some(ref mut held) = sink.held
        {
            held.push(escaped);
        }
        return Lexeme::Text { hashes };
    }
    if character != '"' || !closes_text(characters, hashes) {
        return Lexeme::Text { hashes };
    }
    for _ in 0..hashes {
        characters.next();
    }
    let held = sink.held.take().unwrap_or_default();
    emit_text(&held, sink);
    Lexeme::Code
}

/// Write a finished string literal back out: kept if it was one line, blanked if it spanned
/// several. Its newlines are kept either way, so no line number moves.
fn emit_text(held: &str, sink: &mut Sink) {
    let body = held.strip_suffix('"').unwrap_or(held);
    if !body.contains('\n') {
        sink.current.push('"');
        sink.current.push_str(body);
        sink.current.push('"');
        return;
    }
    for _ in body.split('\n').skip(1) {
        sink.newline();
    }
}

/// Does a `#`-run of `hashes` follow, closing a raw string of that width?
fn closes_text(characters: &core::str::Chars<'_>, hashes: usize) -> bool {
    characters.clone().take(hashes).filter(|c| *c == '#').count() == hashes
}

/// The `#` count if a raw string opens here (the `r` has been consumed), else `None`.
fn raw_string_hashes(characters: &core::str::Chars<'_>) -> Option<usize> {
    let mut hashes = 0_usize;
    for character in characters.clone() {
        match character {
            '#' => hashes = hashes.saturating_add(1),
            '"' => return Some(hashes),
            _ => return None,
        }
    }
    None
}

/// How many characters the char literal starting here occupies, or 0 if this is a lifetime.
fn char_literal_width(characters: &core::str::Chars<'_>) -> usize {
    let mut ahead = characters.clone();
    match (ahead.next(), ahead.next()) {
        (Some('\\'), _) => {
            // `'\n'`, `'\''`, `'\u{1F600}'` - up to the closing quote, bounded so an unterminated
            // literal cannot make this walk the rest of the file.
            let mut width = 1_usize;
            for character in ahead.take(10) {
                width = width.saturating_add(1);
                if character == '\'' {
                    return width;
                }
            }
            0
        }
        (Some(_), Some('\'')) => 2,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::{Shape, code_lines, constructor_name, derives, fallible_constructors, serde_arg, shape_of};

    #[test]
    fn a_trait_impl_is_not_an_inherent_impl() {
        // `impl TryFrom<String> for Digest` declares `try_from`, which returns `Result<Self` -
        // and reading it as the type's own constructor would make every correctly-routed newtype
        // a violation, which is the shape of a gate people learn to disable.
        let code = code_lines(
            "impl TryFrom<String> for Digest {\n    fn try_from(raw: String) -> Result<Self, Bad> {\n        Self::parse(raw)\n    }\n}\n",
        );
        assert!(fallible_constructors(&code).is_empty());
    }

    #[test]
    fn a_wrapped_constructor_signature_is_still_read() {
        let code = code_lines(
            "impl Digest {\n    pub fn parse(\n        raw: &str,\n    ) -> Result<Self, Bad> {\n        todo!()\n    }\n}\n",
        );
        assert_eq!(fallible_constructors(&code).get("Digest").map(String::as_str), Some("parse"));
    }

    #[test]
    fn a_generic_impl_names_the_type_after_its_parameters() {
        let code =
            code_lines("impl<T> Holder<T> {\n    pub fn parse(raw: T) -> Result<Self, Bad> {\n        todo!()\n    }\n}\n");
        assert!(fallible_constructors(&code).contains_key("Holder"));
    }

    #[test]
    fn a_constructor_in_a_later_impl_block_is_not_attributed_to_an_earlier_type() {
        let code = code_lines(
            "impl Anchor {\n    pub const fn new(v: u8) -> Self {\n        Self { v }\n    }\n}\n\nimpl Digest {\n    pub fn parse(raw: &str) -> Result<Self, Bad> {\n        todo!()\n    }\n}\n",
        );
        let found = fallible_constructors(&code);
        assert!(!found.contains_key("Anchor"), "{found:?}");
        assert!(found.contains_key("Digest"), "{found:?}");
    }

    #[test]
    fn a_method_taking_self_is_not_a_constructor() {
        assert!(constructor_name("    pub fn narrowed(&self, to: u8) -> Result<Self, Bad> ").is_none());
    }

    #[test]
    fn a_result_of_another_type_is_not_this_ones_constructor() {
        assert!(constructor_name("    pub fn digest_of(x: &X) -> Result<Digest, Bad> ").is_none());
    }

    #[test]
    fn derive_is_required_before_a_trait_name_counts() {
        assert!(derives("#[derive(Debug, serde::Deserialize)]", "Deserialize"));
        assert!(!derives("#[serde(with = \"Deserialize\")]", "Deserialize"));
        // The suffix comparison earns its keep here: a substring test would read `Deserialize`
        // as a `Serialize` derive and make every deserializing type look symmetric.
        assert!(!derives("#[derive(serde::Deserialize)]", "Serialize"));
    }

    #[test]
    fn a_serde_argument_is_read_by_name() {
        let attrs = "#[serde(try_from = \"TermRepr\", into = \"TermRepr\")]";
        assert_eq!(serde_arg(attrs, "try_from").as_deref(), Some("TermRepr"));
        assert_eq!(serde_arg(attrs, "into").as_deref(), Some("TermRepr"));
        assert!(serde_arg(attrs, "from").is_none());
    }

    #[test]
    fn a_shape_is_read_off_the_declaration() {
        let one = code_lines("pub struct Digest(String);\n");
        assert_eq!(shape_of(&one, 0), Shape::Newtype(String::from("String")));
        let two = code_lines("pub struct Pair(A, B);\n");
        assert_eq!(shape_of(&two, 0), Shape::Other);
        let unit = code_lines("pub struct Marker;\n");
        assert_eq!(shape_of(&unit, 0), Shape::Other);
        let named = code_lines("pub struct Range {\n    start: Date,\n    end: Date,\n}\n");
        assert_eq!(
            shape_of(&named, 0),
            Shape::Named(vec![String::from("start"), String::from("end")])
        );
    }

    #[test]
    fn a_generic_field_type_does_not_split_a_newtype_into_two_fields() {
        let code = code_lines("pub struct Held(BTreeMap<String, Note>);\n");
        assert_eq!(shape_of(&code, 0), Shape::Newtype(String::from("BTreeMap<String, Note>")));
    }

    #[test]
    fn a_body_opened_on_a_later_line_is_still_read() {
        let code = code_lines("pub struct Wrapper<T>\nwhere\n    T: Clone,\n{\n    inner: T,\n}\n");
        assert_eq!(shape_of(&code, 0), Shape::Named(vec![String::from("inner")]));
    }

    #[test]
    fn a_single_line_string_keeps_its_content_because_the_attribute_needs_it() {
        let lines = code_lines("#[serde(try_from = \"String\")]\n");
        assert_eq!(lines.first().map(String::as_str), Some("#[serde(try_from = \"String\")]"));
    }

    #[test]
    fn blanking_a_string_does_not_move_a_line_number() {
        let text = "fn a() {}\nconst X: &str = r#\"\none\ntwo\n\"#;\npub struct Late(String);\n";
        let lines = code_lines(text);
        assert_eq!(
            lines.iter().position(|line| line.contains("struct Late")),
            Some(5),
            "{lines:?}"
        );
    }

    #[test]
    fn a_quote_inside_a_char_literal_opens_no_string() {
        let text = "fn a() { let q = '\"'; }\npub struct After(String);\n";
        let lines = code_lines(text);
        assert!(
            lines.iter().any(|line| line.contains("struct After")),
            "the char literal swallowed the file: {lines:?}"
        );
    }

    #[test]
    fn a_lifetime_is_not_a_char_literal() {
        let text = "impl<'a> Digest<'a> {}\npub struct After(String);\n";
        let lines = code_lines(text);
        assert!(lines.iter().any(|line| line.contains("struct After")), "{lines:?}");
    }

    #[test]
    fn a_block_comment_hides_a_declaration_and_keeps_the_lines() {
        let text = "/*\npub struct Hidden(String);\n*/\npub struct Real(String);\n";
        let lines = code_lines(text);
        assert!(!lines.iter().any(|line| line.contains("struct Hidden")), "{lines:?}");
        assert_eq!(lines.iter().position(|line| line.contains("struct Real")), Some(3));
    }
}
