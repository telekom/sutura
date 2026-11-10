# Served tests

The deployment a user actually gets: a settings file, a catalog directory, a listener, a bearer
token, one question, rows.

**This is the only place in the repository where anything asks the SERVED deployment a
question.** `github.com/telekom/sutura#117` is why it had to exist: `sutura serve`'s own named
tests are all startup refusals, `sutura_http::harness` exercises the router in-crate over a fake
service, and `crates/sutura-cli/tests/example.rs` runs the example through a different command
with no HTTP at all. So every invariant that lives only on the composed path - the bearer gate,
route governance, the generated document as SERVED, the refusal statuses out of a real transport
- had no end-to-end evidence on any command in any tier.

# It spawns the binary, and that is not a shortcut

`sutura-cli` is `[[bin]]` and nothing else: there is no library target, so an integration test
cannot call `serve::run` and a test that assembled its own router would be proving the router
rather than the composition. `CARGO_BIN_EXE_sutura` is the binary cargo just built from this
crate's own `main.rs`, invoked with `serve` as its first argument, so what is under test is the
REAL composition root - the same environment reading, the same layered settings, the same
catalog load, the same anchor re-execution, the same `ServiceState`, the same router, the same
listener - reached the only way a deployment reaches it.

# Why it can be a gate rather than a `nix run` app

A files-backed source needs no network and no credential, so this runs inside `checks.nextest`,
which makes it a real gate. The port comes from the kernel (`server.port: 0`, read back off the
socket), because two checks share one sandbox and a fixed port is how that becomes a flake.

# What the harness reads, and why the log is the channel

`telemetry.format: bunyan` is written into the settings so the service's own log is one JSON
object per line, and the harness waits for the `listening` event and takes the address off its
`bound` field. Not a guess and not a scan for a free port: the kernel chose it, the process
reported it, and the two cannot disagree. The same stream is what the ordering assertion in
`the_liveness_probe_answers_only_once_the_catalog_has_loaded` reads.

# `unix` only

Stopping is `SIGTERM`, the shutdown path `sutura_runtime::shutdown` handles. `unsafe_code` is
`forbid`, so there is no `libc::kill`, and `Child::kill` is `SIGKILL`, which proves destruction
rather than a drain. So the signal goes through `/bin/sh`'s builtin `kill`, and the module is
`cfg(unix)`.
