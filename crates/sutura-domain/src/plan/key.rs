//! One group-by key: a column, or a column truncated to a grain.

use crate::model::Grain;
use crate::plan::{PlanColumn, ResultLabel};

/// One group-by key.
///
/// **The label is a [`ResultLabel`] and not a `String`, which is `telekom/sutura#337`.** A key used
/// to be labelled with whatever text its producer computed, and what kept that out of the internal
/// namespace a federated leg also projects into was a derivation held by review. The type is the
/// mechanism now: `ResultLabel` carries the compile-fail pair that says so.
///
/// **The column is read only through [`PlanKey::value`]**, a closed enum, so a renderer that groups
/// by the raw column of a truncated key does not compile. A plain key serializes without a `grain`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct PlanKey {
    label: ResultLabel,
    column: PlanColumn,
    #[serde(skip_serializing_if = "Option::is_none")]
    grain: Option<Grain>,
}

/// What a [`PlanKey`] groups by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyValue<'key> {
    /// The column as stored.
    Column(&'key PlanColumn),
    /// The column truncated to `grain`, with the function a time bucket and a truncated join use.
    Truncated { column: &'key PlanColumn, grain: Grain },
}

impl PlanKey {
    #[inline]
    pub const fn new(label: ResultLabel, column: PlanColumn) -> Self {
        Self {
            label,
            column,
            grain: None,
        }
    }

    /// A key on `column` truncated to `grain`.
    #[inline]
    pub const fn truncated(label: ResultLabel, column: PlanColumn, grain: Grain) -> Self {
        Self {
            label,
            column,
            grain: Some(grain),
        }
    }

    /// The text this key is projected under.
    ///
    /// Text rather than the [`ResultLabel`], because every reader of a label renders it: the
    /// generator quotes it as an alias and the combiner looks a column up by it. What the type
    /// holds is the way IN.
    #[inline]
    pub fn label(&self) -> &str {
        self.label.as_str()
    }

    #[inline]
    pub const fn value(&self) -> KeyValue<'_> {
        match self.grain {
            None => KeyValue::Column(&self.column),
            Some(grain) => KeyValue::Truncated {
                column: &self.column,
                grain,
            },
        }
    }
}
