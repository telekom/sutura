//! The two framing bounds a [`super::RemoteLegService`] is built with.
//!
//! Two types rather than two `NonZeroUsize`s, because two adjacent arguments of one type are what a
//! swapped call site passes without complaint.

use std::num::NonZeroUsize;

/// The most command bytes one request's descriptor may carry, checked before it is decoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DescriptorLimit(NonZeroUsize);

impl DescriptorLimit {
    /// A limit that is never zero by its type.
    #[must_use]
    pub const fn new(bytes: NonZeroUsize) -> Self {
        Self(bytes)
    }

    /// Parses a limit, refusing zero.
    pub const fn parse(bytes: usize) -> Result<Self, NoLimit> {
        match NonZeroUsize::new(bytes) {
            Some(bytes) => Ok(Self(bytes)),
            None => Err(NoLimit),
        }
    }

    /// The limit in bytes.
    #[must_use]
    pub const fn bytes(self) -> NonZeroUsize {
        self.0
    }
}

/// The most encoded bytes one request's frames may carry, descriptor included, checked before the
/// parameters are decoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameBudget(NonZeroUsize);

impl FrameBudget {
    /// A budget that is never zero by its type.
    #[must_use]
    pub const fn new(bytes: NonZeroUsize) -> Self {
        Self(bytes)
    }

    /// Parses a budget, refusing zero.
    pub const fn parse(bytes: usize) -> Result<Self, NoLimit> {
        match NonZeroUsize::new(bytes) {
            Some(bytes) => Ok(Self(bytes)),
            None => Err(NoLimit),
        }
    }

    /// The budget in bytes.
    #[must_use]
    pub const fn bytes(self) -> NonZeroUsize {
        self.0
    }
}

/// A framing limit of zero bytes, which would refuse every request.
///
/// Declared after both limits: `xtask check-boundaries`' pub-field scan reads a unit struct
/// followed by an `impl` block as that struct's fields (the same placement as `Budget`'s
/// `NoBudget`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("a remote leg framing limit must be at least one byte")]
pub struct NoLimit;

#[cfg(test)]
mod tests {
    use super::{DescriptorLimit, FrameBudget, NoLimit};

    #[test]
    fn a_zero_byte_framing_limit_is_refused() {
        assert_eq!(DescriptorLimit::parse(0), Err(NoLimit));
        assert_eq!(FrameBudget::parse(0), Err(NoLimit));
        assert_eq!(DescriptorLimit::parse(1).map(|limit| limit.bytes().get()), Ok(1));
        assert_eq!(FrameBudget::parse(1).map(|budget| budget.bytes().get()), Ok(1));
    }
}
