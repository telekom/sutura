<!-- GENERATED FILE - do not edit.
     Written by docs/.tools/rustdoc_to_markdown.py from rustdoc JSON.
     Edit the doc comments in the crate source instead, then regenerate.
     The commands are on the API reference landing page. -->

# sutura-domain

The public API of `sutura-domain`, rendered from rustdoc JSON.

The hexagon's interior: the types the business rules are written in, and the port traits it
names its dependencies by.

Nothing here may depend on a framework: no async runtime, no web server, no query engine.
`cargo xtask check-boundaries` enforces it over the whole transitive tree, because the rule
is worth more as a check than as a sentence in a design document. The allowlist is `serde` and
`thiserror` and their proc-macro support, plus the `serde_json` and `sha2` that the definition
digest needs, and nothing else - which is why there is a hand-written calendar in `calendar`
and no SQL parser anywhere in this crate, `expression` included.

**Four ports live here now, and each arrived with the adapter that implements it.** A port exists
to invert a dependency on something outside the hexagon, so a trait with no implementor is a
guess at a signature that only the first real adapter can settle, and in a library crate `pub`
hides such a guess from `dead_code`. `pinned::SemanticCatalog` arrived with the local catalog
adapter, `warehouse::Warehouse` with the `DuckDB` one, `audit::AuditSink` with the structured
writer in `sutura-runtime` - the sink a deployment that attaches nothing else gets - and
`identity::CredentialBroker` with `sutura_config::StaticCredentialBroker`, which is in the
settings crate because the identity provider it reads *is* the settings tree.

**What the credential port did and did not buy, said here because the count above invites the
wrong reading.** There is no longer a signature that reaches a data system with a question and no
credential, and a subject with no credential at a source is refused rather than answered as the
process. What is absent is the other end: no adapter in this build has anywhere for a per-subject
credential to arrive, so a leg runs under the identity an operator declared for that source and
`pinned::Provenance` records which. The one method that still executes with no credential is
`warehouse::Warehouse::verify_anchor`, the boot path's; `clippy.toml` bans it everywhere else and
`plan::AnchorPlan` says plainly that it is a self-check on that path rather than a barrier.

The modules are grouped by concept rather than named after traits, so a port sits next to the
types it speaks in:

- `model` and `calendar` are the vocabulary: names, closed sets, dates.
- `measure` is what a metric measures, as a closed vocabulary of shapes rather than an
  expression language.
- `expression` is the escape hatch beside it: SQL a catalog author wrote, for the metrics that
  vocabulary cannot say. It holds no parser - `sutura_sql` compiles a fragment at load - and
  `expression::Computation` is what makes "this metric is authored SQL" a word rather than an
  absence.
- `plan` is what we decided to execute, and the artifact the execution port speaks in.
- `federation` is how a measure survives being computed in pieces: which aggregates descend
  into a leg, which one descends decomposed, and which needs its rows pulled up. Nothing executes
  it yet - there is no splitter and no combiner - so it is a classification with no production
  caller, and its own header says so.
- `catalog` is what a catalog says, and where its cross-references are checked.
- `knowledge` is what a catalog says ABOUT what it defines - the glossary, the caveats, the
  terms deliberately left undefined, the worked questions - checked against a `catalog` and read
  by nothing but the agent-facing prompt. It is separate from `catalog` because the compiler
  must not be able to reach it: descriptive content that could select what executes would not be
  descriptive content.
- `pinned` is the hashed snapshot a question resolves against, plus the catalog port.
- `query` is the tool surface, defined mostly by what it has no field for.
- `warehouse` is the execution port. It speaks in plans, so an adapter that executes without
  generating any SQL is a first-class implementation of it rather than a special case.
- `source` is what a deployment declares about one source: which identity a query reaches it as,
  which identity re-ran its anchors at boot, and - separately, because a different party declares
  it - whether the linked adapter can carry a per-subject credential at all. It also holds the
  per-leg execution record `pinned::Provenance` carries, which is read off what the adapter was
  handed rather than off a settings tree.
- `definitions` and `identity` hold the digest and the credential-shaped newtypes. The
  principal chain a call is attributed to lives in `identity` as well, beside the redaction and
  the credential port, because all three are properties of who is asking rather than of what was
  asked.
- `audit` is the record one call is written to, and the port it goes through. It is not a
  store: sutura writes a record before the outcome returns and retains nothing, so what the
  sink does with it is the deployment's.

One module is private, and it is the only one: `text` holds the set of invisible and
direction-changing code points that a phrase, a note body, a version label and an authored SQL
fragment all refuse. It exists because that set was written down twice, in two files, and the two
had already drifted.

## Module `audit`

The record one call is written to, and the port it is written through.

# Why this is sutura's job and cannot be delegated downstream

`docs/adr/0008` walks every identity model a data system offers and finds that only one of them
can express "an agent acting for a human" in the session itself. The token exchange this design
uses issues an *impersonation* token rather than a *delegation* one - there is no `act` claim to
carry - so the source's own audit log says "this person" and cannot say "sutura, for this
person". The principal chain therefore exists nowhere downstream, and a record of it has to be
written here or nowhere.

# Written before the outcome returns, and retained not at all

Two claims, and they answer different questions.

**Written.** One record per call, refusals included, before the outcome goes back to the caller.
Before rather than after, because a record written after the response is the record a crash
loses, and the call worth having a record of is the one that went wrong. It is a different
channel from `crate::pinned::Provenance`: provenance rides on the result and a client is free
to drop it, and a record only the caller holds is not a record.

**Retained nothing.** No archive, no rotation, no retention window, no query interface over past
calls, and no obligation inherited from any of those. Everything after the write belongs to the
deployment: where the records go, how long they are kept, who may read them.

**The limit, next to the claim.** An emitted record is worth what the sink behind it is worth,
and sutura cannot vouch for a sink it does not retain. A deployment whose sink drops records has
no audit trail on this side and nothing here can tell it so - which is why the sources' own logs,
written under the asking subject, carry the part of the obligation that matters.

# The incident question, and which half of it this record can answer

`docs/adr/0008` fixes the full content as the chain, the outcome, **the sources the plan read and
the posture each leg ran under, and the expiry the credentials carried.**

**The posture is reachable, and it was not named until a review asked the question it exists for.**
A verified subject's question can be answered under the *deployment's* own identity on a source
declared `shared-service-user` - that is honest, acknowledged and not impersonation - and the
incident question is then "whose access filtered these rows". The answer is
`crate::source::ExecutedAs`, which rides on the `Provenance` an answer carries, and
`CallRecord::executed_as` is the accessor: a sink writing an audit line does not have to know
that provenance transitively holds it. It answers `None` for a refusal, because nothing executed.

**`asked_by` is the chain, and it is deliberately not a second field - and that sentence used to
be an assumption rather than a fact.** `LegCredentials::asked_by` is the broker's copy of who
asked and the chain is the transport's. This paragraph said they *agree by construction, because
`mint` reads the request context* - and a review pointed out that this is a claim about what a
well-behaved broker does, not a property of the types: `LegCredentials::minted` is `pub` and takes
any `crate::identity::Subject`, so a broker that returned somebody else's grant would have
executed under one principal and been recorded under another. **It is a fact now**, because
`sutura_app::answer` compares the two and refuses a disagreement before anything reaches an
adapter - see `crate::identity::Minted::agreeing_with`. So the reason there is one field stands,
and what makes it safe is a check somebody can point at rather than a habit brokers are trusted to
have.

**The expiry IS here**, as `CallRecord::executed_until`, and it arrived for the same reason the
posture did: a review found the value carried and read by nobody. It is enforced first - a
credential whose deadline has passed never reaches an adapter - and recorded second, so an
incident can ask how much life the credential that read these rows had left. `None` means nothing
was minted for this call, which is the honest answer for a question refused before the broker was
asked.

A plan reads from the sources a `Compiled::Federated` answer spans, or from a single source for a
`Compiled::Planned` one (a question spanning three or more is refused at plan time by
`crate::query::RefusalReason::PlanSpansTooManySources`), so the source set is a name a reader
already has from the bundle.

### `trait AuditSink`

```rust
pub trait AuditSink
```

Where a record of one call goes.

# Returns nothing a caller can branch on

Deliberately. A sink that could refuse would make writing the record a step the query path has
to decide about - continue without a record, or refuse the question - and both answers are worse
than the question. Continuing silently is the failure this port exists to prevent; refusing a
question because a log pipeline is unwell is an availability decision nobody asked for. So the
port takes the record and owns everything that happens to it, including failing, which is the
deployment's half of the bargain the module header states.

`&self` rather than `&mut self`, so one sink is shared by every request without a lock in the
port's signature. Synchronous, because the ports either side of it are: the interior names no
framework, and a transport that answers on a blocking pool is already off the reactor.

# Its first implementor

`sutura_runtime::TracingAuditSink`, a structured writer over the tracing subscriber this
repository already composes. It needs nothing from anybody, which is what makes it the sink a
deployment that attaches nothing else gets - and what keeps this trait from being a guess at a
signature.

### `struct CallRecord`

```rust
pub struct CallRecord<'a>
```

What one call is recorded as.

Borrows rather than owns: it is built at the call site, handed to the sink, and dropped. A sink
that needs to keep something copies what it needs, which is the sink's decision rather than a
cost this type imposes on every call.

#### Methods

```rust
pub const fn chain(&self) -> &PrincipalChain
```

Who the call is attributable to.

```rust
pub const fn executed_as(&self) -> Option<&ExecutedAs>
```

Which identity produced each leg, where anything executed.

**The incident question's own accessor**, and it is derived rather than stored: the value is
the `Provenance`'s, which the `ToolOutcome` already carried, so there is one place it lives
and nothing here can describe a leg as impersonated that ran shared. `None` on a refusal, and
that is a case a reader names rather than an absence to interpret - a refused question reached
no data system, so there is no identity it ran as.

**Recording is not a control**, the way `Provenance`'s own documentation says: this reaches a
sink after the rows were read. What it is for is being able to answer, afterwards, whether a
verified subject's question was filtered by that subject's own access or by the identity this
deployment holds for the source.

```rust
pub const fn executed_until(&self) -> Option<Expiry>
```

How long the credentials this call ran under were good for.

**A field rather than a derivation, unlike `Self::executed_as`**, because the deadline is
deliberately absent from the `ToolOutcome`: the outcome's provenance is caller-facing, and a
credential's lifetime is this deployment's business rather than the asker's.

Three states and a reader has to name all three, which is why it is an `Option<Expiry>` rather
than a number: nothing was minted for this call, the credential does not expire, or it expires
at an instant. The first is the honest answer for a question declined before the broker was
asked - `sutura_app`'s own suite pins that a refused question never reaches it - and the second
is what every credential the shipping broker mints answers, because it mints from a file.

**Recording is not the control.** A credential whose deadline had passed never reached an
adapter, and `crate::identity::Minted::agreeing_with` is where that is decided; this is what
lets an incident ask afterwards how much life was left.

```rust
pub fn of(chain: &'a PrincipalChain, outcome: &'a ToolOutcome, executed_until: Option<Expiry>) -> Self
```

The only constructor, and it derives the outcome half from the outcome itself.

There is no way to build a record that describes an answer as a refusal or the other way
round: the match is here, once, rather than at every call site that would otherwise be
trusted to get it right. That is the same reason `crate::pinned::PinnedDefinitions::pin`
takes no digest parameter.
`executed_until` is the deadline the credentials this call ran under carried, and `None` means
nothing was minted for it - a question refused before the broker was asked. It is a parameter
rather than something derived from the outcome because the credential is deliberately not on
the `ToolOutcome`: provenance rides to the caller, and what a credential's lifetime is is
not the caller's business.

```rust
pub const fn outcome(&self) -> &RecordedOutcome<'a>
```

How it ended.

#### Implements

`Debug`

### `enum RecordedOutcome`

```rust
pub enum RecordedOutcome<'a>
```

How the call ended, as the two outcomes a question has.

**A refusal is a variant here for the same reason it is one in `ToolOutcome`**, and it is the
half a log line gets wrong by omission: refusals are the demand signal for which questions have
no certified answer, and a channel that records only answers cannot report it.

#### Variants

- `Answered` - The question was answered. The row count sizes it; the provenance says which definitions produced it, so a record can be matched against the bundle that was serving.
- `Refused` - The question was declined. The variant is what a reader needs - not a sentence - because it is what an aggregate over records can group by.

#### Implements

`Debug`

## Module `calendar`

Dates, and the bounded range a question has to carry.

Hand-rolled rather than taken from a date library, and that is a boundary decision rather than
taste: `cargo xtask check-boundaries` holds `sutura-domain` to an allowlist of `serde` and
`thiserror` over the whole transitive tree, so a date crate would have to be argued onto that
list. What is needed here is a calendar date with an ordering and one parser, which is less code
than the argument would be.

No clock, no time zone, no instant. A grain is a calendar concept, and "the month of June" is
not a question about an offset from an epoch. When a time zone becomes necessary it arrives with
the data system that needs one, not before.

### `struct Date`

```rust
pub struct Date
```

A calendar date, with no time and no zone.

Construct it with `Date::parse` or `Date::new`. The fields are private and ordered
year-month-day so the derived `Ord` is chronological: a reordering of the declaration would
silently invert every comparison, which is why the ordering is asserted in a test.

**`try_from` and `into` are a pair, and one without the other was a real asymmetry.** This type
carried `try_from = "String"` alone, and `serde(try_from)` affects `Deserialize` only - so the
derived `Serialize` wrote the STRUCT, and a date this crate serialized was a date this crate's
own `Deserialize` rejected. Two places depend on the two halves agreeing: the digest in
`crate::definitions` is taken over the serialized form, so it has to be taken over the ISO text
a catalog author actually wrote rather than over a field layout that never appears in a file; and
a schema generated from this type describes a wire value the surface accepts as a string. Every
other type here with a canonical text form is written the same way - `TermRepr` in
`crate::measure` pairs them so that what a digest covers and what a catalog wrote are the same
text.

#### Methods

```rust
pub const fn day(self) -> u8
```

```rust
pub fn days_since_epoch(self) -> i32
```

Days since 1970-01-01, which is how a columnar engine stores a date.

The inverse of `Date::from_days_since_epoch`, and the two are asserted to round-trip. It
exists because an in-process engine takes a date as an `i32` day count rather than as text:
there is no statement for a date literal to be written into, so the value is handed over as
the number the column actually holds.

Counted by walking years, for the same reason the inverse does: integer division and the
remainder operator are both banned by the lint table, the loop runs at most a few hundred
times for any date this type can hold, and the leap rule stays in one place.

```rust
pub fn from_days_since_epoch(days: i32) -> Result<Self, InvalidDate>
```

A date from a count of days since 1970-01-01.

Needed because a data system returns a truncated date as a day number, and the alternative
was casting the column to text inside the generated statement, which would bake one dialect's
idea of a date format into every dialect's SQL.

Walks a year at a time rather than dividing. That is not naivety about performance: integer
division and the remainder operator are both banned by the lint table, the loop runs at most a
few hundred times for any date this type can hold, and the leap rule stays in one place
instead of being re-derived as a correction term.

```rust
pub const fn month(self) -> u8
```

```rust
pub fn new(year: i16, month: u8, day: u8) -> Result<Self, InvalidDate>
```

Builds a date, rejecting a day the month does not have.

Parse rather than validate: once this returns `Ok`, nothing downstream re-checks, because the
30th of February is unrepresentable rather than merely unwelcome.

```rust
pub fn parse(raw: impl AsRef<str>) -> Result<Self, InvalidDate>
```

Parses `YYYY-MM-DD`.

The widths are fixed at four-two-two so a short year cannot be read as a long one: `26-06-01`
is rejected rather than becoming the year 26. Fixed widths also mean a leading `-` cannot
reach the number parser, so a negative component is a layout error rather than a date in the
distant past.

**A width alone was not enough, and that was a real hole.** `i16::from_str` and
`u8::from_str` both accept a leading `+`, so `+026-06-01` measured four wide and parsed as
the year 26 - exactly what the fixed width exists to refuse - and `2026-+6-+1` parsed as the
1st of June. Every byte of every component has to be an ASCII digit, so a sign cannot occupy
the column a digit was supposed to be in.

```rust
pub fn to_iso(self) -> String
```

`YYYY-MM-DD`, which is what a bind parameter carries.

```rust
pub const fn year(self) -> i16
```

#### Implements

`Clone`, `Copy`, `Debug`, `Deserialize<'de>`, `Display`, `Eq`, `Hash`, `Ord`, `PartialEq`, `PartialOrd`, `Serialize`

### `enum InvalidDate`

```rust
pub enum InvalidDate
```

Why a date was rejected.

Each variant carries what was wrong as typed fields rather than a formatted sentence, and the
numeric parse failure keeps its cause: a discarded cause is the difference between "the month is
not a number" and knowing which character stopped it.

#### Variants

- `Malformed` - Not `YYYY-MM-DD`. Exactly one layout is accepted, because a parser that guesses between `03-04-2026` and `2026-04-03` guesses wrong for half the world.
- `NotANumber` - A component was not a number at all.
- `YearOutOfRange` - A year outside the range this type accepts, which is 1 to 9999 so the four-digit written form is the whole domain.
- `MonthOutOfRange` - A month outside 1 to 12.
- `NoSuchDay` - A well-formed date that does not exist, such as the 30th of February.

#### Implements

`Debug`, `Display`, `Eq`, `Error`, `PartialEq`

### `struct TimeRange`

```rust
pub struct TimeRange
```

A half-open interval of dates: `start` included, `end` excluded.

**Both endpoints are always present, and that is the whole of what this type promises.** There is
no constructor that omits one, so an unbounded range is unrepresentable rather than refused.

**What it does not promise is that the interval is small.** `[0001-01-01, 9999-12-31)` satisfies
every check here, and a predicate built from it reads the whole table - which is the shape a
manipulated agent asks for. An earlier version of this comment claimed the newtype prevented a
table scan; it prevents an *absent* bound, and nothing more. The size of the interval a *caller*
may ask for is capped where a caller's question is resolved, against
`crate::query::MAX_RANGE_DAYS`, and refused as
`crate::query::RefusalReason::TimeRangeTooLong`.

**The cap is deliberately not on this constructor**, and the reason is who each caller is. This
same type is also a metric's anchor range, authored in a catalog by the person who defines the
metric - not requested by an agent, not on the hot path, and executed once at startup. A catalog
author who wants a decade-long anchor is not the threat the cap exists for, and a hard maximum
here would make a governance decision about requests by constraining authorship. Use
`TimeRange::days` to measure a range; decide what is too long where you know whose range it is.

Half-open rather than inclusive because a month is `[2026-06-01, 2026-07-01)` at every grain and
in every dialect, while an inclusive end needs a different last day per month and per grain. One
of those two conventions produces off-by-one bugs at month boundaries and the other does not.

**No `into` beside the `try_from`, unlike `Date`, and that is not the same omission.** A range's
wire form is a two-field mapping - `start` and `end`, which is how a catalog author writes one -
and both halves already agree on it: `Serialize` derives that mapping and `try_from` reads it back
through `TimeRange::new`. What this type does NOT have is a canonical text form to convert into.
[`Display`](core::fmt::Display) renders `[2026-06-01, 2026-07-01)` for a human reading a refusal,
and nothing parses that shape, so serializing into it would produce exactly the asymmetry the
`into` on `Date` exists to remove. The round trip that has to hold here is the mapping one, and it
is asserted as such.

#### Methods

```rust
pub fn days(self) -> i32
```

How many days the interval covers.

Always at least 1, because the constructor refuses `end <= start`. This is the number a cost
bound has to be expressed in: rows read are a function of how much history the date predicate
admits, and *not* of the grain, which decides how the admitted rows are grouped afterwards.
A year of history is a year of scanning whether it comes back as 365 buckets or as 1.

Derived from `Date::days_since_epoch` rather than from a second piece of calendar
arithmetic, so a leap year cannot be counted one way here and another way there.
`saturating_sub` because the subtraction is checked-by-construction - `end > start`, and both
day numbers are within the range a four-digit year can reach - and a saturated value would
still be refused by any cap rather than wrapping into a small one.

```rust
pub const fn end(self) -> Date
```

```rust
pub fn new(start: Date, end: Date) -> Result<Self, InvalidTimeRange>
```

Builds a half-open range, rejecting an empty one.

```rust
pub const fn start(self) -> Date
```

#### Implements

`Clone`, `Copy`, `Debug`, `Deserialize<'de>`, `Display`, `Eq`, `PartialEq`, `Serialize`

### `enum InvalidTimeRange`

```rust
pub enum InvalidTimeRange
```

Why a range was rejected.

#### Variants

- `Empty` - `end` is at or before `start`.

#### Implements

`Debug`, `Display`, `Eq`, `Error`, `PartialEq`

## Module `capabilities`

What a metadata provider declares it can supply, and what it declares it cannot.

**The absence is the point.** A `SemanticCatalog` adapter over a directory of markdown documents
written here can carry every field `crate::catalog::Definitions` has, because the format is this
repository's own and grows with the domain. An adapter over a fixed external schema cannot: a
metadata service that holds a measure as a raw expression string in a dialect nothing here renders
supplies structure, prose and join columns, and supplies no measure this repository will execute.
Those two must not look alike, and an empty collection cannot tell them apart - so the adapter
says, here, and the content is then just content.

This is `crate::knowledge::KnowledgeCapabilities`'s argument applied to the other half of a
bundle, and the two halves are declared together in `MetadataCapabilities` because an adapter is
one thing. `docs/adr/0011-pluggable-by-declaration.md` decided the shape and
`docs/adr/0016-what-datahub-can-carry.md` is the measurement that scheduled it: the first source
measured against this port provides part of a model rather than all of one.

# What this module does NOT do

It does not refuse a load. `crate::knowledge::Knowledge::assemble` refuses content for an
undeclared knowledge capability, and nothing here refuses anything: a declaration is a property of
the **code** rather than of the bundle, so it is not under the definition digest and no
composition root reads it yet. What holds it honest is `MetadataCapabilities::checked_against`,
which a conformance suite runs over a real adapter's real bundle. **That is a test rather than an
invariant, and it is written down that way deliberately** - the mechanism that cannot be omitted
is the declaration itself, which the port requires with no default.

### `enum DefinitionKind`

```rust
pub enum DefinitionKind
```

One kind of thing a catalog's *definitions* can carry.

A closed set, for the reason `crate::knowledge::Capability` is one: the alternative is a string,
and a provider that declared `"metrics "` would silently declare nothing at all.

**Nine kinds, and the test for whether one belongs here is whether a real source can be missing it
on its own:** a metadata service can have tables and no metrics, metrics and no definitional
filters, joins whose cardinality it does not vouch for, and dimensions with no reviewed value list.

**`Grains` is the exception and it is stated rather than smoothed over.**
`Definitions::assemble` refuses a metric declaring no grain as `NoGrains`, so a bundle cannot
hold a metric without one - which means the definition side never *observes* `Grains` absent while
`Metrics` is present, and the fidelity check below therefore cannot catch a wrong claim about
grains independently of the claim about metrics. It is still worth declaring: a source with no
grain vocabulary cannot produce a metric at all, and the declaration is what says *why* the
metrics are missing rather than leaving a reader to guess. What it is not is an independently
checkable claim, and describing it as one would be the overstatement this file's own rules name as
a defect.

**The variants are named after what a caller loses**, not after a struct field. `Cardinality` is
the clearest case: every `crate::catalog::Relationship` holds a `crate::model::JoinType`
because the type has no other shape, so what a source can fail to supply is not the field but the
*warrant* - and what a caller sees when the warrant is missing is that no dimension is reachable
through a relationship. `MetadataCapabilities::produced` observes exactly that.

#### Variants

- `Structure` - Physical models: a table, and the column set it exposes.
- `Descriptions` - Prose about a model, a metric or a dimension.
- `Relationships` - Declared joins between models: the two endpoints and their columns.
- `Cardinality` - A join's cardinality, vouched for well enough to license a dimension reached through it.
- `Metrics` - Metrics, each with a measure from the closed vocabulary.
- `RequiredFilters` - Predicates that are part of what a metric MEANS.
- `Grains` - The time resolutions a metric may be asked at. Declarable, and not independently observable - see this enum's own doc comment for why, and do not read a green fidelity test as covering it.
- `AllowedValues` - The reviewed set of values a dimension may be filtered on.
- `Anchors` - The number a metric produced when it was certified.

#### Methods

```rust
pub const fn as_str(self) -> &'static str
```

The word this kind answers to, in a message and in a declaration.

```rust
pub fn every() -> impl Iterator<Item>
```

Every kind there is, in declaration order.

Derived from `Self::next` rather than listed, and seeded by the one variant the assertion
above this `impl` block pins to discriminant zero.

#### Implements

`Clone`, `Copy`, `Debug`, `Deserialize<'de>`, `Display`, `Eq`, `Hash`, `Ord`, `PartialEq`, `PartialOrd`, `Serialize`

### `struct DefinitionCapabilities`

```rust
pub struct DefinitionCapabilities
```

Which definition kinds one provider declares.

A `BTreeSet` rather than nine booleans, for the reason
`crate::knowledge::KnowledgeCapabilities` is one: the order is deterministic, and adding a kind
does not add a field to every construction site.

The constructor is infallible. Any set of kinds is a legitimate declaration - a source that
carries nothing but tables is a real source, and 0016 checked that a bundle of models with no
metrics assembles, pins and validates. What is not legitimate is a bundle that disagrees with the
declaration, and that is `MetadataCapabilities::checked_against`'s to report.

**`conditional` is 0011's *declared-and-empty* state, and it is separate from `declared` on
purpose.** A source maps a schema the deployment authors (datahub's deployment-defined
structured property is the first), so whether a KIND is produced is a property of the deployment
rather than of the code:
the adapter declares the kind, and a bundle that carries none is a faithful bundle rather than an
aspirational declaration. `Self::of_may_provide` is what such an adapter writes. Everything
else - the reference adapter, the goldens, an adapter over a fixed external schema - declares
unconditionally through `Self::of`, which is why the serialized form below carries only the
declared set and why no existing digest moves.

**The conditional marking is a property of the CODE, not of the serialized declaration.** It is
deliberately absent from the `Serialize`/`Deserialize` below, which emit and read the declared
set exactly as the previous newtype did - the contribution manifest's digest therefore records
which kinds a source declared (so widening any declaration moves the digest) and not whether a
kind was conditional (a property `sutura-app`'s assembler and the conformance suite read off the
adapter's own `capabilities()`, never off a wire). A value that round-trips through serde loses
the marking and reads as unconditionally declared, which is the stricter direction and the honest
one: nothing in this repository deserializes a live declaration to serve with.

#### Methods

```rust
pub fn all() -> Self
```

Every kind there is.

**What a REFERENCE adapter declares, and it means more than "all nine today".** A provider
calling this says it supplies whatever kinds exist, including ones added later - which is true
of a catalog format defined in this repository and is not true of anything mapping a schema
somebody else owns.

```rust
pub fn and_may_provide(self, kinds: impl IntoIterator<Item>) -> Self
```

Adds kinds a bundle may lawfully omit to this declaration, leaving the rest unchanged.

What a source writes whose kinds split by whether the deployment authors them: structure,
prose and joins are per-instance unconditional, while the deployment-authored content is
declared-and-empty until a bundle carries any of it.

```rust
pub const fn declared(&self) -> &BTreeSet<DefinitionKind>
```

Everything declared, in a deterministic order.

```rust
pub fn declares(&self, kind: DefinitionKind) -> bool
```

Does this provider supply that kind at all?

```rust
pub fn is_conditional(&self, kind: DefinitionKind) -> bool
```

Is a declared kind one a bundle may lawfully omit?

```rust
pub fn is_empty(&self) -> bool
```

Is nothing at all declared?

```rust
pub const fn none() -> Self
```

A provider with none of them.

```rust
pub fn of(kinds: impl IntoIterator<Item>) -> Self
```

The kinds a provider says it supplies, unconditionally.

**What an adapter over a fixed external schema writes**, so that a tenth kind added here
leaves its declaration alone rather than silently widening it.

```rust
pub fn of_may_provide(kinds: impl IntoIterator<Item>) -> Self
```

The kinds a provider may supply, where the deployment decides which a bundle carries.

**What a source whose content is deployment-authored writes** - datahub's
deployment-defined structured property - where the adapter can carry a kind but every given
bundle may carry none of it.
The kinds are declared (the *not declared* direction still catches content), and absent from
a bundle is a faithful bundle rather than an aspirational declaration.

#### Implements

`Clone`, `Debug`, `Deserialize<'de>`, `Eq`, `PartialEq`, `Serialize`

### `enum DeclarableKind`

```rust
pub enum DeclarableKind
```

One kind of content, either half of a bundle.

Exists so `MetadataCapabilities::checked_against` is one function with one failure type rather
than two that a caller has to remember to run both of.

#### Variants

- `Definition` - Something the definitions carry.
- `Knowledge` - Something the knowledge carries.

#### Implements

`Clone`, `Copy`, `Debug`, `Display`, `Eq`, `Hash`, `Ord`, `PartialEq`, `PartialOrd`

### `enum UnfaithfulDeclaration`

```rust
pub enum UnfaithfulDeclaration
```

Why a bundle does not match what the adapter that produced it declared.

**Two variants because they are two different defects with two different readers.** A bundle
carrying something undeclared means a caller was told an absence that is not one, and whoever
reads the declaration to decide what to trust was misled. A declaration claiming something the
bundle does not carry means the declaration is aspirational, and the next person to widen it will
not know which half of it was ever true.

#### Variants

- `Undeclared` - The bundle carries content of a kind the adapter did not declare.
- `Unprovided` - The adapter declared a kind and the bundle carries nothing of it.

#### Implements

`Clone`, `Debug`, `Display`, `Eq`, `Error`, `PartialEq`

### `struct MetadataCapabilities`

```rust
pub struct MetadataCapabilities
```

What one `SemanticCatalog` adapter declares it can supply.

Both halves of a bundle in one value, because an adapter is one thing and a caller deciding what
to trust reads one declaration. The knowledge half is
`crate::knowledge::KnowledgeCapabilities` verbatim rather than a second vocabulary over the same
four kinds: there is already a closed set for those, it is already rendered into the agent-facing
prompt, and a copy of it would be a second thing to keep in step.

**This value duplicates nothing and derives nothing.** The knowledge capabilities a *bundle*
carries (`crate::knowledge::Knowledge::declares`) are under the definition digest and travel
with the answer; the declaration here is a property of the linked code. That the two agree is
exactly what `Self::checked_against` checks, and it is a check rather than a derivation because
a derivation could not fail.

#### Methods

```rust
pub fn checked_against(&self, produced: &Self) -> Result<(), UnfaithfulDeclaration>
```

Declaration fidelity: is this declaration exactly what `produced` was produced?

The assertion a **declaring** adapter gets in place of the golden adapters' oracle, and it is
two claims rather than one: everything declared was produced, so a declaration is not
aspirational; and nothing of an undeclared kind appears, so a declared absence is visibly
absent rather than silently missing.

**One exemption, and it is the point of`DefinitionCapabilities::of_may_provide`.** A kind
an adapter marks conditional - declared, yet absent from a bundle is lawful because whether a
bundle carries it is the deployment's decision - does not fail the *unprovided* direction. The
*undeclared* direction is unaffected, so a bundle carrying a conditional kind is still
checked against the declared half of it. A declaration that lost its marking (one that
round-tripped through the wire) reads as unconditional, which fails on an absent kind - the
stricter and therefore safe direction.

**The undeclared direction is checked first, and the order is not cosmetic.** That one is the
safety failure - a caller was told an absence that is not one - and reporting it first means a
suite that stops at the first error stops on the worse of the two. Named in the error either
way, so a reader is never left to infer which happened.

One kind per call, deliberately. A `Vec` of every discrepancy would be a presentation of an
error rather than an error, and the workspace's rule is that the variant is the contract.

# Errors

`UnfaithfulDeclaration`, naming the first kind the two disagree about.

```rust
pub fn declares(&self, kind: DeclarableKind) -> bool
```

Does this declaration cover that kind?

```rust
pub const fn definitions(&self) -> &DefinitionCapabilities
```

The definition half.

```rust
pub fn every_kind() -> impl Iterator<Item>
```

Every kind either half could declare, in a deterministic order.

Derived from the two vocabularies' own walks rather than listed, so a kind added to either one
arrives here without an edit. Definitions first, then knowledge, and only because a reader has
to be told some order - nothing depends on which.

```rust
pub fn everything() -> Self
```

Everything there is, in both halves.

**What a REFERENCE adapter declares.** `sutura-catalog-local` already argued this for its
knowledge half and the argument generalises unchanged: a catalog format defined in this
repository supplies whatever kinds the domain grows, so a tenth definition kind or a fifth
knowledge capability needs no edit at that adapter. Anything mapping a schema somebody else
owns writes `Self::of` with two explicit lists instead.

```rust
pub const fn knowledge(&self) -> &KnowledgeCapabilities
```

The knowledge half.

```rust
pub const fn nothing() -> Self
```

A provider that declares nothing.

Legitimate, and not a synonym for a broken adapter: a source that supplies nothing this port
models is one whose bundle is empty, and the pair still has to agree. What it is NOT is a
default - `crate::pinned::SemanticCatalog::capabilities` has none, so nothing reaches this
by omission.

```rust
pub const fn of(definitions: DefinitionCapabilities, knowledge: KnowledgeCapabilities) -> Self
```

The declaration one adapter makes.

```rust
pub fn produced(definitions: &Definitions, knowledge: &Knowledge) -> Self
```

What a bundle actually carries, read off the content.

**Not what the bundle says it carries.** The knowledge half here is observed from the four
collections and never from `crate::knowledge::Knowledge::declares`, which is what lets
`Self::checked_against` catch a bundle whose own declaration and content disagree rather
than comparing one claim against a copy of itself.

Two of the nine definition kinds are read through their consequence rather than their field,
and both are worth stating because a reader will otherwise look for the field:

- **`Cardinality`** is observed as *some dimension is reached through a relationship*. Every
  `crate::catalog::Relationship` holds a `crate::model::JoinType` because the type has no
  other shape, so the presence of the field says nothing; what a source failing to vouch for
  cardinality costs a caller is that no relationship licenses a join, and
  `Definitions::assemble` is what turns an unvouched-for declaration into that.
- **`Descriptions`** is observed as *some description is non-empty*, over models, metrics and
  dimensions alike. A bundle of empty descriptions is a bundle with no prose in it, whatever
  the fields are.

The other seven are the presence of the thing itself.

#### Implements

`Clone`, `Debug`, `Deserialize<'de>`, `Eq`, `PartialEq`, `Serialize`

## Module `catalog`

What a catalog says: models, the relationships between them, and the metrics defined over them.

These are the types every `SemanticCatalog` adapter produces, and `Definitions::assemble` is
the one place their cross-references are checked. That matters more than it looks: a directory of
files and a metadata service over HTTP disagree about almost everything except this, so a check
that lived in an adapter would be a check the other adapter did not have. Two adapters reading
the same content must produce the same `Definitions` or one of them is wrong, and the golden
suite asserts exactly that.

Nothing here holds SQL. See `docs/adr/0001-first-party-semantic-models.md`.

### `struct Model`

```rust
pub struct Model
```

One physical table, and what the catalog knows about it.

`columns` is the whole set the model exposes, and it is a set rather than a list because it is
only ever asked "does this column exist?". Declaring it at all is what lets a dimension naming a
column that is not there be a refusal from the pinned bundle instead of an error from the data
system, which is the difference between a governed answer and a stack trace.

#### Methods

```rust
pub const fn columns(&self) -> &BTreeSet<ColumnName>
```

```rust
pub fn description(&self) -> &str
```

```rust
pub fn has_column(&self, column: &ColumnName) -> bool
```

```rust
pub const fn name(&self) -> &ModelName
```

```rust
pub fn new(name: ModelName, source: SourceName, table: impl Into<QualifiedTable>, columns: BTreeSet<ColumnName>, description: Description) -> Self
```

A model over one physical table, wherever that table lives.

**`impl Into<QualifiedTable>` and not `QualifiedTable`, and that is the compatibility hinge
rather than a convenience.** `From<TableName>` yields an unqualified path, so every existing
caller - a catalog document naming only a table, and every fixture in this workspace - passes
a `TableName` and compiles unchanged, meaning exactly what it used to. It costs the `const`
this constructor used to be, which nothing depended on.

```rust
pub const fn source(&self) -> &SourceName
```

```rust
pub const fn table(&self) -> &QualifiedTable
```

Where the table lives: the whole path, which is what a `FROM` clause names.

Read `Self::table_name` instead wherever what is wanted is the name a column is qualified by
or the name a file-registering engine registers under. Two accessors rather than one that
guesses - `QualifiedTable::name` carries why both readings are real.

```rust
pub const fn table_name(&self) -> &TableName
```

The table's own name, without whatever sits above it.

#### Implements

`Clone`, `Debug`, `Eq`, `PartialEq`, `Serialize`

### `struct Relationship`

```rust
pub struct Relationship
```

A declared join between two models: two columns and a cardinality.

A pair of columns rather than a condition string. The condition form is what the reference
modelling languages use, and it is an escape hatch: `a.x = b.y OR 1 = 1` is a valid condition.
Equality on one column each is the whole of what a model needs to say here.

#### Methods

```rust
pub const fn join_type(&self) -> JoinType
```

```rust
pub const fn name(&self) -> &RelationshipName
```

```rust
pub const fn new(name: RelationshipName, origin_model: ModelName, origin_column: ColumnName, target_model: ModelName, target_column: ColumnName, join_type: JoinType) -> Self
```

```rust
pub const fn origin_column(&self) -> &ColumnName
```

```rust
pub const fn origin_model(&self) -> &ModelName
```

```rust
pub const fn target_column(&self) -> &ColumnName
```

```rust
pub const fn target_model(&self) -> &ModelName
```

#### Implements

`Clone`, `Debug`, `Eq`, `PartialEq`, `Serialize`

### `struct Dimension`

```rust
pub struct Dimension
```

An attribute a metric declares it can be broken down by.

`via` is `None` for a column on the metric's own model and `Some` for one reached through exactly
one declared relationship. **One hop, deliberately.** Two hops need a join order, join order
changes which rows a measure sees, and "the number changed because the planner chose differently"
is the failure this whole repository is arranged against. A second hop arrives with a plan type
that can represent it, not with a loop here.

`allowed_values` is what makes a dimension filterable. `None` means it can be grouped by and not
filtered: a filter needs an allowlist, because the alternative is comparing against a value the
caller supplied, and the pinned bundle is the only thing entitled to say which values exist.

**Every entry is a `DimensionValue`, and how many there may be is
`MAX_VALUES_PER_DIMENSION`.** Both are new, and both close the same hole: this was
`Option<BTreeSet<String>>` read straight out of a YAML document, and `sutura_app::prompt`
interpolates the whole list into the line of an agent-facing document that tells an agent what it
may filter on. A value with an invisible code point in it made that line read as something other
than what it said; an unbounded count made it as long as an author liked. The character rule is
the type's and the count rule is `Definitions::assemble`'s, because a count is not a fact about
one value.

#### Methods

```rust
pub const fn allowed_values(&self) -> Option<&BTreeSet<DimensionValue>>
```

```rust
pub const fn column(&self) -> &ColumnName
```

```rust
pub fn description(&self) -> &str
```

```rust
pub const fn is_filterable(&self) -> bool
```

May this dimension be filtered on at all?

Read from the presence of an allowlist rather than from a separate flag, so the two cannot
disagree: a `filterable: true` beside an empty allowlist would be a dimension that permits
filtering and permits no value.

```rust
pub const fn name(&self) -> &DimensionName
```

```rust
pub const fn new(name: DimensionName, column: ColumnName, via: Option<RelationshipName>, allowed_values: Option<BTreeSet<DimensionValue>>, description: Description) -> Self
```

```rust
pub fn permits(&self, value: &DimensionValue) -> bool
```

Is `value` one the bundle declares?

A dimension with no allowlist answers `false` for everything, which is the safe direction:
the caller gets `DimensionNotFilterable` rather than a query.

Takes a `DimensionValue` rather than a `&str`, so the two sides of the comparison are the
same type: a caller's value is parsed by `DimensionValue::parse` at the wire boundary the way
their metric name is parsed by [`MetricName::parse`](crate::model::MetricName::parse), and text
that could not have been declared never reaches this comparison to be found absent from it.

```rust
pub const fn via(&self) -> Option<&RelationshipName>
```

#### Implements

`Clone`, `Debug`, `Eq`, `PartialEq`, `Serialize`

### `struct Anchor`

```rust
pub struct Anchor
```

A number a metric is expected to produce, so that "it still means what it claimed" is checkable.

The value is text rather than a float on purpose. It is compared against the canonical rendering
of what the data system returned, and a float would make the comparison depend on how two
languages happen to print the same bits.

**Text, and now parsed text.** It was a `String` behind a `const` constructor written by both
catalog adapters, which made it the one authored scalar that entered this crate with no character
rule on it - see `AnchorValue` for the channel that closes and what it deliberately still does
not check. No `Deserialize`: nothing deserializes an `Anchor`, because each adapter deserializes
its own document shape and converts, so the derive was a public surface with no caller and one
more path into a private field.

#### Methods

```rust
pub const fn new(range: TimeRange, value: AnchorValue) -> Self
```

```rust
pub const fn range(&self) -> TimeRange
```

```rust
pub fn value(&self) -> &str
```

The certified number as text.

A `&str` rather than a `&AnchorValue`, because every caller either compares it against a
rendered cell or prints it - and both want the text. **Whoever prints it uses `{:?}`**, for
the reason `RequiredFilter`'s `Display` gives: quoting is what makes spacing visible in a
line a person reads to decide whether a metric still means what it claimed.

#### Implements

`Clone`, `Debug`, `Eq`, `PartialEq`, `Serialize`

### `struct Metric`

```rust
pub struct Metric
```

A certified metric: one measure over one model, and the shapes of question it will answer.

#### Methods

```rust
pub const fn anchor(&self) -> Option<&Anchor>
```

```rust
pub fn description(&self) -> &str
```

```rust
pub fn dimension(&self, name: &DimensionName) -> Option<&Dimension>
```

```rust
pub const fn dimensions(&self) -> &BTreeMap<DimensionName, Dimension>
```

```rust
pub const fn grains(&self) -> &BTreeSet<Grain>
```

```rust
pub const fn measure(&self) -> &Measure
```

```rust
pub const fn model(&self) -> &ModelName
```

```rust
pub const fn name(&self) -> &MetricName
```

```rust
pub fn new(name: MetricName, model: ModelName, measure: Measure, required_filters: Vec<RequiredFilter>, time_column: ColumnName, grains: BTreeSet<Grain>, dimensions: Vec<Dimension>, anchor: Option<Anchor>, description: Description) -> Result<Self, InconsistentDefinitions>
```

A certified metric, or a refusal if two of its dimensions answer to one label.

**Takes a `Vec<Dimension>` and returns a `Result`, and the argument for that is already
written one level up.** `Definitions::assemble`: *"Takes vectors rather than maps so the
duplicate checks are ours: a caller that built a map first has already silently dropped one
of a duplicated pair."* This constructor took a map, so the check was not ours, and the two
shipped adapters had answered the question differently - `sutura_catalog_local` refused a
duplicate and `sutura_catalog_datahub` collected into a map and kept the last. One content,
two `Definitions`. The module header above says two adapters reading the same content must
produce the same one or one of them is wrong, and the golden suite could not see it because
no fixture declares a duplicate.

**A vector makes the bypass a compile error rather than a rule**, which is why the signature
changed instead of a check being added beside the old one: an adapter cannot collapse the
pair before this point any more, because there is nowhere earlier for it to collapse it. The
field stays a `BTreeMap` - the digest is taken over the serialized form and every reader
looks a dimension up by name - so the difference between the parameter and the field is the
whole mechanism.

**One scan and two refusals, because a duplicate and a folded pair are one rule.** The
comparison is `IdentifierCase::COARSEST`, which is true of two identical spellings too, so
an exact repeat is the special case and is named as one:
`InconsistentDefinitions::DuplicateDimension` says *declares dimension `region` twice*,
which is what an author needs to read, and
`InconsistentDefinitions::TwoDimensionsOneLabel` carries the pair. Asking it here rather
than in `Definitions::assemble` is what makes this a parse: after `Ok`, no two of a
metric's dimensions name one label and nothing downstream re-asks. `assemble` could not have
asked - by the time a `Metric` reaches it the map has collapsed an exact pair - and the
DECLARED order is here and nowhere later, so the refusal names the two spellings in the
order the file wrote them. Same shape, and the same argument, as
[`StatementTables::parse`](crate::plan::StatementTables::parse).

**What a folded pair costs was measured rather than argued.** `DuckDB` 1.5.5
(`v1.5.5 Variegata d8cdaa33fd`), whose `sutura_sql::Dialect::identifier_case` declares
`IdentifierCase::InsensitiveAscii`:
`SELECT "Region" FROM (SELECT 1 AS region, 2 AS "Region")` returns **1** - the `region`
column's value - in a result column named `region`, and raises no ambiguity error.
`SELECT *` over the same subquery projects `region, Region_1`, so the second label a caller
was told to expect is not in the result at all. A wrong number and a missing column, from a
catalog that loaded. Folded under `COARSEST` and not under the serving target's rule for the
reason that constant carries: a bundle is dialect-agnostic, so the coarsest rule is the only
one that cannot be wrong in the direction that returns a number.

**Quadratic, and nothing caps how many dimensions a metric may declare**, so the limit is
stated rather than implied: there is a cap on a dimension's VALUES
(`MAX_VALUES_PER_DIMENSION`) and on the group-by keys one question may ask for
(`crate::query::MAX_DIMENSIONS`), and neither is this. What makes it affordable anyway is
position rather than size - it runs once per metric while a document that was read whole is
being converted, and `Definitions`'s own `check_labels_against_table` is already the same shape
over the same list. A cap on declared dimensions is worth having on its own merits and is not
this constructor's to add.

The refusal is an `InconsistentDefinitions` rather than an error of this constructor's own,
so both adapters map it through the variant they already have for that type and neither
grows a second one.

```rust
pub fn required_filters(&self) -> &[RequiredFilter]
```

The predicates every question about this metric carries, whether the caller asked or not.

```rust
pub fn supports_grain(&self, grain: Grain) -> bool
```

```rust
pub const fn time_column(&self) -> &ColumnName
```

#### Implements

`Clone`, `Debug`, `Eq`, `PartialEq`, `Serialize`

### `use None`

### `use None`

### `use None`

### `use None`

### `use None`

### `use None`

### `use None`

### `use None`

### `use None`

### `use None`

### `constant TIME_BUCKET_LABEL`

The label a generated projection gives the truncated time column.

It lives here rather than in the compiler because it is part of the result schema, which is a
contract, and because `Definitions::assemble` has to know it: a dimension by this name would
produce two columns with one label, and a caller reading a result by name would get whichever
the data system listed first.

### `constant MAX_VALUES_PER_DIMENSION`

The most values one dimension may declare.

**Measured before it was chosen, and it is the count that was missing rather than the length.**
The largest allowlist in this repository's example catalog is `region`, with five values, and the
next largest is `product_family` with four. Nothing here declares more than five, and the
question this bound answers is what a REVIEWED allowlist can plausibly be: 64 is twelve times the
largest one written here and four times the sixteen German federal states, which is the largest
enumeration a person writes out by hand in one line of a document. A dimension needing two
hundred country codes is not an allowlist somebody read; it is a lookup table, and it wants a
mechanism that does not put every entry into an agent's prompt.

Argued the way `crate::query::MAX_RANGE_DAYS` is argued, including about what it does not bound.
**The number that matters is the product of this and `MAX_DIMENSION_VALUE_CHARS`**, because
`sutura_app::prompt` lists every declared value of a dimension on one line of the document an
agent reads: 64 values of 64 characters is 4 KiB, the same order as
`crate::knowledge::MAX_NOTE_BODY_BYTES`, so one dimension's value list is bounded by about what
one note body is. Per-value caps alone let N conforming values do what one oversized value cannot,
which is the same argument `crate::knowledge::MAX_KNOWLEDGE_BYTES` makes for notes.

What it does not bound, said plainly. It bounds ONE dimension: nothing here caps how many
dimensions a metric declares or how many metrics a catalog holds, so the size of the whole
rendered document is still a function of how much a catalog says. Those are the same shape of hole
and want the same kind of fix; this is the one the review named, and the honest statement of what
holds is better than a bound nobody measured.

## Module `definitions`

The identifiers of a pinned definition set, and the one operation that computes one.

Definitions are authored upstream and arrive as an immutable, hashed snapshot. The digest
is what makes "the same question returns the same number" checkable rather than asserted,
and what stops a catalogue edit from changing what executes - so a value that is not a
digest must not be able to occupy the slot where one is expected.

**The canonical form and its hash live HERE, and that is a correction rather than a
preference.** They used to live in a catalog adapter, and
`crate::pinned::PinnedDefinitions::pin` took the hashing function from its caller. A review
found what that leaves open: any safe public code could pass `|_| Ok(some_other_digest)` and pair
an unrelated digest with a set of definitions, so an answer could carry provenance for content
that did not produce it. Handing the definitions to a function is not proof that the function read
them. The digest has to be computed by code the domain trusts, which means code the domain holds,
which is what `DefinitionDigest::of` is.

The cost is two entries on the domain's dependency allowlist - `sha2` and `serde_json`, twelve
crates transitively - and `xtask/src/boundaries.rs` records what was measured and why it was
accepted. Neither is a framework, both were already linked into the shipped binary through the
catalog adapter, and no lockfile entry is new.

### `struct DefinitionDigest`

```rust
pub struct DefinitionDigest
```

Content hash of a pinned definition set.

Two ways in and they answer different questions. `DefinitionDigest::of` computes one FROM a set
of definitions and is what `crate::pinned::PinnedDefinitions::pin` uses; `DefinitionDigest::parse`
reads one that arrived as text - a provenance record, a serialized bundle - and checks its shape.
The field is private and `Deserialize` is routed through `parse`, so a `DefinitionDigest` that is
not `HEX_LEN` hex characters does not exist to be passed anywhere.

**A parsed digest is not a forgery route, and the difference is worth being precise about.**
Anybody may parse any hex string into one of these; what nothing can do is get it into a
`crate::pinned::PinnedDefinitions`, because that type's constructor takes no digest and computes
its own. So this type says "this is digest-shaped" and the bundle says "this digest is of that
content", and only the second claim is one an answer rests on.

#### Methods

```rust
pub fn as_str(&self) -> &str
```

```rust
pub fn parse(raw: impl Into<String>) -> Result<Self, InvalidDigest>
```

Parses a digest, rejecting anything that is not one.

Parse, not validate: once this returns `Ok`, nothing downstream re-checks the shape,
because an ill-formed digest is unrepresentable. Case is normalised here rather than
at comparison sites, so one digest has one spelling and the derived `PartialEq`,
`Hash` and `Serialize` all agree about which digest this is.

#### Implements

`Clone`, `Debug`, `Deserialize<'de>`, `Eq`, `Hash`, `PartialEq`, `Serialize`

### `enum NotDigestible`

```rust
pub enum NotDigestible
```

Why a set of definitions could not be reduced to a digest.

Two variants, and neither is reachable from any catalog this repository can load - which is why
they are variants rather than a panic. Neither `Definitions` nor `Knowledge` holds a float and
every map key in both is a newtype over a string, so the serializer has nothing to refuse; and
lower-case hex of 32 bytes is
what a digest is. Each variant names which half changed, so a future field of a type that does not
serialize says so instead of surfacing as "the catalog is broken".

#### Variants

- `Canonicalize` - The definitions could not be written into their canonical form.
- `NotADigest` - The computed hash is not digest-shaped, which means the hashing changed and not the catalog.

#### Implements

`Debug`, `Display`, `Error`

### `enum InvalidDigest`

```rust
pub enum InvalidDigest
```

Why a digest was rejected. Parse failures are values, not panics: this crate denies
`unwrap`/`panic` in lints.

Each variant carries the offending input as a typed field, not a pre-formatted sentence.
The variants and their fields are the contract; the `#[error]` text is a convenience for
a human and may be reworded without breaking a caller that matched on `WrongLength`.

#### Variants

- `Empty` - Empty or whitespace-only, so an unhashed snapshot cannot masquerade as a pinned one.
- `NotHex` - Not hexadecimal. `offending` is the first character that is not, which is the one worth reporting - a message naming all of them tells the reader less.
- `WrongLength` - Hexadecimal, but not a SHA-256 digest. `expected` rides along so a caller can render its own message from fields, and so this text and `HEX_LEN` cannot drift apart.

#### Implements

`Debug`, `Display`, `Eq`, `Error`, `PartialEq`

## Module `expression`

The escape hatch: SQL a catalog author wrote, for the metrics the closed vocabulary cannot say.

[`measure`](crate::measure) is closed and stays closed. A window function, a percentile, an
expression over two columns - `SUM(price * quantity)` - has no `Measure` and cannot get one
without turning that vocabulary into an expression language. Two things forced a second path
anyway. Metrics people actually certify use those constructs; and a provider whose catalog
**already holds SQL per metric** - wren's cubes carry
`SUM(CASE WHEN status = 'active' THEN mrr_eur END)` in the file - has nothing to map onto a
closed vocabulary and would arrive as "unsupported" for its entire metric set.

So this module is the hatch, and everything about its shape is arranged so that it cannot be
used by accident or unnoticed:

**It is a sibling of the closed vocabulary, not a field on it.** `Computation` has two
variants, `Computation::Measure` is the ordinary one, and a metric that uses SQL says so in a
word - `authored_sql` - that a reviewer greps for and an operator can list. There is no
`expression:` key on a measure, no `Option<String>` beside one, and no shape in which "this
metric is free-text SQL" is invisible in a diff.

**Nothing here parses.** A `SqlFragment` is checked for being *a plausible fragment* - present,
bounded, and free of the characters that make the text a reviewer reads differ from the text
that compiles - and nothing more. Whether it is one SQL expression, over
columns this model declares, reaching no table it was not given, is decided by `sutura_sql`, at
catalog-compile time, and a fragment that fails is a **load failure naming line and column**. The
domain may not do that work: it holds no SQL parser and `cargo xtask check-boundaries` keeps it
that way. The consequence is worth stating plainly - **a `Computation::AuthoredSql` that has not
been through `sutura_sql::expression::compile` is unvalidated**, and the composition root is what
must not skip it.

**It is a provider CAPABILITY, not a feature every provider has.** A wren-style directory has
authored SQL because a person wrote the file. A metadata service that stores no executable SQL
per metric, and an RDF vocabulary that never will, produce `Computation::Measure` for every
metric and are complete rather than degraded. That is why the closed vocabulary is a *variant*
and not the `None` arm of an `Option`: "no expression" is the ordinary shape of the type.

**Nothing here is wren-shaped.** No `base_object`, no result `type:`, no assumption that the text
came out of a `cubes/*.yml`. What a provider read, and out of what file, is the adapter's
business; what arrives here is an authored fragment per dialect and nothing else. A provider that
already stores per-dialect SQL maps onto `AuthoredSql`'s map directly, which is the strongest
argument for that shape over a single string.

### `enum InvalidFragment`

```rust
pub enum InvalidFragment
```

Why a fragment is not one.

#### Variants

- `Empty` - Empty or whitespace-only. This is the input that made the obvious fragment API unusable: the dialect layer's `Parser::parse_expressions` panics on an empty token list, and under `panic = "abort"` a blank line in a catalog file would end the process. It is refused here, before anything can be asked of it.
- `TooLong`
- `ControlCharacter` - A control character other than tab and newline. Those two are formatting a person might use inside a long `CASE`; the rest are not text, and their likeliest origin is a paste accident or an attempt to hide part of a fragment from a reviewer's terminal.
- `InvisibleCharacter` - A character a terminal, a diff and a browser do not render, or render in the wrong order.

#### Implements

`Debug`, `Display`, `Eq`, `Error`, `PartialEq`

### `enum InvalidDialectTag`

```rust
pub enum InvalidDialectTag
```

Why a dialect word is not one.

#### Variants

- `Empty`
- `TooLong`
- `IllegalCharacter`

#### Implements

`Debug`, `Display`, `Eq`, `Error`, `PartialEq`

### `struct SqlFragment`

```rust
pub struct SqlFragment
```

One authored SQL fragment, as text and nothing more.

Its own type rather than a `String` field, so the checks happen once and a value that reached
them cannot be confused with a string that did not. Deliberately **not** an identifier newtype:
the character set of SQL is not the character set of a name, and narrowing it here would reject
the quotes, parentheses and commas the whole feature exists to allow.

#### Methods

```rust
pub fn as_str(&self) -> &str
```

```rust
pub fn parse(raw: impl AsRef<str>) -> Result<Self, InvalidFragment>
```

Checks that this is a plausible fragment. It does **not** check that it is SQL.

#### Implements

`Clone`, `Debug`, `Deserialize<'de>`, `Eq`, `Hash`, `Ord`, `PartialEq`, `PartialOrd`, `Serialize`

### `struct DialectTag`

```rust
pub struct DialectTag
```

Which dialect a fragment was authored for, as a word at rest.

**The domain does not own the list of data systems we render for, and that is not an oversight.**
`sutura_sql::dialect::Dialect` owns it, because each entry there is a claim that we generate
correct SQL for that system and have a golden that says so - and a second copy of the set here
would be one that has to be kept in step with nothing checking it, which is exactly what
`crate::measure::Term` declines to do for aggregates. So a tag is a *word* until the compile
step, which resolves it against the list that build actually renders for and refuses an unknown
one naming the choices. A `postgresql:` where `postgres:` was meant is therefore a load failure
and not a variant that is silently never chosen.

#### Methods

```rust
pub fn as_str(&self) -> &str
```

```rust
pub fn is_portable(&self) -> bool
```

```rust
pub fn parse(raw: impl AsRef<str>) -> Result<Self, InvalidDialectTag>
```

Checks that this is one dialect word. **Surrounding whitespace is a load failure, not
something trimmed away**, and that is the half worth writing down.

A tag is a key in `AuthoredSql`'s map. Trimming made `duckdb` and ` duckdb ` the same tag,
and `BTreeMap`'s deserialize keeps the LAST value for a repeated key - so a document writing
both had one of its two authored fragments silently discarded and the other certified, with
the definition digest taken over the survivor. That is the outcome
`Computation::assemble` refuses when a metric writes `measure` beside `authored_sql`, for
the same reason: a document that writes two means one of them, and choosing certifies a
number its author did not ask for. Refusing the whitespace costs the author one character.

`SqlFragment::parse` still trims, and the asymmetry is deliberate: a fragment that differs
from another only by surrounding whitespace is the same fragment, so trimming there loses
nothing. Two map keys that differ only by whitespace are two keys.

```rust
pub fn portable() -> Self
```

The word resolution falls back to, and the only word it falls back to.

**The one constructor in this module that writes the private field without going through
`Self::parse`**, which is the shape a newtype is supposed to make impossible - and it is
written down here rather than left as an oddity a reader has to notice. It cannot go through
`parse`, because `parse` is fallible and this is not: the alternatives are an `unwrap`, which
the workspace denies outright, or an `Err` arm that would write the same field by another
route and prove nothing.

So the two agreeing is pinned by a test instead of by the type - see
`portable_is_the_word_parse_would_have_produced`. What that test catches is the reachable
mistake: a future tightening of `parse` - a shorter length bound, a narrower character set -
that would refuse `portable` while this constructor kept minting it, leaving a value in a map
key position that no catalog file could ever have written.

#### Implements

`Clone`, `Debug`, `Deserialize<'de>`, `Display`, `Eq`, `Hash`, `Ord`, `PartialEq`, `PartialOrd`, `Serialize`

### `enum InvalidAuthoredSql`

```rust
pub enum InvalidAuthoredSql
```

Why a set of authored fragments is not usable.

#### Variants

- `NoFragments` - The key was written and no fragment was given under it.

#### Implements

`Debug`, `Display`, `Eq`, `Error`, `PartialEq`

### `struct AuthoredSql`

```rust
pub struct AuthoredSql
```

Catalog-authored SQL for one metric: one fragment per dialect, and a `portable` fallback.

**A map and not a single string, because the honest answer to "it does not translate" is to say
so per dialect.** Wren's own `Measure` has no expression field at all, and its cube path carries
one string with no dialect attached to it; its OSI importer is the part that got this right, with
`{dialects: [{dialect: SNOWFLAKE, expression: ..}, {dialect: ANSI_SQL, ..}]}`. This is that
shape, as a map, so the key is unique by construction rather than by a duplicate check.

**Resolution is exact dialect, then `portable`, then refuse - and the third step is where this
departs from the importer it copies.** Wren falls back to the first non-empty variant. That
hands a Postgres query a Snowflake expression because it happened to be listed first, which is a
number computed by a definition nobody chose, under a certified name. Refusing names the dialect
and costs an operator one line in a file.

**On disk it is the map itself and not a struct holding one**, so a document writes
`authored_sql: { portable: .. }` rather than `authored_sql: { fragments: { portable: .. } }`. The
serde route is `try_from`/`into` rather than `transparent`, for the reason
`crate::measure::Term`'s on-disk representation gives: `transparent` writes straight past
`AuthoredSql::new`, so the empty-map refusal would hold for a constructor call and not for the
one path that actually carries a catalog file.

#### Methods

```rust
pub fn exact(&self, dialect: &DialectTag) -> Option<&SqlFragment>
```

The fragment authored for exactly this dialect, if there is one.

```rust
pub const fn fragments(&self) -> &BTreeMap<DialectTag, SqlFragment>
```

One authored fragment per dialect word, in a canonical order.

```rust
pub fn new(fragments: BTreeMap<DialectTag, SqlFragment>) -> Result<Self, InvalidAuthoredSql>
```

Takes the authored fragments, refusing an empty set.

`BTreeMap` for the reason `crate::catalog::Definitions` uses one: the definition digest is
taken over the serialized form, and a map that serialized in hash order would move the digest
without the catalog moving.

```rust
pub fn portable(&self) -> Option<&SqlFragment>
```

The `portable` fragment, if there is one.

```rust
pub fn tags(&self) -> Vec<&DialectTag>
```

Which dialects this metric was authored for, for a refusal that lists them.

#### Implements

`Clone`, `Debug`, `Deserialize<'de>`, `Display`, `Eq`, `PartialEq`, `Serialize`

### `enum InvalidComputation`

```rust
pub enum InvalidComputation
```

Why a metric does not say what it computes.

#### Variants

- `Nothing` - Neither key. Refused rather than defaulted: a metric with no measure has no number.
- `Both` - Both keys. Refused rather than resolved by precedence, for the reason `crate::measure::InvalidTerm::TwoTerms` gives: a document that writes both means one of them, and choosing would certify a number the author did not ask for. It matters more here than there, because the two would not merely differ - one is composed by the generator and the other is text somebody wrote.

#### Implements

`Debug`, `Display`, `Eq`, `Error`, `PartialEq`

### `enum Computation`

```rust
pub enum Computation
```

What a metric computes, and which of the two ways it says so.

**Externally tagged and meant to be flattened into the metric document, which is what keeps every
existing catalog byte-identical.** `Computation::Measure` serializes as `{"measure": ..}`,
exactly the field a metric already had, so a closed-vocabulary metric's canonical form - and so
its definition digest - does not move for gaining this type. An `authored_sql` metric is a new
key, visible in the diff, which is the whole point.

#### Variants

- `Measure` - The closed vocabulary, and the ordinary case. Every metadata provider can produce this, and nothing about it is optional or degraded.
- `AuthoredSql` - SQL somebody wrote in the catalog, compiled at load. The exception, named so that it reads as one.

#### Methods

```rust
pub fn assemble(measure: Option<Measure>, authored_sql: Option<AuthoredSql>) -> Result<Self, InvalidComputation>
```

Builds a computation from the two sibling keys a metric document may carry.

Two `Option`s in, and a refusal for each wrong combination - the shape
`crate::measure::Term` uses, for the same reason and one more. `deny_unknown_fields` cannot
coexist with `serde(flatten)`, so the adapter declares the two keys and this decides what
they mean; and putting the decision here means a second catalog adapter cannot disagree about
whether writing both is an error.

```rust
pub const fn authored_sql(&self) -> Option<&AuthoredSql>
```

The authored SQL, if this metric uses the escape hatch.

```rust
pub const fn kind(&self) -> &'static str
```

The word a catalog writes, and the word an operator lists metrics by.

This is the mechanism behind "a reviewer and an operator must be able to see which metrics
use the hatch": one accessor over the pinned definitions, rather than a grep over files.

```rust
pub const fn measure(&self) -> Option<&Measure>
```

The closed measure, if this metric uses the closed vocabulary.

Every consumer that walks columns, resolves terms or renders an aggregate reads this, and a
`None` is the signal that the number comes from a compiled fragment instead. An adapter that
cannot execute one has to **refuse** on that `None` rather than skip the metric.

#### Implements

`Clone`, `Debug`, `Deserialize<'de>`, `Display`, `Eq`, `PartialEq`, `Serialize`

### `constant MAX_FRAGMENT_LEN`

The longest authored fragment accepted.

A bound rather than a judgement about style: the fragment is handed to a recursive-descent parser
at load, and an unbounded string out of a file is an unbounded amount of work and stack.
Generous enough for the conditional sums and guarded ratios this exists for; anything longer is a
derived column that belongs upstream, which is what `docs/adr/0001` says about the whole class.

## Module `federation`

How a measure federates: what descends into a leg, and the one computation that happens above
them.

**This module is a classification and a rule, and nothing executes it.** There is no leg plan
type, no splitter and no combiner in this workspace yet, so nothing here has a production
caller: the same shape `.agents/skills/sutura/query-surface`'s built-and-not-wired inventory describes for the
authored-SQL hatch. It is stated here rather than left for a reader to discover, because a
classification that looks wired is worse than one that says it is not.

**The problem it answers.** Grouping a fact leg by a remote join key is a strictly finer grouping
than the answer, so a combine above the legs has to aggregate again - and whether that is correct
depends entirely on the aggregate. `AVG` of `AVG`s is not the average, and two exact distinct
counts added together over-count every key the two legs share. Neither of those raises an error
anywhere: they are wrong numbers under a certified metric name, which is the failure mode this
repository exists to prevent.
`docs/adr/0007-federating-across-different-data-systems.md` is the finding and
`docs/adr/0009-the-plan-from-one-source-to-many.md` Decision 2 is the decision.

**A measure that does not descend is not a refusal.** Decision 2 is explicit: where an aggregate
cannot be computed per leg and re-aggregated, the leg carries finer-grained rows and the
aggregate happens above, paying the processing cost. So there is no error type in this module and
no [`RefusalReason`](crate::query::RefusalReason) variant behind it - `Descent` is total over
the vocabulary, and its third variant is a plan rather than a decline.

**Three exhaustive matches, and each of them is the mechanism.** `Descent::of` matches
`Aggregate`, so a seventh aggregate cannot compile without stating which of the three classes
it is in; `descend` matches `Term`, so a third term has to say the same thing; and
`Federation::of` matches `Measure`, so a third shape does too. A `match` that has to gain an
arm is the whole content of this branch.

**The division cannot happen in a leg, and that is a shape rather than a check.** The vocabulary
already splits into a `Term` - one number - and a `Measure` - one term or a ratio of two.
What a leg carries is a `Carried`, and a `Carried` is built out of the *term* level: it has no
variant that divides and no field a `ZeroDenominator` fits in, at any depth. The only
`ZeroDenominator` in this module is on `Above::Quotient`, which is the node above every leg.
The bug that closes is specific: `ZeroDenominator::Null` renders as `NULLIF(d, 0)`, so applied
*inside* a leg a subgroup with a zero denominator becomes null, the `SUM` above skips nulls, and
that subgroup's numerator is silently dropped from the answer instead of nulling it.

What is deliberately NOT here: the join kind. Decision 2's other half - INNER for a remote
dimension carrying a filter, LEFT for one that does not - is a function of where the filters
went, which only a splitter can know. It belongs to the branch that builds one.

### `struct Pushed`

```rust
pub struct Pushed
```

An aggregate that descends as written, paired with the function that re-aggregates it above.

**Two aggregates rather than one, because they are not always the same one.** A `Count` pushed
into a leg is re-aggregated above with a `Sum`: adding the leg counts is the count, and counting
them again counts legs. That is the single most repeated arithmetic mistake in a hand-written
combine, and recording both halves is what stops it being restated at each call site.

**The fields are private and there is no public constructor**, so the only values of this type
are the ones `Descent::of` returns. That is what makes `Carried::Aggregated` unable to *name*
an aggregate the classification did not call pushable: `Avg` and `CountDistinct` have no
`Pushed` anywhere, so no caller can write one. **The limit, stated with the claim:** the
guarantee is module-scoped, since code in this file can write the struct literal - which is
exactly where the classification lives, and nowhere else.

#### Methods

```rust
pub const fn combine(self) -> Aggregate
```

The aggregate that re-aggregates the leg's column above.

```rust
pub const fn push(self) -> Aggregate
```

The aggregate the leg computes.

#### Implements

`Clone`, `Copy`, `Debug`, `Eq`, `PartialEq`, `Serialize`

### `struct Pulled`

```rust
pub struct Pulled
```

An aggregate that does not descend at all, and runs above the legs on the rows they carried.

Private field and no public constructor, for `Pushed`'s reason: a `Carried::Keys` can only
name an aggregate `Descent::of` classified as non-descending, and it carries *which* one rather
than assuming `CountDistinct` is the only one it will ever be.

#### Methods

```rust
pub const fn above(self) -> Aggregate
```

The aggregate the combine applies to the pulled-up rows.

#### Implements

`Clone`, `Copy`, `Debug`, `Eq`, `PartialEq`, `Serialize`

### `enum Descent`

```rust
pub enum Descent
```

How one aggregate of the closed vocabulary descends into a leg.

**Three variants, because there are three answers and not two.** An earlier version of the plan
named `combine_with() -> Option<Aggregate>`, and `Option` cannot say the third one: descends as
itself, descends as two columns, does not descend and travels as a grouping key. The compile
error a seventh aggregate produces is therefore *you have not said which of the three you are*
rather than *you have not said whether you can*.

#### Variants

- `AsWritten` - Pushable as written: one column in the leg, one function above it.
- `Decomposed` - Pushable decomposed: two columns in the leg, divided once above.
- `AsGroupingKey` - Not pushable. The column travels as a grouping key and the aggregate runs above.

#### Methods

```rust
pub const fn of(aggregate: Aggregate) -> Self
```

The total function from an aggregate to how it federates.

Exhaustive over `Aggregate` by construction: this `match` is the mechanism the whole
branch exists for, and a new variant of that closed enum does not compile until it appears
here.

#### Implements

`Clone`, `Copy`, `Debug`, `Eq`, `PartialEq`, `Serialize`

### `enum Carried`

```rust
pub enum Carried
```

What one leg carries for one number the answer needs.

**The type that cannot divide.** There is no `Quotient` variant here and no `ZeroDenominator`
reachable from one: a `Carried` is a `Pushed` or a `Pulled` over a `ColumnName`, and none
of those three can hold one. That is the ratio rule as a shape rather than as a rule somebody
remembers - see this module's header for the wrong number it prevents.

The division that a leg cannot express does not compile:

```compile_fail
use sutura_domain::federation::Carried;
use sutura_domain::measure::ZeroDenominator;

// There is no variant of `Carried` that divides, so this names nothing.
fn _per_leg(numerator: Carried, denominator: Carried, zero_denominator: ZeroDenominator) -> Carried {
    Carried::Quotient { numerator, denominator, zero_denominator }
}
```

Nor does the zero guard, which is the half that silently drops a subgroup when it is applied
inside a leg:

```compile_fail
use sutura_domain::federation::{Carried, Pushed};
use sutura_domain::measure::ZeroDenominator;
use sutura_domain::model::ColumnName;

fn _guarded(pushed: Pushed, column: ColumnName, zero_denominator: ZeroDenominator) -> Carried {
    Carried::Aggregated { pushed, column, zero_denominator }
}
```

And the twin, so a rename cannot make either block pass vacuously: the division exists, one level
up, where every leg is already below it.

```
use sutura_domain::federation::{Above, Carried};
use sutura_domain::measure::ZeroDenominator;

fn _above(numerator: Carried, denominator: Carried, zero_denominator: ZeroDenominator) -> Above {
    Above::Quotient {
        numerator: Box::new(Above::Total(numerator)),
        denominator: Box::new(Above::Total(denominator)),
        zero_denominator,
    }
}
```

#### Variants

- `Aggregated` - One aggregate the leg computes and hands up as one column.
- `CountIf` - A conditional count the leg computes and hands up as one column.
- `Keys` - No aggregate in the leg at all: the column travels as a grouping key, and the aggregate `Pulled` names runs above the rows it carried.

#### Methods

```rust
pub const fn column(&self) -> &ColumnName
```

The column this leg reads.

```rust
pub const fn combine(&self) -> Aggregate
```

The aggregate the combine applies to what this leg carried.

One place, so no combiner has to restate it - and so `Count` pushed down, `Sum` above stays
one decision rather than one per call site.

```rust
pub const fn is_pulled_up(&self) -> bool
```

Does this leg carry rows at the fact grain rather than one row per group?

The price of the pull-up, and the quantity worth logging: it is the difference between a leg
returning one row per group and one row per distinct key.

#### Implements

`Clone`, `Debug`, `Eq`, `PartialEq`, `Serialize`

### `enum Above`

```rust
pub enum Above
```

The one computation above the legs.

**A tree rather than a pair, and the reason is a nested case that is representable today**:
`ratio(avg(x), count(y))` has an `Avg` numerator, and an `Avg` is itself a division, so the
above-step nests. Depth is bounded at two by the vocabulary - a `Measure` is at most a ratio of
terms, and a term contributes at most one division - so the `Box` is indirection for a
recursive type rather than an unbounded structure.

Every division in this workspace's federated path is one of these nodes. That is the property:
the numbers a leg produces are re-aggregated, and only then divided.

#### Variants

- `Total` - One column the legs carried, re-aggregated by `Carried::combine`.
- `Quotient` - Two of those, divided once - above every leg, with the zero handling the definition asked for applied to the final denominator rather than to a leg's.

#### Implements

`Clone`, `Debug`, `Eq`, `PartialEq`, `Serialize`

### `struct Federation`

```rust
pub struct Federation
```

How a measure federates: the whole answer for one measure.

Produced by `Federation::of`, which is total: every measure the closed vocabulary can express
federates, and the ones that cannot descend pull rows up instead of being declined.

#### Methods

```rust
pub const fn above(&self) -> &Above
```

What happens once, above the legs.

```rust
pub fn carried(&self) -> Vec<&Carried>
```

What every leg carries, in the order a reader would say the measure.

The leaves of `Above`, collected rather than stored twice: a second copy is a second thing
to keep in step with the tree.

```rust
pub fn of(measure: &Measure) -> Self
```

Classifies one measure.

Exhaustive over `Measure`'s shapes, and over `Term`'s terms through `descend`. A ratio
becomes an `Above::Quotient` whose halves are pushed separately, which is the rule stated
as the only tree this function can build.

```rust
pub fn pulls_up_rows(&self) -> bool
```

Does answering this measure need rows at the fact grain?

True for anything reaching a `Descent::AsGroupingKey`, which is `CountDistinct` today. This
is the cost Decision 2 accepts rather than refuses, so it is a quantity to report and not a
condition to fail on.

#### Implements

`Clone`, `Debug`, `Eq`, `PartialEq`, `Serialize`

### `fn descend`

```rust
pub fn descend(term: &crate::measure::Term) -> Above
```

How one term descends.

Exhaustive over `Term`, which is the second of this module's three matches: a third term has to
say how it federates before this compiles.

## Module `identity`

Who a request runs as, and the credential material that proves it.

Named for the concept rather than for the mechanism it currently uses. `redact` was the
earlier name, and it described one property of one type - so the module could not hold
the principal chain, the request context or the `CredentialBroker` port that belong beside
it, and every one of those would have arrived somewhere else.

All three are here now. `principal` holds the chain a call is attributed to and the request
context that carries it; `credential` holds what one answer executes with and the
`CredentialBroker` port that mints it. The port arrived with its first implementor, which is
`sutura_config::StaticCredentialBroker` - the static-credential broker a single-user deployment
already needs, rather than a fake standing in for one.

**Nothing in `principal` is `Serialize` or `Deserialize`, and `Secret` is neither either.**
That is one property rather than two coincidences: an identity is derived from what a transport
established, and a type that could be read off the wire is a caller stating its own. The
credential material and the chain are the two things in this workspace where being unable to
parse the value from a request body is the control.

**`Secret` is now a COMPILE ERROR where it used to be a redaction**, and the difference is the
whole of `docs/adr/0020`. A hand-written `Display` printing `REDACTED` left `format!("{token}")`
and `tracing::info!(%token)` compiling: nothing leaked, and every structured-logging call site
stayed a chance for an author to believe they had logged a value. The type is built on
`secrecy::SecretString`, which has no `Display` and no `PartialEq`, so those two and `==` do not
build at all - pinned by `compile_fail` doctests with compiling twins on `Secret` itself. A
secret that reaches a log is not recoverable once shipped, so the type, not the call site, is
where this is fixed.

### `struct Secret`

```rust
pub struct Secret
```

An opaque secret that nothing can render.

The value is reachable only through an explicit, greppable call to `Secret::expose_secret`:
there is no formatter to reach here, and none on what it wraps.

# What the compiler refuses, and why that replaced a redaction

This was `Secret(String)` with a hand-written `Debug` **and** `Display`, both printing
`REDACTED`. Nothing ever leaked through them - and the accident stayed *representable*, which is
the defect: `format!("{token}")` compiled, `tracing::info!(%token)` compiled, and each produced a
line reading `REDACTED` where its author believed a value had been logged. A redaction is a
CHECK, and this workspace's rule is to prefer unrepresentable to checked, so the value now lives
in a `secrecy::SecretString`, which has no `Display` and no `PartialEq` - and the four accidents
below stopped compiling rather than started printing a placeholder. `docs/adr/0020` is the
decision, including what the dependency costs and what keeping the hand-written type would have
bought instead.

**It is a newtype over `SecretString` rather than an alias for it, and that is load-bearing in
three ways.** A `Deserialize` cannot arrive by FEATURE UNIFICATION: `secrecy`'s `serde` feature
gives `SecretBox` one, cargo unions features across a build graph, and a newtype that derives
nothing is unaffected by whether some other crate turns it on - so the manifest's
`default-features = false` is a supply-chain decision and not the mechanism. `SecretString` also
has a `Default`, which is the newtype guide's own counterexample pointed at credentials - a
default secret is not a thing - and `From<&str>` plus `From<String>`, two ways in that bypass
`Secret::new`. The wrapper refuses all three by not restating them.

## `Display`, and the `{}` that used to compile

```compile_fail
let token = sutura_domain::identity::Secret::new("hunter2");
// No `Display`, so there is no formatter for `{}` to select.
let line = format!("token={token}");
drop(line);
```

The compiling twin, differing by that one formatter - `Debug` is still there, and still cannot
print the value:

```
let token = sutura_domain::identity::Secret::new("hunter2");
let line = format!("token={token:?}");
assert!(!line.contains("hunter2"), "{line}");
assert!(line.contains("REDACTED"), "{line}");
```

## A `%` tracing field, which is that same missing `Display`

`tracing`'s `%` sigil records a field through `tracing::field::display`, whose argument is bound
`T: Display` - so the mechanism is the bound below rather than anything about the macro, and this
is the honest place to pin it: `sutura-domain` has no `tracing` dependency and may not acquire
one, because `cargo xtask check-boundaries` walks this crate's whole resolve graph. The macro
itself is pinned where a crate has both, in `sutura_http::inbound`.

```compile_fail
fn recorded_as_a_display_field(value: impl std::fmt::Display) -> String {
    value.to_string()
}
let token = sutura_domain::identity::Secret::new("hunter2");
let line = recorded_as_a_display_field(token);
drop(line);
```

The compiling twin, differing by the one call that says out loud what it is doing:

```
fn recorded_as_a_display_field(value: impl std::fmt::Display) -> String {
    value.to_string()
}
let token = sutura_domain::identity::Secret::new("hunter2");
assert_eq!(recorded_as_a_display_field(token.expose_secret()), "hunter2");
```

## `==`

Deliberately NOT `PartialEq`/`Eq`, and now inherited rather than merely omitted - `SecretBox` has
no `PartialEq` either, so neither the wrapper nor a future `#[derive]` on it can produce one from
the inner value. A derived comparison on credential material returns on the first differing byte,
which is a timing oracle at whatever call site adds it, and the call site is where it would be
invisible. The one comparison this workspace needs is
`sutura_config::AccessToken::matches_in_constant_time`, which is named for what it is.

```compile_fail
let a = sutura_domain::identity::Secret::new("hunter2");
let b = sutura_domain::identity::Secret::new("hunter2");
assert!(a == b, "no `PartialEq` to call");
```

The compiling twin, differing by the one thing that makes the comparison visible - and it is
still the wrong way to compare a credential, which is the point of it being conspicuous:

```
let a = sutura_domain::identity::Secret::new("hunter2");
let b = sutura_domain::identity::Secret::new("hunter2");
assert!(a.expose_secret() == b.expose_secret());
```

## `Deserialize`

There is none, so caller-supplied bytes cannot become credential material - the other half of
`crate::identity::principal`'s rule that a caller cannot state its own identity.

```compile_fail
let token: sutura_domain::identity::Secret = serde_json::from_str(r#""hunter2""#).expect("no");
drop(token);
```

The compiling twin, differing by where the value comes from - a constructor a reviewer can see,
rather than a deserializer a request reaches:

```
let token = sutura_domain::identity::Secret::new(String::from("hunter2"));
assert_eq!(token.expose_secret(), "hunter2");
```

# The value is wiped on drop, and the limit is not small

`SecretBox`'s `Drop` writes zeros over the boxed buffer through `zeroize`'s volatile writes, so
the copy THIS TYPE holds does not outlive it in the process image. **It says nothing about copies
made before the value arrived.** A settings file read into a `String`, a `serde`-deserialized
`Option<String>` on a wire document, and the `String` `Secret::new` itself consumes are all
ordinary allocations - and `String::into_boxed_str` reallocates whenever capacity exceeds length,
which leaves the original buffer freed and unwiped. Shortening that window means the secret being
parsed into this type closer to where it is read, not a stronger claim here.

There is no test for the wipe, deliberately: observing it means reading memory after free, which
is undefined behaviour, and `unsafe_code` is `forbid` in this workspace. What is tested is that
the type composes - the property above is `zeroize`'s, audited there.

#### Methods

```rust
pub fn expose_secret(&self) -> &str
```

Named to be conspicuous in review and in a grep, and named after `secrecy`'s own trait
method so the vocabulary is one word rather than two.

An inherent method rather than an `ExposeSecret` impl: the trait would have to be in scope at
every call site to be callable, which buys nothing here and makes `Secret` substitutable for
the library type in generic code - the opposite of what the newtype is for.

```rust
pub fn new(value: impl Into<String>) -> Self
```

Infallible on purpose: every string is a valid secret. There is no invariant here beyond
opacity, and a constructor that returned `Result` would be inventing one.

The canonical constructor, and the only one - `From<String>` and `From<&str>` exist on
`SecretString` and are deliberately not restated on the wrapper, so there is one place a
secret comes into existence.

#### Implements

`Clone`, `Debug`

### `use None`

### `use None`

### `use None`

### `use None`

### `use None`

### `use None`

### `use None`

### `use None`

### `use None`

### `use None`

### `use None`

### `use None`

### `use None`

### `use None`

### `use None`

### `use None`

### `use None`

### `use None`

### `use None`

### `use None`

### `use None`

### `use None`

## Module `knowledge`

What a catalog says ABOUT what it defines: the words a question arrives in, the traps a reader
has to be warned about, the things deliberately NOT defined, and worked questions.

`crate::catalog` holds what executes. This holds what a person needs in order to choose from it,
and the two are separate types on purpose: `docs/architecture.md` and `docs/concepts.md` have both
promised a glossary behind `SemanticCatalog` since before there was one, and the gap that promise
covered is the largest single difference between this repository and the reference implementation
it is measured against. A metric named `recurring_revenue` is unreachable to somebody who asks
about "monatlicher Umsatz", and a refusal naming a metric they never heard of is not an answer to
that.

# The governance invariant, stated first because it is the one that can be lost

`AGENTS.md`: *"Reading from the catalog at request time - descriptive content only, nothing that
selects, widens or parameterizes what executes."* Knowledge is descriptive **only while the
prompt is its only consumer**, and that is a property of where it is read rather than of what it
contains.

So: **never add server-side phrase resolution.** `crate::query::Query` keeps a
`MetricName` and never gains a phrase, the glossary renders into the agent-facing prompt, and
the AGENT does the resolving - which puts the resolution in the agent's own transcript, where it
is auditable, instead of inside a service whose answer would then depend on a synonym table
nobody saw. "Let sutura resolve the synonyms" is the plausible next feature, it looks like a
convenience, and it moves the choice of what executes from a name a caller sent to a phrase match
a caller did not.

There is also **no new `crate::query::RefusalReason` variant**, and the absence is structural
rather than an omission. Knowledge is checked when the bundle is loaded: a note naming a metric
that does not exist fails the load, so no request can reach a state where the knowledge is wrong.
A `PhraseNotDefined` refusal in particular would be unreachable - there is no field a caller
could put a phrase in - and a variant no test can provoke is exactly what
`crate::query::RefusalReason`'s own documentation refuses to carry.

# Four kinds, and the two that were deliberately not adopted

`GlossaryEntry`, `Caveat`, `Absence` and `Example`. The reference implementation keeps two
more, and neither is here:

* **A `rules` kind.** Five of its seven rules are already enforced by types in this crate or are
  unrepresentable here - certified metrics only, no DML, a bounded time range, a join that cannot
  duplicate rows, one data system - so restating them in the prompt is exactly what
  `sutura_app::prompt`'s module documentation argues against: text teaching an agent to attempt
  what the surface refuses by construction. The other two are a glossary entry and a caveat, which
  this module has. And a `rules` kind is the only shape among the five that would be scoped to
  NOTHING - a body of prose about the deployment at large - which is an unscoped global text
  channel from the catalog into the prompt. The injection answer depends on that channel not
  existing: every note here is attached to something the bundle declares, and
  `InconsistentKnowledge::CaveatAboutNothing` is the check that keeps it so.
* **A `certified-metrics` table.** Its definitional half is already generated from the pinned
  bundle by `sutura_app::prompt`, so a second copy would break "one owner per artefact" and drift
  the first time a measure changed. The reference implementation needs the prose table because its
  measures are SQL strings a reader cannot check; here a measure is a closed vocabulary that
  renders itself. Only the ABSENCE half of that document is information the bundle does not
  already carry - "customer lifetime value has no definition here" - and that half is `Absence`.

# Capability asymmetry: an adapter DECLARES what it supports

Not every provider has all four concepts, and the asymmetry is permanent rather than a gap to be
filled. A metadata service has glossary terms with synonyms and has nowhere at all to put a
reviewed list of what is deliberately undefined or a worked question somebody signed off; those
two are things only a reviewed first-party catalog can supply. That is the honest reason such a
catalog is not merely a degraded metadata service.

**So the adapter states its capabilities, in `KnowledgeCapabilities`, and the content is then
just content.** Declaring is the mechanism; emptiness is not evidence of anything. Two facts that
an empty collection cannot tell apart:

* `not_defined` declared, nothing in it - nothing is recorded as deliberately undefined here, and
  the record is one somebody keeps.
* `not_defined` not declared - this provider has no way to record that, so the absence of an entry
  says nothing at all.

The difference decides what the rendered prompt may CLAIM, which is the whole point of carrying
it: with `not_defined` declared, the prompt may say the list is authoritative and an agent may
decline a question on the strength of it; without it, the prompt must imply nothing whatever about
what is undefined. `sutura_app::prompt` renders that difference explicitly rather than by
omission.

Two consequences follow, and both are checks rather than intentions:

* `Knowledge::assemble` REFUSES content for a kind the adapter did not declare. A glossary entry
  arriving from a provider that declared no glossary is an adapter bug, and it fails the load at
  the boundary where it happened instead of rendering into a document that then claims a
  capability nobody said they had.
* The declaration is under the definition digest, because it changes what the agent is told. A
  deployment that silently stopped declaring `not_defined` has changed what its prompt claims, and
  provenance that did not move would certify the old claim.

**The port does not grow a method per kind.** `crate::pinned::SemanticCatalog::load` returns one
bundle carrying the declaration; a provider with no glossary declares none and writes no other
code. A trait method per kind would mean every adapter implementing four functions, most of them
returning nothing, which is the "a port arrives with its implementor" rule inverted.

# The bounds, and the measurement behind them

Every note is authored prose, and prose reaches the agent-facing prompt. So it is bounded, and
bounded **at load** rather than truncated at render: a description cut to fit makes the prompt say
something the author did not write, about a bundle whose digest certifies the text as it stands.

What was measured before the numbers were chosen. The reference implementation's entire knowledge
tree is 6231 bytes across seven files, and its largest single note is 1566 bytes. The largest prose
body already in this repository's own example catalog is 3513 bytes over 51 lines
(`revenue_per_churned_subscription.md`), and that document is at the far end of how much argument
one definition has ever needed here.

* `MAX_NOTE_BODY_BYTES` is 4 KiB - a sixth more than the longest body this repository has ever
  written, and two and a half times the longest note the reference implementation has. A note that
  does not fit is not a note, it is a document.
* `MAX_NOTE_LINES` is 200, about four times that same body's 51. It exists beside the byte cap
  rather than instead of it because the two bound different things: four thousand newlines are
  four thousand lines of a rendered prompt and well inside the byte budget.
* `MAX_KNOWLEDGE_BYTES` is 32 KiB of authored text across every note - five times the reference
  implementation's whole tree, and eight notes at the per-note cap. It exists because per-note
  caps alone let N conforming notes do what one oversized note cannot, and the thing being bounded
  is the size of one prompt rather than the size of one file.

Argued the way `crate::query::MAX_RANGE_DAYS` is argued, and with the same honesty about what is
not bounded: this caps the text, not the persuasiveness of it. Nothing here can catch prose that
misleads without escaping. What bounds that is the same thing that bounds a metric description - a
catalog is reviewed, authored content whose digest moves when a word of it changes.

### `struct Phrase`

```rust
pub struct Phrase
```

A natural-language phrase: what somebody says instead of a metric name.

**Not an identifier, and the difference is the reason for the type.** Spaces are legitimate,
non-ASCII letters are legitimate, and mixed case is meaning rather than noise - "monatlicher
Umsatz" and "monthly recurring revenue" are the values this exists to hold. So it cannot reuse
`crate::model::InvalidIdentifier`'s parser, whose whole job is to refuse those.

What it refuses is what makes a phrase unusable as one: nothing, a newline or any other control
character - a phrase is one line, and the prompt renders it inline, so a newline in one writes a
line of that document - a run of punctuation with no letter or digit in it, and anything long
enough to be a sentence.

**What it NORMALISES is the other half, and it is there so that two phrases a reader cannot tell
apart cannot both exist.** Runs of whitespace collapse to one space and the invisible code points
`crate::text::is_invisible` names are dropped, so "monthly  revenue" and "monthly revenue" are one
value, and neither a zero-width space nor a soft hyphen inside "mrr" is a second spelling of it -
the second of those is the one that was getting through, because the set here used to be a
narrower copy of the set authored SQL is held to. Case is deliberately
NOT folded here - it is meaning, and the prompt renders the spelling an author chose - which is
why identity is `phrase_identity` and not this type's `Eq`.

#### Methods

```rust
pub fn as_str(&self) -> &str
```

```rust
pub fn parse(raw: impl AsRef<str>) -> Result<Self, InvalidPhrase>
```

Parses a phrase, refusing anything that is not one and normalising what is.

#### Implements

`Clone`, `Debug`, `Deserialize<'de>`, `Display`, `Eq`, `Hash`, `Ord`, `PartialEq`, `PartialOrd`, `Serialize`

### `enum InvalidPhrase`

```rust
pub enum InvalidPhrase
```

Why a phrase was rejected.

#### Variants

- `Empty` - Empty or whitespace-only, invisible code points included. A synonym for nothing resolves everything.
- `ControlCharacter` - Holds a control character, a newline included. The prompt renders a phrase inline, so a newline here writes a line of a document nobody authored.
- `NotAWord` - No letter and no digit. **The clause that catches a scalar that is not text**: a `term: ~` in a YAML document reaches this constructor as the one-character string it prints as, and would otherwise render into the prompt as a glossary entry for a tilde. It refuses the placeholders somebody meant to fill in later for the same reason - a row of hyphens or of full stops.
- `TooLong`

#### Implements

`Debug`, `Display`, `Eq`, `Error`, `PartialEq`

### `struct NoteBody`

```rust
pub struct NoteBody
```

Bounded authored prose: the body of one note.

**Refused at load when it is over the cap, and never truncated at render.** A truncated
description makes the rendered prompt say something the author did not write, about a definition
whose digest certifies the text as it stands - and it would do so silently, because nothing
downstream of the render can tell a cut body from a short one. The module documentation records
what was measured to choose the numbers.

Newlines and tabs are content here, where `Phrase` refuses them: a body is a markdown block and
its paragraph breaks are the author's. Other control characters survive parsing and are dropped by
the renderer, which is the one place that knows what it is rendering into - so the emptiness check
is made on what the renderer will keep and a body that would draw nothing is refused rather than
rendered as a heading over blank space.

**What does NOT survive parsing is an invisible or direction-changing code point, and unlike a
`Phrase` a body is not normalised** - it is refused, naming the character. The two types differ
because what they are is different: a phrase is a key, so two spellings that read as one word have
to become one value, and a body is prose a person reviewed, so silently editing it would make the
rendered document differ from the text the definition digest certifies. This is the same argument
`crate::expression::InvalidFragment::InvisibleCharacter` makes for authored SQL, at the one
remaining channel that carried reviewed prose into an agent's context verbatim: a body reading
`status = 'active'` in every terminal and every diff, saying something else, under a digest taken
over text nobody read - CVE-2021-42574 with the fragment replaced by a paragraph.

#### Methods

```rust
pub fn as_str(&self) -> &str
```

```rust
pub fn parse(raw: impl AsRef<str>) -> Result<Self, InvalidNoteBody>
```

Parses a body, rejecting anything too large to be one.

The lengths are reported without the offending text, which is the opposite of what
`InvalidPhrase` does and is deliberate: a phrase is short enough to name in a message and a
four-kilobyte body is not, so the error says how much there was and where the limit is.

#### Implements

`Clone`, `Debug`, `Deserialize<'de>`, `Eq`, `PartialEq`, `Serialize`

### `enum InvalidNoteBody`

```rust
pub enum InvalidNoteBody
```

Why a note body was rejected.

#### Variants

- `Empty` - Nothing a reader would see. A note with no body is a claim with no reason attached, and the prompt would render a heading over empty space - so this covers whitespace, control characters the renderer drops, and the zero-width code points that draw nothing, as well as the empty string.
- `InvisibleCharacter` - One of them, mixed into prose. **Separate from `Self::Empty`, because a body made ENTIRELY of these characters was already refused and a body with one in the middle of a sentence was not** - and the second is the dangerous one: the first renders as a blank heading somebody notices, the second renders as a paragraph that reads correctly and is not what it says.
- `TooLong` - Over `MAX_NOTE_BODY_BYTES`. The document does not load; it is not shortened.
- `TooManyLines` - Over `MAX_NOTE_LINES`. Separate from the byte cap because four thousand newlines are four thousand lines of a rendered prompt and eight kilobytes of nothing.

#### Implements

`Debug`, `Display`, `Eq`, `Error`, `PartialEq`

### `struct NoteName`

```rust
pub struct NoteName
```

The name of one note: the handle a reviewer, a log line or an error uses for it.

 An identifier and not a `Phrase`, because it names a document rather than saying anything: a
 caveat is referred to by name in a review the way a metric is, and the parser that keeps a
 metric name spellable keeps this one greppable. The macro is `crate::model`'s, so there is
 one identifier parser in this crate and nothing for a second one to drift from.

Construct it with `parse`. There is no other way in: the field is private and
`Deserialize` is routed through the same constructor, so a value that is not a legal
identifier does not exist to be passed anywhere.

#### Methods

```rust
pub fn as_str(&self) -> &str
```

```rust
pub fn parse(raw: impl AsRef<str>) -> Result<Self, $crate::model::InvalidIdentifier>
```

Parses a name, rejecting anything that is not one.

#### Implements

`Clone`, `Debug`, `Deserialize<'de>`, `Display`, `Eq`, `Hash`, `Ord`, `PartialEq`, `PartialOrd`, `Serialize`

### `enum Referent`

```rust
pub enum Referent
```

What one note is about: something the pinned bundle declares.

**There is deliberately no variant for a model, a table or a column, and that absence is load
bearing rather than tidy.** A caller cannot ask about any of the three - `crate::query::Query` has no field
for one - and a name in an agent's context is a name it will eventually try to use. Every
STRUCTURED rendering `sutura_app::prompt` builds out of a note - the glossary line, a caveat's
scope, the request in a worked question - is rendered from a `Referent`, so none of them CAN name
a model, a table or a column, whatever an author writes. That is the claim the type holds up, and
it is worth stating at its real width:

* **The structured renderings cannot name one.** There is no variant to put it in, so this half
  is a property of the type rather than a review of each rendering.
* **`Phrase` and `NoteBody` are free text, and both reach the rendered document.** Nothing
  here stops an author writing a column name into a glossary term or a note body, and the
  pre-existing metric-description channel already carries such names into the prompt - the
  shipped example catalog's own prose names `mrr_cents` and `status`, and
  `sutura-cli`'s prompt snapshot records that it does. Prose is bounded, authored, reviewed
  content whose digest moves when a word of it changes; it is not mechanically constrained, and
  claiming otherwise would be claiming the wrong mechanism.

A load-time scan of every phrase and body for the bundle's own model, table and column names
would close the second half. It is not here: it is a larger change than the type-level property
needs, it would make an authored note refuse for naming a column in a sentence about why the
column is not the thing being asked for, and the honest statement of what holds is the cheaper
half of it.

It carries `Deserialize` as well as `Serialize`, for the same reason `crate::measure::Measure`
does: this IS the on-disk shape, and a mirror of it in the adapter would be a second place to
forget the next variant.

**Flat on disk, and read through `ReferentRepr` rather than by an external tag**, which is
`crate::measure::Term`'s decision and its argument applies here unchanged. The one-key mapping
the rest of this format uses would spell the commonest referent
`means: { metric: { metric: recurring_revenue } }`: the tag word and the field word are the same
word, so the nesting says nothing. `#[serde(untagged)]` is not the way out either - it reports
"data did not match any variant", which names nothing. So a referent is one flat mapping with a
`deny_unknown_fields` struct behind it, a misspelled key is an error naming the typo, and the one
combination that is not a referent - a value with no dimension - is an `InvalidReferent` that
says so.

#### Variants

- `Metric` - The metric as a whole.
- `Dimension` - One dimension of one metric. Scoped to the metric because a dimension is: two metrics may declare `segment` over the same column and still not permit the same questions about it.
- `Value` - One declared value of one dimension of one metric.

#### Methods

```rust
pub const fn dimension(&self) -> Option<&DimensionName>
```

The dimension, when this referent names one.

```rust
pub const fn metric(&self) -> &MetricName
```

The metric every referent is scoped to.

```rust
pub const fn value(&self) -> Option<&DimensionValue>
```

The value, when this referent names one.

#### Implements

`Clone`, `Debug`, `Deserialize<'de>`, `Eq`, `Ord`, `PartialEq`, `PartialOrd`, `Serialize`

### `enum InvalidReferent`

```rust
pub enum InvalidReferent
```

Why a referent was rejected.

One variant, because there is one combination of the three fields that is not a referent. A
missing `metric` is a serde missing-field error naming the field, which is a better message than
anything this enum could produce for it.

#### Variants

- `ValueWithoutDimension`

#### Implements

`Debug`, `Display`, `Eq`, `Error`, `PartialEq`

### `enum Capability`

```rust
pub enum Capability
```

One thing a provider can declare it supports.

A closed set, for the reason `crate::model::Aggregate` is one: the alternative is a string, and a
provider that declared `"glossaries"` or `"Glossary "` would silently declare nothing at all. With
an enum, an unrecognised capability is a parse error naming what was written and listing what
exists.

**The variants are named after the domain concepts rather than after the words a document writes.**
`Absences` here is `kind: not_defined` in a markdown file; the format's word says what an author is
doing, and this one says what the thing is.

#### Variants

- `Glossary` - The business glossary: phrases, and the one thing each of them means.
- `Caveats` - Notes a reader has to see before trusting a number.
- `Absences` - A reviewed list of terms that are deliberately not defined.
- `Examples` - Worked questions: how somebody asked, and what to send.

#### Methods

```rust
pub const fn as_str(self) -> &'static str
```

The word this capability answers to, in a message and in a declaration.

```rust
pub fn every() -> impl Iterator<Item>
```

Every capability there is, in declaration order.

Derived from `Self::next` rather than listed, and seeded by the one variant
`Self::previous` answers `None` for - which is asserted where this enum is declared rather
than assumed by whoever reads the line.

#### Implements

`Clone`, `Copy`, `Debug`, `Deserialize<'de>`, `Display`, `Eq`, `Hash`, `Ord`, `PartialEq`, `PartialOrd`, `Serialize`

### `struct KnowledgeCapabilities`

```rust
pub struct KnowledgeCapabilities
```

What one provider declares it supports.

**The declaration is the mechanism, and emptiness is not evidence.** A metadata service has
glossary terms and no way to record that a term is deliberately undefined; a reviewed first-party
catalog has both. Which of the two is talking decides what the rendered prompt may claim, and an
empty collection cannot say - so the adapter says, here, and the content is then just content.

A `BTreeSet` rather than four booleans: the order is content order, which the digest needs, and
adding a capability does not add a field to every construction site. Wrapped in a newtype with the
usual treatment - private field, one constructor, accessors, no `Deref` - because
`xtask check-boundaries` fails a `pub` field on a `pub struct` and the rule does not make an
exception for a type with no invariant to protect.

The constructor is infallible, for the reason `crate::identity::Secret::new` gives: any set of
capabilities is a legitimate declaration, and a `Result` here would be inventing an invariant. What
is NOT legitimate is content for a capability that was not declared, and that is
`Knowledge::assemble`'s to refuse.

#### Methods

```rust
pub fn all() -> Self
```

Every capability there is.

**What a REFERENCE adapter declares, and it means more than "all four today".** A provider
calling this is saying it supports whatever kinds exist, including ones added later - which is
true of a reviewed first-party catalog, whose format grows with the domain, and is not true of
anything that maps a fixed external schema. Such an adapter lists its capabilities with
`Self::of`, so a new kind leaves its declaration alone.

```rust
pub const fn declared(&self) -> &BTreeSet<Capability>
```

Everything declared, in a deterministic order.

```rust
pub fn declares(&self, capability: Capability) -> bool
```

Does this provider support that kind at all?

```rust
pub fn is_empty(&self) -> bool
```

Is nothing at all declared?

Read by the prompt: a provider that declares nothing gets no section about what it records,
because there is no distinction to draw and four lines saying "not recorded" is noise rather
than information.

```rust
pub const fn none() -> Self
```

A provider with none of them.

```rust
pub fn of(capabilities: impl IntoIterator<Item>) -> Self
```

The capabilities a provider says it has.

#### Implements

`Clone`, `Debug`, `Deserialize<'de>`, `Eq`, `PartialEq`, `Serialize`

### `use None`

### `use None`

### `use None`

### `use None`

### `use None`

### `use None`

### `use None`

### `constant MAX_NOTE_BODY_BYTES`

The longest note body, in bytes.

### `constant MAX_NOTE_LINES`

The most lines one note body may have.

### `constant MAX_KNOWLEDGE_BYTES`

The most authored text a whole `Knowledge` may carry, in bytes.

### `type_alias Glossary`

The glossary, keyed by the term each entry defines.

An alias rather than the map written out at every use site, and not only for reading: the
workspace's `type_complexity` threshold is 100 against clippy's default 250, so
`Result<BTreeMap<Phrase, GlossaryEntry>, InconsistentKnowledge>` is a lint. Naming the four
collections is the fix the lint asks for and the one that makes the accessors read as what they
return.

### `type_alias Caveats`

The caveats, keyed by name.

### `type_alias Absences`

The terms declared undefined, keyed by the phrase each note is about.

### `type_alias Examples`

The worked examples, keyed by name.

## Module `measure`

What a metric measures, and the filters that are part of its definition rather than of a
question.

**A closed vocabulary, and the axis it is closed along is the term rather than the shape.** The
first version of this crate allowed exactly one aggregate over one column, which was safe and
could not express the metrics people actually certify: a revenue that means "active subscriptions
only", an average revenue per user that is one aggregate divided by another, a churn rate that is
a conditional count over a distinct count. Two of seven real metrics fitted; five did not.

The second version bought most of it back with three sibling shapes - `simple`, `count_if`,
`ratio` - and left the seventh metric unsayable, for a reason that was structural rather than
incidental. A conditional count was a *shape*, so it could not be a *half* of a ratio, and
`count_if(churned) / count_distinct(subscription)` had every ingredient present and no way to
write it. Adding `count_if_over_x` shapes would have been the same mistake once per numerator.

So the vocabulary is two levels: a `Term` is what one number is computed from, and a
`Measure` is one term or a ratio of two. Widening it is adding a `Term`, once, and every shape
gets the new term for free.

The security property was never "one aggregate". It was **no free-text SQL**: every leaf is a
column the model declares, every operation is a variant the generator has an arm for, and there
is no string anywhere that reaches a statement unexamined. Two shapes, two terms and four
predicates buy back the expressiveness while keeping exactly that.

What is still unrepresentable, deliberately: an expression over two columns
(`sum(price * quantity)`), a window function, a three-table join. Those need an expression
language, and an expression language on this path is the escape hatch
`docs/adr/0001-first-party-semantic-models.md` argues against. They belong to a definition
rendered upstream and taken as given.

### `struct AggregatedColumn`

```rust
pub struct AggregatedColumn
```

One aggregate applied to one declared column.

`Count` is the case where the column is not read and still has to be named: a `COUNT(*)` over a
joined result counts join products rather than facts, so naming the column is what makes the
generated count count the thing the model says it counts.

Carries no `serde` derive. A term's on-disk shape belongs to `TermRepr` and to nothing else, so
there is exactly one place where the format of `{ aggregate: sum, column: x }` is decided.

#### Methods

```rust
pub const fn aggregate(&self) -> Aggregate
```

```rust
pub const fn column(&self) -> &ColumnName
```

```rust
pub const fn new(aggregate: Aggregate, column: ColumnName) -> Self
```

#### Implements

`Clone`, `Debug`, `Eq`, `PartialEq`

### `enum Term`

```rust
pub enum Term
```

One number a measure is computed from.

**The extensible axis.** A shape says how terms combine; a term says what one of them is. That
split is what makes a conditional count usable as a ratio's numerator, which is the metric the
previous vocabulary could not say with every one of its ingredients already present.

**Flat on disk, and read through `TermRepr` rather than by an external tag.** The one-key
mapping the rest of this format uses would spell the common half of a ratio
`numerator: { aggregate: { aggregate: sum, column: mrr_cents } }`: the tag word and the field
word are the same word, so the nesting says nothing and every existing `simple:` document in
every catalog would have to be rewritten to gain it. `#[serde(untagged)]` is not the way out
either - it reports "data did not match any variant", which names nothing, and `document.rs`
carries a test that exists to keep that error out of this format.

So a term is one flat mapping with a `deny_unknown_fields` struct behind it, and the word that
says which term it is - `aggregate` or `count_if` - is a key the author writes rather than a
shape inferred from an absence. A misspelled key is still an error naming the typo, an
unrecognised term is an error naming the word that was written, and a document that writes half
of one or both of them gets a `InvalidTerm` that says which.

#### Variants

- `Aggregate` - One aggregate over one column: `SUM(amount_cents)`.
- `CountIf` - How many rows have this boolean column true.

#### Methods

```rust
pub const fn column(&self) -> &ColumnName
```

The column this term reads.

```rust
pub const fn kind(&self) -> &'static str
```

The name of this term, for a refusal or a description.

#### Implements

`Clone`, `Debug`, `Deserialize<'de>`, `Display`, `Eq`, `PartialEq`, `Serialize`

### `enum InvalidTerm`

```rust
pub enum InvalidTerm
```

Why a term was rejected.

Four variants rather than one message, because each of them is a different mistake and the
variant is what says which. A single "invalid term" would send an author back to compare their
line against a grammar.

#### Variants

- `Empty` - Nothing was written. The mapping parsed and named no term at all.
- `TwoTerms` - Both terms at once. Refused rather than resolved by precedence: a document that writes both means one of them, and picking one would certify a number the author did not ask for.
- `NoColumn`
- `NoAggregate`

#### Implements

`Debug`, `Display`, `Eq`, `Error`, `PartialEq`

### `enum ZeroDenominator`

```rust
pub enum ZeroDenominator
```

What a zero denominator means.

An enum and not a boolean, because `zero_safe: true` records that somebody thought about it and
not what they decided. Both behaviours are defensible - a rate over an empty period is arguably
null and arguably an error - the difference shows up only in the periods where it matters, and a
definition should say which one it means in a word a reader can check against the metric's prose.

**The on-disk words are `yields_null` and `fails`, and the first one is not cosmetic.** The
obvious spelling of the null case is `null`, and in YAML `null` is the null literal: a document
writing `zero_denominator: null` would hand the deserializer a unit value, and the author of the
most natural spelling in the vocabulary would get a type error about a line that looks right.
Naming the variants after what a zero denominator *does* means the field and its value read as
one sentence and neither of them can collide with a scalar YAML resolves itself.

#### Variants

- `Null` - The measure is null for that row. The generator guards the denominator - a `NULLIF`, or the dialect's own safe-divide.
- `Fail` - The division is emitted unguarded, and the fault is raised where the value crosses back into the domain. A definition choosing this is saying an empty period is a fault and not a figure.

#### Methods

```rust
pub const fn as_str(self) -> &'static str
```

The word a catalog writes, and the word a description reads back.

#### Implements

`Clone`, `Copy`, `Debug`, `Deserialize<'de>`, `Eq`, `PartialEq`, `Serialize`

### `enum Measure`

```rust
pub enum Measure
```

What a metric measures.

Externally tagged, so a document names its shape: `simple:` or `ratio:`. That makes a measure's
shape a word an author writes rather than something inferred from which fields are present, and
it makes an unrecognised shape an error naming what it found.

#### Variants

- `Simple` - One term: `SUM(amount_cents)`, or a conditional count.
- `Ratio` - One term divided by another: an average revenue per user, a rate, a share.

#### Methods

```rust
pub fn columns(&self) -> Vec<&ColumnName>
```

Every column this measure reads.

One place, so `crate::catalog::Definitions` can check them all against the model without
knowing the shapes, and so a shape added here cannot be forgotten there.

```rust
pub const fn shape(&self) -> &'static str
```

The name of this shape, for a refusal or a description.

```rust
pub fn terms(&self) -> Vec<&Term>
```

The terms this measure is computed from, in the order a reader would say them.

One place, so a shape added here is a shape everything that walks terms already handles.

#### Implements

`Clone`, `Debug`, `Deserialize<'de>`, `Display`, `Eq`, `PartialEq`, `Serialize`

### `enum RequiredFilter`

```rust
pub enum RequiredFilter
```

A predicate that is part of what a metric means.

**Definitional, not a question.** `mrr` means recurring revenue *from active subscriptions*, and
a statement that omits that predicate returns a different number under the same name - the exact
failure this repository exists to prevent, arrived at by omission rather than by tampering. So a
required filter is applied to every question about the metric, and a caller cannot see it, choose
it or turn it off.

Its values come from the catalog rather than from a caller, and they are still bound as
parameters rather than written into the statement. Not because the catalog is untrusted in the
way a caller is, but because a value that is sometimes inlined and sometimes bound is a generator
with two paths, and the inlining path is the one that would eventually be handed caller text.

**The value is a `DimensionValue` rather than a `String`, and that is the decision worth
recording here.** It was the last authored string on the wired path with no character rule on it:
it deserializes straight out of a metric document's frontmatter, and it reaches a person twice -
`sutura-cli`'s `definitions` command prints it beside the measure, which is where somebody
deciding whether a metric means what it claims reads it. A right-to-left override inside
`status = 'active'` made that line render one way and the bound parameter another, which is the
finding `crate::expression::SqlFragment` and `crate::knowledge::Phrase` already closed,
arriving at a third channel. `DimensionValue` is the type that already refuses it, and the
argument its documentation makes for using one type on both sides of the caller/allowlist pair
applies again here: a definitional filter's value is compared against the same column a caller's
filter is, so a second, laxer character rule on this side would be a rule nothing compares
against the first.

The cost is the one that type states: a column whose values genuinely carry a tab, a no-break
space or a double space cannot be filtered on - by a caller or by a definition. It also bounds
the length at `crate::catalog::MAX_DIMENSION_VALUE_CHARS`, which this field did not have.

Externally tagged for the same reason `Measure` is: the operator is a word, not an inference.

#### Variants

- `Equals` - `column = value`.
- `NotEquals` - `column <> value`. Note what this does NOT match in SQL: a null column. `NotEquals` on a nullable column excludes null rows, and a definition that means "everything except x, including unknown" needs `IsNull` beside it - which does not exist yet, because nothing has needed it.
- `IsTrue` - `column IS TRUE`, for a boolean column.
- `IsNotNull` - `column IS NOT NULL`.

#### Methods

```rust
pub const fn column(&self) -> &ColumnName
```

```rust
pub const fn value(&self) -> Option<&DimensionValue>
```

The value this filter compares against, if it compares against one.

`None` for the two that need no value, which is what tells the generator whether to emit a
placeholder and the plan whether to bind a parameter.

#### Implements

`Clone`, `Debug`, `Deserialize<'de>`, `Display`, `Eq`, `PartialEq`, `Serialize`

## Module `model`

The vocabulary a semantic model is written in: the names it uses, and the closed sets it
chooses from.

Every name here ends up inside a quoted identifier in generated SQL, and every aggregate and
grain ends up as a keyword the generator emits. So the parsing is deliberately narrow: a value
that would need escaping, or that names an operation we cannot spell, must not exist to be
passed to the generator in the first place.

**There is no free-text expression type in this module, and that is the point.** A measure is an
aggregate over a column, never the string `sum(amount)`; a relationship is a pair of columns,
never the string `a.x = b.y`. `docs/adr/0001-first-party-semantic-models.md` argues why: a string
field is an escape hatch, and an escape hatch on the query path is the thing being defended
against.

### `enum IdentifierCase`

```rust
pub enum IdentifierCase
```

Whether a data system tells two identifiers in one statement apart by case.

**The vocabulary lives here and the declaration lives on `sutura_sql::Dialect`**, which is the
shape `Qualification` already has and for the same reason: the domain names what the difference
IS, a target answers for itself, and a comparison decides. One type rather than two so the two can
be compared.

# What it covers, and it is more than the word "alias" suggests

Three resolutions in a generated statement read an identifier back, and a target that folds case
folds all three:

- the identifier a column is qualified by - `orders` in `orders.amount_cents`, which is the
  IMPLICIT alias `FROM a.b.orders` gives the table;
- the alias a projected column is labelled with, which `GoogleSQL` resolves ahead of a table of
  the same spelling - the wrong-answer report behind
  [`LabelShadowsTable`](crate::catalog::InconsistentDefinitions::LabelShadowsTable);
- the name of a result column, which is what makes two labels one column rather than two.

# Why the catalog and the plan both compare under `Self::COARSEST`

A bundle is dialect-agnostic: the same definitions are served against whichever target a
deployment opened, and nothing at load knows which. So the identity two identifiers are compared
under has to be the coarsest any target uses, and refusing a pair that one target would have told
apart costs a catalog author a rename - while accepting a pair the *serving* target folds is a
wrong number under a certified name. That asymmetry is the whole argument, and it is the same one
`check_labels_against_table` already makes for refusing the whole cross product.

# The mechanism, and what it is not

`sutura_sql::Dialect::identifier_case` is an exhaustive match, so a fifth target cannot compile
without answering; and a test there asserts every declared value is no coarser than
`Self::COARSEST`, so a variant added below that folds MORE than ASCII case - Unicode folding,
say - fails that assertion instead of silently invalidating the comparison the catalog makes.

**A declaration in the `Sensitive` direction cannot make a bundle unsafe**, and that is worth
stating because two of the four are not measured here: the catalog and the plan compare under
`COARSEST` whatever a dialect declares, so the declaration's only consumer is that assertion.
What it buys is that the assumption is written down per target rather than asserted once in prose.

**The variant order is load-bearing and is asserted rather than assumed.** The derived `Ord` on
an enum is declaration order, so `Sensitive < InsensitiveAscii` is what makes
`dialect.identifier_case() <= IdentifierCase::COARSEST` mean *folds at most as much as*.

#### Variants

- `Sensitive` - Two identifiers differing only in case name two different things.
- `InsensitiveAscii` - Two identifiers differing only in ASCII case name one thing.

#### Methods

```rust
pub const fn as_str(self) -> &'static str
```

```rust
pub fn names_one_thing(self, left: &str, right: &str) -> bool
```

Do these two identifiers name one thing under this rule?

ASCII rather than Unicode folding, deliberately and for the reason `Phrase::parse` gives for
the same choice: there is no NFC/NFD anywhere in this workspace, so a decomposed spelling and
a homoglyph are each a second identifier - and `parse_identifier` admits neither, because it
accepts `[A-Za-z0-9_]` and nothing else. So on the values this type is ever handed, ASCII
folding IS full folding.

#### Implements

`Clone`, `Copy`, `Debug`, `Display`, `Eq`, `Hash`, `Ord`, `PartialEq`, `PartialOrd`, `Serialize`

### `enum InvalidIdentifier`

```rust
pub enum InvalidIdentifier
```

Why a name was rejected.

The variants carry the offending input as typed fields rather than a formatted sentence: the
variant is the contract and the `#[error]` text is a convenience for a human.

#### Variants

- `Empty` - Empty or whitespace-only. An unnamed column is a modelling mistake, not a wildcard.
- `BadFirstCharacter` - Starts with something other than a letter or underscore. A leading digit is legal in some dialects and not others, so accepting it would make a model portable by luck.
- `IllegalCharacter` - Contains a character that is not `[A-Za-z0-9_]`. `offending` is the first one, which is the one worth reporting: a message naming all of them tells the reader less.
- `TooLong` - Longer than a target data system will keep. The limit is 63 characters, the tightest among the data systems targeted here.
- `TrailingHyphen` - Ends in a hyphen.

#### Implements

`Debug`, `Display`, `Eq`, `Error`, `PartialEq`

### `enum Hyphens`

```rust
pub enum Hyphens
```

Whether a hyphen is a character this name may contain.

**A parameter rather than a second parser, for the reason `identifier_newtype` gives:** the
names in this module are used interchangeably by the resolver, so two parsers would be two places
for the answer to differ. One body, one flag, one error enum.

**What this flag may NOT be widened to admit, because two golden claims rest on it.** The corpus
assertions in `sutura-app` strip quoted spans out of a statement with a single toggle, and that is
sound only because no name can contain `"`, `'` or `` ` ``. A hyphen is none of those, so
admitting one leaves the argument intact - and that is the *whole* licence this flag has. A
variant admitting a quote character, a dot or whitespace would silently invalidate the stripping
rather than fail a test.

#### Variants

- `Rejected` - `[A-Za-z_][A-Za-z0-9_]*`. Every identifier a model, a metric or a column is named with.
- `Allowed` - `[A-Za-z_][A-Za-z0-9_-]*`, not ending in `-`. The one shape that needs it is a cloud project id, which is where a table's topmost qualifier comes from.

#### Implements

`Clone`, `Copy`, `Debug`, `Eq`, `PartialEq`

### `struct ModelName`

```rust
pub struct ModelName
```

The name of a model: one physical table plus what we know about it.

Construct it with `parse`. There is no other way in: the field is private and
`Deserialize` is routed through the same constructor, so a value that is not a legal
identifier does not exist to be passed anywhere.

#### Methods

```rust
pub fn as_str(&self) -> &str
```

```rust
pub fn parse(raw: impl AsRef<str>) -> Result<Self, $crate::model::InvalidIdentifier>
```

Parses a name, rejecting anything that is not one.

#### Implements

`Clone`, `Debug`, `Deserialize<'de>`, `Display`, `Eq`, `Hash`, `Ord`, `PartialEq`, `PartialOrd`, `Serialize`

### `struct TableName`

```rust
pub struct TableName
```

The name of a physical table, as the data system knows it.

Construct it with `parse`. There is no other way in: the field is private and
`Deserialize` is routed through the same constructor, so a value that is not a legal
identifier does not exist to be passed anywhere.

#### Methods

```rust
pub fn as_str(&self) -> &str
```

```rust
pub fn parse(raw: impl AsRef<str>) -> Result<Self, $crate::model::InvalidIdentifier>
```

Parses a name, rejecting anything that is not one.

#### Implements

`Clone`, `Debug`, `Deserialize<'de>`, `Display`, `Eq`, `Hash`, `Ord`, `PartialEq`, `PartialOrd`, `Serialize`

### `struct ColumnName`

```rust
pub struct ColumnName
```

The name of a column on a physical table.

Construct it with `parse`. There is no other way in: the field is private and
`Deserialize` is routed through the same constructor, so a value that is not a legal
identifier does not exist to be passed anywhere.

#### Methods

```rust
pub fn as_str(&self) -> &str
```

```rust
pub fn parse(raw: impl AsRef<str>) -> Result<Self, $crate::model::InvalidIdentifier>
```

Parses a name, rejecting anything that is not one.

#### Implements

`Clone`, `Debug`, `Deserialize<'de>`, `Display`, `Eq`, `Hash`, `Ord`, `PartialEq`, `PartialOrd`, `Serialize`

### `struct MetricName`

```rust
pub struct MetricName
```

The name of a metric: something somebody certified, such as revenue or active subscribers.

Construct it with `parse`. There is no other way in: the field is private and
`Deserialize` is routed through the same constructor, so a value that is not a legal
identifier does not exist to be passed anywhere.

#### Methods

```rust
pub fn as_str(&self) -> &str
```

```rust
pub fn parse(raw: impl AsRef<str>) -> Result<Self, $crate::model::InvalidIdentifier>
```

Parses a name, rejecting anything that is not one.

#### Implements

`Clone`, `Debug`, `Deserialize<'de>`, `Display`, `Eq`, `Hash`, `Ord`, `PartialEq`, `PartialOrd`, `Serialize`

### `struct DimensionName`

```rust
pub struct DimensionName
```

The name of a dimension a metric declares it can be broken down by.

Construct it with `parse`. There is no other way in: the field is private and
`Deserialize` is routed through the same constructor, so a value that is not a legal
identifier does not exist to be passed anywhere.

#### Methods

```rust
pub fn as_str(&self) -> &str
```

```rust
pub fn parse(raw: impl AsRef<str>) -> Result<Self, $crate::model::InvalidIdentifier>
```

Parses a name, rejecting anything that is not one.

#### Implements

`Clone`, `Debug`, `Deserialize<'de>`, `Display`, `Eq`, `Hash`, `Ord`, `PartialEq`, `PartialOrd`, `Serialize`

### `struct RelationshipName`

```rust
pub struct RelationshipName
```

The name of a declared relationship between two models.

Construct it with `parse`. There is no other way in: the field is private and
`Deserialize` is routed through the same constructor, so a value that is not a legal
identifier does not exist to be passed anywhere.

#### Methods

```rust
pub fn as_str(&self) -> &str
```

```rust
pub fn parse(raw: impl AsRef<str>) -> Result<Self, $crate::model::InvalidIdentifier>
```

Parses a name, rejecting anything that is not one.

#### Implements

`Clone`, `Debug`, `Deserialize<'de>`, `Display`, `Eq`, `Hash`, `Ord`, `PartialEq`, `PartialOrd`, `Serialize`

### `struct SourceName`

```rust
pub struct SourceName
```

The name of a data system a model's table lives in.

 A plan resolves to exactly one of these, so it is the value that decides whether a question
 is answerable at all rather than a routing hint.

Construct it with `parse`. There is no other way in: the field is private and
`Deserialize` is routed through the same constructor, so a value that is not a legal
identifier does not exist to be passed anywhere.

#### Methods

```rust
pub fn as_str(&self) -> &str
```

```rust
pub fn parse(raw: impl AsRef<str>) -> Result<Self, $crate::model::InvalidIdentifier>
```

Parses a name, rejecting anything that is not one.

#### Implements

`Clone`, `Debug`, `Deserialize<'de>`, `Display`, `Eq`, `Hash`, `Ord`, `PartialEq`, `PartialOrd`, `Serialize`

### `enum Aggregate`

```rust
pub enum Aggregate
```

The aggregates a measure may use.

A closed set, and the reason is the whole of
`docs/adr/0001-first-party-semantic-models.md`: an open set would be a string, and a string is
SQL somebody wrote. Adding a variant is a visible diff plus a generator arm plus a golden, which
is the review it deserves.

#### Variants

- `Sum`
- `Count`
- `CountDistinct`
- `Avg`
- `Min`
- `Max`

#### Methods

```rust
pub const fn as_str(self) -> &'static str
```

The name this aggregate is written with in a catalog, and in a refusal.

Not the SQL spelling: how an aggregate is spelled differs per dialect and belongs to the
generator. A domain type that knew the SQL would be a domain type that had opinions about
`ClickHouse`.

#### Implements

`Clone`, `Copy`, `Debug`, `Deserialize<'de>`, `Display`, `Eq`, `Hash`, `PartialEq`, `Serialize`

### `enum Grain`

```rust
pub enum Grain
```

The time resolutions an answer may be aggregated to.

Daily revenue and monthly revenue are the same metric at two grains, not two metrics, which is
why this is a parameter of a question rather than part of a metric's name.

#### Variants

- `Day`
- `Week`
- `Month`
- `Quarter`
- `Year`

#### Methods

```rust
pub const fn as_str(self) -> &'static str
```

#### Implements

`Clone`, `Copy`, `Debug`, `Deserialize<'de>`, `Display`, `Eq`, `Hash`, `Ord`, `PartialEq`, `PartialOrd`, `Serialize`

### `enum JoinType`

```rust
pub enum JoinType
```

How many rows on each side of a relationship a join may match.

Recorded because it decides whether a join can change a measure's value. Joining to a
`ManyToOne` side cannot duplicate a fact row; joining to a `OneToMany` side can, which turns a
`sum` into a different number without any error anywhere. The resolver uses this to refuse
rather than to optimise.

#### Variants

- `OneToOne`
- `ManyToOne`
- `OneToMany`

#### Methods

```rust
pub const fn as_str(self) -> &'static str
```

```rust
pub const fn may_duplicate_rows(self) -> bool
```

Can a join along this relationship duplicate rows of the model it starts from?

A `sum` over duplicated rows is a wrong number that looks like a right one, so the answer
decides a refusal rather than a plan detail.

#### Implements

`Clone`, `Copy`, `Debug`, `Deserialize<'de>`, `Eq`, `Hash`, `PartialEq`, `Serialize`

### `use None`

### `use None`

### `use None`

### `use None`

### `use None`

### `use None`

## Module `pinned`

The snapshot a question is answered against, and the port it arrives through.

Definitions do not arrive live. They arrive as a `PinnedDefinitions`: a whole
`crate::catalog::Definitions`, the `crate::knowledge::Knowledge` written about it, a version,
and a digest over the canonical form of both. Two consequences follow, and both are the reason
this type exists rather than passing `Definitions` around directly.

A catalog edit cannot change what a question means between two invocations, because the bundle a
request resolves against was fixed before the request arrived. It changes the digest instead, and
the digest travels with the answer.

And the catalog cannot see who is asking. `SemanticCatalog::load` takes no request context, so
there is nothing for an implementation to branch on. A trait that accepted one could return a
different definition to different callers, which would make the pinning meaningless and the
provenance a lie.

**One thing this module deliberately does NOT hold, and one it now does.** It cannot execute a
statement, so it holds `AnchorReport` - the evidence - and `AnchorReport::verdict` - the rule -
but not the proof. The proof is `sutura_app::Validated`, whose only constructor is
`sutura_app::verify_and_validate` and therefore cannot be reached without a `Warehouse` having
been called. A report is public data anybody can build, and nothing anybody builds here turns
into a bundle the service will serve.

What it does hold is the hashing. `PinnedDefinitions::pin` takes a version, a set of definitions,
the knowledge about them and the composition that produced both, and nothing else: the digest is
computed here, from the values being stored, by `DefinitionDigest::of`. The previous shape took the
hash *function* from its caller, on the argument that the domain could not hash - and that left the
hole intact, because a function handed the definitions is not a function that read them.
`crate::definitions` says what the twelve allowlisted crates bought, and `crate::pinned::manifest`
is the fourth piece of content that made the digest cover the composition.

### `struct DefinitionVersion`

```rust
pub struct DefinitionVersion
```

Which snapshot of the definitions this is.

Free-form on purpose, because what identifies a snapshot differs per catalog: a commit id, a
build number, an export timestamp. What is *not* free-form is its shape, because it is echoed
into provenance and into an audit record, and a value with a newline in it can forge a second
record.

#### Methods

```rust
pub fn as_str(&self) -> &str
```

```rust
pub fn parse(raw: impl AsRef<str>) -> Result<Self, InvalidVersion>
```

#### Implements

`Clone`, `Debug`, `Deserialize<'de>`, `Display`, `Eq`, `Hash`, `PartialEq`, `Serialize`

### `enum InvalidVersion`

```rust
pub enum InvalidVersion
```

Why a version label was rejected.

#### Variants

- `Empty` - Empty or whitespace-only. An unversioned snapshot must not be able to claim it is one.
- `ControlCharacter` - Holds a control character. This is the one that matters: provenance is written to an audit sink line by line, so a newline here appends a record nobody wrote.
- `InvisibleCharacter` - Holds an invisible or direction-changing code point. **The second half of the reason the variant above exists**, and it was missing: `char::is_control` is false for every one of these (general category `Cf`, not `Cc`), so the check that refuses a newline could not see a right-to-left override, and a version label carrying one passed.
- `TooLong`

#### Implements

`Debug`, `Display`, `Eq`, `Error`, `PartialEq`

### `struct Provenance`

```rust
pub struct Provenance
```

What defined an answer, and what each of its legs executed as - travelling with it.

A result cannot be separated from what defined it, so this is a typed field a caller reads
deliberately rather than a sentence concatenated into a channel that also carries instructions.

# Two halves with two different owners, and the posture is BESIDE the digest rather than under it

The version and the digest identify the *authored content*: two deployments serving the same
catalog certify the same numbers, which is the one property the digest exists to have. The
posture per leg is *deployment configuration* - the same bundle may be served by a deployment that
impersonates and one that does not - so hashing it in would make one catalog produce two digests
in two deployments. That is why `crate::source::ExecutedAs` is a field here and not an input to
`PinnedDefinitions::pin`, and it is the opposite of the knowledge declaration, which *is* under
the digest because it is content a catalog author wrote.

**Recording is not a control.** Provenance is read by whoever holds the answer, after the rows
were served, so it cannot prevent a disclosure and does not attempt to. It makes one attributable
and it makes a misconfiguration visible to whoever reads an answer; the thing that keeps a shared
source from being served unnoticed is a boot refusal.

#### Methods

```rust
pub const fn digest(&self) -> &DefinitionDigest
```

```rust
pub const fn executed_as(&self) -> &ExecutedAs
```

What each leg of this answer ran as.

Read off the posture the **adapter was handed**, never off a settings tree - see
`crate::source`. Non-empty, because `ExecutedAs` has no empty form.

```rust
pub const fn version(&self) -> &DefinitionVersion
```

#### Implements

`Clone`, `Debug`, `Eq`, `PartialEq`, `Serialize`

### `struct PinnedDefinitions`

```rust
pub struct PinnedDefinitions
```

An immutable, hashed snapshot of everything a catalog said.

**Two halves, and the split between them is a governance boundary rather than a filing decision.**
`Definitions` is exactly what the compiler reads - `sutura_semantic` resolves a question against
it and nothing else - and `Knowledge` is what a person reads: the glossary, the caveats, the
terms deliberately undefined, the worked questions. Keeping the second out of the first is what
makes "descriptive content only" checkable: the resolver is handed a bundle whose `definitions()`
carries no prose channel at all, so reaching the knowledge would mean naming
`Self::knowledge` in the query path, which is a one-line diff a reviewer sees rather than a
property somebody has to remember. `sutura_app::prompt` is the only thing in this workspace that
names it.

The digest covers all three. A glossary decides which metric an agent asks about, so a bundle whose
glossary changed answers different questions from the same words - `crate::definitions` argues it
where the hashing is. And `ContributionManifest` is the composition: which sources composed the
bundle and what each declared, so a re-composition that assembles identically is still a different
bundle. `docs/adr/0011`'s *contribution manifest* section states both, and `crate::pinned::manifest`
is where the digest is taken over it.

#### Methods

```rust
pub fn anchored_metrics(&self) -> impl Iterator<Item>
```

The metrics that declare an anchor, and therefore have to be checked before serving.

```rust
pub const fn definitions(&self) -> &Definitions
```

```rust
pub const fn digest(&self) -> &DefinitionDigest
```

```rust
pub const fn knowledge(&self) -> &Knowledge
```

What this bundle says ABOUT what it defines.

**Read by the prompt renderer and by nothing on the query path**, which is the whole of the
governance argument in `crate::knowledge`: a glossary is descriptive content while a person
or an agent is the one resolving it, and a selecting input the moment the service does.

```rust
pub const fn manifest(&self) -> &ContributionManifest
```

Which metadata sources composed this bundle, and what each declared.

Under the digest, beside the definitions and the knowledge: a re-composition that assembles
identically is a different bundle, which is the whole reason the manifest exists -
`crate::pinned::manifest` and `docs/adr/0011`'s contribution-manifest section.

```rust
pub fn pin(version: DefinitionVersion, definitions: Definitions, knowledge: Knowledge, manifest: ContributionManifest) -> Result<Self, NotDigestible>
```

Pins a set of definitions, the knowledge about them, and the composition that produced both,
computing the digest here, from all three.

**Four arguments, and the absence of a fifth is the mechanism.** This constructor has been
wrong twice, and the second time is the more interesting one:

* `new(version, digest, definitions)` took any syntactically valid digest next to any
  `Definitions` and conceded in its own comment that the one need not describe the other.
* `pin(version, definitions, digest_fn)` then took the hash *function* from its caller, on
  the argument that the domain could not hash. A review re-tested it and it was still
  forgeable: safe public code could pass `|_| Ok(elsewhere)`, and a unit test that inspected
  `given.metrics().len()` inside the closure proved only that the closure had been handed the
  definitions - not that the digest it returned described them. **Passing content to
  untrusted code is not the same as that code having used it.**

So the canonical digest operation is the constructor boundary now. `pin` calls
`DefinitionDigest::of` on the values it is about to store, and there is no parameter, closure
or trait through which a caller can influence what the digest is taken over.
`crate::definitions` holds the canonical form, the hash, and the measured cost of the two
dependencies that made it possible.

The knowledge argument arrived after both of those corrections and did not reopen either: it is
a third piece of CONTENT, hashed with the rest, and not a third opinion about the hashing.
The manifest is a fourth, and it is the resolution of `docs/adr/0011`'s *"no manifest
parameter"*: read against the two bugs above, that sentence means no digest, no closure, no
trait - the manifest is content like the definitions and the knowledge, a caller can still not
influence what the digest is taken over, and making the digest cover the composition is the
whole reason the manifest exists at all.

The forgery a caller could write before does not compile - there is no parameter to pass it
as, and adding the knowledge and the manifest did not add one:

```compile_fail
use core::convert::Infallible;
use sutura_domain::catalog::Definitions;
use sutura_domain::definitions::DefinitionDigest;
use sutura_domain::knowledge::Knowledge;
use sutura_domain::pinned::{ContributionManifest, DefinitionVersion, PinnedDefinitions};

// The digest of some OTHER catalog, returned by a closure that ignores its argument.
fn _forged(
    version: DefinitionVersion,
    definitions: Definitions,
    knowledge: Knowledge,
    manifest: ContributionManifest,
    elsewhere: DefinitionDigest,
) -> Result<PinnedDefinitions, Infallible> {
    PinnedDefinitions::pin(version, definitions, knowledge, manifest, |_| Ok(elsewhere))
}
```

Nor is there a way past the constructor. The fields are private, so the struct literal that
would pair them by hand is not a struct literal a caller can write:

```compile_fail
use sutura_domain::catalog::Definitions;
use sutura_domain::definitions::DefinitionDigest;
use sutura_domain::knowledge::Knowledge;
use sutura_domain::pinned::{ContributionManifest, DefinitionVersion, PinnedDefinitions};

fn _by_hand(
    version: DefinitionVersion,
    digest: DefinitionDigest,
    definitions: Definitions,
    knowledge: Knowledge,
    manifest: ContributionManifest,
) -> PinnedDefinitions {
    PinnedDefinitions { version, digest, definitions, knowledge, manifest }
}
```

And the twin of both blocks, which pins the signature so that a rename cannot make either of
them pass vacuously:

```
use sutura_domain::catalog::Definitions;
use sutura_domain::definitions::NotDigestible;
use sutura_domain::knowledge::Knowledge;
use sutura_domain::pinned::{ContributionManifest, DefinitionVersion, PinnedDefinitions};

fn _pin(
    version: DefinitionVersion,
    definitions: Definitions,
    knowledge: Knowledge,
    manifest: ContributionManifest,
) -> Result<PinnedDefinitions, NotDigestible> {
    PinnedDefinitions::pin(version, definitions, knowledge, manifest)
}
```

```rust
pub fn provenance(&self, executed_as: ExecutedAs) -> Provenance
```

The provenance to attach to one answer produced from this bundle.

**`executed_as` is a required argument and there is no second door that omits it.** An answer
carries a `Provenance`, `Provenance::new` is private, and this is the only way to one - so an
answer cannot be produced without saying which posture each of its legs ran under. That is the
same shape `Self::pin` uses for the digest: the value is computed from what the caller
already has rather than accepted as an optional decoration.

A caller that only wants to *describe* this bundle - a catalog endpoint, the agent-facing
prompt - reads `Self::version` and `Self::digest` instead. Nothing executed for it, and a
`Provenance` with an empty execution record would be the one shape this argument exists to
make unrepresentable.

```rust
pub const fn version(&self) -> &DefinitionVersion
```

#### Implements

`Clone`, `Debug`, `Eq`, `PartialEq`, `Serialize`

### `enum NotExecutedReason`

```rust
pub enum NotExecutedReason
```

Why one anchor produced no verdict at all.

Typed, one variant per branch of the check, because anchor verification is the readiness gate:
when it fails, this value is the whole of what an operator gets. A single `String` here was the
bug - `thiserror` prints only the outermost message, so an adapter error's `source` chain was
discarded on the way in and the operator was told "the anchor query failed" and nothing else.

Each variant sends a reader somewhere different. A missing grain is a catalog to fix, a refusal
is a governance outcome, a source mismatch is a composition root that opened the wrong data
system, and only `NotExecutedReason::Failed` is the data system's own fault.

Two variants carry text rather than a typed cause, and that is a boundary rather than a
shortcut: the `Warehouse` port's error is a generic parameter and the compiler's error lives in a
crate the domain must not depend on, so neither type can be stored here. Both are walked to
exhaustion at the call site and arrive as a message plus its chain, which is the lossless option
available at that boundary.

#### Variants

- `BundleMissingMetric` - The report names a metric the bundle does not define, so there was nothing to run.
- `NoGrain` - The metric declares no grain, so no single period - and therefore no single number - is available to compare the declared one against.
- `NotCompiled` - The anchor's own question would not compile against the bundle that carries it.
- `Refused` - The anchor's own question was refused. A governance outcome, surfaced as one: an anchor a caller could not have asked for is not a failure of the data system.
- `SourceNotConfigured` - The plan names a data system this process did not open. Not prose in a report field: it is the same condition the query path refuses, and it is a misconfigured composition root rather than an outage.
- `NotOneNumber` - The declared range covers more than one period at the metric's coarsest grain, so the result is several numbers and an anchor is one.
- `NoMeasureColumn` - The result carries no column named after the metric, so there is nothing to compare.
- `ResultShapeMismatch` - The result set was not the shape it reported.
- `NotAnAnchor` - The plan the boot path compiled is not this anchor's own, so nothing executed it.
- `Failed` - The data system failed the statement. `message` is the adapter's own, `chain` is every cause beneath it - the driver error included, which is the part that names a table, a column or a file and the part a single string used to throw away.

#### Implements

`Clone`, `Debug`, `Display`, `Eq`, `Error`, `PartialEq`, `Serialize`

### `enum AnchorCheck`

```rust
pub enum AnchorCheck
```

What happened when one metric's anchor was checked.

#### Variants

- `Matched` - The metric reproduced its declared number.
- `Mismatch` - It produced a different one. This is the interesting failure: the definition still runs, so nothing errors, and the number is simply not the one that was certified.
- `NotExecuted` - The check could not run at all: the data system was unreachable, or the statement failed.

#### Implements

`Clone`, `Debug`, `Eq`, `PartialEq`, `Serialize`

### `struct AnchorReport`

```rust
pub struct AnchorReport
```

The outcome of checking every anchor in a bundle.

Built by whoever can execute a statement, which is not this crate.

**This is evidence, and evidence is forgeable - deliberately so.** `AnchorReport::new` and
`AnchorReport::record` are public because an operator-facing surface has to be able to render
and serialize a report, and because the rule below is worth testing here, where the bundle's
shape lives. What used to be wrong is that this same public pair also reached the proof: a caller
could enumerate `PinnedDefinitions::anchored_metrics`, record `AnchorCheck::Matched` for each
without ever opening a data system, and hand the result to a constructor that returned a bundle
the service would serve. So the wrapper attested to nothing but the caller's own assertion, and
read like proof.

The proof now lives one layer out, in `sutura_app::Validated`, whose only constructor is
`sutura_app::verify_and_validate` - which takes a `Warehouse` and calls it. `Self::verdict` is
the rule that constructor applies, and returns `Result<(), NotValidated>`: a verdict, not a
bundle. Nothing in this crate can turn a report into something servable.

#### Methods

```rust
pub const fn checks(&self) -> &BTreeMap<MetricName, AnchorCheck>
```

```rust
pub fn new() -> Self
```

```rust
pub fn record(&mut self, metric: MetricName, check: AnchorCheck)
```

Records what happened for one metric.

Last write wins, because a re-check after a transient failure should replace it rather than
accumulate. The coverage check below is on presence, so a replaced entry cannot hide one.

```rust
pub fn verdict(&self, pinned: &PinnedDefinitions) -> Result<(), NotValidated>
```

Whether this report shows every anchor `pinned` declares having matched.

The rule, with no proof attached. It returns `Result<(), NotValidated>` rather than a
validated bundle on purpose: this crate cannot tell whether the checks in the report ever
reached a data system, so it is not the crate that gets to say a bundle is fit to serve.
`sutura_app::verify_and_validate` runs the anchors and then applies this, and it is the only
thing that mints the proof.

The unknown-metric check is not tidiness: without it, a report built against a different
bundle would satisfy the coverage check for whatever it happened to overlap, and a bundle
would be "validated" by evidence about something else.

#### Implements

`Clone`, `Debug`, `Default`, `Eq`, `PartialEq`, `Serialize`

### `enum NotValidated`

```rust
pub enum NotValidated
```

Why a bundle is not validated.

#### Variants

- `AnchorMismatch` - The declared number and the produced one, both quoted.
- `AnchorNotExecuted` - The reason is the `source`, not the message, so whoever renders this walks the chain and gets the data system's own complaint. Interpolating it would have printed the outermost message and stopped, which is the whole of what was wrong before.
- `AnchorUnchecked`
- `UnknownMetricChecked`

#### Implements

`Debug`, `Display`, `Eq`, `Error`, `PartialEq`

### `enum CatalogKind`

```rust
pub enum CatalogKind
```

Which class of catalog adapter this is: held to the whole model, or supplying part of it.

The two classes are measured differently, and `docs/adr/0016` is where the difference is
decided. A **golden** adapter defines the model here - the wren-style directory of markdown -
so it can be held to producing the whole of it, which is what agreeing with the hand-written
oracle asserts. Everything else is **declaring**: it supplies part of the model and must say
which part through `SemanticCatalog::capabilities`, and is measured against that declaration
(the two directions of `MetadataCapabilities::checked_against`) rather than against the oracle.

**Required on `SemanticCatalog` with no default, so an adapter that omits it does not build.**
The two classes give a registration different assertions and different goldens - the oracle for
a golden adapter, fidelity for a declaring one - so leaving the choice to a default would mean
an adapter that said nothing got measured the wrong way, and both defaults are wrong: defaulted
to golden, a narrow source silently keeps a test it cannot pass; defaulted to declaring, a
reference adapter that forgot the line loses the test that exists to hold it honest.

#### Variants

- `Golden` - A **reference** adapter: can produce every kind the model defines, so it is held to the hand-written oracle that states the same model. `sutura-catalog-local` is one.
- `Declaring` - Supplies part of the model. Measured against its own `SemanticCatalog::capabilities` declaration rather than against the oracle, and registered with only the catalog cells that apply to a partial model.

#### Implements

`Clone`, `Copy`, `Debug`, `Eq`, `PartialEq`

### `trait SemanticCatalog`

```rust
pub trait SemanticCatalog
```

Where definitions come from.

One trait, implemented once per catalog. A directory of files in git and a metadata service over
HTTP are two adapters behind it, and swapping one for the other does not touch the query path.

**`load` takes no request context, and that is the whole design of this port.** A catalog that
could see the caller could return a different definition per caller, and then the digest that
travels with an answer would describe something other than what produced it.

# The declaration, and why an adapter cannot be silent

`Self::capabilities` says which kinds of thing this adapter can supply at all - and, by
omission from its own lists, which it cannot. **An absence has to be declared rather than
inferred from silence**, because a bundle with no metrics in it is two entirely different facts: a
reviewed catalog that has not certified one yet, and a source that holds a measure this
repository will not execute. An empty collection cannot tell those apart, and a caller deciding
what to trust needs to know which it is looking at.

`crate::warehouse::Warehouse::IMPERSONATION` is the shape this copies and its argument carries
over word for word: a defaulted capability would mean an adapter that said nothing got the benefit
of the doubt in whichever direction the default pointed, and both directions are wrong. Defaulted
to *supplies*, a narrow source would silently claim measures it does not have. Defaulted to *does
not*, a complete adapter that forgot the line would be reported as narrow and somebody would fix
that by deleting the check.

The rule that decides required from defaulted is that trait's, demonstrated there twice:
**required with no default where the absence changes what a caller may believe, defaulted with a
stated reason where it is a missed optimisation.** This one is the first case - `dry_run` next
door is the second, and says so in its own words.

**An adapter that declares no capabilities does not compile:**

```compile_fail
use sutura_domain::pinned::{PinnedDefinitions, SemanticCatalog};

struct Undeclared;

// No `fn capabilities`, so this impl is incomplete: the trait declares it with no default.
impl SemanticCatalog for Undeclared {
    type Error = core::fmt::Error;

    fn load(&self) -> Result<PinnedDefinitions, Self::Error> {
        Err(core::fmt::Error)
    }
}
```

The compiling twin, so the block above cannot be passing on a typo - the only difference between
the two is the declaration:

```
use sutura_domain::capabilities::{DefinitionCapabilities, DefinitionKind, MetadataCapabilities};
use sutura_domain::knowledge::{Capability, KnowledgeCapabilities};
use sutura_domain::pinned::{CatalogKind, PinnedDefinitions, SemanticCatalog};

struct Declared;

impl SemanticCatalog for Declared {
    type Error = core::fmt::Error;

    const KIND: CatalogKind = CatalogKind::Golden;

    fn capabilities() -> MetadataCapabilities {
        MetadataCapabilities::of(
            DefinitionCapabilities::of([DefinitionKind::Structure, DefinitionKind::Descriptions]),
            KnowledgeCapabilities::of([Capability::Glossary]),
        )
    }

    fn load(&self) -> Result<PinnedDefinitions, Self::Error> {
        Err(core::fmt::Error)
    }
}

assert!(
    !<Declared as SemanticCatalog>::capabilities()
        .definitions()
        .declares(DefinitionKind::Metrics),
    "this adapter declares no metrics, and the declaration is what says so"
);
```

### `use None`

### `use None`

### `use None`

### Module `manifest`

The contribution manifest: which metadata sources composed a bundle, and what each declared.

`PinnedDefinitions`' digest is taken over a canonical form of the definitions, the knowledge
and this manifest, so the digest covers the **composition** and not only the assembly -
`docs/adr/0011`'s "two different compositions that assemble identically are indistinguishable"
is the gap this closes. Decision and serialized form: `docs/adr/0011`, *The contribution
manifest is built, and its serialized form is decided*.

**The manifest says what was configured and reached, not what a source returned.** Each entry is
the source's own declared capability list, its required-or-optional declaration, and whether it
was reached for this bundle - no host, no credential, no URL. Fidelity between the declaration
and a bundle's content is `MetadataCapabilities::checked_against`, which `sutura_app`'s
metadata assembler runs per contributor.

Split out of `pinned.rs` for `cargo xtask max-lines`, the catalog precedent applied to the
pinned-bundle half of the module.

#### `enum RequiredOrOptional`

```rust
pub enum RequiredOrOptional
```

Whether a contributor is required for this deployment to serve, or may be absent.

**Every value this code can produce is `Self::Required`** - no settings shape declares an
optional source yet, so a bundle exists only when every configured source loaded. The variant is
carried because the availability rule `docs/adr/0011` decided lands on top of it: a deployment
that can declare `optional` is the diff that first writes `Self::Optional`, and the digest's
job is to make that run look different from the one that included the source.

##### Variants

- `Required` - The deployment does not serve without this source.
- `Optional` - The deployment may serve without it, and a bundle that did differs in digest from one that did not - nothing here can produce one yet, because no declaration says so.

##### Implements

`Clone`, `Copy`, `Debug`, `Deserialize<'de>`, `Eq`, `PartialEq`, `Serialize`

#### `struct Contribution`

```rust
pub struct Contribution
```

One metadata source's record in the manifest.

##### Methods

```rust
pub const fn capabilities(&self) -> &MetadataCapabilities
```

What this source declared it supplies, the manifest record of
[`SemanticCatalog::capabilities`](crate::pinned::SemanticCatalog::capabilities).

```rust
pub const fn missing(capabilities: MetadataCapabilities) -> Self
```

The record for a configured source this bundle serves without.

The availability case `docs/adr/0011` prices: optional and unreachable at startup, recorded
as such so the digest differs from a run that included it. Nothing in this repository can
produce one today - no deployment declares an optional source - so it is the shape a future
declaration fills, and it stops this constructor being omitted.

```rust
pub const fn of(capabilities: MetadataCapabilities) -> Self
```

The record for a source that was configured, declared these capabilities, and loaded.

**`reached = true` is what a served bundle records.** A source an optional deployment could
not reach is a manifest entry with `reached = false`, which this constructor's `of` does not
produce - see `Self::missing`.

```rust
pub const fn reached(&self) -> bool
```

Whether this source was reached for this bundle.

```rust
pub const fn required_or_optional(&self) -> RequiredOrOptional
```

Whether this source was declared required for the deployment to serve.

##### Implements

`Clone`, `Debug`, `Deserialize<'de>`, `Eq`, `PartialEq`, `Serialize`

#### `struct ContributionManifest`

```rust
pub struct ContributionManifest
```

Which metadata sources composed a bundle, keyed by each source's declared name.

**A `BTreeMap`, so collection order is content order** - the same determinism requirement
`crate::catalog::Definitions` and `crate::knowledge::Knowledge` carry, for the same reason:
the digest is taken over the serialized form, and an unordered map serializes in whatever order
its hasher chose this run.

**A single-source deployment carries a one-entry manifest** rather than none, because a shape
that differed between one source and N would put the interesting case on the untested path.

##### Methods

```rust
pub fn count(&self) -> usize
```

How many sources composed this bundle.

```rust
pub const fn entries(&self) -> &BTreeMap<SourceName, Contribution>
```

Every entry, keyed on each contributor's declared name.

```rust
pub fn get(&self, source: &SourceName) -> Option<&Contribution>
```

One contributor's record, or `None` if the name was not configured.

```rust
pub fn of(entries: impl IntoIterator<Item>) -> Self
```

A bundle read from several sources, in declaration order.

```rust
pub fn single(source: SourceName, contribution: Contribution) -> Self
```

A bundle read from exactly one source.

##### Implements

`Clone`, `Debug`, `Deserialize<'de>`, `Eq`, `PartialEq`, `Serialize`

## Module `plan`

What we decided to execute, and the two things nothing else may decide.

**A plan lives in the domain rather than in the compiler, and that is what let a second kind of
adapter exist.** The `Warehouse` port used to take a rendered statement, which quietly said that
every data system speaks SQL. One does not: an in-process engine executes a logical plan over
Arrow and generates no SQL at all. So the port takes a `QueryPlan` and *how* to execute it is
the adapter's business - render a statement, or build a plan of its own.

That is worth more than the tidiness. Rendering SQL for a local file was where every dialect bug
lived: a truncated date coming back as a timestamp, an alias emitted unquoted, a `GROUP BY` given
an aliased expression, a placeholder in the wrong syntax. An adapter that never renders SQL
cannot have any of them.

A plan holds no SQL. Its serialized form is what a golden snapshot pins, so a change to what we
decided shows up as a reviewable diff rather than as a different number.

**Two shapes, not one, and `leg` holds the second.** A `QueryPlan` is a whole answer from one
data system. A `LegPlan` is one data system's share of an answer assembled above it, and it is
its own type rather than a `QueryPlan` with three fields made optional - `leg` says at length
why. `Executable` is what the port takes, so an adapter's match over what it can be handed is
exhaustive.

### `struct PlanColumn`

```rust
pub struct PlanColumn
```

A column, qualified by the table it is read from.

Qualified always, even when there is only one table. An unqualified column in a statement that
later grows a join binds to whichever table happens to have it, and that is a wrong number rather
than an error.

#### Methods

```rust
pub const fn column(&self) -> &ColumnName
```

```rust
pub const fn new(table: TableName, column: ColumnName) -> Self
```

```rust
pub const fn table(&self) -> &TableName
```

#### Implements

`Clone`, `Debug`, `Eq`, `PartialEq`, `Serialize`

### `struct PlanJoin`

```rust
pub struct PlanJoin
```

One join, as the plan will make it.

#### Methods

```rust
pub const fn join_type(&self) -> JoinType
```

```rust
pub fn new(relationship: RelationshipName, table: impl Into<QualifiedTable>, join_type: JoinType, origin: PlanColumn, target: PlanColumn) -> Self
```

One join to a table, wherever that table lives.

**A joined table carries its own qualifier, and that is the whole point of the feature rather
than completeness:** a fact table in one dataset joined to a dimension table in another is
what a multi-project estate looks like, and it is one statement, one job and one credential -
a native join the data system pushes down, not a second source. `sutura_semantic::plan` says
so where a source count decides between one statement, a split and
`PlanSpansTooManySources`.

`impl Into<QualifiedTable>` for the reason `Model::new` gives.

```rust
pub const fn origin(&self) -> &PlanColumn
```

```rust
pub const fn relationship(&self) -> &RelationshipName
```

```rust
pub const fn table(&self) -> &QualifiedTable
```

Where the joined table lives: the whole path, which is what a `JOIN` clause names.

```rust
pub const fn table_name(&self) -> &TableName
```

The joined table's own name, which is what its columns are qualified by.

```rust
pub const fn target(&self) -> &PlanColumn
```

#### Implements

`Clone`, `Debug`, `Eq`, `PartialEq`, `Serialize`

### `struct PlanBucket`

```rust
pub struct PlanBucket
```

The truncated time column, and the label it is projected under.

#### Methods

```rust
pub const fn column(&self) -> &PlanColumn
```

```rust
pub const fn grain(&self) -> Grain
```

```rust
pub fn label(&self) -> &str
```

```rust
pub const fn new(label: String, grain: Grain, column: PlanColumn) -> Self
```

#### Implements

`Clone`, `Debug`, `Eq`, `PartialEq`, `Serialize`

### `struct PlanKey`

```rust
pub struct PlanKey
```

One group-by key.

#### Methods

```rust
pub const fn column(&self) -> &PlanColumn
```

```rust
pub fn label(&self) -> &str
```

```rust
pub const fn new(label: String, column: PlanColumn) -> Self
```

#### Implements

`Clone`, `Debug`, `Eq`, `PartialEq`, `Serialize`

### `enum PlanTerm`

```rust
pub enum PlanTerm
```

One term of a measure, with its column resolved to a table.

`Term` restated over `PlanColumn` rather than `ColumnName`. Mirrored at the same level the
domain names it, so an adapter that renders one half of a ratio and one that renders a whole
measure reach for the same function instead of each flattening two levels its own way.

#### Variants

- `Aggregate`
- `CountIf`

#### Methods

```rust
pub const fn column(&self) -> &PlanColumn
```

#### Implements

`Clone`, `Debug`, `Eq`, `PartialEq`, `Serialize`

### `enum PlanMeasure`

```rust
pub enum PlanMeasure
```

What is measured, under what label, with every column resolved to a table.

The shape is `Measure`'s, restated over `PlanTerm`: the plan knows which table each column
comes from and the catalog does not have to.

#### Variants

- `Simple`
- `Ratio`

#### Implements

`Clone`, `Debug`, `Eq`, `PartialEq`, `Serialize`

### `enum PlanPredicate`

```rust
pub enum PlanPredicate
```

One predicate in the plan's filter, and which parameter carries its value.

The parameter index is recorded rather than implied by position, so a reader of a plan can see
which value goes where without reconstructing the generator's ordering in their head - and so an
adapter that binds by index cannot disagree with one that binds by order.

#### Variants

- `AtOrAfter` - `column >= param`, the inclusive start of the range.
- `Before` - `column < param`, the exclusive end.
- `Equals`
- `NotEquals`
- `IsTrue`
- `IsNotNull`

#### Methods

```rust
pub const fn column(&self) -> &PlanColumn
```

```rust
pub const fn param(&self) -> Option<usize>
```

#### Implements

`Clone`, `Debug`, `Eq`, `PartialEq`, `Serialize`

### `enum PredicateOrigin`

```rust
pub enum PredicateOrigin
```

Where a predicate came from.

Recorded because it is the difference between a number being wrong and a caller being refused. A
definitional predicate is part of what the metric means and a caller cannot see or remove it; a
requested one came from the question and was checked against an allowlist. Keeping the two
distinguishable in the plan is what lets a golden assert that the definitional ones are always
present.

#### Variants

- `Definition` - From the metric's own definition: a required filter, or the bounded time range.
- `Requested` - From the question, having passed the pinned allowlist.

#### Implements

`Clone`, `Copy`, `Debug`, `Eq`, `PartialEq`, `Serialize`

### `struct PlanFilter`

```rust
pub struct PlanFilter
```

A predicate and where it came from.

#### Methods

```rust
pub const fn new(origin: PredicateOrigin, predicate: PlanPredicate) -> Self
```

```rust
pub const fn origin(&self) -> PredicateOrigin
```

```rust
pub const fn predicate(&self) -> &PlanPredicate
```

#### Implements

`Clone`, `Debug`, `Eq`, `PartialEq`, `Serialize`

### `struct QueryPlan`

```rust
pub struct QueryPlan
```

One statement's worth of decisions, and no SQL.

#### Methods

```rust
pub const fn bucket(&self) -> &PlanBucket
```

```rust
pub fn definitional_params(&self) -> Vec<&ParamValue>
```

Every parameter a definitional predicate binds.

Used by the golden that asserts a required filter is bound rather than written into the
statement.

```rust
pub fn filters(&self) -> &[PlanFilter]
```

```rust
pub fn joins(&self) -> &[PlanJoin]
```

```rust
pub fn keys(&self) -> &[PlanKey]
```

```rust
pub const fn max_rows(&self) -> u32
```

The most rows this plan's result may carry before it is refused.

The cap itself, which is what a row count is compared against. Read it with
[`row_limit`](QueryPlan::row_limit): the two differ by one, deliberately, and neither is
useful without the other.

```rust
pub const fn measure(&self) -> &PlanMeasure
```

```rust
pub fn measure_label(&self) -> &str
```

```rust
pub const fn metric(&self) -> &MetricName
```

```rust
pub fn new(source: SourceName, metric: MetricName, tables: StatementTables, bucket: PlanBucket, keys: Vec<PlanKey>, measure: PlanMeasure, measure_label: String, filters: Vec<PlanFilter>, params: Vec<ParamValue>, range: TimeRange) -> Self
```

One statement's worth of decisions.

**The tables arrive as a `StatementTables` and not as a table plus a vector of joins**, and
that argument is the whole of what keeps this constructor infallible: the check that two of
them do not answer to one identifier happens where that set is parsed, so a plan holding the
ambiguous pair does not exist to be rendered. `crate::plan::tables` is where the defect, the
measurement and the choice of a refusal over an alias are argued.

```rust
pub fn params(&self) -> &[ParamValue]
```

```rust
pub const fn range(&self) -> TimeRange
```

```rust
pub fn result_labels(&self) -> Vec<String>
```

The labels this plan's result will carry, in order.

One definition, so an adapter that builds a schema and an adapter that renders a projection
cannot disagree about it - which is the whole reason two adapters can be compared against
each other at all.

```rust
pub const fn row_limit(&self) -> u32
```

How many rows an adapter asks for: one more than [`max_rows`](QueryPlan::max_rows).

**The extra row is the whole mechanism.** Fetching exactly the cap makes a result AT the cap
indistinguishable from a result the cap cut short, and the second of those is a partial total
under a certified name. Asking for one more makes "there is more" observable at no cost - the
extra row is never returned to a caller, because a result carrying it is refused as
[`RefusalReason::ResultTooLarge`](crate::query::RefusalReason::ResultTooLarge).

Saturating, so a cap of `u32::MAX` stays a number rather than wrapping to zero and asking
a data system for nothing.

```rust
pub const fn source(&self) -> &SourceName
```

```rust
pub const fn table(&self) -> &QualifiedTable
```

Where the table lives: the whole path, which is what the `FROM` clause names.

Read `Self::table_name` instead where what is wanted is the name a column is qualified by.

```rust
pub const fn table_name(&self) -> &TableName
```

The table's own name, which is what this plan's columns are qualified by.

#### Implements

`Clone`, `Debug`, `Eq`, `PartialEq`, `Serialize`

### `struct AnchorPlan`

```rust
pub struct AnchorPlan<'bundle>
```

The one thing [`Warehouse::verify_anchor`](crate::warehouse::Warehouse::verify_anchor) accepts:
a plan the pinned bundle itself agrees is one of its anchors' own.

# What this type is, and what it is not

**It is a self-check on the boot path, and it is NOT an authority.** That distinction is the whole
of what a second review corrected, and getting it wrong once put a false sentence in ten places
across seven files - `docs/adr/0008`'s second amendment to its correction 2 lists them. [`Warehouse::execute`](crate::warehouse::Warehouse::execute) cannot be called without a
[`Presented`](crate::identity::Presented); `verify_anchor` deliberately takes no credential,
because there is no caller at boot, and it therefore runs under whatever identity the deployment
configured that adapter with. So the question is what bounds its INPUT.

`Self::of` answers "did the boot path compile the question it meant to" and nothing stronger.
The plan has to compute a metric **this bundle** defines, that metric has to declare an anchor,
and the plan has to be that anchor's own question: the metric's coarsest declared grain, exactly
the range the anchor certifies, no group-by keys, and no predicate a question asked for. Every one
of those facts is read off the `PinnedDefinitions` rather than accepted as an argument, which is
what makes the check worth making - a caller no longer supplies the anchor it will be compared
against.

**What it cannot do is stop code that wants to.** Every value it reads is publicly constructible -
`QueryPlan::new`, `PinnedDefinitions::pin`, the metric and range types - and Rust has no
cross-crate friend visibility, so a constructor `sutura-app` can call is a constructor anything in
the workspace can call. A reviewer defeated the previous version of this type in one function by
fabricating the tuple it took, and the fix for that class is not a fifth guard: a shape check over
caller-constructible values can only ever be a shape check.

# So what makes the credential-free path boot-only

A lint, and it is named here rather than implied: `clippy.toml` bans
`sutura_domain::warehouse::Warehouse::verify_anchor`, verified to resolve by writing the call and
watching clippy reject it. `sutura_app::verify_anchors` holds the single `#[expect]`, so a second
call site is an error under `-D warnings` until somebody writes a second expectation a reviewer
sees in the diff. That is the same mechanism the ban on the panicking fragment API and the ban on a
bare `spawn_blocking` already rest on. **Its limit is that a lint is not a type:** it reaches this
workspace and not a crate outside it, and an `#[allow]` walks past it.

A genuinely closed constructor is not available. The domain cannot compile a plan - compilation is
`sutura-semantic`'s and dependencies point inward - and a token only `sutura-app`'s private `proof`
module could mint would have to be constructible from `sutura-domain`, which is the same public
door one level down. `docs/adr/0008`'s own corrections are the precedent for saying this rather
than implying more.

#### Methods

```rust
pub fn of(plan: &'bundle QueryPlan, pinned: &PinnedDefinitions, metric: &MetricName) -> Result<Self, NotAnAnchorsPlan>
```

Parses a plan as one of `pinned`'s own anchors', reading every fact it compares off the bundle.

Takes the metric's name as well as the bundle, because the bundle holds many anchors and the
caller is asserting *which* one this plan is of - so the first check is that the plan agrees.
Everything after that is the bundle's own statement about that metric.

**It does not take a `sutura_app::Validated` bundle, and it cannot:** validating a bundle is
what this call is part of, so the proof does not exist yet. That is one more reason the type is
a self-check rather than an authority.

The order of the checks is chosen for the diagnostic rather than for cost - every input is
already bounded and in memory. Which metric, then what the bundle says about that metric, then
the two shapes only a question has, then the two values an anchor's own question pins.

```rust
pub const fn plan(&self) -> &QueryPlan
```

The plan, for the adapter that has to execute it.

#### Implements

`Debug`

### `enum NotAnAnchorsPlan`

```rust
pub enum NotAnAnchorsPlan
```

A plan that is not a declared anchor's own, so the boot path did not compile what it meant to.

**An error and not a refusal**: reaching it means the boot path compiled something other than the
anchor's question, which is a defect here rather than anything about a caller.

#### Variants

- `NotThatMetric` - The plan computes a different metric from the one whose anchor it would be checked against.
- `MetricNotDefined` - The bundle this plan is checked against does not define the metric at all.
- `DeclaresNoAnchor` - The metric is defined and declares no certified number, so there is no anchor to be a plan of.
- `Grouped` - The plan groups by something. An anchor is a metric's own number, not a slice of it.
- `Requested` - The plan carries a predicate a question asked for, which an anchor's plan never does.
- `NotTheCoarsestGrain` - The plan buckets at a finer grain than the metric's coarsest, so it returns a series.
- `NotTheAnchorsRange` - The plan's range is not the range the anchor's author certified.

#### Implements

`Clone`, `Debug`, `Display`, `Eq`, `Error`, `PartialEq`

### `fn plan_measure`

```rust
pub fn plan_measure(measure: &crate::measure::Measure, resolve: impl Fn(&crate::model::ColumnName) -> PlanColumn) -> PlanMeasure
```

Restated over plan columns, so an adapter does not need the catalog to know which table a
measure's column comes from.

Resolves one term at a time rather than one shape at a time, which is why a term added to the
vocabulary is one arm here instead of one arm per shape.

### `fn plan_required_filter`

```rust
pub fn plan_required_filter(filter: &crate::measure::RequiredFilter, column: PlanColumn, bind: impl FnOnce(String) -> usize) -> PlanPredicate
```

A required filter as a plan predicate, binding a parameter when it needs one.

`bind` is called only for the operators that compare against a value, and returns the index it
was stored at. Passing the binding in rather than returning a value keeps the parameter list in
one place: the caller owns the order, which is what the placeholder-position contract depends on.

### `use None`

### `use None`

### `use None`

### `use None`

### `use None`

### `use None`

### `use None`

### `use None`

### `use None`

### `use None`

### `use None`

### `constant MAX_ROWS`

The most rows any plan may return.

A hard cap rather than a budget, for now. A bounded range and a bounded set of group-by keys
still permit a large result, and the cost of that lands on a shared data system. When there is a
real budget this becomes its floor.

**It is a refusal and not a truncation, and that is the correction a review forced.** This used
to be the `LIMIT` on the statement and nothing else: nothing compared the rows that came back
against it. So a question at `day` grain over a year, grouped by up to
[`MAX_DIMENSIONS`](crate::query::MAX_DIMENSIONS) keys, answered with the first ten thousand
groups by group key, carried a provenance digest, and said nowhere that it was partial. Summing
those rows gives a wrong number under a certified name, arrived at by omission - which is the
failure mode this repository exists to prevent, and the one a caller has no way to detect.

What holds it up is two things that have to be read together. `QueryPlan::row_limit` is one
MORE than this, so a result that reached the cap is distinguishable from a result the cap cut
short; and a row count above this is
[`RefusalReason::ResultTooLarge`](crate::query::RefusalReason::ResultTooLarge). A refusal is the
honest outcome: "your question is too wide to certify" is a governance answer, not an error, and
the caller's move is to narrow the range or drop a dimension.

### Module `federated`

The federated question: two legs, and the combine that happens above them.

**This module gives `crate::federation` its caller.** Until now that module was "a
classification and a rule, nothing executes it": `Descent::of` and `Federation::of` were total but
nothing produced a `crate::plan::LegPlan` and nothing consumed the rows one returns. This module
is the other half - the small, closed contract that a splitter fills with facts and this module's
own `FederatedPlan::combine` turns back into rows.

**What the splitter and the combiner agree on, and it is one function.** A fact leg's terms are
projected under labels, and the combiner has to find each term's column *by* its label - the
mistake this design refuses to make is the two halves agreeing by review. `labels` is that one
function: the splitter names the fact leg's terms with it and the combiner re-derives the same
names from the same `Federation` and looks them up in the fact leg's result. There is no second
copy of the naming rule to drift.

**The division cannot happen in a leg, and [`combine`](FederatedPlan::combine) is where it
happens instead.** The `Above` tree already carries the only
[`ZeroDenominator`](crate::measure::ZeroDenominator) in the federated path; this module walks it
above the legs, after every leg's rows have been re-aggregated. Applying a guard inside a leg is
the wrong number this shape exists to prevent.

**What this module will not do, because `combine` cannot express it.** The re-aggregation
`combine` performs covers the leaves a *decomposable* measure produces - a re-aggregating
[`Sum`](crate::model::Aggregate::Sum), [`Min`](crate::model::Aggregate::Min) or
[`Max`](crate::model::Aggregate::Max) over already-aggregated leg columns. A measure that does not
decompose at all (a distinct count) has no re-aggregating function, and the honest answer for this
slice is to refuse it in the splitter rather than pull its rows up through a combiner that would
have to re-count. The refusal names the aggregate.

#### `struct FederatedPlan`

```rust
pub struct FederatedPlan
```

The one federated shape this workspace combines: a fact leg on one source and a lookup leg on
another, linked by a single column.

**Two legs, as two named fields.** A match over `LegPlan` is exhaustive, so the fact leg *is*
the [`Fact`](LegPlan::Fact) variant and the lookup leg the [`Lookup`](LegPlan::Lookup) one, and
a plan that had anything other than exactly these two is a type that does not exist rather than a
count a caller checks. The shape is deliberately the one `crate::plan::leg` pins in its goldens:
the metric's own rows (and any same-source dimension) form the fact leg, and a dimension on a
second data system forms the lookup leg. The final answer groups by the answer's keys - each
named by which leg's result it is read from, in question order - bucketed and measured under the
metric's own name.

**The `serde::Serialize` derive exists for the CLI's plan dump and nothing else.** A plan is
serialized to be printed; nothing in the workspace gains `serde::Deserialize`, so a plan cannot
be reconstructed from its serialized form and no field here is a request a caller writes.

##### Methods

```rust
pub fn combine(&self, fact: &RowSet, lookup: &RowSet, byte_budget: u64) -> Result<RowSet, FederatedFailure>
```

Turns one result per leg into one answer's rows.

The fact and lookup results are joined on the recorded link column, grouped by the answer's
keys - in the order the question asked them, matching the mono path - and the bucket,
re-aggregated by each leaf's own `Carried::combine`, and only then divided through the
`Above` tree.

`byte_budget` is the working-set ceiling `docs/adr/0009` applies at the conversion boundary:
the answer materialised here is counted as it is built, and a question that would cross it is
refused as `FederatedFailure::ResourcesExhausted` rather than truncated, so a caller never
reads a result that stopped early as a result that returned.

```rust
pub const fn fact(&self) -> &LegPlan
```

The fact leg.

```rust
pub fn keys(&self) -> &[AnswerKey]
```

The answer's group-by keys, in question order.

```rust
pub const fn legs(&self) -> [&LegPlan; 2]
```

Every leg, in execution order: the fact leg, then the lookup leg.

```rust
pub const fn lookup(&self) -> &LegPlan
```

The lookup leg.

```rust
pub const fn metric(&self) -> &MetricName
```

The metric this answer is measured in.

```rust
pub fn new(metric: MetricName, measure_label: String, bucket: PlanBucket, fact: LegPlan, lookup: LegPlan, fact_join: String, lookup_join: String, include_unmatched: bool, federation: Federation, keys: Vec<AnswerKey>) -> Result<Self, FederatedPlanError>
```

Constructs a federated plan from its two legs and the answer's key order.

A `Result` constructor is this workspace's convention for a value with an invariant: a plan
that is not a fact leg beside a lookup leg, or that names one data system on both legs, is not
a plan and cannot be built.

```rust
pub fn sources(&self) -> impl Iterator<Item> + '_
```

Every data system this plan reads from, in execution order.

##### Implements

`Clone`, `Debug`, `Eq`, `PartialEq`, `Serialize`

#### `enum LegSide`

```rust
pub enum LegSide
```

Which leg's result an answer key is read from.

##### Variants

- `Fact` - The metric's own leg.
- `Lookup` - The second data system's leg.

##### Implements

`Clone`, `Copy`, `Debug`, `Eq`, `PartialEq`, `Serialize`

#### `struct AnswerKey`

```rust
pub struct AnswerKey
```

One group-by key of the answer: which leg owns it, and the label it carries in that leg's result.

##### Methods

```rust
pub const fn fact(label: String) -> Self
```

A key read from the fact leg's result, under `label`.

```rust
pub fn label(&self) -> &str
```

The label this key carries in its leg's result.

```rust
pub const fn lookup(label: String) -> Self
```

A key read from the lookup leg's result, under `label`.

```rust
pub const fn side(&self) -> LegSide
```

Which leg this key is read from.

##### Implements

`Clone`, `Debug`, `Eq`, `PartialEq`, `Serialize`

#### `enum FederatedPlanError`

```rust
pub enum FederatedPlanError
```

Why a federated plan could not be built.

##### Variants

- `NotFact` - The leg meant to be the fact leg is not a `LegPlan::Fact`.
- `NotLookup` - The leg meant to be the lookup leg is not a `LegPlan::Lookup`.
- `SameSource` - Both legs name the same data system, which is a single-source question, not a federated one.
- `KeyNotOnLeg` - An answer key names a column the leg it belongs to does not project.
- `LeafDoesNotReaggregate` - A carried leaf names an aggregate the combine has no re-aggregating function for.

##### Implements

`Clone`, `Debug`, `Display`, `Eq`, `Error`, `PartialEq`

#### `enum FederatedFailure`

```rust
pub enum FederatedFailure
```

Why a federated answer could not be assembled.

The shape failures are defects in this workspace's own wiring - a leg result missing a column
`labels` named, or a row narrower than its result's own columns. The [`NonFinite`](FederatedFailure::NonFinite)
variant is a `fails` guard meeting a zero denominator, which no divide-tree node can produce a
value for.

##### Variants

- `MissingColumn` - A column `combine` reached for by label was absent from a leg's result.
- `NonFinite` - A division happened by a zero denominator while the measure declared `fails`.
- `DuplicateLabels` - A leg result had two columns under one label, so the combiner could not tell which of them a leaf or key names.
- `FloatLinkKey` - A link cell carried a floating-point key, which the ADR's float-key rule forbids.
- `AmbiguousLink` - A link value had more than one lookup row, which would double every measure.
- `NonNumericLeaf` - A leaf cell that was not a number reached a re-aggregating aggregate.
- `MixedNumericLeaf` - A leaf column carried two numeric types, so no total or comparison over it is exact.
- `Overflow` - A leaf total overflowed a 64-bit integer.
- `UnsupportedAggregate` - An aggregate the combiner does not know how to re-aggregate with.
- `ResourcesExhausted` - Materialising the answer crossed the byte budget `docs/adr/0009` applies at the conversion boundary.
- `MalformedRow` - A row whose width contradicts the result's own column count.

##### Implements

`Clone`, `Debug`, `Display`, `Error`, `PartialEq`

#### `fn labels`

```rust
pub fn labels(federation: &crate::federation::Federation, metric: &crate::model::MetricName) -> Vec<String>
```

The one definition of what a carried leaf is projected under.

The splitter and the combiner both call this, so the column the combiner reads a leaf from and
the label the splitter projected it under cannot disagree - there is no second copy of the rule.

**A single leaf is the answer's own name; several leaves disambiguate by position.** A plain sum
travels as the metric's own label, and the halves of a decomposition travel as `metric__{n}`,
where `n` is the leaf's position in carried order. Position cannot collide: a ratio of two sums -
`sum(a) / sum(b)` - is one aggregating function twice, so naming by aggregate would give both
leaves the same label and a combine that divides a column by itself. Whatever makes the labels
unique within one plan is enough - the final measure comes back under the metric's own name - and
this rule is that minimum.

### Module `leg`

One source's share of a federated question, and the only thing the port can be handed.

**The shapes, their closure, and nothing that produces or executes one.** There is no splitter
and no combiner in this workspace, so no production code constructs a `LegPlan`: what is here
is the vocabulary a splitter will emit and `sutura-sql` already renders, pinned per dialect
before anything runs it. `.agents/skills/sutura/query-surface` carries that state, and this
module says it rather than leaving it to be discovered.
`docs/adr/0007-federating-across-different-data-systems.md` decides the shape and
`docs/adr/0009-the-plan-from-one-source-to-many.md` Decision 2 decides what a leg may compute.

**Why a leg is not a `QueryPlan`, which is the whole reason this module exists.** A
`QueryPlan` requires a `PlanBucket`, a [`PlanMeasure`](crate::plan::PlanMeasure) and a measure
label, and a dimension lookup has none of the three: a dimension table has no time column, no
measure and no metric name. Reaching for `Option` on those three fields would make *a dimension
leg carrying a time range* constructible, which is the checked shape where this one is the
unrepresentable one.

And the measure is worse than absent. [`PlanMeasure`](crate::plan::PlanMeasure) has exactly two
variants and the only one carrying two terms is the one that **divides** them -
`sutura_sql::generate` renders a `Ratio` as `CAST(numerator AS DOUBLE) / NULLIF(denominator, 0)`.
So a decomposed `Avg` travelling as a sum beside a count, and a ratio travelling as an undivided
numerator and denominator, are not expressible by `PlanMeasure` at all - and the division per leg
that 0009's Decision 2 forbids is exactly what it *would* express. `LegTerm` is the answer: a
`PlanTerm` and a label, with no shape that divides and no field a
[`ZeroDenominator`](crate::measure::ZeroDenominator) fits in, at any depth. That is the same
argument `crate::federation::Carried` makes one level up, and the two are deliberately built
the same way.

**Two variants and not three.** There are three shapes a leg can be - an aggregate fact leg, a
distinct-key fact leg, and a dimension lookup - and only one of the two axes they split along is
worth a variant. Splitting by *which model is read* moves four fields together: a dimension
model has no time column, so no bucket and no range; it is not the metric's own model, so no
metric name; and a one-hop dimension join does not start from it, so no joins. Splitting by
*whether an aggregate is applied* moves one bit, and both halves render identically - project the
key list, group by the key list. So the distinct-key leg is a `LegPlan::Fact` whose `terms` are
empty, and it needs no variant of its own.

**No leg carries a row cap.** A leg is not an answer, and [`MAX_ROWS`](crate::plan::MAX_ROWS)
caps one answer's rows; `sutura_sql::generate_leg` emits no `LIMIT` for the same reason.

#### `struct LegTerm`

```rust
pub struct LegTerm
```

One number a leg computes, and the label it is projected under.

**A `PlanTerm` and not a [`PlanMeasure`](crate::plan::PlanMeasure), and that is a type rather
than a convention.** There is no shape here that divides, so a leg's statement cannot carry a
`NULLIF` guard and a per-leg quotient is unrepresentable rather than discouraged. The bug that
closes is specific: applied *inside* a leg, `ZeroDenominator::Null` turns a subgroup with a zero
denominator into a null, the re-aggregating `SUM` above skips nulls, and that subgroup's
numerator is silently dropped from the answer instead of nulling it.

A leg's terms come off `crate::federation::Federation::carried`, which is built purely from the
term level for the same reason.

The division a leg cannot express does not compile:

```compile_fail
use sutura_domain::measure::ZeroDenominator;
use sutura_domain::plan::{LegTerm, PlanMeasure, PlanTerm};

// `LegTerm::new` takes a term, and a ratio is not one.
fn _divided(numerator: PlanTerm, denominator: PlanTerm, zero_denominator: ZeroDenominator) -> LegTerm {
    LegTerm::new(
        PlanMeasure::Ratio {
            numerator,
            denominator,
            zero_denominator,
        },
        String::from("ratio"),
    )
}
```

And the twin, so a rename cannot make that block pass vacuously: the two halves travel as two
terms, and the division happens above every leg.

```
use sutura_domain::plan::{LegTerm, PlanTerm};

fn _undivided(numerator: PlanTerm, denominator: PlanTerm) -> Vec<LegTerm> {
    vec![
        LegTerm::new(numerator, String::from("numerator")),
        LegTerm::new(denominator, String::from("denominator")),
    ]
}
```

##### Methods

```rust
pub fn label(&self) -> &str
```

```rust
pub const fn new(term: PlanTerm, label: String) -> Self
```

```rust
pub const fn term(&self) -> &PlanTerm
```

##### Implements

`Clone`, `Debug`, `Eq`, `PartialEq`, `Serialize`

#### `enum LegPlan`

```rust
pub enum LegPlan
```

One source's share of a federated question.

An enum rather than a struct with four `Option`s, which is this repository's *prefer
unrepresentable to checked* applied to the one place it buys the most: there are exactly two
legal shapes, and the illegal combinations - a measure with no bucket, a time range on a table
with no time column - are not constructible.

A third shape cannot arrive without a rendering arm, because a match over this enum is
exhaustive:

```compile_fail
use sutura_domain::plan::LegPlan;

// Non-exhaustive: `Lookup` renders differently and has to say so.
fn _render(leg: &LegPlan) -> &str {
    match *leg {
        LegPlan::Fact { .. } => "fact",
    }
}
```

```
use sutura_domain::plan::LegPlan;

fn _render(leg: &LegPlan) -> &str {
    match *leg {
        LegPlan::Fact { .. } => "fact",
        LegPlan::Lookup { .. } => "lookup",
    }
}
```

A lookup leg has no bucket, no terms, no range and no metric, and that is the type rather than a
check somebody runs:

```compile_fail
use sutura_domain::calendar::TimeRange;
use sutura_domain::model::{QualifiedTable, SourceName};
use sutura_domain::plan::LegPlan;

fn _dated(source: SourceName, table: QualifiedTable, range: TimeRange) -> LegPlan {
    LegPlan::Lookup {
        source,
        table,
        keys: Vec::new(),
        filters: Vec::new(),
        params: Vec::new(),
        range,
    }
}
```

```
use sutura_domain::model::{QualifiedTable, SourceName};
use sutura_domain::plan::LegPlan;

fn _undated(source: SourceName, table: QualifiedTable) -> LegPlan {
    LegPlan::Lookup {
        source,
        table,
        keys: Vec::new(),
        filters: Vec::new(),
        params: Vec::new(),
    }
}
```

A fact leg's tables arrive as a checked set, so a producer cannot state a table beside a vector
of joins and skip the ambiguity guard - which is exactly what the first producer of a leg did:

```compile_fail
use sutura_domain::calendar::TimeRange;
use sutura_domain::model::{MetricName, QualifiedTable, SourceName};
use sutura_domain::plan::{LegPlan, PlanBucket, PlanJoin};

fn _unchecked(
    source: SourceName,
    metric: MetricName,
    table: QualifiedTable,
    joins: Vec<PlanJoin>,
    bucket: PlanBucket,
    range: TimeRange,
) -> LegPlan {
    LegPlan::Fact {
        source,
        metric,
        table,
        joins,
        bucket,
        keys: Vec::new(),
        terms: Vec::new(),
        filters: Vec::new(),
        params: Vec::new(),
        range,
    }
}
```

```
use sutura_domain::calendar::TimeRange;
use sutura_domain::model::{MetricName, QualifiedTable, SourceName};
use sutura_domain::plan::{LegPlan, PlanBucket, PlanJoin, StatementTables};

fn _checked(
    source: SourceName,
    metric: MetricName,
    table: QualifiedTable,
    joins: Vec<PlanJoin>,
    bucket: PlanBucket,
    range: TimeRange,
) -> Result<LegPlan, sutura_domain::plan::AmbiguousTables> {
    Ok(LegPlan::Fact {
        source,
        metric,
        tables: StatementTables::parse(table, joins)?,
        bucket,
        keys: Vec::new(),
        terms: Vec::new(),
        filters: Vec::new(),
        params: Vec::new(),
        range,
    })
}
```

##### Variants

- `Fact` - Read off the metric's own model, grouped by the bucket and by every key the answer or a non-descending term needs.
- `Lookup` - Read off one remote dimension model: its join key, the columns the answer groups by, and whatever filters went with it.

##### Methods

```rust
pub fn filters(&self) -> &[PlanFilter]
```

The predicates this leg applies.

```rust
pub fn keys(&self) -> &[PlanKey]
```

The columns this leg groups by and projects.

```rust
pub fn params(&self) -> &[ParamValue]
```

The values bound to this leg's placeholders, in placeholder order.

```rust
pub fn result_labels(&self) -> Vec<String>
```

The labels this leg's result will carry, in the order it projects them.

One definition, for `QueryPlan::result_labels`'s reason: an adapter that builds a schema
and an adapter that renders a projection cannot disagree about it. Keys, then the bucket if
there is one, then one label per term.

```rust
pub const fn source(&self) -> &SourceName
```

The one data system this leg runs against.

Every leg is mono-source, so *a plan cannot silently span two sources* applies per leg
unchanged: neither variant has a second `SourceName` to disagree with this one.

```rust
pub const fn table(&self) -> &QualifiedTable
```

Where the table this leg reads lives: the whole path, which is what its `FROM` names.

A leg qualifies exactly as a whole-answer plan does, and it shares `generate`'s rendering to
make sure of it - two `FROM`-building paths would be two places for identifier quoting to
differ, which is the drift `sutura_sql::generate`'s own header is written against.

```rust
pub const fn table_name(&self) -> &TableName
```

The table's own name, which is what this leg's columns are qualified by.

##### Implements

`Clone`, `Debug`, `Eq`, `PartialEq`, `Serialize`

#### `enum Executable`

```rust
pub enum Executable<'plan>
```

What the execution port can be handed.

**One method rather than two, and the exhaustive match is the reason.** A second port method for
legs is a smaller diff and worse where it matters: a second method invites a default body, a
default that errors lets an adapter be silently non-federating, and *adding a data system is a
registration* then stops being true in the one direction nobody would notice. With one method
taking this enum, a third leg shape cannot be added without every adapter stating what it does
with it.

**It borrows.** A plan is built once and executed once, and the federated path multiplies the
working set against a memory bound that refuses - so a copy per leg would be a correctness
question rather than a style one.

An adapter that does not answer for every shape does not compile:

```compile_fail
use sutura_domain::plan::Executable;

fn _dispatch(executable: Executable<'_>) -> &'static str {
    match executable {
        Executable::Query(_) => "a whole answer",
    }
}
```

```
use sutura_domain::plan::Executable;

fn _dispatch(executable: Executable<'_>) -> &'static str {
    match executable {
        Executable::Query(_) => "a whole answer",
        Executable::Leg(_) => "one source's share of one",
    }
}
```

##### Variants

- `Query` - A whole answer from one data system.
- `Leg` - One data system's share of an answer assembled above it.

##### Methods

```rust
pub fn params(self) -> &'plan [ParamValue]
```

The values bound to this statement's placeholders, in placeholder order.

```rust
pub fn result_labels(self) -> Vec<String>
```

The labels the result will carry, in the order it projects them.

```rust
pub const fn source(self) -> &'plan SourceName
```

The one data system this runs against, whichever shape it is.

##### Implements

`Clone`, `Copy`, `Debug`, `Eq`, `PartialEq`

### Module `tables`

Every table one statement reads, and the guarantee that the statement can tell them apart.

# The defect this type exists for

A column in a plan is qualified by a table's BARE name - `PlanColumn` holds a `TableName` - and
the reason is that `FROM a.b.orders` gives the reference an implicit alias of `orders` in every
target this workspace renders for. `crate::model::qualified` argues that at length and it is
right; what it does not do is say what happens when TWO of the tables in one statement end their
paths with the same name.

What happened was this, and it was reproduced rather than reasoned about: a fact table at
`analytics-prod.sales.orders` joined to a dimension table at `reference-data.crm.orders` rendered
a `FROM` and a `LEFT JOIN` whose `ON` clause compared `orders.customer_id` with `orders.id` - one
table with itself - and every projected column was qualified by an identifier that named two
tables. On a real `DuckDB` 1.5.5 that statement is
`Binder Error: Ambiguous reference to table "orders"`; a target that binds it to one side instead
returns a number under a certified metric name, which is the failure class this repository is
arranged against. **Same-name tables are the normal shape of the estate `docs/adr/0019` exists
for** - dev/prod splits, per-tenant datasets, staging copies - so this is reachable rather than
exotic.

# Why a refusal, and not distinct explicit aliases

Distinct aliases are the fix that would keep the question answerable, and they are **not reachable
through the SQL builder this workspace renders with**, which was measured rather than assumed
against `polyglot-sql` 0.9.2: `SelectBuilder::from_expr` takes an expression, so the `FROM` side
could carry an `AS`, but `left_join` and every other join method take a `&str` table name and
`join_with_kind` is private - so the JOINED side cannot be aliased without hand-building a select
expression with upwards of thirty fields, which `sutura_sql`'s renderer rules out at its own header
for a reason. An alias on one side of a join and not the other is not a fix.

So the decision is the other one, and it is made where the plan is built rather than where it is
rendered: **a statement whose tables cannot be told apart is unrepresentable.** There is no
[`QueryPlan`](crate::plan::QueryPlan) and no [`LegPlan::Fact`](crate::plan::LegPlan::Fact) holding
such a set, because `StatementTables` is the only way to construct either and its canonical
constructor refuses the pair. `sutura_semantic::plan` turns
that refusal into
[`PlanTablesShareAnIdentifier`](crate::query::RefusalReason::PlanTablesShareAnIdentifier), so the
question is declined and the metric stays authorable: a question that does NOT reach the colliding
table is still answered. The alternative - refusing the metric at load - would make the estate
shape unauthorable, and unlike a label a physical table is not something an author can rename.

# The limit, and what it used to be

**It used to be the fact leg, and that was not theoretical: the change that gave a leg a producer
shipped the bypass this section predicted.** `LegPlan::Fact` carried a `table` and a `joins` field
and was built by struct literal, so a federated question over a fact table at
`analytics_prod.sales.orders` with a same-source dimension table at `reference_data.crm.orders`
compiled, and its fact leg rendered
`FROM ...sales.orders LEFT JOIN ...crm.orders ON orders.customer_id = orders.customer_id`. Worse
than the whole-answer case rather than equal to it, because a leg's rows are combined above it and
nothing downstream sees the statement. That variant now takes a `StatementTables` as its field
instead, pinned by a `compile_fail` doctest with a compiling twin, so a second leg producer cannot
reintroduce it - **a prediction in a doc comment is not a mechanism, which is the lesson worth
keeping from this.**

What remains is narrow and stated so it is not mistaken for the above. A
[`Lookup`](crate::plan::LegPlan::Lookup) leg reads ONE table and declares no joins, so it has no
pair to compare - the shape is the check. And two LEGS whose tables collide are not this defect:
each leg is its own statement on its own data system, so nothing binds one identifier to two
tables; what the combiner joins on is a label, and a label that shadowed a table is
[`LabelShadowsTable`](crate::catalog::InconsistentDefinitions::LabelShadowsTable)'s refusal at
load.

#### `enum AmbiguousTables`

```rust
pub enum AmbiguousTables
```

Why the tables one statement reads could not be told apart inside it.

One variant today, and an enum rather than a struct because a second way for a statement's tables
to be indistinguishable - a target that folds more than ASCII case, an alias this workspace starts
emitting - is a variant a caller can branch on rather than a change to a message.

##### Variants

- `OneIdentifierTwoTables` - Two of the statement's tables answer to one identifier.

##### Methods

```rust
pub const fn alias(&self) -> &TableName
```

The identifier two tables collapsed to.

##### Implements

`Clone`, `Debug`, `Display`, `Eq`, `Error`, `PartialEq`

#### `struct StatementTables`

```rust
pub struct StatementTables
```

The tables one statement reads: the `FROM` table, and one per join.

**If an instance of this type exists, every table in it is distinguishable from every other one
inside the statement** - which is the whole return on the newtype, and what lets
[`QueryPlan::new`](crate::plan::QueryPlan::new) stay infallible while the plan it builds cannot be
the ambiguous one. See this module's header for what the ambiguity does and why it is refused
rather than aliased around.

##### Methods

```rust
pub fn joins(&self) -> &[PlanJoin]
```

Every join, in the order the statement will make them.

```rust
pub fn only(table: impl Into<QualifiedTable>) -> Self
```

One table and no joins.

Infallible by construction rather than by a skipped check: a set of one has no pair to compare.

```rust
pub fn parse(table: impl Into<QualifiedTable>, joins: Vec<PlanJoin>) -> Result<Self, AmbiguousTables>
```

The `FROM` table and its joins, or a refusal if two of them answer to one identifier.

**The canonical constructor.** `Self::only` is the no-join spelling of it and repeats no
check, because one table cannot collide with itself.

Compared under `IdentifierCase::COARSEST` rather than by equality, because `GoogleSQL`
resolves an alias case-insensitively and a real `DuckDB` binds `"orders".id` against a table
declared `"Orders"` - so `Orders` beside `orders` is the same defect spelled to look like two
names. That type's own note is where the argument for comparing under the coarsest rule lives.

The comparison is over POSITIONS and not over distinct paths, so the same table joined twice
through two relationships is refused too: two occurrences under one identifier is a duplicate
alias whether or not they name the same rows.

```rust
pub const fn table(&self) -> &QualifiedTable
```

Where the statement's own table lives: the whole path, which is what the `FROM` names.

##### Implements

`Clone`, `Debug`, `Eq`, `PartialEq`, `Serialize`

## Module `query`

The tool surface: what a caller may ask, and what comes back.

**This module is the governance boundary, and it is defined by what it does not contain.**
`Query` has no field for SQL, a table name, a filter expression or a list of row ids. An
uncertified question is therefore unrepresentable rather than refused, which is a stronger
property than it sounds: a refusal can be retried until something succeeds, and an absent field
cannot.

Widening this is a governance change. `AGENTS.md` says which mechanism has to still hold.

### `struct Filter`

```rust
pub struct Filter
```

One equality filter: a dimension, and a value the pinned bundle declares.

The value is a `DimensionValue` here and a bind parameter by the time it reaches a statement. It
is checked against the metric's allowlist first, so the parameterisation is the second line of
defence rather than the only one.

# Why a caller's value is parsed by the type a catalog author's value is parsed by

It was a `String`, and the review that gave `DimensionValue` to the catalog side asked whether the
request side wanted it too. It does, for four reasons, and the last one is the decisive one:

* **It refuses nothing a request could have been answered.** The two are compared for equality
  against the metric's allowlist, and every entry in that allowlist is a `DimensionValue`. Text
  that cannot be one cannot be in there, so parsing here turns a `DimensionValueNotAllowed`
  refusal into a `400` naming the field and loses no answerable question.
* **The precedent is already here and is older than this type.** A caller's `metric` and
  `dimension` arrive as text and are parsed by `MetricName` and `DimensionName` - the same
  types the catalog loader uses, at the same boundary, by the same constructor. A value being the
  one field held to a laxer rule was the asymmetry, not the fix.
* **It bounds what a request may carry before anything allocates it.** A ten-megabyte filter value
  used to be compared against the allowlist and refused, having been read, cloned into
  `Self::literals` and rendered into whatever an audit sink keeps.
* **A second character rule is a rule nothing compares against the first.** `crate::text` exists
  because one such rule was written down twice and the copies drifted. A request-side value type
  with its own idea of what a value may hold would be that mistake, deliberately, in a place where
  one side of the comparison is content and the other is a caller.

**What does NOT follow is that a refusal may name the text.** `sutura_http::wire` parses the value
and reports `filters[i].value` without the parse error underneath it, because
[`InvalidDimensionValue`](crate::catalog::InvalidDimensionValue) carries the offending input and
`RefusalReason`'s own rule is that caller-supplied text is never reflected into a message that
reaches a log, a UI and an agent's context.

#### Methods

```rust
pub const fn dimension(&self) -> &DimensionName
```

```rust
pub const fn new(dimension: DimensionName, value: DimensionValue) -> Self
```

```rust
pub const fn value(&self) -> &DimensionValue
```

#### Implements

`Clone`, `Debug`, `Deserialize<'de>`, `Eq`, `PartialEq`, `Serialize`

### `struct Query`

```rust
pub struct Query
```

A modelled question.

`deny_unknown_fields` is load-bearing rather than strict-for-its-own-sake. Without it a question
carrying `sql:` or `table:` deserializes cleanly with the extra field dropped on the floor, and a
caller who believes they sent SQL gets an answer to a different question. With it, the attempt is
an error naming the field.

#### Methods

```rust
pub fn dimensions(&self) -> &[DimensionName]
```

```rust
pub fn filters(&self) -> &[Filter]
```

```rust
pub const fn grain(&self) -> Grain
```

```rust
pub fn literals(&self) -> BTreeSet<String>
```

The literal text this question carries, for the assertion that none of it reaches the SQL.

It exists so the no-injection golden can be written as "no value from the question appears in
the statement" rather than as a list of places to look, which is the form that goes stale the
first time a field is added.

```rust
pub const fn metric(&self) -> &MetricName
```

```rust
pub const fn new(metric: MetricName, grain: Grain, range: TimeRange, dimensions: Vec<DimensionName>, filters: Vec<Filter>) -> Self
```

```rust
pub const fn range(&self) -> TimeRange
```

#### Implements

`Clone`, `Debug`, `Deserialize<'de>`, `Eq`, `PartialEq`, `Serialize`

### `enum RefusalReason`

```rust
pub enum RefusalReason
```

Why a question was not answered.

Typed rather than prose, because the variant is the contract and the message is not. Every
variant has a test that provokes it: a refusal nobody has seen happen is a refusal nobody knows
works.

**There is no `TimeRangeUnbounded` variant, deliberately.** `TimeRange` has no unbounded form,
so such a refusal could never be provoked, and a variant with no test that can reach it looks
like coverage while being dead code. The type does that job instead.

[`TimeRangeTooLong`](RefusalReason::TimeRangeTooLong) is the variant that exists for the half the
type does *not* do, and the pair is worth reading together: an absent bound is unrepresentable, a
bound that is present and enormous is refused. The second has to be a refusal rather than a parse
error because the same `TimeRange` is also a catalog author's anchor range, and a maximum on the
type would govern authorship in order to govern requests.

Note what these variants do *not* carry: a rejected filter value is never echoed back.
`DimensionValueNotAllowed` names the dimension and stops there. Reflecting caller-supplied text
into a message that reaches a log, a UI and an agent's context is how a rejected value becomes
somebody else's input.

#### Variants

- `MetricUnknown` - No metric of that name is in the pinned bundle.
- `GrainNotSupported` - The metric exists and does not declare that grain. Not a narrower question: a grain the author did not render is a number nobody certified.
- `DimensionNotPermitted` - The metric does not declare that dimension. A dimension a metric did not declare is a name that does not resolve, not a filter to apply anyway.
- `DimensionNotFilterable` - The dimension exists but declares no value allowlist, so it can be grouped by and not filtered.
- `DimensionValueNotAllowed` - The dimension is filterable and the value is not one the bundle declares.
- `DuplicateDimension` - The same dimension appears twice in one question. Refused rather than deduplicated: a caller who sent it twice believes something we do not.
- `TooManyDimensions` - More group-by keys than `MAX_DIMENSIONS`.
- `ResultTooLarge` - The result was too much data to certify, and `ResultBound` says which bound said so.
- `TimeRangeTooLong` - A span of history longer than `MAX_RANGE_DAYS`.
- `PlanSpansTooManySources` - The plan would need to read from more than the deployment serves.
- `FederationNotExecutable` - The question asked is served by two sources, but this build has no adapter that can execute a leg.
- `FederationLinkAmbiguous` - The question's remote dimensions join the metric's own through more than one relationship.
- `MeasureDoesNotFederate` - The question's measure cannot be decomposed into one leg per source.
- `PlanTablesShareAnIdentifier` - Two tables the plan would read answer to one identifier inside one statement.
- `SourceUnavailable` - The plan named a data system this process did not open.
- `ResourcesExhausted` - An engine operator asked its memory pool for more than the deployment's working-set ceiling.
- `CredentialUnavailable` - The asking subject has no credential at that data system.

#### Implements

`Clone`, `Debug`, `Eq`, `PartialEq`, `Serialize`

### `enum ResultBound`

```rust
pub enum ResultBound
```

Which bound a result was too large for.

**The vocabulary exists so one refusal can be honest about two causes.** The answer a caller gets
is one sentence - *too much data, ask a narrower question* - and
`RefusalReason::ResultTooLarge` is that one answer. This is what the deployment knows about why,
and the two arms differ in who measured it: the row cap is a number an operator configured here,
and the volume bound belongs to the data system and is not one this process was told.

**Closed, and read by exhaustive matches with no wildcard arm in both transports.** A third bound
is a compile error in each of them rather than a case one renders as another - which is what stops
a bound with no number being described using somebody else's number. The agent-facing prompt is
deliberately NOT one of those matches: `sutura_app::prompt::guide_for` keys on the
`RefusalReason` variant and never the payload, because what it must tell an agent - *too much
data, ask a narrower question* - is the same for both bounds and a guide per bound would give an
agent two paragraphs saying one thing.

#### Variants

- `Rows` - The plan's row cap, in rows.
- `Volume` - The data system would not hand this result back in one piece.

#### Implements

`Clone`, `Copy`, `Debug`, `Eq`, `PartialEq`, `Serialize`

### `enum ToolOutcome`

```rust
pub enum ToolOutcome
```

What a tool call produced.

A refusal is a *variant of the result*, not an `Err`. A caller cannot mistake it for a transport
hiccup and retry until something works, which is what an error would invite.

#### Variants

- `Answer`
- `Refusal`

#### Methods

```rust
pub const fn is_refusal(&self) -> bool
```

```rust
pub const fn refusal(&self) -> Option<&RefusalReason>
```

The refusal reason, if this is one. Convenience for tests and for an audit sink.

#### Implements

`Clone`, `Debug`, `PartialEq`, `Serialize`

### `constant MAX_DIMENSIONS`

The most dimensions one question may group by.

A bound for the same reason the time range is bounded: a group-by over every column is a table
scan with a plausible name, and the cost lands on a shared data system. Four covers the questions
a person asks and refuses the ones a loop generates.

### `constant MAX_RANGE_DAYS`

The longest span of history one question may ask about, in days.

**This is the bound the `TimeRange` newtype does not provide.** That type refuses an *absent*
endpoint; it accepts `[0001-01-01, 9999-12-31)`, which is over three and a half million days and, on
both execution paths, a full scan. `plan::MAX_ROWS` does not help: it bounds the rows *returned*
after the aggregate - refusing a result that exceeds them - so a question that scans everything and
groups it into one bucket is inside it. The span is what rows-read is a function of, so the span is
where the cap goes.

**3653 days is ten calendar years, counted at its longest.** Ten consecutive Gregorian years hold
3652 or 3653 days depending on where the leap days fall, so this number is the one that lets
*any* ten-year window through rather than most of them. Ten years is chosen because it covers the
reporting a person actually does - a decade of annual figures, five years of quarters, three years
of months - and the longest range anywhere in this repository's example corpus, in its questions
and in its anchors alike, is 181 days - so nothing authored today is anywhere near it.

It also stays under `plan::MAX_ROWS`, and that is not a coincidence worth losing: at `day` grain
the time axis of a permitted question is at most 3653 buckets, so the row cap can only ever be
reached by dimension cardinality and never by the range alone. Raising this past the row cap would
make `RefusalReason::ResultTooLarge` the normal outcome of a wide range - a refusal nobody could
act on, because narrowing the range would not be what got them there.

A *span*, not a bucket count, and the difference matters. A bucket count would let `year` grain
through with a thousand years of scanning for a thousand rows, which is precisely the request this
exists to refuse; the span bounds the scan at every grain and bounds the buckets as a consequence.

**What it does not bound, said plainly rather than left for someone to discover.** It bounds ONE
question: three permitted ten-year questions cover thirty years, and nothing here correlates two
requests, because a per-caller budget needs a clock, a subject and somewhere to keep a counter and
this crate has none of the three. And inside a permitted span the *groups* are still the span times
the cardinality of up to `MAX_DIMENSIONS` dimensions - a dimension declared without a value list
has whatever cardinality the column has - so `plan::MAX_ROWS` refuses that result rather than
bounding the work that produced it: the groups are built, and then the answer is declined. A
refusal is not a budget. A day count is also only a proxy for rows: ten years of a small table and
ten years of a large one are the same number here. A real budget is expressed in rows or bytes
scanned, which needs something from the data system that no port asks for yet.

## Module `source`

How one source establishes the identity a query runs as, what an adapter can carry, and what
each leg of an answer actually executed as.

**Three facts by three different declarers, and conflating any two of them is how a mode acquires
two owners.** [Pluggable by declaration](https://github.com/telekom/sutura/blob/main/docs/adr/0011-pluggable-by-declaration.md)
is explicit about the split and this module is that split expressed as types:

| Fact | Who declares it | The type here |
| --- | --- | --- |
| Which identity a query reaches this source as | configuration, per source | `SourcePosture` |
| Whether the linked adapter can carry a per-subject credential *at all* | code, per adapter | `ImpersonationCapability` |
| Which identity re-ran this source's anchors at boot | configuration, per source | `SourceIdentity` |

The first two are compared at boot - `SourcePosture::deliverable_by` - because a posture the
build cannot perform is a configuration that would have to fall back, and there is no fallback.
The third is the *boot* identity and is deliberately a different type from anything on the
request path: an anchor runs before a caller exists.

# What each leg ran as, and why it is not read off the settings tree

`ExecutedAs` is what `crate::pinned::Provenance` carries. It is built from the posture the
**adapter was handed**, never from the configuration that was supposed to reach it. The two are
meant to agree, and if they ever disagreed the record has to say what *ran* - a field derived
from a file would report a leg as impersonated on the strength of a file, which is the one thing
this record exists to stop.

**State the limit next to the claim.** Recording is not a control. An answer says how it was
executed, and provenance is read by whoever holds the answer *after* the rows were served, so it
cannot prevent a disclosure and does not attempt to. What keeps a shared source from being served
unnoticed is the boot refusal in `sutura_config::Settings::refusals` and the cross-check above,
both of which happen before a listener is bound.

# Nothing here is `Deserialize`, and that is the same property `crate::identity` has

`AcknowledgementReason` and `VerificationIdentity` are text an **operator** wrote, and the
settings layer turns the key into the type by calling `parse`. A `Deserialize` would let a value
reach these newtypes without passing that constructor, and for `SharedIdentityDeclared` it
would mean a witness that no operator wrote - which is exactly the state the witness exists to
make unreachable.

What that does **not** claim: these constructors are `pub`, so any crate holding this one could
call them. The property is that a *file* cannot, and that neither type has a `Default` - so the
shared posture cannot be arrived at by leaving anything unset, and cannot be inherited from a
neighbouring source.

### `enum InvalidOperatorText`

```rust
pub enum InvalidOperatorText
```

Why a piece of operator-written text is not usable here.

One error for both newtypes below, because they are one parse with two bounds. The variants carry
the offending input as typed fields; the `#[error]` text is a convenience for a human.

#### Variants

- `Empty` - Empty or whitespace-only.
- `ControlCharacter` - Holds a control character. The startup log prints this on one line, so a newline here appends a line nobody wrote.
- `InvisibleCharacter` - Holds an invisible or direction-changing code point.
- `TooLong`

#### Implements

`Clone`, `Debug`, `Display`, `Eq`, `Error`, `PartialEq`

### `struct AcknowledgementReason`

```rust
pub struct AcknowledgementReason
```

The operator's stated reason for serving one source under one identity for every caller.

**A required value rather than a flag, because the reason is the part a reviewer needs and the
part nobody writes unless the type demands it.** A boolean acknowledgement records that somebody
clicked past a question; this records what they meant, on the source's own entry, and the startup
log prints it beside the posture.

Construct it with [`parse`](Self::parse). There is no other way in: the field is private, there is
no `Deserialize`, and `TryFrom<String>` delegates to the same constructor.

No `Default`, deliberately. A default reason is a reason nobody gave.

#### Methods

```rust
pub fn as_str(&self) -> &str
```

```rust
pub fn parse(raw: impl AsRef<str>) -> Result<Self, InvalidOperatorText>
```

Parses a reason written under `Self::KEY`.

Delegates to `Self::written_under` rather than repeating the checks.

```rust
pub fn written_under(key: &'static str, raw: impl AsRef<str>) -> Result<Self, InvalidOperatorText>
```

The canonical constructor: parses a reason written under `key`.

`key` is carried into the refusal so a message names the entry an operator has to change
rather than describing a category. There are two keys in the settings tree that produce one of
these - a source's own acknowledgement and the single-user mode declaration - and one parse, so
a rule that held for one and not the other cannot exist.

#### Implements

`Clone`, `Debug`, `Display`, `Eq`, `PartialEq`, `Serialize`

### `struct VerificationIdentity`

```rust
pub struct VerificationIdentity
```

The identity the anchor path runs as, for one source.

**A distinct type from anything on the request path, on purpose.** An anchor executes at boot,
before any caller exists, and the credential that re-runs it must not be reachable from a
handler. `sutura_app::answer` holds no value of this type and cannot construct one; when the
execution port learns to take a credential, this is what its second method takes and the request
path takes the other.

It is a *name*, not material: a role or service-account name is not secret, and the credential
behind it arrives with the credential port. Least authority on it is a configuration requirement
an operator arranges - an identity holding a row-level-security bypass certifies the *unfiltered*
number, so the anchor passes and proves less than it appears to.

Construct it with [`parse`](Self::parse). No `Deserialize` and no `Default`, so it cannot be
arrived at by leaving anything unset and cannot be inherited from a neighbouring source.

#### Methods

```rust
pub fn as_str(&self) -> &str
```

```rust
pub fn parse(raw: impl AsRef<str>) -> Result<Self, InvalidOperatorText>
```

Parses the declared identity, rejecting anything that is not a name.

#### Implements

`Clone`, `Debug`, `Display`, `Eq`, `PartialEq`, `Serialize`

### `struct SharedIdentityDeclared`

```rust
pub struct SharedIdentityDeclared
```

The witness that an operator acknowledged serving one source under one identity for everybody.

It carries the reason and nothing else, and it exists so the weaker posture is **unreachable
without an operator's own words**. A `SharedServiceUser` posture cannot be constructed without
one, so there is no arrangement of a configuration file that arrives at it by leaving a key out.

**The same witness travels onto the leg.** When the credential port lands, this is the payload of
the third `Presented` variant - the one that carries no credential material at all - which is what
ties the boot-time acknowledgement to the thing that actually executed. An acknowledgement no
operator wrote has no value to travel, so there is no leg for it to reach.

#### Methods

```rust
pub const fn of(reason: AcknowledgementReason) -> Self
```

Wraps an operator's reason as the witness.

Takes the parsed reason rather than a string, so the only way to a witness is through
`AcknowledgementReason::parse` - one canonical constructor, and this is not a second copy of
its checks.

```rust
pub const fn reason(&self) -> &AcknowledgementReason
```

What the operator said, for the startup log.

#### Implements

`Clone`, `Debug`, `Eq`, `PartialEq`, `Serialize`

### `enum SourcePosture`

```rust
pub enum SourcePosture
```

How a source establishes the identity a query runs as.

**No `Default`, and that is where it differs from `sutura_config::TlsTermination`, which is
otherwise the shape this copies.** That type defaults to `None` and is refused only where the bind
is reachable off-host - a default plus a conditional refusal - and it is right there, because a
loopback bind genuinely is the case where nothing needs declaring. Identity has no equivalent
condition: there is no bind address that makes "one identity for every caller" safe to assume. So
a source that declares no posture is refused unconditionally, and a `Default` here would be a
value that never passed a constructor.

Two variants, and the closed set is the point: an answer cannot claim a third thing happened.

#### Variants

- `SharedServiceUser` - Every query reaches this source under one identity the deployment holds. Carries the operator's acknowledgement, so it cannot be reached by leaving anything at a default.
- `ImpersonationAtSource` - Each query reaches this source as the asking subject, so the SOURCE decides what that subject sees: its own authorization, its row and column policies, its own catalog.

#### Methods

```rust
pub const fn as_str(&self) -> &'static str
```

The spelling, for the startup log and for a wire shape.

The one definition of the word, so what a log line says and what an answer carries cannot
drift apart.

```rust
pub fn deliverable_by(&self, capability: ImpersonationCapability, source: &SourceName) -> Result<(), PostureNotDeliverable>
```

Refuses a posture the linked adapter has no way to perform.

**The boot cross-check, and it is here rather than in the settings tree because half of it is
a property of the BUILD.** Configuration says which posture the deployment is asking for;
`ImpersonationCapability` says whether the code that was linked can carry a per-subject
credential at all. `sutura-config` cannot see the second, so the comparison lives where both
are in scope - the composition root - and this is the one function that makes it.

Two exhaustive matches with no wildcard arm, so a third posture or a third capability is a
compile error here rather than a case that quietly falls through to `Ok`.

```rust
pub const fn what_decides_what_a_caller_sees(&self) -> &'static str
```

What decides what a subject sees here, as a sentence for the startup log.

A function rather than a comment for the reason `TlsTermination::cleartext_hop` is one: the
log, the documentation and this type read the same value, so none of them can drift into
claiming this deployment impersonates when it does not.

#### Implements

`Clone`, `Debug`, `Display`, `Eq`, `PartialEq`, `Serialize`

### `struct PostureNotDeliverable`

```rust
pub struct PostureNotDeliverable
```

A posture was configured that the adapter behind that source cannot perform.

Its own type rather than a string, because the composition root has to name the source in a
message an operator acts on, and because a refusal that carried only prose could not be asserted
on by a test without asserting on the prose.

#### Methods

```rust
pub const fn at(&self) -> &SourceName
```

Which source was misconfigured.

Named `at` rather than `source`, and not by preference: `thiserror`'s derive gives this type an
`Error::source`, and `clippy::same_name_method` is denied - an inherent `source` beside a trait
`source` is a call site whose meaning depends on which traits are in scope.

#### Implements

`Clone`, `Debug`, `Display`, `Eq`, `Error`, `PartialEq`

### `enum ImpersonationCapability`

```rust
pub enum ImpersonationCapability
```

Whether an adapter can carry a per-subject credential **at all**.

A property of code, declared by the adapter as a required associated item on
`crate::warehouse::Warehouse` that it cannot omit. It is deliberately **not** the mode: an
adapter cannot declare a mode it does not own, because the same adapter is correct in either
posture and only the deployment knows which one it is being asked for.

`Copy`, because it is a declaration rather than a value with an identity, and because the boot
check reads it beside a posture it borrows.

**The variants are named for the MECHANISM rather than for yes and no**, which is not only style:
`Can...`/`Cannot...` share a postfix and `clippy::enum_variant_names` is denied, and the names that
survived that are the better ones anyway - a reader of `NoPlaceForASubject` at an adapter's
declaration is told why, not just that.

#### Variants

- `PerSubjectCredential` - There is a place in this adapter's path for a subject's own credential to arrive.
- `NoPlaceForASubject` - There is not. An in-process engine over local files is this: one process, one operating-system identity, and nowhere for a subject to appear. Saying so explicitly is the point of the declaration - a file engine is the easiest source in the world to assume nothing about, and "nobody declared anything for the engine" is how a deployment ends up believing its whole surface impersonates because its *network* source does.

#### Methods

```rust
pub const fn as_str(self) -> &'static str
```

The spelling, for the startup log.

#### Implements

`Clone`, `Copy`, `Debug`, `Eq`, `PartialEq`

### `struct SourceIdentity`

```rust
pub struct SourceIdentity
```

One source's whole identity declaration: the posture, and the identity its anchors re-run under.

**Parsed rather than validated**, and the pairing is what it parses: the two fields are not
independent, and three of the four combinations mean something different from the other. A
`Self::declared` that returned the struct unchecked would leave every reader to work that out
again.

#### Methods

```rust
pub const fn anchors_run_as(&self) -> AnchorIdentity<'_>
```

Which identity re-runs an anchor on this source.

**An exhaustive match rather than an `Option`, because the asymmetry is the useful part.** On a
shared source the verification identity *is* the shared identity, so nothing is configured and
an anchor is a complete claim: every caller reads that source as that one identity, so the
number the anchor certifies is the number every caller gets. On an impersonating source the
operator declares one, and if none is declared there is nothing to run the anchor as - which
`AnchorIdentity::NoneDeclared` says out loud rather than answering `None` and leaving a
reader to decide whether that is a permitted mode.

```rust
pub fn declared(source: &SourceName, posture: SourcePosture, verification: Option<VerificationIdentity>) -> Result<Self, ConflictingSourceIdentity>
```

The canonical constructor: a posture, and the verification identity if one was declared.

`source` is taken for the refusal's sake alone - a message an operator acts on has to name the
entry - and is not stored, because the registry that holds these is already keyed by it.

```rust
pub const fn posture(&self) -> &SourcePosture
```

Which identity a query reaches this source as.

#### Implements

`Clone`, `Debug`, `Eq`, `PartialEq`

### `enum ConflictingSourceIdentity`

```rust
pub enum ConflictingSourceIdentity
```

Why a source's two identity declarations do not go together.

#### Variants

- `VerificationIdentityOnASharedSource` - A verification identity was declared on a shared source, where nothing would read it.

#### Implements

`Clone`, `Debug`, `Display`, `Eq`, `Error`, `PartialEq`

### `enum AnchorIdentity`

```rust
pub enum AnchorIdentity<'declaration>
```

What an anchor on one source would re-execute as.

Three variants, and the third is not the absence of the other two: it is the state a deployment
must not boot in if the bundle declares an anchor reading that source. Not skipped, not warned
about and not treated as a passing anchor, which are the three ways this would otherwise become a
mode nobody chose.

#### Variants

- `TheSharedIdentity` - The source is shared, so the anchor runs as the one identity every caller reads it as.
- `Declared` - The source impersonates, and the operator declared a static identity for the boot path.
- `NoneDeclared` - The source impersonates and nothing was declared. There is no identity to run an anchor as.

#### Implements

`Clone`, `Copy`, `Debug`, `Eq`, `PartialEq`

### `struct ExecutedAs`

```rust
pub struct ExecutedAs
```

What each leg of one answer executed as.

**Non-empty by construction: an answer has at least one leg.** There is no empty form and no
`remove`, so an answer cannot carry an execution record that claims nothing ran - which is the
shape `crate::pinned::PinnedDefinitions::pin` uses for the digest, applied to the other half of
what travels with a result.

One entry per source rather than per leg, because a plan reads a source once: two legs against one
source would be one leg. `Self::and` refuses a second entry for a source already recorded rather
than overwriting it, so a wiring defect that ran the same source twice under two postures is an
error instead of whichever value was written last.

#### Methods

```rust
pub fn and(self, source: SourceName, posture: SourcePosture) -> Result<Self, LegAlreadyRecorded>
```

A second leg, for a federated answer.

Consumes and returns, so a record is built in one expression and there is no half-built state
for something else to read. Nothing constructs a second leg today - there is no combiner - and
the method is here because the shape of the record is what decides whether it can be added
without moving the digest, and that is cheaper to settle now than after an answer format
ships.

```rust
pub fn legs(&self) -> impl Iterator<Item>
```

Every leg, by source, in source order.

```rust
pub fn of(source: SourceName, posture: SourcePosture) -> Self
```

One leg. The canonical constructor, and the only way a record comes into existence.

```rust
pub fn posture(&self, source: &SourceName) -> Option<&SourcePosture>
```

What one source's leg ran as, if this answer has one.

#### Implements

`Clone`, `Debug`, `Eq`, `PartialEq`, `Serialize`

### `struct LegAlreadyRecorded`

```rust
pub struct LegAlreadyRecorded
```

A second leg was recorded for a source that already had one.

#### Implements

`Clone`, `Debug`, `Display`, `Eq`, `Error`, `PartialEq`

## Module `warehouse`

The execution port: the plan that goes out, and the rows that come back.

The trait is named `Warehouse`, which is the port's name and says nothing about what sits behind
it. A file read by an in-process engine and a cluster with a login are both implementations.

**No statement appears in this module, and its absence is the decision rather than an omission.**
A rendered statement used to live here, on the argument that the port had to hand one to
something. The port takes a `crate::plan::QueryPlan` now - `Warehouse` below says why that is
what makes a second kind of adapter possible - so nothing in the domain constructs or reads a
statement, and the type that carries one moved out to `sutura-sql`, beside the code that renders
it. A domain holding a rendered statement has acquired a concept no domain operation uses.

What stays is `ParamValue`, and it stays because the type the port *does* take is built out of
it: a `crate::plan::QueryPlan` carries a vector of them. It is also where the rule lives - a
value is a closed set of typed variants an adapter binds, never text somebody concatenated.
`crate::query` is the *tool* surface, where SQL must be unrepresentable because the text would
come from a caller; here there is no text for a value to reach at all.

### `enum ParamValue`

```rust
pub enum ParamValue
```

A value bound to a placeholder.

A closed set rather than a string, because the whole point is that these never become text on
our side. An adapter binds them with whatever its driver offers, and the driver is what decides
how a date is written on the wire.

**There is no `Integer`, and its absence is the decision rather than an omission.** The variant
was here and nothing in the workspace constructed one: every caller value and every required
filter binds as [`Text`](ParamValue::Text), because that is the type both of them are. Both
adapters carried an arm for it and the goldens carried a rendering, so it read as covered while
no question could reach it - and the dead arm was the lesser half of the cost. The real half is
that a *numeric* definitional filter cannot be expressed safely here: `equals: { column:
amount_cents, value: "500" }` compares an integer column against a text parameter, `DuckDB`
casts it and answers, a driver that sends an explicitly-typed text parameter does not, and
nothing refuses the definition because a `crate::catalog::Model` declares only column NAMES -
there is no column type to check the value against. Adding the variant back without one would
mean guessing the type from the value's own text, which makes a text column whose allowed value
is `"500"` compare as a number: the same wrong comparison, arrived at from the other side.

So it goes when a typed column model does, and not before. The reasoning is the one
`sutura_exec_datafusion`'s `cell` gives for leaving `Date64` unmapped: an unreachable arm holding
a semantic choice nobody reviewed is worse than not having the arm.

#### Variants

- `Text`
- `Date`

#### Methods

```rust
pub fn render(&self) -> String
```

A human-readable form, for showing a plan to a person.

**Display only.** It is deliberately not the SQL literal for the value: a function that
produced one would be the thing somebody reaches for the day they want to inline a parameter,
and inlining a parameter is the one move this type exists to prevent. Text is quoted the way
`Debug` quotes it, which makes an empty or space-padded value visible rather than SQL-shaped.

#### Implements

`Clone`, `Debug`, `Eq`, `PartialEq`, `Serialize`

### `enum NotFinite`

```rust
pub enum NotFinite
```

Why a floating-point cell was refused.

Two variants rather than one, because the two faults have different causes and a reader chasing
one is not chasing the other: an infinity is a non-zero quantity divided by zero, and a `NaN` is
zero divided by zero. The variant carries the value rather than a formatted sentence, for the
reason every error in this crate does.

#### Variants

- `Infinite` - Infinite, in either direction.
- `NotANumber` - Not a number at all. Its own variant rather than a value on the one above, because `NaN` compares unequal to itself: an `Infinite` carrying one would make two of these errors unequal for a reason that has nothing to do with what happened.

#### Implements

`Debug`, `Display`, `Error`, `PartialEq`

### `struct Real`

```rust
pub struct Real
```

A real number a result may carry: finite, and nothing else.

**Parsed rather than validated, and the class it closes is larger than the bug that found it.**
A cell used to be a raw `f64`, so `inf`, `-inf` and `NaN` were all representable, and
`Value::render` turned the first of them into the string `"inf"` - an answer under a metric's
own certified name that reads as data and is not a number. The route in was a ratio measure
declaring `zero_denominator: fails`: both adapters cast the numerator to a floating type before
dividing, so the division is IEEE float division, and IEEE float division by zero does not fail.
It answers `inf`, or `NaN` when both halves are zero.

Making the domain type refuse a non-finite value closes all three at once, at the one boundary
every adapter has to cross, rather than guarding the one variant that exposed it. An adapter that
gets one back has an error naming the column, which is what `fails` was always claiming to mean.

Construct it with `parse`. The field is private, so a non-finite value is unrepresentable
rather than merely rejected. There is deliberately no `Deref` and no arithmetic: two finite
numbers divide to a non-finite one, so a type that let the result back in without passing
`parse` again would be the hole this closes. `Value` is `Serialize` only today - if it ever
gains `Deserialize`, this needs `#[serde(try_from = ..)]` routing through `parse`, because a
derived one writes straight into the private field.

`parse`: Real::parse

#### Methods

```rust
pub const fn get(self) -> f64
```

The number, for a caller that has to do arithmetic on it.

Named rather than reached through `Deref`, so the point at which the invariant stops applying
is a call somebody wrote.

```rust
pub const fn parse(value: f64) -> Result<Self, NotFinite>
```

Parses a real number, rejecting a non-finite one.

#### Implements

`Clone`, `Copy`, `Debug`, `Display`, `LowerExp`, `PartialEq`, `Serialize`

### `enum Value`

```rust
pub enum Value
```

One cell of a result.

`Real` is deliberately last on the list of things to reach for. A measure over integer minor
units stays exact, and an anchor comparison over a float would depend on how two languages print
the same bits. It exists because `avg` has to land somewhere - and it is a checked type rather
than an `f64`, so the one thing a float can be that a number cannot does not fit in a cell.

#### Variants

- `Null`
- `Integer`
- `Real`
- `Text`

#### Methods

```rust
pub fn render(&self) -> String
```

The canonical text form, which is what an anchor is compared against.

One function so there is one answer. An anchor comparison that formatted the value at the
call site would compare differently in two places, and the failure would look like a data
problem rather than a formatting one.

#### Implements

`Clone`, `Debug`, `PartialEq`, `Serialize`

### `struct RowSet`

```rust
pub struct RowSet
```

A result set: the column labels, and the rows.

Labels are `String` rather than `crate::model::ColumnName` because a generated projection names
things a model did not: the truncated time bucket, and the measure under the metric's own name.
Constraining them to model column names would mean either lying about what they are or refusing
to name them.

#### Methods

```rust
pub fn cell(&self, row: usize, column: usize) -> Option<&Value>
```

One cell, by row and column position.

`Option` rather than indexing, because `indexing_slicing` is denied for library crates here
and because a caller that has a position from `column_index` still should not be able to
panic on a result set that came back a different shape than expected.

```rust
pub fn column_index(&self, label: &str) -> Option<usize>
```

Where a column with this label sits, if there is exactly one.

`None` for a label that appears twice, not the first match. Two columns with one label means
the projection is not what we think it is, and returning either of them would answer with a
number from a column nobody chose. `Definitions::assemble` refuses the catalog shapes that
could cause it, so this is the second line rather than the first.

```rust
pub fn columns(&self) -> &[String]
```

```rust
pub fn new(columns: Vec<String>, rows: Vec<Vec<Value>>) -> Result<Self, MalformedRowSet>
```

Builds a result set, rejecting a ragged one.

```rust
pub fn rows(&self) -> &[Vec<Value>]
```

```rust
pub const fn scalar(&self) -> Option<&Value>
```

The single cell of a single-row, single-column result, which is what an anchor check reads.

`None` for any other shape rather than a panic or a silent first-cell: an anchor query that
came back with three rows means the statement is not the one we thought, and reading its
first cell would turn that into a wrong number.

#### Implements

`Clone`, `Debug`, `PartialEq`, `Serialize`

### `enum MalformedRowSet`

```rust
pub enum MalformedRowSet
```

Why a result set could not be built.

#### Variants

- `RowWidth` - A row has a different number of cells than there are columns.

#### Implements

`Debug`, `Display`, `Eq`, `Error`, `PartialEq`

### `enum PreFlight`

```rust
pub enum PreFlight
```

What a pre-flight established.

**`Self::NotAsked` is not `Self::Accepted`, and no caller can read it as one.** Before
`dry_run` took a credential, a default of `Ok(())` was defensible: with nothing to be wrong
about, "nothing went wrong" is honest. With a subject in the signature it stops being honest,
because `Ok(())` from an adapter that did not look is indistinguishable from `Ok(())` from an
adapter that asked the data system as that subject and was told yes - so a defaulted pre-flight
would read as "this subject may run this plan" for every adapter that declined to implement one.

The shape is the one the row cap already uses, where `row_limit()` is `max_rows + 1` so a result
*at* the cap is distinguishable from one cut off *by* it. `docs/adr/0008` part 1 is the decision.

**The limit, stated with the claim:** `Self::Accepted` is the data system's opinion at
pre-flight time and not a guarantee about `execute`, so it is worth a round trip and is not an
authorization decision. Nothing in the plan path may treat it as one, and there is no mechanism
that would stop it - skipping a check on the strength of `Accepted` is a review question.

#### Variants

- `NotAsked` - The adapter did not ask. The default, and the honest answer for an adapter where checking costs what running costs.
- `Accepted` - The data system was asked, as this subject, and accepted the plan.

#### Implements

`Clone`, `Copy`, `Debug`, `Eq`, `PartialEq`

### `struct AnchorRows`

```rust
pub struct AnchorRows
```

The rows one anchor's plan produced at boot.

**A wrapper with a private field, so a boot result cannot be handed back to a caller as an
answer without a named conversion somebody wrote.** The anchor path and the request path are two
ways into a data system and they run as different identities: `execute` takes the asking
subject's credential and cannot be called without one, and `Warehouse::verify_anchor` takes no
credential at all - it runs as whatever identity the deployment configured that adapter with,
which is what `docs/adr/0008` part 1 decides for a path that has no caller.

Two types rather than one so the separation is visible at a call site rather than in a comment.
`Self::verified_at_boot` is named to be conspicuous in review and in a grep, the way
`crate::identity::Secret::expose_secret` is.

#### Methods

```rust
pub const fn of(rows: RowSet) -> Self
```

What an adapter returns from a verification run.

```rust
pub const fn verified_at_boot(&self) -> &RowSet
```

The rows, for the boot path that compares them against what an author certified.

#### Implements

`Clone`, `Debug`, `PartialEq`

### `trait Warehouse`

```rust
pub trait Warehouse
```

Where a plan runs.

**The port takes a `crate::plan::QueryPlan`, not a statement, and that is what makes a second
kind of adapter possible.** Taking a rendered statement said that every data system speaks SQL.
An in-process engine does not: it executes a logical plan over Arrow and generates no SQL at all.
So the plan is the contract and rendering is one adapter's private business.

`dry_run` exists separately from `execute` because "would this be accepted" is worth being able
to ask before committing to the cost of an answer - **where asking is cheaper than answering.**
It is defaulted rather than required for exactly that reason: an adapter for which it is not
cheaper has no way to say so if the port demands an implementation, and the honest thing for it to
do is nothing.

# Nothing here executes without saying whose credential it holds

`Self::execute` takes a `Presented` and has no default, so there is no code path into a data
system that runs as whatever the process happens to be. **Today's signature IS the fallback:** an
adapter with no credential parameter runs as the process, and nothing anywhere had to decide
that. `docs/adr/0008` part 1 is the decision, and the mechanism is the absence of a signature
rather than a rule somebody follows.

The boot path is the other caller of this port and it has no subject, so it gets its own method:
`Self::verify_anchor` takes no credential and returns `AnchorRows` rather than a `RowSet`.
**Which is narrower than the record asked for, deliberately.** `docs/adr/0008` gave that method a
`VerificationIdentity` parameter so the two credentials could not be confused at a call site, and
then named a `compile_fail` test asserting that answering a question cannot pass one. That test
could not have held: `crate::source::VerificationIdentity::parse` is `pub`, so any crate can
construct one. A method that takes NO credential has no parameter to pass one to, which is the
property the record wanted, reached by removing the argument instead of by typing it.

**What bounds a method with no credential is WHERE it is called from, and that is a lint here
rather than a type - which is a second review's correction to a claim this comment used to make.**
The first correction gave `Self::verify_anchor` an `AnchorPlan` instead of a bare
[`QueryPlan`](crate::plan::QueryPlan) and said the method could no longer be handed a question. A
reviewer disproved that in one function: the constructor is `pub`, every value it read was
publicly constructible, and a fabricated tuple passed all four guards. That is not a hole a fifth
guard closes - a shape check over caller-constructible values can only ever be a shape check, and
Rust has no cross-crate friend visibility to hide the constructor behind.

So the two mechanisms are named separately, because they do different jobs:

- `clippy.toml` bans `sutura_domain::warehouse::Warehouse::verify_anchor`, verified to resolve by
  writing the call and watching clippy reject it. `sutura_app::verify_anchors` holds the single
  `#[expect]`, so a second call site is an error under `-D warnings` until somebody writes a
  second expectation a reviewer sees in the diff. **That is what makes the path boot-only**, and
  its limit is that a lint reaches this workspace and an `#[allow]` walks past it.
- `AnchorPlan` checks that the boot path compiled the question it meant to, reading the metric's
  definition, its anchor's range and its coarsest grain off the pinned bundle rather than taking
  them as arguments. It is a **self-check on that one caller and not an authority**, and the type
  says so at length.

# The two identity declarations, and why they are two

`Self::IMPERSONATION` is a property of the **code**: whether this adapter has anywhere for a
subject's own credential to arrive. It is an associated constant with no default, so an adapter
cannot omit it, and it cannot vary per instance - which is what lets a boot check mean anything.

`Self::posture` is a property of the **deployment**: which identity a query is to reach this
source as. It is a method, because the composition root hands it to the adapter at construction,
and it is required rather than defaulted because a default posture is a posture nobody chose.

The two are compared at boot by `SourcePosture::deliverable_by`. Conflating them was the tempting
mistake and it gives the mode two owners: an adapter cannot declare a mode it does not own,
because the same adapter is correct in either posture and only the deployment knows which one it
is being asked for.

**An adapter that declares no impersonation capability does not compile:**

```compile_fail
use sutura_domain::identity::Presented;
use sutura_domain::model::SourceName;
use sutura_domain::plan::{AnchorPlan, Executable};
use sutura_domain::source::SourcePosture;
use sutura_domain::warehouse::{AnchorRows, RowSet, Warehouse};

struct Undeclared {
    source: SourceName,
    posture: SourcePosture,
}

// No `const IMPERSONATION`, so this impl is incomplete: the trait declares it with no default.
impl Warehouse for Undeclared {
    type Error = core::fmt::Error;

    fn source(&self) -> &SourceName {
        &self.source
    }

    fn posture(&self) -> &SourcePosture {
        &self.posture
    }

    fn execute(&self, _executable: Executable<'_>, _presented: &Presented) -> Result<RowSet, Self::Error> {
        Err(core::fmt::Error)
    }

    fn verify_anchor(&self, _plan: AnchorPlan<'_>) -> Result<AnchorRows, Self::Error> {
        Err(core::fmt::Error)
    }
}
```

The compiling twin, so the block above cannot be passing on a typo - the only difference between
the two is the one line that declares the capability:

```
use sutura_domain::identity::Presented;
use sutura_domain::model::SourceName;
use sutura_domain::plan::{AnchorPlan, Executable};
use sutura_domain::source::{ImpersonationCapability, SourcePosture};
use sutura_domain::warehouse::{AnchorRows, RowSet, Warehouse};

struct Declared {
    source: SourceName,
    posture: SourcePosture,
}

impl Warehouse for Declared {
    type Error = core::fmt::Error;

    const IMPERSONATION: ImpersonationCapability = ImpersonationCapability::NoPlaceForASubject;

    fn source(&self) -> &SourceName {
        &self.source
    }

    fn posture(&self) -> &SourcePosture {
        &self.posture
    }

    fn execute(&self, _executable: Executable<'_>, _presented: &Presented) -> Result<RowSet, Self::Error> {
        Err(core::fmt::Error)
    }

    fn verify_anchor(&self, _plan: AnchorPlan<'_>) -> Result<AnchorRows, Self::Error> {
        Err(core::fmt::Error)
    }
}

assert_eq!(
    <Declared as Warehouse>::IMPERSONATION,
    ImpersonationCapability::NoPlaceForASubject
);
```

### Module `preflight`

The pre-flight's own vocabulary: what a data system said about the tables a bundle names.

**`pub mod` with no re-export beside it, and that is a documentation decision rather than a
style one.** The domain's usual shape is a private submodule plus a `pub use`, which rustdoc
inlines into the parent - and it did NOT inline here: `just api` generated three
`### use None` stubs and no content for these three types, so the published reference would have
carried a port method returning a type it does not describe. A public module gets documented.
What a data system answered when it was asked whether the bundle's tables are there.

**A module of its own rather than three types in `warehouse.rs`**, and the seam is a real one:
nothing here is about a plan, a credential or a row. It is the vocabulary for one question a
composition root asks once, at boot, before a listener is bound - *does this data system hold the
tables the bundle names* - and the reason it needs a vocabulary at all is that there are three
honest answers and only two of them are about the tables.

# The asymmetry this exists to close

A `files` deployment whose catalog names a table with no file behind it does not start: the
engine is given one file per model, and a missing one is a refusal naming the model. A networked
data system has no such step - the tables live in the dataset, and this process learns whether
one is there when a question reaches it. So the same mistyped table name cost a boot refusal on
one kind of deployment and a failed answer for whoever asked first on the other.

[`Warehouse::preflight`](crate::warehouse::Warehouse::preflight) is the port that closes it, and
this is the answer it returns.

**The links here are `crate::`-prefixed rather than `super::`-prefixed on purpose.** The API
reference pages are generated from these doc comments and copied through verbatim, and
`AGENTS.md` records a MEASURED boundary: `crate::`-prefixed links do not warn under
`mkdocs build --strict`, while another shape aborted it. `super::` was never measured, so it is
not the form to find out with.

#### `struct AbsentTables`

```rust
pub struct AbsentTables
```

A non-empty set of tables a data system was asked about and does not hold.

**A newtype whose `parse` refuses an empty set, so `AllBut(nothing)` is unrepresentable rather
than checked.** The variant it fills is read by a composition root as *refuse to start and name
these*, and a set with no names in it would produce a refusal naming nothing - the shape of a
boot failure an operator cannot act on. There is one way to get one and it takes the set.

##### Methods

```rust
pub const fn named(&self) -> &BTreeSet<QualifiedTable>
```

The tables, for a refusal that names them.

```rust
pub fn parse(tables: BTreeSet<QualifiedTable>) -> Result<Self, NotAbsent>
```

Parses a set of tables a data system does not hold.

**The canonical constructor**, and `TablesPresent::of` is the only caller that matters: it
routes an empty set to `TablesPresent::All`, so no call site has to decide what an empty
set means.

##### Implements

`Clone`, `Debug`, `Eq`, `PartialEq`

#### `enum NotAbsent`

```rust
pub enum NotAbsent
```

Why a set of absent tables is not one.

One variant, and it is an enum rather than a unit struct for the reason every other error in this
domain is one: a second reason has somewhere to go.

##### Variants

- `Nothing` - Nothing was named. Say `TablesPresent::All` instead.

##### Implements

`Clone`, `Copy`, `Debug`, `Display`, `Eq`, `Error`, `PartialEq`

#### `enum TablesPresent`

```rust
pub enum TablesPresent
```

What a data system said about the tables it was asked for.

**`Self::NotAsked` is not `Self::All`, and no caller can read it as one.** That is the shape
[`PreFlight`](crate::warehouse::PreFlight) already uses and it is here for the same reason: the port's
default has to be *nothing to report*, because an adapter that cannot ask a data system cheaply
must not be forced to lie - and a default of "every table is there" is exactly that lie, told at
boot, in the one place a deployment is deciding whether to serve at all. An adapter that really
looked and found everything answers `Self::All`; one that did not look answers the default, and
a composition root can tell which it got.

**The third outcome is not a variant here, deliberately.** A data system that could not be
asked at all - a credential with no permission to list, a dataset that is not there, an endpoint
that did not answer - is an `Err` from the port, not a variant of this enum. Two reasons, and the
first is the one that decides it: *could not verify* and *this table is absent* must not collapse
into one message, because an operator told the wrong one fixes the wrong thing, and the adapter's
own error type is where the reason lives in the detail an operator needs. The second is that a
variant would need a reason field, and a reason field in a domain enum is either a bounded string
nobody owns or an erased cause the domain has no vocabulary for.

**The limit, stated with the claim:** what this reports is that a table EXISTS. It says nothing
about the columns a model names on it, and nothing about whether the identity that asked can
read it: a listing grant and a read grant are two grants. An anchor is what covers both, for the
metrics that have one.

##### Variants

- `NotAsked` - The adapter did not ask. The port's default, and the honest answer for an adapter that has already answered this question another way - a file engine is given its tables at boot, so a second check would be a check on the set it just built.
- `All` - The data system was asked and holds every table it was asked about.
- `AllBut` - The data system was asked and does not hold these.

##### Methods

```rust
pub const fn absent(&self) -> Option<&AbsentTables>
```

The tables that are not there, if any were named.

`None` for both `Self::NotAsked` and `Self::All`, which is correct for a caller asking
*what do I refuse over* and is exactly why `Self::was_asked` is a separate question.

```rust
pub fn of(absent: BTreeSet<QualifiedTable>) -> Self
```

The answer for an adapter that asked, from the tables it found absent.

**One constructor, and it is what keeps the empty case from being a decision at a call
site.** An adapter computes the difference between what it was asked about and what it holds
and hands the result over; an empty difference is `Self::All`, which is the only honest
reading of *I asked and nothing was missing*.

```rust
pub const fn was_asked(&self) -> bool
```

Whether the adapter looked at all.

Read by a composition root's log line, which says a different thing for a deployment nobody
verified than for one that was verified clean.

##### Implements

`Clone`, `Debug`, `Eq`, `PartialEq`
