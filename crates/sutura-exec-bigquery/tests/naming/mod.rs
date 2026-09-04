//! Per-run table naming: the whole of the re-entrancy fix, and the one part of it with no endpoint.
//!
//! **Why it is a module of its own rather than more of `corpus.rs`.** Two runs pointed at one
//! dataset used to create and replace each other's fixture tables; what stops them is that every
//! table a run writes carries a token unique to that run. That mechanism is a pure function of a
//! bundle, a token and a leg name - no network, no filesystem, no environment except the one
//! `std::env` read in [`run_token`] - which is why it is testable without a project, and why the
//! deterministic tests in `corpus.rs` can hold it.
//!
//! In `tests/naming/mod.rs` rather than `tests/naming.rs` so cargo does not build it as a test
//! target of its own - the reason `tests/support/mod.rs` gives. Unlike `support`, only the corpus
//! leg declares it: `dead_code` is `deny` in the workspace lint table, and `tests/acceptance.rs`
//! names its one table from the developer's own environment rather than deriving it, so an item
//! here would be dead in that target.
//!
//! **Every `#[test]` over this module stays in `corpus.rs`.** `.agents/skills/sutura/gates` states
//! the reason as a rule: `just causality` reverts a file that added no test and keeps one that did,
//! so moving assertions out of a file turns it revertible and orphans the module they moved into.
//! What moved here is the harness.
//!
//! **The token is safe in a public log**, which is what lets a run print its table names as it
//! loads them: a name is a committed fixture name plus a token plus a leg, and only the dataset and
//! the project are resources.

use sutura_domain::catalog::{Definitions, Description, Metric, Model, Relationship};
use sutura_domain::model::{InvalidIdentifier, TableName};
use sutura_domain::pinned::PinnedDefinitions;

/// A token unique to this RUN of the leg, so two runs never share a fixture table.
///
/// **`GITHUB_RUN_ID` under GitHub Actions, a clock+pid value otherwise.** The issue this solves is a
/// race between two runs against one dataset: two CI pull requests, or a developer's shell beside a
/// CI run, both pointing at one shared dataset. The run id is the value CI already has, and it is
/// what the printed table names should show so a log says WHICH run wrote them. Locally there is no
/// run id, so the token is derived from the clock and the process id.
///
/// The env pair's decision is [`ci_run_id`] and the join is [`build_token`], both pure, because the
/// two claims that matter - two runs differ, and CI differs from local - cannot be asserted through
/// `std::env` without a test's result depending on the other tests in its process.
pub(crate) fn run_token() -> String {
    let flag = std::env::var("GITHUB_ACTIONS").ok();
    let id = std::env::var("GITHUB_RUN_ID").ok();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let ci = ci_run_id(flag.as_deref(), id.as_deref());
    build_token(ci.as_deref(), nanos, std::process::id())
}

/// Which run id GitHub Actions supplies, or none.
///
/// **Both halves are required, and the run id alone is not proof of CI.** A stale `GITHUB_RUN_ID`
/// exported in a developer's shell would otherwise collapse two local runs onto one token and
/// reintroduce the very race the suffix removes. `GITHUB_ACTIONS` is `"true"` in every GitHub
/// Actions job and set by nothing else, so requiring both means the run id is used exactly when
/// GitHub supplies it.
///
/// The id is trimmed because it becomes part of a table name, where surrounding whitespace is not a
/// legal identifier character, and a blank id is no id: an empty suffix IS the collision.
pub(crate) fn ci_run_id(actions: Option<&str>, run_id: Option<&str>) -> Option<String> {
    match (actions, run_id.map(str::trim)) {
        (Some("true"), Some(id)) if !id.is_empty() => Some(id.to_owned()),
        _ => None,
    }
}

/// Maps the run's identity inputs to its token, as a pure function so the token source is testable.
///
/// The CI branch is the run id alone, because GitHub issues one per run and a log reading it can
/// name the run. The local branch joins the clock and the process id with `_` - a separator that
/// makes the concatenation unambiguous, since a separator-less join maps distinct pairs onto one
/// string (`AB`+`C` and `A`+`BC`), and one that [`TableName`] admits, where a hyphen is refused as
/// an illegal character.
pub(crate) fn build_token(ci_run_id: Option<&str>, nanos: u128, pid: u32) -> String {
    ci_run_id.map_or_else(|| format!("{nanos:x}_{pid}"), String::from)
}

/// The table a model's committed fixture becomes: its committed name plus this run's suffix.
///
/// **The whole of the re-entrancy fix sits on this one function.** Every table the leg writes is
/// named `dim_customer_<token>_<leg>` etc., so two runs - or the corpus leg's own tests running in
/// parallel under nextest - can never create or replace a table the other is reading. The committed
/// name is preserved at the front so a human reading a log or a dataset still sees which model a
/// table holds.
///
/// The 63-character ceiling is [`TableName::parse`]'s, so the refusal is RETURNED rather than
/// re-checked or panicked: a data system that silently truncates leaves the plan naming a table the
/// load did not write, and reading the wrong table is a wrong number rather than an error. Handing
/// back the parse's own [`InvalidIdentifier`] lets a caller assert WHICH refusal it got - a test
/// that catches a panic instead passes on a panic from anywhere, including one this never raised.
pub(crate) fn suffixed_table(committed: &TableName, token: &str, leg: &str) -> Result<TableName, InvalidIdentifier> {
    TableName::parse(format!("{committed}_{token}_{leg}"))
}

/// The bundle, with every model's table renamed to this run's suffixed physical table.
///
/// **This is the seam that makes the plan agree with the dataset.** `sutura_semantic::compile`
/// resolves a question against the pinned definitions and the plan carries each model's table name,
/// so a leg that loaded fixtures under suffixed names but compiled against the committed names would
/// have its two sides reading different tables. Rebuilding the definitions - only the models' tables
/// change, relationships and metrics are cloned verbatim - puts the suffixed names where the plan
/// reads them, for BOTH the engine and `BigQuery` (they share one bundle, which is what keeps the
/// comparison honest). The digest is recomputed by `pin` over the suffixed definitions, so the
/// answers carry a correct bundle.
pub(crate) fn suffixed_bundle(tokened: &PinnedDefinitions, token: &str, leg: &str) -> PinnedDefinitions {
    let models: Vec<Model> = tokened
        .definitions()
        .models()
        .values()
        .map(|model| {
            let table = suffixed_table(model.table_name(), token, leg)
                .expect("this leg's own tokens leave every fixture name inside the ceiling");
            Model::new(
                model.name().clone(),
                model.source().clone(),
                table,
                model.columns().clone(),
                Description::parse(model.description()).expect("a loaded description reparses"),
            )
        })
        .collect();
    let relationships: Vec<Relationship> = tokened.definitions().relationships().values().cloned().collect();
    let metrics: Vec<Metric> = tokened.definitions().metrics().values().cloned().collect();
    let suffixed = Definitions::assemble(models, relationships, metrics)
        .expect("suffixing table names keeps the cross-references consistent");
    PinnedDefinitions::pin(
        tokened.version().clone(),
        suffixed,
        tokened.knowledge().clone(),
        tokened.manifest().clone(),
    )
    .expect("a suffixed bundle pins like the original")
}
