//! Tasks that judge the SHAPE of first-party Rust: which way dependencies point, what a port
//! may declare, what a type admits, and what a refusal has to prove.
//!
//! The seam is the subject, not the cost: every row here reads `crates/` source and fails on a
//! structural claim, so a reviewer asking *did the hexagon hold* has one file to read.

use crate::registry::{Falsifier, Kind, Reads, Task};
use crate::{
    boot_order, boundaries, bounded_wait, conformance, newtype_leaks, one_bound, orphan_modules, refusals, serde_parse,
    shared_client, threshold_expect, worktree_state,
};

pub(crate) const TASKS: &[Task] = &[
    Task {
        name: "check-boundaries",
        description: "the domain crate depends on no framework",
        kind: Kind::Hygiene(Reads::Code),
        falsifier: Falsifier::declared_in_programme(),
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
        falsifier: Falsifier::declared_in_programme(),
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
        },
        run: newtype_leaks::run,
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
        falsifier: Falsifier::declared_in_programme(),
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
        falsifier: Falsifier::declared_in_programme(),
        run: one_bound::run,
    },
    Task {
        // Beside `check-arrow` because it is the same shape of gate for the same reason: a MEASUREMENT
        // written into a record, checked against the lock it was taken from. `docs/adr/0018` says the
        // BigQuery wire costs zero new packages because `libduckdb-sys` already resolves the same
        // `ureq`; that record's own last consequence noted nothing gated it, which AGENTS.md calls a
        // wish rather than a rule.
        name: "check-shared-client",
        description: "one `ureq` in the lock, still shared with `libduckdb-sys` (docs/adr/0018)",
        kind: Kind::Hygiene(Reads::Code),
        falsifier: Falsifier::declared_in_programme(),
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
        falsifier: Falsifier::declared_in_programme(),
        run: serde_parse::run,
    },
    Task {
        // Beside `check-serde-parse` because it is the third rule from the same page held by the
        // same kind of check - and this one is about the ERROR principle rather than the newtype
        // one. Name coverage is narrower than proving a test actually provokes the refusal.
        name: "check-refusal-coverage",
        description: "every variant of an enrolled refusal enum is named, or separately excused with a date and reason",
        kind: Kind::Hygiene(Reads::Code),
        falsifier: Falsifier::declared_in_programme(),
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
        falsifier: Falsifier::declared_in_programme(),
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
        falsifier: Falsifier::declared_in_programme(),
        run: conformance::run,
    },
    Task {
        // A threshold lint's cause is a NUMBER, which is a property of the surrounding
        // function rather than of the code the attribute sits on - so two branches can each
        // move that number correctly and only their merge is wrong. See the module doc.
        name: "check-expect-thresholds",
        description: "no #[expect] on a count-threshold lint (too_many_lines / too_many_arguments / cognitive_complexity)",
        kind: Kind::Hygiene(Reads::Code),
        falsifier: Falsifier::declared_in_programme(),
        run: threshold_expect::run,
    },
];
