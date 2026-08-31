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

use sutura_config::{Environment, InboundIdentity, Settings};

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
    announce_provenance(settings);
    announce_perimeter(settings);
    announce_limits(settings);
    announce_capacity(settings);
    announce_surface(settings);
    announce_identity(settings);
}

/// Where the configuration came from.
///
/// **The line whose absence made every other line in this report unfalsifiable.** The report
/// described the resolved values in detail and named no source, so a mistyped configuration
/// directory, a config volume that failed to mount and a deployment that genuinely has no files
/// produced identical logs - all three starting on embedded defaults, none of them saying so. An
/// operator reading a value they did not write had nothing to look at.
///
/// `info` rather than `warn`, and running on defaults is deliberately not a warning: it is the
/// documented posture for a developer on a laptop, and the defaults are loopback-only precisely so
/// that it is safe. What is off-posture is caught by the refusals in `sutura-config`, which decline to
/// start rather than log about it - this line is the evidence an operator needs when the values are
/// legal and still not the ones they wrote.
///
/// Paths only. `ConfigLayers` has nowhere for a value to go, which is what makes this safe to log
/// next to a tree that contains an access token.
fn announce_provenance(settings: &Settings) {
    tracing::info!(
        config_layers = %settings.layers(),
        "the configuration files in effect, in the order they were applied - later beats earlier, and \
         a variable beats both"
    );
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
fn announce_capacity(settings: &Settings) {
    let runtime = settings.runtime();
    tracing::info!(
        max_concurrent_queries = runtime.max_concurrent_queries().count(),
        admission_timeout_seconds = runtime.admission_timeout().seconds(),
        "questions executing at once is bounded; one that cannot get a slot inside the window is \
         answered 503 rather than queued. A question already executing is NOT cancelled by any \
         timeout here"
    );
    let working_set = runtime.working_set();
    if let Some(available) = working_set.checked_against() {
        tracing::info!(
            working_set_max_bytes = working_set.bytes().get(),
            available_bytes = available,
            "the engine's operators may reserve this much at once, checked against the memory this \
             process can reach. It bounds OPERATOR RESERVATIONS - a hash-join build side, aggregate \
             state, a sort - and NOT this process's memory: nothing here bounds what a driver \
             buffers or what materialising a result costs. A reservation over it is a refused \
             question, never a spill to local disk"
        );
    } else {
        // Its own branch, because a ceiling nobody checked is a different fact from one that passed.
        // On a platform that will not report its memory - macOS - an over-configured ceiling starts
        // and dies inside a join later, which an operator has to be able to know from the log.
        tracing::info!(
            working_set_max_bytes = working_set.bytes().get(),
            "the engine's operators may reserve this much at once. NOTHING CHECKED IT: this platform \
             does not report the memory available to the process, so a ceiling above it was not \
             refused at startup and would end the process rather than refuse a question"
        );
    }
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

/// The one line an operator must not be able to miss, and it is now two lines because the answer
/// stopped being one.
///
/// A separate function with a name that says what it is, so it cannot be lost in the middle of a list
/// of fields. **The day the constant stopped being a constant has arrived**: this used to call
/// `SecuritySettings::describes_identity()` as an associated function that always answered `false`,
/// with a comment saying the line would change when it did not. `security.inbound` is what changed
/// it.
///
/// Both branches are `warn`, and that is not an oversight in the second one. A deployment with no
/// inbound identity is warned that it has none; a deployment *with* one is warned about the half it
/// still does not have - leg 1 establishes who is asking and does not make a source execute as that
/// person - and both sentences are read from the config types rather than written here, so neither can
/// drift into claiming the other.
fn announce_identity(settings: &Settings) {
    let security = settings.security();
    match security.inbound() {
        None => tracing::warn!(
            per_caller_identity = security.describes_identity(),
            inbound_mode = security.inbound_mode(),
            "NO PER-CALLER IDENTITY: an access token authenticates the DEPLOYMENT, not the caller. \
             There is no verified caller and no row-level scoping - every question is answered with \
             whatever access this process already had, whoever asked it. A credential IS minted per \
             question, and on this deployment it is the identity this process holds for that source \
             rather than anybody's own. `security.inbound` is the key that changes who is asking"
        ),
        Some(inbound) => {
            tracing::warn!(
                per_caller_identity = security.describes_identity(),
                inbound_mode = security.inbound_mode(),
                establishes = inbound.who_authenticated(),
                token_class = inbound.type_check(),
                limit = InboundIdentity::what_it_does_not_do(),
                "PER-CALLER IDENTITY IS ESTABLISHED AND IS NOT PER-CALLER ACCESS"
            );
            announce_token_class(inbound);
        }
    }
}

/// The line a deployment that switched off the token-class check must not be able to miss.
///
/// **A line of its own, and only when the check is off.** Review found that without a `typ` check any
/// JWT the issuer signed with this audience verifies, an OIDC ID token included - so a deployment that
/// wrote `any` has accepted that, deliberately, and the log has to say so rather than carry the fact as
/// one field among six. The sentence comes off the configuration type, so this function cannot describe
/// a posture the code does not have.
fn announce_token_class(inbound: &InboundIdentity) {
    if !inbound.accepts_any_token_class() {
        return;
    }
    tracing::warn!(
        token_class = inbound.type_check(),
        "THE TOKEN CLASS CHECK IS OFF: `any` was written, so a token of any class this issuer signs \
         for this audience establishes a caller. Where the resource identifier is also a client id, \
         that includes an OIDC ID token"
    );
}

#[cfg(test)]
mod tests {
    use sutura_config::{Environment, Settings, Sources};

    use super::{BANNER, announce_provenance};

    #[test]
    fn the_startup_report_says_which_configuration_files_are_in_effect() {
        // THE BUG. `announce` logged every resolved value in detail and named no source, so a
        // deployment started on embedded defaults - because the configuration directory was mistyped,
        // or a volume failed to mount - looked in the log exactly like one running the operator's own
        // files. Asserted on the bytes, because a `tracing` call has no return value to check.
        let recorded = crate::testing::capture(|| {
            let settings = Settings::load(&Sources::defaults(Environment::Development)).expect("the defaults load");
            announce_provenance(&settings);
        });
        assert!(recorded.contains("config_layers"), "the field is missing:\n{recorded}");
        assert!(
            recorded.contains("embedded defaults only"),
            "an empty layer list has to say so rather than log a blank:\n{recorded}"
        );
    }

    #[test]
    fn a_configured_file_is_named_in_the_report() {
        // The other half: the line is worth nothing if it says the same thing either way. One real
        // file, so what is asserted is the path reaching the log - which is the thing an operator
        // compares against the path they meant to mount.
        let dir = std::env::temp_dir().join(format!("sutura-runtime-layers-{}", std::process::id()));
        drop(std::fs::remove_dir_all(&dir));
        std::fs::create_dir_all(&dir).expect("a scratch directory is creatable");
        std::fs::write(dir.join("base.yaml"), "server:\n  port: 9101\n").expect("a scratch file is writable");

        let recorded = crate::testing::capture(|| {
            let settings = Settings::load(&Sources::defaults(Environment::Development).with_directory(dir.clone()))
                .expect("one layer over the defaults loads");
            announce_provenance(&settings);
        });
        assert!(recorded.contains("base.yaml"), "{recorded}");
        assert!(
            !recorded.contains("embedded defaults only"),
            "a deployment with a file is not running on defaults only:\n{recorded}"
        );
        // Paths, and never a value: the port came out of that file and has no business on this line.
        assert!(
            !recorded.contains("9101"),
            "a value from a file reached the provenance line:\n{recorded}"
        );
        drop(std::fs::remove_dir_all(&dir));
    }

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
