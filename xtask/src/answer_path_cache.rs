//! No map or cache, in the answer path's own interior, holds a type read off that path.
//!
//! **The invariant is true today and was held by nobody.** `docs/adr/0031`'s own *What this
//! explicitly leaves for later* names this gate as owed, and `github.com/telekom/sutura#381`'s
//! last comment amended its acceptance to exactly this shape: *"a gate holds that the answer path
//! caches nothing, starting green over today's tree"* - the same shape `check-newtype-leaks`
//! already uses for its own SEALED-witness scan, so this module copies it rather than inventing a
//! second one.
//!
//! # Why this is worth a gate rather than a sentence
//!
//! The credential cache `docs/adr/0031` landed lives entirely inside
//! `sutura_exec_bigquery::WorkloadIdentityBroker`'s private `sts::cache` module and never touches
//! `sutura-app` or `sutura-domain` - which is exactly *held by recall* rather than by a mechanism.
//! A `HashMap` keyed by subject and holding a `sutura_domain::warehouse::RowSet` would
//! answer a second identical question without asking the source again, silently reusing an
//! answer across two subjects the moment their questions coincide - the opposite of what
//! `docs/where-identity-is-proven.md` argues a green run must mean.
//!
//! # Scope, narrower than the acceptance text and said so
//!
//! The acceptance that deferred this asked for a scan over *"a field reachable from the answer
//! path"*. **"Reachable" is not a property a text scan decides** - `sutura-app` calls into every
//! adapter crate, and walking that call graph to decide what is truly reachable is a second gate's
//! worth of work this one does not attempt. What is built instead, and all that is claimed: **no
//! named-field `struct` in `crates/sutura-app/src` or `crates/sutura-domain/src` declares a
//! `HashMap` or `BTreeMap` field whose value type names one of [`ANSWER_PATH_TYPES`]** - the
//! hexagon's own interior, never an adapter. A map living inside an adapter crate (`BigQuery`'s
//! own `sts::cache`, or a future one) is out of scope by construction, not by an exclusion this gate
//! carries and could be argued open - the two directories named above are the whole of its walk.
//!
//! **Three further limits, stated where the claim is:**
//!
//! * A **type cannot forbid IO**, and Rust has no effect system - nothing here, or anywhere, stops
//!   a method from opening a file and holding what it reads for as long as it likes without ever
//!   naming a `HashMap`. This gate reads a SHAPE, not a guarantee; the shape is what every cache
//!   this codebase has written so far looks like from the outside.
//! * A field declared across more than one physical line - a generic argument list wrapped for
//!   width - is not read. Measured: nothing in scope wraps today.
//! * Only a **named-field** `struct` is walked; a tuple struct's single field is out of scope.
//!   Theoretical rather than active here: every struct in scope names its fields.
//!
//! # The declared list, held in both directions
//!
//! [`ANSWER_PATH_TYPES`] names what a cache would have to hold to be this gate's business - the
//! result of running a plan (`RowSet`, `AnchorRows`), the plan itself compiled
//! (`QueryPlan`, `AnchorPlan`, `LegPlan`, `FederatedPlan`), what a warehouse is asked to run
//! (`Executable`), and the answer a caller receives (`Answered`, `AnsweredRaw`). Adding or
//! removing an entry is an architecture decision, exactly as `newtype_leaks::LEAKY` and `::SEALED`
//! are - and, as with those two, an entry naming a type that moved or was renamed would guard
//! nothing silently, so [`run`] checks the reverse direction: every declared `declared_in` file
//! must actually declare the name beside it, or the gate fails rather than passing over a name
//! nothing in the tree answers to.

use crate::Verdict;
use crate::repo;
use crate::serde_parse::scan::{code_lines, matching_angle};

/// A type read off the answer path: the result of executing a plan, the compiled plan itself, or
/// what a caller receives. A `HashMap`/`BTreeMap` field naming one of these as its VALUE type is
/// what this gate refuses.
struct AnswerPathType {
    /// The type's own name, as it would appear as a generic argument.
    name: &'static str,
    /// The one file this gate trusts to declare it - read back in [`run`] so the list cannot rot
    /// into guarding a name nothing declares.
    declared_in: &'static str,
}

/// **Adding or removing an entry here is an architecture decision.** Each name is a real type in
/// this workspace, checked against the file beside it every run.
const ANSWER_PATH_TYPES: &[AnswerPathType] = &[
    AnswerPathType {
        name: "RowSet",
        declared_in: "crates/sutura-domain/src/warehouse/rows.rs",
    },
    AnswerPathType {
        name: "AnchorRows",
        declared_in: "crates/sutura-domain/src/warehouse/rows.rs",
    },
    AnswerPathType {
        name: "Executable",
        declared_in: "crates/sutura-domain/src/plan/leg.rs",
    },
    AnswerPathType {
        name: "LegPlan",
        declared_in: "crates/sutura-domain/src/plan/leg.rs",
    },
    AnswerPathType {
        name: "FederatedPlan",
        declared_in: "crates/sutura-domain/src/plan/federated.rs",
    },
    AnswerPathType {
        name: "QueryPlan",
        declared_in: "crates/sutura-domain/src/plan.rs",
    },
    AnswerPathType {
        name: "AnchorPlan",
        declared_in: "crates/sutura-domain/src/plan.rs",
    },
    AnswerPathType {
        name: "Answered",
        declared_in: "crates/sutura-app/src/lib.rs",
    },
    AnswerPathType {
        name: "AnsweredRaw",
        declared_in: "crates/sutura-app/src/raw.rs",
    },
];

/// The map-shaped generics this gate treats as a cache. Each already carries the `<` it opens, so
/// a match's end is exactly where the generic argument list begins.
const MAP_TYPES: &[&str] = &["HashMap<", "BTreeMap<"];

/// The two directories this gate walks - the hexagon's own interior, never an adapter. Trailing
/// `/` so a sibling crate whose name merely starts with the same prefix cannot match.
const SCOPE: &[&str] = &["crates/sutura-app/src/", "crates/sutura-domain/src/"];

/// Is `rel` inside [`SCOPE`]?
fn in_scope(rel: &str) -> bool {
    std::path::Path::new(rel)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("rs"))
        && SCOPE.iter().any(|prefix| rel.starts_with(prefix))
}

/// A field this gate refuses.
struct Violation {
    path: String,
    line: usize,
    /// The field's own text, trimmed - printed so a reader does not have to open the file to see
    /// what tripped the rule.
    field: String,
    /// `HashMap` or `BTreeMap`, without its own `<`.
    map: &'static str,
    /// The [`ANSWER_PATH_TYPES`] name the field's value type carries.
    holds: &'static str,
}

/// What one walk over the tree found.
struct Scanned {
    violations: Vec<Violation>,
    /// The [`ANSWER_PATH_TYPES`] names actually found declared where the table says they are.
    declared: Vec<&'static str>,
    /// In-scope files read.
    read: usize,
    witness: String,
}

/// The files [`ANSWER_PATH_TYPES`] declares - this gate cannot have a verdict without reading
/// each of them, so an entry whose file moved refuses here rather than silently declaring nothing.
fn anchors() -> Vec<&'static str> {
    let mut paths: Vec<&'static str> = ANSWER_PATH_TYPES.iter().map(|entry| entry.declared_in).collect();
    paths.sort_unstable();
    paths.dedup();
    paths
}

/// The scan, over a census it does not mint - the same split `check-newtype-leaks` uses so a test
/// can hand it a scratch tree instead of asserting on this file's own source text.
fn scan(census: repo::Census, must_judge: &[&str]) -> Result<Scanned, repo::Refusal> {
    let mut violations = Vec::new();
    let mut declared: Vec<&'static str> = Vec::new();
    let mut read = 0_usize;

    let scope: repo::Scope = in_scope;
    let inspected = census.inspect(must_judge, scope, |rel, bytes| {
        let text = String::from_utf8_lossy(bytes);
        read = read.saturating_add(1);
        let code = code_lines(&text);
        for entry in ANSWER_PATH_TYPES {
            if entry.declared_in == rel && declares(&code, entry.name) {
                declared.push(entry.name);
            }
        }
        for (line, field) in struct_field_lines(&code) {
            let Some((_, declared_type)) = field.split_once(':') else {
                continue;
            };
            if let Some((map, holds)) = cached_answer_path_type(declared_type) {
                violations.push(Violation {
                    path: String::from(rel),
                    line,
                    field: field.trim_end_matches(',').trim().to_owned(),
                    map,
                    holds,
                });
            }
        }
    })?;

    Ok(Scanned {
        violations,
        declared,
        read,
        witness: inspected.verdict(),
    })
}

pub(crate) fn run(_args: &[String]) -> Verdict {
    // The same empty-list refusal `check-newtype-leaks` carries: a declared list emptied to `&[]`
    // would still read every file and print `ok`, guarding nothing while looking like a pass.
    if ANSWER_PATH_TYPES.is_empty() || MAP_TYPES.is_empty() {
        eprintln!(
            "xtask check-answer-path-caches: FAILED - a declared list is empty (ANSWER_PATH_TYPES, \
             MAP_TYPES); this gate would guard nothing"
        );
        return Verdict::Fail;
    }

    let census = match repo::all_files() {
        Ok(census) => census,
        Err(why) => {
            eprintln!("xtask check-answer-path-caches: FAILED - {}", why.describe());
            return Verdict::Fail;
        }
    };
    let found = match scan(census, &anchors()) {
        Ok(found) => found,
        Err(why) => {
            eprintln!("xtask check-answer-path-caches: FAILED - {}", why.describe());
            return Verdict::Fail;
        }
    };
    decide(&found)
}

/// What to say about a scan. Separated from [`run`] so both verdicts are reachable from a test.
fn decide(found: &Scanned) -> Verdict {
    let missing = undeclared(&found.declared);
    if found.violations.is_empty() && missing.is_empty() {
        println!(
            "xtask check-answer-path-caches: ok - {} file(s) in the answer path's interior, no map \
             or cache holds one of {} answer-path type(s); {}",
            found.read,
            ANSWER_PATH_TYPES.len(),
            found.witness
        );
        return Verdict::Pass;
    }

    for violation in &found.violations {
        eprintln!(
            "xtask check-answer-path-caches: FAILED - {}:{}: `{}` is a {} holding `{}`, a type read \
             off the answer path",
            violation.path, violation.line, violation.field, violation.map, violation.holds
        );
    }
    if !found.violations.is_empty() {
        eprintln!();
        eprintln!(
            "The answer path's interior (`sutura-app`, `sutura-domain`) may not remember an \
             earlier answer keyed for reuse: a `HashMap`/`BTreeMap` holding a RowSet, a compiled \
             plan or an Executable would answer a later question without asking the source again -"
        );
        eprintln!(
            "silently reusing an answer across two subjects the moment their questions coincide. If \
             a data system's own driver needs to memoise something, that belongs inside the adapter \
             crate for that source, never here."
        );
    }
    for entry in &missing {
        eprintln!(
            "xtask check-answer-path-caches: FAILED - `{}` is declared an answer-path type but `{}` \
             does not declare it, so this gate is guarding a name rather than a type",
            entry.name, entry.declared_in
        );
    }
    Verdict::Fail
}

/// The [`ANSWER_PATH_TYPES`] entries the scan did not find declared where the table says they are.
///
/// The reverse direction, so the list cannot rot into naming a type that moved. Its own function
/// so the refusal is held by a test rather than by the whole gate.
fn undeclared(declared: &[&'static str]) -> Vec<&'static AnswerPathType> {
    ANSWER_PATH_TYPES
        .iter()
        .filter(|entry| !declared.contains(&entry.name))
        .collect()
}

/// Does `code` declare a type called `name`? The same shape `newtype_leaks::declares` uses for its
/// own SEALED table, over a different list.
fn declares(code: &[String], name: &str) -> bool {
    code.iter().any(|line| {
        ["struct ", "enum "].iter().any(|keyword| {
            line.split_once(keyword).is_some_and(|(_, rest)| {
                rest.strip_prefix(name)
                    .is_some_and(|tail| !tail.starts_with(|c: char| c.is_alphanumeric() || c == '_'))
            })
        })
    })
}

/// Where the line-by-line walk of a file currently is, for [`struct_field_lines`].
enum Scan {
    /// Not inside a named-field struct's body.
    Outside,
    /// Saw the declaration, but a generic or `where` clause pushed the `{` to a later line.
    AwaitingBody,
    /// Inside a struct's body, at this brace depth.
    InBody { depth: usize },
}

/// Every field-list line inside a named-field `struct`'s body - depth 1 relative to the struct's
/// own braces - paired with its 1-based line number.
///
/// Every `struct`, not only a `pub` one: an answer-path cache leaks the invariant this gate is
/// about whether or not the field is visible outside its own crate. A tuple struct's single field,
/// and a field wrapped across more than one physical line, are out of this scan's reach - stated
/// in this module's own header, and theoretical rather than active on the tree this gate reads.
fn struct_field_lines(code: &[String]) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut scan = Scan::Outside;
    for (index, line) in code.iter().enumerate() {
        let number = index.saturating_add(1);
        let trimmed = line.trim();
        let opens = trimmed.matches('{').count();
        let closes = trimmed.matches('}').count();
        scan = match scan {
            Scan::Outside => {
                if !trimmed.contains("struct ") {
                    continue;
                }
                // A tuple struct (`struct X(..);`) or one declared and closed on one line
                // (`struct X {}`) opens nothing this walk needs to enter.
                if opens > closes {
                    Scan::InBody {
                        depth: opens.saturating_sub(closes),
                    }
                } else if opens == 0 && !trimmed.contains('(') && !trimmed.ends_with(';') {
                    Scan::AwaitingBody
                } else {
                    Scan::Outside
                }
            }
            Scan::AwaitingBody => {
                if opens > closes {
                    Scan::InBody {
                        depth: opens.saturating_sub(closes),
                    }
                } else {
                    Scan::AwaitingBody
                }
            }
            Scan::InBody { depth } => {
                if depth == 1 && trimmed.contains(':') {
                    out.push((number, trimmed.to_owned()));
                }
                let next = depth.saturating_add(opens).saturating_sub(closes);
                if next == 0 {
                    Scan::Outside
                } else {
                    Scan::InBody { depth: next }
                }
            }
        };
    }
    out
}

/// The text a map's own generic argument list carries - between the two angle brackets `map`
/// opens - or `None` when they do not close on this line, which is this gate's stated limit for a
/// field wrapped across more than one physical line.
fn map_arguments<'text>(field_type: &'text str, map: &str) -> Option<&'text str> {
    let at = field_type.find(map)?;
    // `map` already ends in its own `<`; back up one character so the slice this hands to
    // `matching_angle` STARTS at that bracket, which is what it requires of its argument.
    let bracket = field_type.get(at.saturating_add(map.len().saturating_sub(1))..)?;
    let close = matching_angle(bracket)?;
    bracket.get(1..close)
}

/// The last comma-separated argument at nesting depth zero - a map's VALUE type, since its key
/// precedes it. The same shape `boundaries::api_shape::last_top_level` uses for a `Result`'s error
/// type, over a different question; `-` and `=` guard against `->` and `=>` for the same reason.
fn last_top_level(args: &str) -> Option<&str> {
    let mut depth: usize = 0;
    let mut start: Option<usize> = None;
    let mut previous = ' ';
    for (at, character) in args.char_indices() {
        match character {
            '<' | '(' | '[' => depth = depth.saturating_add(1),
            '>' if previous != '-' && previous != '=' => depth = depth.saturating_sub(1),
            ')' | ']' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => start = Some(at.saturating_add(1)),
            _ => {}
        }
        previous = character;
    }
    args.get(start?..).map(str::trim)
}

/// Does `haystack` carry `needle` as a whole identifier - not as part of a longer one, so
/// `MalformedRowSet` is not read as holding `RowSet`? The same identifier-boundary test
/// `orphan_modules::reached_as_segment` uses, over a `match_indices` walk rather than a manual one.
fn contains_word(haystack: &str, needle: &str) -> bool {
    let is_ident = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    let bytes = haystack.as_bytes();
    haystack.match_indices(needle).any(|(at, matched)| {
        let before_ok = bytes.get(at.wrapping_sub(1)).is_none_or(|b| !is_ident(*b));
        let after_ok = bytes.get(at.saturating_add(matched.len())).is_none_or(|b| !is_ident(*b));
        before_ok && after_ok
    })
}

/// If `field_type` is a [`MAP_TYPES`] generic whose value argument names an
/// [`ANSWER_PATH_TYPES`] entry, the map's own name (without its `<`) and the entry's name.
fn cached_answer_path_type(field_type: &str) -> Option<(&'static str, &'static str)> {
    MAP_TYPES.iter().find_map(|map| {
        let args = map_arguments(field_type, map)?;
        let value = last_top_level(args)?;
        ANSWER_PATH_TYPES
            .iter()
            .find(|entry| contains_word(value, entry.name))
            .map(|entry| (map.trim_end_matches('<'), entry.name))
    })
}

#[cfg(test)]
mod tests;
