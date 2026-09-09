//! Repo automation. Run as `cargo xtask <task>`, or `cargo run -q -p xtask -- <task>`.
//!
//! These are gates, not conveniences: each answers "what fails if this rule is violated?"
//! with a non-zero exit and a count, not with a paragraph of guidance. A rule that lands here
//! instead of in a document is a rule that cannot rot unnoticed.
//!
//! Gates live in one binary rather than a script per check: one thing to install, one language
//! to review, and they are unit-tested by `cargo nextest run --workspace` like any other code.

mod action_shell;
mod api_docs;
mod api_links;
mod arrow_major;
mod attribution;
mod boot_order;
mod boundaries;
mod bounded_wait;
mod branches;
mod causality;
mod changes;
mod commit_msg;
mod compose;
mod conformance;
mod crap;
mod default_feature_tests;
mod default_features;
mod devenv_linter;
mod devenv_shell;
mod docs;
mod examples;
#[cfg(test)]
mod falsifier;
mod feature_remedies;
mod fmt;
mod fuzz;
mod gate_classification;
mod guidance;
mod hook_coverage;
mod hooks;
mod inconclusive;
mod jscpd;
mod line_endings;
mod markdown;
mod max_lines;
mod newtype_leaks;
mod one_bound;
mod pins;
mod refusals;
mod registry;
mod release_provenance;
mod repo;
mod rust_source;
mod serde_parse;
mod shared_client;
mod shipped;
mod skills;
mod tasks;
mod text;
mod threshold_expect;
mod unused_deps;
mod venues;
mod warm_start;
mod workflows;
mod worktree_state;

use std::process::ExitCode;

// The registry's types live in `registry`, which `max-lines` is the reason for - see that
// module's header. Re-exported here so every gate's `crate::Verdict` resolves unchanged.
use registry::{Kind, Task};
pub(crate) use registry::{Reads, Verdict};

const TASKS: &[Task] = &[
    Task {
        name: "check-boundaries",
        description: "the domain crate depends on no framework",
        kind: Kind::Hygiene(Reads::Code),
        run: boundaries::run,
    },
    Task {
        // The jscpd copy/paste gate (issue #474). `Reads::Code`, so a `docs/*.md`-only diff
        // stays excluded from the docs.yml skip. See the module header for why it FAILS CLOSED
        // when `jscpd` is absent - locally and in the nix sandbox - and for the allowlist
        // contract.
        name: "check-jscpd",
        description: "no copied block in crates/ or xtask/ without a reason in devco/dup-ignore",
        kind: Kind::Hygiene(Reads::Code),
        run: jscpd::run,
    },
    Task {
        name: "max-lines",
        description: "no file over 1000 lines (exemptions: devco/max-lines-ignore)",
        kind: Kind::Hygiene(Reads::Prose),
        run: max_lines::run,
    },
    Task {
        name: "check-pins",
        description: "no tool is pinned by both nix and pixi",
        kind: Kind::Hygiene(Reads::Code),
        run: pins::run,
    },
    Task {
        // Beside `check-pins` because it is the same shape of gate: two files, read as text
        // rather than evaluated, one value that has to be the same in both.
        name: "check-warm-start",
        description: "the warm start's directory, stamp, profile and sweep agree with what reads them",
        kind: Kind::Hygiene(Reads::Code),
        run: warm_start::run,
    },
    Task {
        name: "unused-deps",
        description: "every declared dependency is actually used",
        kind: Kind::Hygiene(Reads::Code),
        run: unused_deps::run,
    },
    Task {
        name: "check-arrow",
        description: "one Arrow major in Cargo.lock, or an explained exception",
        kind: Kind::Hygiene(Reads::Code),
        run: arrow_major::run,
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
        run: shared_client::run,
    },
    Task {
        // Beside `check-shared-client` because it is the same shape again: one declaration, read
        // as text, and every place that had to spell it a second time. Here the declaration is
        // `nix/shipped.nix`'s `binaries` list and the copies are `BINARIES` in two workflows and
        // an input default in two composite actions - which cannot be derived from it, because a
        // matrix takes literals and a job cannot evaluate a flake before installing nix.
        //
        // `Reads::Prose` because its second rule reads `docs/**` - the documented `cargo build
        // --features` a `probeFeatures` entry has to cover. Declared here rather than left at
        // `Code`, which is the failure `docs/implementation-plan-identity-and-services.md` records
        // for `check-crap`: a gate whose inputs grew into `docs/` while its classification did not.
        name: "check-shipped-binaries",
        description: "every release-path binary literal equals nix/shipped.nix, and every documented feature build is probed",
        kind: Kind::Hygiene(Reads::Prose),
        run: shipped::run,
    },
    Task {
        // Beside `check-shipped-binaries` because it reads the same declaration, and STANDALONE
        // rather than hygiene for `check-attribution-current`'s reason: it invokes cargo, so it
        // needs a resolvable registry and a target directory the nix sandbox has not got, so `just
        // gates` is its caller. The lane it covers is the one every other compiling gate is blind
        // to; CI reaches it as `nix run .#default-features` inside the one required job, and it
        // takes no argument there - the profile is derived, not passed. The module's header says why.
        name: "check-default-features",
        description: "every shipped package compiles and lints at cargo's default features",
        kind: Kind::Standalone,
        run: default_features::run,
    },
    Task {
        // The other half of the line above, and it is a separate task because the two cost
        // different amounts: that one stops at metadata, this one links and RUNS. Standalone for
        // the same reason, and it reads the same declaration. What it closes is a whole category
        // of test that was compiled by that gate and executed by no venue at all - the module's
        // header carries the count and the measurement.
        name: "check-default-feature-tests",
        description: "every shipped package runs its tests at cargo's default features",
        kind: Kind::Standalone,
        run: default_feature_tests::run,
    },
    Task {
        // Beside `check-arrow` and `check-shared-client` because it is the same shape of gate: a
        // GENERATED artefact checked against the file it is generated from, read as text so the
        // check needs no resolver. `docs/adr/0021`'s attribution amendment is the decision, and
        // `just attribution` is the fix every failure message names.
        name: "check-attribution",
        description: "ATTRIBUTION.md names every third-party crate in Cargo.lock",
        kind: Kind::Hygiene(Reads::Code),
        run: attribution::run_check,
    },
    Task {
        // Beside the boundary gate because it is the same principle in the same shape: a rule from
        // one of the four sources `AGENTS.md` adopts as policy, held by a check rather than by a
        // sentence. `check-boundaries` owns the dependency direction and the typed surface; this
        // one owns the two serde rules that were *review* in the Rust skill's own table.
        name: "check-serde-parse",
        description: "a validated newtype's serde goes through its constructor, both ways",
        kind: Kind::Hygiene(Reads::Code),
        run: serde_parse::run,
    },
    Task {
        // Beside `check-serde-parse` because it is the third rule from the same page held by the
        // same kind of check - and this one is about the ERROR principle rather than the newtype
        // one. Name coverage is narrower than proving a test actually provokes the refusal.
        name: "check-refusal-coverage",
        description: "every variant of an enrolled refusal enum is named, or separately excused with a date and reason",
        kind: Kind::Hygiene(Reads::Code),
        run: refusals::run,
    },
    Task {
        // Beside `check-refusal-coverage` because it is the other half of the same subject: that
        // one asks whether a refusal is named, this one whether the REMEDY it prints can be
        // acted on. `github.com/telekom/sutura#246` made a gate's own remedy resolve; this is the
        // same rule where the claim is about the manifest rather than about the justfile.
        name: "check-feature-remedies",
        description: "a refusal that says to rebuild names a feature the crate declares",
        kind: Kind::Hygiene(Reads::Code),
        run: feature_remedies::run,
    },
    Task {
        // The third of the newtype rules held by a check, beside `check-serde-parse`. It starts
        // GREEN - there was no first-party `Deref` and no `Borrow` in the tree when it was written
        // - so its whole job is to keep it that way, which makes it the cheapest gate here and the
        // one most likely to earn its keep years from now.
        name: "check-newtype-leaks",
        description: "no first-party Deref or Borrow - both leak a newtype's invariant",
        kind: Kind::Hygiene(Reads::Code),
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
        run: one_bound::run,
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
        run: conformance::run,
    },
    Task {
        name: "line-endings",
        description: "every text file uses LF, not CRLF",
        kind: Kind::Hygiene(Reads::Prose),
        run: line_endings::run,
    },
    Task {
        name: "text-hygiene",
        description: "conflict markers, whitespace, final newline, file size; --fix",
        kind: Kind::Hygiene(Reads::Prose),
        run: text::run,
    },
    Task {
        name: "check-skills",
        description: "the skill router and the skill tree agree",
        kind: Kind::Hygiene(Reads::Code),
        run: skills::run,
    },
    Task {
        // Beside `check-guidance` because it is the same kind of rule - a claim checked against
        // the thing it claims - and a different SCOPE: guidance reads documentation and filters
        // to `.md`, `.nix`, `.yml`, `.yaml`, `.toml` and `.sh`, so the extensionless `justfile`
        // is in neither its scan nor the citation script's `*.md` one. This gate is that file's.
        name: "check-scope",
        description: "a narrowed just recipe prints the scope it covered",
        kind: Kind::Hygiene(Reads::Code),
        run: tasks::run,
    },
    Task {
        // Beside `check-scope` because it is the same shape as `check-boot-order`: a small DECLARED
        // list of sites, refusing a site the scan finds that the list does not name. What it holds
        // is `Verdict::Inconclusive`'s own argument - the default is closed only while no venue
        // suppresses exit 3, which was a fact about the tree and held by nothing.
        name: "check-inconclusive",
        description: "every venue invoking a gate that can answer INCONCLUSIVE handles exit 3",
        kind: Kind::Hygiene(Reads::Code),
        run: inconclusive::run,
    },
    Task {
        // Beside `check-scope` for the same reason it sits beside `check-guidance`: a claim
        // checked against the thing it claims, over a file no other gate reads. `check-scope`
        // owns the `justfile`; this one owns `.pre-commit-config.yaml`, where the tiering
        // decision lives and where deleting one block silently un-tiers it.
        name: "check-hook-tiers",
        description: "the pre-push stage runs only the security checks, and compiles nothing",
        kind: Kind::Hygiene(Reads::Code),
        run: hooks::run,
    },
    Task {
        // The third of that shape, over the one remaining file: `devenv.nix` and every module its
        // `imports` reach. What it holds is that a shell body there goes through the wrapper
        // ShellCheck reads - and it replaces two forbidden LITERALS that
        // `github.com/telekom/sutura#402` walked past six ways, because a needle enumerates one
        // spelling of one attribute in one file. `Reads::Code`: a `docs/*.md` diff can change
        // nothing it reads.
        name: "check-devenv-shell",
        description: "every devenv script body goes through the wrapper ShellCheck reads",
        kind: Kind::Hygiene(Reads::Code),
        run: devenv_shell::run,
    },
    Task {
        name: "check-guidance",
        description: "docs and comments still describe this repo",
        kind: Kind::Hygiene(Reads::Prose),
        run: guidance::run,
    },
    Task {
        // The identity venue map, and the one venue whose limit is a workflow property. `Prose`,
        // because the map is a `docs/*.md` page - so a prose-only pull request DEFERS this verdict
        // to the `main` push, which the plan's second table has to say.
        name: "check-venues",
        description: "every identity claim's venue states its limit, and the acceptance job holds it",
        kind: Kind::Hygiene(Reads::Prose),
        run: venues::run,
    },
    Task {
        name: "check-workflows",
        description: "every flake output a workflow names exists",
        kind: Kind::Hygiene(Reads::Code),
        run: workflows::run,
    },
    Task {
        name: "check-docs",
        description: "the nav in mkdocs.yml and the pages under docs/ agree",
        kind: Kind::Hygiene(Reads::Prose),
        run: docs::run,
    },
    Task {
        // Beside `check-docs` because both read published pages, and a DIFFERENT concern: that one
        // asks whether a destination resolves to a page in this tree, this one whether the
        // destination is a URL at all. It is in the cheap sweep and `check-api-docs` is not,
        // because this reads the committed pages as text - no rustdoc, no nightly, no registry.
        name: "check-api-links",
        description: "no page under docs/api links to a Rust path",
        kind: Kind::Hygiene(Reads::Prose),
        run: api_links::run,
    },
    Task {
        // `Reads::Code`, and the two inputs are why: the directories under `examples/` and the
        // Rust that reaches for them. A `docs/*.md`-only diff can change neither.
        name: "check-examples",
        description: "every directory under examples/ is reached by a test",
        kind: Kind::Hygiene(Reads::Code),
        run: examples::run,
    },
    Task {
        // `Reads::Prose`, and it has to be: the page it reads is a `docs/*.md` one, so the
        // classification it holds is itself deferred by the skip it describes. That is not a
        // circularity - the `main` push runs it unconditionally, and a wrong classification
        // merged is exactly what the row for this gate in that table says is deferred.
        name: "check-gate-classification",
        description: "every hygiene gate is in exactly one of the plan's two groups",
        kind: Kind::Hygiene(Reads::Prose),
        run: gate_classification::run,
    },
    Task {
        // A threshold lint's cause is a NUMBER, which is a property of the surrounding
        // function rather than of the code the attribute sits on - so two branches can each
        // move that number correctly and only their merge is wrong. See the module doc.
        name: "check-expect-thresholds",
        description: "no #[expect] on a count-threshold lint (too_many_lines / too_many_arguments / cognitive_complexity)",
        kind: Kind::Hygiene(Reads::Code),
        run: threshold_expect::run,
    },
    Task {
        // CHEAP HALF of the CRAP gate: it reads `.cargo-crap.toml`, checks the allowlist
        // discipline and the scope, and compiles nothing. The expensive half is `crap` below, and
        // the split is the same one `check-api-docs` is kept out of hygiene for - this sweep runs
        // on every commit and inside the Nix sandbox, so nothing in it may need a coverage build.
        name: "check-crap",
        description: "the CRAP policy is a gate, its allowlist annotated, its scope real",
        kind: Kind::Hygiene(Reads::Prose),
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
        run: crap::run_delta,
    },
    Task {
        name: "commit-msg",
        description: "the commit subject is a conventional commit (the hook passes the file)",
        kind: Kind::Standalone,
        run: commit_msg::run,
    },
    Task {
        name: "classify",
        description: "what a diff requires; --since <ref>, or paths (fails open)",
        kind: Kind::Standalone,
        run: changes::run_classify,
    },
    Task {
        // Beside `classify` because it reads the same diff, and STANDALONE because it reads a
        // prek LOG - an argument, produced by a run that has already happened, which no
        // argument-free sweep can have. It exists because `just ship-check` said `green` over a
        // diff five of its ten hooks never looked at, and printed neither number.
        //
        // FAILS CLOSED where `classify` fails open, and the two directions are deliberate: that
        // gate widens what runs when it cannot read a diff; this one reports what a run covered,
        // where an unreadable diff would declare every surface untouched and every gap absent.
        name: "hook-coverage",
        description: "what a diff-scoped hook run left uninspected; --since <ref> [--log <stage>:<path>]... [--ran <task>]... [--surface-tasks]",
        kind: Kind::Standalone,
        run: hook_coverage::run,
    },
    Task {
        name: "changed-packages",
        description: "the cargo packages owning the given .rs paths",
        kind: Kind::Standalone,
        run: changes::run_changed_packages,
    },
    Task {
        name: "check-changed",
        description: "cargo check, narrowed to the packages that changed",
        kind: Kind::Standalone,
        run: changes::run_check_changed,
    },
    Task {
        // NOT `Kind::Hygiene`, and not by oversight. The hygiene sweep is cheap,
        // argument-free and runs everywhere a developer commits - including hosts and
        // sandboxes with no Rust nightly at all. This one COMPILES the library crates and
        // needs the nightly toolchain, because `--output-format json` is an unstable rustdoc
        // option. Collecting it would make the cheap sweep expensive and, worse, unrunnable
        // in the places it currently runs.
        name: "check-api-docs",
        description: "docs/api/*.md is what the generator produces (NIGHTLY; compiles)",
        kind: Kind::Standalone,
        run: api_docs::run,
    },
    Task {
        // The BYTE-COMPARE half, and `check-api-docs` is the shape it copies including why it is
        // not in the hygiene sweep: it needs an input the nix sandbox has not got - a compiler
        // there, a resolvable registry here. It exists because a review found that the offline gate
        // could only see that a licence cell was non-empty, so the main content of a generated
        // artefact was trusted rather than compared.
        name: "check-attribution-current",
        description: "ATTRIBUTION.md is what the generator produces (needs a resolvable registry)",
        kind: Kind::Standalone,
        run: attribution::run_check_current,
    },
    Task {
        // NOT `Kind::Hygiene`, and for `check-api-docs`' reason rather than its own: it invokes
        // `cargo metadata`, which needs a resolvable registry, and the hygiene sweep runs inside a
        // nix sandbox with no network. `check-attribution` above is the half that runs everywhere,
        // and it reads `Cargo.lock` precisely so it can.
        name: "attribution",
        description: "regenerate ATTRIBUTION.md from cargo metadata (needs a resolvable registry)",
        kind: Kind::Standalone,
        run: attribution::run_generate,
    },
    Task {
        // NOT `Kind::Hygiene`, and for a different reason from `check-api-docs`: it compiles
        // nothing and needs no network, but it is only half a gate. It EXTRACTS; the verdict comes
        // from `shellcheck`, which is a nix-pinned tool the cheap sweep must not require. So the
        // pairing lives in `just lint-actions` and in `ci.yml`'s workflow-analysis step, beside
        // the `actionlint` call that cannot read these files at all.
        name: "action-shell",
        description: "extract every composite action's shell into a directory, for shellcheck",
        kind: Kind::Standalone,
        run: action_shell::run,
    },
    Task {
        name: "collect-provenance",
        description: "export five release attestation bundles after exact subject-set checks",
        kind: Kind::Standalone,
        run: release_provenance::run,
    },
    Task {
        // The other half of `check-devenv-shell`, and standalone for `action-shell`'s reason plus
        // one of its own: it takes an ARGUMENT - the store path of a body the wrapper produced,
        // interpolated by nix at the call site - and it reads a derivation, which needs a store
        // the cheap sweep has no nix to query. That gate holds the STRUCTURE; this one reads what
        // the wrapper actually emitted, which is the half `github.com/telekom/sutura#402`'s
        // seventh escape defeated with one line and every textual gate green.
        name: "check-devenv-linter",
        description: "the devenv wrapper's checkPhase still runs bash -n and a store shellcheck; <store-path>",
        kind: Kind::Standalone,
        run: devenv_linter::run,
    },
    Task {
        name: "fmt",
        description: "cargo fmt, scoped to our packages (--check to verify)",
        kind: Kind::Standalone,
        run: fmt::run,
    },
    Task {
        name: "check-fuzz",
        description: "every fuzz target is declared, seeded, and run by the workflow",
        kind: Kind::Hygiene(Reads::Code),
        run: fuzz::run,
    },
    Task {
        name: "hygiene",
        description: "every cheap structural gate, in order (the one list)",
        kind: Kind::Standalone,
        run: run_hygiene,
    },
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
        run: compose::run_up,
    },
    Task {
        name: "dev-down",
        description: "remove this worktree's services and volumes; --dry-run says what it would take",
        kind: Kind::Standalone,
        run: compose::run_down,
    },
    Task {
        name: "dev-endpoints",
        description: "where this worktree's services are listening, from the discovery file",
        kind: Kind::Standalone,
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
        run: compose::run_endpoint,
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
        run: branches::run,
    },
    Task {
        name: "test-causality",
        description: "a changed test is red on base, green on head; --since <ref>",
        kind: Kind::Standalone,
        run: causality::run,
    },
];

/// Every task name. `check-guidance` reads this to reject a doc citing a task that is gone.
pub(crate) fn task_names() -> impl Iterator<Item = &'static str> {
    TASKS.iter().map(|t| t.name)
}

/// Every gate the `hygiene` sweep collects, with what it reads.
///
/// Derived from the same table and the same predicate `run_hygiene` filters on, because a second
/// list of the sweep's members is the drift this table was built to end.
pub(crate) fn hygiene_gates() -> impl Iterator<Item = (&'static str, Reads)> {
    TASKS.iter().filter_map(|t| match t.kind {
        Kind::Hygiene(reads) => Some((t.name, reads)),
        Kind::Standalone => None,
    })
}

/// Run every hygiene gate, in declaration order, stopping at the first failure.
///
/// Stopping rather than collecting: these are ordered cheapest-first, so the first failure is
/// usually the cheapest to read, and a wall of output from eight gates is worse than one.
fn run_hygiene(_args: &[String]) -> Verdict {
    let gates: Vec<&Task> = TASKS.iter().filter(|t| matches!(t.kind, Kind::Hygiene(_))).collect();
    for task in &gates {
        // No arguments: a hygiene gate takes none, which is what `Kind::Hygiene` asserts.
        match (task.run)(&[]) {
            Verdict::Pass => {}
            other => {
                eprintln!();
                eprintln!("xtask hygiene: stopped at `{}`", task.name);
                return other;
            }
        }
    }
    println!("xtask hygiene: ok - {} gate(s)", gates.len());
    Verdict::Pass
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rest = args.split_first().map(|(_, rest)| rest).unwrap_or_default();

    match args.first().map(String::as_str) {
        Some("--help" | "-h" | "help") => {
            usage();
            ExitCode::SUCCESS
        }
        Some(requested) => TASKS.iter().find(|t| t.name == requested).map_or_else(
            || {
                eprintln!("xtask: unknown task `{requested}`");
                usage();
                ExitCode::from(2)
            },
            |task| (task.run)(rest).exit_code(),
        ),
        None => {
            usage();
            ExitCode::from(2)
        }
    }
}

fn usage() {
    eprintln!("usage: cargo xtask <task>");
    for task in TASKS {
        // A leading dot marks a member of the `hygiene` sweep, so the set is readable here
        // rather than only in the source.
        let mark = if matches!(task.kind, Kind::Hygiene(_)) { "." } else { " " };
        eprintln!("{mark} {:<18} {}", task.name, task.description);
    }
    eprintln!();
    eprintln!(". = run together by `cargo xtask hygiene`");
}

/// `cargo metadata` as JSON, with `extra` appended (for example `--no-deps`).
///
/// One place, because three gates read the workspace graph and each wants the same `--locked`
/// guarantee: a gate must not be the thing that rewrites `Cargo.lock`.
fn cargo_metadata(extra: &[&str]) -> Result<serde_json::Value, String> {
    // `env!("CARGO")` looks equivalent and is not: it bakes cargo's absolute store path into
    // the binary at compile time, which put the whole cargo closure into the shipped package's
    // runtime closure and made the build non-reproducible. Cargo sets this variable whenever it
    // invokes us, so the runtime lookup is the same value with none of that.
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| String::from("cargo"));
    let output = std::process::Command::new(cargo)
        .args(["metadata", "--format-version", "1", "--locked"])
        .args(extra)
        .output()
        .map_err(|e| format!("could not run cargo metadata: {e}"))?;
    if !output.status.success() {
        return Err(format!("cargo metadata failed: {}", String::from_utf8_lossy(&output.stderr)));
    }
    serde_json::from_slice(&output.stdout).map_err(|e| format!("cargo metadata was not valid JSON: {e}"))
}

#[cfg(test)]
mod tests {
    use super::{TASKS, task_names};

    #[test]
    fn task_names_are_unique() {
        let mut names: Vec<&str> = task_names().collect();
        let count = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), count, "duplicate task name in TASKS");
    }

    #[test]
    fn every_task_has_a_help_line() {
        for task in TASKS {
            assert!(!task.description.is_empty(), "{} has no --help line", task.name);
        }
    }

    #[test]
    fn the_hygiene_set_is_not_empty() {
        // An empty set would make `hygiene` a green no-op - the failure this whole change
        // exists to prevent, arrived at from the other direction.
        assert!(TASKS.iter().any(|t| matches!(t.kind, super::Kind::Hygiene(_))));
    }

    #[test]
    fn the_sweep_has_a_gate_on_each_side_of_the_prose_line() {
        // `Kind::Hygiene(Reads)` makes the classification total - a gate cannot join the sweep
        // without declaring a side - so what is left to check is that neither side is EMPTY.
        // An empty side is how the plan's argument passes vacuously: with nothing classified as
        // reading prose there is no table of deferred verdicts to be wrong, and with nothing
        // classified as reading code the skip it justifies covers nothing.
        let mut code = 0_usize;
        let mut prose = 0_usize;
        for (_, reads) in super::hygiene_gates() {
            match reads {
                super::Reads::Code => code += 1,
                super::Reads::Prose => prose += 1,
            }
        }
        assert!(
            code > 0,
            "no hygiene gate reads code - the skip in docs.yml justifies nothing"
        );
        assert!(
            prose > 0,
            "no hygiene gate reads prose - the deferred-verdict table is then empty"
        );
    }

    #[test]
    fn hygiene_does_not_contain_itself() {
        // Marked `Standalone`, so `run_hygiene` cannot collect and re-enter itself.
        let me = TASKS.iter().find(|t| t.name == "hygiene").expect("hygiene is registered");
        assert_eq!(me.kind, super::Kind::Standalone);
    }

    #[test]
    fn no_hygiene_gate_needs_arguments() {
        // `run_hygiene` calls each with an empty slice. A gate that needs `--since` or a path
        // would silently do the wrong thing, so those stay `Standalone`.
        let needs_args = [
            "classify",
            "check-changed",
            "test-causality",
            "commit-msg",
            "changed-packages",
            // Its argument is a store path nix interpolates at the call site, so an
            // argument-free invocation has nothing to read - and the sweep would call it that way.
            "check-devenv-linter",
        ];
        for name in needs_args {
            let task = TASKS.iter().find(|t| t.name == name).expect("task is registered");
            assert_eq!(task.kind, super::Kind::Standalone, "{name} must not be in the hygiene set");
        }
    }

    #[test]
    fn the_api_docs_gate_is_not_collected_into_hygiene() {
        // It takes no arguments, so the test above would not catch this one. The reason is
        // COST and REACH: it compiles the library crates and needs the nightly toolchain for
        // rustdoc's unstable JSON output, while the hygiene sweep runs on every commit and
        // inside the Nix sandbox, neither of which has a nightly.
        let task = TASKS.iter().find(|t| t.name == "check-api-docs").expect("task is registered");
        assert_eq!(task.kind, super::Kind::Standalone);
    }

    #[test]
    fn the_branch_cleanup_is_not_collected_into_hygiene() {
        // It takes no REQUIRED arguments, so the argument test above would not catch this one, and
        // it is the entry in this table that can DELETE things. A hygiene sweep runs on every
        // commit; a task that removes a branch may not be in it whatever its default mode is.
        let task = TASKS.iter().find(|t| t.name == "clean-branches").expect("task is registered");
        assert_eq!(task.kind, super::Kind::Standalone);
    }

    #[test]
    fn the_crap_gate_is_not_collected_into_hygiene() {
        // Same reasoning as the api-docs gate above, and the argument test would not catch this
        // one either: `crap` takes no arguments. It compiles the scoped crates under
        // `-C instrument-coverage` and shells out to two tools the cheap sweep must not require -
        // and the Nix sandbox and the commit hook both run that sweep.
        let task = TASKS.iter().find(|t| t.name == "crap").expect("task is registered");
        assert_eq!(task.kind, super::Kind::Standalone);
        // Its configuration half IS cheap and must stay in the sweep: that is what stops the
        // policy file from rotting on a tree nobody has run the expensive half against.
        let cheap = TASKS.iter().find(|t| t.name == "check-crap").expect("task is registered");
        assert!(matches!(cheap.kind, super::Kind::Hygiene(_)));
        // And the DELTA half is standalone for a third reason: it needs a baseline that arrives
        // over the network in CI, and the hygiene sweep runs in a sandbox with no network.
        let delta = TASKS.iter().find(|t| t.name == "crap-delta").expect("task is registered");
        assert_eq!(delta.kind, super::Kind::Standalone);
    }

    #[test]
    fn a_verdict_maps_to_the_expected_exit_code() {
        use std::process::ExitCode;

        use super::Verdict;
        // `ExitCode` cannot be compared, so assert via the Debug form - the one thing it does
        // expose. Pass must be 0 or a failing gate would not fail the build.
        assert_eq!(format!("{:?}", Verdict::Pass.exit_code()), format!("{:?}", ExitCode::SUCCESS));
        assert_eq!(format!("{:?}", Verdict::Fail.exit_code()), format!("{:?}", ExitCode::FAILURE));
        assert_eq!(
            format!("{:?}", Verdict::Usage.exit_code()),
            format!("{:?}", ExitCode::from(2))
        );
        // AND IT IS NOT 0, which is the whole of #307: a gate that measured nothing may not hand
        // a required step the same code a proof does. Asserted against `SUCCESS` as well as
        // against 3, because the failure mode being closed here is someone mapping it back.
        assert_eq!(
            format!("{:?}", Verdict::Inconclusive.exit_code()),
            format!("{:?}", ExitCode::from(3))
        );
        assert_ne!(
            format!("{:?}", Verdict::Inconclusive.exit_code()),
            format!("{:?}", ExitCode::SUCCESS)
        );
    }

    #[test]
    fn cargo_metadata_reads_this_workspace() {
        let meta = super::cargo_metadata(&["--no-deps"]).expect("cargo metadata should succeed in-tree");
        let packages = meta.get("packages").and_then(|p| p.as_array()).expect("packages array");
        assert!(
            packages
                .iter()
                .any(|p| p.get("name").and_then(|n| n.as_str()) == Some("xtask"))
        );
    }

    #[test]
    fn every_registered_hygiene_gate_refuses_a_tree_it_cannot_attest() {
        use std::process::ExitCode;

        // EVERY GATE IN THE TABLE, EXECUTED AGAINST A TREE IT MUST REFUSE -
        // `github.com/telekom/sutura#371`. `crate::falsifier` carries the tree and the argument
        // for its shape; what this adds is that the gates are reached through the FN POINTER out
        // of `TASKS` and judged by the EXIT CODE. Membership is the table, so nothing opts out.
        //
        // HELD, NOT WISHED - the first version got that wrong in the change whose subject it is.
        // `set_current_dir` is process-global: measured, `cargo test -p xtask --bin xtask` went
        // from `917 passed` to `907 passed; 11 failed`, the eleven every real-tree anchor
        // resolving through `repo::root`, whose walk reads the current directory first.
        // `AGENTS.md` bans a bare `cargo clippy` and `cargo nextest`, NOT `cargo test`, so
        // "correct under nextest" was a sentence. nextest gives each test its own process and sets
        // `NEXTEST`; refusing without it fails before the directory moves.
        assert!(
            std::env::var_os("NEXTEST").is_some(),
            "this test moves the process's current directory, so it must have the process to \
             itself: run it under `just test`, which is cargo-nextest and one process per test. \
             Under `cargo test`'s threads it breaks every sibling resolving a path through \
             `repo::root` - 11 of them, measured."
        );

        let tree = crate::falsifier::falsifier_tree();
        let original = std::env::current_dir().expect("a current directory");
        std::env::set_current_dir(&tree).expect("point the process at the falsifier tree");

        let mut executed: Vec<&str> = Vec::new();
        let mut attested: Vec<&str> = Vec::new();
        for task in TASKS {
            if !matches!(task.kind, super::Kind::Hygiene(_)) {
                continue;
            }
            let verdict = (task.run)(&[]);
            executed.push(task.name);
            // `Fail`'s code, not merely "not SUCCESS": `Usage` is 2 and `Inconclusive` is 3, and
            // the second exists here precisely because *could not measure* is not a clean bill.
            // All 31 answer `Fail` today, so the stricter form is live rather than aspirational.
            if format!("{:?}", verdict.exit_code()) != format!("{:?}", ExitCode::FAILURE) {
                attested.push(task.name);
            }
        }

        // Restored before any assertion, so a failure cannot leave a wrong directory behind.
        std::env::set_current_dir(&original).expect("restore the current directory");
        drop(std::fs::remove_dir_all(&tree));

        // THE FLOOR IS A SET OF NAMES AND ITS OTHER SIDE IS `hygiene_gates`. Two counts off two
        // spellings of one expression are two enforcers of one key: measured, `.take(18)` on BOTH
        // left the previous version green with 13 gates unexecuted. `hygiene_gates` is the
        // registry's other reader - `check-gate-classification` reconciles it against the
        // implementation plan's two tables, both directions - so narrowing it to hide a narrowed
        // loop reddens that gate instead. The hand-written anchor list this replaces was #371's
        // own defect 8: red when a name joins the list, green when one is left out of it.
        let registered: Vec<&str> = super::hygiene_gates().map(|(name, _)| name).collect();
        assert!(
            !registered.is_empty(),
            "the sweep registers no gate - this test judged nothing"
        );
        assert_eq!(
            executed, registered,
            "the gates this test executed are not the gates the sweep registers - one it skipped \
             is one it says nothing about"
        );

        assert_eq!(
            attested,
            Vec::<&str>::new(),
            "{} of {} gate(s) did not FAIL over a tree that is not this repository. A gate that \
             cannot be made to fail is a gate whose green says nothing - `github.com/telekom/\
             sutura#371`. WHICH REMEDY IS RIGHT DEPENDS ON THE GATE'S SUBJECT. If that subject is \
             every text file in the tree, absence is a legitimate pass and nothing is wrong with \
             the gate: seed a violation into `falsifier_tree`, whose doc carries the argument. \
             Otherwise the gate needs a floor over what it actually read, or a refusal on the input \
             whose absence makes its other rules vacuous.",
            attested.len(),
            registered.len()
        );
    }
}
