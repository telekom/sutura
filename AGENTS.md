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
| `sutura-sql` | `QueryPlan` → one statement in one dialect, and `LegPlan` → one leg's statement through `generate_leg`. The only crate that names `polyglot-sql` |
| `sutura-app` | The service, generic over the ports. Holds the `Surface` driving port and `LocalService`, its one implementor, plus `Warehouses` - the data systems this process opened, keyed by the name a plan selects them with. **Every entry is the same adapter type**, so a heterogeneous set is an architecture decision rather than a change to that file |
| `sutura-catalog-local` | `SemanticCatalog` over a directory of markdown documents with YAML frontmatter |
| `sutura-exec-duckdb` | `Warehouse` over DuckDB as a data source: renders through `sutura-sql` and pushes down. A dev-dependency, not shipped |
| `sutura-exec-datafusion` | THE engine. A plan becomes a logical plan over Arrow; no SQL is generated. Behind the `Warehouse` port today; belongs above it once federation lands |
| `sutura-config` | The settings tree, the `Environment`, the startup refusals. No framework; reads paths, opens no socket. Also **`StaticCredentialBroker`**, the first implementor of the credential port - it is here because the identity provider it reads IS the settings tree: it mints what an operator declared per source and opens nothing |
| `sutura-runtime` | Process-global concerns: tracing subscriber, panic hook, shutdown signal, banner |
| `sutura-http` | Transport only. Versioned `v1` tree, liveness probe, generated interface description, rate limiting, bearer gate, optional TLS, and **leg 1** - `inbound`, which verifies a caller's own token where a deployment declares `security.inbound`. **The bearer token authenticates the deployment, not the caller; leg 1 authenticates the caller and still does not make a source execute as them** |
| `sutura-mcp` | The agent-facing surface. Transport only, over the Model Context Protocol: **two tools, one per `Surface` operation**, each schema generated from a wire type and committed as a snapshot. Which tools exist is `sutura_app::Capability`'s to say, not this crate's. Depends on `sutura-app`'s driving port and on nothing in `sutura-http`. **No composition root links it yet** - `serve_stdio` is the entry point and which binary gets it is undecided |
| `sutura-serve` | Composition root for the HTTP surface. Synchronous down to one `block_on`: the engine holds its own runtime |
| `sutura-cli` | The binary; composes adapters |
| `xtask` | The repo gates |
| `-datahub`, `-postgres`, `-clickhouse`, `sutura-arrow` | Planned. None exists |

`cargo check -p sutura-domain --no-default-features` is the inner loop; keep it under a second.
**That width is the reason `just check` now prints what it covered:** cargo's own `Finished` line
says nothing about scope, so a green run read as a green tree and a branch whose `sutura-config` did
not compile was pushed on the strength of it. `cargo xtask check-scope` is what keeps the printed
scope equal to the `-p` flags above it, so the notice cannot drift into a lie the way a comment
would. **What no gate can do is know what a developer believed a task covered** - the honest output
is the fix for that half, and `just check-changed` with no arguments is the cheap answer to "does
what I touched compile".

A data system's driver is a dev-dependency. `sutura-cli` links the engine only, which is what keeps
the musl artifacts building - nixpkgs has no musl `libduckdb`. `nix/duckdb.nix` is the single path
from nixpkgs to that library, imported by `flake.nix` and `devenv.nix` alike.
## Commands

`direnv allow` once per clone. Then:

```bash
just validate       # THE gate. Run this before saying a change is done.
just check          # fast inner loop, DOMAIN CRATE ONLY - and it says so on the way out
just check-changed  # cargo check over what your working tree actually changes
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
their own copy of the tree - the only way to catch a file the build needs and that copy does not
have. Every other command reads the real tree and cannot see that class of bug. The copy is
GIT-DERIVED, and that is the mechanism: an untracked file is invisible to it, so a new module
compiles under `cargo` and then does not exist in the sandbox. `git add -N` is enough to make it
visible. **The source filter is a SECOND way to lose a file, and it now drops things.**
`flake.nix`'s arms match a REPO-RELATIVE path; they used to match the absolute one, and that made
the whole `||` chain short-circuit to true - a nix source root IS `/nix/store/<hash>-source`, so the
arm written for our own `nix/` directory matched every path in the tree and the filter dropped
nothing. What it keeps is the arms: the Rust sources and manifests crane recognises, plus `vendor/`,
`examples/`, `crates/*/tests`, `crates/*/src`, `nix/`, `rust-toolchain.toml` and `docs/crap.md`.
Everything else - `docs/`, `.github/`, `.agents/`, the top-level markdown, `flake.nix` itself - is
now absent from the sandbox, so **the rule `flake.nix` states is live rather than theoretical: any
directory a build or a test reads has to be named there.** **The blast radius is narrower than that
sounds, and worth knowing before believing a green run:** `clippy`, `doctest`, `fmt`,
`packages.xtask` and the release builds read the filtered copy, while `nextest`, `hygiene`, `crap`
and `api-docs` each set `src = ./.` and read the whole tree - which is why the gates that inspect
repo files are unaffected. It does not reach the dependency closure at all: crane synthesises
`sutura-deps` from the manifests, so that derivation does not move whatever the filter does.

Rules:

- Cite a `just` task, never a raw command line. `cargo xtask check-guidance` fails on a citation of
  a task that does not exist, and on a cited `cargo` line missing `--all-features`.
- **Never run a bare `cargo clippy` or `cargo nextest` when a `just` task exists - run the task.**
  `just lint` IS the gate's invocation, defined once: it sources `nix/stable-env.sh` and runs
  `cargo clippy --workspace --all-targets --all-features -- -D warnings`. A hand-written cargo line
  diverges from it in TWO ways, and naming only the first is how this rule gets complied with and still
  fails. **One:** the dev shell's cargo is nightly for the cranelift backend and reports lints stable
  has not got, so a bare run is red on lints CI does not have - `stable-env.sh` is the fix. **Two:**
  the gate adds `-D warnings`, so a bare run makes a `restriction`-category finding a WARNING that a
  grep for `^error` does not see - it passes locally and fails the gate. Both were hit in one session,
  by two different agents, after reading the half of this rule that only named the first.
  `just test` and `just check` stand in the same relation to their gates.
- nix is the only pin for a tool whose version changes what it reports. pixi holds only `prek` and
  `python`. `cargo xtask check-pins` fails if a tool appears in both.
- If tooling is missing, report the exact install command and ask before installing it.
- Hook tiers, commit format and the PR checklist: CONTRIBUTING.md.
- The leak guard is not a hook here. Its pattern list lives in a private repo and runs from there.
- **`just validate` does not render the site, so `just docs` is owed by any change that touches a doc
  comment.** `cargo xtask check-docs` reads the `nav` and the assets; it does not build a page, and no
  nix check does either - the site build lives in the `verify` workflow. So a rustdoc link the
  api-docs generator copies through verbatim can pass `check`, `lint`, `test`, `hygiene`, `api` and
  every nix check, and then fail `mkdocs build --strict`. **Measured rather than reasoned:** a doc
  comment writing ``[`MAX_ROWS`](sutura_domain::plan::MAX_ROWS)`` - a link to another crate's path -
  produced `Doc file 'api/sutura-sql.md' contains an unrecognized relative link` and aborted the
  strict build, while the `crate::`-prefixed links already on four other generated pages did not
  warn. The safe form for a cross-crate reference is plain backticks; **which shapes mkdocs accepts
  was not established**, which is exactly why this is a rule to run the task rather than a gate that
  would have to encode a boundary nobody has measured. It is not in `validate` because that recipe
  would then fail on a clone whose pixi `docs` environment is not installed, and a gate that fails
  for an environment reason gets disabled.
 . - **This shell's environment follows `cargo` into OTHER repositories, and it breaks builds there.**
  `devenv` exports `CARGO_UNSTABLE_CODEGEN_BACKEND=true` and
  `CARGO_PROFILE_DEV_CODEGEN_BACKEND=cranelift` for our own inner loop, plus `DUCKDB_LIB_DIR` and
  `DUCKDB_INCLUDE_DIR` pointing at nix store paths. Nothing scopes those to this directory, so a
  `cargo` invocation in an unrelated checkout inherits all four: it gets built by cranelift, and a
  crate linking C++ gets OUR DuckDB. **Measured, not theorised** - a control build of `duckdb-rs`
  from this shell aborted with `libc++abi: terminating due to uncaught foreign exception` behind
  3.4 million `ld: could not create compact unwind` lines and a 1.7 GB log, because cranelift's
  unwind tables cannot carry an exception across the C++/Rust boundary. The same suite is green with
  the four variables unset. So when working in a checkout outside this repository - upstreaming a
  patch, reproducing a bug against a dependency - **unset them first, and treat a red run from this
  shell as unexplained until you have.** There is deliberately no mechanism for this: a gate here
  cannot see a build somewhere else, which is exactly why it is written down.
## Canonical Sources And Generated Output

One owner per artefact. Nothing here is hand-edited: each is regenerated from its source, and the
regeneration is checked rather than trusted.

| Artefact | Owner | Rule |
| --- | --- | --- |
| Metric definitions and anchors | the catalog | Arrive as `PinnedDefinitions` + `DefinitionVersion` + `DefinitionDigest`. `docs/adr/0001` is the decision |
| MCP tool schemas | a `schemars` derive on a WIRE type in the transport, never on a domain type | **Built, for every tool.** `sutura_mcp::wire::AskArgs` and `DescribeCatalogArgs` derive `JsonSchema`, and `TryFrom<AskArgs> for Query` is the only way to a question; `sutura_mcp::tool::input_schema` is `schema_for!` over one of them chosen by an exhaustive match on `sutura_app::Capability`, and nothing in that crate hand-writes a JSON object. The byte-compare is an `insta` snapshot **per capability**, named after the capability's own identifier, under `crates/sutura-mcp/src/snapshots/` - so a new or widened input is a failing test until somebody re-accepts it, and `the_question_tool_takes_exactly_the_five_fields_a_question_has` fails by NAME rather than by diff on a field called `sql`, `table` or `predicate`. **The derive is on the wire type and not the domain because that is an architecture decision, not a convenience** - a macro crate on a domain type enters `ALLOWED_IN_DOMAIN` and `check-boundaries` walks the whole transitive tree. **A doc comment on a wire type is CALLER-FACING prose**, because `schemars` puts a root doc comment into the schema's `description` and a model reads it; reasoning about the type goes in a plain comment beside it |
| Which tools the surface has, and the scope each needs | `sutura_app::Capability` | One declaration in the crate that owns the driving port, because `Surface`'s two operations *are* the tool set and the two transports cannot see each other. `sutura-mcp` renders it as tool names and `sutura-http` as routes plus `operation_id`s, and each has a `both_transports_describe_the_same_tools` test against that source - **two tests over one source, rather than a comparison between the transports, which an adapter may not make.** `id()` and `scope()` are separate literals so a tool rename cannot rename a scope, and both are pinned by value because an authorization server is configured with them by hand. **What is still kept equal by REVIEW and not by a test:** the field lists of the two wire types that do the same job - `sutura_mcp::wire::AskArgs` against `sutura_http::wire::QuestionBody`, and `CatalogContent` against `CatalogBody` - and the prose, deliberately, because a tool description and an OpenAPI summary are written for different readers |
| The OpenAPI spec | `utoipa` derives on `sutura_http::wire` | Generated from the handlers, never hand-written. There is no `docs/generated/` and no dumping task: the document is built at startup and served, so a missing `#[utoipa::path]` fails to compile rather than producing a page with a gap |
| The executed SQL | `sutura-sql` | Goldens per dialect, regenerated and reviewed as a diff, never typed. The plan's serialized form is pinned too. `differential.rs` runs one plan both ways and compares rows |
| Compiler version, anything shipped | `rust-toolchain.toml` | One pin for CI, the release build and the image. Do not add a second one to any of those |
| Compiler version, local inner loop | `devco/rust-toolchain-nightly.toml` | Two narrow uses. Locally: the cranelift backend. In CI: `checks.api-docs` only, and only to EMIT RUSTDOC JSON - `--output-format json` is unstable and has no stable equivalent. It builds no artifact any gate reads: the `xtask` binary that drives the check is compiled by `rust-toolchain.toml` on the same `ci` closure the other five checks and `packages.xtask` reuse, and nightly is a command prefix around the `cargo rustdoc` child alone. THE REUSE IS CHECKABLE, and was checked rather than asserted: `nix-store -q --references` on each of those seven derivations names ONE `sutura-deps`, and the flake declares nightly as a PACKAGE - `nightlyToolchain` - so there is no second `crane.mkLib` for it to hang a second closure off. **Not "nightly never compiles first-party code":** it compiles the `cargo rustdoc` child's INPUTS, which are ~313 third-party crates PLUS six of our own libs as `rmeta` dependencies of the ten that get documented - `sutura-app`, `sutura-config`, `sutura-domain`, `sutura-runtime`, `sutura-semantic`, `sutura-sql` - all at `--profile ci`. Re-measure with `cargo rustdoc -p <lib> --all-features --profile ci -Z unstable-options --unit-graph`, which lists the units WITHOUT building them: 482 distinct units over the ten crates, ten of which are the `doc` units themselves. None of it is linted, tested, linked, shipped or read by another gate, and the child gets its own `CARGO_TARGET_DIR` so the two channels never share artifacts. Every gate that lints, tests or ships is `rust-toolchain.toml`. `nix/toolchains.nix` is the single code path from either file to a compiler |
| `Cargo.lock`, `devenv.lock`, `pixi.lock` | their own tools | Regenerate, never hand-merge |
| Third-party derived code | `VENDOR.md` - upstream repo, commit, date, local changes | The `cargo-deny` licence gate keeps the obligation from rotting: an exact allowlist, with `unused-allowed-license = "deny"` so an allowed licence nothing uses is itself a failure. **There is NO `NOTICE` check** - this row claimed one, and `NOTICE` appears in `xtask` only in `classify`'s docs-only path list, which checks nothing. "Inspired by" is not a licence position |
| The leak-guard pattern list | a private repo | Deliberately not vendored here; the hook calls it by path and fails closed |

## Dependency Currency

**The target is the newest set of versions that resolve together on the day the work is done - not the
newest of each crate, which is not a coherent set, and not whatever a record happened to name when it
was written.**

A version number in a design record is stale before anybody builds from it. So a record states the
**constraint** - what the code needs to be true of a dependency - and where a number is unavoidable it
carries the date it was checked and an instruction to re-check. `rust-toolchain.toml` is the one place a
version is authoritative rather than indicative, and the *Canonical Sources* table above says so.

### Arrow's major belongs to the engine

`sutura-exec-datafusion` is THE engine. **Its Arrow major is the workspace's Arrow major, and every other
Arrow-consuming dependency conforms to it.** Not the reverse, and not a negotiation per adapter: a data
source is replaceable and the engine is not, so the engine sets the type vocabulary. Downgrading the
engine to match an adapter is not on the table.

### A duplicate and a type boundary are different problems, and the difference decides urgency

Getting this backwards wastes a week in either direction, so it is written down:

| | What it costs | How urgent |
| --- | --- | --- |
| **A duplicate** - two majors present, no first-party code crossing between them | Build time, and supply-chain surface: two copies to patch when one has an advisory | Real, bounded, and not a correctness risk |
| **A type boundary** - first-party code holds a value from one major and hands it to something expecting the other | It does not compile; or, forced across FFI, it is undefined behaviour | Blocking. Nothing ships through it |

**The test for which one you have is not the lock file, it is whether any first-party crate names the
type.** As of 2026-08-28 the workspace holds eleven duplicated Arrow crates at 58.4.0 and 59.2.0 -
`datafusion` pulls 59.2.0, `duckdb` pulls 58.4.0 - and it is a **duplicate, not a boundary**, because
`sutura-exec-duckdb` declares no `arrow` dependency, names no Arrow type, and converts results into a
neutral row type. `differential.rs` compares rows rather than batches for the same reason. Twenty-seven
crates are duplicated in total; the other sixteen are ordinary transitive churn nobody has an opinion
about.

### The mechanism

`deny.toml` sets `multiple-versions = "warn"`, and that stays: denying it needs a skip list of
twenty-seven entries that rots on every `just update`, and a gate that fails on correct code gets
disabled - the same reasoning `allow-wildcard-paths` already carries there.

So the mechanism is narrow and aimed at the case that matters: **`cargo xtask check-arrow` reads
`Cargo.lock` and fails when the Arrow family spans more than one major**, unless every major present is
named in `devco/arrow-majors-allow` with a date and a reason. It is a hygiene gate, so `just validate`
runs it. It checks the whole `arrow-*` family rather than the `arrow` crate alone, because a transitive
dependant can pin `arrow-schema` by itself and that is the same defect. Warn is what let the current split arrive unremarked - the
`deny.toml` comment predicted it, deferred it, and was right - so the gate exists to make the *next* one
arrive in a diff.

### When the newest set cannot be made compatible

**Do not vendor, inline, fork or pin back on your own judgement. Raise it.** In preference order, and
each step is only reached because the one above it failed:

1. **Bump the conforming crate** to a release that already agrees with the engine.
2. **Disable the feature that pulls the conflicting version**, where the code does not use it. Cheapest
   possible outcome and the easiest to miss.
3. **`[patch.crates-io]` on the DEPENDENT crate - never on the shared dependency.** This one is written
   out because the obvious form of it does not work and the failure is silent. Patching the shared
   dependency cannot widen a requirement: `[patch]` replaces a SOURCE, so against disjoint requirements
   like `^58` and `^59` cargo reports `patch ... was not used in the crate graph` as a warning and keeps
   the old version. *Verified against this workspace.* What works is patching the crate that declares
   the stale requirement, with its own manifest line changed - also one line, also no fork, and it must
   be proved by a build rather than assumed. For the Arrow case that was proved: a single `arrow 59.2.0`,
   a clean compile, and the patched crate's own suite green, identical to the unpatched control.
   **A step 1 turned out to be available for this case and was taken, which is why the option order
   above is not decoration:** `duckdb-rs` had simply not bumped, so the fix went upstream as a
   one-line manifest change rather than living here as a patch. Measured on 2026-08-29 against
   `duckdb-rs` at `199547d`, the same stable toolchain on both legs and the `bundled modern-full
   vscalar vscalar-arrow vtab-full` feature set: 469 lib tests passed and 0 failed on **both** Arrow
   58.4.0 and 59.2.0, with `libduckdb-sys` at 12 and 0 on both, and **zero source changes** - Arrow
   59's breaking changes do not reach that crate. An earlier version of this paragraph recorded
   *"288 passed"* from a narrower feature set; the number is dropped rather than corrected in place,
   because a bare count with no feature set and no date attached is not reproducible and this file's
   own *Dependency Currency* rule says a figure carries the day it was checked.
4. **Vendor**, last, and never silently: `VENDOR.md` takes upstream repo, licence, commit, date and
   local changes, the `cargo-deny` licence gate applies, and *"inspired by" is not a licence position*.
   The real cost is not the patch - it is that **a vendored copy makes us the security response for it**,
   permanently and invisibly after the first commit.

A discussion raised at step 4 states: which crates conflict; whether it is a duplicate or a type
boundary; what each of the four options costs; and what breaks if nothing is done. An escalation without
those four is a question rather than a decision.

### A version requirement that encodes something else needs a tilde, not a caret

A caret requirement is the right default and there is one shape where it is actively wrong: a crate whose
version encodes the version of a **native library it expects to find**. `duckdb`'s second semver component
is the DuckDB C release, so `1.10505.0` means C library 1.5.5 - which is what `nix/duckdb.nix` supplies -
and the crate's own README recommends a tilde for exactly this reason.

Under a caret, `just update` may move the crate to a release expecting a newer native library than the
sandbox provides. **It links and then fails at runtime on a missing symbol**, which is the worst shape of
failure available: the build is green and the fault appears when the code runs. So a dependency that
carries a native-library expectation in its version pins with `~`, and the reason goes next to the pin
rather than in a commit message.

## Invariants

Enforced by a type, a lint, a hook or a gate - never by recall. Changing one is an architecture
decision. A row that loses its mechanism gets deleted, not demoted to advice.

| Invariant | Enforced by |
| --- | --- |
| No SQL, table name, filter expression or row-id list on the tool surface | `Query` declares no such field, and `deny_unknown_fields` makes an attempt an error naming it. **Unqualified, because the surface that exists is the whole surface.** A previous version of this row narrowed it to the *certified* surface and cited a raw tool's result type - and `docs/adr/0013` decides such a tool while saying plainly that nothing of it is built, so the row was describing an unwritten type and pre-weakening a live mechanism for it. Which is this table's own deletion rule pointed the other way: a row does not get *widened* by an unbuilt feature either. When a raw tool is built, the diff that builds it is the one that re-scopes this row, with the new outcome type in front of a reviewer |
| Refusal is a result, not an error | `ToolOutcome::Refusal`; a golden provokes every variant a question can reach |
| A question's time range is bounded, and bounded to a size | `TimeRange` has no unbounded form; `resolve` refuses a span over `MAX_RANGE_DAYS` (3653) as `TimeRangeTooLong`. Goldens stand one day either side |
| A catalog edit cannot change what executes | Definitions arrive as `PinnedDefinitions` with a digest over their canonical form; the digest travels with the answer |
| A result cannot be separated from what defined it | `PinnedDefinitions::pin` computes the digest from the definitions it stores - no digest parameter, no hasher parameter. `ToolOutcome::Answer` carries `Provenance` with no constructor that omits it |
| An unvalidated bundle is never served | `sutura_app::verify_and_validate` is the only constructor of `Validated`, lives in a private module, and takes the `Warehouses` registry. Two `compile_fail` doctests, each with a compiling twin. The boot path now goes through `Warehouse::verify_anchor` rather than `execute` - a required method that returns `AnchorRows` and **takes no credential**, so the request path's `&Presented` cannot reach it and there is no parameter a caller's credential could arrive in. **What keeps that method to the boot path is a LINT, and its input type is a self-check rather than a barrier - which is a SECOND review's correction, after the first one overstated it in three documents.** The first correction gave it an `AnchorPlan` instead of a bare `QueryPlan` and this row then said the method could no longer be handed a question. A reviewer disproved that in one function: `AnchorPlan::of` is `pub`, `Anchor::new`, `QueryPlan::new` and the name and range types are all `pub`, so a fabricated tuple passed all four guards and `verify_anchor` returned rows. **A fifth guard is not the answer** - a shape check over caller-constructible values can only ever be a shape check, and Rust has no cross-crate friend visibility to hide the constructor behind, so a constructor `sutura-app` can call is one the workspace can call. So the two mechanisms are named apart. **The one that holds:** `clippy.toml` bans `sutura_domain::warehouse::Warehouse::verify_anchor`, VERIFIED to resolve by writing the call and watching clippy reject it - it fired on a call through a concrete type's own impl - and `sutura_app::verify_anchors` holds the single `#[expect]`, so a second call site is an error under `-D warnings` until somebody writes a second expectation a reviewer sees in the diff. Its limit is that a lint is not a type: it reaches this workspace and an `#[allow]` walks past it. **The one that is a self-check:** `AnchorPlan::of` takes the `PinnedDefinitions` and the metric name, and reads off the bundle everything it compares - that the metric is defined, that it declares an anchor, the range that anchor certifies, and the COARSEST grain it declares - so a caller no longer supplies the anchor it will be checked against, and a boot path that compiled the wrong question is caught. The grain check closed a fifth gap the same review found: nothing read the grain, so a `Day`-grain plan over the anchor's range passed and came back as a series rather than the one certified number. **Narrower than "the certified number is the number a caller sees", and the limit is worth stating before a network source arrives:** an anchor certifies whatever identity the adapter was configured with - the process, for the file engine that ships. `docs/adr/0008` part 1 wants that method to take a DECLARED `VerificationIdentity`, and it does not: `VerificationIdentity::parse` is `pub`, so the `compile_fail` test that record named could not have held, and what is built is the property it wanted reached by removing the argument instead. The *declaration* and the *boot refusal* exist - see the row below - and nothing passes the declaration to the port |
| A bundle with an anchor on a source that has no identity to re-run it under does not boot | `SourceIdentity::anchors_run_as` is an exhaustive match returning a three-variant `AnchorIdentity` whose third variant is `NoneDeclared` rather than an `Option`, so a reader names the case instead of deciding what an absence permits; `sutura-serve`'s `refuse_unverifiable_anchors` walks `anchored_metrics` and refuses, naming the metric AND the source. On a `SharedServiceUser` source the verification identity IS the shared identity, so nothing is configured and the anchor is a complete claim. **Two limits:** the check reads a DECLARATION, and nothing yet passes it to the port - so it refuses a deployment that could not have verified honestly rather than proving which identity ran; and the arm is reachable on this build only because the check runs before the capability cross-check, since the one linked adapter cannot impersonate at all |
| A source is served under a declared identity, or the deployment does not boot | `SourcePosture` has **no `Default`** - unlike `TlsTermination`, because no bind address makes "one identity for every caller" safe to assume - and its `SharedServiceUser` variant carries a `SharedIdentityDeclared` whose only constructor takes a parsed `AcknowledgementReason`, which has no `Deserialize`: a file cannot produce the witness, so the posture cannot be reached by leaving a key out. `Settings::refusals` returns `SharedSourceNotAcknowledged` per source in multi-user mode and `DeploymentIdentityUndeclared` when any source is configured and `security.identity` is absent; `sutura-serve` refuses a catalog source with no `sources.<alias>` entry. `DeploymentIdentity` is DECLARED and never derived - "every source shared means single-user" exempts a multi-tenant deployment whose sources are all shared from the one check it most needs. **The limit:** the constructors are `pub`, so what is closed is the path from a *file*, not from another crate |
| Which adapter opens a source is decided by a declaration, not by its name | `sutura_config::SourceKind` is a closed set whose vocabulary IS the vocabulary of adapters - one variant, `files` - so an unknown word is a parse refusal listing what is available, and `sutura-serve`'s `build_engine` matches on it exhaustively with no wildcard arm, which makes a second kind a compile error at that line. **This REPLACED a control rather than adding one, and the replaced one was wrong in both directions:** the root used to refuse any source not literally named `local`, which made a second source unopenable on any build and refused a legitimate deployment whose files-backed source is called something else. The refusal it looked like it was making - "this build has no adapter for that" - now lives where the vocabulary does. **The limit, stated because one variant makes the match look decorative:** nothing today can provoke a wrong-kind arm in the root, so what holds is the compiler and not a test, and `sutura-cli` reads no registry at all |
| A posture the linked adapter cannot deliver does not boot | `Warehouse::IMPERSONATION` is a **required associated constant** with no default, so an adapter cannot omit it - a `compile_fail` doctest with a compiling twin, differing by that one line. `SourcePosture::deliverable_by` is two exhaustive matches with no wildcard arm, and `sutura-serve` calls it per source beside `open_engine`. It is in the composition root and not `sutura-config` because whether the linked code can carry a per-subject credential is a property of the BUILD. **The cost, recorded where the constant is:** an associated const makes the port not object-safe, so a heterogeneous adapter set wants a closed enum rather than `dyn` - an architecture decision, not a signature tweak |
| No leg of an ANSWER reaches a data system without a credential somebody minted for it | `Warehouse::execute` takes a `&Presented` and has **no default**, so there is no signature that runs as whatever the process is - a `compile_fail` doctest with a compiling twin, differing by that one argument. `dry_run` takes it too and returns `PreFlight`, whose default is `NotAsked`, so an adapter that did not ask cannot be read as having accepted. The credential comes from `identity::CredentialBroker::mint`, called **once per answer for every source the plan reads**, and `LegCredentials` hoists the asker and the deadline out of the legs: one `asked_by`, one `not_after`, a private `by_source` with no `insert`, and a constructor that refuses a set not covering the sources it was minted for. `Presented` has three variants and the third carries no credential material at all, so the shared posture is a shape a reader sees rather than a value an adapter ignores. **And each adapter compares the leg against its own declared posture and not only against its own capability**, through `Presented::agrees_with` - one exhaustive match over the pair - so a shared leg carrying a *different* operator acknowledgement is a typed `Err` instead of a leg that executes and is then reported under the adapter's declaration. The two values are independent by construction: a broker reads the settings tree and an adapter holds what the composition root handed it. **The limit, because the witness is prose:** equality is the comparison available, so a fabricated witness whose text is byte-for-byte this source's is indistinguishable from it. Added by a review, which found the shape being checked and the agreement not. **Three limits, and the first is the one to read:** no adapter in this build can carry a per-subject credential - both declare `NoPlaceForASubject` and return a typed `Err` for the two subject shapes - so what a broker can mint here is the deployment's own identity, and *no source executes as the asking subject yet*. The `Expiry` the set carries is read by nothing: the domain has no clock, and `docs/adr/0008` part 6 puts the floor in the broker adapter. And the port takes the `RequestContext` and **not** the caller's own assertion, because `docs/adr/0014` Decision 3's exchange is blocked on a verification nobody has done - a `Secret` field now would be the guess this port was delayed to avoid. **The row says ANSWER for a reason:** the BOOT path reaches a data system through `Warehouse::verify_anchor`, which takes no credential and runs under whatever identity the adapter was configured with. What keeps that to ONE call site is the `clippy.toml` ban on it rather than its input type - see the unvalidated-bundle row, which says at length why |
| A subject with no credential at a source is refused, not answered as the process | `RefusalReason::CredentialUnavailable`, `403 credential_unavailable` on HTTP and its own sentence on the agent surface, from the exhaustive match each transport holds. Provoked by the REAL implementor from a real configuration rather than by a fake: `sutura_config::StaticCredentialBroker` holds an entry only for a source declared `shared-service-user`, so a source declared `impersonation-at-source` gets nothing and the question is refused. **It amends `docs/adr/0005`** - "the 403s are not a statement about a credential" stops being true - so a test asserts the sentence does not send a caller back to authenticate against this service, because the missing grant is at the data system. A broker that could not be reached is **not** this: it is `SurfaceFailure::Broker` and `503 identity_unavailable`, which shares its status with a dead data system and not its code. **The limit, and it is the one to state:** on the SHIPPED serve binary this refusal is currently unreachable end to end, because the only configuration the shipped broker refuses - a source declared `impersonation-at-source` - is one the composition root will not boot against an adapter that declares `NoPlaceForASubject`. What provokes it is the broker asked directly, and `answer` asked with a broker that refuses. It becomes reachable end to end with the first adapter that CAN impersonate, which is also the first deployment where it matters |
| An answer says which identity produced each of its legs | `Provenance` carries an `ExecutedAs`, `Provenance::new` is private, and `PinnedDefinitions::provenance` **requires** the record as an argument - so an answer cannot be produced without saying what ran. `ExecutedAs` has no empty form and no `remove`, so it cannot claim nothing executed, and `and` refuses a second leg for a source already recorded. The value is read off `Warehouse::posture` - the adapter the plan selected - and never off the settings tree, which is what stops a leg being reported as impersonated on the strength of a file. Both transports render it, and neither sends the operator's acknowledgement prose. **Recording is not a control**, and the field's own documentation says so: it reaches a caller after the rows did. **And every record has exactly one entry today**, which is worth stating now that `LegPlan` exists: `ExecutedAs::and` is the second-leg door and nothing calls it, because nothing constructs a leg - so what is pinned is the SHAPE a federated record needs, not a federated record. *Built And Not Wired* owns the other half |
| We never TRANSLATE SQL, and the one thing we parse is parsed at load | The dialect layer's `transpile` feature is not compiled, so a call to it does not build. NOT a lint for that: an unresolvable path in `disallowed-methods` is silently ignored by clippy AT THE PINNED VERSION, verified, so such an entry would read as enforcement and do nothing. Measured boundary: the clippy that ships with the PINNED toolchain ignores it silently, while a later one warns `does not refer to a reachable function` and offers an `allow-invalid` opt-out - so writing the call is still the only check that holds, and it stops being necessary when the pin moves past that. A verified claim with no version attached is one that quietly expires. The single exception to "we do not parse" is `sutura_sql::expression`, and that module has no caller - see *Built And Not Wired* below, which is why it is not a row here. `transpile` stays uncompiled because its default `unsupported_level: Warn` returns `Ok(sql)` and discards the diagnostic, and `Raise` errors on every non-count aggregate targeting ClickHouse while staying silent on the four real breakages |
| No value from a question reaches the statement as text | Every value becomes a bind parameter; `GeneratedQuery` keeps statement and parameters in separate fields with no merging constructor. Two goldens per dialect assert it - the question corpus and the leg shapes - through one pair of helpers in `golden/shared.rs`, so the two axes cannot hold two opinions about what the claim is. **The POSITIVE half is what the claim now rests on:** the statement names exactly one bind parameter per value the plan carries, counted per dialect through `PlaceholderStyle` and compared against the PLAN's parameter list, so a value written into the statement leaves a placeholder missing whatever it spells. The negative half is two searches - substring with the quoted identifiers stripped, and word-bounded on the raw statement. **The second search exists because there was a hole here and it was measured, not reasoned about:** the strip drops every double-quoted span, forced identifier quoting means an inlined value renders as one, and `parse_identifier` accepts `[A-Za-z_][A-Za-z0-9_]*` - so a mutation rendering `"fct_subscription_monthly"."status" = "active"` in all three dialects passed both goldens. **The limit that remains:** a value that word-boundedly EQUALS a name the statement quotes is invisible to both searches - `month` is a declared column in the corpus and a legal filter value - and only the placeholder count reaches that case |
| No identifier reaches the statement unquoted | Forced quoting for identifiers and aliases; a golden asserts it over the corpus with quoted spans stripped first |
| Every generated statement is well formed SQL, and parses under its target dialect | The golden suite parses each statement with the dialect it was generated for. **Narrower than "the data system accepts it":** the dialect layer's parser is not gated per dialect for every construct. Acceptance is vouched for by the anchors and by `differential.rs`, which runs a real DuckDB. Postgres and ClickHouse are rendered and parse-checked, nothing more |
| Adding a metadata provider or a data system is a registration, not a test edit | The golden corpus, the refusal corpus and the anchor check are a matrix over `tests/adapters`. One registry entry adds a catalog or a data system, and `adapters::posture` is where the declaration each one is opened with lives - so an adapter that CAN impersonate grows that function by a value rather than editing a test. **WHICH test a catalog registration selects is now part of this**, and `docs/adr/0016` is where it is decided: `agrees_with_the_oracle` is the GOLDEN adapters' contract and is not weakened for anything, while a *declaring* adapter - one that supplies part of the model - is measured against its own `SemanticCatalog::capabilities`. Both are expanded over every registration today, because the one registered catalog is golden AND declares everything; the diff that registers the first narrow adapter is the one that splits the arm, with its declaration in front of a reviewer |
| The measure vocabulary is closed, and holds no SQL expression | `Measure` is two shapes over a `Term` of two terms, `RequiredFilter` four operators, `deny_unknown_fields` at every depth. There is no `expression:` field and no `Option<String>` anywhere on it. A new shape is a domain variant plus a plan variant plus a generator arm plus a golden |
| The panicking fragment API cannot be called | `clippy.toml` bans `polyglot_sql::parser::Parser::new` and `::parse_expressions`, both **verified to resolve** by writing the call and watching clippy reject it. They panic on an empty token list, which is what empty, whitespace-only and comment-only input tokenize to - an abort reachable from a catalog file |
| A definitional filter is always applied | `required_filters` compile into every plan for the metric, marked `PredicateOrigin::Definition`. A caller has no field that could name or remove one |
| A join cannot silently change a measure | `Definitions::assemble` refuses a dimension reached through a relationship whose *declared* cardinality may duplicate rows, and a reconciliation test checks grouped rows against the ungrouped total. **Catalog cardinality is a trusted precondition:** nothing checks the declaration against the data, and an anchor cannot see it because an anchor is asked with no dimensions |
| A result that hit the row cap is refused, not truncated | `row_limit()` is `max_rows + 1`, so a result at the cap is distinguishable from one cut off by it; `answer()` returns `ResultTooLarge`. Both legs pinned: 63 SQL goldens read `LIMIT 10001`, and the engine leg asserts the fetch on its logical plan. `check-guidance` counts the goldens and fails if that number drifts |
| The engine's operators run against a bounded memory pool, and exhaustion is a refusal rather than process death | Both `SessionContext` construction sites in `sutura-exec-datafusion` take `new_with_config_rt` with a `RuntimeEnvBuilder` carrying a `GreedyMemoryPool` sized from `runtime.working_set_max_bytes`, and neither constructor has a signature that lets a caller omit it. A refused reservation leaves as `RefusalReason::ResourcesExhausted` through `Warehouse::working_set_exhausted`, whose exhaustive match has no wildcard arm. `WorkingSetCeiling::parse` refuses a zero and refuses a value above the memory the process can reach at boot. `pool/ceiling_tests.rs` shows the bound biting on a real grouped aggregate. **The limit, and it is not small:** the pool counts operator reservations only - not what a driver buffers, not `collect()` materialising batches, not the row set built during conversion - so a question large enough to end the process on one of those paths still ends it, and `docs/adr/0009` puts the bound that reaches them with the execution boundary. On a platform that will not report available memory - macOS - no boot check is made, and `WorkingSetCeiling::checked_against` is what the banner reads to say so |
| Nothing spills the asking subject's rows to local disk | `DiskManagerMode::Disabled` on every session this adapter builds, so a spilling operator answering a refused reservation has nowhere to write. `docs/adr/0009` Decision 3 decides fail-immediately over spill, and the reason is data-at-rest rather than performance |
| Two result columns cannot share a label | `Definitions::assemble` refuses a dimension named after the time bucket or after its own metric |
| A catalog document's fields are exactly what it declares | `deny_unknown_fields` on every on-disk shape |
| A plan cannot silently span two sources | The plan stage collects sources into a `BTreeSet` and refuses `PlanSpansTwoSources` unless exactly one is in it. **A CATALOG spanning two sources is no longer refused at boot, and that is a deliberate narrowing:** `sutura-serve` opens one adapter per source the catalog names, so two declared sources are a servable deployment and only a QUESTION that would span both is refused. `sutura-cli` still refuses a multi-source catalog, because it takes one data directory on the command line and reads no source registry |
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
| Knowledge read from the catalog is descriptive only | The prompt is its only consumer. `Query` carries a `MetricName` and has no field a phrase fits in, `knowledge::Referent` can name only a metric, a dimension of one or a declared value of one, and `load()` has no `RequestContext`. A second consumer is an architecture decision, not a feature |
| No server-side phrase resolution | There is nothing to resolve into: the glossary renders into the agent-facing prompt and the AGENT states which metric it chose, in its own transcript. `RefusalReason` has no `PhraseNotDefined`, and a variant no test can provoke is what that enum refuses to carry |
| A metadata adapter cannot be silent about a kind it cannot supply | `SemanticCatalog::capabilities` is a **required associated item with no default**, returning a `MetadataCapabilities` - the definition side's nine kinds plus the four knowledge capabilities that already had a vocabulary - so an adapter that omits it does not compile: a `compile_fail` doctest with a compiling twin, differing by that one item. `Warehouse::IMPERSONATION` is the shape it copies and the rule it copies is that trait's own: **required with no default where the absence changes what a caller may believe, defaulted with a stated reason where it is a missed optimisation.** An associated FUNCTION taking no `self` rather than a constant, and the doc says why: the set reuses `KnowledgeCapabilities`, a `BTreeSet` newtype no `const` expression can build, and the const-friendly alternative is a boolean per kind that `crate::knowledge` already argues against - taking no `self` buys the property the constness bought, that the declaration cannot vary per instance. The walk is guarded the way the knowledge one is: `DefinitionKind::next` plus a `const` assertion on the discriminant, with `capabilities::carried` and `capabilities::recorded` two more exhaustive matches, so a tenth kind does not compile until somebody says how it is observed. **The limit, and it is the whole of what this row does NOT claim: nothing refuses a LOAD whose content disagrees with the declaration.** The declaration is a property of the code rather than of the bundle, so it is not under the definition digest and no composition root reads it. What holds it honest is `MetadataCapabilities::checked_against` - the two directions `docs/adr/0016` decision 6 specifies - run by `crates/sutura-app/tests/golden/catalogs.rs` over every registered catalog and over the suite's own two narrow fakes. That is a test, which is why this row claims the declaration and not the fidelity |
| Content for a knowledge capability nobody declared fails the load | `Knowledge::assemble` walks `Capability::every()` and refuses `UndeclaredContent`. The walk cannot be walked past: `Capability::next` and `Capability::previous` are two exhaustive matches, a `const` assertion holds `every()`'s seed, and `prompt::knowledge::claim` is a third match that decides what a capability licenses the document to say |
| A note is attached to something the bundle declares | `CaveatAboutNothing` refuses an unscoped caveat, and every other kind carries a `Referent`, a `Phrase` checked against the definitions, or a `Query`. There is no `rules` kind, and adding one would be an unscoped text channel from the catalog into the prompt's preamble |
| Note prose is bounded, and refused at load rather than cut at render | `NoteBody::parse` caps bytes and lines and refuses a body that would render as nothing; `MAX_KNOWLEDGE_BYTES` caps the aggregate, so N conforming notes cannot do what one oversized note cannot; `Phrase::parse` bounds one line and normalises it - invisible code points removed, every run of whitespace one space, trimmed, and case folded for the identity only - so two phrases differing in *those* cannot both load. **Not a Unicode-normalisation claim:** there is no NFC/NFD anywhere in the workspace, so a decomposed spelling and a Cyrillic homoglyph are each a second phrase. Nothing anywhere shortens a body |
| A worked example is a question this surface would accept | `Knowledge::assemble` checks each example's `Query` against the metric, its grains, its dimensions and its allowlists, and against `MAX_RANGE_DAYS` and `MAX_DIMENSIONS` read from `sutura_domain::query`. The prompt tells an agent an example is a question this deployment answers, and a bundle carrying one it would decline does not load |
| The knowledge declaration is under the definition digest | `PinnedDefinitions::pin` hashes the `Knowledge` alongside the definitions, the `KnowledgeCapabilities` included. A deployment that quietly stopped declaring `not_defined` has changed what its prompt claims, and provenance that did not move would certify the old claim |
| Work handed to the blocking pool carries the request's span | `clippy.toml` bans `tokio::task::spawn_blocking`, VERIFIED to resolve by writing the call and watching clippy reject it. `sutura_runtime::spawn_carrying_span` is the one caller, holding the single expectation, and `crates/sutura-runtime/tests/blocking_span.rs` asserts the span survives the thread boundary - an integration test rather than a unit one, because a pool thread reads the GLOBAL dispatcher and a thread-scoped subscriber cannot see it |
| No caller-supplied text reaches the log unbounded or unvalidated | `CorrelationId::parse` bounds the length and restricts the character set, and mints a fresh id rather than erroring, so a malformed header cannot fail an answerable question. Its refusal carries a position and never the value. **The limit:** this is the one inbound field read into a log today; nothing generalises it to a field added later |
| A call attributed to a subject can be told apart from one attributed to an agent acting for them | `identity::PrincipalChain` is subject, then actors, then task - ordered - and `attribution()` returning a two-variant `Attribution` is the ONLY way to the actors, so a reader names the case rather than reading past an absence. `ActorChain` has no empty state: it holds its innermost link in a field of its own, so `ActingFor` cannot be a claim about nobody and `immediate()` needs no `unwrap`. `Subject` is two variants with `established()` as an exhaustive match, so today's `TheDeploymentItself` cannot be confused with a verified subject that happens to be *named* like one. **Both tail positions are always absent today** - nothing establishes a caller identity and nothing names a task - which is exactly why the shape is in now: the distinction cannot be backfilled into records already written |
| A caller cannot state its own identity | No type in `identity::principal` implements `Deserialize` or `Serialize`, so there is no code that could turn a request body into a chain - a `compile_fail` doctest on `PrincipalChain` with a compiling twin, and the failure was checked to be the missing `Deserialize` rather than a typo. `deny_unknown_fields` makes a body naming a `subject` a parse error that names the field. **The transport half of this row moved from an ARITY to a TYPE, and it is not weaker.** It used to read "`sutura_http::principal::established()` takes no argument, so the transport has no parameter a request could reach", and leg 1 cannot keep that shape and work - a verified identity IS read out of a request. `established()` still takes no argument; the verified path takes a `sutura_http::inbound::VerifiedCaller`, whose ONE constructor is `pub(crate)`, is called from exactly one place after a signature check against a pinned asymmetric algorithm plus an issuer, an expiry and this deployment's own audience, and which implements no `Deserialize` - a second `compile_fail` doctest with a compiling twin, and that failure was checked to be the missing impl too. The question a reviewer asks is no longer "can a request reach this parameter" but "can a request produce this type" |
| A caller's identity, where one is established, comes from a signature and never from a header | `sutura_config::InboundIdentity` is a closed enum with no default: `security.inbound` with no `mode` is `SettingsError::InboundModeUndeclared` and the process does not start, because defaulting either way is wrong in opposite directions. Its `BehindGateway` variant carries a `TransitProof` whose fields are an issuer, an audience, a key set, a pinned algorithm, a required token class and a lifetime ceiling - there is **no field for the name of a header holding a username**, and a test presents such a header and gets nobody. Algorithm confusion is unrepresentable in two places rather than checked: `SigningAlgorithm` has no `None` and no `HS*` variant, and `sutura_http::inbound::keys` refuses an `oct` key in a key set. The audience is compared against this deployment's own resource identifier with `aud` in `required_spec_claims`, so a token carrying none is refused rather than passing a check with nothing to compare. **Four limits, each stated where the claim is:** keys come from a file and there is no JWKS endpoint; the two metadata documents `docs/adr/0014` describes are not served; `Scopes` decides which OPERATIONS a caller may invoke and decides nothing about which rows an answer contains, because leg 2 does not exist; and a gateway assertion's *replay window* is bounded while nothing binds one to a request. `docs/adr/0014`'s *What is built* section is the authority on all of it |
| A signature, an issuer and an audience do not decide a token's CLASS, so the class is decided separately | `sutura_config::RequiredTokenType` is two variants and no `Option`, checked in `sutura_http::inbound::token::TokenValidator::verify` on `decoded.header` - **after** the signature, so it is a rule about a document the issuer signed rather than about an unauthenticated header. RFC 9068's `at+jwt` is the `direct` default and `behind-gateway` makes the key required; `any` is the written opt-out and `banner::announce_token_class` prints it at `WARN` on its own line. A token with **no** `typ` is refused, so the check is not satisfiable by omission, and one parser folds both sides so RFC 7515's three spellings of one media type compare equal. **This row exists because review found it missing:** an OIDC ID token has the same issuer and, wherever the resource identifier is also a client id, the same audience - and it verified. A regression test presents exactly that token and expects `TokenRejected::WrongTokenType` |
| A key removed from the key set stops verifying within a bound a caller cannot influence | `sutura_http::inbound::keys::KeySetCache` has two triggers, and the second is the one that answers revocation: an unknown key id refetches at most once per `MIN_REFETCH_INTERVAL`, and *age* re-reads once per `MAX_KEY_SET_AGE` - from a timer armed by the composition root **and** from `key_for` itself, so forgetting the timer does not leave a serving deployment stale. **The caller-driven refetch provably cannot bound revocation**, which is why both exist: a caller presenting a revoked key presents a `kid` the cache holds, so nothing triggers. A candidate that will not parse, or that holds no key of the pinned family, is `Refreshed::Rejected` and the previous keys keep verifying. `KeySet::parse` also refuses two keys under one `kid`, because otherwise which one verifies is decided by their order in the document |
| A gateway assertion's replay window is this deployment's number, not the component's | `sutura_config::ProofLifetime` caps `exp - iat` at `security.inbound.transit_max_lifetime_seconds`, bounded to 1..3600, and `iat` is **required** in `behind-gateway` - checked by `TokenValidator::within_the_lifetime_ceiling` rather than by the library, whose `required_spec_claims` honours only `exp`, `nbf`, `aud`, `iss` and `sub`. An `iat` dated forward past the leeway is refused too, or a component could buy a longer window by dating forward. **The limit is the row's other half and is not small:** nothing binds an assertion to a request and nothing records what has been seen, so inside the window an intercepted assertion replays - a regression test asserts the replay rather than pretending otherwise. `docs/adr/0014` downgrades its own "proof that the request transited" wording to a *gateway-issued identity assertion* for exactly that reason, and names the hop from the component as a trusted transport boundary |
| A deployment that declares an inbound identity cannot serve without one | `sutura_http::router::assemble` returns `RouterNotBuilt::InboundIdentityNotAttached` when the settings declare a mode and the state carries no gate. The gate is built by the composition root because building it READS THE KEY SET, so it can fail - and an unreadable key set has to stop the process rather than become a deployment that answers `401` to everybody while its startup log says it establishes a caller identity. `sutura-serve` reads it before the listener opens |
| A forged key id cannot turn every request into an outbound call, **whatever the concurrency** | `sutura_http::inbound::keys::KeySetCache::reserve` compares the window and stamps the attempt in **one** write-lock acquisition, and it is the only place either window is compared - so both triggers and the timer pass through one gate and exactly one caller can look per window. The source read stays outside the lock. `key_for`'s two comparisons are a fast path that decides nothing. Measured from the last **attempt** rather than the last success, so a source that is down is limited too. Two tests, and they cover different cases: a sequential one spends ten forged ids for one read, and a concurrent one puts two callers through a `tokio::sync::Barrier` so both are provably past the fast path before either proceeds, then asserts exactly one read and exactly one `Refreshed::NotDue`. **This row's wording is unchanged and the code was wrong against it:** the check used to sit outside the lock, and review measured three reads where two were required. A bound three places state and concurrency breaks is a defect in the code, not in the claim. **The limit:** the only source that ships reads a local file, so what the bound protects today is this process rather than an authorization server |
| Two credentials cannot be configured to arrive in one header | `NotFitToServe::DeploymentTokenSharesTheHeader` refuses `security.access_token` together with `security.inbound.mode: direct`, both of which are read from `authorization: Bearer`. Asked of the derived `TokenRequirement` rather than of the enum variant, so a mode added later that also lands there cannot slip past it. The consequence is a row of its own: the production access-token requirement is satisfied by either credential, because a validated audience-bound token per caller is strictly more than one shared secret every caller holds |
| An operation a caller was not granted is neither advertised nor answered | `sutura_app::Capability` is a closed enum whose `next`, `id` and `scope` are three exhaustive matches, so a third operation does not compile until each has been answered; `sutura_app::Permitted` is the one derivation from a token's scopes to what a caller may do, and `advertised()` and `includes()` read the same field so the two cannot disagree. **On HTTP:** `sutura_http::capability::require_capability` is a LAYER over the versioned subtree, so no handler can forget it, and `RouterNotBuilt::RouteNotGoverned` refuses to assemble a router whose generated document holds a route the table names no capability for - checked over all eight methods `utoipa`'s `PathItem` can carry, and shown non-vacuous by a test that hands it a route nobody mapped. A refused operation is `403 insufficient_scope` naming the scope, which is RFC 6750's own shape. **On MCP:** `AgentSurface::new` requires a `Permitted`, `tools/list` filters and `tools/call` refuses. **Three limits, and the first is the one to read:** what a scope gates is which OPERATIONS a caller may invoke and never which rows an answer contains, because leg 2 does not exist and no source executes as the asking subject - so *authorization* is the right word for the surface and the wrong word for the data. Filtering the advertisement is PRESENTATION and the refusal at invocation is the control, which is why both are built and tested separately. And **nothing narrows the agent surface today**: `serve_stdio` passes every capability, because a pipe has no header a token could arrive in - the parameter exists so that decision arrives as a composition change |
| Every outcome, answer and refusal alike, is recorded before it is returned | `audit::AuditSink` takes a `CallRecord` and returns nothing a caller can branch on; `CallRecord::of` derives the outcome half from the `ToolOutcome`, so no call site can describe an answer as a refusal, and `CallRecord::executed_as` derives the per-leg identity from the same outcome - `None` on a refusal, because nothing executed. **`CallRecord::executed_until` carries the credential's deadline**, which `docs/adr/0008` fixes as part of a record's content and which had nowhere to travel until `sutura_app::Answered` existed: it rides beside the outcome rather than on the caller-facing `Provenance`, because how long this deployment's credential for a data system is good for is not the asker's business. `None` means nothing was minted, which is a question declined before the broker was asked. `TracingAuditSink` writes it as its own field, which is what lets a record answer, afterwards, whether a verified subject's answer was filtered by that subject's access or by the identity this deployment holds for the source. `LocalService::answer` writes through the sink before its `Ok`, and `LocalService::start` **requires** a sink, so a service with none does not exist. **Two limits, both deliberate:** sutura retains nothing - what a record is worth is what the deployment's sink is worth, and nothing here can tell it otherwise - and a `SurfaceFailure` is not an outcome, so an `Err` writes no record and is logged by the transport instead |

### Built And Not Wired

**Nothing in this section is an invariant, and none of it may be cited as one.** It is here because
the code it describes exists, is tested, and has no caller from any binary - and because three rows
of the table above used to state it as enforced. Those rows were **deleted rather than moved**, which
is the rule at the head of that table applied to itself: a row that loses its mechanism gets deleted,
and a row that never had one is the same case. What is below is a description of unbuilt wiring, in a
section a reader cannot mistake for the table.

`sutura_domain::expression` and `sutura_sql::expression` are the catalog-authored SQL hatch that
[`docs/adr/0004`](docs/adr/0004-a-named-escape-hatch-for-authored-sql.md) decides. Both are complete
and neither is reachable: `catalog::Metric` holds a `Measure` and not a `Computation`, `MetricDoc` has
no `authored_sql` key, and `sutura_sql::expression::compile` has no production caller - it cannot have
one today, because `sutura-catalog-local` does not depend on `sutura-sql`, and `sutura-serve` links no
SQL generator at all.

Every gate passes over it, and that is the lesson worth carrying rather than the feature: `unused-deps`
and `check-boundaries` read manifests, `max-lines` reads files, `check-guidance` reads prose, and the
missing thing here is a **call** - which is the same shape as a `disallowed-methods` entry that reads
as enforcement while resolving to nothing.

**The federation half of this section is the same shape with one difference worth naming.** The leg
plan types, their rendering and their per-dialect goldens are here for the reason the hatch is - no
production caller - but the missing call is not merely absent: both `Warehouse` implementors answer a
leg with a typed error, so the absence is *stated in the code* rather than only in this file. That is
what a port change bought and it is not the same as being wired. `Executable` on `Warehouse::execute`
makes a leg something every adapter has to answer for; what no adapter can do is answer with rows,
because there is nothing above the legs to combine them.

| Claim | What is built | What is missing before it could be a row above |
| --- | --- | --- |
| SQL a catalog wrote is a separate named shape, never a field on the closed one | `Computation`'s two variants, `InvalidComputation::Both` for a document that writes both, `Computation::kind()` for listing which metrics use the hatch | A `Metric` that holds a `Computation` and a `MetricDoc` that can write `authored_sql`. Until then `kind()` is an accessor on a value nothing constructs, and the claim is true only because the shape is unreachable |
| A catalog-authored fragment is parsed at LOAD, once, for every dialect | `compile` renders one string per entry in `dialect::ALL` and re-parses each in its own target; `embed` inserts the compiled string verbatim, so nothing parses on the query path | A composition root that calls it. `sutura-cli` links `sutura-sql` and could; `sutura-serve` does not - its own manifest omits it, and `FORBIDDEN_EDGES` keeps it out of `sutura-semantic`'s tree so it cannot arrive transitively through `sutura-app` - so `sutura-serve` would have to **refuse** an authored metric rather than serve it |
| How a measure federates is stated once, and a new aggregate cannot compile without saying | `sutura_domain::federation`: three exhaustive matches over the closed vocabulary, `Descent`'s three classes, and `Carried` built purely from the term level so a per-leg ratio division is UNREPRESENTABLE - pinned by two `compile_fail` doctests with compiling twins, each verified non-vacuous by unmarking it | A splitter and a combiner. **The leg plan type now EXISTS - see the row below - and this cell used to give its absence as the reason nothing calls this module; that half is spent.** What is still true is the conclusion: `Descent::of` has no production caller, because nothing constructs a leg. Its only callers are `sutura_domain::plan::leg`'s own tests, where the leg fixtures take their term shape off `Federation::carried` so a fixture and the classification cannot disagree about how many columns travel - which is evidence, not wiring. **Do not cite it as an invariant until a splitter exists**, and three rows were deleted from the table above for exactly this mistake |
| A federated question is made of two leg shapes, and a third cannot arrive without a rendering arm | `sutura_domain::plan::leg`: `LegPlan`'s two variants with the distinct-key shape as a `Fact` carrying no terms, `LegTerm` holding a `PlanTerm` so a per-leg division is unrepresentable, and `Executable` on `Warehouse::execute` and `dry_run` so every adapter's match over what it can be handed is exhaustive. Four `compile_fail` doctests with compiling twins, each verified non-vacuous by unmarking it and checking the compiler's reason: E0308 for a ratio handed to `LegTerm::new`, E0559 for a `range` on a `Lookup`, and E0004 twice for a match missing an arm. `sutura_sql::generate_leg` renders every shape, and `crates/sutura-app/tests/golden/legs.rs` pins five leg fixtures: the statement and its parameters per dialect with a parse check in the dialect it was generated for, the leg plan's own serialized form once - a leg plan is a function of the split and not of a renderer - plus that no leg statement carries a value as text and none carries a row cap. `docs/adr/0007` decides the shape | **A splitter and a combiner, which is the same gap as the row above.** Nothing constructs a `LegPlan` outside a test, so the golden fixtures are hand-built and say so; both `Warehouse` implementors answer `Executable::Leg` with a typed `LegWithoutCombiner`, which is a refusal to execute rather than an execution path. **So this is rendering pinned, not federation delivered:** a leg executed with nothing above it returns rows at a finer grouping than the question asked for, and that is a wrong number under a certified name. `feat/two-source-execution` is where the leg arms stop erroring, and its differential test against the single-source corpus is what would make any of this citable |
| A fragment cannot reach past the metric's own model, and cannot use a construct that translates wrong | `Construct` over the parsed AST, the five shape guards plus the rendering comparison behind them, the qualification postcondition, and an allowlist of callable function names. **Not every refusal it declares has a fragment that provokes it, and the suite says which rather than leaving a list that reads as coverage:** `Construct::SchemaStatement` is tested at the dialect layer's own DDL classifier, because the two spellings that get a DDL node into a parsed tree are refused as `Construct::Query` one guard earlier and the keyword in expression position does not parse at all - both asserted in that same test, though "no fragment reaches it" is measured against today's parser rather than proved, which is why the guard stays. `ExpressionError::{Qualify, Unrenderable, Render, RenderedDoesNotParse}` have none either, and **not for one reason:** `Qualify` needs the dialect layer's own transformer to break an invariant, `Unrenderable` and `Render` are ruled out by construction - a fragment is capped at 1024 characters and refused past a depth of 32, while the layer's complexity guard sits at a million nodes or a depth of 512 and no dialect configuration raises its unsupported level - and `RenderedDoesNotParse` is deliberately not ruled out at all - it is the load-time net for a generator emitting text its own parser rejects. So what is pinned for those four is the WIRING - the typed fields, and that the cause survives `#[source]` - in a test named `the_four_refusals_only_a_dialect_layer_defect_can_produce` rather than for coverage | The same call site. The checks are exercised by their own suite and by nothing that reads a file |

The gap that is not wiring, and the reason finishing the load path would not finish the feature:
**no shipped binary could execute an authored expression even with the load path in place.**
`sutura-exec-datafusion` is the engine and generates no SQL, so `Computation::measure()` returning
`None` has to be a refusal there; `sutura-exec-duckdb` renders through `sutura-sql` and is a
dev-dependency. Wiring the load alone would move the refusal from load time to query time rather than
deliver an answer, and which composition root gets an execution path for authored SQL is an
architecture decision. `docs/adr/0004` records the state and the two things that would have to be
decided.

## Changing The Query Path Or The Tool Surface

The tool surface is the governance boundary. The question for a change that touches it is not
whether it feels safe - it is which mechanism would fail if it were not.

| Change | Must still hold | What fails if it does not |
| --- | --- | --- |
| A new or widened tool input | No field carries SQL, a table, a predicate or row ids | `deny_unknown_fields` on each wire shape, which makes an undeclared field an error naming it - asserted through the transport in both `sutura_http::wire` and `sutura_mcp::server` rather than assumed to survive it. **Plus the byte-compare this row used to claim and did not have:** it exists now, for the agent surface, as the committed schema snapshot in `crates/sutura-mcp/src/snapshots/` - so a DECLARED field arriving unreviewed fails a test, where `deny_unknown_fields` only stops an UNDECLARED one from being answered. **The limit:** there is no equivalent dump for the OpenAPI document, so on the HTTP surface a widened input is still caught by the deserializer and by review alone |
| A new failure mode | It is a `RefusalReason` variant inside `ToolOutcome`, not an `Err` | **Two** exhaustive matches with no wildcard arm, one per transport: `sutura_http::wire::refusal` decides status, code and detail together, and `sutura_mcp::refusal` decides code and detail - MCP has no status. A variant added to the domain **fails to compile in both** until somebody assigns them, which is the cost of an adapter not reaching into another adapter and is a compile error at a named line rather than a silent gap. The two crates cannot see each other, so what keeps their vocabularies equal is a derivation rather than a comparison: `the_code_is_the_variant_name_in_snake_case` reads the variant name out of the domain type's own `Serialize` and asserts the code is its snake_case spelling. **Nothing compares the two sentences**, and nothing should - they are written for different readers |
| Reading from the catalog at request time | Descriptive content only - nothing that selects, widens or parameterizes what executes | `load()` has no `RequestContext` to pass it; dimension validation reads `PinnedDefinitions`, not the scoped view |
| A second execution leg | One answer has one asker, and no leg runs as a third identity: each leg runs as the asker, or under that source's acknowledged shared identity, and the answer records which. **"Every leg runs as the same subject" was the wording here and it was overstated** - a source configured to serve everyone as one identity does not run as the asker, and a shape that made the labels agree would not have made the identities agree | The plan stage's one-source set, which refuses `PlanSpansTwoSources` today. **The RECORDING half is now built** - `Provenance` carries an `ExecutedAs` with one entry per leg, read off the adapter, and `ExecutedAs::and` refuses a second leg for one source - so an answer says which posture produced it. **The CREDENTIAL half is now built too**, and it does not deliver the sentence in the left column: `identity::LegCredentials` holds one asker and one deadline for N legs, `Presented` says per leg which of the two identities ran, and `Warehouse::execute` cannot be called without one. What is still absent is an adapter that can carry a per-subject credential - both shipped ones declare `NoPlaceForASubject` - so **a test that asserts two subjects get different rows still does not exist and still cannot.** Adding a second leg without it is an architecture decision, not a feature |
| A change to a definition or its anchor | It was authored upstream, not here | The digest moves and the anchor test re-executes the statement |
| A new knowledge kind, or a second consumer of one | The prompt stays the only consumer, and nothing a note carries selects, widens or parameterizes what executes | For the kind: `Capability::next`, `Capability::previous` and `prompt::knowledge::claim` are three exhaustive matches it has to pass through, plus the `const` assertion that holds the seed of the walk `Knowledge::assemble` guards with. For the consumer: **nothing mechanical.** `Query` having no field a phrase fits in is what makes the glossary descriptive, so reading a note anywhere else is an architecture decision - flag it in the handoff |
| A new tool, route or operation on either surface | It names a `sutura_app::Capability` and that capability names a scope, and both transports describe the same set | `Capability::next`, `::id` and `::scope` are three exhaustive matches with no wildcard arm, so a new capability does not compile until each is answered - and `sutura_mcp::tool`'s `description` and `input_schema` are two more, so a tool nobody wrote a description or a wire type for does not compile either. A route added to `sutura_http::routes::v1` with no row in `capability::governed` fails `assemble` as `RouteNotGoverned`. `both_transports_describe_the_same_tools`, once per transport against the one declaration, is what fails when only one of them grows. **Adding a capability is a change to the DEPLOYED contract**: an authorization server has to be configured with the new scope, which is why the identifiers and the scopes are pinned by value |
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

Current groups: `agent-system/` (how skills themselves are written and policed), `engineering/`
(Rust here, debugging, OAuth and token exchange), `git-ops/` (stacked branches, which the ADRs
route to) and `reasoning/` (autoreason). **This sentence was wrong until review caught it** - it
listed two of the four, so a reader following it would not have known `git-ops/` existed while two
accepted records cited it. `cargo xtask check-skills` compares the router against the tree in both
directions and does not read this line, which is why it drifted; the fix for that class is to keep
the prose to what the tree says and let the gate own the router. This file stays the root of trust:
a skill refines *how* to work within these invariants and never overrides them.

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

## Design Principles

The last bullet of *Conventions* names three principles in one line each. This is the long form, and
it exists because "we follow the newtype guide" is not a rule anybody can be held to. Adopted as
policy means every distinction the source draws, including the ones that are inconvenient here.

Four sources, and they are the definition of *correct* for a review here:

| Principle | Source |
| --- | --- |
| A newtype parses rather than validates | [the newtype guide](https://www.howtocodeit.com/guides/ultimate-guide-rust-newtypes) |
| An error is a typed enum whose fields carry the context | [structured error handling](https://www.howtocodeit.com/guides/the-definitive-guide-to-rust-error-handling#structured-error-handling-in-rust) |
| Dependencies point inward | [hexagonal architecture in Rust](https://www.howtocodeit.com/guides/master-hexagonal-architecture-in-rust) |
| Security comes out of modelling the domain precisely, not out of a layer on top | secure by design, in the sense Johnsson, Deogun and Sawano give it: domain primitives, invariants enforced at construction, illegal states unrepresentable, failing securely. **No URL, deliberately** - the rules below are derived from those four and from what this repo already does, so read them as ours rather than as a summary of theirs |

**Nothing in this section is an invariant, and none of it may be cited as one.** Where a rule below
has a mechanism, that mechanism is already a row in *Invariants* and is NAMED here rather than
restated, so there is one place to look up what actually fails. Everything else is **advisory -
review catches it or nothing does.** `.agents/skills/engineering/rust/SKILL.md` walks the same
ground mistake-by-mistake, with a *Caught by* column that says *review* wherever nothing fails the
build; this section is the reasoning underneath those tables and does not repeat them.

### Newtypes And Domain Primitives

The whole return on the pattern is the guide's guarantee - *"If an instance of a newtype exists, we
know that it is valid"* - because that is what lets downstream code stop asking. A newtype that can
still be checked has moved the question rather than answered it, and a check that accepts more than
the type's name claims is worse than none - the skill's table carries the real case this repo
already shipped.

- **One canonical constructor, and every other way in delegates to it.** *"Define conversion traits
  in terms of a canonical constructor."* Here that constructor is `parse`, and `From`, `TryFrom` and
  `#[serde(try_from = "..")]` route through it. A second copy of the checks is where the next rule
  fails to get added.
- **`try_from` and `into` are a pair.** `serde(try_from)` affects `Deserialize` only, so a derived
  `Serialize` still writes the struct. `Date` shipped exactly that: a date this workspace serialized
  was a date its own `Deserialize` refused. It mattered because the definition digest is taken over
  the serialized form, so it covered a field layout that appears in no catalog file rather than the
  ISO text an author wrote. Beyond the guide, and learned here.
- **Sanitize, then validate, and do both inside the constructor.** Trim, fold case, strip what is
  invisible, then check the shape - so a derived `PartialEq`, `Hash` and `Serialize` all agree which
  value this is, and no comparison site has to normalise. `Phrase::parse` is the worked example.
  **State the limit with the claim:** normalisation reaches exactly as far as what it names, and
  *Invariants* records that ours stops short of Unicode normalisation.
- **Derive the standard traits where they mean something, and refuse the one that does not.**
  `Debug`, `Clone`, `PartialEq`, `Eq`, `Ord`, `Hash` cost nothing and get used. `Default` is the
  guide's own counterexample: a default email address is not a thing, and a `Default` here would be
  a value that never passed `parse`.
- **Write the comparison by hand when the newtype's ordering is not the inner type's.** Derived `Ord`
  on a struct is declaration order, which is why `Date` documents its field order and asserts it in a
  test: reordering the declaration would silently invert every comparison.
- **`AsRef` yes, `Deref` no, `Borrow` almost never.** `Deref` re-exports the inner API and the
  invariant leaks out with it - that one is in the skill's table. `Borrow` is the one worth adding
  here: the guide calls it *"unofficially unsafe"*, because implementing it PROMISES the wrapper
  hashes, compares and orders identically to what it borrows and the compiler checks nothing - so a
  newtype that folds case where the inner type does not turns a map lookup into a silent miss on an
  entry that is present. *"Scrutinize any `Borrow` implementation you see in code review."* There is
  no first-party `Borrow` impl in this workspace today.
- **Getter names carry the cost.** `as_x` borrows, `into_x` consumes, and a hand-written `to_string`
  shadows the one `Display` already gave you.
- **A mutating method preserves the invariant or does not exist.** The guide's `NonEmptyVec::pop`
  returns `None` rather than emptying the vec, and the payoff is that `last` is infallible. Ours
  mostly sidesteps this: domain values are parsed once and read.
- **Prefer an associated function to an inherent method on a generic wrapper**, so a wrapper method
  and an inner method cannot collide at resolution.
- **The guide's escape hatch does not exist here.** `new_unchecked` is `unsafe`, `unsafe_code` is
  `forbid`, and a crate cannot re-allow it - so re-parsing a value that was already parsed is the
  price of the ban, paid deliberately.
- **No `nutype`, no `derive_more`.** The guide's own caveat is to write the boilerplate by hand first
  and understand what a macro would generate. There is a second reason here: the domain's dependency
  list is four crates and `cargo xtask check-boundaries` walks the whole transitive tree, so a macro
  crate arriving in it is an architecture decision rather than a convenience.
- The orphan rule is a reason to *reach for* a newtype, never the reason to design with them. Type
  safety is.

### Structured Errors

The variant is the contract; the message is not. The reason is Hyrum's Law rather than taste - with
enough callers, every observable behaviour gets depended on, error strings included, and the guide's
example is Go's `http.MaxBytesError`, whose text carries a comment saying it cannot be changed
because something downstream matches on it. So: *"Codify all possible error states in your public
API."*

- **The audience decides the shape.** A caller that must branch gets a typed enum. A dynamic error is
  for the case where nothing but a human will read it, and a caller forced to downcast into your
  types is reading your implementation - the legitimate downcast is a caller retrieving an error it
  handed you itself.
- ***"Return only your own or standard library error types across crate boundaries."*** Mechanised
  for the worst cases: `check-boundaries` fails `Result<_, String>` and a dynamic-error crate in any
  library crate, and `anyhow` appears nowhere in this workspace - `Cargo.lock` included, so not even
  transitively. **Not** mechanised: a variant re-exporting a third-party error type is a review
  question.
- **Erasure has one honest place, and it is a boundary.** `sutura_app::ErasedCause` is a
  `Box<dyn Error + Send + Sync + 'static>`, and it is not a counterexample to the rule above:
  `SurfaceFailure` keeps each cause as an owned `#[source]`, so the chain still walks and a caller
  that knows the adapter can still downcast. `ServiceError<E>` stays generic in the adapter's error
  precisely so the typed error survives to that point. The shape this replaced was a message plus a
  `Vec<String>` of causes - a presentation of an error rather than an error API. Flattening to text
  still happens, at the logging sink, which is the one place text is the point.
- **Narrow, per-operation error types; never one umbrella enum per module.** The guide's rule is to
  prioritise the *relevant* information and minimise unrelated noise: ten variants where two apply
  makes every caller filter. Its own test is the practical one - if enumerating the failure
  permutations is a chore, the type is doing too much. Compose at the boundary instead, with a variant
  for the inner error and `#[from]`.
- **Wire the cause, because nothing does it for you.** `Error::source` defaults to `None`, so a chain
  you did not attach does not exist. `#[from]` or `#[source]`; `.map_err(|_| ..)` throws the cause
  away, and that one *is* caught - `clippy::map_err_ignore`, from the `restriction` category.
- **Errors are `'static` for a reason.** They are handled after the code that produced them returned,
  sometimes on another thread. That is also where `ErasedCause`'s `Send + Sync` comes from: a
  transport answering on a blocking pool sends the failure back across a thread boundary.
- **No catch-all variant.** `std::io::Error` is the guide's cautionary tale: `ErrorKind::Other` became
  load-bearing because callers matched on it, and adding precise variants broke them - the repair was
  a hidden `Uncategorized`. Here the pressure runs the other way, and *Invariants* records it: a
  `RefusalReason` variant no test can provoke is one that enum refuses to carry.
- **An expected outcome is not an error at all.** This is the sharpest departure from the guide's
  framing, which is about what to put in `Err`; the question that comes first here is whether the
  failure belongs there. A governance refusal is `ToolOutcome::Refusal`, in the `Ok`. `Err` is for
  something that went wrong, and a question this deployment declines is something that went right.
  `Surface::answer` is where a transport inherits that, and the reason is in its doc comment: a
  caller must not be able to mistake "you may not ask that" for a hiccup and retry until something
  works.
- **Nothing sensitive in an error.** The typed fields are read by machines and the `Display` by
  humans; neither is a place for a credential, a row, or a path. `Secret` mechanises the credential
  half - see the two rows above, both of which exist because the accident is silent.
- One deliberate deviation, already recorded in the skill: `#[non_exhaustive]` is not used here, and
  `missing_errors_doc` is allowed because an exhaustive typed enum already is the documentation.

### Ports And Adapters

`sutura-domain` is the hexagon's interior; *Layout* above is the map. The guide's line is that the
flow of dependencies points in one direction, towards the domain. `check-boundaries` is the only
mechanism, and it reads **dependency direction** - not intent, and not which crate declares a trait.

- **The domain declares the port, named for what the domain needs**, and an adapter conforms to it.
  `Warehouse`, `SemanticCatalog`, and `Surface` for the driving side. That a driving port is not
  owned by one of its callers is a row above, and that row says plainly it is not gated.
- **The adapter wraps the library and maps its errors at the edge.** *"Wrap external libraries and
  expose only the functionality your application requires."* A `datafusion` or `duckdb` error
  reaching a caller of the port is the failure mode; nothing stops it except the port's signature
  naming domain types and domain errors only.
- ***"Always separate your public errors from their domain representations."*** The public shape here
  is the refusal code on the HTTP surface, and choosing it is the transport's job.
- **Transport adapters stay thin:** parse the wire shape, translate into the domain type, call the
  port, map the outcome back. A predicate assembled in a handler is business logic in an adapter, and
  *Changing The Query Path Or The Tool Surface* is what it would have to get past.
- **Composition happens once** - `sutura-serve` for the HTTP surface, `sutura-cli` for the binary -
  with generics and trait bounds rather than `dyn`. *"The less code you put in `main`, the smaller
  your testing dead zone."*
- **A port gets a fake, and the reason is coverage rather than speed.** Integration tests are not
  suited to exhaustive coverage, and every refusal variant has to be provoked somewhere.
  *Conventions* says fakes, not mocked HTTP; this is why.
- **An adapter never calls another adapter.**
- **No serde on a domain type for a transport's convenience** - a wire shape belongs to the
  transport. The skill records the one exception and why `#[serde(try_from)]` keeps it from being a
  hole.
- **One domain, on purpose.** The guide says start with a single large domain, and that entities
  which must change together in one atomic operation belong in the same one; the tell that a boundary
  is wrong is a transaction leaking into business logic. The equivalent tell here is a plan that
  would span two sources, and it is refused as `PlanSpansTwoSources` rather than split.
- **Two deviations, both deliberate.** The guide lets `anyhow` flow freely and recommends an
  `Unknown(anyhow::Error)` catch-all in a domain error enum; neither is allowed here, because
  `check-boundaries` fails a dynamic-error crate in a library crate and a catch-all is exactly what
  the refusal enum may not have. And the guide's "do not panic on an unexpected error" is stricter
  here than there: it argues from a poisoned mutex, while shipped profiles compile with
  `panic = "abort"`, so a panic is process death.
- The guide also lists when hexagonal is not worth the tax - a solo project, CRUD with no business
  logic, a path where the transformation cost is the product. This is none of those: the boundary is
  the product, and adding a metadata provider or a data system is a registration rather than a test
  edit, which is a row above.

### Borrowing, And What Deserves An `Arc`

Security and performance are decided together here, and both are decided early. A needless copy on the
federated path multiplies the working set against a memory bound that REFUSES, so an allocation in a
leg is a correctness question rather than a style one.

- **Prefer borrowing. A clone is a decision with a reason, never a way past the borrow checker.** If a
  lifetime is hard, the shape is usually wrong: something is being held across an await it does not
  need to cross, or a value is being owned where a reference would do.
- **`Arc` is for state that is genuinely shared across tasks and immutable once built** - the pinned
  bundle, the certified key the TLS resolver hands out. It is not a lifetime escape hatch, and
  `Arc<Mutex<_>>` around per-request state is the shape to stop and rethink.
- **The scoped view BORROWS the pinned definitions** rather than copying them, and that is not an
  optimisation: it is what keeps `load()` off the request path and makes visibility filtering
  incapable of acquiring I/O.
- **A port takes `&self` and holds no request state.** That is an invariant above, and it is also what
  makes sharing an adapter across tasks free rather than something to engineer.
- **Know which clones are cheap.** Arrow buffers are reference-counted by construction, so cloning a
  batch moves no data; treating it as a copy produces worse code, not safer code. The opposite mistake
  is cloning a `String` per row because the signature asked for one.
- **Measure rather than assert.** The numbers that decided the federation shape were wall clock and
  peak resident set on a real corpus, not reasoning about allocations. A claim about cost in a review
  is worth what its measurement is worth.

### Secure By Design

A control that holds by construction is the only kind this file counts. The three sections above are
that argument applied to types, errors and boundaries; what follows is the rest of it, and most of it
is a habit rather than a gate.

- **A catalog document is untrusted input, not trusted configuration.** It reaches a parser, and the
  row about the panicking fragment API exists because an abort was reachable from a catalog file.
  Anything read off disk gets the treatment a question off the wire gets.
- **Bound the input at the edge, before anything does work proportional to it.** The transport caps
  the request body from `server.max_body_bytes` and holds a request timeout; the bundle caps authored
  text with `MAX_KNOWLEDGE_BYTES`; a question's range and dimensions are bounded, and so is the
  result. An unbounded input is a denial-of-service primitive whatever else it is, which is why
  **availability is treated as a security property here and not as an operational one.**
- **Order the checks inside a parse for the clearest diagnostic, and say that is what you are doing.**
  The classic ordering - origin, size, lexical content, syntax, meaning - is there so an expensive
  check never runs on input a cheap one would have rejected. Once the input is already bounded, that
  argument is spent, and `NoteBody::parse` deliberately checks *is there prose here at all* before it
  checks bytes, because "this note is empty" is the more accurate thing to tell its author than "this
  note has a hidden character". The source says so at the branch. **A defence and a diagnostic are
  different jobs; do not let a comment claim one and deliver the other.**
- **Prefer unrepresentable to checked.** `TimeRange` has no unbounded form. `Measure` has no
  `expression:` field and no `Option<String>` at any depth. A check can be moved, skipped or ordered
  wrongly; a shape that cannot hold the value cannot.
- **Fail closed on the query path, and state which way each default points.** A refusal the caller
  can see beats a degraded answer it cannot: a result at the row cap is refused rather than truncated,
  a plan over two sources is refused rather than downgraded, and an unvalidated bundle is never
  served. The deliberate opposite lives in the tooling, where `classify` and its siblings **fail
  open**, because the expensive failure there is a new directory silently skipped rather than a wasted
  minute. **Neither direction is the default: what a wrong answer costs decides it, per mechanism,
  written down where the mechanism is.**
- **A credential does not travel through a log, an error or a `Debug`.** `Secret` is the mechanism,
  and it is two rows rather than one because there are two silent accidents - a redaction that only
  holds at the top level, and a derived comparison that becomes a timing oracle at whatever call site
  adds it later.
- **Least authority on the execution leg.** End-to-end impersonation is the point of the product: a
  query executes as the subject who asked it. Where that is not true yet, this file says so - and it
  is worth being precise about which half exists now. **Leg 1 is built**: a deployment that declares
  `security.inbound` knows who is asking, from a signature. **Leg 2 is half built, and the halves are
  worth telling apart because only one of them is what the product promises.** The PORT is built: a
  credential per leg exists, `Warehouse::execute` cannot be called without one, and a subject with no
  credential at a source is refused rather than answered as the process - so there is no longer a
  signature that runs as this process, which is what the fallback used to be. **The one method that runs
  with no credential is the boot path's, `Warehouse::verify_anchor`, and what keeps it there is a LINT
  rather than its input type - two reviews to get that sentence right, and the first version of it was
  false.** `clippy.toml` bans the method and `sutura_app::verify_anchors` holds the single `#[expect]`,
  so a second call site is an error under `-D warnings`; a lint reaches this workspace and an
  `#[allow]` walks past it, which is the limit. Its `AnchorPlan` input is a **self-check on that one
  caller**, reading the metric's definition, its anchor's range and its coarsest grain off the pinned
  bundle - so it catches a boot path that compiled the wrong question and it is NOT a barrier: every
  value it reads is publicly constructible. Do not cite the type as a control. What is NOT built is a
  source that executes as the asking subject: both shipped adapters declare that they have nowhere for
  a per-subject credential to arrive, and the broker that ships mints from configuration and performs
  no token exchange. So a deployment can now name the subject in every audit record, record which
  posture each leg ran under, and still read every row as one identity - which is the confusion
  `docs/adr/0014` and `docs/adr/0010` both warn about, and why the startup log prints the limit beside
  the mode rather than only the mode.

  **A scope narrows the SURFACE and it is not leg 2 arriving early.** `sutura_app::Capability` gives a
  deployment two grants to hand out, so a caller can be allowed to read the catalog and not to ask a
  question - which is least authority over the operations, and it is worth having. It buys nothing at
  all over the rows: both operations read the same bundle and every question runs with whatever access
  the process already had. Describing scope filtering as per-caller access would be exactly the
  overstatement the row above exists to name.
- **State the limit next to the claim.** The strongest habit in this file, and the easiest to lose:
  the leak guard cannot catch a paraphrase, catalog cardinality is a trusted precondition nothing
  checks against the data, and parse-checking a statement is narrower than a data system accepting
  it. A control described as stronger than it is spends trust a reviewer needed elsewhere, so **an
  overstated claim is itself the defect** - and *Built And Not Wired* is what it looks like when we
  find one and refuse to leave it in the table.

## Where Detailed Guidance Lives

- `devenv.nix` - the shell, the tool pins, and the task names used above.
- `nix/*.nix` - the modules `flake.nix` imports, one topic each: `toolchains`, `duckdb` and
  `crap` are shared with `devenv.nix` so a pin cannot differ between the shell and CI;
  `mimalloc`, `oci`, `api-docs` and `cargo-env` are flake-only, carved out when `flake.nix`
  reached the 1000-line limit. **What may NOT move out of `flake.nix`:** `apps.<name>`, the
  `packages = ` block and the `checks = {` block, because `xtask/src/pins.rs` and
  `xtask/src/workflows.rs` scan that file for them textually and both fail closed on finding
  none. A module holds what an app or a check *points at*, never the declaration.
- `.pre-commit-config.yaml` - what runs on commit (tests included), on commit-msg and on push
  (where clippy runs a second time, because a rebase resolution used to reach the remote with
  nothing having compiled it). `cargo xtask check-hook-tiers` is what keeps that true.
- `clippy.toml` and the workspace lint table - the bans, each with its reason. The whole
  `restriction` category is on; the override list is where a specific ban gets disagreed with.
- `deny.toml` - advisories, licence allowlist, duplicate versions.
- `devco/` - config that only this repo's own tooling reads.
- `xtask/` - every gate, each unit-tested, because a gate with no test is one nobody has seen
  fail. `cargo xtask --help` lists them; `hygiene` runs the cheap ones. `classify` /
  `check-changed` / `changed-packages` decide what a diff requires, and **fail open**: an
  unmapped path, a bad base ref, an empty diff or a git that will not answer all run everything
  and say why, because the expensive failure is a new directory silently skipped, not a wasted CI
  minute. `check-scope` is the one that reads the `justfile` rather than Rust: a recipe compiling
  PART of the workspace has to print which part, name a task that covers the whole of it, and cite
  nothing that has been renamed away - which is checkable where "did you read the scope in the
  comment" is not. `check-hook-tiers` is its sibling over `.pre-commit-config.yaml`: the push stage
  has to run a hook that COMPILES, and that hook's entry has to be the commit stage's own - the
  first because every compiling gate used to be a commit hook and a rebase runs none of them, the
  second because cargo keys its fingerprints on the invocation, so a push-stage command differing
  by one flag rebuilds the workspace instead of reusing what the commit hook built. Hook tiers are
  bypassable with `--no-verify`, so neither is an invariant and neither is a row above; what the
  gate holds is that the tiers `CONTRIBUTING.md` documents are the tiers the file declares.
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
