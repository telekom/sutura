---
title: The twin-root link between leg 1 and the pool
description: A source may declare the issuer and STS audience its identity pool trusts, the broker refuses a boot whose direct leg-one document can never satisfy that pool, and it checks the subject token's iss/aud claims before any exchange - closing, by mechanism, the gap where a document leg 1 verifies is one the pool would decline.
---

# The twin-root link between leg 1 and the pool

Status: **accepted.** `github.com/telekom/sutura#817` is the issue. Leg 1 (`who is asking`) verifies a
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
`InboundIdentity::Direct`, `build_broker` (`crates/sutura-cli/src/serve/broker.rs`) compares each
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
