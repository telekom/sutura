//! One cell of a result, and the checked real number a cell may carry.
//!
//! Its own module because these are the value vocabulary every adapter maps into, and none of them
//! knows about a plan, a credential or a row. **What it does NOT hold:
//! [`ParamValue`](crate::warehouse::ParamValue), which travels the other way** - a value bound to a
//! on the way OUT - and which the port module keeps beside the reason it exists.
//!
//! `pub mod` so rustdoc documents these types rather than emitting a re-export stub, which is what
//! [`crate::warehouse::preflight`] records happening to a private module with only a `pub use`. The
//! beside the declaration keeps the `sutura_domain::warehouse::` path its importers already use.

/// Why a floating-point cell was refused.
///
/// Two variants rather than one, because the causes differ and a reader chasing one is not chasing
/// the other: an infinity is a non-zero quantity divided by zero, a `NaN` is zero divided by zero.
/// The variant carries the value rather than a formatted sentence, for the reason every error in
/// this crate does.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum NotFinite {
    /// Infinite, in either direction.
    #[error("{value} is not a finite number")]
    Infinite { value: f64 },
    /// Not a number at all. Its own variant rather than a value on the one above, because `NaN`
    /// compares unequal to itself: an [`Infinite`] carrying one would make two of these errors
    /// unequal for a reason unrelated to what happened.
    ///
    /// [`Infinite`]: NotFinite::Infinite
    #[error("NaN is not a number")]
    NotANumber,
}

/// A real number a result may carry: finite, and nothing else.
///
/// **Parsed rather than validated, and the class it closes is larger than the bug that found it.**
/// A cell was a raw `f64`, so `inf`, `-inf` and `NaN` were representable and [`Value::render`]
/// turned the first into the string `"inf"` - an answer under a metric's own certified name that
/// reads as data and is not a number. The route in was a ratio measure declaring
/// `zero_denominator: fails`: both adapters cast the numerator to a floating type before dividing,
/// so the division is IEEE float division, which by zero does not fail - it answers `inf`, or `NaN`
/// when both halves are zero.
///
/// Refusing a non-finite value in the domain type closes all three at once, at the one boundary
/// every adapter crosses, rather than guarding the variant that exposed it. An adapter that gets one
/// back has an error naming the column, which is what `fails` always claimed to mean.
///
/// Construct it with [`parse`]. The field is private, so a non-finite value is unrepresentable
/// rather than merely rejected. No `Deref` and no arithmetic, deliberately: two finite numbers
/// divide to a non-finite one, so a type letting the result back in without passing [`parse`] again
/// would be the hole this closes. [`Value`] is `Serialize` only - if it gains `Deserialize`, this
/// needs `#[serde(try_from = ..)]` routing through [`parse`], because a derived one writes straight
/// into the private field.
///
/// [`parse`]: Real::parse
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct Real(f64);

impl Real {
    /// Parses a real number, rejecting a non-finite one.
    pub const fn parse(value: f64) -> Result<Self, NotFinite> {
        if value.is_nan() {
            return Err(NotFinite::NotANumber);
        }
        if value.is_infinite() {
            return Err(NotFinite::Infinite { value });
        }
        Ok(Self(value))
    }

    /// The number, for a caller that has to do arithmetic on it.
    ///
    /// Named rather than reached through `Deref`, so where the invariant stops applying is a call
    /// somebody wrote.
    #[inline]
    pub const fn get(self) -> f64 {
        self.0
    }
}

/// Shortest round-trip formatting, so a value that came back as an exact decimal is rendered as one
/// rather than as its binary expansion. Delegated rather than reimplemented, and this is the one
/// definition [`Value::render`] uses.
impl core::fmt::Display for Real {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::Display::fmt(&self.0, f)
    }
}

/// Exponent form, forwarding the formatter's precision.
///
/// Comparing two engines' floats is done at a fixed number of significant digits - summing the same
/// rows in a different order changes the last place of an `f64` - and `{:.12e}` is how that is
/// written. A formatting trait rather than `get`, so the comparison need not leave the type.
impl core::fmt::LowerExp for Real {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::LowerExp::fmt(&self.0, f)
    }
}

/// One cell of a result.
///
/// [`Real`] is deliberately last to reach for: a measure over integer minor units stays exact, and
/// an anchor comparison over a float would depend on how two languages print the same bits. It
/// exists because `avg` has to land somewhere, and it is a checked type rather than an `f64` so the
/// one thing a float can be that a number cannot does not fit in a cell.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub enum Value {
    Null,
    Integer(i64),
    Real(Real),
    Text(String),
}

impl Value {
    /// The canonical text form, which is what an anchor is compared against.
    ///
    /// One function so there is one answer: an anchor comparison formatting the value at the call
    /// site would compare differently in two places, and the failure would look like a data problem
    /// rather than a formatting one.
    pub fn render(&self) -> String {
        match *self {
            Self::Null => String::from("null"),
            Self::Integer(v) => v.to_string(),
            // One definition of what a real number looks like, on the type that carries one.
            Self::Real(v) => v.to_string(),
            Self::Text(ref v) => v.clone(),
        }
    }
}
