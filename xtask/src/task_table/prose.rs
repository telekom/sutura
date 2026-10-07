//! Tasks that judge the PUBLISHED and GUIDANCE surface: the docs nav, the generated API pages
//! and their links, the skill tree, guidance claims, the examples, and how gates classify.
//!
//! Why it is its own area: a `docs/*.md`-only diff skips the `hygiene` build, and the argument
//! for that skip is a classification of what each gate READS. These are the rows that make it
//! true, so `Reads::Prose` and this file should be read together.

use crate::registry::{Edit, Falsifier, Kind, Paired, Reads, Task};
use crate::{api_docs, api_links, docs, examples, gate_classification, guidance, inconclusive, skills, tasks};

/// `stale_phrases`' own rule: a `FORBIDDEN` needle in README.md, outside a code span so no
/// citation rule reads it.
const CHECK_GUIDANCE_PAIRED: Paired = Paired {
    inputs: &["."],
    violation: &[Edit {
        path: "README.md",
        find: "data runtime. It is built",
        replace: "data runtime. Never run cargo fmt --all here. It is built",
    }],
};

pub(crate) const TASKS: &[Task] = &[
    Task {
        name: "check-skills",
        description: "the skill router and the skill tree agree",
        kind: Kind::Hygiene(Reads::Code),
        falsifier: Falsifier {
            // The router parses and one skill resolves; its second route names no skill.
            seeds: &[
                (
                    ".agents/skills/skill-router.json",
                    "{\"groups\":{\"sample\":{\"skills\":{\"kept\":{},\"missing\":{}}}}}\n",
                ),
                (
                    ".agents/skills/sample/kept/SKILL.md",
                    "---\nname: kept\ndescription: A routed skill.\n---\n\n# Kept\n",
                ),
            ],
            in_scope: Some(".agents/skills/skill-router.json"),
            paired: None,
        },
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
        falsifier: Falsifier {
            // The narrow recipe points to a wider check, but omits its own package from output.
            seeds: &[
                (
                    "justfile",
                    concat!(
                        "narrow:\n    @echo 'See `just ",
                        "wide`'\n    cargo nextest run -p xtask\nwide:\n    cargo nextest run --workspace\nparity:\n    cargo nextest run -E 'test(foo)'\n",
                    ),
                ),
                (
                    "flake.nix",
                    "apps.parity = {\n  program = ''\n    cargo nextest run -E 'test(foo)'\n  '';\n};\n",
                ),
            ],
            in_scope: Some("justfile"),
            paired: None,
        },
        run: tasks::run,
    },
    Task {
        name: "check-guidance",
        description: "docs and comments still describe this repo",
        kind: Kind::Hygiene(Reads::Prose),
        falsifier: Falsifier::paired(&CHECK_GUIDANCE_PAIRED),
        run: guidance::run,
    },
    Task {
        name: "check-docs",
        description: "the nav in mkdocs.yml and the pages under docs/ agree, and site links name a version",
        kind: Kind::Hygiene(Reads::Prose),
        falsifier: Falsifier {
            // The site has a nav, a reachable link, and its required font setting. One page is
            // still outside the nav and the exclusion set.
            seeds: &[
                (
                    "mkdocs.yml",
                    "nav:\n  - Home: index.md\ntheme:\n  name: material\n  font: false\n",
                ),
                ("docs/index.md", "[Home](index.md)\n"),
                ("docs/orphan.md", "A page with no nav entry.\n"),
            ],
            in_scope: Some("docs/orphan.md"),
            paired: None,
        },
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
        falsifier: Falsifier {
            // The generated-page and scheme-agreement floors hold; the page publishes a dead
            // Rust-path destination that the link scanner must name.
            seeds: &[
                ("docs/.tools/rustdoc_to_markdown.py", "REAL_SCHEMES = (\"http\", \"https\")\n"),
                (
                    "docs/api/generated.md",
                    "<!-- GENERATED FILE - do not edit. -->\n[bad](crate::path::Item)\n",
                ),
            ],
            in_scope: Some("docs/api/generated.md"),
            paired: None,
        },
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
        falsifier: Falsifier {
            // One workspace test reaches a published example, satisfying the corpus and test
            // floors. The second published directory has no reachable test.
            seeds: &[
                ("Cargo.toml", "[workspace]\nmembers = [\"crates/sutura-catalog-datahub\"]\n"),
                ("xtask/src/main.rs", "fn main() {}\n"),
                (
                    "crates/sutura-catalog-datahub/tests/multi_player.rs",
                    "#[test]\nfn reads_example() { let _ = \"../../examples/reached/data.txt\"; }\n",
                ),
                ("examples/reached/data.txt", "fixture\n"),
                ("examples/orphan/README.md", "No test reaches this example.\n"),
            ],
            in_scope: Some("examples/orphan/README.md"),
            paired: None,
        },
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
        falsifier: Falsifier {
            // Both tables parse and each contains a real gate. The other registered gates are
            // unclassified, so the disagreement walk, not a missing-page floor, must refuse.
            seeds: &[(
                "docs/implementation-plan-identity-and-services.md",
                "| Gate | What it reads |\n| --- | --- |\n| `check-pins` | code |\n\n| Gate | On a prose-only pull request |\n| --- | --- |\n| `text-hygiene` | deferred |\n",
            )],
            in_scope: Some("docs/implementation-plan-identity-and-services.md"),
            paired: None,
        },
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
        falsifier: Falsifier {
            // All four declared sites are present and handle exit 3. The extra invocation in
            // nix/rogue.sh has no declared handling and must be named as a new site.
            seeds: &[
                ("justfile", "causality:\n    cargo run -p xtask -- test-causality\n"),
                ("flake.nix", "{ } # root marker\napp = nix run .#causality\n"),
                (
                    "devenv.nix",
                    "status=0\ncargo run -p xtask -- test-causality || status=$?\nif [ \"$status\" -eq 3 ]; then exit 3; fi\nexit \"$status\"\n",
                ),
                (
                    ".github/workflows/ci.yml",
                    "run: |\n  status=0\n  cargo run -p xtask -- test-causality || status=$?\n  if [ \"$status\" -eq 3 ]; then exit 3; fi\n  exit \"$status\"\n",
                ),
                ("nix/rogue.sh", "#!/usr/bin/env bash\ncargo run -p xtask -- test-causality\n"),
            ],
            in_scope: Some("nix/rogue.sh"),
            paired: None,
        },
        run: inconclusive::run,
    },
];
