//! #2038 Opus re-review Round B: the raw prefilter's wildcard, rename and quoted-identifier
//! guards, isolated from the rest of the Oracle leg so neither file crosses the line-count gate.

use std::collections::HashMap;
use std::ops::ControlFlow;

use datafusion::sql::sqlparser::ast::{Expr, Query, SetExpr, Statement, TableFactor, Visit as _, Visitor};
use datafusion::sql::sqlparser::dialect::GenericDialect;
use datafusion::sql::sqlparser::parser::Parser;

use super::oracle::{CapturedLegs, base_table_selects, docs_table, reaches_oracle};
use super::*;

/// A duplicate of `tests/text_prefilter.rs`'s own `table`, kept private to this file rather than
/// shared, so neither file's pre-existing helper has to change visibility for the other's sake.
fn notes_table(enabled: bool) -> StatementTable {
    let mut policy = serde_json::json!({"redact": [{"pattern": "secret[^ ]*", "replace": "[masked]"}]});
    if enabled {
        policy["raw_prefilter"] = serde_json::json!({"acknowledged_because": "accepted secret-dependent timing"});
    }
    policy["max_text_bytes"] = serde_json::json!(128);
    let metadata = HashMap::from([(String::from("sutura:text_policy"), policy.to_string())]);
    StatementTable::new(
        named("data"),
        QualifiedTable::parse("notes").expect("a table"),
        TypeDialect::DataFusion,
        Arc::new(Schema::new(vec![
            Field::new("BODY", DataType::Utf8, true).with_metadata(metadata),
            Field::new("id", DataType::Int64, true),
        ])),
    )
}

#[derive(Default)]
struct NotesPredicates(Vec<Option<Expr>>);

impl Visitor for NotesPredicates {
    type Break = ();

    fn pre_visit_query(&mut self, query: &Query) -> ControlFlow<Self::Break> {
        if let SetExpr::Select(select) = query.body.as_ref()
            && let [from] = select.from.as_slice()
            && matches!(&from.relation, TableFactor::Table { name, .. } if name.0.last().and_then(|part| part.as_ident()).is_some_and(|part| part.value == "notes"))
        {
            self.0.push(select.selection.clone());
        }
        ControlFlow::Continue(())
    }
}

fn parsed(sql: &str) -> Statement {
    Parser::parse_sql(&GenericDialect, sql)
        .expect("generated SQL parses")
        .pop()
        .expect("one statement")
}

/// A duplicate of `tests/text_prefilter.rs`'s own `raw_predicates`, over the fixed `notes_table`
/// shape this file needs rather than its full parameterization.
fn raw_predicates(sql: &str) -> Vec<Option<Expr>> {
    let table = notes_table(true);
    let dialect = table.dialect();
    let baseline = notes_table(false);
    let baseline = super::super::text::apply(String::from(sql), &[baseline], dialect)
        .expect("the disabled policy must rewrite successfully");
    let rewritten = super::super::text::apply(String::from(sql), &[table], dialect)
        .expect("an acknowledged policy must rewrite successfully");
    let original = parsed(&baseline);
    let rewritten = parsed(&rewritten);
    let outer = |statement: Statement| {
        let Statement::Query(query) = statement else {
            panic!("a query");
        };
        let SetExpr::Select(select) = *query.body else {
            panic!("a select");
        };
        select.selection
    };
    assert_eq!(
        outer(original),
        outer(rewritten.clone()),
        "the exact redacted predicate must remain decisive"
    );
    let mut raw = NotesPredicates::default();
    assert!(rewritten.visit(&mut raw).is_continue());
    assert!(!raw.0.is_empty(), "the source table must still be read");
    raw.0
}

/// A `LIKE` anywhere in an expression tree whose left side mentions `SCRIPT` - including under an
/// `UPPER(...)` wrapper - the shape [`text_prefilter::raw_condition`] builds. A base-table `WHERE`
/// may legitimately carry an unrelated pushdown filter (`OBJECT_NAME IS NOT NULL`), so the check
/// looks for this specific shape rather than refusing any selection at all.
struct RawLikeOnScript(bool);

impl Visitor for RawLikeOnScript {
    type Break = ();

    fn pre_visit_expr(&mut self, expression: &Expr) -> ControlFlow<Self::Break> {
        if let Expr::Like {
            negated: false,
            any: false,
            expr,
            ..
        } = expression
        {
            let target = match expr.as_ref() {
                Expr::Function(function) => function.args.to_string(),
                other => other.to_string(),
            };
            if target.to_uppercase().contains("SCRIPT") {
                self.0 = true;
            }
        }
        ControlFlow::Continue(())
    }
}

fn holds_no_raw_like_on_script(selection: &Expr) -> bool {
    let mut probe = RawLikeOnScript(false);
    let _never_breaks = selection.visit(&mut probe);
    !probe.0
}

/// #2038 Opus review MEDIUM4(c): every shape the HIGH1/HIGH2 fixes in `text_prefilter.rs` must
/// reject - each asserted to leave no raw `LIKE` on `SCRIPT` at the base-table level, the only
/// level the source ever receives the stored value from.
///
/// A case belongs here only when the dangerous shape is load-bearing enough that the planner
/// cannot legitimately flatten or merge it away before `discover` ever sees it - a case with an
/// unused column, a merely re-filtering level, or a key the predicate already commutes with (a
/// `GROUP BY`/`DISTINCT` key, a two-level `WHERE`, an unreferenced scalar subquery or window
/// column) proves nothing at this level and is covered instead directly against
/// [`super::super::text_prefilter::discover`] in
/// `raw_prefilter_guards_reject_renames_transforms_and_option_bearing_wildcards_directly`, where
/// no planning sits between the SQL text and the guard.
///
/// Pins pre-existing chain-walk logic this PR did not introduce (green against base); its
/// `Claim-Cell` trailer and killing mutation live on the commit touching it, per
/// `devco/claim-mutations/`.
#[test]
fn raw_prefilter_never_reaches_the_source_through_a_row_changing_or_renaming_level() {
    for sql in [
        // A derived table between the owner and the base carries its own LIMIT.
        "SELECT count(OBJECT_NAME) FROM (SELECT OBJECT_NAME, SCRIPT FROM DOCS LIMIT 5) d WHERE SCRIPT LIKE '%MRC%'",
        // ... an aggregate that computes SCRIPT rather than passing it through - unlike grouping
        // or filtering BY SCRIPT itself, this does not commute with a filter above it, so the
        // planner cannot legitimately flatten it into the plain base case.
        "SELECT count(OBJECT_NAME) FROM (SELECT OBJECT_NAME, max(SCRIPT) AS SCRIPT FROM DOCS GROUP BY OBJECT_NAME) d WHERE SCRIPT LIKE '%MRC%'",
        // ... a window function in its projection, with its own output load-bearing in the outer
        // WHERE (an unused window column is pruned away before discover ever sees it, flattening
        // this into the plain safe shape - the same mistake a filter on a GROUP BY key made).
        "SELECT count(OBJECT_NAME) FROM (SELECT OBJECT_NAME, SCRIPT, row_number() OVER (ORDER BY OBJECT_NAME) AS r FROM DOCS) d WHERE SCRIPT LIKE '%MRC%' AND r <= 5",
        // A level renames the predicate's own column instead of passing it through bare.
        "SELECT count(*) FROM (SELECT OBJECT_NAME AS SCRIPT FROM DOCS) d WHERE SCRIPT LIKE '%MRC%'",
        // ... or transforms it.
        "SELECT count(*) FROM (SELECT UPPER(OBJECT_NAME) AS SCRIPT FROM DOCS) d WHERE SCRIPT LIKE '%MRC%'",
        // A UNION sits inside a derived level.
        "SELECT count(OBJECT_NAME) FROM (SELECT OBJECT_NAME, SCRIPT FROM DOCS UNION SELECT OBJECT_NAME, SCRIPT FROM DOCS) d WHERE SCRIPT LIKE '%MRC%'",
    ] {
        let reached = reaches_oracle(sql);
        let no_raw_like_on_script = base_table_selects(&reached, "DOCS")
            .iter()
            .all(|base| base.selection.as_ref().is_none_or(holds_no_raw_like_on_script));
        assert!(
            no_raw_like_on_script,
            "a row-changing or renaming level must keep the redacted-only plan, never push a raw \
             LIKE on SCRIPT to the base table: {sql} -> {reached}"
        );
    }
}

/// #2038 Opus re-review (VACUITY): the planner may rewrite a rename, a transform, a doubly
/// filtered derived level, or a filter over a `GROUP BY`/`DISTINCT` key away before `discover`
/// ever sees the dangerous shape - a filter on a grouping key or a `DISTINCT` column commutes
/// below it, so the planner can legitimately flatten it into the safe, plain case before
/// `discover` ever runs. These mirror those cases, plus the two new HIGH/LOW wildcard shapes,
/// directly against [`super::super::text_prefilter::discover`] with no `DataFusion` planning in
/// between, so the guard - not the optimizer - is what rejects them.
#[test]
fn raw_prefilter_guards_reject_renames_transforms_and_option_bearing_wildcards_directly() {
    for sql in [
        // A level renames the predicate's own column instead of passing it through bare.
        "SELECT count(id) FROM (SELECT id, id AS \"BODY\" FROM \"notes\") d WHERE \"BODY\" LIKE '%TERM%'",
        // ... or transforms it.
        "SELECT count(id) FROM (SELECT id, UPPER(id) AS \"BODY\" FROM \"notes\") d WHERE \"BODY\" LIKE '%TERM%'",
        // Two levels each carry their own WHERE - the two filters may merge into one, so this is
        // checked directly against the guard rather than through the planner.
        "SELECT count(id) FROM (SELECT id, \"BODY\" FROM \"notes\" WHERE id IS NOT NULL) d WHERE \"BODY\" LIKE '%TERM%'",
        // A scalar subquery sits inside a derived level's own projection, unreferenced outside it -
        // a planner may prune it away before discover ever runs, so this is checked directly too.
        "SELECT count(id) FROM (SELECT id, \"BODY\", (SELECT count(*) FROM \"notes\" o WHERE o.id = \"notes\".id) AS n FROM \"notes\") d WHERE \"BODY\" LIKE '%TERM%'",
        // A WHERE sits above a GROUP BY level, filtering on the grouping key itself - a shape a
        // planner may flatten away, so it must be rejected at this unvarnished textual level too.
        "SELECT count(id) FROM (SELECT id, \"BODY\", count(*) AS n FROM \"notes\" GROUP BY id, \"BODY\") d WHERE \"BODY\" LIKE '%TERM%'",
        // A WHERE sits above a DISTINCT level, filtering on a column DISTINCT already dedups.
        "SELECT count(id) FROM (SELECT DISTINCT id, \"BODY\" FROM \"notes\") d WHERE \"BODY\" LIKE '%TERM%'",
        // HIGH: a wildcard carrying REPLACE or RENAME is not a bare passthrough.
        "SELECT count(id) FROM (SELECT * REPLACE (id AS id) FROM \"notes\") d WHERE \"BODY\" LIKE '%TERM%'",
        "SELECT count(id) FROM (SELECT * RENAME (id AS renamed_id) FROM \"notes\") d WHERE \"BODY\" LIKE '%TERM%'",
        // LOW: a bare wildcard beside another item aliased to the predicate column's own name.
        "SELECT count(id) FROM (SELECT *, id AS \"BODY\" FROM \"notes\") d WHERE \"BODY\" LIKE '%TERM%'",
        // LOW, fail-closed: the shadowing alias differs only in case from the predicate column -
        // must still refuse rather than let the case difference slip the shadow check.
        "SELECT count(id) FROM (SELECT *, id AS \"body\" FROM \"notes\") d WHERE \"BODY\" LIKE '%TERM%'",
        // Fail-closed, the other direction: a bare identifier differing only in case/quoting from
        // the predicate column is NOT a passthrough of it - `"body"` and `"BODY"` are distinct
        // physical columns, so treating them as the same name here would bind the raw condition to
        // the wrong one.
        "SELECT count(id) FROM (SELECT id, \"body\" FROM \"notes\") d WHERE \"BODY\" LIKE '%TERM%'",
    ] {
        assert!(raw_predicates(sql).iter().all(Option::is_none), "{sql}");
    }
    // A bare wildcard with no shadowing alias is still a passthrough.
    let passthrough = "SELECT count(id) FROM (SELECT * FROM \"notes\") d WHERE \"BODY\" LIKE '%TERM%'";
    assert!(
        raw_predicates(passthrough)[0].is_some(),
        "a bare wildcard must still pass the predicate's own column through unchanged"
    );
}

/// #2038 Opus re-review (ADD): `resolve.rs`'s `Fold::name` keeps a quoted identifier exact, so a
/// quoted lowercase `"docs"`/`"script"` never folds up to the native dictionary's uppercase
/// `DOCS`/`SCRIPT` - it must be refused before the prefilter, or anything else, ever runs, not
/// silently read as a different (nonexistent) table.
///
/// Pins `resolve.rs`'s pre-existing quoted-identifier fold, which this PR did not change (green
/// against base); its `Claim-Cell` trailer and killing mutation live on the commit touching it,
/// per `devco/claim-mutations/`.
#[test]
fn quoted_lowercase_docs_or_script_is_refused_before_any_pushdown() {
    for sql in [
        "SELECT count(\"object_name\") FROM \"docs\" WHERE \"script\" LIKE '%MRC%'",
        "SELECT count(object_name) FROM \"docs\" WHERE \"script\" LIKE '%MRC%'",
    ] {
        let statement = RawStatement::parse(sql).expect("a parseable query");
        let mut legs = CapturedLegs::default();
        let answer = DataFusionCombiner::new().expect("a combiner builds").answer_statement(
            StatementQuestion::new(
                &statement,
                &docs_table(),
                false,
                UNBOUNDED,
                opened(Duration::ZERO, Duration::from_secs(60)),
            ),
            &mut legs,
        );
        assert!(
            matches!(answer, Some(Err(StatementStopped::UnknownTable))),
            "a quoted lowercase \"docs\" must not fold up to the native dictionary's DOCS: {sql} -> {answer:?}"
        );
        assert!(legs.statements.is_empty(), "no statement may reach any source: {sql}");
    }
}
