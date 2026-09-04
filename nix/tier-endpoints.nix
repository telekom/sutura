# One writer for `.sutura-dev/endpoints.json`, shared by every nix-native tier.
#
# **The defect this removes existed the moment there were two tiers.** `nix/postgres-tier.nix`
# wrote the whole document with one `printf`, which is correct while it is the only nix tier and
# silently wrong afterwards: the second tier to `start` replaces the file, the first service's entry
# disappears, and `sutura_dev::provisioned::here` reports the surviving service as the only one
# provisioned. Nothing would have failed - discovery would have answered a truthful file about half
# a tier, which is the class of bug this repository treats as worse than a crash.
#
# So an entry is MERGED and withdrawn per service, and the file is the only thing either tier
# touches. `dev/src/discovery.rs` remains the reader's one door; this is the nix venue's writer,
# the way `discovery::publish` is docker's.
#
# **`stop` still withdraws the claim, and the last one out removes the file.** That is
# `nix/postgres-tier.nix`'s measured lesson kept intact: the file's EXISTENCE is read as
# availability, so a stale entry makes a fail-closed cell panic on a dead server where the honest
# outcome is a skip. Withdrawing one service leaves the others; withdrawing the last removes the
# file, which is byte-for-byte what a postgres-only worktree saw before this existed.
#
# The write is atomic - a temporary file in the same directory and a `mv` - because a harness can be
# reading while a tier is starting, and half a JSON document is a malformed-file error attributed to
# whatever ran next.
{ pkgs }:
{
  script = pkgs.writeShellApplication {
    name = "sutura-tier-endpoint";
    runtimeInputs = [ pkgs.jq ];
    text = ''
      set -o errexit -o nounset

      usage() {
        echo "usage: $0 publish <worktree> <service> <host> <port>" >&2
        echo "       $0 withdraw <worktree> <service>" >&2
        exit 2
      }

      # `project` and `provisioner` match what `nix/postgres-tier.nix` wrote before this file
      # existed, so `Endpoints::provisioner` still answers `nix` and no reader changes.
      publish() {
        root="$1"; service="$2"; host="$3"; port="$4"
        state="$root/.sutura-dev"
        file="$state/endpoints.json"
        mkdir -p "$state"
        if [ ! -f "$file" ]; then
          printf '{"project":"sutura","provisioner":"nix","services":{}}\n' > "$file"
        fi
        jq --arg service "$service" --arg host "$host" --argjson port "$port" \
          '.provisioner = "nix" | .services[$service] = { host: $host, port: $port }' \
          "$file" > "$file.new"
        mv "$file.new" "$file"
      }

      withdraw() {
        root="$1"; service="$2"
        file="$root/.sutura-dev/endpoints.json"
        # A tier that was never started has no file to withdraw from, and that is not a failure.
        [ -f "$file" ] || return 0
        jq --arg service "$service" 'del(.services[$service])' "$file" > "$file.new"
        mv "$file.new" "$file"
        # The last service out takes the file with it: its existence is what discovery reads as
        # "something is provisioned here".
        if [ "$(jq -r '.services | length' "$file")" = "0" ]; then
          rm -f "$file"
        fi
      }

      case "''${1:-}" in
        publish) [ "$#" -eq 5 ] || usage; publish "$2" "$3" "$4" "$5" ;;
        withdraw) [ "$#" -eq 3 ] || usage; withdraw "$2" "$3" ;;
        *) usage ;;
      esac
    '';
  };
}
