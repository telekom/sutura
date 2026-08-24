# Changelog

Generated from conventional commit subjects by git-cliff. Do not edit by hand: the next
release overwrites it. Fix a wrong entry by fixing the commit message convention, not the file.

## Unreleased

### CI

- Remove every third-party action, add the weekly security audit
- Classify changes, inline the remote hooks, --all-features everywhere
- Drop devenv from CI, add the opt-in performance release

### Documentation

- Trim getting-started and enterprise-mirrors

### Features

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

- **gates**: Skip crane's .cargo-home, drop four lint allows, ASCII output
- **ci**: Make the Nix path actually work, and stop rebuilding Rust four times

### Review

- 1.98, rustfmt 130, drop the hook, cross-built releases, cranelift dev-only

