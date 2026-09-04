---
name: query-surface
description: The governance boundary - for each kind of change to the query path or either transport, which mechanism fails if the guarantee does not hold, and which changes have no mechanism at all. Plus the generated-artefact rules that are not obvious from the code, and what is built and not wired. Open before touching either surface.
---

# The query path and the tool surface

The tool surface **is** the governance boundary. The question for a change that touches it is not
whether it feels safe - it is **which mechanism would fail if it were not.** A change that cannot be
tied to one is unproven: say so, and adding the missing check beats adding a sentence.

| Change | Must still hold | What fails, or does not |
| --- | --- | --- |
| A new or widened tool input | No field carries SQL, a table, a predicate or row ids | `deny_unknown_fields` makes an *undeclared* field an error naming it - asserted **through** the transport rather than assumed to survive it. A *declared* field needs the committed MCP schema snapshot, which is why that byte-compare exists. **Limit:** there is no equivalent dump for the OpenAPI document, so on HTTP a widened input is caught by the deserializer and review alone |
| A new failure mode | A `RefusalReason` variant inside `ToolOutcome`, never an `Err` | Two exhaustive matches with no wildcard arm, one per transport, so it fails to compile in both. The crates cannot see each other, so their vocabularies are kept equal by a *derivation* - the code is the variant name in snake_case, read off the domain type's own `Serialize` - not by a comparison an adapter may not make |
| A new tool, route or operation | It names a `Capability`, that capability names a scope, and both transports describe the same set | Five exhaustive matches plus `RouteNotGoverned`. `both_transports_describe_the_same_tools` runs **once per transport against the one declaration** - two tests over one source, not a comparison between adapters. Adding a capability changes the DEPLOYED contract: an authorization server is configured with the scope by hand, which is why ids and scopes are pinned by value and are separate literals, so a tool rename cannot rename a scope |
| Reading from the catalog at request time | Descriptive content only - nothing that selects, widens or parameterizes what executes | `load()` has no `RequestContext` to pass it; dimension validation reads the pinned definitions, not the scoped view |
| A second execution leg | One answer has one asker, and no leg runs as a third identity: each runs as the asker **or** under that source's acknowledged shared identity, and the answer records which. *"Every leg runs as the same subject"* was the wording here and was overstated - a source serving everyone as one identity does not run as the asker, and making the labels agree would not have made the identities agree | Recording, credentials and the refusal are all built; see `../identity/SKILL.md`. **Not the promise:** no test asserts two subjects get different ROWS - that needs a live dataset with row-level security and two real grants |
| A new knowledge kind | The prompt stays the only consumer | Three exhaustive matches plus a `const` assertion on the walk's seed |
| A **second consumer** of a knowledge kind | Same | **Nothing mechanical.** `Query` having no field a phrase fits in is what makes the glossary descriptive, so reading a note elsewhere is an architecture decision - flag it in the handoff |
| Anything that stores or forwards rows | - | **Nothing mechanical.** A human review question, not an agent's to certify: flag it in the handoff |

## Generated artefacts

One owner each, nothing hand-edited, every regeneration checked rather than trusted. The owners are
visible in the code; these rules are not:

- **The MCP schema derive is on the WIRE type and never on a domain type**, because a macro crate on
  a domain type enters the domain's allowlist, which is walked transitively. The byte-compare is a
  snapshot **per capability**, and a second reader checks the same files off the wire as parsed JSON
  - so what a client generator consumes is the document a reviewer accepted, and a transport that
  stopped putting the generated schema on the tool is a failure rather than a green snapshot.
- **A doc comment on a wire type is CALLER-FACING prose.** `schemars` puts it into the schema's
  `description` and a model reads it; reasoning about the type goes in a plain comment beside it.
- **Kept equal by review, not a test:** the field lists of the two wire types doing the same job on
  the two transports, and the prose - deliberately, since a tool description and an OpenAPI summary
  are written for different readers.
- **The OpenAPI document is built at startup and served**, not dumped, so a missing path attribute
  fails to compile rather than producing a page with a gap.
- **`ATTRIBUTION.md` is deliberately WIDER than any one binary**, which is the opposite of an
  SBOM's rule and for a stated reason: an SBOM overstating what is in an artefact is a false claim
  about it, while an attribution document naming a crate that did not ship discharges an obligation
  nobody had. What selects a row is **not being a workspace member**, never the presence of a
  `source` line - `source` means registry-or-git, which omitted the vendored path dependencies while
  a shipped binary links one of them. **Limit:** it carries no Apache-2.0 `NOTICE` text, and
  **there is no `NOTICE` check** - "inspired by" is not a licence position.
- Two attribution gates, split by the input each needs: one reads the lock and holds the crate SET;
  the other regenerates and byte-compares, holding the CONTENT, and needs a resolvable registry.
  Without the second, changing any row's licence to arbitrary text passed.

## The dialect lesson worth carrying

A golden pins the statement **text**, so two data systems can execute identical text and disagree.
Measured: `ORDER BY x` did not say where a null goes, `DataFusion` orders nulls LAST and GoogleSQL
orders them FIRST, and five of 31 corpus questions returned the same rows in a different order - one
at 61 rows, no number wrong. **No golden could see it.** What differed was the order of a certified
answer the plan claims by emitting `ORDER BY`.

The fix converges **behaviour, not text**: the keyword renders for the one dialect whose default is
the other way and collapses for the three where it already is the default, and a test asserts its
ABSENCE for those three - so "all four dialects spell it" would have been red the day it was
written.

Generalise it: a fifth dialect cannot compile without answering the exhaustive declarations for
identifier case, path depth and bucket shape, and each of those exists because **a parse check was
measured blind to it.**

## Built and not wired - do not cite as an invariant

Exists, is tested, has no caller from any binary. Three invariant rows once stated this as enforced;
they were **deleted rather than demoted**, which is the table's own rule applied to itself.

- **The authored-SQL hatch** (`docs/adr/0004`) is complete and unreachable: the metric type holds no
  computation, the document shape has no key for it, and `compile` has no production caller. **The
  gap that is not wiring:** no shipped binary could execute an authored expression even with the load
  path in place - the engine generates no SQL, and the renderer-backed adapter is a dev-dependency.
  Wiring the load alone would move a refusal from load time to query time.
- **Federation is wired above the port and not below it.** Splitter, leg plans, rendering, goldens
  and the orchestrating call all exist and run; what gates it is a defaulted-`false`
  `EXECUTES_LEGS`, which only the dev-only DuckDB vehicle sets true. So the shipped binary refuses a
  two-source question rather than letting a typed leg refusal surface as a retryable `503`. A full
  two-DuckDB differential is not written, which is exactly why this stays here.
- **The DataHub catalog adapter decides a whole metric, and nothing reads or serves it.**
  `sutura-catalog-datahub` provides `Structure`, `Descriptions` and `Relationships` unconditionally
  and declares the metric kinds as **declared-and-empty may-provide** kinds
  (`DefinitionCapabilities::of_may_provide`) - so a DataHub that defines no metric content still
  loads, models and prose and joins, no metrics. Where the deployment defines a string-valued
  `sutura` structured property, that ONE scalar JSON document carries the whole metric - measure,
  time column, grains, required filters, dimensions with their allowlists, anchor and prose -
  certified over the domain's closed vocabularies, with `deny_unknown_fields` at every depth,
  including inside an anchor's range, which is where *every depth* was one depth short until the
  attribute reached `calendar::TimeRangeInput`. `docs/adr/0016`'s amendment and its 2026-09-04
  revision are the record. **The two halves that do not exist:** a real `AspectReader` over
  DataHub's versioned OpenAPI v3 entity surface - the only implementor outside a test is the recorded
  fixture source, so no library code shapes a request or maps a response - and a composition root,
  because
  `sutura-serve` refuses `catalog.kind: datahub` by name and the crate's only dependant is
  `sutura-app`, as a dev-dependency. **Do not read the declaration as availability:** what is proved
  is that the adapter decides correctly against a fake reader.
- **The provisioned DataHub tier proves the VENUE and the PLATFORM's half, and not a read path.**
  `just dev-up-datahub` stands up DataHub 1.7.0 behind a compose profile - upstream's own
  `quickstart-backend` selection minus its actions container - and `just datahub-acceptance` gets a
  `2xx` off `openapi/v3/entity/dataset` **and** round-trips the recorded corpus's own document
  through a structured property registered under a name the TEST chooses and the library never
  spells: accepted by the platform's validator, served back, decoded through the adapter's own
  `MetricAspect` into a certified `Metric`, with `SINGLE` cardinality, the declared value type and
  the scalar's ceiling all refused server-side - the ceiling named by the platform as its
  Elasticsearch `keywordMaxLength`, an index setting rather than a constant here - what was
  measured is that the refusal NAMES it, not that raising it works. **Still absent:** any HTTP
  `AspectReader`, so the requests and the response mapping live in the test rather than in `src/`,
  and the structural half of that snapshot is still the recorded corpus; any authentication
  (`METADATA_SERVICE_AUTH_ENABLED: "false"`, so the auth half is untested); any frontend, so there is
  no UI; and any CI job, because the nix sandbox has no docker socket. **A platform that accepts the
  document is not a read path**, and the acceptance cells are `#[ignore]`d, so they are evidence of
  whatever the last `just datahub-acceptance` run reported and of nothing in the default suite - run
  without `SUTURA_DEV_REQUIRE_TIER=1` they report `ok` having asserted nothing, which is why the task
  sets it.
- **Two DataHub read surfaces, and only one is read-your-writes** - measured 2026-09-04 by
  `just datahub-acceptance` against 1.7.0, and it is the caveat the read path's COST claim needs.
  `GET openapi/v3/entity/metric/{urn}` answers a synchronous upsert immediately. The PAGED
  `GET openapi/v3/entity/metric?aspects=..` is search-backed - it answers `facets` and `totalCount` -
  and lagged the same write by ~2.2 s. So a page per entity type is the right cost for a bundle and
  it is eventually consistent, which is the whole reason the acceptance cell polls that surface to a
  deadline and asks the by-urn one exactly once. **The version of that cell before this was red for
  exactly this reason** and had been reported as measured: it wrote, paged once, and only passed on a
  re-run whose index was already warm.
- **DataHub tier parallelism holds for the CONTAINERS and not for DISCOVERY** - project name from a
  path digest, ephemeral published ports, named volumes, all gated - but
  `.sutura-dev/endpoints.json` has two writers and each rewrites it wholesale, **in both
  directions** - measured, a live nix postmaster absent from a file `xtask dev-up` had just
  rewritten, and the reverse for the whole of `just test`. Nothing gates that.
- **The BigQuery adapter is a whole adapter in this state, and has been leaving a piece at a time.**
  Everything above the wire is decided and tested against a fake; the wire exists behind a
  default-off feature; a real dataset has accepted the whole corpus and reproduced its anchors, green
  in CI; and a composition root opens the kind. **What has still never happened: no published
  artifact links the crate**, and `checks.shipped-features` reads that absence off the artefact
  rather than off a manifest. The `data_systems:` golden axis therefore gains no entry - that
  registry's rule is that a cell which cannot execute reads as coverage. The DIALECT axis does have
  one.
- **What both acceptance legs say nothing about is identity.** A service-account key is one identity
  for everybody who asks, so what they establish is *accepted, and correct for that identity*.
