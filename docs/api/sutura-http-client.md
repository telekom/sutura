<!-- GENERATED FILE - do not edit.
     Written by docs/.tools/rustdoc_to_markdown.py from rustdoc JSON.
     Edit the doc comments in the crate source instead, then regenerate.
     The commands are on the API reference landing page. -->

# sutura-http-client

The public API of `sutura-http-client`, rendered from rustdoc JSON.

The bounded, TLS-rotating `ureq` client `sutura-catalog-datahub`'s and
`sutura-catalog-openmetadata`'s real HTTP readers both build on.

**Extracted, not designed up front** - `github.com/telekom/sutura#970`'s review found
`sutura-catalog-openmetadata/src/http.rs` and `sutura-catalog-datahub/src/http.rs` sharing an
`Endpoint` parser, a `ReadBounds`/`Budget` pair and a `rotating_agent` constructor byte-for-byte,
and the same shape again in each crate's own `tls_roots.rs` and `test_support.rs`.
`cargo xtask check-jscpd`'s allowlist (`devco/dup-ignore`) explicitly refuses an exemption for
anything under `crates/`, so the fix has to be structural: this crate is what
`.agents/skills/sutura/crate-map/SKILL.md` already argues `sutura-tls` is one layer down for - "a
small read or computation two same-class adapters both need... joins no existing prefix's rules
and starts in no forbidden class by construction". `sutura-catalog-*` (the "metadata providers"
class `xtask/src/boundaries/adapters.rs` names) may not reach another member of its own class,
but nothing forbids two of them reaching a THIRD, unprefixed crate - the same argument that
already lets both depend on `sutura-tls`.

# What is generic, and what stays in each reader

Everything here is protocol-agnostic ureq/TLS plumbing: it never names an entity kind, a wire
field, or a mapping. `Endpoint::parse`'s grammar, `ReadBounds`'s two settings, the shared
`Budget` a reader's own `read()` opens once, `rotating_agent`/`fixed`'s TLS wiring and the
anchor fold in `tls` are the same read for `DataHub`'s `OpenAPI` v3 surface and `OpenMetadata`'s
REST API alike. What stays in each reader crate: the entity-shaped `HttpReaderError` variants
(their `Display` text names the platform), the paged `fetch`/`entities` helpers built over
`Budget`, and every `harvest_*` mapping function - `docs/what-openmetadata-can-carry.md` and
`sutura-catalog-datahub`'s own module header are explicit that those mappings are a first-party
claim about each platform's wire shape, which this crate must never blur by generalizing over.

# TLS

`fixed` and `rotating_agent` are the two ways a reader gets an outbound `ureq::Agent`: fixed
at construction over an already-loaded anchor bundle, or rebuilt on every
`sutura_tls::POLL_INTERVAL` poll from a `security.outbound` declaration
(`github.com/telekom/sutura#125`). Both fold through `tls::config`, the one place a loaded
`sutura_tls::LoadedAnchors`/`sutura_tls::LoadedIdentity` becomes `ureq::tls::TlsConfig` -
`sutura-tls` itself may never depend on `ureq` (`xtask/src/boundaries/edges.rs`'s
`sutura-tls -> ring` forbidden edge is what that would reintroduce), so the fold lives here
instead, the same reasoning `sutura-catalog-datahub`'s own `tls_roots.rs` module header gave
before this crate existed.

## `use OutboundAgent`

A reader's rotating agent handle and (when a declaration exists) the poll handle that keeps it
current - named because the spelled-out pair is over this workspace's `type_complexity`
threshold.

## `use fixed`

A fixed agent over an already-loaded anchor bundle, presenting no client identity.

`anchors` is `None` for `ureq`'s compiled-in roots - the constructor-time half of a reader's TLS
posture; `rotating_agent` is the one that also presents an identity, over a
`security.outbound` declaration a composition root polls.

## `use rotating_agent`

Builds a reader's rotating agent handle for a declared `security.outbound` set.

Also returns, when one is declared, the `sutura_tls::Rotator` the composition root drives on
`sutura_tls::POLL_INTERVAL`. `None` (no declaration) returns a fixed handle over `ureq`'s
compiled-in roots, presenting no identity, and no poll handle. Rebuilt over
`RootCerts::Specific` from each freshly loaded bundle and, when
`sutura_tls::Declared::identity` is declared, the freshly loaded identity too - never a union,
never a second external read.

# Errors

The declared bundle or client identity cannot be loaded at boot.

## `use DEFAULT_MAX_RESPONSE_BYTES`

The recommended default response-size cap, in bytes, for a composition root's settings default.

A metadata page is descriptions, column names and a handful of documents, not query rows, so
what this defends against is something that is not the endpoint answering at all - a redirect
loop, a proxy gone wrong - rather than a realistic upper bound on a legitimate page.

## `use DEFAULT_TIMEOUT_SECONDS`

The recommended default request timeout, in seconds, for a composition root's settings default.

Matches `server.request_timeout_seconds`'s own shipped default: a metadata read that outlives
the request timeout in front of it cannot answer inside the budget the caller was promised
anyway. **Not read by anything in this crate** - a caller passes the number it resolved, through
`ReadBounds::parse`, the same single-owner shape `BytesBilledCeiling::parse` holds for
`BigQuery`'s ceiling: this crate owns the range, a settings tree owns that the key was written.

## `use InvalidReadBounds`

Why a declared bound is not usable.

## `use ReadBounds`

What one reader's `read()` call may spend: a request timeout and a response-size cap.

A newtype rather than two loose arguments, so a reader cannot be built with an unchecked pair -
a request timeout paired with a response-size cap, and no money bound: a metadata read is not
billed.

## `use Budget`

One shared budget across a reader's `read()` call and its several requests.

The same shape `sutura_domain::warehouse::deadline::Deadline` holds for a job's execution, and
for the same reason: a budget opened per request lets independent timeouts sum to more than a
deployment declared - an instant opened once, read as what is left rather than re-derived.

## `use Endpoint`

A validated catalog HTTP endpoint, obtainable only through `Self::parse`.

What it accepts, exactly: `scheme://host[:port]`, scheme `http` or `https` (case-folded), on a
`Uri` (`ureq`'s own re-export of the `http` crate's parser, the SAME type `ureq` itself parses
a request URL into before dialling), an OPTIONAL nonzero valid `:port`, an OPTIONAL trailing
`/`, and NOTHING else. `https://` is accepted for any host; `http://` only for an IP loopback
LITERAL - a hostname is not an address, so `localhost` does not count either, and only something
that parses as `IpAddr` and answers `is_loopback()` does.

## `use InvalidEndpoint`

Why a declared endpoint is not usable.

## `use EndpointMessage`

The endpoint's own message on a refusal.

Bounded, filtered, and reachable only through `Self::as_str` - never through `Debug`, which
is the rendering a cause-chain walk uses, so a `Display`-flattened error chain never carries
endpoint-owned text.

## Module `test_support`

A real local HTTP server - "ports get fakes, not mocked HTTP".

`pub` behind this crate's `test` feature so a reader's own tests AND a composition root's
served-binary suite can build the same fake rather than each crate carrying a second copy.

**Moved here (issue #970's review) from `sutura-catalog-datahub::test_support`**, which issue
#202's second PR had already made `pub` behind that crate's own `http` feature for the identical
reason: `sutura-cli`'s served suite needs a real loopback server to boot a composed deployment
against, and an integration test binary cannot see another crate's `tests/` directory at all -
Rust exposes no such thing, only the library does. Once `sutura-catalog-openmetadata` needed the
SAME fake, the two copies were byte-for-byte identical wire plumbing (`cargo xtask check-jscpd`
measured it), which is what moved it one crate further out rather than leaving a second copy.

**`#[cfg(feature = "test")]`, not `#[cfg(test)]`.** A downstream crate's OWN test
compilation is what needs to see this, and `#[cfg(test)]` on an item never crosses a dependency
edge. A catalog crate folds `sutura-http-client/test` into its own `http` feature, so
this module compiles into any NON-test `--features http` build too (`sutura-cli --features
datahub`, in particular) - dead code there, never called by production composition, but real
object code in a shipped binary. **Stated rather than hidden**, the same trade
`.agents/skills/sutura/crate-map/SKILL.md`'s "why a networked adapter hides behind a default-off
feature" already accepted for `sutura-catalog-datahub::test_support`: `std::net::TcpListener`
adds no new DEPENDENCY edge to the four cross builds' `crane.buildDepsOnly` derivation, which is
what that argument is about.

### `struct Scripted`

```rust
pub struct Scripted
```

One scripted answer: a status, a body, and how long to wait before sending it.

#### Methods

```rust
pub fn body(&self) -> &[u8]
```

The body this scripted answer is served with - see `Self::status_code` for why it is public.

```rust
pub fn delayed(body: &serde_json::Value, delay: Duration) -> Self
```

```rust
pub fn ok(body: &serde_json::Value) -> Self
```

```rust
pub const fn raw(status: u16, body: Vec<u8>) -> Self
```

A response whose body is opaque bytes, as a size-cap test needs: a body that must be too big
to be legal JSON (the length check runs before decode), so it is not served through the
JSON-typed constructors above.

```rust
pub fn status(status: u16, body: &str) -> Self
```

```rust
pub const fn status_code(&self) -> u16
```

The status this scripted answer is served with - public so a TLS loopback variant of the
fake (`crate::tls_test_support`) can serve the SAME answers this server does, instead of a
second worth of page-building. Named `status_code` rather than `status` because
`Self::status` is already the constructor's name.

### `struct FakeServer`

```rust
pub struct FakeServer
```

A real local HTTP/1.1 server answering one `Scripted` response per connection, in order, then
closing. Captures each request's `authorization` header so a test can assert the bearer was sent.

#### Methods

```rust
pub const fn addr(&self) -> SocketAddr
```

The bound loopback address, for a case that builds its own (malformed) endpoint string
around it rather than using `Self::endpoint` as-is.

```rust
pub fn endpoint(&self) -> String
```

```rust
pub fn finish(self) -> CapturedAuthorizations
```

Joins the server thread and returns every request's `authorization` header, in order.

Only called by a test that knows exactly how many connections it will make - a test that
deliberately stops short drops the server instead, and the abandoned thread exits with the
process.

```rust
pub fn start(answers: Vec<Scripted>) -> Self
```

### `type_alias CapturedAuthorizations`

Every request this fake server has answered, in order: `authorization` header or `None`.

## Module `tls_test_support`

A real loopback TLS server and an `rcgen`-issued self-signed leaf.

The fixture shape every one of this workspace's outbound-TLS suites rests on, `pub` behind this
crate's `tls-test` feature.

**Moved here (issue #970's review) from `sutura-catalog-datahub`'s and
`sutura-catalog-openmetadata`'s own `tests/http_reader.rs::tls_anchors` modules**, which
`cargo xtask check-jscpd` measured as byte-for-byte identical wire plumbing once the second
reader carried its own copy.

**A dev-dependency-only feature, unlike `crate::test_support`.** Nothing outside a reader's
OWN `tests/http_reader.rs` needs this - `sutura-cli`'s served suite only dials the plaintext
fake behind `crate::test_support` - so a catalog crate takes `sutura-http-client` with this
feature in `[dev-dependencies]` only, never folded into its own `http` feature: no `rcgen`/
`rustls` object code reaches a `--features http` build that did not already carry `rustls`
transitively through `ureq`.

### `struct Issued`

```rust
pub struct Issued
```

A self-signed leaf whose SAN names the IP literal the reader dials (`127.0.0.1`).

Freshly generated per call. `security.outbound` verification is by dial, so the SAN must match
the dialed address - a DNS-name-only cert would not verify against `https://127.0.0.1:<port>`.

### `struct Scratch`

```rust
pub struct Scratch
```

A scratch directory a test owns, removed when it ends.

#### Methods

```rust
pub fn bundle(&self, name: &str, certificate: &rcgen::Certificate) -> PathBuf
```

Writes a declared PEM bundle of exactly one certificate and returns its path - what a
composition root would read via `sutura_tls::load_anchors(&Anchors::Bundle(path))`.

```rust
pub fn new(name: &str) -> Self
```

#### Implements

`Drop`

### `struct TlsFakeServer`

```rust
pub struct TlsFakeServer
```

A loopback TLS server presenting one leaf and answering the scripted pages a `read()` makes.

The SAME happy-path corpus `crate::test_support::FakeServer` serves, over TLS, so these cells
prove the same read every other test certifies completes under a declared bundle.

Bounded by the answer count AND a deadline: a refused-handshake cell (the client never sends a
request and never reconnects after the refusal) must not hang the serve thread.

#### Methods

```rust
pub fn endpoint(&self) -> String
```

```rust
pub fn start(issued: &Issued, answers: Vec<Scripted>) -> Self
```

#### Implements

`Drop`

### `fn issue`

```rust
pub fn issue() -> Issued
```

### `fn server_config`

```rust
pub fn server_config(issued: &Issued) -> std::sync::Arc<rustls::ServerConfig>
```

### `fn declared_anchors`

```rust
pub fn declared_anchors(scratch: &Scratch, name: &str, issued: &Issued) -> sutura_tls::LoadedAnchors
```

Loads a scratch-written bundle carrying `issued`'s own certificate, the way a composition root
would from a declared `security.outbound.transport_anchors` bundle path.

# Panics

The freshly written bundle fails to load - a test invariant, never a production path.
