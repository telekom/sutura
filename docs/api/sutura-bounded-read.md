<!-- GENERATED FILE - do not edit.
     Written by docs/.tools/rustdoc_to_markdown.py from rustdoc JSON.
     Edit the doc comments in the crate source instead, then regenerate.
     The commands are on the API reference landing page. -->

# sutura-bounded-read

The public API of `sutura-bounded-read`, rendered from rustdoc JSON.

The bounded, single-open catalog document read and the sorted directory walk that three catalog
adapters need.

`sutura-catalog-local`, `sutura-catalog-okf` (`github.com/telekom/sutura#1022` hardened it there)
and `sutura-catalog-datacontract` (`#1043`) each carried a copy of the same read and walk: a
`rustix` open with `O_NOFOLLOW | O_NONBLOCK | O_CLOEXEC`, an `fstat` on that handle for being a
regular file, a byte budget enforced on the read itself with a post-read length recheck, and a
`BTreeSet`-sorted walk refused when empty. Only the error type name differed between the three;
the `sutura/crate-map` skill's `sutura-tls` precedent is exactly this shape - a small read two or
more same-class adapters both need, with no network client and no `sutura-config`, carrying no
adapter prefix so it joins no forbidden class.

# What this crate owns, and what each catalog keeps

This crate reads bytes and returns them; it renders nothing to a caller. Each catalog maps
`ReadError` / `WalkError` into its own error enum, keeping its own variants and rendered
messages unchanged, and keeps whatever it does with the text after the read (parse the document,
split its frontmatter, dispatch on its kind). The two sharp bounds a catalog clamps each document
to - the document-count cap on the walk and the aggregate-byte cap on the read - are both declared
here and supplied by the caller, so the bound lives beside the mechanism rather than one crate
from it.

**This is a pure refactor: no behavioural change.** The three catalogs' existing integration
bounds tests (`tests/bounds.rs` in each) assert on rendered refusal messages through their own
`load()` driving port, and they pass unchanged. A finisher proves it by running those three suite
files against the refactor and against base.

## `use MAX_CATALOG_BYTES`

## `use ReadError`

Why a bounded document read could not complete.

Every variant carries the path (a catalog is many files, and a message that names no file sends
a reader to read all of them), and, for `Self::TooLarge`, the whole of the refusal's context,
so a caller can name the same things its own refusal did before this crate existed.

## `use read_document`

Reads one catalog document whole and against the aggregate byte budget.

`root` is the catalog root (carried only so the `ReadError::TooLarge` text can name it the way
each caller's own refusal does) and `total_bytes` is the running total of bytes already read, so
the remaining budget is the cap minus it. Returns the document's text, which is what the caller
parses next.

# Errors

`ReadError::Open` for an open or `fstat` that the OS refused (a swapped symlink `ELOOP`, a
swapped FIFO `ENXIO`); `ReadError::NotARegularFile` for a handle that is not a regular file;
`ReadError::TooLarge` when the document is oversized against the aggregate budget, whether its
declared size already is, it grew past it while being read, or the read delivered more than the
budget left; `ReadError::Io` for the read itself or a failed UTF-8 conversion.

## `use MAX_CATALOG_DOCUMENTS`

The most documents a catalog root may hold.

A startup bound, checked on every `BTreeSet` insert - a directory built to be large is refused
as soon as it is large enough, rather than walked to its end first. **Documents, not entries**:
a directory holding this many document-shaped files plus an unbounded number of other entries
(subdirectories, non-document files, a skipped symlink) is unaffected - this bounds what becomes
a document, not the size of the tree it lives in.

## `use WalkError`

Why the walk could not produce the sorted document list.

## `use walk`

Every document under `root`, in sorted order, refused when empty or missing.

Depth-first with the entries of each directory sorted, so the traversal is a function of the
tree rather than of the filesystem. `extensions` is the set of file extensions treated as
documents; anything else is skipped. A document with the right extension and the wrong content
still fails loudly at deserialisation, which is the caller's job.

The decision is made from the directory entry's own type rather than from the path, because
`Path::is_dir` follows a link and answers about the target.

# Errors

`WalkError::NotADirectory` when `root` is not a directory; `WalkError::Io` for a failure to
read `root` or a directory within it; `WalkError::TooManyDocuments` once a directory crosses
`MAX_CATALOG_DOCUMENTS`; `WalkError::Empty` when `root` holds no documents.
