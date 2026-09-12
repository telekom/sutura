#!/bin/sh
# The cheap, text-only half of the citation gate: does every task this repository's prose
# CITES actually exist?
#
# WHY THIS EXISTS AT ALL. `docs.yml`'s verify job skips the `hygiene` nix build for a
# prose-only pull request, because that build is 15m45s of Rust dependency closure so that
# `xtask` can read files, against 3 seconds for the site build. Exactly one property of
# `hygiene` was worth paying that for on a prose change - a citation of a task that does not
# exist is a failure - and deferring it to the `main` push moves that failure from the pull
# request to the default branch, where it blocks the publish instead of the merge. A post-merge
# failure is not a pull-request gate. So the property is checked HERE, in seconds of shell with
# no compiler, and `docs/implementation-plan-identity-and-services.md` records the trade.
#
# NARROWER THAN `check-guidance`, AND SAID SO RATHER THAN IMPLIED. This checks citations and
# nothing else - not a stale phrase, not a version against its pin, not a contradicted claim,
# not a count in prose. Those are still deferred to the `main` push on a prose-only pull
# request, and the plan section names them as the residual risk rather than leaving the reader
# to assume parity.
#
# TWO CITATION FORMS, TWO AUTHORITIES, NEITHER OF THEM A LIST KEPT HERE. A second copy of the
# task names would be one more thing to keep in step, which is the class of drift this
# repository already has gates about:
#
#   `just <task>`         names come from `just --summary`, read on stdin
#   `cargo xtask <task>`  names come from the `TASKS` table in xtask/src/task_table/, which
#                         same table `--help` and dispatch are built from
#
# FAIL CLOSED IN THE DIRECTION THAT COSTS A RUNNER MINUTE. Either authority coming back empty
# means the SCAN is broken rather than the prose, so this exits 2 - a distinct code the caller
# turns into "run the full gates" rather than into a pass. An empty list would otherwise make
# the check pass by looking at nothing, which is the failure mode a text-scanning gate is most
# prone to.
#
# Usage:
#   just --summary | sh .github/scripts/check-task-citations.sh [file.md ...]
#
# With no file arguments every tracked `*.md` is read, which is deliberate: it needs no base
# ref, so there is no bad-diff path that could quietly narrow the scan to nothing.
#
# Exit codes:
#   0  every citation names a task that exists
#   1  a citation names a task that does not
#   2  the scan could not be performed - the caller must run the full gates
#
# POSIX sh, and no heredoc anywhere. An indented heredoc terminator does not terminate, which
# is what broke the first attempt at this step; `sh -n` and `shellcheck` - the latter through
# `just lint-ci`, whose script list is a `find` over the tree - both read this file.
set -eu

recipes="$(cat)"

if [ -z "$(printf '%s' "$recipes" | tr -d ' \t\n')" ]; then
    echo "check-task-citations: the just task list on stdin is empty" >&2
    echo "  the scan is broken, not the prose" >&2
    exit 2
fi

# The xtask names, out of the one table `--help` and dispatch already share. Read as TEXT
# rather than by running the binary, because running it needs the dependency closure this
# check exists to avoid paying. A format change in that table yields nothing here and is
# caught by the emptiness test below rather than by silently passing.
gates="$(sed -n 's/^ *name: "\([a-z0-9-]*\)",$/\1/p' xtask/src/task_table/*.rs || true)"

if [ -z "$(printf '%s' "$gates" | tr -d ' \t\n')" ]; then
    echo "check-task-citations: parsed no task names out of xtask/src/task_table/" >&2
    echo "  the scan is broken, not the prose" >&2
    exit 2
fi

if [ "$#" -gt 0 ]; then
    candidates="$*"
else
    candidates="$(git ls-files '*.md')"
fi

# ONE EXEMPTION, AND IT IS A SPEECH-ACT DISTINCTION RATHER THAN A HOLE TO PUT THINGS IN. An
# implementation plan's job is to name what does NOT exist yet - `docs/implementation-plan-*.md`
# has a section headed "`just demo`" about a task nobody has written - so a forward reference
# there is the document working correctly, and a gate that failed on it would be a gate people
# route around. State the limit with the claim: a plan that cites a task which was RENAMED is
# not caught here either, by the same exemption.

# EMPTINESS IS TWO DIFFERENT ANSWERS, and conflating them is how a plan-only pull request would
# come to pay the 15m45s this whole step exists to avoid. Nothing to read AT ALL means the scan
# is broken - exit 2, and the caller runs the full gates. Nothing left AFTER the exemption means
# there was genuinely nothing here to check - exit 0.
if [ -z "$(printf '%s' "$candidates" | tr -d ' \t\n')" ]; then
    echo "check-task-citations: no markdown to read" >&2
    echo "  the scan is broken, not the prose" >&2
    exit 2
fi

files="$(printf '%s\n' "$candidates" | tr ' ' '\n' | grep -v '^docs/implementation-plan' || true)"

if [ -z "$(printf '%s' "$files" | tr -d ' \t\n')" ]; then
    echo "check-task-citations: ok - nothing outside the implementation plans to read"
    exit 0
fi

# One awk pass. Candidates come from two places, because this repository writes citations in
# both:
#
#   * an inline code span - `just validate` - which is the convention in prose, and the same
#     odd-field backtick walk `check-guidance`'s own dead-path check uses;
#   * a line inside a fenced block, which is how AGENTS.md's command listing is written.
#
# Prose OUTSIDE a code span is not read, and that is the whole reason this is not a plain grep:
# "just" is an English adverb and this repository uses it as one constantly. A citation that
# wraps across a line break is missed - it fails OPEN on that one instance, which is the same
# limit `check-guidance` has, and is why the full gate still runs on `main`.
#
# shellcheck disable=SC2086
# ^ DELIBERATE word splitting: `files` is a newline-separated path list. No tracked path here
#   contains a space; `git ls-files` would quote one if it did, and awk would then fail to open
#   it - a loud failure rather than a silent skip.
printf '%s\n' "$recipes" | awk -v gates="$gates" '
    # Counts are kept by hand rather than with `length(array)`, which is a gawk/mawk extension
    # the awk on a developer machine may not have. The summary line has to print everywhere the
    # check runs, or nobody trusts what it says it read.
    function register(list, into,   parts, n, i, added) {
        added = 0
        n = split(list, parts, /[ \t\n]+/)
        for (i = 1; i <= n; i++) {
            if (parts[i] != "" && !(parts[i] in into)) {
                into[parts[i]] = 1
                added = added + 1
            }
        }
        return added
    }
    # Is `word` the name of a task, or something that is not a name at all?
    #
    # Skipped rather than reported: a flag (`just --list` is correct), a placeholder
    # (`just <task>` is how a usage line is written) and an empty remainder. Reading any of
    # those as a deleted task is how a gate comes to fail on the page that documents it -
    # which has happened here, and is why `check-guidance` has the same three exclusions.
    function verdict(word, known, kind, path, line,   name) {
        if (substr(word, 1, 1) == "-") return
        name = word
        sub(/[^A-Za-z0-9_-].*$/, "", name)
        if (name == "") return
        if (name in known) return
        printf "%s:%d: `%s %s` names no %s task - it was renamed, deleted or never existed\n", \
            path, line, kind, name, kind > "/dev/stderr"
        bad = 1
    }
    function citation(span, path, line,   parts, n) {
        n = split(span, parts, /[ \t]+/)
        if (n < 2) return
        if (parts[1] == "just") { verdict(parts[2], recipes, "just", path, line); return }
        if (parts[1] == "cargo" && parts[2] == "xtask" && n >= 3) {
            verdict(parts[3], xtasks, "cargo xtask", path, line)
        }
    }
    BEGIN { bad = 0; seen = 0; nrecipes = 0; nxtasks = register(gates, xtasks) }
    NR == FNR { nrecipes = nrecipes + register($0, recipes); next }
    FNR == 1 { fenced = 0; seen = seen + 1 }
    /^[ \t]*```/ { fenced = !fenced; next }
    {
        if (fenced) {
            bare = $0
            sub(/^[ \t]*/, "", bare)
            citation(bare, FILENAME, FNR)
        }
        # Fields 2, 4, 6 ... of a backtick split are the spans; the bound stops an
        # unterminated trailing backtick from being read as one.
        n = split($0, spans, "`")
        for (i = 2; i < n; i += 2) citation(spans[i], FILENAME, FNR)
    }
    END {
        if (bad) {
            print "" > "/dev/stderr"
            print "A task named in prose that does not exist reads as current. Fix the citation," > "/dev/stderr"
            print "or add the task. The authorities are `just --summary` and the TASKS table in" > "/dev/stderr"
            print "xtask/src/task_table/ - not a list in this script." > "/dev/stderr"
            exit 1
        }
        printf "check-task-citations: ok - %d file(s), %d just task(s), %d xtask task(s)\n", \
            seen, nrecipes, nxtasks
    }
' - $files
