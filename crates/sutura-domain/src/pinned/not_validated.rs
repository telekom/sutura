//! Why a bundle is not validated.

use super::NotExecutedReason;
use crate::model::{ColumnName, MetricName, ModelName};
use crate::warehouse::cardinality::{KeyNotCounted, KeyNotUnique};

/// Why a bundle is not validated.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum NotValidated {
    #[error("model {model} declares a text policy: it requires a mapped text column and no certified metric reading that column")]
    TextPolicy { model: ModelName },
    /// Two models over one physical column pin different known collations; an unknown one
    /// contradicts nothing.
    #[error("model {model} pins column {column} under a collation another model over the same physical column contradicts")]
    CollationDisagreement { model: ModelName, column: ColumnName },
    #[error("metric {metric} reads text-policy column {model}.{column}; certified execution cannot apply that policy")]
    MetricReadsProtectedText {
        metric: MetricName,
        model: ModelName,
        column: ColumnName,
    },
    /// The bundle carries a metric whose computation is catalog-authored SQL, and the adapter this
    /// build selected does not declare `Warehouse::EXECUTES_AUTHORED_SQL` - which today is every
    /// adapter this workspace ships. The fragment is stored as written and compiled by nothing, so
    /// this refusal is what stands between it and a served bundle; `docs/adr/0004` is the record.
    #[error("metric {metric} uses authored SQL, and the selected execution adapter cannot execute it")]
    AuthoredSqlNotExecutable { metric: MetricName },
    /// The declared number and the produced one, both quoted.
    ///
    /// `{:?}` and not `{}`, for the reason [`crate::measure::RequiredFilter`]'s `Display` gives: a
    /// bundle is refused on this line and somebody reads it to find out why, so the spacing of both
    /// halves has to be visible. `expected` came from a catalog author and is an
    /// [`AnchorValue`](crate::catalog::AnchorValue) at rest; `actual` came from the data system and
    /// is held to nothing, which is the half that makes the quoting worth having.
    #[error("metric {metric} was expected to produce {expected:?}, and produced {actual:?}")]
    AnchorMismatch {
        metric: MetricName,
        expected: String,
        actual: String,
    },
    /// The reason is the `source`, not the message, so whoever renders this walks the chain and
    /// gets the data system's own complaint. Interpolating it would have printed the outermost
    /// message and stopped, which is the whole of what was wrong before.
    #[error("metric {metric}'s anchor could not be checked")]
    AnchorNotExecuted {
        metric: MetricName,
        #[source]
        reason: NotExecutedReason,
    },
    #[error("metric {metric} declares an anchor and no check was recorded for it")]
    AnchorUnchecked { metric: MetricName },
    #[error("a check was recorded for {metric}, which this bundle does not define")]
    UnknownMetricChecked { metric: MetricName },
    /// A declared join key the data contradicts.
    ///
    /// **The one boot outcome that stops a deployment over a cardinality declaration**, and it is a
    /// `NotValidated` rather than a warning because of what the declaration buys: a `many_to_one` is
    /// what licenses a join to be measure-preserving, and a target column that is not unique makes
    /// the same question answer differently depending on how the plan was shaped. See
    /// [`cardinality`](crate::warehouse::cardinality) for the two numbers that measurement produced.
    ///
    /// **The counts and no key value**, deliberately: this reaches an operator's log, and a
    /// duplicated dimension key printed there is source data copied into a sink nobody scoped for
    /// it. What it carries locates the table; the operator queries it.
    ///
    /// **Boxed**, because `result_large_err` is deliberately left on in this workspace and five
    /// parsed names inline made this the widest `Result` the boot path returns.
    #[error("{0}")]
    DeclaredKeyNotUnique(Box<KeyNotUnique>),
    /// A declared join key no data system would count.
    ///
    /// **A refusal rather than a warning, and the direction is the opposite of
    /// [`crate::warehouse::Warehouse::preflight`]'s on purpose.** A pre-flight runs in a composition
    /// root, where *could not verify* has somewhere to go: a `WARN` line and a deployment that
    /// serves. This runs inside the operation that mints the proof, where the only two outcomes are
    /// *validated* and *not* - and where the existing rule for a statement the data system would not
    /// run is already refusal ([`NotValidated::AnchorNotExecuted`]). Silence was the third option and
    /// is the one this variant exists to remove: it made *the identity may not read the dimension
    /// table* - the identity-relevant case in an identity-aware runtime - indistinguishable from a
    /// clean check.
    ///
    /// **What it costs**, stated with the claim: a data system briefly unreachable at boot now stops
    /// a deployment that would have served, for a source carrying no anchored metric. For every
    /// source that carries one, the anchor pass already refused it.
    #[error("{0}")]
    DeclaredKeyNotCounted(Box<KeyNotCounted>),
    /// A federated model whose catalog declares no column type: a missing declaration, not a type to
    /// fall back from. The federation planner takes each federated table's schema from the catalog.
    #[error("federated model {model} declares no column type, and its schema comes from the catalog")]
    FederatedModelUntyped { model: ModelName },
}
