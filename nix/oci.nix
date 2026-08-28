# A shipped binary becomes a container image.
#
# ITS OWN FILE for the reason `nix/api-docs.nix` gives: flake.nix sat at exactly the 1000-line
# limit `cargo xtask max-lines` enforces. `ociImages` and `packages.oci` stay in flake.nix, so
# the `packages = ` block a text-scanning gate reads is untouched; what moves is the two
# functions those call.
#
# `version` is a PARAMETER and not read back out of a manifest here: the image label and the
# `pname`/`version` crane builds under have to be the same string, and flake.nix's `commonArgs`
# is where that string is declared once.
{ pkgs, version }:

let
  # The OCI `architecture` field for a target triple. Not cosmetic: an image built from an
  # aarch64 binary that claims `amd64` gets scheduled onto a node that cannot run it, and
  # the failure surfaces as a crash loop rather than as a rejected placement.
  ociArch = target: if pkgs.lib.hasPrefix "aarch64-" target then "arm64" else "amd64";

  # Contents are the binary, CA certificates and tzdata. NO shell and NO package
  # manager: the attack surface of a governed service should be one executable, and it
  # is also the mechanical proof that no interpreter is in the query path.
  #
  # `cacert` and `tzdata` come from the NATIVE package set even in a cross image, and
  # deliberately: both outputs are data only - PEM text and endian-fixed TZif files - so
  # cross-building them would add a toolchain closure per architecture for a byte-for-byte
  # identical result. The binary is the only architecture-dependent thing in here.
  ociFor = { bin, architecture }: pkgs.dockerTools.streamLayeredImage {
    name = "sutura";
    tag = "latest";
    inherit architecture;
    # Pinned, not `now`: an image whose digest changes on every build cannot be the
    # thing a deployment pins.
    created = "1970-01-01T00:00:01Z";
    contents = [ bin pkgs.cacert pkgs.tzdata ];
    config = {
      Entrypoint = [ "/bin/sutura" ];
      Cmd = [ "--version" ];
      Env = [ "SSL_CERT_FILE=/etc/ssl/certs/ca-bundle.crt" ];
      # Non-root by default. The binary needs no privilege, and a cluster policy of
      # `runAsNonRoot` should be satisfied by the image rather than by a deployment
      # someone has to remember to write. 65532 is the conventional `nonroot` uid.
      User = "65532:65532";
      # `docker inspect` should answer "which commit is this" without a lookup table.
      Labels = {
        "org.opencontainers.image.title" = "sutura";
        "org.opencontainers.image.description" = "identity-aware semantic data runtime for AI agents";
        "org.opencontainers.image.licenses" = "Apache-2.0";
        "org.opencontainers.image.source" = "https://github.com/telekom/sutura";
        "org.opencontainers.image.version" = version;
      };
    };
  };
in
{
  inherit ociArch ociFor;
}
