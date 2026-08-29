//! Helpers every axis of the golden suite shares.
//!
//! In `tests/golden/` rather than beside the target, because `tests/*.rs` at the top level is a
//! test target and this is not one.

use sutura_domain::query::Query;
use sutura_domain::warehouse::{ParamValue, RowSet, Value};
use sutura_sql::{Dialect, PlaceholderStyle};

use crate::adapters::{questions, read_question};

/// Settings every snapshot in this suite uses.
///
/// The snapshot path is set explicitly so the files land in `tests/snapshots/` next to the corpus
/// rather than wherever the macro would guess from the module path - the axes are modules under
/// `tests/golden/`, so the path is relative to there - and the module prefix is
/// dropped so a snapshot is named after the question and the adapter rather than after the nesting
/// the matrix produces.
///
/// An empty suffix is left unset rather than set to nothing, because insta writes the separator
/// whenever a suffix is present and `anchor_mismatch@.snap` is not a name anybody wants to read.
pub(crate) fn settings(suffix: &str) -> insta::Settings {
    let mut settings = insta::Settings::clone_current();
    settings.set_snapshot_path("../snapshots");
    settings.set_prepend_module_to_snapshot(false);
    if !suffix.is_empty() {
        settings.set_snapshot_suffix(suffix);
    }
    settings
}

/// Renders a plan, because compiling no longer does.
///
/// The dialect is named at the call site now rather than passed into `compile`, which is the point
/// of the split: a golden that pins SQL is asking for a rendering, and says so. It is also a
/// different CRATE now - `sutura-sql`, a dev-dependency here - so a test that wants SQL declares
/// that it wants SQL and nothing in `src/` pulls a generator in on its behalf.
pub(crate) fn sql_for(plan: &sutura_domain::plan::QueryPlan, dialect: Dialect) -> sutura_sql::GeneratedQuery {
    sutura_sql::generate(plan, dialect).expect("a planned question renders")
}

/// An error and every cause beneath it, outermost first, as one block.
///
/// Snapshotted rather than asserted with `contains`, for the reason the service walks the chain at
/// all: `Display` on a `thiserror` enum prints the outermost message and stops, and the outermost
/// message here is "the data system did not answer", which names nothing. What a reader needs is
/// the column and which of the three non-finite values it was, and both of those live one and two
/// levels down.
pub(crate) fn chain(error: &dyn core::error::Error) -> String {
    let mut out = error.to_string();
    let mut cursor = error.source();
    while let Some(cause) = cursor {
        out.push_str("\n  caused by: ");
        out.push_str(&cause.to_string());
        cursor = cause.source();
    }
    out
}

/// One cell, with its type kept and a float cut to twelve significant digits.
///
/// **The truncation is what makes a row snapshot per data system worth having.** Summing the same
/// rows in a different order changes the last place of an `f64`, so pinning the full binary
/// expansion asserts WHICH ENGINE RAN - and the snapshot then goes red on an upstream version bump
/// that altered no number anybody reports. Twelve digits is far beyond any figure a metric carries
/// and far short of the noise. The variant is kept, so a measure that changed from an exact integer
/// to a float is still a diff.
fn cell(value: &Value) -> String {
    match *value {
        Value::Null => String::from("Null"),
        Value::Integer(v) => format!("Integer({v})"),
        Value::Real(v) => format!("Real({v:.12e})"),
        Value::Text(ref v) => format!("Text({v})"),
    }
}

/// A result set as a snapshot: the labels, then one line per row.
///
/// Text rather than the serialized `RowSet`, because what a reader checks is the shape and the
/// numbers, and a tab-separated block is the form the CLI already prints them in.
pub(crate) fn stable(rows: &RowSet) -> String {
    let mut out = rows.columns().join("\t");
    for row in rows.rows() {
        out.push('\n');
        out.push_str(&row.iter().map(cell).collect::<Vec<String>>().join("\t"));
    }
    out
}

/// One question by the name of its file.
///
/// Found in `questions` rather than joined onto a path, so a fixture that was renamed fails saying
/// so instead of failing as a missing file two frames further in.
pub(crate) fn question(file: &str) -> Query {
    let path = questions()
        .into_iter()
        .find(|path| path.file_name().is_some_and(|name| name == file))
        .unwrap_or_else(|| panic!("there is no question fixture called {file}"));
    read_question(&path)
}

/// Every refusal variant a question file can provoke, and the fixture that provokes it.
///
/// A table rather than a test each, so the exhaustiveness assertion can be written against it.
///
/// Three variants are absent on purpose, because no question file can reach one, and each has its
/// own test in `golden/service.rs` instead. `PlanSpansTwoSources` needs a catalog naming two data
/// systems and `SourceUnavailable` a plan for a data system nobody opened - both decided above the
/// port, by the service rather than by the compiler. `ResultTooLarge` is the third and is absent for
/// a different reason worth keeping straight: it is decided AFTER a data system has answered, and
/// `a_refused_question_never_reaches_the_data_system` asserts of every entry in this table that
/// nothing ran. A fixture that provoked it here would make that assertion false.
pub(crate) const PROVOKED: &[(&str, &str)] = &[
    ("refused-metric-unknown", "MetricUnknown"),
    ("refused-grain-not-supported", "GrainNotSupported"),
    ("refused-dimension-not-permitted", "DimensionNotPermitted"),
    ("refused-dimension-not-filterable", "DimensionNotFilterable"),
    ("refused-value-not-allowed", "DimensionValueNotAllowed"),
    ("refused-duplicate-dimension", "DuplicateDimension"),
    ("refused-too-many-dimensions", "TooManyDimensions"),
    ("refused-range-too-long", "TimeRangeTooLong"),
];

/// A parameter's value as text, for comparing against what a question or a leg carried.
///
/// Deliberately not `ParamValue::render`, and the difference is the finding this module was
/// rewritten for: `render` is display-only and quotes text the way `Debug` does, so a search built
/// on it would look for `"north"` where the value is `north`. A value that reaches a statement
/// carrying those quotes is the defect, not the thing to search for.
pub(crate) fn bound_value(param: &ParamValue) -> String {
    match *param {
        ParamValue::Text(ref v) => v.clone(),
        ParamValue::Date(d) => d.to_iso(),
    }
}

/// The statement with every span delimited by `delimiter` removed, delimiters included.
///
/// One toggle, parameterised, because there are two spans worth dropping and the claims that read
/// this module want DIFFERENT ones dropped - see `without_identifiers` and
/// `without_string_literals`. Two hand-written copies of this loop would be two things to keep in
/// step for no gain; one implementation cannot drift from itself.
///
/// A single toggle is enough for either delimiter because an identifier here is `[A-Za-z0-9_]` and
/// nothing else - `parse_identifier` in `sutura-domain` rejects every other character, which is
/// what makes that a fact rather than an assumption - so neither a `"` nor a `'` can occur inside
/// a name. It also handles SQL's doubled `''` escape with no special case: the pair flips out of
/// the literal and straight back into it, which drops the same characters either way.
///
/// **That argument is about ESCAPING and licenses nothing about what may be dropped.** It says the
/// toggle finds the right span boundaries; it does not say the span behind those boundaries is an
/// identifier. `without_identifiers` used to be read as if it did, and the section below is what
/// that cost.
fn without_spans(sql: &str, delimiter: char) -> String {
    let mut out = String::with_capacity(sql.len());
    let mut inside = false;
    for ch in sql.chars() {
        if ch == delimiter {
            inside = !inside;
        } else if !inside {
            out.push(ch);
        }
    }
    out
}

/// The statement with every double-quoted span removed.
///
/// **A haystack for a substring search, and never the only search.** What it buys is one real false
/// positive, and the catalog it happened under has been deleted while the record is kept on purpose,
/// because the false positive is a property of substring search rather than of that catalog: the
/// e-commerce metric `web_revenue` had a required filter of `channel = 'web'`, the predicate was
/// correctly bound as `"orders"."channel" = ?`, and a plain substring search still found "web" -
/// inside the alias `AS "web_revenue"`. The value had not reached the statement at all, and what the
/// test was reading was the metric's own name.
///
/// **Nothing in the telco vocabulary collides that way today, which is why the pass stays rather
/// than why it could go.** `recurring_revenue` binds `status = 'active'`, and it takes only one
/// metric alias containing the word - `active_subscriptions` is already there, in statements about
/// itself - or one dimension value that is a substring of a column name, to put the false positive
/// straight back. The mechanism is what protects the claim; a corpus that happens not to collide is
/// not.
///
/// **What it costs, measured rather than reasoned about, and the reason `assert_absent_as_text`
/// searches twice.** A value the generator wrapped in double quotes is inside a dropped span, so it
/// is gone from this haystack before any search runs. That is not hypothetical: forced identifier
/// quoting is on, so `builder::col("active")` renders `"active"`, and a mutation that replaced a
/// predicate's placeholder with exactly that produced
/// `"fct_subscription_monthly"."status" = "active"` in all three dialects while every no-injection
/// test in this suite reported success. `parse_identifier` is what makes the case unfixable from
/// inside this function: it accepts `[A-Za-z_][A-Za-z0-9_]*`, and `active` and `north` are both of
/// that shape, so no test of a span's CONTENTS can tell an identifier from a value that was written
/// where one goes.
///
/// **The single-quoted spans STAY, and keeping this pass that narrow is the point.** They are
/// exactly where a leaked value would be sitting, so a version of this that dropped them would
/// delete the evidence rather than the noise: `recurring-revenue-annual-in-north` written as
/// `= 'north'` instead of bound would leave nothing behind to search for.
/// `without_string_literals` drops them for the one assertion that has to look PAST a literal
/// rather than at it, layered on top of this rather than folded into it. Two haystacks, because the
/// two claims want opposite things from the same characters.
pub(crate) fn without_identifiers(sql: &str) -> String {
    without_spans(sql, '"')
}

/// The statement with the quoted identifiers **and** the single-quoted string literals removed.
///
/// Layered on `without_identifiers`, and read by `dialects::quotes_every_identifier` and by
/// `placeholders` below.
///
/// **The time bucket's grain reaches the statement as a string literal, and a grain keyword is a
/// perfectly good column name.** `generate` renders the bucket as `DATE_TRUNC(<unit>, <column>)`
/// with the unit written by `generate::unit`, so a month-grain question renders
/// `DATE_TRUNC('month', "fct_subscription_monthly"."month")` - and `'month'` there is a STRING
/// LITERAL, an argument to a function, not an identifier. The statement is correct: the column
/// beside it is quoted.
///
/// What goes wrong is the SEARCH. A model that declares a column called `month` puts `month` into
/// the set of names the identifier claim looks for; `'` is not a word character, so `appears_bare`
/// matches the grain keyword and that test reports an unquoted identifier the generator never
/// emitted. **This is live rather than hypothetical, and it is why the pass was written before the
/// corpus moved:** the telco catalog under `examples/single-player` declares `month` on its
/// `subscriptions` model, so every question at month grain renders
/// `DATE_TRUNC('month', "fct_subscription_monthly"."month")`. The e-commerce fixture this crate used
/// to read named no column after a grain, which is the only reason the hole stayed latent for as
/// long as it did - and every unit `Grain` has (`day`, `week`, `month`, `quarter`, `year`) is a name
/// a modeller could reasonably pick. A catalog is data, so "no catalog names a column after a grain"
/// is not something this suite gets to assume.
///
/// **So the identifier claim gets its own haystack and the value claim keeps the one it had.** They
/// are not the same claim. The value test asks whether text reached the statement AT ALL, and a
/// string literal is precisely where such text would be sitting; the identifier test asks whether a
/// NAME reached it unquoted, and a string literal is somewhere a name cannot have leaked to.
/// Folding this pass into `without_identifiers` would fix the false positive here and silently
/// destroy the no-injection guarantee there, which is the trade this arrangement exists to refuse.
///
/// Applied on top of the pass above rather than as an independent strip of the raw SQL. Neither
/// delimiter can appear inside an identifier (see `without_spans`), so the order does not change
/// the result today; composing this way makes that independence something a reader can see instead
/// of something to re-derive.
pub(crate) fn without_string_literals(sql: &str) -> String {
    without_spans(&without_identifiers(sql), '\'')
}

/// Whether `needle` appears in `haystack` as a bare word rather than inside a longer one.
///
/// Word-bounded rather than a plain substring, and the boundary is what makes an assertion over
/// this precise instead of merely alarming. With the quoted spans removed a statement still holds
/// `DATE_TRUNC`, `CAST`, `DOUBLE` and `DISTINCT`, so a model column called `date` or a metric called
/// `cast` would match a substring search that no bare identifier caused. An identifier emitted
/// unquoted is surrounded by whitespace, a comma or a parenthesis, which is exactly what this
/// admits.
///
/// **It is also what makes the second value search usable on the RAW statement**, and that is a
/// second job rather than a coincidence: `"` is not a word character, so a value the generator
/// wrapped in double quotes reads as a bare word here, while a value that is merely a PREFIX of a
/// quoted name - `web` inside `"web_revenue"`, `active` inside `"active_subscriptions"` - does not.
/// The false positive `without_identifiers` exists to avoid is exactly the one a word boundary
/// already rejects.
pub(crate) fn appears_bare(haystack: &str, needle: &str) -> bool {
    let word = |ch: char| ch == '_' || ch.is_ascii_alphanumeric();
    haystack.match_indices(needle).any(|(at, _)| {
        let before = haystack.get(..at).and_then(|head| head.chars().next_back());
        let after = haystack
            .get(at.saturating_add(needle.len())..)
            .and_then(|tail| tail.chars().next());
        !before.is_some_and(word) && !after.is_some_and(word)
    })
}

/// The one-based parameter positions the statement's bind placeholders name, sorted.
///
/// Read off the DIALECT rather than guessed from the text: `PlaceholderStyle` is `sutura-sql`'s own
/// decision about how a target writes a bind parameter, and the match below is exhaustive with no
/// wildcard arm, so a fourth dialect with a third style is a compile error here rather than a
/// statement this counts as carrying no placeholders at all.
///
/// Counted with the quoted spans and the string literals gone. An identifier cannot contain `?` or
/// `$` - `parse_identifier` allows `[A-Za-z0-9_]` only - but a string literal can, and the grain
/// keyword is proof that literals reach the statement.
///
/// Sorted rather than left in textual order, because what the assertion needs is that each
/// parameter is named exactly once. Demanding they also appear in ascending order would fail a
/// generator that legitimately reordered a `WHERE` clause, and reordering is not the defect this is
/// aimed at.
pub(crate) fn placeholders(sql: &str, dialect: Dialect) -> Vec<usize> {
    let searchable = without_string_literals(sql);
    let mut found: Vec<usize> = Vec::new();
    match dialect.placeholder_style() {
        // Anonymous: the position IS the order of appearance, so the ordinal is the position.
        PlaceholderStyle::Question => {
            for (ordinal, _) in searchable.match_indices('?').enumerate() {
                found.push(ordinal.saturating_add(1));
            }
        }
        // Numbered: the statement says which parameter it wants, and a repeated `$1` is a different
        // query rather than a cosmetic difference - `sutura_sql::dialect` says so at the variant.
        PlaceholderStyle::Numbered => {
            let mut digits = String::new();
            let mut inside = false;
            // One trailing space, so a `$1` sitting at the very end of the statement is flushed by
            // the same branch as every other one instead of by a copy of it after the loop.
            for ch in searchable.chars().chain(core::iter::once(' ')) {
                if inside {
                    if ch.is_ascii_digit() {
                        digits.push(ch);
                        continue;
                    }
                    inside = false;
                    if let Ok(position) = digits.parse::<usize>() {
                        found.push(position);
                    }
                }
                if ch == '$' {
                    inside = true;
                    digits.clear();
                }
            }
        }
    }
    found.sort_unstable();
    found
}

/// Asserts that `value` did not reach `sql` as text, by two searches neither of which is enough.
///
/// **The negative half of the no-injection claim, and it is two searches because one of them was
/// blind to a whole class.** `origin` names where the value came from - a question's literal or a
/// catalog's required filter - so a failure cannot be misattributed.
///
/// **Search one:** a substring search with the quoted identifiers stripped. It catches a value
/// inlined into a string literal, a value inlined bare with no quotes at all, and a value that is
/// only PART of what reached the statement - `north` inside `'north%'`. `without_identifiers`
/// carries the false positive that shaped it.
///
/// **Search two:** a word-bounded search on the RAW statement. It exists because search one cannot
/// see a value the generator wrapped in double quotes: that span is dropped before the search runs,
/// and no test of the span's contents can help, because a value like `active` is a legal identifier
/// spelling. A mutation that rendered a predicate's value as a quoted identifier passed search one
/// in all three dialects.
///
/// **The limit, and it is the reason `assert_one_placeholder_per_parameter` exists rather than a
/// third search.** A value that word-boundedly EQUALS a name the statement quotes is invisible to
/// both searches: search one drops the span, and search two cannot tell `= "month"` from
/// `"fct_subscription_monthly"."month"`. `month` is a declared column in this corpus and a legal
/// filter value, so that is a real shape rather than a contrived one. What covers it is the
/// positive assertion, which never reads the value at all.
pub(crate) fn assert_absent_as_text(fixture: &str, dialect: Dialect, origin: &str, value: &str, sql: &str) {
    assert!(
        !without_identifiers(sql).contains(value),
        "{fixture} for {dialect} carries {origin} {value:?} outside a quoted identifier:\n{sql}"
    );
    assert!(
        !appears_bare(sql, value),
        "{fixture} for {dialect} carries {origin} {value:?} as a bare word, which is what a value \
         wrapped in double quotes looks like:\n{sql}"
    );
}

/// Asserts the statement names each of `params` bind parameters exactly once, and no others.
///
/// **The positive half of the no-injection claim, and it is the half that does not depend on a
/// haystack.** Every search for an absent value depends on the value's spelling; this counts
/// placeholders, so a value written into the statement instead of bound leaves a placeholder
/// missing whatever it spells - inside double quotes, inside a literal, or bare. That is what makes
/// it the mechanism for the case the two searches above state as their limit.
///
/// `params` is read off the PLAN rather than off the rendered query, so a generator that inlined a
/// value and also dropped it from its own parameter list is still one placeholder short.
///
/// For a numbered dialect it checks the numbering too, which a count could not: a statement with
/// `$1` repeated and `$2` absent binds the same value twice and is a different query.
pub(crate) fn assert_one_placeholder_per_parameter(fixture: &str, dialect: Dialect, params: usize, sql: &str) {
    let found = placeholders(sql, dialect);
    let expected: Vec<usize> = (1..=params).collect();
    assert_eq!(
        found, expected,
        "{fixture} for {dialect} does not name one bind parameter per value the plan carries; a \
         value written into the statement leaves a placeholder missing whatever it spells:\n{sql}"
    );
}

/// The placeholder scan, over statements written by hand rather than generated.
///
/// The corpus exercises the ordinary shape in all three dialects, so what this pins is the three
/// things it cannot reach - and each of them is a claim the doc comments above make in prose:
///
/// - **A repeated `$1`.** A count accepts it and a numbering check must not, because a statement
///   that binds one value twice and never binds the second is a different query.
/// - **A placeholder at the very end**, with no character after it. That is the case the sentinel
///   space in the scan exists for, and without it the last parameter of every Postgres statement
///   ending in `$n` would go uncounted.
/// - **A `?` inside a string literal.** Not a placeholder, which is why the scan runs over
///   `without_string_literals` rather than over the raw statement.
#[test]
fn the_placeholder_scan_reads_the_dialect_and_not_the_text() {
    // Anonymous: the ordinal IS the position, because that is how the data system counts them.
    assert_eq!(placeholders("a = ? AND b = ?", Dialect::DuckDb), vec![1, 2]);
    assert_eq!(placeholders("a = ? AND b = 'why?'", Dialect::DuckDb), vec![1]);
    assert_eq!(placeholders("a = ?", Dialect::ClickHouse), vec![1]);

    // Numbered: the statement names the parameter it wants.
    assert_eq!(placeholders("a = $1 AND b = $2", Dialect::Postgres), vec![1, 2]);
    assert_eq!(placeholders("a = $1", Dialect::Postgres), vec![1]);
    // Two digits, so the scan is not reading one character and stopping.
    assert_eq!(placeholders("a = $9 AND b = $10", Dialect::Postgres), vec![9, 10]);
    // Sorted, so the same parameter twice reads as `[1, 1]` - which is what
    // `assert_one_placeholder_per_parameter` compares against `[1, 2]` and refuses.
    assert_eq!(placeholders("a = $1 AND b = $1", Dialect::Postgres), vec![1, 1]);
}
