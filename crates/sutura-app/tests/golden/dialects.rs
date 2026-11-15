//! What a dialect decides, expanded over every one `sutura-sql` renders for.
//!
//! The catalog is `adapters::ReferenceCatalog` throughout, named once there rather than here: which
//! one it is does not matter to a renderer, and the catalog axis is what makes that true.

use std::collections::{BTreeMap, BTreeSet};
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
            sutura_domain::plan::Ceilings::DEFAULT,
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
            sutura_domain::plan::Ceilings::DEFAULT,
        )
        .expect("the corpus compiles");
        let Compiled::Planned { ref plan } = compiled else {
            continue;
        };
        let query = sql_for(plan, dialect);
        let name = stem(&path);

        // The positive half, first because it is the one that holds when a search cannot.
        // `Numbered` (Postgres's `$n`) binds by index, so it is counted against the PLAN's own
        // parameters (a repeated placeholder there reuses one value rather than needing another) -
        // a generator that inlined a value and dropped it from the plan's own list is still one
        // placeholder short. `Question` and `Colon` both bind a plain statement by occurrence -
        // Oracle's driver pushes one bind per occurrence for a plain, non-PL/SQL statement despite
        // its named `:n` placeholders - so a guard embedded more than once needs its value bound
        // again for both, counted against the RENDERED query's own parameters, which is what
        // `sutura_sql::generate` builds for exactly that dialect family - see
        // `assert_one_placeholder_per_parameter`'s own doc for why both are "how many physical
        // placeholders this statement needs".
        let expected_params = match dialect.placeholder_style() {
            sutura_sql::dialect::PlaceholderStyle::Question | sutura_sql::dialect::PlaceholderStyle::Colon => {
                query.params().len()
            }
            sutura_sql::dialect::PlaceholderStyle::Numbered => plan.params().len(),
        };
        assert_one_placeholder_per_parameter(&name, dialect, expected_params, query.sql());

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
        names.extend(model.columns().map(|column| String::from(column.name().as_str())));
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
            sutura_domain::plan::Ceilings::DEFAULT,
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
        quoted.extend(plan.measures().iter().map(|measure| String::from(measure.label())));
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
            sutura_domain::plan::Ceilings::DEFAULT,
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
            sutura_domain::plan::Ceilings::DEFAULT,
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

/// One snapshot family of one dialect, parsed out of a `.snap` file name.
///
/// **The parse is the mechanism, and it exists because a glob got this wrong.**
/// `anchor_report@duckdb.snap` carries no `__` separator at all, so a `*__$family@$dialect.snap`
/// pattern misses it and undercounts a dialect's execution goldens by exactly the one
/// separator-less `anchor_report` per executing dialect - so a glob reads 34 where the census reads
/// 35 (measured 2026-09-21 at `35c5289a` over `tests/snapshots`: rows=23, refused=10, error=1,
/// anchor=1, per executing dialect). Every reader of the census
/// goes through here, so there is one place that can be wrong about it instead of one per caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Family {
    /// The rendered statement, from `pins_the_statement_and_its_parameters`. Render evidence: it
    /// asserts what THIS workspace's generator emitted and nothing a data system said back.
    Sql,
    /// The values bound to that statement. Render evidence, for the same reason.
    Params,
    /// Planned source statements recorded without executing them.
    PlannedShapes,
    /// Rows a venue returned, from `crate::data_systems::runs_the_corpus_and_pins_the_rows`.
    Rows,
    /// Planned statement rows expected from the native matrix; file presence alone proves no run.
    PlannedRows,
    /// A refusal a venue's answer produced, from the same cell.
    Refused,
    /// An error a venue produced, from the same cell.
    Error,
    /// `crate::data_systems::reproduces_every_declared_anchor`'s report - the one family whose file
    /// name has no case prefix and therefore no `__`.
    AnchorReport,
}

impl Family {
    /// The families only a venue that RAN the corpus can produce. [`Evidence::Executed`] claims
    /// all of them, which is what makes it a registry rather than a floor.
    const EXECUTION: [Self; 5] = [Self::Rows, Self::Refused, Self::Error, Self::AnchorReport, Self::PlannedRows];

    /// The families pinning emitted statements or parameters, without engine execution.
    const RENDER: [Self; 3] = [Self::Sql, Self::Params, Self::PlannedShapes];

    /// The family a snapshot stem - the file name with its `@{dialect}.snap` suffix already
    /// stripped - belongs to, or `None` if this census cannot name it.
    ///
    /// `None` is a failure at the call site and never a skip: a family nobody named here counts
    /// as zero everywhere, which is the exact shape of silence this axis exists to refuse.
    fn of(stem: &str) -> Option<Self> {
        if stem == "anchor_report" {
            return Some(Self::AnchorReport);
        }
        if stem == "planned_statement_shapes" {
            return Some(Self::PlannedShapes);
        }
        if stem == "planned_statement_rows" {
            return Some(Self::PlannedRows);
        }
        let (_case, family) = stem.rsplit_once("__")?;
        match family {
            "sql" => Some(Self::Sql),
            "params" => Some(Self::Params),
            "rows" => Some(Self::Rows),
            "refused" => Some(Self::Refused),
            "error" => Some(Self::Error),
            _ => None,
        }
    }
}

/// How many goldens exist per [`Family`] for one dialect, read off `tests/snapshots/`.
///
/// Off the directory and not off any registry, because the question this answers is what EXISTS.
/// Keyed by [`Dialect::as_str`] - the same string the render cells snapshot under.
///
/// Per-family and no longer one total, which is the change `github.com/telekom/sutura#919` asked
/// for: a total is a liveness floor, so a deletion that leaves any one golden standing passes it.
/// A per-family census names the family that emptied.
fn census(dialect: Dialect) -> BTreeMap<Family, usize> {
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots");
    let entries = std::fs::read_dir(&directory).unwrap_or_else(|cause| panic!("{} did not read: {cause}", directory.display()));
    let suffix = format!("@{}.snap", dialect.as_str());
    let mut counted = BTreeMap::<Family, usize>::new();
    for entry in entries.filter_map(Result::ok) {
        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();
        let Some(stem) = file_name.strip_suffix(suffix.as_str()) else {
            continue;
        };
        let family = Family::of(stem).unwrap_or_else(|| {
            panic!(
                "{file_name} is a {} golden in a family this census cannot name, so it counts as \
                 zero and reads as absent; add it to `Family` and decide there whether it is \
                 render or execution evidence",
                dialect.as_str()
            )
        });
        let held = counted.entry(family).or_insert(0_usize);
        *held = held.saturating_add(1);
    }
    counted
}

/// What stands behind one dialect's goldens.
///
/// **A typed value, and that is the whole of `github.com/telekom/sutura#919`.** What this replaces
/// was a `&'static str` of prose per dialect: honest when written, comparable against nothing, and
/// so still green after the fact it stated stopped being true. The declaration is checked against
/// the tree by [`every_dialect_declares_what_backs_its_goldens`], so one that goes stale is a red
/// rather than a sentence nobody re-read.
///
/// **One arm, because every dialect now executes somewhere.** A dialect whose goldens only render
/// needs a second arm that says so, and it is absent rather than reserved: the workspace denies
/// `dead_code`, so a variant nothing constructs does not compile.
#[derive(Debug, Clone, Copy)]
enum Evidence {
    /// A venue in this workspace ran the corpus for this dialect: every one of
    /// [`Family::EXECUTION`] holds goldens, so what a real engine returned is pinned.
    ///
    /// **Does not reach** whether those rows are the right ANSWER - that comparison against the
    /// engine is `golden/data_systems.rs`, a different axis. This one only knows a venue answered.
    Executed,
}

/// What backs each dialect's goldens.
///
/// **Exhaustive over [`Dialect`], never a wildcard arm**, so a sixth dialect does not compile until
/// somebody decides what backs it. That much already held before `#919`; what is new is that the
/// decision is a value the test below compares against the tree, rather than prose it could only
/// check was non-empty.
fn evidence(dialect: Dialect) -> Evidence {
    match dialect {
        // DuckDB executes in process, Postgres against the postmaster `nix/postgres-tier.nix`
        // stands up in the same sandbox, ClickHouse against the server `nix/clickhouse-tier.nix`
        // stands up beside it (`github.com/telekom/sutura#920`): all three on every run.
        //
        // BigQuery and Oracle each execute in ONE venue and no gate: the `bigquery-conformance` and
        // `oracle-tier` CI jobs, against the dataset and the database they provision (`just
        // bigquery-conformance`, `just oracle-acceptance`). Their execution goldens were seeded from
        // the answer the other executing data systems already return byte for byte - the
        // conformance claim itself - and BigQuery's one error golden is pinned through the adapter's
        // own layers, because the driver's message links the run's job. **So a golden of either is
        // an expectation until its job has run green**, and no nix check or `just validate` leg
        // reaches it. Oracle's runs the example corpus only, over a database a developer can also
        // start by hand.
        Dialect::DuckDb | Dialect::Postgres | Dialect::ClickHouse | Dialect::BigQuery | Dialect::Oracle => Evidence::Executed,
    }
}

/// **What `#919` closes.** For every dialect `sutura-sql` renders for, [`evidence`] declares what
/// backs its goldens and that declaration is compared against the tree - two checks, neither of
/// which the prose it replaced could make:
///
/// 1. **Render goldens exist at all.** Without it a dialect with nothing whatsoever could declare a
///    limit and read as covered-but-honest.
/// 2. **[`Evidence::Executed`] means every one of [`Family::EXECUTION`] is non-empty**, and a
///    missing one is NAMED. The count it replaces was a floor: 34 of the 35 execution goldens
///    deleted still passed it.
///
/// **Limits, next to the claim.** This counts files per family and reads none of them: a golden
/// copied from another dialect is counted exactly as one a venue produced, so nothing here knows
/// that Oracle's or `BigQuery`'s were not (each is compared with its engine only in its own CI job,
/// and only for the example corpus). Nor does it reach whether a golden's CONTENT is right:
/// measured while `ClickHouse` was still render-only, flipping its `date_trunc_shape` to the wrong
/// shape and re-accepting the render goldens left this test and
/// `dialects::clickhouse::every_generated_statement_parses_here` both green, because the ceiling of
/// a render golden is that it pins what the generator emitted.
/// `DataFusion` and Cube are outside `sutura_sql::dialect::ALL`; this census measures neither's file presence.
/// Their render and native matrices use the separate `statement_dialects` and `data_systems` registry axes.
#[test]
fn every_dialect_declares_what_backs_its_goldens() {
    for &dialect in dialect::ALL {
        let counted = census(dialect);
        let held = |family: Family| counted.get(&family).copied().unwrap_or_default();
        let name = dialect.as_str();

        for family in Family::RENDER {
            assert!(
                held(family) > 0,
                "{name} has no {family:?} goldens, so it renders nothing this suite pins - \
                 whatever `evidence` declares about it describes an empty cell"
            );
        }

        let Evidence::Executed = evidence(dialect);
        let empty: Vec<Family> = Family::EXECUTION.into_iter().filter(|family| held(*family) == 0).collect();
        assert!(
            empty.is_empty(),
            "{name} declares `Evidence::Executed` but {empty:?} hold no goldens - either a \
             venue stopped producing them, or the declaration is wrong. A count over all \
             families would have passed this on the {} that remain",
            Family::EXECUTION.len().saturating_sub(empty.len())
        );
    }
}
