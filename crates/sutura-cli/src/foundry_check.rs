//! `sutura foundry-check`: the configured Foundry catalogs and sources, read through the same
//! compositions `serve` opens and checked one step at a time, one line per step.
//!
//! Every line is `<subject>: ok|refused|failed - <reason>`, for a tester to paste into a public
//! issue. **What a line may carry**: a count, an HTTP status, Foundry's `errorName`, an OAuth 2.0
//! error code, a scope, an object type's API name and the model name it maps to, a configured
//! alias. **What it never carries**: a token, a host, an ontology identifier, request or answer
//! text, a row value. Held by construction - a reason is the top-level `Display` of this
//! workspace's own error, whose fields are those kinds, plus a [`TransportFault`] in place of a
//! transport error's own text, which can name the host - and by `tests`, which look for a token
//! and the host in the output. **The limit:** a new field on one of those errors reaches the
//! output unreviewed, and only a field the tests provoke is looked for.
//!
//! `refused` is the far side, or this deployment's own grammar, saying no; `failed` is no answer.
//! The exit code is non-zero unless every line is `ok`.

use std::cell::Cell;
use std::process::ExitCode;

use sutura_catalog_foundry::document::{ObjectType, Ontology};
use sutura_catalog_foundry::http::{HttpOntologyReader, HttpReaderError, TokenSource, TokenUnavailable, TransportFault};
use sutura_catalog_foundry::{FoundryCatalog, OntologyReader};
use sutura_domain::identity::{DelegationFailed, Presented, Secret};
use sutura_domain::model::{ModelName, QualifiedTable, SourceName};
use sutura_domain::pinned::SemanticCatalog as _;
use sutura_domain::source::SourcePosture;
use sutura_domain::warehouse::Warehouse as _;
use sutura_domain::warehouse::deadline::{Budget, Deadline};
use sutura_exec_foundry::http::HttpFoundryError;
use sutura_http_client::delegation::{BrokeredLogin, StoredAnswer};

use crate::foundry::FoundrySource;

/// The variable a caller's own access token may be given in, when no file is.
pub(crate) const CALLER_TOKEN_VARIABLE: &str = "SUTURA_CALLER_TOKEN";

/// How long a parse-only request may take.
const PARSE_ONLY_SECONDS: u64 = 30;

/// How one step came out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Verdict {
    Ok,
    Refused,
    Failed,
}

impl Verdict {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Refused => "refused",
            Self::Failed => "failed",
        }
    }
}

/// One step: what was checked, how it came out, and why.
#[derive(Debug)]
struct Line {
    subject: String,
    verdict: Verdict,
    reason: String,
}

/// Every step, in the order it ran.
#[derive(Debug, Default)]
pub(crate) struct Report(Vec<Line>);

impl Report {
    fn push(&mut self, subject: impl Into<String>, verdict: Verdict, reason: impl Into<String>) {
        self.0.push(Line {
            subject: subject.into(),
            verdict,
            reason: reason.into(),
        });
    }

    /// Whether every step is `ok`.
    pub(crate) fn passed(&self) -> bool {
        !self.0.is_empty() && self.0.iter().all(|line| line.verdict == Verdict::Ok)
    }

    pub(crate) fn render(&self) -> String {
        self.to_string()
    }
}

impl core::fmt::Display for Report {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        for line in &self.0 {
            writeln!(f, "{}: {} - {}", line.subject, line.verdict.as_str(), line.reason)?;
        }
        Ok(())
    }
}

/// The object types one catalog listed, by the alias a source of the same name answers to.
type Probed = (SourceName, Vec<Probe>);

/// An object type a source's parse-only step probes: its API name and the table it names.
#[derive(Debug, Clone)]
pub(crate) struct Probe {
    api_name: String,
    table: QualifiedTable,
}

impl Probe {
    const fn dotted(&self) -> bool {
        self.table.object_path().is_some()
    }
}

/// The command: the configuration `serve` reads, then each Foundry catalog and source.
pub(crate) fn foundry_check(args: &[String]) -> ExitCode {
    let mut report = Report::default();
    match crate::sources::configured() {
        Ok(settings) => match caller_token(args.first()) {
            Ok(caller) => check_configured(&settings, caller.as_ref(), &mut report),
            Err(reason) => report.push("caller token", Verdict::Failed, reason),
        },
        Err(_unprinted) => report.push(
            "configuration",
            Verdict::Failed,
            "the settings did not load; `sutura serve` prints why, with values this output leaves out",
        ),
    }
    print!("{}", report.render());
    if report.passed() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// The caller's own access token: the file `path` names, else [`CALLER_TOKEN_VARIABLE`], else none.
fn caller_token(path: Option<&String>) -> Result<Option<Secret>, &'static str> {
    if let Some(path) = path {
        return crate::password_file::read_key("caller-token-file", std::path::Path::new(path))
            .map(Some)
            .map_err(|_unprinted| "the caller token file is not readable, is empty, or is over 64 KiB");
    }
    Ok(std::env::var(CALLER_TOKEN_VARIABLE)
        .ok()
        .map(|token| String::from(token.trim()))
        .filter(|token| !token.is_empty())
        .map(Secret::new))
}

/// Every `foundry` catalog, then every `foundry` source, as `settings` declares them.
pub(crate) fn check_configured(settings: &sutura_config::Settings, caller: Option<&Secret>, report: &mut Report) {
    let outbound = match crate::serve::outbound::resolve(settings) {
        Ok(outbound) => outbound,
        Err(_unprinted) => {
            report.push(
                "security.outbound",
                Verdict::Failed,
                "the declared transport anchors did not load; `sutura serve` prints why",
            );
            return;
        }
    };
    let mut probes: Vec<Probed> = Vec::new();
    for catalog in settings
        .catalogs()
        .each()
        .filter(|catalog| catalog.kind() == sutura_config::CatalogKind::Foundry)
    {
        let subject = format!("catalog {}", catalog.name());
        match crate::catalog::foundry::reader(catalog, outbound.as_ref()) {
            Ok(reader) => probes.push((catalog.name().clone(), check_catalog(&subject, catalog, &reader, report))),
            Err(_unprinted) => report.push(
                format!("{subject} credential"),
                Verdict::Failed,
                "the entry could not be composed - an unreadable token or client secret file, or an unusable value; `sutura serve` prints which",
            ),
        }
    }
    let mut declared = settings
        .catalogs()
        .each()
        .filter(|catalog| catalog.kind() == sutura_config::CatalogKind::Foundry)
        .count();
    for (alias, configured) in settings.sources().each() {
        if configured.kind() != sutura_config::SourceKind::Foundry {
            continue;
        }
        declared += 1;
        let probing = probes
            .iter()
            .find(|(name, _)| name == alias)
            .map_or(&[][..], |(_, probes)| probes.as_slice());
        let source = crate::foundry::build(
            alias,
            configured,
            outbound.as_ref(),
            crate::sources::working_set(settings.runtime()),
        );
        let login = match configured.caller_identity() {
            Some(sutura_config::sources::caller_identity::CallerIdentity::FoundryLogin(login)) => Some(
                crate::serve::broker::linked_login(&format!("sources.{alias}.foundry_login"), login, outbound.as_ref()),
            ),
            _ => None,
        };
        let subject = format!("source {alias}");
        let minted = matches!(
            *configured.placement(),
            sutura_config::SourcePlacement::Foundry {
                client_credentials: Some(_),
                ..
            }
        );
        let checking = Checking {
            subject: &subject,
            caller,
            probes: probing,
            minted,
        };
        match (source, login.transpose()) {
            (Ok(source), Ok(login)) => check_source(&checking, &source, login.as_ref(), report),
            (Err(_unprinted), _) | (_, Err(_unprinted)) => report.push(
                format!("{subject} credential"),
                Verdict::Failed,
                "the entry could not be composed - an unreadable token or client secret file, or an unusable value; `sutura serve` prints which",
            ),
        }
    }
    if declared == 0 {
        report.push(
            "foundry",
            Verdict::Failed,
            "the configuration declares no `foundry` catalog and no `foundry` source",
        );
    }
}

/// The credential step: one bearer from `token` within `within`, obtained and dropped - a fixed
/// token as it is, or one client-credentials mint through the workspace's one grant. Whether a
/// bearer came back.
pub(crate) fn check_credential(subject: &str, token: &TokenSource, within: core::time::Duration, report: &mut Report) -> bool {
    let (verdict, reason) = match token.bearer(sutura_http_client::Budget::opened(within)) {
        Ok(_bearer) => (
            Verdict::Ok,
            String::from(match token {
                TokenSource::Fixed(_) => "the token file was read",
                TokenSource::ClientCredentials(_) => "a client-credentials token was minted",
            }),
        ),
        Err(cause) => token_reason(&cause),
    };
    report.push(format!("{subject} credential"), verdict, reason);
    verdict == Verdict::Ok
}

/// One catalog's steps; the object types a source of the same alias may probe.
pub(crate) fn check_catalog(
    subject: &str,
    catalog: &sutura_config::CatalogSettings,
    reader: &HttpOntologyReader,
    report: &mut Report,
) -> Vec<Probe> {
    if !check_credential(subject, reader.token(), reader.timeout(), report) {
        return Vec::new();
    }
    let object_types = match reader.object_types() {
        Ok(object_types) => object_types,
        Err(cause) => {
            let (verdict, reason) = reader_reason(&cause);
            report.push(format!("{subject} object types"), verdict, reason);
            return Vec::new();
        }
    };
    let probes = mapped(subject, &object_types, report);
    let mut outgoing = std::collections::BTreeMap::new();
    let mut read: Vec<(String, usize)> = Vec::new();
    let mut unread = 0_usize;
    for object_type in &object_types {
        let api_name = object_type.api_name();
        match reader.outgoing_link_types(api_name) {
            Ok(sides) => {
                read.push((String::from(api_name), sides.len()));
                outgoing.insert(String::from(api_name), sides);
            }
            Err(cause) => {
                unread += 1;
                let (verdict, reason) = reader_reason(&cause);
                report.push(format!("{subject} links {api_name}"), verdict, reason);
            }
        }
    }
    let load = loaded(catalog, Ontology::new(object_types, outgoing));
    let carried = load.as_ref().ok().map(|(carried, _)| carried);
    for (api_name, sides) in read {
        let reason = carried.map_or_else(
            || format!("{sides} link side(s) read; the load was refused, so no join is counted"),
            |carried| {
                let joins = sutura_catalog_foundry::model_name(&api_name)
                    .ok()
                    .and_then(|(model, _)| carried.get(&model).copied())
                    .unwrap_or_default();
                format!(
                    "{sides} link side(s) read, {joins} join(s) carried, {} omitted",
                    sides.saturating_sub(joins)
                )
            },
        );
        report.push(format!("{subject} links {api_name}"), Verdict::Ok, reason);
    }
    match load {
        Ok((_, reason)) if unread > 0 => report.push(
            format!("{subject} load"),
            Verdict::Failed,
            format!("{reason} without the {unread} link list(s) that were not read; `serve` refuses the whole load"),
        ),
        Ok((_, reason)) => report.push(format!("{subject} load"), Verdict::Ok, reason),
        Err(reason) => report.push(format!("{subject} load"), Verdict::Refused, reason),
    }
    probes
}

/// The object-type step: the count, each dotted API name and the model name it maps to, and each
/// one the mapping refuses. The probes are the first plain and the first dotted object type.
fn mapped(subject: &str, object_types: &[ObjectType], report: &mut Report) -> Vec<Probe> {
    let mut dotted = 0_usize;
    let mut lines = Report::default();
    let mut plain_probe = None;
    let mut dotted_probe = None;
    for object_type in object_types {
        let api_name = object_type.api_name();
        match sutura_catalog_foundry::model_name(api_name) {
            Ok((model, table)) => {
                let probe = Probe {
                    api_name: String::from(api_name),
                    table,
                };
                if probe.dotted() {
                    dotted += 1;
                    lines.push(format!("{subject} mapped {api_name}"), Verdict::Ok, format!("model {model}"));
                    dotted_probe.get_or_insert(probe);
                } else {
                    plain_probe.get_or_insert(probe);
                }
            }
            Err(cause) => lines.push(format!("{subject} mapped {api_name}"), Verdict::Refused, chain(&cause)),
        }
    }
    report.push(
        format!("{subject} object types"),
        Verdict::Ok,
        format!("{} listed, {dotted} dotted API name(s) mapped", object_types.len()),
    );
    report.0.extend(lines.0);
    plain_probe.into_iter().chain(dotted_probe).collect()
}

/// An ontology already read, handed to the catalog's own load once.
struct AlreadyRead(Cell<Option<Ontology>>);

impl core::fmt::Debug for AlreadyRead {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("AlreadyRead")
    }
}

impl OntologyReader for AlreadyRead {
    type Error = std::convert::Infallible;

    fn read(&self) -> Result<Ontology, Self::Error> {
        Ok(self.0.take().unwrap_or_default())
    }
}

/// The joins each model carries.
type Carried = std::collections::BTreeMap<ModelName, usize>;

/// The joins each model carries and the load's line, or the refusal's line.
type Load = Result<(Carried, String), String>;

/// The catalog's own load over what was read: the joins it carries per model and the load's line,
/// or its refusal's.
fn loaded(catalog: &sutura_config::CatalogSettings, ontology: Ontology) -> Load {
    let loading = FoundryCatalog::new(
        catalog.name().clone(),
        catalog.version().clone(),
        catalog.name().clone(),
        AlreadyRead(Cell::new(Some(ontology))),
    );
    match loading.load() {
        Ok(pinned) => {
            let definitions = pinned.definitions();
            let mut carried = Carried::new();
            for relationship in definitions.relationships().values() {
                *carried.entry(relationship.origin_model().clone()).or_insert(0_usize) += 1;
            }
            let line = format!(
                "{} model(s), {} join(s)",
                definitions.models().len(),
                definitions.relationships().len()
            );
            Ok((carried, line))
        }
        Err(cause) => Err(chain(&cause)),
    }
}

/// What one source's steps are checked with, beside the source itself.
pub(crate) struct Checking<'a> {
    subject: &'a str,
    caller: Option<&'a Secret>,
    probes: &'a [Probe],
    /// Whether a shared source's own token is a client-credentials mint rather than a file.
    minted: bool,
}

/// One source's steps: a shared source's own token, or the broker answer for a `foundry_login`
/// source; then a parse-only request per probe under the token the source sends.
pub(crate) fn check_source(checking: &Checking<'_>, source: &FoundrySource, login: Option<&BrokeredLogin>, report: &mut Report) {
    let &Checking {
        subject,
        caller,
        probes,
        minted,
    } = checking;
    let presented = match (source.posture(), login) {
        (SourcePosture::SharedServiceUser { declared }, _) => {
            let (verdict, reason) = match source.deployment_token(parse_only_deadline()) {
                Some(Ok(())) if minted => (Verdict::Ok, String::from("a client-credentials token was minted")),
                Some(Ok(())) => (Verdict::Ok, String::from("the token file was read")),
                Some(Err(cause)) => token_failed_reason(&cause),
                None => (
                    Verdict::Failed,
                    String::from("a shared source holding no token of its own, which composition should have refused"),
                ),
            };
            report.push(format!("{subject} credential"), verdict, reason);
            if verdict != Verdict::Ok {
                return;
            }
            Presented::SharedServiceUser {
                declared: declared.clone(),
            }
        }
        (SourcePosture::ImpersonationAtSource, None) => {
            report.push(
                format!("{subject} broker"),
                Verdict::Failed,
                "an impersonating source with no `foundry_login`, which the settings should have refused",
            );
            return;
        }
        (SourcePosture::ImpersonationAtSource, Some(login)) => {
            let Some(caller) = caller else {
                report.push(
                    format!("{subject} broker"),
                    Verdict::Refused,
                    format!("no caller token: name a file holding one, or set {CALLER_TOKEN_VARIABLE}"),
                );
                return;
            };
            match login.stored(caller) {
                Ok(answer) => match brokered(subject, answer, report) {
                    Some(material) => Presented::SubjectToken { material },
                    None => return,
                },
                Err(cause) => {
                    let (verdict, reason) = delegation_reason(&cause);
                    report.push(format!("{subject} broker"), verdict, reason);
                    return;
                }
            }
        }
    };
    if probes.is_empty() {
        report.push(
            format!("{subject} parse-only"),
            Verdict::Failed,
            "no object type to probe: no `foundry` catalog of this alias listed one",
        );
        return;
    }
    for probe in probes {
        let shape = if probe.dotted() {
            " (dotted, one quoted identifier)"
        } else {
            ""
        };
        let line = format!("{subject} parse-only {}{shape}", probe.api_name);
        match source.parse_object(&probe.table, &presented, parse_only_deadline()) {
            Ok(()) => report.push(line, Verdict::Ok, "Foundry parsed the statement"),
            Err(cause) => {
                let (verdict, reason) = source_reason(&cause);
                report.push(line, verdict, reason);
            }
        }
    }
}

/// The broker step's line, and the token when `serve` would use it.
fn brokered(subject: &str, answer: StoredAnswer, report: &mut Report) -> Option<Secret> {
    let scope = answer.scope().map_or_else(
        || String::from("the answer names no scope"),
        |scope| format!("scope {}", scope.as_str()),
    );
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(u64::MAX, |since| since.as_secs());
    let expiry = answer.not_after_unix_seconds();
    match answer.delegated_at(now) {
        Ok(delegated) => {
            let left = expiry.map_or(0, |at| at.saturating_sub(now));
            report.push(
                format!("{subject} broker"),
                Verdict::Ok,
                format!("a linked Foundry token was returned; accessTokenExpiration present, {left} s left; {scope}"),
            );
            Some(delegated.into_token())
        }
        Err(cause) => {
            let why = match cause {
                DelegationFailed::AlreadyExpired => "accessTokenExpiration has passed",
                _ => "accessTokenExpiration is absent",
            };
            report.push(
                format!("{subject} broker"),
                Verdict::Refused,
                format!("a linked Foundry token was returned, and {why}, so `serve` refuses it; {scope}"),
            );
            None
        }
    }
}

fn parse_only_deadline() -> Deadline {
    #[expect(clippy::unreachable, reason = "the literal budget is non-zero by construction")]
    let budget = Budget::parse(core::time::Duration::from_secs(PARSE_ONLY_SECONDS))
        .unwrap_or_else(|_| unreachable!("a non-zero literal is a budget"));
    Deadline::opened_at(std::time::Instant::now(), budget)
}

/// An error and each cause beneath it, on one line. Only for this workspace's own catalog errors,
/// which quote API names and nothing read off the wire.
fn chain(error: &dyn core::error::Error) -> String {
    let mut out = error.to_string();
    let mut cursor = error.source();
    while let Some(cause) = cursor {
        out.push_str(": ");
        out.push_str(&cause.to_string());
        cursor = cause.source();
    }
    out
}

fn token_reason(cause: &TokenUnavailable) -> (Verdict, String) {
    match cause {
        TokenUnavailable::Unreachable(transport) => (
            Verdict::Failed,
            format!("{cause}: {}", TransportFault::of(transport).as_str()),
        ),
        TokenUnavailable::Refused { .. } => (Verdict::Refused, cause.to_string()),
        TokenUnavailable::DeadlineSpent
        | TokenUnavailable::TooLarge { .. }
        | TokenUnavailable::Malformed { .. }
        | TokenUnavailable::WrongTokenType => (Verdict::Failed, cause.to_string()),
    }
}

/// A deployment token's failure: the grant's own refusal when it is one, never its text otherwise.
fn token_failed_reason(cause: &sutura_exec_foundry::TokenFailed) -> (Verdict, String) {
    cause
        .0
        .downcast_ref::<TokenUnavailable>()
        .map_or_else(|| (Verdict::Failed, cause.to_string()), token_reason)
}

fn error_name(name: Option<&sutura_http_client::ErrorName>) -> String {
    name.map_or_else(
        || String::from(", no errorName"),
        |name| format!(", errorName {}", name.as_str()),
    )
}

fn reader_reason(cause: &HttpReaderError) -> (Verdict, String) {
    match cause {
        HttpReaderError::Token { cause: token, .. } => {
            let (verdict, reason) = token_reason(token);
            (verdict, format!("{cause}: {reason}"))
        }
        HttpReaderError::Unreachable { cause: transport, .. } | HttpReaderError::Unreadable { cause: transport, .. } => (
            Verdict::Failed,
            format!("{cause}: {}", TransportFault::of(transport).as_str()),
        ),
        HttpReaderError::Refused { error_name: name, .. } => (Verdict::Refused, format!("{cause}{}", error_name(name.as_ref()))),
        HttpReaderError::ObjectTypeName { .. } => (Verdict::Refused, chain(cause)),
        HttpReaderError::Paging { cause: paging, .. } => (Verdict::Failed, format!("{cause}: {paging}")),
        HttpReaderError::DeadlineSpent { .. }
        | HttpReaderError::TooLarge { .. }
        | HttpReaderError::Shape { .. }
        | HttpReaderError::PageTokenTooLong { .. } => (Verdict::Failed, cause.to_string()),
    }
}

fn delegation_reason(cause: &DelegationFailed) -> (Verdict, String) {
    match cause {
        DelegationFailed::Unreachable { cause: transport } => (
            Verdict::Failed,
            TransportFault::within(transport.as_ref())
                .map_or_else(|| cause.to_string(), |fault| format!("{cause}: {}", fault.as_str())),
        ),
        DelegationFailed::Refused { .. } | DelegationFailed::NotLinked => (Verdict::Refused, cause.to_string()),
        _ => (Verdict::Failed, cause.to_string()),
    }
}

fn source_reason(cause: &sutura_exec_foundry::FoundryError<HttpFoundryError>) -> (Verdict, String) {
    use sutura_exec_foundry::FoundryError;

    match cause {
        FoundryError::Endpoint { cause: endpoint } => match endpoint {
            HttpFoundryError::Refused { error_name: name, .. } => {
                (Verdict::Refused, format!("{endpoint}{}", error_name(name.as_ref())))
            }
            HttpFoundryError::Unreachable(transport) | HttpFoundryError::Unreadable(transport) => (
                Verdict::Failed,
                format!("{endpoint}: {}", TransportFault::of(transport).as_str()),
            ),
            HttpFoundryError::TooLarge { .. } | HttpFoundryError::DeadlineSpent | HttpFoundryError::Unencodable(_) => {
                (Verdict::Failed, endpoint.to_string())
            }
        },
        FoundryError::Render { cause: render } => (Verdict::Failed, format!("{cause}: {render}")),
        FoundryError::Token { cause: token } => {
            let (verdict, reason) = token_failed_reason(token);
            (verdict, format!("{cause}: {reason}"))
        }
        _ => (Verdict::Failed, cause.to_string()),
    }
}

#[cfg(test)]
mod tests;
