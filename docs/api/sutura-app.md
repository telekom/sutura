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

`verify_anchors` re-executes every metric that declares a certified number and reports whether
it still produces it. `answer` takes a `Validated` bundle, which is the only thing
`sutura_domain::pinned::Validated::new` will produce from that report, so **a bundle whose
anchors were never checked cannot reach the query path.** Not by discipline: there is no other
constructor.

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
pub fn answer<W>(definitions: &sutura_domain::pinned::Validated<sutura_domain::pinned::PinnedDefinitions>, query: &sutura_domain::query::Query, warehouse: &W) -> Answered<W>
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

## `type_alias Answered`

What answering produced, or why it could not.

A named alias because the inline form is over the complexity threshold in `clippy.toml`, and
naming it is the better half of that trade: the generic parameter is a warehouse, not a result.
