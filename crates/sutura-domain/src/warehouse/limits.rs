//! The threshold that refused a result while its adapter was reading it.

/// Facts an adapter can report about a result refusal. A threshold gives no total rejected size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResultLimit {
    Unknown,
    /// The query's final-answer limit, excluding its extra probe row.
    AnswerRows {
        limit: u32,
    },
    StreamRows {
        limit: usize,
    },
    MaterializationBytes {
        limit_bytes: usize,
    },
}
