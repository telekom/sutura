//! `--shard <i>/<n>` for `check-claim-mutation-kills`: one of `n` disjoint, deterministic slices of
//! the committed patch set, so `n` workflow dispatches cover it inside the hosted job limit.
//!
//! The slice is positional over the SORTED names - position `p`, counted from 0, belongs to shard
//! `p % n + 1` - so every shard of one `n` agrees on the order only while the set does not change
//! between dispatches. Dispatch all `n` from one commit. Nothing here checks that.

use std::fmt;
use std::num::NonZeroUsize;
use std::str::FromStr;

/// What a refusal names, so a mistyped value is told the form rather than guessed at.
const FORM: &str = "--shard <i>/<n> with 1 <= i <= n, for example --shard 2/4";

/// One slice of `count`: `index` is 1-based, as a person writes it, and never exceeds `count`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Shard {
    index: NonZeroUsize,
    count: NonZeroUsize,
}

/// Why an invocation does not name a shard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ShardError {
    /// Arguments other than none or `--shard <value>`.
    Arguments,
    /// The value is not `<digits>/<digits>`.
    Form(String),
    /// A side of the fraction is not a positive whole number.
    Number(String),
    /// `index` is outside `1..=count`.
    Range { index: usize, count: usize },
}

impl fmt::Display for ShardError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Arguments => write!(f, "usage: {FORM}; with none, every patch runs"),
            Self::Form(got) => write!(f, "`{got}` is not <i>/<n>; the form is {FORM}"),
            Self::Number(got) => write!(
                f,
                "`{got}` has a side that is not a positive whole number; the form is {FORM}"
            ),
            Self::Range { index, count } => write!(f, "shard {index}/{count} is out of range; the form is {FORM}"),
        }
    }
}

impl Shard {
    /// Every patch: `1/1`, what an invocation without `--shard` means.
    pub(super) const WHOLE: Self = Self {
        index: NonZeroUsize::MIN,
        count: NonZeroUsize::MIN,
    };

    /// The shard `args` name, or [`Shard::WHOLE`] for none.
    pub(super) fn of_args(args: &[String]) -> Result<Self, ShardError> {
        match args {
            [] => Ok(Self::WHOLE),
            [flag, value] if flag == "--shard" => value.parse(),
            _ => Err(ShardError::Arguments),
        }
    }

    /// The items at this shard's positions. `items` must already be in a stable order.
    pub(super) fn take<T>(self, items: Vec<T>) -> Vec<T> {
        let residue = self.index.get().saturating_sub(1);
        items
            .into_iter()
            .enumerate()
            .filter_map(|(position, item)| (position % self.count == residue).then_some(item))
            .collect()
    }
}

impl fmt::Display for Shard {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.index, self.count)
    }
}

impl FromStr for Shard {
    type Err = ShardError;

    fn from_str(text: &str) -> Result<Self, ShardError> {
        let (left, right) = text.split_once('/').ok_or_else(|| ShardError::Form(text.to_owned()))?;
        let number = |side: &str| {
            let digits = !side.is_empty() && side.bytes().all(|byte| byte.is_ascii_digit());
            digits
                .then(|| side.parse::<usize>().ok())
                .flatten()
                .ok_or_else(|| ShardError::Number(text.to_owned()))
        };
        let (index, count) = (number(left)?, number(right)?);
        match (NonZeroUsize::new(index), NonZeroUsize::new(count)) {
            (Some(index), Some(count)) if index <= count => Ok(Self { index, count }),
            (Some(_), Some(_)) => Err(ShardError::Range { index, count }),
            _ => Err(ShardError::Number(text.to_owned())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Shard, ShardError};

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|arg| (*arg).to_owned()).collect()
    }

    #[test]
    fn a_shard_within_its_count_parses_and_prints_back() {
        for text in ["1/1", "2/4", "4/4", "17/300"] {
            let shard: Shard = text.parse().expect("a well-formed shard");
            assert_eq!(shard.to_string(), text);
        }
    }

    #[test]
    fn a_shard_outside_its_count_or_not_a_fraction_is_refused() {
        let refused = |text: &str| text.parse::<Shard>().expect_err("a malformed shard");
        assert_eq!(refused("5/4"), ShardError::Range { index: 5, count: 4 });
        for text in ["0/4", "1/0", "0/0", "a/b", "+1/4", "-1/4", "1/ 4", "1/", "/4"] {
            assert_eq!(refused(text), ShardError::Number(text.to_owned()), "{text}");
        }
        for text in ["4", "", "1-4"] {
            assert_eq!(refused(text), ShardError::Form(text.to_owned()), "{text}");
        }
        assert_eq!(refused("1/2/3"), ShardError::Number("1/2/3".to_owned()));
        assert!(refused("99999999999999999999999/4").to_string().contains("--shard <i>/<n>"));
    }

    #[test]
    fn no_argument_is_the_whole_set_and_a_stray_one_is_refused() {
        assert_eq!(Shard::of_args(&[]), Ok(Shard::WHOLE));
        assert_eq!(Shard::of_args(&args(&["--shard", "2/4"])), "2/4".parse());
        for stray in [&["--shard"][..], &["2/4"], &["--shard", "2/4", "x"], &["--since", "2/4"]] {
            assert_eq!(Shard::of_args(&args(stray)), Err(ShardError::Arguments), "{stray:?}");
        }
    }

    #[test]
    fn the_shards_of_n_partition_the_set_and_one_shard_is_the_whole_of_it() {
        let set: Vec<usize> = (0..23).collect();
        assert_eq!(Shard::WHOLE.take(set.clone()), set);
        for count in [1_usize, 2, 4, 7, 23, 30] {
            let mut union: Vec<usize> = Vec::new();
            for index in 1..=count {
                let shard: Shard = format!("{index}/{count}").parse().expect("in range");
                let taken = shard.take(set.clone());
                let every_nth: Vec<usize> = (index - 1..set.len()).step_by(count).collect();
                assert_eq!(taken, every_nth, "{index}/{count}");
                union.extend(taken);
            }
            union.sort_unstable();
            assert_eq!(union, set, "the {count} shards cover the set once each");
        }
    }
}
