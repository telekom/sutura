# AGENTS.md

sutura is an identity-aware semantic data runtime for AI agents. Given pluggable metadata and data
sources it compiles a semantic query plan and even allows for (light) federated queries. Security is
key! We support e2e impersonation.

Guidance for coding agents; root of trust. `CLAUDE.md` and any other agent-specific file reference
this one. Status: early - the plan is settled, the code is not.

**This file is a routing table, not an argument.** A row here is one line: the rule, and the
mechanism that enforces it. The reasoning, the limits and the history live in the documentation, and
**a claim is only as good as the limit stated with it** - so read the long form before citing a
mechanism in a review or a pull request description.

| Long form | Where |
| --- | --- |
| Every invariant with its mechanism and its limit, what is built and not wired, what a change to the query path must still hold | `docs/invariants.md` |
| Newtypes, typed errors, ports and adapters, borrowing, secure by design | `docs/design-principles.md` |
| What version to target, a duplicate against a type boundary, the escalation order | `docs/dependency-currency.md` |
| The environment, the gates, commits, branches, releases, pull requests | `CONTRIBUTING.md` |
| The settled design and what ships | `docs/architecture.md` |
| Decisions, in sutura's own numbering | `docs/adr/` |

## This Repository Is Public

`origin` is `github.com/telekom/sutura`. Everything here is world-readable: docs, comments,
fixtures, commit messages, branch names.

Do not commit: internal product/platform/service names, non-public hostnames, domains, wiki or
tracker URLs and page ids, filesystem paths outside this repo or other repo/directory names, cloud
project/account/tenant ids, people's names, usernames or emails (use `user@example.com`), internal
classification schemes (use `internal` / `confidential` / `restricted`), references to documents that
live elsewhere.

A description specific enough to identify any of the above is disclosure. Write the capability and
its constraint generically - *"where a gateway enforces auth centrally and requires services to
validate a short-lived token proving the request transited it"* - and the point usually improves. The
pattern-matching backstop lives OUTSIDE this repository by design, and it cannot catch a paraphrase.
**The control is not writing it down here.**

## Layout

`sutura-domain` is the hexagon's interior. Everything else is an adapter, and nothing depends on an
adapter. A port trait arrives with its first implementor. Detail: `docs/architecture.md`.

| Crate | Role |
| --- | --- |
| `sutura-domain` | Domain types and port traits. Deps: `serde`, `serde_json`, `sha2`, `secrecy`, `thiserror`. No framework |
| `sutura-semantic` | `Query` to `QueryPlan`. Renders nothing and names no dialect |
| `sutura-sql` | `QueryPlan` to one statement in one dialect, `LegPlan` to one leg's. The only crate that names `polyglot-sql` |
| `sutura-app` | The service, generic over the ports. Holds the `Surface` driving port, `LocalService` and `Warehouses` |
| `sutura-catalog-local` | `SemanticCatalog` over a directory of markdown documents with YAML frontmatter |
| `sutura-exec-datafusion` | THE engine. A plan becomes a logical plan over Arrow; no SQL is generated |
| `sutura-exec-duckdb` | `Warehouse` over DuckDB, in-process, rendering through `sutura-sql`. A dev-dependency, and the only adapter declaring `EXECUTES_LEGS` |
| `sutura-exec-bigquery` | `Warehouse` over BigQuery: renders the fourth dialect, pushes down, declares `PerSubjectCredential`, and has a wire behind a default-off `wire` feature. Registered in `sutura-serve` behind a default-off `bigquery` feature, and in no published artifact |
| `sutura-exec-postgres` | `Warehouse` over Postgres on a static credential. Pure-Rust driver, `NoTls`, localhost tier only. A dev-dependency; its cells run under `nix/postgres-tier.nix` |
| `sutura-config` | The settings tree, the `Environment`, the startup refusals, and `StaticCredentialBroker` |
| `sutura-runtime` | Process-global concerns: tracing subscriber, panic hook, shutdown signal, banner |
| `sutura-http` | Transport only: versioned `v1` tree, liveness probe, generated interface description, rate limiting, bearer gate, optional TLS, and leg 1 (`inbound`). The bearer token authenticates the deployment; leg 1 authenticates the caller |
| `sutura-mcp` | The agent-facing surface over the Model Context Protocol: two tools, one per `Surface` operation, each schema a committed snapshot. No composition root links it yet |
| `sutura-serve` | Composition root for the HTTP surface. Synchronous down to one `block_on` |
| `sutura-cli` | The binary; composes adapters, and links the engine only |
| `xtask` | The repo gates |
| `-datahub`, `-clickhouse`, `sutura-arrow` | Planned. None exists |

Rules:

- **An adapter with a native or an outbound-TLS dependency arrives behind a default-off feature** on
  whichever composition root wants it. `crane.buildDepsOnly` is deliberately unscoped so the checks
  can share one dependency derivation, so the four `cross` CI jobs compile the whole workspace's
  dependency closure for their target; those jobs are the gate that says whether it was necessary.
- A data system's driver is a dev-dependency, which is what keeps the musl artifacts building.
  `nix/duckdb.nix` is the single path from nixpkgs to that library.
- `cargo check -p sutura-domain --no-default-features` is the inner loop; keep it under a second.
  `just check` runs it and prints what it covered, and `cargo xtask check-scope` keeps that notice
  equal to the flags above it. No gate can know what a developer believed a task covered:
  `just check-changed` is the cheap answer to "does what I touched compile".

## Commands

`direnv allow` once per clone. Then:

```bash
just validate       # THE gate. Run this before saying a change is done
just check          # fast inner loop, domain crate only, and it says so on the way out
just check-changed  # cargo check over what your working tree actually changes
just test           # tests
just lint           # clippy
just fmt            # format
just docs           # render the site
just api            # regenerate the committed API pages
just classify       # what does this change require?
just causality      # red-before-green proof
just ship-check     # the finishing gate
just update         # bump every lock
just doctor         # is this machine set up
```

`just` with no argument lists the rest. Why each gate is shaped the way it is: CONTRIBUTING.md.

Rules:

- **`just validate` is the only thing that counts as verified.** It runs the nix checks, which build
  a GIT-DERIVED copy of the tree - so an untracked file is invisible to them and a new module
  compiles under `cargo` and then does not exist in the sandbox. `git add -N` makes it visible.
- **Any directory a build or a test reads has to be named in `flake.nix`'s source filter**, which
  drops what it does not name. `nextest`, `hygiene`, `crap` and `api-docs` set `src = ./.` and read
  the whole tree; `clippy`, `doctest`, `fmt`, `packages.xtask` and the release builds read the
  filtered copy.
- **Never run a bare `cargo clippy` or `cargo nextest` when a `just` task exists - run the task.**
  `just lint` and `just test` source `nix/stable-env.sh` and add `-D warnings`, and a hand-written
  line diverges on BOTH counts: the dev shell's cargo is a cranelift nightly reporting lints stable
  has not got, and without `-D warnings` a `restriction` finding is a warning a grep for `^error`
  does not see. Naming only the first is how this rule gets complied with and still fails.
- Cite a `just` task, never a raw command line. `cargo xtask check-guidance` fails on a citation of a
  task that does not exist, and on a cited `cargo` line missing `--all-features`.
- **`just docs` is owed by any change that touches a doc comment.** No nix check builds the site, so
  a cross-crate rustdoc link that `just api` copies through verbatim passes every gate and then
  fails `mkdocs build --strict`. Plain backticks are the safe form for a cross-crate reference.
- nix is the only pin for a tool whose version changes what it reports; pixi holds `prek` and
  `python`. `cargo xtask check-pins` fails if a tool appears in both.
- If tooling is missing, report the exact install command and ask before installing it.
- The leak guard is not a hook here: its pattern list lives in a private repo and runs from there.
- **This shell's environment follows `cargo` into OTHER repositories and breaks builds there.**
  `CARGO_UNSTABLE_CODEGEN_BACKEND`, `CARGO_PROFILE_DEV_CODEGEN_BACKEND`, `DUCKDB_LIB_DIR` and
  `DUCKDB_INCLUDE_DIR` are exported unscoped, so an unrelated checkout gets built by cranelift with
  OUR DuckDB. Unset them first, and treat a red run from this shell as unexplained until you have.

## Canonical Sources And Generated Output

One owner per artefact. Nothing here is hand-edited: each is regenerated from its source, and the
regeneration is checked rather than trusted. The reasoning and the limits: `docs/invariants.md`.

| Artefact | Owner | Kept honest by |
| --- | --- | --- |
| Metric definitions and anchors | the catalog | `PinnedDefinitions` + `DefinitionVersion` + `DefinitionDigest`; `docs/adr/0001` |
| MCP tool schemas | a `schemars` derive on a WIRE type in the transport, never on a domain type | an `insta` snapshot per capability under `crates/sutura-mcp/src/snapshots/` |
| Which tools the surface has, and the scope each needs | `sutura_app::Capability` | one declaration, and a test per transport against it |
| The OpenAPI spec | `utoipa` derives on `sutura_http::wire` | built at startup and served, so a missing `#[utoipa::path]` does not compile |
| The executed SQL | `sutura-sql` | goldens per dialect, regenerated and reviewed as a diff; `differential.rs` runs one plan both ways |
| The API pages | rustdoc JSON | `just api`, byte-compared by `cargo xtask check-api-docs` |
| Compiler version, anything shipped | `rust-toolchain.toml` | one pin for CI, the release build and the image |
| Compiler version, local inner loop | `devco/rust-toolchain-nightly.toml` | two narrow uses: cranelift locally, and rustdoc JSON in `checks.api-docs` |
| `Cargo.lock`, `devenv.lock`, `pixi.lock` | their own tools | `just update`; never hand-merge |
| Third-party derived code | `VENDOR.md` | the `cargo-deny` licence allowlist, with `unused-allowed-license = "deny"` |
| The leak-guard pattern list | a private repo | called by path, and it fails closed |

## Dependency Currency

- **Target the newest set of versions that resolve together on the day the work is done** - not the
  newest of each crate, and not whatever a record happened to name. A record states the CONSTRAINT;
  where a number is unavoidable it carries the date it was checked.
- `sutura-exec-datafusion` is THE engine, so its Arrow major is the workspace's and every other
  Arrow-consuming dependency conforms to it. Downgrading the engine is not on the table.
- A **duplicate** (two majors present, no first-party code crossing) costs build time and
  supply-chain surface. A **type boundary** (first-party code hands a value from one major to the
  other) blocks. The test is whether any first-party crate names the type, not the lock file.
- `cargo xtask check-arrow` fails when the `arrow-*` family spans more than one major, unless
  `devco/arrow-majors-allow` names each major with a date and a reason.
- A crate whose version encodes the version of a **native library it expects to find** pins with `~`,
  not `^`: under a caret `just update` can move it to a release wanting a newer native library, which
  links and then fails at runtime on a missing symbol.
- **When the newest set cannot be made compatible, do not vendor, inline, fork or pin back on your
  own judgement. Raise it.** The four options in order, what each costs, and the four things an
  escalation has to state: `docs/dependency-currency.md`.

## Invariants

Enforced by a type, a lint, a hook or a gate - never by recall. Changing one is an architecture
decision. A row that loses its mechanism gets deleted, not demoted to advice.

**A row here is the claim; the LIMIT stated with it in `docs/invariants.md` is part of that claim.**
Every mechanism below has one, several are narrower than they read, and a control described as
stronger than it is spends trust a reviewer needed elsewhere.

| Invariant | Enforced by |
| --- | --- |
| No SQL, table name, filter expression or row-id list on the tool surface | `Query` declares no such field, and `deny_unknown_fields` makes an attempt an error naming it |
| Refusal is a result, not an error | `ToolOutcome::Refusal`; a golden provokes every variant a question can reach |
| A question's time range is bounded, and bounded to a size | `TimeRange` has no unbounded form; `resolve` refuses a span over `MAX_RANGE_DAYS` (3653) |
| A catalog edit cannot change what executes | `PinnedDefinitions` carries a digest over the canonical form, and the digest travels with the answer |
| A result cannot be separated from what defined it | `PinnedDefinitions::pin` computes the digest from what it stores; `ToolOutcome::Answer` has no constructor omitting `Provenance` |
| An unvalidated bundle is never served | `sutura_app::verify_and_validate` is the only constructor of `Validated`, in a private module; two `compile_fail` doctests with compiling twins |
| A bundle with an anchor on a source that has no identity to re-run it under does not boot | `SourceIdentity::anchors_run_as` is exhaustive over a three-variant `AnchorIdentity`; `sutura-serve`'s `refuse_unverifiable_anchors` names the metric and the source |
| A source is served under a declared identity, or the deployment does not boot | `SourcePosture` has no `Default`, and its shared variant needs a witness no file can produce; `Settings::refusals` |
| Which adapter opens a source is decided by a declaration, not by its name | `sutura_config::SourceKind` is a closed set, and `sutura-serve`'s `open_engine` matches it with no wildcard arm |
| A posture the linked adapter cannot deliver does not boot | `Warehouse::IMPERSONATION` is a required associated constant with no default; `SourcePosture::deliverable_by`, once per source per adapter |
| No leg of an ANSWER reaches a data system without a credential somebody minted for it | `Warehouse::execute` takes a `&Presented` and has no default; `LegCredentials` from `identity::CredentialBroker::mint`, once per answer per source |
| A subject with no credential at a source is refused, not answered as the process | `RefusalReason::CredentialUnavailable`, `403 credential_unavailable`, from the exhaustive match each transport holds |
| An answer says which identity produced each of its legs | `Provenance` carries an `ExecutedAs`, `Provenance::new` is private, and the record is a required argument |
| We never TRANSLATE SQL, and the one thing we parse is parsed at load | the dialect layer's `transpile` feature is not compiled, so a call does not build. Deliberately not a lint: an unresolvable `disallowed-methods` entry is ignored silently at the pinned version |
| No value from a question reaches the statement as text | every value is a bind parameter, and `GeneratedQuery` has no merging constructor; the goldens count one placeholder per value per dialect, plus two searches |
| No identifier reaches the statement unquoted | forced quoting for identifiers and aliases; a golden over the corpus with quoted spans stripped first |
| Every generated statement is well formed SQL, and parses under its target dialect | the golden suite parses each statement with the dialect it was generated for |
| Adding a metadata provider or a data system is a registration, not a test edit | the golden corpus, the refusal corpus and the anchor check are a matrix over `tests/adapters`; a required `SemanticCatalog::KIND` routes which cells a catalog gets |
| The measure vocabulary is closed, and holds no SQL expression | `Measure` is two shapes over a `Term` of two terms, `RequiredFilter` four operators, `deny_unknown_fields` at every depth |
| The panicking fragment API cannot be called | `clippy.toml` bans `polyglot_sql::parser::Parser::new` and `::parse_expressions`, both verified to resolve |
| No `#[expect]` on a count-threshold lint | `cargo xtask check-expect-thresholds` |
| A definitional filter is always applied | `required_filters` compile into every plan for the metric, marked `PredicateOrigin::Definition` |
| A join cannot silently change a measure | `Definitions::assemble` refuses a dimension reached through a relationship whose declared cardinality may duplicate rows; a reconciliation test |
| A result that is too much data is refused, not truncated - and not reported as an outage | `row_limit()` is `max_rows + 1`; `RefusalReason::ResultTooLarge` carries a closed `ResultBound`, one code, `413` and never `503`. 93 SQL goldens read `LIMIT 10001` |
| The engine's operators run against a bounded memory pool, and exhaustion is a refusal rather than process death | both `SessionContext` sites take a `GreedyMemoryPool` sized from `runtime.working_set_max_bytes`; `WorkingSetCeiling::parse` refuses a zero and an over-large value |
| Nothing spills the asking subject's rows to local disk | `DiskManagerMode::Disabled` on every session this adapter builds |
| Two result columns cannot share a label | `Definitions::assemble`, compared under `IdentifierCase::COARSEST` rather than by equality |
| A catalog document's fields are exactly what it declares | `deny_unknown_fields` on every on-disk shape |
| A table outside the connection's own dataset is a composition of parsed names, never a string with dots in it | `QualifiedTable` over an `Option<TableQualifier>` and a `TableName`, each part parsed by the parser for its position |
| How deep a table path a data system resolves is declared, not guessed | `Dialect::qualification` is an exhaustive match; a deeper path is `GenerateError::QualificationUnsupported`, never a dropped qualifier |
| Two tables one statement reads can be told apart inside it | `sutura_domain::plan::StatementTables` is the only way to a `QueryPlan` and to a `LegPlan::Fact`, and its `parse` refuses two paths that collapse to one identifier |
| Whether a target folds an identifier's case is declared, not guessed | `Dialect::identifier_case` is an exhaustive match, and the bundle's own checks compare under `IdentifierCase::COARSEST` |
| Cross-project is not federation, and a native cross-project join is ONE source | a source is a credential plus a billing project: `sutura_semantic::plan` collects `SourceName` and nothing from any table path |
| A label a statement projects cannot be spelled the same as a table it reads | `Definitions::assemble` refuses `LabelShadowsTable`, case-folded, for every dialect |
| A question cannot silently span more sources than the answer can combine | three or more is `PlanSpansTooManySources`; exactly two is split into legs, or refused as `FederationNotExecutable` while no selected adapter declares `EXECUTES_LEGS` |
| No result cache | there is none to key. Adding any cache of rows is an architecture decision, keyed on subject first or not at all |
| No panic path reachable from input | `unwrap_used`, `expect_used`, `panic`, `indexing_slicing` and the integer overflow lints denied; `panic = "abort"` |
| A credential cannot be logged by accident | `Secret` wraps `secrecy::SecretString`, which has no `Display`, so `format!("{token}")` and a `%` tracing field do not build; two `compile_fail` doctests with compiling twins |
| A credential cannot be compared by accident | `Secret` implements no `PartialEq`, so `==` does not compile; `AccessToken::matches_in_constant_time` is the one real comparison |
| The domain acquires no framework dependency | `cargo xtask check-boundaries` walks the whole transitive tree against `ALLOWED_IN_DOMAIN` |
| The SQL generator is not in the compiler's closure | `FORBIDDEN_EDGES` forbids `sutura-semantic` reaching `polyglot-sql` AND `sutura-sql`; the second stops the first returning transitively |
| A driving port is not owned by one of its callers | `Surface`, `SurfaceFailure` and `LocalService` live in `sutura-app`. Not gated: `check-boundaries` reads dependency direction |
| A newtype's invariant cannot be walked around | `check-boundaries` fails a `pub` field on a `pub struct` in a library crate |
| A library crate's errors are typed, not prose | `check-boundaries` fails `Result<_, String>` and a dynamic-error crate in a library. Binaries are exempt |
| No file exceeds 1000 lines | `cargo xtask max-lines`; `devco/max-lines-ignore` cannot exempt anything under `crates/` or `xtask/` |
| Complexity in the invariant core is covered by tests | `cargo xtask check-crap`, threshold 30, scope `sutura-domain` |
| No dependency is declared and unused | `cargo xtask unused-deps` |
| No first-party `unsafe` | `unsafe_code = "forbid"`, so a crate cannot re-allow it locally |
| Dead code does not accumulate, and cannot hide behind `pub` | `dead_code = "deny"` plus the unreachable-`pub` lint |
| A suppression cannot outlive its cause | `#[expect]` over `#[allow]`: an expectation that stops firing is a warning, and `-D warnings` makes it an error |
| No interpreter in the query path | no scripting engine is a dependency, and `check-boundaries` holds the domain's tree to its allowlist |
| Knowledge read from the catalog is descriptive only | the prompt is its only consumer; `Query` has no field a phrase fits in and `load()` takes no `RequestContext` |
| No server-side phrase resolution | there is nothing to resolve into, and `RefusalReason` has no `PhraseNotDefined` |
| A metadata adapter cannot be silent about a kind it cannot supply | `SemanticCatalog::capabilities` is a required associated item with no default; a `compile_fail` doctest with a compiling twin |
| Content for a knowledge capability nobody declared fails the load | `Knowledge::assemble` walks `Capability::every()` and refuses `UndeclaredContent` |
| A note is attached to something the bundle declares | `CaveatAboutNothing` refuses an unscoped caveat; every other kind carries a `Referent`, a checked `Phrase` or a `Query` |
| Note prose is bounded, and refused at load rather than cut at render | `NoteBody::parse`, `MAX_KNOWLEDGE_BYTES` over the aggregate, and `Phrase::parse`. Nothing anywhere shortens a body |
| A worked example is a question this surface would accept | `Knowledge::assemble` checks each example's `Query` against the metric, its grains, its dimensions, its allowlists and the query bounds |
| The knowledge declaration is under the definition digest | `PinnedDefinitions::pin` hashes the `Knowledge` alongside the definitions, the `KnowledgeCapabilities` included |
| Work handed to the blocking pool carries the request's span | `clippy.toml` bans `tokio::task::spawn_blocking`; `sutura_runtime::spawn_carrying_span` is the one caller, and an integration test asserts the span survives |
| No caller-supplied text reaches the log unbounded or unvalidated | `CorrelationId::parse` bounds the length and the character set, and mints a fresh id rather than erroring |
| A call attributed to a subject can be told apart from one attributed to an agent acting for them | `identity::PrincipalChain` is ordered, and `attribution()` is the only way to the actors; `ActorChain` has no empty state |
| A caller cannot state its own identity | no type in `identity::principal` implements `Deserialize`, and `VerifiedCaller`'s one constructor is `pub(crate)`; two `compile_fail` doctests with compiling twins |
| A caller's identity, where one is established, comes from a signature and never from a header | `sutura_config::InboundIdentity` is a closed enum with no default; `TransitProof` has no field for a header name; `SigningAlgorithm` has no `None` and no `HS*` variant |
| A signature, an issuer and an audience do not decide a token's CLASS, so the class is decided separately | `RequiredTokenType`, checked on `decoded.header` AFTER the signature. A token with no `typ` is refused, so the check is not satisfiable by omission |
| A key removed from the key set stops verifying within a bound a caller cannot influence | `KeySetCache` re-reads once per `MAX_KEY_SET_AGE`, from a composition-root timer and from `key_for` itself |
| A gateway assertion's replay window is this deployment's number, not the component's | `sutura_config::ProofLifetime` caps `exp - iat`, and `iat` is required in `behind-gateway` |
| A deployment that declares an inbound identity cannot serve without one | `sutura_http::router::assemble` returns `RouterNotBuilt::InboundIdentityNotAttached` |
| A forged key id cannot turn every request into an outbound call, whatever the concurrency | `KeySetCache::reserve` compares the window and stamps the attempt in ONE write-lock acquisition, and is the only place either window is compared |
| Two credentials cannot be configured to arrive in one header | `NotFitToServe::DeploymentTokenSharesTheHeader`, asked of the derived `TokenRequirement` rather than of the enum variant |
| An operation a caller was not granted is neither advertised nor answered | `sutura_app::Capability` is closed with three exhaustive matches; `require_capability` is a layer, `RouteNotGoverned` refuses an ungoverned route, and `AgentSurface::new` requires a `Permitted` |
| Every outcome, answer and refusal alike, is recorded before it is returned | `audit::AuditSink` takes a `CallRecord` and returns nothing a caller can branch on, and `LocalService::start` requires a sink |

### Built And Not Wired

**Nothing here is an invariant, and none of it may be cited as one.** It is code that exists, is
tested, and has no caller from any shipped binary. **The sentence that matters: no source a
deployment SERVES executes as the asking subject.**

- **The catalog-authored SQL hatch** - `sutura_domain::expression` and `sutura_sql::expression`,
  `docs/adr/0004`. No `Metric` holds a `Computation`, no `MetricDoc` writes `authored_sql`, and no
  composition root calls `compile`. Every gate passes over it, because the missing thing is a CALL.
- **`sutura-exec-bigquery`** decides, renders and refuses correctly, has a wire, has had the whole
  corpus accepted by a real dataset, and is registered in `sutura-serve` behind a default-off
  `bigquery` feature. No published artifact links it: the image and all four cross binaries are
  `sutura-cli`, and `sutura-serve` is no `nix` package.
- **`WorkloadIdentityBroker`** really exchanges a subject's assertion, and no served source is opened
  against it: `build_bigquery` refuses `impersonation-at-source` by name.
- **Federation** is wired through `answer_federated` and proved over the DuckDB dev vehicle, and both
  shipped adapters declare `EXECUTES_LEGS = false`, so the shipped binary refuses a two-source
  question.

What each of those does and does not prove, in full: `docs/invariants.md`.

## Changing The Query Path Or The Tool Surface

The tool surface is the governance boundary. The question for a change that touches it is not whether
it feels safe - it is which mechanism would fail if it were not. The full table, with what each
mechanism does NOT cover: `docs/invariants.md`.

| Change | What fails if it does not hold |
| --- | --- |
| A new or widened tool input | `deny_unknown_fields` per wire shape, asserted through each transport, plus the committed MCP schema snapshot per capability. There is no equivalent dump for the OpenAPI document |
| A new failure mode | two exhaustive matches with no wildcard arm, one per transport, plus a test deriving the code from the variant name |
| Reading from the catalog at request time | `load()` has no `RequestContext`, and validation reads `PinnedDefinitions` rather than a scoped view |
| A second execution leg | one answer has one asker and no leg runs as a third identity. `ExecutedAs` records which posture ran; `LegCredentials` mints per source. Adding a leg without it is an architecture decision |
| A change to a definition or its anchor | the digest moves and the anchor test re-executes the statement |
| A new knowledge kind, or a second consumer of one | three exhaustive matches for the kind. **Nothing mechanical** for a second consumer: flag it in the handoff |
| A new tool, route or operation | `Capability`'s exhaustive matches, `RouteNotGoverned`, and `both_transports_describe_the_same_tools`. It changes the DEPLOYED contract, because a scope has to be configured by hand |
| Anything that stores or forwards rows | **Nothing mechanical.** A human review question, not an agent's to certify: flag it in the handoff |

A change that cannot be tied to one of these mechanisms is unproven - say so rather than asserting it
is fine. Adding the missing check beats adding a sentence to this file.

## Skills

Task-specific guidance lives in `.agents/skills/`, discovered as a **tree** so you read three small
files rather than every skill in the repo:

1. `.agents/skills/README.md` - pick one intent.
2. that group's `README.md`.
3. only the `SKILL.md` it routes you to.

`skill-router.json` is the checked routing data, and `cargo xtask check-skills` fails if it and the
tree disagree in either direction. **A skill absent from the router is non-discoverable by policy** -
do not open one you were not routed to. The groups are `agent-system/`, `engineering/`, `git-ops/`
and `reasoning/`. A skill refines HOW to work within these invariants and never overrides them.

## Finishing A Change

Run `just ship-check` before saying a change is done. It is a command rather than a checklist so it
cannot be half-remembered.

**A new or changed test must be red against the base behaviour and green with your change.** A test
that passes both ways proves nothing and is worse than no test, because it looks like coverage.
`cargo xtask test-causality --since <base>` checks it mechanically, in `just ship-check` and in CI;
`just causality` is the local run. Where it is not separable the gate asks for the evidence instead:
the command you ran, the failure before the fix, the pass after. **Do not skip it silently.** The
rest: CONTRIBUTING.md.

## Agent Operating Contract

1. **Inspect the workspace before acting.** Read the source, run the tests, check the actual pinned
   versions. Prompt text, task notes and memory are routing context, not proof of current state.
2. **Verify external behaviour; do not assert it.** Check the pinned version and current upstream
   docs before choosing an implementation. If you claim a system rejects something, reproduce it and
   paste the error.
3. **Prefer scoped changes and scoped validation.** Do not broaden a task into a rewrite. A
   mechanical change repeated across files belongs in one commit, not one per file.
4. **Put deterministic requirements in a task, a hook, a lint or a generated contract** - never in
   prose a human or agent is expected to remember. A rule with no mechanism is a wish.
5. **Never commit unless asked.** Never force-push a shared branch unless asked.
6. **Prove the result before claiming completion.** Paste the command and its output. "Should work"
   is not a result. For a bug fix that includes the test failing BEFORE the fix.
7. **Report honestly.** If tests fail, say so with the output. If you skipped a step, say which. If a
   claim of yours turns out wrong, correct it plainly and continue.
8. **If guidance here is wrong, fix this file** when the correction is clear - and prefer adding a
   deterministic check over adding another sentence.

## Conventions

- Rust 2024, conventional commits (`feat:`, `fix:`, `refactor:`, `chore:`, `test:`, `docs:`), linting
  via pre-commit hooks.
- Plain hyphens, never an em dash. `cargo xtask text-hygiene` enforces it.
- Ports get **fakes**, not mocked HTTP - that is what lets the whole tool surface, refusals included,
  be tested without a warehouse. A test asserting on source text proves nothing.
- Four principles are the definition of *correct* in a review here: a **newtype parses rather than
  validates**, so a violating value is unrepresentable; an **error is a typed enum whose fields carry
  the context**, because the variant is the contract and the message is not; **dependencies point
  inward**, so the domain names what it needs and adapters implement it; and **security comes out of
  modelling the domain precisely, not out of a layer on top**. The long form, with the source each
  rule comes from and which rules only review catches, is `docs/design-principles.md`;
  `.agents/skills/engineering/rust/SKILL.md` walks the same ground mistake by mistake.

## Where Detailed Guidance Lives

| Topic | Where |
| --- | --- |
| The shell, the tool pins, the task names | `devenv.nix` |
| The flake modules, one topic each | `nix/*.nix`. `apps.<name>`, the `packages = ` block and the `checks = {` block may NOT move out of `flake.nix`: `xtask` scans that file textually and fails closed on finding none |
| What runs on commit, on commit-msg and on push | `.pre-commit-config.yaml`. `cargo xtask check-hook-tiers` keeps the documented tiers equal to the declared ones; `--no-verify` bypasses them, so neither is an invariant |
| The bans, each with its reason | `clippy.toml` and the workspace lint table. The whole `restriction` category is on |
| Advisories, licence allowlist, duplicate versions | `deny.toml` |
| Config only this repo's own tooling reads | `devco/` |
| Every gate, each unit-tested | `xtask/`. `cargo xtask --help` lists them and `just hygiene` runs the cheap ones. `classify`, `check-changed` and `changed-packages` decide what a diff requires and **fail open**; `clean-branches` points the other way, is a dry run unless `--delete` is passed, and keeps any branch it cannot decide about |
| CI | `.github/workflows/`: `ci.yml` on every push and pull request, `release.yml` on a `v*` tag, `release-performance.yml` on manual dispatch with a typed confirmation. None installs devenv |
| Task guidance, entered through the router | `.agents/skills/`. Not a substitute for this file |
| The published site | `docs/` and `mkdocs.yml`. `cargo xtask check-docs` fails on a page in no `nav` entry, a `nav` entry with no file, or a missing asset |
| Third-party material adapted here | `VENDOR.md`, with upstream, licence, commit and changes |
