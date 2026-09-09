//! What a scan was OFFERED, against what it reached - as a witness rather than as a sentence.
//!
//! `github.com/telekom/sutura#414`, third level. [`super::census`] closed this class for the
//! *repository* walk: the loop lives inside `Census::inspect`, so a gate never holds the listing
//! and `.take(n)` has nowhere to be written. What it does not reach is the walk a gate does
//! **inside** what the census handed it - a per-glob file scan, a per-file line scan, a set the
//! gate partitions itself. Three of those were measured on `036bce03`, each reporting `ok` over a
//! subject it had mostly not read:
//!
//! | narrowing | `just hygiene` | what was lost |
//! | --- | --- | --- |
//! | `.take(100)` on `absences`' per-file line walk | **exit 0** | 214500 of 294744 production lines, `file(s)` byte-identical |
//! | one `continue` after that gate's glob filter | **exit 0** | 136 of 164 files, `just test` green at 2831 passed |
//! | `.take(3)` on `test-causality`'s revert classification | - | a 4-file revert excused from 3, suite green |
//!
//! **The floor those gates had was derived from the loop it was meant to police**, which is the
//! whole shape: `lines > files * 4` is 656 against a real 294744, so a walk that reads a hundred
//! lines per file satisfies it with 73% of the walk gone. A number a narrowing moves along with
//! itself is not a witness.
//!
//! # What this holds, and where it stops
//!
//! * **[`Offered::each`] owns the loop.** The caller passes a closure and never receives the
//!   sequence, so the narrowing has nowhere to be written on the caller's side - `Offered` is a
//!   sealed witness type in `check-newtype-leaks` for that reason.
//! * **[`Reached`] carries one outcome per subject BY CONSTRUCTION.** `each` pushes the closure's
//!   return value once per call, and [`Reached::of`] refuses a payload whose length is not the
//!   number of subjects offered - so a `.take(n)` written *inside* `each` is a [`Short`] rather
//!   than a shorter walk. That is the arm the two absences instances above are red on now.
//! * **A witness cannot be minted while unequal**, and the number a verdict prints is the
//!   witness's own length rather than a count the caller kept. Generalised from
//!   `warm_start::pairing::Swept`, which keeps its own derivation on purpose.
//! * **It does NOT hold the SELECTION.** `matching` counts its subjects with the same glob call
//!   the walk then makes, so narrowing the glob *itself* - in the table, or inside `matching` -
//!   moves both sides and is held by review and by the caller's own floors, not by this type.
//!   That is the same residue [`super::census`] records for a narrowed `Scope`.
//! * **It does NOT hold what the closure DOES.** A closure is `FnMut` and may capture, so it can
//!   count its own calls and decline the rest with `return;` - `#414`'s instance 8 one level down.
//!   What closes that here is the caller comparing its own record of reached subjects against
//!   [`Reached::subjects`], which `absences` does per sighting; nothing in this type can require
//!   it, because Rust has no effect system and a closure's body is the caller's business.

/// The subjects one scan is about, selected HERE rather than in the caller's loop.
///
/// The field is private and there is no accessor, no `IntoIterator` and no `Deref`: **a caller
/// that cannot hold the sequence cannot narrow it**, which is [`super::census::Census`]'s argument
/// at the scale of one gate's inner walk.
pub(crate) struct Offered<'a> {
    /// What a subject IS, for the message: `"file"`, `"production line"`, `"reverted file"`.
    what: &'static str,
    /// The subjects, in the order the scan will see them.
    subjects: Vec<&'a str>,
}

impl<'a> Offered<'a> {
    /// Every path in `all` that one of `globs` matches.
    ///
    /// **The glob filter is here, not in the caller's loop**, which is what the second measured
    /// instance needed: that gate's loop began `if !matches_any(sighting.over, rel) { continue; }`,
    /// and one more `continue` under it left 136 of 164 files unscanned at exit 0 with the suite
    /// green. There is no filter beside the loop to hide in now.
    pub(crate) fn matching(what: &'static str, all: &'a [String], globs: &[&str]) -> Self {
        Self {
            what,
            subjects: all
                .iter()
                .filter(|rel| super::matches_any(globs, rel))
                .map(String::as_str)
                .collect(),
        }
    }

    /// Every line of `text`, one subject each, so the walk INSIDE a file is accounted the same way
    /// the walk over files is. `each` numbers them from one, as a reader's editor does.
    pub(crate) fn lines(what: &'static str, text: &'a str) -> Self {
        Self {
            what,
            subjects: text.lines().collect(),
        }
    }

    /// A set the caller already holds.
    ///
    /// **The weakest of the three constructors and the only honest way to say so is here**: the
    /// caller built the slice, so a narrowing written *before* this call is not visible to
    /// anything. It closes the loop only - `for path in revert` narrowed to `.take(3)` - which is
    /// exactly the third measured instance.
    pub(crate) fn over(what: &'static str, subjects: &'a [String]) -> Self {
        Self {
            what,
            subjects: subjects.iter().map(String::as_str).collect(),
        }
    }

    /// Hand every subject to `visit`, and keep what it returns for each.
    ///
    /// The loop is here. `visit` gets the subject's 1-based ordinal and the subject, and its
    /// return value is collected - so [`Reached`] holds one outcome per subject and the length
    /// equality is a property of this function rather than a comparison a caller remembers to
    /// make. A `.take(n)` written on the iterator below is a [`Short`], because `subjects` was
    /// counted before the walk and the payload is counted after it.
    pub(crate) fn each<T>(self, mut visit: impl FnMut(usize, &str) -> T) -> Result<Reached<T>, Short> {
        let offered = self.subjects.len();
        let mut of = Vec::with_capacity(offered);
        for (index, subject) in self.subjects.iter().enumerate() {
            of.push(visit(index.saturating_add(1), subject));
        }
        Reached::of(self.what, offered, of)
    }
}

/// A scan that reached EVERY subject it was offered, and what it made of each.
///
/// [`Reached::of`] is private and [`Offered::each`] is the only thing that calls it, so the
/// payload cannot be shorter than the subject list: a caller cannot construct this while unequal.
/// The verdict prints `of.len()`, which is the witness's own length rather than a second source
/// for a number that must have exactly one.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Reached<T> {
    what: &'static str,
    of: Vec<T>,
}

impl<T> Reached<T> {
    /// The only constructor, and it refuses a payload that is not one outcome per subject.
    fn of(what: &'static str, offered: usize, of: Vec<T>) -> Result<Self, Short> {
        if of.len() == offered {
            return Ok(Self { what, of });
        }
        Err(Short {
            what,
            offered,
            reached: of.len(),
        })
    }

    /// How many subjects were offered, which is how many were reached. **Minted by the walk**, so
    /// a caller comparing its own record against this is comparing two derivations.
    pub(crate) const fn subjects(&self) -> usize {
        self.of.len()
    }

    /// What the scan made of each subject, in order.
    ///
    /// **This is deliberately not sealed and the reason is worth the line**: the outcomes ARE the
    /// gate's findings, so a witness that could not lend them would be a witness nothing could
    /// report from. What is sealed is [`Offered`] - the sequence a narrowing would have to reach.
    pub(crate) fn outcomes(&self) -> &[T] {
        &self.of
    }

    /// Resolve every outcome, or nothing at all.
    ///
    /// `collect` into an `Option` short-circuits, so a `Some` result holds exactly as many items
    /// as this witness did: **the one-per-subject property survives the map**, which is what lets
    /// a caller turn `Reached<Option<Found>>` into `Reached<Found>` without a length check of its
    /// own. `None` means at least one subject had no outcome, and that is the caller's fail-closed
    /// arm rather than a shorter list.
    pub(crate) fn all<U>(self, resolve: impl FnMut(T) -> Option<U>) -> Option<Reached<U>> {
        let what = self.what;
        self.of
            .into_iter()
            .map(resolve)
            .collect::<Option<Vec<U>>>()
            .map(|of| Reached { what, of })
    }
}

/// A scan that reached fewer subjects than it was offered. Each field comes from a different side
/// of the walk, which is the only reason the pair is worth anything.
#[derive(Debug)]
pub(crate) struct Short {
    what: &'static str,
    offered: usize,
    reached: usize,
}

impl Short {
    /// The wording lives here once, so no gate writes its own.
    pub(crate) fn describe(&self) -> String {
        format!(
            "reached {} of {} {}(s) - the walk is short by {}, so its verdict is about a subset \
             and every count in it agrees with itself over ground it never read",
            self.reached,
            self.offered,
            self.what,
            self.offered.saturating_sub(self.reached)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{Offered, Reached};

    fn paths() -> Vec<String> {
        ["crates/a/src/lib.rs", "crates/b/src/lib.rs", "docs/a.md"]
            .into_iter()
            .map(String::from)
            .collect()
    }

    #[test]
    fn every_offered_subject_reaches_the_closure_and_the_witness_is_its_own_length() {
        // The property the three measured instances lacked: the number the verdict prints is the
        // walk's own length, and the walk is not the caller's to shorten.
        let all = paths();
        let mut seen = Vec::new();
        let reached = Offered::matching("file", &all, &["crates/*/src/**/*.rs"])
            .each(|ordinal, rel| {
                seen.push(format!("{ordinal}:{rel}"));
            })
            .expect("every offered subject was visited");
        assert_eq!(reached.subjects(), 2, "the glob selects two, and both were reached");
        assert_eq!(seen, vec!["1:crates/a/src/lib.rs", "2:crates/b/src/lib.rs"]);
    }

    #[test]
    fn a_payload_shorter_than_the_subject_list_is_refused_rather_than_reported() {
        // `.take(n)` inside `each` is exactly this: the subjects were counted before the walk and
        // the payload after it, so a truncated walk cannot mint the witness. Constructed directly
        // because `each` is the only thing that can produce the mismatch, and it cannot be asked
        // to truncate itself.
        let short = Reached::of("production line", 294_744, vec![(); 80_244])
            .expect_err("80244 outcomes over 294744 subjects is not a verdict");
        let said = short.describe();
        assert!(said.contains("reached 80244 of 294744 production line(s)"), "{said}");
        assert!(said.contains("short by 214500"), "{said}");
    }

    #[test]
    fn a_line_walk_numbers_from_one_and_covers_the_whole_file() {
        // The per-file half. A file's last line is as much a subject as its first, which is the
        // instance a per-FILE count could not see.
        let text = "one\ntwo\nthree\n";
        let mut numbers = Vec::new();
        let reached = Offered::lines("production line", text)
            .each(|number, line| {
                numbers.push((number, String::from(line)));
            })
            .expect("every line was walked");
        assert_eq!(reached.subjects(), 3);
        assert_eq!(numbers.last().map(|(n, _)| *n), Some(3), "{numbers:?}");
    }

    #[test]
    fn resolving_every_outcome_keeps_the_one_per_subject_property_and_one_gap_yields_nothing() {
        // What lets a caller narrow `Reached<Option<T>>` to `Reached<T>` with no length check of
        // its own - and what makes a single unresolved subject a fail-closed arm rather than a
        // shorter list, which is the third instance's whole defect.
        let all = paths();
        let reached = Offered::over("reverted file", &all)
            .each(|_, rel| rel.contains("/src/").then(|| String::from(rel)))
            .expect("every subject was visited");
        assert_eq!(reached.subjects(), 3);
        assert!(reached.all(|one| one).is_none(), "one subject had no outcome");

        let every = Offered::over("reverted file", &all)
            .each(|_, rel| Some(String::from(rel)))
            .expect("every subject was visited")
            .all(|one| one)
            .expect("all three resolved");
        assert_eq!(every.subjects(), 3);
        assert_eq!(every.outcomes().len(), 3, "one outcome per subject, still");
    }
}
