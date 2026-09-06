# Where each identity claim is proven

Impersonation is sutura's central claim, and the venue a claim was tested in decides what the green run
means. This page is the map: **one row per claim, and the venue that can honestly answer it**.

It exists because the failure mode here is not a red run. It is a green one, read as proving more than
it did - and the most expensive version of that is a *mock* answering a question only a real system
can. So every venue below carries what it **cannot** answer, next to what it can.

!!! note "What this page is not"

    It is a map of venues, not a coverage report. **yes** means a venue can answer the claim and a
    test in it does; **can** means the venue is capable and the standing test lives somewhere else,
    with the cell saying where; **unrun** means the standing test lives HERE and nothing has run it;
    **wired** means a job now reaches that test and no run of it has been observed. The test names
    in the venue sections are what has actually been written, and they are the list to check a claim
    against.

    **`unrun` and `wired` are tokens and not caveats, and the difference is the point.** A written
    test is not a green run, and neither is a job that will run one, so a venue in either state may
    not be cited - and `cargo xtask check-venues` holds five things: only `yes` and `can` count as
    answering, neither token may be used by a venue nothing reaches, the venue's own section has to
    use whichever word its cell states, **a venue whose `Reached by` task CI invokes may not say
    `unrun`**, and **a venue whose `Reached by` task CI does not invoke may not say `wired`**. The
    last two are one rule pointing both ways, so exactly one of the two tokens is available for any
    given tree and neither can be reached by spelling.

    **Why there are two of them rather than one, which is a defect the fourth rule had:** the change
    that wires a leg into a job cannot also produce that leg's first green run - the run happens
    after the push. So for one commit the only moves were a cell the gate refuses and a `yes` nobody
    had earned, which is a gate satisfiable only by an overstatement. `wired` is that commit's
    honest state, and it is still not evidence.

    **What those two rules do not reach, said next to them:** what is read is an *invocation* in a
    workflow, a local composite action or the shared `nix/` shell - not a green run. A wired job
    that always skips reads the same as one that passes, and a run somebody did by hand is invisible
    to both. So *`unrun` has stopped being honest* and *`wired` is not yet earned* are mechanical;
    *`yes` is earned* is review's, with the run named beside it.

## The venues

| Venue | Where it runs | What it costs | Reached by |
| --- | --- | --- | --- |
| **A fake at the port** | in process, every run | nothing | `just test`, `just validate` |
| **A mock issuer in the sandbox** | in process, every run | nothing - no network, no docker, no secret | `just test`, `just validate` |
| **A real dataset under a shared key** | a GitHub environment, on demand | a service-account key and a billing project | `just bigquery-acceptance` |
| **A real dataset under two keys** | a GitHub environment, on demand | two more service-account keys, and a row access policy per principal | `just bigquery-two-principals` |
| **A real enterprise identity provider** | nowhere yet | a provider to configure and somebody to configure it | not built |
| **A real token exchange, and two grants** | nowhere yet | a workload-identity pool and two subjects with different access | not built |

The rule the mock issuer's row establishes: **the mock issuer is the default venue, and it may never be cited
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
    binary over a **mock issuer's** published key set, and those two are this page's mock-issuer venue on
    a composed binary. `just mcp-e2e` has no such case and cannot have one.

    So a green `serve-e2e` is evidence for exactly two rows below, and they are named rather than left
    to be counted: *the composition root arms leg 1 over the governed routes, or does not start*, and
    *the caller a signature established reaches the answer's own record*. Nothing else in the table.

## Which venue answers which claim

| Claim | Fake at the port | Mock issuer | Real dataset, shared key | Real dataset, two keys | Real provider | Real exchange |
| --- | --- | --- | --- | --- | --- | --- |
| A refusal is a result and every variant is reachable | **yes** | - | - | - | - | - |
| A caller cannot state its own identity | **yes** (a type with no `Deserialize`) | - | - | - | - | - |
| A signature verifies, and a forged one does not | - | **yes** | - | - | redundant | - |
| `kid` selection, and an unknown key id | - | **yes** | - | - | redundant | - |
| Algorithm confusion: `alg: none`, a symmetric key in the set, the wrong key family | - | **yes** | - | - | redundant | - |
| Issuer, audience against this deployment's own resource identifier, expiry, `nbf` | - | **yes** | - | - | redundant | - |
| An `aud` ARRAY, the form RFC 7519 permits | - | **can** - the builder takes several audiences; the standing test is at the gate | - | - | redundant | - |
| Token class: an ID token where an access token is required | - | **yes**, that *we refuse one* | - | - | redundant - we refuse the token from its own claims, and whether such a token can be OBTAINED is the bold row below | - |
| The `iat` ceiling on a gateway assertion | - | **can** - the builder takes `iat` and `exp` separately for exactly this; the standing test is at the gate, over in-crate fixtures | - | - | redundant | - |
| Key rotation: a removed key stops verifying within the bound | - | **yes**, and it is the only venue where a rotation is scriptable | - | - | redundant - painful to script there, and the mock issuer is the only scriptable venue | - |
| The refetch rate limit under concurrency | - | **yes**, at the cache - over a source that counts its own calls, never the published file | - | - | - | - |
| `credential_unavailable` through the request path | - | **yes** | - | - | - | - |
| Two subjects driving two different credentials to the port | - | **yes** | - | no - two credentials reach the port here, and neither is a subject's | - | - |
| The RFC 8693 request document a broker sends | **yes**, against a fake exchange | - | - | - | - | redundant |
| The document leg 1 verified is the `subject_token` the **shipped** exchanging broker offers | - | **yes**, over a fake exchange - the two halves were each green against their own fixture | - | - | - | redundant |
| A subject the shipped exchanging broker holds nothing for reaches no authorization server and no data system | - | **yes** | - | - | - | - |
| The composition root arms leg 1 over the governed routes, or does not start | - | **yes**, on the spawned binary | - | - | redundant | - |
| The caller a signature established reaches the answer's own record | - | **yes**, on the spawned binary - the only venue that can see it | - | - | redundant | - |
| **Whether a real provider will mint an ID token whose `aud` is a third party's client id** | no | **no - and a mock answers _yes_ by construction, which is worse than no test** | no | no | **only here** | no |
| Whether a statement we generate is accepted by a real data system | - | - | **yes** | redundant - the same endpoint, and the standing test is the shared-key leg | - | - |
| Whether a token exchange endpoint accepts what we send it | - | - | - | no | - | **only here** |
| **Whether two subjects read two different row sets** | no | no | no - one key is one identity | no - a key on disk is not an asking subject, which is this venue's whole exclusion | no | **only here** |
| **Whether a data system applies the row grant of the principal whose bearer a leg presented, so two principals read two different row sets** | no | no | no - one key is one identity, and it is the transport's own | **wired** - the only venue that could; `bigquery-acceptance` reaches it now and no run of it has been observed | no | redundant |

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

## A real dataset under two keys

`just bigquery-two-principals`, against the same environment's project and a **different dataset**: the
one whose table carries a `RowAccessPolicy` per principal, granting each of two service accounts a
disjoint set of rows. `docs/adr/0017`'s eighth amendment is the record, and issue #123 is the cell.

### What only this venue can answer

**Whether the data system applies the row grant of the principal whose bearer the leg PRESENTED**, rather
than the one the transport holds. `each_principal_reads_exactly_the_rows_its_row_access_policy_grants_and_not_the_others`
submits one statement twice - one `QueryPlan` value, borrowed twice, so it is the same statement and not
two that resemble each other - and asserts that each answer is made of that principal's own grouping
value and that **neither is empty**. The second is there because two vacuous greens over an unseeded
table is the shape this cell would otherwise pass as.

**There is no disjointness assertion, and its absence is a property rather than a gap.** It could not
fail once the two equalities passed - each answer is one grouping value, and the fixture control below
has already refused a pair whose two values are equal - and an assertion that cannot go red reads as a
third independent check while being none. That is why the refusal is a control with a test of its own.

`the_deployments_own_identity_reads_neither_principals_rows` is the control without which those two
greens are satisfied by a coincidence: the same statement over the same table under the DEPLOYMENT's own
credential, which holds no row grant on that table. If the rows a principal saw were really the
transport's, this leg would see them too. The transport's own credential is present on every leg of this
cell and read on none of them - `BigQueryWire::submit` decides which bearer authorizes the job once, and
a leg carrying a subject's credential never reaches the credential source.

Three tests here are **not** `#[ignore]`d, because they are controls on the fixture rather than on the
endpoint, and a fixture defect should fail in the gate every change runs:
`one_key_document_named_twice_is_refused_before_a_socket_is_opened` and
`one_grant_described_twice_is_refused_because_disjointness_would_be_unassertable` refuse a pair that
describes one principal twice - two environment variables pointing at one key document is one character
in a workflow, and it would produce two equal row sets that read as *the policies are not enforced*. And
`the_question_this_cell_asks_projects_the_column_the_policies_filter_on` holds that the question projects
the grouping column: without it the answer is one number per month, and two principals reading different
rows differ only by arithmetic.

### What it cannot answer - read this before citing a green run

1. **That the row this venue answers has been answered.** *No run has been observed*, which the
   matrix says in one word: **`wired`**, not `yes` and not `can`. The job step exists - the
   `bigquery-acceptance` job runs `nix run .#bigquery-two-principals` after the shared-key leg - and
   that is the whole distance between this state and the `unrun` this cell used to be in. It is not
   a shorter distance to evidence: a step that exists proves nothing about what it answered.

   **And the step will FAIL until somebody provisions five values**, deliberately. The policied
   dataset and table, the grouping column, and the value each policy grants are not in the
   environment that holds the two keys, and the step exits non-zero on any one of them being unset
   rather than skipping - so until `just infra-set` has pushed them, the honest reading of this venue
   is *wired, and red for a configuration reason*. That is why the change that wires it is held in
   draft rather than merged: a job red for a configuration reason is one people learn to ignore.

   **The mechanism, and what it does not reach.** `cargo xtask check-venues` resolves
   `just bigquery-two-principals` against every task and app the workflows, the local composite
   actions and the shared `nix/` shell invoke, and it refuses `unrun` the moment one of them reaches
   this venue and refuses `wired` while none of them does - one rule pointing both ways, so the cell
   cannot sit in the wrong one of the two. It also refuses either token from a venue nothing reaches,
   and a cell whose section does not use its own word. What it resolves is an *invocation*, not a
   green run: it cannot see a run's result, because the authority for that is the GitHub API and the
   sandbox the gate runs in cannot reach it. So a job that always skips reads exactly like one that
   passes, and a hand-run is invisible. Moving this cell to **`yes`** is therefore review's
   judgement with the run named beside it; what is mechanical is that neither `unrun` nor a silent
   `wired`-forever is available once the wiring lands.
2. **Whether a deployment can OBTAIN such a credential for the caller who asked.** Each bearer here is
   minted from a service-account key *on disk*, through the crate's own `Credential`, so what a green run
   establishes is that a source executes as the principal whose credential a leg carried. Nobody asked
   and nothing was exchanged. **This venue is leg 2's SOURCE half and not leg 2**, and the last row of
   the table is where the other half lives.
3. **Anything about a subject.** A key a test holds is not an asking subject, which is why the *two
   subjects read two different row sets* row stays `only here` on the exchange venue rather than moving
   up to this one. Two principals is not two subjects, and eliding those is the overstatement this page
   exists to prevent.
4. **Whether the grant survives what a deployment would do to the table.** This leg deliberately loads
   nothing: `BigQueryWarehouse::load_fixture` renders `CREATE OR REPLACE TABLE`, and replacing a table
   drops its row access policies - so the one loader this crate has would disarm the grant the cell
   asserts on, and there is no arbitrary-SQL path to reach for instead. The rows therefore belong beside
   the policies, in the stack.
5. **What the endpoint answers a principal no policy grants**, which decides how strong the control leg
   is. Documented behaviour is no rows; the observable alternative is a refusal. Both are *not reading
   either principal's rows*, so the control accepts either and prints which it got - and the first green
   run is what narrows this to one sentence. **The accepted set is exactly those two, held by an
   exhaustive match rather than by a wildcard:** review found the leg accepting every error the
   adapter has, including the ones that mean *rows came back and one cell would not map* - so a
   deployment that had just read the policied table was reported as having been refused, and the
   control passed without looking. A new error variant is now a compile error at that line. **What a
   refusal still cannot tell apart:** a `403` says this identity was refused, not which grant it was
   missing, so *no row access policy grants it* and *it may not submit jobs in this project* look the
   same here. Both satisfy the leg's assertion and neither is evidence about the policy.
   **MEASURED, and it is why this is still open:** the
   acceptance credential is refused `bigquery.rowAccessPolicies.create`, so a policy cannot be created,
   replaced or inspected from a developer machine at all - the probe that would have answered this
   returned `Access Denied ... Permission bigquery.rowAccessPolicies.create denied`. The policies are the
   stack's, and this question has no venue but a CI run.

## A real enterprise identity provider

Not built. Its job is the one row above that only it can answer, and keeping it to that row is the point
of this page.

## A real token exchange, and two grants

Not built. `sutura_exec_bigquery::WorkloadIdentityBroker` decides correctly against a fake exchange,
`StsOverHttp` serializes the documented request, and the mock-issuer venue above now shows that broker
reached **through the transport** with the caller's own verified token as the `subject_token`. What has
never happened is an exchange against a real endpoint, and no answer any deployment has produced was
evaluated under an asker.

Two things would make it a venue, and **one of them is now provisioned**: two identities whose access
at the data system genuinely differs exist, and the two-keys venue above is what they became. What is
left is the exchange itself - a workload-identity pool to exchange against, and a caller whose own
verified token is what a broker turns into one of those two identities. So *two subjects read two row
sets* is still a claim rather than a hope, and the reason has narrowed from *no differing access* to *no
subject bound to either grant*. Until that exists, `AGENTS.md` keeps the shipped position:

> no source a deployment SERVES executes as the asking subject.

## Keeping this page honest

A venue that cannot state its limit is how *verified* drifts. So:

- A new venue arrives as a row in the table above **with its exclusions written**, in the same change.
- A test moving from one venue to another moves its row, rather than gaining a second one.
- `just validate` runs every venue that needs no network. The other three do not, and each says so where
  it is invoked.
