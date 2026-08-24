---
name: rust
description: Rust in this workspace - the lint table, the panic ban, feature gating, and what the gates reject before review does.
---

# Rust here

This workspace is stricter than default Rust. Most surprises come from four choices.

## 1. The whole `restriction` category is on

`clippy.toml` and the workspace lint table enable `restriction` at `warn`, and CI runs
`-D warnings`. Consequences that bite in normal code:

| You wrote | Use instead |
| --- | --- |
| `x.to_string()` | `String::from(x)` - `str_to_string` is denied |
| `format!` into an existing `String` | `push_str` / `write!` - `format_push_string` |
| `let _ = f()` on a `#[must_use]` | handle it, or `drop(f())` if truly discardable |
| `use` after a statement | move it to the top of the module |
| `#[allow(..)]` | `#[expect(.., reason = "..")]` - `allow_attributes` fails a bare allow |
| `match x { Some(v) => v, None => .. }` | `if let` / `?` - clippy will name the lint |

Overrides live in the lint table, each with its reason. Disagree with a specific line there;
do not add a blanket allow.

## 2. No panic path from input

`unwrap_used`, `expect_used`, `panic` and `indexing_slicing` are **denied** for library code
and exempt in tests (`allow-*-in-tests` in `clippy.toml`). Shipped profiles use
`panic = "abort"`, so a panic is a process death, not an exception.

Slices are walked with `split_first` rather than indexed. `unsafe_code` is `forbid` - not
`deny` - so a crate cannot re-allow it locally.

## 3. Features are default-off, so `--all-features` is mandatory

Adapters are feature-gated and off by default. A bare `cargo clippy --workspace` inspects
almost nothing and still reports success. Every gate passes `--all-features`; so should you.

The exception is the fast inner loop, deliberately narrow:

```bash
cargo check -p sutura-domain --no-default-features   # must stay sub-second
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo nextest run --workspace --all-features
```

`sutura-domain` must acquire **no** framework dependency - no tokio, axum, rmcp, datafusion,
arrow. `cargo xtask check-boundaries` enforces it.

## 4. The gates reject before review does

Run `gates` before you claim done. Individually:

| Gate | Rejects |
| --- | --- |
| `cargo xtask max-lines` | any file over 1000 lines; exemptions only in `.max-lines-ignore`, and never under `crates/` or `xtask/` |
| `cargo xtask unused-deps` | a declared dependency nothing references, and a `[workspace.dependencies]` entry nobody inherits |
| `cargo xtask line-endings` | CRLF. `fmt` fixes it |
| `cargo xtask text-hygiene` | conflict markers, trailing whitespace, missing final newline, files over 512 kB |
| `cargo xtask check-boundaries` | a framework dependency reaching the domain crate |
| `cargo xtask commit-msg` | a subject that is not a conventional commit, over 72 chars |

## Conventions

- Rust 2024. One version for the workspace; crates inherit with `version.workspace = true`.
- The compiler pin is `rust-toolchain.toml` and nowhere else - rustup and Nix both read it.
- Ports get **fakes**, not mocked HTTP. A test asserting on source text proves nothing.
- Adding a dependency: `unused-deps` requires it to be referenced, and `cargo-deny` checks
  its licence and advisories. Both run in the gates.

## Before claiming completion

Paste the command and its output. A new or changed test must also satisfy the causality
requirement in `AGENTS.md`: red against base behaviour, green on your change.
