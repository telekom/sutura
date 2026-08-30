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

# Two halves, and the second one is what a caller uses

* `discovery` is the file: publishing it, reading it, and the fact that there is no other way
  to learn a port. It is the door that *can* be opened.
* `provisioned` is the door a caller *should* open. Same file underneath, plus the two things
  no test should have to write twice: the diagnostic that names the task to run, and the
  skip-or-fail decision from `requirement`. A harness that read `discovery` directly would
  get a connection refused thirty seconds later, blamed on the code under test.

Publishing has one door and consumption has one door, and they are not the same door because the
two callers are not the same: provisioning knows it is provisioning, while a test does not know
whether anything is up.

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
pub fn provisioner(&self) -> Option<&str>
```

What provisioned this tier, where the file says.

`docker` for `xtask dev-up`, `nix` for `nix/postgres-tier.nix`. `None` when an older file (or
a hand-written one) carried no marker - a reader must not assume docker from the absence.

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
- `HostNeitherLoopbackNorSocket` - A service host that is neither loopback nor a `/`-prefixed socket directory.

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

## Module `provisioned`

The consumption half: how a test, an example or a demo reaches a service this worktree brought up.

`crate::discovery` is the door that *can* be opened; this module is the one a caller should
actually use, and the difference is two things neither a test nor a reader should have to write
twice.

# 1. The diagnostic, which is most of the value here

Without it the failure a developer sees is a connection refused, thirty seconds into a test,
attributed to the adapter under test rather than to a tier that was never started. Every path
out of `here` and `in_worktree` carries `Absent` instead, which names the worktree, the
file it looked in, what the file said, and the task to run. The whole point of allocating a port
per worktree is that nobody has to know the port; the cost of that is that nobody can guess it
either, so the message has to close the gap.

# 2. The skip-or-fail decision, made once

`here` applies `crate::requirement`: a missing tier skips loudly on a developer machine and
fails where the tier is required. A test that made that decision for itself would make it
differently from the next test, and one of them would make it silently.

# What is deliberately not here

**No fallback port, at any level.** Not a default, not a "try the container port", not an
environment variable a caller could set to a constant. A fallback connects to whatever else
holds that port, and on a machine running two worktrees of this repository that is the
neighbour's fixture - a test that passes against the wrong data and says nothing about it. That
is the exact failure the per-worktree design removes, so re-introducing it as a convenience
would remove the design.

**No knowledge of docker**, for the same reason the rest of this crate has none: bringing
services up is `xtask`'s job. This module reads a file.

### `enum Provisioned`

```rust
pub enum Provisioned
```

What a harness gets when it asks for a provisioned service.

Two variants and no third, because the fail direction does not return: see `here`.

#### Variants

- `At` - It is up, and this is where. Read from the discovery file, which is the only place a host port for this worktree exists.
- `Skipped` - Nothing to connect to, on a machine class where that is not a failure.

#### Methods

```rust
pub const fn endpoint(&self) -> Option<&Endpoint>
```

The endpoint, where there is one.

For a caller that wants `let Some(endpoint) = .. else { return }` rather than a match. The
skip has already been reported either way, so discarding the `Absent` loses nothing.

#### Implements

`Debug`

### `struct Absent`

```rust
pub struct Absent
```

Nothing to connect to, and what to do about it.

The typed fields are the contract and the `Display` form is the message; a caller that wants to
branch reads `Absent::reason` rather than the prose.

Plain backticks on `Display` rather than a rustdoc link to `std::fmt::Display`, and that is the
rule `AGENTS.md` states rather than a preference: the api-docs generator copies a link to another
crate's path through verbatim, and `mkdocs build --strict` then aborts on an unrecognized
relative link - which every nix check passes over, because none of them builds the site.

**Boxed, and it is a lint that says so rather than taste.** The diagnostic is four fields wide
and one of them is another error, which puts the whole thing past `result_large_err`: every
`Ok(Endpoint)` on the way back would carry room for it. This is the cold path and it can afford
one allocation, so the box is here and not at the call sites - a public
`Result<Endpoint, Box<Absent>>` would push it onto everybody instead.

#### Methods

```rust
pub const fn reason(&self) -> &Reason
```

What stopped it. Branch on this, never on the message.

```rust
pub fn service(&self) -> &str
```

The service that was asked for.

#### Implements

`Debug`, `Display`, `Error`

### `enum Reason`

```rust
pub enum Reason
```

Which half failed. A variant rather than a sentence, because "run `just dev-up`" is the wrong
advice for two of these and a caller matching on prose has no contract.

#### Variants

- `NoWorktree` - The directory given is not inside a checkout of this repository, so there is no worktree whose discovery file could be read.
- `NoScope` - A worktree root that could not become a `Scope`.
- `NotDiscovered` - There is a worktree, and its discovery file does not answer.

#### Implements

`Debug`, `Display`

### `fn in_worktree`

```rust
pub fn in_worktree(root: &std::path::Path, service: &str) -> Result<crate::discovery::Endpoint, Absent>
```

Where one service in ONE named worktree is listening.

The half with no environment and no printing in it, so a caller that already knows which
worktree it means - a `just` task, a test over a fixture directory - can drive it directly.

```
use sutura_dev::provisioned;

// A temporary directory is a real directory and nothing has provisioned it, so this is the
// diagnostic path rather than an endpoint. Note what it is NOT: a default port.
let problem = provisioned::in_worktree(&std::env::temp_dir(), "postgres")
    .expect_err("nothing is provisioned in a temporary directory");
assert_eq!(problem.service(), "postgres");
assert!(problem.to_string().contains("just dev-up"), "{problem}");
```

### `fn here`

```rust
pub fn here(inside: &std::path::Path, service: &str) -> Provisioned
```

Where one service in THIS worktree is listening, with the skip-or-fail decision applied.

`inside` is any directory in the worktree; an integration test passes
`Path::new(env!("CARGO_MANIFEST_DIR"))`, which is the one thing a test reliably knows about
where it is. The worktree root is found by walking upwards - see `worktree_root`.

# Panics

In the `Requirement::Required` direction, and only there. The caller is a test, a panic is how
a test fails, and returning `Provisioned::Skipped` there would be the silent green run this
whole tier exists to prevent. On a developer machine the direction is
`Requirement::Optional`, the notice goes to stderr, and nothing panics.

### `fn worktree_root`

```rust
pub fn worktree_root(inside: &std::path::Path) -> Option<std::path::PathBuf>
```

The worktree root at or above `inside`, or `None` if there is not one.

**Both markers, not either**, and this is borrowed from `xtask`'s own root walk because the same
two mistakes are available: `flake.nix` alone appears in unrelated directories, and `Cargo.toml`
alone matches every crate on the way up - which would stop the walk at a workspace MEMBER and
derive a scope for a directory no provisioning ever used.

A walk rather than `git rev-parse`, deliberately. A harness runs where a `.git` directory may not
be - a nix sandbox copies the tree without one - and shelling out to git from a test is a
subprocess in the way of an assertion.

## Module `requirement`

Whether an absent service tier is a skip or a failure - one definition, read by both halves.

The decision has two call sites and they are on opposite sides of the tier:

* **Provisioning** asks it when there is no container runtime to bring services UP with.
* **A harness** asks it when there is nothing provisioned to CONNECT to.

It lived in `xtask` while there was only the first, and it moved here when the second arrived.
Two copies of a fail-open/fail-closed decision is the shape that drifts: the copies are edited
months apart, one of them stops matching the documentation, and the direction a wrong answer
costs the most is the one that silently flipped.

**Neither direction is the default, and what a wrong answer costs decides it.** A false failure
blocks a contributor who is not touching services - docker is a host dependency this repository
deliberately does not pin with nix. A false pass reports green having tested nothing, which is
the failure the whole tier exists to prevent.

**So the signal is "somebody provisioned a tier here", and it is NOT the `CI` variable.** That
distinction was learned rather than designed: this module first read `CI`, on the reasoning that
CI is where a silent skip costs most. The reasoning was right and the signal was wrong, and the
event that proved it happened IN CI: the branch that added this module provisioned no tier, so
`CI=true` made a missing tier fatal right where its absence was expected - on its first push, in
a step that had provisioned nothing. And nothing has changed that shape: no CI job sets `CI`
only when it has provisioned a tier. What DOES opt in is the nix `checks.nextest` derivation,
which provisions its own Postgres over a unix socket (`nix/postgres-tier.nix`) and sets the
variable below; a docker tier needs docker on the host and opts in the same way.

Only the thing that provisions the tier knows that it did. So that thing opts in by setting the
variable below and gets the fail-closed direction; everything else skips loudly and names what did
not run. **The limit, stated with the claim:** nothing here verifies that a process setting the
variable really did provision anything - it is a declaration, and a process that lies about it
gets the failure it asked for.

### `enum Requirement`

```rust
pub enum Requirement
```

Whether a missing tier is fatal.

#### Variants

- `Required` - A missing tier FAILS. What a job that has PROVISIONED the tier asks for by setting `FORCE`: there, a green run that quietly tested nothing is the failure the whole tier exists to prevent.
- `Optional` - A missing tier SKIPS, loudly, naming what did not run. The developer-machine direction.

#### Methods

```rust
pub fn from_env() -> Self
```

The direction this process is running under, read from the environment.

```rust
pub const fn is_required(self) -> bool
```

Is a missing tier fatal here?

#### Implements

`Clone`, `Copy`, `Debug`, `Eq`, `PartialEq`

### `fn decide`

```rust
pub fn decide(forced: Option<&str>) -> Requirement
```

The decision, over the value rather than over the environment, so it is testable.

**One parameter, and it used to be two.** The other was `CI`, and it is gone rather than ignored:
a parameter a function does not read is a parameter a caller believes in. See the module header for
why that signal was the wrong one.

### `constant FORCE`

The variable that overrides the machine class, in **both** directions.

Named once, here, because a message that tells somebody to set it and a read that spells it
differently is a fix that does not work and looks like it should.

## Module `scope`

Per-worktree isolation: the part that has to be right.

Several worktrees of this repo are open at once - that is the point of stacked branches -
and each needs its own services. Two worktrees sharing a
container is the worst outcome available: a test passes because the *other* branch's
migration ran, and the failure appears in whichever branch is unlucky.

Every containerised service in the compose tier is scoped to a worktree (Postgres is not here:
it is nix-native, provisioned by `nix/postgres-tier.nix` and run by `checks.nextest` and by
`just test`, over a unix socket in a short per-worktree directory under `$TMPDIR` - see that
module).

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
pub const fn is_default(&self) -> bool
```

Is this service started when no profile was asked for?

```rust
pub const fn name(&self) -> &'static str
```

Name used in the compose file, in the discovery file and in output.

```rust
pub const fn profile(&self) -> Option<&'static str>
```

The compose profile that turns this service on, or `None` for one always started.

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

### `fn profiles`

```rust
pub fn profiles() -> Vec<&'static str>
```

Every profile any service declares, in declaration order and without repeats.

Teardown enables all of them, and that is the reason this exists: `docker compose down` only
considers services in ACTIVE profiles, so a destroy that forgot one would leave that service's
container and named volume behind **while reporting success** - the same silent-success failure
the teardown contract below is about. Derived rather than listed, so adding a profile does not
need a second edit somewhere else to stay correct.

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
