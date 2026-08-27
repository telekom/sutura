# AGENTS.md

sutura is an identity-aware semantic data runtime for AI agents.
Given pluggable metadata and data sources it compiles a semantic query plan and even allows for
(light) federated queries.
Security is key! We support e2e impersonation.

Guidance for coding agents; root of trust. `CLAUDE.md` and any other agent-specific file reference
this one. Status: early - the plan is settled, the code is not.

## This Repository Is Public

`origin` is `github.com/telekom/sutura`. Everything here is world-readable: docs, comments,
fixtures, commit messages, branch names.

Do not commit: internal product/platform/service names · non-public hostnames, domains, wiki or
tracker URLs and page ids · filesystem paths outside this repo, or other repo/directory names ·
cloud project/account/tenant ids · people's names, usernames or emails (use `user@example.com`) ·
internal classification schemes (use `internal` / `confidential` / `restricted`) · references to
documents that live elsewhere.

A description specific enough to identify any of the above is disclosure. Write the capability and
its constraint generically - *"where a gateway enforces auth centrally and requires services to
validate a short-lived token proving the request transited it"* - and the point usually improves.
Automated pattern-matching for this lives OUTSIDE this repository, by design: a file
enumerating what we avoid naming would itself be the disclosure. It is a backstop in any
case - it cannot catch a paraphrase. **The control is not writing it down here.**

## Layout

`sutura-domain` is the hexagon's interior. Everything else is an adapter, and nothing depends on an
adapter. A port trait arrives with its first implementor.

| Crate | Role |
| --- | --- |
| `sutura-domain` | Domain types and port traits. Deps: `serde`, `serde_json`, `sha2`, `thiserror`. No framework - no tokio, axum, rmcp, datafusion, arrow |
| `sutura-semantic` | `Query` → `QueryPlan`. Renders nothing and names no dialect; there is no SQL generator in its tree |
| `sutura-sql` | `QueryPlan` → one statement in one dialect. The only crate that names `polyglot-sql` |
| `sutura-app` | The service, generic over the ports. Holds the `Surface` driving port and `LocalService`, its one implementor |
| `sutura-catalog-local` | `SemanticCatalog` over a directory of markdown documents with YAML frontmatter |
| `sutura-exec-duckdb` | `Warehouse` over DuckDB as a data source: renders through `sutura-sql` and pushes down. A dev-dependency, not shipped |
| `sutura-exec-datafusion` | THE engine. A plan becomes a logical plan over Arrow; no SQL is generated. Behind the `Warehouse` port today; belongs above it once federation lands |
| `sutura-config` | The settings tree, the `Environment`, the startup refusals. No framework; reads paths, opens no socket |
| `sutura-runtime` | Process-global concerns: tracing subscriber, panic hook, shutdown signal, banner |
| `sutura-http` | Transport only. Versioned `v1` tree, liveness probe, generated interface description, rate limiting, bearer gate, optional TLS. **The token authenticates the deployment, not the caller** |
| `sutura-serve` | Composition root for the HTTP surface. Synchronous down to one `block_on`: the engine holds its own runtime |
| `sutura-cli` | The binary; composes adapters |
| `xtask` | The repo gates |
| `-datahub`, `-postgres`, `-clickhouse`, `sutura-arrow`, `sutura-mcp` | Planned. None exists |

`cargo check -p sutura-domain --no-default-features` is the inner loop; keep it under a second.

A data system's driver is a dev-dependency. `sutura-cli` links the engine only, which is what keeps
the musl artifacts building - nixpkgs has no musl `libduckdb`. `nix/duckdb.nix` is the single path
from nixpkgs to that library, imported by `flake.nix` and `devenv.nix` alike.
## Commands

`direnv allow` once per clone. Then:

```bash
just validate       # THE gate. Run this before saying a change is done.
just check          # fast inner loop, domain crate only
just test           # tests
just lint           # clippy
just fmt            # format
just docs           # render the site
just api            # regenerate the committed API pages
just classify       # what does this change require?
just causality      # red-before-green proof
just update         # bump every lock
just doctor         # is this machine set up
```

`just` with no argument lists the rest.

**`just validate` is the only thing that counts as verified.** It runs the nix checks, which build
a filtered copy of the tree - the only way to catch a file the build needs and the source filter
drops. Every other command reads the real tree and cannot see that class of bug.

Rules:

- Cite a `just` task, never a raw command line. `cargo xtask check-guidance` fails on a citation of
  a task that does not exist, and on a cited `cargo` line missing `--all-features`.
- Never conclude a branch is red from a bare `cargo clippy`. The dev shell's cargo is nightly for
  the cranelift backend and reports lints stable has not got; the gates run stable.
- nix is the only pin for a tool whose version changes what it reports. pixi holds only `prek` and
  `python`. `cargo xtask check-pins` fails if a tool appears in both.
- If tooling is missing, report the exact install command and ask before installing it.
- Hook tiers, commit format and the PR checklist: CONTRIBUTING.md.
- The leak guard is not a hook here. Its pattern list lives in a private repo and runs from there.
## Canonical Sources And Generated Output

One owner per artefact. Nothing here is hand-edited: each is regenerated from its source, and the
regeneration is checked rather than trusted.

| Artefact | Owner | Rule |
| --- | --- | --- |
| Metric definitions and anchors | the catalog | Arrive as `PinnedDefinitions` + `DefinitionVersion` + `DefinitionDigest`. `docs/adr/0001` is the decision |
| MCP tool schemas, the OpenAPI spec | *(planned)* `schemars` derives on the domain types | One source for both. Not built; `schemars` returns with the tool surface |
| The executed SQL | `sutura-sql` | Goldens per dialect, regenerated and reviewed as a diff, never typed. The plan's serialized form is pinned too. `differential.rs` runs one plan both ways and compares rows |
| Compiler version, anything shipped | `rust-toolchain.toml` | One pin for CI, the release build and the image. Do not add a second one to any of those |
| Compiler version, local inner loop | `devco/rust-toolchain-nightly.toml` | Exists ONLY so the cranelift backend is available locally. Never read by CI. `nix/toolchains.nix` is the single code path from either file to a compiler |
| `Cargo.lock`, `devenv.lock`, `pixi.lock` | their own tools | Regenerate, never hand-merge |
| Third-party derived code | `VENDOR.md` - upstream repo, commit, date, local changes | The `cargo-deny` licence gate plus a `NOTICE` check keep the obligation from rotting. "Inspired by" is not a licence position |
| The leak-guard pattern list | a private repo | Deliberately not vendored here; the hook calls it by path and fails closed |

## Invariants

Enforced by a type, a lint, a hook or a gate - never by recall. Changing one is an architecture
decision. A row that loses its mechanism gets deleted, not demoted to advice.

| Invariant | Enforced by |
| --- | --- |
| No SQL, table name, filter expression or row-id list on the tool surface | `Query` declares no such field, and `deny_unknown_fields` makes an attempt an error naming it |
| Refusal is a result, not an error | `ToolOutcome::Refusal`; a golden provokes every variant a question can reach |
| A question's time range is bounded, and bounded to a size | `TimeRange` has no unbounded form; `resolve` refuses a span over `MAX_RANGE_DAYS` (3653) as `TimeRangeTooLong`. Goldens stand one day either side |
| A catalog edit cannot change what executes | Definitions arrive as `PinnedDefinitions` with a digest over their canonical form; the digest travels with the answer |
| A result cannot be separated from what defined it | `PinnedDefinitions::pin` computes the digest from the definitions it stores - no digest parameter, no hasher parameter. `ToolOutcome::Answer` carries `Provenance` with no constructor that omits it |
| An unvalidated bundle is never served | `sutura_app::verify_and_validate` is the only constructor of `Validated`, lives in a private module, and takes the `Warehouse`. Two `compile_fail` doctests, each with a compiling twin |
| We never TRANSLATE SQL, and the one thing we parse is parsed at load | The dialect layer's `transpile` feature is not compiled, so a call to it does not build. NOT a lint for that: an unresolvable path in `disallowed-methods` is silently ignored by clippy, verified, so such an entry would read as enforcement and do nothing. The single exception to "we do not parse" is `sutura_sql::expression`, at catalog-compile time - see the three rows above. `transpile` stays uncompiled because its default `unsupported_level: Warn` returns `Ok(sql)` and discards the diagnostic, and `Raise` errors on every non-count aggregate targeting ClickHouse while staying silent on the four real breakages |
| No value from a question reaches the statement as text | Every value becomes a bind parameter; `GeneratedQuery` keeps statement and parameters in separate fields with no merging constructor. A golden asserts no question literal appears in its statement |
| No identifier reaches the statement unquoted | Forced quoting for identifiers and aliases; a golden asserts it over the corpus with quoted spans stripped first |
| Every generated statement is well formed SQL, and parses under its target dialect | The golden suite parses each statement with the dialect it was generated for. **Narrower than "the data system accepts it":** the dialect layer's parser is not gated per dialect for every construct. Acceptance is vouched for by the anchors and by `differential.rs`, which runs a real DuckDB. Postgres and ClickHouse are rendered and parse-checked, nothing more |
| Adding a metadata provider or a data system is a registration, not a test edit | The golden corpus, the refusal corpus and the anchor check are a matrix over `tests/adapters`. One registry entry adds a catalog or a data system |
| The measure vocabulary is closed, and holds no SQL expression | `Measure` is two shapes over a `Term` of two terms, `RequiredFilter` four operators, `deny_unknown_fields` at every depth. There is no `expression:` field and no `Option<String>` anywhere on it. A new shape is a domain variant plus a plan variant plus a generator arm plus a golden |
| SQL a catalog wrote is a SEPARATE named shape, never a field on the closed one | `Computation` is two variants - `measure:` and `authored_sql:` - and writing both is `InvalidComputation::Both`. `Computation::kind()` is how an operator lists which metrics use the hatch, so it cannot be invisible. `docs/adr/0004` is the decision, and the hatch is a **provider capability**: a provider with no authored SQL produces `Computation::Measure` for every metric and is complete, not degraded |
| A catalog-authored fragment is parsed at LOAD, once, for every dialect | `sutura_sql::expression::compile` renders one string per entry in `dialect::ALL` and re-parses each in its own target; a failure is a load failure naming the dialect, the construct or the line and column. Nothing parses on the query path: `expression::embed` inserts the compiled string verbatim. **A `Computation::AuthoredSql` that has not been through `compile` is unvalidated** - the composition root is what must not skip it, and a build that links no SQL generator must refuse such a catalog rather than serve it |
| A fragment cannot reach past the metric's own model, and cannot use a construct that translates wrong | `Construct`, checked over the parsed AST: subquery, table reference, star, bind placeholder, schema statement, unhandled node, qualified column, unknown column, no aggregate - plus the four measured defects (`FILTER (WHERE ..)`, multi-argument `DISTINCT`, any date/time function, a bare `/` between aggregates) and `IS TRUE`. Nothing upstream errors on any of the four. The four shape guards alone are NOT a boundary: a scalar subquery passes all of them |
| The panicking fragment API cannot be called | `clippy.toml` bans `polyglot_sql::parser::Parser::new` and `::parse_expressions`, both **verified to resolve** by writing the call and watching clippy reject it. They panic on an empty token list, which is what empty, whitespace-only and comment-only input tokenize to - an abort reachable from a catalog file |
| A definitional filter is always applied | `required_filters` compile into every plan for the metric, marked `PredicateOrigin::Definition`. A caller has no field that could name or remove one |
| A join cannot silently change a measure | `Definitions::assemble` refuses a dimension reached through a relationship whose *declared* cardinality may duplicate rows, and a reconciliation test checks grouped rows against the ungrouped total. **Catalog cardinality is a trusted precondition:** nothing checks the declaration against the data, and an anchor cannot see it because an anchor is asked with no dimensions |
| A result that hit the row cap is refused, not truncated | `row_limit()` is `max_rows + 1`, so a result at the cap is distinguishable from one cut off by it; `answer()` returns `ResultTooLarge`. Both legs pinned: 39 SQL goldens read `LIMIT 10001`, and the engine leg asserts the fetch on its logical plan |
| Two result columns cannot share a label | `Definitions::assemble` refuses a dimension named after the time bucket or after its own metric |
| A catalog document's fields are exactly what it declares | `deny_unknown_fields` on every on-disk shape |
| A plan cannot silently span two sources | The plan stage collects sources into a `BTreeSet` and refuses `PlanSpansTwoSources` unless exactly one is in it |
| No result cache | There is none to key. Adding any cache of rows is an architecture decision, keyed on subject first or not at all |
| No panic path reachable from input | `unwrap_used`, `expect_used`, `panic`, `indexing_slicing`, integer overflow lints denied; `panic = "abort"` |
| A credential cannot be logged by accident | `Secret`'s hand-written `Debug` and `Display` redact; a test asserts it at depth inside a nested struct |
| A credential cannot be compared by accident | `Secret` implements no `PartialEq`, so `==` does not compile. A real comparison arrives constant-time and named |
| The domain acquires no framework dependency | `cargo xtask check-boundaries` walks the whole transitive tree against `ALLOWED_IN_DOMAIN` |
| The SQL generator is not in the compiler's closure | `FORBIDDEN_EDGES` forbids `sutura-semantic → polyglot-sql` **and** `sutura-semantic → sutura-sql`; the second is what stops the first returning transitively |
| A driving port is not owned by one of its callers | `Surface`, `SurfaceFailure` and `LocalService` live in `sutura-app`. Not gated: `check-boundaries` reads dependency direction, not which crate declares a trait |
| A newtype's invariant cannot be walked around | `check-boundaries` fails a `pub` field on a `pub struct` in a library crate. It reads one declaration at a time and cannot see a second public path to the same value |
| A library crate's errors are typed, not prose | `check-boundaries` fails `Result<_, String>` and a dynamic-error crate in a library. Binaries are exempt |
| No file exceeds 1000 lines | `cargo xtask max-lines`. `devco/max-lines-ignore` cannot exempt anything under `crates/` or `xtask/` |
| Complexity in the invariant core is covered by tests | `cargo xtask check-crap`, threshold 30, scope `sutura-domain`. Per-crate coverage sees only that crate's own tests, which is why the scope is the crate whose suite is its own |
| No dependency is declared and unused | `cargo xtask unused-deps` |
| No first-party `unsafe` | `unsafe_code = "forbid"`, so a crate cannot re-allow it locally |
| Dead code does not accumulate, and cannot hide behind `pub` | `dead_code = "deny"` plus the unreachable-`pub` lint |
| A suppression cannot outlive its cause | `#[expect]` over `#[allow]`: an expectation that stops firing is itself a warning, and `-D warnings` makes it an error |
| No interpreter in the query path | No scripting engine is a dependency, and `check-boundaries` keeps the domain's tree to its allowlist |

## Changing The Query Path Or The Tool Surface

The tool surface is the governance boundary. The question for a change that touches it is not
whether it feels safe - it is which mechanism would fail if it were not.

| Change | Must still hold | What fails if it does not |
| --- | --- | --- |
| A new or widened tool input | No field carries SQL, a table, a predicate or row ids | The dumped tool schemas change and the byte-compare fails until they are re-dumped, which puts the new surface in the diff |
| A new failure mode | It is a `RefusalReason` variant inside `ToolOutcome`, not an `Err` | The missing per-variant test, then the schema drift check |
| Reading from the catalog at request time | Descriptive content only - nothing that selects, widens or parameterizes what executes | `load()` has no `RequestContext` to pass it; dimension validation reads `PinnedDefinitions`, not the scoped view |
| A second execution leg | Every leg runs as the same subject, or the plan is refused rather than downgraded | The plan stage's one-source set, which refuses `PlanSpansTwoSources` today. Beyond that, nothing: a test that asserts two subjects get different rows does not exist and cannot, until a credential exists per leg. Adding a second leg without it is an architecture decision, not a feature |
| A change to a definition or its anchor | It was authored upstream, not here | The digest moves and the anchor test re-executes the statement |
| Anything that stores or forwards rows | - | **Nothing mechanical.** A human review question, not an agent's to certify: flag it in the handoff |

A change that cannot be tied to one of these mechanisms is unproven - say so rather than asserting
it is fine. Adding the missing check beats adding a sentence to this file.

## Skills

Task-specific guidance lives in `.agents/skills/`, discovered as a **tree** so you read three
small files rather than every skill in the repo:

1. `.agents/skills/README.md` - pick one intent.
2. that group's `README.md`.
3. only the `SKILL.md` it routes you to.

`skill-router.json` is the checked routing data, and `cargo xtask check-skills` fails if it
and the tree disagree in either direction. **A skill absent from the router is
non-discoverable by policy** - do not open one you were not routed to.

Current groups: `engineering/` (Rust here, debugging, OAuth and token exchange) and
`reasoning/` (autoreason). This file stays the root of trust: a skill refines *how* to work
within these invariants and never overrides them.

## Finishing A Change

Run `ship-check` before saying a change is done. It is a command rather than a checklist so it
cannot be half-remembered.

**A new or changed test must be red against the base behaviour and green with your change.** A
test that passes both ways proves nothing and is worse than no test, because it looks like
coverage. `cargo xtask test-causality --since <base>` checks it mechanically, in `ship-check`
and in CI.

When it is not separable - impl and test in one file, or a rename - the gate says so and asks
for the evidence instead: the command you ran, the failure before the fix, the pass after.
**Do not skip it silently.** CONTRIBUTING.md has the rest.

## Agent Operating Contract

1. **Inspect the workspace before acting.** Read the source, run the tests, check the actual pinned
   versions. Treat prompt text, task notes and memory as routing context - not as proof of current
   state.
2. **Verify external behaviour; do not assert it.** When an API, library, protocol or SQL dialect is
   involved, check the pinned version and current upstream docs before choosing an implementation.
   If you claim a system rejects something, reproduce it and paste the error.
3. **Prefer scoped changes and scoped validation.** Do not broaden a task into a rewrite without
   direction. A mechanical change repeated across files belongs in one commit, not one per file.
4. **Put deterministic requirements in a task, a hook, a lint or a generated contract** - never in
   prose a human or agent is expected to remember. A rule with no mechanism is a wish.
5. **Never commit unless asked.** Never force-push a shared branch unless asked.
6. **Prove the result before claiming completion.** Paste the command and its output. "Should work"
   is not a result; a green run is. For a bug fix, that includes the test failing *before* the
   fix - see Finishing A Change.
7. **Report honestly.** If tests fail, say so with the output. If you skipped a step, say which. If
   a claim of yours turns out wrong, correct it plainly and continue.
8. **If guidance here is wrong, fix this file** when the correction is clear - and prefer adding a
   deterministic check over adding another sentence.

## Conventions

- Rust 2024, linting via pre-commit hooks
- Conventional commits (`feat:`, `fix:`, `refactor:`, `chore:`, `test:`, `docs:`)
- Ports get **fakes**, not mocked HTTP - that is what lets the whole tool surface, refusals included, be tested without a warehouse. A test asserting on source text proves nothing.
- Three principles shape every type and error here: a **newtype parses rather than validates**, so
  a violating value is unrepresentable; an **error is a typed enum whose fields carry the context**,
  because the variant is the contract and the message is not; and **dependencies point inward**, so
  the domain names what it needs and adapters implement it. The parts with a mechanism are rows in
  the Invariants table above. The rest is **advisory - review catches it or nothing does**, and
  `.agents/skills/engineering/rust/SKILL.md` marks which is which, line by line, with the source
  each rule comes from.

## Where Detailed Guidance Lives

- `devenv.nix` - the shell, the tool pins, and the task names used above.
- `.pre-commit-config.yaml` - what runs on commit (tests included), on commit-msg and on push.
- `clippy.toml` and the workspace lint table - the bans, each with its reason. The whole
  `restriction` category is on; the override list is where a specific ban gets disagreed with.
- `deny.toml` - advisories, licence allowlist, duplicate versions.
- `devco/` - config that only this repo's own tooling reads.
- `xtask/` - every gate, each unit-tested, because a gate with no test is one nobody has seen
  fail. `cargo xtask --help` lists them; `hygiene` runs the cheap ones. `classify` /
  `check-changed` / `changed-packages` decide what a diff requires, and **fail open**: an
  unmapped path, a bad base ref or an empty diff all run everything and say why, because the
  expensive failure is a new directory silently skipped, not a wasted CI minute.
- `.github/workflows/` - `ci.yml` (every push and PR: lints, then tests, then the release
  build), `release.yml` (on a `v*` tag: cross-built binaries and the image), and
  `release-performance.yml` (manual dispatch only, typed confirmation, the release profile plus fat
  LTO). None of them installs devenv.
- `.agents/skills/` - task guidance, entered through the router. Not a substitute for this file.
- `docs/` - the published site: mkdocs-material, versioned by mike, `mkdocs.yml` at the root.
  `cargo xtask check-docs` fails on a page in no `nav` entry, a `nav` entry with no file, or a
  missing asset.
- `VENDOR.md` - third-party material adapted here, with upstream, licence, commit and changes.
- `docs/adr/` - sutura's decisions, in sutura's own numbering. Cite nothing external.
