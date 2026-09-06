//! The two numbers this gate's verdict names, and the type that keeps them honest.
//!
//! `github.com/telekom/sutura#402`'s finding about its predecessor was not that a count was
//! wrong. It was that `11 phrase rule(s)` was **compared to nothing**, so deleting both
//! registrations printed `9 phrase rule(s)` and exit 0. A number a reader can see is not a
//! control; a number read off the set that was judged is.
//!
//! So `discovered` is captured from the discovery pass BEFORE anything is judged, `rows` gets one
//! entry per body the judge reached, and the sentence reads both off the same value. The fields
//! are private and [`Judged::of`] is the only constructor, so no caller can name a denominator it
//! did not count. [`unread`] is the refusal when the two disagree, taken as a free function over
//! the numbers precisely so a test can provoke it without a door into `Judged` that production
//! code could reach for.

use super::scan::Value;

/// A shell body a devenv module hands to bash.
pub(super) struct Body {
    /// Repo-relative module it is in.
    pub(super) module: String,
    /// 1-based line of the `=`.
    pub(super) line: usize,
    /// The whole assigned path, for the message.
    pub(super) path: String,
    /// The last segment - the devenv option this is.
    pub(super) attribute: String,
    /// What the value begins with.
    pub(super) value: Value,
    /// Why the discovery pass picked it up. Printed, because a rule whose reason is unstated gets
    /// reverted - and because the two reasons have different limits.
    pub(super) because: &'static str,
}

/// What this gate concluded about one body.
pub(super) enum Held {
    /// Its value is an application of a binding that reaches `linted`.
    Routed(String),
    /// It is not, and this is what a reader has to fix.
    Loose,
}

/// One body and its verdict.
pub(super) struct Row {
    /// The body.
    pub(super) body: Body,
    /// What was concluded about it.
    pub(super) held: Held,
}

/// Every shell-bearing assignment the discovery pass found.
///
/// A newtype over the vector so the number below cannot be taken from anywhere else.
pub(super) struct Discovered(Vec<Body>);

impl Discovered {
    /// The bodies the discovery pass produced.
    pub(super) const fn of(bodies: Vec<Body>) -> Self {
        Self(bodies)
    }

    /// Nothing found at all - the empty-scan floor its caller refuses on.
    pub(super) const fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// The judged set: the denominator, and one row per body that reached a verdict.
pub(super) struct Judged {
    /// How many bodies the discovery pass handed over, read before the first judgement.
    discovered: usize,
    /// One entry per body this gate reached a verdict about.
    rows: Vec<Row>,
}

impl Judged {
    /// Judge every discovered body.
    ///
    /// The loop is here rather than in the caller so the denominator and the numerator are
    /// produced in one place: `discovered` is the length of the set that came in, and a `continue`
    /// or a `filter` added to this loop makes them disagree, which [`unread`] refuses.
    pub(super) fn of(found: Discovered, judge: impl Fn(&Body) -> Held) -> Self {
        let Discovered(bodies) = found;
        let discovered = bodies.len();
        let mut rows = Vec::with_capacity(discovered);
        for body in bodies {
            let held = judge(&body);
            rows.push(Row { body, held });
        }
        Self { discovered, rows }
    }

    /// Every body this gate reached a verdict about.
    pub(super) fn rows(&self) -> &[Row] {
        &self.rows
    }

    /// `inspected of discovered`, both read off this value.
    pub(super) fn sentence(&self) -> String {
        format!("{} of {} shell body(s)", self.rows.len(), self.discovered)
    }

    /// The refusal when the judge did not reach every discovered body.
    pub(super) fn gap(&self) -> Option<String> {
        unread(self.discovered, self.rows.len())
    }
}

/// The refusal when a scan judged fewer bodies than it found.
///
/// A free function over the two numbers, for the reason the module header gives: it is the one
/// arm of this gate that cannot be provoked through the real path, because [`Judged::of`] maps
/// every element. Keeping it callable makes the arm testable without giving production code a way
/// to build a `Judged` whose numbers were never counted.
pub(super) fn unread(discovered: usize, inspected: usize) -> Option<String> {
    if inspected >= discovered {
        return None;
    }
    Some(format!(
        "reached a verdict about {inspected} of the {discovered} shell body(s) found - the scan \
         dropped {}, so this verdict is about less than the tree",
        discovered.saturating_sub(inspected)
    ))
}

#[cfg(test)]
mod tests {
    use super::{Body, Discovered, Held, Judged, unread};
    use crate::devenv_shell::scan::Value;

    fn body(attribute: &str) -> Body {
        Body {
            module: String::from("devenv.nix"),
            line: 1,
            path: String::from(attribute),
            attribute: String::from(attribute),
            value: Value::Literal { lines: 1 },
            because: "a test",
        }
    }

    #[test]
    fn the_sentence_reads_both_numbers_off_the_set_it_judged() {
        let judged = Judged::of(Discovered::of(vec![body("a"), body("b"), body("c")]), |_| Held::Loose);
        assert_eq!(judged.sentence(), "3 of 3 shell body(s)");
        assert!(judged.gap().is_none(), "every body was judged");
        assert_eq!(judged.rows().len(), 3);
    }

    #[test]
    fn a_dropped_body_is_a_refusal_rather_than_a_smaller_number() {
        // The shape #402 measured on the gate this replaces: a denominator nothing compares. Here
        // the two numbers are compared, so a scan that judged four of five refuses.
        let refusal = unread(5, 4).expect("a short scan refuses");
        assert!(refusal.contains("4 of the 5"), "{refusal}");
        assert!(refusal.contains("dropped 1"), "{refusal}");
        assert!(unread(5, 5).is_none());
    }

    #[test]
    fn an_empty_set_judges_nothing_and_says_so() {
        let judged = Judged::of(Discovered::of(Vec::new()), |_| Held::Loose);
        assert_eq!(judged.sentence(), "0 of 0 shell body(s)");
        // The floor is the caller's - this only has to not pretend it measured something.
        assert!(judged.gap().is_none());
    }
}
