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
    /// Rows a venue returned, from `crate::data_systems::runs_the_corpus_and_pins_the_rows`.
    Rows,
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
    /// all four of them, which is what makes it a registry rather than a floor.
    const EXECUTION: [Self; 4] = [Self::Rows, Self::Refused, Self::Error, Self::AnchorReport];

    /// The families a dialect has whatever executes it, because `sutura-sql` renders for it.
    const RENDER: [Self; 2] = [Self::Sql, Self::Params];

    /// The family a snapshot stem - the file name with its `@{dialect}.snap` suffix already
    /// stripped - belongs to, or `None` if this census cannot name it.
    ///
    /// `None` is a failure at the call site and never a skip: a family nobody named here counts
    /// as zero everywhere, which is the exact shape of silence this axis exists to refuse.
    fn of(stem: &str) -> Option<Self> {
        if stem == "anchor_report" {
            return Some(Self::AnchorReport);
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

/// What stands behind one dialect's goldens - and, where nothing executed them, which half of a
/// venue is absent.
///
/// **A typed value, and that is the whole of `github.com/telekom/sutura#919`.** What this replaces
/// was a `&'static str` of prose per dialect: honest when written, comparable against nothing, and
/// so still green after the fact it stated stopped being true. Every field here is checked against
/// the tree by [`every_dialect_declares_what_backs_its_goldens`], so a declaration that goes stale
/// is a red rather than a sentence nobody re-read.
///
/// The prose is not restated here: each render-only arm names the file whose own header carries the
/// argument at length, and that path is asserted to exist.
#[derive(Debug, Clone, Copy)]
enum Evidence {
    /// A venue in this workspace ran the corpus for this dialect: every one of
    /// [`Family::EXECUTION`] holds goldens, so what a real engine returned is pinned.
    ///
    /// **Does not reach** whether those rows are the right ANSWER - that comparison against the
    /// engine is `golden/data_systems.rs`, a different axis. This one only knows a venue answered.
    Executed,
    /// Render-only: the goldens pin what `sutura-sql` emitted, and nothing a data system said about
    /// it.
    ///
    /// **Does not reach** the semantic half even where green. The parse-back check that backs these
    /// cells is syntax only, and `crates/sutura-sql/src/generate.rs`'s own
    /// `the_parse_check_cannot_tell_the_two_bucket_shapes_apart` is the standing proof: a construct
    /// that parses can still compute the wrong number. A wrong `TRUNC` argument order or a wrong
    /// grain keyword is unguarded here and this arm is the declaration of that, not a mitigation.
    RenderOnly {
        /// WHERE the venue is, given that it is not a leg of any gate. Two dialects can be
        /// render-only for entirely different reasons, and before this field they read the same.
        venue: Venue,
        /// The file whose own header states this dialect's limit at length. Asserted to EXIST, so a
        /// moved or deleted header reddens this axis instead of leaving a dangling citation.
        ///
        /// Does not reach the CONTENT: nothing here reads the file, so a header that stops arguing
        /// what it used to argue is still green. A path is what a test can hold.
        stated_in: &'static str,
    },
}

/// Where a dialect's execution venue is, given that it is not here.
///
/// **A venue a developer brings up by hand is the shape
/// `docs/where-identity-is-proven.md` has no row for, and that is why this is a second enum rather
/// than a citation of that page's vocabulary.** Its venue column reads *in process, every run* /
/// *a GitHub environment, on demand* / *nowhere yet*, because it grades venues a GATE reaches;
/// [`Self::OnDemand`] is its second run-site token exactly, and [`Self::ByHandOnly`] is a
/// `compose.services.yaml` service no task in that page's `Reached by` column can invoke. That
/// page's other axis - `unrun`/`wired`/`yes` - is deliberately not borrowed at all: it grades how
/// strongly an EXISTING venue's claim has been observed, and borrowing it here would have graded
/// an absence as a weak presence.
///
/// **Both arms claim an adapter exists**, because on this tree every dialect has one (`ls
/// crates/sutura-exec-*`). A dialect with no adapter at all needs a third arm, and it is absent
/// rather than reserved: the workspace denies `dead_code`, so a variant nothing constructs does not
/// compile - measured, `error: variant ... is never constructed`. The exhaustive match in
/// [`evidence`] is what forces the decision when that dialect arrives.
#[derive(Debug, Clone, Copy)]
enum Venue {
    /// *A GitHub environment, on demand.* Checked - `task` names a real `just` task, and
    /// `compose.services.yaml` carries NO service for the dialect, so this arm and the next cannot
    /// both describe one venue.
    OnDemand {
        /// The `just` task that reaches the venue. Asserted to be declared in the `justfile`, so
        /// this cannot cite a task that was renamed away.
        task: &'static str,
    },
    /// A `compose.services.yaml` service a developer brings up by hand, and nothing else. Checked -
    /// that service is named there, so the claim is not of a venue nobody can reach at all.
    ByHandOnly,
}

/// What backs each dialect's goldens.
///
/// **Exhaustive over [`Dialect`], never a wildcard arm**, so a sixth dialect does not compile until
/// somebody decides what backs it. That much already held before `#919`; what is new is that the
/// decision is a value the test below compares against the tree, rather than prose it could only
/// check was non-empty.
fn evidence(dialect: Dialect) -> Evidence {
    match dialect {
        // All three execute on every run - DuckDB in-process, Postgres against the postmaster
        // `nix/postgres-tier.nix` stands up in the same sandbox, ClickHouse against the server
        // `nix/clickhouse-tier.nix` stands up beside it (`github.com/telekom/sutura#920`).
        Dialect::DuckDb | Dialect::Postgres | Dialect::ClickHouse => Evidence::Executed,
        Dialect::BigQuery => Evidence::RenderOnly {
            venue: Venue::OnDemand {
                task: "bigquery-acceptance",
            },
            stated_in: "crates/sutura-exec-bigquery/tests/corpus.rs",
        },
        // `sutura-exec-oracle` arrived with `github.com/telekom/sutura#127` PR 2 while this change
        // was in review, and this arm MOVED for it: the declaration it replaced said *no adapter
        // executes Oracle on this tree*, prose that went false on a merge no gate would have read.
        // That is `#919` in one line.
        Dialect::Oracle => Evidence::RenderOnly {
            venue: Venue::ByHandOnly,
            stated_in: "crates/sutura-exec-oracle/src/lib.rs",
        },
    }
}

/// **What `#919` closes.** For every dialect `sutura-sql` renders for, [`evidence`] declares what
/// backs its goldens and that declaration is compared against the tree - six checks, none of which
/// the prose it replaced could make:
///
/// 1. **Render goldens exist at all.** Without it a dialect with nothing whatsoever could declare a
///    limit and read as covered-but-honest.
/// 2. **[`Evidence::Executed`] means every one of [`Family::EXECUTION`] is non-empty**, and a
///    missing one is NAMED. The count it replaces was a floor: 34 of the 35 execution goldens
///    deleted still passed it.
/// 3. **[`Evidence::RenderOnly`] means no execution family holds anything.** So the declaration
///    EXPIRES: a dialect that acquires a venue is red here until its arm moves.
/// 4. **The adapter the declaration claims is there.**
/// 5. **No `nix/{dialect}-tier.nix` exists.** This is the sharpest of the six, because a nix tier is
///    exactly what would put the dialect inside a `just validate` leg - so adding one makes
///    *render-only* false, and this check is what says so. Not the only reader of that path:
///    `xtask`'s `compose::file::every_nix_tier_module_is_provisioned_by_a_nix_check` already holds
///    a tier module to being provisioned, and this one holds it to the dialect's GOLDENS, which is
///    the edge that was missing.
/// 6. **The venue named is the venue there is**: [`Venue::OnDemand`]'s task is declared
///    in the `justfile` and the dialect has no compose service; [`Venue::ByHandOnly`]'s
///    compose service is named. Without this the `venue` field would be decoration, and `BigQuery`'s
///    hosted absence would read the same as `Oracle`'s by-hand one.
///
/// Plus `stated_in` resolving to a file, so a moved header is a red and not a dangling citation.
///
/// **Limits, next to the claim.** The dialect-to-crate-directory mapping is
/// `sutura-exec-{Dialect::as_str}` and the tier path `nix/{as_str}-tier.nix`: naming conventions
/// this test relies on and no mechanism holds, so an adapter or a tier named otherwise reads as
/// absent. A present directory is not a WIRED adapter - nothing here asks whether a composition
/// root links it, and `sutura-exec-oracle` is the standing case of one nothing links. The
/// compose and `justfile` reads are substring searches over the file, not a parse of either. And
/// none of this reaches whether a golden's CONTENT is right: measured while `ClickHouse` was still
/// render-only, flipping its `date_trunc_shape` to the wrong shape and re-accepting the render
/// goldens left this test and `dialects::clickhouse::every_generated_statement_parses_here` both
/// green, which is the ceiling [`Evidence::RenderOnly`] declares rather than closes - and the
/// ceiling `BigQuery` and `Oracle` still stand under.
#[test]
fn every_dialect_declares_what_backs_its_goldens() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let read = |relative: &str| {
        std::fs::read_to_string(root.join(relative)).unwrap_or_else(|cause| panic!("{relative} did not read: {cause}"))
    };
    let justfile = read("justfile");
    let compose = read("compose.services.yaml");
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

        let standing: Vec<Family> = Family::EXECUTION.into_iter().filter(|family| held(*family) > 0).collect();
        match evidence(dialect) {
            Evidence::Executed => {
                let empty: Vec<Family> = Family::EXECUTION.into_iter().filter(|family| held(*family) == 0).collect();
                assert!(
                    empty.is_empty(),
                    "{name} declares `Evidence::Executed` but {empty:?} hold no goldens - either a \
                     venue stopped producing them, or the declaration is wrong. A count over all \
                     four would have passed this on the {} that remain",
                    standing.len()
                );
            }
            Evidence::RenderOnly { venue, stated_in } => {
                assert!(
                    standing.is_empty(),
                    "{name} declares `Evidence::RenderOnly` and yet {standing:?} hold goldens - \
                     something executes it now, so move its arm in `evidence` to \
                     `Evidence::Executed` rather than leaving a limit nothing expires"
                );
                assert!(
                    root.join(format!("crates/sutura-exec-{name}")).is_dir(),
                    "{name} declares {venue:?}, which claims an adapter, but crates/sutura-exec-{name} \
                     is not there"
                );
                let tier = format!("nix/{name}-tier.nix");
                assert!(
                    !root.join(&tier).exists(),
                    "{tier} exists, so a `just validate` leg can provision {name} and its goldens are \
                     no longer render-only - move its arm in `evidence` to `Evidence::Executed` and \
                     produce execution goldens from that tier"
                );
                let service = format!("\n  {name}:\n");
                match venue {
                    Venue::OnDemand { task } => {
                        assert!(
                            justfile.contains(&format!("\n{task}:")),
                            "{name} declares its venue is reached by `just {task}`, which the justfile \
                             does not declare"
                        );
                        assert!(
                            !compose.contains(&service),
                            "{name} declares an on-demand venue and compose.services.yaml names a \
                             {name} service too - one of the two is the venue, so say which"
                        );
                    }
                    Venue::ByHandOnly => assert!(
                        compose.contains(&service),
                        "{name} declares a by-hand compose venue and compose.services.yaml names no \
                         {name} service, so nothing reaches it at all - that is a stronger absence \
                         than this arm states"
                    ),
                }
                assert!(
                    root.join(stated_in).is_file(),
                    "{name}'s `stated_in` cites {stated_in}, which is not a file - the limit is \
                     stated somewhere this citation no longer reaches"
                );
            }
        }
    }
}
