//! The iterator [`super::PlanPredicate::bound_params`] returns.
//!
//! A file rather than inline in `plan.rs`, which is this repository's usual shape for a small
//! self-contained type: the parent module crossed the thousand-line limit `cargo xtask max-lines`
//! enforces, and the only way past that gate is to split the file.

/// No allocation for any shape a predicate binds.
pub(crate) enum BoundParams<'a> {
    None,
    One(Option<usize>),
    Many(<&'a crate::nonempty::NonEmpty<usize> as IntoIterator>::IntoIter),
}

impl Iterator for BoundParams<'_> {
    type Item = usize;

    fn next(&mut self) -> Option<usize> {
        match self {
            Self::None => None,
            Self::One(value) => value.take(),
            Self::Many(iter) => iter.next().copied(),
        }
    }
}
