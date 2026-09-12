//! Tasks a developer INVOKES to change or run the tree: the local data tiers, formatting, the
//! causality proof, the fuzzer, and the `hygiene` sweep that collects every gate above.
//!
//! The seam is the caller: these rows take arguments, start processes or rewrite files, which is
//! also what keeps `hygiene` from recursing into itself - it is `Kind::Standalone`.

use crate::registry::{Falsifier, Kind, Reads, Task};
use crate::run_hygiene;
use crate::{causality, compose, fmt, fuzz};

pub(crate) const TASKS: &[Task] = &[
    Task {
        // The compose (docker) tier. `Kind::Standalone`, and not for the usual reason: these are
        // not expensive, they are ACTIONS - they start and remove containers on the host. The
        // hygiene sweep runs on every commit and inside the Nix sandbox, which has neither a
        // network nor a docker socket, so a DOCKER tier collected into it could not run and must
        // not try. (A service needing neither, like the sandbox's Unix-socket Postgres, can be a
        // check - see `nix/postgres-tier.nix`.)
        name: "dev-up",
        description: "this worktree's services, on ephemeral ports, with a discovery file (needs docker)",
        kind: Kind::Standalone,
        falsifier: Falsifier::declared_in_programme(),
        run: compose::run_up,
    },
    Task {
        name: "dev-down",
        description: "remove this worktree's services and volumes; --dry-run says what it would take",
        kind: Kind::Standalone,
        falsifier: Falsifier::declared_in_programme(),
        run: compose::run_down,
    },
    Task {
        name: "dev-endpoints",
        description: "where this worktree's services are listening, from the discovery file",
        kind: Kind::Standalone,
        falsifier: Falsifier::declared_in_programme(),
        run: compose::run_endpoints,
    },
    Task {
        // The SINGULAR one, and it is not a convenience duplicate of the plural: it prints
        // `host:port` on stdout and nothing else, so it substitutes into a shell. That is what lets
        // a reader following `examples/` reach a provisioned service without learning what a scope
        // or an ephemeral port is.
        name: "dev-endpoint",
        description: "one service's host:port on stdout, for a shell to substitute; <service>",
        kind: Kind::Standalone,
        falsifier: Falsifier::declared_in_programme(),
        run: compose::run_endpoint,
    },
    Task {
        name: "fmt",
        description: "cargo fmt, scoped to our packages (--check to verify)",
        kind: Kind::Standalone,
        falsifier: Falsifier::declared_in_programme(),
        run: fmt::run,
    },
    Task {
        name: "test-causality",
        description: "a changed test is red on base, green on head; --since <ref>",
        kind: Kind::Standalone,
        falsifier: Falsifier::declared_in_programme(),
        run: causality::run,
    },
    Task {
        name: "check-fuzz",
        description: "every fuzz target is declared, seeded, and run by the workflow",
        kind: Kind::Hygiene(Reads::Code),
        falsifier: Falsifier::declared_in_programme(),
        run: fuzz::run,
    },
    Task {
        name: "hygiene",
        description: "every cheap structural gate, in order (the one list)",
        kind: Kind::Standalone,
        falsifier: Falsifier::declared_in_programme(),
        run: run_hygiene,
    },
];
