//! Process-lifecycle concerns for a sutura service: the log, the panic hook, the shutdown signal,
//! the banner, and the bound on how much executes at once.
//!
//! # Why this is its own crate
//!
//! Everything here is process-*global*. Installing a subscriber, replacing the panic hook and
//! registering signal handlers are one-per-process operations that a library must not perform as a
//! side effect of being used - so they belong somewhere a composition root calls deliberately,
//! rather than inside a transport crate that a test also links.
//!
//! It also means the transport crates do not have to agree on any of it. `sutura-http` takes a
//! shutdown as an argument and emits `tracing` events like any other library; nothing in it
//! installs anything. A second surface - an MCP transport, say - gets the same treatment for free.
//!
//! # The order the composition root uses it in
//!
//! ```no_run
//! use sutura_config::{Settings, Sources, environment_from_process};
//! use sutura_runtime::{Admission, banner, shutdown::Shutdown, telemetry};
//!
//! # fn main() -> Result<(), Box<dyn core::error::Error>> {
//! // 1. The environment, first: it decides the log format and which file is layered.
//! let environment = environment_from_process()?;
//! // 2. The banner, to standard output, before any subscriber exists.
//! banner::print(env!("CARGO_PKG_VERSION"), environment);
//! // 3. The configuration. A refusal here is a process that does not start.
//! let settings = Settings::load(&Sources::from_process_environment(environment, None))?;
//! // 4. The log, then the panic hook - in that order, so a panic during setup is not the one
//! //    that gets lost.
//! telemetry::install(settings.telemetry())?;
//! sutura_runtime::install_panic_hook();
//! // 5. What was resolved, including the line about what this service does not do.
//! banner::announce(&settings);
//! // 6. The shutdown, shared with the server and with the signal listener. Built with the
//! //    configured grace period, so the number an operator wrote is the number that bounds
//! //    stopping.
//! let shutdown = Shutdown::with_grace(settings.runtime().shutdown_grace().duration());
//! tokio::spawn(sutura_runtime::shutdown::listen(shutdown.clone()));
//! // 7. The bound on how many questions execute at once, shared with every transport. One per
//! //    process: two independently sized ones would each report a limit the other can exceed.
//! let admission = Admission::from_settings(settings.runtime());
//! # Ok(())
//! # }
//! ```
//!
//! # What is deliberately not here
//!
//! No metrics and no traces-to-a-collector. A span per request exists and is rendered into the log,
//! which is what makes one request's lines findable; exporting it anywhere is a decision about a
//! backend, a sampling rate and an egress path, and none of those has been made. Adding a
//! dependency now to satisfy the word "observability" would be the shape of the thing without the
//! thing.

pub mod admission;
pub mod banner;
pub mod blocking;
pub mod panics;
pub mod shutdown;
pub mod telemetry;

/// The log-capture writer.
///
/// `cfg(test)` for this crate's own suite, and behind `test-capture` for another crate's. The
/// feature's comment in `Cargo.toml` says why only the writer is exposed and not the helpers
/// around it.
#[cfg(any(test, feature = "test-capture"))]
pub mod testing;

pub use crate::admission::{Admission, AtCapacity, Slot};
pub use crate::blocking::spawn_carrying_span;
pub use crate::panics::install_panic_hook;
pub use crate::shutdown::{Shutdown, ShutdownReason};
pub use crate::telemetry::TelemetryNotInstalled;
