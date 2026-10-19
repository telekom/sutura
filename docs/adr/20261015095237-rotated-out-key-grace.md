---
title: A key rotated out of the key set verifies for a bounded grace behind a gateway
description: Behind a gateway, the key set a rotation replaced keeps verifying for the assertion lifetime ceiling plus the clock-skew leeway, so a key set file that holds only the current key can rotate without refusing the assertions the old key signed. The direct mode keeps today's revocation bound.
---

# A key rotated out of the key set verifies for a bounded grace behind a gateway

Status: **accepted**. It amends `docs/adr/0014`, whose eighth amendment points here.

## The question

The key set file is re-read on every look, and a key that leaves it stops verifying within
`MAX_KEY_SET_AGE` plus one read. That is the revocation bound `docs/adr/0014` decides. A writer that
wants an overlap has to publish the new key beside the old one first.

Certificate tooling that writes one key per Secret cannot do that: with a rotation policy that makes
a new key, the file holds the new key only. The gateway signs with the new key from then on, but
every assertion that the old key signed and that has not yet expired is refused. How does such a
deployment rotate without that outage?

## Decision

**A rotation keeps the key set it replaced, for a bounded grace.** When a look adopts a new key set,
the previous set becomes the retired set. A key id that the current set does not hold is looked up
in the retired set while the grace is open. A key id the current set holds is always served from
the current set.

**The grace is the assertion lifetime ceiling plus the clock-skew leeway**:
`security.inbound.transit_max_lifetime_seconds` plus the validator's 30 s leeway. An assertion the
old key signed before this deployment saw the rotation has `iat` at or before the rotation, so the
validator accepts it until at most `iat` + ceiling + leeway. The grace starts when this deployment
adopts the new set, so it covers every such assertion while the gateway's clock is not ahead of this
deployment's. The validator accepts an `iat` up to 30 s ahead, so a gateway clock up to 30 s ahead
can sign an old-key assertion that the validator accepts after the grace closes: the grace refuses
its last 30 s or less. That fails closed. A shorter grace would refuse assertions the validator
still accepts. A longer one would keep a key that verifies nothing new.

**Only `behind-gateway` has a grace.** That mode has a lifetime ceiling. `direct` has none: an
access token's lifetime is the issuer's choice, so no grace can be derived, and an issuer's published
key set carries its own overlap. In `direct` the revocation bound is unchanged.

**One retired set, the immediately previous one.** A second rotation inside the grace replaces the
retired set with the set it just replaced, and the older set stops verifying at once.

**The grace clock is not extended by a look that adopts nothing.** An unreadable file, a document
that does not parse and a document with no key of the pinned family leave the retired set as it
is, and its grace keeps running. Reading the set in use again after such a look is no rotation,
so it neither retires that set nor ends the grace. A broken new file cannot keep an old key alive.

**There is no key to configure.** The grace is derived from values the deployment already declares.
A separate key could only be set longer than the ceiling, which keeps a key that can verify no
assertion the ceiling admits.

## The revocation bound, restated

| Mode             | A key removed from the key set stops verifying within                                                                 |
| ---------------- | --------------------------------------------------------------------------------------------------------------------- |
| `direct`         | `MAX_KEY_SET_AGE` (60 s) plus one read - unchanged                                                                    |
| `behind-gateway` | 60 s plus one read plus `transit_max_lifetime_seconds` plus 30 s; 210 s plus one read at the default ceiling of 120 s |

Both rows hold while refresh works, as `docs/adr/0014`'s second amendment states: a source that stays
unreadable keeps the keys in use for as long as it stays that way.

## What this does not cover

- **A deliberate revocation has no fast path.** Behind a gateway, removing a compromised key from the
  file leaves it verifying for the whole grace. There is no setting that revokes at once. An operator
  who must revoke at once restarts the deployment with the new file: the boot read adopts it with no
  retired set.
- **Two rotations inside one grace.** The oldest signer is refused from the second rotation on, even
  if one of its assertions has not expired.
- **A gateway that signs with the old key after this deployment rotated.** The grace starts when
  this deployment adopts the new file. If the gateway changes its signer later, an old-key
  assertion it signs after that verifies only if it expires inside the grace. The lag between the
  two is outside this deployment's control.
- **The grace is per process.** It starts when each replica adopts the new file, so two replicas
  can close it at different times.
