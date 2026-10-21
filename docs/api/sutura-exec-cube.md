<!-- GENERATED FILE - do not edit.
     Written by docs/.tools/rustdoc_to_markdown.py from rustdoc JSON.
     Edit the doc comments in the crate source instead, then regenerate.
     The commands are on the API reference landing page. -->

# sutura-exec-cube

The public API of `sutura-exec-cube`, rendered from rustdoc JSON.

A `Warehouse` over a Cube metrics serving layer's REST API - `github.com/telekom/sutura#1340`.

sutura plans the question from the catalog definitions, with every plan-time refusal it makes for
any other source. This adapter then asks Cube for the planned members as a JSON query on
`/cubejs-api/v1/load`, and Cube computes each metric from its own model. sutura never computes a
Cube metric itself: Cube down, or Cube refusing, is a typed error and never a partial answer.

# How a plan names Cube members

A metric is `<table>.<metric>` and a column `<table>.<column>`, where `<table>` is the model's
table in the catalog: the catalog names each table after its cube and each metric after its Cube
measure. Every filter of the plan - the range, each required filter and each requested one - is
a Cube filter whose values are JSON values, never query text. `query` holds the mapping.

Before a query is sent, every member it names is looked up in Cube's `/cubejs-api/v1/meta`
answer, read with the same credential: a member Cube does not serve refuses the question.

# Identity

A `shared-service-user` source sends the deployment's declared Cube token on every request. An
`impersonation-at-source` source sends each question's own token - the one the credential broker
exchanged the caller's for - and Cube's own check of that token decides what the caller may read.
Each token is a `Secret`: no error of this crate carries it, or Cube's own text.

`Secret`: sutura_domain::identity::Secret

## `trait CubeTransport`

```rust
pub trait CubeTransport
```

The two round trips this adapter makes, as a seam: `http::HttpCube` for a running Cube, and a
scripted implementor for the unit cells.

## `enum CubeError`

```rust
pub enum CubeError<E>
```

Why Cube did not answer a plan.

### Variants

- `Endpoint`
- `NotExpressible`
- `AbsentMember`
- `LegWithoutCombiner`
- `Undeliverable`
- `PresentedDisagreesWithPosture`
- `ZeroDenominator`
- `NotFinite`
- `NotANumber`
- `InexactInteger`
- `MalformedAnswer`
- `Shape`

### Implements

`Debug`, `Display`, `Error`

## `struct CubeWarehouse`

```rust
pub struct CubeWarehouse<T>
```

One Cube deployment, as one source.

### Methods

```rust
pub const fn of(source: SourceName, posture: SourcePosture, token: Secret, transport: T) -> Self
```

A source over `transport`. `token` is the deployment's declared Cube token: every request of a
shared source and every boot anchor check sends it.

### Implements

`Debug`, `Warehouse`

## `type_alias Answer`

One answer's bytes, or why the transport did not get them.

## Module `fixture`

The fixture tier's deployment token, for the golden matrix in `sutura-app`'s tests.

Behind the default-off `fixtures` feature, like `sutura_exec_clickhouse::fixture`: nothing here
belongs in a composition root.

### `enum UnsignedToken`

```rust
pub enum UnsignedToken
```

Why the tier's token was not signed.

#### Variants

- `Clock`
- `Sign`

#### Implements

`Debug`, `Display`, `Error`

### `fn tier_token`

```rust
pub fn tier_token() -> Result<sutura_domain::identity::Secret, UnsignedToken>
```

An HS256 token over the tier's secret, valid for one hour - the deployment token Cube's default
`check_auth` verifies.

# Errors

`UnsignedToken` when the clock reads before 1970 or the token does not sign.

## Module `http`

The real `CubeTransport`: `GET /cubejs-api/v1/meta` and `POST /cubejs-api/v1/load` against a
running Cube.

Each request has `Authorization: Bearer <token>`. `Endpoint::parse` accepts `https://` for any
host and `http://` only for an IP loopback literal, so a token never travels in plain text to a
host that is not this one. Redirects are not followed. `ReadBounds` caps each answer's size;
the plan's deadline bounds each request's time.

Cube answers a query it is still computing with `{"error":"Continue wait"}`, and the client asks
again. This transport asks again until the deadline runs out.

### `enum HttpCubeError`

```rust
pub enum HttpCubeError
```

Why a request to Cube did not produce an answer.

No variant carries the token or Cube's own text: `HttpCubeError::Refused` keeps only the
status, because Cube's error text can repeat what the request sent.

#### Variants

- `Unreachable`
- `Unreadable`
- `Refused`
- `TooLarge`
- `DeadlineSpent`
- `Unencodable`

#### Implements

`Debug`, `Display`, `Error`

### `struct HttpCube`

```rust
pub struct HttpCube
```

A Cube deployment's REST API, reached over HTTP.

#### Methods

```rust
pub fn new(endpoint: Endpoint, bounds: ReadBounds, anchors: Option<sutura_tls::LoadedAnchors>) -> Self
```

A transport over a fixed agent: `anchors` `None` keeps `ureq`'s compiled-in roots, `Some`
replaces them with exactly the declared certificates.

```rust
pub const fn rotating(endpoint: Endpoint, bounds: ReadBounds, agent: sutura_tls::Rotating<ureq::Agent>) -> Self
```

A transport over the rotating agent a composition root built.

#### Implements

`Clone`, `CubeTransport`, `Debug`

### `use Endpoint`

### `use InvalidEndpoint`

### `use InvalidReadBounds`

### `use ReadBounds`

### `constant META_PATH`

The metadata API's path under a Cube server's root.

### `constant LOAD_PATH`

The query API's path under a Cube server's root.

## Module `query`

A `QueryPlan` as a Cube JSON query: which members it names, and what each filter becomes.

| In the plan                    | In the Cube query                                           |
| ------------------------------ | ----------------------------------------------------------- |
| a metric                       | the measure `<table>.<metric>`                              |
| a key                          | the dimension `<table>.<column>`                            |
| the bucket                     | a time dimension with the grain as its `granularity`        |
| `>=` / `<` a date              | `afterOrOnDate` / `beforeDate`                              |
| `=` / `IN`                     | `equals` with one or more values                            |
| `<>` / `NOT IN`                | `notEquals`, and `set` - Cube's `notEquals` keeps a `NULL`  |
| `IS TRUE` / `IS NOT NULL`      | `equals` `true` / `set`                                     |
| `top`                          | `order` and `limit`                                         |

Every value is a JSON string in `values`, never query text. A required filter of the metric is
sent like every other filter, so Cube's own measure filter is never the only one. A plan of
several metrics carries a required filter as one metric's guard, which one Cube query cannot
scope to that metric alone, so `CubeQuery` splits such a plan rather than drop the filter.

### `enum NotExpressible`

```rust
pub enum NotExpressible
```

Why a plan cannot be asked of Cube as one query.

#### Variants

- `UnboundParameter`
- `TopAcrossMetricQueries`
- `TruncatedKey`

#### Implements

`Clone`, `Debug`, `Display`, `Eq`, `Error`, `PartialEq`

### `struct CubeQuery`

```rust
pub struct CubeQuery
```

A plan as Cube queries, and the columns their answers come back as.

One query when no metric carries a guard. Otherwise the plan's own SQL would fold each guard
into that metric's aggregate, so the plan becomes one query for the groups - the dimensions and
the bucket under the shared filters, no measure - and one query per metric under its own guard
and the shared filters. `crate::rows` joins them back on the groups.

#### Methods

```rust
pub fn members(&self) -> &[String]
```

Every member the queries name, each once.

```rust
pub fn of(plan: &QueryPlan, limit: u32) -> Result<Self, NotExpressible>
```

The queries for `plan`, each asking for at most `limit` rows unless the plan's `top` sets fewer.

# Errors

The plan is one `NotExpressible` names.

```rust
pub fn queries(&self) -> &[serde_json::Value]
```

The queries, each as `/cubejs-api/v1/load` takes it under `query`. With a guard, the first
asks for the groups and each other one for one metric, in the plan's order.

#### Implements

`Clone`, `Debug`, `Eq`, `PartialEq`
