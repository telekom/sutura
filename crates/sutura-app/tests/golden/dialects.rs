//! What a dialect decides, expanded over every one the compiler renders for.
//!
//! The catalog is `adapters::ReferenceCatalog` throughout, named once there rather than here: which
//! one it is does not matter to a renderer, and the catalog axis is what makes that true.

use std::collections::BTreeSet;

use sutura_semantic::{Compiled, Dialect, compile, dialect};

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
        sutura_domain::warehouse::ParamValue::Integer(v) => v.to_string(),
        sutura_domain::warehouse::ParamValue::Date(d) => d.to_iso(),
    }
}

/// The statement with every quoted identifier removed.
///
/// Searching the raw SQL for a value gives false positives, and one bit immediately: the
/// metric `web_revenue` has a required filter of `channel = 'web'`, the predicate is correctly
/// bound as `"orders"."channel" = ?`, and a plain substring search still found "web" - inside
/// the alias `AS "web_revenue"`. The value had not reached the statement at all.
///
/// Identifiers are double-quoted and a value inlined as text would be single-quoted, so
/// dropping the double-quoted spans leaves exactly the part of the statement a value could
/// have leaked into. It is a stronger check than looking for `'value'` would be: it also
/// catches a value inlined bare, without quotes.
///
/// Toggling on `"` is enough because an identifier here cannot contain one - `ColumnName` and
/// its siblings reject it, which is what makes that a fact rather than an assumption.
fn without_identifiers(sql: &str) -> String {
    let mut out = String::with_capacity(sql.len());
    let mut inside = false;
    for ch in sql.chars() {
        if ch == '"' {
            inside = !inside;
        } else if !inside {
            out.push(ch);
        }
    }
    out
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

/// The other half of not being an injection.
///
/// An identifier that reached the statement bare would bind to whatever the data system
/// decided, and a column called `order` would be a syntax error. Asserted over the corpus
/// rather than in one example.
fn quotes_every_identifier(dialect: Dialect) {
    let pinned = load::<ReferenceCatalog>();
    for path in questions() {
        let asked = read_question(&path);
        let compiled = compile(&asked, &pinned).expect("the corpus compiles");
        let Compiled::Planned { ref plan } = compiled else {
            continue;
        };
        let query = sql_for(plan, dialect);
        assert!(
            query.sql().contains("\"orders\""),
            "{} for {dialect} does not quote its table:\n{}",
            stem(&path),
            query.sql()
        );
    }
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
fn every_dialect_the_compiler_renders_for_is_registered() {
    // `dialect::ALL` is the compiler's own list and the registry is this suite's. A variant added
    // to the compiler with no line in the registry would be a target that renders with no golden,
    // which reads as covered and is not. Compared as sets rather than by counting, so a variant
    // swapped for another is caught too.
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
        "the compiler renders for a dialect the golden suite does not cover; add a line to \
         `adapters::registered` and re-run with INSTA_UPDATE=always"
    );
}
