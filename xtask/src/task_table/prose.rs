//! Tasks that judge the PUBLISHED and GUIDANCE surface: the docs nav, the generated API pages
//! and their links, the skill tree, guidance claims, the examples, and how gates classify.
//!
//! Why it is its own area: a `docs/*.md`-only diff skips the `hygiene` build, and the argument
//! for that skip is a classification of what each gate READS. These are the rows that make it
//! true, so `Reads::Prose` and this file should be read together.

use crate::registry::{Falsifier, Kind, Reads, Task};
use crate::{api_docs, api_links, docs, examples, gate_classification, guidance, inconclusive, skills, tasks};

pub(crate) const TASKS: &[Task] = &[
    Task {
        name: "check-skills",
        description: "the skill router and the skill tree agree",
        kind: Kind::Hygiene(Reads::Code),
        falsifier: Falsifier::declared_in_programme(),
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
        falsifier: Falsifier::declared_in_programme(),
        run: tasks::run,
    },
    Task {
        name: "check-guidance",
        description: "docs and comments still describe this repo",
        kind: Kind::Hygiene(Reads::Prose),
        falsifier: Falsifier::declared_in_programme(),
        run: guidance::run,
    },
    Task {
        name: "check-docs",
        description: "the nav in mkdocs.yml and the pages under docs/ agree",
        kind: Kind::Hygiene(Reads::Prose),
        falsifier: Falsifier::declared_in_programme(),
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
        falsifier: Falsifier::declared_in_programme(),
        run: api_links::run,
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
        falsifier: Falsifier::declared_in_programme(),
        run: api_docs::run,
    },
    Task {
        // `Reads::Code`, and the two inputs are why: the directories under `examples/` and the
        // Rust that reaches for them. A `docs/*.md`-only diff can change neither.
        name: "check-examples",
        description: "every directory under examples/ is reached by a test",
        kind: Kind::Hygiene(Reads::Code),
        falsifier: Falsifier::declared_in_programme(),
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
        falsifier: Falsifier::declared_in_programme(),
        run: gate_classification::run,
    },
    Task {
        // Beside `check-scope` because it is the same shape as `check-boot-order`: a small DECLARED
        // list of sites, refusing a site the scan finds that the list does not name. What it holds
        // is `Verdict::Inconclusive`'s own argument - the default is closed only while no venue
        // suppresses exit 3, which was a fact about the tree and held by nothing.
        name: "check-inconclusive",
        description: "every venue invoking a gate that can answer INCONCLUSIVE handles exit 3",
        kind: Kind::Hygiene(Reads::Code),
        falsifier: Falsifier::declared_in_programme(),
        run: inconclusive::run,
    },
];
