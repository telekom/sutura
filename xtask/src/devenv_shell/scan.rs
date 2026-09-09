//! What a devenv module ASSIGNS, read off the Nix code projection.
//!
//! One question, asked per assignment: what does the value BEGIN with? That is enough to hold the
//! rule its caller needs, and it is enough because of what [`crate::workflows::code_lines`] throws
//! away - **a string literal's interior is blanked, so a value that projects to nothing WAS a
//! string literal.** No second parse, no quote counting, and the `"` and `''` forms are one case
//! rather than two literals to enumerate. `${...}` is the exception the lexer keeps as code, so a
//! literal that starts with an interpolation projects to a `$` and is read as a literal too.
//!
//! **Why not a needle.** `github.com/telekom/sutura#402` measured six spellings of one hazard that
//! a fixed `.exec = "` literal walked past: no leading dot, two spaces, a newline after the `=`,
//! the `''` form, a different attribute name, and the same string in another file. Every one of
//! them is the same assignment to this reader, because it keys on the `=` and on what follows it.
//!
//! **What it does not model.** Nix, mostly. There is no evaluator here: a value that reaches a
//! wrapper through a function this scan cannot follow reads as unwrapped, which is the safe
//! direction. A dynamic attribute name (`${...} = ...`) is blanked to nothing and yields no
//! assignment - so it is invisible rather than misread, and [`super`]'s floor is what makes an
//! empty read a refusal instead of a pass.

use crate::workflows::code_lines;

/// What the first non-blank code of a value says about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Value {
    /// It begins with an identifier path - an application, or a reference to a binding.
    Head(String),
    /// It begins with `{`, `[` or `(`: a set, a list or a parenthesised expression. Never a bare
    /// shell body, whatever it contains - the members are separate assignments to this scan.
    Structure,
    /// Nothing but blanks and interpolations before the terminator, which in this projection is
    /// what a STRING LITERAL looks like. `lines` is how many lines it covered, so a one-liner and
    /// a `''...''` block are told apart without reading a delimiter.
    Literal { lines: usize },
}

/// One assignment: where it is, what it names, and what its value begins with.
pub(super) struct Assignment {
    /// 1-based line of the `=`, so a failure names something a reader can open.
    pub(super) line: usize,
    /// The last segment of the assigned path - what devenv keys an option on. `scripts.fmt.exec`,
    /// `fmt.exec` and a bare `exec` inside a `scripts` block all give `exec`, which is why the
    /// leading dot the old needle required is not a spelling this can miss.
    pub(super) attribute: String,
    /// The whole path as written, for the message.
    pub(super) path: String,
    /// How many `let`s were open where it starts. Zero is a module attribute; more is a binding,
    /// and at brace depth alone the two are indistinguishable.
    pub(super) lets: u32,
    /// What the value begins with.
    pub(super) value: Value,
    /// The value's whole code projection, `=` to terminator. Used to derive which bindings are
    /// wrappers, and to read the argument set of a `writeShellApplication` call.
    pub(super) code: String,
    /// The value's lambda parameters, if it is a lambda. `name: body: runs name ...` gives
    /// `["name", "body"]` - which is what makes the identifier check below able to tell a
    /// parameter from a builder it has never heard of.
    pub(super) params: Vec<String>,
    /// The first identifier path AFTER the lambda parameters: what the value APPLIES.
    ///
    /// Separate from [`Assignment::value`], and the difference is a measured escape: the head of
    /// `name: body: runs name ''...''` is the parameter `name`, while what it applies is `runs`.
    /// Routing is about the second.
    pub(super) applied: Option<String>,
    /// Does the value's span carry a MULTI-LINE `''...''` literal?
    ///
    /// The shape of a shell body, wherever it sits. `pkgs.writeShellScriptBin "x" ''...''`
    /// projects to an application rather than to a literal, so [`Value`] alone cannot see the
    /// body in it - measured as escape 3b on `github.com/telekom/sutura#409`.
    pub(super) indented: bool,
}

/// How many lines a value may span before this stops looking for its terminator.
///
/// A bound rather than the file's length, because a projection that has desynchronised would
/// otherwise walk the rest of the module and report one enormous value. The longest real body in
/// this tree is about sixty lines; anything past this is a broken read, and the caller sees it as
/// an unwrapped literal - the safe direction.
const SPAN_LIMIT: usize = 400;

/// Characters that end a value at depth zero.
const TERMINATORS: &[char] = &[';', ',', '}', ']', ')'];

/// Nix keywords that can sit immediately before an `=` without being an attribute path.
const KEYWORDS: &[&str] = &["let", "in", "if", "then", "else", "with", "inherit", "rec", "assert", "or"];

/// Is this character part of an identifier or an attribute path?
fn word(c: char) -> bool {
    ident(c) || c == '.'
}

/// Is this character part of one identifier?
///
/// Separate from [`word`] because `.` is inside an attribute PATH and is a boundary between
/// identifiers - so `pkgs.writeShellApplication` names `writeShellApplication`, which is how
/// [`mentions`] finds the builder. Reading the dot as an identifier character is a defect this
/// gate had for one run: the wrapper set came back EMPTY over the real tree, and empty is a
/// refusal, so the direction it failed in was the safe one.
fn ident(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '-' | '\'')
}

/// Is the `=` at `column` a binding, rather than half of `==`, `!=`, `<=` or `>=`?
fn binds(chars: &[char], column: usize) -> bool {
    if chars.get(column) != Some(&'=') {
        return false;
    }
    if chars.get(column.saturating_add(1)) == Some(&'=') {
        return false;
    }
    let previous = column.checked_sub(1).and_then(|i| chars.get(i)).copied();
    !matches!(previous, Some('!' | '<' | '>' | '='))
}

/// The attribute path immediately left of the `=` at `column`, if there is one.
///
/// Whitespace between the path and the `=` is allowed and whitespace INSIDE it is not, which is
/// what keeps `x."y".z` from reading as one path: the quotes are blanked, so the run stops there
/// and the last segment - the one devenv keys on - is what comes back.
fn path_before(chars: &[char], column: usize) -> Option<String> {
    let mut index = column;
    while let Some(previous) = index.checked_sub(1) {
        match chars.get(previous) {
            Some(c) if c.is_whitespace() => index = previous,
            _ => break,
        }
    }
    let end = index;
    while let Some(previous) = index.checked_sub(1) {
        match chars.get(previous) {
            Some(&c) if word(c) => index = previous,
            _ => break,
        }
    }
    let path: String = chars.get(index..end)?.iter().collect();
    let trimmed = path.trim_matches('.');
    if trimmed.is_empty() || KEYWORDS.contains(&trimmed) {
        return None;
    }
    Some(String::from(trimmed))
}

/// Where a value's first non-blank code is, and what it is.
struct Head {
    /// What the value begins with.
    value: Value,
    /// The value's code, `=` to terminator.
    code: String,
    /// Did any line of the span start inside a `''...''` literal?
    indented: bool,
}

/// Walk the projection from just after an `=` to the value's terminator.
///
/// Depth-tracked over `{}[]()`, because the `;` that ends `linted`'s value sits after an attrset
/// whose own members are `;`-separated - stopping at the first one would read the wrapper's
/// argument set as the whole value.
fn head_of(lines: &[crate::workflows::CodeLine], start: usize, column: usize) -> Head {
    let mut depth = 0_i32;
    let mut code = String::new();
    let mut value: Option<Value> = None;
    let mut spanned = 1_usize;
    let mut indented = false;

    for offset in 0..SPAN_LIMIT {
        let index = start.saturating_add(offset);
        let Some(line) = lines.get(index) else { break };
        indented = indented || line.in_indented;
        let chars: Vec<char> = line.code.chars().collect();
        let from = if offset == 0 { column } else { 0 };
        spanned = offset.saturating_add(1);
        let mut position = from;
        let mut ended = false;
        while let Some(&current) = chars.get(position) {
            if depth == 0 && TERMINATORS.contains(&current) {
                ended = true;
                break;
            }
            match current {
                '{' | '[' | '(' => depth = depth.saturating_add(1),
                '}' | ']' | ')' => depth = depth.saturating_sub(1),
                _ => {}
            }
            if value.is_none() && !current.is_whitespace() {
                value = Some(match current {
                    '{' | '[' | '(' => Value::Structure,
                    // An interpolation cannot open a value in Nix, so a `$` here is one inside a
                    // string whose surrounding characters the lexer blanked.
                    '$' => Value::Literal { lines: 0 },
                    c if c.is_alphanumeric() || c == '_' => {
                        let mut name = String::new();
                        let mut walk = position;
                        while let Some(&c) = chars.get(walk).filter(|c| word(**c)) {
                            name.push(c);
                            walk = walk.saturating_add(1);
                        }
                        Value::Head(name)
                    }
                    // A `:` here is a lambda with no parameter name, `/` a path, `!` a negation -
                    // none of them a literal and none of them a wrapper.
                    _ => Value::Structure,
                });
            }
            code.push(current);
            position = position.saturating_add(1);
        }
        code.push(' ');
        if ended {
            break;
        }
    }

    Head {
        value: match value {
            // A literal's line count is only known once the terminator is found, so it is filled
            // in here rather than at the character that opened it.
            Some(Value::Literal { .. }) | None => Value::Literal { lines: spanned },
            Some(other) => other,
        },
        code,
        indented,
    }
}

/// The lambda parameters a value opens with, and where they end.
///
/// `name: body: runs name ...` gives `(["name", "body"], <byte offset of `runs`>)`. A `:` that
/// follows anything but a bare identifier ends the walk, so an attrset argument pattern
/// (`{ a, b }: ...`) contributes no parameters rather than a wrong one.
fn lambda(code: &str) -> (Vec<String>, usize) {
    let mut params = Vec::new();
    let mut rest = code;
    let mut consumed = 0_usize;
    loop {
        let trimmed = rest.trim_start();
        let skipped = rest.len().saturating_sub(trimmed.len());
        let taken: String = trimmed.chars().take_while(|c| ident(*c)).collect();
        let after = trimmed.get(taken.len()..).unwrap_or_default();
        if taken.is_empty() || !after.starts_with(':') {
            return (params, consumed.saturating_add(skipped));
        }
        consumed = consumed.saturating_add(skipped).saturating_add(taken.len()).saturating_add(1);
        params.push(taken);
        rest = code.get(consumed..).unwrap_or_default();
    }
}

/// Every identifier path in `code` that is a USE rather than a declaration.
///
/// An attribute key (`x =`) and an `inherit` name are excluded: those are the argument set, which
/// its own rule reads. A Nix PATH is excluded too - a token containing `/` - so
/// `${./nix/toolchains.nix}` does not read as three unknown identifiers.
pub(super) fn identifiers(code: &str) -> Vec<String> {
    let mut found = Vec::new();
    let chars: Vec<char> = code.chars().collect();
    let mut index = 0_usize;
    let mut inheriting = false;
    while index < chars.len() {
        let Some(&current) = chars.get(index) else { break };
        if !word(current) && current != '/' {
            if current == ';' {
                inheriting = false;
            }
            index = index.saturating_add(1);
            continue;
        }
        let mut token = String::new();
        let mut walk = index;
        while let Some(&c) = chars.get(walk).filter(|c| word(**c) || **c == '/') {
            token.push(c);
            walk = walk.saturating_add(1);
        }
        index = walk;
        // The next non-space character decides whether this was a key.
        let mut probe = walk;
        while chars.get(probe).is_some_and(|c| c.is_whitespace()) {
            probe = probe.saturating_add(1);
        }
        let is_key = chars.get(probe) == Some(&'=') && chars.get(probe.saturating_add(1)) != Some(&'=');
        if token == "inherit" {
            inheriting = true;
            continue;
        }
        if is_key || inheriting || token.contains('/') {
            continue;
        }
        let trimmed = token.trim_matches('.');
        if !trimmed.is_empty() {
            found.push(String::from(trimmed));
        }
    }
    found
}

/// Every assignment in one Nix module, as this projection can see one.
pub(super) fn assignments(text: &str) -> Vec<Assignment> {
    let lines = code_lines(text);
    let mut found = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let chars: Vec<char> = line.code.chars().collect();
        for column in 0..chars.len() {
            if !binds(&chars, column) {
                continue;
            }
            let Some(path) = path_before(&chars, column) else { continue };
            let head = head_of(&lines, index, column.saturating_add(1));
            let attribute = path.rsplit('.').next().unwrap_or(path.as_str());
            let (params, after) = lambda(&head.code);
            // ONLY when the value is an application. A `${...}` inside a blanked string is code to
            // the lexer, so the first identifier in `''source ${linted ...}''` is `linted` - and
            // reading that as what the value applies made a bare literal wrapped in a `source`
            // line report as routed. `Value` already says whether the value STARTS with an
            // identifier; that is the discriminator.
            let applied = match &head.value {
                Value::Head(_) => identifiers(head.code.get(after..).unwrap_or_default())
                    .into_iter()
                    .find(|token| !params.contains(token)),
                Value::Structure | Value::Literal { .. } => None,
            };
            found.push(Assignment {
                line: index.saturating_add(1),
                attribute: String::from(attribute),
                path,
                lets: line.lets,
                value: head.value,
                code: head.code,
                params,
                applied,
                indented: head.indented,
            });
        }
    }
    found
}

/// Does `code` name `word` as a whole identifier?
///
/// Word-bounded, because `unlinted` contains `linted` and a wrapper set built by substring would
/// admit a binding that only looks like one.
pub(super) fn mentions(code: &str, name: &str) -> bool {
    let mut rest = code;
    while let Some(at) = rest.find(name) {
        let before = rest.get(..at).and_then(|s| s.chars().next_back());
        let after = rest.get(at.saturating_add(name.len())..).and_then(|s| s.chars().next());
        if !before.is_some_and(ident) && !after.is_some_and(ident) {
            return true;
        }
        rest = rest.get(at.saturating_add(1)..).unwrap_or_default();
    }
    false
}

#[cfg(test)]
mod tests {
    use super::{Value, assignments, mentions};

    #[test]
    fn a_dotted_path_names_its_last_identifier() {
        // The bug this gate shipped with for one run: `.` read as an identifier character, so
        // `pkgs.writeShellApplication` did not "mention" the builder and the wrapper set came
        // back empty over the real tree.
        assert!(mentions("a = pkgs.writeShellApplication { };", "writeShellApplication"));
        assert!(mentions("a = writeShellApplication { };", "writeShellApplication"));
        assert!(!mentions("a = pkgs.writeShellApplicationX { };", "writeShellApplication"));
    }

    fn one(text: &str, attribute: &str) -> Value {
        assignments(text)
            .into_iter()
            .find(|a| a.attribute == attribute)
            .unwrap_or_else(|| panic!("no assignment to `{attribute}` in:\n{text}"))
            .value
    }

    #[test]
    fn the_six_measured_spellings_of_one_body_are_one_assignment() {
        // Every row of `github.com/telekom/sutura#402`'s escape table, as this scan reads it. The
        // point is that the ATTRIBUTE is the same in all of them, which is what the fixed
        // `.exec = "` needle could not say.
        for (label, text) in [
            ("no leading dot", "{ scripts = { a = { exec = \"bad\"; }; }; }"),
            ("two spaces", "{ scripts.a.exec =  \"bad\"; }"),
            ("newline after =", "{ scripts.a.exec =\n    \"bad\";\n}"),
            ("indented string", "{ scripts.a.exec = ''bad'';\n}"),
        ] {
            assert!(
                matches!(one(text, "exec"), Value::Literal { .. }),
                "{label}: a bare literal must read as one"
            );
        }
        assert!(matches!(one("{ enterTest = ''bad''; }", "enterTest"), Value::Literal { .. }));
    }

    #[test]
    fn a_wrapped_body_reads_as_an_application_whatever_the_quoting() {
        assert_eq!(
            one("{ a.exec = runs \"a\" \"good\"; }", "exec"),
            Value::Head(String::from("runs"))
        );
        assert_eq!(
            one("{ a.exec = sourced \"a\" ''\n  good\n'';\n}", "exec"),
            Value::Head(String::from("sourced"))
        );
        // The newline spelling that defeats a fixed needle does not defeat this: the head is
        // found on the next line and is still the wrapper.
        assert_eq!(
            one("{ a.exec =\n  runs \"a\" \"good\";\n}", "exec"),
            Value::Head(String::from("runs"))
        );
    }

    #[test]
    fn a_block_literal_is_told_from_a_one_liner_by_its_line_count() {
        // The discriminator for the second rule: a shell body of any size is a block, and a block
        // is a literal spanning more than one line whatever attribute it is assigned to.
        assert_eq!(one("{ enterTest = \"one\"; }", "enterTest"), Value::Literal { lines: 1 });
        let block = one("{ novel = ''\n  for f in $(ls *.rs); do echo $f; done\n'';\n}", "novel");
        assert!(matches!(block, Value::Literal { lines } if lines > 1), "{block:?}");
    }

    #[test]
    fn an_attrset_and_a_list_are_not_literals() {
        assert_eq!(one("{ scripts = { }; }", "scripts"), Value::Structure);
        assert_eq!(one("{ packages = [ pkgs.git ]; }", "packages"), Value::Structure);
        // A string whose first code is an interpolation is still a string - this is the shape
        // `enterShell` had before #402, and reading it as an application would have passed it.
        assert!(matches!(
            one("{ enterShell = ''source ${linted \"x\" [ ] \"y\"}/bin/x''; }", "enterShell"),
            Value::Literal { .. }
        ));
    }

    #[test]
    fn a_comment_is_not_an_assignment_and_a_comparison_is_not_a_binding() {
        let text = "{\n  # exec = \"commented out\";\n  real = 1;\n}";
        let names: Vec<String> = assignments(text).into_iter().map(|a| a.attribute).collect();
        assert_eq!(
            names,
            vec![String::from("real")],
            "a `#` line is blanked, so it assigns nothing"
        );
        assert!(
            assignments("{ ok = if a == b then 1 else 2; }")
                .iter()
                .all(|a| a.attribute == "ok"),
            "`==` is not a binding"
        );
    }

    #[test]
    fn a_let_binding_is_told_from_a_module_attribute() {
        let text = "let\n  helper = ''body'';\nin\n{\n  enterTest = helper;\n}";
        let found = assignments(text);
        let helper = found.iter().find(|a| a.attribute == "helper").expect("the binding");
        let attribute = found.iter().find(|a| a.attribute == "enterTest").expect("the attribute");
        assert!(helper.lets > 0, "a `let` binding is inside a scope");
        assert_eq!(attribute.lets, 0, "a module attribute is not");
    }

    #[test]
    fn the_value_span_reaches_past_a_nested_semicolon() {
        // `linted`'s own shape: the `;` that ends it is after an attrset whose members are
        // `;`-separated, so a first-semicolon scan would read only half the value and miss the
        // argument set the emission rule is about.
        let text = "let\n  linted = n: b: t:\n    pkgs.writeShellApplication { name = \"s-${n}\"; inherit b t; };\nin\n{ }";
        let found = assignments(text);
        let linted = found.iter().find(|a| a.attribute == "linted").expect("the binding");
        assert!(
            linted.code.contains("writeShellApplication") && linted.code.contains("inherit"),
            "the span stopped early: {}",
            linted.code
        );
    }

    #[test]
    fn a_word_match_is_not_a_substring_match() {
        assert!(mentions("a = linted x;", "linted"));
        assert!(!mentions("a = unlinted x;", "linted"));
        assert!(!mentions("a = lintedly x;", "linted"));
    }
}
