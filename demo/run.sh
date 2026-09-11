#!/usr/bin/env bash
#
# The demo container's supervisor: the sutura server and the chat client as two children of ONE
# process, so either one ending ends the demo.
#
# The rule compose cannot state. `depends_on` gates startup, not lifetime, so two services would
# leave a chat client answering nothing once its tool server stopped. Here `wait -n` returns on
# whichever child exits first, the trap signals the other, and the container exits non-zero - which
# the tier's health gate reads as a failed provision rather than a silent success.
#
# NOTHING HERE LOGS AN ENVIRONMENT VALUE. The model API key arrives in the environment and is
# written only into the chat client's own process environment; it is never echoed, and neither is
# the deployment token this script generates.
set -uo pipefail

corpus="${SUTURA_DEMO_CORPUS:-/opt/sutura-demo/corpus}"
sutura_port="${SUTURA_DEMO_SUTURA_PORT:-9000}"
webui_port="${SUTURA_DEMO_WEBUI_PORT:-8080}"
run_dir="${SUTURA_DEMO_RUN_DIR:-/run/sutura-demo}"

endpoint="${SUTURA_DEMO_MODEL_ENDPOINT:?SUTURA_DEMO_MODEL_ENDPOINT is required}"
model="${SUTURA_DEMO_MODEL:?SUTURA_DEMO_MODEL is required}"
acknowledged="${SUTURA_DEMO_ACKNOWLEDGE:?SUTURA_DEMO_ACKNOWLEDGE is required}"
api_key="${SUTURA_DEMO_MODEL_API_KEY:-}"

mkdir -p "$run_dir"
chmod 0700 "$run_dir"

# The bearer token that authenticates the chat client to the server. Generated per start, written
# 0600 under `run_dir`, and read back by the probe - never printed, never in an image layer.
token="$(python3 -c 'import secrets; print(secrets.token_urlsafe(48))')"
printf '%s' "$token" > "$run_dir/token"
chmod 0600 "$run_dir/token"

# The deployment the server serves. `single-user` with an operator acknowledgement is the ONLY shape
# this demo has: one identity reads the example, and pretending otherwise is what the walkthrough
# warns against. `development` keeps the generated interface description on, which the OpenAPI
# connection needs.
{
    printf '%s\n' \
        'server:' \
        '  host: "127.0.0.1"' \
        "  port: ${sutura_port}" \
        'security:' \
        '  identity: "single-user"' \
        "  single_user_because: \"${acknowledged}\"" \
        "  access_token: \"${token}\"" \
        'catalogs:' \
        '  - name: "model"' \
        '    kind: "markdown"' \
        "    dir: \"${corpus}/catalog\"" \
        "    data_dir: \"${corpus}/data\"" \
        '    version: "demo"' \
        'sources:' \
        '  local:' \
        '    kind: "files"' \
        "    data_dir: \"${corpus}/data\"" \
        '    posture: "shared-service-user"'
} > "$run_dir/base.yaml"
chmod 0600 "$run_dir/base.yaml"

export SUTURA_CONFIG_DIR="$run_dir"
export SUTURA_ENVIRONMENT="development"

# The chat client's own configuration. The tool connection is the NATIVE OpenAPI shape - no plugin:
# `type: openapi`, the server's loopback URL, `path: openapi.json`, a bearer `auth_type`, and
# `config.enable`. The document it reads describes exactly the two semantic operations.
export OPENAI_API_BASE_URL="$endpoint"
export OPENAI_API_KEY="${api_key:-local}"
export DEFAULT_MODELS="$model"
# A single-user demo: no signup, no login. The port is loopback-bound by the compose block, so this
# is one person on one machine, which is the deployment this page describes and no more.
export WEBUI_AUTH="false"
export ENABLE_SIGNUP="false"
export TOOL_SERVER_CONNECTIONS="[{\"type\": \"openapi\", \"url\": \"http://127.0.0.1:${sutura_port}\", \"spec_type\": \"url\", \"spec\": \"\", \"path\": \"openapi.json\", \"auth_type\": \"bearer\", \"key\": \"${token}\", \"config\": {\"enable\": true}, \"info\": {\"id\": \"sutura\", \"name\": \"sutura\", \"description\": \"Certified metric questions\"}}]"

printf 'sutura-demo: starting the server on 127.0.0.1:%s and the chat client on 127.0.0.1:%s\n' \
    "$sutura_port" "$webui_port" >&2

/usr/local/bin/sutura-serve &
sutura_pid=$!

# The chat client's own entrypoint, from its own working directory.
(
    cd /app/backend || exit 1
    exec bash start.sh
) &
webui_pid=$!

terminate() {
    trap - TERM INT
    kill -TERM "$sutura_pid" "$webui_pid" 2>/dev/null || true
    wait 2>/dev/null || true
}
trap 'terminate; exit 143' TERM INT

wait -n "$sutura_pid" "$webui_pid"
status=$?
printf 'sutura-demo: a supervised process exited (status %s); terminating the demo\n' "$status" >&2
terminate

# A child that exited 0 is still the demo ending: the container is only healthy while BOTH serve.
if [ "$status" -eq 0 ]; then
    status=1
fi
exit "$status"
