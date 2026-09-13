//! A dry run's own byte estimate, and nothing else.
//!
//! **Split out of `warehouse.rs` when that file crossed the thousand-line cap `cargo xtask
//! max-lines` enforces**, at a real seam rather than an arbitrary cut: [`EstimatedBytes`] is one
//! type with one job, and `PreFlight` - which carries it - stays in the parent module where the
//! rest of the pre-flight vocabulary lives.
//!
//! **`pub mod` with no re-export beside it, for `crate::warehouse::preflight`'s own documented
//! reason.** The domain's usual shape is a private submodule plus a `pub use`, which rustdoc
//! inlines into the parent - and that shape produced an undocumented `### use None` stub for types
//! reached only through a re-export. A public module gets documented at its own path instead.

/// A dry run's own estimate of the bytes a statement would scan.
///
/// **A newtype over `u64` rather than a bare integer carried on [`super::PreFlight::Accepted`]**,
/// so a byte count read off a dry run cannot be confused with any of the plan's other `u64`s.
/// `docs/adr/0030` decides this shape and the `Option` it sits inside together.
///
/// **Zero is a legitimate estimate, not a stand-in for "unknown".** A cached result or a trivial
/// `SELECT` can genuinely cost nothing to scan, so [`Self::parse`] cannot fail: this type validates
/// nothing beyond fitting in a `u64`. That is unlike a bound such as
/// `BytesBilledCeiling`, where zero would refuse every question and is refused itself - an estimate
/// of zero is simply the truth for some questions. What means "could not price" is the `Option`
/// around this type on [`super::PreFlight::Accepted`], never a reserved value inside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EstimatedBytes(u64);

impl EstimatedBytes {
    /// Wraps a byte count a dry run reported.
    ///
    /// Infallible, on purpose: every `u64` is a byte count some statement could scan, and the
    /// ambiguity this type exists to remove is one level up, in whether an estimate exists at all.
    #[must_use]
    #[inline]
    pub const fn parse(bytes: u64) -> Self {
        Self(bytes)
    }

    /// The estimate, in bytes.
    #[must_use]
    #[inline]
    pub const fn bytes(self) -> u64 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::EstimatedBytes;
    use crate::warehouse::PreFlight;

    #[test]
    fn an_estimate_of_zero_is_a_real_answer_and_not_an_absent_one() {
        // The claim this type's own documentation makes: zero is a legitimate byte count, not a
        // stand-in for "could not price". `PreFlight::Accepted { estimated_bytes: None }` is where
        // "could not price" lives instead - one level up, never inside this type.
        assert_eq!(EstimatedBytes::parse(0).bytes(), 0);
        assert_ne!(
            PreFlight::Accepted {
                estimated_bytes: Some(EstimatedBytes::parse(0))
            },
            PreFlight::Accepted { estimated_bytes: None },
            "a priced zero and an unpriced absence must not compare equal"
        );
    }

    #[test]
    fn an_estimate_round_trips_the_byte_count_it_was_given() {
        assert_eq!(EstimatedBytes::parse(123_456_789).bytes(), 123_456_789);
        assert_eq!(EstimatedBytes::parse(u64::MAX).bytes(), u64::MAX);
    }

    #[test]
    fn two_estimates_of_the_same_count_are_the_same_estimate() {
        // `Eq`/`PartialEq`/`Ord` are derived rather than asserted individually - this is the one
        // observable behaviour a caller comparing two estimates depends on, which a derive can
        // silently stop providing if the field it is derived over ever grows a second one.
        assert_eq!(EstimatedBytes::parse(42), EstimatedBytes::parse(42));
        assert!(EstimatedBytes::parse(1) < EstimatedBytes::parse(2));
    }
}
