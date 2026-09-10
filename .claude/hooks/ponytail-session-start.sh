#!/usr/bin/env sh
# Injects the ponytail ruleset at session start, when it is switched on.
#
# Upstream ponytail (github.com/DietrichGebert/ponytail, MIT) ships this as a Node plugin.
# This is a shell reimplementation of just the activation hook, for one reason: a Rust repo
# whose only Node dependency is an agent hook has a Node dependency, and the hook has to run
# before anything else works. Twenty lines of `sh` needs nothing.
#
# It reads the ruleset from the skill file, so there is ONE copy of the rules. A hook with its
# own inlined copy is a second place to edit and a guaranteed divergence.
#
# Wired for Claude Code (.claude/settings.json) and Codex (.codex/hooks.json). OpenCode loads
# upstream's own plugin instead - see opencode.json - because it supports one properly.
#
# ON by default, at mode `full`. Override per checkout with .ponytail-active (gitignored):
#   echo ultra > .ponytail-active     stronger
#   echo lite  > .ponytail-active     weaker
#   echo off   > .ponytail-active     switch it off
#
# `off` is one of upstream's own modes, so the override file stays the ONE place the mode
# lives - an opt-out file next to it would be a second piece of state saying the same thing.
set -eu

root="${CLAUDE_PROJECT_DIR:-$(git rev-parse --show-toplevel 2>/dev/null || pwd)}"
flag="$root/.ponytail-active"
skill="$root/.agents/skills/engineering/ponytail/SKILL.md"

mode=full
if [ -f "$flag" ]; then
    mode="$(head -n1 "$flag" 2>/dev/null | tr -d '[:space:]')"
    [ -n "$mode" ] || mode=full
fi
if [ "$mode" = off ]; then
    exit 0
fi
[ -f "$skill" ] || {
    echo "ponytail: on (mode $mode) but $skill is missing" >&2
    exit 0
}

printf 'PONYTAIL MODE: %s. The following is active for every response.\n\n' "$mode"
cat "$skill"
