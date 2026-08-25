# Vendored and derived material

Third-party material adapted into this repo. "Inspired by" is not a licence position, so each
entry names the upstream, the commit, the date and what changed locally.

The `cargo-deny` licence gate covers dependencies. This file covers everything else -
documentation, guidance, fixtures - which no dependency check can see.

| Local path | Upstream | Licence | Commit | Date | Local changes |
| --- | --- | --- | --- | --- | --- |
| `.agents/skills/engineering/oauth/SKILL.md` | `github.com/curityio/oauth-developer-skills` | Apache-2.0 | `d411f195ab0d03bc68de6b16504036b6f7533244` | 2026-05-27 | Rewritten, not copied. Retains the upstream's JWT-validation rule set (pin the algorithm from configuration, verify `iss` and `aud`, cache JWKS by `kid`, RFC 6750 error shapes) and its two-layer scope-then-claims authorization model. Added: the downstream token-exchange leg (RFC 8693 / RFC 8707), the refuse-rather-than-downgrade rule, and the subject-keyed cache constraint - none of which are upstream, since it addresses a plain resource server rather than one that also acts as a client. Dropped the Curity-product-specific setup and deployment instructions. |

| `.agents/skills/engineering/oauth-flows/SKILL.md` | Public OAuth/OIDC agent skills, notably `github.com/0xfurai/claude-code-subagents` (`agents/oauth-oidc-expert.md`) | MIT | fetched from `main` | 2026-08-24 | Rewritten, not copied. Retains the grant-selection table, the PKCE-always rule, the `state`/`nonce`/exact-redirect-URI checks, the token-storage split and refresh-token rotation with reuse detection. Added: the `iss` callback check for provider mix-up, RFC 8707 resource indicators, the reason `state` and `nonce` are not interchangeable, and the negative-case test list. Dropped the provider-specific integration walkthroughs (Auth0, Okta, Keycloak). |

| `.agents/skills/engineering/ms-rust/[0-9][0-9]_*.md` | `microsoft.github.io/rust-guidelines` (`agents/all.txt`) | MIT | content sha256 `c9f1ac1c` | 2026-08-24 | **Generated, not written.** `generate.py` splits the upstream file by section and records the sha256, so "is this current" has an answer. Verbatim text; the em-dash sweep and other repo-wide edits skip these files deliberately. Section names are read from the source rather than hardcoded, because upstream reorganises - `Safety` became `Correctness`, and `Macros` and `Project` are new since the snapshot this was taken from. `SKILL.md` and `README.md` here are ours: the routing table, the precedence rules, and the two places where sutura deliberately departs (`M-APP-ERROR`, `M-TARGET-CPU`). |

| `.agents/skills/engineering/ponytail/SKILL.md`, `.claude/hooks/ponytail-session-start.sh` | `github.com/DietrichGebert/ponytail` | MIT | `2ed6c52c9d7e5e56942508591085fd45dea277d3` | 2026-08-07 | **adapted.** The ladder and the persistence model are upstream's. Added: where laziness stops in this repo, since ponytail is about volume of code and the governance mechanisms are not volume. The activation hook is a shell reimplementation of upstream's Node one - a Rust repo should not acquire a Node dependency to run an agent hook - and it reads the ruleset from the skill file so there is one copy. OpenCode uses upstream's own plugin via `opencode.json`, which it supports properly. |
| `.agents/skills/agent-system/using-skills/SKILL.md` | `github.com/addyosmani/agent-skills` (`skills/using-agent-skills`) | MIT | via a normalized private copy | 2026-08-24 | **adapted.** Phase routing and the operating behaviours are upstream's; the routing table is this repo's, and the prove-it rules are ours. |
| `.agents/skills/agent-system/skill-policy/SKILL.md` | a private repo's external-skill policy | - | - | 2026-08-24 | **adapted.** Classification and provenance rules are the upstream idea; the two-tier layout, the inclusion bar and the `check-skills` enforcement are ours. |
| `.github/pull_request_template.md`, `.github/ISSUE_TEMPLATE/*` | a private repo's PR template | - | - | 2026-08-24 | **adapted.** The review-snapshot / what / why / how-to-review / validation shape is the upstream idea. Dropped its visual-evidence machinery (no UI here); added the test-causality and invariants sections, which are this repo's distinctive requirements. |

| `.agents/skill-library/**/SKILL.md` (20 skills) | `github.com/addyosmani/agent-skills` | MIT | `5a5ea45e806f82273549fd85e60adb95d55f510d` | 2026-08-21 | **mirror.** Imported as-is into the non-discoverable library tier, each with its own `## Provenance` block; `cargo xtask check-skills` fails if one lacks it. Not imported, deliberately: `frontend-ui-engineering` and `browser-testing-with-devtools` (no UI here), `using-agent-skills` (adapted into the active tier instead), `interview-me` (personal rather than repo workflow). |

## Acknowledged prior art

Not vendored - no code from either is present - but the semantic-compile shape is theirs and
saying so is the honest position. Both Apache-2.0.

| Project | What it contributes to the design |
| --- | --- |
| [Wren](https://github.com/Canner/WrenAI) | Compiling a modelled question into SQL over DataFusion, and the idea that the model rather than the prompt is what makes a number defensible |
| [Spice](https://github.com/spiceai/spiceai) | Federation and acceleration across sources, also on DataFusion |

If code from either is ever adapted, it moves into the table above with a commit and a
description of the local changes.

## Vendored source code

| Local path | Upstream | Licence | Commit / tag | Date | Local changes |
| --- | --- | --- | --- | --- | --- |
| `vendor/mimalloc_rust/**` except `libmimalloc-sys/c_src` | `github.com/purpleprotocol/mimalloc_rust`, the published `mimalloc` 0.1.52 and `libmimalloc-sys` 0.1.49 artifacts | MIT | `abcd2be6171e89190f8087fcaccf001d8bd5bc96`, tag `v0.1.52` | 2026-08-25 | **Vendored to control the bundled allocator version**: the crate bundles mimalloc 3.3.2 and we ship 3.5.0. Kept as path dependencies rather than rewritten, so `unsafe impl GlobalAlloc` stays in third-party code and the workspace's `unsafe_code = "forbid"` is untouched. Four local changes. (1) `libmimalloc-sys/build.rs` prefers a prebuilt archive named by `SUTURA_MIMALLOC_LIB_DIR` and otherwise falls through to upstream's `cc::Build` path unchanged, so `nix build` links a cached derivation while a bare `cargo build` still compiles the bundled amalgamation. (2) `path = "libmimalloc-sys"` restored on the wrapper's dependency: publishing had normalised it to a registry dependency, and without the path the vendored wrapper silently pulls the registry sys crate instead of this one. (3) The `v2` feature is removed from both manifests, because only the v3 tree is vendored and a feature that cannot build is a trap. (4) The `extended` feature, its two `extended.rs` modules and the optional `cty` dependency are removed: we never enable it, and it would otherwise pull `cty` into the lockfile and the vendor tree for nothing. |
| `vendor/mimalloc_rust/libmimalloc-sys/c_src/mimalloc/v3/**` | `github.com/microsoft/mimalloc`, tag `v3.5.0` | MIT | tag `v3.5.0` = commit `18b08671c9302247bfb682286e6bf3cc1773f801` | 2026-08-25 | **Upstream content, subsetted, otherwise unmodified.** Source artifact: `codeload.github.com/microsoft/mimalloc/tar.gz/refs/tags/v3.5.0`, 1 436 476 bytes, sha256 `1e432f0559a4ab512143b9bff7a700541a2c8d4712b26a72de3e0222790da305`. That hash is **of the published tarball**, not of this subset and not a Nix NAR hash, so it can be checked against upstream with `curl` and `sha256sum`; `flake.nix` pins the same hash. Subsetted to `include/` plus the Linux parts of `src/`: `src/prim/{windows,osx,wasi,emscripten}` are omitted because `src/prim/prim.c` picks the platform with `#if defined(_WIN32)` / `__APPLE__` / `__wasi__` / `__EMSCRIPTEN__` / `#else`, so only `prim/unix/prim.c` is ever compiled for our four Linux targets. No file content is altered and no line endings were normalised: upstream ships LF, and the one CRLF file in the release, `src/prim/windows/etw.man`, falls in an omitted directory. The C is compiled by `flake.nix` rather than by the build script; the flags and the reason CMake is not used are argued at `mimallocFor` there. |

### Keeping this current, and who notices

Nothing notices automatically, and it is better to say so than to imply a process. `libmimalloc-sys`
pins its own bundled mimalloc, so neither `cargo update` nor a dependency bot can see the C at
all, and no gate compares `mimallocVersion` in `flake.nix` against upstream's newest tag.

So the mechanism is a person watching `github.com/microsoft/mimalloc/releases`. When a 3.6.0
lands, three things have to happen together: bump `mimallocVersion` and `sha256` in `flake.nix`,
re-copy the subset into `c_src/mimalloc/v3`, and re-run the four-target proof.

**The drift hazard that follows from the fallback**, named because it is not obvious: the version
lives in TWO places. `flake.nix` fetches 3.5.0 for the cached archive, and `c_src` holds 3.5.0 for
the bare-`cargo` path. They agree today. If a future bump changes one and not the other, `nix
build` and `cargo build` will link different allocator versions with no error, and the only symptom
would be `MIMALLOC_VERBOSE=1` printing different versions between the two.
