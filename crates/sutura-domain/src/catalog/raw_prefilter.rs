//! A per-column acknowledgement of secret-dependent raw-search work.

use crate::source::{AcknowledgementReason, InvalidOperatorText};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields, try_from = "WrittenRawPrefilter")]
pub struct RawPrefilter {
    acknowledged_because: AcknowledgementReason,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct WrittenRawPrefilter {
    acknowledged_because: String,
}

impl TryFrom<WrittenRawPrefilter> for RawPrefilter {
    type Error = InvalidOperatorText;

    fn try_from(written: WrittenRawPrefilter) -> Result<Self, Self::Error> {
        Self::parse(written.acknowledged_because)
    }
}

impl RawPrefilter {
    /// Records why this column may use a raw necessary-condition search.
    ///
    /// This permits timing differences, never a result decision on unredacted text.
    pub fn parse(reason: impl AsRef<str>) -> Result<Self, InvalidOperatorText> {
        AcknowledgementReason::written_under("raw_prefilter.acknowledged_because", reason)
            .map(|acknowledged_because| Self { acknowledged_because })
    }

    pub fn acknowledged_because(&self) -> &str {
        self.acknowledged_because.as_str()
    }
}
