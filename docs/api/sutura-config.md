<!-- GENERATED FILE - do not edit.
     Written by docs/.tools/rustdoc_to_markdown.py from rustdoc JSON.
     Edit the doc comments in the crate source instead, then regenerate.
     The commands are on the API reference landing page. -->

# sutura-config

The public API of `sutura-config`, rendered from rustdoc JSON.

The service's configuration: layered sources in, one typed tree out, and a refusal instead of
a permissive default.

# What a caller does

Read the environment, then load. Two steps rather than one, because the environment decides
which file is layered and which defaults apply, so it has to be known first.

```
use sutura_config::{Environment, Settings, Sources};

// A test or an example supplies the environment directly; the binary reads it from the
// process with `sutura_config::environment_from_process`.
let settings = Settings::load(&Sources::defaults(Environment::Development))?;
assert!(settings.server().bind().is_loopback());
# Ok::<(), sutura_config::SettingsLoadError>(())
```

# Precedence

Later beats earlier:

1. the defaults embedded in this crate (`Settings` can always be built from them alone);
2. `<dir>/base.yaml`, if a directory was given and the file is there;
3. `<dir>/<environment>.yaml`, likewise;
4. environment variables - `SUTURA__SERVER__PORT` sets `server.port`.

The environment itself is chosen by `SUTURA_ENVIRONMENT` and by nothing else. It is
deliberately *not* a configuration key: it selects which file is layered, so a file that could
change it would be self-referential. Both `environment:` in a file and `SUTURA__ENVIRONMENT` in
the shell are therefore unknown-field errors rather than settings that quietly do nothing.

Every layer is checked with `deny_unknown_fields`, at every depth. A misspelled key is an error
naming the key, not an override that silently did not happen.

# There is no per-caller ACCESS, and this crate says so out loud

sutura has a request context and a credential broker now, and a deployment that declares
`security.inbound` establishes who is asking - so what is missing is narrower than it was and it is
the part that matters: **no adapter in this build can carry a per-subject credential.** Every
question executes with a credential a broker minted, and what that credential says is *the identity
this process holds for that source*. `AGENTS.md` records which half is mechanised, and
`examples/README.md` explains why single-player makes "every query runs as the calling
principal" trivially true and worth nothing.

That is a property of the runtime, so it is a property of every deployment this crate
configures. An `AccessToken` authenticates *the deployment*: a caller
who presents it proves they hold a secret an operator configured, and nothing more. It does not
say which caller, it cannot be scoped to a subset of the catalog, it does not reach the data
system, and every query still runs with whatever access the process already had.
`SecuritySettings::describes_identity` is the
function that answers this, it always answers `false`, and the startup log prints that answer
on every boot so an operator cannot deploy this believing otherwise.

Rate limiting is not authentication either - see `limits` for what it does and does not buy.

# What refuses to start

`NotFitToServe` is the whole list, and each variant is a refusal rather than a warning. A
warning is read by whoever is looking at the log in the format the collector was configured
for; a process that does not start is read by everybody.

- A bind address other hosts can reach with `security.tls_termination: none`. In *every*
  environment, including a laptop. **The bind itself is not refused** - an ingress controller or
  a sidecar terminating TLS in front of a plaintext pod-local listener is the normal
  arrangement - what is refused is not saying which of those it is, because that is what decides
  how far the bearer token travels in cleartext.
- No `security.access_token`, in production or on a non-loopback bind.
- `rate_limit.enabled: false` in production.
- `rate_limit.client_address: forwarded` with an empty `rate_limit.trusted_proxies`, or a
  non-empty list that nothing reads.
- `security.tls_termination: in-process` without a certificate and key, or in a binary built
  without the `tls` feature; or a certificate and key no declaration would ever read.
- `server.port: 0` in production.
- A configured source with no `security.identity`. The mode has no default and no derivation - see
  `security::DeploymentIdentity`, which explains why no combination of source
  postures may answer it on the operator's behalf.
- A `shared-service-user` source in `multi-user` mode with no `acknowledged_because` on that
  source's own entry. Per source, because an acknowledgement inherited from a neighbour is how a
  source nobody thought about gets served to everybody as somebody else's identity.

The checks read the *loaded* values, not any one file, because the variable layer is applied
last: a check against `production.yaml` would be checking something the process is not running
on.

**Two refusals a deployment can still hit are NOT in this crate, and the split follows what each
half can see.** Whether the *linked adapter* can carry a per-subject credential at all is a property
of the build, and whether the *bundle* declares an anchor is a property of the catalog; this crate
sees neither, so both are startup refusals in the composition root. `sources` says so where the
declarations are.

**A value out of range is a different refusal, through a different type, and one of them reads the
machine.** `NotFitToServe` is about a *combination* of settings that are each individually legal;
a single value the type will not accept is a `SettingsError` inside `SettingsLoadError`, so it
refuses to start too and is not in that list. The one worth naming here is
`runtime.working_set_max_bytes`: it is checked against the memory this process can actually reach -
a cgroup limit, or the machine - and refuses above it, because shipped profiles compile
`panic = "abort"` and a ceiling over what is reachable is the unbounded case with a number written
next to it. **On a platform that will not report that number, notably macOS, no check is made**,
and `WorkingSetCeiling::checked_against` is what lets the startup log say which of the two
happened rather than implying the check was run.

## `use ApiSettings`

Whether the generated documentation surface is served.

## `use CatalogKind`

Which adapter the catalog configuration names, and therefore which one opens it.

**A closed set of typed declarations, `crate::sources::SourceKind`'s shape on the metadata
side.** A DATA source's adapter is chosen by `SourceKind` and dispatched by the composition
root's exhaustive match with no wildcard arm; a METADATA source has exactly the same need, and
until this type existed the settings tree carried a directory and a version and no word an
operator could write to say *read the model from somewhere else* - so a second catalog kind
could merge complete and silently remain unreachable from any binary.

Two variants today. `Self::Datahub` says which and why, the way `SourceKind::BigQuery` does for
data systems: the vocabulary is the vocabulary of adapters this repository has, and an adapter
that exists in a record rather than in a linked crate is still a word an operator might write.

## `use CatalogSettings`

Where the definitions and the data are, and what the resulting bundle is called.

Each catalog carries a declared NAME, the way a `sources:` entry carries an alias: the
contribution manifest keys on it, and a reviewer reads it in a settings file. It is named by
code and not by index so that reordering the list does not silently rename a contributor.

## `use Catalogs`

The catalogs a deployment declares, in declaration order.

**A non-empty, ordered collection, and the empty member is unrepresentable.** Composition -
the point of having N - is the metadata assembler in `sutura-app`; this type is the declared
configuration it is handed. Order is declaration order, which is content order: the contribution
manifest is a `BTreeMap` keyed on each entry's `CatalogSettings::name`, so this ordering is
what a reviewer reads and manifest determinism does not depend on it surviving a rename.

## `use InvalidCatalogSettings`

Why a catalog configuration is not usable.

## `use UnknownCatalogKind`

The configured word did not name a kind of catalog this build has.

## `use StaticCredentialBroker`

Mints from what an operator declared, and nothing else.

Holds one entry per source declared `shared-service-user`, carrying that source's acknowledgement
witness. A source declared `impersonation-at-source` is deliberately **absent**: there is no
static credential that could execute as an asking subject, and an entry that pretended otherwise
would be the fallback this port exists to remove.

**Built from the DECLARATION and never from an adapter.** `docs/adr/0008` part 4 requires that a
broker produce the shared shape only for a source configured shared, and the check that catches a
broker which did not is the adapter's own exhaustive match on what it received. Those two are only
an independent pair if they read different things: this reads the settings tree, and the adapter
holds what the composition root handed it. A broker that read the posture off the adapter would
make that check compare a value against itself.

## `use StaticCredentialsUnusable`

A defect in this broker itself, which no configuration reaches.

**Stated rather than unwrapped, and the reason is worth a line.** The one thing minting can fail
on is `LegCredentials::minted` refusing a set that does not cover the sources it was asked about -
and this broker builds its map from that same set, one entry per source it was asked about, so the
check cannot fire here. It is still answered for rather than unwrapped: `unwrap_used` is denied,
and a panic on this path would be process death under `panic = "abort"` for a case a type already
describes. Nothing in this crate's suite can provoke it, and that is said here rather than left
for a reader to assume it is covered.

## `use Environment`

A deployment kind.

Ordered from most permissive to least, which is also the order the refusals in
`crate::Settings` tighten in.

## `use UnknownEnvironment`

The string was not one of the three.

Carries what it found, because the whole point of the type is that a typo fails loudly, and
a failure that does not quote the typo makes the operator guess which of the three spellings
they got wrong.

## `use InboundIdentity`

How the identity of a caller reaches this deployment. Printed at startup, per deployment.

A closed enum with a required key, in the shape
`TlsTermination` already uses here - and for the same reason
`Environment` is a parsed enum rather than a string with a fallback: the value that decides a
posture must not be satisfiable by silence.

## `use InvalidAlgorithms`

Why a pinned algorithm list is not one.

## `use InvalidInboundValue`

Why a value in an inbound-identity declaration is not one.

One error for every newtype in this module, because they are one parse with different
vocabularies: the same five things can be wrong about each of them, and three hand-written copies
of that list is three things to keep in step.

**No variant quotes the whole value back.** A resource identifier is not a credential, but an
issuer URL and a header name are both deployment topology, and this crate's own rule is that an
error carries the typed context and not a rendering of the input. A position is what an operator
needs to find the character; the character itself is often the one that draws nothing.

## `use IssuerUrl`

Where the tokens this deployment accepts are minted.

Compared against the `iss` claim, byte for byte, for the reason `ResourceIdentifier` is: an
issuer is a configured string on both sides and normalising ours would make it disagree with
theirs.

## `use KeyFamily`

Which kind of key verifies a `SigningAlgorithm`.

Here rather than left implicit because **a pinned set spanning two families is a set that can
verify nothing**, and finding that out from a `401` is expensive. One key set holds keys of
whatever kinds the issuer publishes; one *token* is verified by one key with one algorithm, and
the validator this feeds refuses a permitted-algorithm list whose family disagrees with the key
it looked up. So the mixed list is refused at startup instead - see `PinnedAlgorithms::parse`.

## `use KeySetFile`

Where the signing keys are read from.

**A file and not a URL, and that gap is named rather than left to be discovered.** A JWKS
endpoint needs an outbound HTTP client, which is a supply-chain change with its own review and
its own failure mode - `docs/adr/0014` says plainly that the authorization server becomes a hard
runtime dependency and that an outage there must stay distinguishable from a dead data system.
None of that is built. What is built is the *rotation* mechanism: the key set is cached, refetched
when a key id is not in it, and that refetch is rate limited - and every one of those properties
is the same whether the source is a file a sidecar rewrites or an endpoint. See
`sutura_http::inbound::keys`.

## `use PinnedAlgorithms`

The algorithms this deployment will accept, all of one family and at least one.

Non-emptiness and single-family-ness are both structural: the first link lives in its own field,
which is the shape `sutura_domain::identity::ActorChain` already uses for the same reason - there
is no state of this type that means "nothing is permitted", so nothing downstream has to check.

## `use ProofHeader`

The header a transit proof arrives in.

Lower case, because that is what HTTP/2 puts on the wire and what a header map is keyed on here -
so folding it at construction is what makes a configured `X-Transit` and an arriving `x-transit`
the same header rather than a lookup that silently misses.

## `use ProofLifetime`

The longest lifetime a transit proof may declare.

**A server-chosen ceiling on somebody else's token**, and the reason it exists is that
`docs/adr/0014` calls a transit proof *short-lived* while the lifetime is entirely the fronting
component's to choose. Review demonstrated a proof with `exp` ten years out being accepted, and
accepted again on a replay of the identical token. So the deployment declares what it will call
short-lived, and a proof claiming more is refused.

Bounded above because a ceiling of a year is not a ceiling. Bounded below by one second, because a
zero would refuse every proof - a way of turning the mode off that reads like a tuning value.

## `use RequiredTokenType`

Which class of token this deployment will accept, out of the `typ` header.

**Two variants because the check has to be switchable and must not be switchable by silence.** The
finding it answers is cross-JWT substitution: without it, any JWT the issuer signed with this
audience verifies, an OIDC ID token included whenever the resource identifier equals the client id.
So `Self::Exactly` is the default in the `direct` mode - RFC 9068's `at+jwt` - and turning it off
is a value an operator writes, `any`, which the startup log prints at `WARN`.

There is no `Option<TokenType>` here, for the reason `docs/adr/0014` gives about `mode`: an absent
value reads as "not configured yet" at every call site, and the one thing that has to be legible is
whether a deployment decided to accept every class of token.

## `use ResourceIdentifier`

What this deployment calls itself when it validates an audience.

**This is the security decision in `docs/adr/0014` given a type.** A token is accepted only if
its audience matches this value. A client may also *ask* its authorization server for a token
scoped to this resource - RFC 8707's resource indicator - and that is welcome and is an
optimisation: it makes the token narrower before it ever arrives. It is never what makes the
token safe. The check is ours, it is unconditional, and it is not skippable when the client sent
no indicator.

## `use SigningAlgorithm`

A signature algorithm this deployment will accept.

**No `none` and no `HS*`, and their absence is the enforcement.** Algorithm confusion is the
classic direct-validation defect and it is silent when it works: a token signed with `HS256`
using the issuer's *public* key as the HMAC secret verifies, if the validator will accept a
symmetric algorithm. There is no variant here that could.

## `use TokenLocation`

Where the token being validated arrives.

Two variants because the two modes put it in two places, and neither is a preference: an OAuth
2.1 client puts its access token in `Authorization: Bearer` and has no option to do otherwise,
while a fronting component sets a header of its own and would collide with the deployment bearer
token if it used that one. `ProofHeader::parse` refuses `authorization` for exactly that reason.

## `use TokenRequirement`

The whole validation this deployment performs, borrowed out of whichever mode is configured.

**A view and not a second configuration surface.** There is no constructor: the only way to one
of these is `InboundIdentity::requirement`, so the validator cannot be handed a requirement
that does not correspond to a declaration an operator wrote and this crate refused or accepted.

## `use TokenType`

A `typ` header value this deployment will accept on a token.

**The type that closes cross-JWT substitution**, which is the finding it exists for: without a
`typ` check, *any* JWT the issuer signed with this audience verifies - and an OIDC ID token has the
same issuer and, whenever the resource identifier equals the client id, the same audience. That is
the ordinary identity-provider arrangement, so the substitution is not exotic. A verified ID token
would establish a caller from a document minted to describe a login rather than to authorize an API
call.

RFC 9068 section 4 requires `at+jwt` for the JWT access-token profile. RFC 8725 section 3.12 is the
wider rule and the reason this is configurable rather than hard-coded: an issuer using another
profile still has to give a deployment *some* way to distinguish token classes mechanically, and
which way that is is the deployment's fact rather than ours.

# What normalisation happens, and why exactly this much

Case is folded and a leading `application/` is stripped, both at construction. RFC 7515 section
4.1.9 says `typ` is a media type and that the `application/` prefix **may be omitted**, so
`at+jwt`, `AT+JWT` and `application/at+jwt` are three spellings of one value - and a comparison
treating them as three would refuse tokens that are correct. This is deliberately the opposite
decision from `ResourceIdentifier`, where nothing is normalised: an audience is an opaque string
an operator and an issuer configured identically, and a media type is a value a registry defines.

## `use TransitProof`

What a fronting component has to prove on every request.

Every field is a validation input, and that is the point of the type: there is no field here that
holds an asserted identity. See this module's documentation for why.

## `use InvalidQuota`

Why a pair of numbers is not a quota.

## `use Quota`

A sustained rate and the burst allowed above it.

Both non-zero: a quota of zero requests per second is a closed door, which is what
`enabled: false` says properly, and a zero burst is a limiter that rejects the first request
of every idle period.

## `use RateLimitSettings`

Both tiers, the switch, and what a bucket is keyed on.

The switch is separate from the numbers on purpose. A deployment that turns limiting off is
making a decision, and it should be one word in a file rather than a quota set so high it
never fires - which reads as a configured limit and is not one.

**The switch itself has no fixed default; it follows `crate::Environment`.** Off on a laptop,
because a limiter that fires while somebody is iterating is a bug report about sutura that is
really a bug report about the tier; on in production, because that is the deployment an
unbounded caller costs something. The same shape as `telemetry.format` and `api.docs`, including
the recorded flag - see `Self::enabled_default_for`.

**Not `Copy`, and that is the trusted-proxy list.** It is a `Vec`, so this group is cloned
rather than copied and the accessors borrow. Every call site is inside the assembled router,
once, at startup.

## `use CatalogProse`

Whether the catalog's own prose is quoted into the prompt.

The word an operator writes. The type that does the work is
`sutura_app::prompt::CatalogProse`, and the split is the one
`LogFilter` already uses: this crate parses what was written down,
and the crate that acts on it owns the type that acts.

## `use InstructionsFile`

Where the operator's own prompt text lives.

A newtype rather than a `PathBuf` so the one thing that can be wrong about it is wrong in one
place. The field is private and `Self::parse` is the only way in.

## `use InvalidPromptSettings`

Why the prompt configuration is not usable.

## `use PromptSettings`

Everything that goes into the prompt beyond the pinned bundle and the tool list.

## `use UnknownCatalogProse`

The word was neither spelling.

## `use Cidr`

An address or a block of them, as an operator writes it.

`10.0.0.0/8` or a bare `10.0.0.7`, in either address family. A bare address is a block whose
prefix covers every bit, so there is one shape to match against rather than two.

## `use ClientAddressSource`

Where the address a request is counted against comes from.

## `use InvalidTrustedProxy`

Why a string is not an address block.

## `use TrustedProxies`

The peers whose forwarded header this service will believe.

**Empty by default, and the emptiness is the safe posture rather than an unset value.** With
nothing in the list the peer address is the key, which is correct for a service with no proxy
in front. It becomes non-empty only when an operator names the hop.

## `use UnknownClientAddressSource`

The configured value did not name a source.

## `use AdmissionTimeout`

How long a question may wait for one of those slots.

Bounded because the alternative is a queue nothing empties. Shorter than the request timeout by
default, on purpose: a caller who has waited five seconds for a slot is better served by a
`503` they can retry than by a `408` twenty-five seconds later that says the same thing less
clearly.

## `use EngineWorkers`

How many threads the in-process engine's own runtime gets.

Resolved to a number at load time rather than kept as "whatever the machine has", so the value
in the startup log is the value in effect. An absent key follows
`std::thread::available_parallelism`; a present one wins, which is what a container with a CPU
quota needs - `available_parallelism` reports what the kernel exposes, and on most container
runtimes that is the host's core count rather than the cgroup's share.

## `use QueryConcurrency`

How many questions may be executing at once.

Not how many may be *in flight*: a request waiting for a slot, parsing a body or writing a
response is not counted. This is the number of questions holding a blocking-pool thread and a
data system, which is the resource that runs out.

## `use RuntimeSettings`

Everything about how much runs at once and how the process stops.

## `use ShutdownGrace`

How long stopping may take, once stopping has been asked for.

Chosen against the deadline on the other side rather than as a round number. An orchestrator
sends a termination signal and starts a kill timer - the usual window is thirty seconds - and a
process still running when that expires is killed mid-answer, so whatever it would have done on
the way out does not happen. Fifteen seconds leaves room for the exit itself.

It bounds the *whole* of stopping and not the connection drain alone. The drain gets the budget
first; what is left of it is what the runtime will wait for a blocking task it cannot cancel.
See `sutura_runtime::Shutdown::remaining_grace`.

## `use WorkingSetCeiling`

How many bytes the engine's operators may reserve at once, across the whole process.

**This is the bound that did not exist, and its absence was process death.** Nothing constructed
a `RuntimeEnv`, so the engine installed its unbounded memory pool: a hash join or an aggregate
wide enough to outgrow the machine allocated until the allocator failed, and shipped profiles
compile `panic = "abort"`, so that is not an error for the caller who asked - it is the process
ending for every caller in flight. A bounded pool turns it into a reservation that fails, which
leaves as `RefusalReason::ResourcesExhausted`.

# What it counts, and what it does not

The pool counts what the engine's own operators reserve - a hash-join build side, aggregate
state, a sort - and nothing else. **Not** what a driver buffers before conversion, **not**
`collect()` materialising every batch, **not** the row set built while a result is converted into
domain rows. So this is not a bound on the process's memory and must not be read as one: a
question large enough to end the process on one of those paths still ends it. The bound that
reaches those is a byte budget applied as rows are converted, which
`docs/adr/0009-the-plan-from-one-source-to-many.md` puts with the execution boundary rather than
here.

# Global, and no per-source override

There is one combiner and one working set, so a per-source ceiling would be a number with nothing
to bound - and 0009 decides that a source declaration carrying one is **refused at parse rather
than ignored**, because a setting that silently does nothing is worse than a missing one.
`deny_unknown_fields` on every on-disk shape is the mechanism, and the test that provokes it is in
`crate::settings`. **The deadline is the bound that takes a per-source override; this one does
not**, and the two are decided separately on purpose.

# Never spill

Decided rather than defaulted, and the second reason is what settles it. A refusal the caller sees
beats a degraded answer it cannot; and spilling writes the *asking subject's rows* to the pod's
local disk, an ungoverned data-at-rest surface, on the one path whose whole purpose is that a
query runs as the person who asked. So no spill directory and no disk sizing - the adapter builds
its runtime with temporary files disabled, and `sutura_exec_datafusion::WorkingSet` is where that
is written down.

# A provisional number

`Self::DEFAULT_BYTES` is a gibibyte and nobody has measured it. It is a starting point recorded
as one, not a finding.

## `use available_memory_bytes`

How many bytes this process can actually reach, when the platform will say.

**Three sources, smallest wins**, because they answer three different questions and the binding
one is whichever is tightest:

1. `/sys/fs/cgroup/memory.max` - the cgroup v2 limit, and the number that matters in a container.
2. `/sys/fs/cgroup/memory/memory.limit_in_bytes` - the same under cgroup v1, where "unlimited" is
   a sentinel near `u64::MAX` rather than a word, which is why taking the minimum with the
   machine total is what disarms it rather than a comparison against the sentinel.
3. `MemTotal` in `/proc/meminfo` - the machine, for a process with no cgroup limit.

**`None` on any platform that has none of these, and that is a limit on the claim rather than a
fallback.** macOS is such a platform: nothing here reads `sysctl`, so a laptop makes no boot check
at all and an over-configured ceiling there starts and dies later. The shipped artifacts are Linux,
which is where the check has to hold - and `WorkingSetCeiling::checked_against` is what lets the
startup log say which of the two happened rather than implying the check was made.

It reads files and cannot fail: an unreadable or unparsable source contributes nothing rather than
refusing to start, because the expensive failure here is a deployment that will not boot on a
kernel laid out differently, and the cheap one is a boot check that did not run and said so.

## `use AccessToken`

A pre-shared secret a caller presents to reach the service.

The configured token is reduced to its SHA-256 digest at parse and nothing else is retained, so
the whole settings tree can be written to the startup log with `Debug` and the token cannot
come out with it. `Debug` prints a placeholder for the same reason `Secret`'s does.

**Not comparable with `==`.** `AccessToken` implements no `PartialEq`: a derived comparison on
credential material returns on the first differing byte, which is a timing oracle at whatever
call site adds it. The comparison lives here instead, once, as
`AccessToken::matches_in_constant_time`.

## `use DeploymentIdentity`

Which kind of deployment this is, and therefore where a shared source's acknowledgement may come
from.

**Two modes that differ in kind rather than in degree, and the deployment DECLARES which it is.**

*Single-user* means credentials are static configuration: one user, one host, not multi-tenant.
There is no per-request identity to establish, so a shared source is correct for **everything** -
the one user reads all, by design, and the configured credential is that user's own.
`examples/single-player` is this, and it is a first-class deployment rather than a degraded one.

*Multi-user* means the caller's identity arrives per request. Shared sources are still permitted,
and that is the whole difficulty: the deployment has to say so **per source**, on purpose.

# It is declared and never derived, and the derivation that was on offer is unsound

The tempting derivation is "every source shared means single-user, any source impersonating means
multi-user". It fails in exactly the configuration that most needs the check: a genuinely
multi-tenant deployment whose sources are *all* shared derives to single-user, and the
acknowledgement is required in multi-user mode only - so the derivation would exempt from the
acknowledgement the one deployment where every caller reads every source as somebody else's
identity. The failure is silent, it is one user's data served to another, and it arrives by leaving
a field out.

So there is **no `Default`**, no derivation, and a deployment that configures a source without
declaring the mode does not boot -
`NotFitToServe::DeploymentIdentityUndeclared`.
The refusal is keyed on a source being configured rather than raised unconditionally, and that is
not a softening: a deployment with no source configured cannot answer anything, and the composition
root refuses it on the catalog naming a source with no declaration - so every deployment that can
serve a question has to declare the mode.

# What flipping the mode does

It re-evaluates every source. A single-user deployment legitimately holds every source under one
static credential; the same file in multi-user mode serves every one of those sources to every
caller as one identity. The mode is an input to the whole check rather than to an incremental view
of what changed, so a deployment that flips it and has acknowledged nothing does not boot.

# The variant names are not the configured words, and that is deliberate

A deployment writes `single-user` or `multi-user` - `Self::as_str` and `Self::NAMES` own those
spellings, and they are the vocabulary
[a credential per leg](https://github.com/telekom/sutura/blob/main/docs/adr/0008-a-credential-per-leg-for-the-calling-subject.md)
5a names. The variants are named for the *property each mode decides* instead, because
`SingleUser`/`MultiUser` share a postfix and `clippy::enum_variant_names` is denied - and the names
that survived that say more: what changes between the two is whether credentials are static
configuration or a subject arrives per request.

## `use InvalidAccessToken`

Why a string is not usable as an access token.

## `use InvalidDeploymentIdentity`

Why a deployment mode declaration is not usable.

## `use SecuritySettings`

The access posture, and the declaration that goes with a non-loopback bind.

Two fields rather than one, because they answer different questions and collapsing them was
the tempting mistake: a token says *who may reach this*, and the declaration says *what, if
anything, encrypts the path it travels*. A deployment that sets a token but binds the wildcard
with nothing in front has answered only the first.

**The declaration replaced a boolean, and that is the point of it.** The boolean it replaced -
`expose_beyond_loopback` - recorded that somebody meant to publish the service and said nothing
about what protects the token in flight, so a wildcard bind with no terminator anywhere read
exactly like one behind a gateway. A value naming the terminator cannot be satisfied by
agreeing that off-host is intended.

**Five fields now, with the last three answering separate deployment questions.**
`Self::inbound` is how the identity of a *caller* reaches this deployment; `Self::identity` is
who a query then runs *as*; `Self::metrics_token` independently gates `/metrics`. **Neither
inbound nor identity implies the other, and that is the fact worth writing down rather than the
count:** a deployment can verify exactly who is asking and still read every row under one
configured identity, because leg 2 - a credential per execution leg - is not built. The reverse
holds too, and is the shape that ships: a single-user deployment with no inbound block knows what
a query runs as and nothing about who asked. Omitting the metrics token is likewise an explicit
choice not to gate that route; it does not alter either query authentication or caller identity.

The inbound declaration lives in this group rather than one of its own because of
`Self::describes_identity`: that function used to be a constant answering `false`, and a
deployment that establishes a caller identity has to be able to make it answer otherwise from a
value rather than from a rewrite. The deployment declaration is an `Option` for a different
reason - it has no default and its absence is a refusal rather than a value; see
`DeploymentIdentity`, which explains why no combination of source postures may answer it on the
operator's behalf.

## `use TlsTermination`

Where TLS is terminated for this deployment.

**A declaration, not a control.** Nothing here encrypts anything except
`Self::InProcess`; the other three name a terminator that lives somewhere else, and the point
of writing it down is that the *cleartext hop* it implies is then a stated fact rather than an
assumption. The bearer token crosses that hop in the clear, and how far the hop reaches is the
whole difference between the three:

| Declared | What terminates TLS | What the token crosses in cleartext |
| --- | --- | --- |
| `none` | nothing | the whole path from the caller. Only sane on loopback |
| `sidecar` | a proxy in this pod | a loopback hop inside the pod |
| `ingress` | an ingress controller or gateway | the pod network, from that hop to this process |
| `in-process` | this process | nothing - the connection ends here |

So `ingress` is not a weaker `sidecar`: it is the same posture with a longer cleartext segment,
and whether that segment is acceptable is a question about the cluster network - a mesh with
mutual TLS between pods answers it differently from a flat one. This type does not pretend to
know, and a startup log that said "TLS enabled" would be pretending.

## `use UnknownDeploymentIdentity`

The configured value did not name a deployment mode.

## `use UnknownTlsTermination`

The configured value did not name a place TLS is terminated.

## `use BindAddress`

The socket the service listens on.

An `IpAddr` and a port, never a hostname. A hostname is refused rather than resolved: a name
resolves to whatever the resolver says today, which may be a public interface tomorrow, and
a perimeter that moves when DNS moves is not a perimeter. The operator writes the interface
they mean.

## `use BodyLimit`

The largest request body the service will read.

A modelled question is a metric name, a grain, two dates and at most four dimensions, which
is a few hundred bytes. The bound exists because a body limit is the cheapest availability
control there is, and because the default in most stacks is whatever arrives.

## `use InvalidBindAddress`

Why a host and port are not an address to listen on.

## `use InvalidBound`

Why a bound is not a bound.

## `use InvalidTlsMaterial`

Why a pair of paths is not usable TLS material.

## `use RequestTimeout`

How long one request may take before the service gives up on it.

Bounded at both ends. Zero is a service that answers nothing, and an hour is a connection
held open long enough that a handful of them are the outage: a question here is one
aggregate over a bounded range, so a minute is already generous and five is the ceiling.

## `use ServerSettings`

Everything about the socket, the two per-request bounds, and the TLS material if there is any.

**Not `Copy`, and that is the TLS paths.** Every accessor borrows or returns a `Copy` value, and
the group itself is read once, at assembly time.

## `use TlsMaterial`

A certificate chain and the private key that goes with it, as paths.

**Paths and nothing more, and the split is deliberate.** This crate holds no framework and
reads no files: it parses the *pair* - both halves or neither - and stops there. Whether the
files are readable, whether they are PEM at all, and whether the key matches the certificate
are questions only the TLS implementation can answer, so they are answered once, in
`sutura_http::tls`, before the socket is bound. Two checks in two crates would be two messages
for one mistake, and the weaker one would be the reassuring one.

## `use CONFIG_DIR_VARIABLE`

The variable that points at the configuration directory a load layers files from.

**One name, here, because two binaries read it and neither may own it.** `sutura-serve` read it
out of a private constant of its own while the `sutura` command took a directory positionally, so
the two composition roots named the same operator-facing thing in two places and only one of them
could be found by grepping this crate. It is exported for the same reason
`ENVIRONMENT_VARIABLE` is: a startup message, a command's `--help` and the documentation cannot
disagree about a name none of them owns.

## `use ConfigLayers`

Which configuration files were observed, in application order.

**The answer to a question the resolved values cannot be asked.** Every file layer is optional, so
a mistyped configuration directory and a deployment with no files produce the same settings - and
the startup report described those settings in detail while naming no source, which is a report
that cannot distinguish "the operator's file is in effect" from "the operator's file was never
found". An operator reading a value they did not write has nothing to look at.

A type rather than a bare `Vec<PathBuf>` for one reason: `Display` is the
single owner of the wording, including the empty case, so the startup log and the `prompt` command
cannot describe the same deployment differently.

**Paths only, and never a value.** A path is not a credential; a value can be one, and
`security.access_token` is set by exactly this mechanism. Nothing read out of a file reaches this
type - there is nowhere in it for a value to go. The origin it now also carries is the same rule
applied to provenance: a per-key source label is not the value that came from it.

## `use ENVIRONMENT_VARIABLE`

The variable that chooses the deployment environment.

One name, exported so a startup message and the documentation cannot disagree about it.

## `use NotFitToServe`

A deployment this service refuses to start as.

**These are the security posture, and each one is a refusal rather than a warning on purpose.**
The thing being guarded against is not an operator who ignores a log line - it is an operator
who never sees one, because the line was emitted in a format nothing was collecting, on a
process that went on to serve traffic. A process that does not start is noticed.

Every variant names the key to change, because a refusal that does not say what to do is a
support request.

## `use Settings`

The whole resolved configuration.

`Clone` because it is held in the request state, and every field is either `Copy` or a small
owned value. `Debug` is safe to log in full: the only credential-shaped field is held in
`sutura_domain::identity::Secret`, whose `Debug` redacts, and a test in `crate::security`
asserts that at struct depth.

## `use SettingsError`

Why a configuration could not be turned into settings.

One variant per thing that can be wrong, each keeping the typed cause underneath it. The
message names the concern and the `#[source]` chain names the value, so an operator reading
stderr gets both without either being formatted into the other.

## `use SettingsLoadError`

A failed configuration load and the file layers observed before it failed.

The source remains typed; this message adds only file context. Observed paths do not prove
that each file parsed or contributed a value. Variables and text overlays have no file path.

## `use Sources`

Where a load reads from.

A value rather than a set of arguments, for one reason: the process environment is global, and
`std::env::set_var` is `unsafe` in this edition - so a test that wanted to exercise the variable
layer by setting variables could not be written under `unsafe_code = "forbid"`. Supplying the
variables as a map makes that layer a pure function of its input, and
`Sources::from_process_environment` is the one place that reads the real environment.

## `use VARIABLE_PREFIX`

The prefix every configuration variable carries, and the separator between key segments.

`SUTURA__SERVER__PORT` sets `server.port`. Two underscores for both, so a key segment that
itself contains an underscore - `access_token`, `max_body_bytes` - needs no escaping.

## `use VARIABLE_SEPARATOR`

The separator between nested key segments in a configuration variable name.

## `use config_dir_from_process`

Reads the configuration directory from the process, if one was named.

**An empty value is the same as an absent one, deliberately.** A container platform that
templates `SUTURA_CONFIG_DIR` from an unset field sets it to the empty string, and
`PathBuf::from("")` layers `base.yaml` relative to whatever the working directory happens to be -
which is a file nobody wrote resolving somewhere nobody chose. The absence is the safe reading:
the embedded defaults are complete.

Not fallible, and that is not a shortcut: unlike `environment_from_process` there is no
permissive branch to fall into. A non-Unicode path is still a path this process can open, so it is
carried through as an `OsString` rather than refused.

## `use configuration_variables_from_process`

The NAMES of the `SUTURA__*` variables this process has set, sorted.

**Names only, never values, and that is the security half rather than brevity.**
`SUTURA__SECURITY__ACCESS_TOKEN` is one of these, so printing the environment as pairs would put
a deployment credential into the text of a refusal - which goes to stderr, into a log, and into
whatever collects one.

**The limit, next to the claim: this is what is SET, not what was USED.** A name here is a
candidate for the refusal above it, not a diagnosis - it may be setting a key the refusal is not
about, and the refusal may be about a key that came from a file.

That is a choice rather than a wall: the pinned `config` records the origin of every value and
`crate::settings::read` discards it, which `github.com/telekom/sutura#440` measures and costs.

## `use environment_from_process`

Reads the deployment environment from the process.

Absent means `Environment::Development`: a developer running the binary with no environment
set is on a laptop, and the permissive default is safe there precisely because the other
defaults are loopback-only. An environment that is *present and unrecognised* is an error and
never falls back, because falling back would select the permissive branch of five decisions.

## `use BillingProject`

The project a `BigQuery` query job is billed to.

**Declared, never inferred.** For this data system that is structural rather than a policy we
chose: the project is a PATH SEGMENT of the request URL that submits a job, so there is no field
it could be omitted from and nothing it could be defaulted from. The reason it is declared HERE,
one step before anything impersonates, is that a federated identity has no project of its own to
bill - so the per-subject step needs this declaration to already exist rather than introducing it
alongside a credential exchange.

# What `parse` enforces, and why the argument is ours

The value is interpolated into a URL path segment. So what has to be impossible is a value that
LEAVES that segment: a `/`, a `?`, a `#`, a `%`-escape, whitespace, a control character, anything
non-ASCII. The accepted set is therefore `[a-z0-9-]`, starting with a letter, not ending with a
hyphen, and 6 to 30 characters.

That happens to be the documented shape of a project id, and it is deliberately not justified
that way: **the argument for the character set is the path segment**, which holds whether or not
the provider widens its own rules later. If the provider ever narrows them further, a value we
accept and they reject is a startup failure against a real endpoint - the safe direction. If they
widen them, this refuses a legal id and the fix is a considered change here rather than a value
that silently escapes a URL.

**One shape this knowingly refuses, stated because it is a real deployment and not a hypothetical:**
a LEGACY domain-scoped project identifier carries a colon - the provider's own SQL reference uses
`google.com:my_project` as its example and tells an author to wrap it in backticks. A colon in a
URL path segment is legal, so this is a narrowing we are choosing rather than one escaping forces,
and it is chosen because such an id also has to survive being a path segment, a JSON field and a
backticked SQL identifier, and nothing here has ever been exercised against one. A deployment that
needs it gets a considered change with a test, not a widened character set.

No `Default`: a default project is a project somebody else pays for.

## `use DatasetId`

The dataset unqualified table names in a generated statement resolve within.

**Why this is configuration and not catalog:** a model in the catalog names a bare `table:`, and
which dataset that table lives in is a property of the deployment's connection rather than of the
metric's definition. The same catalog served against a staging dataset and a production one is one
catalog and two deployments, which is exactly the split this type keeps.

The generated statement therefore stays a bare, quoted table name in every dialect - the request
carries the dataset beside the SQL rather than the generator qualifying it - so nothing about
`sutura-sql` has to know this exists.

`parse` accepts `[A-Za-z0-9_]`, 1 to 1024 characters. Unlike `BillingProject` this one does not
reach a URL path, so the constraint is not an escaping argument: it is that a dataset id which is
not an identifier is a misconfiguration worth refusing when the file is read rather than on the
first question. Case is PRESERVED, because a dataset id is case-sensitive and folding it here
would turn a working declaration into a dataset that does not exist.

## `use HostName`

A `postgres` source's host or address, exactly as written.

**Unrepresentable rather than checked, per `secure-by-design`.** Before this type, `host` was a
bare `String` carried past `parse_placement` unexamined - the exclusivity of `host` and
`unix_socket` was a check in that function, but the FIELD still admitted whatever text was
there, so `crate::sources::placement::PostgresDial` existed only as a `match` two composition
roots each wrote by hand. Refuses only shapes that cannot be a host at all - empty, embedded
whitespace, a URL scheme, a path separator - and nothing about reachability: a value that parses
may still fail to resolve, or fail the TLS name check at connect time, and neither is this
type's question. `crate::sources::transport::host_is_loopback` still does the loopback test on
the parsed text.

## `use InvalidHostName`

Why a declared Postgres host cannot be dialled at all.

## `use InvalidResourceName`

Why a declared name for a cloud resource was not usable.

One type for both newtypes above, with the offending key named by the caller rather than by the
variant: the shapes differ and the *reasons* do not, so two near-identical enums would be two
places to keep one set of sentences.

## `use PostgresDial`

How a `postgres` source is dialled: over TCP to a named host, or through a unix socket
directory. Exactly one, decided once in `crate::sources::parse_placement`.

**Replaces `host: Option<String>` plus `unix_socket: Option<PathBuf>` on the placement.** Those
two fields admitted `(None, None)` and `(Some, Some)`, states `parse_placement` already refused -
so both composition roots carried a `match (host, unix_socket)` with a fourth arm the parser had
already made unreachable, and a comment saying so at each. This enum is the same argument
`SourcePlacement` itself makes about `Files` versus `BigQuery`: unrepresentable beats checked
twice.

## `use SourcePlacement`

Where one declared source's data is.

The module header carries why this is an enum. What is worth repeating at the type is that
`SourceKind` is DERIVED from it - see `Self::kind` - rather than stored beside it, so the two
cannot disagree about what a source is.

# The limit, because an enum variant's fields are always public

There is no way to make these private, so **a placement is constructible in-process by any crate
that can name the type** - including one carrying a relative `data_dir`, which `parse_data_dir`
refuses when it reads a file. That is a real gap in this type and it is not the one that matters,
for the reason AGENTS.md already states about the other constructors here: what is closed is the
path from a **configuration file**. `ConfiguredSource` holds its
placement in a private field and has no public constructor, so a
`SourceRegistry` can still only come into existence through
`Settings::parse`, and that is the only door a deployment goes through.

Written down rather than left to be re-derived, because "the fields are public" and "the checks can
be skipped" look like the same sentence and are not.

## `use ConfiguredSource`

One declared data system.

The identity is an `Option`, and its `None` is **fail-closed rather than permissive**: it means
this entry declared the shared posture and nobody acknowledged it, which
`Settings::refusals` refuses. A `Settings` obtained through `Settings::load` therefore has `Some`
for every source. It stays an `Option` rather than being unwrapped here because a composition root
that treated the absence as permission is a bug the type should not be able to hide, and because
`Settings::parse` is reachable from this crate's own tests without the refusal having run.

## `use InvalidSourceRegistry`

Why a `sources:` tree is not usable.

Every variant names the alias, because a refusal that does not say which entry to change is a
support request - and a deployment with several sources is exactly the deployment where "one of
your sources is wrong" is useless.

No `Clone`, and the reason is worth a line rather than a shrug: one variant's cause is
`sutura_domain::model::InvalidIdentifier`, which is not `Clone` either. Deriving it here would mean
either a second copy of that error's shape or a `Clone` added to a domain type for a config crate's
convenience, and nothing needs to clone a startup refusal.

## `use SourceKind`

What kind of data system a source is.

**A closed set of typed declarations rather than something discovered**, which is the whole of
*pluggable by declaration*: a capability nobody declared cannot be used, and a new kind is a
compile error in every place that has to decide about it. Two variants today, and only one of them
can be OPENED by a shipped binary - `Self::BigQuery` says which and why.

**It replaced a comparison against a hard-coded source NAME**, and that is the change worth reading
rather than the enum. The composition root used to refuse any source not called `local`, on the
argument that the engine has its own identity and does not borrow the catalog's. That argument was
right while the catalog was the only signal - a catalog naming `production_warehouse` said nothing
about what the deployment held - and it stops being right once the DEPLOYMENT declares each source:
an operator who writes `sources.production_warehouse.kind: files` with a directory beside it has
stated that this source is a directory of files, which is the statement the name comparison was
standing in for. Under the old rule that deployment could not be served at all, and it is a
legitimate one.

## `use SourceRegistry`

Every source this deployment declares, keyed by the alias a model's `source:` names.

A newtype over the map rather than the map, so `Self::parse` is the only way one comes into
existence and the duplicate-alias refusal cannot be skipped by building the map directly.

**May be empty, and that is not a refusal here.** A deployment configuring no source is one that
has not said where its data is; what refuses it is the composition root, which finds the catalog
naming a source with no declaration and stops before a listener is bound. Refusing an empty tree in
this crate would mean `Settings::load` on the embedded defaults could not produce a `Settings` at
all, and the defaults are what the `prompt` command and every settings test read.

## `use UnknownPosture`

The configured word did not name a posture.

## `use UnknownSourceKind`

The configured word did not name a kind of data system.

## `use InvalidLogFilter`

Why a string is not a filter directive.

## `use InvalidServiceName`

Why a string is not a service name.

## `use LogFilter`

A tracing filter directive, as written for `RUST_LOG`.

Kept as text here and turned into a real filter by whatever installs the subscriber, because
the type that parses one lives in `tracing-subscriber` and this crate holds no framework. What
*is* checked here is that it is not empty and not a smuggled second line: an empty filter
silently means "no directives", which is a service that logs at the default level while its
configuration says otherwise.

## `use LogFormat`

How a log line is rendered.

## `use ServiceName`

The name every log line is attributed to.

A separate type because it is the field a collector groups by, so an empty or whitespace one
makes every line from this deployment unattributable.

## `use TelemetrySettings`

Everything about the log.

## `use UnknownLogFormat`

The string was neither format.

## Module `api`

Whether the generated documentation is served, and why the default differs by environment.

The `OpenAPI` document and the browser UI over it are generated from the handlers, so they are
always *correct*; the question is whether an unauthenticated caller should be handed a map of
the surface. On a laptop the answer is obviously yes - it is how the surface is explored at
all. In production it is a decision, and the safer default is the one that has to be turned
on rather than the one that has to be remembered.

It is a default and not a refusal. Serving an interface description is a legitimate choice for
a deployment behind a gateway that already authenticates, and refusing to start over it would
be this crate overruling an operator on something that leaks no data. What it does instead is
log the decision, at a level that shows up.

### `struct ApiSettings`

```rust
pub struct ApiSettings
```

Whether the generated documentation surface is served.

#### Methods

```rust
pub const fn docs_default_for(environment: crate::Environment) -> bool
```

The default for an environment: everywhere but production.

A total match rather than a comparison, so a fourth environment has to state its own
answer instead of inheriting whichever branch it happens to fall into.

```rust
pub const fn docs_enabled(self) -> bool
```

```rust
pub const fn docs_were_explicit(self) -> bool
```

```rust
pub const fn new(docs_enabled: bool, docs_were_explicit: bool) -> Self
```

#### Implements

`Clone`, `Copy`, `Debug`, `Eq`, `PartialEq`

## Module `catalog`

Which catalog the service serves, and where the files it reads live.

Two directories rather than one, because they are owned by different people. The catalog is
authored and reviewed - it is the definitions somebody certified - and the data directory is
wherever the files the engine reads happen to be mounted. Conflating them would make a
deployment that moved its data look like a catalog change, which is the one thing a pinned
bundle exists to make visible.

### `enum CatalogKind`

```rust
pub enum CatalogKind
```

Which adapter the catalog configuration names, and therefore which one opens it.

**A closed set of typed declarations, `crate::sources::SourceKind`'s shape on the metadata
side.** A DATA source's adapter is chosen by `SourceKind` and dispatched by the composition
root's exhaustive match with no wildcard arm; a METADATA source has exactly the same need, and
until this type existed the settings tree carried a directory and a version and no word an
operator could write to say *read the model from somewhere else* - so a second catalog kind
could merge complete and silently remain unreachable from any binary.

Two variants today. `Self::Datahub` says which and why, the way `SourceKind::BigQuery` does for
data systems: the vocabulary is the vocabulary of adapters this repository has, and an adapter
that exists in a record rather than in a linked crate is still a word an operator might write.

#### Variants

- `Markdown` - A directory of markdown documents with YAML frontmatter, read by `sutura-catalog-local`.
- `Datahub` - A metadata service, read through the adapter `docs/adr/0016` specifies and #114 builds.

#### Methods

```rust
pub const fn as_str(self) -> &'static str
```

The spelling, for the startup log.

```rust
pub fn parse(raw: impl AsRef<str>) -> Result<Self, UnknownCatalogKind>
```

Reads the configured word.

#### Implements

`Clone`, `Copy`, `Debug`, `Eq`, `PartialEq`

### `struct UnknownCatalogKind`

```rust
pub struct UnknownCatalogKind
```

The configured word did not name a kind of catalog this build has.

#### Implements

`Clone`, `Debug`, `Display`, `Eq`, `Error`, `PartialEq`

### `struct CatalogSettings`

```rust
pub struct CatalogSettings
```

Where the definitions and the data are, and what the resulting bundle is called.

Each catalog carries a declared NAME, the way a `sources:` entry carries an alias: the
contribution manifest keys on it, and a reviewer reads it in a settings file. It is named by
code and not by index so that reordering the list does not silently rename a contributor.

#### Methods

```rust
pub fn data_dir(&self) -> &Path
```

```rust
pub fn dir(&self) -> &Path
```

```rust
pub const fn kind(&self) -> CatalogKind
```

Which adapter opens this catalog.

```rust
pub const fn name(&self) -> &SourceName
```

The declared name, which the contribution manifest keys on.

```rust
pub fn parse(name: SourceName, kind: CatalogKind, dir: PathBuf, data_dir: PathBuf, version: DefinitionVersion) -> Result<Self, InvalidCatalogSettings>
```

Reads the declared name, kind, the two directories and the version label.

The version arrives already parsed, because what identifies a snapshot of a directory is
a commit id or a build number and only the caller has it. Existence of the directories is
deliberately *not* checked here: this type is the configuration, and a directory that
disappears between reading the configuration and loading the catalog would make an
existence check here a claim that goes stale immediately. The load is what fails.

```rust
pub const fn version(&self) -> &DefinitionVersion
```

#### Implements

`Clone`, `Debug`, `Eq`, `PartialEq`

### `enum InvalidCatalogSettings`

```rust
pub enum InvalidCatalogSettings
```

Why a catalog configuration is not usable.

#### Variants

- `EmptyPath` - A path was empty, which resolves to the process working directory - a different directory on every host, and never the one the operator meant.
- `EmptyCatalog` - No catalog was declared, so there is nothing to serve.
- `DuplicateName` - Two catalogs share one declared name, so the contribution manifest could not tell them apart.

#### Implements

`Clone`, `Debug`, `Display`, `Eq`, `Error`, `PartialEq`

### `struct Catalogs`

```rust
pub struct Catalogs
```

The catalogs a deployment declares, in declaration order.

**A non-empty, ordered collection, and the empty member is unrepresentable.** Composition -
the point of having N - is the metadata assembler in `sutura-app`; this type is the declared
configuration it is handed. Order is declaration order, which is content order: the contribution
manifest is a `BTreeMap` keyed on each entry's `CatalogSettings::name`, so this ordering is
what a reviewer reads and manifest determinism does not depend on it surviving a rename.

#### Methods

```rust
pub const fn count(&self) -> usize
```

How many catalogs are declared.

```rust
pub fn each(&self) -> impl Iterator<Item>
```

Every catalog, in declaration order.

```rust
pub fn parse(entries: Vec<CatalogSettings>) -> Result<Self, InvalidCatalogSettings>
```

Reads the declared catalogs, refusing an empty list and any duplicated name.

#### Implements

`Clone`, `Debug`, `Eq`, `PartialEq`

## Module `credentials`

The credential broker a deployment gets when its credentials are configuration.

**The first implementor of `sutura_domain::identity::CredentialBroker`, and it is a shipping
deployment mode rather than test scaffolding.** `AGENTS.md`'s rule is that a port trait arrives
with its first implementor, because a trait with no implementor is a guess at a signature - and the
calling-subject floor this port answers to is `docs/adr/0008` part 6. What arrives with
it here is the static-credential broker single-user mode already needs: credentials as
configuration, one user, one host.

# Why this crate

Because the identity provider this broker talks to *is* the settings tree. It opens no socket,
reads no clock and holds no secret of its own: what it hands back for a shared source is the
operator's own acknowledgement witness, which this crate is the only place that can parse. A
broker that exchanges a token belongs in a crate that can make a network call, and that crate does
not exist yet - `docs/adr/0014` Decision 3 says why, and `sutura_domain::identity`'s own header
records what the port cannot express until it does.

# What it refuses, and why that is the interesting half

**A source declared `impersonation-at-source` gets no credential from this broker**, so a question
against one is refused as `CredentialUnavailable` rather than answered under the deployment's own
identity. That is the whole point of the port: the fallback is not forbidden by a rule, it is
absent from every signature, and a broker with nothing to present has to say so.

It is also what makes the refusal provokable **without a network and without a fake** - by the
real implementor, from a real configuration - which is what `docs/adr/0008` part 6 asks of the one
refusal variant this design adds.

### `enum StaticCredentialsUnusable`

```rust
pub enum StaticCredentialsUnusable
```

A defect in this broker itself, which no configuration reaches.

**Stated rather than unwrapped, and the reason is worth a line.** The one thing minting can fail
on is `LegCredentials::minted` refusing a set that does not cover the sources it was asked about -
and this broker builds its map from that same set, one entry per source it was asked about, so the
check cannot fire here. It is still answered for rather than unwrapped: `unwrap_used` is denied,
and a panic on this path would be process death under `panic = "abort"` for a case a type already
describes. Nothing in this crate's suite can provoke it, and that is said here rather than left
for a reader to assume it is covered.

#### Variants

- `Coverage` - The credentials built here did not cover the sources they were asked about.

#### Implements

`Debug`, `Display`, `Error`

### `struct StaticCredentialBroker`

```rust
pub struct StaticCredentialBroker
```

Mints from what an operator declared, and nothing else.

Holds one entry per source declared `shared-service-user`, carrying that source's acknowledgement
witness. A source declared `impersonation-at-source` is deliberately **absent**: there is no
static credential that could execute as an asking subject, and an entry that pretended otherwise
would be the fallback this port exists to remove.

**Built from the DECLARATION and never from an adapter.** `docs/adr/0008` part 4 requires that a
broker produce the shared shape only for a source configured shared, and the check that catches a
broker which did not is the adapter's own exhaustive match on what it received. Those two are only
an independent pair if they read different things: this reads the settings tree, and the adapter
holds what the composition root handed it. A broker that read the posture off the adapter would
make that check compare a value against itself.

#### Methods

```rust
pub fn count(&self) -> usize
```

How many sources this broker can mint for.

Read by this crate's own suite, which is where the interesting assertion is: a source declared
`impersonation-at-source` is ABSENT rather than mapped to a default, so the count is what shows
that a broker holding nothing for a source is a broker that refuses it. Nothing in a composition
root prints it today.

```rust
pub fn for_one_shared_source(source: SourceName, declared: SharedIdentityDeclared) -> Self
```

One shared source, declared in code rather than in a file.

**For a composition root that has no settings tree**, which is the `sutura` command: it answers
one question and exits, reading the files of whoever ran it as that person's own
operating-system identity, and there is no `sources:` entry for an operator to write. The
acknowledgement is still an `SharedIdentityDeclared` that went through
`AcknowledgementReason::parse`, so what the leg carries is bounded and checked by the same code
a configuration file's is - the difference is who wrote the sentence, not whether one exists.

It is here rather than as a second broker in that binary because one implementor of the port is
what keeps its contract in one place: a second one would be a second thing to keep in step with
what an adapter accepts.

```rust
pub fn from_registry(registry: &SourceRegistry) -> Self
```

Reads the declared sources.

Infallible: a registry that parsed is a registry whose postures are declared, and a source
this broker cannot mint for is a refusal at request time rather than a startup failure. The
startup failures that DO belong to identity are already elsewhere and are not duplicated here -
`Settings::refusals` refuses an unacknowledged shared source in multi-user mode, and the
composition root refuses a posture the linked adapter cannot deliver.

#### Implements

`Clone`, `CredentialBroker`, `Debug`

## Module `environment`

Which deployment this process believes it is.

One value, read before anything else, that decides three things which must not be decided
separately: which configuration file is layered on top of the defaults, whether the log is
machine-readable or human-readable, and how strict the startup refusals are. Separate
switches for those would let a deployment be production for logging and development for
safety, which is exactly the combination nobody would choose on purpose.

Deliberately three values and not a free string. A typo in an environment name is otherwise
the most expensive kind of configuration bug there is: it silently selects the permissive
branch of every decision above, and the log line saying so is in the format nobody is
collecting.

### `enum Environment`

```rust
pub enum Environment
```

A deployment kind.

Ordered from most permissive to least, which is also the order the refusals in
`crate::Settings` tighten in.

#### Variants

- `Development` - A developer's machine. Human-readable logs, loopback only, no token required.
- `Test` - An automated test. Same posture as development, and named separately so a test can assert on it without pretending to be a laptop.
- `Production` - A deployment serving somebody. Machine-readable logs, and every refusal in `crate::Settings::parse` applies.

#### Methods

```rust
pub const fn as_str(self) -> &'static str
```

The canonical spelling, which is also the file stem this environment layers.

```rust
pub const fn is_production(self) -> bool
```

Is this the environment the strict refusals apply to?

A method rather than `== Environment::Production` at each call site: there are five
refusals keyed off it, and a fourth variant added later has to answer this question once.

```rust
pub fn parse(raw: impl AsRef<str>) -> Result<Self, UnknownEnvironment>
```

Reads an environment name.

Case-insensitive and trimmed, because this arrives from a shell variable and
`SUTURA_ENVIRONMENT=Production ` with a trailing space is not a different deployment.
Nothing else is forgiven: `prod` is not accepted, because an abbreviation somebody has to
guess is a spelling this type exists to remove.

#### Implements

`Clone`, `Copy`, `Debug`, `Deserialize<'de>`, `Display`, `Eq`, `Hash`, `Ord`, `PartialEq`, `PartialOrd`, `Serialize`

### `struct UnknownEnvironment`

```rust
pub struct UnknownEnvironment
```

The string was not one of the three.

Carries what it found, because the whole point of the type is that a typo fails loudly, and
a failure that does not quote the typo makes the operator guess which of the three spellings
they got wrong.

#### Methods

```rust
pub fn found(&self) -> &str
```

#### Implements

`Clone`, `Debug`, `Display`, `Eq`, `Error`, `PartialEq`

## Module `inbound`

How the identity of a caller reaches this deployment - leg 1, and the one fact that decides it.

`docs/adr/0014` decides two inbound modes and says plainly that **neither of them is a default**.
A deployment either *is* the resource server and validates the caller's token itself, or it sits
behind a component that already authenticated the caller and validates a short-lived **identity
assertion that component signed**. Both defaults are wrong in opposite directions: defaulting to
`InboundIdentity::Direct` makes a gateway deployment reject every caller, and defaulting to
`InboundIdentity::BehindGateway` makes a directly exposed deployment accept an assertion anybody
can mint.

So the mode is a required key **inside** the declaration, and the declaration as a whole is
optional. Those are two different absences and the difference matters:

| Written | What it means |
| --- | --- |
| no `security.inbound` block at all | this is a single-player deployment. There is no per-caller identity to establish, the bearer token authenticates the deployment, and nothing here becomes required. `docs/adr/0008` part 5a calls that a first-class shape rather than a degraded one |
| a block with no `mode` | a deployment that meant to establish identity and did not say how. It does not start |

**What this does NOT deliver, and it must not be read as delivered:** leg 1 proves who is asking.
It does *not* make a data source execute as that person - that is leg 2, and it needs a credential
per leg plus a source that declares it can impersonate. A deployment with leg 1 and no leg 2 knows
who is asking and still reads every row as one identity. `InboundIdentity::what_it_does_not_do`
is that sentence as a value, printed at startup, for the same reason
`TlsTermination::cleartext_hop` is one: a log
line and this documentation read the same string, so neither can drift into claiming per-user
access because there is authentication.

# `BehindGateway` does not mean "trust a header", and the type is what stops it meaning that

A component asserting an identity in a header is not authentication - anything that can reach the
port can write that header, and the failure is invisible in a diff: a header named
`x-authenticated-user` that means "authenticated" because of where it is *expected* to come from.

What `TransitProof` carries is therefore not a header holding a name. It is a header holding a
**signed token**, with an issuer, an audience, a key set and a pinned algorithm - the same four
things `InboundIdentity::Direct` validates - and the subject is derived by us from the claims of
a token whose signature checked out. There is no shape in this module that could hold "the name of
the header the username is in".

**The limits, stated next to the claim, and there are three.** Under `BehindGateway` this
deployment trusts the component's *authentication of the caller*, because that is what the mode
means; what it does not trust is a string. The signature says the claims came from the component,
and nothing here can say the component authenticated correctly.

And what a signed assertion proves is that **the component issued it**, not that *this request*
carried it there first. Review found the wording overstating exactly that: a proof was replayable
for as long as its `exp` allowed, and its `exp` was the component's to choose. Two of those three
are now bounded - `ProofLifetime` caps `exp - iat` and an `iat` is required, so the replay window
is a number this deployment chose rather than one it was handed. **Binding an assertion to a
particular request is not built**: there is no nonce store and nothing hashes a method, a path or a
body into the proof, so inside the lifetime window an intercepted assertion replays. That is why
this module and `docs/adr/0014` now call it a *gateway-issued identity assertion* rather than a
proof that the request transited anything, and why the trusted transport boundary - the hop between
the component and this process - is load-bearing rather than incidental.

# One derived view, two named modes

`docs/adr/0014` says the difference between the modes is "one fact rather than two code paths".
`InboundIdentity::requirement` is that sentence made mechanical: the enum keeps the two names an
operator writes and a reviewer reads, and the validator downstream consumes a single
`TokenRequirement` borrowed out of whichever variant is configured. There is one validator, so
there is one place algorithm pinning and the audience check can be got wrong.

### `enum RequiredTokenType`

```rust
pub enum RequiredTokenType
```

Which class of token this deployment will accept, out of the `typ` header.

**Two variants because the check has to be switchable and must not be switchable by silence.** The
finding it answers is cross-JWT substitution: without it, any JWT the issuer signed with this
audience verifies, an OIDC ID token included whenever the resource identifier equals the client id.
So `Self::Exactly` is the default in the `direct` mode - RFC 9068's `at+jwt` - and turning it off
is a value an operator writes, `any`, which the startup log prints at `WARN`.

There is no `Option<TokenType>` here, for the reason `docs/adr/0014` gives about `mode`: an absent
value reads as "not configured yet" at every call site, and the one thing that has to be legible is
whether a deployment decided to accept every class of token.

#### Variants

- `Exactly` - A token whose `typ` is this, compared after the signature verified.
- `Any` - Any class of token the issuer signed for this audience.

#### Methods

```rust
pub fn accepts(&self, presented: Option<&TokenType>) -> bool
```

Does a presented `typ` satisfy this?

`None` is an ABSENT `typ` header, and it satisfies nothing but `Self::Any`: a token carrying no
type is exactly the shape a class check exists to refuse, and treating absence as acceptable
would make the check satisfiable by omission.

```rust
pub fn access_token() -> Self
```

RFC 9068's access-token type. The `direct` mode's default.

```rust
pub fn as_str(&self) -> &str
```

What is required, as a word for a log line and for a refusal message.

```rust
pub fn parse(key: &'static str, raw: impl AsRef<str>) -> Result<Self, InvalidInboundValue>
```

Reads the configured word: `any`, or a media type.

`any` is compared on the folded value, so `Any` and `ANY` are the same answer - and a deployment
whose component really does emit a `typ` of `any` cannot express it. That collision is worth
having: the word is checked before the media type precisely so that turning the check off cannot
happen by accident.

#### Implements

`Clone`, `Debug`, `Display`, `Eq`, `PartialEq`

### `enum TokenLocation`

```rust
pub enum TokenLocation<'header>
```

Where the token being validated arrives.

Two variants because the two modes put it in two places, and neither is a preference: an OAuth
2.1 client puts its access token in `Authorization: Bearer` and has no option to do otherwise,
while a fronting component sets a header of its own and would collide with the deployment bearer
token if it used that one. `ProofHeader::parse` refuses `authorization` for exactly that reason.

#### Variants

- `AuthorizationBearer` - `Authorization: Bearer <token>`. Where RFC 6750 puts an access token.
- `Header` - A named header whose whole value is the token. No scheme prefix: a component setting its own header has no reason to wrap the value, and a prefix nobody agreed on is a parse to get wrong.

#### Implements

`Clone`, `Copy`, `Debug`, `Display`, `Eq`, `PartialEq`

### `struct TransitProof`

```rust
pub struct TransitProof
```

What a fronting component has to prove on every request.

Every field is a validation input, and that is the point of the type: there is no field here that
holds an asserted identity. See this module's documentation for why.

#### Methods

```rust
pub const fn header(&self) -> &ProofHeader
```

The header the proof arrives in.

```rust
pub const fn new(header: ProofHeader, issuer: IssuerUrl, audience: ResourceIdentifier, key_set: KeySetFile, algorithms: PinnedAlgorithms, token_type: RequiredTokenType, max_lifetime: ProofLifetime) -> Self
```

Assembles a declaration from parts that have each already been parsed.

Seven arguments, over `clippy.toml`'s threshold of five, and taken rather than grouped
deliberately: a parts struct would need public fields, which `cargo xtask check-boundaries`
refuses on a public struct in a library crate - for the reason it exists, that a public field is
a second way to build a value without its invariant. Every one of these is already a parsed
newtype, so the list is seven invariants rather than seven strings.

#### Implements

`Clone`, `Debug`, `Eq`, `PartialEq`

### `enum InboundIdentity`

```rust
pub enum InboundIdentity
```

How the identity of a caller reaches this deployment. Printed at startup, per deployment.

A closed enum with a required key, in the shape
`TlsTermination` already uses here - and for the same reason
`Environment` is a parsed enum rather than a string with a fallback: the value that decides a
posture must not be satisfiable by silence.

#### Variants

- `Direct` - This deployment is the resource server. It validates the caller's token itself: signature, issuer, expiry, and an audience matching its own resource identifier.
- `BehindGateway` - A fronting component authenticated the caller. This deployment validates a **signed identity assertion** the component issued, and derives the subject from that assertion's own claims rather than from a string somebody set.

#### Methods

```rust
pub const fn accepts_any_token_class(&self) -> bool
```

Is the class check switched off?

Read by the startup log to decide the level, so the answer is a value rather than a comparison
somebody writes at the call site.

```rust
pub const fn mode(&self) -> &'static str
```

The mode, as a stable word for the startup log and for a record field.

A `&'static str` from an exhaustive match rather than a `Display` of something structured,
because it has to be a value a query over logs can group by.

```rust
pub const fn reads_the_authorization_header(&self) -> bool
```

Does this deployment read the deployment bearer token's own header?

The one question a refusal needs answered, and it is asked of the *requirement* rather than of
the variant, so a mode added later that also lands in `Authorization` cannot slip past it.

```rust
pub const fn requirement(&self) -> TokenRequirement<'_>
```

The one validation this deployment performs, whichever mode it is in.

See this module's documentation: the two modes are one fact and not two code paths, so there
is one validator and one place the audience check can be got wrong.

```rust
pub const fn type_check(&self) -> &'static str
```

What the `typ` check does on this deployment, as a sentence for the startup log.

**A sentence rather than a boolean, because the interesting value is the one that reads as
nothing.** A deployment that wrote `any` has switched off the check that stops an OIDC ID token
from establishing a caller, and `type_check = "any"` on a log line does not say that. This does,
and `crate::security::SecuritySettings` prints it at `WARN`.

```rust
pub const fn what_it_does_not_do() -> &'static str
```

The sentence that keeps leg 1 from being read as leg 2.

The same for both modes, deliberately: `docs/adr/0014`'s table says the mode changes who
authenticates the caller and changes **nothing** about who is responsible for the chain or for
leg 2. A constant rather than a `match` would have said that less clearly than a function
whose whole body is one string does.

```rust
pub const fn who_authenticated(&self) -> &'static str
```

Who authenticated the caller, as a sentence for the startup log.

A function rather than a comment for the reason
`TlsTermination::cleartext_hop` is one: the
log, this documentation and the type read the same value, so none of them can drift into
claiming more than the mode does.

#### Implements

`Clone`, `Debug`, `Eq`, `PartialEq`

### `struct TokenRequirement`

```rust
pub struct TokenRequirement<'inbound>
```

The whole validation this deployment performs, borrowed out of whichever mode is configured.

**A view and not a second configuration surface.** There is no constructor: the only way to one
of these is `InboundIdentity::requirement`, so the validator cannot be handed a requirement
that does not correspond to a declaration an operator wrote and this crate refused or accepted.

#### Methods

```rust
pub const fn algorithms(&self) -> &'inbound PinnedAlgorithms
```

The algorithms this deployment will accept - never the one in the token's own header.

```rust
pub const fn audience(&self) -> &'inbound ResourceIdentifier
```

The value the `aud` claim must contain, byte for byte.

**Unconditional, and that is the security decision in `docs/adr/0014`.** A client may send a
resource indicator asking its authorization server for a narrower token; that is welcome and
it is an optimisation. It is never what makes the token safe, and this check is not skippable
when the indicator is absent.

```rust
pub const fn issuer(&self) -> &'inbound IssuerUrl
```

The issuer the `iss` claim must equal, byte for byte.

```rust
pub const fn key_set(&self) -> &'inbound KeySetFile
```

Where the signing keys are read from.

```rust
pub const fn location(&self) -> TokenLocation<'inbound>
```

Where the token arrives.

```rust
pub const fn max_lifetime(&self) -> Option<ProofLifetime>
```

The ceiling on `exp - iat`, where this deployment puts one. See the field.

```rust
pub const fn token_type(&self) -> &'inbound RequiredTokenType
```

Which class of token, out of the `typ` header, checked after the signature verified.

#### Implements

`Clone`, `Copy`, `Debug`, `Eq`, `PartialEq`

### `use InvalidAlgorithms`

Why a pinned algorithm list is not one.

### `use InvalidInboundValue`

Why a value in an inbound-identity declaration is not one.

One error for every newtype in this module, because they are one parse with different
vocabularies: the same five things can be wrong about each of them, and three hand-written copies
of that list is three things to keep in step.

**No variant quotes the whole value back.** A resource identifier is not a credential, but an
issuer URL and a header name are both deployment topology, and this crate's own rule is that an
error carries the typed context and not a rendering of the input. A position is what an operator
needs to find the character; the character itself is often the one that draws nothing.

### `use IssuerUrl`

Where the tokens this deployment accepts are minted.

Compared against the `iss` claim, byte for byte, for the reason `ResourceIdentifier` is: an
issuer is a configured string on both sides and normalising ours would make it disagree with
theirs.

### `use KeyFamily`

Which kind of key verifies a `SigningAlgorithm`.

Here rather than left implicit because **a pinned set spanning two families is a set that can
verify nothing**, and finding that out from a `401` is expensive. One key set holds keys of
whatever kinds the issuer publishes; one *token* is verified by one key with one algorithm, and
the validator this feeds refuses a permitted-algorithm list whose family disagrees with the key
it looked up. So the mixed list is refused at startup instead - see `PinnedAlgorithms::parse`.

### `use KeySetFile`

Where the signing keys are read from.

**A file and not a URL, and that gap is named rather than left to be discovered.** A JWKS
endpoint needs an outbound HTTP client, which is a supply-chain change with its own review and
its own failure mode - `docs/adr/0014` says plainly that the authorization server becomes a hard
runtime dependency and that an outage there must stay distinguishable from a dead data system.
None of that is built. What is built is the *rotation* mechanism: the key set is cached, refetched
when a key id is not in it, and that refetch is rate limited - and every one of those properties
is the same whether the source is a file a sidecar rewrites or an endpoint. See
`sutura_http::inbound::keys`.

### `use PinnedAlgorithms`

The algorithms this deployment will accept, all of one family and at least one.

Non-emptiness and single-family-ness are both structural: the first link lives in its own field,
which is the shape `sutura_domain::identity::ActorChain` already uses for the same reason - there
is no state of this type that means "nothing is permitted", so nothing downstream has to check.

### `use ProofHeader`

The header a transit proof arrives in.

Lower case, because that is what HTTP/2 puts on the wire and what a header map is keyed on here -
so folding it at construction is what makes a configured `X-Transit` and an arriving `x-transit`
the same header rather than a lookup that silently misses.

### `use ProofLifetime`

The longest lifetime a transit proof may declare.

**A server-chosen ceiling on somebody else's token**, and the reason it exists is that
`docs/adr/0014` calls a transit proof *short-lived* while the lifetime is entirely the fronting
component's to choose. Review demonstrated a proof with `exp` ten years out being accepted, and
accepted again on a replay of the identical token. So the deployment declares what it will call
short-lived, and a proof claiming more is refused.

Bounded above because a ceiling of a year is not a ceiling. Bounded below by one second, because a
zero would refuse every proof - a way of turning the mode off that reads like a tuning value.

### `use ResourceIdentifier`

What this deployment calls itself when it validates an audience.

**This is the security decision in `docs/adr/0014` given a type.** A token is accepted only if
its audience matches this value. A client may also *ask* its authorization server for a token
scoped to this resource - RFC 8707's resource indicator - and that is welcome and is an
optimisation: it makes the token narrower before it ever arrives. It is never what makes the
token safe. The check is ours, it is unconditional, and it is not skippable when the client sent
no indicator.

### `use SigningAlgorithm`

A signature algorithm this deployment will accept.

**No `none` and no `HS*`, and their absence is the enforcement.** Algorithm confusion is the
classic direct-validation defect and it is silent when it works: a token signed with `HS256`
using the issuer's *public* key as the HMAC secret verifies, if the validator will accept a
symmetric algorithm. There is no variant here that could.

### `use TokenType`

A `typ` header value this deployment will accept on a token.

**The type that closes cross-JWT substitution**, which is the finding it exists for: without a
`typ` check, *any* JWT the issuer signed with this audience verifies - and an OIDC ID token has the
same issuer and, whenever the resource identifier equals the client id, the same audience. That is
the ordinary identity-provider arrangement, so the substitution is not exotic. A verified ID token
would establish a caller from a document minted to describe a login rather than to authorize an API
call.

RFC 9068 section 4 requires `at+jwt` for the JWT access-token profile. RFC 8725 section 3.12 is the
wider rule and the reason this is configurable rather than hard-coded: an issuer using another
profile still has to give a deployment *some* way to distinguish token classes mechanically, and
which way that is is the deployment's fact rather than ours.

# What normalisation happens, and why exactly this much

Case is folded and a leading `application/` is stripped, both at construction. RFC 7515 section
4.1.9 says `typ` is a media type and that the `application/` prefix **may be omitted**, so
`at+jwt`, `AT+JWT` and `application/at+jwt` are three spellings of one value - and a comparison
treating them as three would refuse tokens that are correct. This is deliberately the opposite
decision from `ResourceIdentifier`, where nothing is normalised: an audience is an opaque string
an operator and an issuer configured identically, and a media type is a value a registry defines.

## Module `limits`

How many requests a caller gets, and the two tiers that answer differently.

**Rate limiting is not authentication.** It bounds how fast something can be done, not who
may do it, and on a surface with no per-caller identity - see `crate::security` - the key
it counts against is a network address rather than a principal. A shared egress address is
therefore one bucket for everybody behind it, and a caller with many addresses has many
buckets. Both of those are properties of the mechanism, not bugs in the configuration, and
neither is repaired by tightening the numbers.

What it does buy is real: it turns an unbounded loop against a data system into a bounded
one, and a question here is an aggregate over up to ten years of history, so the cost of one
request is not small.

**The switch follows the environment, and the refusal does not.** `rate_limit.enabled` has no
fixed default: it is off in development and test and on in production, the same shape
`telemetry.format` and `api.docs` already have here. That is a convenience in one direction only.
An explicit `false` in production is still a refusal to start - see
`crate::Settings::refusals` - because a default nobody had to write down and a control an
operator switched off are different facts, and the second one has to be visible.

**Which address the bucket is keyed on is a configuration decision, and it has to be.** The
peer address is unforgeable and is the proxy's for every request behind an ingress controller,
which is one bucket for the whole internet; a forwarded header is per-caller and is a value any
caller can write. `crate::proxy` is where that trade lives, and the refusal that keeps the
header from being believed without a named hop is in `crate::Settings::refusals`.

Two tiers, because the two surfaces have different shapes. The *probe* tier covers what a
caller may poll - liveness, and the generated `OpenAPI` document - and is tight, because nothing
there changes between two requests. The *api* tier covers the versioned API, where a legitimate
caller asks several questions in a row.

The names avoid the word that would be natural for the first tier, and not for a style reason:
`cargo xtask check-boundaries` flags a field whose name *begins* with `pub` as a public field,
because its scan is line-oriented and tests `starts_with("pub")`. A field called `public` is
therefore a gate failure on correct code. Renaming here is the cheap side of that trade, and the
names are more precise anyway - the first tier is not the unauthenticated one, since the
interface description sits behind the access token when one is configured.

### `struct Quota`

```rust
pub struct Quota
```

A sustained rate and the burst allowed above it.

Both non-zero: a quota of zero requests per second is a closed door, which is what
`enabled: false` says properly, and a zero burst is a limiter that rejects the first request
of every idle period.

#### Methods

```rust
pub const fn burst(self) -> NonZeroU32
```

```rust
pub const fn parse(name: &'static str, per_second: u32, burst: u32) -> Result<Self, InvalidQuota>
```

Reads a tier.

`name` is the configuration key this tier came from, so the error says which of the two
tiers is wrong rather than that one of them is.

```rust
pub const fn per_second(self) -> NonZeroU32
```

#### Implements

`Clone`, `Copy`, `Debug`, `Eq`, `PartialEq`

### `enum InvalidQuota`

```rust
pub enum InvalidQuota
```

Why a pair of numbers is not a quota.

#### Variants

- `Zero` - One of the two was zero.
- `BurstBelowRate` - The burst is below the sustained rate, which is a limiter that cannot sustain its own rate: the bucket refills faster than it can hold.

#### Implements

`Clone`, `Debug`, `Display`, `Eq`, `Error`, `PartialEq`

### `struct RateLimitSettings`

```rust
pub struct RateLimitSettings
```

Both tiers, the switch, and what a bucket is keyed on.

The switch is separate from the numbers on purpose. A deployment that turns limiting off is
making a decision, and it should be one word in a file rather than a quota set so high it
never fires - which reads as a configured limit and is not one.

**The switch itself has no fixed default; it follows `crate::Environment`.** Off on a laptop,
because a limiter that fires while somebody is iterating is a bug report about sutura that is
really a bug report about the tier; on in production, because that is the deployment an
unbounded caller costs something. The same shape as `telemetry.format` and `api.docs`, including
the recorded flag - see `Self::enabled_default_for`.

**Not `Copy`, and that is the trusted-proxy list.** It is a `Vec`, so this group is cloned
rather than copied and the accessors borrow. Every call site is inside the assembled router,
once, at startup.

#### Methods

```rust
pub const fn api(&self) -> Quota
```

The tier for the versioned API.

```rust
pub const fn client_address(&self) -> ClientAddressSource
```

Where the address a bucket is keyed on comes from.

```rust
pub const fn enabled(&self) -> bool
```

```rust
pub const fn enabled_default_for(environment: crate::Environment) -> bool
```

The default for an environment: on in production, off everywhere else.

**A default and not a refusal in one direction, and a refusal in the other.** Nothing here
stops a developer switching the limiter on, and nothing here decides production: an explicit
`enabled: false` in production is refused by
`crate::Settings::refusals` regardless of what this function
would have returned. The two must not be conflated - default-off in development is a
convenience, and silently-off in production is how a deployment loses a control nobody
noticed it had.

A total match rather than a comparison, so a fourth environment has to state its own answer
instead of inheriting whichever branch it happens to fall into.

`crate::Environment::Test` is grouped with development, and that is an argument rather than
a convenience. A test that exercises the limiter cannot rely on this value anyway: asserting
a refusal needs a quota small enough to exhaust in two requests, so such a test writes
`rate_limit.enabled` and a tier down together - which is what
`sutura-http`'s harness already does. So defaulting on here would buy no coverage, and would
charge every unrelated test in the suite a limiter it never asked for, at a tier
(`probe_burst: 5`) that a loop over a corpus of requests can exhaust. A limiter nothing exercises is
untested code; the answer to that is a test that names the switch, not a default that fires
during somebody else's assertion.

```rust
pub const fn enabled_was_explicit(&self) -> bool
```

Did an operator write the switch down, or did the environment decide it?

For the startup log, which says which arm was taken and whether anybody chose it.

```rust
pub const fn new(enabled: bool, enabled_was_explicit: bool, probe: Quota, api: Quota, client_address: ClientAddressSource, trusted_proxies: TrustedProxies) -> Self
```

```rust
pub const fn probe(&self) -> Quota
```

The tier for what an unauthenticated caller can reach.

```rust
pub const fn trusted_proxies(&self) -> &TrustedProxies
```

The hops whose forwarded header is believed.

#### Implements

`Clone`, `Debug`, `Eq`, `PartialEq`

## Module `prompt`

What goes into the agent-facing system prompt that this deployment hands out.

Two keys, and each one is read by something: `sutura_app::prompt::render` is the consumer, and
`sutura prompt` is the command that reaches it. That is a requirement rather than a remark - this
crate has shipped a group of keys that were parsed, range-checked, refused on a bad value and
consumed by nothing, and it was a finding. A key nobody reads reads as a control that is in
place.

# Why an operator can add to the prompt and cannot replace it

`PromptSettings::instructions_file` is layered *on top of* the derived text and appended as its
last section. There is deliberately no key that substitutes for the derived part.

The derived part carries the refusal guidance, which is the one thing an agent talking to this
surface most needs and least often has: a refusal arrives as a successful result, and an agent
that reads it as an outage retries until something works - which is precisely the behaviour the
refusal exists to prevent. A key whose worst setting silently deletes that paragraph would be a
key whose failure mode is invisible, and this crate's whole shape is arranged against those. If
wholesale replacement is ever wanted it should arrive as its own named key with its own argument,
not as an omission from this one.

# Why there is no environment-derived default here

`ApiSettings` and `LogFormat` default by
`Environment` and record whether an operator wrote the value down, so the
startup log can tell "somebody chose this" from "nobody did". Neither key here does, and the
reason is that neither decision is a function of the environment.

Whether a catalog's authors are trusted enough to quote their prose into an agent's context is a
fact about who writes the catalog, not about whether the process is on a laptop. A default that
dropped the prose in production would be worse than either fixed answer: an agent with no
descriptions does not stop, it infers a metric's meaning from its name and reports the inference.
And the interesting value - `CatalogProse::Omitted` - is never a default, so a deployment
running with it is visible from the value itself. That is the case an explicitness flag exists to
make legible, and here the value already is.

# Why a configured instructions file that is missing is not tolerated

The reference implementation this prompt is modelled on reads `<project>/instructions.md` when it
is there and silently omits the section when it is not, which is right for a *convention*: no
file means nobody wrote one. Here it is a *configured path*, so absence means the operator wrote
a path down and the file behind it is not there - and quietly serving a prompt without the
operator's rules in it would be the failure this crate refuses everywhere else. Existence is not
checked at parse time, for the reason `CatalogSettings` does
not check its directories: a check here is a claim that is already stale by the time the file is
read. The read is what fails, loudly, at the composition root.

### `enum CatalogProse`

```rust
pub enum CatalogProse
```

Whether the catalog's own prose is quoted into the prompt.

The word an operator writes. The type that does the work is
`sutura_app::prompt::CatalogProse`, and the split is the one
`LogFilter` already uses: this crate parses what was written down,
and the crate that acts on it owns the type that acts.

#### Variants

- `Quoted` - Quoted in, with `> ` at the start of every line and the trust boundary named above the block. The default.
- `Omitted` - Left out. For a deployment whose catalog authors are not the people who decide what its agents are told.

#### Methods

```rust
pub const fn as_str(self) -> &'static str
```

```rust
pub const fn is_quoted(self) -> bool
```

```rust
pub fn parse(raw: impl AsRef<str>) -> Result<Self, UnknownCatalogProse>
```

Reads the word.

#### Implements

`Clone`, `Copy`, `Debug`, `Default`, `Display`, `Eq`, `PartialEq`

### `struct UnknownCatalogProse`

```rust
pub struct UnknownCatalogProse
```

The word was neither spelling.

#### Implements

`Clone`, `Debug`, `Display`, `Eq`, `Error`, `PartialEq`

### `struct InstructionsFile`

```rust
pub struct InstructionsFile
```

Where the operator's own prompt text lives.

A newtype rather than a `PathBuf` so the one thing that can be wrong about it is wrong in one
place. The field is private and `Self::parse` is the only way in.

#### Methods

```rust
pub fn parse(raw: impl AsRef<str>) -> Result<Self, InvalidPromptSettings>
```

Reads the path.

Existence is deliberately not checked - see this module's documentation. The read at the
composition root is what fails when a configured file is not there, and it fails loudly
rather than omitting the section.

```rust
pub fn path(&self) -> &Path
```

#### Implements

`Clone`, `Debug`, `Display`, `Eq`, `PartialEq`

### `enum InvalidPromptSettings`

```rust
pub enum InvalidPromptSettings
```

Why the prompt configuration is not usable.

#### Variants

- `EmptyPath` - The path was present and empty.

#### Implements

`Clone`, `Debug`, `Display`, `Eq`, `Error`, `PartialEq`

### `struct PromptSettings`

```rust
pub struct PromptSettings
```

Everything that goes into the prompt beyond the pinned bundle and the tool list.

#### Methods

```rust
pub const fn catalog_prose(&self) -> CatalogProse
```

```rust
pub const fn instructions_file(&self) -> Option<&InstructionsFile>
```

The operator's own text, if a path was configured.

```rust
pub const fn new(instructions_file: Option<InstructionsFile>, catalog_prose: CatalogProse) -> Self
```

Assembles the group from parts that have each already been parsed.

Infallible, like the other groups here: there is no cross-field rule inside it. `omitted`
prose with no operator text is a coherent deployment - it says the metric names and the rules
and nothing about meaning - so it is a choice rather than a refusal.

#### Implements

`Clone`, `Debug`, `Default`, `Eq`, `PartialEq`

## Module `proxy`

Which address a request is counted against, and who is allowed to say what it is.

# The problem this module exists for

A rate limiter needs a key, and on a surface with no per-caller identity - see
`crate::security` - the only key available is a network address. There are two ways to learn
one and they fail in opposite directions.

The **peer address** is the far end of the TCP connection. It cannot be forged by a caller,
and behind a reverse proxy or an ingress controller it is the *proxy's* address for every
request that has ever arrived - so every caller on the internet shares one bucket. Either one
abusive caller limits everybody, or the quota is set high enough to be no limit at all.

A **forwarded header** carries the address the proxy saw. It is the right answer behind a
proxy and it is a value any caller can write, so on a service that is *not* behind one it
makes every bucket the caller's to choose - which is worse than one shared bucket, because it
is a limiter that reports a configured limit and bounds nothing.

Neither is correct on its own. What is correct is a header read **only from a peer the
operator named**, which is what `TrustedProxies` is: an explicit list, empty by default, and
`ClientAddressSource::Forwarded` refuses to start without one rather than trusting a
spoofable header because a key was left at its default.

# Which entry of the header is the caller

`X-Forwarded-For` is appended to, hop by hop, so it reads left to right as oldest to newest.
Everything to the left of what our own trusted proxy wrote is a value the caller supplied, and
a caller who writes `X-Forwarded-For: 10.0.0.1` gets that value read as their address by
anything that takes the leftmost entry.

So the walk is from the **right**: skip the entries that are addresses of trusted proxies, and
the first entry that is not one is the client. If every entry is a trusted proxy, or the list
runs out, or an entry is not an address at all, the peer address is used - it is the one value
in the request nobody but the network can choose.

### `struct Cidr`

```rust
pub struct Cidr
```

An address or a block of them, as an operator writes it.

`10.0.0.0/8` or a bare `10.0.0.7`, in either address family. A bare address is a block whose
prefix covers every bit, so there is one shape to match against rather than two.

#### Methods

```rust
pub fn contains(&self, address: IpAddr) -> bool
```

Is `address` inside this block?

Mixed families never match: a v4 block does not contain a v6 address, and the canonical
form applied on both sides is what keeps a v4 peer arriving over a dual-stack socket from
being one.

```rust
pub fn parse(raw: impl AsRef<str>) -> Result<Self, InvalidTrustedProxy>
```

Reads one entry.

#### Implements

`Clone`, `Copy`, `Debug`, `Display`, `Eq`, `PartialEq`

### `enum InvalidTrustedProxy`

```rust
pub enum InvalidTrustedProxy
```

Why a string is not an address block.

#### Variants

- `NotAnAddress` - The part before the slash was not an `IPv4` or `IPv6` literal.
- `PrefixNotANumber` - The part after the slash was not a number.
- `PrefixTooLong` - The prefix is longer than the address family has bits.

#### Implements

`Clone`, `Debug`, `Display`, `Eq`, `Error`, `PartialEq`

### `struct TrustedProxies`

```rust
pub struct TrustedProxies
```

The peers whose forwarded header this service will believe.

**Empty by default, and the emptiness is the safe posture rather than an unset value.** With
nothing in the list the peer address is the key, which is correct for a service with no proxy
in front. It becomes non-empty only when an operator names the hop.

#### Methods

```rust
pub const fn is_empty(&self) -> bool
```

Did the operator name any hop at all?

```rust
pub const fn len(&self) -> usize
```

How many blocks are trusted, for the startup log.

```rust
pub fn parse<S>(entries: &[S]) -> Result<Self, InvalidTrustedProxy>
```

Reads the configured list.

```rust
pub fn trusts(&self, address: IpAddr) -> bool
```

Is `address` one of the hops the operator named?

#### Implements

`Clone`, `Debug`, `Default`, `Eq`, `PartialEq`

### `enum ClientAddressSource`

```rust
pub enum ClientAddressSource
```

Where the address a request is counted against comes from.

#### Variants

- `Peer` - The far end of the connection. Unforgeable, and one bucket for everything behind a proxy.
- `Forwarded` - `X-Forwarded-For`, read only from a peer in `TrustedProxies`, falling back to the peer address for anything else.

#### Methods

```rust
pub const fn as_str(self) -> &'static str
```

The spelling, for the startup log.

```rust
pub fn parse(raw: impl AsRef<str>) -> Result<Self, UnknownClientAddressSource>
```

Reads the configured value.

```rust
pub const fn reads_a_header(self) -> bool
```

Does this source read a caller-supplied header?

#### Implements

`Clone`, `Copy`, `Debug`, `Default`, `Display`, `Eq`, `PartialEq`

### `struct UnknownClientAddressSource`

```rust
pub struct UnknownClientAddressSource
```

The configured value did not name a source.

#### Implements

`Clone`, `Debug`, `Display`, `Eq`, `Error`, `PartialEq`

## Module `runtime`

How much work may be in flight at once, how wide the engine runs, and how long stopping may
take.

# Why this is its own group and not four more `server` keys

Because it bounds a different thing from anything in `crate::server`. Those keys are about one
request: where it arrives, how big it may be, how long the caller waits for a reply. These are
about the *process*: how many questions execute at once, how wide the engine that executes them
is, and what the budget for winding down is.

The distinction matters most for the timeout. `server.request_timeout_seconds` is a deadline on
the **reply**, not a bound on the **work**. When it expires the caller is answered `408` and the
handler future is dropped - and a started `tokio::task::spawn_blocking` task cannot be aborted,
so the question keeps running. Without something else in the picture, a caller asking questions
that cost more than the timeout gets a fast turnaround while the deployment keeps the whole
cost, and in-flight work accumulates at the rate limit with nothing shedding it. The blocking
pool defaults to 512 threads with an unbounded queue, so that backlog is bounded by memory.

`QueryConcurrency` is the bound on the work. It is the number of questions that may be
*executing*, and the permit is held by the blocking task rather than by the handler future - so
a timed-out request does not hand its slot back until the work it started actually finishes.
That is what makes the backlog a number somebody chose.

`AdmissionTimeout` stops the queue in front of that bound from being a second unbounded thing.
A question that cannot get a slot inside it is refused rather than left waiting, and a refused
waiter costs a dropped future rather than a thread.

`EngineWorkers` is not a bound at all - it is a width. The in-process engine drives its own
runtime and blocks on it, so a single-threaded one is a contention point every concurrent
question shares. See `sutura_exec_datafusion::DataFusionWarehouse`.

`WorkingSetCeiling` is the bound that did not exist. Nothing built a `RuntimeEnv`, so the engine
installed its unbounded memory pool - and under `panic = "abort"` a hash join wide enough to
outgrow the machine is the process ending for every caller in flight rather than an error for the
one who asked. It is a **query-wide** value with no per-source override, and the reason it is not
symmetric with the deadline is on the type.

`ShutdownGrace` is the budget for stopping, and it covers the whole of stopping rather than
the connection drain alone.

Every type here has the shape of `crate::server::RequestTimeout`, which is the pattern this
crate already had: a private field, one `parse` that is the only constructor, a ceiling the type
declares, and a zero refused rather than read as `no limit`. They share
`InvalidBound` rather than growing a second error, so a bad value here lands in the same
`SettingsError` variant a bad server bound does.

### `struct QueryConcurrency`

```rust
pub struct QueryConcurrency
```

How many questions may be executing at once.

Not how many may be *in flight*: a request waiting for a slot, parsing a body or writing a
response is not counted. This is the number of questions holding a blocking-pool thread and a
data system, which is the resource that runs out.

#### Methods

```rust
pub const fn count(self) -> usize
```

```rust
pub const fn parse(count: usize) -> Result<Self, InvalidBound>
```

Reads a concurrency bound.

#### Implements

`Clone`, `Copy`, `Debug`, `Eq`, `PartialEq`

### `struct AdmissionTimeout`

```rust
pub struct AdmissionTimeout
```

How long a question may wait for one of those slots.

Bounded because the alternative is a queue nothing empties. Shorter than the request timeout by
default, on purpose: a caller who has waited five seconds for a slot is better served by a
`503` they can retry than by a `408` twenty-five seconds later that says the same thing less
clearly.

#### Methods

```rust
pub const fn duration(self) -> Duration
```

```rust
pub const fn parse(seconds: u64) -> Result<Self, InvalidBound>
```

Reads a wait in whole seconds.

```rust
pub const fn seconds(self) -> u64
```

#### Implements

`Clone`, `Copy`, `Debug`, `Eq`, `PartialEq`

### `struct EngineWorkers`

```rust
pub struct EngineWorkers
```

How many threads the in-process engine's own runtime gets.

Resolved to a number at load time rather than kept as "whatever the machine has", so the value
in the startup log is the value in effect. An absent key follows
`std::thread::available_parallelism`; a present one wins, which is what a container with a CPU
quota needs - `available_parallelism` reports what the kernel exposes, and on most container
runtimes that is the host's core count rather than the cgroup's share.

#### Methods

```rust
pub const fn count(self) -> usize
```

```rust
pub fn parse(configured: Option<usize>) -> Result<Self, InvalidBound>
```

Reads a worker count, or resolves the absent one.

Not a `const fn`, unlike its neighbours, because the absent case asks the operating system
how many threads this machine can run at once. A machine that will not answer is treated as
one, which is the behaviour this adapter had before the key existed rather than a failure to
start.

```rust
pub const fn was_chosen(self) -> bool
```

Did an operator write this number, or did the machine?

For the startup log, and for the same reason `api.docs` records it: an operator reading
`engine_worker_threads = 8` needs to know whether that was their decision.

#### Implements

`Clone`, `Copy`, `Debug`, `Eq`, `PartialEq`

### `struct WorkingSetCeiling`

```rust
pub struct WorkingSetCeiling
```

How many bytes the engine's operators may reserve at once, across the whole process.

**This is the bound that did not exist, and its absence was process death.** Nothing constructed
a `RuntimeEnv`, so the engine installed its unbounded memory pool: a hash join or an aggregate
wide enough to outgrow the machine allocated until the allocator failed, and shipped profiles
compile `panic = "abort"`, so that is not an error for the caller who asked - it is the process
ending for every caller in flight. A bounded pool turns it into a reservation that fails, which
leaves as `RefusalReason::ResourcesExhausted`.

# What it counts, and what it does not

The pool counts what the engine's own operators reserve - a hash-join build side, aggregate
state, a sort - and nothing else. **Not** what a driver buffers before conversion, **not**
`collect()` materialising every batch, **not** the row set built while a result is converted into
domain rows. So this is not a bound on the process's memory and must not be read as one: a
question large enough to end the process on one of those paths still ends it. The bound that
reaches those is a byte budget applied as rows are converted, which
`docs/adr/0009-the-plan-from-one-source-to-many.md` puts with the execution boundary rather than
here.

# Global, and no per-source override

There is one combiner and one working set, so a per-source ceiling would be a number with nothing
to bound - and 0009 decides that a source declaration carrying one is **refused at parse rather
than ignored**, because a setting that silently does nothing is worse than a missing one.
`deny_unknown_fields` on every on-disk shape is the mechanism, and the test that provokes it is in
`crate::settings`. **The deadline is the bound that takes a per-source override; this one does
not**, and the two are decided separately on purpose.

# Never spill

Decided rather than defaulted, and the second reason is what settles it. A refusal the caller sees
beats a degraded answer it cannot; and spilling writes the *asking subject's rows* to the pod's
local disk, an ungoverned data-at-rest surface, on the one path whose whole purpose is that a
query runs as the person who asked. So no spill directory and no disk sizing - the adapter builds
its runtime with temporary files disabled, and `sutura_exec_datafusion::WorkingSet` is where that
is written down.

# A provisional number

`Self::DEFAULT_BYTES` is a gibibyte and nobody has measured it. It is a starting point recorded
as one, not a finding.

#### Methods

```rust
pub const fn bytes(self) -> core::num::NonZeroUsize
```

The ceiling, for whatever builds the pool.

```rust
pub const fn checked_against(self) -> Option<u64>
```

What this ceiling was compared against at boot, if the platform would say.

For the startup log, and it is the honest half of the claim: a `None` here means nothing
verified that the configured ceiling is reachable, so an over-configured deployment on such a
platform starts and dies later rather than refusing now.

```rust
pub fn parse(bytes: u64, available: Option<u64>) -> Result<Self, InvalidBound>
```

Reads a ceiling in bytes, against what the process can actually reach.

`available` is passed in rather than probed here, and that is deliberate twice over: it makes
this function total and testable - the interesting case is a machine nobody has - and it keeps
the one place that reads `/proc` and `/sys` separate from the one that decides. `None` means
the platform would not say, and then the comparison is not made; `available_memory_bytes`
says which platforms those are.

Bytes and not a suffixed string, for the reason `crate::server::RequestTimeout` takes whole
seconds: a parser for `1GiB` is a second grammar for a single value, and the two spellings of
a gibibyte that differ by 7% are exactly the confusion it would introduce.

Not a `const fn`, unlike most of its neighbours and for the same shape of reason
`EngineWorkers::parse` is not: the width check below is a `TryFrom`, which is not yet const.
An `as` cast would be const and would truncate silently on the one target where the check
matters, which is the wrong direction for a bound.

#### Implements

`Clone`, `Copy`, `Debug`, `Eq`, `PartialEq`

### `struct ShutdownGrace`

```rust
pub struct ShutdownGrace
```

How long stopping may take, once stopping has been asked for.

Chosen against the deadline on the other side rather than as a round number. An orchestrator
sends a termination signal and starts a kill timer - the usual window is thirty seconds - and a
process still running when that expires is killed mid-answer, so whatever it would have done on
the way out does not happen. Fifteen seconds leaves room for the exit itself.

It bounds the *whole* of stopping and not the connection drain alone. The drain gets the budget
first; what is left of it is what the runtime will wait for a blocking task it cannot cancel.
See `sutura_runtime::Shutdown::remaining_grace`.

#### Methods

```rust
pub const fn duration(self) -> Duration
```

```rust
pub const fn parse(seconds: u64) -> Result<Self, InvalidBound>
```

Reads a grace period in whole seconds.

```rust
pub const fn seconds(self) -> u64
```

#### Implements

`Clone`, `Copy`, `Debug`, `Eq`, `PartialEq`

### `struct RuntimeSettings`

```rust
pub struct RuntimeSettings
```

Everything about how much runs at once and how the process stops.

#### Methods

```rust
pub const fn admission_timeout(self) -> AdmissionTimeout
```

```rust
pub const fn engine_workers(self) -> EngineWorkers
```

```rust
pub const fn max_concurrent_queries(self) -> QueryConcurrency
```

```rust
pub const fn new(max_concurrent_queries: QueryConcurrency, admission_timeout: AdmissionTimeout, engine_workers: EngineWorkers, working_set: WorkingSetCeiling, shutdown_grace: ShutdownGrace) -> Self
```

Assembles the group from parts that have each already been parsed.

Infallible, like `crate::server::ServerSettings::new` and for the same reason: there is no
cross-field rule inside this group. The one relationship worth knowing - that an admission
timeout above the request timeout is never reached - spans two groups and is documented on
`AdmissionTimeout::MAX_SECONDS` rather than refused.

```rust
pub const fn shutdown_grace(self) -> ShutdownGrace
```

```rust
pub const fn working_set(self) -> WorkingSetCeiling
```

How many bytes the engine's operators may reserve at once.

#### Implements

`Clone`, `Copy`, `Debug`, `Eq`, `PartialEq`

### `fn available_memory_bytes`

```rust
pub fn available_memory_bytes() -> Option<u64>
```

How many bytes this process can actually reach, when the platform will say.

**Three sources, smallest wins**, because they answer three different questions and the binding
one is whichever is tightest:

1. `/sys/fs/cgroup/memory.max` - the cgroup v2 limit, and the number that matters in a container.
2. `/sys/fs/cgroup/memory/memory.limit_in_bytes` - the same under cgroup v1, where "unlimited" is
   a sentinel near `u64::MAX` rather than a word, which is why taking the minimum with the
   machine total is what disarms it rather than a comparison against the sentinel.
3. `MemTotal` in `/proc/meminfo` - the machine, for a process with no cgroup limit.

**`None` on any platform that has none of these, and that is a limit on the claim rather than a
fallback.** macOS is such a platform: nothing here reads `sysctl`, so a laptop makes no boot check
at all and an over-configured ceiling there starts and dies later. The shipped artifacts are Linux,
which is where the check has to hold - and `WorkingSetCeiling::checked_against` is what lets the
startup log say which of the two happened rather than implying the check was made.

It reads files and cannot fail: an unreadable or unparsable source contributes nothing rather than
refusing to start, because the expensive failure here is a deployment that will not boot on a
kernel laid out differently, and the cheap one is a boot check that did not run and said so.

## Module `security`

The access control this service has, and the honest name for what each part of it is not.

**Two controls that answer two different questions, and collapsing them is the mistake this
module is arranged against:**

- the `AccessToken`: *may this caller reach this service at all*
- `InboundIdentity`: *who is asking*

What an `AccessToken` does is narrower than authentication, and the narrowness is the point: a
presented token proves the caller holds a secret the deployment was configured with. It proves
nothing about *which* caller, it cannot be scoped, it cannot be revoked for one party without
revoking it for all of them, and it does not reach the data system. It is worth having anyway,
because the alternative on a non-loopback interface is an unauthenticated way to read whatever the
process can read - and it is not worth mistaking for identity.

`SecuritySettings::describes_identity` is what keeps that distinction printable. It used to be
an associated function that always answered `false`; `crate::inbound` is what made it a value, and
the startup log prints it on every boot so an operator cannot deploy either shape believing it is
the other.

**The limit that survives all of it:** a caller whose identity is established is still a caller
whose questions run with whatever access this process already had.
`InboundIdentity::what_it_does_not_do` is that sentence, and the startup log prints it beside the
mode rather than leaving a reader to infer it.

### `struct AccessToken`

```rust
pub struct AccessToken
```

A pre-shared secret a caller presents to reach the service.

The configured token is reduced to its SHA-256 digest at parse and nothing else is retained, so
the whole settings tree can be written to the startup log with `Debug` and the token cannot
come out with it. `Debug` prints a placeholder for the same reason `Secret`'s does.

**Not comparable with `==`.** `AccessToken` implements no `PartialEq`: a derived comparison on
credential material returns on the first differing byte, which is a timing oracle at whatever
call site adds it. The comparison lives here instead, once, as
`AccessToken::matches_in_constant_time`.

#### Methods

```rust
pub fn equals(&self, other: &Self) -> bool
```

Whether this token is the same as another configured token.

**The one comparison two configured credentials need, and it is not the value-comparison a
`PartialEq` would be.** Both sides are already digests of at-rest configuration, so neither
is an attacker-presented value arriving at a timing-sensitive boundary; comparing them at
boot with `subtle`'s constant-time equality keeps even that much out. It exists because
`docs/adr/0015` Decision 1 refuses a metrics token equal to the API token, and the refusal
needs the two digests compared once, at startup.

```rust
pub fn matches_in_constant_time(&self, presented: &str) -> bool
```

Does `presented` equal the configured token?

Named for the property rather than for the operation, because the property is the only
reason this function exists rather than a `==`.

**The expected side is the digest stored at parse, and the presented side is hashed here.**
`subtle` compares equal-length byte slices without branching, which removes the
early-return oracle - but comparing the raw strings would still have to decide what to do
about differing lengths, and every answer to that leaks the length before it leaks anything
else. Reducing both sides to a fixed 32 bytes removes the question: every comparison is over
the same number of bytes whatever arrived, and the configured token is never re-hashed per
request because it is already a digest.

What this still does not do: it is not a password hash. There is no salt and no work
factor, because the input is a high-entropy secret an operator generated rather than
something a person chose, and nothing here is stored for an attacker to find offline.

```rust
pub fn parse(raw: impl AsRef<str>) -> Result<Self, InvalidAccessToken>
```

Reads a configured token.

**Parses the wire grammar, not merely a length.** The type is named for a value that
arrives in an `Authorization` header, so what it accepts is what such a header can carry:
see `Self::wire_grammar` and `InvalidAccessToken::NotRepresentableOnTheWire`.

#### Implements

`Clone`, `Debug`

### `enum InvalidAccessToken`

```rust
pub enum InvalidAccessToken
```

Why a string is not usable as an access token.

#### Variants

- `TooShort` - Shorter than `AccessToken::MIN_LENGTH`.
- `TooLong` - Longer than `AccessToken::MAX_LENGTH`.
- `Untrimmed` - Whitespace at either end, which is almost always a copy-paste artefact and would otherwise make every request fail for a reason nobody can see in a log.
- `NotRepresentableOnTheWire` - A character no `Authorization` header could carry to us.

#### Implements

`Clone`, `Debug`, `Display`, `Eq`, `Error`, `PartialEq`

### `enum TlsTermination`

```rust
pub enum TlsTermination
```

Where TLS is terminated for this deployment.

**A declaration, not a control.** Nothing here encrypts anything except
`Self::InProcess`; the other three name a terminator that lives somewhere else, and the point
of writing it down is that the *cleartext hop* it implies is then a stated fact rather than an
assumption. The bearer token crosses that hop in the clear, and how far the hop reaches is the
whole difference between the three:

| Declared | What terminates TLS | What the token crosses in cleartext |
| --- | --- | --- |
| `none` | nothing | the whole path from the caller. Only sane on loopback |
| `sidecar` | a proxy in this pod | a loopback hop inside the pod |
| `ingress` | an ingress controller or gateway | the pod network, from that hop to this process |
| `in-process` | this process | nothing - the connection ends here |

So `ingress` is not a weaker `sidecar`: it is the same posture with a longer cleartext segment,
and whether that segment is acceptable is a question about the cluster network - a mesh with
mutual TLS between pods answers it differently from a flat one. This type does not pretend to
know, and a startup log that said "TLS enabled" would be pretending.

#### Variants

- `None` - Nothing terminates TLS. Plaintext from the caller to here.
- `Sidecar` - A terminator inside this pod or on this host, reached over loopback.
- `Ingress` - An ingress controller or gateway. The hop from it to this process crosses the pod network.
- `InProcess` - This process. Requires the `tls` feature and a certificate and key.

#### Methods

```rust
pub const fn as_str(self) -> &'static str
```

The spelling, for the startup log.

```rust
pub const fn cleartext_hop(self) -> &'static str
```

The cleartext hop this declaration implies, as a sentence for the startup log.

A function rather than a comment for the same reason
`SecuritySettings::describes_identity` is one: the log, the documentation and this type
read the same value, so none of them can drift into claiming end-to-end encryption.

```rust
pub const fn is_declared(self) -> bool
```

Was anything said at all?

`Self::None` is the default, so "not declared" and "declared as nothing" are the same
value - which is why the refusal for a non-loopback bind is keyed on this rather than on an
`Option`. An operator who means plaintext on loopback writes nothing and gets it.

```rust
pub fn parse(raw: impl AsRef<str>) -> Result<Self, UnknownTlsTermination>
```

Reads the configured value.

```rust
pub const fn terminates_here(self) -> bool
```

Does this process hold the TLS connection itself?

#### Implements

`Clone`, `Copy`, `Debug`, `Default`, `Display`, `Eq`, `PartialEq`

### `struct UnknownTlsTermination`

```rust
pub struct UnknownTlsTermination
```

The configured value did not name a place TLS is terminated.

#### Implements

`Clone`, `Debug`, `Display`, `Eq`, `Error`, `PartialEq`

### `enum DeploymentIdentity`

```rust
pub enum DeploymentIdentity
```

Which kind of deployment this is, and therefore where a shared source's acknowledgement may come
from.

**Two modes that differ in kind rather than in degree, and the deployment DECLARES which it is.**

*Single-user* means credentials are static configuration: one user, one host, not multi-tenant.
There is no per-request identity to establish, so a shared source is correct for **everything** -
the one user reads all, by design, and the configured credential is that user's own.
`examples/single-player` is this, and it is a first-class deployment rather than a degraded one.

*Multi-user* means the caller's identity arrives per request. Shared sources are still permitted,
and that is the whole difficulty: the deployment has to say so **per source**, on purpose.

# It is declared and never derived, and the derivation that was on offer is unsound

The tempting derivation is "every source shared means single-user, any source impersonating means
multi-user". It fails in exactly the configuration that most needs the check: a genuinely
multi-tenant deployment whose sources are *all* shared derives to single-user, and the
acknowledgement is required in multi-user mode only - so the derivation would exempt from the
acknowledgement the one deployment where every caller reads every source as somebody else's
identity. The failure is silent, it is one user's data served to another, and it arrives by leaving
a field out.

So there is **no `Default`**, no derivation, and a deployment that configures a source without
declaring the mode does not boot -
`NotFitToServe::DeploymentIdentityUndeclared`.
The refusal is keyed on a source being configured rather than raised unconditionally, and that is
not a softening: a deployment with no source configured cannot answer anything, and the composition
root refuses it on the catalog naming a source with no declaration - so every deployment that can
serve a question has to declare the mode.

# What flipping the mode does

It re-evaluates every source. A single-user deployment legitimately holds every source under one
static credential; the same file in multi-user mode serves every one of those sources to every
caller as one identity. The mode is an input to the whole check rather than to an incremental view
of what changed, so a deployment that flips it and has acknowledged nothing does not boot.

# The variant names are not the configured words, and that is deliberate

A deployment writes `single-user` or `multi-user` - `Self::as_str` and `Self::NAMES` own those
spellings, and they are the vocabulary
[a credential per leg](https://github.com/telekom/sutura/blob/main/docs/adr/0008-a-credential-per-leg-for-the-calling-subject.md)
5a names. The variants are named for the *property each mode decides* instead, because
`SingleUser`/`MultiUser` share a postfix and `clippy::enum_variant_names` is denied - and the names
that survived that say more: what changes between the two is whether credentials are static
configuration or a subject arrives per request.

#### Variants

- `StaticCredentials` - Static credentials, one user, one host - the `single-user` mode. Carries the operator's own reason, so the mode is unreachable by leaving a key out.
- `SubjectPerRequest` - A subject per request, established by the transport - the `multi-user` mode.

#### Methods

```rust
pub const fn as_str(&self) -> &'static str
```

The spelling, for the startup log.

```rust
pub const fn needs_per_source_acknowledgement(&self) -> bool
```

Does a shared source need an acknowledgement on its own entry under this mode?

```rust
pub fn parse(word: &str, reason: Option<&str>) -> Result<Self, InvalidDeploymentIdentity>
```

Reads the declared mode and, for single-user, the operator's reason.

The reason is **required** for single-user and **refused** for multi-user, which is the same
rule `server.tls_certificate` gets: a value nothing reads is a control that appears to be in
place. Both halves are returned as one typed error rather than checked later, because the mode
and its witness are one declaration.

```rust
pub const fn shared_witness(&self) -> Option<&AcknowledgementReason>
```

The reason a shared source may borrow as its acknowledgement, if this mode supplies one.

`Some` for single-user only, and an exhaustive match rather than an `is_single_user()` boolean:
what the mode contributes is the *witness*, so returning the value is what a caller needs and a
boolean would leave every caller to work out where the witness comes from.

#### Implements

`Clone`, `Debug`, `Eq`, `PartialEq`

### `struct UnknownDeploymentIdentity`

```rust
pub struct UnknownDeploymentIdentity
```

The configured value did not name a deployment mode.

#### Implements

`Clone`, `Debug`, `Display`, `Eq`, `Error`, `PartialEq`

### `enum InvalidDeploymentIdentity`

```rust
pub enum InvalidDeploymentIdentity
```

Why a deployment mode declaration is not usable.

#### Variants

- `Unknown`
- `SingleUserWithoutAReason` - Single-user mode with no reason written.
- `ReasonWithoutSingleUser` - A single-user reason on a multi-user deployment, where nothing would read it.
- `Reason`

#### Implements

`Clone`, `Debug`, `Display`, `Eq`, `Error`, `PartialEq`

### `struct SecuritySettings`

```rust
pub struct SecuritySettings
```

The access posture, and the declaration that goes with a non-loopback bind.

Two fields rather than one, because they answer different questions and collapsing them was
the tempting mistake: a token says *who may reach this*, and the declaration says *what, if
anything, encrypts the path it travels*. A deployment that sets a token but binds the wildcard
with nothing in front has answered only the first.

**The declaration replaced a boolean, and that is the point of it.** The boolean it replaced -
`expose_beyond_loopback` - recorded that somebody meant to publish the service and said nothing
about what protects the token in flight, so a wildcard bind with no terminator anywhere read
exactly like one behind a gateway. A value naming the terminator cannot be satisfied by
agreeing that off-host is intended.

**Five fields now, with the last three answering separate deployment questions.**
`Self::inbound` is how the identity of a *caller* reaches this deployment; `Self::identity` is
who a query then runs *as*; `Self::metrics_token` independently gates `/metrics`. **Neither
inbound nor identity implies the other, and that is the fact worth writing down rather than the
count:** a deployment can verify exactly who is asking and still read every row under one
configured identity, because leg 2 - a credential per execution leg - is not built. The reverse
holds too, and is the shape that ships: a single-user deployment with no inbound block knows what
a query runs as and nothing about who asked. Omitting the metrics token is likewise an explicit
choice not to gate that route; it does not alter either query authentication or caller identity.

The inbound declaration lives in this group rather than one of its own because of
`Self::describes_identity`: that function used to be a constant answering `false`, and a
deployment that establishes a caller identity has to be able to make it answer otherwise from a
value rather than from a rewrite. The deployment declaration is an `Option` for a different
reason - it has no default and its absence is a refusal rather than a value; see
`DeploymentIdentity`, which explains why no combination of source postures may answer it on the
operator's behalf.

#### Methods

```rust
pub const fn access_token(&self) -> Option<&AccessToken>
```

The configured token, if there is one.

```rust
pub const fn describes_identity(&self) -> bool
```

Does anything here establish who the caller is?

**This stopped being a constant, which is the change `docs/adr/0014` predicted.** It was an
associated function that always answered `false`, with a comment saying it would change when a
request context and an inbound credential existed. They exist, so it reads a value: `true`
exactly when an inbound declaration is configured, and `false` for the deployment token alone -
which authenticates the deployment and not the caller, whatever else is set.

It is still a function rather than a comment so the startup log and this documentation read
the same value.

**`Self::identity` does not change this answer either, and that is deliberate.** A declared
`multi-user` mode says what the deployment *intends* and decides where a shared source's
acknowledgement has to be written; it does not make a caller identity arrive. Reading the
declaration back as "this deployment knows who is asking" is the exact confusion this function
exists to prevent, and the two keys are independent for that reason.

**The limit, next to the claim:** `true` here says a caller's identity is *established*. It
does not say a data source executes as that caller - see
`InboundIdentity::what_it_does_not_do`, which the startup log prints beside this.

```rust
pub const fn identity(&self) -> Option<&DeploymentIdentity>
```

Which kind of deployment this is, if the operator declared one.

```rust
pub const fn inbound(&self) -> Option<&InboundIdentity>
```

How the identity of a caller reaches this deployment, if it does.

`None` is the shape that ships today and the shape a single-player deployment keeps: the
bearer token authenticates the deployment, and there is no per-request identity to establish.

```rust
pub const fn inbound_mode(&self) -> &'static str
```

Which mode establishes the caller's identity, as a word for the startup log.

`"none"` rather than an `Option` because the caller is a log line, and a field that is
sometimes absent reads as a field that is sometimes broken.

```rust
pub const fn metrics_token(&self) -> Option<&AccessToken>
```

The token that gates `/metrics`, when one is configured.

```rust
pub const fn new(access_token: Option<AccessToken>, tls_termination: TlsTermination, inbound: Option<InboundIdentity>, identity: Option<DeploymentIdentity>, metrics_token: Option<AccessToken>) -> Self
```

Assembles the group from parts that have each already been parsed.

The inbound declaration is an `Option` because its absence is a posture rather than a gap: a
deployment that establishes no per-caller identity is a single-player deployment, which
`docs/adr/0008` part 5a calls a first-class shape. What is *not* optional is saying which mode,
once a block exists at all - and that refusal lives in `crate::settings::parse_inbound`,
because the shape here cannot hold "a mode nobody named".

The metrics token is an `Option` the same way: a deployment that chooses not to gate
`/metrics` is making a posture, not leaving a gap.

```rust
pub const fn tls_termination(&self) -> TlsTermination
```

Where the operator said TLS is terminated.

```rust
pub const fn token_state(&self) -> &'static str
```

Whether a token is configured, as a word for the startup log.

A method and not a `Debug` of the option, so the log line cannot become the token by
somebody changing the field type later.

#### Implements

`Clone`, `Debug`, `Default`

## Module `server`

Where the service listens, and the two bounds every request is held to.

The interesting type here is `BindAddress`, and it is interesting for a security reason
rather than an ergonomic one. This service has no per-caller identity - see the module
documentation on `crate` - so the interface it listens on is the whole of its perimeter.
Making the bind an IP address rather than a string is what lets
`BindAddress::is_loopback` be a fact the startup refusals can read, instead of a substring
test somebody has to remember to write.

### `struct BindAddress`

```rust
pub struct BindAddress
```

The socket the service listens on.

An `IpAddr` and a port, never a hostname. A hostname is refused rather than resolved: a name
resolves to whatever the resolver says today, which may be a public interface tomorrow, and
a perimeter that moves when DNS moves is not a perimeter. The operator writes the interface
they mean.

#### Methods

```rust
pub const fn is_loopback(self) -> bool
```

Is this address reachable only from this host?

The question the production refusals are keyed on. The two wildcard addresses answer
`false`, which is the answer that matters: the wildcard is the bind that reaches every
interface, and it is the one an operator reaches for without meaning to publish anything.

```rust
pub fn parse(host: impl AsRef<str>, port: u16) -> Result<Self, InvalidBindAddress>
```

Reads a host and port.

```rust
pub const fn port(self) -> u16
```

The port, so a caller can report the one it actually got.

```rust
pub const fn socket(self) -> SocketAddr
```

The address, for whatever binds the socket.

#### Implements

`Clone`, `Copy`, `Debug`, `Display`, `Eq`, `PartialEq`

### `enum InvalidBindAddress`

```rust
pub enum InvalidBindAddress
```

Why a host and port are not an address to listen on.

#### Variants

- `NotAnIpAddress` - The host was not an `IPv4` or `IPv6` literal.

#### Implements

`Clone`, `Debug`, `Display`, `Eq`, `Error`, `PartialEq`

### `struct RequestTimeout`

```rust
pub struct RequestTimeout
```

How long one request may take before the service gives up on it.

Bounded at both ends. Zero is a service that answers nothing, and an hour is a connection
held open long enough that a handful of them are the outage: a question here is one
aggregate over a bounded range, so a minute is already generous and five is the ceiling.

#### Methods

```rust
pub const fn duration(self) -> Duration
```

```rust
pub const fn parse(seconds: u64) -> Result<Self, InvalidBound>
```

Reads a timeout in whole seconds.

Seconds and not a duration string: sub-second precision is meaningless for a bound this
coarse, and a parser for a suffixed number is a second grammar for a single value.

```rust
pub const fn seconds(self) -> u64
```

#### Implements

`Clone`, `Copy`, `Debug`, `Eq`, `PartialEq`

### `struct BodyLimit`

```rust
pub struct BodyLimit
```

The largest request body the service will read.

A modelled question is a metric name, a grain, two dates and at most four dimensions, which
is a few hundred bytes. The bound exists because a body limit is the cheapest availability
control there is, and because the default in most stacks is whatever arrives.

#### Methods

```rust
pub const fn bytes(self) -> usize
```

```rust
pub const fn parse(bytes: usize) -> Result<Self, InvalidBound>
```

Reads a body limit in bytes.

#### Implements

`Clone`, `Copy`, `Debug`, `Eq`, `PartialEq`

### `struct ShutdownGrace`

```rust
pub struct ShutdownGrace
```

How long stopping may take, once stopping has been asked for.

Chosen against the deadline on the other side rather than as a round number. An orchestrator
sends a termination signal and starts a kill timer - the usual window is thirty seconds - and a
process still running when that expires is killed mid-answer, so whatever it would have done on
the way out does not happen. Fifteen seconds leaves room for the exit itself.

It bounds the *whole* of stopping and not the connection drain alone: the drain gets the budget
first, and what is left of it is what the runtime will wait for a blocking task it cannot
cancel. See `sutura_runtime::Shutdown::remaining_grace`.

Zero is refused for the reason every bound in this module is. A zero grace period means "drop
every in-flight answer immediately", which is a decision somebody would write differently.

#### Implements

`Clone`, `Copy`, `Debug`, `Eq`, `PartialEq`

### `enum InvalidBound`

```rust
pub enum InvalidBound
```

Why a bound is not a bound.

#### Variants

- `Zero` - Nothing here may be zero: a zero timeout answers nothing and a zero body limit accepts nothing, and both read as no limit at all to somebody writing the file.
- `TooLarge` - Above the ceiling this type declares.
- `AboveAvailableMemory` - Above the memory this process can actually reach.

#### Implements

`Clone`, `Debug`, `Display`, `Eq`, `Error`, `PartialEq`

### `struct TlsMaterial`

```rust
pub struct TlsMaterial
```

A certificate chain and the private key that goes with it, as paths.

**Paths and nothing more, and the split is deliberate.** This crate holds no framework and
reads no files: it parses the *pair* - both halves or neither - and stops there. Whether the
files are readable, whether they are PEM at all, and whether the key matches the certificate
are questions only the TLS implementation can answer, so they are answered once, in
`sutura_http::tls`, before the socket is bound. Two checks in two crates would be two messages
for one mistake, and the weaker one would be the reassuring one.

#### Methods

```rust
pub fn certificate(&self) -> &Path
```

The certificate chain, in PEM.

```rust
pub fn key(&self) -> &Path
```

The private key, in PEM.

```rust
pub fn parse(certificate: Option<&str>, key: Option<&str>) -> Result<Option<Self>, InvalidTlsMaterial>
```

Reads the pair, or nothing.

`None` for both is "no in-process TLS", which is the default and is not an error.

#### Implements

`Clone`, `Debug`, `Eq`, `PartialEq`

### `enum InvalidTlsMaterial`

```rust
pub enum InvalidTlsMaterial
```

Why a pair of paths is not usable TLS material.

#### Variants

- `OnlyOneHalf` - One half was given and the other was not.
- `EmptyPath` - A path was given as an empty string, which is not a path.

#### Implements

`Clone`, `Debug`, `Display`, `Eq`, `Error`, `PartialEq`

### `struct ServerSettings`

```rust
pub struct ServerSettings
```

Everything about the socket, the two per-request bounds, and the TLS material if there is any.

**Not `Copy`, and that is the TLS paths.** Every accessor borrows or returns a `Copy` value, and
the group itself is read once, at assembly time.

#### Methods

```rust
pub const fn bind(&self) -> BindAddress
```

```rust
pub const fn max_body(&self) -> BodyLimit
```

```rust
pub const fn new(bind: BindAddress, request_timeout: RequestTimeout, max_body: BodyLimit, tls: Option<TlsMaterial>) -> Self
```

Assembles the group from parts that have each already been parsed.

Infallible, and that is the shape the newtypes buy: there is no cross-field rule inside
this group, so once every part exists the group exists. The cross-field rules - the ones
that pair a bind address with an environment, a token and a TLS declaration - live in
`crate::Settings::parse`, because they need the other groups to decide.

```rust
pub const fn request_timeout(&self) -> RequestTimeout
```

```rust
pub const fn tls(&self) -> Option<&TlsMaterial>
```

The certificate and key this process would terminate TLS with, if any were configured.

#### Implements

`Clone`, `Debug`, `Eq`, `PartialEq`

## Module `sources`

The sources this deployment declares: one entry per data system, keyed by the alias a model names.

**Beside `catalog.data_dir` rather than instead of it, and the two answer different questions.**
`catalog.dir` and `catalog.data_dir` are the *catalog*: authored definitions, and the directory the
`sutura` command reads. A `sources:` entry is a *data system*: what kind it is, where it is, which
identity a query reaches it as, and which identity re-ran its anchors at boot. A model's `source:`
is the key that selects one.

**The service reads this tree and not `catalog.data_dir`**, which is the one operator-facing break
worth stating at the top: a deployment that pointed the service at its files with
`catalog.data_dir` has to declare a source instead, and one that declares none does not serve -
the catalog names a source with no entry, and the composition root refuses before a listener is
bound.

# What is refused here, and what is refused later

This module is the **parse**: an alias that is not a name, two entries whose aliases are one name,
a kind this build has no adapter for, an entry with no file location, a relative path, a word that
is not a posture, and a pairing of declarations that contradict each other. Every one of those is a
value or a combination this type cannot hold, so it is a
`SettingsError` naming the key.

Two checks are deliberately **not** here, and they are not here for two different reasons.

- **The shared-identity acknowledgement** is a `NotFitToServe` out of
  `Settings::refusals`, because whether it is required depends on the *declared deployment mode* -
  a fact about the tree as a whole rather than about this entry. A single-user deployment holds
  every source under one static credential legitimately: that credential *is* the one user's.
- **Whether the linked adapter can carry a per-subject credential at all** is a startup refusal in
  the composition root, because it is a property of the BUILD. This crate cannot see which adapters
  were linked and must not pretend to.

And one is not a parse check on purpose: **whether the directory exists.** `CatalogSettings::parse`
declines the same check for the reason that applies here unchanged - a directory that disappears
between reading the configuration and opening the engine would make an existence check a claim that
is already stale, and it would make configuration validation depend on the filesystem. A missing
*file* is refused where it is discovered, at boot, by the composition root that tries to attach it.

### `enum SourceKind`

```rust
pub enum SourceKind
```

What kind of data system a source is.

**A closed set of typed declarations rather than something discovered**, which is the whole of
*pluggable by declaration*: a capability nobody declared cannot be used, and a new kind is a
compile error in every place that has to decide about it. Two variants today, and only one of them
can be OPENED by a shipped binary - `Self::BigQuery` says which and why.

**It replaced a comparison against a hard-coded source NAME**, and that is the change worth reading
rather than the enum. The composition root used to refuse any source not called `local`, on the
argument that the engine has its own identity and does not borrow the catalog's. That argument was
right while the catalog was the only signal - a catalog naming `production_warehouse` said nothing
about what the deployment held - and it stops being right once the DEPLOYMENT declares each source:
an operator who writes `sources.production_warehouse.kind: files` with a directory beside it has
stated that this source is a directory of files, which is the statement the name comparison was
standing in for. Under the old rule that deployment could not be served at all, and it is a
legitimate one.

#### Variants

- `Files` - A directory of CSV or Parquet files, read by the in-process engine.
- `BigQuery` - A `BigQuery` dataset, queried by rendering the plan into `GoogleSQL` and pushing it down.
- `Postgres` - A `PostgreSQL` database, queried by rendering the plan into that dialect and pushing it down.

#### Methods

```rust
pub const fn as_str(self) -> &'static str
```

The spelling, for the startup log.

```rust
pub fn parse(raw: impl AsRef<str>) -> Result<Self, UnknownSourceKind>
```

Reads the configured word.

#### Implements

`Clone`, `Copy`, `Debug`, `Eq`, `PartialEq`

### `struct UnknownSourceKind`

```rust
pub struct UnknownSourceKind
```

The configured word did not name a kind of data system.

#### Implements

`Clone`, `Debug`, `Display`, `Eq`, `Error`, `PartialEq`

### `struct ConfiguredSource`

```rust
pub struct ConfiguredSource
```

One declared data system.

The identity is an `Option`, and its `None` is **fail-closed rather than permissive**: it means
this entry declared the shared posture and nobody acknowledged it, which
`Settings::refusals` refuses. A `Settings` obtained through `Settings::load` therefore has `Some`
for every source. It stays an `Option` rather than being unwrapped here because a composition root
that treated the absence as permission is a bug the type should not be able to hide, and because
`Settings::parse` is reachable from this crate's own tests without the refusal having run.

#### Methods

```rust
pub const fn identity(&self) -> Option<&SourceIdentity>
```

How this source establishes identity, once the deployment-level refusal has passed.

`None` only for a shared source nobody acknowledged - see the type's own note.

```rust
pub const fn kind(&self) -> SourceKind
```

What kind of data system this is, which is what decides which adapter opens it.

Read off the placement rather than stored beside it - see `SourcePlacement::kind`.

```rust
pub const fn placement(&self) -> &SourcePlacement
```

Where this source's data is, in the terms its own kind uses.

**This replaced a `data_dir()` that every kind had to have.** A `BigQuery` source has no
directory, so a path accessor on the shared shape would have had to return something - an
empty path, or an `Option` whose `None` every caller re-interprets. Matching on the placement
makes the composition root say which kind it is opening, which is the same thing the kind's
exhaustive match there already asks of it.

```rust
pub fn posture(&self) -> Option<&SourcePosture>
```

The posture this source was declared with, if it is one a deployment may be served with.

```rust
pub const fn workload_identity(&self) -> Option<&WorkloadIdentityConfig>
```

The token-exchange setup this `impersonation-at-source` source declared.

`Some` exactly when the source is impersonating: the parse refuses an impersonating entry with
none, and refuses a non-impersonating entry with one, so an accessor's shape and a deployment's
posture cannot disagree about which sources exchange a subject's token.

#### Implements

`Clone`, `Debug`, `Eq`, `PartialEq`

### `struct UnknownPosture`

```rust
pub struct UnknownPosture
```

The configured word did not name a posture.

#### Implements

`Clone`, `Debug`, `Display`, `Eq`, `Error`, `PartialEq`

### `enum InvalidSourceRegistry`

```rust
pub enum InvalidSourceRegistry
```

Why a `sources:` tree is not usable.

Every variant names the alias, because a refusal that does not say which entry to change is a
support request - and a deployment with several sources is exactly the deployment where "one of
your sources is wrong" is useless.

No `Clone`, and the reason is worth a line rather than a shrug: one variant's cause is
`sutura_domain::model::InvalidIdentifier`, which is not `Clone` either. Deriving it here would mean
either a second copy of that error's shape or a `Clone` added to a domain type for a config crate's
convenience, and nothing needs to clone a startup refusal.

#### Variants

- `Alias` - The key is not a name a model could write in its `source:` field.
- `DuplicateAlias` - Two entries name one source.
- `NoDataDirectory` - The entry names no file location.
- `RelativeDataDirectory` - The path is relative, so it resolves against the process working directory.
- `RelativePath` - A path a kind requires is relative, so it resolves against the process working directory.
- `Posture` - The `posture:` word is not one of the two.
- `Kind` - The `kind:` word does not name a data system this build has an adapter for.
- `Text` - A piece of operator-written text on this entry is not usable.
- `Conflict` - The entry's two identity declarations contradict each other.
- `MissingForKind` - A key this kind requires was not written.
- `KeyNotForKind` - A key was written that means nothing for this kind.
- `ResourceName` - A declared cloud resource name is not usable.
- `Host` - A declared `host` cannot be dialled at all - a shape refusal, not a reachability one.
- `MissingWorkloadIdentity` - An `impersonation-at-source` source declared no token-exchange setup.
- `WorkloadIdentityNotImpersonating` - A workload-identity block was declared on a source that is not impersonating.
- `WorkloadIdentity` - The declared workload-identity value is not usable.
- `Transport` - The declared transport of a source was not usable.
- `RemoteWithoutTls` - A source a network can reach was declared with no transport security.

#### Implements

`Debug`, `Display`, `Eq`, `Error`, `PartialEq`

### `struct SourceRegistry`

```rust
pub struct SourceRegistry
```

Every source this deployment declares, keyed by the alias a model's `source:` names.

A newtype over the map rather than the map, so `Self::parse` is the only way one comes into
existence and the duplicate-alias refusal cannot be skipped by building the map directly.

**May be empty, and that is not a refusal here.** A deployment configuring no source is one that
has not said where its data is; what refuses it is the composition root, which finds the catalog
naming a source with no declaration and stops before a listener is bound. Refusing an empty tree in
this crate would mean `Settings::load` on the embedded defaults could not produce a `Settings` at
all, and the defaults are what the `prompt` command and every settings test read.

#### Methods

```rust
pub fn count(&self) -> usize
```

How many sources are declared.

```rust
pub fn each(&self) -> impl Iterator<Item>
```

Every declared source, in alias order.

```rust
pub fn get(&self, alias: &SourceName) -> Option<&ConfiguredSource>
```

The source declared under `alias`, if there is one.

```rust
pub fn is_empty(&self) -> bool
```

Did this deployment declare any source at all?

#### Implements

`Clone`, `Debug`, `Default`, `Eq`, `PartialEq`

### Module `placement`

Where a source's data is, per kind, plus the two `BigQuery` resource newtypes.
Where a declared source's data actually is, in the terms its own kind uses.

**One enum rather than a struct of optional fields, and the difference is unrepresentable versus
checked.** A files source has a directory and no billing project; a `BigQuery` source has a
billing project and a dataset and no directory. Carried as `Option` fields on one struct, the
wrong combination would be representable - a `BigQuery` source with a data directory and no
project - and every reader would have to decide for itself what an absence meant. As two variants
there is nothing to decide: a reader matches, and the compiler asks about a third kind.

This module is also where the two `BigQuery` newtypes live, and their `parse` functions carry an
argument that is **ours rather than the provider's format rules restated** - see
`BillingProject`.

#### `struct BillingProject`

```rust
pub struct BillingProject
```

The project a `BigQuery` query job is billed to.

**Declared, never inferred.** For this data system that is structural rather than a policy we
chose: the project is a PATH SEGMENT of the request URL that submits a job, so there is no field
it could be omitted from and nothing it could be defaulted from. The reason it is declared HERE,
one step before anything impersonates, is that a federated identity has no project of its own to
bill - so the per-subject step needs this declaration to already exist rather than introducing it
alongside a credential exchange.

# What `parse` enforces, and why the argument is ours

The value is interpolated into a URL path segment. So what has to be impossible is a value that
LEAVES that segment: a `/`, a `?`, a `#`, a `%`-escape, whitespace, a control character, anything
non-ASCII. The accepted set is therefore `[a-z0-9-]`, starting with a letter, not ending with a
hyphen, and 6 to 30 characters.

That happens to be the documented shape of a project id, and it is deliberately not justified
that way: **the argument for the character set is the path segment**, which holds whether or not
the provider widens its own rules later. If the provider ever narrows them further, a value we
accept and they reject is a startup failure against a real endpoint - the safe direction. If they
widen them, this refuses a legal id and the fix is a considered change here rather than a value
that silently escapes a URL.

**One shape this knowingly refuses, stated because it is a real deployment and not a hypothetical:**
a LEGACY domain-scoped project identifier carries a colon - the provider's own SQL reference uses
`google.com:my_project` as its example and tells an author to wrap it in backticks. A colon in a
URL path segment is legal, so this is a narrowing we are choosing rather than one escaping forces,
and it is chosen because such an id also has to survive being a path segment, a JSON field and a
backticked SQL identifier, and nothing here has ever been exercised against one. A deployment that
needs it gets a considered change with a test, not a widened character set.

No `Default`: a default project is a project somebody else pays for.

##### Methods

```rust
pub fn as_str(&self) -> &str
```

The id, for building a request.

```rust
pub fn parse(raw: impl AsRef<str>) -> Result<Self, InvalidResourceName>
```

Parses a declared project id.

The canonical constructor: every other way in delegates here, so there is one copy of the
checks. Trims first, because a trailing space in a configuration file is a typo rather than a
different project - and trimming BEFORE measuring is what stops the length refusal reporting a
number that counts whitespace the value does not have.

##### Implements

`Clone`, `Debug`, `Eq`, `Hash`, `Ord`, `PartialEq`, `PartialOrd`

#### `struct DatasetId`

```rust
pub struct DatasetId
```

The dataset unqualified table names in a generated statement resolve within.

**Why this is configuration and not catalog:** a model in the catalog names a bare `table:`, and
which dataset that table lives in is a property of the deployment's connection rather than of the
metric's definition. The same catalog served against a staging dataset and a production one is one
catalog and two deployments, which is exactly the split this type keeps.

The generated statement therefore stays a bare, quoted table name in every dialect - the request
carries the dataset beside the SQL rather than the generator qualifying it - so nothing about
`sutura-sql` has to know this exists.

`parse` accepts `[A-Za-z0-9_]`, 1 to 1024 characters. Unlike `BillingProject` this one does not
reach a URL path, so the constraint is not an escaping argument: it is that a dataset id which is
not an identifier is a misconfiguration worth refusing when the file is read rather than on the
first question. Case is PRESERVED, because a dataset id is case-sensitive and folding it here
would turn a working declaration into a dataset that does not exist.

##### Methods

```rust
pub fn as_str(&self) -> &str
```

The id, for building a request.

```rust
pub fn parse(raw: impl AsRef<str>) -> Result<Self, InvalidResourceName>
```

Parses a declared dataset id.

##### Implements

`Clone`, `Debug`, `Eq`, `Hash`, `Ord`, `PartialEq`, `PartialOrd`

#### `enum InvalidResourceName`

```rust
pub enum InvalidResourceName
```

Why a declared name for a cloud resource was not usable.

One type for both newtypes above, with the offending key named by the caller rather than by the
variant: the shapes differ and the *reasons* do not, so two near-identical enums would be two
places to keep one set of sentences.

##### Variants

- `Empty` - Nothing was written, or only whitespace was.
- `Length` - Outside the length the name may be.
- `Character` - A character that is not in the accepted set.
- `Boundary` - The first or last character is one the shape does not allow there.

##### Implements

`Clone`, `Debug`, `Display`, `Eq`, `Error`, `PartialEq`

#### `struct HostName`

```rust
pub struct HostName
```

A `postgres` source's host or address, exactly as written.

**Unrepresentable rather than checked, per `secure-by-design`.** Before this type, `host` was a
bare `String` carried past `parse_placement` unexamined - the exclusivity of `host` and
`unix_socket` was a check in that function, but the FIELD still admitted whatever text was
there, so `crate::sources::placement::PostgresDial` existed only as a `match` two composition
roots each wrote by hand. Refuses only shapes that cannot be a host at all - empty, embedded
whitespace, a URL scheme, a path separator - and nothing about reachability: a value that parses
may still fail to resolve, or fail the TLS name check at connect time, and neither is this
type's question. `crate::sources::transport::host_is_loopback` still does the loopback test on
the parsed text.

##### Methods

```rust
pub fn as_str(&self) -> &str
```

The host, for dialling and for the loopback check.

```rust
pub fn parse(raw: impl AsRef<str>) -> Result<Self, InvalidHostName>
```

Parses a declared host or address.

##### Implements

`Clone`, `Debug`, `Display`, `Eq`, `PartialEq`

#### `enum InvalidHostName`

```rust
pub enum InvalidHostName
```

Why a declared Postgres host cannot be dialled at all.

##### Variants

- `Empty` - Nothing was written, or only whitespace was.
- `Whitespace` - A host cannot contain whitespace - it would not survive being one token in a connection string, and a name split by a space is not a name any resolver would look up.
- `Scheme` - A URL was written where a bare host belongs - `host` is not a connection string.
- `PathSeparator` - A `/` is a path separator, not a character a host or an address ever carries.

##### Implements

`Clone`, `Debug`, `Display`, `Eq`, `Error`, `PartialEq`

#### `enum PostgresDial`

```rust
pub enum PostgresDial
```

How a `postgres` source is dialled: over TCP to a named host, or through a unix socket
directory. Exactly one, decided once in `crate::sources::parse_placement`.

**Replaces `host: Option<String>` plus `unix_socket: Option<PathBuf>` on the placement.** Those
two fields admitted `(None, None)` and `(Some, Some)`, states `parse_placement` already refused -
so both composition roots carried a `match (host, unix_socket)` with a fourth arm the parser had
already made unreachable, and a comment saying so at each. This enum is the same argument
`SourcePlacement` itself makes about `Files` versus `BigQuery`: unrepresentable beats checked
twice.

##### Variants

- `Tcp` - A TCP dial to a named host or address.
- `UnixSocket` - A unix domain socket dial.

##### Implements

`Clone`, `Debug`, `Eq`, `PartialEq`

#### `enum SourcePlacement`

```rust
pub enum SourcePlacement
```

Where one declared source's data is.

The module header carries why this is an enum. What is worth repeating at the type is that
`SourceKind` is DERIVED from it - see `Self::kind` - rather than stored beside it, so the two
cannot disagree about what a source is.

# The limit, because an enum variant's fields are always public

There is no way to make these private, so **a placement is constructible in-process by any crate
that can name the type** - including one carrying a relative `data_dir`, which `parse_data_dir`
refuses when it reads a file. That is a real gap in this type and it is not the one that matters,
for the reason AGENTS.md already states about the other constructors here: what is closed is the
path from a **configuration file**. `ConfiguredSource` holds its
placement in a private field and has no public constructor, so a
`SourceRegistry` can still only come into existence through
`Settings::parse`, and that is the only door a deployment goes through.

Written down rather than left to be re-derived, because "the fields are public" and "the checks can
be skipped" look like the same sentence and are not.

##### Variants

- `Files` - A directory of CSV or Parquet files, read by the in-process engine.
- `BigQuery` - A `BigQuery` dataset, plus the project its jobs are billed to.
- `Postgres` - A `PostgreSQL` database, reached over a connection the deployment declares.

##### Methods

```rust
pub const fn kind(&self) -> SourceKind
```

Which kind of data system this placement describes.

**Derived rather than stored**, so `kind:` in a file and the fields beside it cannot describe
two different data systems. The parse reads the word to decide which variant to build; from
then on the variant is the answer.

##### Implements

`Clone`, `Debug`, `Eq`, `PartialEq`

### Module `transport`

How the channel to a source is secured, per source and never globally.
How sutura secures the channel to one source, per source and never globally.

**This module is `docs/adr/0010`'s configuration half, and it exists because a source
connection is the first thing in this repository that must VERIFY a peer's chain.** The serving
side presents a chain and verifies none, so `sutura-http` deliberately pulls in neither
`webpki-roots` nor `rustls-native-certs`; a source verifies, so which anchors are trusted
becomes a decision with a name. Everything here is about making that decision a TYPE rather than
a habit, because this is the one decision whose wrong answer is a password and a whole result set
sent to an impostor.

# The shape is closed, and why

```text
Plaintext                                      - no TLS. A named choice an operator wrote.
Verified { anchors }                           - TLS, verified. No unverified variant exists.
Mutual    { anchors, identity }                - TLS, verified, and sutura presents a certificate.
```

There is deliberately no `Verified`-without-anchors shape: a source asking for TLS and naming no
trust store is a refusal at load, naming the source (ADR 0010 rule 2). `TrustAnchors` has no
default, so there is no value the loader could have filled in on the operator's behalf.

And there is deliberately no way to ask for TLS without verification. Every library in this
space offers the escape hatch - `danger_accept_invalid_certs`, `sslmode=require` - and each one
is an encrypted channel with an unknown peer. A `bool` named `verify` would make that reachable
from a configuration file, and a `bool` defaulted to `true` would make it reachable from a typo.

# What "no transport" means

`Plaintext` is a named choice, not the absence of a setting. A source that names no TLS
`transport_mode` is refused at load; an operator who wants no TLS writes
`transport_mode: plaintext`, and the startup log prints it. That ordering is what lets
`sutura-serve` keep #124's fail-closed refusal for a **non-loopback host with no TLS** while the
unix-socket tier keeps working: a socket or loopback host may declare `plaintext`, and any host a
network can reach it from must not.

**The keys an operator writes are FLAT, not a nested `transport:` block.** The schema holds
`transport_mode`, `transport_anchors`, `client_certificate` and `client_key` as top-level source
keys (see `crate::raw`), and every refusal below names one of those flat keys - never a
`transport.*` spelling that the tree would refuse as an unknown field. The word `transport` here
is the CONCEPT (what the channel is made of), and the flat spelling is what an operator edits.

#### `enum TrustAnchors`

```rust
pub enum TrustAnchors
```

The trust anchors a source chain may be verified against.

**No `Default`, and the field is named here rather than filled in.** Rule 2 of `docs/adr/0010`
is that the trust store is stated, not inherited - defaulting to whatever the host happens to
trust is how a source is silently accepted from the wrong issuer. So there is no value this type
could hold on the operator's behalf, and a `Default` impl would be a value that never passed a
constructor.

The system store remains reachable, but only by an operator WRITING it - a source that wants the
host's own anchors says so - and the startup line then prints that it was chosen.

##### Variants

- `File` - A PEM bundle at this absolute path. The file is read by the adapter, at boot, once.
- `System` - The host's own trust store. An explicit choice rather than a default.

##### Implements

`Clone`, `Debug`, `Eq`, `PartialEq`

#### `struct ClientIdentity`

```rust
pub struct ClientIdentity
```

The client certificate and key this deployment presents to a source.

A pair - a certificate with no key, or a key with no certificate, is refused at load naming the
missing half. The paths are read by the adapter at boot; only the paths live in configuration.

##### Methods

```rust
pub const fn certificate(&self) -> &PathBuf
```

```rust
pub const fn key(&self) -> &PathBuf
```

##### Implements

`Clone`, `Debug`, `Eq`, `PartialEq`

#### `enum SourceTransport`

```rust
pub enum SourceTransport
```

How a source connection is secured.

Three states, and the middle one is the one that gets forgotten - which is why it has a test of
its own. There is no unverified TLS variant, and no `Default`: what a source's channel is made
of is a decision the deployment makes.

##### Variants

- `Plaintext` - No transport security. For a local file or a process-local unix socket. A named choice.
- `Verified` - TLS, verified against the declared `anchors`.
- `Mutual` - TLS, verified, and sutura presents a `ClientIdentity`.

##### Methods

```rust
pub const fn anchors(&self) -> Option<&TrustAnchors>
```

The declared anchors, if this channel verifies anything.

`None` for `Plaintext` - nothing to verify against - and `Some` for both TLS variants,
because a TLS channel always names its store. This is what lets a caller say "no transport
security" without matching the variant, and it is what the load-time refusal of a remote
`plaintext` source reads: a source with no anchors and a host a network can reach is refused
rather than connected to in the clear.

```rust
pub const fn describe(&self) -> &'static str
```

A phrase for the startup log, per source.

##### Implements

`Clone`, `Debug`, `Eq`, `PartialEq`

#### `enum InvalidTransport`

```rust
pub enum InvalidTransport
```

Why a declared transport was not usable.

Every variant names the source (`alias`) whose entry is refused, because a refusal that does not
say which entry to change is a support request. No `Clone`: the cause is a `PathBuf` and the
value that would be cloned is a path, which is fine to own here.

##### Variants

- `UnknownTransport`
- `PlaintextWithMaterial` - A `plaintext` channel also named anchors or a client identity, which nothing would read.
- `TlsWithoutAnchors` - A source declared TLS and named no trust anchors.
- `MissingHalf` - A client certificate was written without its key, or the reverse.
- `MutualWithoutIdentity` - A `mutual` channel declared no client identity at all.
- `RelativePath` - A path a transport declares is relative.

##### Implements

`Debug`, `Display`, `Eq`, `Error`, `PartialEq`

#### `fn parse`

```rust
pub fn parse(alias: &sutura_domain::model::SourceName, mode: &str, anchors: Option<&str>, client_certificate: Option<&str>, client_key: Option<&str>) -> Result<SourceTransport, InvalidTransport>
```

Reads one source's transport from its written fields, refusing the combinations ADR 0010 says
a closed type must refuse.

`mode` is the `transport_mode` word. `anchors` is the written `transport_anchors` value (a path or
the word `system`). `client_certificate`/`client_key` are the optional identity pair. `plaintext`
is the one way to declare no TLS; a `plaintext` declaration that also names material is refused.

#### `fn host_is_loopback`

```rust
pub fn host_is_loopback(host: &str) -> bool
```

Whether a declared source host can only be reached from this machine.

**A name is not an address**, which is the rule `crate::server::BindAddress` already applies to
the serving bind read the other way round: `localhost` resolves to whatever the resolver says
today, so it cannot carry a claim about what a network can reach. Only an `IpAddr` literal
answers `true`, and only a loopback one - so a `plaintext` declaration is refused for a hostname
however it happens to resolve. That is the fail-closed direction issue 124 asks for: an operator
who means a loopback TCP dial writes `127.0.0.1` or `::1`.

### Module `workload_identity`

The token-exchange setup one `impersonation-at-source` source declares.
The token-exchange setup one `impersonation-at-source` source declares.

**This is the tape a subject's own credential is exchanged against** - RFC 8693 handed to a
Workload Identity Federation provider. A source that executes as the asking subject has to say
*which* provider receives the subject's token and *what the exchanged credential may do*, and
both are that source's declaration rather than this process's guess. See `docs/adr/0008` and the
issue that wired the adapter that presents one.

The two newtypes are declared here, in the settings tree that owns the value, and the broker that
performs the exchange holds its own copies in the adapter that links it - the same reason
`BillingProject` is checked both here and in the transport that interpolates
it: an adapter may not depend on the settings tree, so the format is checked where it is declared
AND where it is sent.

#### `struct WifAudience`

```rust
pub struct WifAudience
```

The audience a subject token is exchanged for: a workload identity provider resource.

##### Methods

```rust
pub fn as_str(&self) -> &str
```

The audience, for building a request.

```rust
pub fn parse(raw: &str) -> Result<Self, InvalidWorkloadIdentity>
```

Parses an audience.

The accepted set is the printable ASCII a workload identity provider resource is built from -
letters, digits and `/ : . - _` - so a value that would escape the STS request body cannot
exist here. Bounded in length, because it is a foreign string heading for a request and a log.

##### Implements

`Clone`, `Debug`, `Eq`, `Hash`, `Ord`, `PartialEq`, `PartialOrd`

#### `struct WifScope`

```rust
pub struct WifScope
```

The OAuth scope the exchanged credential is minted for.

##### Methods

```rust
pub fn as_str(&self) -> &str
```

The scope, for building a request.

```rust
pub fn parse(raw: &str) -> Result<Self, InvalidWorkloadIdentity>
```

Parses a scope.

A scope is a URL (`https://www.googleapis.com/auth/bigquery.readonly`), so it allows the `%`
and letters a URL does rather than the narrower set an audience does. Same bound, same reason:
it belongs in a request and a refusal should never log it raw.

##### Implements

`Clone`, `Debug`, `Eq`, `Hash`, `Ord`, `PartialEq`, `PartialOrd`

#### `struct WorkloadIdentityConfig`

```rust
pub struct WorkloadIdentityConfig
```

The token-exchange setup a `impersonation-at-source` source needs.

##### Methods

```rust
pub const fn audience(&self) -> &WifAudience
```

The provider audience.

```rust
pub fn parse(audience: impl AsRef<str>, scope: impl AsRef<str>) -> Result<Self, InvalidWorkloadIdentity>
```

Parses a declared audience and scope together, since neither is usable alone.

```rust
pub const fn scope(&self) -> &WifScope
```

The scope the exchanged credential carries.

##### Implements

`Clone`, `Debug`, `Eq`, `PartialEq`

#### `enum InvalidWorkloadIdentity`

```rust
pub enum InvalidWorkloadIdentity
```

Why a declared workload-identity value is not usable.

**The position is carried and the value is not**, for the reason every refusal about
operator-written text carries it: an audience and a scope are foreign strings heading for a
request, and neither belongs in a log.

##### Variants

- `Empty` - Nothing was written, or only whitespace was.
- `TooLong` - Longer than the endpoint's ceiling.
- `Character` - A character outside the accepted set.

##### Implements

`Clone`, `Debug`, `Display`, `Eq`, `Error`, `PartialEq`

## Module `telemetry`

What the log looks like, and who is meant to read it.

One decision with two right answers, which is why it is a type rather than a flag. In
production a log line is read by a collector: it has to be one JSON object per line, with the
span context attached, so a query can find every line belonging to one request. On a laptop
the same line is read by a person compiling every thirty seconds, and a JSON object per line
is unreadable there.

The default therefore follows `crate::Environment` and nothing else, so the two cannot be
set inconsistently by omission. An explicit value overrides it - a developer debugging what
the collector will actually receive needs that - and the startup log says which of the two
happened.

### `enum LogFormat`

```rust
pub enum LogFormat
```

How a log line is rendered.

#### Variants

- `Bunyan` - One JSON object per line, in the bunyan schema. What a collector ingests.
- `Pretty` - Indented, coloured, and meant for a terminal.

#### Methods

```rust
pub const fn as_str(self) -> &'static str
```

```rust
pub const fn default_for(environment: Environment) -> Self
```

The format an environment gets when the configuration does not say.

**This is the split, in one function.** Production is machine-readable and everything else
is human-readable, and it is expressed as a total match rather than an `if` so a fourth
environment cannot inherit an answer nobody chose for it.

```rust
pub fn parse(raw: impl AsRef<str>) -> Result<Self, UnknownLogFormat>
```

Reads a format name.

#### Implements

`Clone`, `Copy`, `Debug`, `Deserialize<'de>`, `Display`, `Eq`, `PartialEq`, `Serialize`

### `struct UnknownLogFormat`

```rust
pub struct UnknownLogFormat
```

The string was neither format.

#### Implements

`Clone`, `Debug`, `Display`, `Eq`, `Error`, `PartialEq`

### `struct LogFilter`

```rust
pub struct LogFilter
```

A tracing filter directive, as written for `RUST_LOG`.

Kept as text here and turned into a real filter by whatever installs the subscriber, because
the type that parses one lives in `tracing-subscriber` and this crate holds no framework. What
*is* checked here is that it is not empty and not a smuggled second line: an empty filter
silently means "no directives", which is a service that logs at the default level while its
configuration says otherwise.

#### Methods

```rust
pub fn as_str(&self) -> &str
```

```rust
pub fn parse(raw: impl AsRef<str>) -> Result<Self, InvalidLogFilter>
```

Reads a filter directive.

#### Implements

`Clone`, `Debug`, `Display`, `Eq`, `PartialEq`

### `enum InvalidLogFilter`

```rust
pub enum InvalidLogFilter
```

Why a string is not a filter directive.

#### Variants

- `Empty` - Empty, or only whitespace.
- `ControlCharacter` - A control character, which in practice is a newline pasted in with the value. A newline in a filter is a log line an attacker can forge if the value ever reaches the log itself.
- `TooLong` - Longer than `LogFilter::MAX_LENGTH`. A directive that long is not a filter anyone wrote by hand, and an unbounded configured string is an availability surface.

#### Implements

`Clone`, `Debug`, `Display`, `Eq`, `Error`, `PartialEq`

### `struct ServiceName`

```rust
pub struct ServiceName
```

The name every log line is attributed to.

A separate type because it is the field a collector groups by, so an empty or whitespace one
makes every line from this deployment unattributable.

#### Methods

```rust
pub fn as_str(&self) -> &str
```

```rust
pub fn parse(raw: impl AsRef<str>) -> Result<Self, InvalidServiceName>
```

Reads a service name.

Narrow on purpose: a name with a space or a quote in it has to be escaped by every
consumer, and the one that forgets produces a log line that does not parse.

#### Implements

`Clone`, `Debug`, `Display`, `Eq`, `PartialEq`

### `enum InvalidServiceName`

```rust
pub enum InvalidServiceName
```

Why a string is not a service name.

#### Variants

- `Empty`
- `NotAnIdentifier`
- `TooLong` - Longer than `ServiceName::MAX_LENGTH`. The name is the field a collector groups by and it appears on every log line, so an unbounded one is both an availability surface and a log line an operator has to read on every record.

#### Implements

`Clone`, `Debug`, `Display`, `Eq`, `Error`, `PartialEq`

### `struct TelemetrySettings`

```rust
pub struct TelemetrySettings
```

Everything about the log.

#### Methods

```rust
pub const fn filter(&self) -> &LogFilter
```

```rust
pub const fn format(&self) -> LogFormat
```

```rust
pub const fn format_was_explicit(&self) -> bool
```

```rust
pub const fn new(service_name: ServiceName, filter: LogFilter, format: LogFormat, format_was_explicit: bool) -> Self
```

```rust
pub const fn service_name(&self) -> &ServiceName
```

#### Implements

`Clone`, `Debug`, `Eq`, `PartialEq`
