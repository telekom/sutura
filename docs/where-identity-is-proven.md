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
    not be cited. `cargo xtask check-venues` holds it: only `yes` and `can` count as answering,
    neither token may be used by a venue nothing reaches, the venue's own section has to use
    whichever word its cell states, **a venue whose `Reached by` task CI invokes may not say
    `unrun`**, and **a venue whose `Reached by` task CI does not invoke may not say `wired`**. The
    last two are one rule pointing both ways, so exactly one of the two is available for any tree.

    **A `yes` cell over a venue that runs *a GitHub environment* - on demand - ties the same way,
    and it did not used to.** `unrun` and `wired` each had to appear in their own venue's section;
    a cell moving to `yes` had nothing tying it to the section beside it at all, so a row could
    leave either state in the matrix while its section still read as if it had not. Two rules now
    hold that cell for an on-demand venue: its section has to **name a run somebody observed** -
    the word `observed` in a sentence that is not a denial (not `No … observed`, `not observed` or
    `never observed`), plus either a GitHub Actions run link (`actions/runs/` followed by digits)
    or an ISO date (`2026-01-01`) - and it may no longer say `unrun` or `wired`, because a section
    still in either word has not left that state whatever the matrix now claims. A sentence that
    only denies a run, however many dates it names, is the opposite of a held `yes` and is refused
    with the rest; what a green run *means* is still review's to judge. An `in process` venue
    answers every push, so this does not reach it; only a venue nothing but a job's own run can
    prove is held to it.

    **Why there are two of them rather than one.** The change that wires a leg into a job cannot
    also produce that leg's first green run - the run happens after the push. So for one commit the
    only moves were a cell the gate refuses and a `yes` nobody had earned, which is a gate
    satisfiable only by an overstatement. `wired` is that commit's honest state, and it is still not
    evidence. **Nothing expires it:** a cell can sit in `wired` for as long as nobody looks, exactly
    as `can` could before `unrun` existed. What is mechanical is the pair of transitions around it.

    **Three more rules, each added because review found one of the above reachable by spelling.** A
    venue whose own `Where it runs` cell says it runs **nowhere** may not say `yes`, `can` or
    `wired`: the two columns of a row are read together, because a `Reached by` that merely named a
    task was otherwise enough to earn a citation for a venue that runs nowhere - and that is the row
    carrying leg 2. A **built** venue whose `Reached by` names no `just <task>` or `nix run .#<app>`
    in backticks is refused, because dropping that prefix made the cell resolve to nothing and its
    verdict permanent. And *CI invokes it* reads a **command** rather than a substring, because a
    task named inside an `echo` was resolving as an invocation.

    **`Where it runs` is a closed vocabulary too:** `in process`, `a GitHub environment`, or
    `nowhere`, with explanatory text after the token. A synonym such as `not anywhere yet` is
    refused rather than silently changing which verdicts the row permits. This closes a spelling
    bypass, not the truth of the row: an accepted token falsely claiming a run site still passes.

    **What none of it reaches, said next to it.** What is read is an *invocation* in a workflow, a
    local composite action or the shared `nix/` shell - never a green run. A wired job that always
    skips reads the same as one that passes, and a hand-run is invisible to both. So *`unrun` has
    stopped being honest* and *`wired` is not yet earned* are mechanical; *`yes` is earned* is
    review's, with the run named beside it.

    **A citation is ANCHORED in the tree, and the anchor is narrower than *CI invokes it*.** Two
    rules, and the second is conditioned on the run site - which is what makes that closed
    vocabulary do mechanical work rather than only be spell-checked. First: a venue may state
    `yes`, `can` or `wired` only if its `Reached by` names a task that **runs this workspace's
    tests**, because CI invokes lints, builds, docs and release jobs too. Second: a venue that runs
    in **a GitHub environment** - *on demand* - additionally needs a job to invoke that task, since
    a job is the only thing that can demand a run there. Measured before those rules: pointing the
    two-keys row at `` `nix run .#actionlint` `` - a real, CI-invoked, entirely unrelated lint - and
    moving its cell to `wired` passed at exit 0; and setting an on-demand venue's cell to `yes`
    with no run ever performed passed at exit 0, because only `unrun` and `wired` read the
    invocation set at all. Both are refused now.

    **Why the second rule is not universal, measured rather than assumed.** Requiring an invocation
    everywhere refused every `in process` venue, whose claims are answered on every push: this
    workspace's suite runs in CI as a nix **check**, and what the gate resolves is `just <task>` and
    `nix run .#<app>` - so `just test` is genuinely run by CI and is invisible to that reader. A
    gate that reddens correct work gets disabled, and the run-site token already states the
    difference the rule needs.

    **What that costs, rather than left to be discovered:** for an on-demand venue, a green run
    somebody did by hand can no longer be cited as `yes`. That follows from this page's own
    argument - nothing here can see a hand-run - so a `yes` resting on one was resting on recall.
    The honest cell until a job demands the run is `unrun`.

    **And what still gets through, so a reader does not stop looking.** For an `in process` venue
    the anchor is the first rule alone, so what holds a `yes` there is that the suite really is the
    venue - which its own token asserts and nothing mechanical proves. For every venue, what is
    held is that the named task runs tests, never that it runs *this* venue's tests: repointing a
    row at ANOTHER venue's test task still passes, and whether an accepted run-site token is true
    is still prose. So a false leg-2 `yes` costs three edits rather than two - the run site, the
    verdict, and a `Reached by` pointed at some other venue's test task - and the third is the one
    review can see, because it names a task that visibly belongs to a different row.

    **A narrower anchor - binding `Reached by` to a POSITIVE `binary(<stem>)`/`test(<name>)` atom
    naming the venue's own standing test file - was measured against the `justfile` and not
    built, because this tree's cheapest candidate refutes it.** `just bigquery-acceptance`'s own
    `cargo nextest -E` filter is `not binary(exchanged_identity) and not binary(cross_resource)`
    - a NEGATION that selects everything else in the package and never positively names its own
    `acceptance` binary at all. A rule requiring the positive atom would refuse that venue's true
    citation on today's clean tree, which is worse than the limit above: *that a `Reached by` task
    is bound to running the workspace's tests, and not to running the venue the row names*, so a
    `yes` in that column still rests on review, not on the gate.

    **Invocation reading has syntax limits too.** Workflow and action sources contribute their
    `run:` bodies, not ordinary YAML names or descriptions; shared shell scripts are read whole.
    The indentation reader is not a YAML parser: a `run:`-shaped line inside a prose block scalar
    can still be collected. Heredoc content beginning with a task can invent an invocation; an
    apostrophe in unquoted prose can instead hide one. Neither is evidence of shell execution.

    **And *the two columns of a row are read together* buys less than it sounds like, measured.**
    `Where it runs` has no external anchor either - it is prose on this page, editable in the same
    diff as the cell it guards. Overstating the row-grant claim took three edits before that rule
    and took **two** after it: change a `Where it runs` cell from *nowhere yet* to *a GitHub
    environment, on demand*, then change the verdict - `check-venues` exited 0 on the pair with a
    byte-identical summary. The anchor rule above put the third edit back, because the exchange
    venue's task is one no job invokes. The rules raise the cost of the overstatement; they do not
    make it impossible, and only a run named beside a `yes` does that.

## The venues

| Venue | Where it runs | What it costs | Reached by |
| --- | --- | --- | --- |
| **A fake at the port** | in process, every run | nothing | `just test`, `just validate` |
| **A mock issuer in the sandbox** | in process, every run | nothing - no network, no docker, no secret | `just test`, `just validate` |
| **A provisioned Postgres source** | in process, every run - against a real postmaster nix stands up in the same sandbox | nothing - no secret and no docker; `nix/postgres-tier.nix` says so in its own header | `just test`, `just validate` |
| **A provisioned Keycloak realm** | in process, on the paths that touch it - a JVM the tier boots inside the job | nothing - no secret and no docker; `nix/keycloak-tier.nix` says so in its own header | `just keycloak-served-test`, `just e2e-datahub-bigquery` |
| **A real dataset under a shared key** | a GitHub environment, on demand | a service-account key and a billing project | `just bigquery-acceptance`, `just e2e-datahub-bigquery` |
| **A real enterprise identity provider** | nowhere yet | a provider to configure and somebody to configure it | not built |
| **A real token exchange, and two grants** | a GitHub environment, on demand | a hosted run resolved both principals to their own accounts - **the pool is provisioned and the two subject assertions are minted at job time** | `just bigquery-exchanged-identity` |
| **A served binary under a verified human caller** | nowhere yet | a provisioned WIF pool whose IdP issues subjects the declared map names, a project granting them `iam.workloadIdentityUser`, and a hosted run to demand it | not built |

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

| Claim | Fake at the port | Mock issuer | Provisioned Postgres | Provisioned Keycloak | Real dataset, shared key | Real provider | Real exchange | Served caller |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| A refusal is a result and every variant is reachable | **yes** | - | - | - | - | - | - | - |
| A caller cannot state its own identity | **yes** (a type with no `Deserialize`) | - | - | - | - | - | - | - |
| A signature verifies, and a forged one does not | - | **yes** | - | - | - | redundant | - | - |
| `kid` selection, and an unknown key id | - | **yes** | - | - | - | redundant | - | - |
| Algorithm confusion: `alg: none`, a symmetric key in the set, the wrong key family | - | **yes** | - | - | - | redundant | - | - |
| Issuer, audience against this deployment's own resource identifier, expiry, `nbf` | - | **yes** | - | - | - | redundant | - | - |
| An `aud` ARRAY, the form RFC 7519 permits | - | **can** - the builder takes several audiences; the standing test is at the gate | - | - | - | redundant | - | - |
| Token class: an ID token where an access token is required | - | **yes**, that *we refuse one* | - | - | - | redundant - we refuse the token from its own claims, and whether such a token can be OBTAINED is the bold row below | - | - |
| The `iat` ceiling on a gateway assertion | - | **can** - the builder takes `iat` and `exp` separately for exactly this; the standing test is at the gate, over in-crate fixtures | - | - | - | redundant | - | - |
| Key rotation: a removed key stops verifying within the bound | - | **yes**, and it is the only venue where a rotation is scriptable | - | - | - | redundant - painful to script there, and the mock issuer is the only scriptable venue | - | - |
| The refetch rate limit under concurrency | - | **yes**, at the cache - over a source that counts its own calls, never the published file | - | - | - | - | - | - |
| `credential_unavailable` through the request path | - | **yes** | - | - | - | - | - | - |
| Two subjects driving two different credentials to the port | - | **yes** | - | - | - | - | - | - |
| The RFC 8693 request document a broker sends | **yes**, against a fake exchange | - | - | - | - | - | redundant | - |
| The document leg 1 verified is the `subject_token` the **shipped** exchanging broker offers | - | **yes**, over a fake exchange - the two halves were each green against their own fixture | - | - | - | - | redundant | - |
| A subject the shipped exchanging broker holds nothing for reaches no authorization server and no data system | - | **yes** | - | - | - | - | - | - |
| The composition root arms leg 1 over the governed routes, or does not start | - | **yes**, on the spawned binary | - | - | - | redundant | - | - |
| The caller a signature established reaches the answer's own record | - | **yes**, on the spawned binary - the only venue that can see it | - | - | - | redundant | - | - |
| An unverified caller on the agent surface (`/mcp`) is refused with the same `401` every forgery gets | - | **unrun** - the standing test is `sutura_http::inbound::tests::router::the_agent_route_refuses_an_unverified_caller_with_the_same_challenge_every_forgery_gets`, run under `just test` | - | - | - | - | - | - |
| Two verified callers see two different tool lists on the agent surface | - | **unrun** - the standing test is `served.rs::two_verified_callers_over_the_composed_binary_see_two_different_tool_lists`, run under `just test` | - | - | - | - | - | - |
| **A real IdP's own signature and JWKS verify through the composed binary - not #105's third-party-audience question** | - | no - it cannot generate an RSA key, so it is not a real provider for this claim either | - | **yes** | - | - | - | - |
| **Whether a real provider will mint an ID token whose `aud` is a third party's client id** | no | **no - and a mock answers _yes_ by construction, which is worse than no test** | no | no - the tier mints an audience for its OWN client, never a browser-delegated third party's | no | **only here** | no | - |
| Whether a statement we generate is accepted by a real data system | - | - | **yes** | - | **yes** | - | - | - |
| Whether a token exchange endpoint accepts what we send it | - | - | - | - | - | - | **yes** - the hosted run of 2026-09-16 exchanged and STS plus `iamcredentials` accepted it (run https://github.com/telekom/sutura/actions/runs/35076526218); re-confirmed 2026-09-18 (run https://github.com/telekom/sutura/actions/runs/35324133081) once the `sts.googleapis.com`/`iamcredentials.googleapis.com` enablement moved into the Pulumi bootstrap (`test-infra/pulumi/google/__main__.py`) so a fresh `up` now provisions the two APIs it needs | - |
| **Whether a deployment holding ONE workload identity can obtain, per subject, a credential the data system resolves to a DIFFERENT principal** | no | no | no - the adapter is `NoPlaceForASubject`, so there is no per-subject credential to obtain | - | no - one key is one identity | no | **yes** - in the hosted run each principal's exchange resolved to its own account (run https://github.com/telekom/sutura/actions/runs/35076526218) | - |
| **Whether two subjects read two different row sets** | no | no | no - one database role is one identity | - | no - one key is one identity | no | no - trusted to the data system, not re-verified by sutura (telekom/sutura#123) | - |
| **Whether a data system applies the row grant of the principal whose bearer a leg presented, so two principals read two different row sets** | no | no | no - the connection presents a password or a certificate, never a subject's bearer | - | no - one key is one identity, and it is the transport's own | no | no - withdrawn with the two-principal cell, trusted and not re-verified (telekom/sutura#123) | - |
| **Whether the shipped Postgres source executes as the asking subject** | no - the adapter declares `ImpersonationCapability::NoPlaceForASubject` and `deliverable_by` holds a declared posture against it at boot in both composition roots, so a deployment that asked for impersonation there does not start and no venue has anything to prove | no | no - the same boot refusal applies before this venue is ever reached | - | no - a different data system | no | no - a different data system | - |
| Whether a real source's chain is VERIFIED, so anchors that do not name its issuer refuse the connection | - | - | **only here** | - | - | - | - | - |
| Whether a source accepts the client certificate this DEPLOYMENT presents, and refuses a client that presents none | - | - | **only here** | - | - | - | - | - |
| **A served binary executes as a verified human caller through the declared per-source map** | - | - | - | - | - | - | - | - |
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

`crates/sutura-cli/tests/served.rs` spawns the shipped binary over a settings file declaring
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

**PR4 adds two agent-surface rows to the matrix, still `unrun`.** The standing tests are written and
compile - `the_agent_route_refuses_an_unverified_caller_with_the_same_challenge_every_forgery_gets`
(`sutura_http::inbound::tests::router`) proves leg 1 stands in front of `/mcp` exactly as it stands in
front of the versioned surface, and `two_verified_callers_over_the_composed_binary_see_two_different_tool_lists`
(`crates/sutura-cli/tests/served/agent.rs`) proves `establish_asked` derives each request's `Asked` from the
caller leg 1 verified and `AgentSurface::permitted` narrows the tool list per caller on the real
composed binary - but no green run of either has been observed, so `yes` is not earned. `wired` is not
theirs either: it means *CI reaches this and no run has been observed*, and the mock-issuer venue's
`Reached by` cell names `just test`/`just validate`, which no CI job invokes under its own name - so a
venue no job reaches is `unrun`. Both cells are `#[cfg(feature = "agent")]`; a green run under
`just test` (`--all-features`) earns them a `yes`.

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

## A provisioned Postgres source

`nix/postgres-tier.nix` stands up a real PostgreSQL server - a unix socket, a loopback TCP listener,
a generated server certificate and a generated client pair - and every venue that runs this
workspace's suite provisions it, so nothing has to demand a run here. `just test` reaches it through
`nix/with-tier.sh`, and `just validate` runs the cells as `checks.nextest` - its
`checks.postgres-tier` leg proves the tier itself comes up and asserts nothing about the adapter.

Its `Where it runs` cell says `in process, every run` because that is the closed vocabulary's
nearest token and the cells really do run in the suite's own process; what they run **against** is a
separate postmaster, which is the whole reason this row exists and is why the cell says so beside
the token. The vocabulary has no token for a provisioned tier, and inventing one belongs to a change
about the vocabulary rather than to this row.

### What only this venue can answer

1. **That a chain is actually VERIFIED.** `crates/sutura-exec-postgres/src/tls.rs` proves the
   `rustls::ClientConfig` construction refuses what a closed type refuses, and nothing there
   connects - so nothing there shows a handshake failing.
   `a_source_chain_from_the_declared_anchor_is_verified_and_answers` and
   `a_source_chain_from_an_untrusted_issuer_is_refused` are the two directions against a real
   server, and the second is the one that matters.
2. **That a source accepts the certificate this DEPLOYMENT presents, and refuses a client that
   presents none.** `a_mutual_source_presents_the_identity_the_server_demands` and
   `a_mutual_role_refuses_a_client_that_presents_no_certificate`, against the tier's mutual-only
   role. A `verified` entry that named a client certificate used to reach neither cell, because it
   was discarded at parse time and nothing refused it - `github.com/telekom/sutura#659`, now a load
   refusal in `sutura_config::sources::transport`.
3. **That a statement this workspace generates is accepted by a real data system**, over the
   conformance packs in `crates/sutura-exec-postgres/tests/conformance.rs`. The shared-key BigQuery
   venue answers the same claim for a different dialect, which is why that one row reads **yes** in
   two columns.

### What it cannot answer - read this before citing a green run

1. **Anything about a calling subject.** The adapter is `NoPlaceForASubject`: one connection is one
   static database role, established by a password or by the client certificate above. A client
   certificate is an identity - **this deployment's**, not an asker's - so a green mutual-TLS cell is
   evidence for leg 1's channel and for nothing in leg 2.
2. **Two subjects reading two row sets.** That needs two real grants and a subject bound to each,
   and there is one role here. The tier could carry two roles; nothing would bind either to who
   asked.
3. **Whether the anchors an OPERATOR names are the right ones.** The tier generates its own issuer
   and the test declares it, so what is proved is that a declared store decides the outcome - never
   that a deployment's store names the authority it meant.

## A provisioned Keycloak realm

`nix/keycloak-tier.nix` stands up a real Keycloak - a realm, one confidential client and two
subjects, every credential generated at `start` and written nowhere else - and
`just keycloak-served-test` starts it, runs the one cell that needs it, and stops it whatever the
cell does. `just e2e-datahub-bigquery --datahub tier` (the wave-one hosted job) verifies the
SAME realm over HTTP on its own composed deployment - a second, wider `Reached by` for the same
venue, not a second row: its three asks all ride Keycloak-minted tokens.

Its own job in `.github/workflows/ci.yml`, gated on the `identity` category
(`xtask/src/affected.rs`'s `IDENTITY_PATHS`, which now names `crates/sutura-cli/src/serve/`,
`crates/sutura-cli/tests/served` and `nix/keycloak-tier` beside the inbound transport), rather
than `just test`: the JVM boot is a cost
that suite should not pay on every push, the same tradeoff `nix/keycloak-tier.nix`'s own header
states for why this tier is not (yet) in `checks.nextest`'s `preCheck`.

**What only this venue can answer, and it is one claim.** The mock issuer generates no RSA key by
deliberate design (`docs/where-identity-is-proven.md`'s own mock-issuer section says so), so nothing
before this venue ran a real IdP's RS256 signature over its published JWKS through the composed
binary. What runs here is exactly that: the binary has no HTTP client, so the JWKS reaches it only
as the `key_set_file` its settings name - a document the TEST HARNESS fetched over HTTPS, trusting
only the tier's CA, and wrote to scratch - and no discovery document is read by the binary at all.
`crates/sutura-cli/tests/served/keycloak_test.rs`'s
`a_real_keycloak_issued_token_is_verified_by_the_composed_binary_and_a_wrong_audience_is_refused` is
the cell: a password-grant token for one provisioned subject is accepted with the right rows and a
`verified` audit record, a token for the OTHER subject names a different subject in that record, and
the same valid token is refused at a deployment declaring a different audience. So this row says
**yes**, moved from `wired` on 2026-09-15 by the hosted run of PR #751, whose `keycloak-served-test`
job concluded `success` (run
https://github.com/telekom/sutura/actions/runs/34905742355/job/104186453042) on the old
masked-comparison cell; the narrowed wording below was held by PR #755's own hosted run of the
unmasked cell, `keycloak-served-test` `success` (run
https://github.com/telekom/sutura/actions/runs/34928050323/job/104251967900).

**What "a different subject" means, exactly.** The audit record masks every `sub` to its first
character plus `***`, and Keycloak subjects are UUIDs, so two different `sub`s' masked forms
collide whenever their UUIDs share a first hex character (1 in 16). The old masked-comparison cell
therefore held uniqueness on that one hex char alone, and it flaked on the real tier: the hosted
`keycloak-served-test` history failed **5 of 18** runs - far more often than one-in-16 per pair -
and why is UNEXPLAINED. Fixed realm ids, the same record returned twice, and a mis-masked asker
are all ruled out; what could NOT be ruled out is the tier minting one `sub` for two subjects. The
unmasked cell is built to surface exactly that: `sub_a != sub_b` compares the two tokens' OWN `sub`
claims (the harness decodes the tokens it created) and PRINTS BOTH on failure, so the next hosted
failure names the cause instead of one masked character. If the hosted tier ever mints one `sub`
for two users, this cell fails - the correct outcome. The two `assert_eq!` ties and `subject_field`
pass through the same one-hex-char mask (each record's masked `subject` is matched to its OWN
token's `sub` via the same `SubjectId` mask the deployment wrote), so they carry attribution to
that record, not uniqueness - the uniqueness this venue relies on is `sub_a != sub_b`.

### What it cannot answer - read this before citing a green run, and this is the row that matters

1. **Issue #105's own question.** #105 asks whether an enterprise IdP will mint an ID token whose
   `aud` is accepted by a THIRD PARTY (Google STS, via a delegation flow at a human's own sign-in) -
   this tier provisions one client and a password grant, nothing that resembles a browser-based
   delegation flow or a second registered resource-server client. The audience this cell's
   deployment declares is whatever Keycloak's own client scopes minted for `sutura-dev-cli`,
   **read off the token rather than asserted** - that is a real provider's own audience for its own
   client, not a third party's, and #105's row above stays `no` here.
2. **Two subjects reading two row sets.** Same exclusion as the Postgres venue above: this
   deployment reads its fixture files under one shared identity whoever asks.
3. **Anything about leg 2.** No data system's own grant is involved; the claim is entirely about
   leg 1 verifying a real signature.
4. **The RFC 9068 token-class check does not apply here.** This realm's password-grant token
   carries `typ: JWT`, so the fixture's settings declare `token_type: "any"` rather than the
   `at+jwt` default - what verifies is a real signature and a real key set, not that this
   deployment's own class-check also applies to a provider that does not mint RFC 9068's class by
   default. That is fixture-only: the diff touches no `sutura-config` code, so no production
   default moves.

## A real dataset under a shared key

`just bigquery-acceptance`, against a GitHub environment's own dataset. `docs/adr/0017` and
`docs/adr/0019` are the records, and the sentence that matters here is short: **a service-account key is
one identity for everybody who asks**, so what those legs establish is *accepted, and correct for that
identity* - and nothing whatever about per-subject execution. `just e2e-datahub-bigquery --datahub tier`
(the wave-one hosted `e2e-datahub-bigquery` job) reads the SAME `bq-test` environment's dataset for its BigQuery leg -
same venue, no second row - and adds nothing to this claim's cell: it still executes under one shared
credential (`shared-service-user`), so the per-subject half stays exactly where the exchange row below
keeps it.

Not merely written - the run was observed: `the_endpoint_accepts_one_statement_this_repository_generated`
(`crates/sutura-exec-bigquery/tests/acceptance.rs`) passed in the `bigquery-acceptance` job on
2026-08-31, CI run [33382663404](https://github.com/telekom/sutura/actions/runs/33382663404/job/99464748052)
against the `bq-test` environment's real dataset - the push that landed `docs/adr/0017`'s third
amendment, which records the same run (8 tests passed, 5 the smoke leg's and 3 the corpus leg's).

## A real enterprise identity provider

Not built. Its job is the one row above that only it can answer, and keeping it to that row is the point
of this page.

## A real token exchange, and two grants

`just bigquery-exchanged-identity`, against the `bq-test` environment's workload-identity provider.
`crates/sutura-exec-bigquery/tests/exchanged_identity.rs` is the standing test and its header is the
long form of everything below.

### What only this venue can answer, and the word for its state today

**Whether a deployment holding ONE workload identity can obtain, per subject, a credential the data
system resolves to a DIFFERENT principal.** A key on disk is not an asking subject, which is why the
withdrawn two-principal cell (`tests/two_principals.rs`, telekom/sutura#123) could never have reached
this claim even while it stood - and it is the half that separates impersonation from credential
selection. The test
`each_principal_is_who_this_source_says_it_is_executing_as` is written to exchange a subject's own
assertion through the composition `sutura serve` ships and read `SESSION_USER()` back through the
adapter, asserting the account each leg became;
`the_deployments_own_identity_is_neither_principal` is the control, the same read under the
credential the transport itself holds, without which an exchange that did nothing at all would pass.

**The state is `yes`**, held by a run somebody observed. On 2026-09-16 a `workflow_dispatch` of
`.github/workflows/bigquery-exchanged-identity.yml` concluded `success` (run
https://github.com/telekom/sutura/actions/runs/35076526218): the nextest log shows
`each_principal_is_who_this_source_says_it_is_executing_as` PASS, whose assertion is
`TheExpectedPrincipal` for each leg and their distinctness - a passing test prints no verdict, so
`TheExpectedPrincipal` is what the assertion requires - with
`the_deployments_own_identity_is_neither_principal` as the control. The limit beside
the claim: the job that minted the two subject assertions holds both principals' own keys by
construction, so this proves the STS/`iamcredentials` mechanics resolve per subject - never that an
untrusted caller could. Leg 2 is proven here for BigQuery only, through the map this source
declares (below).

### What the green run established, and the hop that got closed to make it green

**The pool is provisioned and the audience it exports is the `SUTURA_BQ_WORKLOAD_AUDIENCE` this cell
reads.** Two subject assertions are minted at job time from the same per-principal keys the
withdrawn two-principal cell used - no new long-lived secret. A plain RFC 8693 exchange yields
exactly one identity per subject token - whoever the token's `sub` is - so two principals need two
subject tokens; `examples/mint_subject_assertion.rs` mints one Google-issued ID token per principal
from `SVC_SUTURUA_BQ_PRINCIPAL_A`/`_B`, writes each to a file, and the cell reads
`SUTURA_BQ_PRINCIPAL_A_ASSERTION_FILE`/`_B_`.

**The hop that used to be the missing third thing is now built: a pool subject becomes a service
account.** `wire::StsOverHttp` posts the RFC 8693 request and returns what comes back, which for a
workload-identity pool is a FEDERATED credential: Google resolves it to a pool subject, not to a
service account. Turning that into a service account is a second call, and
`wire::IamCredentialsOverHttp` now makes it - `iamcredentials.generateAccessToken` for the account
`WorkloadIdentity::target_for` declares (telekom/sutura#774). That is why the hosted run resolved
each principal to its own account rather than a bare pool subject.

**The limit this closes with, not without:** the job that mints these assertions holds both
principals' own keys by construction, so a green run proves the STS/`iamcredentials` mechanics
resolve per subject and nothing about an unprivileged caller - the fifth "would NOT establish" item
below. And the account a source's exchange targets is declared per source
(`WorkloadIdentity::target_for`), so leg 2 is proven here for BigQuery only.

### What a green run here still would NOT establish

1. **Anything about rows.** No row access policy is involved and none is asserted on. Whether the
   data system then filters correctly for that identity is the vendor's guarantee, trusted and not
   re-verified by sutura (telekom/sutura#123's decision, made after the two-principal cell that used
   to carry this claim was withdrawn).
2. **Anything about another source.** `BigQuery`'s adapter is the only one declaring
   `PerSubjectCredential`; the in-process engines execute under one identity.
3. **That a browser-facing caller's token reaches the exchange.** That is the transport's half, and
   the mock-issuer venue's `the_shipped_exchanging_broker_exchanges_the_document_leg_one_verified` is
   where it is answered.
4. **That any deployment answered anybody.** This cell drives the composition directly; no served
   binary is involved, so `AGENTS.md`'s served-half clause is unchanged by any run of it.
5. **That the subject assertions were not minted from the principals' own keys.** They are, by
   construction (telekom/sutura#376): the two-workflow-step mint that produces them holds
   `SVC_SUTURUA_BQ_PRINCIPAL_A`/`_B`, the same keys the withdrawn two-principal cell used, so
   "nothing here holds a principal's key" is a property of this cell's own code path, not of the run
   as a whole. A green run is citable only together with the step that produced its two assertions,
   and proves the STS/`iamcredentials` mechanics resolve per subject - never that an unprivileged
   caller who is not one of the two principals could obtain the same result, because no such caller
   exists in this harness (PR #382's review finding 2a, still open).

### The half that is still nowhere

**This venue's own claim - that a deployment holding ONE workload identity resolves each subject to a
DIFFERENT principal - is now green, on BigQuery.** `sutura_exec_bigquery::WorkloadIdentityBroker`
decides correctly, `StsOverHttp` serializes the documented request, `wire::IamCredentialsOverHttp`
makes the hop, and all of it was measured against a real endpoint under each principal's own
assertion in the hosted run above. The half a green run here still does not touch is the served
one: this cell drives the composition directly and no served binary is involved, so who a served
deployment answers under an asker is still not proven by it. `AGENTS.md` keeps the position
verbatim:

> no served binary has executed as a caller yet.

And the claim that two subjects read two different ROW sets stays withdrawn (telekom/sutura#123): a
data system enforcing row-level security is trusted to do so, and this venue asserts only who the
source became - the account - never the rows it returned.

### The exchanged-credential cache's own stated limit

`docs/adr/0031` adds a per-process cache in front of `WorkloadIdentityBroker`'s exchange, off by
default (`security.credential_cache.enabled`). It changes nothing above - a cached credential is
still presented to the source and still re-checked against the source's own grant on every query -
but it does change how quickly a revocation at the IDENTITY PROVIDER (an account disabled, a
principal removed) reaches this deployment: a cached entry is served for up to
`min(the credential's own remaining life minus the broker's floor, security.credential_cache.window_seconds)`
after the provider would have refused to mint it again. That window is stated here because it is a
number an operator can act on, and because per-request minting would not have done better: an
exchanged token is typically valid for close to an hour regardless of how it was minted, so a fresh
mint carries the same stale grant for the rest of its own life either way. What the cache adds is
strictly bounded by the configured window on top of that, never by more.

## A served binary under a verified human caller

**This venue runs nowhere yet, in the sense that decides what its column may claim: no job invokes
it and no green run has been observed, so its one claim is `-` until a run is demanded.** The cell
this row would earn is what a green `bigquery-exchanged-identity` run cannot touch - a SERVED binary
answering under the declared per-source map: it boots with `security.inbound`, a verified human
caller whose full `sub` is a declared map key is granted through the bigquery source (the hop ran,
not the shared identity), and an undeclared caller is refused at the broker door before anything is
exchanged. The standing cell is
`served/e2e.rs::a_served_binary_executes_a_verified_human_caller_as_the_declared_account`,
`#[ignore]`d - it needs the provisioned WIF pool plus the IdP-to-SA `iam.workloadIdentityUser`
grant, and it will not run locally. **The raw `SESSION_USER() == declared SA` string stays the
exchange venue's `exchanged_identity.rs` cell**: it is unreachable over `/v1/sql/run`, because
`BigQueryWarehouse` keeps `ACCEPTS_RAW_STATEMENTS = false`, so the served surface's observable is
the granted answer's `executed_as: impersonation-at-source`, not the raw account.

## Keeping this page honest

A venue that cannot state its limit is how *verified* drifts. So:

- A new venue arrives as a row in the table above **with its exclusions written**, in the same change.
- A test moving from one venue to another moves its row, rather than gaining a second one.
- `Where it runs` must use the closed vocabulary; the gate holds its meaning, not whether the
  claimed run site is true.
- A `yes`, `can` or `wired` cell needs a `Reached by` task that **runs tests** - a lint CI invokes
  is not a venue anything is proven in - and an **on demand** venue needs a job to invoke it as
  well. That the task runs *this* venue's tests is still review's.
- `just validate` runs every venue that needs no network. The other three do not, and each says so where
  it is invoked.
