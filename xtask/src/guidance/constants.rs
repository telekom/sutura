//! A doc comment that restates a constant's value, held against the constant.
//!
//! **Why a gate and not a phrase rule.** `github.com/telekom/sutura#295`: a published page said an
//! adapter's `IMPERSONATION` was `NoPlaceForASubject` while the constant declared
//! `PerSubjectCredential`. The sentence was TRUE when it was written and went false when a constant
//! three commits away in another file changed, so nothing in `CONTRADICTED` could have held it - a
//! forbidden wording is a ratchet on a sentence somebody has already got wrong once, and this one
//! had not been. `just api` then regenerated `docs/api/**` from the doc comment faithfully, which is
//! the part worth naming: **regeneration is not verification.**
//!
//! **A module beside [`advice`](super::advice) rather than lines in the parent, and the same seam.**
//! The parent has to have room for its tables, and this shares nothing with them but the flattened
//! view; `claims.rs` is against the unexemptable 1000-line cap besides.
//!
//! # What it holds
//!
//! One sentence of a doc comment, naming a constant by intra-doc link and a variant of that
//! constant's enum. The constant is resolved out of the tree and the sentence has to name the
//! variant it actually holds. Two citation shapes, both of which the tree writes:
//!
//! * `` [`Type::CONST`] `` - the link IS the pair;
//! * `` [`crate::Type`] ``'s `` `CONST` `` - a type link plus a screaming-snake span in the same
//!   sentence.
//!
//! # What it does NOT hold, before anybody trusts it
//!
//! * **A value the compiler resolves and this does not.** The declaration has to be a literal
//!   `const NAME: Enum = Enum::Variant;` inside an `impl` for the linked type, in the same crate as
//!   the doc comment. A constant reached through an alias, a `const fn`, or a re-export is not
//!   resolved, and an unresolved pair is SKIPPED rather than reported - it under-claims.
//! * **A variant nobody names.** The comparison fires only when the sentence contains another
//!   variant OF THAT ENUM, word-bounded. Prose that describes the value without naming a sibling
//!   variant is invisible here, which is the price of the precision story: a rule that judged the
//!   description would be reading a sentence rather than resolving a citation.
//! * **Anything outside `crates/*/src`.** `docs/api/**` is generated from these files, so the page
//!   follows the source; a hand-written page is `CONTRADICTED`'s scope, not this one.
//! * **The sentence boundary is derived from punctuation** - see [`sentences`] - so a claim written
//!   across two sentences escapes. Widening the window to the doc BLOCK was measured against this
//!   tree and rejected: `crates/sutura-exec-bigquery/src/lib.rs`'s module header states the correct
//!   value in one paragraph and discusses the other variant fourteen lines later, and a
//!   block-scoped rule reports that as a contradiction.
//! * **A `///` inside a multi-line string literal reads as a doc comment.** The code half is
//!   blanked by [`code_lines`], so such a line looks like prose here. Nothing in this tree is in
//!   that shape and a false report would need a resolvable citation on the same line.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use super::claims::flatten;
use crate::markdown;
use crate::repo::matches_any;
use crate::serde_parse::scan::code_lines;

/// The library source a published doc comment lives in. `docs/api/**` is derived from it.
const OVER: &[&str] = &["crates/*/src/**/*.rs"];

/// Where a constant's value is declared, and what it is.
struct Held {
    /// The crate directory, so a citation resolves against the doc comment's own crate.
    krate: String,
    /// The type whose `impl` block declares it - the half an intra-doc link names.
    on: String,
    /// The constant's own name.
    name: String,
    /// The enum its type is, whose variants the comparison is against.
    kind: String,
    /// The variant it holds.
    variant: String,
    /// Repo-relative file of the declaration, so a failure names both ends.
    file: String,
    /// One-based line of the declaration.
    line: usize,
}

/// The crate directory a repo-relative path is in - `crates/<name>` - or the path itself.
///
/// The resolution scope. A constant is resolved against the crate the doc comment is in rather
/// than against the workspace, because `IMPERSONATION` is a required associated item that a dozen
/// adapters and fakes each declare: a workspace-wide lookup would be ambiguous everywhere and
/// resolve nothing.
fn crate_of(rel: &str) -> String {
    let mut parts = rel.split('/');
    match (parts.next(), parts.next()) {
        (Some(first), Some(second)) => format!("{first}/{second}"),
        _ => String::from(rel),
    }
}

/// The leading identifier of `text`, if it starts with one.
fn ident_at(text: &str) -> Option<&str> {
    let end = text
        .char_indices()
        .take_while(|(_, c)| c.is_ascii_alphanumeric() || *c == '_')
        .map(|(at, c)| at.saturating_add(c.len_utf8()))
        .last()?;
    text.get(..end)
}

/// Is this an `ALL_CAPS` constant name rather than a type or a word?
///
/// Digits and underscores are allowed inside; the first character has to be an uppercase letter,
/// and no lowercase letter may appear. `IMPERSONATION` and `MAX_ROWS` qualify, `BigQuery` does not.
fn screaming(name: &str) -> bool {
    name.starts_with(|c: char| c.is_ascii_uppercase())
        && !name.chars().any(char::is_lowercase)
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Is this a type-shaped identifier - one an intra-doc link could name?
fn camel(name: &str) -> bool {
    name.starts_with(|c: char| c.is_ascii_uppercase()) && name.chars().any(char::is_lowercase)
}

/// The last path segment of an intra-doc link target, generics and sigils stripped.
fn last_segment(path: &str) -> &str {
    path.rsplit("::").next().unwrap_or(path).trim_start_matches(['&', '\''])
}

/// The type an `impl` header is for, or `None` if this line opens no `impl` block.
///
/// `impl<T> Warehouse for BigQueryWarehouse<T>` is the shape that matters - the type after the
/// LAST ` for `, which is the implementor rather than the trait. A bare `impl Foo` has no ` for `
/// and the token after `impl` is the type.
fn impl_target(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    let after = trimmed.strip_prefix("impl")?;
    if after.starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_') {
        return None;
    }
    let head = after.split_once('{').map_or(after, |(before, _)| before);
    let head = head.split(" where ").next().unwrap_or(head);
    let subject = head.rsplit(" for ").next().unwrap_or(head).trim();
    // Generics on the implementor are stripped by taking the leading identifier, and a
    // fully-qualified implementor by taking the last segment first.
    let name = ident_at(last_segment(subject.split('<').next().unwrap_or(subject).trim()))?;
    camel(name).then(|| String::from(name))
}

/// One `const NAME: Enum = Enum::Variant;` declaration, as read off a line.
///
/// A named struct rather than a tuple because `clippy::type_complexity` refuses the tuple - and it
/// reads better at both ends.
struct Declared {
    /// The constant's name.
    name: String,
    /// The enum its type is.
    kind: String,
    /// The variant it holds.
    variant: String,
}

/// `const NAME: Enum = Enum::Variant;` read off one line of code, or `None`.
fn const_declaration(line: &str) -> Option<Declared> {
    let trimmed = line
        .trim_start()
        .trim_start_matches("pub ")
        .trim_start_matches("pub(crate) ")
        .trim_start_matches("pub(super) ")
        .trim_start();
    let after = trimmed.strip_prefix("const ")?;
    let name = ident_at(after.trim_start())?;
    if !screaming(name) {
        return None;
    }
    let (declared, value) = after.split_once('=')?;
    let kind = ident_at(last_segment(declared.split_once(':')?.1.trim()))?;
    let (path, variant) = value.trim().trim_end_matches(';').rsplit_once("::")?;
    let variant = ident_at(variant.trim())?;
    // The value's own path has to be the declared type: `Enum::Variant`, not some other constant.
    if ident_at(last_segment(path.trim())) != Some(kind) || !camel(kind) || !camel(variant) {
        return None;
    }
    Some(Declared {
        name: String::from(name),
        kind: String::from(kind),
        variant: String::from(variant),
    })
}

/// One in-scope file, read.
struct Source {
    /// Repo-relative path.
    rel: String,
    /// Its whole text.
    text: String,
}

/// The in-scope files that could be read, and one problem per file that could not.
///
/// A named struct rather than a tuple because `clippy::type_complexity` refuses the tuple, and
/// because the read failures are part of the ANSWER rather than an aside.
struct Sources {
    /// What was read.
    read: Vec<Source>,
    /// One entry per in-scope file that is not readable text.
    unreadable: Vec<String>,
}

/// The in-scope library source, read once, with an unreadable file as a PROBLEM.
///
/// **Fail closed on a read, not just on a parse.** Review of `github.com/telekom/sutura#301`
/// measured the sibling shape one gate over: a non-UTF-8 page under `docs/` left
/// `check-shipped-binaries` at `ok` and exit 0, because the reader said `continue`. The same
/// `let Ok(..) else { continue }` here would drop the file holding a declaration or the file
/// making a false claim, and every other file would keep the verdict looking like an answer. One
/// read for all three passes, so there is one place this can go wrong instead of three.
fn readable(root: &Path, files: &[String]) -> Sources {
    let mut read = Vec::new();
    let mut unreadable = Vec::new();
    for rel in files {
        if !matches_any(OVER, rel) {
            continue;
        }
        match std::fs::read_to_string(root.join(rel)) {
            Ok(text) => read.push(Source { rel: rel.clone(), text }),
            Err(why) => unreadable.push(format!(
                "{rel}: cannot be read as UTF-8 text - {why}. A file this check cannot look at may \
                 be the one holding the declaration or the one making the claim, so the verdict is \
                 over the files it names or it is nothing"
            )),
        }
    }
    Sources { read, unreadable }
}

/// Every `const NAME: Enum = Enum::Variant;` in the library source, with the type it is declared on.
///
/// Read from [`code_lines`], which is what keeps a doctest out of it: `sutura-domain`'s
/// `Warehouse` documentation shows an `impl` block with this exact declaration inside a fenced
/// example, and a raw-text scan reads that as a second declaration of the same constant.
fn declarations(read: &[Source]) -> Vec<Held> {
    let mut found = Vec::new();
    for Source { rel, text } in read {
        let mut on: Option<String> = None;
        for (index, line) in code_lines(text).iter().enumerate() {
            if let Some(target) = impl_target(line) {
                on = Some(target);
                continue;
            }
            let Some(subject) = on.as_ref() else {
                continue;
            };
            if let Some(declared) = const_declaration(line) {
                found.push(Held {
                    krate: crate_of(rel),
                    on: subject.clone(),
                    name: declared.name,
                    kind: declared.kind,
                    variant: declared.variant,
                    file: rel.clone(),
                    line: index.saturating_add(1),
                });
            }
        }
    }
    found
}

/// Every enum in the library source and the variants it declares.
///
/// The variant list is what makes the comparison narrow: a `CamelCase` word in a sentence is only
/// compared when it is a variant OF THE ENUM the resolved constant holds.
fn variants(read: &[Source]) -> BTreeMap<String, BTreeSet<String>> {
    let mut found: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for Source { text, .. } in read {
        let mut open: Option<(String, usize)> = None;
        for line in code_lines(text) {
            if let Some((name, depth)) = open.as_mut() {
                let after = depth
                    .saturating_add(line.matches('{').count())
                    .saturating_sub(line.matches('}').count());
                if *depth == 1
                    && let Some(variant) = ident_at(line.trim_start())
                    && camel(variant)
                {
                    found.entry(name.clone()).or_default().insert(String::from(variant));
                }
                *depth = after;
                if after == 0 {
                    open = None;
                }
                continue;
            }
            if let Some(name) = enum_header(&line) {
                open = Some((name, 1));
            }
        }
    }
    found
}

/// The enum a line declares, if the line both names one and opens its body.
fn enum_header(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    let after = trimmed
        .strip_prefix("pub ")
        .or_else(|| trimmed.strip_prefix("pub(crate) "))
        .or_else(|| trimmed.strip_prefix("pub(super) "))
        .unwrap_or(trimmed)
        .trim_start()
        .strip_prefix("enum ")?;
    if !line.contains('{') {
        return None;
    }
    let name = ident_at(after.trim_start())?;
    camel(name).then(|| String::from(name))
}

/// The doc comment lines of a Rust file, one entry per SOURCE line so a report names a line a
/// reader will open, with every fenced example blanked.
///
/// The fences go through [`markdown`] rather than a local toggle, for
/// `github.com/telekom/sutura#301`'s reason: a doc comment is markdown, and the block a
/// `sutura-domain` example writes contains the very declaration this gate resolves. **An unclosed
/// block is an `Err`**, so a file whose examples cannot be lexed is a failure rather than a file
/// read as all prose.
fn doc_prose(text: &str) -> Result<String, markdown::Unlexable> {
    let mut doc = String::new();
    for line in text.lines() {
        let trimmed = line.trim_start();
        let body = trimmed
            .strip_prefix("//!")
            .or_else(|| trimmed.strip_prefix("///"))
            .unwrap_or("");
        doc.push_str(body.strip_prefix(' ').unwrap_or(body));
        doc.push('\n');
    }
    // The complement of the code half: a doc line is prose when the code half kept nothing from
    // it. A fence DELIMITER is blank in both halves and therefore reads as prose here - it carries
    // an info string and no claim, so nothing this gate looks for can be on one.
    let fenced = markdown::code(&doc)?;
    let mut out = String::new();
    for (index, line) in doc.lines().enumerate() {
        if fenced.get(index).is_some_and(String::is_empty) {
            out.push_str(line);
        }
        out.push('\n');
    }
    Ok(out)
}

/// One sentence of flattened prose: where it starts, and its text.
struct Sentence {
    /// One-based source line the sentence starts on.
    line: usize,
    /// The flattened text.
    text: String,
}

/// Split flattened prose into sentences.
///
/// **A terminator, optional closing markup, whitespace, and a next word that does not begin
/// lowercase.** The markup step is load-bearing rather than tidy: this tree bolds a lead sentence,
/// so `existed.** This adapter` has its terminator two characters away from the space, and without
/// it two sentences merge into one window. A lowercase continuation is treated as the same sentence
/// so that `0.4.1` and `docs/adr/0018.` do not split one.
fn sentences(text: &str) -> Vec<Sentence> {
    let (flat, lines) = flatten(text);
    let characters: Vec<char> = flat.chars().collect();
    let mut out = Vec::new();
    let mut start = 0_usize;
    for (at, character) in characters.iter().enumerate() {
        if !matches!(*character, '.' | '!' | '?') {
            continue;
        }
        let mut after = at.saturating_add(1);
        while characters
            .get(after)
            .is_some_and(|c| matches!(*c, '*' | '_' | '`' | ')' | '"' | ']'))
        {
            after = after.saturating_add(1);
        }
        if characters.get(after).is_none_or(|c| *c != ' ') {
            continue;
        }
        let next = after.saturating_add(1);
        if characters.get(next).is_some_and(|c| c.is_lowercase()) {
            continue;
        }
        out.push(Sentence {
            line: lines.get(start).copied().unwrap_or(1),
            text: characters.get(start..after).map(|s| s.iter().collect()).unwrap_or_default(),
        });
        start = next;
    }
    if let Some(tail) = characters.get(start..).filter(|rest| !rest.is_empty()) {
        out.push(Sentence {
            line: lines.get(start).copied().unwrap_or(1),
            text: tail.iter().collect(),
        });
    }
    out
}

/// Every intra-doc link target in one sentence: the text between `` [` `` and `` `] ``.
fn linked(sentence: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = sentence;
    while let Some(at) = rest.find("[`") {
        let tail = rest.get(at.saturating_add(2)..).unwrap_or("");
        match tail.find("`]") {
            Some(end) => {
                if let Some(target) = tail.get(..end) {
                    out.push(target);
                }
                rest = tail.get(end.saturating_add(2)..).unwrap_or("");
            }
            None => break,
        }
    }
    out
}

/// Every `(type, constant)` pair one sentence cites, in either shape.
fn cited_pairs(sentence: &str) -> Vec<(String, String)> {
    let links = linked(sentence);
    let mut out = Vec::new();
    for target in &links {
        let last = last_segment(target);
        // Shape one: the link is the pair. `Self::CONST` is deliberately not resolved - the type
        // it means is the enclosing item, which a text scan may not claim to know.
        if screaming(last)
            && let Some((head, _)) = target.rsplit_once("::")
        {
            let on = last_segment(head);
            if camel(on) {
                out.push((String::from(on), String::from(last)));
            }
        }
    }
    // Shape two: a type link plus a screaming span elsewhere in the same sentence. The link
    // targets are themselves in backticks, so a span that IS a link target is either screaming
    // (shape one, already read) or type-shaped, and neither collides with this.
    let screams: Vec<&str> = super::spans(sentence)
        .into_iter()
        .filter(|span| screaming(span) && !span.contains("::"))
        .collect();
    for target in &links {
        let on = last_segment(target);
        if !camel(on) {
            continue;
        }
        for name in &screams {
            out.push((String::from(on), String::from(*name)));
        }
    }
    out
}

/// Does `text` contain `word` with an identifier boundary on both sides?
fn names(text: &str, word: &str) -> bool {
    let mut from = 0_usize;
    while let Some(at) = text.get(from..).and_then(|rest| rest.find(word)) {
        let offset = from.saturating_add(at);
        let before = text.get(..offset).and_then(|head| head.chars().next_back());
        let after = text
            .get(offset.saturating_add(word.len())..)
            .and_then(|tail| tail.chars().next());
        let bounded = |c: Option<char>| c.is_none_or(|c| !c.is_alphanumeric() && c != '_');
        if bounded(before) && bounded(after) {
            return true;
        }
        from = offset.saturating_add(word.len().max(1));
    }
    false
}

/// What one pass over the library source found.
struct Reading {
    /// Sentences whose cited pair resolved to exactly one declaration.
    resolved: usize,
    /// Of those, the ones that named the variant the constant actually holds.
    confirmed: usize,
    /// The disagreements.
    problems: Vec<String>,
}

/// The rule, over one file's doc comments.
fn read_file(rel: &str, text: &str, held: &[Held], enums: &BTreeMap<String, BTreeSet<String>>, into: &mut Reading) {
    let prose = match doc_prose(text) {
        Ok(prose) => prose,
        Err(why) => {
            into.problems.push(format!("{rel}: the doc comments cannot be lexed - {why}"));
            return;
        }
    };
    let krate = crate_of(rel);
    for sentence in sentences(&prose) {
        for (on, name) in cited_pairs(&sentence.text) {
            let mut matched = held
                .iter()
                .filter(|one| one.krate == krate && one.on == on && one.name == name);
            let Some(one) = matched.next() else {
                continue;
            };
            // Two declarations of one pair in one crate is ambiguous, and a gate may not guess.
            if matched.next().is_some() {
                continue;
            }
            into.resolved = into.resolved.saturating_add(1);
            if names(&sentence.text, &one.variant) {
                into.confirmed = into.confirmed.saturating_add(1);
            }
            let Some(siblings) = enums.get(&one.kind) else {
                continue;
            };
            for wrong in siblings.iter().filter(|v| **v != one.variant) {
                if names(&sentence.text, wrong) {
                    into.problems.push(format!(
                        "{rel}:{}: says `{wrong}` in the sentence citing [`{on}`]'s `{name}`, which holds \
                         `{}` ({}:{}) - state the variant it holds, or say what the declaration MEANS and \
                         let the link carry the value",
                        sentence.line, one.variant, one.file, one.line
                    ));
                }
            }
        }
    }
}

/// A doc comment may not name a variant its own constant does not hold.
pub(in crate::guidance) fn constant_problems(root: &Path, files: &[String]) -> (Vec<String>, usize) {
    let sources = readable(root, files);
    let held = declarations(&sources.read);
    let enums = variants(&sources.read);
    let mut reading = Reading {
        resolved: 0,
        confirmed: 0,
        problems: sources.unreadable,
    };
    for Source { rel, text } in &sources.read {
        read_file(rel, text, &held, &enums, &mut reading);
    }
    // FAIL CLOSED, and on the CONFIRMED count rather than the resolved one. A resolver that runs
    // and never compares is the shape `.agents/skills/sutura/gates/SKILL.md` records under a
    // derived value no page states: the walk succeeds, nothing disagrees, and the verdict is about
    // silence. At least one doc comment in this tree states a constant's value correctly, so a
    // zero here means the sentence reader stopped reading rather than the prose being clean.
    if reading.confirmed == 0 {
        reading.problems.push(format!(
            "no doc comment under {OVER:?} was read as stating the variant its own constant holds, \
             out of {} resolvable pair(s) - so the comparison ran over nothing. Either the sentence \
             reader stopped reading, or the last doc comment that stated a value has stopped \
             stating one: state one, or delete this check rather than leaving it green over silence",
            reading.resolved
        ));
    }
    (reading.problems, reading.confirmed)
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use super::Held;

    fn held() -> Vec<Held> {
        vec![Held {
            krate: String::from("crates/sutura-exec-bigquery"),
            on: String::from("BigQueryWarehouse"),
            name: String::from("IMPERSONATION"),
            kind: String::from("ImpersonationCapability"),
            variant: String::from("PerSubjectCredential"),
            file: String::from("crates/sutura-exec-bigquery/src/lib.rs"),
            line: 624,
        }]
    }

    fn enums() -> BTreeMap<String, BTreeSet<String>> {
        BTreeMap::from([(
            String::from("ImpersonationCapability"),
            BTreeSet::from([String::from("PerSubjectCredential"), String::from("NoPlaceForASubject")]),
        )])
    }

    fn read(text: &str) -> super::Reading {
        let mut reading = super::Reading {
            resolved: 0,
            confirmed: 0,
            problems: Vec::new(),
        };
        super::read_file(
            "crates/sutura-exec-bigquery/src/wire/credential.rs",
            text,
            &held(),
            &enums(),
            &mut reading,
        );
        reading
    }

    /// THE INSTANCE, in the wording it shipped in.
    ///
    /// `github.com/telekom/sutura#295`: the sentence names the sibling variant, the constant holds
    /// the other one, and the published page carried it because `just api` regenerates faithfully.
    #[test]
    fn a_sentence_naming_the_sibling_variant_is_refused() {
        let reading = read(concat!(
            "//! [`Bearer`] carries the deadline because a minted token has one, and\n",
            "//! [`crate::BigQueryWarehouse`]'s `IMPERSONATION` still says `NoPlaceForASubject`\n",
            "//! because nothing mints one.\n",
        ));
        assert_eq!(reading.resolved, 1);
        assert_eq!(reading.confirmed, 0);
        assert_eq!(reading.problems.len(), 1, "{:?}", reading.problems);
        let problem = reading.problems.first().map(String::as_str).unwrap_or_default();
        assert!(problem.contains("says `NoPlaceForASubject`"), "{problem}");
        assert!(problem.contains("holds `PerSubjectCredential`"), "{problem}");
        // The line the claim STARTS on, found through the flattened view: the constant and the
        // variant are on source line 2 and the sentence begins on line 1.
        assert!(problem.contains("credential.rs:1:"), "{problem}");
    }

    /// The other citation shape, and the direction that proves the comparison runs.
    #[test]
    fn the_link_can_be_the_pair_and_agreement_is_counted() {
        let reading = read(
            "//! [`BigQueryWarehouse::IMPERSONATION`] is `PerSubjectCredential`, which is what\n//! makes a source representable.\n",
        );
        assert_eq!(reading.resolved, 1);
        assert_eq!(reading.confirmed, 1);
        assert!(reading.problems.is_empty(), "{:?}", reading.problems);
    }

    /// A sibling variant in a DIFFERENT sentence is not a contradiction.
    ///
    /// Measured against this tree rather than assumed: `sutura-exec-bigquery`'s module header
    /// states the value correctly and then discusses the other variant further down, so a rule
    /// scoped to the doc BLOCK reports a correct file. This is why the window is a sentence.
    #[test]
    fn the_window_is_a_sentence_and_not_the_block() {
        let reading = read(concat!(
            "//! [`BigQueryWarehouse::IMPERSONATION`] is `PerSubjectCredential`, which is what makes\n",
            "//! a source executed as the asking subject representable here.\n",
            "//!\n",
            "//! The reason it is a refusal is the reason the whole-shape `NoPlaceForASubject` it\n",
            "//! replaced existed.\n",
        ));
        assert_eq!(reading.confirmed, 1);
        assert!(reading.problems.is_empty(), "{:?}", reading.problems);
    }

    /// A bolded lead sentence still ends where a reader ends it.
    ///
    /// `existed.** This` puts the terminator two characters from the space, which the first
    /// version of [`super::sentences`] did not skip - so the two sentences merged and a correct
    /// file was reported.
    #[test]
    fn a_terminator_behind_closing_markup_still_ends_the_sentence() {
        let split = super::sentences("**What it replaced.** [`BigQueryWarehouse::IMPERSONATION`] is `PerSubjectCredential`.");
        assert_eq!(split.len(), 2, "{:?}", split.iter().map(|s| &s.text).collect::<Vec<_>>());
    }

    /// A fenced example inside a doc comment declares nothing this gate reads.
    ///
    /// `sutura-domain`'s `Warehouse` documentation shows an `impl` block carrying this exact
    /// declaration, so a scan over raw doc text would read the example as a claim.
    #[test]
    fn a_fenced_example_in_a_doc_comment_is_not_prose() {
        let prose = super::doc_prose(concat!(
            "/// ```\n",
            "/// [`BigQueryWarehouse::IMPERSONATION`] is `NoPlaceForASubject`\n",
            "/// ```\n",
            "/// and outside the block it is not stated.\n",
        ))
        .unwrap_or_else(|e| panic!("{e}"));
        assert!(!prose.contains("NoPlaceForASubject"), "{prose}");
        assert!(prose.contains("outside the block"), "{prose}");
    }

    /// An unclosed example is a refusal rather than a file read as all prose.
    #[test]
    fn a_doc_comment_whose_example_never_closes_is_refused() {
        let reading = read("/// ```\n/// [`BigQueryWarehouse::IMPERSONATION`] is `NoPlaceForASubject`\n");
        assert!(
            reading.problems.iter().any(|p| p.contains("cannot be lexed")),
            "{:?}",
            reading.problems
        );
    }

    /// The declaration reader, and the two shapes it has to tell apart.
    #[test]
    fn a_const_declaration_is_read_off_the_impl_it_is_in() {
        assert_eq!(
            super::impl_target("impl<T> Warehouse for BigQueryWarehouse<T>").as_deref(),
            Some("BigQueryWarehouse")
        );
        assert_eq!(
            super::impl_target("impl BigQueryWarehouse {").as_deref(),
            Some("BigQueryWarehouse")
        );
        // Not an `impl`, and not a word that starts with one.
        assert_eq!(super::impl_target("implementor of the port"), None);
        let read = super::const_declaration(
            "    const IMPERSONATION: ImpersonationCapability = ImpersonationCapability::PerSubjectCredential;",
        )
        .unwrap_or_else(|| panic!("the declaration this whole check resolves was not read"));
        assert_eq!(read.name, "IMPERSONATION");
        assert_eq!(read.kind, "ImpersonationCapability");
        assert_eq!(read.variant, "PerSubjectCredential");
        // A constant whose value is not a variant of its own type is not this rule's shape.
        assert!(super::const_declaration("    const LIMIT: usize = 10_001;").is_none());
        assert!(super::const_declaration("    const POSTURE: SourcePosture = Other::Shared;").is_none());
    }

    /// A variant nobody names, and a citation that resolves to nothing, are both SKIPPED.
    ///
    /// The under-claiming direction, asserted so a later widening has to move it deliberately.
    #[test]
    fn an_unresolvable_citation_is_skipped_rather_than_reported() {
        let reading = read("//! [`crate::SomethingElse`]'s `IMPERSONATION` says `NoPlaceForASubject`.\n");
        assert_eq!(reading.resolved, 0);
        assert!(reading.problems.iter().all(|p| !p.contains("says")), "{:?}", reading.problems);
    }

    /// THE REAL TREE, because the floor this check fails closed on is a property of the tree.
    ///
    /// A fixture cannot establish that a doc comment somewhere states a constant's value correctly,
    /// and that is the only thing keeping the comparison from passing over silence - the shape
    /// `.agents/skills/sutura/gates/SKILL.md` records for a derived value no page states. So the
    /// liveness is asserted here rather than described, in both directions: the workspace resolves
    /// pairs, at least one agrees, and nothing disagrees.
    #[test]
    fn the_real_workspace_is_what_the_floor_is_about() {
        let Some(crate::repo::RepoFiles { root, files }) = crate::repo::all_files() else {
            panic!("could not locate the repo");
        };
        let sources = super::readable(&root, &files);
        assert!(sources.unreadable.is_empty(), "{:?}", sources.unreadable);
        let held = super::declarations(&sources.read);
        assert!(
            held.iter()
                .any(|one| one.name == "IMPERSONATION" && one.kind == "ImpersonationCapability"),
            "the declaration reader found no IMPERSONATION at all, so nothing below means anything"
        );
        let (problems, confirmed) = super::constant_problems(&root, &files);
        assert!(problems.is_empty(), "{problems:?}");
        assert!(
            confirmed > 0,
            "no doc comment states a constant's value, so the floor is what fails"
        );
    }
}
