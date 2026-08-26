//! What the process says about itself on the way up.
//!
//! Two things, and they go to different places on purpose.
//!
//! The banner is `println!`, before any subscriber exists. It is for a person watching a terminal
//! or scrolling to the top of a container log, and it answers "what is this and which build is it"
//! without needing the log format to be readable yet.
//!
//! The configuration report is `tracing`, after the subscriber is installed, so it lands in
//! whatever a collector is ingesting. It is for an operator, and the lines that matter most are
//! the ones about what this service does *not* do.

use sutura_config::{Environment, SecuritySettings, Settings};

/// The name, in block letters.
///
/// A static string rather than a figlet library, and that is a dependency decision: one word is
/// rendered once, at startup, from a fixed font, so computing it at runtime buys a crate, its
/// embedded font tables and two unwraps for an output that never changes.
///
/// Pure ASCII, because the workspace denies `clippy::non_ascii_literal` and because a box-drawing
/// banner is a row of question marks on a terminal without UTF-8.
const BANNER: &str = r"
             _
 ___  _   _ | |_  _   _  _ __   __ _
/ __|| | | || __|| | | || '__| / _` |
\__ \| |_| || |_ | |_| || |   | (_| |
|___/ \__,_| \__| \__,_||_|    \__,_|
";

/// Prints the banner and the build line to standard output.
///
/// Before the subscriber, deliberately. `version` is the caller's `CARGO_PKG_VERSION`: taking it
/// as an argument rather than reading this crate's own means the number printed is the binary's,
/// which is the one somebody is trying to identify.
pub fn print(version: &str, environment: Environment) {
    println!("{BANNER}");
    println!("  identity-aware semantic data runtime for AI agents");
    println!("  version {version} - environment {environment}");
    println!();
}

/// Writes the resolved configuration to the log.
///
/// **Every value comes from the loaded [`Settings`], not from a file.** The environment layer is
/// applied last, so a report built from a file could describe a deployment the process is not
/// running as - which is the same reason the refusals in `sutura-config` read the loaded value.
///
/// The whole tree goes out as one `Debug` field. That is safe because the only credential-shaped
/// value in it is held in a type whose `Debug` redacts, and `sutura-config` has a test asserting
/// that at the outermost struct - not because this function was careful.
pub fn announce(settings: &Settings) {
    tracing::info!(
        environment = %settings.environment(),
        resolved = ?settings,
        "configuration resolved"
    );
    announce_perimeter(settings);
    announce_limits(settings);
    announce_capacity(settings);
    announce_surface(settings);
    announce_identity_gap();
}

/// How much runs at once, how wide the engine is, and how long stopping may take.
///
/// Separate from [`announce_limits`] because it bounds a different thing: those keys are about one
/// request, these are about the process. An operator sizing a deployment reads this line, so it
/// carries the numbers in effect rather than the numbers in a file - `engine_worker_threads` in
/// particular is resolved from the machine when it is absent, and `chosen` is what says whether the
/// number being printed was anybody's decision.
///
/// The sentence about what the concurrency bound does *not* do is here rather than in a document,
/// because the failure mode is an operator reading `max_concurrent_queries` as a bound on how long
/// stopping can take. It is not: a question already executing runs to completion.
#[expect(
    clippy::cognitive_complexity,
    reason = "every branch here is a tracing macro expanding into one; the control flow is a single decision"
)]
fn announce_capacity(settings: &Settings) {
    let runtime = settings.runtime();
    tracing::info!(
        max_concurrent_queries = runtime.max_concurrent_queries().count(),
        admission_timeout_seconds = runtime.admission_timeout().seconds(),
        "questions executing at once is bounded; one that cannot get a slot inside the window is \
         answered 503 rather than queued. A question already executing is NOT cancelled by any \
         timeout here"
    );
    let workers = runtime.engine_workers();
    if workers.was_chosen() {
        tracing::info!(
            engine_worker_threads = workers.count(),
            "in-process engine width, as configured"
        );
    } else {
        // Worth its own branch: `available_parallelism` reports what the kernel exposes, and under a
        // CPU quota that is the host's core count rather than this container's share. An operator
        // seeing a number they did not choose needs to know it came from the machine.
        tracing::info!(
            engine_worker_threads = workers.count(),
            "in-process engine width, resolved from the machine - a container with a CPU quota \
             should set runtime.engine_worker_threads instead"
        );
    }
    tracing::info!(
        shutdown_grace_seconds = runtime.shutdown_grace().seconds(),
        "the budget for the WHOLE of stopping: the connection drain first, then what is left of it \
         for questions already executing"
    );
}

/// Where it listens, and whether anything guards that.
#[expect(
    clippy::cognitive_complexity,
    reason = "every branch here is a tracing macro expanding into one; the control flow is a single decision"
)]
fn announce_perimeter(settings: &Settings) {
    let bind = settings.server().bind();
    let security = settings.security();
    if bind.is_loopback() {
        tracing::info!(
            bind = %bind,
            access_token = security.token_state(),
            "listening on loopback only - reachable from this host and no other"
        );
    } else {
        // `warn` and not `info`: this is the state where an unauthenticated request from another
        // host would be answered if the token were ever removed. Reaching it takes an explicit
        // `security.tls_termination`, because an off-host bind is refused until the operator has
        // said where TLS ends - so the state was chosen rather than defaulted into, and saying so
        // once per boot is the cost of that choice.
        tracing::warn!(
            bind = %bind,
            access_token = security.token_state(),
            "listening beyond loopback - other hosts can reach this service"
        );
    }
}

/// What bounds a caller.
#[expect(
    clippy::cognitive_complexity,
    reason = "every branch here is a tracing macro expanding into one; the control flow is a single decision"
)]
fn announce_limits(settings: &Settings) {
    let limits = settings.rate_limit();
    if limits.enabled() {
        tracing::info!(
            probe_per_second = limits.probe().per_second().get(),
            probe_burst = limits.probe().burst().get(),
            api_per_second = limits.api().per_second().get(),
            api_burst = limits.api().burst().get(),
            "rate limiting enabled - buckets are keyed by network address, which is not a caller"
        );
    } else {
        // Unreachable in production: `sutura-config` refuses to start there. Kept because the
        // configuration is what decides, and a state that cannot happen is cheaper to log than to
        // reason about.
        tracing::warn!("rate limiting is DISABLED - a caller is bounded only by the data system");
    }
    tracing::info!(
        request_timeout_seconds = settings.server().request_timeout().seconds(),
        max_body_bytes = settings.server().max_body().bytes(),
        "per-request bounds"
    );
}

/// What is served, and what the answers are computed from.
#[expect(
    clippy::cognitive_complexity,
    reason = "every branch here is a tracing macro expanding into one; the control flow is a single decision"
)]
fn announce_surface(settings: &Settings) {
    let api = settings.api();
    if api.docs_enabled() && settings.environment().is_production() {
        tracing::warn!(
            explicit = api.docs_were_explicit(),
            "the generated interface description is served in production"
        );
    } else {
        tracing::info!(docs = api.docs_enabled(), "generated interface description");
    }
    tracing::info!(
        catalog_dir = %settings.catalog().dir().display(),
        data_dir = %settings.catalog().data_dir().display(),
        definition_version = %settings.catalog().version(),
        log_format = %settings.telemetry().format(),
        log_format_explicit = settings.telemetry().format_was_explicit(),
        "catalog and log"
    );
}

/// The one line an operator must not be able to miss.
///
/// A separate function with a name that says what it is, so it cannot be lost in the middle of a
/// list of fields, and `warn` so it survives a filter that drops `info`. It is unconditional:
/// there is no configuration that makes this untrue today, and the day there is, the function that
/// answers it stops being a constant and this line changes with it.
fn announce_identity_gap() {
    tracing::warn!(
        per_caller_identity = SecuritySettings::describes_identity(),
        "NO PER-CALLER IDENTITY: an access token authenticates the DEPLOYMENT, not the caller. \
         There is no request context, no per-request credential and no row-level scoping - every \
         question is answered with whatever access this process already had, whoever asked it"
    );
}

#[cfg(test)]
mod tests {
    use super::BANNER;

    #[test]
    fn the_banner_is_ascii_and_carries_no_trailing_whitespace() {
        // Both halves are gates elsewhere - `clippy::non_ascii_literal` and the text-hygiene
        // check - and both would fail on the file rather than on the value. Asserting on the
        // value is what catches a banner assembled at runtime later on.
        assert!(BANNER.is_ascii(), "the banner must render on a terminal without UTF-8");
        for line in BANNER.lines() {
            assert_eq!(line.trim_end(), line, "trailing whitespace in {line:?}");
        }
    }

    #[test]
    fn the_banner_is_five_lines_of_block_letters() {
        // A shape assertion rather than a content one: it fails if the art is truncated or if a
        // row is lost to an editor, which is the way this file actually breaks.
        let rows: Vec<&str> = BANNER.lines().filter(|line| !line.trim().is_empty()).collect();
        assert_eq!(rows.len(), 5, "{rows:?}");
    }
}
