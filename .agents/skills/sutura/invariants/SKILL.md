---
name: invariants
description: The guarantees held by a type, a lint or a gate - and, for each, the limit it does not cover. The limit column is the point: it is what no grep of the code will tell you. Open before changing a mechanism or claiming anything is enforced.
---

# Invariants

Held by a **type, a lint, a hook or a gate - never by recall.** Changing one is an architecture
decision. **A row that loses its mechanism gets deleted, not demoted to advice**; a rule that never
had one belongs in `secure-by-design` instead.

`cargo xtask --help`, `clippy.toml` and the workspace lint table tell you *what* runs. This file
exists for the other column: **what each control does not reach.** An overstated control spends
trust a reviewer needed elsewhere, so an overstated claim is itself the defect.

House pattern: a `compile_fail` doctest always comes with a compiling twin differing by exactly the
line under test, verified non-vacuous by unmarking it. A `compile_fail` with no twin proves the
snippet is broken, not that the rule holds.

## The surface

| Guarantee | Held by | Does not reach |
| --- | --- | --- |
| No SQL, table, predicate or row-id on the tool surface | `Query` declares no such field; `deny_unknown_fields` names the attempt | - |
| A refusal is a value, not an `Err` | `ToolOutcome::Refusal`, and two per-transport exhaustive matches with no wildcard arm, so a new variant fails to compile in both | Nothing compares the two *sentences*, and nothing should - different readers |
| Every `RefusalReason` variant is provoked, or excused with a date | `check-refusal-coverage` | **A file naming EVERY variant is a census and is evidence for none** - five exist. And a NAME is not a provocation: a variant mentioned in a test that does not trigger it counts, because deciding otherwise needs to know what a test asserts |
| A caller cannot state its own identity | Nothing in `identity::principal` implements `Deserialize`; the verified path needs a `VerifiedCaller` whose one constructor is `pub(crate)` | The reviewer's question is "can a request PRODUCE this type", not "can it reach this parameter" |
| A granted scope gates the operation | `Capability`'s three exhaustive matches; `RouteNotGoverned` refuses a router with an ungoverned route | **Never which rows an answer contains.** And nothing narrows the agent surface: a pipe has no header a token could arrive in, so `serve_stdio` passes every capability |
| Every outcome is recorded before it is returned | `AuditSink` returns nothing a caller can branch on; `LocalService::start` requires a sink and `Surface::answer` writes the record before its `Ok`; a sixth half of `check-boundaries` refuses a **caller of the driving port** that names `sutura_app::answer` in its own `src/` | **Recording is not a control** - it reaches a caller after the rows did. sutura retains nothing. A `SurfaceFailure` is not an outcome, so an `Err` writes no record. **The constructor was the whole mechanism and `sutura query` was outside it**: it called the answer function directly and wrote nothing, on the shipped binary, which is why the gate exists - `answer` stays `pub` for the golden suites that assert `ServiceError`'s variants, so what holds this is a text scan over callers' `src/` and a crate that is not a caller is outside it. `#[cfg(test)]` is skipped there, deliberately: test code is not in a published binary. **A record is written and not kept** - neither CLI command installs a tracing subscriber, so `query` and `mcp` both write onto a dispatcher that discards; only `sutura-serve` installs one. **A catalog read is not an outcome**, so `DescribeCatalog` writes no record on any transport - argued at `sutura_mcp::server::describe`, and a listing of what a deployment measures is business information with no row of data in it |

## The plan and the statement

| Guarantee | Held by | Does not reach |
| --- | --- | --- |
| No value reaches the statement as text | One bind placeholder per plan value, counted per dialect and compared against the plan's parameter list | A value that word-boundedly EQUALS a name the statement quotes is invisible to both negative searches (`month` is a column *and* a legal filter value). Only the placeholder count reaches it |
| Every statement parses under its target dialect | The golden suite parses each statement in the dialect it was generated for | **Narrower than "the data system accepts it".** ClickHouse and BigQuery are never executed. And MEASURED: within one target the parser cannot see a function's ARGUMENT ORDER, so both `DATE_TRUNC` spellings parse as BigQuery - which is why `DateTruncShape` is an exhaustive declaration rather than a check |
| A dialect declares what it folds and how deep it resolves | `identifier_case` and `qualification` are exhaustive matches, so a fifth dialect cannot compile without answering | **A self-check, not a barrier:** a bundle is dialect-agnostic, so both comparisons use `IdentifierCase::COARSEST` whatever a dialect declares. Postgres and ClickHouse are declared from docs, not measured - neither has a server here to ask |
| Two tables one statement reads can be told apart | `StatementTables` is the only way to a `QueryPlan` or a `LegPlan::Fact`, so the ambiguous plan is unrepresentable rather than checked at render time | A query-time refusal, deliberately - a physical table name is not something an author can rename. A `Lookup` leg declares no joins, so what holds there is the shape |
| A join cannot silently change a measure | `Definitions::assemble` refuses a dimension reached through a relationship whose *declared* cardinality may duplicate rows | **Catalog cardinality is a trusted precondition**: nothing checks the declaration against the data, and an anchor cannot see it - an anchor is asked with no dimensions |
| A question cannot span more sources than the answer can combine | Three or more is `PlanSpansTooManySources`; exactly two is split, or `FederationNotExecutable` | The shipped binary refuses **every** two-source question - both its adapters declare `EXECUTES_LEGS = false` |
| Cross-project is one source | A source is *a credential plus a billing project*, not a project; the plan stage collects `SourceName` and nothing from a table path | That a data system PERFORMS such a join is shown by nothing - the acceptance credential's IAM refuses `datasets.create` |
| We never translate SQL | The dialect layer's `transpile` feature is not compiled, so the call does not build | Deliberately **not** a lint: an unresolvable `disallowed-methods` path is silently ignored at the pinned clippy version - verified - so the entry would read as enforcement and do nothing. A later clippy warns instead, so this expires |

## The bundle

| Guarantee | Held by | Does not reach |
| --- | --- | --- |
| A catalog edit cannot change what executes | `PinnedDefinitions::pin` computes the digest from what it stores - no digest parameter, no hasher parameter - and it travels with the answer | - |
| An unvalidated bundle is never served | `verify_and_validate` is `Validated`'s only constructor, in a private module. Boot reaches a data system through `verify_anchor`, which takes **no credential** | An anchor certifies whatever identity the adapter was configured with. What keeps `verify_anchor` to boot is a `clippy.toml` ban plus one `#[expect]`, **not** its input type: `AnchorPlan::of` is a self-check, and a reviewer fabricated a tuple that passed four guards. **Do not cite the type as a control** |
| An anchor with no identity to re-run it under does not boot | `anchors_run_as` returns a three-variant enum whose third is `NoneDeclared` rather than an `Option`, so a reader names the case instead of deciding what an absence permits | It reads a DECLARATION and nothing passes it to the port - it refuses a deployment that could not have verified honestly rather than proving which identity ran |
| A bundle naming an absent table does not boot | `Warehouse::preflight`, one call per dataset, decided once in `sutura_app::preflight::ask` for both serving roots. **Its ORDER** - after the credential is read, before the transport opens - is held by `check-boot-order` in `just hygiene`, reading three call sites per root as text and cross-checking the roots it declares against every file under `crates/` that calls the pre-flight: both roots held it in prose, one of them calling it *a convention this line keeps* | **It reads the bundle each root loaded FIRST**, and both roots load twice - so a model added to the catalog directory between the two loads is caught on a `files` source by `refuse_unattached` and by nothing on a `bigquery` one. A dataset that could not be ASKED is split by `preflight_was_refused`: `401`/`403` refuse the boot naming the grant, everything else - unreachable, undecodable, `404` - is a `WARN` and the deployment serves, the alternative being a skip flag set in the deployment that most needs it. **A live dataset has answered one** since the `bigquery-acceptance` job went green on 2026-09-02; a live dataset has never FAILED to answer, so the split itself is exercised against a fake transport only. The order gate reads TEXT order in one file, so a pre-flight moved into a helper that runs after the listener binds reads the same to it - and the agent root's transport anchor IS inside such a helper, so what is compared there is where that helper is *defined*. A root that omits the pre-flight ENTIRELY is invisible to it - the scan keys on the pre-flight's own name, so it finds the roots that call it and cannot find one that never did. **Measured:** with its root list emptied it printed `ok - 0 composition root(s)` and exited zero, which is why the list is compared against a scan of `crates/` rather than trusted |
| A metadata adapter declares what it cannot supply | `capabilities` is a required associated item with no default | **Nothing refuses a LOAD whose content disagrees with the declaration.** It is a property of the code, not the bundle, so it is not under the digest. Fidelity is a test, not this row |
| Catalog knowledge is descriptive only | The prompt is its only consumer; `Query` has no field a phrase fits in; `load()` takes no `RequestContext` | A second consumer is an architecture decision and **nothing mechanical stops one** - flag it in the handoff |
| Note prose is bounded and normalised | `NoteBody::parse` caps bytes and lines; `Phrase::parse` strips invisibles, collapses whitespace, folds case for identity only | **Not a Unicode-normalisation claim** - there is no NFC/NFD anywhere, so a decomposed spelling and a Cyrillic homoglyph are each a second phrase |

## Identity

Detail and the built-not-wired inventory: `../identity/SKILL.md`.

| Guarantee | Held by | Does not reach |
| --- | --- | --- |
| A source is served under a declared identity or does not boot | `SourcePosture` has no `Default`; its shared variant needs a witness with no `Deserialize`, so a file cannot produce it | The constructors are `pub` - what is closed is the path from a *file*, not from another crate |
| A posture the linked adapter cannot deliver does not boot | `IMPERSONATION` is a required associated constant; `deliverable_by` is called per source per adapter in the composition root, because it is a property of the BUILD | An associated const makes the port not object-safe, so a heterogeneous adapter set wants a closed enum rather than `dyn` |
| No answer leg reaches a data system without a minted credential | `execute` takes a `&Presented` with **no default**; `agrees_with` compares each leg against the adapter's own declared posture | The witness is prose, so equality is the only comparison - a byte-identical forgery is indistinguishable. `Expiry` is read by nothing: the domain has no clock. The BOOT path is exempt by lint, not by type |
| A subject with no credential is refused, not answered as the process | `CredentialUnavailable`, `403` | **Unreachable end to end on the shipped binary**, because the only configuration the shipped broker refuses is one the root will not boot. A broker that could not be *reached* is `503 identity_unavailable` instead - same status as a dead data system, different code |
| An answer says which identity produced each leg | `ExecutedAs` read off `Warehouse::posture`, never the settings tree | Every shipped answer has exactly one entry - no shipped adapter runs a leg |
| An established identity comes from a signature, never a header | `InboundIdentity` is closed with no default; its gateway variant has **no field for a header holding a username**, and a test presents one and gets nobody | Keys come from a file - no JWKS endpoint. Scopes decide operations, not rows |
| Revocation is bounded by a window a caller cannot influence | Age-triggered re-read plus an unknown-`kid` refetch, both compared and stamped in **one** write-lock acquisition | **The caller-driven trigger provably cannot bound revocation** - a revoked key's `kid` is one the cache holds, so nothing fires. That is why both exist. The only shipped source is a local file |
| A gateway assertion's replay window is this deployment's number | `ProofLifetime` caps `exp - iat`; `iat` is required and a forward-dated one is refused | **Nothing binds an assertion to a request and nothing records what has been seen**, so inside the window an intercepted assertion replays - a regression test asserts the replay rather than pretending otherwise |
| A credential cannot be logged or compared by accident | `Secret` has no `Display` and no `PartialEq`, inherited from `secrecy` rather than merely omitted, so a `#[derive]` cannot produce either | `{secret:?}` still compiles, safe only because that formatter cannot reach the value. `expose_secret` is a lint, so `#[allow]` walks past it and doctests are outside it |

## Resources

| Guarantee | Held by | Does not reach |
| --- | --- | --- |
| Too much data is refused, not truncated, and not reported as an outage | `row_limit()` is `max_rows + 1`, so a result AT the cap is distinguishable from one cut off by it. One variant, one code, `413` never `503`, for both bounds. Pinned across the corpus rather than in one snapshot: 115 SQL goldens read `LIMIT 10001` under `crates/*/tests/snapshots`, and `check-guidance` counts them and fails this row when the number drifts - it caught `39` at a real 63 | `Volume` carries no number and cannot - the endpoint reports neither its cap nor the reply's size, and a test asserts the sentence contains no digit. Only the unpublished adapter can reach that bound |
| Operator memory is bounded; exhaustion refuses rather than kills the process | A `GreedyMemoryPool` neither `SessionContext` constructor lets a caller omit | **Not small:** the pool counts operator reservations only - not driver buffering, not `collect()`, not the row set built during conversion. On macOS no boot check is made |
| Nothing spills the asking subject's rows to disk | `DiskManagerMode::Disabled` on every session | - |
| No result cache | There is none to key. Adding one is an architecture decision, keyed on subject first or not at all | - |

## Boundaries

The lint-shaped guarantees - no first-party `unsafe`, no panic path from input, no dead code behind
`pub`, no file over 1000 lines, no unused dependency, `#[expect]` over `#[allow]` - are in the lint
table and `cargo xtask --help`. What is not discoverable there:

| Guarantee | Held by | Does not reach |
| --- | --- | --- |
| The domain acquires no framework dependency | `check-boundaries` walks the whole **transitive** tree against an allowlist, so a framework arriving through an innocuous crate fails too | - |
| The SQL generator is not in the compiler's closure | `FORBIDDEN_EDGES` forbids the edge to the renderer **as well as** to the dialect layer - the second is what stops the first returning transitively | - |
| A driving port is not owned by one of its callers | The caller set is DERIVED (a member declaring a normal dependency on `sutura-app`), so a fifth transport is covered the day it is written. Zero callers is a failure, not a pass | An **allowlist**, not an attempt to tell a driving port from a driven one - which of those a trait IS depends on who implements it, and a text scan may not pretend to answer that. `src/` only; line-oriented |
| An adapter never reaches an adapter of its own KIND | An edge inside one class only - a renderer, process globals and a composition root are legitimate cross-adapter edges. Transitive, so laundering through a third crate is caught. Dev-dependencies are exempt: that is how a corpus reaches a real system | A crate joins a class by NAME, so an adapter called something else is outside every class. A class with ONE member cannot be violated. A class matching NOTHING is a failure, not a pass |
| No first-party `Deref` and no `Borrow` | `Deref` re-exports the inner API so the invariant leaks with it; `Borrow` is *"unofficially unsafe"* - it promises identical hash, compare and order, and the compiler checks nothing, so a case-folding `parse` turns a map lookup into a silent miss. **The gate started green**, so its job is keeping it that way | Comments and string interiors are blanked first, and that is load-bearing: three doc comments here say *there is deliberately no `Deref`*. Matches the last path segment, so a first-party trait named `Deref` is reported too - the safe direction |
| A validated newtype's `Deserialize` goes through its constructor | `check-serde-parse`. **The one that bites**: a derived `Deserialize` writes straight into the private field, on the path carrying untrusted input | Recognised by `-> Result<Self`, so another spelling is invisible - under-claims rather than failing correct code. `impl` blocks matched per FILE. Deliberately ignores a type with no fallible constructor: a derive bypasses nothing there, and a gate that failed them is one somebody disables |
| A type that parses on the way in serializes the way it came | The second half of the same gate. A shipped bug, gated after the fact: `Date` read ISO text and wrote `{year, month, day}`, and the digest is taken over the serialized form | The comparison is over field NAMES. Two structs with the same names serializing differently pass, because deciding otherwise needs serde's own resolution |
| A provisioned service that CAN be hermetic is run by `just validate`, and one that cannot says why | Four links, each a gate: every discoverable service's compose block declares one CI venue; a `nix native` declaration names a `nix/<service>-tier.nix` that must exist; that module must be imported by `flake.nix` **and** its `sutura-<service>-tier` script named inside the `checks = {` block, so a tier only `just` runs does not count; and every check `flake.nix` declares must appear in the `just ci` loop, which is what `just validate` runs. A `compose only` declaration needs a reason and a convergence path, and fails if the tier file exists | **Nothing can decide whether a service COULD be hermetic** - that judgement is the `# Because:` line, which is prose held by review, and it is the whole content of the DataHub row. The chain holds that the declaration and the tree agree, not that the declaration is true. It also cannot see a tier a check builds and never STARTS beyond the script's name appearing, and `just ci` is checked by name, so a check renamed in both places moves silently |
| No `#[expect]` on a count-threshold lint | A threshold lint's cause is a NUMBER - a property of the surrounding function - so two branches can each move it correctly and only their merge is wrong. The message names the two honest fixes: split the function, or raise the threshold deliberately | The categorical lints are exempt on purpose: their cause is the code the attribute sits on |

## What no gate here reads

Every gate reads something - a manifest, a file, a line, prose. **None reads for a missing CALL.**
`sutura_domain::expression` and `sutura_sql::expression` are complete, tested, reachable from
nothing, and every gate passes over them. Same shape as a `disallowed-methods` entry that reads as
enforcement while resolving to nothing.

So when adding a mechanism, ask what would fail if it were absent, and check the answer is not
"nothing, because nobody calls it". `../query-surface/SKILL.md` and `../identity/SKILL.md` hold the
built-and-not-wired inventory, and **none of it may be cited as an invariant.**
