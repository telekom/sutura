---
title: Caching an exchanged credential
description: A per-process cache, keyed on the whole verified chain, in front of the one broker that performs a token exchange - the key, the TTL as the issue's three bounds with two of them enforced, what is never cached, and the limits this does not close (per-replica only, no bound on the caller's own assertion lifetime, revocation delayed by at most the configured window).
---

# Caching an exchanged credential

Status: **accepted.** `crates/sutura-domain/src/identity/credential.rs`'s own module doc named this
an architecture decision rather than an optimisation: `CredentialBroker::mint` is called once per
accepted question with no bound of its own, and caching what it minted needed a key shape and a
lifetime decided once rather than assumed at the call site. `github.com/telekom/sutura#381` is the
issue; its own comments are load-bearing and this record does not repeat their argument in full -
the correction that revocation is bounded by a token's own remaining life either way, and the
addition that an enterprise IdP's token endpoint is built for session establishment and not for the
synchronous inner loop `github.com/telekom/sutura#378`'s chain puts on it.

## What this decides

**Cache the exchanged material, not the decision to mint.** `sutura_exec_bigquery::WorkloadIdentityBroker`

- the one broker that performs a network round trip - gains an optional, per-process,
  per-`(chain, audience, scope)` map in `src/sts/cache.rs`, consulted and populated inside its own
  `mint`. Nothing outside that one file knows the cache exists; `sutura-domain` and `sutura-app` are
  untouched.

### The key: `(PrincipalChain, audience, scope)`, never less, never the whole request

**Corrected after review.** The first version of this record keyed on `Subject` alone and stated
the chain's second position (an acting agent) "is always absent today", which was false of the tree
it landed beside: `sutura_http`'s inbound gate already builds one from an RFC 8693 `act` claim
(`crates/sutura-http/src/inbound/token.rs`) and the audit record already writes it
(`crates/sutura-runtime/src/audit.rs`). A subject-only key would serve one caller's credential to
another presenting a different assertion under a different attribution, the moment a workload's
attribute mapping reads the `act` claim - which is exactly the cross-subject leak this whole cache
exists to make unrepresentable, reappearing one layer down. The key is the WHOLE chain
`RequestContext::chain()` carries at the moment `mint` is called, paired with the literal input to
the exchange call - not the `SourceSet` a request happened to name (two requests over different
subsets of sources must still hit per source), and not the `SourceName` either, though a source's
audience and scope are declared once at startup and never change while a process runs: the issue's
own wording asks for the literal exchange input, and keying on it directly is the shape that stays
correct if that assumption ever stops holding.
`the_same_subject_through_a_different_acting_agent_pays_its_own_round_trip`
(`crates/sutura-exec-bigquery/src/sts/cache.rs`) is the permanent regression.

**The residual, stated rather than assumed away.** A verified `PrincipalChain` is exactly what
`sutura_domain::identity` parses from a token's `sub` and `act` claims - a claim a provider maps
from something else (group membership, a custom attribute a workload-identity pool's attribute
condition reads) never reaches this chain, because the domain does not verify or carry it. Two
requests identical in every claim this key can see, differing only in a claim the domain never
parses, would still share an entry - a limit of what leg 1 verifies, not something this cache could
key around. Separately, `RequestContext::with_assertion(PrincipalChain::of(Subject::TheDeploymentItself), ..)`
is `pub` and would collapse every caller onto one key if anything built one; nothing shipped does -
both transports' `established()` use `RequestContext::of`, which carries no assertion, so that chain
can never reach an impersonating source at all.

**Unrepresentable from outside, by construction rather than by discipline.** The key type has no
`pub` constructor anywhere - not `pub(crate)`, not exported - because the whole cache is private to
`sts` and reached only through `mint`'s own arguments. There is no code path from a caller-supplied
value to a cache entry, which is the same shape `sutura_domain::identity::PrincipalChain` uses for
"a caller cannot state its own identity": not a check that runs, an absence of the door.

### The lifetime: the issue's three bounds, two of them enforced

The issue's body numbers three bounds; this record keeps that numbering rather than inventing its
own, because renumbering is exactly how "the second" and "the third" drift apart in review:

1. **the minted credential's own `not_after`, minus the same FLOOR `docs/adr/0008` part 6 already
   refuses inside** (`WorkloadIdentityBroker::with_floor`). **Enforced** - a cache that ignored the
   floor would re-introduce exactly the bug the floor exists for: the first caller gets refused for
   having too little life left, and the second gets the same credential anyway because it was
   already sitting in the map.
2. **the caller's own assertion expiry.** **Not enforced.** The shipped broker never parses the
   caller's assertion beyond handing it to the exchange as opaque material, so this record cannot
   fold it into `min(...)` today. Stated as a limit below, not silently dropped -
   [what this explicitly leaves for later](#what-this-explicitly-leaves-for-later) names it.
3. **`security.credential_cache.window_seconds`**, an operator's own ceiling. **Enforced** -
   `CacheWindow` in `sutura_config::identity_cache` documents itself as a ceiling and never a grant:
   the fold can only shorten how long an entry is served, never lengthen what the exchange actually
   minted.

An entry is served only until the EARLIEST of bounds 1 and 3 - `min`, not a three-way fold, because
bound 2 is absent from the computation entirely rather than defaulted to "no limit".

### What is never cached, and why each is an absence rather than a check

- **A refusal, or an `Err`.** The cache's `put` is called from exactly one line, immediately after a
  successful exchange. There is no second call site, so "never cache a negative result" is a
  property of the code's shape rather than a flag that could default the wrong way.
- **A shared leg.** `Presented::SharedServiceUser` carries a declared witness, not material a round
  trip produced - there is nothing to save a call on, and caching it would only widen the map for no
  saving.
- **A result, a plan, or anything not keyed by who asked.** Unchanged: the invariants table's "No
  result cache" row stands, because this is a *credential* cache and the distinction between the two
  is the whole reason one is a bounded engineering decision and the other is not on the table -
  `docs/adr/0008`'s own credential module doc, restated in the issue's comments: the source is the
  backstop for a credential and re-checks it on every query; there is no backstop for a data path.

## What this does NOT close

**Per-replica only.** The cache is an in-process `HashMap` behind a `parking_lot::Mutex`. A
multi-pod deployment has one cache per process, with a hit rate that falls as replica count rises
and behaviour that differs per pod. This is the issue's own conclusion, not a shortcoming: a shared
credential cache across pods is a larger blast radius for the same saving, and nothing here proposes
one.

**A revoked-at-the-IdP token stays usable until the sooner of the window or the token's own `exp`.**
Bounded by `security.credential_cache.window_seconds`, inside a token lifetime that per-request
minting would have handed out anyway. `docs/where-identity-is-proven.md` states the number as the
limit of what this venue proves; it is not zero, and it is not unbounded.

**The floor's own limit is inherited, not repeated.** Whether the floor a broker is configured with
actually covers the per-answer deadline (`docs/adr/0029`'s `Deadline`, the request-timeout budget) is
set by composition - `sutura-serve` wires `with_floor(request_timeout.seconds())`, which happens to
exceed the deadline's own budget by the reply margin - and is not itself asserted by a type or a
test anywhere in this tree. A served credential, cached or freshly minted, can therefore still expire
mid-question if a future composition wires a smaller floor than its own deadline. This record does
not add that assertion; a cached entry clears exactly the same floor a fresh mint is held to, and
nothing more or less.

**The caller's own assertion lifetime is not a bound here.** See above.

**No measurement backs the saving.** `github.com/telekom/sutura#376` - a real token exchange against
a real authorization server - has not landed. `security.credential_cache.enabled` defaults to
`false` for exactly that reason: nothing here claims a number nobody has taken.

## What this explicitly leaves for later

- **A gate proving the answer path caches nothing** (the issue's own added acceptance item) is not
  built by this record. The invariant is already true today, with zero caches anywhere on that path,
  and this change does not touch `sutura-app` or `sutura-domain` at all - building a general
  no-data-cache scanner as a rider on a credential-cache change is the scope creep the line-count
  cap exists to catch. Tracked as a fast-follow rather than folded in here.
- **A chain's whole hop sequence in the key**, once `github.com/telekom/sutura#378` lands multi-hop
  exchanges. Today's key already carries the literal `(audience, scope)` pair rather than a source
  name, which is the shape a hop sequence extends rather than replaces.
- **The caller-assertion-expiry bound**, once something in this crate has reason to parse a bearer
  assertion's own claims.

## Not taken

**A shared cache (Redis, a sidecar, anything outside the process).** Priced explicitly in the
issue body and not reopened here: a shared credential store is a single larger blast radius for
every subject a deployment serves, in exchange for a hit rate a per-process cache mostly already
gets under ordinary session-length traffic.

**Caching inside `StsExchange` instead of `CredentialBroker::mint`.** The exchange port
(`audience, scope, subject_token: &Secret`) carries no verified `Subject` at all - only the caller's
raw assertion bytes - so a cache at that seam could key only on the assertion's own content, which
conflates "what the caller presented" with "who the domain has already verified them to be" and
loses the one value (`RequestContext::chain().subject()`) the rest of this record's key depends on.
`mint` is the one place both exist together.

## Amendment, 2026-09-16: `sutura-cli` wires the floor now

*The floor's own limit is inherited, not repeated* said "`sutura-serve` wires
`with_floor(request_timeout.seconds())`". `sutura-serve` folded into `sutura-cli`'s `serve` module
(`github.com/telekom/sutura#685` step 2); the call site is `broker.rs`
now, and the claim about what it does and does not assert is unaffected.
