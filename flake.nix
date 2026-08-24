# Two outputs that matter: the devenv shell, and the OCI image.
#
# The image is built BY NIX, not by a Dockerfile. Two build definitions for one artifact
# is exactly the drift this removes — the dev shell and the release image read the same
# rust-toolchain.toml, so they cannot disagree about the compiler.
{
  description = "sutura — an identity-aware semantic data runtime for AI agents";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    crane.url = "github:ipetkov/crane";
  };

  outputs = { self, nixpkgs, flake-utils, crane, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
        craneLib = crane.mkLib pkgs;

        sutura = craneLib.buildPackage {
          src = craneLib.cleanCargoSource ./.;
          strictDeps = true;
          # The shipped profile is deliberately cheap to build; `release-performance`
          # exists for when throughput has actually been measured.
          CARGO_PROFILE = "release";
          doCheck = true;
        };
      in
      {
        packages = {
          default = sutura;
          sutura = sutura;

          # `nix build .#oci` -> a loadable image tarball.
          #
          # streamLayeredImage, not buildLayeredImage: it avoids materialising a
          # multi-hundred-MB tarball in the store just to push it.
          #
          # Contents are the binary, CA certificates and tzdata. NO shell and NO package
          # manager: the attack surface of a governed service should be one executable.
          oci = pkgs.dockerTools.streamLayeredImage {
            name = "sutura";
            tag = "latest";
            # Pinned, not `now`: an image whose digest changes on every build cannot be
            # the thing a deployment pins, and "which build is running" stops being
            # answerable.
            created = "1970-01-01T00:00:01Z";
            contents = [ sutura pkgs.cacert pkgs.tzdata ];
            config = {
              Entrypoint = [ "/bin/sutura" ];
              Cmd = [ "--version" ];
              Env = [ "SSL_CERT_FILE=/etc/ssl/certs/ca-bundle.crt" ];
            };
          };
        };

        checks.default = sutura;
        formatter = pkgs.nixpkgs-fmt;
      });
}
