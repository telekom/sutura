# Claim-cell mutation patches

The causality gate's claim arm reads ONE patch per declared claim cell from this directory. A
`Claim-Cell: <test-fn-name>` commit trailer is a claim; a patch here is the mechanism that holds it.

Each file must be named `<test-fn-name>.patch`, matching the exact function ident the commit's
`Claim-Cell:` trailer names and the diff's added `fn` ident byte-for-byte.

A patch is:

- a `git apply`-able **unified diff** over **production code only** - it may not edit a TEST region
  of any file it touches. The gate keeps this two ways: it refuses a touch of a file that is all
  test at HEAD (a `/tests/` target, a `#![cfg(test)]` or out-of-line `#[cfg(test)] mod` file), and
  for a MIXED file (production + an inline `#[cfg(test)] mod tests`) it applies the patch and then
  requires every `#[cfg(test)]` region of the post-image to be byte-identical to HEAD's, re-locating
  the regions by content on each image. A line-number comparison would be an exploit - a
  deletion-only hunk above a region shifts the numbers and lets a second hunk rewrite the cell's
  own assertion - so the comparison is of BYTES, and a production-line mutation of a mixed file
  stays a legitimate target, the shape an inline claim cell needs to patch its own file;
- **committed** with the declaring commit, so it is in the same `base..HEAD` range the trailer is
  and is reviewable in the diff;
- **killing**: applied in the isolated causality target and run against the named cell, it must make
  that cell FAIL by its OWN ASSERTION - a `panicked at <path>:<line>` in the cell's OWN file, inside
  the cell's own test fn (located by the fn's name on the post-image). A patch that does not apply,
  touches a test line or test region, kills only by a production panic, a downstream `.expect()`, a
  panic inside another file's test region (a shared helper), an exit/abort/signal, or a FAIL with no
  site at all, or leaves the cell green, refuses the whole arm. A `#[track_caller]` panic called
  DIRECTLY from the cell panics at the cell's own line and is not refused by any site rule - that
  shape is review-held, read in the patch, not claimed here.

The gate applies each patch in the isolated `target/causality-target`, runs the declared cell,
requires it to fail, restores the touched files, and accepts the arm only when every declared cell
is killed (`ok - claim cells: N declared, N killed`).
