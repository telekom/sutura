# Invariants

A guarantee with no mechanism is a wish. Every row below is held up by a type, a lint, a
hook or a gate - never by recall, and never by a sentence in a document asking people to
remember it. Changing one is an architecture decision, not a refactor.

The authoritative table lives in
[AGENTS.md](https://github.com/telekom/sutura/blob/main/AGENTS.md); if this page and that
table disagree, that table wins. What follows is the same set grouped for a reader, with
the mechanism named in each case.

## The tool surface

| Guarantee | Held up by |
| --- | --- |
| No SQL, table name, filter expression or row-id list can be asked for | `Query` has no such field, so an uncertified question is unrepresentable rather than refused at runtime. Widening it moves the dumped tool schemas, which puts the new surface in the diff |
| A refusal is a result, not an error | `ToolOutcome::Refusal { reason: RefusalReason }` is the public type, and a test provokes every variant |
| Every call is attributable, refusals included | `AuditSink` records the whole principal chain before the outcome is returned |

## Identity

| Guarantee | Held up by |
| --- | --- |
| Every query runs as the calling principal | `CredentialBroker::credential_for(&RequestContext, ..)` mints per request. A leg that cannot run as the subject returns `RefusalReason::SourceIdentityUnavailable`; there is no fallback to a service identity. A nightly two-identity test asserts two users get different rows |
| A plan cannot silently span two sources | `PlanSources` is asserted to have length one by the governance-invariant tests |
| No result cache | Under row-level security a query-keyed cache is a cross-user leak. No mechanism can prove an absence, so this one is explicit: adding any cache of rows is an architecture decision, keyed on subject first or not at all |

## Definitions

| Guarantee | Held up by |
| --- | --- |
| A catalog edit cannot change what executes | Definitions are pinned and hashed at build time, arguments validate against the pinned allowlist, and `SemanticCatalog::load` takes no request context - so it cannot reach the hot path |
| An unvalidated bundle is never served | The service accepts only `Validated<PinnedDefinitions>`; anything else does not compile. An anchor test re-runs each pinned statement in CI and at startup, and failure fails readiness |
| We never re-parse SQL we did not generate | The pinned statement is spliced in byte-for-byte as a derived table, asserted by the SQL goldens. **Gap:** no lint yet bans a transpile call on the query path, so review carries this one until one exists |
| A result cannot be separated from what defined it | Provenance rides in the Arrow schema metadata, and both wire envelopes share one encoder |

## The code

| Guarantee | Held up by |
| --- | --- |
| No panic path reachable from input | `unwrap_used`, `expect_used`, `panic` and `indexing_slicing` are denied for library crates; `clippy.toml` exempts test code, and `panic = "abort"` is set on the shipped profiles |
| A credential cannot be logged by accident | Credential-shaped types are newtypes with a hand-written `Debug`, plus a unit test asserting the secret is absent from the debug output |
| No first-party `unsafe` | `unsafe_code = "forbid"` in the workspace lint table. `forbid` rather than `deny`, so a crate cannot re-allow it locally and lifting it is a visible diff |
| Dead code cannot hide behind `pub` | `dead_code`, `unused_must_use` and `unreachable_pub` are `deny` rather than the default `warn`, so a plain `cargo build` fails on them |
| A suppression cannot outlive its cause | `clippy::allow_attributes` is on, so a bare `#[allow]` is an error. `#[expect(.., reason = "..")]` is required, and it fails once the underlying warning stops firing |
| No interpreter in the query path | Python is build-time tooling only, and the image from `nix build .#oci` holds one binary - so a query-path dependency on an interpreter could not ship |

## The repository

These are the ones with a gate you can run yourself; see [The gates](gates.md).

| Guarantee | Held up by |
| --- | --- |
| The domain acquires no framework dependency | `cargo xtask check-boundaries` |
| No file exceeds 1000 lines | `cargo xtask max-lines`. Generated output is exemptable, anything under `crates/` or `xtask/` is not - the gate fails on such a pattern rather than honouring it |
| No dependency is declared and unused | `cargo xtask unused-deps`, in both directions: a crate must use what it declares, and every workspace dependency must be inherited by somebody |
| Every documentation page is reachable | `cargo xtask check-docs`, in both directions: a chapter in `SUMMARY.md` must exist, and a page in `docs/src/` must be in `SUMMARY.md` |
| Prose still describes this repo | `cargo xtask check-guidance` - a forbidden phrase, a stale version, or a named gate that no longer exists |
| A changed test proves something | `cargo xtask test-causality` requires it red against the base behaviour and green with the change |
