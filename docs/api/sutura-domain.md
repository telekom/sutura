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
`thiserror` and their proc-macro support, and nothing else - which is why there is a hand-written
calendar in `calendar` and no `serde_json` in any test here.

**Two ports live here now, and each arrived with the adapter that implements it.** A port exists
to invert a dependency on something outside the hexagon, so a trait with no implementor is a
guess at a signature that only the first real adapter can settle, and in a library crate `pub`
hides such a guess from `dead_code`. `pinned::SemanticCatalog` arrived with the local catalog
adapter and `warehouse::Warehouse` with the `DuckDB` one. `CredentialBroker` is still absent for
the same reason it always was: nothing implements it yet.

The modules are grouped by concept rather than named after traits, so a port sits next to the
types it speaks in:

- `model` and `calendar` are the vocabulary: names, closed sets, dates.
- `measure` is what a metric measures, as a closed vocabulary of shapes rather than an
  expression language.
- `plan` is what we decided to execute, and the artifact the execution port speaks in.
- `catalog` is what a catalog says, and where its cross-references are checked.
- `pinned` is the hashed snapshot a question resolves against, plus the catalog port.
- `query` is the tool surface, defined mostly by what it has no field for.
- `warehouse` is the execution port. It speaks in plans, so an adapter that executes without
  generating any SQL is a first-class implementation of it rather than a special case.
- `definitions` and `identity` hold the digest and the credential-shaped newtypes.

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
pub const fn new(name: ModelName, source: SourceName, table: TableName, columns: BTreeSet<ColumnName>, description: String) -> Self
```

```rust
pub const fn source(&self) -> &SourceName
```

```rust
pub const fn table(&self) -> &TableName
```

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

#### Methods

```rust
pub const fn allowed_values(&self) -> Option<&BTreeSet<String>>
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
pub const fn new(name: DimensionName, column: ColumnName, via: Option<RelationshipName>, allowed_values: Option<BTreeSet<String>>, description: String) -> Self
```

```rust
pub fn permits(&self, value: &str) -> bool
```

Is `value` one the bundle declares?

A dimension with no allowlist answers `false` for everything, which is the safe direction:
the caller gets `DimensionNotFilterable` rather than a query.

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

#### Methods

```rust
pub const fn new(range: TimeRange, value: String) -> Self
```

```rust
pub const fn range(&self) -> TimeRange
```

```rust
pub fn value(&self) -> &str
```

#### Implements

`Clone`, `Debug`, `Deserialize<'de>`, `Eq`, `PartialEq`, `Serialize`

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
pub const fn new(name: MetricName, model: ModelName, measure: Measure, required_filters: Vec<RequiredFilter>, time_column: ColumnName, grains: BTreeSet<Grain>, dimensions: BTreeMap<DimensionName, Dimension>, anchor: Option<Anchor>, description: String) -> Self
```

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

### `struct Definitions`

```rust
pub struct Definitions
```

Everything a catalog said, with its cross-references checked.

`BTreeMap` throughout rather than `HashMap`, and that is load-bearing: the digest is taken over
the serialized form of this value, and an unordered map serializes in whatever order its hasher
chose this run. A digest that moves without the content moving is a digest nobody trusts, and
then the pinning is decoration.

#### Methods

```rust
pub fn assemble(models: Vec<Model>, relationships: Vec<Relationship>, metrics: Vec<Metric>) -> Result<Self, InconsistentDefinitions>
```

Assembles definitions from what an adapter read, checking every cross-reference.

Takes vectors rather than maps so the duplicate checks are ours: a caller that built a map
first has already silently dropped one of a duplicated pair, and "the second declaration of
revenue won" is not a thing to discover from a number.

```rust
pub fn metric(&self, name: &MetricName) -> Option<&Metric>
```

```rust
pub const fn metrics(&self) -> &BTreeMap<MetricName, Metric>
```

```rust
pub fn model(&self, name: &ModelName) -> Option<&Model>
```

```rust
pub const fn models(&self) -> &BTreeMap<ModelName, Model>
```

```rust
pub fn relationship(&self, name: &RelationshipName) -> Option<&Relationship>
```

```rust
pub const fn relationships(&self) -> &BTreeMap<RelationshipName, Relationship>
```

#### Implements

`Clone`, `Debug`, `Eq`, `PartialEq`, `Serialize`

### `enum InconsistentDefinitions`

```rust
pub enum InconsistentDefinitions
```

Why a set of definitions does not hold together.

Every variant is a dangling reference of some kind. Catching them here, once, is what lets the
resolver assume that a metric's model exists and that a dimension's column is real: without it
each of those becomes a runtime branch on the query path, and the failure surfaces as a data
system error rather than as a refusal.

#### Variants

- `DuplicateModel`
- `DuplicateMetric`
- `DuplicateRelationship`
- `UnknownModel`
- `UnknownMeasureColumn`
- `UnknownRequiredFilterColumn`
- `UnknownTimeColumn`
- `NoGrains`
- `UnknownRelationship`
- `RelationshipFromUnknownModel`
- `RelationshipToUnknownModel`
- `RelationshipUnknownColumn`
- `UnknownDimensionColumn`
- `RelationshipNotFromMetricModel`
- `JoinWouldDuplicateRows`
- `EmptyAllowlist`
- `DimensionShadowsTimeBucket`
- `DimensionShadowsMeasure`

#### Implements

`Debug`, `Display`, `Eq`, `Error`, `PartialEq`

### `constant TIME_BUCKET_LABEL`

The label a generated projection gives the truncated time column.

It lives here rather than in the compiler because it is part of the result schema, which is a
contract, and because `Definitions::assemble` has to know it: a dimension by this name would
produce two columns with one label, and a caller reading a result by name would get whichever
the data system listed first.

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
they are variants rather than a panic. `Definitions` holds no floats and every map key is a
newtype over a string, so the serializer has nothing to refuse; and lower-case hex of 32 bytes is
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

## Module `identity`

Who a request runs as, and the credential material that proves it.

Named for the concept rather than for the mechanism it currently uses. `redact` was the
earlier name, and it described one property of one type - so the module could not hold
the principal chain, the request context or the `CredentialBroker` port that belong beside
it, and every one of those would have arrived somewhere else.

The redaction is the point of `Secret`, so it has a test. A secret that reaches a log
through `{:?}` is not recoverable once shipped, and every structured-logging call site is
a chance for it - so the type, not the call site, is where this is fixed.

### `struct Secret`

```rust
pub struct Secret
```

An opaque secret. `Debug` prints a placeholder; the value is reachable only by an
explicit, greppable call to `Secret::expose`.

Deliberately NOT `PartialEq`/`Eq`. A derived comparison on credential material is a
byte-wise one that returns early on the first difference, which is a timing oracle at
whatever call site adds it later - and the call site is where it would be invisible.
Nothing here needs to compare secrets; when something does, it arrives with a
constant-time implementation and a name that says so, not with a derive. Until then the
absence of the impl is the enforcement: `a == b` on a `Secret` does not compile.

#### Methods

```rust
pub fn expose(&self) -> &str
```

Named to be conspicuous in review and in a grep. Prefer passing `Secret` around.

```rust
pub fn new(value: impl Into<String>) -> Self
```

Infallible on purpose: every string is a valid secret. There is no invariant here
beyond opacity, and a constructor that returned `Result` would be inventing one.

#### Implements

`Clone`, `Debug`, `Display`

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
pub const fn value(&self) -> Option<&String>
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

#### Implements

`Debug`, `Display`, `Eq`, `Error`, `PartialEq`

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
pub fn parse(raw: impl AsRef<str>) -> Result<Self, InvalidIdentifier>
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
pub fn parse(raw: impl AsRef<str>) -> Result<Self, InvalidIdentifier>
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
pub fn parse(raw: impl AsRef<str>) -> Result<Self, InvalidIdentifier>
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
pub fn parse(raw: impl AsRef<str>) -> Result<Self, InvalidIdentifier>
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
pub fn parse(raw: impl AsRef<str>) -> Result<Self, InvalidIdentifier>
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
pub fn parse(raw: impl AsRef<str>) -> Result<Self, InvalidIdentifier>
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
pub fn parse(raw: impl AsRef<str>) -> Result<Self, InvalidIdentifier>
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

## Module `pinned`

The snapshot a question is answered against, and the port it arrives through.

Definitions do not arrive live. They arrive as a `PinnedDefinitions`: a whole
`crate::catalog::Definitions` with a version and a digest over its canonical form. Two
consequences follow, and both are the reason this type exists rather than passing
`Definitions` around directly.

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

What it does hold is the hashing. `PinnedDefinitions::pin` takes a version and a set of
definitions and nothing else: the digest is computed here, from the value being stored, by
`DefinitionDigest::of`. The previous shape took the hash *function* from its
caller, on the argument that the domain could not hash - and that left the hole intact, because a
function handed the definitions is not a function that read them. `crate::definitions` says what
the twelve allowlisted crates bought.

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
- `TooLong`

#### Implements

`Debug`, `Display`, `Eq`, `Error`, `PartialEq`

### `struct Provenance`

```rust
pub struct Provenance
```

What defined an answer, travelling with it.

A result cannot be separated from what defined it, so this is a typed field a caller reads
deliberately rather than a sentence concatenated into a channel that also carries instructions.

#### Methods

```rust
pub const fn digest(&self) -> &DefinitionDigest
```

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
pub fn pin(version: DefinitionVersion, definitions: Definitions) -> Result<Self, NotDigestible>
```

Pins a set of definitions, computing the digest here, from them.

**Two arguments, and the absence of a third is the mechanism.** This constructor has been
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
`DefinitionDigest::of` on the value it is about to store, and there is no parameter, closure
or trait through which a caller can influence what the digest is taken over.
`crate::definitions` holds the canonical form, the hash, and the measured cost of the two
dependencies that made it possible.

The forgery a caller could write before does not compile - there is no third parameter to
pass it as:

```compile_fail
use core::convert::Infallible;
use sutura_domain::catalog::Definitions;
use sutura_domain::definitions::DefinitionDigest;
use sutura_domain::pinned::{DefinitionVersion, PinnedDefinitions};

// The digest of some OTHER catalog, returned by a closure that ignores its argument.
fn _forged(
    version: DefinitionVersion,
    definitions: Definitions,
    elsewhere: DefinitionDigest,
) -> Result<PinnedDefinitions, Infallible> {
    PinnedDefinitions::pin(version, definitions, |_| Ok(elsewhere))
}
```

Nor is there a way past the constructor. The fields are private, so the struct literal that
would pair them by hand is not a struct literal a caller can write:

```compile_fail
use sutura_domain::catalog::Definitions;
use sutura_domain::definitions::DefinitionDigest;
use sutura_domain::pinned::{DefinitionVersion, PinnedDefinitions};

fn _by_hand(
    version: DefinitionVersion,
    digest: DefinitionDigest,
    definitions: Definitions,
) -> PinnedDefinitions {
    PinnedDefinitions { version, digest, definitions }
}
```

And the twin of both blocks, which pins the signature so that a rename cannot make either of
them pass vacuously:

```
use sutura_domain::catalog::Definitions;
use sutura_domain::definitions::NotDigestible;
use sutura_domain::pinned::{DefinitionVersion, PinnedDefinitions};

fn _pin(
    version: DefinitionVersion,
    definitions: Definitions,
) -> Result<PinnedDefinitions, NotDigestible> {
    PinnedDefinitions::pin(version, definitions)
}
```

```rust
pub fn provenance(&self) -> Provenance
```

The provenance to attach to any answer produced from this bundle.

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
- `SourceMismatch` - The plan names a data system this process did not open. Not prose in a report field: it is the same condition the query path refuses, and it is a misconfigured composition root rather than an outage.
- `NotOneNumber` - The declared range covers more than one period at the metric's coarsest grain, so the result is several numbers and an anchor is one.
- `NoMeasureColumn` - The result carries no column named after the metric, so there is nothing to compare.
- `ResultShapeMismatch` - The result set was not the shape it reported.
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

- `AnchorMismatch`
- `AnchorNotExecuted` - The reason is the `source`, not the message, so whoever renders this walks the chain and gets the data system's own complaint. Interpolating it would have printed the outermost message and stopped, which is the whole of what was wrong before.
- `AnchorUnchecked`
- `UnknownMetricChecked`

#### Implements

`Debug`, `Display`, `Eq`, `Error`, `PartialEq`

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
pub const fn new(relationship: RelationshipName, table: TableName, join_type: JoinType, origin: PlanColumn, target: PlanColumn) -> Self
```

```rust
pub const fn origin(&self) -> &PlanColumn
```

```rust
pub const fn relationship(&self) -> &RelationshipName
```

```rust
pub const fn table(&self) -> &TableName
```

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
pub const fn new(source: SourceName, metric: MetricName, table: TableName, joins: Vec<PlanJoin>, bucket: PlanBucket, keys: Vec<PlanKey>, measure: PlanMeasure, measure_label: String, filters: Vec<PlanFilter>, params: Vec<ParamValue>, range: TimeRange) -> Self
```

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
pub const fn table(&self) -> &TableName
```

#### Implements

`Clone`, `Debug`, `Eq`, `PartialEq`, `Serialize`

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

The value is a `String` here and a bind parameter by the time it reaches a statement. It is
checked against the metric's allowlist first, so the parameterisation is the second line of
defence rather than the only one.

#### Methods

```rust
pub const fn dimension(&self) -> &DimensionName
```

```rust
pub const fn new(dimension: DimensionName, value: String) -> Self
```

```rust
pub fn value(&self) -> &str
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
- `ResultTooLarge` - The result would carry more rows than `plan::MAX_ROWS`.
- `TimeRangeTooLong` - A span of history longer than `MAX_RANGE_DAYS`.
- `PlanSpansTwoSources` - The plan would need to read from more than one data system.
- `SourceUnavailable` - The plan named a data system this process did not open.

#### Implements

`Clone`, `Debug`, `Eq`, `PartialEq`, `Serialize`

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

## Module `warehouse`

The execution port: the statement that goes out, and the rows that come back.

The trait is named `Warehouse`, which is the port's name and says nothing about what sits behind
it. A file read by an in-process engine and a cluster with a login are both implementations.

This is the one module in the domain that names SQL, and the distinction is worth being precise
about. `crate::query` is the *tool* surface, where SQL must be unrepresentable because the text
would come from a caller. Here the text is something sutura generated a moment ago from a pinned
definition, and the port has to hand it to something. What the port does *not* accept is a
statement with values pasted into it: `GeneratedQuery` keeps them apart, so an adapter cannot
receive a query whose parameters have already been flattened into the text.

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

### `struct GeneratedQuery`

```rust
pub struct GeneratedQuery
```

A statement, its parameters, and the one data system it runs against.

**Parameters are a separate field and there is no constructor that merges them.** That is the
mechanism behind "no value from a question reaches the statement as text": to inline a value an
adapter would have to build the string itself, which is a diff rather than an oversight.

`source` rides along because a plan resolves to exactly one data system, and carrying it here is
what lets the composition root check that the adapter it is about to call is the one the plan
named.

#### Methods

```rust
pub const fn new(source: SourceName, sql: String, params: Vec<ParamValue>) -> Self
```

```rust
pub fn params(&self) -> &[ParamValue]
```

```rust
pub const fn source(&self) -> &SourceName
```

```rust
pub fn sql(&self) -> &str
```

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
