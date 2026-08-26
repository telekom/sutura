<!-- GENERATED FILE - do not edit.
     Written by docs/.tools/rustdoc_to_markdown.py from rustdoc JSON.
     Edit the doc comments in the crate source instead, then regenerate.
     The commands are on the API reference landing page. -->

# sutura-config

The public API of `sutura-config`, rendered from rustdoc JSON.

The service's configuration: layered sources in, one typed tree out, and a refusal instead of
a permissive default.

# What a caller does

Read the environment, then load. Two steps rather than one, because the environment decides
which file is layered and which defaults apply, so it has to be known first.

```
use sutura_config::{Environment, Settings, Sources};

// A test or an example supplies the environment directly; the binary reads it from the
// process with `sutura_config::environment_from_process`.
let settings = Settings::load(&Sources::defaults(Environment::Development))?;
assert!(settings.server().bind().is_loopback());
# Ok::<(), sutura_config::SettingsError>(())
```

# Precedence

Later beats earlier:

1. the defaults embedded in this crate (`Settings` can always be built from them alone);
2. `<dir>/base.yaml`, if a directory was given and the file is there;
3. `<dir>/<environment>.yaml`, likewise;
4. environment variables - `SUTURA__SERVER__PORT` sets `server.port`.

The environment itself is chosen by `SUTURA_ENVIRONMENT` and by nothing else. It is
deliberately *not* a configuration key: it selects which file is layered, so a file that could
change it would be self-referential. Both `environment:` in a file and `SUTURA__ENVIRONMENT` in
the shell are therefore unknown-field errors rather than settings that quietly do nothing.

Every layer is checked with `deny_unknown_fields`, at every depth. A misspelled key is an error
naming the key, not an override that silently did not happen.

# There is no per-caller identity, and this crate says so out loud

sutura has no request context, no credential broker and no way for a caller's identity to reach
the query path. `AGENTS.md` records "every query runs as the calling principal" as an
aspiration that is **not mechanised**, and `examples/multi-player/README.md` explains why
single-player makes it trivially true and worth nothing.

That is a property of the runtime, so it is a property of every deployment this crate
configures. An [`AccessToken`](security::AccessToken) authenticates *the deployment*: a caller
who presents it proves they hold a secret an operator configured, and nothing more. It does not
say which caller, it cannot be scoped to a subset of the catalog, it does not reach the data
system, and every query still runs with whatever access the process already had.
[`SecuritySettings::describes_identity`](security::SecuritySettings::describes_identity) is the
function that answers this, it always answers `false`, and the startup log prints that answer
on every boot so an operator cannot deploy this believing otherwise.

Rate limiting is not authentication either - see `limits` for what it does and does not buy.

# What refuses to start

`NotFitToServe` is the whole list, and each variant is a refusal rather than a warning. A
warning is read by whoever is looking at the log in the format the collector was configured
for; a process that does not start is read by everybody.

- A bind address other hosts can reach, without `security.expose_beyond_loopback: true`. In
  *every* environment, including a laptop.
- No `security.access_token`, in production or on a non-loopback bind.
- `rate_limit.enabled: false` in production.
- `server.port: 0` in production.

The checks read the *loaded* values, not any one file, because the variable layer is applied
last: a check against `production.yaml` would be checking something the process is not running
on.

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## Module `api`

Whether the generated documentation is served, and why the default differs by environment.

The `OpenAPI` document and the browser UI over it are generated from the handlers, so they are
always *correct*; the question is whether an unauthenticated caller should be handed a map of
the surface. On a laptop the answer is obviously yes - it is how the surface is explored at
all. In production it is a decision, and the safer default is the one that has to be turned
on rather than the one that has to be remembered.

It is a default and not a refusal. Serving an interface description is a legitimate choice for
a deployment behind a gateway that already authenticates, and refusing to start over it would
be this crate overruling an operator on something that leaks no data. What it does instead is
log the decision, at a level that shows up.

### `struct ApiSettings`

```rust
pub struct ApiSettings
```

Whether the generated documentation surface is served.

#### Methods

```rust
pub const fn docs_default_for(environment: crate::Environment) -> bool
```

The default for an environment: everywhere but production.

A total match rather than a comparison, so a fourth environment has to state its own
answer instead of inheriting whichever branch it happens to fall into.

```rust
pub const fn docs_enabled(self) -> bool
```

```rust
pub const fn docs_were_explicit(self) -> bool
```

```rust
pub const fn new(docs_enabled: bool, docs_were_explicit: bool) -> Self
```

#### Implements

`Clone`, `Copy`, `Debug`, `Eq`, `PartialEq`

## Module `catalog`

Which catalog the service serves, and where the files it reads live.

Two directories rather than one, because they are owned by different people. The catalog is
authored and reviewed - it is the definitions somebody certified - and the data directory is
wherever the files the engine reads happen to be mounted. Conflating them would make a
deployment that moved its data look like a catalog change, which is the one thing a pinned
bundle exists to make visible.

### `struct CatalogSettings`

```rust
pub struct CatalogSettings
```

Where the definitions and the data are, and what the resulting bundle is called.

#### Methods

```rust
pub fn data_dir(&self) -> &Path
```

```rust
pub fn dir(&self) -> &Path
```

```rust
pub fn parse(dir: PathBuf, data_dir: PathBuf, version: DefinitionVersion) -> Result<Self, InvalidCatalogSettings>
```

Reads the two directories and the version label.

The version arrives already parsed, because what identifies a snapshot of a directory is
a commit id or a build number and only the caller has it. Existence of the directories is
deliberately *not* checked here: this type is the configuration, and a directory that
disappears between reading the configuration and loading the catalog would make an
existence check here a claim that goes stale immediately. The load is what fails.

```rust
pub const fn version(&self) -> &DefinitionVersion
```

#### Implements

`Clone`, `Debug`, `Eq`, `PartialEq`

### `enum InvalidCatalogSettings`

```rust
pub enum InvalidCatalogSettings
```

Why a catalog configuration is not usable.

#### Variants

- `EmptyPath` - A path was empty, which resolves to the process working directory - a different directory on every host, and never the one the operator meant.

#### Implements

`Clone`, `Debug`, `Display`, `Eq`, `Error`, `PartialEq`

## Module `environment`

Which deployment this process believes it is.

One value, read before anything else, that decides three things which must not be decided
separately: which configuration file is layered on top of the defaults, whether the log is
machine-readable or human-readable, and how strict the startup refusals are. Separate
switches for those would let a deployment be production for logging and development for
safety, which is exactly the combination nobody would choose on purpose.

Deliberately three values and not a free string. A typo in an environment name is otherwise
the most expensive kind of configuration bug there is: it silently selects the permissive
branch of every decision above, and the log line saying so is in the format nobody is
collecting.

### `enum Environment`

```rust
pub enum Environment
```

A deployment kind.

Ordered from most permissive to least, which is also the order the refusals in
`crate::Settings` tighten in.

#### Variants

- `Development` - A developer's machine. Human-readable logs, loopback only, no token required.
- `Test` - An automated test. Same posture as development, and named separately so a test can assert on it without pretending to be a laptop.
- `Production` - A deployment serving somebody. Machine-readable logs, and every refusal in `crate::Settings::parse` applies.

#### Methods

```rust
pub const fn as_str(self) -> &'static str
```

The canonical spelling, which is also the file stem this environment layers.

```rust
pub const fn is_production(self) -> bool
```

Is this the environment the strict refusals apply to?

A method rather than `== Environment::Production` at each call site: there are five
refusals keyed off it, and a fourth variant added later has to answer this question once.

```rust
pub fn parse(raw: impl AsRef<str>) -> Result<Self, UnknownEnvironment>
```

Reads an environment name.

Case-insensitive and trimmed, because this arrives from a shell variable and
`SUTURA_ENVIRONMENT=Production ` with a trailing space is not a different deployment.
Nothing else is forgiven: `prod` is not accepted, because an abbreviation somebody has to
guess is a spelling this type exists to remove.

#### Implements

`Clone`, `Copy`, `Debug`, `Deserialize<'de>`, `Display`, `Eq`, `Hash`, `Ord`, `PartialEq`, `PartialOrd`, `Serialize`

### `struct UnknownEnvironment`

```rust
pub struct UnknownEnvironment
```

The string was not one of the three.

Carries what it found, because the whole point of the type is that a typo fails loudly, and
a failure that does not quote the typo makes the operator guess which of the three spellings
they got wrong.

#### Methods

```rust
pub fn found(&self) -> &str
```

#### Implements

`Clone`, `Debug`, `Display`, `Eq`, `Error`, `PartialEq`

## Module `limits`

How many requests a caller gets, and the two tiers that answer differently.

**Rate limiting is not authentication.** It bounds how fast something can be done, not who
may do it, and on a surface with no per-caller identity - see `crate::security` - the key
it counts against is a network address rather than a principal. A shared egress address is
therefore one bucket for everybody behind it, and a caller with many addresses has many
buckets. Both of those are properties of the mechanism, not bugs in the configuration, and
neither is repaired by tightening the numbers.

What it does buy is real: it turns an unbounded loop against a data system into a bounded
one, and a question here is an aggregate over up to ten years of history, so the cost of one
request is not small.

Two tiers, because the two surfaces have different shapes. The *probe* tier covers what a
caller may poll - liveness, and the generated `OpenAPI` document - and is tight, because nothing
there changes between two requests. The *api* tier covers the versioned API, where a legitimate
caller asks several questions in a row.

The names avoid the word that would be natural for the first tier, and not for a style reason:
`cargo xtask check-boundaries` flags a field whose name *begins* with `pub` as a public field,
because its scan is line-oriented and tests `starts_with("pub")`. A field called `public` is
therefore a gate failure on correct code. Renaming here is the cheap side of that trade, and the
names are more precise anyway - the first tier is not the unauthenticated one, since the
interface description sits behind the access token when one is configured.

### `struct Quota`

```rust
pub struct Quota
```

A sustained rate and the burst allowed above it.

Both non-zero: a quota of zero requests per second is a closed door, which is what
`enabled: false` says properly, and a zero burst is a limiter that rejects the first request
of every idle period.

#### Methods

```rust
pub const fn burst(self) -> NonZeroU32
```

```rust
pub const fn parse(name: &'static str, per_second: u32, burst: u32) -> Result<Self, InvalidQuota>
```

Reads a tier.

`name` is the configuration key this tier came from, so the error says which of the two
tiers is wrong rather than that one of them is.

```rust
pub const fn per_second(self) -> NonZeroU32
```

#### Implements

`Clone`, `Copy`, `Debug`, `Eq`, `PartialEq`

### `enum InvalidQuota`

```rust
pub enum InvalidQuota
```

Why a pair of numbers is not a quota.

#### Variants

- `Zero` - One of the two was zero.
- `BurstBelowRate` - The burst is below the sustained rate, which is a limiter that cannot sustain its own rate: the bucket refills faster than it can hold.

#### Implements

`Clone`, `Debug`, `Display`, `Eq`, `Error`, `PartialEq`

### `struct RateLimitSettings`

```rust
pub struct RateLimitSettings
```

Both tiers, and the switch.

The switch is separate from the numbers on purpose. A deployment that turns limiting off is
making a decision, and it should be one word in a file rather than a quota set so high it
never fires - which reads as a configured limit and is not one.

#### Methods

```rust
pub const fn api(self) -> Quota
```

The tier for the versioned API.

```rust
pub const fn enabled(self) -> bool
```

```rust
pub const fn new(enabled: bool, probe: Quota, api: Quota) -> Self
```

```rust
pub const fn probe(self) -> Quota
```

The tier for what an unauthenticated caller can reach.

#### Implements

`Clone`, `Copy`, `Debug`, `Eq`, `PartialEq`

## Module `security`

The access control this service has, and the honest name for what it is not.

**There is no per-caller identity in sutura today, and nothing here invents one.** No request
context reaches the query path, no credential is minted per request, and the `CredentialBroker`
port that would do it is deliberately absent because a port arrives with its adapter.
`examples/multi-player/README.md` is where that gap is written down for a reader.

So what an `AccessToken` does is narrower than authentication, and the narrowness is the
point of this module documentation: a presented token proves the caller holds a secret the
deployment was configured with. It proves nothing about *which* caller, it cannot be scoped,
it cannot be revoked for one party without revoking it for all of them, and it does not reach
the data system - every query still runs with whatever access the process already had.

It is worth having anyway, because the alternative on a non-loopback interface is an
unauthenticated way to read whatever the process can read. It is not worth mistaking for
identity, which is why `SecuritySettings::describes_identity` exists as an associated function
that always answers the same thing: the startup log prints it, so an operator cannot deploy this
believing otherwise.

### `struct AccessToken`

```rust
pub struct AccessToken
```

A pre-shared secret a caller presents to reach the service.

Held as a `Secret`, so the whole settings tree can be written to the startup log with
`Debug` and the token cannot come out with it.

**Not comparable with `==`, and that is inherited rather than reimplemented.** `Secret`
implements no `PartialEq` on purpose: a derived comparison on credential material returns on
the first differing byte, which is a timing oracle at whatever call site adds it. The
comparison lives here instead, once, as `AccessToken::matches_in_constant_time`.

#### Methods

```rust
pub fn matches_in_constant_time(&self, presented: &str) -> bool
```

Does `presented` equal the configured token?

Named for the property rather than for the operation, because the property is the only
reason this function exists rather than a `==`.

**Both sides are hashed first, and that is not ceremony.** `subtle` compares equal-length
byte slices without branching, which removes the early-return oracle - but comparing the
raw strings would still have to decide what to do about differing lengths, and every
answer to that leaks the length before it leaks anything else. Reducing both sides to a
fixed 32 bytes removes the question: every comparison is over the same number of bytes
whatever arrived.

What this still does not do: it is not a password hash. There is no salt and no work
factor, because the input is a high-entropy secret an operator generated rather than
something a person chose, and nothing here is stored for an attacker to find offline.

```rust
pub fn parse(raw: impl AsRef<str>) -> Result<Self, InvalidAccessToken>
```

Reads a configured token.

#### Implements

`Clone`, `Debug`

### `enum InvalidAccessToken`

```rust
pub enum InvalidAccessToken
```

Why a string is not usable as an access token.

#### Variants

- `TooShort` - Shorter than `AccessToken::MIN_LENGTH`.
- `Untrimmed` - Whitespace at either end, which is almost always a copy-paste artefact and would otherwise make every request fail for a reason nobody can see in a log.

#### Implements

`Clone`, `Debug`, `Display`, `Eq`, `Error`, `PartialEq`

### `struct SecuritySettings`

```rust
pub struct SecuritySettings
```

The access posture, and the acknowledgement that goes with a non-loopback bind.

Two fields rather than one, because they answer different questions and collapsing them was
the tempting mistake: a token says *who may reach this*, and the acknowledgement says *the
operator meant to publish it*. A deployment that sets a token but binds the wildcard by
accident has answered only the first.

#### Methods

```rust
pub const fn access_token(&self) -> Option<&AccessToken>
```

The configured token, if there is one.

```rust
pub const fn describes_identity() -> bool
```

Does anything here establish who the caller is?

Always `false`, and it is a function rather than a comment so the startup log and the
documentation read the same value. When a `CredentialBroker` and a request context exist,
this stops being a constant and the log line changes with it; until then a deployment is
told, on every boot, that the token authenticates the deployment and not the caller.

```rust
pub const fn expose_beyond_loopback(&self) -> bool
```

Did the operator explicitly say they meant to listen off-host?

```rust
pub const fn new(access_token: Option<AccessToken>, expose_beyond_loopback: bool) -> Self
```

```rust
pub const fn token_state(&self) -> &'static str
```

Whether a token is configured, as a word for the startup log.

A method and not a `Debug` of the option, so the log line cannot become the token by
somebody changing the field type later.

#### Implements

`Clone`, `Debug`, `Default`

## Module `server`

Where the service listens, and the two bounds every request is held to.

The interesting type here is `BindAddress`, and it is interesting for a security reason
rather than an ergonomic one. This service has no per-caller identity - see the module
documentation on `crate` - so the interface it listens on is the whole of its perimeter.
Making the bind an IP address rather than a string is what lets
`BindAddress::is_loopback` be a fact the startup refusals can read, instead of a substring
test somebody has to remember to write.

### `struct BindAddress`

```rust
pub struct BindAddress
```

The socket the service listens on.

An `IpAddr` and a port, never a hostname. A hostname is refused rather than resolved: a name
resolves to whatever the resolver says today, which may be a public interface tomorrow, and
a perimeter that moves when DNS moves is not a perimeter. The operator writes the interface
they mean.

#### Methods

```rust
pub const fn is_loopback(self) -> bool
```

Is this address reachable only from this host?

The question the production refusals are keyed on. The two wildcard addresses answer
`false`, which is the answer that matters: the wildcard is the bind that reaches every
interface, and it is the one an operator reaches for without meaning to publish anything.

```rust
pub fn parse(host: impl AsRef<str>, port: u16) -> Result<Self, InvalidBindAddress>
```

Reads a host and port.

```rust
pub const fn port(self) -> u16
```

The port, so a caller can report the one it actually got.

```rust
pub const fn socket(self) -> SocketAddr
```

The address, for whatever binds the socket.

#### Implements

`Clone`, `Copy`, `Debug`, `Display`, `Eq`, `PartialEq`

### `enum InvalidBindAddress`

```rust
pub enum InvalidBindAddress
```

Why a host and port are not an address to listen on.

#### Variants

- `NotAnIpAddress` - The host was not an `IPv4` or `IPv6` literal.

#### Implements

`Clone`, `Debug`, `Display`, `Eq`, `Error`, `PartialEq`

### `struct RequestTimeout`

```rust
pub struct RequestTimeout
```

How long one request may take before the service gives up on it.

Bounded at both ends. Zero is a service that answers nothing, and an hour is a connection
held open long enough that a handful of them are the outage: a question here is one
aggregate over a bounded range, so a minute is already generous and five is the ceiling.

#### Methods

```rust
pub const fn duration(self) -> Duration
```

```rust
pub const fn parse(seconds: u64) -> Result<Self, InvalidBound>
```

Reads a timeout in whole seconds.

Seconds and not a duration string: sub-second precision is meaningless for a bound this
coarse, and a parser for a suffixed number is a second grammar for a single value.

```rust
pub const fn seconds(self) -> u64
```

#### Implements

`Clone`, `Copy`, `Debug`, `Eq`, `PartialEq`

### `struct BodyLimit`

```rust
pub struct BodyLimit
```

The largest request body the service will read.

A modelled question is a metric name, a grain, two dates and at most four dimensions, which
is a few hundred bytes. The bound exists because a body limit is the cheapest availability
control there is, and because the default in most stacks is whatever arrives.

#### Methods

```rust
pub const fn bytes(self) -> usize
```

```rust
pub const fn parse(bytes: usize) -> Result<Self, InvalidBound>
```

Reads a body limit in bytes.

#### Implements

`Clone`, `Copy`, `Debug`, `Eq`, `PartialEq`

### `enum InvalidBound`

```rust
pub enum InvalidBound
```

Why a bound is not a bound.

#### Variants

- `Zero` - Nothing here may be zero: a zero timeout answers nothing and a zero body limit accepts nothing, and both read as no limit at all to somebody writing the file.
- `TooLarge` - Above the ceiling this type declares.

#### Implements

`Clone`, `Debug`, `Display`, `Eq`, `Error`, `PartialEq`

### `struct ServerSettings`

```rust
pub struct ServerSettings
```

Everything about the socket and the two per-request bounds.

#### Methods

```rust
pub const fn bind(&self) -> BindAddress
```

```rust
pub const fn max_body(&self) -> BodyLimit
```

```rust
pub const fn new(bind: BindAddress, request_timeout: RequestTimeout, max_body: BodyLimit) -> Self
```

Assembles the group from parts that have each already been parsed.

Infallible, and that is the shape the newtypes buy: there is no cross-field rule inside
this group, so once every part exists the group exists. The cross-field rules - the ones
that pair a bind address with an environment and a token - live in
`crate::Settings::parse`, because they need the other groups to decide.

```rust
pub const fn request_timeout(&self) -> RequestTimeout
```

#### Implements

`Clone`, `Copy`, `Debug`, `Eq`, `PartialEq`

## Module `telemetry`

What the log looks like, and who is meant to read it.

One decision with two right answers, which is why it is a type rather than a flag. In
production a log line is read by a collector: it has to be one JSON object per line, with the
span context attached, so a query can find every line belonging to one request. On a laptop
the same line is read by a person compiling every thirty seconds, and a JSON object per line
is unreadable there.

The default therefore follows `crate::Environment` and nothing else, so the two cannot be
set inconsistently by omission. An explicit value overrides it - a developer debugging what
the collector will actually receive needs that - and the startup log says which of the two
happened.

### `enum LogFormat`

```rust
pub enum LogFormat
```

How a log line is rendered.

#### Variants

- `Bunyan` - One JSON object per line, in the bunyan schema. What a collector ingests.
- `Pretty` - Indented, coloured, and meant for a terminal.

#### Methods

```rust
pub const fn as_str(self) -> &'static str
```

```rust
pub const fn default_for(environment: Environment) -> Self
```

The format an environment gets when the configuration does not say.

**This is the split, in one function.** Production is machine-readable and everything else
is human-readable, and it is expressed as a total match rather than an `if` so a fourth
environment cannot inherit an answer nobody chose for it.

```rust
pub fn parse(raw: impl AsRef<str>) -> Result<Self, UnknownLogFormat>
```

Reads a format name.

#### Implements

`Clone`, `Copy`, `Debug`, `Deserialize<'de>`, `Display`, `Eq`, `PartialEq`, `Serialize`

### `struct UnknownLogFormat`

```rust
pub struct UnknownLogFormat
```

The string was neither format.

#### Implements

`Clone`, `Debug`, `Display`, `Eq`, `Error`, `PartialEq`

### `struct LogFilter`

```rust
pub struct LogFilter
```

A tracing filter directive, as written for `RUST_LOG`.

Kept as text here and turned into a real filter by whatever installs the subscriber, because
the type that parses one lives in `tracing-subscriber` and this crate holds no framework. What
*is* checked here is that it is not empty and not a smuggled second line: an empty filter
silently means "no directives", which is a service that logs at the default level while its
configuration says otherwise.

#### Methods

```rust
pub fn as_str(&self) -> &str
```

```rust
pub fn parse(raw: impl AsRef<str>) -> Result<Self, InvalidLogFilter>
```

Reads a filter directive.

#### Implements

`Clone`, `Debug`, `Display`, `Eq`, `PartialEq`

### `enum InvalidLogFilter`

```rust
pub enum InvalidLogFilter
```

Why a string is not a filter directive.

#### Variants

- `Empty` - Empty, or only whitespace.
- `ControlCharacter` - A control character, which in practice is a newline pasted in with the value. A newline in a filter is a log line an attacker can forge if the value ever reaches the log itself.

#### Implements

`Clone`, `Debug`, `Display`, `Eq`, `Error`, `PartialEq`

### `struct ServiceName`

```rust
pub struct ServiceName
```

The name every log line is attributed to.

A separate type because it is the field a collector groups by, so an empty or whitespace one
makes every line from this deployment unattributable.

#### Methods

```rust
pub fn as_str(&self) -> &str
```

```rust
pub fn parse(raw: impl AsRef<str>) -> Result<Self, InvalidServiceName>
```

Reads a service name.

Narrow on purpose: a name with a space or a quote in it has to be escaped by every
consumer, and the one that forgets produces a log line that does not parse.

#### Implements

`Clone`, `Debug`, `Display`, `Eq`, `PartialEq`

### `enum InvalidServiceName`

```rust
pub enum InvalidServiceName
```

Why a string is not a service name.

#### Variants

- `Empty`
- `NotAnIdentifier`

#### Implements

`Clone`, `Debug`, `Display`, `Eq`, `Error`, `PartialEq`

### `struct TelemetrySettings`

```rust
pub struct TelemetrySettings
```

Everything about the log.

#### Methods

```rust
pub const fn filter(&self) -> &LogFilter
```

```rust
pub const fn format(&self) -> LogFormat
```

```rust
pub const fn format_was_explicit(&self) -> bool
```

```rust
pub const fn new(service_name: ServiceName, filter: LogFilter, format: LogFormat, format_was_explicit: bool) -> Self
```

```rust
pub const fn service_name(&self) -> &ServiceName
```

#### Implements

`Clone`, `Debug`, `Eq`, `PartialEq`
