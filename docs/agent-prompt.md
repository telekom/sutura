---
title: The agent prompt
description: The system prompt sutura generates for an agent, what each section is derived from, and what an operator can layer on top.
---

# The agent prompt

An agent that has not been told what this surface is treats it as a database. It looks for a field
to put SQL in and finds none. It sends a metric name it remembers from somewhere else and gets a
refusal. It reads the refusal as a transport failure and retries. Each step is a reasonable thing
for a general-purpose agent to do, and each step is behaviour that the types in
[Concepts](concepts.md) are arranged to prevent. The types stop the *damage*. They cannot stop the
loop.

`sutura prompt` renders the document that can.

```bash
sutura prompt examples/single-player/catalog > agent-prompt.md
```

The output is markdown on standard output. It is meant for pasting or piping into an agent's system
prompt. Nothing in it is hand-written: each section is derived from the tool surface, from the
pinned bundle, or from a file an operator named.

## Where each section comes from

| Section                                                | Derived from                                                                                                                                                                                                                              |
| ------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| What this is                                           | Fixed text. Two facts: the set of answerable questions is finite and listed, and a question outside it is declined rather than approximated                                                                                               |
| What to do for every question                          | **The tool list.** The step that reads the catalog is present only when that operation is exposed, and is replaced by a sentence saying the list in the document is the whole of it when it is not                                        |
| A refusal is an answer, not an error                   | Every `RefusalReason` variant, with what it means and what to change. The most load-bearing section in the document                                                                                                                       |
| Terms this deployment records as NOT defined           | **The pinned bundle's knowledge**, `not_defined` kind. Present only when the provider declared that capability; a declared-and-empty capability renders the sentence that nothing is recorded, which is a different fact from not knowing |
| The bounds a question is held to                       | `MAX_DIMENSIONS`, `MAX_FILTERS`, `MAX_RANGE_DAYS` and `MAX_ROWS`, read from the code rather than typed                                                                                                                                    |
| What this surface has no field for                     | Fixed text, and deliberately short - see below. Gains one sentence, only where a deployment turned on the raw SQL tool, pointing at the exception rather than leaving the fixed text to contradict the operations list                    |
| The operations you have                                | **The tool list**, rendered from the same slice the workflow was composed from. `run_sql` appears here, framed as ungoverned and never as certified, only where a deployment set `tools.run_sql.enabled: true` - see below                |
| What this deployment records about its own definitions | **The declared knowledge capabilities**, and what is *not* declared is listed too - because a kind that is not recorded is a kind an agent must not draw a conclusion from                                                                |
| The words a question may arrive in                     | **The pinned bundle's knowledge**, `glossary` kind. Rendered so the *agent* does the resolving; there is no field on a question a phrase fits in                                                                                          |
| Physical structure is not a certified metric           | **The pinned definitions.** Present only when the bundle has models and has no metrics; it explains authored prose and the semantic promotion path without exposing a table or column name                                                |
| The metrics this deployment defines                    | **The pinned bundle.** Name, grains, dimensions, permitted values, the catalog author's own prose, and any `caveat` printed under the metric it is about                                                                                  |
| Worked questions                                       | **The pinned bundle's knowledge**, `example` kind. Each carries a `Query` the bundle would not load if this surface would decline it                                                                                                      |
| Provenance                                             | Fixed text: quote the version and digest with every number                                                                                                                                                                                |
| Instructions from this deployment's operator           | `prompt.instructions_file`, when one is configured. Omitted entirely when none is                                                                                                                                                         |

Two of those sections are derived from a list rather than written down, and that is the point. A
document that names an operation a deployment does not mount costs the agent the turns it spends to
discover the absence. A document that lists a metric the bundle does not define costs the agent a
refusal. Neither can happen here, because there is no place for either to come from.

## What it deliberately does not say

**Nothing about composing SQL against the certified surface.** The reference implementation that
this design is modelled on spends most of its length teaching an agent three things: to write SQL
against semantic model names, to avoid raw database tables, and to dry-plan a complex statement
before running it. None of that transfers to `query`. A question there names a metric, a grain, a
bounded period, up to four dimensions, and filters that match or exclude declared values. There is
no field for anything else. So that guidance would teach an agent to attempt something that the
`query` surface refuses by construction. One short section replaces it. The section says the field
does not exist and that there is no way to widen it. A long section about what is absent would hand
an agent a long list of things to try.

**The one deliberate exception.** Where a deployment turned on the off-by-default `run_sql` tool,
the document DOES say something about SQL. It adds one paragraph under "The operations you have".
The paragraph frames the tool as ungoverned. The tool runs under the deployment's own role, never
the caller's, and its result carries no provenance of the kind a `query` answer carries. The
document never frames the tool as certified, in either direction. "What this surface has no field
for" gains one sentence that points at this exception rather than staying silent about it. Silence
would leave the document internally inconsistent: it would say "there is no way in" and separately
advertise `run_sql`. That is worse than either sentence alone.

**No measure expression.** Of what a metric *is*, the document renders what `GET /v1/catalog`
renders, and no further field. A caller needs a metric's name, prose, grains, dimensions and
permitted values to ask a valid question. It needs no column name for that, and a column name in an
agent's context is a name the agent will eventually try to use. By default no model,
table or column name reaches the output. An operator can enable
`prompt.list_physical_schema` to list descriptive model, table and column metadata. That listing
does not make a physical name queryable, and it does not certify a metric.

The document carries four sections for which the catalog
body has no field at all, listed in the table above: the glossary, the terms recorded as not
defined, what this deployment records about its own definitions, and the worked questions. It also
carries any caveat, under the metric it is about. All five are knowledge from the pinned bundle, and
all are descriptive. A glossary entry may mean a declared model or one of its columns. It then names
that model or column, whether or not `prompt.list_physical_schema` is on. The model's audience
decides who sees it.

**Nothing about identity.** There is none. The bearer token authenticates the *deployment*, not the
caller - see [Serving over HTTP](serving.md).

## Configuration

There are four keys. They sit in the same layered tree as every other key: embedded defaults, then
`base.yaml`, then `<environment>.yaml`, then one environment variable per key.

| Key                             | Default  | What it does                                                                                                                                                                                                                  |
| ------------------------------- | -------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `prompt.catalog_prose`          | `quoted` | `quoted` includes each metric's own prose, with `>` at the start of every line and the trust boundary named above the block. `omitted` leaves it out, and the document then says the descriptions exist and were not included |
| `prompt.instructions_file`      | absent   | A path to markdown that is appended as the document's LAST section. Absent means no operator section at all                                                                                                                   |
| `prompt.instructions_max_bytes` | `32768`  | Maximum operator instruction bytes read at startup, configurable up to 1 MiB                                                                                                                                                  |
| `prompt.list_physical_schema`   | `false`  | List models, tables and columns on operator-side prompts and caller-scoped catalog tool replies                                                                                                                               |

```bash
SUTURA__PROMPT__CATALOG_PROSE=omitted \
SUTURA__PROMPT__INSTRUCTIONS_FILE=prompts/house-rules.md \
  sutura prompt ./catalog ./config
```

`sutura prompt` takes the deployment's configuration directory as its optional second argument. The
text it renders is therefore the operator-side whole-bundle preview. The served MCP `initialize`
prompt renders the same optional physical listing, cut to the verified caller's own `ScopedView`.
`describe_catalog` renders the same per-request view, so the two agree. The command loads settings
the way the service does, refusals included. A production configuration with no access token will
not render a document either, and the refusal names the key to fix. That is deliberate: a second,
weaker door into the settings is a door that can disagree with the first.

Three properties of these keys are decisions rather than accidents.

**An operator can add to the document and cannot replace it.** `prompt.instructions_file` is layered
on top of the derived text and appended last, under a heading that says so. No key substitutes for
the derived part, because the derived part carries the refusal guidance. A key whose worst setting
silently deletes that guidance has an invisible failure mode. Last place rather than first is
deliberate too. A preamble ahead of the rules reads as the governing frame, and the governing frame
is not the operator's to set.

**A configured instructions file that cannot be read is an error, not a missing section.** The
reference implementation reads `<project>/instructions.md` when the file exists and silently omits
the section when it does not. That is right for a *convention*: no file means nobody wrote one. Here
the operator wrote the path down, so absence means the operator's rules are missing from a document
that claims to carry them.

**Catalog prose and the instruction path do not default by environment.** `telemetry.format` and
`api.docs` do. They record whether somebody wrote the value down, so the startup log can tell a
decision from a default. Neither decision here is a function of the environment. Whether a catalog's
authors are trusted enough to quote their prose into an agent's context is a fact about who writes
the catalog. It is not a fact about whether the process runs on a laptop. A default that drops the
prose in production would also be worse than either fixed answer. An agent with no descriptions does
not stop: it infers a metric's meaning from its name and reports the inference. And the interesting
value, `omitted`, is never a default. So a deployment that runs with it is visible from the value
itself.

## Catalog prose is untrusted content

Whoever authored the catalog writes a metric's description. This repository's threat model treats
catalog content as untrusted. A description that contains a sentence aimed at the agent rather than
at a human is prompt injection through the catalog. Three measures address this.

**A delimiter cannot separate instruction from data, because the content can contain the
delimiter.** [Concepts](concepts.md#provenance) already says
so. So the mitigation is not a fence. It is a per-line prefix that sutura applies: sutura emits
every line of prose with `>` in front of it. **No line of catalog text can reach the document at
column zero.** Catalog text cannot emit a heading, close a block, or open something that reads as a
new section.

**The trust boundary is named in the text**, immediately above the quoted block, in terms an agent
can act on. The block is data. A sentence inside it that reads as an instruction is content, not an
instruction. Encountering one is something to report, not to obey.

**An operator who does not trust their catalog authors can drop the prose entirely.** The setting is
`prompt.catalog_prose: omitted`. It applies to the deployment, not only to this document: the agent
tool's `describe_catalog` and the HTTP `GET /v1/catalog` body honour the same setting. So no second
path carries a description that the deployment declined to render to a reader. The body says which
way the setting points, so an absent description is a fact and not an empty catalog.

Under the default setting, metric and dimension descriptions reach any token-holder through
`GET /v1/catalog`, so the document adds framing, not reach. Under `omitted`, neither the document
nor `GET /v1/catalog` carries them. That makes the setting a decision about the deployment, not a
preference about one document.
