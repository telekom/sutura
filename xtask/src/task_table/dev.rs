//! Tasks a developer INVOKES to change or run the tree: the local data tiers, formatting, the
//! causality proof, the fuzzer, and the `hygiene` sweep that collects every gate above.
//!
//! The seam is the caller: these rows take arguments, start processes or rewrite files, which is
//! also what keeps `hygiene` from recursing into itself - it is `Kind::Standalone`.

use crate::registry::{Falsifier, Kind, Reads, Task};
use crate::run_hygiene;
use crate::{causality, compose, fmt, fuzz};

/// The falsifier seed for `check-claim-mutations` (`github.com/telekom/sutura#950`): a committed
/// patch that is not a `git apply`-able diff at all, so the gate's own rule - not an absent
/// `devco/claim-mutations/` directory, which is a legitimate pass - is what fires over the seeded
/// tree.
const ROTTED_CLAIM_MUTATION_SEED: &str = "devco/claim-mutations/a_falsifier_seed_that_never_applies.patch";

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
        // The QUESTION, next to the action that needs it answered. It removes nothing, and its
        // whole contribution is the exit code: 0 clear, 1 this worktree holds resources, 3 the
        // runtime did not say. Named for the SAFE direction on purpose - a task that exited 0 when
        // a project was running would be called with `!`, and `!` turns the unknown code into a
        // success, so the one answer that must never read as "nothing there" would be the one
        // that does.
        name: "dev-clear",
        description: "is this worktree clear of compose resources? 0 clear, 1 holds, 3 unknown (needs docker)",
        kind: Kind::Standalone,
        falsifier: Falsifier::declared_in_programme(),
        run: compose::run_clear,
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
        // THE APPLY HALF (`github.com/telekom/sutura#950`): `git apply --check` needs a work tree
        // and nothing else - no compiler, no test run - so this is cheap enough for `hygiene`.
        // `causality::rot`'s header carries the residual this does NOT cover: a patch that still
        // applies but no longer kills its cell is [`check-claim-mutation-kills`] below, not this.
        name: "check-claim-mutations",
        description: "every committed devco/claim-mutations/*.patch still applies against the work tree",
        kind: Kind::Hygiene(Reads::Code),
        falsifier: Falsifier {
            seeds: &[(ROTTED_CLAIM_MUTATION_SEED, "not a git diff at all\n")],
            in_scope: Some(ROTTED_CLAIM_MUTATION_SEED),
        },
        run: causality::rot::check_apply,
    },
    Task {
        // THE KILL HALF. `Standalone`, not `hygiene`: it recompiles this workspace once per
        // committed patch - `causality::claim`'s own ~68s isolated rebuild, paid once per cell -
        // so folding it into the sweep that runs on every commit would multiply that by however
        // many `devco/claim-mutations/` holds. An on-demand task for the release path, or for a
        // person re-verifying the set after touching something nearby.
        name: "check-claim-mutation-kills",
        description: "every committed devco/claim-mutations/*.patch still kills its named cell",
        kind: Kind::Standalone,
        falsifier: Falsifier::declared_in_programme(),
        run: causality::rot::run,
    },
    Task {
        name: "hygiene",
        description: "every cheap structural gate, in order (the one list)",
        kind: Kind::Standalone,
        falsifier: Falsifier::declared_in_programme(),
        run: run_hygiene,
    },
];
