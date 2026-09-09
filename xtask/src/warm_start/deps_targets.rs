//! Does every dependency-only build SAY whether it compiles test targets?
//!
//! crane's `buildDepsOnly` defaults `doCheck` to `true`, and that default is not free: the deps
//! derivation then runs `cargo check`, `cargo build` AND `cargo test --no-run`, so the whole
//! closure is codegen'd twice. Measured on `nix/jscpd.nix`'s deps derivation, before and after
//! stating `doCheck = false`: **231 `Compiling` lines fell to 137**, the 94 being a second
//! codegen of a 94-crate closure that nothing in this repository can use, because that
//! derivation's only consumer sets `doCheck = false` and never builds a test target.
//!
//! WHY A GATE RATHER THAN THE COMMENT BESIDE EACH SITE. Getting this wrong breaks nothing. The
//! artifacts are still correct, every check still reaches the same verdict, the tree is still
//! green - it just compiled a closure twice for nobody, which is the shape of waste that survives
//! longest because no failure ever attributes it to the line that caused it. The default is
//! SILENT, and silence is what this refuses: a `buildDepsOnly` that does not state `doCheck` is
//! taking crane's default without having decided anything.
//!
//! WHAT IT DOES NOT HOLD, next to the claim. It holds that the choice is MADE, never that it is
//! correct - `doCheck = true` passes this gate whether or not any consumer builds a test target.
//! Deciding that would need the producer-to-consumer graph, which [`super::pairing`] builds for a
//! different question; the reason it is not reused here is that a `true` this gate accepted is
//! still a line in a diff with a reason beside it, which is what review can act on.
//!
//! IT READS THROUGH A NAME, not only off the application. `crossLib.buildDepsOnly args` states
//! nothing at the call, and refusing it would be a gate insisting on a spelling: the decision may
//! perfectly well live in the `args` binding it names, and in `nix/shipped.nix` it does. So a bare
//! identifier is followed to its NEAREST PRECEDING binding in the same file and the decision is
//! looked for there. **The limit:** nearest-preceding is lexical scope as it is actually written
//! here, not as nix defines it - two `let`s in one file can each bind `args`, which is exactly the
//! case this reads correctly, but a binding that arrives as a function PARAMETER or from an
//! imported module is not followed, and is refused rather than guessed at.
//!
//! FAIL CLOSED, like its neighbours: a scan that finds no `buildDepsOnly` at all is a broken scan
//! rather than a tree without one, and an argument this cannot resolve to a decision - a name with
//! no binding it can see, or a shape it cannot read - is a failure naming the file and line.

use super::pairing::NixCode;

/// crane's dependency-only constructor - the one whose `doCheck` default costs a cargo pass.
const PRODUCER: &str = "buildDepsOnly";

/// The attribute every application of it has to state.
const DECIDED: &str = "doCheck";

/// One application of [`PRODUCER`]: where a reader will find it, and where its argument starts.
struct Site {
    rel: String,
    line: usize,
    /// The offset just past the constructor's name, which is where its argument begins.
    argument: usize,
}

/// Every application of [`PRODUCER`] states [`DECIDED`], or a failure naming the first that does not.
///
/// Each file arrives as the nix CODE view, comments and string interiors already blanked, and the
/// blanking matters: a `doCheck` written in a COMMENT beside the call would otherwise satisfy a
/// gate whose entire subject is what the evaluator sees.
pub(super) fn holds(files: &[NixCode]) -> Result<Vec<String>, String> {
    let mut decided = Vec::new();
    let mut seen = 0_usize;
    for NixCode { rel, code } in files {
        for site in applications(rel, code) {
            seen = seen.saturating_add(1);
            let argument = decision_text(code, site.argument).ok_or_else(|| {
                format!(
                    "{}:{} applies `{PRODUCER}` and this gate could not resolve its argument to \
                     anywhere `{DECIDED}` could be written, so it has checked NOTHING about that \
                     site. Either parenthesise it - `{PRODUCER} (args // {{ {DECIDED} = \
                     <true|false>; }})` - or name a variable this file binds, so the decision is \
                     somewhere a reader and this gate both look",
                    site.rel, site.line
                )
            })?;
            if !mentions(&argument, DECIDED) {
                return Err(format!(
                    "{}:{} applies `{PRODUCER}` without stating `{DECIDED}`, so it takes crane's \
                     default of `true` and builds test targets by accident rather than by decision.\n\n\
                     That default runs a THIRD cargo pass over the whole dependency closure \
                     (`cargo test --no-run` after `cargo check` and `cargo build`) purely to cache \
                     dev-dependencies. Measured on nix/jscpd.nix: 231 `Compiling` lines against 137 \
                     with `{DECIDED} = false`. Nothing goes red when it is wrong, which is why this \
                     is a gate.\n\n\
                     Write `{DECIDED} = true;` where a consumer really does build test targets - a \
                     `cargoNextest` check does - and `{DECIDED} = false;` where none does. Either way \
                     the reason goes beside it.",
                    site.rel, site.line
                ));
            }
            decided.push(format!("{}:{}", site.rel, site.line));
        }
    }
    if seen == 0 {
        return Err(format!(
            "no nix file in this tree applies `{PRODUCER}`, so this gate adjudicated NOTHING. \
             crane builds this workspace's dependency closure through exactly that constructor, so \
             a scan that finds none of them is broken rather than satisfied"
        ));
    }
    Ok(decided)
}

/// Where [`PRODUCER`] is APPLIED in one file, as a whole word.
///
/// A whole word so `buildDepsOnlyFor` - or any wrapper someone writes - is not mistaken for the
/// constructor itself, and so this cannot be satisfied by a longer identifier that happens to
/// contain the name.
fn applications(rel: &str, code: &str) -> Vec<Site> {
    let mut found = Vec::new();
    let mut from = 0_usize;
    while let Some(offset) = code.get(from..).and_then(|rest| rest.find(PRODUCER)) {
        let start = from.saturating_add(offset);
        let end = start.saturating_add(PRODUCER.len());
        from = end;
        if !whole_word(code, start, end) {
            continue;
        }
        let line = code.get(..start).map_or(1, |head| head.lines().count().max(1));
        found.push(Site {
            rel: String::from(rel),
            line,
            argument: end,
        });
    }
    found
}

/// The text in which the decision must appear, or `None` when there is nowhere it could be.
///
/// Two shapes, because both are written here. A PARENTHESISED argument carries its own attrset and
/// is read as it stands. A BARE IDENTIFIER names a variable, and the decision may be inside that
/// variable's value - so it is followed, and the binding's value is what gets read.
fn decision_text(code: &str, from: usize) -> Option<String> {
    if let Some(parenthesised) = argument_of(code, from) {
        return Some(parenthesised);
    }
    let name = named_argument(code, from)?;
    bound_value(code, from, &name)
}

/// The bare identifier applied at `from`, if that is the shape.
fn named_argument(code: &str, from: usize) -> Option<String> {
    let rest = code.get(from..)?;
    let start = rest.find(|c: char| !c.is_whitespace())?;
    let tail = rest.get(start..)?;
    let word = |c: char| c.is_alphanumeric() || matches!(c, '_' | '-' | '\'');
    let end = tail.find(|c: char| !word(c)).unwrap_or(tail.len());
    let name = tail.get(..end)?;
    let first = name.chars().next()?;
    (first.is_alphabetic() || first == '_').then(|| String::from(name))
}

/// The value of `name`'s NEAREST PRECEDING binding in this file.
///
/// Nearest-preceding rather than first: `nix/shipped.nix` binds `args` twice, once per builder,
/// and the first one is not the one a later application means. The value runs from the `=` to the
/// first `;` at nesting depth zero, so an attrset or a `//` chain arrives whole.
fn bound_value(code: &str, before: usize, name: &str) -> Option<String> {
    let head = code.get(..before)?;
    let mut binding = None;
    let mut from = 0_usize;
    while let Some(offset) = head.get(from..).and_then(|rest| rest.find(name)) {
        let start = from.saturating_add(offset);
        let end = start.saturating_add(name.len());
        from = end;
        if !whole_word(head, start, end) {
            continue;
        }
        let after = head.get(end..)?.trim_start();
        if after.starts_with('=') && !after.starts_with("==") {
            binding = Some(end);
        }
    }
    let equals = code.get(binding?..)?.find('=')?.saturating_add(binding?);
    let value = code.get(equals.saturating_add(1)..)?;
    let mut depth = 0_usize;
    for (index, character) in value.char_indices() {
        match character {
            '{' | '(' | '[' => depth = depth.saturating_add(1),
            '}' | ')' | ']' => depth = depth.saturating_sub(1),
            ';' if depth == 0 => return value.get(..index).map(String::from),
            _ => {}
        }
    }
    None
}

/// The parenthesised argument that follows, or `None` when what follows is not one.
fn argument_of(code: &str, from: usize) -> Option<String> {
    let rest = code.get(from..)?;
    let open = rest.find(|c: char| !c.is_whitespace())?;
    if rest.get(open..).and_then(|tail| tail.chars().next()) != Some('(') {
        return None;
    }
    let mut depth = 0_usize;
    for (index, character) in rest.get(open..)?.char_indices() {
        match character {
            '(' => depth = depth.saturating_add(1),
            ')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return rest.get(open..open.saturating_add(index)).map(String::from);
                }
            }
            _ => {}
        }
    }
    None
}

/// Is `name` mentioned as a whole word in `text`?
fn mentions(text: &str, name: &str) -> bool {
    let mut from = 0_usize;
    while let Some(offset) = text.get(from..).and_then(|rest| rest.find(name)) {
        let start = from.saturating_add(offset);
        let end = start.saturating_add(name.len());
        from = end;
        if whole_word(text, start, end) {
            return true;
        }
    }
    false
}

/// Is this the whole identifier, rather than the tail of a longer one?
///
/// A copy of [`super::pairing`]'s predicate rather than a shared import, because that one is
/// `pub(super)` to a module whose types this one deliberately does not take.
fn whole_word(code: &str, start: usize, end: usize) -> bool {
    let word = |c: char| c.is_alphanumeric() || matches!(c, '_' | '-' | '\'');
    let before = code.get(..start).and_then(|head| head.chars().next_back());
    let after = code.get(end..).and_then(|tail| tail.chars().next());
    !before.is_some_and(word) && !after.is_some_and(word)
}

#[cfg(test)]
mod tests {
    use super::NixCode;

    fn file(code: &str) -> Vec<NixCode> {
        vec![NixCode {
            rel: String::from("nix/example.nix"),
            code: String::from(code),
        }]
    }

    #[test]
    fn a_stated_decision_passes_either_way() {
        for stated in ["doCheck = false;", "doCheck = true;"] {
            let code = format!("craneLib.buildDepsOnly (args // {{ {stated} }})");
            assert!(super::holds(&file(&code)).is_ok(), "{stated} should pass");
        }
    }

    #[test]
    fn taking_cranes_default_silently_is_refused() {
        let why = super::holds(&file("craneLib.buildDepsOnly (args // { pname = \"x\"; })")).unwrap_err();
        assert!(why.contains("without stating `doCheck`"), "{why}");
        assert!(why.contains("nix/example.nix:1"), "{why}");
    }

    #[test]
    fn a_bare_argument_with_no_binding_this_file_can_see_is_refused() {
        let why = super::holds(&file("ciArtifacts = craneLib.buildDepsOnly ciArgs;")).unwrap_err();
        assert!(why.contains("could not resolve its argument"), "{why}");
    }

    #[test]
    fn a_decision_inside_the_named_binding_counts() {
        let code = concat!(
            "  args = commonArgs // { doCheck = false; };\n",
            "  out = crossLib.buildPackage (args // inheritedArtifacts (crossLib.buildDepsOnly args));\n",
        );
        assert!(super::holds(&file(code)).is_ok(), "should read through the binding");
    }

    #[test]
    fn the_nearest_preceding_binding_is_the_one_read() {
        // Two builders, one file, both binding `args` - the shape `nix/shipped.nix` has. The
        // FIRST binding states the decision and the second does not, so a gate reading the first
        // would pass this and be wrong about the site it is judging.
        let code = concat!(
            "  args = commonArgs // { doCheck = false; };\n",
            "  native = craneLib.buildDepsOnly args;\n",
            "  args = commonArgs // { CARGO_PROFILE = \"release\"; };\n",
            "  cross = crossLib.buildDepsOnly args;\n",
        );
        let why = super::holds(&file(code)).unwrap_err();
        assert!(why.contains("without stating `doCheck`"), "{why}");
    }

    #[test]
    fn a_decision_written_in_a_comment_does_not_count() {
        // The caller hands this gate the CODE view, comments already blanked. Blanked text is
        // what a comment looks like by the time it arrives, so this is that case and not a
        // second parser: the gate must not read a decision out of it.
        let blanked = "craneLib.buildDepsOnly (args //  {                })";
        let why = super::holds(&file(blanked)).unwrap_err();
        assert!(why.contains("without stating `doCheck`"), "{why}");
    }

    #[test]
    fn a_longer_identifier_is_not_the_constructor() {
        let why = super::holds(&file("craneLib.buildDepsOnlyFor (args // { })")).unwrap_err();
        assert!(why.contains("adjudicated NOTHING"), "{why}");
    }

    #[test]
    fn a_scan_that_finds_no_producer_is_a_failure_and_not_a_pass() {
        let why = super::holds(&file("craneLib.buildPackage args")).unwrap_err();
        assert!(why.contains("adjudicated NOTHING"), "{why}");
    }
}
