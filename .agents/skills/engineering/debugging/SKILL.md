---
name: debugging
description: Root-cause a failing test, a red gate, a build error, or unexplained behaviour - evidence before hypothesis, and reproduce before fixing.
---

# Debugging

The rule: **reproduce, then explain, then fix.** A fix applied before the cause is understood
is a guess, and a guess that turns the symptom green is worse than a red test.

## 1. Reproduce, narrowest first

```bash
cargo nextest run --workspace --all-features <test_name>   # one test
cargo run -q -p xtask -- <gate>                            # one gate
nix build .#checks.x86_64-linux.<name> -L                  # what CI actually ran
```

If it reproduces only in CI, the difference is the environment, and there are three usual
ones: **no `.git` in the Nix sandbox**, **no network in a Nix build**, and **`--all-features`
in the gates but not in your inner loop**.

## 1a. A gate that HANGS is a different failure, and it prints nothing

No error to paste, so it reads as a slow machine and sends you to the wrong place. Measured cost
here before it was understood: 38 min, 50 min, and an 864s SIGTERM, across **three sessions that
each diagnosed it independently and declined it as out of lane.**

The cause is a subprocess wait with no timeout. `Command::status()` and `.output()` wait forever, so
a dependency that ACCEPTS and never answers - a wedged Docker Desktop is the case that happened -
hangs whatever called it. `docker version` and `docker info` were both still running when a 25s
external timeout killed them, twice, 50 minutes apart.

| Symptom | What to check |
| --- | --- |
| `just test` / `just causality` / `just ship-check` and every pre-commit tier block, no output | `timeout 20 docker info; echo $?` - **124 means wedged**, and `SKIP=rust-tests` will not help because the hang is not in the hook's own step |
| stray processes accumulate | the probe kills its child, not the child's descendants; `docker` CLI plugins outlive it until the daemon recovers |
| a gate hangs only AFTER provisioning starts | it should not any more - the readiness loop's `ps` carries the query budget, so this is a bug rather than the known shape |

Every wait on a docker child *that goes through `compose::docker`* is bounded, and the budgets are
per KIND of call because one number cannot serve a pull and a status query:
`SUTURA_DOCKER_PROBE_TIMEOUT_SECS` for the pre-flight, `SUTURA_DOCKER_QUERY_TIMEOUT_SECS` for a
`ps` / `port` / `ls`, and `SUTURA_DOCKER_PROVISION_TIMEOUT_SECS` for an `up` / `down`. The defaults
and the argument for each are in `xtask/src/compose/docker/bounded.rs`, not repeated here.

**What holds that, and where it stops.** Nothing does yet - it is one wait loop in one module,
which is a shape rather than a mechanism, and a `.output()` written beside it would be unbounded
again with every test still green. The path-scoped gate that would hold it is issue 274, and what
even that holds is narrower than the sentence: *no second wait loop in that directory*, never
*every docker child is bounded*. So do not read a green run as the property, and check these three
before concluding a hang is not a wait:

| Not covered | Why |
| --- | --- |
| the descendants | the kill reaches `docker`, not the CLI plugins it spawned, so a wedged daemon leaves those until it recovers |
| the reap after the kill | unbounded, deliberately - see `abandon`'s doc; the process has just been sent `SIGKILL`, so a bound there would be a bound on the kernel |
| `compose::lock`'s holder probe (**open**, unlike the two above) | `lsof` is run with `.output()` on the same `dev-up` path - not a docker child, so the sentence survives literally, and a hang there still reads exactly like one |

One more thing a green run does NOT mean: a timed-out `up` deliberately does NOT tear down - it
names `just dev-down` instead, for the reasons `xtask/src/compose.rs`'s `abandoned` records, so a
failed provision can leave containers running. What is fail-closed is the discovery file: it is
removed BEFORE the tier is touched, so a failed `dev-up` or `dev-down` leaves no endpoint a harness
can connect to. **A tier that is up with no discovery file is that**, and `just dev-up` again is the
fix - so is a `just test` whose postgres cells fail closed after a `dev-up` in the same worktree,
because `publish` writes the whole file and the nix tier's entry goes with it
(`sutura-postgres-tier stop && start` republishes).

Held the same way as the sentence above it, and worth the same suspicion: `with_endpoints_forgotten`
covers everything inside it by construction, but a third tier-changing path would have to be routed
through it by hand. One placement in `dev-up` and one in `dev-down`, not a mechanism.

**If a gate hangs for minutes with no output, kill it and say so** - a hang is evidence, not a slow
machine.

## 2. Get the real error

- Paste the actual message. Do not paraphrase it: `unknown lint: 'warnings\r'` and
  `unknown lint: 'warnings'` are different bugs, and the difference is invisible unless
  quoted.
- Suspect encoding when an error names a flag or lint that looks correct. A carriage return
  inside a Nix `''…''` string becomes part of a shell argument.
- `-L` on `nix build` shows the build log. Without it you get a store path and no reason.
- For a cached Nix failure, `nix log <drv>` still has the output.

## 3. Find the cause, not the coincidence

Ask, in order:

1. **What changed?** `git diff`, `git log -p -- <file>`. `cargo xtask classify --since <base>`
   tells you what the diff touches.
2. **Is the failure in the code or in the gate?** A gate that dies on an unreadable path or a
   missing tool **fails open or fails wrong** - check whether the gate itself is broken
   before believing its verdict.
3. **Does the mechanism exist?** A rule stated in prose is not enforced. If an invariant
   should have caught this and did not, the missing check is the bug.

## 4. Fix, and prove it

- Write the test **first** and watch it fail against the current behaviour. That is the only
  evidence the test tests anything; `AGENTS.md` requires it and `cargo xtask test-causality`
  checks it.
- Fix the cause. If you fix a symptom deliberately, say so and say why.
- Re-run the narrow reproduction, then `gates`.

## Failure modes already understood here

Read these before spending an hour on a class of bug this repo has met.

| Symptom | Cause |
| --- | --- |
| Error names a lint or flag that looks correct | CRLF in a `.nix` file; `\r` became part of the argument |
| A gate passes locally, fails in the Nix sandbox | it used `git ls-files`; there is no `.git` there |
| A gate passes in the sandbox but checks nothing | it listed files via an absent tool and got an empty list - fail open, loudly |
| Clippy clean locally, fails in CI | you ran the shell's bare `cargo`, which is a nightly for the cranelift backend and lints differently; `source nix/stable-env.sh` first |
| `cargo-deny` cannot fetch advisories | a Nix build sandbox has no network; it runs as `nix run .#deny` |
| A dependency compiled several times in one CI run | a check not sharing `cargoArtifacts` |
| `401` on a `.narinfo` while `nix-cache-info` succeeds | the cache reads anonymously; artifacts need netrc credentials |
| A detector reports its own source | the pattern matches the file that defines it |

## Do not

- Do not add `#[allow]` or an exemption to make a gate pass. Both are visible diffs and both
  will be asked about.
- Do not widen a test's tolerance until it passes.
- Do not report "should be fixed". Paste the green run.
