# shellcheck shell=bash
# The ClickHouse tier provisioner: start/stop/status/credentials for the server
# `nix/clickhouse-tier.nix` packages as `tier`. Its shape is the two nix tiers before it, ported
# rather than re-derived: the three-answer `status` and the published credential are
# `nix/postgres-tier-provision.sh`'s, and the liveness guard - a kernel-held `flock` rather than a
# pidfile - is `nix/keycloak-tier.nix`'s, for its reason (a lost pidfile says nothing about the
# process). See those files for the defects each rule answers; they are not restated here.
set -o errexit -o nounset

root="$(pwd -P)"
state="$root/.sutura-dev"
# Under the worktree, like Keycloak's: ClickHouse listens on TCP only, so the unix-socket length cap
# that keeps Postgres under `$TMPDIR` does not apply, and one path serves the sandbox and a dev shell.
home="$state/clickhouse"
pidfile="$home/tier.pid"
portfile="$home/port"
log="$home/server.log"
cred="$home/password"
# A SIBLING of `home`, so `start`'s cold path cannot delete the lock a live server holds.
lockfile="$state/clickhouse.lock"
user=sutura

# 0 a server of ours holds the lock, 1 provably none (no state directory), 2 unknown (the lock
# could not be opened) - `nix/keycloak-tier.nix`'s `running`, whose callers treat 2 like 0.
running() {
  [ -d "$state" ] || return 1
  { exec {running_fd}>"$lockfile"; } 2>/dev/null || return 2
  if flock -n -s "$running_fd" 2>/dev/null; then
    flock -u "$running_fd"
    exec {running_fd}>&-
    return 1
  fi
  exec {running_fd}>&-
  return 0
}

answers() {
  curl -fsS --max-time 2 "http://127.0.0.1:$1/ping" >/dev/null 2>&1
}

# A currently free loopback port - a CANDIDATE, not a reservation, so `start` retries a lost race.
allocate_port() {
  python3 - <<'PY'
import socket
with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as listener:
    listener.bind(("127.0.0.1", 0))
    print(listener.getsockname()[1])
PY
}

# Only the HTTP interface, on loopback, and no system log tables: nothing here is kept past `stop`.
# The one user is the one the adapter's `shared-service-user` posture connects as; its password is
# stored as a digest, and the plaintext lives only in `$cred`, 0600, removed with the home. YAML
# rather than XML because every path here is under the worktree, and an XML closing tag for the
# server's scratch directory spells a machine-shared root to `check-worktree-state`.
write_config() {
  digest="$(printf '%s' "$password" | sha256sum | cut -d' ' -f1)"
  cat > "$home/config.yaml" <<EOC
logger:
  level: warning
  console: 1
listen_host: 127.0.0.1
http_port: $port
path: $home/data/
tmp_path: $home/data/scratch/
user_files_path: $home/data/user_files/
disable_internal_dns_cache: 1
users:
  $user:
    password_sha256_hex: $digest
    networks:
      ip: 127.0.0.1
    profile: default
    quota: default
profiles:
  default: {}
quotas:
  default: {}
EOC
}

# A running server whose entry went missing: publish it again, and adopt nothing.
republish() {
  port="$(cat "$portfile" 2>/dev/null || true)"
  if [ -z "$port" ] || ! answers "$port" || [ ! -s "$cred" ]; then
    echo "clickhouse tier: a server holds $lockfile but its port or credential is not readable" >&2
    echo "               here, so it cannot be republished. \`just clickhouse-tier stop\`, then start." >&2
    exit 1
  fi
  echo "clickhouse tier: republishing the entry for the server already running here."
  sutura-tier-endpoint publish "$root" clickhouse 127.0.0.1 "$port"
}

start() {
  if running; then
    if status; then
      echo "clickhouse tier: already up - leaving it to whoever started it."
      return 0
    fi
    republish
    return 0
  elif [ $? -eq 2 ]; then
    echo "clickhouse tier: $lockfile could not be opened to check for a live server, so alive" >&2
    echo "               or dead cannot be proven. $home stays - fix the open and retry." >&2
    exit 1
  fi
  rm -rf "${home:?the tier home is unset}"
  mkdir -p "$home/data"
  # Hex from `/dev/urandom`: no character in it needs quoting in the config or the header.
  password="$(od -An -v -tx1 -N24 < /dev/urandom | tr -d ' \n')"
  ( umask 077; printf '%s' "$password" > "$cred" )
  for _attempt in $(seq 1 10); do
    port="$(allocate_port)"
    write_config
    # The watchdog would fork the server into a child; off, so the process the lock names IS it.
    set -m
    ( exec {start_fd}>"$lockfile"
      flock -x "$start_fd"
      CLICKHOUSE_WATCHDOG_ENABLE=0 exec clickhouse server --config-file="$home/config.yaml"
    ) >"$log" 2>&1 &
    pid=$!
    echo "$pid" > "$pidfile"
    set +m
    for _ in $(seq 1 120); do
      if answers "$port"; then
        printf '%s' "$port" > "$portfile"
        sutura-tier-endpoint publish "$root" clickhouse 127.0.0.1 "$port"
        return 0
      fi
      kill -0 "$pid" 2>/dev/null || break
      sleep 0.5
    done
    if kill -0 "$pid" 2>/dev/null; then
      echo "clickhouse tier: the server did not answer on port $port within 60 seconds" >&2
      tail -20 "$log" >&2
      stop
      exit 1
    fi
    if ! grep -q "Address already in use" "$log"; then
      echo "clickhouse tier: the server exited before it answered" >&2
      tail -20 "$log" >&2
      exit 1
    fi
    echo "clickhouse tier: the allocated port was lost before the server bound it; retrying" >&2
  done
  echo "clickhouse tier: the server could not bind an allocated loopback port after 10 attempts" >&2
  exit 1
}

# `nix/keycloak-tier.nix`'s `stop`: signal the process group, and withdraw the claim and delete the
# home only once no process holds the lock - a server that would not die keeps both.
stop() {
  signalled=0
  if [ -f "$pidfile" ]; then
    signalled=1
    pid="$(cat "$pidfile")"
    kill -TERM -- "-$pid" 2>/dev/null || true
    for _ in $(seq 1 30); do
      kill -0 -- "-$pid" 2>/dev/null || break
      sleep 1
    done
    kill -KILL -- "-$pid" 2>/dev/null || true
    rm -f "$pidfile"
  fi
  refuse=alive
  if running; then
    :
  elif [ $? -eq 2 ]; then
    refuse=unknown
  else
    refuse=
  fi
  if [ -n "$refuse" ]; then
    if [ "$refuse" = unknown ]; then
      echo "clickhouse tier: $lockfile could not be opened, so alive or dead cannot be proven." >&2
    elif [ "$signalled" -eq 1 ]; then
      echo "clickhouse tier: sent TERM and KILL to process group $pid and a server still holds" >&2
      echo "               $lockfile." >&2
    else
      echo "clickhouse tier: no pidfile, and a server still holds $lockfile." >&2
    fi
    echo "               Its endpoint entry and home STAY - find and stop the process by hand," >&2
    echo "               then retry \`stop\`." >&2
    exit 1
  fi
  sutura-tier-endpoint withdraw "$root" clickhouse
  rm -rf "${home:?the tier home is unset}"
}

# The two exports the adapter's `fixture::credential_from_env` reads, for a caller to `eval`. An
# absent credential is a refusal, not an empty answer.
credentials() {
  if [ ! -s "$cred" ]; then
    echo "clickhouse tier: no credential is published for this worktree, so the suite's clickhouse" >&2
    echo "               cells would refuse. \`just clickhouse-tier start\` publishes one." >&2
    exit 1
  fi
  printf 'export SUTURA_CLICKHOUSE_TIER_USER=%s\n' "$user"
  printf 'export SUTURA_CLICKHOUSE_TIER_PASSWORD=%s\n' "$(cat "$cred")"
}

# 0 a server of ours is running AND `endpoints.json` publishes it at the port it answers on;
# 3 one is running and nothing publishes it (`start` republishes; not the caller's to tear down);
# 1 nothing is running here. `nix/postgres-tier-provision.sh`'s three answers.
status() {
  running || return 1
  port="$(cat "$portfile" 2>/dev/null || true)"
  [ -n "$port" ] || return 3
  [ -s "$cred" ] || return 3
  sutura-tier-endpoint published "$root" clickhouse 127.0.0.1 "$port" || return 3
}

case "${1:-}" in
  start) start ;;
  stop) stop ;;
  status) status ;;
  credentials) credentials ;;
  *) echo "usage: $0 start|stop|status|credentials" >&2; exit 2 ;;
esac
