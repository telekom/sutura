#![forbid(unsafe_code)]
//! The agent surface as an agent client gets it: **this binary, spawned, speaking the Model Context
//! Protocol on its own pipes.**
//!
//! The MCP half of `github.com/telekom/sutura#117`, whose third branch was blocked on #110 deciding
//! which composition root serves stdio. That decision is `sutura mcp`, so this is the shape #117
//! asks for pointed at it: list the tools, call one, and compare what came back against the
//! committed schema snapshots in `crates/sutura-mcp/src/snapshots/`.
//!
//! # Why it spawns the binary, when `src/mcp.rs` already has a handshake test
//!
//! `crate::mcp::tests::the_mcp_composition_serves_every_tool_the_surface_declares` joins the SDK's
//! own client and server over `tokio::io::duplex`. That proves `mcp_service` - the composition - and
//! it cannot prove the transport, because the pipe in it is a pair of in-memory buffers rather than
//! this process's standard input and output. #117's own rule for the HTTP half is the same rule
//! here: *a test that builds its own router proves the router*. So the two are not duplicates -
//! that one holds the composition, this one holds the PROCESS.
//!
//! What only a spawned process can answer:
//!
//! * **standard output carries the protocol and nothing else.** The startup notice that states the
//!   every-capability limit is an `eprintln!` precisely because stdout is the protocol channel, and
//!   a single stray `println!` anywhere below `serve_stdio` would corrupt the stream for every
//!   client in the world. [`Agent::reply`] fails by name on that: the first line it reads has to
//!   parse as a JSON-RPC message.
//! * **the process exits when its peer closes the pipe** - through the real `shutdown_timeout` in
//!   `crate::mcp`, with the engine released on the main thread.
//! * **the schemas a client generator consumes off the wire are the reviewed ones.** The snapshot
//!   test in `sutura-mcp` compares `input_schema(capability)` against the committed bytes in
//!   process; this reads the `inputSchema` a served `tools/list` actually put on the pipe and
//!   compares THAT against the same file.
//!
//! # The client is hand-written, deliberately
//!
//! No dev-dependency is added: the MCP stdio framing is one JSON-RPC object per line, so a client
//! is `writeln!` plus `read_line`. `crates/sutura-cli/tests/served.rs` takes the same decision
//! about HTTP for a cost reason; here the reason is evidence. An SDK client on both ends of the
//! pipe would be the SDK agreeing with itself, and it is also free to tolerate a line this suite
//! exists to forbid. A client that is not ours is what makes *stdout is the protocol* an assertion
//! rather than a hope.
//!
//! It is not a conformance suite and does not claim to be: it speaks the subset of the lifecycle
//! this surface serves - `initialize`, `notifications/initialized`, `tools/list`, `tools/call` -
//! and `sutura-mcp` owns interoperability by using the SDK rather than framing its own replies.
//!
//! # What this suite cannot claim
//!
//! Nothing about identity. A pipe has no header a token could arrive in, so this surface grants
//! every capability to whoever can launch the process, and every question below is answered under
//! the single shared identity `examples/single-player` declares.
//! `docs/where-identity-is-proven.md` carries that exclusion in the place a reader of a green run
//! will meet it, beside `just serve-e2e`'s, because a venue that cannot state its limit is how
//! *verified* drifts. `Permitted` narrowing this surface is a composition change nobody has made,
//! so there is no unpermitted-tool case here to write; the unknown-tool case below is the shape it
//! would take.
//!
//! # `unix` only
//!
//! Nothing here signals: the peer closing its end of the pipe IS the shutdown path for this
//! transport, which is why [`Agent::close`] drops the handle rather than sending anything. The
//! module is still `cfg(unix)`, because `CARGO_BIN_EXE_sutura` is the one binary shape this suite
//! reasons about and the platform the gates run on is the one it is measured on.

// `cfg(test)` for the reason `tests/example.rs` gives: clippy honours `allow-expect-in-tests` only
// inside a `#[cfg(test)]` item, and without it every `expect` in the harness below is a lint error.
// Two attributes rather than `cfg(all(test, unix))`, which is not a style choice - clippy looks for
// a literal `#[cfg(test)]` on an ancestor module and does not see through an `all(..)`.
#[cfg(unix)]
#[cfg(test)]
#[path = "mcp/harness.rs"]
mod harness;

// The tests, split by surface for `cargo xtask max-lines`; each takes the two `cfg`s above.
#[cfg(unix)]
#[cfg(test)]
#[path = "mcp/protocol.rs"]
mod protocol;

#[cfg(unix)]
#[cfg(test)]
#[path = "mcp/tools.rs"]
mod tools;

#[cfg(unix)]
#[cfg(test)]
#[path = "mcp/refusals.rs"]
mod refusals;

#[cfg(unix)]
#[cfg(test)]
#[path = "mcp/settings.rs"]
mod settings;

#[cfg(unix)]
#[cfg(test)]
#[path = "mcp/catalog_kinds.rs"]
mod catalog_kinds;
