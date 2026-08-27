//! Findings from an adversarial review of `sutura_sql::expression`, as tests.
//!
//! Every case here was reproduced against the compile at commit `4f6983f` with sutura's own
//! feature set, and each one is a way catalog-authored text reaches a data system in a form the
//! guards claim to have certified. They are written as assertions on the *stated* invariant rather
//! than on current behaviour, which is what let them be written before the fixes existed - and it
//! is what makes them a regression suite now that the fixes do. **Nothing here may be weakened to
//! make it pass.**
//!
//! Each one is closed, and the refusal that closes it is asserted in detail beside the code it
//! guards - `crates/sutura-sql/src/expression/tests.rs` for the compile, and the `mod tests` in
//! `crates/sutura-domain/src/expression.rs` for the two the domain boundary owns. What is here is
//! the finding as the reviewer wrote it.

// `cfg(test)` because clippy only honours `allow-expect-in-tests` and `allow-panic-in-tests` for
// code inside a `#[cfg(test)]` item, and `tests_outside_test_module` wants the `#[test]` functions
// there too. An integration test target is compiled with `--test`, so the gate is true here and
// nothing below is conditional in practice - `crates/sutura-cli/tests/example.rs` carries the same
// wrapper for the same reason.
#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use sutura_domain::expression::{AuthoredSql, DialectTag, SqlFragment};
    use sutura_domain::model::{ColumnName, TableName};
    use sutura_sql::dialect::ALL;
    use sutura_sql::expression::compile;

    fn columns() -> BTreeSet<ColumnName> {
        ["mrr_eur", "status", "customer_key", "region", "order_date", "churned"]
            .into_iter()
            .map(|raw| ColumnName::parse(raw).expect("a test column is a column"))
            .collect()
    }

    fn table() -> TableName {
        TableName::parse("fact_subscription").expect("a test table is a table")
    }

    /// One portable fragment, compiled. `None` if the compile refused it.
    fn accepted(raw: &str) -> Option<sutura_sql::expression::CompiledExpression> {
        let fragment = SqlFragment::parse(raw).ok()?;
        let authored =
            AuthoredSql::new(BTreeMap::from([(DialectTag::portable(), fragment)])).expect("one fragment is authored sql");
        compile(&authored, &table(), &columns()).ok()
    }

    #[test]
    fn finding_1_the_qualification_rewrite_does_not_reach_every_column() {
        // `qualify` runs inside the dialect layer's `transform_map`, whose child coverage is NARROWER
        // than the traversal API's: `transform_recursive_inner` dispatches on a hardcoded list of node
        // kinds and everything else falls to `transform_recursive_reference`, whose final arm is
        // `other => other` - the subtree is returned untouched. So the unknown-column check (a `dfs`
        // walk) sees these columns and the rewrite does not.
        //
        // The consequence is the one the module says qualification exists to prevent: "an unqualified
        // column in a statement that later grows a join binds to whichever table happens to have it,
        // and that is a wrong number rather than an error". Measured in DuckDB 1.5.5, the rendering
        // for the COLLATE case against a joined dimension carrying the same column name is
        // `Binder Error: Ambiguous reference to column name "region"` - at query time, for a metric
        // whose load succeeded.
        let table = table();
        let qualifier = format!("\"{}\".", table.as_str());
        for raw in [
            "MAX(mrr_eur COLLATE NOCASE)",
            "SUM(mrr_eur) SIMILAR TO 'x'",
            "SUM(mrr_eur) WITHIN GROUP (ORDER BY region)",
            "ARRAY_AGG(mrr_eur ORDER BY region)",
            "LIST(mrr_eur ORDER BY region)",
            "GROUP_CONCAT(status ORDER BY region)",
            "FIRST_VALUE(mrr_eur IGNORE NULLS) OVER (PARTITION BY region)",
            "LAST_VALUE(mrr_eur) IGNORE NULLS OVER (PARTITION BY region)",
        ] {
            let Some(compiled) = accepted(raw) else {
                continue; // refused is the correct outcome too
            };
            for dialect in ALL.iter().copied() {
                let sql = compiled.for_dialect(dialect).expect("resolved").sql();
                for column in columns() {
                    let bare = format!("\"{}\"", column.as_str());
                    let with_table = format!("{qualifier}\"{}\"", column.as_str());
                    assert_eq!(
                        sql.matches(&bare).count(),
                        sql.matches(&with_table).count(),
                        "{raw:?} for {dialect} left {} unqualified: {sql}",
                        column.as_str()
                    );
                }
            }
        }
    }

    #[test]
    fn finding_2_a_function_name_is_the_reach_the_denylist_does_not_bound() {
        // A generic `Expression::Function` is emitted verbatim into every target, and the only names
        // checked are the date and time ones. Everything else - including every function that reads a
        // file, an environment variable, a setting, or executes SQL of its own - passes.
        //
        // Measured against DuckDB 1.5.5: the rendering of `MAX(getenv('X'))` is
        // `SELECT (MAX(GETENV('X'))) FROM fact_subscription` and it RETURNS THE VALUE of the
        // environment variable. With `SUTURA_SECRET_PROBE=hunter2-exfiltrated` set in the process,
        // that statement answered `hunter2-exfiltrated`. Every secret the sutura process holds -
        // a service-account path, a warehouse password, a token - is readable this way, under a
        // certified metric name, from a catalog file.
        //
        // `Construct::TableReference::why` states the invariant this breaks: "a measure expression may
        // read only the columns its own model declares".
        for raw in [
            // DuckDB: scalar, reads the process environment.
            "MAX(getenv('SUTURA_SECRET_PROBE'))",
            "SUM(mrr_eur) + LEN(getenv('HOME'))",
            // DuckDB: scalar, reads engine configuration.
            "MAX(current_setting('temp_directory'))",
            // Postgres: scalar, reads an arbitrary file.
            "SUM(mrr_eur) + LENGTH(pg_read_file('/etc/passwd'))",
            // Postgres: scalar, EXECUTES an arbitrary query and returns its rows.
            "SUM(mrr_eur) + LENGTH(query_to_xml('SELECT * FROM secret_table', true, false, ''))",
            // Postgres: scalar, a side effect on disk.
            "SUM(mrr_eur) + lo_import('/etc/passwd')",
            // Postgres: scalar, mutates a sequence.
            "SUM(nextval('some_sequence'))",
            // Postgres: scalar, holds the connection for as long as it likes.
            "SUM(mrr_eur) + pg_sleep(1000000)",
        ] {
            assert!(
                accepted(raw).is_none(),
                "{raw:?} was ACCEPTED: a function name is unbounded reach, and a denylist of date \
                 names does not bound it"
            );
        }
    }

    #[test]
    fn finding_3_a_comment_is_refused_under_only_one_of_seven_names() {
        // `carries(&expression, "trailing_comments")` asks about one field. The AST spells a comment
        // seven ways - `trailing_comments`, `leading_comments`, `comments`, `pre_alias_comments`,
        // `post_select_comments`, `operator_comments`, `left_comments` - and three of the others are
        // re-emitted INTO the statement. Measured renderings, all three ACCEPTED today:
        //   `SUM(mrr_eur) /* c */ + 1` -> `SUM("t"."mrr_eur") /* c */ + 1`   (left_comments)
        //   `SUM(mrr_eur) + /* c */ 1` -> `SUM("t"."mrr_eur") + /* c */ 1`   (operator_comments)
        //   `CASE /* c */ WHEN ..`     -> `CASE WHEN .. END /* c */`         (comments)
        // The `*/` escaping does hold on these paths, so this is not an injection - it is the
        // refusal in `Construct::Comment` not holding.
        for raw in [
            "SUM(mrr_eur) /* left */ + 1",
            "SUM(mrr_eur) + /* operator */ 1",
            "CASE /* leading */ WHEN 1 = 1 THEN SUM(mrr_eur) END",
            "SUM(mrr_eur) -- trailing on a line\n + 1",
        ] {
            assert!(accepted(raw).is_none(), "{raw:?} carried a comment into the statement");
        }
    }

    #[test]
    fn finding_4_an_unterminated_comment_silently_discards_the_rest_of_the_fragment() {
        // `SUM(mrr_eur) /* note */` is refused as a comment. `SUM(mrr_eur) /* note` is ACCEPTED, and
        // everything after the `/*` is dropped by the tokenizer with nothing recording that it was
        // there. That is the shape `Shape::CarriedClause` exists for - text an author wrote, silently
        // removed, and the remainder certified under the metric's name.
        let dropped = "SUM(mrr_eur) /* actually we want SUM(customer_key) here";
        assert!(
            accepted(dropped).is_none(),
            "{dropped:?} compiled to SUM(mrr_eur) with the author's text discarded"
        );
    }

    #[test]
    fn finding_5_a_direction_override_is_not_a_control_character() {
        // `SqlFragment::parse` refuses `char::is_control()`, whose stated reason is that such a
        // character's "likeliest origin is a paste accident or an attempt to hide part of a fragment
        // from a reviewer's terminal". The characters that actually do that are `Cf`, not `Cc`:
        // `is_control()` is false for U+202E RIGHT-TO-LEFT OVERRIDE and for the zero-width set.
        //
        // The fragment below renders in a reviewer's terminal, in a diff and in a browser as
        // `SUM(CASE WHEN status = 'active' THEN mrr_eur END)`, and compiles to a comparison against
        // a string that is not `active` - so the branch never fires and the certified number is zero.
        for raw in [
            "SUM(CASE WHEN status = '\u{202E}evitca\u{202C}' THEN mrr_eur END)",
            "SUM(CASE WHEN status = 'act\u{200B}ive' THEN mrr_eur END)",
            "SUM(CASE WHEN status = '\u{2066}evitca\u{2069}' THEN mrr_eur END)",
        ] {
            assert!(
                SqlFragment::parse(raw).is_err(),
                "an invisible or direction-changing character reached the compile: {raw:?}"
            );
        }
    }

    #[test]
    fn finding_6_two_dialect_keys_that_differ_only_by_whitespace_collapse_into_one() {
        // `DialectTag::parse` trims, so `duckdb` and ` duckdb ` are the same tag - and `BTreeMap`'s
        // deserialize keeps the LAST value for a repeated key. One of two authored fragments is
        // therefore silently discarded and the other is certified, with the definition digest taken
        // over the survivor. `Computation::assemble` refuses `measure` beside `authored_sql` for
        // exactly this reason: "a document that writes both means one of them, and choosing would
        // certify a number the author did not ask for".
        let json = "{\"duckdb\":\"SUM(mrr_eur)\",\" duckdb \":\"SUM(customer_key)\"}";
        if let Ok(authored) = serde_json::from_str::<AuthoredSql>(json) {
            panic!(
                "two keys became {} fragment(s), and the surviving one is {:?}",
                authored.fragments().len(),
                authored.fragments()
            );
        }
    }

    /// FINDING 7. Written as an `#[ignore]`d test, because at the time there was nothing left to assert
    /// in: it ABORTED the process.
    ///
    /// **The `#[ignore]` is gone and the assertion is unchanged**, which is the whole verdict on the fix:
    /// `parse` now bounds the tree depth immediately after the statement comes out of the parser and
    /// before anything clones or serializes it, so the compile refuses this fragment with
    /// `ExpressionError::TooDeep` and the test observes a refusal instead of a dead process. The other
    /// three shapes the review measured - 500 `+ 1` terms, a 500-deep list literal and 250 `NOT`s - are
    /// asserted with this one in `expression::tests`, together with the boundary: 30 parentheses is a
    /// tree of depth 32 and compiles, 31 is depth 33 and does not.
    ///
    /// `SqlFragment` allows 1024 characters, and the parser's own complexity guards allow 512 levels
    /// of parentheses and 512 of AST depth. Between the two, a catalog file may hold a tree 507 levels
    /// deep. Two walks over that tree are sutura's own and neither is guarded:
    ///
    /// - `projection.clone()` at the end of `parse` - `Expression`'s derived `Clone` recurses per node;
    /// - `serde_json::to_value` inside `carries` - the serializer recurses per node, and `has_field`
    ///   recurses over the `Value` it produces.
    ///
    /// The dialect layer guards its own: the parser enforces `ComplexityGuardOptions`, the generator
    /// wraps generation in `stacker::maybe_grow`, and `Drop` is iterative. Measured, all four stages
    /// separately, in a debug build on a 2 MiB stack - which is what a tokio worker thread and a
    /// spawned std thread both have:
    ///
    /// ```text
    ///   depth 200  parse ok   serde ok   generate ok   compile ok
    ///   depth 240  parse ok   serde ABORT             compile ABORT
    ///   depth 505  parse ok   serde ABORT             compile ABORT
    /// ```
    ///
    /// In a release build the same abort needs a smaller stack: `compile` at depth 505 survives on
    /// 512 KiB and aborts on 256 KiB and on 128 KiB.
    ///
    /// Depth 240 is a 492-character fragment - under half of what the domain accepts. Under
    /// `panic = "abort"` there is no unwinding to catch, and a stack overflow is not a panic anyway:
    /// the process dies. A blank line in a catalog file was closed as an abort risk; this one is open.
    ///
    /// Reproduce the abort, against the unfixed compile:
    /// `git stash && cargo test -p sutura-sql --test adversarial_findings finding_7`.
    #[test]
    fn finding_7_a_deeply_nested_fragment_aborts_the_process() {
        let depth = 240;
        let raw = format!("SUM({}mrr_eur{})", "(".repeat(depth), ")".repeat(depth));
        assert!(SqlFragment::parse(&raw).is_ok(), "the domain accepts it");
        let handle = std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(move || accepted(&raw).is_some())
            .expect("spawn");
        let accepted_it = handle.join().expect("a compile does not panic");
        assert!(!accepted_it, "a fragment this deep should be refused");
    }
}
