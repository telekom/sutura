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
principal" holds trivially: a `DuckDB` file has no login to present. That makes it the
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
