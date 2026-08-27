//! The log: one subscriber, two renderings, and the environment decides which.
//!
//! In production a log line is read by a collector. It has to be one JSON object per line with the
//! span context attached, or a query cannot find every line belonging to one request. On a laptop
//! the same line is read by a person who is recompiling every thirty seconds, and a JSON object
//! per line is unreadable there.
//!
//! Both are correct for their reader, so the choice is made once, from
//! [`sutura_config::LogFormat`], whose default follows the environment. That is the whole of the
//! split - there is no second switch that could disagree with it.
//!
//! # What overrides what
//!
//! `RUST_LOG`, if it is set, beats `telemetry.filter`. That is the convention every Rust operator
//! already has, and the configured value is the fallback rather than the ceiling. Unlike the usual
//! spelling of this, an *invalid* `RUST_LOG` is an error rather than something quietly discarded:
//! a discarded filter means the process logs at some other level and nothing says so, which is the
//! failure mode a filter exists to prevent.

use sutura_config::{LogFormat, TelemetrySettings};
use tracing::Subscriber;
use tracing_bunyan_formatter::{BunyanFormattingLayer, JsonStorageLayer};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::SubscriberExt as _;

/// The variable that overrides the configured filter.
pub const FILTER_VARIABLE: &str = "RUST_LOG";

/// Why the log could not be set up.
#[derive(Debug, thiserror::Error)]
pub enum TelemetryNotInstalled {
    /// A filter directive would not parse. Either the configured one or `RUST_LOG`; `source` says
    /// which, because they fail for different reasons and only one of them is in a file.
    #[error("`{source_name}` is not a valid filter directive: {directive}")]
    Filter {
        source_name: &'static str,
        directive: String,
        #[source]
        cause: tracing_subscriber::filter::ParseError,
    },
    /// `RUST_LOG` held bytes that are not text.
    #[error("`{FILTER_VARIABLE}` is not valid Unicode")]
    FilterNotUnicode,
}

/// A subscriber built for one set of settings.
///
/// A named alias because the inline form is over the complexity threshold in `clippy.toml`, and
/// naming it is the better half of that trade: what matters about the type is that it is *one*
/// type, which is the whole reason the box is there.
pub type BuiltSubscriber = Box<dyn Subscriber + Send + Sync>;

/// Builds the subscriber for these settings, writing to `sink`.
///
/// Boxed, and that is what lets the two arms be one type: a pretty formatter and a bunyan
/// formatter compose different layer stacks, so there is no `impl Subscriber` they share. The box
/// is paid once, at startup.
///
/// Generic in the sink so a test can build both arms over a buffer and assert on the bytes. That
/// is the only way to test this: installing a subscriber is process-global and happens once.
pub fn subscriber<Sink>(telemetry: &TelemetrySettings, sink: Sink) -> Result<BuiltSubscriber, TelemetryNotInstalled>
where
    Sink: for<'a> MakeWriter<'a> + Send + Sync + 'static,
{
    let filter = filter(telemetry)?;
    let registry = tracing_subscriber::registry().with(filter);
    let built: BuiltSubscriber = match telemetry.format() {
        LogFormat::Pretty => Box::new(
            registry.with(
                tracing_subscriber::fmt::layer()
                    .with_writer(sink)
                    .with_target(true)
                    .with_file(true)
                    .with_line_number(true)
                    .with_ansi(true)
                    .pretty()
                    // Span timing on close, which is what makes a slow request visible without a
                    // metric: the span for a request reports how long it was open.
                    .with_span_events(tracing_subscriber::fmt::format::FmtSpan::CLOSE),
            ),
        ),
        // `JsonStorageLayer` is not optional here. It is what collects a span's fields so the
        // formatter can attach them to every event inside that span; without it the JSON is one
        // flat event per line with no request context, which is most of the reason for using this
        // format at all.
        LogFormat::Bunyan => Box::new(registry.with(JsonStorageLayer).with(BunyanFormattingLayer::new(
            String::from(telemetry.service_name().as_str()),
            sink,
        ))),
    };
    Ok(built)
}

/// Installs the subscriber for these settings as the process-wide one, writing to standard output.
///
/// Standard output rather than standard error, for both formats: a collector reads one stream, and
/// splitting the log across two means half of it is interleaved somewhere else. Diagnostics that
/// have to be visible *before* this is installed - the banner, and a configuration that refused to
/// load - go to their own stream and say so.
///
/// Idempotent in the sense that matters: a second call is ignored rather than failing, because the
/// only thing that could call twice is a test harness and a panic there would be about the harness
/// rather than about the service.
pub fn install(telemetry: &TelemetrySettings) -> Result<(), TelemetryNotInstalled> {
    let subscriber = subscriber(telemetry, std::io::stdout)?;
    // The `log` crate bridge. Several dependencies still emit through `log`; without this their
    // diagnostics are dropped silently rather than appearing in the same stream as everything else.
    drop(tracing_log::LogTracer::init());
    drop(tracing::subscriber::set_global_default(subscriber));
    Ok(())
}

/// The filter: `RUST_LOG` if it is set and parses, otherwise the configured directive.
fn filter(telemetry: &TelemetrySettings) -> Result<EnvFilter, TelemetryNotInstalled> {
    match std::env::var(FILTER_VARIABLE) {
        Ok(directive) if !directive.trim().is_empty() => {
            EnvFilter::try_new(&directive).map_err(|cause| TelemetryNotInstalled::Filter {
                source_name: FILTER_VARIABLE,
                directive,
                cause,
            })
        }
        // An empty variable is the shape an unset one takes in a shell - `RUST_LOG=` in a
        // manifest - so it falls through to the configured value rather than meaning "no
        // directives", which is what an empty `EnvFilter` would silently be.
        Ok(_) | Err(std::env::VarError::NotPresent) => configured(telemetry),
        Err(std::env::VarError::NotUnicode(_)) => Err(TelemetryNotInstalled::FilterNotUnicode),
    }
}

/// The directive from the configuration.
fn configured(telemetry: &TelemetrySettings) -> Result<EnvFilter, TelemetryNotInstalled> {
    let directive = telemetry.filter().as_str();
    EnvFilter::try_new(directive).map_err(|cause| TelemetryNotInstalled::Filter {
        source_name: "telemetry.filter",
        directive: String::from(directive),
        cause,
    })
}

#[cfg(test)]
mod tests {
    use sutura_config::LogFormat;

    use super::{TelemetryNotInstalled, subscriber};
    use crate::testing::{capture_with, settings};

    /// Emits one event inside a span through a subscriber built for `format`.
    fn render(format: LogFormat) -> String {
        capture_with(format, || {
            let span = tracing::info_span!("a_request", correlation = "abc123");
            let _entered = span.enter();
            tracing::info!(answered = true, "an event inside a span");
        })
    }

    #[test]
    fn the_machine_readable_format_is_one_json_object_per_line_carrying_the_span_context() {
        // The property a collector depends on, and the reason `JsonStorageLayer` is in that arm:
        // a field set on the SPAN has to appear on the EVENT, or a query cannot group one
        // request's lines together.
        let rendered = render(LogFormat::Bunyan);
        let first = rendered.lines().next().expect("at least one line was written");
        assert!(first.starts_with('{'), "{first}");
        assert!(first.ends_with('}'), "{first}");
        assert!(rendered.contains("abc123"), "{rendered}");
        assert!(rendered.contains("sutura-test"), "{rendered}");
    }

    #[test]
    fn the_human_readable_format_is_not_json() {
        // The other side of the split. Asserted as "not a JSON object" rather than on the exact
        // layout, which is the formatter's business and changes between versions.
        let rendered = render(LogFormat::Pretty);
        let first = rendered.lines().next().expect("at least one line was written");
        assert!(!first.trim_start().starts_with('{'), "{first}");
        assert!(rendered.contains("an event inside a span"), "{rendered}");
    }

    #[test]
    fn the_two_formats_render_the_same_event_differently() {
        // Guards against the split silently collapsing: if both arms ever produced the same bytes,
        // one of the two readers is being served the wrong thing.
        assert_ne!(render(LogFormat::Bunyan), render(LogFormat::Pretty));
    }

    #[test]
    fn an_unparseable_configured_directive_is_an_error_naming_the_key() {
        // A filter that does not parse must not fall back to a default level: the process would
        // then log at some other verbosity with nothing anywhere saying so.
        //
        // `LogFilter` accepts this string - it checks for emptiness and control characters, not
        // for filter grammar, because the grammar lives in a crate the configuration layer does
        // not depend on. This is where that check happens.
        // A `let ... else` rather than `expect_err`: the success half of this `Result` is a boxed
        // trait object, which has no `Debug`, so `expect_err` does not compile for it.
        let Err(error) = subscriber(&settings(LogFormat::Pretty, "=="), std::io::sink) else {
            panic!("`==` is not a filter directive and must not build a subscriber");
        };
        let TelemetryNotInstalled::Filter {
            source_name,
            ref directive,
            ..
        } = error
        else {
            panic!("expected a filter error, got {error:?}");
        };
        assert_eq!(source_name, "telemetry.filter");
        assert_eq!(directive, "==");
    }
}
