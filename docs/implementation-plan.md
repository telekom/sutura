# The implementation plan

Operational, and expected to churn. The decisions it executes live in
[the ADRs](adr/0009-the-plan-from-one-source-to-many.md); a step that turns out harder than it looked
does not change them. If a step cannot be done as written, argue with the ADR, not the plan.

**Twenty-six steps, fifteen done.** Every number is counted off the table below rather than
remembered - the fourth attempt at getting them right. A number typed by hand beside the table that
owns it goes stale on the next row, and it has now gone stale three times: "eleven steps" in a
pull-request body against fourteen rows, then "fifteen steps, seven of which" against eighteen rows and
eight startable, then "twenty-two steps, one done" against twenty-four rows and seven done. **If the
table and this sentence ever disagree again, the table is right.** The two commands that settle it are
`grep -c '^| [0-9]'` for the rows and `grep -c '^| [0-9].*\*\*DONE\*\*'` for the finished ones. **The
second command used to search for the strikethrough and it over-counted**, because the needle appeared
in the paragraph explaining the count as well as in the table - so both anchor at the start of a row.

**There used to be a third count here - how many steps could start at once - and it is gone rather
than corrected.** It went stale twice on its own, and the honest reason is that it was never a fact
about this page: what can start is a function of what has landed, what is blocked and what somebody
picked up, and all three now live where they are read rather than in prose. The
[tracker](https://github.com/telekom/sutura/issues/134) carries the pull order and the priority tiers,
and *blocked by* is a relationship on the issues themselves, so GitHub answers it. **A count that
cannot be derived from the thing it describes is a count that will lie again.**

Every step is one branch, one pull request, and green before the next depends on it. `stax` manages the
stack; the `git-ops/stacked-branches` skill has the mechanics.

## The stack

**Steps are named, not numbered, because the order moves and headings should not.** This table is the
order, and it is the **only** place a step gets a number - the ADRs name branches and let this table
say when. The agent-facing surface is first because it is the API we expose: everything else is an
internal that a stable surface can grow behind.

| Order | Branch | Depends on | Can start now |
| --- | --- | --- | --- |
| 1 | ~~`feat/agent-surface`~~ | - | **DONE** - #35 |
| 2 | ~~`docs/inbound-identity`~~ | - | **DONE** - landed as [0014](adr/0014-how-a-caller-proves-who-it-is.md) |
| 3 | ~~`feat/agent-surface-scope`~~ | 1, and leg 1 for a claim to filter on | **DONE** - #49 |
| 4 | ~~`feat/federation-decomposability`~~ | - | **DONE** - #33. Built and NOT wired; see AGENTS.md |
| 5 | ~~`feat/principal-chain`~~ | - | **DONE** - #37. Both tail positions still absent |
| 6 | ~~`feat/query-bounds`~~ | - | **DONE** - #38 |
| 7 | ~~`test/startup-source-refusals`~~ | - | **DONE** - #31 |
| 8 | ~~`feat/source-registry`~~ | - | **DONE** - #48. Two of row 7's tests were replaced rather than kept: a multi-source CATALOG is servable now, and the source-NAME comparison became a declared kind |
| 9 | ~~`feat/leg-plan-types`~~ | - | **DONE** - #47. The shapes and their rendering; built and NOT wired, see AGENTS.md |
| 10 | ~~`feat/two-source-execution`~~ | 8, 9 - both **done** | **DONE** - #89. The splitter, the combiner and `answer_federated`; both SHIPPED adapters still decline a leg, which is #112 |
| 11 | `feat/conformance-packs` | 10 - **done** | **#116**. None of [0012](adr/0012-conformance-packs-for-inputs-and-adapters.md) is built; the golden matrix is still a macro over three axes in one crate's tests |
| 12 | ~~`feat/credential-port`~~ | 2, 5, 8 - all **done** | **DONE**. `Warehouse::execute` takes a `&Presented`, `LegCredentials` hoists one asker and one deadline over N legs, and a subject with no credential at a source is refused rather than answered as the process |
| 13 | `feat/plan-spans-two-identities` | 5, 10, 12 - all **done** | **#113**. A decision before a check: refuse a mixed-posture answer, disclose it, or make it configurable |
| 14 | ~~`feat/compose-tier`~~ | nothing in this repo - docker on the host | **DONE** - #34 |
| 15 | `feat/bigquery-adapter` | 8 - **done**; the fixture decision is **made**, `adr/0017`; the dependency decision is **made**, `adr/0018` | **MOSTLY DONE, and further than the previous text said.** The fourth dialect and its goldens, the adapter, the source declaration, the registry entry, the WIRE (`jobs.query` over `ureq` behind a default-off feature) plus a credential port - and since then **the CORPUS leg, green in CI**: 22 statements accepted, 21 answers agreeing with the engine exactly, 9 refusals agreeing, 6 anchors reproduced, per `adr/0017`'s third and fourth amendments. `sutura-serve` **opens** `kind: bigquery` now, behind a default-off feature. What is left is not the adapter: no published artefact links it - #111 published `sutura-serve` and did NOT close that, because both shipped binaries carry cargo's DEFAULT features and `bigquery` is not one of them, which `checks.shipped-features` now reads off the artefact (#121 is what is left of it) - a cross-dataset read has never been EXECUTED (#118), the acceptance leg races itself (#119), and a bundle naming a table the dataset does not hold still boots (#120) |
| 16 | `feat/bigquery-impersonation` | 12, 15, and the ID-token verification | **#87**, and three of its five parts are built: the adapter declares `PerSubjectCredential`, `WorkloadIdentityBroker` really exchanges, and `sources.<alias>.workload_identity` is declarable. What is left is the composition plus the proof - #147 for the venue, #123 for the two-principal cell |
| 17 | `feat/postgres-adapter` | 8 - **done**, 14, and the artifact question | **HALF DONE, and the halves are worth telling apart.** The adapter is built and held to the corpus, the differential and the anchors against a real `postgresql_18` in the nix tier - as a **dev-dependency**. `SourceKind` has no `postgres`, so no deployment can declare one: that is **#124**, and TLS to it is **#125** (row 19) |
| 18 | `feat/postgres-oauth` | 12, 17 - and the server-side validator decision, which is **made** | **#126**, blocked by #124 and #125. The verification is answered and the answer is unwelcome: no Rust client speaks SASL `OAUTHBEARER`, so the step owes first-party protocol code, and core Postgres ships no validator that reads `aud` |
| 19 | `feat/source-mtls` | 8 - **done**, 17 | **#125**. Nothing this repository connects OUT with verifies a certificate, and `sutura-exec-postgres` is `NoTls` unconditionally |
| 20 | `feat/raw-sql-tool` | 3 - **done**, 8 - **done**, 12 - **done** | **#129**, blocked by #128. Three of [0013](adr/0013-a-raw-sql-tool-off-by-default.md)'s four prerequisites are now spent; the showcase is what moved it up the order |
| 21 | `feat/demo-tasks` | 14, and one example to demo | **#130** step 4, beside the tutorial and the docs cleanup it belongs with |
| 22 | ~~`build/supply-chain`~~ | nothing - orthogonal | **DONE** - [0021](adr/0021-how-a-published-artefact-proves-where-it-came-from.md). A Sigstore bundle per asset, `cosign` on every image reference, CycloneDX and SPDX per leaf image from the auditable binary, SLSA provenance, and REUSE. `ATTRIBUTION.md` is committed and gated by `cargo xtask check-attribution` and `check-attribution-current`, released and signed with every tag, and documented in `docs/verifying-a-release.md` |
| 23 | ~~`ci/prose-change-cost`~~ | nothing - measure first | **DONE**. A prose-only change does not start a run (`paths` with `!` exceptions, since a semantic catalog is a directory of markdown), `classify` gates every expensive step inside the job, and the merge queue is answered. **Per-CATEGORY selection is the widening, and it is #135** - one filter per adapter, with the matrix derived from the registry rather than edited into a workflow |
| 24 | `feat/metrics-endpoint` | 6 - **done** for the memory series, nothing for the rest | **#132**. A served deployment exports nothing, so a refusal and a fault look the same to an operator |
| 25 | ~~`feat/metadata-capabilities`~~ | nothing - [0011](adr/0011-pluggable-by-declaration.md) decided it and `Warehouse::IMPERSONATION` is the shape to copy | **DONE**. `SemanticCatalog::capabilities` is a required associated item with no default, and `MetadataCapabilities::checked_against` runs over every registered catalog in both directions |
| 26 | `feat/datahub-catalog` | 25 - **done**; 11 is **no longer a prerequisite** (#71 split the catalog axis by `CatalogKind`); 14 - **done** - for an instance to read | **#114**, and **#115 beside it**: a bundle of models with no metrics loads, pins and validates - and answers no question, because `Query` carries a `MetricName` and nothing else. Both roots also wire one hard-coded catalog TYPE, so nothing can open a second kind |

**Rows 1 to 11 have their branch sections on this page. Rows 15 and 16 are in
[BigQuery](implementation-plan-bigquery.md), and rows 12 to 14 and 17 to 24 are in
[identity, services and the operational work](implementation-plan-identity-and-services.md)** - three
file names, one document: this table stays the only owner of **these twenty-six** step numbers, and the
split is at the
stack's own phase boundary - nothing up to and including the conformance packs needs a live service or
an identity decision, and everything after it needs one or both.

## Where the backlog lives, and why it is not this page

**This table owns the twenty-six steps that produced the current tree. Everything raised since is an
issue, and no row is added for it.** That is a change of role rather than an oversight, and the reason
is this page's own history: it has recorded a stale count three times, and a step number here beside an
issue number there is two names for one piece of work - which drifts by construction, because only one
of them is where somebody looks when they pick the work up.

So the division is:

| | Owner |
| --- | --- |
| The twenty-six numbered steps, their order, and the arguments for that order | this table, and the two sibling pages |
| Everything raised since, its priority, and what to pull next | [the tracker](https://github.com/telekom/sutura/issues/134) and the board it indexes |
| What blocks what | the issues themselves - *blocked by* is a relationship GitHub answers, and it cannot go stale in prose |
| Which record a piece of work needs, and its number | the tracker's reserved-number table, so two branches cannot both mint `0022` |

**A row that is DONE stays here** - the argument for why a step went where it did is worth more after it
lands than before, and deleting it would leave the next reader unable to tell a decision from an
accident. A row that is still open now names the issue that owns it instead of a *can start now* verdict
this page cannot keep current.

**Six orderings in that table are decisions rather than convenience, and each replaced an earlier
arrangement that would have gone wrong:**

- **The compose tier moves ahead of the first network adapter, not behind it.** An earlier version had
  it depend on Postgres-over-OAuth, which is inverted: compose exists to stand up the source the
  adapter is tested against, so an adapter that lands first has nothing real to run against and its
  tests become a fake asserting our own code back to us.
- **A Postgres adapter with a static credential is its own step, ahead of the OAuth one.** This is
  [track 1](adr/0007-federating-across-different-data-systems.md) - one source, queried directly -
  and 0007 prices it as nearly free: the dialect is compiled, the statement and parameter goldens
  exist, every statement is already parse-checked. The payoff is disproportionate: it answers *which
  shipped artifact links a native driver* - an open question in two ADRs and a cross-build matrix
  change; it puts the rendered SQL in front of a real Postgres for the first time, which the goldens
  cannot do; and it means the SASL OAUTHBEARER verification, if it fails, blocks one step instead of
  the whole network story.
- **BigQuery moves ahead of Postgres, and the argument is value rather than cost.** The bullet above
  is a cost argument and it still holds - Postgres is nearly free. It is not the deciding one:
  **BigQuery is where per-subject execution has to work, and Postgres is where it would be nice if it
  did.** An earlier version of this table had no BigQuery row at all, ordering purely on cost while
  the deployment's priority ordered on value, and the two disagreed silently. BigQuery is genuinely
  the more expensive step - there is no `Dialect::BigQuery`, so it costs a fourth dialect of goldens
  and an AGENTS.md invariant the guidance gate will fail until it is updated - and it goes first
  anyway. **Cheap-first is a tiebreak, not a rule**; when the expensive step is the one that pays for
  the stack, it leads.
- **The agent surface is split, and slice one is deliberately thin.** See below.
- **One step in this stack is a RECORD rather than code, and it is deliberately early.**
  `docs/inbound-identity` answers the question
  [the plan](adr/0009-the-plan-from-one-source-to-many.md)'s Decision 1 leaves open in bold: what the
  inbound token is, who verifies it, what audience it carries, what scopes sutura reads off it, and who
  performs the RFC 8693 exchange. Two branches are blocked on it rather than merely informed by it.
  `feat/credential-port`, because the answer decides that port's signature and 0009 says plainly that
  guessing it produces the wrong one. And `feat/agent-surface-scope`, because filtering advertisement
  by scope is authorization, the bearer gate that ships authenticates the deployment rather than a
  caller, and a filter over an unverified claim is worse than no filter - it looks like a control. A
  question with no owner blocks nothing, which is why this is a row and not a paragraph.
- **The leg plan types are their own branch, ahead of the combine.** An earlier version had one
  federation step that both invented the leg shapes and executed them, and it could not be written
  because the shapes did not exist: `QueryPlan` requires a bucket, a measure and a measure label, and
  a dimension lookup has none of the three. Splitting it follows
  [0007](adr/0007-federating-across-different-data-systems.md)'s own ordering. **The reason given here
  for the split used to be that this branch moves the definition digest, and that was false** - checked
  since: `DefinitionDigest::of` hashes the `Definitions` and the `Knowledge`, so no plan type is under
  it and no committed digest moves. The real reason is better: types plus rendering plus per-dialect
  parse checks are evidence that stands before anything executes them, and one branch landing shapes,
  goldens and a combiner puts three kinds of failure in one review.

## The original thesis, and where each piece stands

Audited against the tree rather than remembered, because a thesis quietly losing a leg is how a
project becomes something else.

| Piece | State | Gap |
| --- | --- | --- |
| Wren's semantic model and compiler | **present** - markdown and YAML catalog, `Query` to `QueryPlan`, and the wren element set adopted | fan-out arithmetic declined on purpose; calculated fields only through an unwired hatch |
| Inspiration from Spice | **compared and declined as a dependency**, correctly - and nothing taken as SHAPE | the connector API, connection pooling and Arrow execution patterns are exactly what `feat/source-registry`, `feat/leg-plan-types`, `feat/two-source-execution` and `feat/postgres-adapter` need |
| Execution from DataFusion | **present**, and it stays the combiner under federation | none |
| Polyglot for rendering, transpilation if needed | **present for rendering**, four dialects compiled | the per-dialect rewrite layer Oracle needs sits behind a feature deliberately not compiled |
| Flexible sources and metadata systems | **decided, not built - and one of the eleven connectors is now measured rather than assumed** | the connectors, and the metadata capability declaration they conform through - which [what DataHub can carry](adr/0016-what-datahub-can-carry.md) schedules, having found the first source that provides part of a model rather than all of it |
| Security | **the strongest part of the record** | the credential port is designed and unbuilt |
| The agent-facing surface | **MISSING FROM THIS PLAN** | there is no `sutura-mcp`, and until now no step for it |

### What wren gives, and what was declined

The model, the element set and the compile are here. What is NOT here is wren's fan-out arithmetic: a
dimension reached through a relationship whose declared cardinality may duplicate rows is **refused**
rather than computed safely. That is deliberate - a refusal beats a wrong number, and the cardinality
declaration is a trusted precondition nothing checks against the data. The consequence, plainly:
**a question wren would answer, sutura declines.** Do not later file that as a bug. The closed measure
vocabulary is narrower than wren's calculated fields for the same reason, and the authored-SQL hatch is
the escape valve that exists and is not wired.

### Spice is compared, never mined

`docs/architecture.md` and ADR 0003 weigh Spice and decline adopting it as a dependency, which is the
right call and is recorded. What never happened is the other half - taking the SHAPES. Three of them
map directly onto steps in this plan, and reading them before designing costs less than after:

- **The connector API and the connection pool**, for `feat/source-registry` and
  `feat/postgres-adapter`. **Branch names rather than step numbers, deliberately:** an earlier version
  of this section cited "steps 5 and 8" and "steps 5 to 9" from a table that has since been replaced
  twice, so the numbers pointed at different work than they were written for. The stack table is the
  only owner of a step number, and prose outside it names branches. This item matters more than it
  sounds: a DuckDB connection is `Send` and not `Sync` while the service requires `Sync`, and
  per-subject credentials make pooling a correctness question rather than a throughput one. Somebody
  has solved this shape already.
- **Arrow execution patterns**, for the owed Arrow record.
- **Its optimizer rules**, for `feat/two-source-execution`'s combine and the pushdown decision.

What to keep declining: the acceleration and cache layer, the inference surface, and the runtime as a
runtime. Code and shapes, never a second control loop.

### Transpilation, if needed

**The position is not "never compile the feature". It is "never translate a third party's SQL."** One
feature flag conflates those two, and today's Oracle finding is the case that exposes it: the
per-dialect REWRITE layer - the thing that turns our own generated `DATE_TRUNC` into Oracle's `TRUNC` -
lives behind `transpile`, alongside the parser we decline. So Oracle and BigQuery correctness needs
either that layer or the same rewrites written here.

If the feature is compiled to reach it, three things must hold and one must change:

- The parser stays uncalled. Our generator's output is what is rewritten, not somebody's SQL.
- The goldens and the per-dialect parse-check still guard every rendered statement, which is what makes
  the rewrite auditable rather than trusted.
- The two documented hazards get handled explicitly rather than inherited: the default level returns
  `Ok` and **discards the diagnostic**, and the raising level errors on every non-count aggregate
  targeting ClickHouse while staying silent on the four real breakages. Neither default is acceptable
  as-is.
- The invariant row changes, because its stated mechanism is "the feature is not compiled". A row whose
  mechanism moves gets rewritten, not reinterpreted.

## Operating inside a platform we do not own

This section is written **generically on purpose.** This repository is public, and the systems these
constraints come from are not ours to name. Each is written as the capability and its constraint, which
is the rule the root-of-trust file states and which usually improves the point anyway: a requirement
that survives being stripped of the product name is a requirement about architecture rather than about
a vendor.

Sutura will eventually run beside three shapes of thing: **a central API gateway fronted by an
enterprise identity front door**, **an agent platform that supplies a client and a review-and-deploy
plane**, and **third-party chat clients acting as callers**. None of them is in the data path. All of
them constrain the surface, and most of the constraints land on the step that is now first.

### What the agent surface must survive

- **Only TOOLS may be load-bearing.** A gateway of this kind surfaces tools and **ignores resources and
  prompts**. So a glossary, a catalog description or a "how to ask" hint may improve a cooperative
  client and must never be the thing that makes an answer correct. That matches what the prompt already
  is - advisory - but it becomes enforced by the transport rather than by our discipline, a stronger
  position worth designing for deliberately.
- **Two authentication adapters behind one port.** Direct callers need the full resource-server dance:
  metadata discovery, a challenge, an audience-bound token we validate ourselves. Behind a gateway, all
  of that is dead code on that route while remaining mandatory on the other. Implement both behind one
  subject port **so the tool surface cannot tell the difference** - the most concrete justification the
  hexagon has been given, and a real cost rather than a free abstraction.
- **A forwarded user token is credential forwarding, not identity proof.** It is acceptable only if it
  is a signed token from a discoverable issuer that sutura verifies itself: issuer, audience,
  signature, expiry. It is unacceptable if the front door re-mints an opaque string only it can
  interpret. **This single question decides whether such a route can ever serve per-subject data** - a
  question to ask rather than to assume.
- **Never accept a plain user-id header as identity.** Behind a service token, anything holding that
  token could obtain a credential for ANY user - a confused deputy, and with a per-user credential
  store it turns our own API into a mass-exfiltration surface. Audit entries naming individual people
  would then be fiction, worse than an honest shared-identity record because it will be trusted.
- **No auth-bearing tool parameter.** A credential in a tool argument is a credential in the model's
  context, and it makes the model a credential carrier. Authentication is transport-level, and the tool
  surface exposes no field for it - which the typed `Query` already guarantees by having no such field.
- **A timeout may change the surface's SHAPE, not just a setting.** A gateway of this kind enforces a
  request timeout in the tens of seconds, and streaming does not exempt a connection from it. If a
  governed turn can exceed it, the surface has to become submit-and-poll - a design change, decided
  before the tools are written rather than after. Measure a real turn first; without push notifications
  the obvious asynchronous pattern is foreclosed. **And this is where two numbers in two records
  disagreed by an order of magnitude** - worth settling before anybody measures: 0009's provisional
  query deadline is three minutes, and a front door that cuts at tens of seconds means a turn allowed
  180 seconds cannot complete on that route. 0009 now decides which yields - **the deadline does.** It
  is bounded by the front door on any route that has one, rather than a default that quietly outlives
  the connection it is supposed to bound. The measurement decides the second half: if a governed turn
  fits inside the front door's limit, the deadline on that route is simply the smaller number and
  nothing else changes; if it does not, the surface on that route is submit-and-poll from the first
  tool, decided before the tool set exists rather than after somebody has built six tools around a
  request shape.

### Two rules that outrank any of the above

- **Do not weaken an invariant to fit a platform.** If the only identity a route can offer is one
  sutura cannot verify, the honest outcome is that the route serves non-per-subject data only. That is a
  smaller product on that path, not a softer invariant.
- **Defence in depth is never a substitute.** A gateway token proving a request transited the gateway
  makes "assume nothing about the client" cheaper on that route, and gateway-level quota and audit
  complement ours. Neither replaces the check we make ourselves, because the platform is not ours to
  rely on.

## The agent-facing surface

**Goal.** The governed surface agents actually speak. `AGENTS.md` lists `sutura-mcp` among the planned
crates and nothing else in this plan mentioned it, which for a runtime whose thesis is *for AI agents*
was the largest omission in the record.

**Depends on nothing that is not already shipped.** The tool surface exists: a typed `Query` with no
field for SQL, a table, a predicate or row ids; refusals as results rather than errors; provenance in
the payload. MCP is a transport over that, not a redesign of it.

**Split in two, and slice one is deliberately thin.** An earlier version of this step bundled the
server, one schema source for two transports, scope-filtered advertisement and the tool set together.
That is four mechanisms in the first branch of the stack - the branch every other branch rebases on -
and the one thing a first slice must not do is stay open. So:

### Slice one: `feat/agent-surface`

**Adds.** One crate, one transport, **one tool**: ask a certified question. Its schema is GENERATED
rather than hand-written, the property that has to be true from the first line because retrofitting it
means reconciling two shapes that have already drifted.

**Generated from what, decided here rather than left to the branch.** An earlier version of this
section said "derived from the domain `Query`", one word away from an architecture decision that does
not survive being looked at: the derive macro the agent transport needs is `schemars`, it appears
**nowhere in `Cargo.toml` or `Cargo.lock` today** - verified, not assumed - and putting it on a domain
type adds a macro crate and its whole tree to `ALLOWED_IN_DOMAIN`, which `AGENTS.md` calls an
architecture decision rather than a convenience, and which `cargo xtask check-boundaries` walks the
transitive tree to enforce. So the derive goes on a **wire type in the agent-surface crate**, with
`TryFrom<..> for Query` as the only way in, exactly the shape the HTTP transport already ships:
`crates/sutura-http/src/wire.rs` holds `QuestionBody` and its `TryFrom<QuestionBody> for Query`, and
the OpenAPI document is generated off that wire type rather than off the domain. Two consequences,
stated because one of them is a cost:

- **The equality is a test, not a type.** Nothing in the compiler makes a wire type stay equal to
  `Query`, so the guard is `a_query_field_the_domain_does_not_declare_is_a_named_parse_error` below
  plus slice two's `both_transports_describe_the_same_tools`. That is weaker than a single source and
  it is the trade the *no serde on a domain type for a transport's convenience* principle asks for.
- **An adapter does not reach into another adapter**, so the agent-surface crate gets its own wire type
  rather than importing `sutura-http`'s. Two wire types kept equal by a test is the cost of that rule,
  and slice two is where the test that catches drift lives.

`AGENTS.md`'s *Canonical Sources* table used to name `schemars` derives on the DOMAIN types as the
planned owner of both the MCP schemas and the OpenAPI spec, which this decision contradicted and which
would also have put a macro crate inside the domain's allowlist. **That row was rewritten to the wire
type earlier in this same change**, so the two now agree and nothing is owed here.

**And one thing this step cannot inherit, because it does not exist.** `AGENTS.md`'s *changing the query
path or the tool surface* table says a new or widened tool input is caught because "the dumped tool
schemas change and the byte-compare fails until they are re-dumped", and that a new failure mode meets
"the schema drift check". **There is no such dump and no such check** - searched rather than assumed:
`docs/generated/` does not exist, no `xtask` subcommand or `just` task dumps a schema, and `schemars` is
in neither manifest nor lockfile. So the drift guard this step is described as inheriting is a guard it
has to BUILD, and it is the one mechanism in slice one load-bearing for the governance boundary rather
than for the transport: without it, "no field carries SQL, a table, a predicate or row ids" is enforced
by `Query`'s own `deny_unknown_fields` and by review, and not by a diff a reviewer cannot miss.

**The tool result carries ROWS, inline. Decided, and a finding was withdrawn to get here.** An audit of
an older private record set surfaced a rule reading *"rows must not enter the model's context window -
the tool result is provenance plus a handle, and the client fetches the data"*, raised here as a gap
this branch should close before slice one fixed the result shape.

**Withdrawn, because it was a misreading of what that rule protects.** The substance is that in
federation mode **the join is done by the engine and its SQL, not by the model** - a language model must
never be the thing combining two sources' rows. That is already decided, in
[federating across different data systems](adr/0007-federating-across-different-data-systems.md): each leg
is a whole plan rendered by `sutura-sql`, and the combine happens in DataFusion above the port. So the
property was never about the context window, and it is already held by the architecture rather than owed
by this step.

Read as a context-window rule it would have bought a handle-based result shape that costs real
capability: an agent asked for a number could not answer without a second fetch, and a plain client
would show its user nothing. **So: one result shape, rows inline, and no handle.**

**What that leaves open, stated rather than assumed away:** the answer row cap is `max_rows + 1` with a
default in the thousands, far above what any model's context tolerates. Whether the agent surface takes
its own lower cap - and what number - is a real question and **nobody has measured it**, so it is not
answered here. The hard bound stays where it is; an advisory cap for this surface is a decision for
whoever builds slice one, with a measurement rather than a guess.

**Tests.**
- `a_certified_question_is_answered_over_the_agent_surface`.
- `an_uncertified_question_is_refused_as_a_RESULT_rather_than_an_error`.
- `a_query_field_the_domain_does_not_declare_is_a_named_parse_error` - the `deny_unknown_fields`
  guarantee, asserted through the new transport rather than assumed to survive it.

**Done when** an agent client can ask a certified question and be REFUSED an uncertified one over the
agent surface, with no client of ours in the loop.

### Slice two: `feat/agent-surface-scope`

**Adds.** The rest of the tool set, and the two properties that need more than one tool to be
meaningful: **one schema source for both transports**, and **advertisement filtered by scope** so a tool
a caller may not invoke is invisible rather than rejected on call. The second is the mechanism
[a raw SQL tool, off by default](adr/0013-a-raw-sql-tool-off-by-default.md) depends on, which is why it
is here and not deferred further.

**Tests.**
- `the_advertised_tools_differ_by_scope`.
- `both_transports_describe_the_same_tools` - one source, or a test asserting the two descriptions are
  equal. This is the drift this slice exists to prevent, and it is untestable with one tool.

**Done when** the two transports cannot disagree, and a caller without a scope cannot see the tool it
lacks.

**Built, and three things about it are worth reading before the next step depends on them.**

- **The one source is `sutura_app::Capability`**, in the crate that declares the driving port, because
  `Surface`'s two operations *are* the tool set and the two transports cannot see each other. So
  `both_transports_describe_the_same_tools` is not a comparison between the transports - it cannot be -
  but a test in each of them against that source. `sutura_app::Permitted` is the derivation from a
  claim to what a caller may do, so the comparison lives once rather than once per transport.
- **A scope names a capability and never a metric**, which answers half of
  [0014](adr/0014-how-a-caller-proves-who-it-is.md)'s last open question. A scope naming a metric would
  put the authorization server's vocabulary under the catalog's version.
- **The filtering is presentation; the refusal at invocation is the control** - and the two halves are
  built separately on purpose, because a caller can skip `tools/list` entirely. On HTTP the refusal is
  a layer over the versioned subtree plus `RouterNotBuilt::RouteNotGoverned`, so a route added without
  a capability does not assemble. **And nothing narrows the agent surface today**: it speaks over
  standard input and output, where there is no header a token could arrive in. The narrowing is a
  required constructor argument there, exercised by tests and by no request path.
- **`Scopes` did not move out of `sutura-http`**, deliberately: 0014's closing section reserves *which
  crate the validator moves to* for whoever makes the agent surface reachable, and moving the parse now
  would take that decision early. `sutura_app::Permitted::granted_by` takes the values instead.

Note the demo step depends on slice one **or** on the OpenAPI route; slice one is the better one, and
the demo must not wait for either slice.

## The inbound identity, and who performs the exchange

**Goal.** A record, not code. **The only step in this stack whose deliverable is an ADR**, and it is
here because [the plan](adr/0009-the-plan-from-one-source-to-many.md)'s Decision 1 states in bold that
guessing this produces a credential port with the wrong signature, and a port signature is the most
expensive thing in this stack to change afterwards - every adapter and both composition roots implement
it.

**The question, precisely, because "identity" is too big to be a task.** Five things, and the first two
are the load-bearing ones:

- **What audience does the token that reaches sutura carry?** 0009 said audience-bound to sutura;
  [a credential per leg](adr/0008-a-credential-per-leg-for-the-calling-subject.md) describes a route
  where the caller's own token is the subject token posted to the exchange and must carry the identity
  provider's audience. Both cannot be true, and which one is true decides whether sutura is a resource
  server that validates a token for itself or a relay for a token minted for somebody else.
- **Who performs the RFC 8693 exchange?** If the inbound token is audience-bound to sutura, something
  has to exchange it at our own authorization server before it can reach a source's exchange - a step
  neither record describes. If it is not, then "the client's token is never forwarded upstream" is
  wrong, and that sentence was in 0009 until this round.
- **What scopes does sutura read off it, and does it verify them itself?** `feat/agent-surface-scope`
  filters advertisement by scope, which is authorization; the bearer gate that ships authenticates the
  deployment rather than a caller, so there is nothing verified to filter on until this is answered.
- **What happens on a route where the front door re-mints an opaque string only it can interpret?** The
  *operating inside a platform we do not own* section above already states the rule - that route serves
  non-per-subject data only - and this record is where it stops being a rule and becomes a per-route
  answer.
- **What the principal chain looks like coming in**, since `feat/principal-chain` builds human then
  agent then task and a token exchange is the shape it is supposed to map onto rather than be
  translated into. If the inbound token cannot express an agent acting for a human, the chain's tail is
  populated by something else, and that something has to be named.

**Touches.** `docs/adr/` and the nav entry. No crate.

**Done when** each of the five has an answer in a record, `feat/credential-port` can be written without
guessing, and 0009's Decision 1 no longer carries an open paragraph. **The honest cost of NOT doing it
first:** two branches in this stack proceed on an assumption, and one of them is a port every adapter
implements.

## Decomposability in the domain

**Goal.** A federated query cannot compute an aggregate that does not survive being computed per leg
and re-aggregated. No plumbing, no adapters, no federation: just the classification and the rule.

**Touches.** `crates/sutura-domain/src/measure.rs`, and the plan type in
`crates/sutura-domain/src/plan.rs` only if the classification needs to be visible there.

**Adds.** A total function from each aggregate to how it federates, as an exhaustive match: pushable as
written (`Sum`, `Count`, `Min`, `Max`), pushable decomposed (`Avg` as a sum and a count), not pushable
(`CountDistinct`). Plus the ratio rule: numerator and denominator are separate pushed aggregates and
the division happens once, above.

**Tests.**
- `every_aggregate_states_how_it_federates` - the exhaustive match compiles and covers the vocabulary.
- `a_ratio_is_decomposed_rather_than_divided_per_leg` - the shape that would divide per leg is not
  constructible.
- `an_average_travels_as_a_sum_and_a_count`.
- `a_distinct_count_is_not_pushable`.

**Done when** a new aggregate cannot compile without stating how it federates, and dividing a ratio per
leg is unrepresentable rather than discouraged. Six of the eleven shipped metrics are affected by this
step, so the fixtures exercise it immediately.

## The principal chain

**Goal.** Human, then agent, then task - ordered - present from the first record, while both tail
positions are always absent. This is the step that cannot be deferred: a record naming only the subject
can never later be told apart from one that meant "an agent acting for" them, and by the time anybody
wants to tell them apart the records are already written.

**It ships the sink it writes into, and that is a change from an earlier version of this step**, which
justified itself by "a stored row" while
[the plan](adr/0009-the-plan-from-one-source-to-many.md) said sutura keeps no audit archive. Two
records disagreeing about whether the thing this step exists for exists at all is not a step anybody
can implement, so 0009 now decides it in one place and this step carries the decision: **sutura writes
one record per call, refusals included, carrying the whole chain, before the outcome returns - and
retains nothing.** The deployment attaches the sink and owns everything after the write. A record
written after the response is the record a crash loses, and the call worth having a record of is the
one that went wrong, which is why the ordering is part of the requirement rather than an optimisation.

**And the port arrives with a real implementor, not a fake.** `AGENTS.md` says a port trait arrives
with its first implementor; the implementor here is a structured writer over the tracing subscriber
`sutura-runtime` already composes, which needs nothing from anybody and is what a deployment that
attaches nothing else gets. Today the only thing that records a call at all is one `tracing::info!` per
outcome in `crates/sutura-http/src/routes/v1/query.rs`, and its own doc comment says there is no audit
sink and nothing records a principal chain - so this step is not adding a second channel beside a
working one, it is turning a log line into the thing two other records already depend on.

**Touches.** `crates/sutura-domain` for the types and the sink port, `crates/sutura-app` for the
request context that carries a chain and for calling the sink on both outcomes, `crates/sutura-runtime`
for the writer, `crates/sutura-http` to pass the chain in and to replace the outcome log line with the
record.

**Adds.** A principal that is a subject plus an ordered list of actors, and a task identifier that
exists from day one. Ordered innermost-last, which is the shape a token exchange maps onto rather than
being translated into. A sink port taking a record and returning nothing a caller can branch on, and
one writer behind it. What the record carries is fixed by
[a credential per leg](adr/0008-a-credential-per-leg-for-the-calling-subject.md): the principal chain,
the outcome - answer or refusal, with the refusal's variant - the sources the plan read and the posture
each leg ran under, and the expiry the credentials carried.

**The limit, stated where the claim is.** An emitted record is worth what the sink behind it is worth,
and sutura cannot vouch for a sink it does not retain. A deployment that attaches a sink which drops
records, or attaches none, has no audit trail on this side and nothing here can tell it so - which is
why the sources' own logs, written under the asking subject, carry the part of the obligation that
matters. **BUILT**, and by `feat/principal-chain` rather than by a later step: `sutura_domain::audit::AuditSink`
is the port and `sutura_runtime::TracingAuditSink` is its first implementor, because `AGENTS.md` requires
a port to arrive with one. `LocalService::start` now REQUIRES a sink, so a deployment that forgot to
attach one is not a state that exists - a stronger guarantee than the sentence this replaced assumed was
unavailable.

**Tests.**
- `a_principal_with_no_actor_is_a_bare_subject_and_says_so`.
- `an_actor_chain_keeps_its_order`.
- `a_record_naming_a_subject_is_distinguishable_from_one_naming_an_agent_acting_for_them` - the whole
  reason for the step, asserted rather than described.
- `a_refused_question_is_recorded_with_its_chain` - refusals are the half a log line gets wrong by
  omission, and they are the demand signal
  [a raw SQL tool, off by default](adr/0013-a-raw-sql-tool-off-by-default.md) reads.
- `the_record_is_written_before_the_outcome_is_returned` - asserted through a sink fake that records
  ordering, because "before" is the requirement and nothing else checks it.
- `a_chain_reaches_the_sink_through_no_field_a_caller_supplies` - the chain comes from the transport's
  verified identity, and a tool argument that could carry one is the confused deputy this plan refuses
  further up.

**Done when** every outcome, answer and refusal alike, reaches the sink carrying the chain, and the
chain is what a budget would be keyed on **if a budget existed** - it does not. There is no budget
port, `feat/principal-chain` builds the key and stops there, and 0009's *what is not decided* says the
same rather than this step implying a mechanism nobody has written.

## The two bounds

**Goal.** A working-set ceiling and a query deadline. Each a typed refusal, never a truncation. **A
per-leg row bound is deliberately NOT here** - it is retired by
[the plan](adr/0009-the-plan-from-one-source-to-many.md), because a count of rows per leg protects
nothing scarce: a leg grouped by a join key returns key cardinality, so 50,000 narrow rows is a few
megabytes and its answer is twelve rows. Bytes are the bound; the answer keeps its own row cap.

**Touches.** `crates/sutura-config/src/limits.rs` for the settings,
`crates/sutura-domain/src/query.rs` for the refusal variants.

**Adds, and the engine half is the substance of this step:** there is no memory pool today - no
`RuntimeEnv` is constructed anywhere, so DataFusion installs its unbounded one, which under
`panic = "abort"` makes a large enough join process death for every concurrent caller rather than an
error for the one who asked. So this step builds: a byte newtype for the ceiling; a `RuntimeEnvBuilder`
at both `SessionContext` construction sites, without disturbing `with_target_partitions`; the
`Arc<dyn MemoryPool>` retained on the warehouse behind an accessor, since its fields are private and its
hand-written `Debug` exposes only the source; **fail-immediately rather than spill**, per 0009 Decision
3; and a new `RefusalReason`, because exhaustion currently leaves as `503 unavailable` and is therefore
indistinguishable from a dead data system - a caller told to retry against a bound that will fire again.
`ResourcesExhausted` appears nowhere in the workspace today. `sutura_http::wire::refusal`'s
wildcard-free match will refuse to compile until the new variant is given a status, a code and a
sentence, which is also what makes it safe as a metric label.

**Adds.** Two newtypes with **provisional** defaults - **1 GB working set, three-minute deadline**.
Provisional is the operative word: nobody has measured them, so this step measures them on the corpus
and the numbers in the record are a starting point rather than a finding. Two refusal variants, each
provokable. The ceiling is checked against the memory the process actually has at boot and refuses to
start above it, because `panic = "abort"` makes an over-configured ceiling process death by default.

**Which value is global and which is per source is DECIDED, and it is not the same answer for both** -
0009's Decision 3 settles it, because "global with per-source overrides" has an obvious hole the moment
one question reaches two sources whose overrides disagree:

- **The working-set ceiling is query-wide and takes no per-source override.** There is one combiner and
  one working set, so a per-source ceiling would be a number with nothing to bound. A source
  declaration that tries to set one is **refused at parse**, not ignored, because a setting that
  silently does nothing is worse than a missing one.
- **The deadline takes per-source overrides, and a multi-source query is governed by the MINIMUM over
  the sources its plan touches**, itself bounded by the query-wide default. An override can therefore
  only make a query stricter, never more patient, and the refusal names the source whose value
  governed - otherwise an operator tuning one number cannot tell which number bit.
- **The deadline bounds the SUM of the legs, not the longest one**, because
  [a credential per leg](adr/0008-a-credential-per-leg-for-the-calling-subject.md) part 4 decides the
  legs run sequentially. That is why it is written here rather than left to the combiner: the bound and
  the execution order are one decision, and parallel legs would turn this sum into a maximum and move
  the bound.

**Two things this step cannot assert, named here so nobody writes the test and believes it.**

- **It cannot refuse an oversized LEG.** The working-set ceiling is the engine's memory pool, and the
  pool counts operator reservations - hash-join build side, aggregate state, sort - and nothing else.
  Not the `RowSet` a driver hands back, not a leg's buffers before conversion. So a leg large enough to
  kill the process kills it before the combiner reserves anything, and the bound that reaches THAT is a
  byte budget applied as rows are converted, which lands with the `RowSet`-to-Arrow boundary in
  `feat/two-source-execution`. This step's acceptance test is therefore an operator reservation over
  the ceiling and nothing wider. An earlier version of this step promised "an oversized intermediate is
  refused rather than aborting", a promise the pool cannot keep.
- **It cannot cancel.** `Warehouse::execute` is synchronous and blocking, so a deadline that fires here
  leaves the leg running inside the driver. 0009 decides the fix - the deadline travels on the port -
  and the port changes in `feat/credential-port`, where a test may first assert that an execution
  stopped. Until then this bound is honestly **"stop waiting"**, and the test is named for that.

**Tests.**
- `an_operator_reservation_over_the_ceiling_is_refused_rather_than_aborting_the_process` - the
  reservation is the thing this bound counts, and the name says so, so nobody reads it as covering a
  driver buffer.
- `a_result_over_the_answer_row_cap_is_refused_rather_than_truncated` - the existing cap, unchanged, and
  asserted here so retiring the per-leg bound cannot be mistaken for retiring this one.
- `a_query_over_its_deadline_is_refused_and_the_execution_may_still_be_running` - renamed from
  `..._and_the_execution_is_cancelled`, which asserted a timeout and read as proof of an interrupt.
  Cancellation gets its test in `feat/credential-port`.
- `a_per_source_deadline_override_wins_over_the_global_default`, and
  `a_two_source_query_is_governed_by_the_smaller_of_the_two_deadlines`.
- `a_per_source_working_set_ceiling_is_refused_at_parse` - the setting with nothing to bound, refused
  rather than accepted and dropped.
- `a_ceiling_of_zero_is_refused_at_parse`, and `a_ceiling_above_the_available_memory_refuses_at_boot` -
  the newtypes and the boot check do the work.
- One test per refusal variant, because a variant no test can provoke is one the enum refuses to
  carry.

**Done when** each bound produces its own refusal, the working-set bound is shown BITING on an operator
reservation rather than described, the multi-source rule is asserted rather than left to the
implementation, the two defaults are backed by a measurement on the corpus, and a partial answer is
impossible. `panic = "abort"` is why the working-set one matters at all: an allocation failure is
process death for every caller, not an error for one.

## The startup refusals that already hold

**Goal.** Test what is already true. **DONE**, and the count in this paragraph was wrong: it said the
more-than-one-source arm had no test in either binary, which undercounted. `test/startup-source-refusals`
found **four** untested arms - more than one source, an empty catalog, a source this build has no adapter
for, and a model with no data file behind it - and **seven** missing tests across the two binaries, since
`sutura-serve` had none at all. Two arms were already covered and got nothing added; two remain untested
and are named there rather than papered over, one being unreachable (a `const` that parses).

**Touches.** `crates/sutura-serve/src/main.rs` and `crates/sutura-cli/src/commands.rs` test modules
only. No production code.

**Tests.** One per binary, each asserting the multi-source arm and asserting it is NOT the neighbouring
wrong-name arm, so it cannot pass on the wrong branch.

**Done when** both are red against a build with the branch removed. This is the cheapest step in the
plan and it closes a real gap.

## The source registry, the mode, and the boot check

**Goal.** More than one source becomes configurable, each declaring its mode and its capabilities.

**Touches.** `crates/sutura-config` (a keyed source structure beside `catalog.data_dir`),
`crates/sutura-app/src/surface.rs` (a service over many warehouses rather than one),
`crates/sutura-app/tests/adapters/mod.rs` (the registry the matrix reads).

**Adds.** A source declaration carrying an alias, a posture (`SharedServiceUser` or
`ImpersonationAtSource`), declared capabilities, the operator's acknowledgement key and its stated
reason where the posture is shared, and the **verification identity the anchor path runs under** for
that source. Provenance gains the posture per leg, taken from the value the adapter actually received
rather than from the settings tree - a field derived from configuration would report what was
configured rather than what ran.

**The boot check is TWO checks in two places, and which half goes where follows what each half can
see** - [a credential per leg](adr/0008-a-credential-per-leg-for-the-calling-subject.md) part 5 decides
it, and an earlier version of this step had one check in the wrong place:

- **The acknowledgement is a `NotFitToServe` variant in `sutura-config`.** `Settings::refusals` already
  exists in `crates/sutura-config/src/settings.rs`, already returns every reason this deployment will
  not be served as a typed list, and is already called by `Settings::load` so nothing can obtain a
  `Settings` that skipped it. `TlsTerminationUndeclared` and `AccessTokenRequired` are the two variants
  this joins. It reads the parsed tree and produces typed refusals naming what is wrong, which is
  exactly this check's shape.
- **The adapter cross-check is a startup refusal in the composition root**, beside `open_engine`,
  because whether the LINKED adapter can carry a per-subject credential at all is a property of the
  build rather than of the file, and `sutura-config` cannot see it.

Neither is in `verify_and_validate`. What `verify_and_validate` does gain is the verification identity
it needs because it *executes* - and that arrives with the port, in `feat/credential-port`.

**The mode is CONFIGURATION and the capability is the adapter's**, and the two are not the same thing -
[pluggable by declaration](adr/0011-pluggable-by-declaration.md) says the mode travels beside the
definition digest because one bundle may be served by a deployment that impersonates and one that does
not. So what the adapter declares is whether it CAN carry a per-subject credential at all, as a
required associated item it cannot omit, and the boot check compares the configured mode against that
declaration. An earlier version of this step named its compile-fail test for the mode, which would have
pinned the wrong thing: an adapter cannot declare a mode it does not own.

**Tests.**
- `a_duplicate_alias_is_refused_at_parse`, `a_missing_file_is_refused_at_parse`,
  `a_relative_path_is_refused_at_parse` - asserted on the typed variant, not the message.
- `a_shared_source_in_a_multi_user_deployment_without_an_acknowledgement_is_not_fit_to_serve` -
  asserted on `Settings::refusals`, which is public and side-effect-free precisely so a test can.
- `a_source_configured_to_impersonate_on_an_adapter_that_cannot_refuses_at_boot` - the configured
  posture against the declared capability, in the composition root, which is the half `sutura-config`
  cannot see.
- `an_anchor_on_a_source_with_no_declared_verification_identity_does_not_boot` - naming the metric and
  the source. Not skipped, not warned, and not treated as a passing anchor, which are the three ways
  this would otherwise become a mode nobody chose.
- `a_posture_is_recorded_in_provenance_per_leg`.
- `an_adapter_that_declares_no_impersonation_capability_does_not_compile` - a compile-fail doctest,
  with a compiling twin.

**Done when** two sources can be configured, each says what it is, and an answer says which mode
produced it.

### What the branch found, and where this section was underspecified

**DONE.** Four corrections, and each one is here because the section as written could not be
implemented as written rather than because it was inconvenient.

**One: the source declaration needed a KIND, and this section never mentions one.** Without it there
is no way for a deployment to say *what* a source is, so the composition root's only signal is the
source NAME - and `sutura-serve` did exactly that, refusing any source not called `local`. That
comparison makes the *Done when* above unreachable: a second source can never be `local`, so two
sources can be configured and only one can ever be opened. It also refuses a legitimate deployment -
an operator holding a warehouse extract as a directory of files and calling that source `warehouse`
was told this build had no adapter for it, on the strength of an alias. So `sources.<alias>.kind` is
required, with `files` the one variant that ships, an unknown word is a parse refusal listing what is
available, and which adapter opens a declared kind is an exhaustive match in the root - a second kind
is a compile error there rather than an arm that falls through. **This deleted a test that landed one
step earlier:** `a_catalog_naming_another_data_system_starts_nothing` asserted the name comparison, and
`a_source_of_a_kind_this_build_cannot_open_cannot_even_be_configured` replaces it.

**Two: `a_missing_file_is_refused_at_parse` cannot mean what it says.** `CatalogSettings::parse`
already declines an existence check and documents why - a directory that disappears between reading
the configuration and opening the engine makes the check a claim that is already stale, and it makes
configuration validation depend on a filesystem. That argument holds unchanged for a source's
directory, so the refusal implemented is `NoDataDirectory`: an entry that named no location at all.
The test is `a_missing_file_location_is_refused_at_parse`. A missing *file* is still refused, at boot,
by the composition root that tries to attach it.

**Three: the DEPLOYMENT MODE is load-bearing here and is not in the *Adds* list.** The
acknowledgement test names a multi-user deployment, so the mode has to be expressible - and
[a credential per leg](adr/0008-a-credential-per-leg-for-the-calling-subject.md) 5a requires it
declared, with no default and no derivation. It also answers a question the *Adds* list leaves open:
in single-user mode a shared source needs no per-source acknowledgement, so where does the witness on
`SourcePosture::SharedServiceUser` come from? From the mode's own required reason. Without that, a
single-user deployment could not construct the posture at all. `security.identity` is therefore
required **once any source is configured**, which keeps `Settings::load` on the embedded defaults
working - and no deployment escapes it, because one with no source is refused by the root for naming a
source it has no entry for.

**Four: a CATALOG spanning two sources stops being a boot refusal**, which is the point of the branch
and worth stating because it deletes a second test from the step before it. Two declared sources are
now two engines, and a QUESTION whose plan would span two is split into a fact leg and a lookup leg
and refused as `FederationNotExecutable` by `answer` while no adapter executes a leg - three or more
refuse at plan time as `PlanSpansTooManySources`. What survives is the half that is still a
misconfiguration: a source the catalog reads and the deployment never declared.

**Two things this step does NOT deliver, so the next reader does not assume them.** `Warehouses<W>` is
generic in one adapter type, so two sources are two engines over two directories and a file engine
beside a network adapter is not expressible - that is `feat/bigquery-adapter`'s, and it wants a closed
enum over the registered adapters rather than `dyn`, because `Warehouse` carries a required associated
constant and is not object-safe. And the verification identity is a DECLARATION with a boot refusal
behind it; nothing passes it to the port, so what re-runs an anchor is still `execute` under whatever
identity the adapter holds. `feat/credential-port` is the step that closes that.

## The leg plan types, and their rendering

**Goal.** The shapes a federated query is made of, as domain types with a generator entry point and
goldens each - before anything executes them. **This is a correction of an earlier version of this plan
that said `crates/sutura-sql` needs "nothing new (it already renders a mono-source plan)".** That was
wrong in a way worth spelling out, because it made the next step look implementable when it was not: a
dimension lookup is not a `QueryPlan`, and the measure a federated fact leg has to project cannot be
expressed by `PlanMeasure` at all.

**Checked against the code rather than reasoned about**, since the whole point of this step is that the
shapes do not exist yet:

- `QueryPlan` in `crates/sutura-domain/src/plan.rs` holds one `bucket`, one `measure` and one
  `measure_label`. A dimension lookup has none of the three, which is
  [0007](adr/0007-federating-across-different-data-systems.md)'s own reason for pricing a second plan
  type, a `sutura-sql` entry point and a golden family.
- `PlanMeasure` has exactly two variants: `Simple { term }` and
  `Ratio { numerator, denominator, zero_denominator }`. And `Ratio` **renders the division into the
  statement** - `measure_expression` in `crates/sutura-sql/src/generate.rs` emits
  `CAST(numerator AS DOUBLE) / NULLIF(denominator, 0)`. So the one shape that carries two terms is the
  shape 0009's Decision 2 forbids per leg, and there is no variant that projects two terms
  side-by-side. **A decomposed `Avg` travelling as a sum and a count, and a ratio travelling as its
  numerator and denominator, are therefore not expressible today.** That is not a rendering detail; it
  is the mechanism the pull-up decision depends on.
- Exact `CountDistinct` needs the distinct KEYS at the combiner, not a count, which is a projection of
  a key set rather than an aggregate. 0007 decides that this is **not** a third type: it is a fact leg
  with no terms, which groups by its keys and projects them. An earlier version of this section made it
  a third variant and left the collapse to the implementer, which put an architecture decision in the
  branch rather than in the record that owns it.

**Touches.** `crates/sutura-domain/src/plan.rs` for the leg types and the closed set over them,
`crates/sutura-sql` for the one new entry point, `crates/sutura-domain/src/warehouse.rs` for what the
port accepts, and `crates/sutura-app/tests/golden` for the golden families.

**Adds.** `LegPlan`, a closed enum with **two** variants, exhaustive, so a third shape cannot arrive
without a match arm saying how it renders.
[0007](adr/0007-federating-across-different-data-systems.md) decides the shape and this step executes
it; an earlier version of this section proposed three variants and deferred the collapse to the branch,
which put the architecture decision in the implementer's hands:

| Variant | What it reads | What it carries |
| --- | --- | --- |
| **`Fact`** | the metric's own model | `source`, `metric`, `table`, `joins`, `bucket`, `keys`, `terms`, `filters`, `params`, `range`. `keys` holds the answer's local dimension keys, every remote dimension's join key, and every distinct key a non-descending term needs - at most seven columns, from `MAX_DIMENSIONS` plus a bucket plus two distinct keys. `terms` holds one column per DESCENDING term, zero to four entries |
| **`Lookup`** | one remote dimension model | `source`, `table`, `keys`, `filters`, `params`. No bucket, no terms, no range, no metric |

**The third shape is not a third variant: a distinct-key leg is a `Fact` with an EMPTY `terms` list.**
It groups by its key list and projects it, which is a distinct key set. Two variants rather than three
because the fact-versus-lookup split moves four fields together - bucket, range, metric and joins are
all absent for a dimension - while the aggregate-versus-distinct split moves one bit. And the other
collapse, folding both distinct shapes into one, was rejected for a reason worth keeping: it needs
`Option<PlanBucket>` and `Option<TimeRange>`, which makes *a dimension leg carrying a time range*
constructible.

**`LegTerm { PlanTerm, label }`, and NOT `PlanMeasure`. This is the mechanism, not a naming
preference:** there is no `Ratio` shape a leg can hold, so `ZeroDenominator` cannot reach a leg's
statement and a per-leg division is unrepresentable rather than discouraged. `PlanTerm` is reused
unchanged.

**One fact leg per source, fused across terms**, because a sum descends perfectly well at the finer
distinct-key grouping. So the leg bound stays **at most five**: one fact leg plus one lookup leg per
remote dimension model.

**`Warehouse::execute` does not keep its signature.** It takes an `Executable` two-variant enum -
`QueryPlan` or `LegPlan` - so every adapter's match is exhaustive. 0007 considered a second port method
and rejected it: a second method invites a default body, and a default that errors makes an adapter
silently non-federating. Note this is the same signature the credential port changes again, so
`feat/credential-port` and this branch both touch `warehouse.rs` and the second one rebases.

**One thing that does not move, and it is the reason this is affordable.** Every leg is still
mono-source, so `QueryPlan`'s single `source` field stays, *a plan cannot silently span two sources*
applies per leg unchanged, and no existing SQL golden moves - `AGENTS.md`'s counted claim about 63
goldens reading `LIMIT 10001` stands, because a federated leg is a new plan shape with its own goldens
rather than an edit to those.

**And the definition digest does NOT move. An earlier version of this section said it did, twice, and
that was false** - a claim invented to justify why this is its own branch. `DefinitionDigest::of` in
`crates/sutura-domain/src/definitions.rs` hashes the `Definitions` and the `Knowledge` and nothing
else; a plan type is under neither, so no committed digest changes. What is pinned about the plan is 21
`plan@markdown` snapshots, and those move only if `QueryPlan` itself changes, which it does not. The
real reason this is its own branch is better than the invented one: the shapes, their rendering and
their per-dialect parse checks are evidence that stands on its own, before anything executes them, and
a branch that lands types plus goldens plus an execution path puts three kinds of failure in one
review.

**Tests.**
- `a_lookup_leg_has_no_measure_and_no_bucket` - the type, not a runtime check.
- `an_aggregate_leg_projects_a_decomposed_average_as_two_terms_undivided`, and its negative twin
  `a_leg_cannot_be_constructed_that_divides_a_ratio` - the shape 0009 forbids is unrepresentable rather
  than discouraged.
- `a_fact_leg_with_no_terms_projects_keys_rather_than_a_count` - the distinct-key shape, which is a
  `Fact` with an empty `terms` list rather than a variant of its own.
- `a_lookup_leg_cannot_carry_a_time_range` - the type, and the reason the two-variant split is the one
  0007 chose over folding both distinct shapes together.
- `every_leg_shape_renders_in_every_compiled_dialect` - the golden family, per shape per dialect, plus
  the per-dialect parse check the existing corpus already applies.
- `no_leg_statement_carries_a_question_literal` - bind parameters per leg, asserted the way the
  mono-source corpus asserts it, because a new entry point is a new place for that to be got wrong.
- `a_leg_statement_carries_no_row_limit` - `generate_leg` emits no `LIMIT`, because a leg is not an
  answer and `row_limit()` is a cap over one.
- `a_new_leg_variant_does_not_compile_without_a_rendering_arm`, and
  `an_adapter_that_does_not_match_every_executable_does_not_compile` - two compile-fail doctests with
  compiling twins, which is what makes the closed set and the `Executable` enum mechanisms rather than
  conventions.

**Done when** both variants exist, render through the one `generate_leg` entry point, parse in their
target dialects and are pinned; no existing golden and no committed digest has moved; and nothing
executes any of it yet.

## Two sources, one question, end to end

**Goal.** The real machinery, with two DuckDB files as its first instance: split by source, render each
leg through `feat/leg-plan-types`, combine above.

**Touches.** `crates/sutura-semantic` for the split, `crates/sutura-exec-datafusion` for the combine
and for the `RowSet`-to-Arrow boundary, and `crates/sutura-app` for the leg set and the observability
of pushed against pulled. `crates/sutura-sql` is touched by the branch BEFORE this one, not by this
one - the entry points and goldens land there.

**Adds.**

- **A split into legs**, producing one aggregate leg plus one lookup leg per remote dimension model,
  plus a distinct-key leg where the measure needs one. At most five legs, which is a consequence of
  `MAX_DIMENSIONS` and the one-hop join rule rather than a new budget.
- **Same-source fusion, and the rule is NOT a `BTreeMap` over `SourceName`.** An earlier version of
  this step said grouping by source was the whole rule and named its test
  `two_models_on_one_source_become_one_leg`; 0009 withdrew that, and this step carries the withdrawal
  rather than pinning the withdrawn rule. Two lookup models on one remote source with no declared
  relationship between them would fuse into a **cross product**. The rule is *same source AND connected
  by a declared relationship in this plan*, which is analysis over the join graph - still far cheaper
  than recovering source membership from a physical plan, and still not a grouping.
- **The join kind derived from where the filters went**: INNER for a remote dimension carrying a
  filter, LEFT for one that does not. 0007 derives this and it is the finding that produces a wrong
  number rather than a refusal.
- **The `RowSet`-to-Arrow boundary**, which this branch answers rather than defers, because the combine
  is where two row-oriented `RowSet`s have to become something joinable. And with it **the byte budget
  0009's Decision 3 puts here**: a bound applied as rows are converted, before a whole `RowSet` exists,
  which is the only thing that can refuse an oversized leg. The engine's memory pool cannot - it counts
  operator reservations and not driver buffers - so `feat/query-bounds` deliberately does not promise
  it and this branch does.
- **Whether a leg was pushed or pulled is observable**, because a renderer that silently stops pushing
  returns a correct answer at seven times the memory with no diagnostic.

**Tests.**
- `two_sources_answer_the_same_rows_as_one_source_over_the_same_data` - the differential property,
  extending `crates/sutura-app/tests/differential.rs`.
- `a_distinct_count_across_two_sources_is_exact` - the distinct keys transported and counted above,
  compared against an independently computed truth. Not "correct or refused": 0009's Decision 2 decides
  the pull-up, so a refusal here is a failure.
- `a_decomposed_average_across_two_sources_equals_the_single_source_answer`.
- `a_filter_on_a_remote_dimension_over_an_orphan_fact_key_matches_the_single_source_rows` - the case
  0007 says neither half catches alone, by name.
- `two_models_on_one_source_connected_by_a_relationship_become_one_leg`, and its negative twin
  `two_unrelated_models_on_one_source_do_not_fuse` - the second is the cross product, and it is the
  test the old name could not have caught.
- `an_oversized_leg_is_refused_at_the_conversion_boundary_rather_than_aborting_the_process` - the byte
  budget, which is why it is in this branch and not in `feat/query-bounds`.
- `a_leg_that_was_pulled_rather_than_pushed_says_so`.

**Done when** rows equal the single-source corpus, each leg's statement is pinned by the golden family
that landed with `feat/leg-plan-types`, an oversized leg is refused at the conversion boundary rather
than killing the process, and **the two-source example flips from a refusal to an answer** - the same
corpus, so the diff is the behaviour change.

## Conformance packs

**Goal.** One set of test bodies, bound to each adapter by a macro, so a new connector proves itself by
registering and declaring.

**Touches.** A new dev-only workspace crate for the packs and the macro;
`crates/sutura-cli/tests/` for the harness extraction that `federation.rs` deferred at two corpora and
is now the third.

**Adds.** Compile packs (a catalog and the question corpus: plan, rendered statement per dialect, bind
parameters, refusal variant - **no data system needed**) and execute packs (rows, and that they are
identical across adapters). A macro generating one named test per behaviour per adapter, so a filter can
select a tier and a failure names the behaviour. Declared capabilities select the packs, and a
capability declared unsupported that turns out to work FAILS.

**Also adds two things about snapshots, because a corpus multiplied by adapters rots quietly.**
`cargo-insta` **goes into `devenv.nix`, and is not there today** - checked rather than remembered:
`grep -rn insta devenv.nix nix/ flake.nix` finds no `cargo-insta` anywhere, and the only `insta` in the
workspace is the library in `Cargo.toml` plus `INSTA_UPDATE = "no"` in `flake.nix`. **An earlier
version of this section said it was already pinned, in the present tense, and that was false** - the
same class of invented debt 0012 corrected about itself in the previous round, reintroduced here, which
is why it is stated as work rather than quietly amended. The pin belongs in nix because the snapshot
tool's version decides whether an orphan is reported, which is exactly the class `AGENTS.md` says nix
is the only pin for, and `check-pins` fails if it also appears in pixi. What it is for is
`cargo insta review`, the interactive accept a developer actually needs. The orphan CHECK stays a gate
in `xtask` rather than `cargo insta test --unreferenced`, because the macro knows the exact case list
and can be more precise than "unreferenced", and because a dev-shell tool is not on a flake check's
path.
[Conformance packs](adr/0012-conformance-packs-for-inputs-and-adapters.md) has the reasoning and the
route not taken.

**Tests.** The packs are the tests. Plus:
- `a_declared_unsupported_capability_that_works_is_a_failure`.
- `adding_an_adapter_touches_a_registration_and_no_pack_body` - asserted by the macro's expansion.
- `a_snapshot_no_case_references_fails_the_gate`, and its twin `every_generated_case_has_a_snapshot`.

**Done when** the semantic compiler is conformance-tested across catalogs with no container anywhere,
the compile half runs on every push, and an orphaned snapshot fails a gate rather than accumulating.

## The metadata capability declaration

**Built on `feat/metadata-capabilities`, not yet merged** - the stack table above is the one owner of
a step's status and it marks a row DONE when its PR lands, so this note says what the branch does and
claims nothing about the table. `sutura_domain::capabilities` holds the vocabulary and
`SemanticCatalog::capabilities` is the required associated item. **Three things landed differently
from what is specified below, and each is written where the code is rather than only here:**

1. **An associated FUNCTION taking no `self`, not an associated constant.** The property
   `Warehouse::IMPERSONATION`'s constness buys is that the declaration cannot vary per instance, and
   taking no `self` buys exactly that. What is given up is const evaluation, and the reason is
   representational: the declaration reuses `KnowledgeCapabilities` verbatim rather than growing a
   second vocabulary over the same four kinds, and that is a `BTreeSet` newtype no `const` expression
   can build. The const-friendly alternative is a boolean per kind, which `sutura_domain::knowledge`
   already argues against. Nothing needs the value in a const context.
2. **Two of the nine kinds are observed through their consequence rather than through a field.**
   `Cardinality` is observed as *some dimension is reached through a relationship*, because every
   `Relationship` holds a `JoinType` - the type has no other shape - so the field's presence says
   nothing and what a caller loses is the join. `Descriptions` is observed as *some description is
   non-empty*, because a bundle of `Description::default()` carries no prose whatever its fields are.
3. **No narrow adapter was registered to exercise declaration fidelity, and none was invented.**
   `tests/adapters/mod.rs` registers only what somebody could deploy, so the declaring case is
   exercised over the two narrow fakes the suite already had - `HandWrittenCatalog`, which carries no
   prose, and `TwoSourceCatalog`, which carries five fewer kinds. Each now declares that, and the
   fidelity test is what holds it. Both fidelity directions also expand over every registered catalog,
   which for the reference adapter asserts the example corpus really carries all thirteen kinds.

**Goal.** A `SemanticCatalog` adapter cannot be silent about what it does not provide.
[Pluggable by declaration](adr/0011-pluggable-by-declaration.md) decided this and
[what DataHub can carry](adr/0016-what-datahub-can-carry.md) is why it is now scheduled: the first
adapter that provides part of a model is the first thing that needs it.

**Touches.** `sutura-domain`'s `SemanticCatalog`, which today is an associated `Error` and `load` and
declares nothing. `sutura-catalog-local`, which gains one line saying it provides everything.
`crates/sutura-app/tests/adapters/mod.rs`, where the declaration joins `posture` as part of what a
registration is.

**Adds.** A closed capability vocabulary for the metadata side - structure, descriptions,
relationships, cardinality, metrics, filters, grains, allowlists, anchors, and the four knowledge kinds
that already have one - carried as a **required associated item with no default.**
`Warehouse::IMPERSONATION` is the shape to copy and the argument is already written there: *"A
defaulted capability would mean an adapter that said nothing got the benefit of the doubt in whichever
direction the default pointed - and both directions are wrong."* So the rule is the one that trait
already demonstrates twice over: **a capability whose absence changes what a caller may believe is
required with no default, and a capability whose absence is merely a missed optimisation may be
defaulted with a doc comment saying why** - `dry_run` is the second case and says so in its own words.

**And the negative gets a NAME.** `ImpersonationCapability::NoPlaceForASubject` is the precedent: the
absence is a variant a reader can see, not a key missing from a map. A metadata adapter that cannot
carry grains says so in a word.

**The reference adapter's declaration is `all()`, and that is not laziness.**
`sutura-catalog-local` already documents why: *"it says 'this provider supports whatever kinds exist',
which is what makes this the reference adapter and what keeps a fifth kind from needing an edit here.
An adapter mapping a fixed external schema gets the opposite treatment - `of([..])`, so a new kind
leaves its declaration alone."* That sentence is about knowledge capabilities and it generalises
unchanged.

**Tests.** Specified here, and the names that landed are given beside each, because two of the four
were merged into one function and a name nobody can grep for is not a test list.
- `an_adapter_that_declares_nothing_does_not_compile` - a `compile_fail` doctest with its compiling
  twin, differing by the one line, verified non-vacuous by unmarking it. **Landed** on
  `SemanticCatalog`'s own doc comment, with the failing struct literally named `Undeclared` and its
  twin named `Declared`, which is `Warehouse::IMPERSONATION`'s shape exactly.
- `a_declared_kind_with_no_content_is_not_the_same_as_an_undeclared_kind` - the three-state
  distinction, which is the whole reason the declaration exists. **Landed under that name twice**:
  once in `sutura_domain::capabilities`'s own tests over a built bundle, and once in
  `tests/golden/catalogs.rs` over the oracle's real one.
- **Declaration fidelity, which is the assertion a declaring adapter gets instead of the oracle:**
  `everything_it_declared_it_produced` and `nothing_of_an_undeclared_kind_appears_in_the_bundle`.
  The second direction already exists for knowledge as `Knowledge::assemble`'s `UndeclaredContent`
  guard and exists nowhere for definitions. **Landed as ONE function** -
  `MetadataCapabilities::checked_against` - because two functions a caller has to remember to run
  both of is the shape this repository avoids, and the returned variant
  (`UnfaithfulDeclaration::Undeclared` or `::Unprovided`) names which direction failed. Its callers
  are `it_provides_exactly_what_it_declares` per registered catalog,
  `a_declaring_adapter_provides_exactly_what_it_declares` over the two narrow fakes, and
  `a_declaration_wider_than_the_bundle_names_the_kind_it_over_claimed` for the over-claim direction
  against a real bundle. The domain's own tests provoke each variant separately and assert the
  undeclared direction is the one reported when a declaration is wrong both ways.
- **`agrees_with_the_oracle` is NOT touched.** It stays the golden adapters' contract. A version of it
  that tolerated a missing measure would pass a golden adapter that had silently stopped reading them,
  which is the one thing that test is for.

**Done when** an adapter cannot be written that is silent about a kind it cannot supply, and the
matrix runs the oracle over the golden adapters and declaration fidelity over the rest.

## The DataHub catalog, and what it declares it cannot do

**Landed - and issue #202 spent one of the absences below rather than delivering it.** The stack
table above is the one owner of a row's status; this note says which sentences in this section the
tree no longer agrees with, and each correction is written where the code is rather than only here:

1. **The measure is a CONDITIONAL provide, not a declared absence.** A deployment defines one
   string-valued structured property - under a name of its own; `sutura` is the field on this
   adapter's canonical shape, not a urn - whose scalar is a closed-vocabulary document,
   and `sutura-catalog-datahub` reads a whole certified metric out of it - measure, time column,
   grains, definitional filters, dimensions with their allowlists, an anchor and prose - so
   `Metrics`, `Grains`, `RequiredFilters`, `AllowedValues`, `Anchors` and `Cardinality` are
   **declared-and-empty may-provide** kinds rather than absences. A deployment that defined nothing
   still loads models, prose and joins, which is what keeps the narrow deployment below true.
   `docs/adr/0016`'s *Amendment, 2026-09-02* and its addendum are the record, and the *Does not
   provide* sentence under this note is the pre-#202 decision for the measure.
2. **`cardinality` is the absence that stayed**, for the reason the *Two of those absences* paragraph
   below gives. The one way this adapter produces one is a dimension the deployment declares with a
   `via`.
3. **The test list below is the pre-#202 one and four of its five names do not exist.** What holds
   the claim now is `crates/sutura-catalog-datahub`'s own suite -
   `it_declares_metrics_and_the_bundle_carries_one`,
   `a_metric_without_the_defined_shape_is_reported_and_not_defined`,
   `a_ratio_and_a_count_if_are_carried_as_closed_measures`,
   `an_unknown_key_at_the_sutura_level_is_refused_and_named`,
   `a_relationship_alone_licenses_no_dimension` and
   `content_for_a_kind_it_did_not_declare_fails_the_load`, over a fake reader on recorded documents.
4. **The read path is still unbuilt; this step's first engineering question is no longer
   unmeasured.** The only `AspectReader` is still the recorded fixture, so no library code shapes a
   request or maps a response. What `just datahub-acceptance` now measures against the provisioned
   instance is the platform's half: a deployment can define the property under a name of its own, the
   corpus's own document is accepted as its scalar and the served aspect decodes into the same
   `document::MetricAspect` the fixture carries, through to the closed-vocabulary `Measure`. The cost
   is ONE paged request per entity type, carrying `structuredProperties` and `metricInfo` inline -
   **and that surface is search-backed, so it is not read-your-writes**: it lagged a synchronous
   write by ~2.2 s where the by-urn read answered at once. The scalar's ceiling is the platform's
   `keywordMaxLength`, and `SINGLE` cardinality and the declared value type are refused server-side.
   `a_document_served_by_a_real_datahub_decodes_into_a_certified_metric` is where each of those is
   asserted; what remains owed is the reader itself, the structural half of a live snapshot, and the
   bearer half, which the tier leaves off.

**Goal.** A `SemanticCatalog` over DataHub that gives a deployment value from the model it already
has, and names every gap rather than leaving it to silence.

**Read [what DataHub can carry](adr/0016-what-datahub-can-carry.md) first.** It is the measurement
this step is shaped by, and its decision 3 is the declaration's content: **provides** structure,
descriptions, the join columns, and - conditionally, where the bundle declares a metric to point at -
glossary phrases and caveats. **Does not provide** measures, cardinality, definitional filters,
grains, value allowlists, anchors, reviewed absences or worked examples.

**Two of those absences are declared for a source that HAS the field**, which is the part not to
quietly reverse. `metricInfo.expression` is a raw string in a dialect set that does not intersect ours,
and the `aggregationFunction` beside it is authored independently with nothing reconciling them, so
taking either certifies half a definition. `cardinality` defaults to `N_N` on the physical
relationship, so a default is indistinguishable from a decision. **Declaring both unsupported costs one
line each and is the better outcome**; harvesting them is the failure mode this whole repository is
arranged against.

**Touches.** A new `sutura-catalog-datahub` crate, and the registry in
`crates/sutura-app/tests/adapters/mod.rs`.

**The read path.** An HTTP client written here, because DataHub publishes Python and Java SDKs and no
Rust one. **Which surface is settled by DataHub's own guidance** and 0016 records why: its GraphQL API
assumes frontend callers and says operations there are intentionally limited in scope, so this reads
the versioned OpenAPI v3 entity surface, against a spec the deployment serves for itself, with a
personal access token as a bearer. What is **not** settled is the cost - how many requests a whole
bundle takes, and what keeps a generated client from drifting - and that is this step's first
engineering question.

**A DataHub-only deployment works, and that is a requirement of this step rather than a nice
outcome.** A bundle of models, relationships and zero metrics assembles, pins and validates - 0016
checked that there is no minimum-metric refusal - and the prompt states the absence of a certified
metric layer as a fact derived from the bundle. What such a deployment needs configured is a
`sources.<alias>` entry per platform its models name, because `sutura-serve` resolves a catalog source
through the registry rather than by comparing names.

**Guidance, not requirement - and this is the sentence to hold the docs to.** A short section tells a
user what they **may** populate and what each thing buys: `cardinality` on a semantic model's
relationships lets those relationships license a join; `AiContext.synonyms` on a metric the deployment
has also certified lets a glossary phrase render; a `sources.<alias>` per platform lets a model's data
system be opened. **Each is an option with a payoff stated, and the absence of all of them is a
supported configuration.** The two sentences that may not be written are *"configure DataHub like
this"* and *"DataHub is not usable without X"* - the second is false, and the first is not ours to say.

**Note what this step does NOT get.** No source-level usage prose in the prompt: 0011 withdrew that
claim in full, a note is attached to a `Referent` that names a metric, and there is nothing legal for a
source's own instructions to point at. And nothing server-side may **match** on a harvested synonym -
DataHub's own roadmap goes the other way, this repository has no `PhraseNotDefined` and the agent
states its choice in its own transcript, and 0016 names that as a fork rather than a gap.

**Tests.** *The pre-#202 list - note 3 at the head of this section names what holds it now.*
- `it_declares_it_provides_no_metrics_and_the_bundle_has_none` - declaration fidelity, from row 25.
- `a_metric_whose_measure_is_an_expression_string_is_reported_and_not_defined`.
- `it_declares_it_provides_no_cardinality_so_a_relationship_licenses_no_dimension`.
- `a_bundle_of_models_and_no_metrics_loads_and_validates` - the DataHub-only deployment, which is the
  test that would fail if somebody made metrics a precondition.
- `content_for_a_kind_it_did_not_declare_fails_the_load` - the existing `UndeclaredContent` guard,
  against a real source rather than a fixture.

**Done when** a deployment running DataHub gets its physical model, its prose and its joins with no
catalog authored here, its declaration says in words which of the nine definition kinds and four
knowledge kinds it cannot supply, and the docs offer a way to get more without implying it is required.

## The rest of the stack

The branch sections above cover the first eleven rows of the table - the surface, the domain and
federation - and rows 25 and 26, the metadata capability declaration and the DataHub connector.
**Those two are here rather than split off by the seam this page otherwise follows**, and the reason is
in `devco/max-lines-ignore`: prose is exempt from the line cap, and the earlier splits of this document
cost cross-references and paragraphs explaining themselves. Row 25 needs no live service and belongs
here by the seam's own rule; row 26 does need one, so by the letter it belongs on the services page.
Separating a finding from the connector it shapes is the mistake that left DataHub with four accepted
records and no row at all, and it is not worth repeating for tidiness. **Rows 12 to 24 continue on two
further pages.**
[BigQuery](implementation-plan-bigquery.md) carries rows 15 and 16.
[Identity, services and the operational work](implementation-plan-identity-and-services.md) carries the
credential port, the compose tier, the two Postgres steps, mutual TLS, the raw SQL tool, the demo
tasks, the supply chain and the CI cost of a prose change - plus the selective-service-CI work that
is deferred rather than scheduled, and what is deliberately not in this plan at all.

**The stack table above stays the only owner of a step number, on either page.** The split happened
because one file crossed the 1000-line limit `cargo xtask max-lines` enforces, and the seam is the
stack's own phase boundary rather than an arbitrary page count: everything up to and including the
conformance packs needs no live service and no identity decision, and everything after it needs one
or both.
