//! Tasks that judge a file AS A FILE - length, duplication, line endings, trailing whitespace,
//! complexity weighed against coverage - independent of what language it is written in.
//!
//! Separate from `architecture` because these rules do not read meaning. `max-lines` is the cap
//! that moved this table out of `main.rs`; it does not report how close any file is to refusing.

use crate::registry::{Falsifier, Kind, Reads, Task};
use crate::{crap, jscpd, line_endings, max_lines, text};

pub(crate) const TASKS: &[Task] = &[
    Task {
        name: "max-lines",
        description: "no file over 1000 lines (exemptions: devco/max-lines-ignore)",
        kind: Kind::Hygiene(Reads::Prose),
        falsifier: Falsifier {
            seeds: &[],
            in_scope: Some("over-long.txt"),
        },
        run: max_lines::run,
    },
    Task {
        // The jscpd copy/paste gate (issue #474). `Reads::Code`, so a `docs/*.md`-only diff
        // stays excluded from the docs.yml skip. See the module header for why it FAILS CLOSED
        // when `jscpd` is absent - locally and in the nix sandbox - and for the allowlist
        // contract.
        name: "check-jscpd",
        description: "no copied block in crates/ or xtask/ without a reason in devco/dup-ignore",
        kind: Kind::Hygiene(Reads::Code),
        falsifier: Falsifier::declared_in_programme(),
        run: jscpd::run,
    },
    Task {
        name: "line-endings",
        description: "every text file uses LF, not CRLF",
        kind: Kind::Hygiene(Reads::Prose),
        falsifier: Falsifier {
            seeds: &[],
            in_scope: Some("carriage-return.txt"),
        },
        run: line_endings::run,
    },
    Task {
        name: "text-hygiene",
        description: "conflict markers, whitespace, final newline, file size; --fix",
        kind: Kind::Hygiene(Reads::Prose),
        falsifier: Falsifier {
            seeds: &[],
            in_scope: Some("carriage-return.txt"),
        },
        run: text::run,
    },
    Task {
        // CHEAP HALF of the CRAP gate: it reads `.cargo-crap.toml`, checks the allowlist
        // discipline and the scope, and compiles nothing. The expensive half is `crap` below, and
        // the split is the same one `check-api-docs` is kept out of hygiene for - this sweep runs
        // on every commit and inside the Nix sandbox, so nothing in it may need a coverage build.
        name: "check-crap",
        description: "the CRAP policy is a gate, its allowlist annotated, its scope real",
        kind: Kind::Hygiene(Reads::Prose),
        falsifier: Falsifier::declared_in_programme(),
        run: crap::run_check,
    },
    Task {
        // NOT `Kind::Hygiene`, for the same reason as `check-api-docs`: it COMPILES, with
        // `-C instrument-coverage`, into a profile that shares nothing with the cached one, and
        // it needs two tools the cheap sweep must not require. `check-crap` above is the part
        // that runs everywhere.
        name: "crap",
        description: "CRAP score over the scoped crates (COMPILES; needs llvm-cov and crap)",
        kind: Kind::Standalone,
        falsifier: Falsifier::declared_in_programme(),
        run: crap::run,
    },
    Task {
        // The DELTA half, and standalone for a different reason from the two above: it needs no
        // compiler and no tool at all, but it needs a BASELINE - a file produced by a `crap` run
        // on the base commit, which in CI arrives over the network from an artifact. A hygiene
        // task must run in the Nix sandbox, and a sandbox has no network, so this cannot be one.
        //
        // It costs no second coverage run. Both sides are baselines earlier `crap` runs already
        // wrote; this reads two files and joins them. See `crap::delta` for the three rules and
        // for why a single sub-threshold regression is reported rather than failed.
        name: "crap-delta",
        description: "did the CHANGE make anything worse; --baseline <F> --head <F> [--comment <F>]",
        kind: Kind::Standalone,
        falsifier: Falsifier::declared_in_programme(),
        run: crap::run_delta,
    },
];
