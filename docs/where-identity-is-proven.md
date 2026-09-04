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
| **A real enterprise identity provider** | nowhere yet | a provider to configure and somebody to configure it | not built |
| **A real token exchange, and two grants** | nowhere yet | a workload-identity pool and two subjects with different access | not built |

The rule the middle row establishes: **the mock issuer is the default venue, and it may never be cited
for the two claims it answers by construction.** A real provider stops being a prerequisite for testing
everything *around* it, and shrinks to the one job only it can do.

!!! warning "A green end-to-end suite is a transport claim, except where the mock issuer is in it"

    `just serve-e2e` and `just mcp-e2e` drive the composed HTTP surface and the composed agent surface
    end to end - a real settings file, a real catalog, a real listener or a real pipe, and a real
    answer. **Most of what is in them establishes no identity claim.** The example they run declares one
    shared identity, so those questions are answered as the deployment; and a pipe has no header a token
    could arrive in, so the agent surface grants every capability to whoever can launch the process and
    says so at startup.

    The exception is named rather than left to be found: two tests inside `just serve-e2e` spawn the
    binary over a **mock issuer's** published key set, and those two are this page's middle venue on a
    composed binary. `just mcp-e2e` has no such case and cannot have one.

    So a green `serve-e2e` is evidence for exactly two rows below, and they are named rather than left
    to be counted: *the composition root arms leg 1 over the governed routes, or does not start*, and
    *the caller a signature established reaches the answer's own record*. Nothing else in the table.

## Which venue answers which claim

| Claim | Fake at the port | Mock issuer | Real dataset, shared key | Real provider | Real exchange |
| --- | --- | --- | --- | --- | --- |
| A refusal is a result and every variant is reachable | **yes** | - | - | - | - |
| A caller cannot state its own identity | **yes** (a type with no `Deserialize`) | - | - | - | - |
| A signature verifies, and a forged one does not | - | **yes** | - | redundant | - |
| `kid` selection, and an unknown key id | - | **yes** | - | redundant | - |
| Algorithm confusion: `alg: none`, a symmetric key in the set, the wrong key family | - | **yes** | - | redundant | - |
| Issuer, audience against this deployment's own resource identifier, expiry, `nbf` | - | **yes** | - | redundant | - |
| An `aud` ARRAY, the form RFC 7519 permits | - | **can** - the builder takes several audiences; the standing test is at the gate | - | redundant | - |
| Token class: an ID token where an access token is required | - | **yes**, that *we refuse one* | - | redundant - we refuse the token from its own claims, and whether such a token can be OBTAINED is the bold row below | - |
| The `iat` ceiling on a gateway assertion | - | **can** - the builder takes `iat` and `exp` separately for exactly this; the standing test is at the gate, over in-crate fixtures | - | redundant | - |
| Key rotation: a removed key stops verifying within the bound | - | **yes**, and it is the only venue where a rotation is scriptable | - | redundant - painful to script there, and the mock issuer is the only scriptable venue | - |
| The refetch rate limit under concurrency | - | **yes**, at the cache - over a source that counts its own calls, never the published file | - | - | - |
| `credential_unavailable` through the request path | - | **yes** | - | - | - |
| Two subjects driving two different credentials to the port | - | **yes** | - | - | - |
| The RFC 8693 request document a broker sends | **yes**, against a fake exchange | - | - | - | redundant |
| The document leg 1 verified is the `subject_token` the **shipped** exchanging broker offers | - | **yes**, over a fake exchange - the two halves were each green against their own fixture | - | - | redundant |
| A subject the shipped exchanging broker holds nothing for reaches no authorization server and no data system | - | **yes** | - | - | - |
| The composition root arms leg 1 over the governed routes, or does not start | - | **yes**, on the spawned binary | - | redundant | - |
| The caller a signature established reaches the answer's own record | - | **yes**, on the spawned binary - the only venue that can see it | - | redundant | - |
| **Whether a real provider will mint an ID token whose `aud` is a third party's client id** | no | **no - and a mock answers _yes_ by construction, which is worse than no test** | no | **only here** | no |
| Whether a statement we generate is accepted by a real data system | - | - | **yes** | - | - |
| Whether a token exchange endpoint accepts what we send it | - | - | - | - | **only here** |
| **Whether two subjects read two different row sets** | no | no | no - one key is one identity | no | **only here** |

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
- `the_shipped_exchanging_broker_exchanges_the_document_leg_one_verified` - the **join**, and the one
  assertion neither half could make alone. `sutura_exec_bigquery::WorkloadIdentityBroker` is driven
  through this router over a fake `StsExchange`, and what is compared is the *bytes*: the `subject_token`
  the shipped broker sent is character for character the compact JWT the gate verified. Each half was
  already green against its own fixture, so a transport retaining a mangled assertion - the `Bearer`
  scheme still on it, say - would have left both suites passing while the exchange received garbage. The
  pool and the scope are asserted too, so what reached the exchange is the declaration held for that
  source rather than anything the request carried.
- `a_source_the_shipped_exchanging_broker_holds_nothing_for_is_refused_before_anything_is_exchanged` -
  the same `403` as above, produced by the **shipped** exchanging broker rather than a fake that only
  refuses, plus the two assertions a status code cannot carry: nothing reached the authorization server
  and nothing reached the data system.

The first six live in `crates/sutura-http/src/inbound/tests/published.rs` and the last five in
`crates/sutura-http/src/identity_e2e.rs`, which is the same split the code has: one file is about
establishing who is asking and the other about what is minted for them.

**What the two exchange-chain tests do NOT reach, and it is unchanged by them:** the fake at the port
is a fake, so nothing here says a real authorization server accepts that document - that stays the
last venue's only claim. And they are router tests rather than composed-binary ones for the reason
below: this composition root refuses to boot an `impersonation-at-source` source while no shipped
broker exchanges for one, so the binary cannot host them at all.

### And on the composed binary, which is a different claim from any of the above

`crates/sutura-serve/tests/served.rs` spawns the shipped binary over a settings file declaring
`security.inbound`, with a key set this issuer published to a real path. Two tests, and what they add
is not a signature check - that is the router's job above - but the **composition**:

- `the_composed_binary_verifies_a_callers_own_token_and_refuses_every_forgery_alike` - the real
  composition root read the key set its own settings file named, armed leg 1 before the listener
  opened, and mounted the gate over the governed routes: a caller's own token is answered with the
  example's number, and seven forgeries get one `401` that is byte-for-byte the same challenge. One of
  the seven is the ID-token substitution `docs/adr/0014` records by name. Two further assertions are
  there because review found them missing and neither is visible from a status code:
  **the caller the gate verified reaches the audit record** - `subject_established` and the subject the
  token named - because `executed_as` reports the *source's* posture and the rows are the example's
  number whether a caller was established or not, so a root that verifies and then answers as the
  deployment would otherwise pass; and **the liveness probe still answers with no token**, because leg 1
  is layered on the versioned router and merged beside the probe, and a change that layered the merged
  one would `401` every orchestrator.
- `a_published_key_set_this_deployment_cannot_use_stops_the_process` - a symmetric key in the set is a
  deployment that **does not start**, rather than one that starts, logs that it establishes a caller
  identity and answers `401` to everybody.

**The boot refusal is asserted on the refusal's own sentence, because nothing else separates the two
ways this deployment can fail to start:** a root that read the key set fine and then forgot to attach
the gate also exits non-zero, also never logs `leg 1 is armed` and also never listens. The startup
banner echoes the whole resolved configuration, so matching the key set's *path* passes for every
refusal this deployment can produce - measured on a build changed to swallow exactly that failure.
**And the limit of that:** `docs/serving.md` documents the *behaviour*, not the wording, so the matched
string is `sutura_http`'s own `Display` held by recall across a crate boundary. Tolerable for a startup
refusal, and not a mechanism - the test says so at the assertion rather than calling it documented.

**The audit assertion can only live here, which is the sharper reason this venue exists.** The record
is written from the blocking pool, so `sutura_http`'s log-capture harness - a *thread-scoped*
subscriber - cannot see it, and two of that crate's own tests stand in `tower_http`'s response line and
say so. A suite reading the process's own streams sees every thread. It is also why the wait is a
blocking read with a budget rather than a sweep: the record is not ordered against the response.

**What the composed binary cannot host, and why the tests above stay at the router:** the key-set
windows cannot be shortened from a settings file, so the rotation and rate-limit bounds would mean a
test sleeping for the shipped minute; and the exchange half needs a source declared
`impersonation-at-source`, which this composition root refuses to boot while no shipped broker
exchanges for one. Those two are the honest reason `credential_unavailable_is_reachable_end_to_end`
and `two_subjects_drive_two_different_exchanged_credentials` are router tests rather than binary ones.

**And the line the transport crate draws, because it decides which fixture a new test should reach
for:** a test that goes through the **router** mints from this venue, through the shared helpers in
`crates/sutura-http/src/testing.rs`; the gate-level unit tests keep their own in-place key pair and
encoder. Two reasons, neither of them tidiness. The router tests are the ones a composed-binary
harness re-points at a socket, and a fixture private to one module could not travel with
them. And the gate-level tests need a rawer tool than a mock issuer should be - they sign claim sets
with a `sub` carrying a newline, with no `exp` at all, and past the token size cap. Widening the
issuer to emit arbitrary JSON would make it a signer of anything and cost it the property that makes
it worth having: **every token it mints is one an issuer could have minted.**

### What it cannot answer - read this before citing a green run

1. **Whether a real identity provider will mint an ID token whose `aud` is a third party's client id.**
   A mock answers *yes* by construction, because the audience is a parameter. So the substitution test
   above says *we refuse such a token* and says nothing about whether one can be obtained. That question
   has exactly one venue, and this is not it.
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

## A real enterprise identity provider

Not built. Its job is the one row above that only it can answer, and keeping it to that row is the point
of this page.

## A real token exchange, and two grants

Not built. `sutura_exec_bigquery::WorkloadIdentityBroker` decides correctly against a fake exchange,
`StsOverHttp` serializes the documented request, and the mock-issuer venue above now shows that broker
reached **through the transport** with the caller's own verified token as the `subject_token`. What has
never happened is an exchange against a real endpoint, and no answer any deployment has produced was
evaluated under an asker.

Two things would make it a venue: a workload-identity pool to exchange against, and two subjects whose
access at the data system genuinely differs. The second is what makes *two subjects read two row sets* a
claim rather than a hope - and until it exists, `AGENTS.md` keeps the shipped position:

> no source a deployment SERVES executes as the asking subject.

## Keeping this page honest

A venue that cannot state its limit is how *verified* drifts. So:

- A new venue arrives as a row in the table above **with its exclusions written**, in the same change.
- A test moving from one venue to another moves its row, rather than gaining a second one.
- `just validate` runs every venue that needs no network. The other three do not, and each says so where
  it is invoked.
