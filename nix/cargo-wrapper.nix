# The pinned cargo wrapper used by the lockfile workflow.
{ pkgs, rustToolchain }:

pkgs.writeShellApplication {
  name = "sutura-cargo";
  text = ''
    export PATH="${rustToolchain}/bin:$PATH"
    exec cargo "$@"
  '';
}
