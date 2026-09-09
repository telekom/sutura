# The pinned cargo wrapper used by the lockfile workflow.
{ pkgs, toolchain }:

pkgs.writeShellApplication {
  name = "sutura-cargo";
  text = ''
    export PATH="${toolchain}/bin:$PATH"
    exec cargo "$@"
  '';
}
