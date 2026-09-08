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
#
# **It is also the READER, and that half arrived late enough to cost a suite run.** A tier used to
# answer *am I up* from its own process - `pg_ctl status` - while the harness answered it from this
# file, which is two statements about one fact. They came apart in the direction that blocks work
# (`github.com/telekom/sutura#298`): a postmaster outlived a teardown that had already withdrawn its
# entry, so the wrapper skipped `start` and every fail-closed cell found nothing. `published` is
# here so a tier's `status` can be DERIVED from the document the harness reads rather than be a
# second opinion about it, and it takes `publish`'s own argument list on purpose - the question a
# caller has is *does the claim I made still stand*.
{ pkgs }:
{
  script = pkgs.writeShellApplication {
    name = "sutura-tier-endpoint";
    runtimeInputs = [ pkgs.jq ];
    text = ''
      set -o errexit -o nounset

      usage() {
        echo "usage: $0 publish <worktree> <service> <host> <port>" >&2
        echo "       $0 published <worktree> <service> <host> <port>" >&2
        echo "       $0 withdraw <worktree> <service>" >&2
        exit 2
      }

      # THE MARKER IS ON THE ENTRY, not on the document - `github.com/telekom/sutura#317`. This
      # used to set a document-level `.provisioner = "nix"`, which is the last writer's opinion
      # about every other writer's service: after an `xtask dev-up` merged its own entries beside
      # these, one field had to answer for both and answered wrong for one of them. So each entry
      # carries what provisioned it, `dev/src/discovery.rs` reads it per service, and neither
      # writer touches a key belonging to the other.
      publish() {
        root="$1"; service="$2"; host="$3"; port="$4"
        state="$root/.sutura-dev"
        file="$state/endpoints.json"
        mkdir -p "$state"
        if [ ! -f "$file" ]; then
          printf '{"project":"sutura","services":{}}\n' > "$file"
        fi
        jq --arg service "$service" --arg host "$host" --argjson port "$port" \
          '.services[$service] = { host: $host, port: $port, provisioner: "nix" }' \
          "$file" > "$file.new"
        mv "$file.new" "$file"
      }

      # Does the document still carry exactly what `publish` was last given for this service? The
      # answer is the exit code, and it is deliberately fail-closed toward *no*: a missing file, a
      # missing entry, a different address and a document that does not parse are one answer, which
      # is that nothing a harness can reach is published here.
      published() {
        root="$1"; service="$2"; host="$3"; port="$4"
        file="$root/.sutura-dev/endpoints.json"
        [ -f "$file" ] || return 1
        # `// empty` rather than a null comparison: `--exit-status` over no output is what makes an
        # absent entry non-zero without a second branch to keep in step with this one.
        jq --exit-status --arg service "$service" --arg host "$host" --argjson port "$port" \
          '.services[$service] // empty | .host == $host and .port == $port' \
          "$file" >/dev/null 2>&1 || return 1
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
        published) [ "$#" -eq 5 ] || usage; published "$2" "$3" "$4" "$5" ;;
        withdraw) [ "$#" -eq 3 ] || usage; withdraw "$2" "$3" ;;
        *) usage ;;
      esac
    '';
  };
}
