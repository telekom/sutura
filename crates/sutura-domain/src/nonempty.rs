//! A list that cannot be empty, because the constructor that would produce one does not exist.
//!
//! **Unrepresentable over checked**, the same argument `secure-by-design` makes for the rest of this
//! crate's newtypes: a `Vec` that a caller happens to always check for emptiness is a rule enforced
//! by discipline at every read site, and a set with no elements is a valid `Vec` that means nothing
//! for a caller who asked for one or more metrics, or one or more values to filter on. `NonEmpty`
//! makes the empty case not exist rather than exist and be refused - there is no `Default`, no
//! `new()`, and [`NonEmpty::parse`] is the only fallible entry point, returning [`EmptySet`] for the
//! one thing that can go wrong.

use serde::de::Error as _;

/// One or more `T`, with no way to construct zero.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NonEmpty<T> {
    head: T,
    tail: Vec<T>,
}

/// [`NonEmpty::parse`] was handed a list with nothing in it.
///
/// Declared with an explicit empty body (`{}`) rather than the plain `;` a unit struct usually
/// takes: `cargo xtask check-boundaries`'s pub-field scan closes a struct's body on the line that
/// opens it only when it sees a brace there, and a bare `;` instead leaves it scanning for the
/// NEXT one - which would otherwise be [`NonEmpty`]'s own inherent `impl` block below, whose
/// `pub fn`s the scan would misread as this struct's fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the list is empty")]
#[expect(
    clippy::empty_structs_with_brackets,
    reason = "the brackets are load-bearing - see the doc comment above"
)]
pub struct EmptySet {}

impl<T> NonEmpty<T> {
    /// Exactly one element.
    #[inline]
    pub const fn one(head: T) -> Self {
        Self { head, tail: Vec::new() }
    }

    /// Every element of `items`, or [`EmptySet`] if there were none.
    pub fn parse(mut items: Vec<T>) -> Result<Self, EmptySet> {
        if items.is_empty() {
            return Err(EmptySet {});
        }
        let head = items.remove(0);
        Ok(Self { head, tail: items })
    }

    /// The number of elements. Never zero.
    #[inline]
    pub const fn len(&self) -> usize {
        1_usize.saturating_add(self.tail.len())
    }

    /// Never true - a method anyway, because clippy's `len_without_is_empty` lint does not know
    /// this type's whole point is that the answer is always the same.
    #[inline]
    #[must_use]
    #[expect(clippy::unused_self, reason = "pairs with len() as a method - see the doc comment above")]
    pub const fn is_empty(&self) -> bool {
        false
    }

    /// The first element - the one every `NonEmpty` is guaranteed to have.
    #[inline]
    pub const fn first(&self) -> &T {
        &self.head
    }

    /// Every element, in the order it was given.
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        core::iter::once(&self.head).chain(self.tail.iter())
    }
}

impl<'a, T> IntoIterator for &'a NonEmpty<T> {
    type Item = &'a T;
    type IntoIter = core::iter::Chain<core::iter::Once<&'a T>, core::slice::Iter<'a, T>>;

    fn into_iter(self) -> Self::IntoIter {
        core::iter::once(&self.head).chain(self.tail.iter())
    }
}

impl<T: serde::Serialize> serde::Serialize for NonEmpty<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.collect_seq(self.iter())
    }
}

impl<'de, T: serde::Deserialize<'de>> serde::Deserialize<'de> for NonEmpty<T> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let items = Vec::<T>::deserialize(deserializer)?;
        Self::parse(items).map_err(|EmptySet {}| D::Error::custom("the list must hold at least one element"))
    }
}

#[cfg(test)]
mod tests {
    use super::{EmptySet, NonEmpty};

    #[test]
    fn one_element_is_never_empty() {
        let set = NonEmpty::one(1);
        assert_eq!(set.len(), 1);
        assert!(!set.is_empty());
        assert_eq!(set.first(), &1);
    }

    #[test]
    fn parsing_an_empty_vec_is_refused() {
        assert_eq!(NonEmpty::<i32>::parse(Vec::new()).unwrap_err(), EmptySet {});
    }

    #[test]
    fn parsing_preserves_order() {
        let set = NonEmpty::parse(vec![1, 2, 3]).expect("three elements is not empty");
        assert_eq!(set.iter().collect::<Vec<_>>(), vec![&1, &2, &3]);
    }

    #[test]
    fn it_round_trips_through_json() {
        let set = NonEmpty::parse(vec!["a", "b"]).expect("two elements is not empty");
        let json = serde_json::to_string(&set).expect("a NonEmpty serializes");
        assert_eq!(json, "[\"a\",\"b\"]");
        let back: NonEmpty<String> = serde_json::from_str(&json).expect("the same JSON deserializes");
        assert_eq!(back.iter().collect::<Vec<_>>(), vec!["a", "b"]);
    }

    #[test]
    fn an_empty_json_array_is_refused() {
        let result: Result<NonEmpty<i32>, _> = serde_json::from_str("[]");
        drop(result.unwrap_err());
    }
}
