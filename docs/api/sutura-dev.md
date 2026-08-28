<!-- GENERATED FILE - do not edit.
     Written by docs/.tools/rustdoc_to_markdown.py from rustdoc JSON.
     Edit the doc comments in the crate source instead, then regenerate.
     The commands are on the API reference landing page. -->

# sutura-dev

The public API of `sutura-dev`, rendered from rustdoc JSON.

What a worktree's services are called, and where they are listening.

A library rather than two modules inside a binary, and the reason is the second module: a test
harness has to be able to LEARN an endpoint, and a binary's modules are reachable from nothing.
So `discovery` is a library door - the only one - and `scope` is beside it because the two
answer halves of one question.

# The split that matters

* **Naming is derived** from the worktree path, in `scope`. It is stable, readable, and a
  collision in it fails loudly at `docker compose up`.
* **Ports are allocated**, not derived: published ephemerally so docker and the operating system
  pick them, and read back afterwards. `discovery` is what reads them back, and there is no
  constant to read instead.

A hash collision in a NAME is a startup error somebody sees. A hash collision in a PORT is a
test that passes against a neighbouring worktree's fixture. That asymmetry is why one of the two
is derived and the other is not.

**What is NOT here: any knowledge of docker.** Provisioning lives in `xtask`, which is the
repo tool and is never packaged. Docker orchestration inside a shipped artifact is test
scaffolding delivered to users; `sutura-dev` is not shipped either, but it is the crate a
harness links, and a harness has no business being able to start a container.

## Module `discovery`

The discovery file: the only way to learn where a provisioned service is listening.

Ports are allocated rather than derived - published ephemerally, so docker and the operating
system pick them and there is no window between a check and a bind for a neighbour to lose a
race in. What that costs is exactly one property: a fixed port somebody could memorise between
runs. This module is what replaces it, with a value that is correct rather than remembered.

# Why the reader has one door

A test that reads a constant port works alone, fails in parallel, and **passes review easily** -
which is why the mechanism has to be the *only* way to learn an endpoint rather than the
encouraged one. So:

* `Endpoint`'s fields are private and it has no public constructor, no `Default` and no
  `parse`. A struct literal for it does not compile, which is asserted by a `compile_fail`
  doctest with a compiling twin.
* `Endpoints::discover` is the only public function that returns an `Endpoints`. It reads the
  file this module writes, in this worktree, and there is nothing else to call.
* `publish` - what provisioning calls - returns the path it wrote and **not** an `Endpoints`,
  so even the writer has to go through the reader's door to look at what it published.

**The limit, stated with the claim:** the mint inside `publish` parses what a container runtime
reported. A caller that fabricated that text would get an endpoint it made up - but that is
lying about docker's output, which is a different and much louder thing than reading a constant,
and no test can do it by accident.

### `struct Endpoint`

```rust
pub struct Endpoint
```

Where one provisioned service is listening, on this host, right now.

There is no way to construct one except by reading the discovery file. That is the point:

```compile_fail
use sutura_dev::discovery::Endpoint;
// A constant endpoint is exactly what this type refuses to be: the fields are private, so
// there is no literal to write.
let _ = Endpoint { host: String::from("127.0.0.1"), port: 5432 };
```

The compiling twin - the one door, which fails at runtime because nothing is provisioned in a
temporary directory, rather than at compile time because the door is missing:

```
use sutura_dev::discovery::Endpoints;
use sutura_dev::scope::Scope;

let Ok(scope) = Scope::from_root(&std::env::temp_dir()) else { return };
assert!(Endpoints::discover(&scope).is_err(), "nothing is provisioned there");
```

#### Methods

```rust
pub fn host(&self) -> &str
```

Host to connect to.

```rust
pub const fn port(&self) -> u16
```

The host port docker allocated for this run.

#### Implements

`Clone`, `Debug`, `Display`, `Eq`, `PartialEq`

### `struct Endpoints`

```rust
pub struct Endpoints
```

Every endpoint one worktree's provisioning bound, plus the compose project they belong to.

#### Methods

```rust
pub fn discover(scope: &Scope) -> Result<Self, DiscoveryError>
```

Read this worktree's discovery file.

**The reader's only door.** Nothing else in this crate returns an `Endpoints`.

```rust
pub fn endpoint(&self, service: &str) -> Result<&Endpoint, DiscoveryError>
```

Where `service` is listening.

```rust
pub fn project(&self) -> &str
```

The compose project these endpoints came from.

```rust
pub fn services(&self) -> impl Iterator<Item>
```

Every service and where it is listening, in name order.

#### Implements

`Clone`, `Debug`, `Eq`, `PartialEq`

### `enum DiscoveryError`

```rust
pub enum DiscoveryError
```

Why an endpoint could not be learned, or could not be recorded.

#### Variants

- `NotProvisioned` - No discovery file. Nothing has provisioned this worktree, or teardown removed it.
- `Unreadable` - The file exists and could not be read.
- `Malformed` - The file is not the shape this module writes.
- `UnknownService` - A service nobody provisioned.
- `UnreadablePublishedAddress` - A published address the container runtime reported that this module cannot read.
- `Unwritable` - The discovery file could not be written.

#### Implements

`Debug`, `Display`, `Error`

### `enum Malformed`

```rust
pub enum Malformed
```

Which part of the discovery file is wrong. A variant rather than a sentence, because a caller
that has to match on prose has no contract.

#### Variants

- `NotJson` - Not JSON at all.
- `NoProject` - No `project` string.
- `NoServices` - No `services` object.
- `ServiceEntry` - A service entry without a readable `host` and `port`.

#### Implements

`Clone`, `Copy`, `Debug`, `Eq`, `PartialEq`

### `fn path_for`

```rust
pub fn path_for(scope: &crate::scope::Scope) -> std::path::PathBuf
```

Where this worktree's discovery file lives. Under the worktree, so a neighbour cannot read it.

### `fn publish`

```rust
pub fn publish(scope: &crate::scope::Scope, reported: &[(&str, String)]) -> Result<std::path::PathBuf, DiscoveryError>
```

Record what provisioning actually bound, and return the path written.

`reported` pairs a service name with the line a container runtime printed for its published
address - `0.0.0.0:32768`, `[::]:32768`, `127.0.0.1:32768`. Parsing it here is what keeps the
mint in one place: nothing else in this crate turns a number into an `Endpoint`.

It returns the PATH and not an `Endpoints`, deliberately. Even the writer reads its own work
back through `Endpoints::discover`, so there is exactly one door and no second shape of it.

### `fn forget`

```rust
pub fn forget(scope: &crate::scope::Scope) -> Result<(), DiscoveryError>
```

Remove this worktree's discovery file, if there is one.

Teardown's half of the contract: endpoints that no longer exist must not be readable, because a
stale file is the one way discovery could hand back a wrong answer instead of an error.

## Module `scope`

Per-worktree isolation: the part that has to be right.

Several worktrees of this repo are open at once - that is the point of stacked branches -
and each needs its own Postgres, `ClickHouse` and an identity provider. Two worktrees sharing a
container is the worst outcome available: a test passes because the *other* branch's
migration ran, and the failure appears in whichever branch is unlucky.

So everything NAMED is scoped to a worktree, and it all derives from one value: a short digest
of the worktree's CANONICAL path.

* the compose project name comes from the digest, so containers, networks and volumes are
  namespaced
* state lives under the worktree, never in a shared directory

# Naming is derived; ports are NOT

An earlier version of this module derived the published ports from the same digest, and that
design is withdrawn. Two defects, and the second is the worse one:

* **A hash into a port range cannot guarantee disjoint blocks.** It is a total function from an
  unbounded set of paths into a finite set of blocks, so collisions exist by construction. A
  corpus of sample paths can only fail to find one, which is not the same claim.
* **Check-then-bind is a race.** "Refuse if the port is already bound" leaves the whole window
  between the check and docker's bind open to anything else on the host - including the
  neighbouring worktree running the same check at the same time. It reads as a guarantee and
  delivers a probability.

Ports are therefore allocated by the thing that owns them: published ephemerally, and read back
after the container is up. See `crate::discovery`, which is the only way to learn one.

**Naming stays derived, because naming has no allocator** - and the asymmetry is the whole
reason one of the two moved and the other did not. A hash collision in a NAME is a startup
error somebody reads; a hash collision in a PORT is a test that passes against the wrong
fixture.

### `struct Service`

```rust
pub struct Service
```

A dev service that gets its own container per worktree.

No port field, derived or otherwise: what a service publishes on the host is allocated at
provision time and read back, so a port here would be a second answer to a question this type
is not allowed to answer.

#### Methods

```rust
pub const fn container_port(&self) -> u16
```

The port INSIDE the container. Provisioning publishes it ephemerally and reads back the
host port docker chose.

```rust
pub const fn name(&self) -> &'static str
```

Name used in the compose file, in the discovery file and in output.

#### Implements

`Clone`, `Copy`, `Debug`, `Eq`, `PartialEq`

### `enum ScopeError`

```rust
pub enum ScopeError
```

Why a worktree root could not become a scope.

#### Variants

- `NotResolvable` - The path could not be canonicalised - it does not exist, or a component is not readable.

#### Implements

`Debug`, `Display`, `Error`

### `struct Scope`

```rust
pub struct Scope
```

Everything derived from one worktree.

If an instance exists, its root is canonical: `Scope::from_root` is the only public
constructor and it canonicalises first, so no caller has to wonder which spelling of a path a
scope was built from.

#### Methods

```rust
pub fn digest(&self) -> &str
```

Short digest of the canonical root. Printed so a stray container can be traced back.

```rust
pub fn from_root(root: &Path) -> Result<Self, ScopeError>
```

Derive a scope from a worktree root.

The canonical constructor. It resolves the path first - symlinks included - so two spellings
of one directory are one worktree, and two genuinely different directories are two. On a
case-folding filesystem that is what makes a case-only difference one worktree, and on a
case-sensitive one it is what stops two real directories being folded into one. Neither
property comes from lowercasing the string, which an earlier version did and which was wrong
on exactly one of those two platforms.

```rust
pub fn project(&self) -> String
```

Compose project name. Lowercase alphanumeric and dashes only, which is all docker
compose accepts, and prefixed so a stray container is identifiable as ours.

**The one function that supplies this name**, to start and to stop alike. Rule 3 of the
teardown contract on `SERVICES` is about the two of them agreeing.

```rust
pub fn root(&self) -> &Path
```

The canonical worktree root.

```rust
pub fn state_dir(&self) -> PathBuf
```

Where provisioning keeps this worktree's state. Under the worktree, never shared.

#### Implements

`Clone`, `Debug`, `Eq`, `PartialEq`

### `constant SERVICES`

The services a worktree may run. Adding one is a row here plus a block in
`compose.services.yaml` - which is NOT `compose.dev.yaml`, the dev-container wrapper.

# The teardown contract provisioning inherits

**Written here because this list is what provisioning reads, and every rule below is a lesson
somebody already paid for.** `xtask/src/compose.rs` is what honours them; this is the statement
of the rules, and none of them may be cited as an invariant - what enforces each one is named
beside it in that module.

1. **Destructive cleanup is dry-runnable.** A command that removes containers, networks, volumes
   or state directories can say what it *would* remove and exit without removing it. The reason is
   not caution in the abstract: the selection logic is the part that goes wrong, and a dry run is
   the only way to inspect the selection without living with it. A destroy whose only mode is
   "do it" is a destroy nobody can review.

2. **Eligibility is re-checked at destroy time, under a lock held across the destroy, and what is
   deliberately spared is reported as its own category.** Deciding a container is stale and then
   removing it are two moments, and another worktree can start between them - so the check that
   said "nobody is using this" has to be re-run inside the lock that the removal happens under,
   not before it. The lock is held for the whole destroy rather than taken per item, because the
   window is what is being closed.

   And the candidates that survived the re-check are **printed as a category of their own** -
   "in use, left alone" - never omitted. Silence there is indistinguishable from "there was
   nothing to consider", which is precisely the case where a reader needs to know the safety
   mechanism fired. **A spared item is a success of the check and has to read as one.**

3. **One function supplies the compose project name to both start and stop, and it supplies it
   the same way.** `Scope::project` is that function. The trap is specific: passing the project
   by command-line flag alone does NOT populate the variable an override file interpolates, so a
   compose file that interpolates the project name into a network, a volume or a container name
   resolves it from an unset variable at destroy time - and the destroy then targets the wrong
   network, or nothing at all, while reporting success. Whatever start relies on, stop has to be
   given identically: the flag AND the environment, from one call site.

# Signalling a process

**A PID is signalled only if its working directory is under this repository.** "Whatever is
listening on a port I expected" is not an identity, and neither is "whatever holds a PID a stale
file names": a PID is reused. The check is the process's own working directory, resolved and
compared against this repository's root, because that is the one property a colliding stranger
cannot accidentally have.
