#!/usr/bin/env bash
#
# `just demo`: bring the local chat demo up, print where to open it, and remove it again.
#
# VALIDATES BEFORE IT BUILDS. The first failure a reader meets should be the one they can fix in a
# second, and the check has to happen before an image is built or a container is started - so the
# configuration is read and checked here, on the host. Nothing below echoes an environment value, so
# a missing or malformed setting is reported by name and a credential never reaches the terminal.
#
# REUSES THE TIER'S OWN MECHANISMS rather than re-implementing them: `xtask dev-up --with demo` is
# the worktree scope, the ephemeral ports, the health gate and the discovery file; `xtask dev-down`
# is the scoped teardown; and `just dev-endpoint demo` is the ONLY way to learn the browser URL,
# because the port is allocated rather than derived.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

fail() {
    printf 'demo: %s\n' "$1" >&2
    exit 1
}

# `--up-only` brings the profile up and leaves it running, which is what the tier's own remedy for
# an absent `demo` service cites. The default is the full lifecycle: print the URL, supervise, and
# remove everything on exit.
up_only=false
case "${1:-}" in
    --up-only) up_only=true ;;
    "") ;;
    *) fail "unknown argument: $1 - just demo takes none, and the profile-only form is just dev-up-demo" ;;
esac

# ---------------------------------------------------------------- configuration ---
#
# Three values and one acknowledgement, all from the operator's environment.
endpoint="${SUTURA_DEMO_MODEL_ENDPOINT:-}"
model="${SUTURA_DEMO_MODEL:-}"
api_key="${SUTURA_DEMO_MODEL_API_KEY:-}"
acknowledged="${SUTURA_DEMO_ACKNOWLEDGE:-}"

[ -n "$endpoint" ] \
    || fail "set SUTURA_DEMO_MODEL_ENDPOINT to the OpenAI-compatible base URL of the model this demo chats with"
case "$endpoint" in
    http://* | https://*) ;;
    *) fail "SUTURA_DEMO_MODEL_ENDPOINT must be an http:// or https:// URL" ;;
esac
case "$endpoint" in
    *[[:space:]]*) fail "SUTURA_DEMO_MODEL_ENDPOINT must not contain whitespace" ;;
esac

[ -n "$model" ] || fail "set SUTURA_DEMO_MODEL to the model id the endpoint serves"
case "$model" in
    *[[:space:]]* | *'"'*) fail "SUTURA_DEMO_MODEL must be a model id with no whitespace or quote" ;;
esac

# A hosted endpoint needs its key; a local one does not - that is the only difference between the
# two supported shapes, and the test is the HOST a model on this machine is reachable from a
# container by, never a guess at the value.
host="${endpoint#*://}"
host="${host%%/*}"
host="${host%%:*}"
case "$host" in
    127.0.0.1 | localhost | host.docker.internal) local_model=true ;;
    *) local_model=false ;;
esac
if [ -z "$api_key" ] && [ "$local_model" = false ]; then
    fail "set SUTURA_DEMO_MODEL_API_KEY for a hosted endpoint; a model on loopback or host.docker.internal needs none"
fi

[ -n "$acknowledged" ] \
    || fail "set SUTURA_DEMO_ACKNOWLEDGE to your own sentence saying why this single-user demo may read the example as a shared service user - it becomes the deployment's single_user_because"
case "$acknowledged" in
    *'"'* | *\\* | *$'\n'*) fail "SUTURA_DEMO_ACKNOWLEDGE must be one line without a double quote or a backslash" ;;
esac

export SUTURA_DEMO_MODEL_ENDPOINT="$endpoint"
export SUTURA_DEMO_MODEL="$model"
export SUTURA_DEMO_MODEL_API_KEY="$api_key"
export SUTURA_DEMO_ACKNOWLEDGE="$acknowledged"

# ------------------------------------------------------------------ build ---
#
# The shipped server image is one executable with no shell, so it can neither supervise the chat
# client nor answer a probe. `demo/Dockerfile` adds both around it; the tier may not build here, so
# the derived image is built and tagged under the registry variable's own name BEFORE it is asked
# for, and `docker compose up` then finds it locally rather than pulling it.
registry="${SUTURA_IMAGE_REGISTRY:-docker.io}"
demo_image="${registry}/sutura/local-chat-demo:0.11.3"

printf 'demo: building the sutura server image\n' >&2
serve_stream="$(nix build .#oci-serve --no-link --print-out-paths)"
"$serve_stream" | docker load >/dev/null

printf 'demo: building %s\n' "$demo_image" >&2
docker build \
    --build-arg "SUTURA_SERVE_IMAGE=sutura-serve:latest" \
    --build-arg "SUTURA_DEMO_BASE_IMAGE=${registry}/openwebui/open-webui:0.11.3" \
    -t "$demo_image" \
    -f demo/Dockerfile \
    . >&2

# --------------------------------------------------------------- provision ---
if [ "$up_only" = false ]; then
    cleanup() {
        printf '\ndemo: removing the demo profile, its containers and its volumes\n' >&2
        cargo run -q -p xtask -- dev-down || true
    }
    trap cleanup EXIT
fi

printf 'demo: starting the demo profile\n' >&2
cargo run -q -p xtask -- dev-up --with demo

# ------------------------------------------------------------------- run ---
endpoint_hostport="$(just dev-endpoint demo)"
printf '\ndemo: open http://%s in a browser\n' "$endpoint_hostport"

if [ "$up_only" = true ]; then
    printf 'demo: the profile is up; run just dev-down to remove it\n'
    exit 0
fi

printf 'demo: the chat client is UNGOVERNED - it renders what the runtime already decided\n'
printf 'demo: single-user; it proves neither caller identity nor source impersonation\n'
printf 'demo: press Ctrl-C to stop and remove it\n\n'

# Block while the demo answers, so Ctrl-C tears it down and a container that dies takes the task
# with it instead of leaving a half-removed tier behind. Bash's own `/dev/tcp` rather than curl: this
# runs on the host, where nothing here should add a dependency for one check.
port="${endpoint_hostport##*:}"
while (exec 3<>"/dev/tcp/127.0.0.1/${port}") 2>/dev/null; do
    sleep 2
done
printf 'demo: the demo stopped answering\n' >&2
