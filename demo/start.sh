#!/usr/bin/env bash
#
# `just demo`: bring the local chat demo up, print where to open it, and remove it again.
#
# VALIDATES BEFORE IT BUILDS. The first failure a reader meets should be the one they can fix in a
# second, and the check has to happen before an image is built or a container is started - so the
# configuration is read and checked here, on the host. Nothing below echoes an environment value, so
# a missing or malformed setting is reported by name and a credential never reaches the terminal.
#
# `--check` stops after validation, so the configuration can be proved without nix, docker or a
# model; `--up-only` brings the profile up and leaves it running; no argument is the full lifecycle.
#
# SCOPED TO THE DEMO. It starts and stops the `demo` service only (`--only demo`), never the default
# clickhouse and never the identity-provider or DataHub profiles, so a demo run beside another tier
# cannot take that tier down.
#
# REUSES THE TIER'S OWN MECHANISMS: `xtask dev-up --only demo` is the worktree scope, the ephemeral
# port, the health gate and the discovery file; `xtask dev-down --only demo` is the scoped teardown;
# and `just dev-endpoint demo` is the ONLY way to learn the browser URL, because the port is
# allocated rather than derived.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
cd "$root"

fail() {
    printf 'demo: %s\n' "$1" >&2
    exit 1
}

# `--check` validates and stops; `--up-only` brings the profile up and leaves it running; the default
# is the full lifecycle: print the URL, supervise, and remove the demo's own resources on exit.
mode=full
case "${1:-}" in
    --check) mode=check ;;
    --up-only) mode=up-only ;;
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

# ------------------------------------------------------- model endpoint policy ---
#
# Split the URL so the HOST can decide two things: whether the endpoint is on this machine, and
# whether a credential may travel to it in the clear. Bracketed IPv6 is handled apart from a colon
# in a hostname, which is why this is not a single `${x%%:*}`.
scheme="${endpoint%%://*}"
authority="${endpoint#*://}"
authority="${authority%%/*}"
case "$authority" in
    *@*) fail "SUTURA_DEMO_MODEL_ENDPOINT must not contain userinfo" ;;
esac
bracketed=false
case "$authority" in
    \[*\]:*) host="${authority%%]*}"; host="${host#\[}"; suffix="${authority#*]}"; bracketed=true ;;
    \[*\]) host="${authority#\[}"; host="${host%\]}"; suffix=""; bracketed=true ;;
    *:*) host="${authority%%:*}"; suffix=":${authority#*:}" ;;
    *) host="$authority"; suffix="" ;;
esac
if [ -z "$host" ]; then
    fail "SUTURA_DEMO_MODEL_ENDPOINT must contain a URL host"
fi

case "$host" in
    localhost | 127.0.0.1 | ::1 | host.docker.internal) local_model=true ;;
    *) local_model=false ;;
esac

# **A cleartext endpoint is refused unless it is genuinely local AND keyless.** An http:// hosted
# provider sends every prompt, and if a key is set the key itself, in the clear - so https is the
# only shape accepted off this machine. Loopback and the host gateway are the exception because a
# local model has no certificate to present; a credential is still refused over http even there,
# because the rule is about the credential travelling in the clear, not about the destination.
if [ "$scheme" = http ]; then
    [ -z "$api_key" ] \
        || fail "refusing an http:// model endpoint that carries a credential: it would send the key in cleartext - leave SUTURA_DEMO_MODEL_API_KEY unset for a local model, or use https://"
    [ "$local_model" = true ] \
        || fail "refusing an http:// model endpoint that is not on this machine: it would send every prompt in cleartext - use https://"
fi
if [ "$local_model" = false ] && [ -z "$api_key" ]; then
    fail "set SUTURA_DEMO_MODEL_API_KEY for a hosted endpoint; a model on this machine needs none"
fi

# A host loopback name means the container itself from inside it, so rewrite it to the host gateway.
# On native Linux this supplies routing, not a listener: the model must also bind an address the
# bridge can reach. The container healthcheck proves reachability; `--check` proves syntax and
# transport policy only.
case "$host" in
    localhost | 127.0.0.1 | ::1) container_host="host.docker.internal" ;;
    *)
        if [ "$bracketed" = true ]; then
            container_host="[${host}]"
        else
            container_host="$host"
        fi
        ;;
esac
container_endpoint="${scheme}://${container_host}${suffix}${endpoint#"$scheme://$authority"}"

[ -n "$acknowledged" ] \
    || fail "set SUTURA_DEMO_ACKNOWLEDGE to your own sentence saying why this single-user demo may read the example as a shared service user - it becomes the deployment's single_user_because"
case "$acknowledged" in
    *'"'* | *\\* | *[[:cntrl:]]*) fail "SUTURA_DEMO_ACKNOWLEDGE must be one line without a double quote or a backslash" ;;
esac

# The container sees the HOST-GATEWAY spelling, never the loopback one. Exported rather than passed
# as a build argument, so nothing here reaches an image layer.
export SUTURA_DEMO_MODEL_ENDPOINT="$container_endpoint"
export SUTURA_DEMO_MODEL="$model"
export SUTURA_DEMO_MODEL_API_KEY="$api_key"
export SUTURA_DEMO_ACKNOWLEDGE="$acknowledged"

if [ "$mode" = check ]; then
    if [ "$local_model" = true ]; then
        printf 'demo: model endpoint accepted: local, addressed from the container as host.docker.internal over %s\n' "$scheme"
    else
        printf 'demo: model endpoint accepted: remote over %s\n' "$scheme"
    fi
    printf 'demo: configuration is valid; nothing was built or started\n'
    exit 0
fi

# ------------------------------------------------------------------ build ---
#
# The shipped server image is one executable with no shell, so it can neither supervise the chat
# client nor answer a probe. `demo/Dockerfile` adds both around the release binary; the tier may not
# build here, so this script passes the Nix package as a read-only build context and builds the
# derived image BEFORE Compose asks for it. The final tag is keyed by the canonical worktree path,
# so concurrent worktrees cannot replace one another's image between build and startup.
registry="${SUTURA_IMAGE_REGISTRY:-docker.io}"
worktree_key="$(printf '%s' "$root" | git hash-object --stdin)"
export SUTURA_DEMO_IMAGE_TAG="0.11.3-${worktree_key}"
demo_image="${registry}/sutura/local-chat-demo:${SUTURA_DEMO_IMAGE_TAG}"

case "$(uname -m)" in
    x86_64) server_target=x86_64-unknown-linux-musl ;;
    aarch64 | arm64) server_target=aarch64-unknown-linux-musl ;;
    *) fail "the demo has no published Linux server binary for this machine architecture" ;;
esac
printf 'demo: building the sutura server binary\n' >&2
serve_package="$(nix build ".#sutura-serve-${server_target}" --no-link --print-out-paths)"

printf 'demo: building %s\n' "$demo_image" >&2
docker build \
    --build-context "sutura-server=${serve_package}" \
    --build-arg "SUTURA_DEMO_BASE_IMAGE=${registry}/openwebui/open-webui:0.11.3" \
    -t "$demo_image" \
    -f demo/Dockerfile \
    . >&2

# --------------------------------------------------------------- provision ---
if [ "$mode" = full ]; then
    # shellcheck disable=SC2329  # invoked by the EXIT trap below, which shellcheck does not count
    cleanup() {
        printf '\ndemo: removing the demo service, its container and its named volume\n' >&2
        # `--only demo` and NOT the whole project: a bare `dev-down` would remove a clickhouse or a
        # keycloak this worktree started for something else. The scoped destroy takes the demo
        # container and its named volume and leaves the rest of the project alone.
        cargo run -q -p xtask -- dev-down --only demo || true
    }
    trap cleanup EXIT
fi

printf 'demo: starting the demo profile\n' >&2
cargo run -q -p xtask -- dev-up --only demo

# ------------------------------------------------------------------- run ---
endpoint_hostport="$(just dev-endpoint demo)"
printf '\ndemo: open http://%s in a browser\n' "$endpoint_hostport"
printf 'demo: the tool server is REGISTERED but NOT selected - in the chat input open Integrations,\n'
printf 'demo: choose Tools, and turn on sutura before asking a question (docs/demo.md has the steps)\n'

if [ "$mode" = up-only ]; then
    printf 'demo: the profile is up; run just dev-down-demo to remove it\n'
    exit 0
fi

printf 'demo: the chat client is UNGOVERNED - it renders what the runtime already decided\n'
printf 'demo: single-user; it proves neither caller identity nor source impersonation\n'
printf 'demo: press Ctrl-C to stop and remove it\n\n'

# Block while the demo answers, so Ctrl-C tears it down. A Ctrl-C interrupts the loop, runs the EXIT
# trap and exits 130. A container that DIES ends the loop without a signal, and that is a failure of
# the demo - so it exits non-zero rather than reporting success over a demo that stopped answering.
# Bash's own `/dev/tcp` rather than curl: this runs on the host, where nothing here should add a
# dependency for one check.
port="${endpoint_hostport##*:}"
while (exec 3<>"/dev/tcp/127.0.0.1/${port}") 2>/dev/null; do
    sleep 2
done
printf 'demo: the demo stopped answering before it was interrupted; its container or its server\n' >&2
printf 'demo: exited, so this is a failure - see the demo container log for why\n' >&2
exit 1
