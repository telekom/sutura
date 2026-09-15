---
title: The iamcredentials impersonation hop
description: A declared per-source subject-to-service-account map, a second port mirroring the STS exchange, and why a direct principal:// grant was rejected even though it would need no adapter code - the RowAccessPolicy grantee shape and telekom/sutura#376's own acceptance text both name a service-account email, and a federated principal string can never satisfy that comparison.
---

# The iamcredentials impersonation hop

Status: **accepted.** `github.com/telekom/sutura#376` is the issue; `crates/sutura-exec-bigquery/src/wire/credential/id_token.rs`
and `docs/where-identity-is-proven.md` both name the gap this closes: a bare RFC 8693 exchange
(`docs/adr/0008` part 2, `wire::StsOverHttp`) resolves a workload-identity pool subject and stops
there. Every grant this stack provisions is anchored to a service account as an IAM member
(`test-infra/pulumi/google/__main__.py`'s `RowAccessPolicy.grantees` and `DatasetIamMember`, both
`serviceAccount:<email>`), and the issue's own acceptance text requires `SESSION_USER()` to be
"asserted equal to that principal's known email" - a pool subject can never satisfy that comparison.

## What this decides

**A second hop, `iamcredentials.generateAccessToken`, behind a declared per-source map.**
`WorkloadIdentityBroker` gains a second port, `ImpersonateAsAccount`
(`crates/sutura-exec-bigquery/src/sts.rs`), called immediately after a successful `StsExchange`
exchange for a source whose declared `WorkloadIdentity` names a target account for the caller's
`SubjectId`. `wire::IamCredentialsOverHttp` is the real implementor, mirroring `wire::StsOverHttp`:
same `WireAgent`, same pins, same outbound-anchor resolution, a different request and response shape
and a different failure vocabulary (`IamCredentialsError`, never `StsError`'s).

**The map is declared, not derived.** `sutura_config::sources::workload_identity::WorkloadIdentityConfig`
carries an `impersonate: BTreeMap<SubjectId, WorkloadIdentitySa>`, keyed on the stable claim leg 1
already establishes rather than a raw, numeric, per-provider pool subject string. A subject absent
from the map never reaches the hop and is never granted a fallback identity - the broker refuses
before any network call, the same as today's "no assertion at all" refusal. An entry with an empty
map is unchanged behaviour: a bare exchange, presented as the caller's own federated credential. This
is what makes the change additive rather than a breaking change to every source that has not opted in.

**The cache key gains the resolved target account.** `sts/cache.rs`'s `ExchangeKey` was
`(PrincipalChain, audience, scope)`; a fourth field, `target_sa: Option<String>`, is now part of it -
the same reasoning `docs/adr/0031` already gives for `(audience, scope)`: the target is itself part of
"what was asked for", and a subject permitted to impersonate more than one account depending on
requested scope must not have one account's cached credential served for another. Two round trips
(STS, then `iamcredentials`) are cached as the one entry the FINAL credential is - not two entries for
one leg.

**The audit trail's limit, stated rather than left implied.** The resolved service account is not
threaded onto `sutura_domain::audit::CallRecord` or `Provenance` in this change.
`SourcePosture`/`ExecutedAs` are declared, per-source, config-time facts that ride on the
CALLER-FACING provenance - widening either to carry a per-request resolved account would mean every
one of the ten-plus match sites on `Presented::SubjectToken` across `sutura-app`, every adapter and
every test fixture, plus a new `CallRecord::of` parameter at both its call sites and its own suite -
a separately-scoped change, not a fourth field on an existing struct. Today the resolved account is
visible in the broker's own state and in `sts/cache.rs`'s key; it is not yet the fact an incident asks
`CallRecord` for. Recorded here rather than silently deferred, so the next person to reach for this
does not have to re-derive why it is missing.

## Options considered

**(1) The declared map + hop - taken.** Shaped narrowly above. Costs a second port, a second wire
adapter, and one more settings key; buys a fixed, human-nameable, auditable service-account identity
that is stable across pool and session churn, and satisfies the acceptance text as written.

**(2) Treat "the pool subject the exchange resolves to" as the account - rejected, and real only
inside this harness.** `bq-test`'s two principals are self-signed by their own keys, so here the pool
subject genuinely IS the account to impersonate and there is no separate caller. A production caller
through the IdP (`docs/adr/0014`) has a `sub`/email that is never a GCP service-account identifier, so
this option has nothing to map from outside the test harness. Not generalizable; not taken as the
shipped mechanism, though it is exactly what `crates/sutura-exec-bigquery/tests/exchanged_identity.rs`
declares for its own two principals (self-impersonation), stated as that cell's own limit.

**(3) Grant `principalSet://`/`principal://` directly to the federated identity, no hop - rejected.**
Real: BigQuery does accept a `principal://iam.googleapis.com/…/subject/X` string as a grantee, and
this option needs zero adapter code. Rejected because both anchors above name a service-account
email, and the issue's own acceptance text requires the same - a federated principal string fails
that comparison outright, regardless of whether the grant itself would work. It also trades a fixed,
nameable identity for one that scales with every distinct caller and reports as an opaque numeric
string in every log and grant forever, which is the tradeoff a reviewer choosing between (1) and (3)
should see stated rather than have to derive.

## What this does NOT close

**Does not prove an arbitrary undeclared caller cannot obtain a credential.** A caller absent from
the map is refused by configuration, before any network call - not by a cloud-side policy this crate
re-verifies. `roles/iam.workloadIdentityUser` is Google's own control over who may start the exchange
at all, and this hop does not reverify it, mirroring `github.com/telekom/sutura#123`'s stance on
trusting a source's own row-level security rather than re-checking it here.

**Does not touch Postgres, Oracle, or any source but BigQuery.** `BigQueryWarehouse` remains the only
adapter carrying `PerSubjectCredential` at all.

**The map key inherits `SubjectId`'s own masking, and that is a residual worth stating rather than
discovering by collision.** `crates/sutura-domain/src/identity/principal.rs`'s mask keeps only the
first character of each `.`/`@`-delimited segment, so two DIFFERENT raw subjects sharing a first
character per segment and the same domain - `alice@corp.example` and `aaron@corp.example`, say -
parse to the identical masked `SubjectId` and therefore the identical map key. A deployment declaring
`impersonate` entries for two such subjects would have the second declaration silently replace the
first in the map, with no parse error naming the collision - this crate's own test suite found the
same collision by construction and had to change its own fixture subjects to avoid it. This is a
property of `SubjectId` this record does not change; an operator choosing subjects for this map
should pick issuer-qualified identifiers where the risk is lower, and a follow-up that makes a
colliding pair a declared refusal rather than a silent overwrite is a fast-follow, not this change.

**Does not move `docs/where-identity-is-proven.md`'s row.** The mechanics are exercised by six
fake-port cells in `sts.rs` and `sts/cache.rs`, never by a live call. A green `bigquery-exchanged-identity`
`workflow_dispatch` is what moves that row - the code landing here is necessary and not sufficient,
per that page's own standard for what may be cited for an identity claim.

**Does not thread the resolved account onto the audit record.** See above; recorded as a fast-follow
rather than folded into this change's own scope.

## What this explicitly leaves for later

- **Threading the resolved account onto `CallRecord`/`Provenance`**, once that widening is itself
  scoped and reviewed on its own - see the limit above.
- **A parsed lifetime derived from the caller's own request budget** rather than the fixed
  3600-second ceiling this hop always requests. The broker's own expiry floor already refuses a
  credential that would age out mid-answer regardless of the lifetime requested, so this is a cache
  hit-window optimisation rather than a correctness gap.
