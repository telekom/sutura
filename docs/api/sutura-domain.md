<!-- GENERATED FILE - do not edit.
     Written by docs/.tools/rustdoc_to_markdown.py from rustdoc JSON.
     Edit the doc comments in the crate source instead, then regenerate.
     The commands are on the API reference landing page. -->

# sutura-domain

The public API of `sutura-domain`, rendered from rustdoc JSON.

The hexagon's interior: the types the business rules are written in, and - when the first
adapter needs one - the port traits it names its dependencies by.

Nothing here may depend on a framework: no async runtime, no web server, no query engine.
`cargo xtask check-boundaries` enforces it over the whole transitive tree, because the rule
is worth more as a check than as a sentence in a design document.

**There are no port traits yet, and that is deliberate.** A port exists to invert a
dependency on something outside the hexagon, and no adapter exists to invert - the catalog,
warehouse and credential adapters are all still planned. A trait with no implementor and no
caller is a guess at a signature that only the first real adapter can settle, and in a
library crate `pub` hides it from `dead_code`, which is exactly how an unused item survives
review. The modules below are grouped by concept so that a port lands next to the types it
speaks in when it arrives, rather than in a module named after the trait.

## Module `definitions`

The identifiers of a pinned definition set.

Definitions are authored upstream and arrive as an immutable, hashed snapshot. The digest
is what makes "the same question returns the same number" checkable rather than asserted,
and what stops a catalogue edit from changing what executes - so a value that is not a
digest must not be able to occupy the slot where one is expected.

### `struct DefinitionDigest`

```rust
pub struct DefinitionDigest
```

Content hash of a pinned definition set.

Construct it with `DefinitionDigest::parse`. There is no other way in: the field is
private and `Deserialize` is routed through the same constructor, so a `DefinitionDigest`
that is not `HEX_LEN` hex characters does not exist to be passed anywhere.

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
