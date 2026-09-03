# Where each identity claim is proven

Impersonation is sutura's central claim, and the venue a claim was tested in decides what the green run
means. This page is the map: **one row per claim, and the venue that can honestly answer it**.

It exists because the failure mode here is not a red run. It is a green one, read as proving more than
it did - and the most expensive version of that is a *mock* answering a question only a real system
can. So every venue below carries what it **cannot** answer, next to what it can.

!!! note "What this page is not"

    It is a map of venues, not a coverage report. **yes** means a venue can answer the claim and a
    test in it does; **can** means the venue is capable and the standing test lives somewhere else,
    with the cell saying where. The test names in the venue sections are what has actually been
    written, and they are the list to check a claim against.

## The venues

| Venue | Where it runs | What it costs | Reached by |
| --- | --- | --- | --- |
| **A fake at the port** | in process, every run | nothing | `just test`, `just validate` |
| **A mock issuer in the sandbox** | in process, every run | nothing - no network, no docker, no secret | `just test`, `just validate` |
| **A real dataset under a shared key** | a GitHub environment, on demand | a service-account key and a billing project | `just bigquery-acceptance` |
| **A real provider in the sandbox** | in process, every run | 19s of JVM per run - no network, no docker, no secret | `just test`, `just validate` |
| **A real enterprise identity provider** | nowhere yet | a provider to configure and somebody to configure it | not built |
| **A real token exchange, and two grants** | nowhere yet | a workload-identity pool and two subjects with different access | not built |

The rule the middle row establishes: **the mock issuer is the default venue, and it may never be cited
for the two claims it answers by construction.** A real provider stops being a prerequisite for testing
everything *around* it, and shrinks to the one job only it can do.

!!! warning "The two end-to-end suites are not venues for anything on this page"

    `just serve-e2e` and `just mcp-e2e` drive the composed HTTP surface and the composed agent surface
    end to end - a real settings file, a real catalog, a real listener or a real pipe, and a real
    answer. **Neither establishes an identity claim.** The example they run declares one shared
    identity, so every question in them is answered as the deployment; and a pipe has no header a token
    could arrive in, so the agent surface grants every capability to whoever can launch the process and
    says so at startup. They are transport venues, named here only so that a green run in one is not
    read as evidence in the table below. Leg 1 on a composed binary is the mock issuer's job.

## Which venue answers which claim

| Claim | Fake at the port | Mock issuer | Real dataset, shared key | Provider in the sandbox | Real enterprise provider | Real exchange |
| --- | --- | --- | --- | --- | --- | --- |
| A refusal is a result and every variant is reachable | **yes** | - | - | - | - | - |
| A caller cannot state its own identity | **yes** (a type with no `Deserialize`) | - | - | - | - | - |
| A signature verifies, and a forged one does not | - | **yes** | - | redundant | redundant | - |
| `kid` selection, and an unknown key id | - | **yes** | - | redundant | redundant | - |
| Algorithm confusion: `alg: none`, a symmetric key in the set, the wrong key family | - | **yes** | - | redundant | redundant | - |
| Issuer, audience against this deployment's own resource identifier, expiry, `nbf` | - | **yes** | - | redundant | redundant | - |
| An `aud` ARRAY, the form RFC 7519 permits | - | **can** - the builder takes several audiences; the standing test is at the gate | - | **can** - the audience mapper produces one, and the standing test is still at the gate | redundant | - |
| Token class: an ID token where an access token is required | - | **yes**, that *we refuse one* | - | - | see below | - |
| The `iat` ceiling on a gateway assertion | - | **can** - the builder takes `iat` and `exp` separately for exactly this; the standing test is at the gate, over in-crate fixtures | - | - | redundant | - |
| Key rotation: a removed key stops verifying within the bound | - | **yes**, and it is the only venue where a rotation is scriptable | - | not asked - a realm's keys are rotatable through `kcadm.sh` and nothing does it; the mock issuer is the venue | painful to script | - |
| The refetch rate limit under concurrency | - | **yes**, at the cache - over a source that counts its own calls, never the published file | - | - | - | - |
| `credential_unavailable` through the request path | - | **yes** | - | - | - | - |
| Two subjects driving two different credentials to the port | - | **yes** | - | - | - | - |
| The RFC 8693 request document a broker sends | **yes**, against a fake exchange | - | - | - | - | redundant |
| **Whether a real provider will mint an ID token whose `aud` is a third party's client id** | no | **no - and a mock answers _yes_ by construction, which is worse than no test** | no | **yes, and only here** | redundant for the mechanism; **only there** for whether a given deployment's policy permits it | no |
| Two subjects a provider distinguishes, each with a real signed ID token | no | **can** - `sub` is a parameter, so it shows the plumbing and not the provider | no | **yes** | redundant | - |
| Whether a statement we generate is accepted by a real data system | - | - | **yes** | - | - | - |
| Whether a token exchange endpoint accepts what we send it | - | - | - | no - there is no exchange endpoint in it | - | **only here** |
| **Whether two subjects read two different row sets** | no | no | no - one key is one identity | no - no data system is involved | no | **only here** |

## The fake at the port

`AGENTS.md`'s *Conventions* says why: *ports get fakes, not mocked HTTP - that is what lets the whole
tool surface, refusals included, be tested without a warehouse.* Every `RefusalReason` variant is
provoked somewhere, and a variant no test can provoke is one the enum refuses to carry.

What a fake cannot do is verify a signature, because there is no signature - which is the gap the next
venue fills.

## The mock issuer in the sandbox

`sutura_dev::issuer` is a mock authorization server, behind `sutura-dev`'s default-off `mock-issuer`
feature. It generates a key pair per test, writes a JWK set to a real path, and mints tokens **every
claim of which is a parameter** - because the useful tests are the negative ones.

**It produces real signatures over real documents.** `rcgen` generates the key, `jsonwebtoken` signs the
claim set, and the public half goes into a JWK the way an issuer publishes one, so the verifier under
test runs its real code path. A mock handing back a decoded claim set would be testing our test.

It is in `sutura-dev` rather than in the crate that first wanted it because leg 1 is verified in the
transport, minted-for in a broker and composed in a root - and a fixture living inside any one of those
three cannot be driven from the other two.

### What is proven here today

Through the **assembled router**, over `FileKeySet` reading a published document - the one key set
source that ships:

- `a_deployment_declaring_inbound_identity_answers_a_caller_it_verified_and_refuses_one_it_did_not`
- `every_negative_this_issuer_can_mint_is_refused_and_none_of_them_says_which_check_failed` - ten
  tokens, each the accepted one with exactly one thing moved, and the second assertion is that the ten
  responses are **indistinguishable**: a caller that could tell *your signature is wrong* from *your
  audience is wrong* has been told which half of a forgery to fix. One of the ten is the ID-token
  substitution `docs/adr/0014` records by name.
- `a_key_removed_from_the_published_set_stops_verifying_within_the_bound` - against a **file that
  changes**, which is what an operator actually does. Both sides of the bound are asserted: inside the
  age window the cached key is still in use, past it the retired key verifies nothing, and the key that
  stayed published still does.
- `a_forged_key_id_does_not_make_this_deployment_read_the_published_set`
- `a_published_set_this_deployment_cannot_use_starts_no_gate_at_all` - a symmetric key, an RSA key and
  one key under two entries, each refused while the gate is *built*, so it is a process that does not
  start rather than one that answers `401` to everybody.
- `all_three_curves_this_issuer_can_generate_verify_off_a_published_set` - the test that makes the
  *fixture* trustworthy rather than the code under test: the issuer writes a JWK by hand, and the two
  shapes it can get wrong are silent. `RS*` and `PS*` are absent and stay absent - three of nine, said
  out loud.
- `credential_unavailable_is_reachable_end_to_end`
- `two_subjects_drive_two_different_exchanged_credentials` - through the transport, and the assertion is
  on the **material**: each credential is derived from that caller's own assertion, so what is shown is
  that the document travelled and not only the name.
- `an_answer_records_the_posture_the_adapter_declared_and_not_the_one_a_file_says`

The first six live in `crates/sutura-http/src/inbound/tests/published.rs` and the last three in
`crates/sutura-http/src/identity_e2e.rs`, which is the same split the code has: one file is about
establishing who is asking and the other about what is minted for them.

**And the line the transport crate draws, because it decides which fixture a new test should reach
for:** a test that goes through the **router** mints from this venue, through the shared helpers in
`crates/sutura-http/src/testing.rs`; the gate-level unit tests keep their own in-place key pair and
encoder. Two reasons, neither of them tidiness. The router tests are the ones a composed-binary
harness will later re-point at a socket, and a fixture private to one module could not travel with
them. And the gate-level tests need a rawer tool than a mock issuer should be - they sign claim sets
with a `sub` carrying a newline, with no `exp` at all, and past the token size cap. Widening the
issuer to emit arbitrary JSON would make it a signer of anything and cost it the property that makes
it worth having: **every token it mints is one an issuer could have minted.**

### What it cannot answer - read this before citing a green run

1. **Whether a real identity provider will mint an ID token whose `aud` is a third party's client id.**
   A mock answers *yes* by construction, because the audience is a parameter. So the substitution test
   above says *we refuse such a token* and says nothing about whether one can be obtained. That question
   is answered by *a real provider in the sandbox*, below, and not here.
2. **How many times a source was read under concurrency.** A file cannot be counted, so the *exactly one
   read per window, whatever the interleaving* bound stays where it is measurable - at the cache, over a
   source that counts its own calls. The mock issuer venue asserts the *observable* half instead: a
   forged key id does not make the deployment look, and after the window it does.
3. **Anything about what a subject can see.** No data system is involved.

## A real dataset under a shared key

`just bigquery-acceptance`, against a GitHub environment's own dataset. `docs/adr/0017` and
`docs/adr/0019` are the records, and the sentence that matters here is short: **a service-account key is
one identity for everybody who asks**, so what those legs establish is *accepted, and correct for that
identity* - and nothing whatever about per-subject execution.

## A real provider in the sandbox

`nix/keycloak-tier.nix`, run by `checks.nextest` and by `just test` through `nix/with-tier.sh`, and
asked by `dev/tests/keycloak.rs`. It brings up nixpkgs' `keycloak` over **loopback only** and
provisions a realm, two clients, an audience mapper and two subjects through `kcadm.sh`, so **no
human is ever needed** and the whole venue fits in a build sandbox with no network and no docker
socket. `just keycloak-tier-up` starts it alone.

**What it is here for is one row**, and that row is the one the mock issuer answers *yes* to by
construction: **whether a real provider will mint an ID token whose `aud` is a third party's client
id.** `docs/adr/0008` records, verified against the vendor's documentation, that a
workforce-identity exchange requires exactly that - the provider's own configured client id in
`aud`, which is not the `audience` value sent to the exchange endpoint - so the shape of the token
a gateway would have to obtain is the provider's decision and not ours.

Measured on 2026-09-03, in this venue: a subject authenticating to the gateway client received an ID
token whose `aud` was `["tier-gateway", "tier-third-party"]` with `azp` of `tier-gateway`, and whose
ACCESS token from the same exchange carried no such audience - which is what makes it the mapper's
doing rather than something the provider does to every token.

### What it cannot answer - read this before citing a green run

1. **Whether any particular deployment will do it.** Keycloak is a real OIDC provider; it is not the
   enterprise provider somebody will federate with, and an organisation's policy can forbid a mapper
   that exists. What is shown is that the mechanism is real and configurable with no human. The row
   below is still the venue for the policy question.
2. **Nothing about leg 2.** `AGENTS.md` keeps the position that no source a deployment serves
   executes as the asking subject, and two real subjects holding two real tokens do not move it:
   what is missing is an adapter that can carry a per-subject credential.
3. **Not `docs/adr/0008`'s two-subject test.** That test asserts two subjects READ TWO DIFFERENT ROW
   SETS. This venue has no data system in it at all - it supplies two subjects whose tokens a
   validator would accept, which is the prerequisite and not the test.
4. **Nothing about verification.** The tests here decode a claim set and do not verify a signature,
   deliberately: whether a signature verifies, whether a forgery is refused and whether a rotation
   is noticed all belong to the mock issuer, over the code that does the verifying. Asking them here
   would spend 19s of JVM per run to re-establish what a fake already holds.

## A real enterprise identity provider

Not built. Its job is now narrower than it was: the row above moved the MECHANISM into the sandbox,
and what is left here is whether a given organisation's provider and policy will actually issue such
a token. Keeping it to that is the point of this page.

## A real token exchange, and two grants

Not built. `sutura_exec_bigquery::WorkloadIdentityBroker` decides correctly against a fake exchange and
`StsOverHttp` serializes the documented request; what has never happened is an exchange against a real
endpoint, and no answer any deployment has produced was evaluated under an asker.

Two things would make it a venue: a workload-identity pool to exchange against, and two subjects whose
access at the data system genuinely differs. The second is what makes *two subjects read two row sets* a
claim rather than a hope - and until it exists, `AGENTS.md` keeps the shipped position:

> no source a deployment SERVES executes as the asking subject.

## Keeping this page honest

A venue that cannot state its limit is how *verified* drifts. So:

- A new venue arrives as a row in the table above **with its exclusions written**, in the same change.
- A test moving from one venue to another moves its row, rather than gaining a second one.
- `just validate` runs every venue that needs no network - which is now three of them, because a
  provisioned provider over loopback needs none. The other three do not, and each says so where it
  is invoked.
