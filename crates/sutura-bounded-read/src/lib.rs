#![forbid(unsafe_code)]
//! The bounded, single-open catalog document read and the sorted directory walk that three catalog
//! adapters need.
//!
//! `sutura-catalog-local`, `sutura-catalog-okf` (`github.com/telekom/sutura#1022` hardened it there)
//! and `sutura-catalog-datacontract` (`#1043`) each carried a copy of the same read and walk: a
//! `rustix` open with `O_NOFOLLOW | O_NONBLOCK | O_CLOEXEC`, an `fstat` on that handle for being a
//! regular file, a byte budget enforced on the read itself with a post-read length recheck, and a
//! `BTreeSet`-sorted walk refused when empty. Only the error type name differed between the three;
//! the `sutura/crate-map` skill's `sutura-tls` precedent is exactly this shape - a small read two or
//! more same-class adapters both need, with no network client and no `sutura-config`, carrying no
//! adapter prefix so it joins no forbidden class.
//!
//! # What this crate owns, and what each catalog keeps
//!
//! This crate reads bytes and returns them; it renders nothing to a caller. Each catalog maps
//! [`ReadError`] / [`WalkError`] into its own error enum, keeping its own variants and rendered
//! messages unchanged, and keeps whatever it does with the text after the read (parse the document,
//! split its frontmatter, dispatch on its kind). The two sharp bounds a catalog clamps each document
//! to - the document-count cap on the walk and the aggregate-byte cap on the read - are both declared
//! here and supplied by the caller, so the bound lives beside the mechanism rather than one crate
//! from it.
//!
//! **This is a pure refactor: no behavioural change.** The three catalogs' existing integration
//! bounds tests (`tests/bounds.rs` in each) assert on rendered refusal messages through their own
//! `load()` driving port, and they pass unchanged. A finisher proves it by running those three suite
//! files against the refactor and against base.

mod read;
mod walk;

pub use read::{MAX_CATALOG_BYTES, ReadError, read_document};
pub use walk::{MAX_CATALOG_DOCUMENTS, WalkError, walk};
