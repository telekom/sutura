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

The directory structure is the architecture: `sutura-domain` is the hexagon's interior, everything
else is an adapter, and nothing depends on an adapter. Each port trait arrives WITH the adapter that
implements it, because a trait with no implementor is a guess at a signature and `pub` hides it from
`dead_code`. Two are here now - `SemanticCatalog` came with the local catalog adapter and `Warehouse`
with the DuckDB one. `CredentialBroker` is still absent for the same reason it always was: nothing
implements it yet.

| Crate | Role |
| --- | --- |
| `sutura-domain` | Domain types; a port trait per adapter, as adapters land. No framework deps - no tokio, axum, rmcp, datafusion, arrow |
| `sutura-semantic` | `Query` → plan → `GeneratedQuery` |
| `sutura-app` | The service; generic over ports, holds no framework types |
| `sutura-catalog-local` / `-datahub` | `SemanticCatalog` adapters (git YAML / catalog) |
| `sutura-exec-duckdb` / `-postgres` / `-clickhouse` | `Warehouse` adapters over DATA SOURCES - a place data already lives, that somebody wants to ask questions of. Each renders the plan into its own dialect with `polyglot-sql` and pushes the statement down. `Warehouse` is the PORT's name and says nothing about what sits behind it. `-duckdb` exists today; the other two are planned |
| `sutura-exec-datafusion` | THE ENGINE, not a data source. A plan becomes a logical plan over Arrow and no SQL is generated, which is why it can hold arrow and tokio while the domain holds neither. **It sits behind the `Warehouse` port today and that is a stepping stone, not the end state**: an engine belongs ABOVE the port, deciding which subplan each data source runs and executing the rest itself. Today it is the engine with zero remote sources - local files only - and the move above the port arrives with federation |
| `sutura-arrow` | `RecordBatch` → Arrow IPC / Flight SQL |
| `sutura-mcp` / `sutura-http` | Transport only, no business logic |
| `sutura-cli` | The binary; composes adapters |
| `xtask` | The repo gates: boundary check (dependency direction, and the typed surface of a library crate), file-length check, unused-dependency check, line-ending check. Schema dump and drift check arrive with the schemas |

The domain crate depends on nothing heavy, and `cargo check -p sutura-domain --no-default-features`
is the inner loop for that reason: its test suite should stay well under a second.

A data system's driver is a **development dependency**, not a shipped one. `sutura-cli` links the
engine and nothing else, which is also what keeps the musl artifacts building: nixpkgs has no musl
`libduckdb`, and the binary never asks for one. The DuckDB adapter is still compiled and tested - it
is a dev-dependency of `sutura-app`, where the golden suite proves the SQL we render actually runs.
`nix/duckdb.nix` is the single code path from nixpkgs to that library, imported by both `flake.nix`
and `devenv.nix` so the dev shell and CI cannot link two different ones.

## Commands

`direnv` loads the devenv on `cd` - run `direnv allow` once per clone. Nix + devenv provisions the
shell and owns the task names; pixi owns the hook runner and the maintenance interpreter, nothing
that reports findings; `prek` runs the hooks.

**Two toolchains, and the trap matters.** The dev shell's bare `cargo` is a pinned **nightly**
(cranelift); every gate runs on the pinned **stable** that CI uses. So a bare `cargo clippy`
reports lints stable has never heard of. **Do not conclude a branch is red from one** - run
`just lint`. CONTRIBUTING.md has the mechanism.

CI does **not** enter this shell: it runs `nix build .#checks.<system>.<name>`, so the pipeline
needs `nix` and nothing more. The two cannot drift because they share an implementation rather than
a shell - the `hygiene` check runs the same `xtask` binary as the `hygiene` script here. Add a gate
in one place only and the omission shows up as a diff.

```bash
just check          # fast inner loop, domain crate only
just lint           # clippy, on stable
just test           # tests, on stable
just hygiene        # the cheap structural gates
just gates          # everything CI runs
just ship-check     # the finishing sequence
just classify       # what does this change require?
just check-changed <paths>
just causality      # red-before-green proof
just hooks          # every hook over every file
just ci             # what CI runs, through nix, no devenv
just docs           # render the site
just image          # the release image
just build-all      # all four shipped binaries
just update         # bump every lock
```

`just` with no argument lists the rest. A task is the only name worth citing: it is one place
to change, and `cargo xtask check-guidance` fails on a citation of a task that does not exist.

- `--all-features` is not optional, and every gate passes it. No crate here declares a `[features]`
  table today, so on the current tree it changes nothing - which is the point: the flag is what
  makes an adapter placed behind a feature inspected from the day it lands rather than from the day
  somebody remembers the flag. `cargo xtask check-guidance` fails a cited command that omits it.
- Hook tiers, commit format and the PR checklist: CONTRIBUTING.md.
- The leak guard is **not** a hook in this repo. Its pattern list lives in a private repo - a file
  here enumerating what we avoid naming would itself be the disclosure - so it runs from there.
- nix is the **only** pin for any tool whose version changes what it reports - zizmor, actionlint,
  shellcheck, clippy, nextest, cargo-deny. pixi holds only `prek` and `python`, which cannot change
  a verdict. `cargo xtask check-pins` fails if a tool appears in both.
- If tooling is missing, report the exact install command and ask before installing it.

## Canonical Sources And Generated Output

One owner per artefact. Nothing here is hand-edited: each is regenerated from its source, and the
regeneration is checked rather than trusted.

| Artefact | Owner | Rule |
| --- | --- | --- |
| Metric definitions, their statements and anchors | **two authoring modes, and a definition uses one of them.** A *first-party model* is authored in this repository's catalog: models, relationships and metrics, from which `sutura-semantic` generates the whole statement. A *pinned statement* is rendered by an upstream semantic layer (dbt / MetricFlow) and taken as given | Either way it arrives as `PinnedDefinitions` + `DefinitionVersion` + `DefinitionDigest`, and the digest is over the canonical form of the parsed definitions. For a pinned statement, editing it here forks the definition from the number it certifies. `docs/adr/0001-first-party-semantic-models.md` is the decision and says why neither mode is a degraded version of the other |
| MCP tool JSON schemas · the OpenAPI spec | *(planned)* `schemars` derives on the domain types | One source for both, so they cannot disagree: a `dump-schemas` task (not yet written) will produce them and CI will byte-compare. **Not yet built** - the `schemars` dependency was removed by the unused-deps gate because nothing references it yet, and returns with the tool surface. Declaring a dependency to satisfy a document is what that gate exists to stop |
| The executed SQL | `sutura-semantic`. For a first-party model it generates all of it - projection, `GROUP BY`, a bounded date predicate, the single-hop join, parameterized values, quoted identifiers and aliases, a row cap. For a pinned statement it would generate only the wrapper. **And not every adapter needs it:** the `Warehouse` port's currency is a `QueryPlan`, not a statement, so an adapter may execute the plan directly and render nothing - the DataFusion one does, and a dialect bug is therefore unreachable on that path | SQL goldens per dialect, regenerated and reviewed as a diff, never typed. The plan's own serialized form is pinned by a golden as well, so what we decided shows up as a reviewable diff rather than as a different number. The gap this row used to record is closed: `crates/sutura-app/tests/differential.rs` runs one plan both ways - executed locally over Arrow by the engine, and rendered as SQL and pushed down to a data source - and compares the rows. Its own module doc is deliberately modest about what that proves, calling it a cheap regression net rather than a proof of correctness: the class of bug it catches is a rendered statement that is valid SQL with different semantics, which the parse golden cannot see and an anchor check would mostly miss. The pinned-statement path is **not built**: `docs/architecture.md` describes the splice and nothing implements it |
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
| No SQL, table name, filter expression or row-id list on the tool surface | `Query` carries no such field, so an uncertified question is unrepresentable rather than merely refused. Any widening lands as a diff in the dumped schemas |
| Refusal is a result, not an error | `ToolOutcome::Refusal { reason: RefusalReason }` is the public surface, and a test provokes every variant |
| A catalog edit cannot change what executes | Definitions are pinned and hashed at build time; arguments validate against the **pinned** allowlist, and `SemanticCatalog::load` takes no request context, so it cannot reach the hot path |
| An unvalidated bundle is never served | The service accepts only `Validated<PinnedDefinitions>` - anything else does not compile. The anchor test re-runs each pinned statement in CI and at startup, and failure fails readiness |
| We never re-parse SQL we did not generate | For a first-party model there is no foreign SQL on the path, so it holds by construction. The dialect layer's `transpile` feature is **not compiled** - see the feature list in the workspace manifest - so a call to it does not build, which is a stronger gate than the lint the previous version of this row wished for. The generated statement is parsed once in the golden suite, per dialect, to prove it is well formed there; that check never re-emits |
| No value from a question reaches the statement as text | Every one becomes a bind parameter, and `GeneratedQuery` keeps the statement and the parameters in separate fields with no constructor that merges them. A golden over the whole question corpus asserts that no literal a question carries appears in the SQL generated for it, and that the parameter list is exactly that set of literals - so a generator that dropped the predicate fails it too |
| No identifier reaches the statement unquoted | The generator forces quoting, for aliases as well as identifiers, and a golden asserts it over the corpus. Without it a column called `order` is emitted bare and is a syntax error at the data system rather than here |
| Every generated statement is well formed SQL, and parses under its target dialect | The golden suite parses each one with the dialect it was generated for. Parse only: it never re-emits, so it cannot introduce a parser-differential bug, and it catches a malformed statement without needing one of each data system in CI. **It is narrower than "the data system accepts it", and the row says so on purpose.** The dialect layer's parser takes the dialect but is not gated on it for every construct, and its generator writes `x IS TRUE` without ever reading its own `is_bool_allowed` flag - so for a construct like that, parsing succeeds under all three targets whatever a real instance would say. What vouches for acceptance is execution: the anchors re-execute, and `crates/sutura-app/tests/differential.rs` runs a real `DuckDB`. For Postgres and `ClickHouse` we render and parse-check, and nothing more |
| The measure vocabulary is closed, and holds no SQL expression | It is closed at two levels: `Term` is an enum of two terms (`aggregate`, `count_if`), `Measure` an enum of two shapes over terms (`simple`, `ratio`), and `RequiredFilter` an enum of four operators - `deny_unknown_fields` at every depth, including the `try_from` struct a term is read through. There is no field a catalog could write `sum(price * quantity)` into, and an unrecognised shape or term is an error naming what it found rather than a key silently dropped. A third term is a domain variant plus a plan variant plus one arm per generator plus a golden - a code change by design, not a catalog edit - and it is the CHEAP axis on purpose: a term added is one arm, not one arm per shape, which is what makes `count_if(churned) / count_distinct(subscription)` sayable. `docs/adr/0002-a-closed-vocabulary-for-measures.md` says why the closed set is the mechanism, why the term rather than the shape is the axis that widens, and why "one aggregate" was only ever a means to it |
| A definitional filter is always applied | A metric's `required_filters` are compiled into every plan for it and marked `PredicateOrigin::Definition`; a golden over the question corpus asserts that a metric declaring one never produces a plan without it, and a second asserts the value is in the parameter list rather than in the statement text. A caller has no field that could name, select or remove one, which is also why nothing a caller does can make either test pass or fail |
| A join cannot silently change a measure | A relationship declares its cardinality, and `Definitions::assemble` refuses a dimension reached through one that may duplicate the metric's rows. A `sum` over duplicated rows is a wrong number that raises no error anywhere |
| Two result columns cannot share a label | `Definitions::assemble` refuses a dimension named after the time bucket's label or after its own metric, which are the two labels the projection already uses. Caught at load, because the caller did not choose it |
| A catalog document's fields are exactly what it declares | `deny_unknown_fields` on every on-disk shape. A misspelled key is otherwise dropped in silence, and the definition that loads is not the one the author wrote - `colums:` yields a model with no columns, which then refuses every question for a reason that says nothing about a typo |
| Every query runs as the calling principal | **Not mechanised, and this row says so rather than crediting something that does not exist.** In single-player it is trivially true and worth nothing: the data is a file, a file has no login, and there is only ever one subject. What would enforce it - a credential minted per request, and a leg that cannot run as the subject refused instead of downgraded - needs a port that is deliberately absent, because a port arrives with its adapter. Until then the honest claim is narrower: nothing in the query path can *choose* an identity, since `SemanticCatalog::load` takes no request context and a plan resolves to exactly one named source. `examples/multi-player/README.md` is where the gap is written down for a reader |
| A plan cannot silently span two sources | The plan stage in `crates/sutura-semantic/src/plan.rs` collects the source of the metric's own model and of every model reached through a join into a `BTreeSet`, and refuses `RefusalReason::PlanSpansTwoSources` unless exactly one name is in it. The count is computed from the plan rather than asserted about it afterwards, and `a_plan_that_would_reach_a_second_data_system_is_refused` in the golden suite builds a two-source catalog to provoke it |
| No result cache | Under row-level security a query-keyed cache is a cross-user leak. *No mechanism can prove an absence: adding any cache of rows is an architecture decision, keyed on subject first or not at all* |
| No panic path reachable from input | `unwrap_used` / `expect_used` / `panic` / `indexing_slicing` denied for library crates in `clippy.toml`, exempt in tests; `panic = "abort"` on shipped profiles |
| A credential cannot be logged by accident | Credential-shaped types are newtypes with a hand-written `Debug`, plus a unit test asserting the secret is absent from `{:?}` |
| A credential cannot be compared by accident | `Secret` implements no `PartialEq`, so `==` on one does not compile. A derived comparison is byte-wise and returns on the first difference, which is a timing oracle at whatever call site adds it - and the call site is where it would be invisible. A real comparison arrives with a constant-time implementation and a name that says so |
| The domain acquires no framework dependency | The dependency-boundary half of the boundary check in `xtask`, run by `gates` and in CI. An **allowlist** over the whole transitive tree, so a framework reached through an innocuous crate fails it too |
| A newtype's invariant cannot be walked around | The field is private and the constructor is the only way in, so a violating value is unrepresentable rather than merely rejected. The typed-surface half of the boundary check fails a `pub` field on a `pub struct` in a library crate. `serde` is routed through the constructor with `#[serde(try_from = ..)]`, because a derived `Deserialize` writes past it - *Gap: that routing is not itself checked; review catches it until a gate does* |
| A library crate's errors are typed, not prose | The typed-surface half of the boundary check fails a `Result<.., String>` or a declared dynamic-error crate (`anyhow`, `eyre`) in any crate with a `[lib]` target. Binaries are deliberately exempt: there the error's audience is a human reading stderr. *Gap: it is line-scoped, so a signature wrapped across lines escapes it* |
| No file exceeds 1000 lines | `cargo xtask max-lines`, in the hooks and in CI. Generated and vendored output is exemptable in `devco/max-lines-ignore`; anything under `crates/` or `xtask/` is not - the gate fails on such a pattern rather than honouring it, so the only way past it is to split the file |
| No dependency is declared and unused | `cargo xtask unused-deps`, in the hooks and in CI. A crate must reference every dependency it declares, and every `[workspace.dependencies]` entry must be inherited by somebody - an entry nothing inherits pins nothing |
| No first-party `unsafe` | `unsafe_code = "forbid"` in the workspace lint table. `forbid` and not `deny`, so a crate cannot re-allow it locally; lifting it is a visible diff to this table |
| Dead code does not accumulate, and cannot hide behind `pub` | `dead_code`, `unused_must_use` and `unreachable_pub` are `deny` rather than the default `warn`, so a plain `cargo build` fails on them. `unreachable_pub` is what stops an unused item from being kept alive by a `pub` that reaches nowhere |
| A suppression cannot outlive its cause | `clippy::allow_attributes` is on, so a bare `#[allow]` is a lint error: `#[expect(.., reason = "..")]` is required and fails once the underlying warning stops firing |
| No interpreter in the query path | Python is build-time tooling only; the image from `nix build .#oci` holds one binary, so a query-path dependency could not ship |
| A result cannot be separated from what defined it | Provenance rides in the Arrow schema metadata, and both wire envelopes share one encoder |
| Every call is attributable, refusals included | `AuditSink` records the whole principal chain before the outcome is returned |

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
