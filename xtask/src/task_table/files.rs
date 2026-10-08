//! Tasks that judge a file AS A FILE - length, duplication, line endings, trailing whitespace,
//! complexity weighed against coverage - independent of what language it is written in.
//!
//! Separate from `architecture` because these rules do not read meaning. `max-lines` is the cap
//! that moved this table out of `main.rs`, and its `ok` verdict names the files closest to
//! refusing next - `github.com/telekom/sutura#626`.

use crate::registry::{Falsifier, Kind, Reads, Task};
use crate::{crap, ignored_tests, jscpd, line_endings, max_lines, text};

/// A Rust clone above the gate's 30-line and 250-token thresholds.
const JSCPD_CLONE: &str = "pub fn calculate(input: usize) -> usize {
    let value_0 = input + 0;
    let value_1 = input + 1;
    let value_2 = input + 2;
    let value_3 = input + 3;
    let value_4 = input + 4;
    let value_5 = input + 5;
    let value_6 = input + 6;
    let value_7 = input + 7;
    let value_8 = input + 8;
    let value_9 = input + 9;
    let value_10 = input + 10;
    let value_11 = input + 11;
    let value_12 = input + 12;
    let value_13 = input + 13;
    let value_14 = input + 14;
    let value_15 = input + 15;
    let value_16 = input + 16;
    let value_17 = input + 17;
    let value_18 = input + 18;
    let value_19 = input + 19;
    let value_20 = input + 20;
    let value_21 = input + 21;
    let value_22 = input + 22;
    let value_23 = input + 23;
    let value_24 = input + 24;
    let value_25 = input + 25;
    let value_26 = input + 26;
    let value_27 = input + 27;
    let value_28 = input + 28;
    let value_29 = input + 29;
    let value_30 = input + 30;
    let value_31 = input + 31;
    let value_32 = input + 32;
    let value_33 = input + 33;
    let value_34 = input + 34;
    let value_35 = input + 35;
    let value_36 = input + 36;
    let value_37 = input + 37;
    let value_38 = input + 38;
    let value_39 = input + 39;
    let value_40 = input + 40;
    let value_41 = input + 41;
    let value_42 = input + 42;
    let value_43 = input + 43;
    let value_44 = input + 44;
    value_0 + value_1
}
";

pub(crate) const TASKS: &[Task] = &[
    Task {
        name: "max-lines",
        description: "no file over 1000 lines (exemptions: devco/max-lines-ignore)",
        kind: Kind::Hygiene(Reads::Prose),
        falsifier: Falsifier {
            seeds: &[],
            in_scope: Some("over-long.txt"),
            paired: None,
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
        falsifier: Falsifier {
            // Seed every input the scanner needs, then present one unexcused clone.
            seeds: &[
                ("devco/jscpd.json", "{\"ignore\": []}\n"),
                ("devco/dup-ignore", "# no clone exceptions\n"),
                (
                    "nix/run-gate.sh",
                    "nix run .#jscpd -- --format rust --min-lines 30 --min-tokens 250\n",
                ),
                ("crates/example/src/first.rs", JSCPD_CLONE),
                ("crates/example/src/second.rs", JSCPD_CLONE),
            ],
            in_scope: Some("crates/example/src/second.rs"),
            paired: None,
        },
        run: jscpd::run,
    },
    Task {
        name: "line-endings",
        description: "every text file uses LF, not CRLF",
        kind: Kind::Hygiene(Reads::Prose),
        falsifier: Falsifier {
            seeds: &[],
            in_scope: Some("carriage-return.txt"),
            paired: None,
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
            paired: None,
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
        falsifier: Falsifier {
            // A valid one-member workspace and matching pin/page leave the unannotated
            // allowlist entry as the sole refusal.
            seeds: &[
                ("Cargo.toml", "[workspace]\nmembers = [\"crates/sutura-domain\"]\n"),
                (
                    "crates/sutura-domain/Cargo.toml",
                    "[package]\nname = \"sutura-domain\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
                ),
                ("crates/sutura-domain/src/lib.rs", "pub fn fixture() {}\n"),
                (
                    ".cargo-crap.toml",
                    "threshold = 30\nfail-above = true\nallow = [\n  \"sutura-domain::*\",\n]\n",
                ),
                ("nix/crap.nix", "crapVersion = \"0.4.3\";\n"),
                ("docs/crap.md", "cargo-crap 0.4.3\n"),
            ],
            in_scope: Some(".cargo-crap.toml"),
            paired: None,
        },
        run: crap::run_check,
    },
    Task {
        // An `#[ignore]` on a live test takes it out of the default suite and nothing said so.
        // `Reads::Code`: no `docs/*.md` diff can add one.
        name: "check-ignored-tests",
        description: "every #[ignore]d test is listed in devco/ignored-tests, and every line there is one",
        kind: Kind::Hygiene(Reads::Code),
        falsifier: Falsifier {
            seeds: &[
                ("xtask/src/main.rs", "#[test]\n#[ignore]\nfn ignored() {}\n"),
                ("devco/ignored-tests", "# No ignored tests are baselined.\n"),
            ],
            in_scope: Some("xtask/src/main.rs"),
            paired: None,
        },
        run: ignored_tests::run,
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
