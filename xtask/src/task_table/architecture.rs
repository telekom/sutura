//! Tasks that judge the SHAPE of first-party Rust: which way dependencies point, what a port
//! may declare, what a type admits, and what a refusal has to prove.
//!
//! The seam is the subject, not the cost: every row here reads `crates/` source and fails on a
//! structural claim, so a reviewer asking *did the hexagon hold* has one file to read.

use crate::registry::{Edit, Falsifier, Kind, Paired, Reads, Task};
use crate::{
    answer_path_cache, boot_order, boundaries, bounded_wait, catalog_opened_once, conformance, newtype_leaks, one_bound,
    orphan_modules, refusals, serde_parse, shared_client, threshold_expect, unsafe_containment, worktree_state,
};

/// `dependency_direction`'s own rule: `tokio` added to `sutura-domain` in its manifest and both
/// locks, so `cargo metadata --locked` still resolves and the domain now reaches a runtime.
const CHECK_BOUNDARIES_PAIRED: Paired = Paired {
    inputs: &["."],
    violation: &[
        Edit {
            path: "crates/sutura-domain/Cargo.toml",
            find: "\nsha2 = { workspace = true }\n",
            replace: "\nsha2 = { workspace = true }\ntokio = { workspace = true }\n",
        },
        Edit {
            path: "Cargo.lock",
            find: DOMAIN_LOCK_TAIL,
            replace: DOMAIN_LOCK_TAIL_TOKIO,
        },
        Edit {
            path: "fuzz/Cargo.lock",
            find: DOMAIN_LOCK_TAIL,
            replace: DOMAIN_LOCK_TAIL_TOKIO,
        },
    ],
};
const DOMAIN_LOCK_TAIL: &str = " \"secrecy\",\n \"serde\",\n \"serde_json\",\n \"sha2\",\n \"thiserror 2.0.21\",\n]";
const DOMAIN_LOCK_TAIL_TOKIO: &str =
    " \"secrecy\",\n \"serde\",\n \"serde_json\",\n \"sha2\",\n \"thiserror 2.0.21\",\n \"tokio\",\n]";

pub(crate) const TASKS: &[Task] = &[
    Task {
        name: "check-boundaries",
        description: "the domain crate depends on no framework",
        kind: Kind::Hygiene(Reads::Code),
        falsifier: Falsifier::paired(&CHECK_BOUNDARIES_PAIRED),
        run: boundaries::run,
    },
    Task {
        // The unreachable-public-module gate (issue #131, the "either way" slice). The sibling of
        // `unused-deps` in the other direction: that gate fails a DEPENDENCY no crate uses; this
        // fails a `pub` MODULE no first-party crate references. Reads the whole workspace, so a
        // consumer in another crate is seen. `Reads::Code` for the same reason `unused-deps` is.
        name: "check-unreachable-public-modules",
        description: "no pub module in a library crate that no first-party crate references",
        kind: Kind::Hygiene(Reads::Code),
        falsifier: Falsifier {
            // The five declared test-support exceptions all exist and stay unreferenced.
            // A sixth crate declares a public module with no production reference.
            seeds: &[
                ("Cargo.toml", "[workspace]\nresolver = \"3\"\nmembers = [\"crates/*\"]\n"),
                (
                    "crates/sutura-http-client/Cargo.toml",
                    "[package]\nname = \"sutura-http-client\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
                ),
                ("crates/sutura-http-client/src/lib.rs", "pub mod tls_test_support {}\n"),
                (
                    "crates/sutura-dev/Cargo.toml",
                    "[package]\nname = \"sutura-dev\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
                ),
                (
                    "crates/sutura-dev/src/lib.rs",
                    "pub mod bench_venue {}\npub mod tolerance {}\n",
                ),
                (
                    "crates/sutura-app/Cargo.toml",
                    "[package]\nname = \"sutura-app\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
                ),
                ("crates/sutura-app/src/lib.rs", "pub mod untrusted {}\n"),
                (
                    "crates/sutura-exec-datafusion/Cargo.toml",
                    "[package]\nname = \"sutura-exec-datafusion\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
                ),
                ("crates/sutura-exec-datafusion/src/lib.rs", "pub mod measurement {}\n"),
                (
                    "crates/sutura-domain/Cargo.toml",
                    "[package]\nname = \"sutura-domain\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
                ),
                ("crates/sutura-domain/src/lib.rs", "pub mod orphan {}\n"),
            ],
            in_scope: Some("crates/sutura-domain/src/lib.rs"),
            paired: None,
        },
        run: orphan_modules::run,
    },
    Task {
        // The third of the newtype rules held by a check, beside `check-serde-parse`. It starts
        // GREEN - there was no first-party `Deref` and no `Borrow` in the tree when it was written
        // - so its whole job is to keep it that way, which makes it the cheapest gate here and the
        // one most likely to earn its keep years from now.
        //
        // The falsifier here is the #371 proof case (`telekom/sutura#371`): before it, the shared
        // tree carried NO `.rs` file, so `run()` refused on its `scanned == 0` empty-scan floor
        // and never on the Deref rule - which is how a real-finding `Fail -> Pass` flip stayed
        // green (FlexGateVerify's item 5, measured live). The seed is a REAL first-party `impl
        // Deref`, so the refusal here must come from this gate's own rule, and the in-scope
        // proviso names the seeded file so a subject that never entered the scan is not accepted.
        name: "check-newtype-leaks",
        description: "no first-party Deref or Borrow - both leak a newtype's invariant",
        kind: Kind::Hygiene(Reads::Code),
        falsifier: Falsifier {
            seeds: &[(
                "src/leaky.rs",
                "struct Digest(String);\nimpl core::ops::Deref for Digest {\n    type Target = str;\n}\n",
            )],
            in_scope: Some("src/leaky.rs"),
            paired: None,
        },
        run: newtype_leaks::run,
    },
    Task {
        // Beside `check-newtype-leaks` for the same reason: it starts GREEN - no map in these two
        // crates holds an answer-path type, and the one credential cache this workspace ever had
        // lived inside an adapter and is now deleted - and its whole job is to keep it that way. Deferred
        // from `telekom/sutura#381`, owed by that record's own "what this leaves for later", and
        // `telekom/sutura#706` is where the deferral is tracked.
        //
        // The falsifier is a named-field struct whose only field is a `HashMap` holding a `RowSet` -
        // the shape a subject-keyed answer cache would take if one were added to either crate.
        name: "check-answer-path-caches",
        description: "no map or cache in sutura-app or sutura-domain holds a type read off the answer path",
        kind: Kind::Hygiene(Reads::Code),
        falsifier: Falsifier {
            seeds: &[(
                "crates/sutura-app/src/leaky_cache.rs",
                "struct SubjectCache {\n    seen: std::collections::HashMap<u8, sutura_domain::warehouse::RowSet>,\n}\n",
            )],
            in_scope: Some("crates/sutura-app/src/leaky_cache.rs"),
            paired: None,
        },
        run: answer_path_cache::run,
    },
    Task {
        // Beside `check-newtype-leaks` for its reason and for one more: the rule this holds used to
        // be held by the COMPILER alone, as `[workspace.lints.rust] unsafe_code = "forbid"`, and
        // `telekom/sutura#929`'s sixth finding had to relax it to `deny` so one crate could declare
        // the ADBC driver's C entrypoint - a static musl artefact has no dynamic loader, so carrying
        // its own driver is the only route it has. Cargo refuses a member that inherits the
        // workspace lints and overrides one entry, and `#[expect]` under an inherited `forbid` is
        // `E0453`, so there was no narrower change available.
        //
        // The falsifier seeds a whole one-member workspace, because this gate's subject is a crate
        // ROOT and the shared tree declares no members - without it the refusal would be the
        // empty-scan floor `falsifier`'s header names rather than this rule.
        name: "check-unsafe",
        description: "every crate root re-asserts forbid(unsafe_code), with one declared exception",
        kind: Kind::Hygiene(Reads::Code),
        falsifier: Falsifier {
            seeds: &[
                ("Cargo.toml", "[workspace]\nmembers = [\n  \"crates/unguarded\",\n]\n"),
                ("crates/unguarded/Cargo.toml", "[package]\nname = \"unguarded\"\n"),
                ("crates/unguarded/src/lib.rs", "pub fn reachable() {}\n"),
            ],
            in_scope: Some("crates/unguarded/src/lib.rs"),
            paired: None,
        },
        run: unsafe_containment::run,
    },
    Task {
        // Beside `check-newtype-leaks` because it is the same shape of gate: a rule the code cannot
        // state about itself, read as text, starting from a tree that already obeys it. This one is
        // the ORDER two composition roots keep - the pre-flight after the credential and before the
        // transport - which `github.com/telekom/sutura#120` asked to have pinned and which both roots
        // held in prose, one of them saying outright that it was "a convention this line keeps".
        name: "check-boot-order",
        description: "the pre-flight runs after the credential and before the transport",
        kind: Kind::Hygiene(Reads::Code),
        falsifier: Falsifier {
            // Both declared roots have call sites; only the HTTP root starts serving before
            // its pre-flight. The other root keeps the missing-call floors satisfied.
            seeds: &[
                (
                    "crates/sutura-cli/src/serve.rs",
                    "fn run() {\n    let opened = open_engine();\n    serve_until_stopped();\n    boot::refuse_absent_tables();\n}\n",
                ),
                (
                    "crates/sutura-cli/src/mcp.rs",
                    "fn run() {\n    let opened = open_engine();\n    boot::refuse_absent_tables();\n    sutura_mcp::serve_stdio();\n}\n",
                ),
            ],
            in_scope: Some("crates/sutura-cli/src/serve.rs"),
            paired: None,
        },
        run: boot_order::run,
    },
    Task {
        // Beside `check-boot-order` because it is the same shape of gate over the same files: a
        // property of a composition root that no signature can hold, read as text. That one holds an
        // ORDER of calls, this one holds a COUNT of them - `sutura_runtime::admission` says a process
        // builds one bound and, until `github.com/telekom/sutura#340`, nothing said it twice.
        name: "check-one-bound",
        description: "one composition root builds one execution bound, and a transport builds none",
        kind: Kind::Hygiene(Reads::Code),
        falsifier: Falsifier {
            // The constructor exists and all three transport takers are called. A single
            // composition root builds two independent bounds.
            seeds: &[
                ("crates/sutura-runtime/src/admission.rs", "pub fn from_settings() {}\n"),
                (
                    "crates/sutura-cli/src/main.rs",
                    "fn run() {\n    let first = Admission::from_settings(settings);\n    let second = Admission::from_settings(settings);\n    ServiceState::new(first);\n    AgentSurface::new(second);\n    serve_stdio(first);\n}\n",
                ),
            ],
            in_scope: Some("crates/sutura-cli/src/main.rs"),
            paired: None,
        },
        run: one_bound::run,
    },
    Task {
        // Beside `check-arrow` because it is the same shape of gate for the same reason: a MEASUREMENT
        // written into a record, checked against the lock it was taken from. `docs/adr/0018` says the
        // BigQuery wire costs zero new packages because `libduckdb-sys` already resolves the same
        // `ureq`; that record's own last consequence noted nothing gated it, which AGENTS.md calls a
        // wish rather than a rule. `docs/adr/0023`'s no-client property was the same shape a step
        // later: a measurement of one transport's feature closure held by review, with the record
        // itself saying a `reqwest` arriving would not trip the gate. The third rule is that
        // property, held against the lock now.
        name: "check-shared-client",
        description: "one `ureq` in the lock, still shared with `libduckdb-sys` (docs/adr/0018), and neither client docs/adr/0023 forbids",
        kind: Kind::Hygiene(Reads::Code),
        falsifier: Falsifier {
            // Two `ureq` versions, with `libduckdb-sys` still depending on one: the two-versions
            // rule is the ONLY arm this lock trips, so the refusal is that rule's.
            seeds: &[(
                "Cargo.lock",
                concat!(
                    "[[package]]\nname = \"libduckdb-sys\"\nversion = \"1.0.0\"\ndependencies = [\n \"ureq\",\n]\n\n",
                    "[[package]]\nname = \"ureq\"\nversion = \"3.4.0\"\n\n",
                    "[[package]]\nname = \"ureq\"\nversion = \"4.0.0\"\n",
                ),
            )],
            in_scope: Some("Cargo.lock"),
            paired: None,
        },
        run: shared_client::run,
    },
    Task {
        // Beside the boundary gate because it is the same principle in the same shape: a rule from
        // one of the four sources `AGENTS.md` adopts as policy, held by a check rather than by a
        // sentence. `check-boundaries` owns the dependency direction and the typed surface; this
        // one owns the two serde rules that were *review* in the Rust skill's own table.
        name: "check-serde-parse",
        description: "a validated newtype's serde goes through its constructor, both ways",
        kind: Kind::Hygiene(Reads::Code),
        falsifier: Falsifier {
            seeds: &[(
                "choice.rs",
                "#[derive(serde::Deserialize)]\npub enum Choice { Empty }\nimpl Choice {\n    pub fn parse(raw: &str) -> Result<Self, Bad> { todo!() }\n}\n",
            )],
            in_scope: Some("choice.rs"),
            paired: None,
        },
        run: serde_parse::run,
    },
    Task {
        // Beside `check-serde-parse` because it is the third rule from the same page held by the
        // same kind of check - and this one is about the ERROR principle rather than the newtype
        // one. Name coverage is narrower than proving a test actually provokes the refusal.
        name: "check-refusal-coverage",
        description: "every variant of an enrolled refusal enum is named, or separately excused with a date and reason",
        kind: Kind::Hygiene(Reads::Code),
        falsifier: Falsifier {
            // Each snapshot names only part of each enum, so neither is a census.
            // T6 is the sole uncovered variant; the enrolled counts must still agree.
            seeds: &[
                (
                    "crates/sutura-domain/src/query.rs",
                    "pub enum RefusalReason {\n    R0,\n    R1,\n    R2,\n    R3,\n    R4,\n    R5,\n    R6,\n    R7,\n    R8,\n    R9,\n    R10,\n    R11,\n    R12,\n    R13,\n    R14,\n    R15,\n    R16,\n    R17,\n    R18,\n    R19,\n    R20,\n    R21,\n    R22,\n    R23,\n    R24,\n    R25,\n    R26,\n    R27,\n    R28,\n    R29,\n}\n",
                ),
                (
                    "crates/sutura-config/src/settings/posture.rs",
                    "pub enum NotFitToServe {\n    S0,\n    S1,\n    S2,\n    S3,\n    S4,\n    S5,\n    S6,\n    S7,\n    S8,\n    S9,\n    S10,\n    S11,\n    S12,\n    S13,\n    S14,\n    S15,\n}\n",
                ),
                (
                    "crates/sutura-domain/src/pinned.rs",
                    "pub enum NotValidated {\n    V0,\n    V1,\n    V2,\n    V3,\n    V4,\n    V5,\n    V6,\n}\n",
                ),
                (
                    "crates/sutura-domain/src/raw.rs",
                    "pub enum RawRefusalReason {\n    W0,\n    W1,\n    W2,\n    W3,\n}\n",
                ),
                (
                    "crates/sutura-http/src/tls.rs",
                    "pub enum TlsNotUsable {\n    T0,\n    T1,\n    T2,\n    T3,\n    T4,\n    T5,\n    T6,\n}\n",
                ),
                (
                    "crates/example/tests/first.snap",
                    "R0 R1 R2 R3 R4 R5 R6 R7 R8 R9 R10 R11 R12 R13 R14 S0 S1 S2 S3 S4 S5 S6 S7 V0 V1 V2 W0 W1 T0 T1 T2\n",
                ),
                (
                    "crates/example/tests/second.snap",
                    "R15 R16 R17 R18 R19 R20 R21 R22 R23 R24 R25 R26 R27 R28 R29 S8 S9 S10 S11 S12 S13 S14 S15 V3 V4 V5 V6 W2 W3 T3 T4 T5\n",
                ),
            ],
            in_scope: Some("crates/sutura-http/src/tls.rs"),
            paired: None,
        },
        run: refusals::run,
    },
    Task {
        // The third of that shape, and the one whose failure mode is the most expensive to
        // diagnose: a gate hanging with no output. The compose tier routes every wait on a
        // container-runtime child through one function that carries a deadline, and nothing made
        // the NEXT call come through it - a `.output()` written into a sibling compiles, reviews
        // clean and restores the hang, and so does a pipe handed to a child and drained to an EOF
        // that never comes. `disallowed-methods` cannot express it, because an entry there is
        // workspace-wide and `xtask` waits on `git` and `cargo` without a bound on purpose. So it
        // is path-scoped, and it starts GREEN.
        name: "check-bounded-wait",
        description: "one place in the compose tier can be blocked by a child process",
        kind: Kind::Hygiene(Reads::Code),
        falsifier: Falsifier {
            // One legitimate docker waiter and the declared lsof allowance satisfy the two
            // witness floors. The second waiter in a sibling is the own-rule violation.
            seeds: &[
                ("xtask/src/compose.rs", "mod docker;\n"),
                (
                    "xtask/src/compose/docker.rs",
                    "fn bounded(child: &mut std::process::Child) { let _ = child.wait(); }\n",
                ),
                (
                    "xtask/src/compose/lock.rs",
                    "fn owner(command: &mut std::process::Command) { let _ = command.output(); }\n",
                ),
                (
                    "xtask/src/compose/extra.rs",
                    "fn unbounded(command: &mut std::process::Command) { let _ = command.status(); }\n",
                ),
            ],
            in_scope: Some("xtask/src/compose/extra.rs"),
            paired: None,
        },
        run: bounded_wait::run,
    },
    Task {
        // `telekom/sutura#405`, and it belongs with the two above rather than with the naming gates:
        // the rule is about a DIRECTORY two things reach and neither may assume, which is
        // `check-warm-start`'s subject one level up. Six collisions were measured in one day and
        // nothing held any of them - a path with no key in it is one path for every checkout on the
        // machine, and the failure is a confident wrong verdict rather than an error. It starts
        // GREEN, over a tree whose one live instance this change fixes.
        name: "check-worktree-state",
        description: "no test or gate writes to a path a second worktree also reaches",
        kind: Kind::Hygiene(Reads::Code),
        falsifier: Falsifier {
            seeds: &[],
            in_scope: Some("nix/shared-scratch.sh"),
            paired: None,
        },
        run: worktree_state::run,
    },
    Task {
        // Beside `check-bounded-wait` because it is the fourth of that shape: a rule the code
        // cannot state about itself, read as text. What it holds is the half `telekom/sutura#116`
        // could not - the packs are bound from an adapter's own crate, so WHICH adapters are held
        // was a reading of which crates carry a `tests/conformance.rs`, and deleting one left
        // `just validate` green. It shipped with ONE declared exemption, because
        // `docs/adr/0012`'s *one registration, not two* was violated as built; that entry was
        // `postgres` and `telekom/sutura#348` deleted it by binding the adapter, so the list is
        // empty and an exemption is now an architecture decision with nothing to hide behind.
        name: "check-conformance-bindings",
        description: "every registered data system is bound to the conformance packs, or declared unbound",
        kind: Kind::Hygiene(Reads::Code),
        falsifier: Falsifier {
            // The declared Oracle exception is live. A second registered data system has a
            // resolvable crate and no binding, leaving that own-rule finding as the refusal.
            seeds: &[
                (
                    "crates/sutura-exec-oracle/Cargo.toml",
                    "[package]\nname = \"sutura-exec-oracle\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
                ),
                (
                    "crates/sutura-exec-mim/Cargo.toml",
                    "[package]\nname = \"sutura-exec-mim\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
                ),
                (
                    "crates/sutura-conformance/Cargo.toml",
                    "[package]\nname = \"sutura-conformance\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
                ),
                (
                    "crates/sutura-app/tests/adapters/adapters.rs",
                    "macro_rules! adapters {\n    (data_systems: $cell:ident) => {\n        $cell!(oracle, sutura_exec_oracle::OracleWarehouse);\n        $cell!(mim, sutura_exec_mim::MimWarehouse);\n    };\n}\n",
                ),
                (
                    "crates/sutura-conformance/src/lib.rs",
                    "macro_rules! execute_packs { () => {} }\n",
                ),
            ],
            in_scope: Some("crates/sutura-app/tests/adapters/adapters.rs"),
            paired: None,
        },
        run: conformance::run,
    },
    Task {
        // Beside `check-answer-path-caches` because it is the same shape: a rule the code cannot
        // state about itself, read as text, starting from a tree that already obeys it. What it
        // holds is "each catalog document is opened once" - the property the file catalogs'
        // own comments said was held by review, because a swap-timing test cannot land in the
        // sub-microsecond window between two back-to-back opens. This gate is the static half:
        // no path-based `std::fs` read outside the registered `read_dir` walk.
        //
        // The falsifier seeds a `std::fs::read_to_string(&path)` into the local catalog's
        // library root - the exact mutation the comment described - so the refusal comes from this
        // gate's own rule rather than from a floor. Two kinds of scaffolding keep the floors from
        // firing first: the workspace `Cargo.toml` and the local catalog's `Cargo.toml` and
        // `lib.rs` give `scope_from_metadata` a `sutura-catalog-*` member to scan, and
        // `sutura-bounded-read`'s `walk.rs` and wren's `lib.rs` are the `REGISTERED` anchors whose
        // absence makes the census refuse as not judged.
        name: "check-catalog-opened-once",
        description: "no path-based std::fs read in the catalog crates outside the registered walk",
        kind: Kind::Hygiene(Reads::Code),
        falsifier: Falsifier {
            seeds: &[
                ("Cargo.toml", "[workspace]\nmembers = [\"crates/sutura-catalog-local\"]\n"),
                (
                    "crates/sutura-bounded-read/src/walk.rs",
                    "pub fn walk() {\n    let _ = std::fs::read_dir(&dir);\n}\n",
                ),
                (
                    "crates/sutura-catalog-local/Cargo.toml",
                    "[package]\nname = \"sutura-catalog-local\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
                ),
                ("crates/sutura-catalog-local/src/lib.rs", "pub fn f() {}\n"),
                (
                    "crates/sutura-catalog-local/src/leaky_read.rs",
                    "fn f(path: &std::path::Path) {\n    let _ = std::fs::read_to_string(&path);\n}\n",
                ),
                (
                    "crates/sutura-catalog-wren/src/lib.rs",
                    "pub fn import() {\n    let _ = std::fs::read_to_string(&path);\n    let _ = std::fs::read_dir(&dir);\n}\n",
                ),
            ],
            in_scope: Some("crates/sutura-catalog-local/src/leaky_read.rs"),
            paired: None,
        },
        run: catalog_opened_once::run,
    },
    Task {
        // A threshold lint's cause is a NUMBER, which is a property of the surrounding
        // function rather than of the code the attribute sits on - so two branches can each
        // move that number correctly and only their merge is wrong. See the module doc.
        name: "check-expect-thresholds",
        description: "no #[expect] on a count-threshold lint (too_many_lines / too_many_arguments / cognitive_complexity)",
        kind: Kind::Hygiene(Reads::Code),
        falsifier: Falsifier {
            // The required Rust anchor is present; its attribute is the forbidden finding.
            seeds: &[(
                "xtask/src/main.rs",
                concat!(
                    "#[",
                    "expect(",
                    "clippy::too_many_lines, reason = \"falsifier\")]\n",
                    "fn oversized() {}\n",
                ),
            )],
            in_scope: Some("xtask/src/main.rs"),
            paired: None,
        },
        run: threshold_expect::run,
    },
];
