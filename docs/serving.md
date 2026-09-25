# Serving over HTTP

`sutura serve` answers certified questions over HTTP. It is a **subcommand of `sutura`**, and
**a release publishes it**: a tarball and a container image at each of the four shipped triples,
signed and with provenance like everything else on the page.

That is a change from when the tool first shipped. Before that, the release artifacts contained the
command-line tool and nothing else, so the HTTP surface, the caller-token verification, the rate
limiter and the generated interface description shipped in no artefact on any platform, and this
page's only answer to "how do I run it" was `cargo run`. It was then a second BINARY,
`sutura-serve`, rather than a subcommand - `sutura` was a tool a person ran and this was a service a
platform scheduled, and folding an async runtime, an I/O driver and a web framework into the former
would have put them in every `sutura compile`. `github.com/telekom/sutura#685` step 2 folded it in
anyway: the cost lands once, in what a `cargo build` produces, and `doctor`, `compile` and `query`
still run one process each and start no listener.

**The published binary carries in-process TLS, the BigQuery adapter, the Postgres adapter and the
DataHub adapter**, since `github.com/telekom/sutura#685` step 5 - each is off by default only in a
*source* build without `--features`, and each of those is still a **startup refusal that names the
feature** rather than a silent degradation - so asking for one on a build without it stops the
process rather than serving something weaker than asked for.
[Terminating it in this process](#terminating-it-in-this-process) and the `bigquery` source notes
below say which command.

## Read this part first

**A caller's identity can now be established, and it is still not per-caller access.** Those are two
different sentences and a deployment that reads them as one is the failure this section exists to
prevent.

By default there is **no per-caller identity at all**: no `security.inbound` block means the bearer
token is the whole story, and presenting it proves the caller holds a secret an operator wrote down -
it authenticates the **deployment**, not the caller. It cannot be scoped to a subset of the catalog,
it cannot be revoked for one party without revoking it for all of them, and it does not reach the data
system. That is a single-player deployment, and it is a first-class shape rather than a degraded one.

A deployment that declares `security.inbound` gets leg 1: every request carries a token this service
verifies itself - signature against a pinned asymmetric algorithm, issuer, expiry, and an audience
matching this deployment's own resource identifier - and the request runs under a verified subject
that every audit record then names. See [who is asking](#who-is-asking) for the two modes and the keys.

**Neither shape, on its own, makes a data system execute as the asking subject.** That is leg 2, and
the part of it that is built is worth stating precisely, because the gap left is the one that
matters. Built: a question cannot execute at all without a credential a broker minted for the source
it reads - there is no signature that runs as this process - and a subject with no credential at a
source is refused as `credential_unavailable` rather than answered under the deployment's identity.
Built, behind a feature: **an adapter that can carry a per-subject credential** -
`sutura-exec-bigquery`, whose exchange resolves per source, `docs/where-identity-is-proven.md` - but
no served binary has executed a leg as the calling subject yet. The engine that ships by default is
one process reading local files under one operating-system identity, so what a broker can mint for
IT is the deployment's own identity, acknowledged by an operator; the shipped broker mints from
configuration and performs no token exchange for that source, or for Postgres.

So a deployment with leg 1 knows who asked, records the posture each leg ran under, and still reads
every row as one identity. Believing otherwise - that authentication implies per-user access - is
precisely the confusion the records warn about.

Both sentences are printed at `WARN` on every boot, read out of the configuration types rather than
written into the log by hand, so an operator meets them without reading this page.

Rate limiting is not authentication either. It bounds how fast something can be done, not who may do
it, and the bucket it counts against is a network address rather than a principal.

## Running it

The engine reads files, so there is nothing to provision. Three ways in, and they take the same
environment - which is **not** three catalog keys: a `sources.<alias>` entry says what kind of data
system the catalog's models read, where its files are and which identity a query reaches it as, and
a catalog naming a source nobody declared is a startup refusal that names the source.

**From a published release**, with no Rust toolchain. Take the tarball for your triple - the musl
ones are statically linked and need no libc at all - and verify it before you run it;
[verifying a release](verifying-a-release.md) is that page. The `cd` below is into the corpus, and
**no release asset carries it**: [the corpus](getting-started.md#the-corpus) is the commands that
put it beside you, and the reason it is not an asset. This fence assumes you ran those, so the
corpus is at `sutura-corpus/` in the directory you are standing in; a clone puts it at
`examples/single-player` instead, and the `cd` is the only line that differs.

```bash
tar -xzf sutura-x86_64-unknown-linux-musl.tar.gz
BINARY="$PWD/sutura"
cd sutura-corpus/examples/single-player
SUTURA__SECURITY__IDENTITY=single-user \
SUTURA__SECURITY__SINGLE_USER_BECAUSE="one operator reading their own files" \
SUTURA__SOURCES__LOCAL__KIND=files \
SUTURA__SOURCES__LOCAL__DATA_DIR="$PWD/data" \
SUTURA__SOURCES__LOCAL__POSTURE=shared-service-user \
  "$BINARY" serve
```

**Or the image.** One tag, one binary: the default command is `--version`, so `docker run` with an
argument of `serve` is what starts the server rather than printing it and exiting.

```bash
docker run --rm --network host \
  --workdir /examples \
  -v "$PWD/examples/single-player:/examples:ro" \
  -e SUTURA__SECURITY__IDENTITY=single-user \
  -e SUTURA__SECURITY__SINGLE_USER_BECAUSE="one operator reading their own files" \
  -e SUTURA__SOURCES__LOCAL__KIND=files \
  -e SUTURA__SOURCES__LOCAL__DATA_DIR=/examples/data \
  -e SUTURA__SOURCES__LOCAL__POSTURE=shared-service-user \
  ghcr.io/telekom/sutura:latest serve
```

**`--network host` rather than `-p 8080:8080`, and the difference is a startup refusal rather than a
preference.** The default bind is loopback, and inside a container loopback is the container - so a
published port would reach nothing. Binding `0.0.0.0` instead makes this deployment one other hosts
can reach, and that needs `security.access_token` and a `security.tls_termination` that says which
cleartext hop the token crosses; without both, the process refuses to start and names both. Which is
the right shape for a real deployment and the wrong one for reading this page. The image runs as uid
65532 with no shell and no package manager in it.

**Every published x86_64 image is smoke-tested with this shape before a release is cut** -
`.github/serve-smoke.sh`, the same mount and the same keys plus a port of its own - and the test is
not a liveness probe. It starts the image, asks the `recurring_revenue` question from
[`examples/single-player`](https://github.com/telekom/sutura/tree/main/examples/single-player),
checks the certified January figure is in the answer, and checks that a question the catalog refuses
comes back `403`. The arm64 pair is built and not run, because executing it would need an emulator
registered on the runner.

**Or from source**, which is what a change to this repository is tested with:

```bash
ROOT="$PWD"
cd examples/single-player
SUTURA__SECURITY__IDENTITY=single-user \
SUTURA__SECURITY__SINGLE_USER_BECAUSE="one operator reading their own files" \
SUTURA__SOURCES__LOCAL__KIND=files \
SUTURA__SOURCES__LOCAL__DATA_DIR="$PWD/data" \
SUTURA__SOURCES__LOCAL__POSTURE=shared-service-user \
  cargo run --manifest-path "$ROOT/Cargo.toml" -p sutura-cli -- serve
```

It binds `127.0.0.1:8080`, needs no token there, and serves the browser interface at `/docs`. Startup
loads the catalog through its port and re-executes **every declared anchor** against the data system;
a bundle whose anchors do not reproduce the numbers their author certified starts nothing. That is not
a check the startup sequence performs and could forget - the type the service accepts has no other
constructor.

**Two suites are the thing to read next rather than this page**, and they are suites rather than
transcripts. `crates/sutura-cli/tests/served.rs` starts this binary against
`examples/single-player` on a kernel-chosen port and asserts the liveness probe answering only once
the catalog has loaded, a question with no bearer token refused by the gate, a certified question
answered, the catalog route, a refusal arriving as its documented status, a caller's own token
verified and every forgery refused alike, a published key set this deployment cannot use stopping the
process, and **four of the refusals in [the table below](#what-it-will-not-start-with) on the binary
that makes them** - a
non-loopback bind with no TLS termination declared, a production deployment with no credential, a
configured source with no `security.identity`, and a misspelled key, each asserted as exit `1`, no
listener opened, and the sentence `sutura_config` renders for that deployment. `just serve-e2e` runs
it. `crates/sutura-http/src/harness.rs` asserts the
envelope one status at a time, in process and with no socket: the token gate, `sql` in a body as a
`400` naming the field, the bounds, the rate-limit tiers, and the interface description served in
development and not in production. It reaches ten refusal reasons, a subset nothing here restates
as a fraction of the whole list - the exhaustive one is `crates/sutura-http/src/wire/refusal.rs`,
which lists every variant's status and `code` and assigns them in a match with no wildcard arm, so
a new refusal is a compile error until somebody decides what it is on the wire.

**What neither asserts, next to the claim:** the startup banner's own wording. `announce_identity`
in `crates/sutura-runtime/src/banner.rs` emits the `NO PER-CALLER IDENTITY` sentence from the
config types, and no test compares it to a string - so quoting it on a page is a promise no gate
keeps. It is step 5 and the four refusal cases exit at step 3, so they reach it in neither direction.
Nor does anything pin a response's JSON *formatting* or the `detail` sentences beside the codes.

**And the refusals in [the table below](#what-it-will-not-start-with) are not all held on the
binary.** Four are; the rest are asserted
over `Settings::load` alone, and nothing in the tree forces a new one onto either venue - there is no
exhaustive match over the refusal enum the way `crates/sutura-http/src/wire/refusal.rs` has one over
the wire's. A startup refusal also names **no configuration layer**, so a deployment refused because
`SUTURA_CONFIG_DIR` was wrong is refused in the same words as one refused for its own file
(`telekom/sutura#445`).

A hand-captured session in `examples/single-player/README.md` used to hold the read-next role, and
`docs/adr/0005` had already recorded it as stale - it showed `200 OK` for refusals, which stopped
being true when a refusal got a status of its own. It is deleted rather than re-captured: a
transcript nobody runs goes stale silently, and a suite cannot.

## What it will not start with

Every one of these is a **refusal to start**, not a warning. A warning is read by whoever happens to
be looking at the log in the format the collector was configured for; a process that does not start
is read by everybody. Every refusal is reported at once, so a fix-and-restart loop does not surface
them one at a time.

| Configuration                                                                                      | Why it refuses                                                                                                                                                                                                                                                                                                                       |
| -------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| a bind address other hosts can reach, without `security.tls_termination` declared                  | with no per-caller identity the bind address is the whole perimeter, and the bearer token crosses whatever hop is in front. Saying which thing terminates TLS is how the cleartext segment becomes a stated fact rather than an assumption. Applies in *every* environment, including a laptop. See [TLS](#tls) for the four answers |
| no `security.access_token` **and** no `security.inbound`, in production or on a non-loopback bind  | the alternative is an unauthenticated way to read whatever the process can read. Either credential satisfies it: a validated, audience-bound, expiring token per caller is strictly more than one shared secret every caller holds                                                                                                   |
| `GET /metrics` reachable with no `security.metrics_token`, in production or on a non-loopback bind | the endpoint is on the same listener as the API, so the same argument applies to what a scrape can read. Set `security.metrics_token`                                                                                                                                                                                                |
| a `security.metrics_token` equal to `security.access_token`                                        | one token behind both surfaces hands the monitoring system every ability a holder of the deployment token has, and nothing at runtime would show it. Use a different value for each                                                                                                                                                  |
| a `security.inbound` block with no `mode`                                                          | both defaults are wrong in opposite directions - `direct` makes a deployment behind a gateway reject every caller, and `behind-gateway` makes a directly exposed one accept a proof anybody can forge. See [who is asking](#who-is-asking)                                                                                           |
| `security.access_token` together with `security.inbound.mode: direct`                              | both are read from `authorization: Bearer`, and a request cannot carry two credentials in one header. In the direct mode the caller's own token is what authenticates the request                                                                                                                                                    |
| `security.inbound.algorithms` naming `none`, an `HS*` algorithm, nothing, or two key families      | `none` is the absence of a signature; a symmetric algorithm is how algorithm confusion works; an empty list is pinning nothing; and a list spanning two key kinds verifies nothing, because one token is verified by one key                                                                                                         |
| a `security.inbound.key_set_file` that cannot be read or is not a usable JWK set                   | the alternative is a process that starts and answers `401` to everybody. A key with no `kid`, a symmetric (`oct`) key, and **two keys under one `kid`** are each refused rather than skipped - the last one because which key verifies would otherwise be decided by their order in the document                                     |
| a key set holding **no key of the kind `security.inbound.algorithms` needs**                       | an RSA key set under `algorithms: ["ES256"]` cannot verify anything, so the deployment would start and answer `401` to everybody with nothing in the log connecting the two                                                                                                                                                          |
| `security.inbound.mode: behind-gateway` with no `security.inbound.transit_token_type`              | a component's `typ` is a fact only the deployment knows, and a guess either rejects every request or checks nothing                                                                                                                                                                                                                  |
| `security.inbound.transit_max_lifetime_seconds` outside 1..3600                                    | a zero refuses every assertion, and past an hour "short-lived" is not being used                                                                                                                                                                                                                                                     |
| an explicit `rate_limit.enabled: false` in production                                              | one question is an aggregate over up to ten years of history, so an unbounded caller is an unbounded load on the data system                                                                                                                                                                                                         |
| `server.port: 0` in production                                                                     | that asks the kernel for an ephemeral port, so nothing can be configured to reach the service                                                                                                                                                                                                                                        |
| an unknown `SUTURA_ENVIRONMENT`                                                                    | a typo would otherwise select the permissive branch of every decision above                                                                                                                                                                                                                                                          |
| any malformed or misspelled configuration key                                                      | a key that is silently ignored is a default the operator believes they overrode                                                                                                                                                                                                                                                      |
| a configured source and no `security.identity`                                                     | the mode decides where a shared source's acknowledgement has to be written, and no combination of source postures may answer it: a multi-tenant deployment whose sources are all shared is exactly the case a derived mode would exempt from the check it most needs                                                                 |
| a `shared-service-user` source in `multi-user` mode with no `acknowledged_because`                 | every caller would read that source as one identity that is not theirs. Sutura declares no data sensitivity, so it cannot tell whether that was fine - what it can do is make the posture impossible to arrive at by accident and impossible to arrive at in silence                                                                 |
| a source the catalog reads and no `sources.<alias>` entry declares                                 | there is no location for its files and no posture for its queries, and defaulting either would serve data under a configuration nobody wrote                                                                                                                                                                                         |
| `posture: impersonation-at-source` on a source this build's adapter cannot impersonate             | the alternative is a deployment that believes it impersonates and reads everything as this process. There is no fallback                                                                                                                                                                                                             |
| an anchor on a metric reading an `impersonation-at-source` source with no `verification_identity`  | there is no identity to re-run that certified number as. Not skipped, not warned about and not treated as a passing anchor - a deployment that wants an impersonating source with no boot identity gets it by authoring no anchors on its metrics                                                                                    |

The checks read the **loaded** values, not any one file. The environment-variable layer is applied
last, so a check against a file would be checking something the process is not running on.

The last two are the **composition root's** rather than the settings tree's, and the split is not
filing: whether the *linked adapter* can carry a per-subject credential at all is a property of the
build, and whether the *bundle* declares an anchor is a property of the catalog. Neither is visible to
a file, so neither is checked where files are parsed.

### One boot check that is a SIGNAL rather than a refusal, and the limit it leaves

Before it opens a transport, a deployment asks each data system whether it holds the tables the
catalog names. Three of the four answers are startup refusals - a table the data system says it does
not have, a listing an identity was not allowed to read (fix it by granting that identity
`bigquery.tables.list` on the dataset), and a listing that reported a total and then named fewer
tables than the total claims. The fourth is a `WARN` and the deployment **serves anyway**: a data
system that could not be ASKED - unreachable, undecodable, or a dataset that is not there.

**The limit, stated here because it is the one an operator has to plan around.** After that `WARN`
nothing has been verified: a mistyped `table:` fails on the first question against that source, not
at boot. That is deliberate rather than a gap being worked on. The catalog is **trusted for
metadata** - a deployment cannot ask a data system about fifty thousand tables at startup, and the
alternative to the `WARN` is either refusing to start whenever a data system is briefly unreachable
or a `skip_preflight` key set in exactly the deployment that most needs the check. Read the startup
log: the `WARN` names the source and the tables it could not account for.

**It is an accepted decision and not an unfinished edge**, and `docs/adr/0018`'s twelfth amendment
is the record: what is deferred, why the fifty-thousand-table number is the argument, what an
operator sees instead, that the failure surfaces on the first question against that source rather
than at boot or on a readiness probe, what is explicitly not promised, and which of those sentences
no mechanism holds.

**Two further limits on the three refusing answers.** What a pre-flight establishes is that a table
EXISTS - not that the model's columns are on it, and not that a question's identity may read it; an
anchor is what covers both, for the metrics that have one. And exactly one adapter asks at all -
`bigquery`. Every other source takes the port's default, which reports *not asked* and is never a
refusal: a `files` deployment is still covered, because attaching a model whose file is missing
fails the boot on its own, while on a `postgres` source a table the catalog names is neither
attached against nor asked about unless it is the target of a declared join key or carries an
anchored metric.

## Who is asking

Leg 1, and it is **opt-in**: a deployment with no `security.inbound` block has no per-caller identity
and is unaffected by everything in this section.

A deployment that wants one picks a mode, and there is deliberately no default, because both would be
wrong in opposite directions.

**`direct` - this deployment is the resource server.** It validates the caller's own token itself. The
token arrives in `authorization: Bearer`, which is where RFC 6750 puts an access token and where an
OAuth 2.1 client has no option to put it - so `security.access_token` cannot also be set, and the pair
is refused at startup.

```yaml
security:
  inbound:
    mode: "direct"
    resource: "https://sutura.example.com/v1/query"
    authorization_server: "https://issuer.example.com" # the `iss` value, exactly
    key_set_file: "/etc/sutura/keys/jwks.json"
    algorithms: ["RS256"]
    # token_type defaults to RFC 9068's `at+jwt`. Leave it out unless your issuer uses another
    # profile - and read the class check below before writing `any`.
```

**`behind-gateway` - a fronting component authenticated the caller.** This deployment validates a
short-lived **identity assertion that component signed**, and derives the subject from that
assertion's own claims. It arrives in a header of the component's own, so the deployment bearer token
keeps `authorization` and both controls survive.

```yaml
security:
  inbound:
    mode: "behind-gateway"
    transit_header: "x-transit-proof"
    transit_issuer: "https://gateway.example.com"
    transit_audience: "https://sutura.example.com"
    key_set_file: "/etc/sutura/keys/gateway-jwks.json"
    algorithms: ["ES256"]
    transit_token_type: "at+jwt" # required; `any` if the component sets no `typ`
    transit_max_lifetime_seconds: 120 # the longest `exp - iat` this deployment accepts
```

**It is called an assertion and not a proof of transit, and the wording is the honest one.** A
signature says the component *issued* the token. It does not say this particular request carried it
there: nothing binds an assertion to a method, a path or a body, and there is no record of which
assertions have been seen. What is bounded is the *window* - an `iat` is required and `exp - iat` is
capped by `transit_max_lifetime_seconds` - so an intercepted assertion replays for at most that long.
**The hop between the component and this process is therefore a trusted transport boundary**, and
`security.tls_termination` is where you say how far it reaches.

**`behind-gateway` does not mean "trust a header", and the configuration is what stops it meaning
that.** There is no key here that names the header a *username* arrives in. A component asserting an
identity in a header is not authentication: anything that can reach the port can write that header,
and the failure is invisible in a diff - a header named `x-authenticated-user` that means
"authenticated" because of where it is *expected* to come from. What this validates is a token, on
every request, and the subject is derived by this service from claims whose signature checked out.
**The limit:** in this mode the component's *authentication of the caller* is trusted, because that is
what the mode means. What is not trusted is a string.

What the checks are, in both modes:

| Check                           | What it is, and what it is not                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| ------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| The signature                   | Against a key from `key_set_file`, selected by the token's `kid`. A token naming no key id is refused rather than tried against every key - otherwise an unknown key and a bad signature are indistinguishable and a rotation is invisible                                                                                                                                                                                                                                                                                                   |
| The algorithm                   | **Pinned from configuration and never read from the token.** `none` and every `HS*` cannot be configured at all, and a symmetric key in the key set is refused at load - both halves have to be closed, because a token signed `HS256` with the issuer's *public* key as the secret verifies against a validator that accepts either                                                                                                                                                                                                         |
| **`typ`, the token's class**    | Checked **after** the signature, on a header the issuer signed. RFC 9068's `at+jwt` by default in `direct`. Without it, *any* JWT this issuer signed for this audience verifies - and where your resource identifier is also a client id, which is the ordinary arrangement, that includes an **OIDC ID token**: a document minted to describe a login, establishing a caller for an API call. `at+jwt`, `AT+JWT` and `application/at+jwt` are one value; a token with **no** `typ` is refused, so the check cannot be satisfied by omission |
| `exp` and `nbf`                 | Both, with thirty seconds of leeway for clock skew. Not configurable: an operator who needs more has a clock problem that a wider window hides                                                                                                                                                                                                                                                                                                                                                                                               |
| `iat`, in `behind-gateway` only | **Required**, and `exp - iat` is capped by `transit_max_lifetime_seconds`. Without an `iat` there is no lifetime to bound, and an assertion whose lifetime is the component's alone is not short-lived in any sense this deployment can enforce. An `iat` dated into the future past the leeway is refused too, or a component could buy a longer window by dating forward                                                                                                                                                                   |
| `iss`                           | Must equal the configured issuer, byte for byte. Not resolved as a URL - see the key table                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
| `aud`                           | Must contain **this deployment's own** resource identifier, byte for byte, and the claim is **required** - a token carrying no audience is refused rather than passing a check with nothing to compare. A client may also ask its authorization server for a narrowly scoped token; that is welcome and it is an optimisation, and it is never what makes the token safe                                                                                                                                                                     |
| `sub`                           | Required, and parsed: a control character or an invisible code point in it is a refusal, because the value is written into an audit record that is one line per call                                                                                                                                                                                                                                                                                                                                                                         |
| `act`                           | RFC 8693's actor claim, if present, becomes the ordered actor chain in the record - so a call by an agent for a person is a different event from a call by that person                                                                                                                                                                                                                                                                                                                                                                       |
| `scope`                         | Parsed, bounded, and **read** - it decides which of this surface's operations the caller may invoke. See *What a scope grants* below. A per-caller ceiling derived from a scope is still not built                                                                                                                                                                                                                                                                                                                                           |

A refused request in the `direct` mode gets `401` with a `WWW-Authenticate: Bearer
realm="<your resource identifier>", error="invalid_token"`. When an origin-form request's raw `Host`
and request target reproduce the exact configured resource identifier, the challenge also carries
`resource_metadata="<absolute metadata URL>"`. RFC 9728 requires clients to discard metadata naming
any other resource, so the parameter is absent for every other spelling. Absolute-form request
targets are not matched either: the HTTP URI parser canonicalises standard schemes, so a match there
would compare against a normalised spelling rather than the byte-exact one configured. The challenge
deliberately does **not** say which check
failed: "the signature verified and the audience did not" tells a caller which half of a forgery to
fix. The log says, in the cause chain, where an operator can read it.

The metadata URL is public and needs no token. It serves RFC 9728 JSON whose `resource` is the exact
configured resource identifier and whose one `authorization_servers` entry is the exact configured
issuer. For `https://sutura.example.com/v1/query`, the route is
`GET /.well-known/oauth-protected-resource/v1/query`; a resource with no path uses
`GET /.well-known/oauth-protected-resource` for direct discovery, but that root document cannot be
advertised from a child path. It is outside `/v1` and capability authorization, uses the probe rate
limit, and exists only in `direct` mode. Authorization-server metadata remains the authorization
server's document, not one served here.

**In `behind-gateway` there is no challenge**, and that is deliberate rather than missing: the caller
holds no bearer token for this resource, so an instruction to present one is one it cannot follow - and
a client that followed it would start putting credentials in a header this deployment refuses to read.

**Rotation and revocation are two questions, and they have two answers.**

| Question                   | What triggers a re-read                        | The bound                                                                                                                                                                                                                                                                                                                                                                                                                         |
| -------------------------- | ---------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| has a key been **added**   | a token naming a `kid` the cache does not hold | at most one read per thirty seconds, **however many requests arrive at once**: the window is compared and reserved in one lock acquisition, so concurrent callers with forged key ids share the one read rather than getting one each. Without that bound a forged key id turns every request into a re-read, which is a denial-of-service primitive aimed at whatever serves the key set                                         |
| has a key been **removed** | age: the cached set is re-read once a minute   | one minute **while the source keeps answering with a usable document**, and no bounded time while it does not: a failing re-read keeps the previous keys verifying and logs `stale_for_ms`, and nothing refuses on that number. This is the one the caller cannot influence, and it is the one that matters for revocation - a caller presenting a revoked key presents an id the cache *has*, so nothing else would ever trigger |

The age re-read happens on a timer *and* on the first request past the horizon, so a deployment gets
the bound whether or not it is serving traffic. A candidate that will not parse, or that holds no key
of the pinned kind, is logged at `error` and **not** adopted: the previous keys keep verifying, because
adopting a broken set turns a rotation mistake into a total outage.

### What a scope grants

**Only where `security.inbound` is configured.** A deployment with no block has no verified claim to
narrow by, so every operation is available - and a filter over an unverified claim would look like a
control and be none.

Two scopes, one per operation this surface has. They are the same strings for every deployment, and
they name a **capability** rather than a metric - deliberately, so that adding or renaming a metric in
the catalog can never change what a token means:

| Scope                 | What it grants    |
| --------------------- | ----------------- |
| `sutura:catalog.read` | `GET /v1/catalog` |
| `sutura:metrics.ask`  | `POST /v1/query`  |

**It fails closed.** A caller is granted exactly the capabilities its `scope` claim names. A token
this deployment verified that carries no capability scope reaches **nothing** - which means switching
`security.inbound` on before authoring these scopes at your authorization server switches every caller
off. The response says so and says what to add: `403`, `code: insufficient_scope`, and the missing
scope string in the detail. A scope this surface does not know is ignored rather than refused, so a
token minted for other resources too is fine.

**What it does not grant, and this is the sentence to keep.** A scope decides which *operations* a
caller may invoke. It decides nothing about which rows an answer contains. Both operations read the
same pinned bundle. `files` and `postgres` execute under their declared shared identity regardless
of the caller's scope. The shipped BigQuery adapter can carry a verified caller's assertion through
its declared per-subject account map, but no served run has proven source acceptance of that path.
The scope grants permission to ask; the source's declared posture determines whose access governs
the rows.

The agent surface offers the same two capabilities under the names `describe_catalog` and `ask_metric`,
from the same declaration, so the two transports cannot describe different tool sets. It speaks over
standard input and output, where there is no header a token could arrive in, so nothing narrows the set
there today.

It is served by the `sutura` binary's `mcp` command, not by this one, and that is the point of the
split: MCP-over-stdio is a locally launched, single-player surface, so it belongs with the command-line
tool that composes the in-process engine over a data directory rather than with the HTTP service.

```bash
sutura mcp examples/single-player/catalog examples/single-player/data
```

An agent client launches that process and speaks the protocol on its pipes - the same two tools this
page describes, from the same `sutura_app::Capability` declaration. The command prints at startup, on
standard error, that it grants every capability to whoever can reach the process, how many questions
it will answer at once, and how long a peer waits for one of them: a pipe has no header a token could
arrive in, which is the limit stated beside the mode rather than left as a default, and the two
numbers are [`runtime.max_concurrent_queries`](#capacity) and `server.request_timeout_seconds`, which
bound that surface exactly as they bound this one.

### The agent surface over HTTP

A second, default-off transport for the same agent surface exists behind `sutura-mcp`'s own `http`
feature: the streamable-HTTP transport `docs/adr/0023` decided on. Since `telekom/sutura#378` PR4
`sutura serve` mounts it at `/mcp`, behind this surface's leg 1 and `establish_asked` -
which is why it only ever serves where a caller can be verified. It is off by default at two
gates, and every published binary has already cleared the first one:

- **Build time (`sutura-cli`'s `agent` feature).** A build without the feature cannot reference
  `sutura_mcp::http` at all, so the route is compiled out of the artefact; setting
  `server.agent_surface.enabled` against such a build is a startup refusal naming the feature.
  Every published binary carries `agent` since `github.com/telekom/sutura#685` step 5; a source
  build without `--features agent` is the one still gated here.
- **Deployment time (`server.agent_surface.enabled`, default `false`).** Even a build with the
  feature linked stays off until an operator sets the key, and setting it without also declaring
  `security.inbound` is a startup refusal (`AgentSurfaceWithoutInboundIdentity`) - `/mcp` is never
  served to "everyone".

Its one fixed decision, carried here so it does not arrive as an unstated default:
`legacy_session_mode: false`, which makes every request self-contained - a `Mcp-Session-Id` header
is never looked up, by any message type - so a caller can never be answered under an earlier
request's identity because no session exists for one to leak into. That guarantee is held by one
config flag: the transport still constructs the SDK's session manager, and `legacy_session_mode:
false` is what keeps it idle - and even where a session exists, `rmcp`'s `create_session` takes no
identity argument, so a session is never bound to a caller; the caller is re-resolved per request
from each request's `Asked`. This costs nothing a current MCP client needs: the pinned SDK still
serves `initialize`, `tools/list` and every other call one-shot under this configuration, protocol
version `2025-11-25` (its own advertised latest) included. Two further transport decisions land
with the mount and are `sutura-http`'s to state: the mount is on the ONE listener (Decision 2 of
`docs/adr/0015`, the same choice `/metrics` makes), and it is outside the version prefix because
MCP versions its own tool set by the protocol's `protocolVersion` negotiation, not by a route
prefix.

**What a verified caller reaches is narrowed per caller.** `establish_asked` derives each request's
`Asked` from the caller leg 1 verified, and `AgentSurface::permitted` answers `tools/list` with only
the tools that caller's `scope` grants - so two verified callers with different scopes see two
different tool lists, and an unverified caller is refused with leg 1's `401` before the transport
is reached. Leg 2 - a source executing AS the asking subject - resolves per source for `bigquery`
(`#376`, `docs/where-identity-is-proven.md`), but no served binary has executed it yet: every tool
answers under the deployment's own credential today, and against `files` or `postgres` it still
would even once one had.

## The endpoints

| Method and path                                               | Token                                                                                                               | What it is                                                                                                                                                                                                                                                                                                                            |
| ------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `GET /health`                                                 | no                                                                                                                  | Liveness. The body is exactly `{"status":"ok"}`                                                                                                                                                                                                                                                                                       |
| `GET /.well-known/oauth-protected-resource[/<resource path>]` | no; `direct` mode only                                                                                              | RFC 9728 protected-resource metadata: the configured resource identifier and authorization server                                                                                                                                                                                                                                     |
| `GET /v1/catalog`                                             | yes, when one is configured; plus `sutura:catalog.read` where `security.inbound` is                                 | The metrics this catalog defines, with grains, dimensions and the values a filter may use                                                                                                                                                                                                                                             |
| `POST /v1/query`                                              | yes, when one is configured; plus `sutura:metrics.ask` where `security.inbound` is                                  | One certified question. `200` only when it was answered; a refusal carries its own status - see [A refusal carries a status](#a-refusal-carries-a-status). `503 at_capacity` when no execution slot is free - see [Capacity](#capacity)                                                                                               |
| `POST /mcp`                                                   | the caller's own bearer, leg 1 (`security.inbound`, `direct`) - **only when `server.agent_surface.enabled` is set** | The agent surface: MCP JSON-RPC over the streamable-HTTP transport (`docs/adr/0023`). `tools/list` answers with the tools the caller's own scope grants, narrowing per caller; no verified bearer gets the same leg-1 `401` every forgery does, before the transport. See [the agent surface over HTTP](#the-agent-surface-over-http) |
| `GET /metrics`                                                | its own token, never `security.access_token`                                                                        | This process's counters, in the Prometheus text exposition format. `401` without the metrics credential. Outside the version prefix and outside the capacity bound - see [the metrics endpoint](#the-metrics-endpoint)                                                                                                                |
| `GET /openapi.json`                                           | yes, when one is configured                                                                                         | The generated interface description                                                                                                                                                                                                                                                                                                   |
| `GET /docs`                                                   | yes, when one is configured                                                                                         | A browser interface over that description                                                                                                                                                                                                                                                                                             |

`/health` is outside the version prefix on purpose: a probe must keep working across a version bump
without an orchestrator being reconfigured. It carries no version, no build identifier, no
dependency list, no configuration and no catalog content, because an unauthenticated caller can
always reach it - so every field it might have is a field handed to anybody who can route a packet.

**A limit on the allowlist, not on the two routes above.** `/health` and `/metrics` are the existing
shape of a plain, non-wildcard `.route(` merged at the top level of `assemble` outside the version
prefix and outside `Ungoverned::mount` - and nothing new here holds that shape. A future route
merged the same way is not caught by the `Ungoverned` type (it never touches the mount), by
`check-boundaries`' text scan (only `.nest`/`.nest_service`/`.route_service`/`.fallback_service` and
a wildcard `.route` are needles - a named `.route(` is deliberately not one, for the reason stated
at `xtask/src/boundaries/ungoverned.rs`), by `ungoverned_routes()`'s allowlist record (nothing is
recorded for it to check), or by a behaviour cell (none dials it). It is held by review alone.

The interface description is served everywhere except production, where it is off by default. It
describes the surface, which is business information even with no row of data in it.

### The metrics endpoint

`GET /metrics` renders this service state's counters in the Prometheus text exposition format. The
shipped server constructs one state per process; separately embedded states do not share a registry.
The endpoint has **its own credential**, `security.metrics_token`, and the deployment token is refused there: a holder of
the API token can read the whole catalog and ask any question the catalog certifies, and a scrape
needs none of that. Configuring the two to the same value is a startup refusal - see
[What it will not start with](#what-it-will-not-start-with).

**One listener, and the trade-off belongs to the operator.** `/metrics` is mounted on the same
listener as `/health` and the versioned API, outside the version prefix so a scrape configuration
survives a version bump. It is not a second listener: a second bind address would be a second drain
to keep in agreement, a second TLS decision and a second load-balancer path. **The cost is real** -
the endpoint is reachable wherever the API is, and the credential is what defends it. A deployment
that must expose the API on a routable address while keeping `/metrics` on a cluster-internal
interface has only the network as its control today; a second listener is the change that would
express it (`docs/adr/0015`, Decision 2).

**A scrape does not make the service work.** The handler holds the registry and nothing else - no
`Surface`, no catalog, no engine, no data system - so it cannot load a catalog or execute a question
even by mistake. Rendering reads atomics and fixed strings, takes no lock any request path holds, and
is deliberately outside the capacity bound: a scrape keeps answering while every execution slot is
full, which is exactly when an operator is watching. It sits behind its own rate-limit tier, so a
wrong-credential scrape costs a bucket cell rather than being free.

**Every label is a closed, static value**, chosen from this transport's own fixed failure codes and
its three limiter tier names. No question text, metric or dimension name, source name, caller
address, credential or definition digest can become a label, because a label value is a `&'static
str` and none of those is one. The rendered exposition is asserted byte for byte in
`crates/sutura-http/src/harness/metrics.rs`, so a series or label added anywhere is a failing test
rather than a silent change.

**What it is not.** Not an access log and not an audit record: a scrape sees counters, nothing that
names a caller and nothing that names a question, and this endpoint writes no audit record. Four
families specified by `docs/adr/0015` are deliberately not exported: `sutura_build_info`,
whose labels would have to carry per-deploy version strings the label type cannot hold, and three
engine-pool families, which stay absent rather than zero because the pool bounds the engine's own
operators and not process memory. Scrapes use the shared info-level request trace, so the endpoint is
excluded from question counters but not from request logs.

### A refusal carries a status

`POST /v1/query` answers `200` when the question was **answered** and nothing else. A refusal carries
an explicit status, the stable `code` it always carried, and a sentence saying what to change - all
three, so a caller is told the same thing whether it reads the status, the code or the prose. The
`outcome` field is what says which envelope arrived:

```json
{
  "outcome": "answer",
  "provenance": {
    "definition_version": "local-1",
    "definition_digest": "c14afabdc65d0088523cfc6b805311547998c12ab1b540fd970b4f369b609270"
  },
  "columns": ["period", "recurring_revenue"],
  "rows": [
    ["2026-01-01", "237320"], ["2026-02-01", "232822"], ["2026-03-01", "216700"],
    ["2026-04-01", "206160"], ["2026-05-01", "202994"], ["2026-06-01", "202121"]
  ]
}
```

The second one comes back `404`:

```json
{
  "outcome": "refusal",
  "reason": {
    "code": "metric_unknown",
    "status": 404,
    "detail": "this catalog defines no metric called `customer_lifetime_value`"
  }
}
```

Both of those are `examples/single-player` over the wire, each captured as one line of JSON and
reformatted here. What ASSERTS them is `crates/sutura-cli/tests/served.rs`, against that same
directory on a kernel-chosen port, and the in-process harness in `crates/sutura-http/src/harness.rs`
one status at a time.

A refusal is still a *result* rather than an error - the caller asked something they may not have,
and the answer is no - and that is a statement about the domain, not about the status. Which status
depends on why:

| `code`                        | Status | What the caller does about it                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| ----------------------------- | ------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `metric_unknown`              | `404`  | Ask `GET /v1/catalog` which metrics this snapshot defines                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| `grain_not_supported`         | `422`  | The metric exists; that grain is not rendered for it. Pick one the catalog lists                                                                                                                                                                                                                                                                                                                                                                                                                                                                              |
| `time_range_too_long`         | `422`  | Narrow the period. The sentence carries the maximum                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| `too_many_dimensions`         | `422`  | Group by fewer. The sentence carries the maximum                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                              |
| `duplicate_dimension`         | `422`  | Send it once                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| `dimension_not_permitted`     | `403`  | The metric declares no such dimension                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         |
| `dimension_not_filterable`    | `403`  | It can be grouped by and not filtered on                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| `dimension_value_not_allowed` | `403`  | Use a value the catalog declares. The rejected value is never echoed back                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| `plan_spans_too_many_sources` | `409`  | Nothing. This deployment will not read from more data systems than it serves                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| `federation_not_executable`   | `409`  | Nothing. This build has no adapter that can execute one half of a two-source question yet                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| `federation_link_ambiguous`   | `409`  | Nothing. The question's remote dimensions join through more than one relationship                                                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| `federation_link_compound`    | `409`  | Nothing. The one relationship crossing into the second data system declares more than one join key, and the combiner links two legs on a single column                                                                                                                                                                                                                                                                                                                                                                                                        |
| `measure_does_not_federate`   | `409`  | Nothing. The measure's aggregate cannot be recombined above two legs                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| `result_too_large`            | `413`  | Narrow the period or group by fewer dimensions. Nothing was truncated to fit. **One code for three bounds:** more rows than this service's cap, more data than the data system would return at once, or more bytes than this service will encode into a response. The sentence says which, and names a number for the first and the third - the second bound belongs to the data system and is not reported to us                                                                                                                                             |
| `resources_exhausted`         | `422`  | Narrow the period, group by fewer dimensions or add a filter. The ceiling is a configured number and the sentence names it                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| `source_unavailable`          | `503`  | The one refusal worth retrying                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| `credential_unavailable`      | `403`  | Nothing you can send. You have no access to that data system, and this deployment will not read it as itself instead - the missing grant is at the data system                                                                                                                                                                                                                                                                                                                                                                                                |
| `deadline_exceeded`           | `422`  | Narrow the period, group by fewer dimensions or add a filter. This deployment stopped the question after its configured budget (`server.request_timeout_seconds` minus a one-second margin); the sentence names it. `docs/adr/0029` records the shape - the in-process engine stops at cooperative yield points, the default-off Postgres source uses `SET LOCAL statement_timeout`, and BigQuery sends what is left of it as the job's `jobTimeoutMs`, a best-effort stop at the service - a job stopped that way reads as a service error, not this refusal |

**The refusal `403`s are not about your credential.** No token and no scope widens a metric's
dimension set; a refusal `403` is the catalog's answer to "may this be asked of this metric", and the
sentence names the metric and the dimension so it cannot be mistaken for the other thing. A verified
caller is a caller whose identity is known, not a caller with more permissions.

**There are now TWO `403`s that ARE about a credential, and `code` is what tells all three apart.**
`insufficient_scope` means the credential you presented to THIS service is valid and does not carry
the scope the *operation* requires; it carries no `outcome` field, because it is a failure rather than
a refusal, and its detail names the scope to grant. It still says nothing about any metric - see *What
a scope grants*.

`credential_unavailable` is the other one, and it is about a credential **at the data system** rather
than at this service: the asking subject has no access there, and this deployment will not read that
source under its own identity instead. It IS a refusal, so it carries `outcome`, and re-authenticating
here changes nothing - what is missing is a grant somewhere else. It arrives with the credential port;
what can produce it today is a `bigquery` source declared `impersonation-at-source`, for a caller
its declared `impersonate` map does not name - the broker DOES hold a per-subject credential for a
caller the map covers. A `files` or `postgres` source declared `impersonation-at-source` never
reaches this refusal at all: the boot check above (*a source this build's adapter cannot
impersonate*) refuses it before any caller is involved, because the broker that ships mints from
configuration for those and holds no per-subject credential for either.

**And one `503` code is new on the failure side:** `identity_unavailable`, for the credential broker
not answering. It shares its status with `unavailable` and not its code, because an identity provider
that is down and a data system that is down clear at different times and are diagnosed in different
places.

**Two statuses are shared with something that is not a refusal**, and `code` is what separates them -
as is the body shape, because only a refusal carries `outcome`:

- `413` is `too_large` when the **request body** was over the limit, and `result_too_large` when the
  **answer** was too much data - over the row cap, over what the data system would return at once, or
  over this service's own ceiling on the bytes a rendered answer may occupy.
- `503` is `unavailable` or `at_capacity` from the failure side, and `source_unavailable` from the
  refusal side.

**And `422` rather than `503` for an exhausted working set, which is a distinction worth keeping.**
RFC 9110 defines 422 as a request that "repeating ... without modification will fail with the same
error" - exactly true of a configured bound. Exhaustion used to arrive as `503 unavailable`, which is
what a dead data system returns, so a caller was told to retry against a bound that would fire again.
The two are now separable by `code` as well as by status, and a test asserts the refusal is not 503.

This used to be a `200` for both outcomes, on the argument that an error status invites a client
library to retry a governance decision until it succeeds. The second half of that is right and the
first half does not survive checking: nothing mainstream retries a `4xx` by default, and 7 refusal
reasons land on `422`, which is documented the other way round, as a status a client should expect
to fail again on an unchanged request. What the `200` did cost was legibility to everything that reads
a status and not a body: an ingress log, a dashboard, an error-rate alert, a generated client whose
success branch is `2xx`. A deployment refusing every question read as perfectly healthy.
`docs/adr/0005-a-refusal-carries-a-status.md` is the record, and
[Questions and answers](qa.md) is what is refused and why.

Cells are rendered as text rather than as JSON numbers. A measure over integer minor units does not
survive a round trip through a JSON number in every client, and an anchor is compared as text - one
rendering everywhere means the number in an answer is the number in the anchor that certified it.

A body carrying `sql`, `table` or any other key the question shape does not declare is a `400`
naming the field. Without that, the key would be dropped silently and a caller who believed they sent
SQL would be answered as though they had asked the modelled question instead.

Every other failure is one shape:

```json
{ "code": "unauthorized", "status": 401, "detail": "this service requires a bearer token" }
```

A `500` carries no detail, ever. The text of an internal error is a path, a table name, a column name
or a driver message, and any of those handed to a caller describes the deployment.

Two failures share the `503` status and differ in `code`, which is what a client branches on:
`unavailable` is a data system that did not answer, and `at_capacity` is this service having no
execution slot free - see [Capacity](#capacity). Both are worth retrying, and they are diagnosed in
completely different places. Only `at_capacity` carries a `Retry-After`, and only because there the
number is already known: it is the admission window the caller just spent waiting out. Nothing here
knows when a data system will come back, so nothing invents a number for it - the refusal side
follows the same rule.

## Capacity

**Three numbers, and they bound three different things.** None of these capacity settings itself
cancels a question that has started. The separate per-request deadline is derived from
`server.request_timeout_seconds`; the engine, Postgres and BigQuery honour it with the different
mechanisms and limits below.

| Key                                 | Default                        | What it bounds                                                      |
| ----------------------------------- | ------------------------------ | ------------------------------------------------------------------- |
| `runtime.max_concurrent_queries`    | `8`                            | How many questions are **executing** at once. At most 512           |
| `runtime.admission_timeout_seconds` | `5`                            | How long a question waits for a slot before it is shed. At most 300 |
| `runtime.engine_worker_threads`     | as many as the machine reports | How wide the in-process engine's own runtime is. At most 256        |

### Why a bound on execution exists at all

`server.request_timeout_seconds` bounds the caller's **whole wait**. The same setting, minus a
one-second reply margin, opens the absolute deadline carried by the `Warehouse` port. The engine
returns after that deadline at a cooperative yield and drops its rows future; Postgres sends
`SET LOCAL statement_timeout` after acquiring its execution lock. **BigQuery sends what is left of
the deadline as the job's `jobTimeoutMs`** through the ADBC driver's `bigquery.query.job_timeout`,
which the service honours on a best-effort basis; this process cancels nothing itself, and that has
not yet been measured against the service. The concurrency bound is therefore load-bearing rather
than belt-and-braces here: it caps how much work can accumulate where cancellation is delayed,
cooperative, or best-effort.

`max_concurrent_queries` is that bound. A question holds its slot from the moment it starts until the
port call returns - **not** merely until the caller is answered. For the engine that is after the
timer is observed at a cooperative yield; for Postgres it is after the execution-lock wait and the
statement stop; for BigQuery it is whenever the driver answers, which nothing bounds - no request
asks the service to stop the job. The backlog is therefore bounded even when a timed-out caller
cannot stop the underlying work at all.

A question that cannot get a slot inside `admission_timeout_seconds` is answered `503` with
`code: at_capacity` and a `Retry-After` in seconds, rather than being left in a queue. `503` and not
`429` on purpose: a `429` says "you personally asked too often", which is a claim about the caller
and is the one the rate limiter already makes. This one is about the deployment, and it is true
whoever asked - a caller well inside their own rate limit can meet it.

The admission window is deliberately shorter than the request timeout. A caller who has waited five
seconds for a slot is better served by a `503` they can retry than by a `408` twenty-five seconds
later that says the same thing less clearly. Setting it *above* the request timeout is allowed and
does nothing: the timeout layer answers first.

**The bound is the process's and not this endpoint's**, which is why the same two keys bound the agent
surface the `mcp` command serves - and one `Admission` per process is what makes them the process's:
`sutura_runtime::Admission` is built by a composition root and handed to whatever serves, and
`cargo xtask check-one-bound` counts the constructions, failing a transport that builds one or a root
that builds two. Its own header states what a text scan cannot see.

`server.request_timeout_seconds` bounds the reply on that surface too, and it bounds the same thing:
the caller's **whole** wait, admission included. Here that is because the timeout is an outer layer
and the admission wait happens inside it; there it is because one deadline wraps both waits. So the
paragraph above holds on both surfaces - a window at or above the reply deadline is allowed and does
nothing, because the deadline answers first.

What differs is how each answer comes back: there is no status code on a pipe, so a shed question and
a question whose deadline expired are both a tool result marked as an error - the first saying to ask
again shortly, the second saying to ask for less. A deadline refusal means the adapter reported its
own stop; the transport timeout still cannot prove the underlying work stopped. The capacity bound
and admission window go to the log rather than into a model's context. Everything under *what it
does not bound* is true of that surface as well, and one thing more: a peer that sends
`notifications/cancelled` stops nothing and learns nothing until the deadline fires, because the
pinned MCP SDK delivers that cancellation as a token the handler does not read.

### What it does not bound

Stated plainly, because each of these has been mistaken for the thing above.

- **It does not cancel anything by itself.** The per-request deadline is a separate mechanism. The
  engine observes it at cooperative yield points, Postgres after its execution-lock wait, and
  BigQuery sends what remains to its service. None of those facts turns the concurrency ceiling into
  a cancellation mechanism.
- **It does not impose one universal duration bound.** Already-running blocking engine work may
  outlive the rows future, Postgres's lock wait is outside its statement timeout, and BigQuery's
  stopped-job reply is unmeasured while its socket allowance may cross the caller's reply margin.
- **It is not a per-caller budget.** One caller can fill every slot and shed everybody else. With leg 1
  configured two callers *can* now be told apart - and nothing does: there is no budget port to key on
  a principal, which is one of the four things [what is not built](#what-is-not-built) names. The
  limiter bounds an address's *rate*; this bounds the deployment's *concurrency*.
- **It does not size the engine.** The in-process engine has its own blocking thread pool at the
  runtime default, which this concurrency setting does not reach.

### The engine's width

The engine drives its own runtime and every request blocks on it from a pool thread, so how wide that
runtime is decides whether concurrent questions actually run concurrently. It used to be one thread,
which was right when the only caller was a command-line tool answering one question and is a ceiling
for a server. Measured - twenty questions per caller over a million rows, sixteen-way host,
throughput normalised to one caller on the old runtime:

| callers | one thread | `engine_worker_threads = callers` |
| ------- | ---------- | --------------------------------- |
| 1       | 1.00x      | 1.07x                             |
| 2       | 1.02x      | 2.08x                             |
| 4       | 1.03x      | 3.94x                             |
| 8       | 1.01x      | 6.06x                             |

The first column is the point: it is flat. A single-threaded engine runtime does not scale with
callers at all on this workload.

An absent key means "as many threads as this machine reports", resolved to a number at load time so
the startup log prints what is in effect rather than a policy. **A container with a CPU quota should
set it explicitly:** `available_parallelism` reports what the kernel exposes, which on most container
runtimes is the host's core count rather than the cgroup's share - so the default is usually too wide
there, and too wide costs memory as well as scheduling. The number also pins the engine's partition
count, so a narrow runtime does not build wide plans it then executes a few at a time.

The command-line tool is unaffected: it answers one question and exits, and one thread is the right
runtime for that.

## Configuration

Four layers, later beating earlier:

1. the defaults compiled into the binary - complete, so a deployment with no files is a working
   loopback development service rather than a failure;
2. `<dir>/base.yaml`, if `SUTURA_CONFIG_DIR` names a directory holding one;
3. `<dir>/<environment>.yaml`;
4. one environment variable per key: `SUTURA__SERVER__PORT` sets `server.port`.

Every layer is checked with `deny_unknown_fields` at every depth.

The environment is chosen by `SUTURA_ENVIRONMENT` and by nothing else - one of `development`, `test`
or `production`, defaulting to `development`. It is deliberately **not** a configuration key: it
selects which file is layered, so a file that could change it would be self-referential. Both
`environment:` in a file and `SUTURA__ENVIRONMENT` in the shell are unknown-key errors.

| Key                                             | Default                              | Notes                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| ----------------------------------------------- | ------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `server.host`                                   | `127.0.0.1`                          | An IP address, never a hostname: a name resolves to whatever the resolver says today. Either family - `::1` and `[::1]` are both read. See [Address families](#address-families)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         |
| `server.port`                                   | `8080`                               |                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| `server.request_timeout_seconds`                | `30`                                 | At most 300. Bounds a caller's whole wait on **both** surfaces - the `408` here, and a tool result on the agent surface. Minus a one-second reply margin, it also opens the shared execution deadline: the engine observes it at cooperative yield points and Postgres after its execution-lock wait. **BigQuery is the exception, and it is one an operator has to know:** what is left is sent as the job's `jobTimeoutMs`, a best-effort stop at the service, and this process cancels nothing itself - the `408` a caller sees does not by itself stop the work behind it                                                                                            |
| `server.max_body_bytes`                         | `65536`                              | At most one mebibyte. A question is a few hundred bytes                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| `server.agent_surface.enabled`                  | `false`                              | Mounts the agent surface at `/mcp`. Off by default even in a build with the `agent` feature linked. `true` with no `security.inbound` block refuses to start (the agent surface is only served where a caller can be verified), and `true` on a build without the `agent` feature refuses naming the feature. See [the agent surface over HTTP](#the-agent-surface-over-http)                                                                                                                                                                                                                                                                                            |
| `security.access_token`                         | absent                               | An RFC 6750 `b64token`, at least 32 characters. Required in production and on a non-loopback bind, **unless `security.inbound` is declared**                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| `security.metrics_token`                        | absent                               | An RFC 6750 `b64token`, at least 32 characters, gating `GET /metrics` and nothing else. Required in production and on a non-loopback bind, like the access token; equal to `security.access_token` is a refusal. See [the metrics endpoint](#the-metrics-endpoint)                                                                                                                                                                                                                                                                                                                                                                                                       |
| `security.tls_termination`                      | `none`                               | One of `none`, `sidecar`, `ingress`, `in-process`. Must be declared for any bind other hosts can reach                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
| `security.identity`                             | **absent, and absence is a refusal** | `single-user` or `multi-user`. Required once any source is configured. See [Sources](#sources)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| `security.single_user_because`                  | absent                               | The operator's reason. Required with `single-user`, refused with `multi-user`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            |
| `security.inbound.mode`                         | absent, and **no default**           | `direct` or `behind-gateway`. Absent means no per-caller identity; present-but-unset does not start. See [who is asking](#who-is-asking)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| `security.inbound.resource`                     | absent                               | `direct` only. This deployment's own resource identifier - an absolute `https` URI, no query, no fragment. What `aud` must equal, byte for byte                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| `security.inbound.authorization_server`         | absent                               | `direct` only. **The `iss` value, exactly** - it is compared byte for byte against the claim, not resolved as a URL. Copy it out of the issuer's own discovery document rather than typing the console URL: Entra's v1 and v2 endpoints publish *different* `iss` values for one tenant, and that is the classic way to configure this wrongly                                                                                                                                                                                                                                                                                                                           |
| `security.inbound.transit_header`               | absent                               | `behind-gateway` only. The header the component's **signed assertion** arrives in. Never a header holding a name. `authorization` is refused - it is the deployment token's                                                                                                                                                                                                                                                                                                                                                                                                                                                                                              |
| `security.inbound.transit_issuer`               | absent                               | `behind-gateway` only. Who must have signed the assertion, again as the `iss` value exactly                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                              |
| `security.inbound.transit_audience`             | absent                               | `behind-gateway` only. The audience the assertion must carry                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| `security.inbound.key_set_file`                 | absent                               | Both modes. A JWK set on disk. **There is no URL source** - see [what is not built](#what-is-not-built). Re-read on a timer and when a token names an unknown key; it must hold at least one key of the pinned algorithms' kind, or the process refuses to start                                                                                                                                                                                                                                                                                                                                                                                                         |
| `security.inbound.algorithms`                   | absent, and **no default**           | Both modes. One or more of `RS256`, `RS384`, `RS512`, `PS256`, `PS384`, `PS512`, `ES256`, `ES384`, `EdDSA`. `none` and every `HS*` are refused by name, and a list spanning two key kinds is refused because one token is verified by one key                                                                                                                                                                                                                                                                                                                                                                                                                            |
| `security.inbound.token_type`                   | `at+jwt`                             | `direct` only. Which class of token, out of the `typ` header. `any` switches the check off and is printed at `WARN` on every boot. **Leaving it alone is the safe reading** - see [who is asking](#who-is-asking)                                                                                                                                                                                                                                                                                                                                                                                                                                                        |
| `security.inbound.transit_token_type`           | absent, and **required**             | `behind-gateway` only. The class the component emits, or `any` if it sets none. Required because a component's `typ` is a fact only the deployment knows                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| `security.inbound.transit_max_lifetime_seconds` | `120`                                | `behind-gateway` only. The longest `exp - iat` this deployment will call short-lived. Between 1 and 3600. An assertion with no `iat` is refused                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| `security.credential_cache.enabled`             | `false`                              | **Parsed and read by nothing: there is no cache in this build.** The module that held one was private to the only broker that exchanged a credential, and both are deleted - the BigQuery path hands the asking subject's own assertion to the driver, so no exchanged material exists to hold. `true` gets no cache and no warning. Removing the three keys is a config-schema change; see `docs/adr/0031-caching-an-exchanged-credential.md`'s second amendment                                                                                                                                                                                                        |
| `security.credential_cache.capacity`            | `1024`                               | What the most live entries WOULD be. Zero is still refused at startup, which is the only thing this key does today - see `enabled` above                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| `security.credential_cache.window_seconds`      | `300`                                | What the operator's own ceiling on an entry's life WOULD be. Zero is still refused at startup, and nothing else reads it. **The broker expiry floor this row used to name as one of three bounds is dormant, not a gap**: it was wired only from the deleted broker's own composition, so no deployment ever ran with one, and what still bounds a credential's use is `BoundToTheRequest::still_usable_at` comparing the assertion's own expiry per request                                                                                                                                                                                                             |
| `security.outbound.transport_anchors`           | absent                               | A PEM bundle path, or `system`. Trust anchors for a FIXED-HOST outbound client, of which **the datahub catalog reader is the only one left** - the BigQuery wire and the STS token exchange are deleted, and the ADBC driver verifies its own TLS against roots nothing in this repository reads, so this key reaches no BigQuery code at all. Distinct from a per-source `transport_anchors`, which only means something when the source's own entry names the host it dials. Absent verifies against the compiled-in roots, unchanged from every prior release; a present `security.outbound` naming no anchors does not start. See `docs/adr/0010`'s fourth amendment |
| `server.tls_certificate`                        | absent                               | A PEM chain. Only with `tls_termination: in-process`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| `server.tls_key`                                | absent                               | The matching PEM private key. Both halves or neither                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| `rate_limit.enabled`                            | follows the environment              | Off in development and test, on in production. `false` in production is refused                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| `rate_limit.probe_per_second`                   | `2`                                  | Liveness, protected-resource metadata, and the interface description                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| `rate_limit.probe_burst`                        | `5`                                  |                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| `rate_limit.api_per_second`                     | `10`                                 | The versioned API                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        |
| `rate_limit.api_burst`                          | `20`                                 |                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| `rate_limit.client_address`                     | `peer`                               | `peer` or `forwarded`. What a rate-limit bucket is counted against                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| `rate_limit.trusted_proxies`                    | empty                                | The hops whose `X-Forwarded-For` is believed. `forwarded` with this empty is refused                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| `telemetry.service_name`                        | `sutura`                             | What a collector groups by                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| `telemetry.filter`                              | `info`                               | `RUST_LOG` overrides it when set                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         |
| `telemetry.format`                              | follows the environment              | `bunyan` in production, `pretty` elsewhere                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| `api.docs`                                      | follows the environment              | Off in production, on elsewhere                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| `catalogs`                                      | one entry, named `model`             | **A LIST, not a group of top-level keys.** Each entry declares its own `name`, `kind`, `dir`, `data_dir` and `version` - see the rows below - and **the list is file-only**: `SUTURA__CATALOGS__0__DIR` does not index into it, because the environment layer builds a map and refuses a sequence it was asked to build instead                                                                                                                                                                                                                                                                                                                                          |
| `catalogs[].name`                               | `model`                              | What the contribution manifest keys this catalog on. Two entries with the same name are a refusal                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        |
| `catalogs[].kind`                               | `markdown`                           | Which adapter opens this catalog. `markdown`, `datahub`, `okf`, `openmetadata` or `rdbms`. `datahub` opens behind its default-off `--features datahub`; a build without the feature refuses the kind by name, naming it. `okf` opens on EVERY build (`sutura-catalog-okf` is an unconditional dependency of the binary). `openmetadata`/`rdbms` are declarable and refused by name on every build (no reader over a real deployment yet - see the follow-up to `github.com/telekom/sutura#152` and `#972`). **A deployment declares catalogs of ONE kind** - a `markdown` and a `datahub` entry together are refused, not merged; see `docs/adr/0016`                    |
| `catalogs[].dir`                                | `catalog`                            | `markdown`/`okf` only for its meaning; **required non-empty for every kind, including `datahub`**, which does not read it - a stated limit of the settings surface rather than a per-kind field. For `markdown`/`okf`, at most 1,000 documents / 16 MiB of documents in total; more is a startup refusal naming the bound; raising it is a source change, not a setting                                                                                                                                                                                                                                                                                                  |
| `catalogs[].data_dir`                           | `data`                               | `markdown` only for its meaning; **printed by the startup banner and read by nothing that opens a data system** even for a `markdown` catalog - a served source's files come from its own `sources.<alias>.data_dir`, and the `sutura` command reads that same entry or else the directory on its command line. Required non-empty for `datahub` and `okf` too, unread by both, the same stated limit as `dir` above                                                                                                                                                                                                                                                     |
| `catalogs[].version`                            | `unversioned`                        | A commit id or a build number. What identifies the snapshot                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                              |
| `catalogs[].endpoint`                           | absent, **required for `datahub`**   | `datahub` only. `scheme://host[:port]`, no trailing slash. `https://` to any host, verified against `security.outbound.transport_anchors` when one is declared (otherwise against the binary's compiled-in roots); `http://` only to an IP loopback literal - anything else is a startup refusal naming the endpoint                                                                                                                                                                                                                                                                                                                                                     |
| `catalogs[].token_file`                         | absent, **required for `datahub`**   | `datahub` only. Absolute path to a personal access token, read once at startup; the token itself is never written to settings                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            |
| `catalogs[].metric_property`                    | absent, **required for `datahub`**   | `datahub` only. The deployment-chosen structured property name a certified metric's content is read from - `docs/adr/0016` decision 7's *not ours to say*, so there is no default                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        |
| `catalogs[].deadline_seconds`                   | absent                               | `datahub` only. Absent means `sutura_catalog_datahub::http::DEFAULT_TIMEOUT_SECONDS`. Shared across the (up to) three requests one read makes; a declared zero is refused                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| `catalogs[].max_response_bytes`                 | absent                               | `datahub` only. Absent means `sutura_catalog_datahub::http::DEFAULT_MAX_RESPONSE_BYTES`. Read before decode; a declared zero is refused                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| `catalogs[].refresh_seconds`                    | absent                               | Every kind, not `datahub` alone. Absent means never re-read. A declared `0` is refused. **`sutura serve` polls on this interval and re-pins a changed digest** (`github.com/telekom/sutura#975`) - and no further: the swap does not reach a request already deciding what to compute, and `sutura mcp`/`sutura query` never poll at all, so a value here has no effect on those two commands                                                                                                                                                                                                                                                                            |

### Sources

**The service reads its data systems from `sources:`, one entry per data system, keyed by the alias a
model's `source:` names.**

| Key | Default | Notes |
| --- | ------- | ----- |

| `sources.<alias>.kind` | absent | `files`, or `bigquery`/`postgres`/`clickhouse`/`oracle` when that default-off feature was built in. Required, with no default |
| `sources.<alias>.data_dir` | absent | Where that source's files are. Required, and absolute |
| `sources.<alias>.host` | absent | Postgres, ClickHouse and Oracle. A DNS name or IP address. On Postgres, exactly one of `host` and `unix_socket`; on ClickHouse and Oracle it is the only dial, and on Oracle it must be a loopback address because `plaintext` is that kind's only mode. That confines the address you DECLARE, not the connection: the Oracle driver follows a listener's redirect to any address, still in plaintext. An IPv6 literal such as `::1` is written bare |
| `sources.<alias>.unix_socket` | absent | Postgres only. An absolute socket directory. Exactly one of `unix_socket` and `host`. Refused on ClickHouse and Oracle, which are dialled over TCP |
| `sources.<alias>.port` | absent | Postgres, ClickHouse and Oracle. Required; no guessed `5432`, `8123` or `1521` |
| `sources.<alias>.database` | absent | Postgres only. Required. Refused on ClickHouse, which sends no database with its statement - so a key here would be one nothing reads - and on Oracle, which names its database by `service_name` |
| `sources.<alias>.service_name` | absent | Oracle only. Required: the service the listener resolves, the path of an EZCONNECT `host:port/service_name` - not a SID. ASCII letters, digits, `_` and `.` only: the driver would read anything after another character as something else. Refused on every other kind |
| `sources.<alias>.user` | absent | Postgres, ClickHouse and Oracle. The one role every caller reaches this source as |
| `sources.<alias>.password_file` | absent | Postgres, ClickHouse and Oracle. Absolute, read at startup; secret text is refused in the settings tree |
| `sources.<alias>.transport_mode` | absent | Postgres, ClickHouse and Oracle. `plaintext`, `verified` or `mutual`; required, with no default. A non-loopback host declared `plaintext` is refused on all three. Oracle accepts `plaintext` only: its driver trusts the certificate authorities compiled into it and takes no declared trust store, so `verified` and `mutual` are refused rather than half-honoured |
| `sources.<alias>.transport_anchors` | absent | Postgres and ClickHouse TLS. `system` as an explicit choice, or an absolute PEM bundle path. Not accepted on Oracle - see `transport_mode` |
| `sources.<alias>.client_certificate` | absent | Postgres and ClickHouse mutual TLS. Absolute PEM chain; both client identity halves or neither |
| `sources.<alias>.client_key` | absent | Postgres and ClickHouse mutual TLS. Absolute PEM private key; both client identity halves or neither |
| `sources.<alias>.posture` | absent | `shared-service-user` or `impersonation-at-source`. Required, with no default |
| `sources.<alias>.acknowledged_because` | absent | The operator's reason. Required for a shared source in `multi-user` mode |
| `sources.<alias>.verification_identity` | absent | The identity that re-runs that source's anchors. Only on an impersonating source |
| `tools.run_sql.enabled` | `false` | `docs/adr/0013`'s raw SQL tool. Refused at boot with a shared source in `multi-user` mode - see [The raw SQL tool](#the-raw-sql-tool-over-the-postgres-source-above) |
| `runtime.max_concurrent_queries` | `8` | How many questions execute at once. See [Capacity](#capacity) |
| `runtime.admission_timeout_seconds` | `5` | How long one waits for a slot before it is shed `503` |
| `runtime.engine_worker_threads` | the machine's | How wide the in-process engine runs. Set it under a CPU quota |
| `runtime.shutdown_grace_seconds` | `15` | The budget for the whole of stopping. See [Stopping](#stopping) |

The job's DEADLINE is not a key here: it is filled from `server.request_timeout_seconds` - **and it
reaches the data system for Postgres and the in-process engine, and BigQuery as a best-effort
`jobTimeoutMs`.** What is left of it is the ADBC driver's `bigquery.query.job_timeout`; the service
stops a job that outlives it on a best-effort basis, and until it does the job is billed for a result
nobody is waiting for. `docs/adr/0029`'s third amendment carries the limit.

**`credential_file` and `max_bytes_billed` are both required for a `bigquery` source; one of them
reaches nothing and the other is now the bound on what a question may cost.** The driver
authenticates itself, so the credential path is checked to be absolute and then passed nowhere.
`max_bytes_billed` is parsed at boot and sent on every statement the ADBC transport submits, as that
driver's `bigquery.query.max_bytes_billed` option - which is BigQuery's own `maximumBytesBilled` job
configuration. So the bound is enforced **by BigQuery and not by a check here**: a job that would
scan past the ceiling fails and is not charged. It covers every statement, not only a question's own
legs - a verified anchor, the identity read and a fixture load go through the same call.

**Two values are refused at boot rather than sent**, and this is a refusal and not the `WARN`
described in [One boot check that is a SIGNAL rather than a
refusal](#one-boot-check-that-is-a-signal-rather-than-a-refusal-and-the-limit-it-leaves): that check
is about what the CATALOG claims, which is trusted for metadata, while this is a number the
deployment wrote about money, which is not trusted at all and cannot be re-asked later. A `0` is
BigQuery's own spelling of *no ceiling* - it reads the field as unset below one - so a deployment
that wrote the tightest possible bound would have been given none; and a value above one tebibyte is
refused because it is indistinguishable from no ceiling in practice. Both name the key and the
source in the startup failure.

**What that ceiling does NOT bound, and the distinction decides whether it is enough.** It bounds
BYTES BILLED for ONE JOB. N questions cost N times it. It is not a bound on a deployment's total
spend, on one subject's spend, or on any window. And on a capacity-priced reservation a job is
billed for SLOT TIME rather than for bytes scanned, so there the ceiling bounds the scan without
bounding the bill.

**`governance.per_replica_spend_ceiling` is a different key and it still bounds nothing on a
`bigquery` source.** That ceiling is charged from a dry run's ESTIMATE, and the shipped ADBC
transport has no call that prices a statement - `AdbcBigQuery::validate` declines, so
`BigQueryWarehouse::dry_run` answers `PreFlight::NotAsked`, and the ledger reads a `None` estimate as
*not counted*, never *free*: it charges nothing and refuses nothing. **So declaring that ceiling
beside a `bigquery` source is refused at startup** (`NotFitToServe::UnpricedSourceUnderSpendCeiling`)
rather than served as a control that never moves. The mechanism itself is real and covered by cells - but every one of them runs over a fake or
a test transport that prices, and no shipped adapter can reach the refusal. The pinned driver does
carry a `bigquery.query.dry_run` option, so closing this is possible rather than blocked; what it
needs is a venue that can observe the estimate the driver returns, and none of the checks here can.
**For a bound across questions, budget at the source system** too, with a cost control on the billing
project: that is outside this deployment and
outside this repository's reach, which is exactly why it is the answer.
`.agents/skills/sutura/invariants` carries the same limit beside the mechanism, and
`BigQueryWarehouse::PRICES_DRY_RUN` carries it at the declaration.

**Two things about which builds can serve this**, and the first is the one to check before writing
the block above:

- **`sutura serve` opens it only when built with `--features bigquery`.** A binary without the
  feature refuses the source at startup, naming the feature - a source build with the feature off
  is the only one that still refuses. Every published artefact carries it: `github.com/telekom/
  sutura#685` step 5 ships the `bigquery` adapter (and `postgres`, `tls`, `datahub`) in every
  release tarball and image, so opening a dataset needs no separate build any more.
- **One process opens one KIND of data system at a time.** A catalog whose models sit on a `files`
  source and a `bigquery` source is refused at startup, naming both entries - the registry a process
  holds is generic in one adapter type, and the alternative is a source nothing opened.

Two facts, declared by two different parties, and conflating them gives the mode two owners:

- **the deployment declares the POSTURE**, per source - which identity a query is to reach that source
  as;
- **the adapter declares its CAPABILITY**, in code - whether it can carry a per-subject credential at
  all. The in-process engine cannot: one process, one operating-system identity, and nowhere for a
  subject to appear. **The `BigQuery` adapter can**, and the mechanism is worth knowing because the
  exchange is not performed in this process: the transport puts the caller's own verified assertion
  behind a workload-identity credential document served over a loopback source, Google's token
  service performs the exchange, and the target the source declares reaches that credential as an
  impersonation URL. What has NOT been observed is the leg end to end - no served binary has executed
  as a caller, and `docs/where-identity-is-proven.md` is the authority for which venue may be cited
  for which claim. Saying which half is mechanism and which half is unproven is the point of the
  declaration.

The boot check compares them. A source configured to impersonate on an adapter that cannot does not
start, and there is no fallback.

### A `postgres` source, least authority, and its channel

Postgres is behind the `postgres` feature on `sutura`, including its `serve` subcommand. A build
without it refuses the entry by name and tells the operator which feature is absent; every
published artefact carries it, the same shape as `bigquery` above, since `github.com/telekom/
sutura#685` step 5. A source build without `--features postgres` (or none at all) is the one that
still needs to ask for it explicitly.

One remote, server-verified source is declared like this:

```yaml
security:
  identity: "multi-user"

sources:
  warehouse:
    kind: "postgres"
    host: "db.example.com"
    port: 5432
    database: "analytics"
    user: "sutura_reader"
    password_file: "/etc/sutura/postgres-password"
    transport_mode: "verified"
    # An explicit choice, never a default. Use `system` to read the host store instead.
    transport_anchors: "/etc/sutura/database-ca.pem"
    posture: "shared-service-user"
    acknowledged_because: "the reporting role is intentionally the same for every caller"
```

The password file contains only the password and should be readable by the service account alone.
The process reads and trims it at startup; an unreadable or empty file stops the process. The source
entry cannot contain the password itself. No host or port is inferred, and `host` and `unix_socket`
are mutually exclusive.

`transport_mode` has three states, not a verification flag:

- `plaintext` uses no TLS. It is accepted only with an absolute unix-socket directory or a loopback
  IP literal; a hostname or non-loopback address is a startup refusal.
- `verified` requires `transport_anchors` and requires the TLS handshake. `system` means the host's
  trust store because the operator wrote it; an absolute path means that PEM bundle alone. It
  presents nothing, so a `client_certificate` or `client_key` written on a `verified` entry is a
  **startup refusal naming the key**, never a setting read past - the mode that presents a
  certificate is `mutual`.
- `mutual` adds `client_certificate` and `client_key`, both absolute and both required. The source
  still verifies the server against `transport_anchors`. The client certificate identifies this
  deployment, not the caller, so it does not change the `shared-service-user` posture.

Create a login role with only the database and objects this catalog names. In ordinary PostgreSQL
terms that means `CONNECT` on the database, `USAGE` on the selected schemas, and `SELECT` on the
named tables (plus equivalent grants for future tables only if the deployment actually needs them).
Do not make the role an owner, superuser, creator, or `BYPASSRLS`. If row-level security is meant to
separate callers, this static source cannot deliver it: every question uses the same role and sees
the same policy result. Per-subject Postgres credentials are separate work.

The gate-backed example loads `examples/single-player/data/*.csv` into the provisioned Postgres tier,
starts the real `sutura serve` binary with the declaration above over verified loopback TLS, and asks
the example's certified June revenue question over HTTP. Run it with `just test`, which is where the
tier is provisioned - `just serve-e2e` scopes `cargo nextest` to `sutura-cli` and does not source
`nix/with-tier.sh`, so run from a shell with no tier up it returns without asserting. No fixed
fixture port is involved either way.

| Posture                   | What it means                                                          | What decides what a subject sees                                                |
| ------------------------- | ---------------------------------------------------------------------- | ------------------------------------------------------------------------------- |
| `shared-service-user`     | Every query reaches the source under one identity the deployment holds | that identity's grants. Every caller sees the same rows                         |
| `impersonation-at-source` | Each query reaches the source as the asking subject                    | the SOURCE: its own authorization, its row and column policies, its own catalog |

**`shared-service-user` is honest, not broken.** It is right for a single-user deployment and right for
a source nobody needs to see per subject. The failure is never the posture; it is a source in that
posture being *believed* to impersonate - which is what the acknowledgement makes impossible to hold
accidentally.

**The mode does not make a caller identity arrive.** Nothing in this service establishes one: the
bearer token authenticates the deployment, and the startup log says so on every boot. What
`security.identity` decides today is where a shared source's acknowledgement has to be written, which
is what a deployment needs in place *before* a subject exists rather than after.

**Every answer carries the posture per leg**, in `provenance` on the HTTP surface and in `executed_as`
beside it. It is read off the adapter that executed rather than off this file, so a record cannot
report a leg as impersonated on the strength of a configuration key. Say plainly what that is worth:
it reaches a caller *after* the rows did, so it cannot prevent a disclosure. It makes one attributable
and it makes a misconfiguration visible to whoever reads an answer; the startup refusals above are the
gate.

**A catalog whose models sit on two declared sources is now servable**, and an engine is opened per
source the catalog names. A *question* whose plan spans exactly two is split by the plan stage into a
fact leg and a lookup leg, and `answer` either executes it or refuses it as `federation_not_executable`
while no adapter can execute a leg - so the split is never served as a partial or a half-executed
answer. Three or more sources refuse at plan time as `plan_spans_too_many_sources`.

*The limit, because it decides what is worth configuring today:* the default build links only the
in-process engine. Builds enabling `bigquery` or `postgres` add that one network adapter, and one
process still opens one KIND of data system at a time.

### The raw SQL tool, over the postgres source above

`docs/adr/0013-a-raw-sql-tool-off-by-default.md` is the record; this is the settings key
and the split between what this service enforces, what the connecting role enforces, and what
neither does. `examples/raw-sql/README.md` is the worked showcase - a settings snippet, a role grant,
one question with no certified metric, over the same Postgres source declared above.

**`tools.run_sql.enabled`, off by default, per deployment.** An absent `tools:` key is the ordinary
case `docs/adr/0013` calls normal, not a narrower mode of it: `run_sql` does not appear in `tools/list`
or the served OpenAPI document, and calling it by name is refused as `tool_not_enabled` - a different
code from `insufficient_scope`, because obtaining `sutura:sql.run` could not help a caller a
deployment switch refused. Turning it on is one line an operator writes and a reviewer sees.

**Enforced by this service, once the switch is on:**

- A caller still needs `sutura:sql.run` beside the deployment switch - two independent gates, and the
  switch alone does not widen anyone's scope.
- The result's wire shape has no field for a definition version, a digest or provenance of any kind -
  `RawOutcome` cannot be rendered as a certified answer because there is nowhere on the type to put
  one, not because a check catches it trying.
- The row cap (`MAX_ROWS`) applies exactly as it does to a certified answer, enforced by reading no
  more than that many rows past the limit off a STREAMED result rather than materialising the whole
  answer first.
- One statement per call. The extended query protocol this adapter uses cannot carry a second command
  in the same `Parse` message, so `select 1; drop table t` is refused by the SERVER as a syntax error
  before either half runs - sutura reads no keyword out of the text to decide this.
- **A boot refusal**, reusing the same `security.identity` mechanism the shared-source acknowledgement
  above already uses: `tools.run_sql.enabled: true` with `security.identity: multi-user` does not
  start. Abridged: *"tools.run_sql.enabled is true and security.identity is `multi-user`. The raw SQL
  tool executes under one shared identity for every caller... it may run only where the deployment is
  single-user or a source executes as the asking subject."* Today's Postgres adapter cannot execute as
  the asking subject, so this refuses on the declared mode alone - stated as the limit it is: a
  boot-time check over a written word, not a runtime measurement that callers are actually one person.
- **A minimal audit record per call, refusal included**, carrying the chain, the row count or the
  refusal, and the statement text as an audit-only field this service never returns to a caller.

**Enforced by the connecting role, and by nothing else:**

- **What the statement may read or write.** Every call runs inside a transaction this adapter opens
  `BEGIN READ ONLY` and always rolls back - a real, server-enforced second control beside the role,
  closing the session-level escape (`SET TRANSACTION READ WRITE`, `default_transaction_read_only`)
  `docs/adr/0013-a-raw-sql-tool-off-by-default.md` already rejects as undoable by the
  caller's own next statement. But that transaction bounds SQL-visible writes for the DURATION of one
  call; it says nothing about what the role could otherwise do, and nothing about a VOLATILE
  function's own side effects (a file write, a network call through an extension) once the role may
  call one at all. `docs/serving.md`'s general Postgres guidance above - `SELECT` on the named tables,
  never an owner, a superuser, a creator or `BYPASSRLS` - is what actually bounds this, and it is an
  operator's `GRANT`, not a setting sutura reads or verifies.
- **How long a statement may run.** The connect-time `statement_timeout` this source's connection
  already carries is the ceiling. It is one number for every caller today, not narrowed per request -
  `docs/adr/0013-a-raw-sql-tool-off-by-default.md` names the caller-derived deadline as a
  prerequisite this build does not yet carry for the raw path.

**What neither enforces, stated because an overstated control is the defect this repository names
directly:**

- **The intent boundary.** The scope gate bounds WHO may call `run_sql`; the role bounds WHAT it may
  read or write. Neither bounds what a prompt-injected instruction can talk the calling agent into
  SENDING as the statement - `docs/adr/0013`'s own accounting of what this tool spends, restated here
  because an operator reading only this page should still see it.
- **Whether the connecting role is actually narrowed to `SELECT`.** Sutura reads no privilege off the
  source; the `GRANT`s above are the only source of truth for what the role can do, exactly as they
  are for the certified path's own source credential.
- **Which of several open sources a statement runs against.** This build targets the sole registered
  data system and refuses rather than guesses where more than one is open; naming one is future work.

### Address families

`server.host` takes an address of either family. `127.0.0.1` and `::1` are both recognised as
loopback, so neither of them trips the off-host refusal above, and `::1` may be written bracketed or
bare. A rate-limit bucket is keyed on the canonical form of the address, so a client reaching a
dual-stack listener as `::ffff:1.2.3.4` shares the bucket of the same client reaching it as
`1.2.3.4` rather than getting a second one - and a v4 entry in `rate_limit.trusted_proxies` still
matches a v4-mapped peer, while never matching a real v6 address.

**Which families a listener actually accepts is the platform's default, not a decision this service
makes, and there is no key for it.** Nothing sets `IPV6_V6ONLY` either way. On Linux, whose default
is off, binding `::` accepts v4 connections too and reports them as v4-mapped; binding `0.0.0.0`
accepts v4 only. So an operator who wants both families gets them from a platform default rather than
by choosing them, and an operator who wants v6 *only* has no way to say so - they would have to set
the socket option outside this process. That is a posture nobody chose, stated here rather than left
to be discovered; a `server.address_family` key is where it would be fixed.

*Not observed end to end: the v6 serving path is covered by the address-parsing tests and by reading
`tokio::net::TcpListener::bind`, and by nothing that has actually accepted a v6 connection - the
development container has no v6 address at all. A test that skipped itself on a host without v6 would
read as coverage and is deliberately not here.*

## TLS

**Normally something else terminates it, and that is the intended arrangement rather than a
shortcut.** In a cluster an ingress controller or a sidecar ends the connection and the hop from
there to this process is plaintext on the pod network. So a non-loopback bind is *not* refused for
being plaintext; what is refused is a non-loopback bind that has not said where TLS is terminated.
`security.tls_termination` is that statement, and the point of writing it down is that the cleartext
hop it implies becomes a stated fact rather than an assumption - the bearer token crosses that hop.

| Declared     | What terminates TLS              | What the token crosses in cleartext                   |
| ------------ | -------------------------------- | ----------------------------------------------------- |
| `none`       | nothing                          | the whole path from the caller. Only sane on loopback |
| `sidecar`    | a proxy in this pod              | a loopback hop inside the pod                         |
| `ingress`    | an ingress controller or gateway | the pod network, from that hop to this process        |
| `in-process` | this process                     | nothing. The connection ends here                     |

`ingress` is therefore not a weaker `sidecar`: it is the same posture with a longer cleartext
segment, and whether that segment is acceptable is a question about the cluster network. A mesh with
mutual TLS between pods answers it differently from a flat one, and nothing here pretends to know.

### Terminating it in this process

For the deployment where nothing sits in front. It needs **a build that has a TLS listener in it**,
which every published binary is, since `github.com/telekom/sutura#685` step 5 - the plain `cargo
build`/`cargo run` this repository's own commands use is the one that is not:

```bash
cargo run -p sutura-cli --features tls -- serve
```

The feature is off by default in a source build because most deployments do not use it, and a TLS
stack compiled into an artifact that will never present a certificate is cost with no return. With
the feature off the dependency is absent from the build rather than merely unused, and asking for
`in-process` termination is a startup refusal that names the feature - so the two cannot disagree,
and a source build without `--features tls` handed this configuration stops rather than serving
cleartext.

Then:

```yaml
security:
  tls_termination: "in-process"
server:
  tls_certificate: "/tls/chain.pem"
  tls_key: "/tls/key.pem"
```

Both halves or neither. A certificate with no key is refused rather than half-configured, and a path
set to the empty string is an error naming the key rather than TLS quietly switching itself off -
an empty string is what an unset variable looks like in a shell.

The material is read and validated **before the socket is bound**: the chain must parse, the key must
parse, and the key's public half must match the certificate's. That last check is the one rustls does
not make on your behalf - a mismatched pair builds a server configuration quite happily and then
fails every handshake, at the client, with a signature error that names no file. So a configuration
mistake here is a process that does not start, and there is **no fallback to plaintext**: a port
somebody configured to be encrypted never comes up unencrypted.

rustls, not OpenSSL. No system library and no C toolchain requirement beyond what the build already
has, which is what keeps the statically linked targets buildable - there is no musl OpenSSL in
nixpkgs.

### Certificate renewal without dropping connections

A renewed certificate does not need a restart. The certificate and key paths are re-read on an
interval; when the bytes change, a candidate pair is built and validated in full, and only then does
it become what new handshakes are offered. A handshake already in flight keeps the pair it resolved,
so nothing in progress is disturbed and no connection is dropped.

**A bad new pair does not take the listener down.** If the replacement will not parse, or its key
does not match its certificate, it is logged at `error` naming both paths and discarded - the
listener keeps serving what it was already serving. Reloading into a broken state would be worse than
not reloading at all: every new connection would fail and the working pair would be gone.

Polling rather than a filesystem watch, and deliberately. Kubernetes replaces a projected Secret by
building a new directory and swapping a symlink, so an `inotify` watch on the file path follows the
old inode and never fires; getting that right means watching the directory and interpreting rename
events. Reading the path answers the question with no cases. Comparing the file *contents* rather
than a timestamp is the same choice again: an `mtime` a writer preserved is a rotation that never
happened. The cost is bounded staleness - up to the poll interval between the write and the swap -
which for something an issuer plans days ahead is nothing.

The arrangement this expects is the one most clusters already run: an external certificate manager
owns renewal and writes the files, and this process follows them. There is **no ACME client here**,
and that is a judgement rather than a gap - see below.

### Outbound trust material rotates the same way - one poll, per consumer's own swap

The serving-side renewal above is the SAME poll the outbound side uses (`github.com/telekom/sutura#125`
item 3, `docs/adr/0010`'s rule 3): the declared anchor bundle (or host store) and an optional client
identity are re-read on the same interval, the bytes compared, and a replacement validated before it
is adopted - the old material kept, with exactly one loud line naming the source class, when a new
one does not load. The DataHub reader's per-request `ureq` agent adopts the new bundle on the NEXT
request - there is no drain question, because
each request makes a fresh resolution. A Postgres source's connection keeps the client pair it was
established under until it closes - **left until closed, not drained**, because there is no connection
pool today and nothing to retire a live connection to. The interval is the same constant as the
serving side's (`sutura-tls::POLL_INTERVAL`, 30 s), for the same reason: a deployment shape, not a
knob. The boot log says once, per polled handle, what is polled and the interval.

### No ACME, and why

`rustls-acme` would do TLS-ALPN-01 with automatic renewal, which sounds like exactly this
requirement. It only makes sense when this process is the edge:

- TLS-ALPN-01 is validated by the certificate authority connecting to **port 443 of the name being
  issued for**. A pod behind an ingress controller is not what answers that connection - the
  ingress is - so the challenge cannot complete. In the deployment this service is normally in, an
  ACME client here would fail every renewal.
- It wants to own the listener, offering its own accept loop. That is the same objection recorded
  against `axum-server` in `crates/sutura-http/src/tls.rs`: this surface has a bounded drain on
  shutdown, and a second serving implementation would mean two drains to keep in agreement.
- Where this process *is* the edge, the ingress that would have terminated TLS is usually also the
  thing that would have obtained the certificate, so the deployments that could use ACME are the
  small ones - which are also the ones where a manually issued pair is least painful.

So the recommendation is file-watch reload plus an external issuer, which is what is built. ACME
belongs behind a *second* feature if it is ever wanted, gated on this process being the edge, and it
should not be the default.

## The log

One decision with two right answers, made from the environment and nothing else. In production a log
line is read by a collector, so it is one JSON object per line in the bunyan schema with the span
context attached; on a laptop the same line is read by a person recompiling every thirty seconds, so
it is indented and coloured. An explicit `telemetry.format` overrides it, and the startup log says
which of the two happened.

On boot the process prints an ASCII banner and the build line to standard output - before any
subscriber exists, so it is readable whatever the log format turns out to be - and then writes the
whole resolved configuration to the log. The configuration line is safe to emit because the only
credential-shaped value in the tree is held in a type whose `Debug` prints a placeholder, and that is
asserted by a test rather than by the log call being careful.

A panic is traced before the process gives up on it. The shipped profiles abort, so there is no
unwinding to catch; what a hook can still do is run first, with the payload and the location in
hand, so the last thing in the log says what happened and where instead of the log just stopping.

## Stopping

`SIGTERM` or an interrupt - and the platform equivalent elsewhere - drains in-flight work and logs
why it stopped. `runtime.shutdown_grace_seconds` is the budget, and it is the budget for the **whole**
of stopping rather than for the connection drain alone.

Fifteen seconds by default, chosen against the deadline on the other side rather than as a round
number: an orchestrator's usual `SIGTERM`-to-`SIGKILL` window is thirty, and a process still running
when that expires is killed mid-answer.

Stopping is two waits, in this order, sharing one budget:

1. **The connection drain.** Waiting for every open connection is what makes a rolling deployment not
   drop answers, and it is also how one connection nothing is going to close pins the process open.
   So the drain gets the budget, and the deadline arms only *after* shutdown has been asked for -
   before that a long-lived connection is not a deadline.
2. **Questions already executing.** Dropping the serve future ends the drain; it does not end the
   work. A question on the pool cannot be aborted, and the runtime's own shutdown waits for it - with
   no bound at all, which is what this budget's remainder now supplies. Whatever the drain did not
   spend is what the process waits here, and then it stops waiting and exits.

**What the number guarantees, precisely:** how long the process *waits*. Not that work finished, and
not that anything was cancelled - a question still running when the budget is spent is left running,
and the process exits out from under it. That is the honest trade, and it is the right one: a process
that exits on its own terms got to run whatever it does on the way out, and one that is `SIGKILL`ed
did not.

Spending the budget twice - a full grace period for the drain and then a full one again for the pool -
would be twice the number the operator chose, which is the number their kill timer is racing. So the
second wait gets the remainder, and the log line says how much that was.

A grace period shorter than a question is not refused, and it is not a misconfiguration either: it
says "stop on time even if that means abandoning an answer in flight", which is a legitimate posture
for a deployment being replaced. Nothing here can know how long a question takes, so nothing here can
check it - the number that *is* checked is that it is neither zero nor above five minutes.

## What is not built

Named rather than implied, because an absence that reads as an oversight gets assumed away.

- **No caller identity on the agent surface.** This bullet used to say the agent transport did not
  exist; then that a pipe (which `sutura mcp` serves over standard input and output) has no header a
  token could arrive in. Both remain true for the stdio surface: a locally launched, single-player
  process, offering every capability to whoever can launch it, and saying so at startup. What IS new
  is the network-reachable surface: `serve` now mounts `/mcp` behind its own leg 1, so a verified
  caller's scope narrows the tool list and an unverified one gets the same `401` every forgery gets.
  It is off by default and refused unless `security.inbound` is declared. Leg 2 - a source
  executing AS the asking subject (`#376`) - resolves per source for `bigquery`
  (`docs/where-identity-is-proven.md`), but no served binary has executed it yet, so even a
  verified caller is answered under the deployment's own credential today. See
  [the agent surface over HTTP](#the-agent-surface-over-http) and
  `docs/adr/0014-how-a-caller-proves-who-it-is.md`.
- **No record STORE.** This bullet said "no audit sink" and that had already stopped being true: there
  is an `AuditSink` port, `sutura-app` writes one record per outcome through it before the outcome
  returns, and the writer a deployment gets for free puts that record on the log below. What does not
  exist is retention - sutura keeps nothing, so what a record is worth is what the deployment's log
  pipeline is worth. What has changed with leg 1 is that the record can now name a **person** rather
  than only the deployment.
- **No key-set endpoint.** `security.inbound.key_set_file` reads a JWK set off disk, and there is no
  URL source: an outbound HTTP client is a supply-chain change with its own review, and it makes the
  authorization server a hard runtime dependency whose outage has to stay distinguishable from a dead
  data system. Everything a URL source would need is built - the cache, the refetch on an unknown key
  id, and the rate limit on that refetch - and a sidecar that rewrites a mounted key set is how a
  process with no egress rotates. **The limit a file has:** no cache header, so a key rotated *without*
  its id changing is one this deployment keeps using.
- **No replay protection on a gateway assertion.** The *window* is bounded - an `iat` is required and
  `exp - iat` is capped - and inside it an intercepted assertion replays. Closing that needs the
  assertion bound to the request (a hash of the method, path and body the component computes) or a
  store of what has been seen, and neither exists. That is why this page calls it an assertion rather
  than a proof of transit, and why the hop from the component is a trusted boundary.
- **No PROVEN source that executes as the asking subject.** Leg 1 establishes who is asking and the
  credential port makes a question unable to execute without a credential minted for its source.
  `bigquery` is a published adapter that CAN carry a per-subject one, federated per source
  (`docs/where-identity-is-proven.md`), but no served binary has executed a leg as the calling
  subject yet - and `files`/`postgres`, which is what a deployment gets without configuring
  `bigquery`, still cannot at all. See the first section - this is the single most important
  absence on this page.
- **No request identifier.** It belongs in the failure body and there is nothing to put in it, and a
  field that is always absent is worse than no field.
- **No readiness endpoint.** There is nothing it could report that is not already true of a process
  that is listening: the bundle validated, or the process did not start. One would arrive with the
  first thing that can become unready *after* startup.
- **No CORS.** A browser is not a client of this surface, and an allow-list nobody needs is an
  allow-list somebody widens.
- **No trace export, and this bullet is narrower than it used to be.** It used to say no metrics or trace
  export, and the metrics half is now built: `GET /metrics` renders this process's counters - see
  [the metrics endpoint](#the-metrics-endpoint). A span per request also exists and is rendered into
  the log, which is what makes one request's lines findable. What does not exist is a trace
  exporter, which is a decision about a backend, a sampling rate and an egress path none of which has
  been made.
- **No configurable client TLS for HTTP adapters.** Postgres now has a per-source three-state
  declaration, explicit anchors, and an optional client identity. The BigQuery wire does not honour
  it: `ureq` still verifies its fixed endpoint against its compiled-in root set and accepts no client
  certificate. Sharing the declaration with HTTP adapters and defining certificate rotation remain
  separate work; Postgres reading its files once at startup is not rotation.
- **No mutual TLS inbound either.** The listener above presents a certificate and verifies no
  client. Client-certificate authentication would be an identity, and this service has none to
  attach one to - see the first section.

## What a caller can still do

Stated plainly, because the posture above is a perimeter and not an authorisation model.

A caller who holds the token can read the whole catalog and ask any question the catalog certifies,
over any period inside the bound, with any permitted filter. There is no way to give one caller less
than that. The controls that exist are the shape of the question - no SQL, no table, no predicate, no
row ids - and the bounds on it, and those apply equally to everybody.

A caller behind a proxy shares a rate-limit bucket with everybody behind the same proxy **unless the
proxy is named**. The default keys on the connection's peer address, which cannot be forged and which
behind an ingress controller is *the ingress* for every request there has ever been - so the whole
internet is one bucket, and the limiter either takes everybody down with one abusive caller or is set
high enough to bound nothing. Setting `rate_limit.client_address: forwarded` and listing the hops in
`rate_limit.trusted_proxies` is what fixes it. Neither half works alone: `forwarded` with an empty
list is refused at startup, because a header nobody vouched for is a bucket the caller picks.

With a proxy named, `X-Forwarded-For` is read only when the *peer* is one of the named hops, and the
entry taken is the rightmost one that is not itself a named hop. A caller who prepends their own
value, or who sends their own header line before the proxy appends one, gets it skipped; a caller who
reaches this service directly and sets the header is ignored entirely. Every helper in the ecosystem
takes the leftmost entry, which hands the key straight to whoever sent the request.

A caller who does not hold the token can still consume rate-limit quota by presenting a wrong one -
that is deliberate and is the point of the limiter sitting *outside* the token gate. It also means an
unauthenticated caller can create a rate-limit bucket on any path that resolves to a handler. Those
buckets are swept on an interval, so the memory is bounded rather than growing for the life of the
process.

An unauthenticated caller can reach `/health` and learn that the process is up. In `direct` mode it
can also read the protected-resource metadata that tells clients which authorization server governs
the resource. A path under the version prefix that matches no route answers `404` without holding a
credential. The paths are in the published interface description in any case. Every other path that
resolves to a handler holds the API credential when one is configured, or `/metrics`'s own.

A caller with the token can occupy every execution slot and shed everybody else, inside their own
rate limit, by asking questions that each cost more than the request timeout. The `503` the others
get is honest and the backlog is bounded, but the *sharing* is not fair and is not made fair here.
With leg 1 there is now a principal to be fair *between* - and nothing keys anything on it: no budget
port exists, so what bounds a caller is still `rate_limit.api_per_second`, which bounds how fast one
address can start questions.

The same caller can keep a question running after being answered `408`, because nothing cancels one.
So the cost of a question is not bounded by anything the caller experiences - only the *number* of
them running at once is.
