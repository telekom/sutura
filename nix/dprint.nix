# dprint, with hash-pinned plugins passed as local store paths.
#
# A WRAPPER rather than a bare `pkgs.dprint`, and hermeticity is the whole reason. dprint's own
# plugin mechanism is a URL in `dprint.json` that it fetches and caches at run time: a network
# fetch inside a gate, and a plugin version nothing in this tree pins. Every plugin here is a
# fixed Nix input instead, so this passes their store paths with `--plugins` and `dprint.json`
# carries the OPTIONS alone. nixpkgs owns packaged versions; release hashes cover upstream lag.
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
    p.g-plane-pretty_yaml
  ]) ++ [
    # Upstream WASM releases keep plugin loading hermetic while nixpkgs catches up.
    (pkgs.fetchurl {
      name = "dprint-plugin-markdown-0.24.0.wasm";
      url = "https://github.com/dprint/dprint-plugin-markdown/releases/download/0.24.0/plugin.wasm";
      hash = "sha256-z35lZ0t+tdkfhRUq6QINyLPnO9eUTvy/sF33EIEhd2Q=";
    })
    (pkgs.fetchurl {
      name = "dprint-plugin-toml-0.8.0.wasm";
      url = "https://github.com/dprint/dprint-plugin-toml/releases/download/0.8.0/plugin.wasm";
      hash = "sha256-actAy6XopTVgzOp/3O0HOlXQTrBUkvDZuHf5Frl2lF0=";
    })
  ];
in
pkgs.writeShellScriptBin "dprint" ''
  exec ${pkgs.dprint}/bin/dprint "$@" --plugins ${pkgs.lib.escapeShellArgs plugins}
''
