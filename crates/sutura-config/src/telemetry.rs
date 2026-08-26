//! What the log looks like, and who is meant to read it.
//!
//! One decision with two right answers, which is why it is a type rather than a flag. In
//! production a log line is read by a collector: it has to be one JSON object per line, with the
//! span context attached, so a query can find every line belonging to one request. On a laptop
//! the same line is read by a person compiling every thirty seconds, and a JSON object per line
//! is unreadable there.
//!
//! The default therefore follows [`crate::Environment`] and nothing else, so the two cannot be
//! set inconsistently by omission. An explicit value overrides it - a developer debugging what
//! the collector will actually receive needs that - and the startup log says which of the two
//! happened.

use crate::Environment;

/// How a log line is rendered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "String")]
pub enum LogFormat {
    /// One JSON object per line, in the bunyan schema. What a collector ingests.
    Bunyan,
    /// Indented, coloured, and meant for a terminal.
    Pretty,
}

/// The string was neither format.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("`{found}` is not a log format - use one of: {}", LogFormat::NAMES.join(", "))]
pub struct UnknownLogFormat {
    found: String,
}

impl LogFormat {
    /// Every accepted spelling.
    pub const NAMES: &'static [&'static str] = &["bunyan", "pretty"];

    /// Reads a format name.
    pub fn parse(raw: impl AsRef<str>) -> Result<Self, UnknownLogFormat> {
        let raw = raw.as_ref().trim();
        match raw.to_ascii_lowercase().as_str() {
            "bunyan" | "json" => Ok(Self::Bunyan),
            "pretty" | "human" => Ok(Self::Pretty),
            _ => Err(UnknownLogFormat {
                found: String::from(raw),
            }),
        }
    }

    /// The format an environment gets when the configuration does not say.
    ///
    /// **This is the split, in one function.** Production is machine-readable and everything else
    /// is human-readable, and it is expressed as a total match rather than an `if` so a fourth
    /// environment cannot inherit an answer nobody chose for it.
    #[inline]
    pub const fn default_for(environment: Environment) -> Self {
        match environment {
            Environment::Production => Self::Bunyan,
            Environment::Development | Environment::Test => Self::Pretty,
        }
    }

    #[inline]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Bunyan => "bunyan",
            Self::Pretty => "pretty",
        }
    }
}

impl TryFrom<String> for LogFormat {
    type Error = UnknownLogFormat;

    fn try_from(raw: String) -> Result<Self, Self::Error> {
        Self::parse(raw)
    }
}

impl core::fmt::Display for LogFormat {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A tracing filter directive, as written for `RUST_LOG`.
///
/// Kept as text here and turned into a real filter by whatever installs the subscriber, because
/// the type that parses one lives in `tracing-subscriber` and this crate holds no framework. What
/// *is* checked here is that it is not empty and not a smuggled second line: an empty filter
/// silently means "no directives", which is a service that logs at the default level while its
/// configuration says otherwise.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogFilter(String);

/// Why a string is not a filter directive.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvalidLogFilter {
    /// Empty, or only whitespace.
    #[error("telemetry.filter is empty - write a directive such as `info` rather than nothing")]
    Empty,
    /// A control character, which in practice is a newline pasted in with the value. A newline in
    /// a filter is a log line an attacker can forge if the value ever reaches the log itself.
    #[error("telemetry.filter contains a control character")]
    ControlCharacter,
}

impl LogFilter {
    /// Reads a filter directive.
    pub fn parse(raw: impl AsRef<str>) -> Result<Self, InvalidLogFilter> {
        let raw = raw.as_ref().trim();
        if raw.is_empty() {
            return Err(InvalidLogFilter::Empty);
        }
        if raw.chars().any(char::is_control) {
            return Err(InvalidLogFilter::ControlCharacter);
        }
        Ok(Self(String::from(raw)))
    }

    #[inline]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl core::fmt::Display for LogFilter {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

/// The name every log line is attributed to.
///
/// A separate type because it is the field a collector groups by, so an empty or whitespace one
/// makes every line from this deployment unattributable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceName(String);

/// Why a string is not a service name.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvalidServiceName {
    #[error("telemetry.service_name is empty")]
    Empty,
    #[error("telemetry.service_name contains a character that is not a letter, digit, `-` or `_`")]
    NotAnIdentifier,
}

impl ServiceName {
    /// Reads a service name.
    ///
    /// Narrow on purpose: a name with a space or a quote in it has to be escaped by every
    /// consumer, and the one that forgets produces a log line that does not parse.
    pub fn parse(raw: impl AsRef<str>) -> Result<Self, InvalidServiceName> {
        let raw = raw.as_ref().trim();
        if raw.is_empty() {
            return Err(InvalidServiceName::Empty);
        }
        if !raw.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
            return Err(InvalidServiceName::NotAnIdentifier);
        }
        Ok(Self(String::from(raw)))
    }

    #[inline]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl core::fmt::Display for ServiceName {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Everything about the log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TelemetrySettings {
    service_name: ServiceName,
    filter: LogFilter,
    format: LogFormat,
    /// Whether [`Self::format`] was written down or derived from the environment.
    ///
    /// Recorded rather than recomputed, because the startup log says which happened and the
    /// derivation is not reversible once the value is stored: `bunyan` in production looks
    /// identical whether somebody chose it or nobody did.
    format_was_explicit: bool,
}

impl TelemetrySettings {
    #[inline]
    pub const fn new(service_name: ServiceName, filter: LogFilter, format: LogFormat, format_was_explicit: bool) -> Self {
        Self {
            service_name,
            filter,
            format,
            format_was_explicit,
        }
    }

    #[inline]
    pub const fn service_name(&self) -> &ServiceName {
        &self.service_name
    }

    #[inline]
    pub const fn filter(&self) -> &LogFilter {
        &self.filter
    }

    #[inline]
    pub const fn format(&self) -> LogFormat {
        self.format
    }

    #[inline]
    pub const fn format_was_explicit(&self) -> bool {
        self.format_was_explicit
    }
}

#[cfg(test)]
mod tests {
    use super::{InvalidLogFilter, InvalidServiceName, LogFilter, LogFormat, ServiceName};
    use crate::Environment;

    #[test]
    fn production_logs_json_and_everything_else_logs_for_a_person() {
        // The split, asserted rather than described. A fourth environment added to the enum
        // makes the match in `default_for` non-exhaustive, so it cannot inherit either answer.
        assert_eq!(LogFormat::default_for(Environment::Production), LogFormat::Bunyan);
        assert_eq!(LogFormat::default_for(Environment::Development), LogFormat::Pretty);
        assert_eq!(LogFormat::default_for(Environment::Test), LogFormat::Pretty);
    }

    #[test]
    fn both_formats_have_a_friendly_spelling_and_an_unknown_one_is_an_error() {
        assert_eq!(LogFormat::parse("json"), Ok(LogFormat::Bunyan));
        assert_eq!(LogFormat::parse("HUMAN"), Ok(LogFormat::Pretty));
        for name in LogFormat::NAMES {
            assert!(LogFormat::parse(name).is_ok(), "{name}");
        }
        let error = LogFormat::parse("logfmt").expect_err("an unsupported format is an error");
        assert!(error.to_string().contains("bunyan"), "{error}");
    }

    #[test]
    fn an_empty_filter_is_refused_rather_than_meaning_no_directives() {
        assert_eq!(LogFilter::parse("   "), Err(InvalidLogFilter::Empty));
        assert_eq!(LogFilter::parse("info\nwarn"), Err(InvalidLogFilter::ControlCharacter));
        assert_eq!(
            LogFilter::parse(" sutura_http=debug,info ")
                .expect("a directive is a directive")
                .as_str(),
            "sutura_http=debug,info"
        );
    }

    #[test]
    fn a_service_name_is_an_identifier() {
        assert_eq!(
            ServiceName::parse("sutura-http").expect("a hyphenated name").as_str(),
            "sutura-http"
        );
        assert_eq!(ServiceName::parse(""), Err(InvalidServiceName::Empty));
        assert_eq!(ServiceName::parse("sutura service"), Err(InvalidServiceName::NotAnIdentifier));
    }
}
