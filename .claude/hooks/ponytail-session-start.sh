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
# Off by default. Switch on:  touch .ponytail-active     (gitignored)
#              or:  echo ultra > .ponytail-active
set -eu

root="${CLAUDE_PROJECT_DIR:-$(git rev-parse --show-toplevel 2>/dev/null || pwd)}"
flag="$root/.ponytail-active"
skill="$root/.agents/skills/engineering/ponytail/SKILL.md"

[ -f "$flag" ] || exit 0
[ -f "$skill" ] || {
    echo "ponytail: switched on but $skill is missing" >&2
    exit 0
}

mode="$(head -n1 "$flag" 2>/dev/null | tr -d '[:space:]')"
[ -n "$mode" ] || mode=full

printf 'PONYTAIL MODE: %s. The following is active for every response.\n\n' "$mode"
cat "$skill"
