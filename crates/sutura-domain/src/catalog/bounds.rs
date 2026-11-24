//! A per-catalog bound an operator may declare over [`super::Definitions::assemble`], and the
//! ceiling it may not cross. Replaces the former fixed `MAX_DEFINITIONS_BYTES` constant
//! (`github.com/telekom/sutura#2071`).
//!
//! Split from `catalog.rs` for the same reason `authored.rs` and `consistency.rs` are: `cargo
//! xtask max-lines` fails at a thousand lines under `crates/` and cannot be exempted, and this type
//! plus its doc would have pushed that file over.

use std::fmt;

/// What [`super::Definitions::assemble`] checks a bundle against unless a catalog declares
/// `max_definitions_bytes` - `github.com/telekom/sutura#2071`.
///
/// A dictionary catalog over a schema of real width carries authored bytes no example catalog
/// does: every column typed and described, across tens of models, routinely sums past the former
/// fixed bound - the issue's own case measured 240538 bytes for about fifty views. 1 MiB is not
/// argued from a reference corpus the way the old fixed bound was; it is sized to clear that real
/// case with headroom (over four times it) while staying an order of magnitude under
/// [`DEFINITIONS_BYTES_CEILING`], so the default still refuses a catalog an order of magnitude
/// larger than one that already needed raising.
pub const DEFAULT_DEFINITIONS_BYTES: usize = 1024 * 1024;

/// The most an operator may raise a catalog's bound to - refused above this at config parse.
///
/// The former fixed bound argued its purpose "the way [`crate::query::MAX_RANGE_DAYS`] is: what it
/// bounds is the size of the document, not whether what is in it is worth reading" - the document
/// being what an agent's prompt and `describe_catalog` tool result carry. That purpose survives
/// making the bound per-catalog: until a paged `describe_catalog` exists
/// (`github.com/telekom/sutura#2071`'s own second ask, not done
/// here), this is still the only thing keeping that document bounded, so letting a deployment
/// declare it away entirely would make the bound configurable in name while reintroducing the
/// unbounded document the issue reports, only larger. 16 MiB is sixteen times
/// [`DEFAULT_DEFINITIONS_BYTES`] - room for a dictionary far wider than the one the issue measured,
/// without admitting a value an operator raised by habit rather than by measurement.
pub const DEFINITIONS_BYTES_CEILING: usize = 16 * 1024 * 1024;

/// A declared `catalogs[].max_definitions_bytes`, parsed once so every later read is infallible.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DefinitionsByteBound(usize);

impl DefinitionsByteBound {
    /// [`DEFAULT_DEFINITIONS_BYTES`], as a `const` an adapter's own `const fn new` can assign -
    /// [`Default::default`] is not `const` on stable Rust, so every adapter constructor that wants
    /// to stay `const fn` reaches for this instead.
    pub const DEFAULT: Self = Self(DEFAULT_DEFINITIONS_BYTES);

    /// Refuses `0` - a bound of zero can never load a catalog with anything in it - and anything
    /// above [`DEFINITIONS_BYTES_CEILING`].
    pub const fn parse(bytes: usize) -> Result<Self, InvalidDefinitionsByteBound> {
        if bytes == 0 {
            return Err(InvalidDefinitionsByteBound::Zero);
        }
        if bytes > DEFINITIONS_BYTES_CEILING {
            return Err(InvalidDefinitionsByteBound::AboveCeiling {
                bytes,
                ceiling: DEFINITIONS_BYTES_CEILING,
            });
        }
        Ok(Self(bytes))
    }

    #[inline]
    pub const fn get(self) -> usize {
        self.0
    }

    /// The bound a composed bundle is checked against: every composed catalog's own declared
    /// bound, summed and capped at [`DEFINITIONS_BYTES_CEILING`] -
    /// `github.com/telekom/sutura#2073`'s fix. A composer that instead checked a merge against one
    /// fixed default is exactly how a raised per-catalog bound got silently re-refused at
    /// composition regardless of what each contributor declared.
    ///
    /// An empty `bounds` composes to the smallest valid bound, `Self(1)` - NOT [`Self::DEFAULT`]:
    /// a saturating sum of nothing is `0`, clamped up only by the `1` floor below, which happens to
    /// be far under the 1 MiB default. A degenerate input no caller here reaches, since every
    /// composer refuses an empty catalog list before checking a size.
    pub fn composed(bounds: impl IntoIterator<Item = Self>) -> Self {
        let summed = bounds
            .into_iter()
            .fold(0usize, |total, bound| total.saturating_add(bound.get()));
        Self(summed.clamp(1, DEFINITIONS_BYTES_CEILING))
    }
}

impl Default for DefinitionsByteBound {
    /// [`Self::DEFAULT`] - what a catalog omitting `max_definitions_bytes` is checked against.
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl fmt::Display for DefinitionsByteBound {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Why a declared `max_definitions_bytes` is not usable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum InvalidDefinitionsByteBound {
    #[error("max_definitions_bytes is 0, which can never load a catalog with anything in it")]
    Zero,
    #[error("max_definitions_bytes is {bytes}, and {ceiling} is the most a deployment may declare")]
    AboveCeiling { bytes: usize, ceiling: usize },
}

/// Held beside this file's own implementation (`#2073`'s review blocking 2): the causality gate
/// reverts an implementation file with no test of its own, and `bounds.rs` has none at base to
/// revert to - it did not exist there. A test here keeps this type, its `parse`/`composed` and both
/// refusal variants present in the base reconstruction every other crate's held test calls into;
/// the larger assertions over the bound actually enforced live beside `Definitions::assemble_bounded`
/// and the composition-root call sites instead, each a non-verdict pass for the same reason.
#[cfg(test)]
mod tests {
    use super::{DEFINITIONS_BYTES_CEILING, DefinitionsByteBound, InvalidDefinitionsByteBound};

    #[test]
    fn zero_is_refused_and_the_ceiling_is_the_most_a_bound_may_declare() {
        assert_eq!(DefinitionsByteBound::parse(0), Err(InvalidDefinitionsByteBound::Zero));
        assert_eq!(
            DefinitionsByteBound::parse(DEFINITIONS_BYTES_CEILING + 1),
            Err(InvalidDefinitionsByteBound::AboveCeiling {
                bytes: DEFINITIONS_BYTES_CEILING + 1,
                ceiling: DEFINITIONS_BYTES_CEILING,
            })
        );
        assert_eq!(
            DefinitionsByteBound::parse(DEFINITIONS_BYTES_CEILING)
                .expect("the ceiling itself is a valid bound")
                .get(),
            DEFINITIONS_BYTES_CEILING
        );
    }

    #[test]
    fn composed_sums_and_clamps_at_the_ceiling() {
        let one = DefinitionsByteBound::parse(1000).expect("1000 is a valid bound");
        let two = DefinitionsByteBound::parse(2000).expect("2000 is a valid bound");
        assert_eq!(DefinitionsByteBound::composed([one, two]).get(), 3000);
        assert_eq!(
            DefinitionsByteBound::composed([DefinitionsByteBound::parse(DEFINITIONS_BYTES_CEILING).unwrap(), one]).get(),
            DEFINITIONS_BYTES_CEILING
        );
        assert_eq!(DefinitionsByteBound::composed([]).get(), 1);
    }
}
