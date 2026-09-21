---
title: The iamcredentials impersonation hop
description: A declared per-source subject-to-service-account map, a second port mirroring the STS exchange, and why a direct principal:// grant was rejected even though it would need no adapter code - the RowAccessPolicy grantee shape and telekom/sutura#376's own acceptance text both name a service-account email, and a federated principal string can never satisfy that comparison.
---

# The iamcredentials impersonation hop

Status: **accepted.** `github.com/telekom/sutura#376` is the issue; `id_token.rs`
and `docs/where-identity-is-proven.md` both name the gap this closes: a bare RFC 8693 exchange
(`docs/adr/0008` part 2, `wire::StsOverHttp`) resolves a workload-identity pool subject and stops
there. Every grant this stack provisions is anchored to a service account as an IAM member
(`test-infra/pulumi/google/__main__.py`'s `RowAccessPolicy.grantees` and `DatasetIamMember`, both
`serviceAccount:<email>`), and the issue's own acceptance text requires `SESSION_USER()` to be
"asserted equal to that principal's known email" - a pool subject can never satisfy that comparison.

## What this decides

**A second hop, `iamcredentials.generateAccessToken`, behind a declared per-source map.**
`WorkloadIdentityBroker` gains a second port, `ImpersonateAsAccount`
(`sts.rs`, deleted since - see the amendment below), called immediately after a successful `StsExchange`
exchange for a source whose declared `WorkloadIdentity` names a target account for the caller's
`SubjectId`. `wire::IamCredentialsOverHttp` is the real implementor, mirroring `wire::StsOverHttp`:
same `WireAgent`, same pins, same outbound-anchor resolution, a different request and response shape
and a different failure vocabulary (`IamCredentialsError`, never `StsError`'s).

**The map is declared, not derived.** `sutura_config::sources::workload_identity::WorkloadIdentityConfig`
carries an `impersonate: BTreeMap<SubjectKey, WorkloadIdentitySa>`, keyed on the FULL verified `sub`
(as a redacting domain newtype that renders only the mask) rather than on a masked, per-provider pool
subject string - keying an authorization decision on the mask would hand every undeclared caller that
shares a declared subject's mask that subject's declared service account. A subject absent from the
map never reaches the hop and is never granted a fallback identity: the broker refuses it before any
network call, naming the source, the same as today's "no assertion at all" refusal. An entry with an
empty map is unchanged behaviour: a bare exchange, presented as the caller's own federated
credential. This is what makes the change additive rather than a breaking change to every source
that has not opted in.

**The cache key gains the resolved target account.** `sts/cache.rs`'s `ExchangeKey` was
`(PrincipalChain, audience, scope)`; a fourth field, `target_sa: Option<String>`, is now part of it -
the same reasoning `docs/adr/0031` already gives for `(audience, scope)`: the target is itself part of
"what was asked for", and a subject permitted to impersonate more than one account depending on
requested scope must not have one account's cached credential served for another. Two round trips
(STS, then `iamcredentials`) are cached as the one entry the FINAL credential is - not two entries for
one leg. Carrying the full `SubjectKey` in the chain is what makes the key mask-safe, too: because the
chain now holds the verified `sub` itself rather than a masked projection, two subjects whose masks
collide (any two opaque `sub`s sharing a first character) are two distinct `ExchangeKey`s and each
pays its own exchange - the pre-existing ADR 0031 defect of serving one caller another's cached
credential on a mask collision is closed as a side effect, held by `two_subjects_with_a_colliding_mask_pay_two_exchanges`.

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

**And the specific consequence of that deferral, stated plainly:** `CallRecord` cannot yet tell a
hop'd leg (whose executing identity is a declared service account) from a bare one (a federated
principal) on the same `impersonation-at-source` source - both record identically today, because
`executed_as` is derived from the declared posture, not from the resolved account. This is accepted
here, and only here, because the broker refuses an undeclared caller on a source that declares a map,
so **every granted leg on a declared map is a hop'd leg and "who executed" is derivable at record time
from the record's subject plus that source's own map** (with the map keyed on the full subject, that
derivation is unambiguous). The follow-up that lands the distinction - threading the resolved account
onto `CallRecord`/`Provenance` - is what this then leaves open, scoped above.

## Options considered

**(1) The declared map + hop - taken.** Shaped narrowly above. Costs a second port, a second wire
adapter, and one more settings key; buys a fixed, human-nameable, auditable service-account identity
that is stable across pool and session churn, and satisfies the acceptance text as written.

**(2) Treat "the pool subject the exchange resolves to" as the account - rejected, and real only
inside this harness.** `bq-test`'s two principals are self-signed by their own keys, so here the pool
subject genuinely IS the account to impersonate and there is no separate caller. A production caller
through the IdP (`docs/adr/0014`) has a `sub`/email that is never a GCP service-account identifier, so
this option has nothing to map from outside the test harness. Not generalizable; not taken as the
shipped mechanism, though it is exactly what `exchanged_identity.rs`
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

## Amendment: option (2)'s "nothing to map from outside the harness" is resolved - a real IdP subject IS a `SubjectKey` the map keys on

**The blocker option (2) named is gone, and the mechanism is unchanged to remove it.** The map is
already keyed on [`SubjectKey`] - the full, verified `sub`, a validated redacting newtype whose two
doors are the settings parse and transport verification. A human IdP subject (an email or an opaque
id a trusted IdP issued and leg 1 of `docs/adr/0014` verified) is a `SubjectKey` like any other: the
broker never interprets the key, only equality-compares it against the declared map, so a production
entry

> `"ada@idp.example" -> "sa-declared@account-project.iam.gserviceaccount.com"`

flows through the exact same exchange-plus-hop as every declared key. Option (2)'s rejection argued
`bq-test`'s self-signed subjects **were** the account to impersonate and a production caller had
"nothing to map from outside the test harness"; both halves of that are stale. A production caller
through the IdP has the very `sub`/email the map keyed on all along, and the accepted
cell's self-impersonation (`exchanged_identity.rs`: `CHAIN_A -> expected_a`) is only the harness's
declared fixture - a property of that cell's own map, not of `sts.rs` or the wire. The offline cell
`sts::tests::a_declared_subject_resolves_through_the_hop_to_the_declared_sa` pins the non-self shape:
an IdP-subject key on a declared map pays one exchange plus one hop and presents the declared SA's
token, never the federated one, with the config boundary refusing an empty, duplicate or over-long
declared subject before it ever reaches the broker. This amendment resolves option (2)'s
generalisability question; it moves no `docs/where-identity-is-proven.md` row, because a served
binary has still not executed as a caller yet - that stays the exchange venue's own unrun half.

**The map keys on the FULL verified subject, and a mask collision is now a refusal, not a silent
overwrite.** The impersonation map is keyed on a redacting [`SubjectKey`] that holds the full `sub`
for equality while rendering only the mask, so two distinct raw subjects - even ones, like
`alice@corp.example` and `aaron@corp.example`, whose `SubjectId` masks are identical - are two
distinct map entries. A caller whose FULL subject is absent from a declared map is refused before any
network call, whatever its mask collides with; and `WorkloadIdentityConfig::parse` refuses two
declared keys that compare equal after parsing (differing only by surrounding whitespace, which
`parse_principal_id` trims) rather than silently keeping whichever came last. The collapse that
`SubjectId`'s masking used to cause, and the escalation it enabled, are the property the reviewer's
attack cell M1 pins (`an_undeclared_caller_with_a_masked_subject_colliding_with_a_declared_one_is_refused_not_handed_the_sa`).

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
  3600-second ceiling this hop always REQUESTS. The granted credential's expiry is now read from the
  endpoint's own `expireTime` (bounded by that requested ceiling, refused if absent or longer than
  requested - so the correctness gap this record's first round called out is closed); what remains an
  optimisation rather than a correctness gap is picking the REQUESTED lifetime from the caller's
  budget, which the broker's own expiry floor already refuses on the safe side regardless.

## Second amendment, 2026-09-21: the hop is deleted with the broker, and no served path performs it

**`ImpersonateAsAccount`, its `NoImpersonation` default, and the `WorkloadIdentityBroker` they hung
off are deleted** (`docs/adr/0018`, eighth amendment), along with the cells this record cited -
`sts::tests::a_declared_subject_resolves_through_the_hop_to_the_declared_sa` and its committed claim
mutation among them. `wire::IamCredentialsOverHttp`, the one real implementor, had already gone with
the `wire` transport, so from that point the hop was reachable only from a test fake.

**What replaces the mechanism, and it is narrower.** A source declared `impersonation-at-source`
resolves a caller through `DeclaredPrincipalBroker` (`crates/sutura-exec-bigquery/src/principal.rs`),
whose declared per-source map keys on the same full [`SubjectKey`] this record argued for - so the
first amendment's resolution of option (2) survives the deletion: a real IdP subject is still the key
the map keys on, and a caller absent from it is still refused before any network call rather than
widened. What does NOT survive is *two accounts from one workload identity*: there is no second hop,
so the pool resolves the asker's assertion to whatever principal it resolves it to, and the declared
map decides only WHETHER this caller may be served here.

**Does not move `docs/where-identity-is-proven.md`'s row, in either direction.** Deleting evidence
that was only ever fake-port cells proves nothing new, and the venue that would answer leg 2 is still
what that page says it is.
