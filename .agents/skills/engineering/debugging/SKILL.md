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
| `just test` / `just causality` / `just ship-check` and every pre-commit tier block, no output | `timeout 20 docker info; echo $?` - **124 means wedged**, and `SKIP=rust-tests` will not help because the hang is not in the hook's own step. Then `df -h` before restarting anything: a restart cannot fix a host out of space, and the bounded paths print that advice where a hang cannot |
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
failed provision can leave containers running. What is fail-closed is **this task's own entries**:
they are withdrawn from the discovery file before the tier is touched, so a failed `dev-up` or
`dev-down` leaves no *docker* endpoint a harness can connect to. **The limit, which is narrower than
that sentence used to be:** since `github.com/telekom/sutura#317` the granularity is one
provisioner's entries rather than the file, so a nix-native tier's entry deliberately survives and
keeps the file alive with it. The file existing therefore says nothing about the docker tier - ask
`just dev-endpoint clickhouse` about a service, not `ls` about the file - and **a docker tier that is
up with its entry withdrawn is the state above**, whose fix is `just dev-up` again.

**The nix Postgres tier in that state HEALS ITSELF now, and only that one.** A postmaster with no
entry is still reachable - a `start` that dies between the bind and its publish, a hand-removed
file, a `stop` that failed - and it used to be reachable the easy way as well, because a `dev-up`
rewrote the whole document from the docker services it read and took a nix tier's entry with it.
That was a `just test` whose postgres cells failed closed, since the wrapper asked
`sutura-postgres-tier status` (the process) while the cells asked the file
(`github.com/telekom/sutura#298`). **Both halves are gone.** `publish` merges per entry
(`github.com/telekom/sutura#317`, held by `dev/src/discovery.rs`'s
`a_second_provisioners_entry_survives_a_publish`), so a `dev-up` leaves a nix entry where it was and
a `just dev-endpoint postgres` between a `dev-up` and the next `just test` answers that
postmaster's socket. And the wrapper's answer is derived from the file, so a genuinely unclaimed
server reads as *unclaimed*, `just test` republishes the entry and leaves the server running.
`checks.postgres-tier` drives the wrapper's three `status` arms (already-up, unclaimed-republish,
stopped) and, inside the unclaimed arm, an entry that is absent and one that points at a different
socket - a mismatched address cannot masquerade as already-up.

**No other nix tier is healed, and for keycloak `start` is NOT the remedy** - it returns 0 having
published nothing. Its guard is `status`, which is the process there and deliberately so (see that
file), so over the surviving JVM it prints *already up* and returns before the `publish` on the
other path. `just keycloak-tier stop` then `start` is what works, and it is not free: `start`
`rm -rf`s the home, so the realm, the client secret and the OS-chosen port are all new and anything
holding the old realm file has to re-read it. The split that would let it heal like postgres is
`github.com/telekom/sutura#324`.

**And a GREEN `just test` no longer means the tier it started came down.** A `stop` that fails keeps
its endpoint entry and says so on standard error; the trap in `nix/with-tier.sh` runs it under
`|| true`, because under the errexit every venue sources that file with, a failing trap command
would rewrite the run's status - including nextest's 100. So the message and the retained claim are
the signal there, never the exit code, and the retry is `just postgres-tier stop`.

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

**A mutation script that restores with `git checkout -- <path>` reverts work the mutation never
made, and the tell is that NOTHING FAILS.** Where causality answers `NOT MECHANICALLY SEPARABLE` or
`INCONCLUSIVE`, the substitute is a loop of *break one thing, run, restore* - and `git checkout`
restores the whole file from the index, not the mutation. Any UNCOMMITTED edit in a file the script
touches is gone, silently, because the restore is exactly what the script is supposed to do.
Measured on this repo: a bypass-reproduction script reverted two uncommitted documentation fixes in
files it mutated, and nothing was red afterwards - it was found by re-grepping for a sentence that
should have been there. **Commit before running one**, and prefer a script that restores from a copy
it made itself; the same class as any `git checkout` in a shared or automated context.

**AND A RESULT FILE WITH A FIXED NAME CANNOT SAY WHICH RUN WROTE IT.** Detaching a long gate and
reading its exit code from a file is the right shape - a pipeline's last stage is the status of
`tail`, not of the gate - but a waiter that fires on `<name>.exit` gets whatever run wrote it last.
**Measured twice in one session, and neither was noticed by care:** a mutation harness that verified
the clean tree only at the END, so a restore that did not take left a mutation behind and the next
mutation adopted it as pristine, reporting `anchor missing` while an unrelated test was red; and a
`ship-check` waiter that reported the PREVIOUS commit's exit 0 as this commit's, because the new run
had not yet replaced the file. Each was caught by a SECOND number disagreeing - the harness's own
`clean tree: red` line, and a metadata file naming a commit that was not `HEAD`.

**So put what is being measured into the artifact's NAME and check it before believing the number**
(`logs/ship-<sha>.exit`), and have a harness snapshot its inputs ONCE rather than re-reading them
per step. The same habit catches the other half: more than one copy of a gate running in one
worktree. `cargo xtask test-causality` creates and removes `target/causality-worktree` and both runs
share one target directory, so two of them clobber each other's reconstruction and NEITHER verdict
is about the tree - three were found running here at once. `pgrep` for the gate before starting one.

## Failure modes already understood here

Read these before spending an hour on a class of bug this repo has met.

| Symptom | Cause |
| --- | --- |
| Error names a lint or flag that looks correct | CRLF in a `.nix` file; `\r` became part of the argument |
| A gate passes locally, fails in the Nix sandbox | it used `git ls-files`; there is no `.git` there |
| A gate passes in the sandbox but checks nothing | it listed files via an absent tool and got an empty list - fail open, loudly |
| Clippy clean locally, fails in CI | you ran a hand-written `cargo clippy` line without `-D warnings`, so a `restriction` lint passed locally that the gate rejects; run `just lint` |
| `cargo-deny` cannot fetch advisories | a Nix build sandbox has no network; it runs as `nix run .#deny` |
| A dependency compiled several times in one CI run | a check not sharing `cargoArtifacts` |
| `401` on a `.narinfo` while `nix-cache-info` succeeds | the cache reads anonymously; artifacts need netrc credentials |
| A detector reports its own source | the pattern matches the file that defines it |
| A lock "released on drop" is still held, and the refusal names THIS process | `flock` lives on the open file DESCRIPTION, so a `close` releases it only when the last descriptor on that description goes. Every `Command::spawn` duplicates the whole table at `fork` and `FD_CLOEXEC` only fires at `exec`, so any spawn in flight holds a copy of every lock. Unlock explicitly rather than relying on the close |
| A harness reads a child's output and the LAST line is missing, under load | it drained the channel once `try_wait` said the process exited. Exited is not READ: the threads reading its pipes may still be in flight, and the last thing a process writes is usually the sentence the assertion is about - so the failure reads as *it never said that* rather than as a lost line. Keep the reader `JoinHandle`s, join, then drain |
| A gate's exit code is the previous run's | the artifact has a fixed name; nothing ties it to the commit it measured |
| Two runs of one gate disagree in one worktree | both create and remove `target/causality-worktree`; neither verdict is about the tree |

**`nextest`'s process-per-test does NOT contain the lock one, and believing it did cost a wrong
diagnosis on `github.com/telekom/sutura#328`.** The reasoning that fails is *the duplicate must come
from another test's fork, so isolating tests removes it*. The forking process is usually **this**
one: a single test that acquires, is refused, drops and re-acquires already forks in between,
because the refusal path probes the holder with `lsof`. Measured on one test per process, single
threaded, with `lsof` unreachable: **6 spurious refusals in 200 runs** before the fix, **0 in 200**
after. Concurrency raises the rate; it is not the cause.

**A FAILING exec is the widest window, so a missing tool makes this MORE likely, not less** - the
opposite of the intuition, and the reason the Nix build sandbox was where it kept appearing. Over
3000 iterations: spawning a program that does not exist gave 29-807 refusals, spawning a present
`lsof` gave 0, spawning nothing gave 0. A child replaced by `exec` drops the copy at once; a child
that fails to exec is torn down instead, and outlived the parent's close by 544 us mean and 5.9 ms
worst here.

The half of that which generalises past the lock: **a flake whose rate scales with load or with
concurrency makes a single green run a bad control** for *this change breaks N tests* - its
greenness is luck in either direction, so it can make the base look broken as easily as it can make
a change look innocent. And when a venue seems to be exempt from a failure, measure the venue rather
than reasoning about its isolation model.

## Do not

- Do not add `#[allow]` or an exemption to make a gate pass. Both are visible diffs and both
  will be asked about.
- Do not widen a test's tolerance until it passes.
- Do not report "should be fixed". Paste the green run.
