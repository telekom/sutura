//! Serde on a type that PARSES: the derive that walks past the constructor, and the pair of
//! derives that disagree about the shape.
//!
//! Both rules come from the newtype guide this repo adopts as policy, both said *review* in
//! `.agents/skills/engineering/rust/SKILL.md`, and both are syntactic - which is the whole
//! argument for a gate over a sentence. `AGENTS.md`: *a rule with no mechanism is a wish*.
//!
//! # Rule one: a derived `Deserialize` writes past `parse`
//!
//! The skill calls this *"the one that bites"*. A derived `Deserialize` writes straight into the
//! private field, so **every check the constructor performs is bypassed by the one path that
//! carries untrusted input** - and a catalog document is untrusted input by this repository's own
//! threat model. `#[serde(try_from = "..")]` routes it through the constructor instead; a
//! hand-written `impl Deserialize` does too, and both count here.
//!
//! What makes a type subject to the rule is that it HAS a fallible constructor - an associated
//! function taking no `self` and returning `Result<Self, ..>`. A type with none establishes no
//! invariant at construction, so a derive bypasses nothing: `Query`, `Anchor`,
//! `sutura_http::wire::QuestionBody` and the `Raw*` settings shapes are all in that class, and all
//! of them are correct as they stand. **A gate that failed them would be a gate somebody
//! disables**, which is the reasoning `deny.toml`'s duplicate-version comment already carries.
//!
//! # Rule two: `try_from` moves `Deserialize` and leaves `Serialize` where it was
//!
//! `#[serde(try_from = "T")]` affects **`Deserialize` only**. A derived `Serialize` beside it still
//! writes the struct - so the type reads one shape and writes another. That is not hypothetical
//! here: `Date` shipped exactly it, and it mattered because the definition digest is taken over the
//! serialized form, so the digest covered a field layout that appears in no catalog file.
//!
//! Two shapes make the pair symmetric, and [`round_trips`] accepts exactly those two:
//!
//! * a **newtype struct** whose single field's type is `T`. Serde derives a newtype struct as its
//!   inner value, so both directions are `T` - which is why the nine `pub struct X(String)` types
//!   carrying `try_from = "String"` are correct with a derived `Serialize`;
//! * a **named-field struct** whose `T` is a struct declared in the same file with the same field
//!   names. That is `TimeRange` over `TimeRangeInput`: the wire form is the two-field mapping both
//!   halves already agree on, and there is no canonical text form to convert into.
//!
//! Anything else needs `#[serde(into = "..")]` or a hand-written `impl Serialize`, and `Date` and
//! `QualifiedTable` are the two that take those routes today.
//!
//! # Scope, and the limits
//!
//! Every Rust file the repo tracks, `vendor/` excluded - not library crates only, which is where
//! this differs from `boundaries::api_shape` next door and does so deliberately. That gate's rules
//! are about a contract another crate depends on, so a binary is out of scope. These two are about
//! the path untrusted input takes into a value, which is the same path in a binary.
//!
//! * **A fallible constructor is recognised by `-> Result<Self`**, which is the idiom here (139
//!   occurrences on 2026-09-02) rather than the language. One written `-> Result<MyType, ..>` is not
//!   seen, and the direction is the safe one: the gate under-claims rather than failing correct code.
//! * **Rule two compares field NAMES, not types.** Two structs with the same names whose fields
//!   serialize differently pass. Comparing serialized shapes needs serde's own resolution, which is
//!   not something a text scan may pretend to.
//! * `impl` blocks are matched per FILE, so a type whose fallible constructor lives in another
//!   module of the same crate is not seen. Every one in this workspace is beside its type.
//! * It does not parse Rust. [`scan`] carries the rest of that argument, and the tests for it.

// `pub(crate)` rather than private, for one item: `crate::newtype_leaks` reads `scan::code_lines`,
// because "each line of this file with everything that is not code blanked out" is one question and
// a second implementation of it would be a second thing to keep in step. Nothing else in there is
// crate-visible.
pub(crate) mod scan;

use crate::Verdict;
use crate::repo;
use crate::serde_parse::scan::{Declared, Shape};

/// What one file's scan found. Held together because both rules need the same walks.
struct FileFacts {
    /// Every struct in the file.
    declared: Vec<Declared>,
    /// Type name to the name of a fallible constructor it declares.
    parses: std::collections::BTreeMap<String, String>,
    /// Types with a hand-written `impl .. Deserialize .. for`.
    reads_by_hand: std::collections::BTreeSet<String>,
    /// Types with a hand-written `impl .. Serialize .. for`.
    writes_by_hand: std::collections::BTreeSet<String>,
}

pub(crate) fn run(_args: &[String]) -> Verdict {
    let Some(repo::RepoFiles { root, files }) = repo::all_files() else {
        eprintln!("xtask check-serde-parse: could not determine the repo root");
        return Verdict::Fail;
    };

    let mut problems: Vec<String> = Vec::new();
    let mut scanned = 0_usize;
    let mut structs = 0_usize;
    for rel in &files {
        if !in_scope(rel) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(root.join(rel)) else {
            continue;
        };
        scanned = scanned.saturating_add(1);
        let facts = facts_of(&text);
        structs = structs.saturating_add(facts.declared.len());
        problems.extend(bypassed_constructors(rel, &facts));
        problems.extend(asymmetric_serde(rel, &facts));
    }

    if scanned == 0 {
        // A gate that silently checked nothing is the failure mode a gate exists to prevent.
        eprintln!("xtask check-serde-parse: no Rust source in scope - this gate would check nothing");
        return Verdict::Fail;
    }
    if problems.is_empty() {
        println!("xtask check-serde-parse: ok - {structs} struct(s) in {scanned} file(s), serde routed through parse");
        return Verdict::Pass;
    }

    eprintln!("xtask check-serde-parse: FAILED - serde walks past a constructor:");
    for problem in &problems {
        eprintln!("  {problem}");
    }
    eprintln!();
    explain();
    Verdict::Fail
}

/// Printed on failure, because a rule whose reason is unstated gets reverted.
fn explain() {
    eprintln!("A newtype's whole return is that an existing value is a valid one, so nothing");
    eprintln!("downstream re-checks. Two serde derives take that away silently:");
    eprintln!("  * `#[derive(Deserialize)]` writes straight into the private field, and the path");
    eprintln!("    it writes on is the one carrying untrusted input. `#[serde(try_from = \"..\")]`");
    eprintln!("    plus a `TryFrom` that calls the constructor is the fix - one code path, not two.");
    eprintln!("  * `#[serde(try_from = ..)]` affects `Deserialize` ONLY. A derived `Serialize`");
    eprintln!("    beside it writes the struct, so the type reads text and writes a field layout.");
    eprintln!("    `Date` shipped that, and the definition digest is taken over the serialized");
    eprintln!("    form - so it covered a shape no catalog file contains. `#[serde(into = \"..\")]`");
    eprintln!("    or a hand-written `impl Serialize` is the fix.");
    eprintln!("If a case here genuinely belongs, change the rule in xtask/src/serde_parse.rs with");
    eprintln!("the reason: that is an architecture decision and should be a visible diff.");
}

/// Is this a Rust file this gate judges?
///
/// `vendor/` is excluded because third-party code adapted here is upstream's shape, and
/// `VENDOR.md` is where a local change to it is argued rather than a lint.
fn in_scope(rel: &str) -> bool {
    !rel.starts_with("vendor/")
        && std::path::Path::new(rel)
            .extension()
            .and_then(std::ffi::OsStr::to_str)
            .is_some_and(|ext| ext.eq_ignore_ascii_case("rs"))
}

/// Everything both rules need from one file.
fn facts_of(text: &str) -> FileFacts {
    let code = scan::code_lines(text);
    let raw: Vec<&str> = text.lines().collect();
    FileFacts {
        declared: scan::declarations(&code, &raw),
        parses: scan::fallible_constructors(&code),
        reads_by_hand: scan::hand_written(&code, "Deserialize"),
        writes_by_hand: scan::hand_written(&code, "Serialize"),
    }
}

/// Rule one: a type that parses, deriving `Deserialize` on a path that skips the constructor.
fn bypassed_constructors(rel: &str, facts: &FileFacts) -> Vec<String> {
    let mut problems = Vec::new();
    for declaration in &facts.declared {
        let Some(constructor) = facts.parses.get(&declaration.name) else {
            continue;
        };
        if !scan::derives(&declaration.attrs, "Deserialize") {
            continue;
        }
        if scan::serde_arg(&declaration.attrs, "try_from").is_some() || facts.reads_by_hand.contains(&declaration.name) {
            continue;
        }
        problems.push(format!(
            "{rel}:{}: `{}` derives Deserialize but is constructed by `{}` - the derive writes past it",
            declaration.line, declaration.name, constructor
        ));
    }
    problems
}

/// Rule two: `try_from` on the way in, a derived `Serialize` writing another shape on the way out.
fn asymmetric_serde(rel: &str, facts: &FileFacts) -> Vec<String> {
    let mut problems = Vec::new();
    for declaration in &facts.declared {
        let Some(target) = scan::serde_arg(&declaration.attrs, "try_from") else {
            continue;
        };
        if !scan::derives(&declaration.attrs, "Serialize") {
            continue;
        }
        if scan::serde_arg(&declaration.attrs, "into").is_some() || facts.writes_by_hand.contains(&declaration.name) {
            continue;
        }
        if round_trips(&declaration.shape, &target, &facts.declared) {
            continue;
        }
        problems.push(format!(
            "{rel}:{}: `{}` deserializes from `{target}` and serializes its own fields - the two shapes differ",
            declaration.line, declaration.name
        ));
    }
    problems
}

/// Does a derived `Serialize` on `shape` write what `target` deserializes from?
///
/// The two accepted cases are the module documentation's two: a newtype struct over `target`
/// itself, which serde writes as its inner value; and a named-field struct whose `target` is a
/// struct declared beside it with the same field names.
fn round_trips(shape: &Shape, target: &str, declared: &[Declared]) -> bool {
    match *shape {
        Shape::Newtype(ref inner) => inner == target,
        Shape::Named(ref fields) => declared
            .iter()
            .find(|other| other.name == target)
            .is_some_and(|other| matches!(other.shape, Shape::Named(ref theirs) if theirs == fields)),
        Shape::Other => false,
    }
}

#[cfg(test)]
mod tests {
    use super::{asymmetric_serde, bypassed_constructors, facts_of, in_scope};

    /// The two rules over one file's text, as the gate runs them.
    fn findings(text: &str) -> Vec<String> {
        let facts = facts_of(text);
        let mut problems = bypassed_constructors("x.rs", &facts);
        problems.extend(asymmetric_serde("x.rs", &facts));
        problems
    }

    /// A validated newtype, with the attribute run given as a parameter so each case below
    /// differs by exactly the line under test.
    fn newtype(attrs: &str) -> String {
        format!(
            "{attrs}\npub struct Digest(String);\n\nimpl Digest {{\n    pub fn parse(raw: &str) -> Result<Self, Bad> {{\n        todo!()\n    }}\n}}\n"
        )
    }

    #[test]
    fn a_validated_newtype_cannot_derive_deserialize_without_going_through_parse() {
        let found = findings(&newtype("#[derive(Debug, serde::Deserialize)]"));
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found.first().is_some_and(|p| p.contains("writes past it")), "{found:?}");
    }

    #[test]
    fn try_from_is_what_makes_the_same_derive_correct() {
        let attrs = "#[derive(Debug, serde::Deserialize)]\n#[serde(try_from = \"String\")]";
        assert!(findings(&newtype(attrs)).is_empty());
    }

    #[test]
    fn a_hand_written_deserialize_is_also_a_route_through_the_constructor() {
        let text = format!(
            "{}impl<'de> serde::Deserialize<'de> for Digest {{}}\n",
            newtype("#[derive(Debug)]")
        );
        assert!(findings(&text).is_empty());
    }

    #[test]
    fn a_type_with_no_fallible_constructor_may_derive_deserialize() {
        // `Query`, `Anchor` and the `Raw*` settings shapes are this class: nothing is checked at
        // construction, so the derive walks past nothing.
        let text = "#[derive(serde::Deserialize)]\npub struct Query {\n    metric: MetricName,\n}\n";
        assert!(findings(text).is_empty());
    }

    #[test]
    fn an_infallible_constructor_is_not_a_parse() {
        let text = "#[derive(serde::Deserialize)]\npub struct Anchor {\n    value: String,\n}\n\nimpl Anchor {\n    pub const fn new(value: String) -> Self {\n        Self { value }\n    }\n}\n";
        assert!(findings(text).is_empty());
    }

    #[test]
    fn a_type_that_parses_on_the_way_in_serializes_the_way_it_came() {
        // The `Date` class: text in, a field layout out, and a digest taken over the layout.
        let text = "#[derive(serde::Serialize, serde::Deserialize)]\n#[serde(try_from = \"String\")]\npub struct Date {\n    year: i32,\n    month: u8,\n}\n";
        let found = findings(text);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(
            found.first().is_some_and(|p| p.contains("the two shapes differ")),
            "{found:?}"
        );
    }

    #[test]
    fn an_into_beside_the_try_from_is_the_fix() {
        let text = "#[derive(serde::Serialize, serde::Deserialize)]\n#[serde(try_from = \"String\", into = \"String\")]\npub struct Date {\n    year: i32,\n}\n";
        assert!(findings(text).is_empty());
    }

    #[test]
    fn a_hand_written_serialize_is_the_other_fix() {
        let text = "#[derive(serde::Serialize, serde::Deserialize)]\n#[serde(try_from = \"String\")]\npub struct Qualified {\n    part: String,\n}\n\nimpl serde::Serialize for Qualified {}\n";
        assert!(findings(text).is_empty());
    }

    #[test]
    fn a_newtype_over_the_try_from_target_round_trips_transparently() {
        // The nine `pub struct X(String)` types in `sutura-domain`: serde writes a newtype struct
        // as its inner value, so both directions are `String`.
        let text =
            "#[derive(serde::Serialize, serde::Deserialize)]\n#[serde(try_from = \"String\")]\npub struct Digest(String);\n";
        assert!(findings(text).is_empty());
    }

    #[test]
    fn a_newtype_over_something_else_does_not() {
        let text = "#[derive(serde::Serialize, serde::Deserialize)]\n#[serde(try_from = \"String\")]\npub struct Days(u16);\n";
        assert_eq!(findings(text).len(), 1);
    }

    #[test]
    fn a_named_struct_whose_input_shape_matches_round_trips() {
        // `TimeRange` over `TimeRangeInput`: the wire form is the mapping both halves agree on.
        let text = "#[derive(serde::Serialize, serde::Deserialize)]\n#[serde(try_from = \"RangeInput\")]\npub struct Range {\n    start: Date,\n    end: Date,\n}\n\n#[derive(serde::Deserialize)]\nstruct RangeInput {\n    start: Date,\n    end: Date,\n}\n";
        assert!(findings(text).is_empty());
    }

    #[test]
    fn a_named_struct_whose_input_shape_differs_does_not() {
        let text = "#[derive(serde::Serialize, serde::Deserialize)]\n#[serde(try_from = \"RangeInput\")]\npub struct Range {\n    start: Date,\n    end: Date,\n}\n\n#[derive(serde::Deserialize)]\nstruct RangeInput {\n    from: Date,\n    to: Date,\n}\n";
        assert_eq!(findings(text).len(), 1);
    }

    #[test]
    fn a_multi_line_derive_is_still_one_attribute_run() {
        let attrs = "#[derive(\n    Debug,\n    serde::Deserialize,\n)]";
        assert_eq!(findings(&newtype(attrs)).len(), 1);
    }

    #[test]
    fn a_doc_comment_between_the_attributes_and_the_struct_does_not_break_the_run() {
        let attrs = "#[derive(serde::Deserialize)]\n/// A digest.";
        assert_eq!(findings(&newtype(attrs)).len(), 1);
    }

    #[test]
    fn a_declaration_inside_a_rustdoc_example_is_not_a_declaration() {
        // The confound that makes comment blanking necessary rather than tidy: this repo's
        // doctests declare types, and one of them deriving Deserialize is not a violation.
        let text = "/// ```\n/// #[derive(serde::Deserialize)]\n/// pub struct Digest(String);\n/// impl Digest { pub fn parse(r: &str) -> Result<Self, E> { todo!() } }\n/// ```\npub fn f() {}\n";
        assert!(findings(text).is_empty());
    }

    #[test]
    fn a_declaration_inside_a_multi_line_string_is_not_a_declaration() {
        // Which is what makes this module's own fixtures invisible to the gate that reads them.
        let text = "fn fixture() -> &'static str {\n    r#\"\n#[derive(serde::Deserialize)]\npub struct Digest(String);\nimpl Digest { pub fn parse(r: &str) -> Result<Self, E> { todo!() } }\n\"#\n}\n";
        assert!(findings(text).is_empty());
    }

    #[test]
    fn vendored_code_is_out_of_scope_and_rust_files_are_in_it() {
        assert!(in_scope("crates/sutura-domain/src/lib.rs"));
        assert!(!in_scope("vendor/mimalloc_rust/src/lib.rs"));
        assert!(!in_scope("AGENTS.md"));
    }
}
