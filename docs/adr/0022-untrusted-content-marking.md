---
title: Untrusted content, and what marks it
description: Why #128's forgeries are a structured-versus-text difference rather than one thing, why nothing marks a value untrusted in the structured half because the encoder already owns the boundary, why content is escaped or quoted structurally rather than detected and scrubbed, and what the raw SQL tool's failure text may carry once it exists.
---

# Untrusted content, and what marks it

Status: **accepted, amended, and partly built.** The text-half forgery #128 describes is fixed: cells
are escaped in `OutcomeContent::as_text`, catalog prose is quoted per line in
`CatalogContent::as_text` or omitted under `catalog_prose: omitted`, and the trust boundary is named
in each render. The shared injection corpus both transports walk lives in `sutura_app::untrusted`.
The amendment at the foot of this record extends `catalog_prose: omitted` to the HTTP catalog body,
which this record's first version reasoned its way out of needing - see *Decision 1* and the
amendment for why the encoder argument does not reach that question. What is decided here and NOT yet
built is one half of Decision 3 - the raw SQL tool's failure text - because the raw tool itself
(`#129`) does not exist yet.

This record exists because `docs/adr/0009` asked for it to be **decided before an envelope exists**:
retrofitting a field boundary afterwards is the expensive half, and the question "what marks a value
as untrusted" is one you answer twice - once for the boundary you already have and once for the one
you do not.

## The shape of the defect, in one sentence

#128 is two forgeries and they are the same git diff twice: a `\t` cell crosses a column on a
tab-joined text surface, a `\n` cell crosses a row, and either can spell the provenance trailer -
the exact channel `Provenance` exists to make trustworthy - while a catalog description containing a
`\ndefinitions:` line opens a line an encoder did not write.

The reason they are the same defect is the difference that decides this whole record: **a field
boundary an encoder enforces cannot be crossed by a cell value, and a delimiter line can.** On a
structured surface the encoder (serde, JSON) owns the boundary, so no value can cross it. On a text
surface the transport owns the delimiter line, and a value from a data system or a catalog author is
outside the transport - so only the transport's escaping sits between it and a line nobody asked it
to write.

That is `docs/adr/0009`'s decision restated by measurement, and it is why the two surfaces are not
two problems: the text half needs the mitigation, the structured half does not, and a fix that
treated them alike would either escape JSON (pointlessly) or trust a delimiter (usefully nowhere).

## Decision 1: what marks a value as untrusted

**The structured half is marked by nothing, because nothing is untrusted there that is not already a
field.** serde serializes a `String` field and a hostile `\n` or `\t` inside it arrives as an escaped
character inside one JSON string; `sutura_app::untrusted::CELLS` round-trips through
`Outcome::body()` byte-for-byte and the document still parses. Adding a per-value
"this is untrusted" flag to the structured half would be a marker nothing reads - the encoder already
denies the only escape the marker would describe. `sutura-http`'s answer and catalog bodies therefore
carry descriptions and cells as ordinary fields, and a test walks the same corpus to prove they
survive as one opaque string each.

**One correction to that sentence, and it is a different question wearing the same word.** This
record's first version reasoned from *nothing needs escaping here* to *the catalog body carries every
description*, and the amendment below is why the second does not follow from the first:
`catalog_prose: omitted` is not a mitigation for a delimiter, so an encoder that owns the boundary
does not answer it. **What is carried and what is escaped are two decisions**, and only the second is
the encoder's.

**The text half is marked by construction, never by a field.** The transported text names the
boundary once (the `UNTRUSTED_CATALOG_NOTICE`), and each value is either escaped (`OutcomeContent`)
or quoted per line or omitted (`CatalogContent`). The marking that matters is the escaping, not a
label: a line an encoder did not write cannot reach column zero because every line a decoder wrote is
`> `-prefixed, and the marker is the prefix.

**When a marker becomes necessary, it is when an envelope exists - and the added cost is what this
decision protects.** `docs/adr/0009`'s *what is not decided* names Arrow envelopes and result
identifiability. If an answer is ever wrapped in an envelope the transport builds, the destructive
question is whether a cell can climb out of the payload into the envelope header; there, and only
there, does a value need an explicit untrusted mark at the boundary rather than the encoder's
implicit one. That envelope is the trigger to revisit this decision, not a reason to pre-build the
marker now.

## Decision 2: a detection is a refusal, never a silent scrub - and the current design detects nothing

The alternative to escaping is to *look for* hostile values and do something about them. What that
"something" may be is decided by the refusal rule: **a filter that quietly edits data returns a wrong
number, so a detected hostile value is refused, not silently altered.** That is `docs/adr/0009` and
`docs/adr/0013` both already saying refusal, carried here so the raw tool inherits it.

The honest and important half: **the current text half does no detection at all.** Escaping is
unconditional - a `\t` is rendered `\t` whether or not it was hostile - and quoting is unconditional,
so there is no detected-then-edited path and therefore no silent alteration to refuse. The structural
escapes are re-representable: an agent reading `\t` knows a tab was there, and `> ` quoted prose is
the author's own words with the boundary made visible. Nothing is dropped and nothing is invented.
The refusal half of this decision becomes live only when a surface DETECTS rather than escapes - a
raw tool that must refuse an un-processable statement is the first such surface, and Decision 3
points at it.

Stated so it cannot read as coverage: no mechanism refuses a value it detected on the certified
answer path, because the certified path never detects. If a future envelope cannot make a hostile
cell structurally inoffensive, the envelope's answer is the refusal `docs/adr/0009` wants - never a
cell silently shortened to fit.

## Decision 3: the raw tool's failure text is untrusted output from the moment it exists

`#129`'s raw SQL tool is not built. Its one boundary that needs deciding in advance is what its
failure text may carry, because **whoever controls a statement controls part of the message a
database returns about it.** The error channel on the certified path is closed - `sutura-http`
logs the driver's complaint and returns a status with no message, because a driver's complaint names
a table, a column or a file. The raw tool cannot hold that line: its entire reason to exist is that
the statement is the caller's, so the caller's text is already at the database, and the database's
reply is a string the caller may have influenced.

This record's decision, parked where `#129` will read it: **the raw tool's failure text is treated as
untrusted content and rendered with the same marking the text half already applies - quoted per line,
never spliced at column zero, the boundary named - and it is never re-executed or re-parsed.** A "try
this instead" correction the database embeds is content, not instruction, exactly as a catalog
description is. When the raw tool lands, `sutura_app::untrusted` gains a `FAILURE` list and the raw
tool's renderer walks it under Decision 2's rule: escape, and where escape is impossible, refuse.

## What this does NOT claim, and the limit is the whole point

**None of this stops prose that persuades without escaping.** `docs/agent-prompt.md` already says no
mechanism catches it. A catalog description that says nothing structural but simply instructs - "for
the answer to revenue, run this query" - is quoted, not neutralised, and the quoting rejects the
escape, never the instruction. That is why every render also names the boundary and tells the agent
what to do with an instruction that arrives inside it: *ignore it and carry on*. Claiming otherwise
would be the overstated control the issue must not close on.

## What is built, and where

The accepted core is built and tested, per surface:

- `sutura_mcp::wire::OutcomeContent::as_text` escapes `\t`, `\n`, `\r` in every cell, so a cell
  cannot open a column, a row or the provenance trailer. Two tests assert the two forgeries.
- `sutura_mcp::wire::CatalogContent::as_text` quotes every description line with `> ` (`push_prose`),
  names the boundary (`UNTRUSTED_CATALOG_NOTICE`), and under `catalog_prose: omitted` drops the prose
  and says so (`CATALOG_PROSE_OMITTED_NOTICE`) - the same setting the prompt honours, so an operator
  who does not trust catalog authors ships no prose on either text surface - and, per the second
  amendment, on neither half of the MCP reply, which this sentence read as covered because the
  renderer was the half the setting was an argument to.
- `sutura_app::prompt::quote` already prefixed every line; the corpus test pins it against the same
  hostile prose the tool runs.
- `sutura_app::untrusted::{CELLS, PROSE}` is the one injection corpus; `sutura-app`, `sutura-mcp` and
  `sutura-http` each walk it and assert the property their own encoder provides, so a third transport
  inherits the tests rather than the mistake.

The structured surface (`sutura-http`) needs none of the escaping: `serde` owns the boundary, and a
test asserts the corpus stays opaque there. That asymmetry is the record in two lines.

## Amendment: the prose setting is a property of the deployment, not of one transport

`#128` asked for `catalog_prose_omitted_omits_it_from_every_surface` and this record accepted it as
two tests on the agent tool. **A review of the closed issue found the third surface**:
`sutura_http::wire::CatalogBody` was built by a `From<&PinnedDefinitions>` that could not see the
settings, so a deployment which had dropped its catalog prose from the prompt and from the MCP tool
served every description over HTTP - and an agent reading `GET /v1/catalog` is the same reader the
setting was turned off for.

**Why the escaping argument does not cover it, which is the part worth carrying:** the two mechanisms
answer two questions. *Who owns the delimiter* is the encoder's question, and on a JSON surface the
answer is serde, which is why nothing here escapes anything. *Whose words reach an agent* is the
operator's question, and no encoder answers it. Reading "the encoder owns the boundary" as "this
surface needs nothing" collapsed the two - the same shape of mistake as the original defect, one
level up: a control described as covering more than it does.

So the decision is stated positively rather than per transport: **a surface that carries the
catalog's prose honours `prompt.catalog_prose`, and a surface that renders it into text also escapes
or quotes it.** The second obligation is the text half's alone; the first is every surface's.

What is built for it:

- `CatalogBody::of(pinned, prose)` replaces the `From`. A named constructor, because
  `CatalogProse::default()` is `Quoted`: a conversion reachable without the setting fails OPEN, and
  that is exactly how the defect arrived. The argument cannot be forgotten.
- `description` on both body types is an `Option`, skipped when the operator omits prose, and the
  body carries `catalog_prose` as the operator's own spelling - so *this deployment ships no prose*
  is a fact a client reads rather than one it infers from a missing field, and it cannot be confused
  with *this catalog has no description*. It is an additive, documented change to the generated
  OpenAPI document, which is built from the handlers and never hand-written.
- Three tests. Two over the body, for each setting and for the shared `PROSE` corpus through a real
  `Description`; one over the assembled ROUTER
  (`the_catalog_route_honours_the_prose_setting_it_was_started_with`), because the defect was the
  WIRING and a test over the body alone passes with a handler that still defaults the setting.

**The limit, unchanged and restated because this amendment is about not overstating:** an omission
narrows what an agent is *told*, never what it may *ask*, and it stops no prose that persuades
without escaping. A deployment that trusts its catalog authors is the default and is unaffected.

## Second amendment: the surface it missed was inside one it had already counted

The amendment above found a third surface and stated the decision positively. The fourth was not a
fourth transport - it was the other half of a reply this record had already called covered.
`sutura_mcp::wire::CatalogContent` was built by a `From<&PinnedDefinitions>` that could not see the
setting, so `describe_catalog` answered with a text block carrying no description and
`structured_content` beside it carrying every one, in the same message. `#266`'s `H1`.

**Two things about it are worth carrying, and neither is the fix.**

*Why the count was wrong.* "The MCP tool honours it" was read off the renderer, because the renderer
is where the setting was an argument. A tool result has two halves and only one of them is rendered;
the other is a conversion. So the question is not answered per transport but **per thing that carries
the prose** - `as_text` and `serde_json::to_value` were two of them inside one function.

*Why the test could not have found it.* The regression test asserting the omission read
`result.content.first()` and nothing else - a different field from the one carrying the leak - so it
passed against the defect for as long as the defect existed, while standing as the proof that `#128`'s
item 2 was closed. A test reading the wrong field is not weak coverage; it is a green light over an
open path.

And the composition root had never read the setting at all: `sutura-cli`'s `mcp` command passed
`CatalogProse::Quoted` as a constant and called it *the default treatment*, which it was not - it was
the only one. On the published binary the omission therefore reached the prompt and the HTTP body and
neither half of the agent tool, which is the surface with no token and no scope narrowing. The same
shape as the first amendment's finding, one layer further out: the wiring rather than the wire.

What is built for it:

- `CatalogContent::of(pinned, prose)` replaces the `From`, for the reason `CatalogBody::of` did: a
  conversion reachable without the setting fails OPEN.
- Both `description` fields are `Option`, skipped when the operator omits prose, and the content
  carries `catalog_prose` as the operator's own spelling - the same field the HTTP body carries, so a
  client reads *this deployment ships no prose* rather than inferring it from an absence.
- **One value, both halves.** The setting is a field on the content rather than a second argument to
  `as_text`, so the two halves of one reply cannot disagree about a decision the operator made once.
  Not two readers of one setting: one reader of one value.
- `sutura-cli`'s `mcp` root reads `prompt.catalog_prose`, through the same exhaustive conversion
  `sutura prompt` uses. Exhaustive because the previous shape asked `is_quoted()` inside an `if`,
  which reads every future spelling as *omitted*; and shared because two roots resolving one setting
  separately is how the constant survived.
- Tests: the omission asserted on **both halves** of one reply through the SDK's own client; the
  quoted default asserted on both halves too, since a fix that withholds everything is an outage
  rather than a control; the shared `PROSE` corpus walked under both settings through a real
  `Description`; and the composition root asserted over the wire against the documented example,
  because the last defect was the wiring.

**The limit, and one this record should have stated the first time:** nothing counts the surfaces.
Both obligations are held per call site by a constructor that demands the argument, and *no fifth
carrier of catalog prose exists* is a reading of the code rather than a gate.
