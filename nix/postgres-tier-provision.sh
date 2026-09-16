# shellcheck shell=bash
# The Postgres tier provisioner: start/stop/status/credentials for the cluster `nix/postgres-tier.nix`
# packages as `tier`. Moved out of that file's `text = ''...''` because the derivation plus its
# conformance check put the file over the 1000-line cap; every `${` here was already the nix escape
# `''${` for a literal bash `${`, so this move is a straight un-escape, not a behaviour change.
set -o errexit -o nounset

root="$(pwd -P)"
# The worktree key names local state. It does NOT choose a port: hashing an unbounded set of
# paths into a finite port range collides by construction.
if [ -n "${NIX_BUILD_TOP:-}" ]; then
  # In the sandbox the build-tree source path is deep, but `$NIX_BUILD_TOP` itself is short.
  pg="$NIX_BUILD_TOP/.sutura-dev/pg"
else
  # A worktree can be far deeper than a socket allows, so the server lives in a short
  # per-worktree directory under TMPDIR, keyed by a hash of the worktree's physical path.
  #
  # **ONE SPELLING OF THAT KEY, AND IT IS `sutura_dev::scope::Scope`'S.** This line derived a
  # `cksum` CRC-32 while `Scope::scratch` derives the first four bytes of SHA-256 over the
  # same canonical root: two keys for one worktree, so no Rust writer could name this
  # directory and nothing compared the two - `github.com/telekom/sutura#405`'s property 1, and
  # the line its instance-5 dismissal rested on. The path below is `Scope::scratch("pg")`
  # exactly, and `the_tier_and_the_rust_scope_derive_one_worktree_key` in
  # `xtask/src/worktree_state.rs` RUNS these two lines and compares what they build with it,
  # so a change on either side the other does not match reddens `just test`.
  key="$(printf '%s' "$root" | sha256sum | cut -c1-8)"
  pg="${TMPDIR:-/tmp}/sutura-$key-pg"
fi
# THE CREDENTIAL, and where it lives is the whole of why the client can stop defaulting one.
#
# `github.com/telekom/sutura#455`: the adapter's `local_config` used to substitute
# `sutura`/`sutura`/`sutura` when `SUTURA_DEV_*` was unset, in a `pub fn` whose `host` and
# `port` are parameters - so the *only local containers can reach this* argument the compose
# file makes did not cover it, and nothing in the tree ever SET those variables. It refuses
# now, which means the provisioner has to publish what it provisioned.
#
# The role and database NAMES are chosen here and published; only the password is generated,
# per worktree, and it is generated rather than derived from the worktree path because a
# derivation is a value anybody who knows the path can compute.
#
# It sits BESIDE the data directory rather than inside it - `initdb` refuses a non-empty
# target - and `stop` removes both, so a fresh cluster can never pair with a stale password.
# Nothing tracked by git ever holds it: this repository is public.
user=sutura
mtls_user=sutura_mtls
db=sutura
cred="$pg.cred"
# The loopback TLS material, written at `start` beside the cluster and removed at `stop` like
# the credential: a private key must not survive in a directory the tier reuses, and nothing
# tracked by git ever holds it. `ca.crt` is the public anchor a TLS served test verifies the
# server chain against.
tls_ca_key="$pg.ssl/ca.key"
tls_key="$pg.ssl/server.key"
tls_cert="$pg.ssl/server.crt"
tls_ca="$pg.ssl/ca.crt"
tls_client_key="$pg.ssl/client.key"
tls_client_cert="$pg.ssl/client.crt"
# A unix socket path caps around 100 bytes on macOS. Refuse early with a message that names the
# cause, rather than let pg_ctl fail with a bare "could not create any Unix-domain sockets" in
# the log. `$TMPDIR` on a dev machine is well short of this; a custom long one is the case
# this catches at the point it fails.
if [ ${#pg} -gt 80 ]; then
  echo "socket path too long (\$pg): a unix socket cannot be created here" >&2
  exit 1
fi

# One ephemeral CA signs both sides of the tier. The server leaf carries both loopback names;
# the client leaf's CN is the dedicated database role that `pg_hba.conf` requires a
# certificate for. This is test PKI, removed with the cluster, never tracked.
generate_tls() {
  mkdir -p "$pg.ssl"
  ( umask 077
    rm -f "$pg.ssl"/*
    openssl genrsa -out "$tls_ca_key" 2048
    openssl req -x509 -new -key "$tls_ca_key" -out "$tls_ca" -days 30 -sha256 \
      -subj "/CN=sutura-postgres-tier-ca" \
      -addext "basicConstraints=critical,CA:TRUE" \
      -addext "keyUsage=critical,keyCertSign,cRLSign"

    openssl genrsa -out "$tls_key" 2048
    openssl req -new -key "$tls_key" -out "$pg.ssl/server.csr" -subj "/CN=postgres-tier"
    cat > "$pg.ssl/server.ext" <<'EO_SERVER_EXT'
basicConstraints=critical,CA:FALSE
keyUsage=critical,digitalSignature,keyEncipherment
extendedKeyUsage=serverAuth
subjectAltName=DNS:localhost,IP:127.0.0.1
EO_SERVER_EXT
    openssl x509 -req -in "$pg.ssl/server.csr" -CA "$tls_ca" -CAkey "$tls_ca_key" \
      -CAcreateserial -out "$tls_cert" -days 30 -sha256 -extfile "$pg.ssl/server.ext"

    openssl genrsa -out "$tls_client_key" 2048
    openssl req -new -key "$tls_client_key" -out "$pg.ssl/client.csr" -subj "/CN=$mtls_user"
    cat > "$pg.ssl/client.ext" <<'EO_CLIENT_EXT'
basicConstraints=critical,CA:FALSE
keyUsage=critical,digitalSignature,keyEncipherment
extendedKeyUsage=clientAuth
EO_CLIENT_EXT
    openssl x509 -req -in "$pg.ssl/client.csr" -CA "$tls_ca" -CAkey "$tls_ca_key" \
      -CAcreateserial -out "$tls_client_cert" -days 30 -sha256 -extfile "$pg.ssl/client.ext"
  )
  chmod 644 "$tls_ca" "$tls_cert" "$tls_client_cert"
}

# Ask the kernel for a currently free loopback port. The socket closes when Python exits, so
# this is a CANDIDATE rather than a reservation; `start_postmaster` treats a lost bind race as
# a retry and publishes only after Postgres owns the listener.
allocate_port() {
  python3 - <<'PY'
import socket
with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as listener:
    listener.bind(("127.0.0.1", 0))
    print(listener.getsockname()[1])
PY
}

# `postmaster.pid` line four is the port the running server itself records. It is the only
# answer an idempotent `start` or `status` accepts; neither recomputes a candidate.
running_port() {
  [ -s "$pg/postmaster.pid" ] || return 1
  candidate="$(sed -n '4p' "$pg/postmaster.pid")"
  case "$candidate" in
    ""|*[!0-9]*) return 1 ;;
    *) printf '%s' "$candidate" ;;
  esac
}

# THE FALLBACK for exactly the case `pg_ctl status` cannot see: github.com/telekom/sutura#807,
# reproduced directly - remove only `postmaster.pid` from a live cluster, and `pg_ctl status`
# answers "no server running" (rc=3) while the server keeps answering `select 1`. `stop`
# below tries `pg_ctl status` FIRST and only calls this when that already said "not running",
# so a SIGSTOPped postmaster whose pidfile is still there never reaches this function at all -
# `pg_ctl status`'s own `kill(pid, 0)` sees a stopped-but-alive process fine, and that path is
# this check's SIGSTOP fixture. This function exists for the pidfile-independent half alone: a
# unix socket file `.s.PGSQL.<port>` is the postmaster's own bind and outlives the pidfile, so
# parsing its name for a port and asking that port a real query is what tells a live cluster
# apart from a socket file a crash left behind, with no pidfile to read at all.
#
# THREE outcomes, not two: a SIGSTOPped or cgroup-frozen postmaster with NO pidfile is a
# real, reproduced fifth state (#807's pidfile loss plus an outside freeze), and an earlier
# draft here called it "unreached" and "the safe direction" - neither held. `psql` against a
# frozen socket HANGS; `PGCONNECT_TIMEOUT` bounds the hang but a larger value only narrows
# the window, never closes it, since a frozen process exhausts any finite timeout. A timeout
# is therefore NOT proof of death - only a REFUSED connection is, meaning no listener at all.
# Return 0 (alive), 1 (dead: every socket refused or none exists), 2 (unknown: a socket
# exists but neither answered nor was refused in time) - the caller MUST treat 2 like 0 and
# refuse. This also covers a postmaster mid-startup, bound but not yet accepting: reads as
# unknown too, the safe answer for either. `kill -0 $pid` has no role: the pid is exactly
# what the missing pidfile lost, and "unknown, refuse" is the honest answer here, not a gap.
alive() {
  inconclusive=
  for sock in "$pg"/.s.PGSQL.*; do
    [ -S "$sock" ] || continue
    port_candidate="${sock##*.s.PGSQL.}"
    case "$port_candidate" in
      ""|*[!0-9]*) continue ;;
    esac
    if output="$(PGCONNECT_TIMEOUT=5 psql -h "$pg" -p "$port_candidate" -U postgres -d postgres \
      -tAc "select 1" 2>&1)"; then
      return 0
    fi
    case "$output" in
      *"Connection refused"*) ;;
      *) inconclusive=1 ;;
    esac
  done
  [ -z "$inconclusive" ] || return 2
  return 1
}

write_server_config() {
  cat > "$pg/postgresql.conf" <<EOC
listen_addresses = '127.0.0.1'
unix_socket_directories = '$pg'
port = $port
ssl = on
ssl_cert_file = '$tls_cert'
ssl_key_file = '$tls_key'
ssl_ca_file = '$tls_ca'
fsync = off
synchronous_commit = off
EOC
  cat > "$pg/pg_hba.conf" <<EOC
local all all trust
hostssl $db $mtls_user 127.0.0.1/32 cert
hostssl all all 127.0.0.1/32 scram-sha-256
host all all 127.0.0.1/32 reject
EOC
}

start_postmaster() {
  for _attempt in $(seq 1 10); do
    port="$(allocate_port)"
    write_server_config
    if pg_ctl -D "$pg" -o "-p $port" -l "$pg/server.log" start; then
      return 0
    fi
    echo "postgres tier: the allocated port was lost before Postgres bound it; retrying" >&2
  done
  echo "postgres tier: Postgres could not bind an allocated loopback port after 10 attempts" >&2
  return 1
}

start() {
  mkdir -p "$root/.sutura-dev"
  # WRECKAGE IS DECIDED BY WHETHER ANYONE IS RUNNING ON IT, NOT BY WHAT IS IN IT - and it is
  # the ephemerality below that makes that decidable. `stop` removes this directory, so a
  # directory that exists here with NO postmaster on it is wreckage from a run that never
  # reached its teardown: a kill, an OOM, a reboot. A live one is a tier somebody else brought
  # up, and reusing it is what keeps `start` idempotent.
  #
  # THE STRUCTURAL TEST THIS REPLACES WAS KEYED ON A FALSE PREMISE, and the premise is the
  # interesting part. `PG_VERSION` is written EARLY, not late - `initdb.c` says "Top level
  # PG_VERSION is checked by bootstrapper, so make it first" - so "non-empty and no
  # PG_VERSION" is not the signature of an interrupted run. Measured here, killing `initdb`:
  #
  #   at  20ms -> 0 entries                                      (nothing to clear)
  #   at  50ms -> 19 entries, PG_VERSION, no global/pg_control
  #   at 100ms+ -> 23 entries, PG_VERSION AND global/pg_control
  #   complete -> 22 entries
  #
  # Past ~100ms the wreck carries MORE entries than a finished cluster and every file a
  # structural check could ask for, while `pg_ctl start` still fails with `FATAL: database
  # "postgres" does not exist`. So no test of the contents can separate the two, and the one
  # that was here covered only a window of a few milliseconds.
  if [ -d "$pg" ] && [ -n "$(ls -A "$pg" 2>/dev/null)" ] && ! pg_ctl -D "$pg" status >/dev/null 2>&1; then
    echo "postgres tier: clearing a data directory left by a run that never reached its teardown" >&2
    if ! rm -rf "${pg:?the tier data directory is unset}"; then
      echo "postgres tier: could not clear $pg, so this worktree's tier cannot start." >&2
      echo "               Remove it by hand; nothing in it is a cluster this tier can open." >&2
      exit 1
    fi
  fi
  mkdir -p "$pg"
  if [ ! -f "$pg/PG_VERSION" ]; then
  initdb -D "$pg" -U postgres -E UTF8 --locale=C
  fi
  # One listener, two dials: the unix socket under the short directory AND loopback TCP for
  # the TLS served cells. Postgres cannot do TLS over a socket (`hostssl`/`sslmode` are
  # ignored for `local`), so the TLS cells dial `127.0.0.1` where `hostssl` applies. The
  # socket keeps `trust`; one loopback role uses password auth over verified TLS and the
  # second is accepted only with the client certificate this tier issued.
  #
  # Idempotent over a running postmaster: read the port it bound and leave its config and PKI
  # alone. A running server missing that material came from another contract and cannot be
  # silently advertised as this one.
  if pg_ctl -D "$pg" status >/dev/null 2>&1; then
    port="$(running_port)" || {
      echo "postgres tier: the running postmaster did not record a usable port" >&2
      exit 1
    }
    for material in "$tls_ca" "$tls_key" "$tls_cert" "$tls_client_key" "$tls_client_cert"; do
      if [ ! -s "$material" ]; then
        echo "postgres tier: a running server is missing TLS material at $material." >&2
        echo "               Stop it and start this tier again; it cannot satisfy the current contract." >&2
        exit 1
      fi
    done
  else
    generate_tls
    start_postmaster
  fi
  # A password for THIS worktree, once. 24 bytes of `/dev/urandom` as hex, so the value is
  # `[0-9a-f]` only - which is why interpolating it into the SQL below cannot inject: there
  # is no apostrophe in the alphabet. `umask` in a subshell, so the file is 0600 and the
  # mask does not leak into the rest of `start`.
  if [ ! -s "$cred" ]; then
    ( umask 077; od -An -v -tx1 -N24 < /dev/urandom | tr -d ' \n' > "$cred" )
  fi
  password="$(cat "$cred")"
  # CREATE or ALTER, rather than create-if-missing: the file is the authority, so a cluster
  # that outlived its credential file - or a wreck cleared out from under one - is brought
  # back into agreement instead of authenticating against a password nothing published.
  if ! psql -h "$pg" -p "$port" -U postgres -d postgres -tAc \
    "SELECT 1 FROM pg_roles WHERE rolname='$user'" | grep -q 1; then
    psql -h "$pg" -p "$port" -U postgres -d postgres \
      -v ON_ERROR_STOP=1 -c "CREATE ROLE \"$user\" LOGIN PASSWORD '$password'"
  else
    psql -h "$pg" -p "$port" -U postgres -d postgres \
      -v ON_ERROR_STOP=1 -c "ALTER ROLE \"$user\" WITH PASSWORD '$password'"
  fi
  if ! psql -h "$pg" -p "$port" -U postgres -d postgres -tAc \
    "SELECT 1 FROM pg_roles WHERE rolname='$mtls_user'" | grep -q 1; then
    psql -h "$pg" -p "$port" -U postgres -d postgres \
      -v ON_ERROR_STOP=1 -c "CREATE ROLE \"$mtls_user\" LOGIN"
  fi
  if ! psql -h "$pg" -p "$port" -U postgres -d postgres -tAc \
    "SELECT 1 FROM pg_database WHERE datname='$db'" | grep -q 1; then
    psql -h "$pg" -p "$port" -U postgres -d postgres \
      -v ON_ERROR_STOP=1 \
      -c "CREATE DATABASE \"$db\" OWNER \"$user\" TEMPLATE template0 LOCALE 'C' ENCODING 'UTF8'"
  fi
  # The harness reads `<root>/.sutura-dev/endpoints.json` and treats the host as the socket
  # dir. MERGED rather than written whole: a second nix tier's entry lives in the same file.
  # ONE entry, and its `port` is the number for BOTH dials: a TLS cell reads it here and dials
  # `127.0.0.1`, because the loopback listener shares the unix socket's port and a second entry
  # would be a second claim about one server.
  sutura-tier-endpoint publish "$root" postgres "$pg" "$port"
}

stop() {
  # The `|| true` this replaces was right about one thing and wrong about the distinction: a
  # tier that was never started must not fail a teardown, but *never started* and *would not
  # stop* are not the same state and ignoring the exit code answered both. So the question is
  # asked instead - and a server that is running and did not stop keeps its entry, because
  # withdrawing a claim over a live postmaster is exactly how a tier came to be `up` to a
  # wrapper and absent to every test.
  #
  # `pg_ctl status` FIRST, `alive` only if that already says "not running" -
  # github.com/telekom/sutura#807. The old guard trusted `pg_ctl status` alone, and that
  # answer is exactly the one that lies once `postmaster.pid` is gone: rc=3, "no server
  # running", over a cluster that keeps answering `select 1`. `alive` is the fallback that
  # asks the socket instead, so a live cluster with no pidfile is still seen as live - and
  # this `stop` refuses rather than deleting it, the same rule `nix/keycloak-tier.nix`'s
  # `stop` holds over a JVM whose own pidfile went missing (#803). Ordering `pg_ctl status`
  # first, rather than replacing it, is what keeps a SIGSTOPped postmaster - pidfile present,
  # unresponsive on its socket - reaching `pg_ctl stop`'s own timeout below instead of
  # reading as absent and falling straight through to the delete.
  #
  # `alive`'s three outcomes collapse to two branches: unknown (2) gets the SAME refusal as
  # alive (0), since a timeout is not proof of death. Only a plain 1 reaches the delete.
  rc=dead
  if [ -d "$pg" ]; then
    if pg_ctl -D "$pg" status >/dev/null 2>&1; then
      rc=alive
    elif alive; then rc=alive
    elif [ $? -eq 2 ]; then rc=unknown
    fi
  fi
  if [ "$rc" = unknown ]; then
    echo "postgres tier: a server at $pg neither answered a query nor was refused within" >&2
    echo "               the connect timeout, so alive or dead cannot be proven - a frozen" >&2
    echo "               postmaster looks identical to a dead one, and postmaster.pid is" >&2
    echo "               gone so there is no pid to check by hand. Its endpoint entry, data" >&2
    echo "               directory, TLS material and credential STAY. Find the postmaster" >&2
    echo "               by hand (\`ps\`), resume and stop it or confirm it is gone, retry." >&2
    exit 1
  fi
  if [ "$rc" = alive ]; then
    if [ ! -f "$pg/postmaster.pid" ]; then
      echo "postgres tier: a server at $pg answers queries but postmaster.pid is gone, so" >&2
      echo "               pg_ctl has no pid to signal. Its endpoint entry, data directory," >&2
      echo "               TLS material and credential STAY. Find the postmaster by hand" >&2
      echo "               and stop it, or restore postmaster.pid, then retry." >&2
      exit 1
    fi
    if ! pg_ctl -D "$pg" stop -m fast; then
      echo "postgres tier: the server did not stop, so its endpoint entry STAYS." >&2
      echo "               It is still running and still discoverable, which is the honest" >&2
      echo "               state - a withdrawn claim over a live server is what made a tier" >&2
      echo "               'already up' to the wrapper and absent to the suite. Retry the" >&2
      echo "               teardown (\`just postgres-tier stop\` in a dev shell)." >&2
      exit 1
    fi
  fi
  # The endpoint entry is a claim that a server is there. Withdraw it, or discovery keeps
  # believing it and the cells fail on a dead socket instead of skipping. Withdrawing the
  # LAST service removes the file, which is what a postgres-only worktree saw when this
  # was a bare `rm -f`; a tier that was never started has nothing to withdraw and that is not
  # a failure.
  sutura-tier-endpoint withdraw "$root" postgres
  # AND THE DATA DIRECTORY GOES WITH IT. This tier is provisioned by nix on demand; nothing it
  # writes is meant to outlive a teardown, and the sandbox arm already behaves that way for
  # free because `$NIX_BUILD_TOP` is fresh every build. The dev-shell arm only looked
  # different because its path is chosen to be SHORT - a unix socket caps near 100 bytes on
  # macOS - and keyed per worktree so two trees cannot clobber each other. Neither reason
  # argues for surviving `stop`, and nothing here ever removed it: measured on one machine,
  # nine directories from four separate days, 40 MB each.
  #
  # The cost of not keeping it is one `initdb`: 0.63-0.73s measured directly, and ~0.70s as the
  # difference between a cold and a warm `start` - which is what a repeated `just test` pays
  # now, against a directory that accumulated forever at 47 MB a time and a wreck that wedged
  # the tier until somebody deleted a path nothing told them about.
  #
  # AFTER the withdraw and after the stop-failure exit above: a server that would not stop
  # keeps both its entry and its data, because removing a live postmaster's directory is a
  # worse failure than the one being fixed.
  rm -rf "${pg:?the tier data directory is unset}"
  # AND THE TLS PRIVATE KEY GOES WITH THE CLUSTER it signed. `ca.crt` is public, but the key
  # next to it is exactly the secret the tier's per-cluster generation exists to keep out of
  # anything tracked by git - leaving it behind would hand a fresh cluster a private key that
  # matches an anchor an earlier run published.
  rm -rf "${pg:?the tier data directory is unset}.ssl"
  # And the credential goes with the cluster it belongs to. Leaving it behind is how a fresh
  # cluster comes up carrying a password an earlier run published, which is the same class of
  # stale claim `endpoints.json` above is about.
  rm -f "${cred:?the tier credential file is unset}"
}

# WHAT THE CLIENT NEEDS AND CANNOT GUESS, as three `export` lines for a caller to `eval`.
#
# This is the supply half of `github.com/telekom/sutura#455`. The adapter's
# `FixtureCredential::from_env` refuses, naming the variable, when any of these is unset -
# there is no fallback left - so the thing that PROVISIONED the server is the thing that says
# how to log in, exactly as `nix/with-tier.sh` already treats `SUTURA_DEV_REQUIRE_TIER`.
#
# `SUTURA_POSTGRES_TIER_*` and not `SUTURA_DEV_*`: those two names are the compose fixture
# credential's, `compose.services.yaml` gives them defaults of its own, and NO postgres service
# exists in that file at all. The shared name was the only coupling, and it made an unrelated
# tier's argument look like it covered this one.
#
# An absent file is a refusal and not an empty answer: a caller that eval'd nothing would run
# the suite against a client that then refuses, which is a worse diagnostic than this one.
credentials() {
  if [ ! -s "$cred" ]; then
    echo "postgres tier: no credential is published for this worktree, so the suite's" >&2
    echo "               postgres cells would refuse rather than connect." >&2
    echo "               \`just postgres-tier start\` publishes one (\`just test\` does it" >&2
    echo "               for you). A server started by an older tier has none: stop it and" >&2
    echo "               start it again." >&2
    exit 1
  fi
  printf 'export SUTURA_POSTGRES_TIER_USER=%s\n' "$user"
  printf 'export SUTURA_POSTGRES_TIER_PASSWORD=%s\n' "$(cat "$cred")"
  printf 'export SUTURA_POSTGRES_TIER_DB=%s\n' "$db"
  # The loopback TLS anchor - the CA a TLS served cell verifies the server's chain against.
  # `eval`d like the rest, so unless a caller needs the "no TLS material" refusal (the config
  # cells that assert it), the served TLS cells read it from here rather than deriving a path.
  printf 'export SUTURA_POSTGRES_TIER_CA=%s\n' "$tls_ca"
  printf 'export SUTURA_POSTGRES_TIER_CLIENT_CERT=%s\n' "$tls_client_cert"
  printf 'export SUTURA_POSTGRES_TIER_CLIENT_KEY=%s\n' "$tls_client_key"
}

# Is a server up, and up in the way THE SUITE will see it? Nothing is changed, and the answer
# is the exit code - so a wrapper can stop only what it started rather than trampling a tier
# somebody else brought up.
#
# THREE answers, because a wrapper needs two bits out of one fact and asking two commands for
# them is how they came apart:
#
#   0  a postmaster is running AND this worktree's `endpoints.json` publishes it at that
#      socket directory - a fail-closed cell will find it
#   3  a postmaster is running and nothing publishes it - `start` heals that, and this server
#      is NOT the caller's to tear down
#   1  nothing is running here
#
# A boolean caller (`if ... status`) reads 3 as down, which is the honest answer to the
# question it asked: there is nothing the suite can reach. `start` deliberately does not go
# through this - its own guard is the postmaster alone, which is what keeps it idempotent over
# a server whose entry has gone.
status() {
  pg_ctl -D "$pg" status >/dev/null 2>&1 || return 1
  port="$(running_port)" || return 3
  sutura-tier-endpoint published "$root" postgres "$pg" "$port" || return 3
}

case "${1:-}" in
  start) start ;;
  stop) stop ;;
  status) status ;;
  credentials) credentials ;;
  *) echo "usage: $0 start|stop|status|credentials" >&2; exit 2 ;;
esac
