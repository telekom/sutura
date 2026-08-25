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
else is an adapter, and nothing depends on an adapter. It holds the domain types today; each port
trait arrives with the adapter that implements it, because a trait with no implementor is a guess
at a signature and `pub` hides it from `dead_code`.

| Crate | Role |
| --- | --- |
| `sutura-domain` | Domain types; a port trait per adapter, as adapters land. No framework deps - no tokio, axum, rmcp, datafusion, arrow |
| `sutura-semantic` | `Query` → plan → `GeneratedQuery` |
| `sutura-app` | The service; generic over ports, holds no framework types |
| `sutura-catalog-local` / `-datahub` | `SemanticCatalog` adapters (git YAML / catalog) |
| `sutura-exec-clickhouse` / `-postgres` / `-duckdb` | `Warehouse` adapters. The near-term targets are ClickHouse and Postgres, with DuckDB for local and single-file work - see `docs/architecture.md`. `Warehouse` is the PORT's name and says nothing about what sits behind it |
| `sutura-arrow` | `RecordBatch` → Arrow IPC / Flight SQL |
| `sutura-mcp` / `sutura-http` | Transport only, no business logic |
| `sutura-cli` | The binary; composes adapters |
| `xtask` | The repo gates: boundary check (dependency direction, and the typed surface of a library crate), file-length check, unused-dependency check, line-ending check. Schema dump and drift check arrive with the schemas |

Adapters are feature-gated and default-off, so `cargo nextest run -p sutura-domain` compiles no heavy
dependency. Keep it that way: its test suite should run in well under a second.

## Commands

`direnv` loads the devenv on `cd` - run `direnv allow` once per clone. Nix + devenv provisions the
shell and owns the task names; pixi owns the hook runner and the maintenance interpreter, nothing
that reports findings; `prek` runs the hooks.

**Two toolchains, and it matters which one you get.** The dev shell's bare `cargo` is the pinned
**nightly** (`devco/rust-toolchain-nightly.toml`), because the cranelift codegen backend is nightly-only
and it is what makes the inner loop fast. Every gate instead sources `nix/stable-env.sh`, which puts
the pinned **stable** (`rust-toolchain.toml`) in front and gives it its own target directory. So:

- Running a `just` task or a devenv script gets stable - the same compiler CI uses.
- Typing `cargo clippy` yourself gets nightly, whose lint set differs. This repo gates on the whole
  clippy `restriction` category with `-D warnings`, so nightly will report lints stable has never
  heard of. **Do not conclude the branch is red from a bare `cargo clippy`** - run `just lint`.
- The separate target directory is not optional: alternating compilers in one directory invalidates
  every artifact in it.

CI does **not** enter this shell: it runs `nix build .#checks.<system>.<name>`, so the pipeline
needs `nix` and nothing more. The two cannot drift because they share an implementation rather than
a shell - the `hygiene` check runs the same `xtask` binary as the `hygiene` script here. Add a gate
in one place only and the omission shows up as a diff.

```bash
cargo check -p sutura-domain --no-default-features   # fast inner loop
test                                                 # tests, on stable
lint                                                 # clippy, on stable
hygiene                                              # line endings, max-lines, unused deps, boundaries
cargo xtask classify --since origin/main             # what does this change require?
cargo xtask check-changed <paths>                    # cargo check, narrowed to those packages
gates                                                # hygiene + fmt, clippy, tests, deny
docs                                                 # render the site to ./site
docs-serve                                           # the site with live reload
prek run --all-files                                 # hooks (config: .pre-commit-config.yaml)
nix build .#oci                                      # the release image
nix build .#sutura-performance                       # fat-LTO build; opt-in, never automatic
nix build .#checks.x86_64-linux.hygiene              # what CI runs, without devenv
pixi run --frozen <task>                                    # hooks and skill sync only
just update                                          # bump flake.lock, pixi.lock, Cargo.lock
stax                                                 # stacked branches / PRs
```

- Hooks are tiered by cost: fmt, clippy `--all-features`, the structural gates and the
  conventional-commit subject check run on commit; tests and `cargo-deny` on push. Scope hook runs
  to touched files while iterating, sweep before a PR.
- `--all-features` is not thoroughness for its own sake. Adapters are feature-gated and default-off,
  so a bare `cargo clippy --workspace` inspects almost nothing and still reports success. Every lint
  and test entry point passes it; a scoped `-p sutura-domain --no-default-features` run is the fast
  inner loop, not the gate.
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
| Metric definitions, their statements and anchors | the upstream semantic layer that renders them (dbt / MetricFlow) - **not this repo** | They arrive as a pinned, hashed snapshot: `PinnedDefinitions` + `DefinitionVersion` + `DefinitionDigest`. Editing a pinned statement here forks the definition from the number it certifies |
| MCP tool JSON schemas · the OpenAPI spec | *(planned)* `schemars` derives on the domain types | One source for both, so they cannot disagree: a `dump-schemas` task (not yet written) will produce them and CI will byte-compare. **Not yet built** - the `schemars` dependency was removed by the unused-deps gate because nothing references it yet, and returns with the tool surface. Declaring a dependency to satisfy a document is what that gate exists to stop |
| The executed SQL | `sutura-semantic`, which generates only the wrapper - projection, `GROUP BY`, a bounded date predicate, parameterized values, identifier quoting | The pinned statement is spliced in as a derived table **without being parsed**. SQL goldens are regenerated and reviewed as a diff, never typed |
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
| We never re-parse SQL we did not generate | Byte-for-byte passthrough, asserted by the SQL goldens. *Gap: no lint yet bans a transpile call on the query path - review catches it until one exists* |
| Every query runs as the calling principal | `CredentialBroker::credential_for(&RequestContext, ..)` mints per request; a leg that cannot run as the subject returns `RefusalReason::SourceIdentityUnavailable` instead of downgrading. The nightly two-identity test asserts two users get different rows |
| A plan cannot silently span two sources | `PlanSources` asserted `len() == 1` by the governance-invariant tests |
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
| A second execution leg | Every leg runs as the same subject, or the plan is refused rather than downgraded | `PlanSources.len() == 1` today; the nightly two-identity test once federation exists |
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

`ship-check` is the finishing sequence, and it is a command rather than a checklist so it
costs no tokens to follow and cannot be half-remembered:

```bash
ship-check                      # hooks over the branch diff, gate self-tests, causality
```

It requires a clean tree and a reachable base ref, then runs the commit-stage hooks over the
merge-base range, the pre-push hooks, and the test-causality proof below. Run it before
saying a change is done.

### Tests must be shown to test something

A new or changed test has to be **red against the base behaviour and green with your change**.
A test that passes both ways proves nothing and is worse than no test, because it looks like
coverage.

`cargo xtask test-causality --since <base>` checks this mechanically: it re-runs changed tests
against the base version of the non-test sources and requires at least one to fail, with no
unrelated failures, then requires them green on your head. It runs in `ship-check` and in CI.

When the change is not separable that way - impl and test in the same file, or a change with
no behavioural difference such as a rename - the gate says so and asks you to state the
evidence instead: the command you ran, the failure you saw before the fix, and the pass after.
Do not skip it silently.

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
- `.pre-commit-config.yaml` - what runs on commit, on commit-msg and on push.
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
