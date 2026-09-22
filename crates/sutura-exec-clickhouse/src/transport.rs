//! The seam this adapter's `execute`/`verify_anchor`/`declared_key` call through.
//!
//! [`Http`] is the one implementor a real connection uses, and the crate's own unit tests bind
//! the port to a CANNED implementor instead, over the exact same trait - the shape
//! `sutura-exec-bigquery`'s `JobTransport` already established for the identical reason (no live
//! server this repository can reach in every venue that runs the suite; see this crate's own
//! `lib.rs` header).
//!
//! [`ClickHouseTransport::run`] takes the RENDERED statement and its bound [`ParamValue`]s
//! untouched - never a rewritten string - so what each implementor does with them is its own
//! business. [`Http`] rewrites `?` into `ClickHouse`'s own named-parameter syntax
//! (`{p0:String}`, ...) and sends each value as a separate `param_p0=...` query field, which
//! `ClickHouse` binds server-side by the declared type: nothing here concatenates a value into the
//! statement text - see [`rewrite_placeholders`] for why counting `?` occurrences is safe rather
//! than a guess.

use std::time::Instant;

use sutura_domain::warehouse::ParamValue;
use sutura_domain::warehouse::deadline::Deadline;

/// What one [`ClickHouseTransport::run`] answers on success.
///
/// A `type` alias so the trait's own signature reads as one name rather than as the two-level
/// generic clippy's `type_complexity` lint asks not to repeat.
pub type RunResult<E> = Result<Vec<u8>, E>;

/// Where a statement runs, seen from this adapter's own port.
///
/// The `Warehouse` port itself is synchronous, and so is this: `ureq` is a blocking client and
/// needs no runtime of its own - see this crate's `Cargo.toml` for the reasoning `sutura-exec-
/// bigquery`'s `wire` entry states at length, which applies here unchanged.
pub trait ClickHouseTransport {
    /// Why the endpoint could not answer. `crate::ClickHouseError<Self::Error>` wraps it and
    /// never lets it reach a caller of the domain port raw.
    type Error: core::error::Error + Send + Sync + 'static;

    /// Runs one rendered statement and returns the response body, as
    /// `JSONCompactEachRowWithNamesAndTypes` bytes: a names row, a types row, then one row per
    /// result row - `crate::rows_from_json` is the shared decode both this trait's implementors
    /// answer into.
    fn run(&self, statement: &str, params: &[ParamValue], deadline: Deadline) -> RunResult<Self::Error>;

    /// Was this failure the endpoint refusing the statement at the identity/authorization level?
    /// `Warehouse::source_refused`'s delegate, one port further down - `Self::Error` is this
    /// implementor's own type, so only it can read the wire-level shape.
    ///
    /// Defaulted to `false`, the honest answer for an implementor that has no such distinction -
    /// see `sutura_domain::warehouse::Warehouse::source_refused`'s own doc for why the default is
    /// the safe direction.
    fn source_refused(&self, _error: &Self::Error) -> bool {
        false
    }

    /// Was this failure the `Deadline` running out, either found spent before the request was
    /// sent or the endpoint stopping it? `Warehouse::deadline_exceeded`'s delegate, for the same
    /// reason as [`Self::source_refused`].
    fn deadline_exceeded(&self, _error: &Self::Error) -> bool {
        false
    }
}

/// The response format every request asks for - a self-describing shape, so a result with no rows
/// still carries its projection (the same reason `sutura_exec_postgres`'s `prepare_and_query` reads
/// labels off the PREPARED statement rather than guessing).
const RESPONSE_FORMAT: &str = "JSONCompactEachRowWithNamesAndTypes";

/// A `ClickHouse` HTTP endpoint address.
#[derive(Debug, Clone)]
pub struct Endpoint {
    scheme: &'static str,
    host: String,
    port: u16,
}

impl Endpoint {
    /// A plaintext (`http://`) endpoint.
    #[must_use]
    pub fn plaintext(host: impl Into<String>, port: u16) -> Self {
        Self {
            scheme: "http",
            host: host.into(),
            port,
        }
    }

    /// A TLS-verified (`https://`) endpoint.
    #[must_use]
    pub fn tls(host: impl Into<String>, port: u16) -> Self {
        Self {
            scheme: "https",
            host: host.into(),
            port,
        }
    }

    fn base_url(&self) -> String {
        format!("{}://{}:{}/", self.scheme, self.host, self.port)
    }
}

/// The credential a `clickhouse` source presents over HTTP Basic authentication.
#[derive(Debug, Clone)]
pub struct BasicAuth {
    user: String,
    password: sutura_domain::identity::Secret,
}

impl BasicAuth {
    /// A user/password pair, as the deployment declared it.
    #[must_use]
    pub fn new(user: impl Into<String>, password: sutura_domain::identity::Secret) -> Self {
        Self {
            user: user.into(),
            password,
        }
    }
}

/// Why [`Http`] could not answer.
#[derive(Debug, thiserror::Error)]
pub enum HttpError {
    /// The endpoint could not be dialled, or its reply could not be read.
    #[error("the ClickHouse endpoint could not be reached")]
    Transport {
        #[source]
        cause: ureq::Error,
    },
    /// The endpoint answered with a non-2xx status.
    ///
    /// `message` is its response body, which `ClickHouse`'s own error text names its exception
    /// CODE inside (`Code: 159. DB::Exception: ...`) - see [`Http::deadline_exceeded`]/
    /// [`Http::source_refused`] for the two matches this adapter makes against it, and their own
    /// stated limit: neither is a typed code, because the driver hands back text and nothing more
    /// structured.
    #[error("the endpoint refused the statement with status {status}: {message}")]
    ServerRefused { status: u16, message: String },
    /// The rendered statement's own `?` count did not match the bound parameters - a defect in
    /// this crate's rewrite rather than in the plan; see [`rewrite_placeholders`].
    #[error("the rendered statement carried {found} placeholders, and {bound} parameters were bound")]
    PlaceholderMismatch { found: usize, bound: usize },
    /// The deadline was already spent before a request was ever sent.
    #[error("the deadline was already spent before a request could be sent")]
    DeadlineSpent,
    #[error("the endpoint's base URL could not be built")]
    InvalidEndpoint,
}

/// The real transport: one `ureq::Agent`, built once and kept for this adapter's life.
///
/// The same shape `sutura_exec_postgres::PostgresWarehouse` keeps its one connection in. A rotated
/// TLS config takes effect on the next agent a composition root builds, never on an agent already
/// standing - `ureq::Agent`'s configuration is fixed at construction, so this is not a choice made
/// here but the shape the client already has.
pub struct Http {
    endpoint: Endpoint,
    agent: ureq::Agent,
    auth: Option<BasicAuth>,
}

impl core::fmt::Debug for Http {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Http")
            .field("endpoint", &self.endpoint)
            .finish_non_exhaustive()
    }
}

/// The header this adapter reads no further than this many bytes of, over the wire - a bound on a
/// hostile or misconfigured endpoint's reply headers, the same shape `sutura-exec-bigquery`'s wire
/// carries for its own agent.
const MAX_HEADER_BYTES: usize = 64 * 1024;

impl Http {
    /// Opens the transport over `endpoint` with no TLS at all - a `plaintext` channel, or the
    /// fixture tier's loopback path.
    #[must_use]
    pub fn connect(endpoint: Endpoint, auth: Option<BasicAuth>) -> Self {
        let agent = ureq::Agent::new_with_config(base_config().build());
        Self { endpoint, agent, auth }
    }

    /// Opens the transport secured as the caller resolved: `tls` is the `ureq::tls::TlsConfig`
    /// `crate::tls::config` built from the declared channel. Both this and [`Self::connect`] are
    /// produced by the composition root, which is the only place that can see the declared
    /// `sutura_config::sources::transport::SourceTransport` - the same boundary
    /// `PostgresWarehouse::connect_secured`'s own signature draws.
    #[must_use]
    pub fn connect_secured(endpoint: Endpoint, auth: Option<BasicAuth>, tls: ureq::tls::TlsConfig) -> Self {
        let agent = ureq::Agent::new_with_config(base_config().tls_config(tls).build());
        Self { endpoint, agent, auth }
    }

    fn authorization(&self) -> Option<String> {
        self.auth.as_ref().map(|auth| {
            use base64::Engine as _;
            #[expect(
                clippy::disallowed_methods,
                reason = "the credential's destination is the Authorization header of the one request \
                          this adapter makes, which is the one place the value itself is the payload"
            )]
            let raw = format!("{}:{}", auth.user, auth.password.expose_secret());
            format!("Basic {}", base64::engine::general_purpose::STANDARD.encode(raw))
        })
    }
}

fn base_config() -> ureq::config::ConfigBuilder<ureq::typestate::AgentScope> {
    ureq::Agent::config_builder()
        .http_status_as_error(false)
        .max_redirects(0)
        .max_response_header_size(MAX_HEADER_BYTES)
        .proxy(ureq::Proxy::try_from_env())
}

impl ClickHouseTransport for Http {
    type Error = HttpError;

    fn run(&self, statement: &str, params: &[ParamValue], deadline: Deadline) -> RunResult<Self::Error> {
        crate::deadline::refuse_if_spent(deadline)?;
        let rewritten = rewrite_placeholders(statement, params)?;
        let sent = format!("{rewritten} FORMAT {RESPONSE_FORMAT}");
        let mut url = url::Url::parse(&self.endpoint.base_url()).map_err(|_cause| HttpError::InvalidEndpoint)?;
        {
            let mut query = url.query_pairs_mut();
            for (index, param) in params.iter().enumerate() {
                query.append_pair(&format!("param_p{index}"), &param_text(param));
            }
            for (setting, value) in request_settings(deadline, Instant::now()) {
                query.append_pair(setting, &value);
            }
        }
        let mut request = self.agent.post(url.as_str());
        if let Some(header) = self.authorization() {
            request = request.header("Authorization", header);
        }
        let mut response = request.send(&sent).map_err(|cause| HttpError::Transport { cause })?;
        let status = response.status();
        let body = response
            .body_mut()
            .read_to_vec()
            .map_err(|cause| HttpError::Transport { cause })?;
        if status.is_success() {
            Ok(body)
        } else {
            let message = String::from_utf8_lossy(&body).into_owned();
            Err(HttpError::ServerRefused {
                status: status.as_u16(),
                message,
            })
        }
    }

    /// `Code: 497` is `NOT_ENOUGH_PRIVILEGES`, `Code: 516` is `AUTHENTICATION_FAILED` - the two
    /// server refusals this adapter classifies as *this identity may not ask* rather than a
    /// transport hiccup, the same split `sutura_exec_postgres::raw::source_refused` draws for its
    /// own two `SqlState` codes.
    fn source_refused(&self, error: &Self::Error) -> bool {
        matches!(
            *error,
            HttpError::ServerRefused { ref message, .. }
                if message.contains("Code: 497") || message.contains("Code: 516")
        )
    }

    fn deadline_exceeded(&self, error: &Self::Error) -> bool {
        match *error {
            HttpError::DeadlineSpent => true,
            HttpError::ServerRefused { ref message, .. } => message.contains("Code: 159"),
            _ => false,
        }
    }
}

impl From<crate::deadline::DeadlineSpent> for HttpError {
    fn from(_: crate::deadline::DeadlineSpent) -> Self {
        Self::DeadlineSpent
    }
}

/// The setting that makes the unmatched side of an outer join answer `NULL`.
///
/// **`ClickHouse`'s default is `join_use_nulls = 0`, and under it this adapter returns a different
/// ANSWER from the engine.** The unmatched side of a `LEFT JOIN` comes back as the column type's
/// default - `''` for a `String` - rather than as `NULL`, so a dimension value that is missing is
/// reported as empty, groups under a different key and sorts to a different place. Every statement
/// `sutura_sql` renders for `Dialect::ClickHouse` is accepted either way, which is why the render
/// goldens could not see it.
///
/// **Measured, 2026-09-22, against `clickhouse-server:26.7` (server 26.7.6.57)**, the tag
/// `compose.services.yaml` pins: the committed `@clickhouse` `sql`/`params` goldens were replayed
/// over the example corpus and their rows compared against the committed `@duckdb` row goldens.
/// Of the 23 questions whose goldens carry bound parameters, 17 agreed and **6 did not** - every
/// one of them an outer join against a dimension with an unmatched fact row. With this setting
/// sent, all 23 agree cell for cell.
///
/// **The limit, next to the claim: no leg of `just validate` executes this.** There is no
/// `nix/clickhouse-tier.nix` and the nix sandbox has no docker socket, so what stands behind the
/// paragraph above is one hand measurement, not a venue a gate reaches -
/// `github.com/telekom/sutura#920` is that gap and this is not its closure.
const JOIN_USE_NULLS: &str = "join_use_nulls";

/// What every request carries beyond the caller's own bound parameters.
///
/// A value rather than two `append_pair` calls inside [`ClickHouseTransport::run`], because the
/// settings ARE part of this adapter's contract with the server - [`JOIN_USE_NULLS`] decides what
/// the rows say - and a contract written inline is one no cell can read back.
///
/// `max_execution_time` is absent rather than `0` when nothing is left: `ClickHouse` reads a zero
/// as *no limit*, so omitting the pair is the only safe spelling of *there is no time* - see
/// [`crate::deadline`]'s own header for why that path is already refused before this is asked.
fn request_settings(deadline: Deadline, now: Instant) -> Vec<(&'static str, String)> {
    let mut settings = vec![(JOIN_USE_NULLS, String::from("1"))];
    if let Some(seconds) = crate::deadline::max_execution_time_seconds(deadline, now) {
        settings.push(("max_execution_time", seconds.to_string()));
    }
    settings
}

/// The `ClickHouse`-side parameter type each [`ParamValue`] declares itself as, in `{name:Type}`
/// syntax.
const fn param_type(param: &ParamValue) -> &'static str {
    match *param {
        ParamValue::Text(_) => "String",
        // Bound as text, the same way `sutura_exec_duckdb` and `sutura_exec_postgres::PgDate`'s
        // ISO rendering both bind a date - `ClickHouse`'s own `Date` parameter parser accepts the
        // same `YYYY-MM-DD` text a domain `Date::to_iso` produces.
        ParamValue::Date(_) => "Date",
    }
}

/// The value text this adapter hands `ClickHouse` for one bound parameter - never SQL, because it
/// travels as a separate `param_pN` field the server binds by the DECLARED type, not as text
/// concatenated into the statement.
fn param_text(param: &ParamValue) -> String {
    match *param {
        ParamValue::Text(ref v) => v.clone(),
        ParamValue::Date(d) => d.to_iso(),
    }
}

/// Rewrites every `?` the renderer left, in order, into `ClickHouse`'s own named-parameter syntax.
///
/// **Safe over a bare count, and here is why that is enough.** `sutura_sql`'s generator is what
/// renders `?` in the first place, for `Dialect::ClickHouse`'s `PlaceholderStyle::Question` - and
/// the domain's own [`ParamValue`] exists precisely so no caller-influenced text ever reaches the
/// rendered statement as a literal: everything a caller could shape binds as a parameter instead.
/// So a `?` in this string is a placeholder position and never a literal character, and counting
/// them against `params.len()` is a defensive equality check on a renderer invariant, not a guess.
fn rewrite_placeholders(statement: &str, params: &[ParamValue]) -> Result<String, HttpError> {
    let found = statement.matches('?').count();
    if found != params.len() {
        return Err(HttpError::PlaceholderMismatch {
            found,
            bound: params.len(),
        });
    }
    let mut out = String::with_capacity(statement.len() + params.len() * 12);
    // `next_back` pops the one part after the last `?` with no trailing placeholder, so the loop
    // below can pair every OTHER part with its parameter by position - a `?`-count already equal
    // to `params.len()` guarantees the zip below is not short on either side.
    let mut parts = statement.split('?').collect::<Vec<_>>().into_iter();
    let Some(tail) = parts.next_back() else {
        return Ok(out);
    };
    for (index, (part, param)) in parts.zip(params.iter()).enumerate() {
        out.push_str(part);
        out.push_str("{p");
        out.push_str(&index.to_string());
        out.push(':');
        out.push_str(param_type(param));
        out.push('}');
    }
    out.push_str(tail);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use core::time::Duration;

    use sutura_domain::calendar::Date;
    use sutura_domain::warehouse::deadline::Budget;

    use super::*;

    /// A deadline with `seconds` left, opened now.
    fn deadline_of(seconds: u64) -> (Deadline, Instant) {
        let budget = Budget::parse(Duration::from_secs(seconds)).unwrap();
        let opened = Instant::now();
        (Deadline::opened_at(opened, budget), opened)
    }

    #[test]
    fn every_request_asks_the_server_for_standard_outer_join_nulls() {
        // Without this, an unmatched LEFT JOIN row answers `''` rather than `NULL` and six of the
        // corpus's questions report a different dimension value than the engine does - measured,
        // see `JOIN_USE_NULLS`. A statement is ACCEPTED either way, so the render goldens and the
        // parse check are both green over it.
        let (deadline, now) = deadline_of(30);
        let settings = request_settings(deadline, now);
        assert!(
            settings.contains(&(JOIN_USE_NULLS, String::from("1"))),
            "the request carried {settings:?}"
        );
    }

    #[test]
    fn a_request_carries_what_is_left_of_the_deadline_as_max_execution_time() {
        let (deadline, now) = deadline_of(30);
        let settings = request_settings(deadline, now);
        assert!(
            settings.contains(&("max_execution_time", String::from("30"))),
            "the request carried {settings:?}"
        );
    }

    #[test]
    fn a_spent_deadline_sends_no_execution_ceiling_rather_than_a_zero_one() {
        // `ClickHouse` reads `max_execution_time = 0` as NO limit, so the absent pair and the zero
        // are opposites. `run` refuses a spent deadline before it gets here; this holds the other
        // half, that the assembly itself never spells the unbounded value.
        let (deadline, opened) = deadline_of(1);
        let settings = request_settings(deadline, opened + Duration::from_secs(2));
        assert_eq!(settings, vec![(JOIN_USE_NULLS, String::from("1"))]);
    }

    #[test]
    #[expect(
        clippy::literal_string_with_formatting_args,
        reason = "the braces are ClickHouse's own named-parameter syntax in a plain expected literal, not an \
                  unformatted `format!` call"
    )]
    fn every_question_mark_becomes_a_named_typed_placeholder() {
        let params = [
            ParamValue::Text(String::from("north")),
            ParamValue::Date(Date::from_days_since_epoch(0).unwrap()),
        ];
        let rewritten = rewrite_placeholders("SELECT * FROM t WHERE region = ? AND day >= ?", &params).unwrap();
        assert_eq!(rewritten, "SELECT * FROM t WHERE region = {p0:String} AND day >= {p1:Date}");
    }

    #[test]
    fn a_placeholder_count_mismatch_is_refused() {
        let params = [ParamValue::Text(String::from("north"))];
        assert!(matches!(
            rewrite_placeholders("SELECT ? , ?", &params),
            Err(HttpError::PlaceholderMismatch { found: 2, bound: 1 })
        ));
    }

    #[test]
    fn a_server_timeout_exception_is_recognised_by_its_code() {
        let error = HttpError::ServerRefused {
            status: 500,
            message: String::from("Code: 159. DB::Exception: Timeout exceeded: elapsed 30.1 seconds, maximum: 30"),
        };
        let http = Http::connect(Endpoint::plaintext("localhost", 8123), None);
        assert!(http.deadline_exceeded(&error));
    }

    #[test]
    fn an_unrelated_server_refusal_is_not_read_as_a_deadline_or_a_refusal() {
        let error = HttpError::ServerRefused {
            status: 404,
            message: String::from("Code: 60. DB::Exception: Table does not exist"),
        };
        let http = Http::connect(Endpoint::plaintext("localhost", 8123), None);
        assert!(!http.deadline_exceeded(&error));
        assert!(!http.source_refused(&error));
    }

    #[test]
    fn a_permission_refusal_is_recognised_by_its_code() {
        let error = HttpError::ServerRefused {
            status: 403,
            message: String::from("Code: 497. DB::Exception: Not enough privileges"),
        };
        let http = Http::connect(Endpoint::plaintext("localhost", 8123), None);
        assert!(http.source_refused(&error));
    }
}
