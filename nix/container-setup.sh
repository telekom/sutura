# shellcheck shell=bash
# Run `just setup` once per ENVIRONMENT CHANGE, not once per container start.
#
# Why this exists at all: `just setup` installs the git hooks, materialises the pixi
# environments and builds the two helper binaries. That is minutes of work, and a developer
# who has to remember to run it gets a container with no hooks - which is how this repo once
# spent a whole session believing gates ran that had never run.
#
# Why it is not a build step: `.git` is in `.dockerignore`, deliberately, so hook installation
# cannot happen at image build time - there is no repository to install into. And the caches it
# warms live in named volumes that mask whatever the image put at those paths.
#
# So the caching is a STAMP rather than a Docker layer: hash the files that decide what setup
# produces, store the hash in a volume-backed path, and skip the work when it matches. Editing
# a `.rs` file does not re-run setup; changing `pixi.lock` does.
set -eu

# The inputs. Each of these changes what setup produces; nothing else does. Kept explicit
# rather than hashing the tree, because hashing the tree would re-run setup on every edit and
# defeat the point.
INPUTS="
Cargo.lock
Cargo.toml
devenv.lock
devenv.nix
flake.lock
pixi.lock
pixi.toml
rust-toolchain-nightly.toml
rust-toolchain.toml
.pre-commit-config.yaml
"

# `target/` is a named volume in compose.dev.yaml, so this survives a container restart -
# which is the entire point - and it is gitignored build output rather than source.
STAMP=/work/target/.setup-stamp

setup_hash() {
    # Missing files are hashed as absent rather than skipped: deleting pixi.toml has to
    # invalidate the stamp too.
    for f in $INPUTS; do
        if [ -f "/work/$f" ]; then
            sha256sum "/work/$f"
        else
            echo "absent $f"
        fi
    done | sha256sum | cut -d' ' -f1
}

run_setup() {
    # Through devenv, because `just` is not on PATH outside the shell - the same reason the
    # git hooks call pixi rather than a nix-provided binary.
    devenv shell -- just setup
}

main() {
    if [ "${SUTURA_SKIP_SETUP:-}" = "1" ]; then
        echo "setup: skipped (SUTURA_SKIP_SETUP=1)"
        return 0
    fi

    want="$(setup_hash)"
    have=""
    [ -f "$STAMP" ] && have="$(cat "$STAMP")"

    if [ "$want" = "$have" ]; then
        echo "setup: environment unchanged ($(printf %.12s "$want"))"
        return 0
    fi

    echo "setup: environment changed, running \`just setup\`"
    mkdir -p "$(dirname "$STAMP")"
    if run_setup; then
        # Only stamp on success. A failed setup must retry on the next start rather than
        # record itself as done - a half-built environment that claims to be ready is worse
        # than one that says it is not.
        printf '%s\n' "$want" > "$STAMP"
        echo "setup: done"
    else
        echo "setup: FAILED - the shell still opens so it can be fixed; setup will retry" >&2
    fi
}

main

# Hand over to whatever was asked for - the CMD, or the command passed to
# `docker compose run`. Without this the container runs setup and exits.
exec "$@"
