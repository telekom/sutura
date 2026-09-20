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
    built, because this tree's cheapest candidate refutes it.** the removed `bigquery-acceptance` leg's own
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

Issue #81 decided **what each venue is allowed to claim**, and `docs/adr/0017`'s fifth amendment
renders it - four rows, quoted verbatim. `Allowed to claim` below is that decision's short form
against the CURRENT venues, `-` where a venue is outside it. It is prose, not a verdict: the gated
per-claim answer is the *Which venue answers which claim* matrix in the next section, and this
column only points a reader of the venues table at it.

| Venue | Where it runs | What it costs | Reached by | Allowed to claim |
| --- | --- | --- | --- | --- |
| **A fake at the port** | in process, every run | nothing | `just test`, `just validate` | "every outcome the port can produce, including each refusal" |
| **A mock issuer in the sandbox** | in process, every run | nothing - no network, no docker, no secret | `just test`, `just validate` | - |
| **A provisioned Postgres source** | in process, every run - against a real postmaster nix stands up in the same sandbox | nothing - no secret and no docker; `nix/postgres-tier.nix` says so in its own header | `just test`, `just validate` | - |
| **A provisioned Keycloak realm** | in process, on the paths that touch it - a JVM the tier boots inside the job | nothing - no secret and no docker; `nix/keycloak-tier.nix` says so in its own header | `just keycloak-served-test` | - |
| **A real enterprise identity provider** | nowhere yet | a provider to configure and somebody to configure it | not built | - |
| **A declared principal at a real dataset** | a GitHub environment - `bq-test`, on demand only | a project with one service account per declared subject and the deployment's own identity granted `roles/iam.serviceAccountTokenCreator` on each, plus somebody to dispatch it | `nix run .#bigquery-declared-principal` | "that a declared subject's question executed as the account the source names for it" |
| **A served binary under a verified human caller** | nowhere yet | everything the row above needs, plus an IdP issuing the subjects the declared map names and a served deployment to ask through | not built | "that a human subject's own identity reaches the source" |

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

| Claim | Fake at the port | Mock issuer | Provisioned Postgres | Provisioned Keycloak | Real provider | A declared principal at a real dataset | Served caller |
| --- | --- | --- | --- | --- | --- | --- | --- |
| A refusal is a result and every variant is reachable | **yes** | - | - | - | - | - | - |
| A caller cannot state its own identity | **yes** (a type with no `Deserialize`) | - | - | - | - | - | - |
| A signature verifies, and a forged one does not | - | **yes** | - | - | redundant | - | - |
| `kid` selection, and an unknown key id | - | **yes** | - | - | redundant | - | - |
| Algorithm confusion: `alg: none`, a symmetric key in the set, the wrong key family | - | **yes** | - | - | redundant | - | - |
| Issuer, audience against this deployment's own resource identifier, expiry, `nbf` | - | **yes** | - | - | redundant | - | - |
| An `aud` ARRAY, the form RFC 7519 permits | - | **can** - the builder takes several audiences; the standing test is at the gate | - | - | redundant | - | - |
| Token class: an ID token where an access token is required | - | **yes**, that *we refuse one* | - | - | redundant - we refuse the token from its own claims, and whether such a token can be OBTAINED is the bold row below | - | - |
| The `iat` ceiling on a gateway assertion | - | **can** - the builder takes `iat` and `exp` separately for exactly this; the standing test is at the gate, over in-crate fixtures | - | - | redundant | - | - |
| Key rotation: a removed key stops verifying within the bound | - | **yes**, and it is the only venue where a rotation is scriptable | - | - | redundant - painful to script there, and the mock issuer is the only scriptable venue | - | - |
| The refetch rate limit under concurrency | - | **yes**, at the cache - over a source that counts its own calls, never the published file | - | - | - | - | - |
| `credential_unavailable` through the request path | - | **yes** | - | - | - | - | - |
| Two subjects driving two different credentials to the port | - | **yes** | - | - | - | - | - |
| The RFC 8693 request document a broker sends | **yes**, against a fake exchange | - | - | - | - | - | - |
| The document leg 1 verified is the `subject_token` the **shipped** exchanging broker offers | - | **yes**, over a fake exchange - the two halves were each green against their own fixture | - | - | - | - | - |
| A subject the shipped exchanging broker holds nothing for reaches no authorization server and no data system | - | **yes** | - | - | - | - | - |
| The composition root arms leg 1 over the governed routes, or does not start | - | **yes**, on the spawned binary | - | - | redundant | - | - |
| The caller a signature established reaches the answer's own record | - | **yes**, on the spawned binary - the only venue that can see it | - | - | redundant | - | - |
| An unverified caller on the agent surface (`/mcp`) is refused with the same `401` every forgery gets | - | **unrun** - the standing test is `sutura_http::inbound::tests::router::the_agent_route_refuses_an_unverified_caller_with_the_same_challenge_every_forgery_gets`, run under `just test` | - | - | - | - | - |
| Two verified callers see two different tool lists on the agent surface | - | **unrun** - the standing test is `served.rs::two_verified_callers_over_the_composed_binary_see_two_different_tool_lists`, run under `just test` | - | - | - | - | - |
| **A real IdP's own signature and JWKS verify through the composed binary - not #105's third-party-audience question** | - | no - it cannot generate an RSA key, so it is not a real provider for this claim either | - | **yes** | - | - | - |
| **Whether a real provider will mint an ID token whose `aud` is a third party's client id** | no | **no - and a mock answers _yes_ by construction, which is worse than no test** | no | no - the tier mints an audience for its OWN client, never a browser-delegated third party's | **only here** | - | - |
| Whether a statement we generate is accepted by a real data system | - | - | **yes** | - | - | - | - |
| Whether a token exchange endpoint accepts what we send it | - | - | - | - | - | - | - |
| **Whether a deployment holding ONE workload identity can obtain, per subject, a credential the data system resolves to a DIFFERENT principal** | no | no | no - the adapter is `NoPlaceForASubject`, so there is no per-subject credential to obtain | - | no | no - nothing per-subject is OBTAINED here: the deployment's own identity becomes a declared account, so there is no credential for a subject to hold | - |
| **Whether two subjects read two different row sets** | no | no | no - one database role is one identity | - | no | no - it reads one identity per question and no rows at all; the row half needs the served surface | - |
| **Whether a data system applies the row grant of the principal whose bearer a leg presented, so two principals read two different row sets** | no | no | no - the connection presents a password or a certificate, never a subject's bearer | - | no | no - no bearer is presented on this path at all | - |
| **Whether the shipped Postgres source executes as the asking subject** | no - the adapter declares `ImpersonationCapability::NoPlaceForASubject` and `deliverable_by` holds a declared posture against it at boot in both composition roots, so a deployment that asked for impersonation there does not start and no venue has anything to prove | no | no - the same boot refusal applies before this venue is ever reached | - | no | no - a different adapter, refused at boot before any venue | - |
| Whether a real source's chain is VERIFIED, so anchors that do not name its issuer refuse the connection | - | - | **only here** | - | - | - | - |
| Whether a source accepts the client certificate this DEPLOYMENT presents, and refuses a client that presents none | - | - | **only here** | - | - | - | - |
| **A served binary executes as a verified human caller through the declared per-source map** | - | - | - | - | - | no - it asks the adapter directly, so nothing here goes through a served binary | - |
| **Whether two distinct subjects resolve to two distinct `BigQuery` principals through the ADBC path** | no - a fake transport records the principal the adapter forwarded and nothing about what a dataset does with it | no | no | no | no | **wired** - the standing cells are `each_subject_executes_as_the_account_this_source_declared_for_it` and its control `the_deployments_own_identity_is_neither_declared_account`, dispatched by `.github/workflows/bigquery-declared-principal.yml`; no run observed | - |
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
last venue's only claim. And they are router tests rather than composed-binary ones because they are
about the EXCHANGING broker, which no composition root can build: its two HTTP hops went away with
the BigQuery `wire` transport. **What the served root attaches instead is
`DeclaredPrincipalBroker`**, which presents the principal a source declared for the asking subject
rather than a credential exchanged for them - so a `bigquery` source declared
`impersonation-at-source` now boots, and these two tests are still about a broker the binary cannot
host.

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
test sleeping for the shipped minute; and the exchange half needs the exchanging broker, which has no
implementor a composition root can reach since the BigQuery `wire` transport was deleted. Those two
are the honest reason `credential_unavailable_is_reachable_end_to_end` and
`two_subjects_drive_two_different_exchanged_credentials` are router tests rather than binary ones.
**The sentence this replaced said the root refuses to boot an `impersonation-at-source` source, and
that stopped being true**: the served root attaches `DeclaredPrincipalBroker` to a declared
`bigquery` source, so such a deployment boots - what it cannot host is a test about an EXCHANGE.

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
cell does. The wave-one hosted job (`e2e-datahub-bigquery`, removed with the wire) verified the
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

## A declared principal at a real dataset

**`wired`, and that is the whole of what this venue is today: a job reaches its two cells and no run
of either has been observed.** `.github/workflows/bigquery-declared-principal.yml` is
`workflow_dispatch`-only against the `bq-test` environment, so a person dispatches it; the two cells
are `each_subject_executes_as_the_account_this_source_declared_for_it` and its control
`the_deployments_own_identity_is_neither_declared_account`, both `#[ignore]`d and both panicking
with the name of any environment value they lack rather than skipping. Until a run is observed and
reported here this venue may not be cited, and the ADBC adoption's own merge precondition is that
somebody dispatches it.

**What only this venue can answer.** Whether two distinct verified subjects resolve to two distinct
`BigQuery` principals at the data system. The oracle is `SELECT SESSION_USER()`, read through
`BigQueryWarehouse::session_user`: it returns the impersonated account's own address rather than a
`principal://` string, so a wrong answer is visible rather than plausible. The control leg is what
makes the pair mean anything - a deployment that answered both questions as itself would satisfy the
first cell alone whenever its own identity happened to be one of the two accounts.

### What it cannot answer - read this before citing a green run

1. **Anything about a caller's own credential.** It is not in the chain: leg 1 verifies the caller
   here and nobody verifies it again, so this venue can show the RESOLUTION is per subject and
   nothing about what would stop a forged subject. `crates/sutura-exec-bigquery/src/lib.rs` states
   the consequence beside the claim and `docs/adr/0018`'s fifth amendment prices the alternative
   that would have kept Google in that chain.
2. **Anything about a SERVED binary.** It opens the adapter directly. `SESSION_USER()` is
   unreachable over `/v1/sql/run` - `BigQueryWarehouse` keeps `ACCEPTS_RAW_STATEMENTS = false` - so
   the served surface's only observable is the granted answer's `executed_as` and its rows, which is
   the next venue's row and is `not built`.
3. **Which ROWS each principal sees.** Both cells read one identity and no data. Two accounts with
   two different dataset grants reading two different row sets is the claim an end-to-end venue
   would add, and nothing here approaches it.
4. **That the driver ships.** It builds one for the runner's own triple. No release artefact carries
   a driver and a static musl binary cannot load one at all -
   `just bigquery-driver-check` is what holds both halves of that, and it is a different venue
   answering a different question.

## A real enterprise identity provider

Not built. Its job is the one row above that only it can answer, and keeping it to that row is the point
of this page.

## A served binary under a verified human caller

**This venue runs nowhere, and there is no standing cell in it - and it is now the SECOND half of
leg 2 rather than the whole of it.** The `SESSION_USER()` half moved to *A declared principal at a
real dataset* above, which is `wired`; what is left here is the half that needs a served binary: a
verified human caller asking through `/v1/*`, and the rows two accounts' own grants give them. No
job invokes it and no green run has been observed. The ignored cell that used to stand here is not
named, because a citation of a test this workspace no longer contains is a citation of nothing and
`cargo xtask check-venues` refuses one.

### What leg 2 now needs, precisely

The bar is the one the withdrawn venue met and not a weaker one: **one run in which two distinct
verified subjects resolve to two distinct BigQuery principals, observed at the data system.** What
changed is only the mechanism under it, so most of the provisioning still applies and one part of it
does not.

**Still applies.** One service account per declared subject, in one project, with the dataset grants
that make the two accounts read different rows. The oracle is unchanged and it is
`SELECT SESSION_USER()`, which `sutura_exec_bigquery::SessionUser` already reads back: it resolves to
the impersonated account's own address rather than a `principal://` string, which is what
`telekom/sutura#376`'s `iamcredentials.generateAccessToken` hop bought and what this transport's
`bigquery.impersonate.target_principal` also goes through. The settings shape is unchanged too - the
declared `workload_identity.impersonate` map, keyed on each subject's full verified `sub`.

**No longer applies.** The Workload Identity Federation pool and OIDC provider
(`test-infra/pulumi/google/__main__.py`, its `issuer_uri` configuration, and the
`roles/iam.workloadIdentityUser` binding from the pool subject to each account). Nothing in this
build exchanges a subject's token against a pool: `sutura serve` attaches
`DeclaredPrincipalBroker`, which reads the verified subject and answers with a NAME, and the driver
impersonates that account from the deployment's own application default credentials. The grant that
replaces the pool binding is **`roles/iam.serviceAccountTokenCreator`, held by the deployment's own
identity on each declared account.** That is a smaller and a *broader* grant at once - smaller
because no federation is provisioned, broader because the deployment can become any declared account
without a caller present, which is the limit `crates/sutura-exec-bigquery/src/lib.rs` states beside
the claim and `docs/adr/0018`'s fifth amendment prices.

**The run, and the first half of it is now built rather than described.**
`.github/workflows/bigquery-declared-principal.yml` is the job: it places one credential - the
deployment's own - builds the driver for the runner's triple, and runs the two cells named in *A
declared principal at a real dataset* above. What a dispatcher has to supply is the environment:
two service accounts, the job's own identity granted `roles/iam.serviceAccountTokenCreator` on both,
`SUTURA_BQ_DATASET`, and the two account addresses. **Nobody has dispatched it**, which is why that
venue says `wired` and not `yes`.

The second half - this venue - is still only described, and it is a served deployment that:

1. boots `sutura serve` with the `bigquery` feature and `security.inbound` armed, pointing
   `SUTURA_BIGQUERY_ADBC_DRIVER` at a driver `.so` for its triple, with one `bigquery` source
   declared `impersonation-at-source` whose `impersonate` map names both subjects;
2. asks one question as each of two verified callers and asserts the two answers differ in the way
   the two accounts' dataset grants make them differ - the ROWS, which is what the adapter-level
   venue cannot see;
3. carries the same control the other venue does, because without it a run in which both callers
   were answered as the deployment passes.

**What that run would still not prove**, and it is the same exclusion the withdrawn venue carried in
a different place: nothing about a caller's own possession of a credential at the data system. The
caller's token is verified by this deployment and by nobody else on the path. A run can show two
subjects resolving to two principals; it cannot show that a forged subject would have been stopped by
anything other than leg 1.

**And the observable over the served surface is narrower than the oracle.** `SELECT SESSION_USER()`
is not reachable over `/v1/sql/run` - `BigQueryWarehouse` keeps `ACCEPTS_RAW_STATEMENTS = false` - so
a served-binary cell reads the granted answer's `executed_as: impersonation-at-source` and the ROWS,
never the account. The account is readable only through `SessionUser`, which is an adapter-level call
with no HTTP route, so the two halves of this claim need two cells: one at the adapter for *which
account* and one at the served binary for *which rows*.

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
- `Allowed to claim` is a summary of the claims matrix, never a second gate on it - `-` is honest for
  a venue outside issue #81's decision, and a summary that outruns its own matrix cells is the drift
  this page exists to catch.
