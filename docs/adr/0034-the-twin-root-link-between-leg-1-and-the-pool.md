---
title: The twin-root link between leg 1 and the pool
description: A source may declare the issuer and STS audience its identity pool trusts, the broker refuses a boot whose direct leg-one document can never satisfy that pool, and it checks the subject token's iss/aud claims before any exchange - closing, by mechanism, the gap where a document leg 1 verifies is one the pool would decline.
---

# The twin-root link between leg 1 and the pool

Status: **accepted, and INVERTED by this record's own first amendment - read that first.** The
mechanism this record decided was built and then deleted with the BigQuery HTTP transport; the
composition root now REFUSES the declaration it argues for. `github.com/telekom/sutura#817` is the
issue. Leg 1 (`who is asking`) verifies a
caller document against `security.inbound`'s issuer and audience; leg 2 (`a source executes AS the
asker`) hands that same subject token to Google STS for a workload-identity pool. Unless the pool's
trusted issuer and its STS audience are the very values leg 1 verifies, the two trust roots are
unconnected by construction: a document this deployment accepts is one the pool declines, and every
credential the exchange would mint is refused before a single row is read.

## What this decides

**A source may declare what its pool trusts, beside the audience and scope it already names.**
`sutura_config::sources::workload_identity::WorkloadIdentityConfig` carries two optional values -
`expected_issuer` (an absolute `https` URI, parsed with the inbound [`IssuerUrl`] rules) and
`expected_audience` (a provider resource, parsed with the `WifAudience` rules). Both are `Option`:
absent keeps today's bare RFC 8693 exchange for a deployment that wrote nothing, so the seam is
additive rather than a tax every existing source pays. This is the same "absent is a value" shape
`impersonate` (ADR 0032) already uses.

**The broker refuses at boot a direct leg-one document can never satisfy.** When leg 1 is
`InboundIdentity::Direct`, `build_broker` (`broker.rs`) compares each
impersonating source's declared expectations against the direct issuer and resource. They are the
very values that must equal the pool's for the SAME document to satisfy both sides; when they cannot,
the deployment refuses to start, naming the source and both pairs, rather than serve questions whose
every credential the pool would decline.

**The broker checks the subject token's claims before any exchange.** `WorkloadIdentityBroker::mint`
decodes the payload of the verified assertion (the document leg 1 already verified - the decode is
deliberately UNVERIFIED, because re-verifying it would be rebuilding leg 1) and refuses with
`ExchangeUnusable::PoolExpectation` before any round trip when its `iss`/`aud` are not the declared
pool values. This is the runtime half: a document that leg 1 accepted but that the pool would decline
never reaches the STS call.

## What this does NOT close

**Does not prove a real Google STS accepts a leg-1-verified document.** The boot refusal and the
runtime check make the two roots agree *by configuration and by claim inspection*, but nothing here
dials a live STS. The decisive check - reading a live pool's provider and testing whether STS accepts
a document leg 1 verifies - still needs grant access and a `workflow_dispatch` run, exactly as
`docs/where-identity-is-proven.md` says. This change deliberately moves no row on that page.

**Only `Direct` leg 1 is linked.** `BehindGateway` (ADR 0014) proves a fronting component's signed
assertion; its issuer and audience live on the transit proof, not on a `security.inbound` direct
block, and this boot refusal does not reach it. The runtime claim check still applies to any source
that declares expectations, whatever the inbound mode.

**The runtime check is opt-in, and a source that declares nothing is unguarded.** The claim check
only fires when a source declares BOTH `expected_issuer` and `expected_audience` (the
`PartialExpectation` variant refuses a lone half). A source that declares neither keeps the
pre-hardening behaviour - its exchange runs with no twin-root check at all - so a deployment that
means to verify the twin root must declare the pair on every impersonating source.

**The decode trusts the token's well-formedness, not its signature.** `jwt_payload` reads `iss`/`aud`
out of the payload without cryptographic verification. That is safe here because the input is the
document leg 1 already verified end to end; it is a stated limit, not a shortcut, and the reason only
`serde_json` and `base64` are promoted to always-on dependencies (the wire gate is about the outbound
network stack, not JSON).

## Why this venue, and why it stays

The refusal and the claim check are both exercised in-process against the fake `StsExchange` and a
constructed `Settings` overlay - mechanism-backed cells (`sts::tests::a_source_with_expectations_*`,
the config parse cells, and the CLI boot cell) that are red against base. This is exactly the venue
ADR 0032 uses for the hop's mechanics: necessary and not sufficient for an identity claim, which is
why `docs/where-identity-is-proven.md` is unchanged.

## First amendment, 2026-09-20: the link is gone, and the declaration is now refused at boot

**Everything below this line describes a mechanism that no longer exists.** The link it decided ran
through an RFC 8693 exchange: `WorkloadIdentityBroker` handed the caller's own verified token to
Google STS for a workload-identity pool, and `WorkloadIdentity::assertion_matches_expectations`
checked the `iss`/`aud` claims against `expected_issuer`/`expected_audience` before that round trip.
The ADBC adoption deleted both hops (`docs/adr/0018`, fifth amendment), so **no transport in this
build exchanges a subject's token against a pool** and there is nothing left for the two declared
values to be checked by.

**The consequence is the opposite of what this record decided, and it is a REFUSAL rather than an
omission.** `crates/sutura-cli/src/serve/broker.rs` refuses to boot a source that declares either
expectation, naming both keys, because a declaration nothing reads is a control that reads as being
in place - which is `sutura_config`'s own `VerificationIdentityOnASharedSource` argument applied to
this block. So an operator who followed this record gets a startup failure telling them to remove
the keys.

**What is unchanged is the PROBLEM this record names.** Leg 1 verifying a document the source's
identity provider would decline is still a real gap, and the shipping mechanism does not close it -
it sidesteps it, by not presenting the caller's credential to the data system at all. The chain is
now *leg 1 verifies the caller, this deployment maps the verified subject to a declared account, and
this deployment's own identity is authorized to become it*, which has no second trust root to be
linked to. `crates/sutura-exec-bigquery/src/lib.rs` states what that costs beside the claim.

`expected_issuer` and `expected_audience` survive in `sutura_config` as parsed, refusable values.
Whether they should be removed from the settings tree is an owner decision and is not taken here:
keeping them means a deployment that once declared them fails loudly rather than silently, which is
the better failure while the question is open.

## Second amendment, 2026-09-21: the broker the first amendment described as unreachable is deleted

The first amendment recorded that no transport exchanges a subject's token against a pool, leaving
`WorkloadIdentity::assertion_matches_expectations` and the broker's claim check with nothing to
guard. Both are now **deleted** with the exchanging broker itself (`docs/adr/0018`, eighth
amendment), and the `sts::tests::a_source_with_expectations_*` cells this record cited as its venue
went with them.

**What still holds, and it is the half that mattered:** the BOOT refusal. A source declaring
`expected_issuer`/`expected_audience` is refused at startup, in `sutura-config`, because there is no
mechanism left to honour the declaration - which is the first amendment's own decision and is held by
the config parse and the CLI boot cell, neither of which was in the deleted tree. Deleting the
runtime half removes a check that could not fire; it does not widen what a deployment may declare.

**Moves no row on `docs/where-identity-is-proven.md`,** in either direction, for the reason this
record has always given: the twin-root question is answered by a live pool, and nothing here dials
one.

## Third amendment, 2026-09-22: a subject's assertion DOES reach the pool, and both amendments above overstate that it does not

**The correction.** The first amendment says *no transport in this build exchanges a subject's token
against a pool* and the second repeats it. That was true of the tree between the fifth and sixth
amendments of `docs/adr/0018`, and it is false now. The shipped ADBC transport builds an
`external_account` credential document per request whose `credential_source` is a loopback `url`
serving the asking subject's own verified assertion, and the driver hands that to Google's token
service, which federates it against the pool `sources.<alias>.workload_identity.audience` names.
What this repository no longer contains is an exchange it performs ITSELF.

**So the chain the first amendment wrote down is also reversed.** It reads *leg 1 verifies the
caller, this deployment maps the verified subject to a declared account, and this deployment's own
identity is authorized to become it*. None of the second and third clauses holds:
`service_account_impersonation_url` is deliberately absent from the document
(`crates/sutura-exec-bigquery/src/adbc/subject/tests.rs` asserts that key is null), so nothing
impersonates a declared account, the federated credential IS the pool principal, and
`roles/iam.serviceAccountTokenCreator` on the deployment's own identity is the grant that stopped
applying. The declared map's KEYS decide whether a caller may be served; its VALUES are read by
nothing. The chain is *leg 1 verifies the caller, the deployment decides whether that caller may be
served at this source, and the declared pool resolves that subject to a principal of its own.*

**What this does NOT change: the decision, and the refusal that survived it.** The problem this
record names - leg 1 verifying a document the source's identity provider would decline - is
reopened rather than sidestepped, because the pool now sees that document. But nothing compares
`expected_issuer`/`expected_audience` against `security.inbound`, so the twin-root link is still
missing, and `crates/sutura-cli/src/serve/broker.rs` still refuses a source that declares either
key. That refusal is the right behaviour under this amendment for a sharper reason than the first
amendment had: the declaration would now describe a comparison that MATTERS and that nothing makes.

**Moves no row on `docs/where-identity-is-proven.md`,** in either direction. Nothing here dials a
pool, and the venue that would is `wired`.

**The limit on this record's own class of defect:** `0034` is not in `check-guidance`'s leg-2
exemption list, so a page here stating a registered *leg 2 is proven* wording is refused; nothing
refuses the two sentences this amendment corrects, because they overstate a NEGATIVE. No gate reads
them, and in Rust the leg-2 rule reads `///` and `//!` doc comments only.

## Fourth amendment, 2026-09-22: the declared map's VALUES decide something again

The third amendment corrected the first two about the ASSERTION and left one clause of its own
overstated. It reads *the declared map's KEYS decide whether a caller may be served; its VALUES are
read by nothing*, and that half was true of the tree it described and is false now.
`telekom/sutura#929` F3 carries the account declared beside each subject onto
`Presented::SubjectToken` and renders it as the credential document's
`service_account_impersonation_url`, so the chain is *leg 1 verifies the caller, the deployment
decides whether that caller may be served at this source, the declared pool resolves that subject to
a principal of its own, and that principal impersonates the account the deployment declared for
them.* Changing a configured target principal therefore changes which account a caller's question
executes as - which it did not for three rounds, and which the re-review called a security-critical
setting accepted and then ignored.

`roles/iam.serviceAccountTokenCreator` on the DEPLOYMENT's own identity is still the grant that
stopped applying: the second hop is authorized by `roles/iam.workloadIdentityUser` on the target
account, whose member is the pool's `principal://.../subject/<id>`. `docs/adr/0018`'s eleventh
amendment is the fuller record.

**What this does NOT change: the decision, and the refusal that survived it.** The problem this
record names is still reopened rather than closed - the pool sees the document leg 1 verified, and
nothing in this process compares its own expectations against the pool's. `expected_issuer` and
`expected_audience` are still refused at boot, and F3 narrows the arm next to them rather than
relaxing either.
