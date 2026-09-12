---
title: A named escape hatch for authored SQL
description: Why a metric may carry a SQL expression somebody wrote, why it is a separate named shape rather than a field on the measure, why the fragment is parsed and generated rather than transpiled, why node kinds are a denylist and function names are an allowlist, and which constructs are refused at load and why each one is on the list.
---

# A named escape hatch for authored SQL

Status: accepted, and **built but not wired.** It amends
[the closed vocabulary decision](0002-a-closed-vocabulary-for-measures.md), whose *Alternatives
considered* rejected exactly this, and it does not supersede it: the closed vocabulary stays closed,
and this record is about what sits **beside** it and how it is kept visible.

## What is built, and what is not

Read this before the rest, because the rest is written in the present tense about a path that has no
caller.

`sutura_domain::expression` and `sutura_sql::expression` are complete and tested. **Nothing reaches
them.** `catalog::Metric` holds a `Measure` and not a `Computation`; a metric document has no
`authored_sql` key; `sutura_sql::expression::compile` has no production caller. So no catalog can
express an authored metric, and every guard below is exercised by its own tests and by nothing that
reads a file. `AGENTS.md`'s *Built And Not Wired* section says the same thing from the other side, and
the three rows that used to state this as enforced were deleted from its Invariants table.

Two things are in the way, and only the first is wiring.

**A refusal in the composition root cannot be made structural today.** The *Consequences* below say a
build that renders no SQL must refuse a catalog that carries authored SQL. The strong form of that -
make it unrepresentable, by having the authored variant carry a value only `compile` can produce - is
not available:

- The witness cannot live in `sutura-domain`. It would have to be keyed by dialect, and the domain
  deliberately does not own the list of data systems this build renders for - `DialectTag`'s own
  documentation gives the reason, and it is the same reason `Measure` does not restate the aggregates.
  A domain-side witness would also have a public constructor, which every crate that depends on the
  domain can call, so it would witness nothing.
- The witness cannot live in `sutura-sql` and still be held by a servable bundle, because the bundle
  is a domain type and `sutura-serve` may not reach `sutura-sql`. `ALLOWED_IN_DOMAIN` is an allowlist
  over the domain's whole transitive tree and `polyglot-sql` is not on it, so the edge fails
  `cargo xtask check-boundaries` outright; and that gate's own record says it walks the
  whole-workspace resolve graph including edges feature unification turns on, so a Cargo feature does
  not buy an exception either.
- The strongest remaining form is a hidden constructor in the domain plus a gate asserting it is
  called from exactly one file. That is a mechanism, and it is deliberately not taken: it is the same
  class as a `disallowed-methods` entry - a text rule about a call site, which this repository has
  already had read as enforcement while resolving to nothing - and it would not change the refusal in
  the composition root from a runtime one, because the loader would still have to be handed a
  compiler and still have to refuse when it was not.

What remains available is a refusal inside the one function that loads a catalog, which no caller can
skip because there is no other path - a placement, not a type. That is a weaker guarantee than the
rest of this record relies on, and it is stated here rather than implied.

**No shipped binary could execute an authored expression even with the load path wired.**
`sutura-exec-datafusion` is the engine, builds a logical plan over Arrow and compiles no `sql`
feature, so `Computation::measure()` returning `None` is a refusal there and not an execution path.
`sutura-exec-duckdb` renders through `sutura-sql` and pushes down, and it is a dev-dependency that no
binary links. So wiring the load alone would move the refusal from load time to query time. Which
composition root gets an execution path for authored SQL is a decision this record does not make.

## Context

Two things pushed against the closed vocabulary from opposite directions, and neither is a metric
that a wider `Measure` would fix.

**Some metrics need constructs a closed vocabulary cannot hold.** A window function, a percentile, an
expression over two columns - `SUM(price * quantity)` - a `sumIf` over one dialect's own aggregate.
Adding a `Term` per construct is the mistake
[0002](0002-a-closed-vocabulary-for-measures.md#two-levels-not-three-siblings) already recorded once
in a smaller form: at some point the vocabulary stops being a vocabulary and becomes a badly-typed
expression language with a bespoke parser.

**Some providers already carry SQL per metric.** A wren cube holds
`SUM(CASE WHEN status = 'active' THEN mrr_eur END)` in the file. There is nothing to map that onto,
so a provider whose catalog is written that way arrives as "unsupported" for its entire metric set -
not as a degraded import, as no import. And this is the point where "not all providers have all
capabilities" stops being an abstract statement: a wren-style directory has authored SQL because a
person wrote the file, and a metadata service that stores no executable SQL per metric, or an RDF
vocabulary that never will, has none and is *complete* rather than degraded.

0002's rejection of free-text SQL gave two reasons. The first - "a parse failure becomes a runtime
refusal instead of a review comment" - is answerable by parsing at load, which 0002 itself concedes
is better. The second - "a catalog that loads on one version and fails to parse on the next fails
readiness for every metric in it" - is real and is not answered. It is accepted as a cost here, and
it is bounded: it applies only to metrics that use the hatch, and the failure is a loud load failure
naming the metric, the dialect and the position, which is the failure mode this repository prefers.

## Decision

**A separate, explicitly-named shape beside the measure - never a field on it.**

`Computation` has two variants and a metric says which in a word:

| Variant                    | On disk         | Who produces it                                         |
| -------------------------- | --------------- | ------------------------------------------------------- |
| `Computation::Measure`     | `measure:`      | Every provider. The ordinary case, and the default path |
| `Computation::AuthoredSql` | `authored_sql:` | Only a provider whose catalog carries SQL per metric    |

The shape matters as much as the capability. There is no `expression:` key on a measure, no
`Option<String>` beside one, and no arrangement in which "this metric is free-text SQL" is invisible
in a diff or absent from an operator's listing. `Computation::kind()` returns the word, so "which
metrics use the hatch" is one accessor over the pinned definitions rather than a grep over files.
Writing both keys is refused rather than resolved by precedence, for the reason a two-termed term is:
a document that writes both means one of them, and choosing would certify a number nobody asked for.

`AuthoredSql` is a **map from a dialect word to a fragment**, with `portable` reserved for "every
target". Resolution is exact dialect, then `portable`, then **refuse**. That third step is a
deliberate departure from wren's own OSI importer, which falls back to the first non-empty variant:
that hands a Postgres query a Snowflake expression because it happened to be listed first, which is a
number computed by a definition nobody chose, under a certified name. A dialect word that is not one
this build renders for is also a load failure, not a variant silently never chosen - otherwise a
`postgresql:` beside a `portable:` means Postgres quietly gets the portable text and nobody learns
that the variant written for it was never read.

The domain holds the fragment as **text** and no more. It checks that a fragment is present, bounded,
and free of the characters that make the text a reviewer reads differ from the text that compiles,
and it owns none of the SQL judgement, because it has no parser and `cargo xtask check-boundaries`
keeps it that way.

Two of those checks are worth naming, because both were found by attacking the text rather than by
reasoning about it.

**`char::is_control` is not the check its own reason asked for.** The refusal's stated purpose is to
stop "an attempt to hide part of a fragment from a reviewer's terminal", and the characters that do
that are general category `Cf`, not `Cc` - so `is_control` is false for every one of them and they
passed. `SUM(CASE WHEN status = '<U+202E>evitca<U+202C>' THEN mrr_eur END)` renders in a terminal, in
a diff and in a pull request as `status = 'active'` while comparing against something else, so the
branch never fires and the metric certifies zero under a name a reviewer approved. That is Trojan
Source (CVE-2021-42574) pointed at a metric definition, and the definition digest covers the text
faithfully while the text is not what the reviewer read. `InvalidFragment::InvisibleCharacter` is a
second refusal beside the first, over an enumerated set of ranges rather than the `Cf` category: a
category test would move with the Unicode table under a dependency bump, which for a load-bearing
refusal is a set that changes with no diff.

**A dialect word is not trimmed.** `DialectTag::parse` used to trim, which made `duckdb` and
`duckdb` the same tag - and `BTreeMap`'s deserialize keeps the LAST value for a repeated key, so
`{"duckdb": A, " duckdb ": B}` silently discarded `A` and certified `B`, with the digest taken over
the survivor. That is the outcome `Computation::assemble` refuses when a metric writes `measure`
beside `authored_sql`, arrived at without anybody writing two keys on purpose. Surrounding whitespace
is now an `IllegalCharacter` load failure. `SqlFragment` still trims, deliberately: two fragments
that differ only by surrounding whitespace are the same fragment, where two map keys are two keys.

### Parse and generate, at load, per dialect

`sutura_sql::expression::compile` does the work, at catalog-compile time, for **every** dialect the
build renders for at once. What it stores is the rendered string per target; what reaches a statement
at query time is that string, verbatim, and nothing is parsed on the query path.

`Dialect::parse` then `Generator::generate`, never `Dialect::transpile`. The reason is stronger than
"we banned translation":

- `TranspileOptions::default()` sets `unsupported_level: Warn`. An unsupported construct returns
  `Ok(sql)` and pushes a diagnostic into `unsupported_messages`, which `Dialect::transpile` then
  **discards**. The default failure mode of the convenient call is silent wrong output.
- Setting the level to `Raise` is not a usable net either. Measured: it errors on every non-count
  aggregate targeting `ClickHouse` - `SUM`, `AVG`, `MIN`, `MAX` all `Err` - while staying silent on
  all four of the breakages below.
- Parse-and-generate is also *more faithful* for the aggregation subset, and needs no feature this
  build does not already compile. Measured byte-identical across all sixteen (read, write) pairs over
  `DuckDB`, Postgres, `ClickHouse` and `BigQuery` for the conditional sum, the guarded ratio,
  `COUNT(DISTINCT k)`, `AVG`, `COALESCE`, a bare `CASE`, `MIN`/`MAX` and
  `ARRAY_AGG(DISTINCT .. ORDER BY ..)`, with `CAST(.. AS DOUBLE)` retargeting per dialect.

The fragment is parsed as `SELECT {fragment}` rather than through the dialect layer's fragment API,
and that is not a stylistic preference. `Parser::new(dialect.tokenize(x))` followed by
`parse_expressions()` **panics** on an empty token list, which is what `""`, whitespace-only and
comment-only input all tokenize to, in all four dialects. Under `panic = "abort"` a blank line in a
catalog file would end the process. `clippy.toml` now bans both methods, and the ban was confirmed to
resolve by writing the call and watching clippy reject it - an unresolvable path in
`disallowed-methods` is silently ignored, so an unverified entry would read as enforcement and do
nothing.

**The generator panics on non-ASCII text, so the fragment is bound to ASCII.** Found by the
`sql_expression` fuzz target on a fragment containing the replacement character (a lossy-decode of
four invalid bytes): the pinned `polyglot-sql 0.9.2` `Generator` byte-slices a string without
respecting character boundaries and panics with *"start byte index N is not a char boundary"* at
`generator.rs:19076`. There is no third-party error to map - it aborts, which under
`panic = "abort"` is the process dying. `ExpressionError::NonAscii` refuses a fragment carrying any
non-ASCII character before it is handed over, which is the bound the aggregation subset this hatch
exists for already lives inside (SQL keywords, identifiers and the allowlisted functions are ASCII
by construction). The exact failing input is quarantined as the committed seed
`fuzz/seeds/sql_expression/non-ascii-crash`, and the defect belongs upstream: `polyglot-sql` is
`github.com/tobilg/polyglot`, and the generator's byte-slicing there is what the bound is a workaround
for. None of that changes what a catalog can carry until a pin fixes it - the bound is a control over
the dependency, not a judgement that authored SQL is ASCII.

**The parser does not return on a parenthesis with no closer, so the fragment is bounded to closed
parentheses.** Found by the `sql_expression` fuzz target as a 26-byte timeout and reduced to six
characters, `a.:S1(`: `.:` reads the next word as a custom data type, and the argument loop in the
pinned `polyglot-sql 0.9.2` `Parser::parse_data_type` breaks only on `check(TokenType::RParen)` -
which answers `false` at the end of the token stream, while `advance()` past the end returns the
last token *without* moving the cursor. The loop therefore cannot terminate, and every turn of it
does `*last = format!("{} {}", last, token.text)`. **One upstream defect, two report shapes:** it
reads as a timeout while that string is being copied and as an out-of-memory once the string is
large, and `MAX_DEPTH` cannot see either because no tree is ever built. Under `panic = "abort"`
unbounded work on an authored fragment is the process never coming back, which on the query surface
is a denial of service rather than a slow load.

`ExpressionError::UnclosedParenthesis` refuses the enabling condition every scan-to-a-closer loop in
that parser needs, which is why the bound is on the class rather than on the route the first
artifact took. **The question is asked of the TOKENS the authoring dialect produces and not of the
text**, and that is the load-bearing half: a count of `(` against `)` fails open exactly the way the
comment-delimiter count did, one character class over, because in `mrr_eur.:S1(')'` the two
characters balance - one `(`, one `)` - while the `)` is a string literal the tokenizer hands over
as a single token and never as an `RParen`. Asking the tokenizer costs nothing that was not going to
be spent and has no second scanner to disagree with about dollar-quoting. A `)` with no opener is
deliberately left to the parser, which errors and returns.

**The construct is the wrong axis, and that is measured rather than argued.** Two more artifacts -
a second timeout and an out-of-memory, both under 27 bytes - reproduce the same non-return, and all
three carry `.:`, which invited a bound on the construct instead. `.:` is neither necessary nor
sufficient on the pinned 0.9.2: `mrr_eur.:S1(9)` parses and returns, so refusing the construct
would refuse a harmless spelling, while `CAST(mrr_eur AS S1(9` loops with no `.:` in it at all, so
it would still miss one. `mrr_eur::S1(`, `mrr_eur::DECIMAL(` and `mrr_eur::STRUCT(a` all error and
return, so an earlier wording here that named `::` beside `CAST` was wrong about that half. Nor is
the closer's *character* the axis: an unclosed `[`, an unclosed `<`, and both inside a `CAST`, all
error and return, so what the parser cannot leave is an open parenthesis and nothing else.

The failing inputs are quarantined as the committed seeds
`fuzz/seeds/sql_expression/unclosed-paren-timeout`, `unclosed-paren-timeout-in-subscript` and
`unclosed-paren-oom`, byte-identical to their artifacts, so `just fuzz-smoke` replays them on every
run. **The defect belongs upstream and this does not fix it:** the bound refuses the pathological
fragment before it reaches that parser, and any other caller of `polyglot-sql` in any other project
is unaffected by it.

**The bound has no accepted cost of the comment refusal's kind, and an earlier wording here claimed
one.** It said the guard accepts the same class - a parenthesis inside a string literal, refused for
being unbalanced in the text - and that is refuted by measurement:
`SUM(CASE WHEN status = '(' THEN mrr_eur END)` is **accepted**, while the `'--'` spelling of the same
fragment is refused by the comment guard. Reading the TOKENS is what removes the cost, which is the
paragraph above's whole point; the comment refusal pays it because it reads TEXT and may not ask the
tokenizer where a literal ends. What this guard refuses beyond the pathological set is a fragment
that is text-balanced but token-unbalanced - `mrr_eur.:S1(')'` - and the parse would have rejected
that anyway. So: a parenthesis inside a string literal is accepted, unlike a comment delimiter
inside one. The accepted case is asserted, so a repair towards the sentence that used to stand here
is red rather than stricter - and that assertion is the one the suite did not have. A naive count
was already caught in the *other* over-refusal direction, by
`a_fragment_that_escapes_its_own_parentheses_is_refused_naming_a_position` over `SUM(mrr_eur))`, an
extra CLOSER; what that cell notices is one refusal arriving instead of another, which a
stricter-but-correct guard could also produce. Nothing held a fragment a count refuses while it is a
fragment a catalog would really write.

**What the bound does not cover.** It does not reach the defect: a `polyglot-sql` caller that is not
this module is exposed exactly as before, and a second caller inside this repository would be too.
It bounds the parenthesis and nothing else, so a future upstream loop scanning for a different
closer is outside it - none of the bracket and angle spellings above loops today, and that is a
measurement of one pinned version rather than a property of the parser. And where the fragment does
not TOKENIZE the guard says nothing at all and the parse it falls through to is what terminates;
that holds because the parse tokenizes before it descends, which nothing mechanical enforces and no
cell can prove, since a base tree with no guard returns on those inputs too.

It also asks a DIFFERENT entry point from the one it is guarding, which the wording above hides by
saying *the tokenizer the parse is about to run*: that is true of the type and not of the call. The
guard calls `Dialect::tokenize`, which is `Tokenizer::tokenize` over `Token`; the parse goes
`Dialect::parse` to a private `parse_with_guard` to `Tokenizer::tokenize_for_parser`, the same state
machine and the same config instantiated over `ParserToken`, behind an input-size check this call
does not make. What holds their parenthesis depths equal is the dependency's own
`guard::token_guard_tests`, over one balanced input, comparing the two streams' guard *verdicts*
rather than the streams - so nothing in this repository holds it.

**And it bounds non-termination, not superlinear work, which is a second class the same target
found.** A 900 s run on the guarded tree crashed nothing and timed nothing out, and wrote three
`slow-unit` artifacts of 287, 354 and 388 bytes. **None of them is refused here:** each tokenizes
with its parentheses balanced, so this guard passes it, and each then reaches the parse and *comes
back* - with a parse error, after the work is already spent. Measured one input at a time, the parse
alone in a release build with no sanitizer: **0.79 s, 13.1 s and 14.6 s**, and every figure in this
section is +-10% run to run because a concurrent build moves it. Through the fuzz harness, which is
ASan plus sancov, the same three cost 12.7 s, 209.4 s and 224.7 s - a ~16x
instrumentation factor, and the reason the run's own `slowest_unit_time_sec: 241` is not the number
a deployment would pay. So ~14 s of parse on a shipped profile is the **measured worst case among
the recorded artifacts**, and it is emphatically **not a ceiling**: the constructed case below is
worse in a quarter of the bytes, and inside `MAX_FRAGMENT_LEN` the curve leaves *slow load* behind
entirely. What the RECORDED three are is a slow load; what the class is, is the first paragraph of
this section again - unbounded work on an authored fragment - reached by a different route and with
nothing here refusing it.

Where the time goes is not a guess: every stack in a six-second sample of the slowest artifact is
inside `polyglot_sql::parser::Parser`, in the precedence chain re-entered through `Parser::parse_if`
and `Parser::parse_expression_inner`, with no frame of ours below `compile` calling `Dialect::parse`.
It is re-parsing, not one long loop.

**All three reduce to the same shape, and the shape is `IF`.** Delta-debugged against a
300 ms predicate they go from 287, 354 and 388 bytes to 70, 66 and 71, and each reduction is a chain
of roughly seventeen `IF~` pairs with a tail that cannot parse - for instance
`IF~I~IF~IF~IF~IF~IF~IF+I?{IF+IF~IF+IF~IF~IF~IF~IF~IF~_F~IF%+I?{E|N`. Constructed rather than
reduced, `("IF~" * k) + "I?{"` doubles per `IF`: 0.004 s at k=12, 0.055 s at 16, 0.76 s at 20,
3.3 s at 22, **13.8 s at k=24 - and that fragment is 75 bytes**. Two facts make it the `IF` and not
the chain. The same chain without the failing tail, `("IF~" * 30) + "1"`, parses in under a
millisecond, because a parse that SUCCEEDS commits instead of backtracking - so the cost is only
ever paid on a fragment that is going to be refused anyway. And of sixteen words tried in the same
chain at k=20 - `ABS CASE CAST COALESCE EXISTS EXTRACT IF INTERVAL NOT NULLIF POSITION SUBSTRING SUM
TRIM TRY_CAST` and a bare identifier - **only `IF` is superlinear**; the other fifteen return in
under a millisecond.

**No bound is written for it, and that is the decision rather than an omission.** Bounding *slow*
needs a complexity or work measure and none is available here. Length is refuted, decisively: the
75-byte fragment above costs 14 s, while `SUM(CASE WHEN status = 'active' THEN mrr_eur ELSE 0 END)`

- the metric this hatch exists for - is 56 characters, so any length cap that refuses the exploit
  refuses the metric. And `MAX_FRAGMENT_LEN`'s own 1024 characters hold about 340 `IF~` pairs, which
  on the measured doubling is not a wait anybody outlives - arithmetic on a measured curve, and not a
  run. The parser's own `ComplexityGuardOptions` sit far above these inputs -
  `max_tokens` at 1,000,000, `max_ast_depth` and `max_parenthesis_depth` at 512,
  `max_function_call_depth` at 64 - and none counts work per token; `MAX_DEPTH` is asked once the parse
  has already paid. A wall-clock budget is the mechanism that would fit, and the pinned parser offers
  no cancellation point for one: a watchdog could observe the deadline and could not reclaim the
  thread, and under `panic = "abort"` there is nothing to unwind.

**The obvious candidate, and why it is left on the table rather than taken.** This guard already
holds the token stream, `IF` is not one of the names `Construct::UnknownFunction` allows, and the
four allowlisted names that contain the letters - `COUNTIF`, `COUNT_IF`, `SUMIF`, `SUM_IF` - carry
**zero** `If` tokens, measured, while `IF(status, mrr_eur, 0)` carries one. So refusing a fragment
whose tokens hold `TokenType::If` would cost nothing an author can reach today and would close every
input above. It is not taken here for one reason: sixteen words is not the keyword set, so it bounds
the ROUTE this run happened to find and would read as bounding the CLASS - which is the mistake the
paragraph about `.:` above exists to avoid, and an overstated control is worse than a recorded one.
Taking it is a behavioural change with its own variant, its own generated page and its own review,
and the bar is the one this section's guard met: 32 authored fragments with zero over-refusals.

The three inputs are **deliberately not committed as seeds.** `just fuzz-smoke` replays every seed
in `fuzz/seeds/<target>/` and runs as a pre-commit hook, so a 225 s seed takes that hook from
seconds to minutes and makes committing unusable. The reduced forms are cheap enough to commit -
0.2 to 0.3 s each - and are still not committed, because with no bound there is nothing for a
replay to be a regression against: it would assert that a parse still returns slowly.

### The fragment is bounded in depth as well as in length

`MAX_FRAGMENT_LEN` bounds the text at 1024 characters and the parser's own `ComplexityGuardOptions`
bound nesting at 512 levels, which between them let a catalog file hold a tree 507 levels deep. Two
walks over that tree are **ours** and neither was guarded: `projection.clone()` at the end of `parse`,
because `Expression`'s derived `Clone` recurses once per node, and `serde_json::to_value` inside
`carries`, which recurses once per node before `has_field` recurses again over the `Value`. The
dialect layer guards its own - the parser enforces its complexity limits, the generator wraps
generation in `stacker::maybe_grow`, and its `Drop` is iterative - so those two were the whole
exposure.

A stack overflow is **not a panic.** `panic = "abort"` is beside the point: there is no unwinding to
catch, and the process dies. Measured, in a debug build on a 2 MiB stack - which is what a tokio
worker thread and a spawned std thread both have - four ordinary fragments each under the length
bound aborted the process: 500 `+ 1` terms, a 500-deep list literal, 250 `NOT`s and 505 parentheses.
In a release build the same abort needs only 128 KiB, which is musl's default main-thread stack.

`parse` therefore bounds the depth at `MAX_DEPTH = 32`, immediately after the statement comes out of
the parser and **before anything clones or serializes it** - a guard in `check` would be too late,
because the clone is inside `parse`. `ExpressionWalk::tree_depth` is iterative, so asking the
question costs no stack at all. The number is the small half of the decision: counting the wrapper's
`SELECT` as a level, the conditional sum this hatch exists for nests four and the deepest fragment any
test in this repository needs is five, while 30 parentheses is a tree of depth 32 that compiles and 31
is 33 and does not.

**The authoring dialect is `DuckDB` and may not be `ClickHouse`.** Measured: ClickHouse's parser
accepts `SUM(x))` and `x) FROM secret --`, silently dropping the tail. That is injection-shaped input
passing validation. DuckDB rejects both, and rejects `SUM(x) garbage garbage`, `SUM(x), COUNT(y)`,
`SUM(x) AS foo`, `SUM(x) FROM t` and `1; DROP TABLE t` through the four shape guards.

### Column references are checked, and qualified

An unknown column **fails the load**. Wren has both behaviours: its cube path does not check column
references and its own documentation tells the agent to expect a runtime error from the warehouse,
while its model path does check them, through a schema-driven AST rewrite. The model path is right. A
metric whose fragment names a column that does not exist is broken whether or not anybody asks about
it, and the difference between finding out at load and finding out at query time is the difference
between a refusal an operator can fix and a stack trace an agent shows a user.

A qualifier written by hand is refused, and the compile qualifies every column itself with the
metric's model table - the same rule the plan already follows, for the same reason: an unqualified
column in a statement that later grows a join binds to whichever table happens to have it, and that
is a wrong number rather than an error.

**And the compile asserts that it did, rather than trusting the rewrite.** The rewrite runs inside
`traversal::transform_map`, whose child coverage is NARROWER than the traversal API's: it dispatches
on a hardcoded list of node kinds and returns a kind it has no arm for untouched, children and all -
its own comment says so, and `WithinGroup`'s adds that it does not descend into `this`. The
unknown-column check is a `dfs` walk, which has the wider coverage. So the check saw columns the
rewrite never reached, and eight measured fragments passed every guard and emitted a bare column:
`MAX(x COLLATE ..)`, `SUM(x) SIMILAR TO ..`, `SUM(x) WITHIN GROUP (..)`, `ARRAY_AGG`/`LIST`/
`GROUP_CONCAT` with an `ORDER BY`, and both placements of `IGNORE NULLS`.

The cost of that is not a load failure. Measured in DuckDB 1.5.5 for the `COLLATE` case against a
joined dimension carrying the same column name: `Binder Error: Ambiguous reference to column name
"region"` - at query time, for a metric whose load SUCCEEDED, so the questions that join nothing are
answered and the ones that join fail. That is exactly what load-time checking exists to prevent, and
on an engine that resolves by precedence rather than erroring it is silently the wrong number.

So after `qualify`, any `Expression::Column` with no table is `ExpressionError::NotQualified`.
Asserting the postcondition rather than widening the rewrite is the choice: the walk belongs to the
dialect layer, so a version of it that gains an arm makes more fragments compile and none escapes the
check either way.

### The denylist

Four constructs are refused at catalog-validation time. **Nothing upstream errors on any of them**,
which is the whole reason the list exists.

| Refused                       | Why                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| ----------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `FILTER (WHERE ..)`           | `aggregate_filter_supported` is set `false` by six dialects and is **never read anywhere in the dialect layer**. FILTER is emitted unconditionally for every target, including the six that cannot run it                                                                                                                                                                                                                                     |
| `COUNT(DISTINCT a, b)`        | Gated by `multi_arg_distinct`. `DuckDB` to `DuckDB` rewrites it into `COUNT(DISTINCT CASE WHEN a IS NULL THEN NULL WHEN b IS NULL THEN NULL ELSE (a, b) END)` while `ClickHouse` leaves it alone, so identity is not identity and the same fragment counts different things per target                                                                                                                                                        |
| Any date or time function     | Truncation is the generator's, from the question's `Grain`, so no metric needs one - and `generator.rs`'s `generate_date_trunc` special-cases only TSQL/Fabric and ClickHouse, so a fourth dialect's argument order is a live defect waiting for that dialect to be compiled. It is a `generate`-level defect, so avoiding `transpile` does not fix it                                                                                        |
| A bare `/` between aggregates | `DuckDB` to Postgres inserts `CAST(.. AS DOUBLE PRECISION)`, `DuckDB` to `BigQuery` does not, and Postgres to anything is left alone. **The transpiler changes the NUMBER depending on which dialect is declared as the source.** An explicit `NULLIF` suppresses the rewrite, which is why the divisor must be one - and it makes the zero-denominator behaviour explicit, which is what `ZeroDenominator` records for the closed vocabulary |

Plus `x IS TRUE`, already recorded as not portable in
[the architecture notes](../architecture.md), and a set of structural refusals that are about *reach*
rather than portability: a subquery, a table reference, a star, a bind placeholder, a schema
statement, a node the generator emits with no handling at all, and a fragment that aggregates
nothing.

### Node kinds are a denylist. Function names are an allowlist

This reverses what *Alternatives considered* decided below, and the reversal is confined to names.

A generic function call is the case where the dialect layer has no typed node, so it emits the name
**verbatim into every target with no lowering of any kind**. The old check compared that name against
the date-and-time list and let everything else through, on the stated grounds that a UDF was the
author's claim to make. Measured against DuckDB 1.5.5, that was not a claim about portability but
about reach: the rendering of `MAX(getenv('X'))` is
`SELECT (MAX(GETENV('X'))) FROM fact_subscription`, and with `SUTURA_SECRET_PROBE` set in the process
that statement **answered the value of the environment variable.** Every secret the sutura process
holds - a service-account path, a warehouse password, a token - was readable that way, under a
certified metric name, out of a catalog file. `Construct::TableReference::why` states the invariant it
breaks: "a measure expression may read only the columns its own model declares". Also accepted at
compile, from one afternoon over three manuals: Postgres `query_to_xml` (executes arbitrary SQL and
returns its rows), `pg_read_file`, `pg_ls_dir`, `lo_import` (writes to disk), `nextval` and `setval`
(mutate a sequence), `pg_sleep` and `dblink`; ClickHouse `dictGet`, `getSetting` and `sleep`; DuckDB
`current_setting`.

**A denylist over function names cannot bound that space.** It is every function every target has,
plus every user-defined one, plus every function a future version of any of them adds. So the
direction is reversed for names: `Construct::UnknownFunction` refuses every name that is not on a
short list, and the refusal **names the list**, generated from the list itself so the sentence an
author reads cannot disagree with the check that refused them.

The count argument below does not transfer, and that is the whole reason this is a reversal rather
than a contradiction. It was made about node KINDS - "some six hundred", and enumerating the ones
that are fine would recreate the closed vocabulary - and it still holds for them, which is why kinds
are still a denylist backed by `traversal::is_query` and `traversal::is_ddl`. The set of *names a
measure needs* is a different set: percentiles, a null guard, a rounding, a min/max pair, and the
aggregates. It fits on one screen:

`ABS`, `ARRAY_AGG`, `AVG`, `CAST`, `COALESCE`, `COUNT`, `COUNTIF`, `COUNT_IF`, `GREATEST`,
`GROUP_CONCAT`, `LEAST`, `LIST`, `MAX`, `MEDIAN`, `MIN`, `NULLIF`, `PERCENTILE_CONT`,
`PERCENTILE_DISC`, `ROUND`, `STDDEV`, `STDDEV_POP`, `STDDEV_SAMP`, `STRING_AGG`, `SUM`, `SUMIF`,
`SUM_IF`, `TRY_CAST`, `UNIQEXACT`, `VARIANCE`, `VAR_POP`, `VAR_SAMP`.

Three things follow.

**A variant needing one more name is a review-visible edit to that list**, which is precisely the
property the hatch exists to have: a metric using SQL is already a named, greppable shape, and now so
is the vocabulary it may use. Matching is `eq_ignore_ascii_case`, so one entry covers a dialect's own
casing - `SUMIF` is how `sumIf` is written - while a spelling differing by more than case, such as
`stddevPop` beside `STDDEV_POP`, needs its own line.

**The residual date fail-open this record used to state is closed.** A date function spelled with a
name not on the date list and with no typed node used to render verbatim; it is now refused as a name
outside the allowlist. The date list survives because it gives such a name a refusal that says *date
function* and points at the argument-order defect, which is a better sentence for an author than "not
on the list".

**Each name carries whether calling it aggregates**, and that mark fixed a second defect in the other
direction. `traversal::contains_aggregate` classifies by node kind, so it is true for `SUM(x)` and
`sumIf(x, p)` and **false** for `uniqExact(k)` - a real ClickHouse aggregate the dialect layer has no
node for. A per-dialect variant nobody could write any other way was therefore refused as
`NotAggregated`. The mark is asked only for names that classifier has no node for, so the aggregate
node kinds are still written down in exactly one place, which is upstream; a test parses
`NAME(mrr_eur)` for every aggregate-marked name and asserts the two agree, with the one exception
enumerated rather than implicit.

### Two more holes in the comment refusal

Both are the refusal not holding rather than the escaping failing - `*/` and `/*` are escaped on
every path that re-emits a comment, tried and confirmed.

**A comment was refused under one of the seven names the AST spells it.** `carries` asked about
`trailing_comments`; the AST also has `leading_comments`, `comments`, `pre_alias_comments`,
`post_select_comments`, `operator_comments` and `left_comments`, and three of those six are re-emitted
INTO the statement - measured, all three accepted: `SUM(x) /* c */ + 1` through `left_comments`,
`SUM(x) + /* c */ 1` through `operator_comments`, and `CASE /* c */ WHEN ..` through `comments`, which
moves it to the end of the `CASE`. Up to a thousand characters of catalog prose between our own
generated tokens. The question is now asked by SUFFIX, so a field nobody has seen yet is covered -
the same argument this file already makes for asking the serialization rather than the type.

**An unterminated `/*` silently discarded the rest of the fragment.**
`SUM(mrr_eur) /* SUM(customer_key) is what runs` was accepted, with everything after the `/*`
swallowed by the tokenizer and nothing left in the tree to record that it was there. That is the same
family as the dropped clause below - the one this record calls the worse of the two - reached through
the tokenizer rather than the parser, which is why the question is asked of the TEXT: by the time
there is an AST, the evidence is gone.

The first form of that question was a count of `/*` against a count of `*/`, and this record used to
say it FAILED CLOSED. **That was wrong: it failed OPEN, and the correction is the second finding
rather than a rewording of the first.** A `*/` inside a string literal balances a later unterminated
`/*`, so the counts agreed and the fragment was accepted with its tail gone. Measured in this build:

```text
SUM(CASE WHEN status = '*/' THEN mrr_eur END) /* SUM(customer_key) is what runs
  -> ACCEPTED, rendered for every target as
     SUM(CASE WHEN "fact_subscription"."status" = '*/' THEN "fact_subscription"."mrr_eur" END)
```

Nothing else could have caught it, and not by luck: the trailing text is in no node, so the
seven-name comment question cannot see it, and the whole statement and the projection render
identically, so the dropped-clause guard cannot either. `'a*/b'` does the same with the delimiter
buried mid-word.

The question is now **presence** - any `/*`, any `*/`, any `--`, anywhere in the text - and presence
is chosen over the two narrower fixes because it needs no agreement with anybody. A string-literal
skip would have to end a literal exactly where the authoring dialect's tokenizer ends one, and DuckDB
has dollar-quoting and escape-string forms, so `SUM(mrr_eur) || $$'$$ /* tail` is a literal holding a
quote followed by an unterminated comment to the tokenizer and a literal that never closes to a
one-state scanner: the delimiter is hidden again, by a construct the scanner has not been taught. That
is the second tokenizer this module exists not to have. A comment delimiter the tokenizer can see must
appear in the bytes, so refusing the three digraphs refuses every comment, terminated or not, wherever
the tokenizer thinks it begins.

The cost is the same class of false positive the count already had, one digraph wider: a string
literal holding `/*`, `*/` or `--` is refused. Accepted for the reason it was accepted before - a
measure has no reason to compare a column against a comment delimiter, and none of the three has any
other meaning in SQL, `--` between two operands being a comment too.

### Two holes the four shape guards do not close

Both were found by testing rather than by reasoning, and both are recorded because the shape guards
read like a boundary and are not one.

**A scalar subquery in the projection.** `SUM(x) + (SELECT secret FROM secret_table)` is one
statement, one expression, no `FROM` and no alias - it passes all four guards - and it reads a table
the plan never granted. Closed by refusing any query node and any table reference, using the dialect
layer's own `is_query` and `is_ddl` classifiers as well as our lists.

**A clause that taking the projection would discard.** `SELECT 1 WHERE true` is legal in the
authoring dialect with no `FROM` at all, so `SUM(x) WHERE secret = 1` parses as one statement with one
projection and no `FROM`, and taking `expressions[0]` **throws the `WHERE` away**. The metric would
then be certified as `SUM(x)`, silently, over a predicate its author wrote and nobody removed on
purpose. Confirmed for `WHERE`, `GROUP BY`, `HAVING`, `QUALIFY`, `ORDER BY`, `LIMIT`, `WINDOW` and a
leading `DISTINCT`: each parses, each is dropped. Closed by rendering the whole wrapper statement and
the projection alone and requiring them to differ by exactly `SELECT` - a check on the *rendering*
rather than on a list of `Select` fields, so it holds for a clause nobody thought of.

A comment is refused too, and it is the mildest of the three: `SUM(x) -- note` is re-emitted as
`SUM(x) /* note */` **into** the statement, putting catalog text between our own generated tokens.
The generator does escape a closing `*/` into `* /` - measured, so it is not an injection - but a
measure has no reason to carry prose that the document around it can hold instead.

## Consequences

- A catalog that uses the hatch cannot be loaded by a build that renders no SQL. `sutura_sql` is
  where the compile lives, and the network binary does not link it. That is not a gap to paper over:
  a build that cannot validate authored SQL must refuse a catalog that carries it, rather than serve
  the metric unvalidated. The composition root is what must call the compile, and a `Computation`
  that has not been through it is unvalidated. **"By construction" is what this sentence used to say
  and it is not available** - see [*What is built, and what is not*](#what-is-built-and-what-is-not)
  for why a witness type cannot be placed anywhere the bundle can hold it, and for the weaker
  placement that is available instead. No such refusal is written today, because no catalog can
  express an authored metric today.
- The engine adapter cannot execute an authored expression at all. It builds a logical plan over
  Arrow and its `sql` feature is deliberately not compiled. So `Computation::measure()` returning
  `None` has to be a refusal there, naming the metric - never a skipped metric and never a
  substituted measure.
- **Portability is the author's claim, not ours.** The compile proves the fragment is one expression
  over declared columns, that it holds none of the refused constructs, and that each rendering parses
  in its own target's dialect. It does **not** prove the target *has* the function: `MEDIAN(x)` and
  `COUNT_IF(x)` are emitted verbatim into Postgres, where neither exists, and `PERCENTILE_CONT`
  likewise into ClickHouse. No per-dialect function catalogue is compiled into this build. Per-dialect
  variants are how an author discharges that claim precisely, and a dialect with no variant and no
  `portable` fragment is refused rather than guessed at.
- **The re-parse in `render` proves well-formedness and nothing about meaning.** It renders for the
  target, parses the result back in that target, and DISCARDS what it parsed - so it cannot see a
  construct whose meaning changed on the way out. Measured, this build:
  `TRY_CAST(SUM(mrr_eur) AS DOUBLE)` renders as `TRY_CAST(.. AS DOUBLE)` for `DuckDB` and as
  `CAST(.. AS DOUBLE PRECISION)` and `CAST(.. AS Nullable(Float64))` for Postgres and `ClickHouse`,
  which turns null-on-failure into an error raised at query time; `SUM(mrr_eur)::VARCHAR` renders
  three ways - `CAST(.. AS TEXT)`, `CAST(.. AS VARCHAR)` and `.. ::VARCHAR`; and a `json_extract`
  becomes a differently-named function with a rewritten path expression. This is the same class as the
  four denylisted defects and is not fixed here: comparing the two ASTs would need a semantic
  equivalence the dialect layer does not offer. It is stated so that the guard is not read as more
  than it is.
- **`RenderedDoesNotParse` is weak for `ClickHouse` specifically.** Its parser accepts `SUM(x))` and
  `x) FROM secret --` by discarding the tail - which is why it may not be the authoring dialect - and
  the same laxity means a malformed rendering targeted at it is less likely to come back as an error.
  The guard is worth having for the two strict targets and is not evidence for the third.
- The residual fail-open on the date denylist is CLOSED, by the allowlist above: a date function with
  an unlisted name and no typed node is now refused as a name outside the allowed set rather than
  rendered verbatim.
- Definition digests do not move. `Computation` is externally tagged so that flattened into a metric
  document a closed-vocabulary metric serializes as the `measure:` key it already had.
- The definition digest now covers a string somebody wrote. A catalog edit still cannot change what
  executes - the digest moves and travels with the answer - but a reviewer reading a diff is now
  sometimes reading SQL, which is a review burden the closed vocabulary did not impose.

## Alternatives considered

**`expression:` as a field on `Measure`, as the reference modelling languages do.** One key, no new
type, and drop-in compatible with the documents people already have. Rejected because it makes the
hatch invisible: a reviewer scanning a measure cannot tell which metrics are governed by a closed
vocabulary and which are text, and `AGENTS.md`'s claim that the vocabulary holds no SQL expression
would become false with nothing to replace it. The claim is worth keeping true of the closed part,
and that requires the two to be different shapes.

**One expression string with no dialect on it, as wren's cube path has.** Simplest, and it is what
the files being imported actually contain. Rejected because "it does not translate" then has no
honest answer: either the fragment is silently wrong on some target, or the metric is unusable there
and nothing says which. A map with a `portable` word and per-dialect overrides says it, and a provider
that already stores per-dialect SQL maps onto it without loss.

**An allowlist of AST node kinds instead of a denylist.** Fail-closed, and it would have caught the
subquery hole by construction. Rejected on the count: the scalar and aggregate space is some six
hundred node kinds, and enumerating the ones that are fine would recreate the closed vocabulary this
hatch exists to widen. The reach half is enumerable and small - there are only so many ways to name a
relation - so reach is a denylist backed by the dialect layer's own `is_query` and `is_ddl`
classifiers, and portability is a denylist of four measured defects. The residual risk of an
unlisted-but-harmful node kind arriving in a future version of the dialect layer is accepted and
stated.

> **Amended.** This still stands for node KINDS and is why they are still a denylist. It was extended
> to called function NAMES, and that extension was wrong: a name is not a node kind, the space is
> unbounded rather than six hundred, and the proof is `MAX(getenv('X'))` returning a secret out of the
> process. Names are now an allowlist -
> [see above](#node-kinds-are-a-denylist-function-names-are-an-allowlist) - and the count argument is
> the reason the two halves differ rather than the reason they agree.

**Refuse the whole class and keep pushing these metrics to the pinned-statement path.** No parser
anywhere on the definition path. Rejected for the reason 0002 rejected it once already: that path
needs an upstream renderer, which is the precondition the first-party path exists because it is
absent - and it now also means refusing an entire class of metadata provider rather than one metric.
