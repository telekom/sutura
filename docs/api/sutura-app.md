<!-- GENERATED FILE - do not edit.
     Written by docs/.tools/rustdoc_to_markdown.py from rustdoc JSON.
     Edit the doc comments in the crate source instead, then regenerate.
     The commands are on the API reference landing page. -->

# sutura-app

The public API of `sutura-app`, rendered from rustdoc JSON.

The service: what happens between a question arriving and an answer leaving.

Generic over the ports and holding no framework types, so it can be exercised against a fake
warehouse in a test and a real one in production without either of them knowing. The composition
root decides which; this crate never names an adapter.

Two entry points, and the order between them is the point:

`verify_and_validate` re-executes every metric that declares a certified number, against the
`Warehouse` it is handed, and hands back the bundle as a `Validated` one only if every anchor
reproduced its number. `answer` takes nothing else. So **a bundle whose anchors were never
checked cannot reach the query path** - not by discipline, and not because a caller was asked to
call the two in order: `Validated` has no other constructor, and the one it has takes a
warehouse and calls it.

That is the correction to what this crate used to claim. The proof used to be
`sutura_domain::pinned::Validated::new(pinned, &report)`, and `AnchorReport::new`,
`AnchorReport::record` and `AnchorCheck::Matched` are all public - so any caller could enumerate
the bundle's anchors, record `Matched` for each without opening a data system, and get a bundle
the service would serve. The golden suite did exactly that. The wrapper attested to the caller's
own assertion and read like proof, which is worse than no wrapper. `verify_anchors` survives
because a report is worth rendering to an operator; what it cannot do any more is mint the proof.

`surface` is those same two entry points with the ports' generic parameters erased, for a
transport whose request handler is a concrete function. It is a *driving* port and it lives here
rather than in a transport crate, which is a correction: it used to be `sutura-http`'s, and a
second transport would have had to depend on the first to reach it. Nothing in that module names
a framework type, so this crate still holds none.

## `enum ServiceError`

```rust
pub enum ServiceError<E>
```

Why the service could not produce an outcome.

Neither variant is a refusal. A refusal is something the caller asked for and may not have; these
are the data system being unreachable and our own bundle or generator being wrong, and offering
either as a refusal would invite a caller to retry a different question forever.

Generic in the warehouse error rather than boxing it, so the adapter that failed keeps its own
typed error all the way out. A `Box<dyn Error>` here would be the same loss of information the
boundary gate bans `anyhow` for, arrived at by a different route.

### Variants

- `Compile`
- `Warehouse`

### Implements

`Debug`, `Display`, `Error`

## `fn answer`

```rust
pub fn answer<W>(definitions: &Validated<sutura_domain::pinned::PinnedDefinitions>, query: &sutura_domain::query::Query, warehouse: &W) -> Answered<W>
```

Answers one question, or says why it will not.

The source check is not a formality. A plan names exactly one data system, and running it against
a different one would answer a question about other data under the same provenance. It is a
refusal rather than an error because it is a governance outcome: this caller cannot have this
question answered here.

## `fn verify_anchors`

```rust
pub fn verify_anchors<W>(pinned: &sutura_domain::pinned::PinnedDefinitions, warehouse: &W) -> sutura_domain::pinned::AnchorReport
```

Re-executes every declared anchor and reports what each produced.

Returns a report rather than a `Result`, because "this one metric no longer computes its number"
and "the data system is down" are both outcomes worth recording per metric. Collapsing either into
a single error would lose which metric, and the whole point is to name it.

## `fn sources`

```rust
pub fn sources(pinned: &sutura_domain::pinned::PinnedDefinitions) -> Vec<&sutura_domain::model::SourceName>
```

The data systems a bundle reads from.

Exposed because a composition root has to decide which adapters to open before it can answer
anything, and reading it off the bundle beats being told twice.

## `fn grains_coarsest_first`

```rust
pub fn grains_coarsest_first(pinned: &sutura_domain::pinned::PinnedDefinitions, metric: &sutura_domain::model::MetricName) -> Vec<sutura_domain::model::Grain>
```

The grains a metric declares, coarsest first.

A small helper the composition root uses to describe a metric, kept here so the ordering is the
same one `verify_anchors` picks a grain by.

## `use None`

## `use None`

## `type_alias Answered`

What answering produced, or why it could not.

A named alias because the inline form is over the complexity threshold in `clippy.toml`, and
naming it is the better half of that trade: the generic parameter is a warehouse, not a result.

## Module `surface`

What a transport needs from this crate, with the ports' generic parameters erased.

# Why this trait exists at all

`crate::answer` is generic over a `Warehouse`, and `Warehouse` has an associated error type.
A transport's request handler is a concrete function - an `axum` handler registered in a route
table by path, and named again by the macro that generates the interface description - so a
handler cannot be generic over the warehouse without the whole router becoming generic in it,
and the generated document becoming generic in it too.

`Surface` is the seam: this crate's two operations, with `W` gone.

# Why it is HERE and not in the transport that uses it

**It used to be in `sutura-http`, and a review was right that that is the wrong crate.** The
module comment there said a second transport - an MCP one - would consume the same trait, which
is exactly the problem: it would have made one transport adapter depend on another, and the rule
this repository is built on is that nothing depends on an adapter. A driving port declared inside
an adapter is a port every other adapter has to reach through that one.

**The tension with "a port arrives WITH its adapter", stated rather than dodged.** That rule
exists because a trait with no implementor is a guess at a signature, and `pub` hides the guess
from `dead_code`. `LocalService` moved with the trait, so the rule still holds: the port and
its only implementor are in one place, and neither is a guess. What the rule does not say is
*which* crate that place has to be.

And there is a sharper reason it is not the transport's. A **driven** port - `Warehouse`,
`SemanticCatalog` - is dependency inversion: the interior declares what it needs, an adapter
outside implements it, and the trait has to sit inside the hexagon or the direction reverses. A
**driving** port inverts nothing. The caller is already outside and the implementation is already
the application, so there is no adapter for it to arrive with: `LocalService` is not an adapter
at all, it is this crate's own service with one generic parameter erased. It holds a
`Validated` bundle and a warehouse, and every method forwards. Putting that in a transport
crate made the application's interface the property of one of its callers.

**Is the erasure an application concern or an HTTP one?** The *trigger* is an HTTP fact: a
handler is a concrete function. The *content* is not - `definitions` and `answer` are this
crate's own two operations, and `LocalService::start` is `crate::verify_and_validate` with
the catalog port consumed. Nothing in this file names a framework type, which is checkable
rather than asserted: `cargo xtask check-boundaries` fails on a framework anywhere in a tree it
governs, and this file added no dependency to this crate's manifest. A shape the application can
offer for free, that its callers all need, belongs with the application.

**Why not delete the trait instead, under YAGNI?** That was the other option offered, and it does
not survive being tried. Deleting a one-implementation trait leaves the concrete
`LocalService<W>`, and then a transport either becomes generic in `W` - the router, its state and
the generated document with it, which is what this trait exists to prevent - or erases `W` inside
`LocalService`, which is the same trait under another name one crate lower. So the trait is not
speculative generality; it is the only shape that keeps a handler concrete with one transport,
let alone two. What WAS speculative is the sentence about MCP, and that sentence is what made the
location wrong rather than the trait. Deleting the trait would have answered a different finding.

# Why the methods are synchronous

Because `Warehouse` is. The port takes `&self` and returns a `Result`, and the engine behind
it drives its own single-threaded runtime and blocks on it. Calling that from inside an `async`
handler on a worker thread would panic - a runtime cannot be entered from within a runtime - so a
transport moves the call onto a blocking pool. Making this trait `async` would hide that
requirement behind a signature that looks like it had been dealt with.

# Where the typed error goes

Erasing the generic means the adapter's own error type cannot survive *as a named type*. It does
survive as an error: each variant of `SurfaceFailure` keeps the cause it was built from as an
owned `#[source]`, so `Error::source()` walks the whole chain and a caller that knows what
adapter is behind the port can still `downcast_ref` to it.

**The previous shape was prose.** Both variants held a `String` message and a `Vec<String>` of
causes, walked to text at construction - so `source()` returned `None`, downcasting was
impossible, and the only thing a caller could do with the failure was print it. A vector of
display strings is a presentation of an error, not an error API. Flattening to text is still what
happens, but it happens at the *logging sink* - `cause_chain`, called by whoever writes the
line - which is the one place where text is the point.

`Box<dyn Error + Send + Sync>` and not `Box<dyn Error>`: a transport that answers on a blocking
pool sends the error back across a thread boundary. That is where the `W::Error: Send + Sync`
bound on the implementations below comes from. It is a bound on *this* type and not on the port,
so `sutura_domain::warehouse::Warehouse` is unchanged, and it is a `std` marker rather than a
framework type - a requirement a transport states, satisfied here.

### `trait Surface`

```rust
pub trait Surface
```

The service, as a transport sees it.

`Send + Sync + 'static` because it is shared between connections and moved onto a blocking pool.
A transport holds it behind an `Arc` in its request state.

### `enum SurfaceFailure`

```rust
pub enum SurfaceFailure
```

Something went wrong that is not a refusal.

Neither variant is something a caller can fix by asking differently, which is why neither is a
refusal: one is our own bundle or generator being wrong, and the other is the data system not
answering.

The variants are exhaustive and stay exhaustive - a transport chooses its status code from the
split - and each one carries the cause it was built from rather than a rendering of it.

#### Variants

- `Compile`
- `Warehouse`

#### Implements

`Debug`, `Display`, `Error`

### `enum ServiceNotStarted`

```rust
pub enum ServiceNotStarted
```

Why a service could not be started.

#### Variants

- `Catalog` - The catalog adapter could not produce a bundle.
- `NotValidated` - The bundle loaded and an anchor did not reproduce the number its author certified, or could not be run at all.

#### Implements

`Debug`, `Display`, `Error`

### `struct LocalService`

```rust
pub struct LocalService<W>
```

The one implementation: a validated bundle and one data system, behind the ports.

Holds the bundle as `Validated`, which has no constructor other than one that executes every
anchor against a warehouse - so a `LocalService` that exists is one whose anchors held. That
is not a check this type performs; it is a type it could not otherwise have been built from.

#### Methods

```rust
pub fn start<C>(catalog: &C, warehouse: W) -> Result<Self, ServiceNotStarted>
```

Loads a catalog through its port, re-runs every anchor against `warehouse`, and returns a
service only if all of them held.

Both ports are consumed here, which is what lets a transport be transport-only: it never
reads a catalog directory and never opens a data system.

`C::Error: Send + Sync` for the same reason `W::Error` is - the cause is kept, owned, and a
startup failure is reported from wherever the composition root happens to be.

#### Implements

`Debug`, `Surface`

### `fn cause_chain`

```rust
pub fn cause_chain(error: &dyn core::error::Error + 'static) -> Vec<String>
```

Every cause beneath `error`, outermost first.

**A logging concern, and it lives here because this is where the erasure happens.** `Display` on
a `thiserror` enum prints the outermost message only, so a line that formatted the error would
say "the data system did not answer" and drop the driver's own complaint - which is the half that
names the table, the column or the file. This is called by whoever writes the line, not by
whoever builds the error, which is the difference between an error that can be inspected and one
that has already been turned into prose.

### `type_alias ErasedCause`

A typed error, owned, with its type erased and its `#[source]` chain intact.

`Send + Sync` because a transport may answer on a blocking pool, so a failure crosses a thread
boundary on the way back.

## Module `prompt`

The agent-facing system prompt: derived from the tool surface, never written by hand.

# Why this exists at all

An agent that has not been told what this surface is will treat it as a database. It will look
for a field to put SQL in, find none, put a metric name it remembers from somewhere else into
`Query::metric`, get a refusal, read the refusal as a transport
failure, and retry. Every one of those steps is a reasonable thing for a general-purpose agent to
do, and every one of them is the behaviour this repository's types are arranged to prevent. The
types stop the *damage*; they cannot stop the loop. A prompt can.

# What it is derived from, and what that buys

Three inputs, and the first two are not text somebody keeps in step by hand:

1. **The tool list** - `PromptInputs::tools`. The workflow is composed from the operations that
   are actually exposed, so a deployment that does not expose the catalog listing gets a prompt
   that does not tell an agent to call it. A prompt naming an operation that is not there is
   worse than a shorter prompt: the agent spends its turns discovering the absence.
2. **The pinned bundle** - every metric, its grains, its dimensions and the values a filter may
   use, read off the same `PinnedDefinitions` every answer is computed from. It cannot describe
   a metric this deployment does not serve, because there is nowhere for such a metric to come
   from.
3. **The operator's own text** - `PromptInputs::instructions`, appended as the last section.
   Appended and never substituted: see the note on layering below.

# What it deliberately does NOT say

**Nothing about composing SQL.** The reference implementation this is modelled on spends most of
its length teaching an agent to write SQL against model names, to avoid raw database tables, and
to dry-plan a complex statement before running it. None of that transfers, because
`Query` has no field for SQL, a table, a filter expression or a
list of row ids and `deny_unknown_fields` makes an attempt an error naming the field. Repeating
the guidance here would teach an agent to attempt something the surface refuses by construction,
which costs a turn and teaches it the wrong model of what it is talking to. What replaces it is
one short section saying the field does not exist and that there is no way to widen it.

**No column, no table, no model and no measure expression.** This is the same content
`GET /v1/catalog` already returns and deliberately not one field more: a metric's name, its
prose, its grains, its dimensions and their permitted values. A caller needs those to ask a valid
question; it needs no column name to do it, and a column name in an agent's context is a name it
will eventually try to use. `tests` asserts that no model name, table name or column name from
the bundle appears in the output.

**Nothing about identity.** There is none - the deployment token authenticates the deployment and
not the caller - and a prompt that mentioned per-caller scoping would describe a control that does
not exist. `AGENTS.md` and `SECURITY.md` record why.

# Tone

The defaults are deliberately strong, and that is borrowed rather than invented: the reference
implementation measured soft phrasing - "for non-trivial questions", "when useful" - being read
as "skip" almost every time. So the workflow says "every time" and the refusal section says "do
not retry" rather than "consider whether to retry".

# Layering, and why an operator cannot replace the derived part

`PromptInputs::instructions` is appended as the LAST section, under a heading that says it is
the operator's. There is deliberately no way to substitute it for the derived text. The refusal
guidance is the single most load-bearing paragraph in the whole document - an agent that treats a
refusal as an outage retries until something works, which is precisely what the refusal exists to
prevent - and a configuration key that could delete it would be a key whose worst setting is
silent. Last place rather than first is also deliberate: a preamble ahead of the rules reads as
the governing frame, and the governing frame is not the operator's to set.

# Catalog prose is untrusted content

A metric's description is written by whoever authored the catalog, and this repository's threat
model treats catalog content as untrusted - see `SECURITY.md`, and the symlink-bounded walk in
`sutura-catalog-local` for the precedent. A description containing a sentence aimed at the agent
rather than at a human is prompt injection through the catalog.

Three things are done about it, and the first is the honest limit:

* **A delimiter cannot separate instruction from data, because the content can contain the
  delimiter.** `docs/concepts.md` already says so. So the mitigation here is not a fence: it is a
  per-line prefix that WE apply. `quote` puts `> ` at the start of every line of prose, so no
  line of catalog text can reach the output at column zero. It cannot emit a heading, close a
  block, or start what looks like a new section of this document.
* **The trust boundary is named in the text**, immediately above the quoted block, in terms an
  agent can act on: the block is data, a sentence inside it that reads as an instruction is
  content and not an instruction, and encountering one is something to report rather than obey.
* **An operator who does not trust their catalog authors can drop the prose entirely** -
  `CatalogProse::Omitted`. The section then says the descriptions exist and are not included,
  which is a fact an agent can act on, rather than silently rendering a catalog with no meaning
  attached to any metric.

What none of that solves is prose that *persuades* without escaping. No mechanism here can catch
it. What bounds it is that a catalog is reviewed, authored content whose digest moves when a
description changes, and that this text is generated by an operator command rather than pasted
from a caller.

### `enum Tool`

```rust
pub enum Tool
```

One operation a transport exposes.

Two variants, because [`Surface`](crate::surface::Surface) has two methods and this enum is the
prompt's name for each. It is a list rather than a constant because the point is that a caller
passes the subset it actually mounts: `Tool::ALL` is what a transport serving the whole surface
passes, and a deployment that mounts only one passes only that one.

**Two entries make this cheap insurance rather than a large win, and it is worth saying so.** The
property it buys is narrow: the rendered workflow cannot instruct an agent to call an operation
that is not there. With two operations that is one branch. It is here because the branch costs a
match arm and the alternative - a hand-written workflow that is right until the day a deployment
stops mounting the listing - costs a debugging session.

#### Variants

- `Catalog` - Reading what this deployment defines. `GET /v1/catalog`, `sutura catalog`, and whatever an MCP transport would call it. [`Surface::definitions`](crate::surface::Surface::definitions).
- `Query` - Asking one certified question. [`Surface::answer`](crate::surface::Surface::answer).

#### Methods

```rust
pub const fn name(self) -> &'static str
```

The one name this operation answers to.

One word, chosen so that it is simultaneously the CLI subcommand, the path segment under the
version prefix, and what an MCP tool would be called. Three spellings that cannot disagree
beats a table mapping between them.

```rust
pub const fn summary(self) -> &'static str
```

What it does, in one line, for the operations list.

#### Implements

`Clone`, `Copy`, `Debug`, `Eq`, `Ord`, `PartialEq`, `PartialOrd`

### `enum CatalogProse`

```rust
pub enum CatalogProse
```

Whether the catalog's own prose reaches the agent, and how.

A separate type from the configuration spelling of the same decision, which lives in
`sutura_config::prompt::CatalogProse`. The split is the one `telemetry.filter` already uses: the
configuration crate parses the word an operator wrote, and the crate that does the work owns the
type that does it. It also keeps this crate's dependency table at `sutura-domain`,
`sutura-semantic` and `thiserror`, which is what `AGENTS.md` cites as holding up the rule that a
driving port is not owned by one of its callers.

#### Variants

- `Quoted` - Included, with `> ` at the start of every line and the trust boundary named above it.
- `Omitted` - Left out. The section says the descriptions exist and were not included, which is a fact an agent can act on; silence would leave it guessing at what a metric means.

#### Methods

```rust
pub const fn is_quoted(self) -> bool
```

#### Implements

`Clone`, `Copy`, `Debug`, `Eq`, `PartialEq`

### `struct PromptInputs`

```rust
pub struct PromptInputs<'a>
```

Everything the prompt is derived from that is not the bundle.

A value rather than four arguments, so a caller that gains a fifth input does not silently get
the wrong one in the third position.

#### Methods

```rust
pub const fn instructions(&self) -> Option<&'a str>
```

```rust
pub const fn new(tools: &'a [Tool], prose: CatalogProse, instructions: Option<&'a str>) -> Self
```

The operations exposed, how catalog prose is treated, and the operator's own text.

`instructions` is already-read text rather than a path, deliberately: this crate performs no
I/O and the composition root is where a configured file that cannot be read has to become a
loud failure. A configured-and-missing file silently omitted would be exactly the failure
`AGENTS.md` warns about in another place - a control that reads as being in place.

```rust
pub const fn prose(&self) -> CatalogProse
```

```rust
pub const fn tools(&self) -> &'a [Tool]
```

#### Implements

`Clone`, `Copy`, `Debug`

### `fn render`

```rust
pub fn render(pinned: &sutura_domain::pinned::PinnedDefinitions, inputs: &PromptInputs<'_>) -> String
```

The whole prompt, as markdown.

Deterministic in its inputs: every collection walked here is a `BTreeMap` or a `BTreeSet`, and
the grains are sorted explicitly. Two calls with the same bundle produce the same bytes, which is
what lets the rendering be pinned by a snapshot rather than described.
