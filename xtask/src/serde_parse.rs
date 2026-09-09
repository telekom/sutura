//! Serde on a type that PARSES: the derive that walks past the constructor, the pair of derives
//! that disagree about the shape, and the input struct that accepts a key nobody declared.
//!
//! The first two come from the newtype guide this repo adopts as policy, both said *review* in
//! `.agents/skills/engineering/rust/SKILL.md`, and all three are syntactic - which is the whole
//! argument for a gate over a sentence. `AGENTS.md`: *a rule with no mechanism is a wish*.
//!
//! # Rule one: a derived `Deserialize` writes past `parse`
//!
//! A plain derived `Deserialize` constructs a value without calling its fallible constructor.
//! For a validated struct, that can write past checks on
//! untrusted catalog input. Rule one also visits enums: this is a routing policy, not evidence that
//! every enum's direct derive admits an invalid value. `#[serde(try_from = "..")]` and a hand-written
//! `impl Deserialize` are accepted routes here; **the scanner does not inspect their implementations
//! or prove that they call the constructor**.
//!
//! What makes a type subject to the rule is that it HAS a fallible constructor - an associated
//! function taking no `self` and returning `Result<Self, ..>`. Without a recognised fallible
//! constructor, this rule has no bypass to check: `Query`,
//! `sutura_http::wire::QuestionBody` and the `Raw*` settings shapes are all in that class, and all
//! of them are outside this rule. **A gate that failed them would be a gate somebody
//! disables**, which is the reasoning `deny.toml`'s duplicate-version comment already carries.
//!
//! **And the class is not a defence, which is where this gate's limit is worth stating.** `Anchor`
//! used to be listed above and was correct under this rule for the whole time its `value` was an
//! unparsed `String`: a type with no invariant to bypass passes because there is nothing to bypass,
//! not because nothing is wrong. What was wrong was that the field wanted a newtype, and no gate
//! here decides which authored scalars want one. That judgement is review's, and #266 is what
//! caught it.
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
//! Every other struct shape needs `#[serde(into = "..")]` or a hand-written `impl Serialize`.
//! `Date` and `QualifiedTable` are the two that take those routes today.
//!
//! # Rule three: the input struct that accepts a key nobody declared
//!
//! The second shape above is where a real defect lived, and it lived there for as long as it did
//! because nothing read the claim. `sutura_domain::calendar::TimeRangeInput` carried no
//! `deny_unknown_fields`, so a key written INSIDE a `range:` object was discarded in silence -
//! on a question, on a markdown metric's `anchor.range`, and on the `sutura` structured property
//! `sutura-catalog-datahub` decodes - while five documents said the closedness held *at every
//! depth*. The outer type's own attribute cannot reach it: `try_from` hands the whole mapping to
//! the input struct, so the input struct is where the keys are accepted or refused.
//!
//! This rule adds no recognition. It is the class rule two already pairs by field names, plus the
//! attribute check that class was missing, so what it costs is one lookup. A `try_from = "String"`
//! names no declaration and a newtype has no fields to deny, which is why both are out of it.
//!
//! # Scope, and the limits
//!
//! Every Rust file the repo tracks, `vendor/` excluded - not library crates only, which is where
//! this differs from `boundaries::api_shape` next door and does so deliberately. That gate's rules
//! are about a contract another crate depends on, so a binary is out of scope. These three are
//! about the path untrusted input takes into a value, which is the same path in a binary.
//!
//! * **Enums are subjects of rule one only.** Rules two and three retain their struct-source
//!   scope; no enum variant or serde tagging layout is inferred from struct field names.
//! * Declaration names are ASCII name fragments following `struct ` or `enum ` on the same line,
//!   after optional visibility. This is a lexical walk, not Rust identifier or macro resolution.
//! * **A fallible constructor is recognised by `-> Result<Self`**, which is the idiom here (139
//!   occurrences on 2026-09-02) rather than the language. One written `-> Result<MyType, ..>` is not
//!   seen, and the direction is the safe one: the gate under-claims rather than failing correct code.
//! * **Rule two compares field NAMES, not types.** Two structs with the same names whose fields
//!   serialize differently pass. Comparing serialized shapes needs serde's own resolution, which is
//!   not something a text scan may pretend to.
//! * **Rule three reads one attribute run and nothing about the FIELDS' own types.** A field whose
//!   type is another struct is closed by that struct's own attribute, which the rule reaches only
//!   where that struct is itself a `try_from` target - so a nested shape reached no other way is
//!   held by rule one and by review.
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
use crate::serde_parse::scan::{Declared, Kind, Shape};

/// What one file's scan found. Held together because all three rules need the same walks.
struct FileFacts {
    /// Every recognised struct or enum in the file.
    declared: Vec<Declared>,
    /// Type name to the name of a fallible constructor it declares.
    parses: std::collections::BTreeMap<String, String>,
    /// Types with a hand-written `impl .. Deserialize .. for`.
    reads_by_hand: std::collections::BTreeSet<String>,
    /// Types with a hand-written `impl .. Serialize .. for`.
    writes_by_hand: std::collections::BTreeSet<String>,
}

pub(crate) fn run(_args: &[String]) -> Verdict {
    let (root, files) = match repo::all_files().and_then(|census| census.into_listing(repo::Unmigrated::SerdeParse)) {
        Ok(listing) => listing,
        Err(why) => {
            eprintln!("xtask check-serde-parse: FAILED - {}", why.describe());
            return Verdict::Fail;
        }
    };

    let mut problems: Vec<String> = Vec::new();
    let mut scanned = 0_usize;
    let mut declarations = 0_usize;
    for rel in &files {
        if !in_scope(rel) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(root.join(rel)) else {
            continue;
        };
        scanned = scanned.saturating_add(1);
        let facts = facts_of(&text);
        declarations = declarations.saturating_add(facts.declared.len());
        problems.extend(bypassed_constructors(rel, &facts));
        problems.extend(asymmetric_serde(rel, &facts));
        problems.extend(open_input_structs(rel, &facts));
    }

    if scanned == 0 {
        // A gate that silently checked nothing is the failure mode a gate exists to prevent.
        eprintln!("xtask check-serde-parse: no Rust source in scope - this gate would check nothing");
        return Verdict::Fail;
    }
    if problems.is_empty() {
        println!(
            "xtask check-serde-parse: ok - {declarations} struct or enum declaration(s) in {scanned} file(s), serde route rules satisfied"
        );
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
    eprintln!("  * an input struct a `try_from` names WITHOUT `deny_unknown_fields` accepts a key");
    eprintln!("    nobody declared and drops it. The outer type's own attribute cannot reach it:");
    eprintln!("    `try_from` hands the whole mapping to the input struct. `TimeRangeInput` was");
    eprintln!("    that, and five documents said the closedness held at every depth while a key");
    eprintln!("    inside a `range:` was discarded in silence. The attribute is the fix.");
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
        let Kind::Struct(ref shape) = declaration.kind else {
            continue;
        };
        let Some(target) = scan::serde_arg(&declaration.attrs, "try_from") else {
            continue;
        };
        if !scan::derives(&declaration.attrs, "Serialize") {
            continue;
        }
        if scan::serde_arg(&declaration.attrs, "into").is_some() || facts.writes_by_hand.contains(&declaration.name) {
            continue;
        }
        if round_trips(shape, &target, &facts.declared) {
            continue;
        }
        problems.push(format!(
            "{rel}:{}: `{}` deserializes from `{target}` and serializes its own fields - the two shapes differ",
            declaration.line, declaration.name
        ));
    }
    problems
}

/// Rule three: the input struct a `try_from` names, accepting keys nobody declared.
///
/// Scoped to a named-field input struct declared in the same file, which is the class rule two
/// already identifies - `deny_unknown_fields` means nothing on a newtype, and `try_from = "String"`
/// names no declaration to check. So the rule adds no new recognition, only the attribute check the
/// class was missing.
fn open_input_structs(rel: &str, facts: &FileFacts) -> Vec<String> {
    let mut problems = Vec::new();
    for declaration in &facts.declared {
        if !matches!(declaration.kind, Kind::Struct(_)) {
            continue;
        }
        let Some(target) = scan::serde_arg(&declaration.attrs, "try_from") else {
            continue;
        };
        let Some(input) = facts.declared.iter().find(|other| other.name == target) else {
            continue;
        };
        if !matches!(input.kind, Kind::Struct(Shape::Named(_))) || scan::serde_flag(&input.attrs, "deny_unknown_fields") {
            continue;
        }
        problems.push(format!(
            "{rel}:{}: `{target}` is the input shape `{}` deserializes through and does not deny unknown \
             fields - a key inside it is dropped in silence",
            input.line, declaration.name
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
            .is_some_and(|other| matches!(other.kind, Kind::Struct(Shape::Named(ref theirs)) if theirs == fields)),
        Shape::Other => false,
    }
}

#[cfg(test)]
mod tests {
    use super::{asymmetric_serde, bypassed_constructors, facts_of, in_scope, open_input_structs};

    /// The three rules over one file's text, as the gate runs them.
    fn findings(text: &str) -> Vec<String> {
        let facts = facts_of(text);
        let mut problems = bypassed_constructors("x.rs", &facts);
        problems.extend(asymmetric_serde("x.rs", &facts));
        problems.extend(open_input_structs("x.rs", &facts));
        problems
    }

    /// A `Range` over an input struct, with the input struct's attribute run given as a parameter
    /// so each case below differs by exactly the line under test.
    fn over_input(input_attrs: &str, input_fields: &str) -> String {
        format!(
            "#[derive(serde::Serialize, serde::Deserialize)]\n#[serde(try_from = \"RangeInput\")]\npub struct Range {{\n    start: Date,\n    end: Date,\n}}\n\n{input_attrs}\nstruct RangeInput {{\n{input_fields}}}\n"
        )
    }

    const SAME_FIELDS: &str = "    start: Date,\n    end: Date,\n";

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
        assert!(findings(&newtype(attrs)).is_empty(), "a try_from-correct newtype has no findings");
    }

    #[test]
    fn a_hand_written_deserialize_is_also_a_route_through_the_constructor() {
        let text = format!(
            "{}impl<'de> serde::Deserialize<'de> for Digest {{}}\n",
            newtype("#[derive(Debug)]")
        );
        assert!(findings(&text).is_empty(), "a hand-written Deserialize is a clean route");
    }

    #[test]
    fn a_type_with_no_fallible_constructor_may_derive_deserialize() {
        // `Query`, `Anchor` and the `Raw*` settings shapes are this class: nothing is checked at
        // construction, so the derive walks past nothing.
        let text = "#[derive(serde::Deserialize)]\npub struct Query {\n    metric: MetricName,\n}\n";
        assert!(findings(text).is_empty(), "a type with no fallible constructor has no findings");
    }

    #[test]
    fn an_infallible_constructor_is_not_a_parse() {
        let text = "#[derive(serde::Deserialize)]\npub struct Anchor {\n    value: String,\n}\n\nimpl Anchor {\n    pub const fn new(value: String) -> Self {\n        Self { value }\n    }\n}\n";
        assert!(findings(text).is_empty(), "an infallible constructor is not a parse");
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
        assert!(findings(text).is_empty(), "an into beside the try_from leaves no findings");
    }

    #[test]
    fn a_hand_written_serialize_is_the_other_fix() {
        let text = "#[derive(serde::Serialize, serde::Deserialize)]\n#[serde(try_from = \"String\")]\npub struct Qualified {\n    part: String,\n}\n\nimpl serde::Serialize for Qualified {}\n";
        assert!(findings(text).is_empty(), "a hand-written Serialize leaves no findings");
    }

    #[test]
    fn a_newtype_over_the_try_from_target_round_trips_transparently() {
        // The nine `pub struct X(String)` types in `sutura-domain`: serde writes a newtype struct
        // as its inner value, so both directions are `String`.
        let text =
            "#[derive(serde::Serialize, serde::Deserialize)]\n#[serde(try_from = \"String\")]\npub struct Digest(String);\n";
        assert!(findings(text).is_empty(), "a newtype over the try_from target round-trips cleanly");
    }

    #[test]
    fn a_newtype_over_something_else_does_not() {
        let text = "#[derive(serde::Serialize, serde::Deserialize)]\n#[serde(try_from = \"String\")]\npub struct Days(u16);\n";
        assert_eq!(findings(text).len(), 1);
    }

    #[test]
    fn a_named_struct_whose_input_shape_matches_round_trips() {
        // `TimeRange` over `TimeRangeInput`: the wire form is the mapping both halves agree on.
        let text = over_input("#[derive(serde::Deserialize)]\n#[serde(deny_unknown_fields)]", SAME_FIELDS);
        assert!(findings(&text).is_empty(), "{:?}", findings(&text));
    }

    #[test]
    fn a_named_struct_whose_input_shape_differs_does_not() {
        let text = over_input(
            "#[derive(serde::Deserialize)]\n#[serde(deny_unknown_fields)]",
            "    from: Date,\n    to: Date,\n",
        );
        assert_eq!(findings(&text).len(), 1);
    }

    #[test]
    fn an_input_struct_that_accepts_an_undeclared_key_is_refused() {
        // The `TimeRangeInput` defect exactly: the outer type's own `deny_unknown_fields` cannot
        // reach the mapping, because `try_from` hands the whole mapping to the input struct. The
        // twin above differs by the one attribute line and is empty.
        let text = over_input("#[derive(serde::Deserialize)]", SAME_FIELDS);
        let found = findings(&text);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(
            found
                .first()
                .is_some_and(|p| p.contains("does not deny unknown fields") && p.contains("RangeInput")),
            "{found:?}"
        );
    }

    #[test]
    fn a_try_from_naming_no_declaration_in_the_file_is_not_this_rule() {
        // `try_from = "String"` and the nine newtypes over it: there is no input struct to check,
        // and a rule that reported them would be reporting the absence of a file it never read.
        let text =
            "#[derive(serde::Serialize, serde::Deserialize)]\n#[serde(try_from = \"String\")]\npub struct Digest(String);\n";
        assert!(findings(text).is_empty(), "a try_from naming no declaration is not this rule's");
    }

    #[test]
    fn a_newtype_input_struct_has_no_fields_to_deny() {
        let text = "#[derive(serde::Serialize, serde::Deserialize)]\n#[serde(try_from = \"Inner\", into = \"Inner\")]\npub struct Wrapped(Inner);\n\n#[derive(serde::Serialize, serde::Deserialize)]\nstruct Inner(String);\n";
        assert!(findings(text).is_empty(), "{:?}", findings(text));
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
        assert!(findings(text).is_empty(), "a rustdoc-example declaration is not a declaration");
    }

    #[test]
    fn a_declaration_inside_a_multi_line_string_is_not_a_declaration() {
        // Which is what makes this module's own fixtures invisible to the gate that reads them.
        let text = "fn fixture() -> &'static str {\n    r#\"\n#[derive(serde::Deserialize)]\npub struct Digest(String);\nimpl Digest { pub fn parse(r: &str) -> Result<Self, E> { todo!() } }\n\"#\n}\n";
        assert!(findings(text).is_empty(), "a string-literal declaration is not a declaration");
    }

    fn choice(attrs: &str, visibility: &str) -> String {
        format!(
            "{attrs}\n{visibility}enum Choice {{\n    Empty,\n    Text(String),\n    Named {{ value: String }},\n}}\n\nimpl Choice {{\n    pub fn parse(raw: &str) -> Result<Self, Bad> {{\n        todo!()\n    }}\n}}\n"
        )
    }

    #[test]
    fn fallible_enum_constructors_are_in_rule_one_for_every_declaration_visibility() {
        let attrs = "#[derive(\n    Debug,\n    serde::Deserialize,\n)]\n/// A choice.";
        let outcomes: Vec<_> = ["", "pub ", "pub(crate) ", "pub(super) ", "pub(in crate::outer) "]
            .into_iter()
            .map(|visibility| (visibility, findings(&choice(attrs, visibility))))
            .collect();
        let expected = "x.rs:6: `Choice` derives Deserialize but is constructed by `parse` - the derive writes past it";
        assert!(
            outcomes.iter().all(|(_, found)| found == &[expected]),
            "every enum visibility must retain its attribute run and rule-one location: {outcomes:?}"
        );
    }

    #[test]
    fn enum_routes_do_not_extend_the_struct_layout_rules() {
        let routed = choice(
            "#[derive(serde::Serialize, serde::Deserialize)]\n#[serde(try_from = \"ChoiceInput\")]",
            "pub ",
        );
        let cases = [
            (
                "unrouted enum",
                choice("#[derive(serde::Deserialize)]", "pub "),
                Some("`Choice` derives Deserialize but is constructed by `parse`"),
            ),
            (
                "routed enum with a named input lacking deny_unknown_fields",
                format!("{routed}\n#[derive(serde::Deserialize)]\nstruct ChoiceInput {{\n    value: String,\n}}\n"),
                None,
            ),
            (
                "handwritten enum deserializer",
                format!(
                    "{}\nimpl<'de> serde::Deserialize<'de> for Choice {{}}\n",
                    choice("#[derive(Debug)]", "pub ")
                ),
                None,
            ),
            (
                "enum without a fallible constructor",
                String::from("#[derive(serde::Deserialize)]\npub enum Choice {\n    Empty,\n    Text(String),\n}\n"),
                None,
            ),
            (
                "a struct does not match a named field inside an enum variant",
                String::from(
                    "#[derive(serde::Serialize, serde::Deserialize)]\n#[serde(try_from = \"ChoiceInput\")]\npub struct Choice {\n    value: String,\n}\n\n#[derive(serde::Deserialize)]\nenum ChoiceInput {\n    Named {\n        value: String,\n    },\n}\n",
                ),
                Some("the two shapes differ"),
            ),
        ];
        let outcomes: Vec<_> = cases
            .iter()
            .map(|(label, text, expected)| (*label, findings(text), *expected))
            .collect();
        assert!(
            outcomes.iter().all(|(_, found, expected)| {
                expected.as_ref().map_or(found.is_empty(), |message| {
                    found.len() == 1 && found.first().is_some_and(|p| p.contains(*message))
                })
            }),
            "only rule one gains enum sources; recognizing a route does not inspect its implementation: {outcomes:?}"
        );
    }

    #[test]
    fn the_gate_entry_point_refuses_an_unrouted_enum_and_accepts_its_try_from_route() {
        assert!(
            std::env::var_os("NEXTEST").is_some(),
            "this fixture changes cwd: run under just test for one process per test"
        );
        let tree = crate::falsifier::falsifier_tree();
        let file = tree.join("choice.rs");
        let original = std::env::current_dir().expect("a current directory");
        let cases = [
            ("#[derive(serde::Deserialize)]", crate::Verdict::Fail),
            (
                "#[derive(serde::Deserialize)]\n#[serde(try_from = \"String\")]",
                crate::Verdict::Pass,
            ),
        ];
        let outcomes: Vec<_> = cases
            .iter()
            .map(|(attrs, expected)| {
                std::fs::write(&file, choice(attrs, "pub ")).expect("the enum fixture");
                std::env::set_current_dir(&tree).expect("enter the fixture");
                let verdict = super::run(&[]);
                std::env::set_current_dir(&original).expect("restore before asserting the verdict");
                (verdict, *expected)
            })
            .collect();
        std::fs::remove_dir_all(tree).expect("remove the owned fixture");
        assert!(
            outcomes.iter().all(|(actual, expected)| actual == expected),
            "the entry point must propagate the enum refusal and recover when it is routed: {outcomes:?}"
        );
    }

    #[test]
    fn vendored_code_is_out_of_scope_and_rust_files_are_in_it() {
        assert!(in_scope("crates/sutura-domain/src/lib.rs"));
        assert!(!in_scope("vendor/mimalloc_rust/src/lib.rs"));
        assert!(!in_scope("AGENTS.md"));
    }
}
