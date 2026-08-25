---
name: ms-rust
description: Microsoft's Pragmatic Rust Guidelines, split by topic. Load after the repo's own `rust` skill; sutura's invariants win where they conflict.
---

<!-- The Pragmatic Rust Guidelines are copyright (c) Microsoft Corporation, MIT licensed. -->

# Microsoft Pragmatic Rust Guidelines

Guidelines content sha256 `c9f1ac1c` (see `all.txt.sha256`).

General Rust discipline, as 13 topic files. **Load only the ones the task touches** - that is
the point of the split; preloading all of them costs ~3,500 lines for no benefit.

## Precedence

`AGENTS.md` first, then `engineering/rust/SKILL.md` (this repo's lint table, panic ban and
gates), then this. Where they conflict on something material - an API shape, a safety
decision, a validation rule - **say so rather than silently picking**. Two cases already
known to differ:

| Upstream says | Here |
| --- | --- |
| Applications may use `anyhow` (`M-APP-ERROR`) | The tool surface returns `ToolOutcome::Refusal`, a typed result. A refusal is not an error, so an opaque error type would erase the distinction the governance boundary depends on |
| Applications target the highest viable `target-cpu` (`M-TARGET-CPU`) | Release artifacts are published and must run on older CPUs. `target-cpu=native` is deliberately absent - see `release-performance.yml` |

## Which file

| Task | File |
| --- | --- |
| Any non-trivial Rust edit - start here | `09_universal_guidelines.md` |
| Writing code an agent will read or extend | `01_ai_guidelines.md` |
| Binaries, CLI surface, app-level error handling | `02_application_guidelines.md` |
| Panics, soundness, `unsafe`, invariants | `03_correctness_guidelines.md` |
| Public docs, module docs, doc structure | `04_documentation.md` |
| FFI, `-sys` crates, DLL boundaries | `05_ffi_guidelines.md` |
| Writing or reviewing a macro | `06_macros_guidelines.md` |
| Allocation, async yield points, profiling | `07_performance_guidelines.md` |
| Workspace layout, crate boundaries, lints | `08_project_guidelines.md` |
| Designing a reusable crate | `10_libraries_building_guidelines.md` |
| Public API surface, `Send`/`Sync`, escape hatches | `11_libraries_interoperability_guidelines.md` |
| Avoiding statics, making I/O mockable | `12_libraries_resilience_guidelines.md` |
| Error types, naming, API ergonomics | `13_libraries_ux_guidelines.md` |

Rules are identified as `M-SOMETHING`; cite that id when you apply or decline one, so a
reviewer can find the rule rather than infer it.

Particularly load-bearing here, and already enforced by the gates rather than by memory:
`M-TAUTOLOGICAL-TESTS` (a test asserting ground truth proves nothing - see the causality
gate), `M-SINGLE-ITEM-PATH` (`unreachable_pub` is denied), `M-PANIC-IS-STOP` and
`M-UNSAFE` (`unwrap_used`/`expect_used`/`panic` denied, `unsafe_code` forbidden).

## Refreshing

```bash
pixi run --frozen ms-rust-refresh
```

Regenerates from upstream and updates `all.txt.sha256`. It is a no-op when the content has
not changed, so the sha is the answer to "is this current". Behind a proxy, fetch
`https://microsoft.github.io/rust-guidelines/agents/all.txt` however your network allows and
pass `--from-file <path>`.

Upstream reorganises: the section list is read from the source rather than hardcoded, so a
refresh that adds or renames a section produces different files. Update the table above when
that happens - `cargo xtask check-skills` will not catch it, because it checks the router and
the tree, not this table.
