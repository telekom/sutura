# dprint, with its plugins resolved from the SAME nixpkgs this flake locks.
#
# A WRAPPER rather than a bare `pkgs.dprint`, and hermeticity is the whole reason. dprint's own
# plugin mechanism is a URL in `dprint.json` that it fetches and caches at run time: a network
# fetch inside a gate, and a plugin version nothing in this tree pins. Every plugin here is a
# nixpkgs derivation instead, so this passes their store paths with `--plugins` and `dprint.json`
# carries the OPTIONS alone - the committed config holds no store path, and a plugin version moves
# only when `flake.lock` does.
#
# `--plugins` comes LAST, after `"$@"`, because it takes many values: a caller's own arguments
# placed after it would be read as further plugin paths rather than as files to format.
#
# THREE PLUGINS AND NOT FOUR. `dprint-plugin-ruff` exists and would fold Python in here, but it
# formats and does not lint - `apps.ruff` is the same binary doing both, so the plugin would be a
# second Python formatter to keep in step with the linter's opinion.
{ pkgs }:
let
  plugins = pkgs.dprint-plugins.getPluginList (p: [
    p.dprint-plugin-markdown
    p.dprint-plugin-toml
    p.g-plane-pretty_yaml
  ]);
in
pkgs.writeShellScriptBin "dprint" ''
  exec ${pkgs.dprint}/bin/dprint "$@" --plugins ${pkgs.lib.escapeShellArgs plugins}
''
