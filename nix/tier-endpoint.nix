# ONE writer for `.sutura-dev/endpoints.json`, shared by every nix-native tier.
#
# Both nix tiers publish into one file, and before this existed each of them wrote the whole
# document: `sutura-postgres-tier start` printed a document naming `postgres` and nothing else. With
# one tier that is indistinguishable from correct. With two it is a defect that cannot be missed -
# whichever tier started second erases the first, and `sutura_dev::provisioned::here` then reports
# the erased service as *not provisioned* while its server is alive and listening.
#
# It is the same defect the compose file's own header records, measured on 2026-09-03 and in both
# directions: `xtask dev-up` left `provisioner: docker` with `clickhouse` while the nix Postgres
# tier's postmaster was still running and no longer named, and `sutura-postgres-tier start` did the
# same to the docker entries. So this merges by SERVICE KEY and touches nothing else in the file.
#
# WHAT IT DELIBERATELY DOES NOT TOUCH, and the reason is that a reader depends on it: the root
# `project` and `provisioner` fields are preserved when the file already exists, and written only
# where this creates it. `provisioner` means *what created this file* and no more than that -
# `dev/tests/provisioned.rs` reads it to decide whether the DOCKER wiring cells apply, and rewriting
# it to `nix` on a file docker wrote is how those cells would silently stop running. The honest
# reading of that field is now narrow rather than wrong, and no per-service provisioner is written
# beside it because nothing would read one.
#
# `jq` rather than a `printf`: a merge has to read what is already there, and splicing shell strings
# into JSON is how a discovery file becomes unparseable. The write goes to a temporary file in the
# same directory and is then renamed, so a reader concurrent with another tier's `start` sees one
# document or the other and never half of one.
{ pkgs }:
pkgs.writeShellApplication {
  name = "sutura-tier-endpoint";
  runtimeInputs = [ pkgs.jq ];
  text = ''
    set -o errexit -o nounset

    usage() {
      echo "usage: $0 set <worktree> <service> <host> <port>" >&2
      echo "       $0 unset <worktree> <service>" >&2
      exit 2
    }

    # The file `sutura_dev::discovery` reads. Under the worktree, never shared - a neighbouring
    # worktree has its own.
    state=""
    file=""
    locate() {
      state="$1/.sutura-dev"
      file="$state/endpoints.json"
    }

    set_one() {
      locate "$1"
      service="$2"
      host="$3"
      port="$4"
      mkdir -p "$state"
      # A fresh document says who made it. An existing one keeps whatever it said, because it may be
      # docker's file and the answer to "what created this" is then still docker.
      if [ ! -f "$file" ]; then
        printf '{"project":"sutura","provisioner":"nix","services":{}}\n' > "$file"
      fi
      tmp="$file.$$"
      jq --arg service "$service" --arg host "$host" --argjson port "$port" \
        '.services = ((.services // {}) + { ($service): { "host": $host, "port": $port } })' \
        "$file" > "$tmp"
      mv "$tmp" "$file"
    }

    unset_one() {
      locate "$1"
      service="$2"
      # A tier that was never started has no file to withdraw from, and that is not a failure.
      [ -f "$file" ] || return 0
      tmp="$file.$$"
      jq --arg service "$service" 'del(.services[$service])' "$file" > "$tmp"
      mv "$tmp" "$file"
      # The endpoint file is a CLAIM that something is there. With nothing left in it the claim is
      # empty, and an empty document is worse than no document: `Endpoints::discover` reads it
      # happily and answers `UnknownService`, whose remedy is different advice. So the last service
      # out removes the file, which is the behaviour `nix/postgres-tier.nix` already had and had
      # measured the cost of losing.
      if [ "$(jq -r '.services | length' "$file")" = "0" ]; then
        rm -f "$file"
      fi
    }

    case "''${1:-}" in
      set) [ "$#" -eq 5 ] || usage; set_one "$2" "$3" "$4" "$5" ;;
      unset) [ "$#" -eq 3 ] || usage; unset_one "$2" "$3" ;;
      *) usage ;;
    esac
  '';
}
