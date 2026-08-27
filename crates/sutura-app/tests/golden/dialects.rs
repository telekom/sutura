//! What a dialect decides, expanded over every one `sutura-sql` renders for.
//!
//! The catalog is `adapters::ReferenceCatalog` throughout, named once there rather than here: which
//! one it is does not matter to a renderer, and the catalog axis is what makes that true.

use std::collections::BTreeSet;

use sutura_domain::catalog::TIME_BUCKET_LABEL;
use sutura_semantic::{Compiled, compile};
use sutura_sql::{Dialect, dialect};

use crate::adapters::{ReferenceCatalog, load, questions, read_question, stem};

use crate::shared::{settings, sql_for};

/// The statement and the values bound to it.
fn pins_the_statement_and_its_parameters(dialect: Dialect) {
    let pinned = load::<ReferenceCatalog>();
    for path in questions() {
        let asked = read_question(&path);
        let name = stem(&path);
        let compiled = compile(&asked, &pinned).unwrap_or_else(|e| panic!("{name} would not compile: {e}"));
        let Compiled::Planned { ref plan } = compiled else {
            continue;
        };
        let query = sql_for(plan, dialect);
        settings(dialect.as_str()).bind(|| {
            insta::assert_snapshot!(format!("{name}__sql"), query.sql());
            insta::assert_yaml_snapshot!(format!("{name}__params"), query.params());
        });
    }
}

/// A parameter's value as text, for comparing against what a question carried.
fn bound_value(param: &sutura_domain::warehouse::ParamValue) -> String {
    match *param {
        sutura_domain::warehouse::ParamValue::Text(ref v) => v.clone(),
        sutura_domain::warehouse::ParamValue::Date(d) => d.to_iso(),
    }
}

/// The statement with every span delimited by `delimiter` removed, delimiters included.
///
/// One toggle, parameterised, because there are two spans worth dropping and the two claims below
/// want DIFFERENT ones dropped - see [`without_identifiers`] and [`without_string_literals`]. Two
/// hand-written copies of this loop would be two things to keep in step for no gain; one
/// implementation cannot drift from itself.
///
/// A single toggle is enough for either delimiter because an identifier here is `[A-Za-z0-9_]` and
/// nothing else - `parse_identifier` in `sutura-domain` rejects every other character, which is
/// what makes that a fact rather than an assumption - so neither a `"` nor a `'` can occur inside
/// a name. It also handles SQL's doubled `''` escape with no special case: the pair flips out of
/// the literal and straight back into it, which drops the same characters either way.
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

/// The statement with every quoted identifier removed.
///
/// Searching the raw SQL for a value gives false positives, and one bit for real. **The catalog it
/// happened under has been deleted and the record is kept on purpose**, because the false positive
/// is a property of substring search rather than of that catalog: the e-commerce metric
/// `web_revenue` had a required filter of `channel = 'web'`, the predicate was correctly bound as
/// `"orders"."channel" = ?`, and a plain substring search still found "web" - inside the alias
/// `AS "web_revenue"`. The value had not reached the statement at all, and what the test was
/// reading was the metric's own name.
///
/// **Nothing in the telco vocabulary collides that way today, which is why the pass stays rather
/// than why it could go.** `recurring_revenue` binds `status = 'active'`, and it takes only one
/// metric alias containing the word - `active_subscriptions` is already there, in statements about
/// itself - or one dimension value that is a substring of a column name, to put the false positive
/// straight back. The mechanism is what protects the claim; a corpus that happens not to collide is
/// not.
///
/// Identifiers are double-quoted and a value inlined as text would be single-quoted, so
/// dropping the double-quoted spans leaves exactly the part of the statement a value could
/// have leaked into. It is a stronger check than looking for `'value'` would be: it also
/// catches a value inlined bare, without quotes.
///
/// **The single-quoted spans STAY, and keeping this pass that narrow is the point.** They are
/// exactly where a leaked value would be sitting, so a version of this that dropped them would
/// delete the evidence rather than the noise: `recurring-revenue-annual-in-north` written as
/// `= 'north'` instead of bound would leave nothing behind to search for, and
/// [`binds_every_value_rather_than_writing_it`] would report success on a generator that had
/// stopped binding. [`without_string_literals`] drops them for the one assertion that has to look
/// PAST a literal rather than at it, layered on top of this rather than folded into it. Two
/// haystacks, because the two claims want opposite things from the same characters.
fn without_identifiers(sql: &str) -> String {
    without_spans(sql, '"')
}

/// The statement with the quoted identifiers **and** the single-quoted string literals removed.
///
/// Layered on [`without_identifiers`], and read by [`quotes_every_identifier`] alone.
///
/// **The time bucket's grain reaches the statement as a string literal, and a grain keyword is a
/// perfectly good column name.** `generate` renders the bucket as `DATE_TRUNC(<unit>, <column>)`
/// with the unit written by `generate::unit`, so a month-grain question renders
/// `DATE_TRUNC('month', "fct_subscription_monthly"."month")` - and `'month'` there is a STRING
/// LITERAL, an argument to a function, not an identifier. The statement is correct: the column
/// beside it is quoted.
///
/// What goes wrong is the SEARCH. A model that declares a column called `month` puts `month` into
/// the set of names [`quotes_every_identifier`] looks for; `'` is not a word character, so
/// [`appears_bare`] matches the grain keyword and the test reports an unquoted identifier the
/// generator never emitted. **This is live rather than hypothetical, and it is why the pass was
/// written before the corpus moved:** the telco catalog under `examples/single-player` declares
/// `month` on its `subscriptions` model, so every question at month grain renders
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
/// Folding this pass into [`without_identifiers`] would fix the false positive here and silently
/// destroy the no-injection guarantee there, which is the trade this arrangement exists to refuse.
///
/// Applied on top of the pass above rather than as an independent strip of the raw SQL. Neither
/// delimiter can appear inside an identifier (see [`without_spans`]), so the order does not change
/// the result today; composing this way makes that independence something a reader can see instead
/// of something to re-derive.
fn without_string_literals(sql: &str) -> String {
    without_spans(&without_identifiers(sql), '\'')
}

/// The mechanical form of the no-injection claim, over the whole corpus.
///
/// Every value that ends up in a predicate must arrive as a bind parameter, so none of them
/// may appear in the SQL - and there are two sources of them. Written against `Query::literals`
/// and `QueryPlan::definitional_params` rather than as a list of places to look, so a field
/// added to either cannot quietly stop being covered.
fn binds_every_value_rather_than_writing_it(dialect: Dialect) {
    let pinned = load::<ReferenceCatalog>();
    let mut checked = 0_usize;
    let mut with_definitional = 0_usize;
    for path in questions() {
        let asked = read_question(&path);
        let literals = asked.literals();
        let compiled = compile(&asked, &pinned).expect("the corpus compiles");
        let Compiled::Planned { ref plan } = compiled else {
            continue;
        };
        let query = sql_for(plan, dialect);

        // Searched with the quoted identifiers removed: see `without_identifiers`.
        let searchable = without_identifiers(query.sql());

        // What the CALLER sent. The classic injection: a filter value or a date written into
        // the statement instead of bound.
        for literal in &literals {
            assert!(
                !searchable.contains(literal.as_str()),
                "{} for {dialect} carries the question's literal {literal:?}:\n{}",
                stem(&path),
                query.sql()
            );
        }

        // What the CATALOG said. A required filter's value is not caller text, so inlining it
        // would not be an injection today - it would be a generator with one inlining path and
        // one binding path, and the inlining path is the one that eventually gets handed
        // caller text. Asserting it here is what keeps there being only one path.
        let definitional: Vec<String> = plan.definitional_params().into_iter().map(bound_value).collect();
        for value in &definitional {
            // A date bound is definitional too, and its ISO text is checked above. Skip the
            // ones the question also carries so the message cannot be misattributed.
            if literals.contains(value) {
                continue;
            }
            assert!(
                !searchable.contains(value.as_str()),
                "{} for {dialect} carries the catalog's value {value:?}:\n{}",
                stem(&path),
                query.sql()
            );
            with_definitional = with_definitional.saturating_add(1);
        }

        // The values are still there, as parameters. Without this half a generator that
        // dropped the predicate entirely would pass everything above.
        let bound: BTreeSet<String> = query.params().iter().map(bound_value).collect();
        for literal in &literals {
            assert!(
                bound.contains(literal),
                "{} for {dialect} did not bind the question's {literal:?}; \
                 the predicate is missing rather than inlined:\n{}",
                stem(&path),
                query.sql()
            );
        }
        checked = checked.saturating_add(1);
    }
    assert!(checked > 0, "the corpus produced no statements to check");
    // The corpus has to contain at least one metric with a required filter, or the second half
    // of this test is asserting over an empty set and would pass with the feature removed.
    assert!(
        with_definitional > 0,
        "no question exercised a required filter; the definitional half of this test proved nothing"
    );
}

/// Whether `needle` appears in `haystack` as a bare word rather than inside a longer one.
///
/// Word-bounded rather than a plain substring, and the boundary is what makes the assertion below
/// precise instead of merely alarming. With the quoted spans removed the statement still holds
/// `DATE_TRUNC`, `CAST`, `DOUBLE` and `DISTINCT`, so a model column called `date` or a metric
/// called `cast` would match a substring search that no bare identifier caused. An identifier
/// emitted unquoted is surrounded by whitespace, a comma or a parenthesis, which is exactly what
/// this admits.
fn appears_bare(haystack: &str, needle: &str) -> bool {
    let word = |ch: char| ch == '_' || ch.is_ascii_alphanumeric();
    haystack.match_indices(needle).any(|(at, _)| {
        let before = haystack.get(..at).and_then(|head| head.chars().next_back());
        let after = haystack
            .get(at.saturating_add(needle.len())..)
            .and_then(|tail| tail.chars().next());
        !before.is_some_and(word) && !after.is_some_and(word)
    })
}

/// The other half of not being an injection, asserted as the property rather than as one example.
///
/// An identifier that reached the statement bare would bind to whatever the data system decided,
/// and a column called `order` would be a syntax error at that data system instead of an error
/// here.
///
/// **What this used to assert, and why it proved almost nothing.** The body was one
/// `contains("\"orders\"")` - the table name appears quoted somewhere - which every question in
/// every dialect satisfies for as long as the generator quotes anything at all. It was blind to the
/// bug `generate`'s own module doc singles out: `always_quote_identifiers` covers identifiers and
/// does **not** cover aliases, so `generate::aliased` sets the alias's own `quoted` flag by hand.
/// Deleting that line left this test green.
///
/// So the claim is made over [`without_string_literals`], which layers on the mechanism
/// `no_value_reaches_the_statement_as_text` already trusts: dropping the double-quoted spans leaves
/// exactly the part of the statement an identifier could have leaked into. Every model column,
/// every dimension label, every metric name and the time bucket's label has to be absent from what
/// is left - the aliases included, which is the half the old body could not see.
///
/// The positive half is what stops it passing on a generator that emits no identifiers at all: the
/// names this plan actually uses have to be present, quoted, in the raw statement.
///
/// It reads [`without_string_literals`] and not [`without_identifiers`] directly, and the extra
/// pass is not tidiness. The bucket's grain keyword is a single-quoted STRING LITERAL that can
/// legitimately equal a column name, so `'month'` inside `DATE_TRUNC('month', ...)` looks exactly
/// like a bare `month` to the search below. This is the one place where this claim and the value
/// claim need DIFFERENT haystacks, and [`without_string_literals`] carries the reason they cannot
/// share one.
fn quotes_every_identifier(dialect: Dialect) {
    let pinned = load::<ReferenceCatalog>();
    let definitions = pinned.definitions();

    // Every name the generator could have to quote, whether or not this question reaches it. Read
    // off the catalog rather than off the plan, so a name the renderer emits for a reason the plan
    // does not record is covered too.
    let mut names: BTreeSet<String> = BTreeSet::from([String::from(TIME_BUCKET_LABEL)]);
    for model in definitions.models().values() {
        names.insert(String::from(model.table().as_str()));
        names.extend(model.columns().iter().map(|column| String::from(column.as_str())));
    }
    for metric in definitions.metrics().values() {
        names.insert(String::from(metric.name().as_str()));
        names.extend(metric.dimensions().keys().map(|label| String::from(label.as_str())));
    }
    assert!(
        names.len() > 4,
        "the example catalog names almost nothing, so this test would prove nothing: {names:?}"
    );

    let mut quoted_names_checked = 0_usize;
    for path in questions() {
        let asked = read_question(&path);
        let compiled = compile(&asked, &pinned).expect("the corpus compiles");
        let Compiled::Planned { ref plan } = compiled else {
            continue;
        };
        let query = sql_for(plan, dialect);
        let stripped = without_string_literals(query.sql());

        for name in &names {
            assert!(
                !appears_bare(&stripped, name.as_str()),
                "{} for {dialect} carries the identifier {name:?} unquoted:\n{}\nwith the quoted \
                 spans and the string literals removed:\n{stripped}",
                stem(&path),
                query.sql()
            );
        }

        // And the names this plan does use are there, quoted. Without this half a generator that
        // emitted no projection at all would satisfy everything above.
        let mut quoted = vec![String::from(plan.table().as_str())];
        quoted.extend(plan.joins().iter().map(|join| String::from(join.table().as_str())));
        quoted.push(String::from(plan.bucket().label()));
        quoted.push(String::from(plan.measure_label()));
        quoted.extend(plan.keys().iter().map(|key| String::from(key.label())));
        for name in &quoted {
            assert!(
                query.sql().contains(&format!("\"{name}\"")),
                "{} for {dialect} does not carry {name:?} quoted:\n{}",
                stem(&path),
                query.sql()
            );
            quoted_names_checked = quoted_names_checked.saturating_add(1);
        }
    }
    assert!(quoted_names_checked > 0, "the corpus produced no statements to check");
}

/// The check that replaces having one of each data system in CI.
///
/// It PARSES and stops: it never re-emits, so it cannot introduce the parser-differential
/// problem that makes translation unusable on the query path. A failure here means we
/// generated something that is not valid SQL for that target, which is otherwise only
/// discoverable by running it. The parse target is paired with the dialect in the registry, so
/// a golden cannot render for one and parse-check against another.
fn parses_in_the_dialect_it_was_generated_for(dialect: Dialect, target: polyglot_sql::DialectType) {
    let pinned = load::<ReferenceCatalog>();
    for path in questions() {
        let asked = read_question(&path);
        let compiled = compile(&asked, &pinned).expect("the corpus compiles");
        let Compiled::Planned { ref plan } = compiled else {
            continue;
        };
        let query = sql_for(plan, dialect);
        let parsed = polyglot_sql::parse(query.sql(), target);
        assert!(
            parsed.is_ok(),
            "{} for {dialect} is not valid there: {:?}\n{}",
            stem(&path),
            parsed.err(),
            query.sql()
        );
    }
}

/// One cell of the dialect axis.
macro_rules! cell {
    ($name:ident, $dialect:expr, $target:expr) => {
        mod $name {
            #[test]
            fn the_corpus_renders_and_every_statement_is_pinned() {
                super::pins_the_statement_and_its_parameters($dialect);
            }

            #[test]
            fn no_value_reaches_the_statement_as_text() {
                super::binds_every_value_rather_than_writing_it($dialect);
            }

            #[test]
            fn a_generated_statement_quotes_every_identifier() {
                super::quotes_every_identifier($dialect);
            }

            #[test]
            fn every_generated_statement_parses_here() {
                super::parses_in_the_dialect_it_was_generated_for($dialect, $target);
            }
        }
    };
}

crate::adapters::registered!(dialects: cell);

#[test]
fn every_dialect_the_renderer_supports_is_registered() {
    // `dialect::ALL` is `sutura-sql`'s own list and the registry is this suite's. A variant added
    // there with no line in the registry would be a target that renders with no golden, which reads
    // as covered and is not. Compared as sets rather than by counting, so a variant swapped for
    // another is caught too.
    //
    // Named for the RENDERER and not for the compiler, which is what it used to say: `compile`
    // stops at a plan and names no dialect at all, and since rendering moved into its own crate the
    // list this compares against is not the compiler's to grow.
    //
    // The cell collects into a local, which is why it is defined inside this function: a
    // `macro_rules!` body resolves a local at its own definition site, and here that site is this
    // scope.
    let mut registered: Vec<Dialect> = Vec::new();
    macro_rules! cell {
        ($name:ident, $dialect:expr, $target:expr) => {
            registered.push($dialect);
        };
    }
    crate::adapters::registered!(dialects: cell);

    registered.sort_unstable();
    let mut all: Vec<Dialect> = dialect::ALL.to_vec();
    all.sort_unstable();
    assert_eq!(
        registered, all,
        "`sutura-sql` renders for a dialect the golden suite does not cover; add a line to \
         `adapters::registered` and re-run with INSTA_UPDATE=always"
    );
}
