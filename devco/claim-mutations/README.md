# Claim-cell mutation patches

The causality gate's claim arm reads ONE patch per declared claim cell from this directory. A
`Claim-Cell: <test-fn-name>` commit trailer is a claim; a patch here is the mechanism that holds it.

Each file must be named `<test-fn-name>.patch`, matching the exact function ident the commit's
`Claim-Cell:` trailer names and the diff's added `fn` ident byte-for-byte.

A patch is:

- a `git apply`-able **unified diff** over **production code only** - it may not add a line inside
  a TEST REGION of any file it touches (a `/tests/` target, a `#![cfg(test)]` or out-of-line
  `#[cfg(test)] mod` file, or a hunk inside a `#[cfg(test)]` region of a mixed file, read with the
  same post-image rule `plan` uses), because the whole point is to break the behaviour the cell
  claims, and a patch that edits the test to fail proves nothing. A MIXED file (production + an
  inline `#[cfg(test)] mod tests`) is a legitimate target in its production lines - the shape an
  inline claim cell needs to patch its own file;
- **committed** with the declaring commit, so it is in the same `base..HEAD` range the trailer is
  and is reviewable in the diff;
- **killing**: applied in the isolated causality target and run against the named cell, it must make
  that cell FAIL by its OWN ASSERTION - a `panicked at <path>:<line>` inside the cell's own test
  region. A patch that does not apply, touches a test line, kills only by a production panic, a
  downstream `.expect()`, a `#[track_caller]` relocation, an exit/abort/signal, or a FAIL with no
  site at all, or leaves the cell green, refuses the whole arm.

The gate applies each patch in the isolated `target/causality-target`, runs the declared cell,
requires it to fail, restores the touched files, and accepts the arm only when every declared cell
is killed (`ok - claim cells: N declared, N killed`).
