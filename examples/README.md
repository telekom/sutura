# Examples

Runnable catalogs. Each directory here is a complete input to the binary: a catalog of
markdown documents, the data those documents describe, and a corpus of questions asked
against them.

They are examples and tests at the same time, and that is the point rather than a
convenience. `crates/sutura-cli/tests/example.rs` loads every catalog here, pins its
digest, re-executes every declared anchor and runs every question, so a quickstart that
stopped working fails the build instead of failing the next person who tried it. There is
no separate copy of the commands below for CI to run.

## The two directories

The split is about identity, which is the thing sutura exists for.

**`single-player/`** is one catalog in git, one file per model on disk, and the access the
process already has. There is nobody else to be, so "every query runs as the calling
principal" holds trivially: a local file has no login to present. That makes it the
right shape for learning the format, and it is also exactly the claim that a laptop
cannot test.

**`multi-player/`** is where that stops being free: per-request credentials, two callers
getting different rows for the same question, and a refusal when a leg cannot run as the
subject. It is a placeholder today, and its README says what is missing and why.

## The data

Every dataset here is synthetic. It is produced by a seeded pseudo-random generator, the
keys look like `C0001` because a generator wrote them, and no row corresponds to a real
person, contract or account. It is shaped like telco data because a semantic layer is
easier to read over a domain with recognisable metrics, and none of the numbers mean
anything outside this repository.

## Reaching a data system, when an example needs one

**Neither directory here needs one today**, and saying so first is the honest order:
`single-player/` is CSVs the engine reads directly, and `multi-player/` is a placeholder. The
development service tier exists for the adapters that are not written yet. This section is here so
that the documented path to it is one command rather than something a reader has to work out, and
so that the answer to "which port?" is never a number in a README.

```bash
just dev-up                            # this worktree's Postgres and ClickHouse
just dev-endpoints                     # the readable table: service, host and port
just dev-endpoint postgres             # just `host:port`, for a shell to substitute
just dev-down                          # remove them, their network and their volumes
```

**No port appears above on purpose, and none appears anywhere else either.** Every port in the
tier is published ephemerally, so docker and the operating system pick it - which is what makes
two worktrees of this repository running the tier at the same time safe rather than lucky. A
number written down here would be wrong on the next run, and right often enough to mislead in
between. `just dev-endpoint <service>` reads what was actually bound, out of a file provisioning
wrote inside this worktree, so nobody following an example has to know that worktree scopes,
compose projects or ephemeral ports exist.

When there is nothing provisioned it says so and names the task to run, rather than handing back
a plausible number. **There is no default to fall back to, deliberately:** a fallback would
connect to whatever else holds that port, and on a machine with two worktrees open that is the
other one's fixture - a run that looks like it worked and read the wrong data. The reasoning is
under *Per-worktree instances, and why a port cannot be a constant* in
`docs/implementation-plan-identity-and-services.md`.

## Serving a catalog

The single-player catalog is also an input to `sutura-serve`, the second binary, and that
surface has a threat model the command line does not: a token is required beyond loopback and
it authenticates the deployment rather than the caller, a refusal carries an error status AND a
machine-readable `code` AND a sentence, and the service refuses to start in a posture nobody chose.
`single-player/README.md` has a captured session showing all of that - the startup output
including the line saying there is no per-caller identity, a question and its `provenance`, a
refusal over the wire, the token gate, the liveness probe, the generated interface description,
and a refusal to start. `docs/serving.md` is the configuration reference behind it.
