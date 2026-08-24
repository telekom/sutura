# AGENTS.md

sutura is an identity-aware semantic data runtime for AI agents.
Given pluggable metadata and data sources it compiles a semantic query plan and even allows for
(light) federated queries.
Security is key! We support e2e impersonation.

Guidance for coding agents; root of trust. `CLAUDE.md` and any other agent-specific file reference
this one. Status: early — the plan is settled, the code is not.

## This Repository Is Public

`origin` is `github.com/telekom/sutura`. Everything here is world-readable: docs, comments,
fixtures, commit messages, branch names.

Do not commit: internal product/platform/service names · non-public hostnames, domains, wiki or
tracker URLs and page ids · filesystem paths outside this repo, or other repo/directory names ·
cloud project/account/tenant ids · people's names, usernames or emails (use `user@example.com`) ·
internal classification schemes (use `internal` / `confidential` / `restricted`) · references to
documents that live elsewhere.

A description specific enough to identify any of the above is disclosure. Write the capability and
its constraint generically — *"where a gateway enforces auth centrally and requires services to
validate a short-lived token proving the request transited it"* — and the point usually improves.
A hook blocks known patterns; it is a backstop, not the control. It cannot catch a paraphrase, and
its pattern list is maintained privately for the same reason.

```sh
git config core.hooksPath .githooks
export SUTURA_LEAK_GUARD=/path/to/guard.py   # required; the hook fails closed without it
```

## Layout

The directory structure is the architecture: `sutura-domain` holds types and ports, everything
else is an adapter, and nothing depends on an adapter.

| Crate | Role |
| --- | --- |
| `sutura-domain` | Types + port traits. No framework deps — no tokio, axum, rmcp, datafusion, arrow |
| `sutura-semantic` | `Query` → plan → `GeneratedQuery` |
| `sutura-app` | The service; generic over ports, holds no framework types |
| `sutura-catalog-local` / `-datahub` | `SemanticCatalog` adapters (git YAML / catalog) |
| `sutura-exec-duckdb` / `-bigquery` | `Warehouse` adapters |
| `sutura-arrow` | `RecordBatch` → Arrow IPC / Flight SQL |
| `sutura-mcp` / `sutura-http` | Transport only, no business logic |
| `sutura-cli` | The binary; composes adapters |
| `xtask` | Schema dump, drift check, boundary check |

Adapters are feature-gated and default-off, so `cargo test -p sutura-domain` compiles no heavy
dependency. Keep it that way: its test suite should run in well under a second.

## Commands

`direnv` loads the devenv on `cd` — run `direnv allow` once per clone. CI enters the same shell, so
a command that works here works there. Nix + devenv provisions the shell and owns the task names;
pixi owns Python only; `prek` runs the hooks.

```bash
cargo check -p sutura-domain --no-default-features   # fast inner loop
cargo nextest run                                    # tests
cargo clippy --workspace --all-targets -- -D warnings
cargo xtask dump-schemas                             # regenerate the generated contracts
gates                                                # fmt, clippy, tests, deny, boundary, leak guard
prek run --all-files                                 # hooks (config: .pre-commit-config.yaml)
nix build .#oci                                      # the release image
pixi run <task>                                      # Python tooling only
stax                                                 # stacked branches / PRs
```

- Hooks are tiered by cost: fmt, clippy, the leak guard and the commit-message check run on commit;
  tests and slow scans on push. Scope hook runs to touched files while iterating, sweep before a PR.
- `rust-toolchain.toml` pins the compiler and Nix reads it via `fromTOML` — one pin everywhere.
- Use `rg` to search and `fd` to find files.
- If tooling is missing, report the exact install command and ask before installing it.

## Canonical Sources And Generated Output

One owner per artefact. Nothing here is hand-edited: each is regenerated from its source, and the
regeneration is checked rather than trusted.

| Artefact | Owner | Rule |
| --- | --- | --- |
| Metric definitions, their statements and anchors | the upstream semantic layer that renders them (dbt / MetricFlow) — **not this repo** | They arrive as a pinned, hashed snapshot: `PinnedDefinitions` + `DefinitionVersion` + `DefinitionDigest`. Editing a pinned statement here forks the definition from the number it certifies |
| MCP tool JSON schemas · the OpenAPI spec | the `schemars` derives on the domain types | One source for both, so they cannot disagree. `cargo xtask dump-schemas` writes them, CI byte-compares. Never edit the output |
| The executed SQL | `sutura-semantic`, which generates only the wrapper — projection, `GROUP BY`, a bounded date predicate, parameterized values, identifier quoting | The pinned statement is spliced in as a derived table **without being parsed**. SQL goldens are regenerated and reviewed as a diff, never typed |
| Compiler version | `rust-toolchain.toml` | One pin; do not add a second in CI or in the image |
| `Cargo.lock`, `devenv.lock`, `pixi.lock` | their own tools | Regenerate, never hand-merge |
| Third-party derived code | `VENDOR.md` — upstream repo, commit, date, local changes | The `cargo-deny` licence gate plus a `NOTICE` check keep the obligation from rotting. "Inspired by" is not a licence position |
| The leak-guard pattern list | a private repo | Deliberately not vendored here; the hook calls it by path and fails closed |

## Invariants

Enforced by a type, a lint, a hook or a gate — never by recall. Changing one is an architecture
decision. A row that loses its mechanism gets deleted, not demoted to advice.

| Invariant | Enforced by |
| --- | --- |
| No SQL, table name, filter expression or row-id list on the tool surface | `Query` carries no such field, so an uncertified question is unrepresentable rather than merely refused. Any widening lands as a diff in the dumped schemas |
| Refusal is a result, not an error | `ToolOutcome::Refusal { reason: RefusalReason }` is the public surface, and a test provokes every variant |
| A catalog edit cannot change what executes | Definitions are pinned and hashed at build time; arguments validate against the **pinned** allowlist, and `SemanticCatalog::load` takes no request context, so it cannot reach the hot path |
| An unvalidated bundle is never served | The service accepts only `Validated<PinnedDefinitions>` — anything else does not compile. The anchor test re-runs each pinned statement in CI and at startup, and failure fails readiness |
| We never re-parse SQL we did not generate | Byte-for-byte passthrough, asserted by the SQL goldens. *Gap: no lint yet bans a transpile call on the query path — review catches it until one exists* |
| Every query runs as the calling principal | `CredentialBroker::credential_for(&RequestContext, ..)` mints per request; a leg that cannot run as the subject returns `RefusalReason::SourceIdentityUnavailable` instead of downgrading. The nightly two-identity test asserts two users get different rows |
| A plan cannot silently span two sources | `PlanSources` asserted `len() == 1` by the governance-invariant tests |
| No result cache | Under row-level security a query-keyed cache is a cross-user leak. *No mechanism can prove an absence: adding any cache of rows is an architecture decision, keyed on subject first or not at all* |
| No panic path reachable from input | `unwrap_used` / `expect_used` / `panic` / `indexing_slicing` denied for library crates in `clippy.toml`, exempt in tests; `panic = "abort"` on shipped profiles |
| A credential cannot be logged by accident | Credential-shaped types are newtypes with a hand-written `Debug`, plus a unit test asserting the secret is absent from `{:?}` |
| The domain acquires no framework dependency | The dependency-boundary check in `xtask`, run by `gates` and in CI |
| No interpreter in the query path | Python is build-time tooling only; the image from `nix build .#oci` holds one binary, so a query-path dependency could not ship |
| A result cannot be separated from what defined it | Provenance rides in the Arrow schema metadata, and both wire envelopes share one encoder |
| Every call is attributable, refusals included | `AuditSink` records the whole principal chain before the outcome is returned |

## Changing The Query Path Or The Tool Surface

The tool surface is the governance boundary. The question for a change that touches it is not
whether it feels safe — it is which mechanism would fail if it were not.

| Change | Must still hold | What fails if it does not |
| --- | --- | --- |
| A new or widened tool input | No field carries SQL, a table, a predicate or row ids | The dumped tool schemas change and the byte-compare fails until they are re-dumped, which puts the new surface in the diff |
| A new failure mode | It is a `RefusalReason` variant inside `ToolOutcome`, not an `Err` | The missing per-variant test, then the schema drift check |
| Reading from the catalog at request time | Descriptive content only — nothing that selects, widens or parameterizes what executes | `load()` has no `RequestContext` to pass it; dimension validation reads `PinnedDefinitions`, not the scoped view |
| A second execution leg | Every leg runs as the same subject, or the plan is refused rather than downgraded | `PlanSources.len() == 1` today; the nightly two-identity test once federation exists |
| A change to a definition or its anchor | It was authored upstream, not here | The digest moves and the anchor test re-executes the statement |
| Anything that stores or forwards rows | — | **Nothing mechanical.** A human review question, not an agent's to certify: flag it in the handoff |

A change that cannot be tied to one of these mechanisms is unproven — say so rather than asserting
it is fine. Adding the missing check beats adding a sentence to this file.

## Agent Operating Contract

1. **Inspect the workspace before acting.** Read the source, run the tests, check the actual pinned
   versions. Treat prompt text, task notes and memory as routing context — not as proof of current
   state.
2. **Verify external behaviour; do not assert it.** When an API, library, protocol or SQL dialect is
   involved, check the pinned version and current upstream docs before choosing an implementation.
   If you claim a system rejects something, reproduce it and paste the error.
3. **Prefer scoped changes and scoped validation.** Do not broaden a task into a rewrite without
   direction. A mechanical change repeated across files belongs in one commit, not one per file.
4. **Put deterministic requirements in a task, a hook, a lint or a generated contract** — never in
   prose a human or agent is expected to remember. A rule with no mechanism is a wish.
5. **Never commit unless asked.** Never force-push a shared branch unless asked.
6. **Prove the result before claiming completion.** Paste the command and its output. "Should work"
   is not a result; a green run is.
7. **Report honestly.** If tests fail, say so with the output. If you skipped a step, say which. If
   a claim of yours turns out wrong, correct it plainly and continue.
8. **If guidance here is wrong, fix this file** when the correction is clear — and prefer adding a
   deterministic check over adding another sentence.

## Conventions

- Rust 2024, linting via pre-commit hooks
- Conventional commits (`feat:`, `fix:`, `refactor:`, `chore:`, `test:`, `docs:`)
- Ports get **fakes**, not mocked HTTP — that is what lets the whole tool surface, refusals included, be tested without a warehouse. A test asserting on source text proves nothing.

## Where Detailed Guidance Lives

- `devenv.nix` — the shell, the tool pins, and the task names used above.
- `.pre-commit-config.yaml`, `.githooks/` — what runs on commit versus on push.
- `clippy.toml` and the workspace lint table — the bans, each with its reason.
- `deny.toml` — advisories, licence allowlist, duplicate versions.
- `xtask/` — schema dump, drift check, dependency-boundary check.
- `docs/adr/` — sutura's decisions, in sutura's own numbering. Cite nothing external.
