//! What a dialect decides, expanded over every one `sutura-sql` renders for.
//!
//! The catalog is `adapters::ReferenceCatalog` throughout, named once there rather than here: which
//! one it is does not matter to a renderer, and the catalog axis is what makes that true.

use std::collections::BTreeSet;
use sutura_domain::pinned::view::ScopedView;

use sutura_domain::catalog::TIME_BUCKET_LABEL;
use sutura_domain::model::Grain;
use sutura_semantic::{Compiled, compile};
use sutura_sql::{Dialect, dialect};

use crate::adapters::{ReferenceCatalog, load, questions, read_question, stem};

use crate::shared::{
    appears_bare, assert_absent_as_text, assert_one_placeholder_per_parameter, bound_value, settings, sql_for,
    without_string_literals,
};

/// The statement and the values bound to it.
fn pins_the_statement_and_its_parameters(dialect: Dialect) {
    let pinned = load::<ReferenceCatalog>();
    let mut checked = 0_usize;
    for path in questions() {
        let asked = read_question(&path);
        let name = stem(&path);
        let compiled = compile(
            &asked,
            &ScopedView::everything(&pinned),
            sutura_domain::plan::RowCeiling::DEFAULT,
        )
        .unwrap_or_else(|e| panic!("{name} would not compile: {e}"));
        let Compiled::Planned { ref plan } = compiled else {
            continue;
        };
        let query = sql_for(plan, dialect);
        settings(dialect.as_str()).bind(|| {
            insta::assert_snapshot!(format!("{name}__sql"), query.sql());
            insta::assert_yaml_snapshot!(format!("{name}__params"), query.params());
        });
        checked = checked.saturating_add(1);
    }
    assert!(checked > 0, "the corpus produced no statements to check");
}

/// The mechanical form of the no-injection claim, over the whole corpus.
///
/// Every value that ends up in a predicate must arrive as a bind parameter, so none of them
/// may appear in the SQL - and there are two sources of them. Written against `Query::literals`
/// and `QueryPlan::definitional_params` rather than as a list of places to look, so a field
/// added to either cannot quietly stop being covered.
///
/// **Three assertions, and the first of them is here because the other two were not enough.** They
/// live in `shared` so this axis and the leg axis cannot drift into two different opinions about
/// what the claim is; `shared::assert_absent_as_text` carries the searches and their limit, and
/// `shared::assert_one_placeholder_per_parameter` carries what covers that limit.
///
/// 1. **The statement names one bind parameter per value the plan carries.** Positive, and the only
///    one of the three that does not read the value's text at all - which is what makes it the
///    assertion that bites whatever a leaked value spells.
/// 2. **No value appears in the statement**, by two searches: substring with the quoted identifiers
///    stripped, and word-bounded on the raw statement.
/// 3. **The question's values are still bound.** Without it a generator that dropped the predicate
///    entirely would satisfy 2 and 1 alike.
fn binds_every_value_rather_than_writing_it(dialect: Dialect) {
    let pinned = load::<ReferenceCatalog>();
    let mut checked = 0_usize;
    let mut with_definitional = 0_usize;
    for path in questions() {
        let asked = read_question(&path);
        let literals = asked.literals();
        let compiled = compile(
            &asked,
            &ScopedView::everything(&pinned),
            sutura_domain::plan::RowCeiling::DEFAULT,
        )
        .expect("the corpus compiles");
        let Compiled::Planned { ref plan } = compiled else {
            continue;
        };
        let query = sql_for(plan, dialect);
        let name = stem(&path);

        // The positive half, first because it is the one that holds when a search cannot. Counted
        // against the PLAN's parameters rather than the rendered query's, so a generator that
        // inlined a value and dropped it from its own list is still one placeholder short.
        assert_one_placeholder_per_parameter(&name, dialect, plan.params().len(), query.sql());

        // What the CALLER sent. The classic injection: a filter value or a date written into
        // the statement instead of bound.
        for literal in &literals {
            assert_absent_as_text(&name, dialect, "the question's literal", literal.as_str(), query.sql());
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
            assert_absent_as_text(&name, dialect, "the catalog's value", value.as_str(), query.sql());
            with_definitional = with_definitional.saturating_add(1);
        }

        // The values are still there, as parameters. Without this half a generator that
        // dropped the predicate entirely would pass everything above.
        let bound: BTreeSet<String> = query.params().iter().map(bound_value).collect();
        for literal in &literals {
            assert!(
                bound.contains(literal),
                "{name} for {dialect} did not bind the question's {literal:?}; \
                 the predicate is missing rather than inlined:\n{}",
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
/// So the claim is made over `shared::without_string_literals`: dropping the double-quoted spans
/// leaves exactly the part of the statement an identifier could have leaked into. Every model
/// column, every dimension label, every metric name and the time bucket's label has to be absent
/// from what is left - the aliases included, which is the half the old body could not see.
///
/// The positive half is what stops it passing on a generator that emits no identifiers at all: the
/// names this plan actually uses have to be present, quoted, in the raw statement.
///
/// **The stripping is sound HERE for a reason the value claim cannot borrow, and that asymmetry is
/// the point.** This claim is about a name that reached the statement UNQUOTED, so a double-quoted
/// span is by construction not where its evidence could be: dropping those spans removes noise and
/// no evidence. The value claim asks whether text reached the statement at all, and a double-quoted
/// span is somewhere such text CAN sit - which is exactly the hole that was found in it. Same
/// helper, opposite consequence.
///
/// It reads `shared::without_string_literals` and not `shared::without_identifiers` directly, and
/// the extra pass is not tidiness. The bucket's grain keyword is a single-quoted STRING LITERAL that
/// can legitimately equal a column name, so `'month'` inside `DATE_TRUNC('month', ...)` looks
/// exactly like a bare `month` to the search below. This is the one place where this claim and the
/// value claim need DIFFERENT haystacks, and `shared::without_string_literals` carries the reason
/// they cannot share one.
fn quotes_every_identifier(dialect: Dialect) {
    let pinned = load::<ReferenceCatalog>();
    let definitions = pinned.definitions();

    // Every name the generator could have to quote, whether or not this question reaches it. Read
    // off the catalog rather than off the plan, so a name the renderer emits for a reason the plan
    // does not record is covered too.
    let mut names: BTreeSet<String> = BTreeSet::from([String::from(TIME_BUCKET_LABEL)]);
    for model in definitions.models().values() {
        names.insert(String::from(model.table_name().as_str()));
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
        let compiled = compile(
            &asked,
            &ScopedView::everything(&pinned),
            sutura_domain::plan::RowCeiling::DEFAULT,
        )
        .expect("the corpus compiles");
        let Compiled::Planned { ref plan } = compiled else {
            continue;
        };
        let query = sql_for(plan, dialect);
        let stripped = without_string_literals(query.sql(), dialect);

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
        let mut quoted = vec![String::from(plan.table_name().as_str())];
        quoted.extend(plan.joins().iter().map(|join| String::from(join.table_name().as_str())));
        quoted.push(String::from(plan.bucket().label()));
        quoted.push(String::from(plan.measure_label()));
        quoted.extend(plan.keys().iter().map(|key| String::from(key.label())));
        // The quote character comes from the dialect, not from a literal `"`. A hard-coded double
        // quote made this half assert that a BigQuery statement carries `"orders"` - which it never
        // does, so the whole positive half failed rather than passing vacuously. The good direction
        // for a fixture to break in, and the reason it is a function of the dialect now.
        let quote = dialect.identifier_quote().character();
        for name in &quoted {
            assert!(
                query.sql().contains(&format!("{quote}{name}{quote}")),
                "{} for {dialect} does not carry {name:?} quoted with {quote:?}:\n{}",
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
    let mut checked = 0_usize;
    for path in questions() {
        let asked = read_question(&path);
        let compiled = compile(
            &asked,
            &ScopedView::everything(&pinned),
            sutura_domain::plan::RowCeiling::DEFAULT,
        )
        .expect("the corpus compiles");
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
        checked = checked.saturating_add(1);
    }
    assert!(checked > 0, "the corpus produced no statements to check");
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

/// The corpus asks for a question at every grain a metric declares - the latch the coverage here
/// would otherwise not have.
///
/// The `week` grain this change proves permanently rides on the presence of one YAML file
/// (`examples/single-player/questions/data-per-subscription-by-week.yaml`). Nothing else holds it:
/// deleting it leaves every gate green, because `INSTA_UPDATE=no` fails a MISSING snapshot and
/// nothing rejects an UNREFERENCED one - the orphaned goldens keep `check-guidance`'s row-cap count
/// agreeing with `AGENTS.md`. This is the demanding form of the suite's existing proof-that-a-file-
/// alone-would-carry idiom: some question must ask for every grain the bundle's metrics declare, so
/// a metric widened to a grain no question exercises fails, and the `week` this PR adds to
/// `data_per_subscription` is only the grain that would trip it today.
#[test]
fn some_question_asks_for_every_grain_a_metric_declares() {
    let pinned = load::<ReferenceCatalog>();
    let declared: BTreeSet<Grain> = pinned
        .definitions()
        .metrics()
        .values()
        .flat_map(|metric| metric.grains().iter().copied())
        .collect();

    let mut asked = BTreeSet::<Grain>::new();
    for path in questions() {
        let asked_question = read_question(&path);
        let compiled = compile(
            &asked_question,
            &ScopedView::everything(&pinned),
            sutura_domain::plan::RowCeiling::DEFAULT,
        )
        .expect("the corpus compiles");
        let Compiled::Planned { ref plan } = compiled else {
            continue;
        };
        asked.insert(plan.bucket().grain());
    }

    let uncovered: Vec<Grain> = declared.difference(&asked).copied().collect();
    assert!(
        uncovered.is_empty(),
        "a metric declares a grain no question in the corpus asks for: {uncovered:?} - add a \
         question exercising it so the golden latches the grain"
    );
}

/// For each dialect this file renders for, the number of EXECUTION goldens - the four snapshot
/// kinds `crate::adapters::runs_the_corpus_and_pins_the_rows`
/// (`__rows`/`__refused`/`__error`) and `crate::adapters::reproduces_every_declared_anchor`
/// (`anchor_report`) produce - never the `__sql`/`__params` pair this file's own
/// `pins_the_statement_and_its_parameters` pins, which asserts only what THIS workspace's own
/// generator emitted (`docs/adr/0012`'s `RenderedDoesNotParse` and its own limits already name what
/// a parse check cannot see).
///
/// A file count over `tests/snapshots/`, keyed by [`Dialect::as_str`] - the same string
/// `pins_the_statement_and_its_parameters` snapshots the render family under - rather than a count
/// read off any registry, so a `.snap` this suite stopped producing (a deleted case, a renamed
/// adapter) is caught the same way an added one is.
fn execution_golden_count(dialect: Dialect) -> usize {
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots");
    let entries = std::fs::read_dir(&directory).unwrap_or_else(|cause| panic!("{} did not read: {cause}", directory.display()));
    let suffix = format!("@{}.snap", dialect.as_str());
    entries
        .filter_map(Result::ok)
        .filter(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            let Some(stem) = name.strip_suffix(&suffix) else {
                return false;
            };
            stem == "anchor_report" || stem.ends_with("__rows") || stem.ends_with("__refused") || stem.ends_with("__error")
        })
        .count()
}

/// Why a dialect with zero [`execution_golden_count`] is not silence.
///
/// **Exhaustive over [`Dialect`], never a wildcard arm.** A fifth dialect - `docs/adr/0012`'s own
/// `RenderedDoesNotParse` names `ClickHouse`'s weak parser as one reason a render-only cell still
/// under-proves - does not compile here until this function answers for it too, which is what
/// keeps a later addition from silently inheriting *no limit needed* the way a wildcard would. Each
/// arm names WHERE the limit is stated at length, rather than restating the whole argument here -
/// two statements of one fact is the thing this repository asks not to hold twice.
fn stated_limit(dialect: Dialect) -> Option<&'static str> {
    match dialect {
        Dialect::DuckDb | Dialect::Postgres => None,
        Dialect::BigQuery => Some(
            "BigQuery's venue is a cloud account, not a local or CI-reachable one: `DataSystemUnderTest::available` \
             answers `false` unconditionally for it (crates/sutura-app/tests/adapters/adapters.rs, the `impl \
             DataSystemUnderTest for BigQueryWarehouse<NoLocalTier>` block), so this axis never asks it a question. \
             Its render goldens pin what this workspace's own generator emits, and nothing about whether a real \
             endpoint accepts it - `crates/sutura-exec-bigquery/tests/corpus.rs` is the separate, narrower claim \
             that does, behind `wire`+`fixtures`, `#[ignore]`d.",
        ),
        Dialect::ClickHouse => Some(
            "No shipped or CI-reachable venue executes ClickHouse: `compose.services.yaml`'s `clickhouse` service \
             is a docker-compose tier a person brings up by hand, the nix sandbox `just validate` runs in has no \
             docker socket, and no `clickhouse-tier.nix` exists to provision one the way `nix/postgres-tier.nix` \
             does. `crates/sutura-exec-clickhouse`'s own `lib.rs` header states this at length, including a real, \
             manually-run measurement against a local ClickHouse server (`github.com/telekom/sutura#920`/`#919`) \
             that this axis does not repeat mechanically. Its render goldens pin what this workspace's own \
             generator emits and nothing about whether a real ClickHouse accepts it or agrees on the number.",
        ),
        // Arrived with `github.com/telekom/sutura#127` PR 1, the RENDERING half, which is why this arm
        // exists at all: the match is exhaustive over `Dialect` precisely so a dialect cannot land
        // without answering here, and this one landed on `main` while this branch was in review.
        Dialect::Oracle => Some(
            "No adapter executes Oracle on this tree - `ls crates/` has no `sutura-exec-oracle`, because \
             `github.com/telekom/sutura#127` split the rendering half (landed) from the adapter and its venue \
             (a separate change). So Oracle has 29 `sql` and 28 `params` render goldens and ZERO \
             `rows`/`refused`/`error`/`anchor_report` goldens. Its venue is decided but not provisioned - a \
             community image, by tag, brought up by hand - and no `oracle-tier.nix` exists the way \
             `nix/postgres-tier.nix` does, so the nix sandbox `just validate` runs in cannot reach one. The \
             render goldens pin what this workspace's own generator emits; nothing here establishes that a real \
             Oracle accepts the statement or agrees on the number. ⚠ And the parse-back check cannot close that \
             gap: `the_parse_check_cannot_tell_the_two_bucket_shapes_apart` is the standing proof that a \
             construct which parses can still mean the wrong thing.",
        ),
    }
}

/// **What `#919` closes, mechanically.** For each dialect `sutura-sql` renders for, either a real
/// execution venue produced `rows`/`refused`/`error`/`anchor_report` goldens, or [`stated_limit`]
/// names - in this file, next to the claim - what its render-only goldens do not cover. Silence is
/// the one outcome this test refuses: a dialect landing with neither is a defect here, not a
/// missing sentence somewhere else.
///
/// Deliberately NOT a fix for the render-only ceiling itself - `pins_the_statement_and_its_parameters`
/// still only proves the generator is stable, and `crates/sutura-sql/src/generate.rs`'s own
/// `the_parse_check_cannot_tell_the_two_bucket_shapes_apart` names what a parse check cannot see
/// even where this test is green.
#[test]
fn every_dialect_either_executes_or_states_its_limit() {
    for &dialect in dialect::ALL {
        let executed = execution_golden_count(dialect);
        let limit = stated_limit(dialect);
        assert!(
            executed > 0 || limit.is_some_and(|text| !text.trim().is_empty()),
            "{} has {executed} execution goldens (rows/refused/error/anchor_report) and no declared stated \
             limit - either produce execution goldens for it, or add an arm to `stated_limit` naming what its \
             render-only goldens do not cover",
            dialect.as_str()
        );
    }
}
