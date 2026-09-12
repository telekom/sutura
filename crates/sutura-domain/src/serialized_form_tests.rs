//! The property every parsed newtype under the definition digest is held to: **a parsed value
//! serializes the way it came, and re-parsing what it serialized to changes nothing.**
//!
//! `DefinitionDigest::of` in [`crate::definitions`] takes the digest over `serde_json` output, so two
//! ways for a newtype to move a digest without its content moving are live at once:
//!
//! 1. **Normalisation that is not idempotent.** [`crate::knowledge::Phrase::parse`] folds runs of
//!    whitespace and drops the invisible code points [`crate::text`] names; every other parser here
//!    trims. A parser whose second pass differs from its first stores one value and re-reads
//!    another, so a bundle that was written and read back hashes differently.
//! 2. **A `Serialize` that does not agree with the `Deserialize` beside it.** Not hypothetical:
//!    [`crate::calendar::Date`] shipped `serde(try_from = "String")` with a *derived* `Serialize`
//!    that wrote the struct, so the digest covered a field layout that appears in no catalog file.
//!    [`crate::model::QualifiedTable`]'s hand-written `Serialize` carries the same note, and
//!    `identity::principal`'s module header cites that bug as its reason for deriving
//!    neither impl.
//!
//! Each of those was argued in prose, per type, at the type. This is the one place that asks all of
//! them, over generated input rather than over the example its author thought of.
//!
//! # Generated rather than tabulated, and why `proptest` is not here
//!
//! `proptest` was measured rather than rejected on taste. `cargo xtask check-boundaries` holds
//! `sutura-domain` to an allowlist over its whole transitive tree - dev-dependencies included, which
//! is that gate's deliberate reading of "what the workspace build can reach" - and adding `proptest`
//! as a dev-dependency here failed it by name with **48 crates** to argue onto the list, `futures-util`,
//! `wasm-bindgen`, `js-sys`, `rustix`, `tempfile` and `rusty-fork` among them. That list's own note
//! says adding a name to it is an architecture decision; an async and a WASM stack inside the hexagon
//! is the decision it exists to refuse, and a test helper is not the argument that should win it.
//!
//! What a random generator would have bought is covered another way. The single-character dimension
//! is better served **exhaustively** - [`crate::text`]'s own tests walk all 0x110000 scalars through
//! a real parser - and what is left is the *combination* of a whitespace run, an invisible code
//! point and a multi-byte character meeting a length bound. So [`sweep`] is exhaustive over a
//! nine-character alphabet, [`perturbed`] inserts each of those characters into a realistic
//! authored value at every position, and [`straddling`] repeats a short pattern to land on both
//! sides of the ten bounds [`REPEATS`] names. No seed, because nothing is random; `just test` runs
//! it like any other test.
//!
//! The sweep is ordered shortest-first and searched in order, which buys the one thing a random
//! generator is actually reached for: the counterexample a failure reports is the shortest one
//! AMONG THE CANDIDATES THIS GENERATOR PRODUCES, with no shrinking step to write or to trust.
//! Not a shortest one over the alphabet, which is what this said while the sweep had a fourth
//! level: [`SWEEP_LEN`] is three, so a defect whose minimal witness is longer than that is
//! reported by whichever other family catches it, at that family's length rather than at the
//! alphabet's shortest. Shortest-FIRST still holds; shortest over the alphabet does not.
//!
//! # Which generator earns its lines, measured
//!
//! A hand table of a few dozen adversarial values is a real alternative: both kills below report a
//! counterexample short enough to write down. So the four families were measured the way the kills
//! were - eight mutations of the parsers these types call, each run against the two tests here with
//! the accepted-value count read per family. `=` is a count that did not move:
//!
//! | mutation | [`sweep`] 820 | [`WITNESSES`] 10 | [`perturbed`] 3 672 | [`straddling`] 174 | caught by |
//! | --- | --- | --- | --- | --- | --- |
//! | the leading-space flag forced on | = | = | = | = | idempotence, at 3 characters |
//! | the same flag inverted | = | = | = | +6 | idempotence, and the count |
//! | the pending separator never reset | = | = | -14 | = | the accepted count |
//! | an invisible code point read as a separator | = | = | = | -6 | the accepted count |
//! | an invisible code point kept, not dropped | = | = | = | -6 | the accepted count |
//! | a dimension value's bound off by one | = | -2 | = | -6 | the accepted count |
//! | a dimension value's length counted in bytes | = | = | = | -12 | the accepted count |
//! | an invisible code point CLEARING the separator | = | = | = | = | **nothing** |
//!
//! **[`straddling`] is what earns a generator over a table**: four of those rows move its count and
//! nothing else's, and it shares a fifth with [`WITNESSES`] - 174 values written as eight lines,
//! six patterns against both sides of ten bounds. Tabulated by hand it *is* 174 rows, so a table is
//! the expensive form of it rather than the cheap one. [`perturbed`] caught one nothing else did.
//!
//! **[`sweep`] moved no count in any of the eight**, which is why [`SWEEP_LEN`] is three: the one
//! thing that family catches reports a THREE-character counterexample, and a fourth level is 6 561
//! more candidates - 58% of the generated space - for no measured catch. All eight were re-run at
//! three and every verdict reproduced, counterexample for counterexample.
//!
//! **The last row is the limit, stated next to the claim.** With `collapse_spacing` mutated so an
//! invisible code point clears the pending separator, `a \u{200b}b` stores as `ab` - a word boundary
//! lost from a phrase the digest covers and `phrase_identity` matches on - and it passes this
//! crate's whole suite, not just these two tests. A normalisation that folds MORE is still
//! idempotent and refuses nothing new, so neither half of this property can see it. What would is a
//! golden of the stored form, which `crates/sutura-app/tests/golden/catalogs.rs` holds for a catalog
//! and nothing holds for a phrase.
//!
//! # Measured, not asserted: what breaking each half of this actually reports
//!
//! Each half was checked by breaking the invariant and reading the counterexample, because a
//! property that has never failed is a property nobody has checked. **Each kill was then run
//! against the WHOLE WORKSPACE**, because "nothing else noticed" is a claim about the repository
//! and a crate-scoped suite cannot make it. Of the three, exactly one is unique to this file:
//!
//! * **Normalisation made non-idempotent** - `collapse_spacing`'s leading-space flag forced on -
//!   reports `Phrase: "\u{200b} a" parsed, stored " a", and re-parsing that is not the same value`.
//!   **This test was the only failure in the workspace**, so nothing else held that property at
//!   all. It is the one kill here that nothing else covers.
//! * **A routing attribute dropped** - `serde(try_from = "String")` removed from `Description`,
//!   `DimensionValue` and `AnchorValue` at once - reports `Description: "\u{200b}" is refused by
//!   parse and accepted by Deserialize`. **Eight pre-existing tests in four other crates fail with
//!   it too**: `sutura-catalog-local`, `sutura-catalog-datahub`, `sutura-app`'s golden suite and
//!   `sutura-cli`'s example and documented-command suites all load a catalog through these types.
//!   Inside `sutura-domain` it looks unique - `crate::model`, `crate::knowledge`,
//!   `crate::definitions` and `crate::expression` each have a
//!   `deserialization_goes_through_the_constructor` test and `catalog::authored` had none - which
//!   is exactly how a crate-scoped count says "nothing else covers this" about a route five suites
//!   traverse. **The scope of the run is part of the measurement.**
//! * **`Date`'s `into = "String"` removed** - the asymmetry that shipped - is caught here AND by
//!   two hand-written tests in [`crate::calendar`]. Recorded because it is the honest half: where a
//!   table already asks the question, this adds nothing but a second voice.
//!
//! # What this does NOT cover, stated next to the claim
//!
//! **The list below is one somebody maintains, and what checks it is complete is rule four of
//! `cargo xtask check-serde-parse`** - run by `just hygiene`, and by `just validate` with
//! everything else. It reads this crate's sources for every type whose `Deserialize` is routed
//! through its constructor and whose `Serialize` the digest is taken over, and fails when one is
//! neither a row here nor exempted beside the test that asks it instead. It needed no task of its
//! own: it is an arm on a walk that already visits every tracked file and already recognises those
//! attributes, which is the normal shape for a rule of this kind and the reason it cost a
//! predicate rather than a second scan of the tree.
//!
//! **The rows are 21 of the 26 types that rule finds.** The other five have no string canonical
//! form - a mapping, an externally tagged enum - so [`survives`] cannot hold them, and each has a
//! both-direction round trip of its own: `Term` in the second test below, `TimeRange`,
//! `AuthoredSql`, `Computation` and `Referent` at their own modules, cited from the gate's
//! exemption list. So the gap between 21 and 26 was an understated limit rather than a coverage
//! hole - but it was stated nowhere, and "21 types" reads as "all of them" for as long as nothing
//! counts.
//!
//! Two types in this crate carry a `Serialize` and no `Deserialize` at all -
//! `crate::source::AcknowledgementReason` and `crate::source::VerificationIdentity`, both operator
//! text rather than catalog content. Nothing deserializes them, so they are outside the rule, and
//! the day one gains a `Deserialize` the same gate is what notices: rule one refuses the derive
//! over their `parse`, the `serde(try_from)` it names as the fix is what makes them rule four's
//! subjects, and rule four then wants a row. That chain is the mechanism; this paragraph is only a
//! signpost to it.
//!
//! It also says nothing about the bundle one level up. There is no bundle-level round trip to ask
//! for: [`crate::catalog::Definitions`] derives `Serialize` and **not** `Deserialize`, which is what
//! `canonical_form`'s *"nothing ever parses these bytes back"* means. The claims that do hold there -
//! a reformat does not move the digest, a changed meaning does, each half is covered - are asserted
//! over hand-written catalogs in `crates/sutura-app/tests/golden/catalogs.rs` and in
//! `pinned/tests.rs`, where a fixture that differs in exactly one dimension says more than a
//! generator could.

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::calendar::Date;
use crate::catalog::{AnchorValue, Description, DimensionValue};
use crate::definitions::DefinitionDigest;
use crate::expression::{DialectTag, SqlFragment};
use crate::knowledge::{NoteBody, NoteName, Phrase};
use crate::measure::{AggregatedColumn, Term};
use crate::model::{
    Aggregate, ColumnName, DatasetName, DimensionName, MetricName, ModelName, ProjectName, QualifiedTable, RelationshipName,
    SourceName, TableName,
};
use crate::pinned::DefinitionVersion;

/// One character per check that any parser in this crate makes, so a combination of two can reach
/// the seam between two checks.
///
/// Not a sample of Unicode - a sample of *decisions*. `a` is a legal letter and a legal identifier
/// first character; the space and the tab are whitespace, and the tab is also a control character,
/// which is the pair a parser that checks one and not the other gets wrong; the newline is content
/// in a note body and a refusal in a phrase; `U+200B` is dropped by a phrase's normalisation and
/// refused by everything else, and `U+00AD` is the one the second copy of that set was missing
/// before [`crate::text`] became the only copy; the han character is three bytes, so a bound
/// measured in bytes and one measured in characters disagree about it; the hyphen is what
/// `Hyphens::Admitted` licenses and what `parse_name` refuses at the end of a name; and the full
/// stop is punctuation with no letter or digit in it, which is a phrase's `NotAWord` and a qualified
/// path's separator.
const ADVERSARIAL: [char; 9] = ['a', ' ', '\t', '\n', '\u{200B}', '\u{00AD}', '\u{6F22}', '-', '.'];

/// How long the exhaustive sweep goes.
///
/// **Three, and the four it used to be is why this carries a reason at all.** The old one read:
/// `a` + whitespace + invisible + `a` is the shortest string putting a run with something invisible
/// in it *between* two letters. True, and the case it describes is the one the module header's last
/// table row measures as caught by nothing - a fold that loses a word boundary stays idempotent. A
/// collapse mishandling its leading-space flag stops being idempotent at three characters, which is
/// what the counterexample reports, and every one of the eight mutations reproduced at three.
const SWEEP_LEN: usize = 3;

/// One authored value of each shape the types below accept, as somebody would write it.
///
/// Every one of them is the input [`perturbed`] and [`straddling`] deform, and between them they
/// give every row in the table a value that parses - which is what the non-vacuity assertion in
/// [`survives`] holds each row to. A row whose witness stopped parsing is a row that silently
/// stopped being checked, and that is the failure mode a green property test has.
const WITNESSES: [&str; 10] = [
    "orders",
    "my-project",
    "project.dataset.orders",
    "monthly recurring revenue",
    "2026-06-01",
    // Lower-case hex of 32 bytes: what a digest is, and what `DefinitionDigest::parse` accepts.
    "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
    "v1.2.3",
    "SUM(amount_cents)",
    "duckdb",
    "Net revenue, in minor units.",
];

/// Patterns short enough that repeating one lands on a bound exactly.
///
/// Three of them carry a character a parser removes or refuses, so a bound measured before
/// normalisation and one measured after it disagree; the han character makes a byte count and a
/// character count disagree by three.
const PATTERNS: [&str; 6] = ["a", "\u{6F22}", "a ", "a\u{200B}", "a\n", "a-"];

/// Repeat counts that put a generated value on both sides of **ten** bounds: 32 for a dialect tag,
/// 63 for an identifier, 64 for a dimension value, 120 for a phrase, 128 for a version label, 200
/// for a line count, 256 for a principal, 400 for a reason, 1024 for a SQL fragment and 4096 for a
/// note body and a description.
///
/// **Ten of this crate's bounds, not all of them.** `MAX_KNOWLEDGE_BYTES`,
/// `MAX_VALUES_PER_DIMENSION`, `MAX_DIMENSIONS` and `MAX_PARTS` are unstraddled, and no repeat
/// count reaches them: three bound a COLLECTION and the fourth counts dot-separated parts, so a
/// value that lands on one is a shape this generator does not build. Each is straddled by a
/// hand-written test at its own type instead - `a.b.c.d` for the last of them.
const REPEATS: [usize; 29] = [
    1, 31, 32, 33, 62, 63, 64, 65, 119, 120, 121, 127, 128, 129, 199, 200, 201, 255, 256, 257, 399, 400, 401, 1023, 1024, 1025,
    4095, 4096, 4097,
];

/// Every string of up to [`SWEEP_LEN`] characters over [`ADVERSARIAL`], the empty one included.
fn sweep() -> Vec<String> {
    let mut all = vec![String::new()];
    let mut frontier = vec![String::new()];
    for _ in 0..SWEEP_LEN {
        let mut longer = Vec::with_capacity(frontier.len().saturating_mul(ADVERSARIAL.len()));
        for prefix in &frontier {
            for character in ADVERSARIAL {
                let mut candidate = prefix.clone();
                candidate.push(character);
                longer.push(candidate);
            }
        }
        all.extend_from_slice(&longer);
        frontier = longer;
    }
    all
}

/// Each [`WITNESSES`] value with each [`ADVERSARIAL`] character inserted at each position, once and
/// twice.
///
/// Twice as well as once because one inserted space is a separator and two are a run, and a run is
/// what collapses. The positions are character boundaries, so a han character is never split.
fn perturbed() -> Vec<String> {
    let mut all = Vec::new();
    for witness in WITNESSES {
        let characters: Vec<char> = witness.chars().collect();
        for position in 0..=characters.len() {
            for insert in ADVERSARIAL {
                for count in 1_usize..=2 {
                    let mut candidate: String = characters.iter().take(position).collect();
                    for _ in 0..count {
                        candidate.push(insert);
                    }
                    candidate.extend(characters.iter().skip(position));
                    all.push(candidate);
                }
            }
        }
    }
    all
}

/// Each [`PATTERNS`] entry repeated each of [`REPEATS`] times, so a bound is met from both sides.
fn straddling() -> Vec<String> {
    PATTERNS
        .iter()
        .flat_map(|pattern| REPEATS.iter().map(move |count| pattern.repeat(*count)))
        .collect()
}

/// Everything the property is asked of, once, so twenty-one types share one generation pass.
fn candidates() -> Vec<String> {
    let mut all = sweep();
    all.extend(WITNESSES.iter().map(|witness| String::from(*witness)));
    all.extend(perturbed());
    all.extend(straddling());
    all
}

/// One type's whole property, and the count of candidates it accepted.
///
/// Both halves of the candidate list are load-bearing, which is why a refusal is not a bare
/// `continue`. A value that PARSED has to survive the form the digest is taken over; a value the
/// parser REFUSED has to be refused by `Deserialize` too, because the one path that carries a
/// catalog file is deserialization and `#[serde(try_from = "String")]` is the whole of what routes
/// it back through the constructor. Every one of those attributes in this crate carries a comment
/// saying parsing has to happen at the boundary "or it is theatre" - this is that sentence asked.
///
/// Four assertions over an accepted value rather than one compound one, because they are four
/// things somebody wrote down separately and a failure should name the one it broke.
fn survives<T, P>(kind: &str, parse: P, candidates: &[String]) -> usize
where
    T: Serialize + DeserializeOwned + PartialEq + core::fmt::Debug,
    P: Fn(&str) -> Option<T>,
{
    let mut accepted = 0_usize;
    for candidate in candidates {
        let Some(parsed) = parse(candidate) else {
            // Zero: what the constructor refuses, the deserializer refuses. The candidate is quoted
            // into a JSON string first, escaping included, so the question is asked in the form the
            // input actually arrives in rather than in one no document could hold.
            let quoted = serde_json::to_string(candidate).unwrap_or_else(|cause| panic!("a string quotes: {cause}"));
            assert!(
                serde_json::from_str::<T>(&quoted).is_err(),
                "{kind}: {candidate:?} is refused by parse and accepted by Deserialize, so the one path that \
                 carries a catalog file bypasses the constructor"
            );
            continue;
        };
        accepted += 1;
        let written = serde_json::to_string(&parsed).unwrap_or_else(|cause| panic!("{kind} serializes: {cause}"));

        // One: it serializes AS TEXT. `Date` derived a `Serialize` that wrote its three fields while
        // its `Deserialize` read a string, and the digest then covered a shape no document holds.
        let text: String = serde_json::from_str(&written)
            .unwrap_or_else(|cause| panic!("{kind} serialized {written} rather than the text it was authored as: {cause}"));

        // Two: parsing is idempotent under normalisation. What was stored re-parses to itself, so a
        // value that survived one load cannot drift on the next.
        assert_eq!(
            parse(&text).as_ref(),
            Some(&parsed),
            "{kind}: {candidate:?} parsed, stored {text:?}, and re-parsing that is not the same value"
        );

        // Three: a parsed value serializes the way it came, in bytes. Value equality is the weaker
        // claim and the digest rests on the stronger one - two values that compare equal and
        // serialize differently move a digest without moving any content.
        let reread: T = serde_json::from_str(&written)
            .unwrap_or_else(|cause| panic!("{kind} does not deserialize what it serialized ({written}): {cause}"));
        assert_eq!(reread, parsed, "{kind}: {candidate:?} does not survive its own serde");
        assert_eq!(
            serde_json::to_string(&reread).ok().as_deref(),
            Some(written.as_str()),
            "{kind}: {candidate:?} serializes to different bytes on the second pass, which moves the digest"
        );
    }
    assert!(
        accepted > 0,
        "{kind}: every candidate was refused, so this row asserted nothing - give WITNESSES a value it accepts"
    );
    accepted
}

/// **The property, asked of every parsed newtype whose serialized form the digest is taken over.**
///
/// One row per type rather than a loop, because the rows are twenty-one different types and Rust has
/// no way to put them in one collection. All **nine** `identifier_newtype!` expansions are here
/// even though the macro's own note says one implementation cannot drift from itself: the row costs
/// a line, and with it nothing rests on a reader knowing which types share a parser.
#[test]
fn a_parsed_value_serializes_the_way_it_came_and_reparses_unchanged() {
    let all = candidates();
    let mut checked = 0_usize;

    checked += survives("Phrase", |raw| Phrase::parse(raw).ok(), &all);
    checked += survives("NoteBody", |raw| NoteBody::parse(raw).ok(), &all);
    checked += survives("NoteName", |raw| NoteName::parse(raw).ok(), &all);
    checked += survives("Description", |raw| Description::parse(raw).ok(), &all);
    checked += survives("DimensionValue", |raw| DimensionValue::parse(raw).ok(), &all);
    checked += survives("AnchorValue", |raw| AnchorValue::parse(raw).ok(), &all);
    checked += survives("SqlFragment", |raw| SqlFragment::parse(raw).ok(), &all);
    checked += survives("DialectTag", |raw| DialectTag::parse(raw).ok(), &all);
    checked += survives("ModelName", |raw| ModelName::parse(raw).ok(), &all);
    checked += survives("TableName", |raw| TableName::parse(raw).ok(), &all);
    checked += survives("ColumnName", |raw| ColumnName::parse(raw).ok(), &all);
    checked += survives("MetricName", |raw| MetricName::parse(raw).ok(), &all);
    checked += survives("DimensionName", |raw| DimensionName::parse(raw).ok(), &all);
    checked += survives("RelationshipName", |raw| RelationshipName::parse(raw).ok(), &all);
    checked += survives("SourceName", |raw| SourceName::parse(raw).ok(), &all);
    checked += survives("DatasetName", |raw| DatasetName::parse(raw).ok(), &all);
    checked += survives("ProjectName", |raw| ProjectName::parse(raw).ok(), &all);
    checked += survives("QualifiedTable", |raw| QualifiedTable::parse(raw).ok(), &all);
    checked += survives("DefinitionVersion", |raw| DefinitionVersion::parse(raw).ok(), &all);
    checked += survives("DefinitionDigest", |raw| DefinitionDigest::parse(raw).ok(), &all);
    checked += survives("Date", |raw| Date::parse(raw).ok(), &all);

    // The size of the generated space, so a generator that silently collapsed - an alphabet emptied,
    // a sweep bound read as zero - fails here rather than passing over nothing. The per-row
    // assertion in `survives` catches a row that accepts nothing; this catches all of them at once.
    assert_eq!(
        all.len(),
        4_676,
        "the generated space is not the one this test was measured on"
    );
    assert_eq!(
        checked, 20_050,
        "a different number of generated values parsed than this test was measured on, so some parser's accept set moved"
    );
}

/// The same claim for the one type under the digest whose canonical form is **not** a string.
///
/// [`Term`] pairs `try_from = "TermRepr"` with `into = "TermRepr"`, which is the pairing `Date`
/// lacked, and `crate::measure`'s own tests ask `TermRepr` -> `Term` for every combination of its
/// three optional fields and never ask the other direction. So the `into` half - the half the digest
/// is actually taken over - had nothing on it.
///
/// A table rather than a generator, because the type has seven inhabitants: one per [`Aggregate`],
/// plus `CountIf`. `crate::measure` states what adding an aggregate costs - a generator arm and a
/// golden - so a seventh arriving without a row here does not arrive quietly.
#[test]
fn a_term_survives_the_on_disk_shape_it_serializes_into() {
    let column = ColumnName::parse("amount_cents").expect("a test column is a column");
    let aggregates = [
        Aggregate::Sum,
        Aggregate::Count,
        Aggregate::CountDistinct,
        Aggregate::Avg,
        Aggregate::Min,
        Aggregate::Max,
    ];
    let terms: Vec<Term> = aggregates
        .into_iter()
        .map(|aggregate| Term::Aggregate(AggregatedColumn::new(aggregate, column.clone())))
        .chain([Term::CountIf { column: column.clone() }])
        .collect();
    assert_eq!(terms.len(), 7, "every term this type can hold is in the table");

    for term in terms {
        let written = serde_json::to_string(&term).expect("a term serializes");
        let reread: Term = serde_json::from_str(&written)
            .unwrap_or_else(|cause| panic!("a term does not deserialize what it serialized ({written}): {cause}"));
        assert_eq!(reread, term, "a term has to survive the shape it writes itself as");
        assert_eq!(
            serde_json::to_string(&reread).expect("a term serializes"),
            written,
            "the same term serializes to different bytes on the second pass, which moves the digest"
        );
    }
}
