# Changelog

Generated from conventional commit subjects by git-cliff. Do not edit by hand: the next
release overwrites it. Fix a wrong entry by fixing the commit message convention, not the file.

## 0.1.0 - 2026-08-25

### Build

- Bump sha2 from 0.10.9 to 0.11.0

### CI

- Remove every third-party action, add the weekly security audit
- Classify changes, inline the remote hooks, --all-features everywhere
- Drop devenv from CI, add the opt-in performance release

### Documentation

- The brand slots are filled, not empty
- **skills**: St sync does not restack, st refresh does
- Trim getting-started and enterprise-mirrors

### Features

- Musl artifacts with mimalloc, plus a contributing guide
- Mkdocs-material docs, FHS dev container, one pin per tool
- Typed newtypes, structured errors, and an honest hexagon
- Secrets gate, dev CLI, justfile, changelog automation
- **gates**: Check-guidance, skills lock, and one dispatch table
- **skills**: Import 20 library-tier skills, enforce provenance
- **agents**: Cross-agent skill wiring, ponytail, PR and issue templates
- Stacked-branches skill, repo-local stax config, generated-file exemption
- **skills**: Add ms-rust, regenerated from current upstream
- **agents**: Skills tree, causality gate, and the checks that keep them honest
- **gates**: Max-lines 1000, unused-deps, and a stricter lint table
- M0 walking skeleton — toolchain, gates, and a binary that runs
- Initial

### Fixes

- **xtask**: A check that could not run must not report a finding
- **xtask**: A pointer file is not a newline violation
- **dev**: Clear build-users-group so the container can build derivations
- **ci**: Catch a missing flake output and a vendored leak locally
- **dev**: Pass a token through only when it exists, and keep nix on PATH
- Unbreak CI on main, and make the hooks work outside the dev shell
- **dev**: Doctor catches uninstalled hooks - they were never installed
- **ci**: A moved file no longer skips its own area
- Ship one binary, one hygiene list, CRLF for every text file
- Nextest everywhere, doctests in CI, and three things I got wrong
- **ci**: Compare against the whole branch, and stop passing on a base that never built
- **deny**: Allow only the licences in use, and fail if that stops being true
- **security**: Act on the review - publish paths, supply chain, betterleaks
- **gates**: Skip crane's .cargo-home, drop four lint allows, ASCII output
- **ci**: Make the Nix path actually work, and stop rebuilding Rust four times

### Review

- 1.98, rustfmt 130, drop the hook, cross-built releases, cranelift dev-only

