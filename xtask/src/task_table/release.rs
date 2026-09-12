//! Tasks that judge PROVENANCE and AUTHORSHIP: what a release records about where it came
//! from, who owns a change, the shape of a commit subject, and retiring merged branches.
//!
//! Its own area because these rows are the only ones whose subject is a published artefact or
//! the history itself, rather than the working tree.

use crate::registry::{Falsifier, Kind, Reads, Task};
use crate::{attribution, branches, commit_msg, release_provenance};

pub(crate) const TASKS: &[Task] = &[
    Task {
        name: "collect-provenance",
        description: "export five release attestation bundles after exact subject-set checks",
        kind: Kind::Standalone,
        falsifier: Falsifier::declared_in_programme(),
        run: release_provenance::run,
    },
    Task {
        // AN ABSENCE GATE, which is a different shape from the two beside it, and the reason it has
        // to exist is that an absence nothing witnesses silently returns. `ATTRIBUTION.md` is
        // generated and deliberately NOT committed - `github.com/telekom/sutura#462`'s decision,
        // because a committed copy fell behind `Cargo.lock` on every dependency bump and made each
        // one red on arrival - and a commit re-adding it would restore all of that with the next
        // tag signing the stale copy. `xtask/src/workflows/sast.rs` is the local precedent.
        //
        // `Reads::Code` covers workflow YAML, which is the second half: with nothing committed, the
        // release is the ONLY place the notice is produced, so a release that stopped generating it
        // would publish binaries with no attribution.
        name: "check-attribution-owner",
        description: "no committed ATTRIBUTION.md, and release.yml generates the attribution asset",
        kind: Kind::Hygiene(Reads::Code),
        falsifier: Falsifier::declared_in_programme(),
        run: attribution::run_check_owner,
    },
    Task {
        // `check-api-docs` is the shape it copies including why it is not in the hygiene sweep: it
        // needs an input the nix sandbox has not got - a compiler there, a resolvable registry
        // here. **It cannot byte-compare, because nothing is committed to compare against**, so
        // the oracle is the disagreement between two independent inputs: `Cargo.lock` decides the
        // package set and `cargo metadata` supplies the licences. It refuses a crate that declares
        // no licence, which the generator used to print and pass.
        name: "check-attribution",
        description: "a generation names every third-party crate in Cargo.lock with a declared licence (needs a resolvable registry)",
        kind: Kind::Standalone,
        falsifier: Falsifier::declared_in_programme(),
        run: attribution::run_check,
    },
    Task {
        // NOT `Kind::Hygiene`, and for `check-api-docs`' reason rather than its own: it invokes
        // `cargo metadata`, which needs a resolvable registry, and the hygiene sweep runs inside a
        // nix sandbox with no network. `check-attribution-owner` is the half that runs everywhere,
        // and it reads a path's absence precisely so it can.
        //
        // Takes an optional destination, because there are two callers and neither wants a
        // committed file: `just attribution` uses the default under `/target`, and `release.yml`
        // names the release's own asset directory.
        name: "attribution",
        description: "write the attribution document from cargo metadata (needs a resolvable registry)",
        kind: Kind::Standalone,
        falsifier: Falsifier::declared_in_programme(),
        run: attribution::run_generate,
    },
    Task {
        name: "commit-msg",
        description: "the commit subject is a conventional commit (the hook passes the file)",
        kind: Kind::Standalone,
        falsifier: Falsifier::declared_in_programme(),
        run: commit_msg::run,
    },
    Task {
        // `Kind::Standalone`, and for the reason the compose tier is: this is an ACTION. It takes
        // flags, and under `--delete` it removes worktrees and branches - neither of which belongs
        // in a sweep that runs on every commit. It also asks the forge, which needs the network the
        // Nix sandbox does not have; a silent forge makes it delete nothing, so collecting it would
        // have added a gate that can only ever report that it could not decide.
        name: "clean-branches",
        description: "branches and worktrees whose work has landed; DRY RUN unless --delete",
        kind: Kind::Standalone,
        falsifier: Falsifier::declared_in_programme(),
        run: branches::run,
    },
];
