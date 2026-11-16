//! Facts about a rejected source stream, not the final answer or its complete rejected size.

/// The limit that refused a source result. An adapter may classify only these facts, never an
/// arbitrary tool refusal. Unknown limits carry no invented number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResultLimit {
    Unknown,
    StreamRows { limit: usize },
    MaterializationBytes { limit_bytes: usize },
}
