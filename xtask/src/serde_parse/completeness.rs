//! Rule four: the type the definition digest covers that nothing asks to round-trip.
//!
//! Rules one to three next door are about the route a value takes in and out of a type. This one is
//! about whether anything ever ASKS, which no attribute can say - [`super`]'s header carries why
//! that is a different failure. What lives here is the recognition, the exemptions and the two
//! refusals that stop the rule passing over nothing.
//!
//! Its two sides are in different files: the types are spread over [`DIGEST_CRATE`] and the list is
//! [`ASKED_FILE`]. So this is a [`Population`] accumulated across the walk and judged once at the
//! end, where the other three rules judge one file at a time.

use super::FileFacts;
use super::scan;

/// Rule four's two sides, accumulated across the walk.
#[derive(Default)]
pub(super) struct Population {
    /// Every type whose serialized form the definition digest covers, with where it is declared.
    subjects: Vec<(String, String)>,
    /// The rows [`ASKED_FILE`] asks the property of.
    asked: std::collections::BTreeSet<String>,
    /// Every function declared in the crate, for the second arm: a citation naming no test is a
    /// dead pointer, which is the defect class this rule exists to stop being held by recall.
    functions: std::collections::BTreeSet<String>,
    /// Whether [`ASKED_FILE`] was in scope at all. Rule four is the only rule here with one
    /// required path, so its absence is refused by [`Population::verdict`] rather than passed over.
    found_list: bool,
}

/// The file that asks the round trip of every type the definition digest covers.
pub(super) const ASKED_FILE: &str = "crates/sutura-domain/src/serialized_form_tests.rs";

/// The crate whose serialized forms the digest is taken over, and so rule four's scope.
pub(super) const DIGEST_CRATE: &str = "crates/sutura-domain/";

/// The types [`ASKED_FILE`] cannot hold, each beside the test that asks the round trip instead.
///
/// Their canonical form is not a string - a mapping, an externally tagged enum - so the generated
/// string candidates that file is built on cannot reach them. The list lives HERE rather than in
/// that file for the reason every other exemption in this repository does: an exemption beside the
/// rule is one a reviewer reading the rule sees, and changing it is a visible diff. Each cited test
/// is checked to exist by [`Population::verdict`], so a rename cannot leave a dead pointer.
const ASKED_ELSEWHERE: [(&str, &str); 5] = [
    ("Term", "a_term_survives_the_on_disk_shape_it_serializes_into"),
    ("TimeRange", "a_range_round_trips_through_the_mapping_a_catalog_author_writes"),
    ("AuthoredSql", "authored_sql_round_trips_through_its_on_disk_shape"),
    ("Computation", "authored_sql_round_trips_through_its_on_disk_shape"),
    ("Referent", "a_referent_serializes_as_what_a_catalog_wrote"),
];

/// The macro whose expansions carry `try_from` and a derived `Serialize` of their own.
const EXPANSION_MACRO: &str = "identifier_newtype!";

impl Population {
    /// One file's contribution to both sides. Called for every file in [`DIGEST_CRATE`], which is
    /// where this rule's scope is decided: the digest is taken over that crate's serialized forms,
    /// so a type anywhere else is not its subject.
    pub(super) fn visit(&mut self, rel: &str, facts: &FileFacts) {
        self.subjects.extend(digest_subjects(rel, facts));
        self.functions.extend(function_names(&facts.code));
        if rel == ASKED_FILE {
            self.found_list = true;
            self.asked = asked_rows(&facts.code);
        }
    }

    /// How many types the digest covers, for the passing verdict. A rule that recognised nothing
    /// and a rule that found every subject accounted for otherwise print the same `ok`.
    pub(super) const fn found(&self) -> usize {
        self.subjects.len()
    }

    /// Rule four's verdict over the whole walk: every subject is asked, and every exemption cites a
    /// test that exists.
    ///
    /// The two ways this rule can check nothing are refused first, because both look like `ok`: the
    /// file that asks can go missing, and the recognition that finds the subjects can break. What is
    /// NOT refused is a tree with neither - `sutura-domain` is not this gate's subject, and the
    /// fixture trees [`super`]'s own entry-point test builds are exactly that tree.
    pub(super) fn verdict(&self) -> Vec<String> {
        if self.subjects.is_empty() {
            if !self.found_list {
                return Vec::new();
            }
            return vec![format!(
                "{ASKED_FILE}: rule four recognised no type under the definition digest in {DIGEST_CRATE}, so it \
                 judged nothing"
            )];
        }
        if !self.found_list {
            return vec![format!(
                "{ASKED_FILE}: absent while {DIGEST_CRATE} declares {} type(s) under the definition digest - the \
                 file that asks them to round-trip has moved",
                self.subjects.len()
            )];
        }
        let exempt: std::collections::BTreeSet<&str> = ASKED_ELSEWHERE.iter().map(|(kind, _)| *kind).collect();
        let mut problems: Vec<String> = self
            .subjects
            .iter()
            .filter(|(name, _)| !self.asked.contains(name) && !exempt.contains(name.as_str()))
            .map(|(name, at)| {
                format!(
                    "{at}: `{name}`'s serialized form is under the definition digest, and {ASKED_FILE} does not ask \
                     it to round-trip"
                )
            })
            .collect();
        problems.extend(
            ASKED_ELSEWHERE
                .iter()
                .filter(|(_, asked)| !self.functions.contains(*asked))
                .map(|(kind, asked)| {
                    format!(
                        "xtask/src/serde_parse/completeness.rs: `{kind}` is exempted from rule four by a citation of \
                         `{asked}`, which is not a test in {DIGEST_CRATE}"
                    )
                }),
        );
        problems
    }
}

/// The types in one file whose serialized form the definition digest covers.
///
/// The subject is the class rules one and two already recognise, read from the other side. A type
/// whose `Deserialize` is routed through its constructor and whose `Serialize` the digest is taken
/// over has two directions that must agree, and rule two only holds that the SHAPES match - not
/// that anything ever asks a value to survive the trip. Three routes make a type one of these:
///
/// * `serde(try_from = "..")` beside a `Serialize`, derived or hand-written. The attribute is
///   itself the evidence that the way in is a fallible constructor;
/// * an [`EXPANSION_MACRO`] invocation, because the macro body carries both. No declaration
///   scanner can see one: the name in the body is `$name`, which is not an identifier;
/// * both impls hand-written, plus a fallible constructor of its own - the `QualifiedTable` shape.
///   The constructor is what separates it from a type that hand-writes serde to control a wire
///   form and parses nothing, which has no constructor for a round trip to go through.
fn digest_subjects(rel: &str, facts: &FileFacts) -> Vec<(String, String)> {
    let mut found: Vec<(String, String)> = Vec::new();
    for declaration in &facts.declared {
        let writes = scan::derives(&declaration.attrs, "Serialize") || facts.writes_by_hand.contains(&declaration.name);
        let reads_through_parse = scan::serde_arg(&declaration.attrs, "try_from").is_some()
            || (facts.reads_by_hand.contains(&declaration.name) && facts.parses.contains_key(&declaration.name));
        if writes && reads_through_parse {
            found.push((declaration.name.clone(), format!("{rel}:{}", declaration.line)));
        }
    }
    found.extend(expansions(rel, &facts.code));
    found
}

/// The [`EXPANSION_MACRO`] invocations in one file, as `(type name, where)`.
///
/// The invocation is the macro's name, a doc comment, and the type name alone on a line. Comments
/// are blanked before this runs, so the only non-empty line inside the braces is that name.
fn expansions(rel: &str, code: &[String]) -> Vec<(String, String)> {
    let mut found = Vec::new();
    let mut opened_at: Option<usize> = None;
    for (index, line) in code.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with(EXPANSION_MACRO) {
            opened_at = Some(index.saturating_add(1));
            continue;
        }
        let Some(at) = opened_at else {
            continue;
        };
        if trimmed.starts_with('}') {
            opened_at = None;
            continue;
        }
        if !trimmed.is_empty() && trimmed.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            found.push((String::from(trimmed), format!("{rel}:{at}")));
        }
    }
    found
}

/// The types [`ASKED_FILE`] asks the property of: the first quoted string on each `survives(` call.
///
/// Read from the CALL rather than from every string in the file. A rule that accepted any
/// occurrence of a type's name could be satisfied by a message or a comment mentioning it, and a
/// rule prose can satisfy is not a rule.
fn asked_rows(code: &[String]) -> std::collections::BTreeSet<String> {
    code.iter()
        .filter(|line| line.contains("survives("))
        .filter_map(|line| quoted_first(line))
        .collect()
}

/// The first double-quoted string on one line. No escape handling, which is why the callers read
/// one construct each: neither a `survives(` row nor a `fn` line contains an escaped quote.
fn quoted_first(line: &str) -> Option<String> {
    line.split('"').nth(1).map(String::from)
}

/// Every function name declared in one file.
fn function_names(code: &[String]) -> Vec<String> {
    code.iter()
        .filter_map(|line| line.split_once("fn "))
        .map(|(_, rest)| {
            let end = rest
                .find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                .unwrap_or(rest.len());
            String::from(rest.get(..end).unwrap_or_default())
        })
        .filter(|name| !name.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::super::facts_of;
    use super::{ASKED_ELSEWHERE, Population, digest_subjects, scan};

    /// One file's subjects, as rule four collects them.
    fn subjects(rel: &str, text: &str) -> Vec<String> {
        digest_subjects(rel, &facts_of(text))
            .into_iter()
            .map(|(name, _)| name)
            .collect()
    }

    /// Rule four's verdict over one file of subjects and one list of rows.
    fn completeness(text: &str, rows: &str) -> Vec<String> {
        let population = Population {
            subjects: digest_subjects("crates/sutura-domain/src/x.rs", &facts_of(text)),
            asked: super::asked_rows(&scan::code_lines(rows)),
            functions: ASKED_ELSEWHERE.iter().map(|(_, asked)| String::from(*asked)).collect(),
            found_list: true,
        };
        population.verdict()
    }

    const PARSED: &str =
        "#[derive(serde::Serialize, serde::Deserialize)]\n#[serde(try_from = \"String\")]\npub struct Digest(String);\n";

    #[test]
    fn a_type_the_digest_covers_and_the_list_does_not_ask_is_refused() {
        let found = completeness(PARSED, "");
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(
            found
                .first()
                .is_some_and(|p| p.contains("`Digest`") && p.contains("does not ask it to round-trip")),
            "{found:?}"
        );
    }

    #[test]
    fn a_row_is_what_accounts_for_it() {
        assert!(
            completeness(
                PARSED,
                "    checked += survives(\"Digest\", |raw| Digest::parse(raw).ok(), &all);\n"
            )
            .is_empty(),
            "a row in the list is what the rule reads"
        );
    }

    #[test]
    fn a_type_named_only_in_a_message_is_not_asked() {
        // The rule reads the `survives(` CALL, not every occurrence of the name: a rule prose can
        // satisfy is not a rule.
        let found = completeness(PARSED, "    panic!(\"Digest is covered somewhere, honestly\");\n");
        assert_eq!(found.len(), 1, "{found:?}");
    }

    #[test]
    fn a_serialized_form_with_no_route_back_through_parse_is_not_a_subject() {
        // `DefinitionCapabilities` is this class: both impls hand-written, no fallible constructor,
        // and a round trip it deliberately does not hold. A rule that demanded a row for it would
        // demand a row for a property the type denies having.
        let text = "#[derive(Debug)]\npub struct Capabilities {\n    kinds: Vec<Kind>,\n}\n\nimpl serde::Serialize for Capabilities {}\n\nimpl<'de> serde::Deserialize<'de> for Capabilities {}\n";
        assert!(
            subjects("crates/sutura-domain/src/capabilities.rs", text).is_empty(),
            "{text}"
        );
    }

    #[test]
    fn both_impls_hand_written_over_a_constructor_is_a_subject() {
        // And `QualifiedTable` is the other side of that line: it parses, so the round trip is a
        // property it has.
        let text = "pub struct Table {\n    name: String,\n}\n\nimpl Table {\n    pub fn parse(raw: &str) -> Result<Self, Bad> {\n        todo!()\n    }\n}\n\nimpl serde::Serialize for Table {}\n\nimpl<'de> serde::Deserialize<'de> for Table {}\n";
        assert_eq!(
            subjects("crates/sutura-domain/src/model/qualified.rs", text),
            vec![String::from("Table")]
        );
    }

    #[test]
    fn a_macro_expansion_is_a_subject_no_declaration_scanner_can_see() {
        let text = "identifier_newtype! {\n    /// A name.\n    ModelName\n}\n";
        let facts = facts_of(text);
        assert!(
            facts.declared.is_empty(),
            "the body's name is `$name`, which is not an identifier: {:?}",
            facts.declared
        );
        assert_eq!(
            subjects("crates/sutura-domain/src/model.rs", text),
            vec![String::from("ModelName")],
            "so the invocation is what rule four reads"
        );
    }

    #[test]
    fn the_two_ways_rule_four_can_judge_nothing_are_refused_and_an_empty_tree_is_not() {
        assert!(
            Population::default().verdict().is_empty(),
            "a tree with no sutura-domain and no list is not this gate's subject"
        );
        let list_gone = Population {
            subjects: vec![(String::from("Digest"), String::from("x.rs:1"))],
            ..Population::default()
        };
        assert!(
            list_gone.verdict().first().is_some_and(|p| p.contains("has moved")),
            "subjects with no list is the file being renamed out from under the rule"
        );
        let recognition_gone = Population {
            found_list: true,
            ..Population::default()
        };
        assert!(
            recognition_gone
                .verdict()
                .first()
                .is_some_and(|p| p.contains("judged nothing")),
            "a list with no subjects is the recognition breaking"
        );
    }

    #[test]
    fn an_exemption_citing_no_test_is_refused() {
        let population = Population {
            subjects: vec![(String::from("Digest"), String::from("x.rs:1"))],
            asked: std::iter::once(String::from("Digest")).collect(),
            functions: std::collections::BTreeSet::new(),
            found_list: true,
        };
        let found = population.verdict();
        assert_eq!(found.len(), ASKED_ELSEWHERE.len(), "{found:?}");
        assert!(
            found.iter().all(|p| p.contains("is not a test in")),
            "an exemption whose test no longer exists is a dead pointer: {found:?}"
        );
    }
}
