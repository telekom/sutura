# Development tools beyond the default versions selected by nixpkgs.
# Like nix/crap.nix, these trust upstream builds and fix their bytes by hash.
# A shallow package-set merge keeps these pins out of nixpkgs' own build dependencies.
{ pkgs }:
let
  system = pkgs.stdenv.hostPlatform.system;
  rustTarget = {
    x86_64-linux = "x86_64-unknown-linux-musl";
    aarch64-linux = "aarch64-unknown-linux-musl";
    x86_64-darwin = "x86_64-apple-darwin";
    aarch64-darwin = "aarch64-apple-darwin";
  }.${system};
  pulumiTarget = {
    x86_64-linux = "linux-x64";
    aarch64-linux = "linux-arm64";
    x86_64-darwin = "darwin-x64";
    aarch64-darwin = "darwin-arm64";
  }.${system};
  release = { pname, version, url, hashes, versionArgs ? "--version", nativeInstallCheckInputs ? [ ], installPhase ? ''
    install -Dm755 ${pname} "$out/bin/${pname}"
  '', installCheckPhase ? ''
    "$out/bin/${pname}" ${versionArgs} | grep -Fw "${version}"
  '' }:
    pkgs.stdenvNoCC.mkDerivation {
      inherit pname version installPhase installCheckPhase nativeInstallCheckInputs;
      src = pkgs.fetchurl { inherit url; hash = hashes.${system}; };
      nativeBuildInputs = pkgs.lib.optionals pkgs.stdenv.hostPlatform.isLinux [ pkgs.autoPatchelfHook ];
      buildInputs = pkgs.lib.optionals pkgs.stdenv.hostPlatform.isLinux [ pkgs.stdenv.cc.cc.lib ];
      doInstallCheck = true;
      meta.mainProgram = pname;
    };
in
{
  # Keep the compiler and linker on the same packaged LLVM release.
  clang = pkgs.llvmPackages_latest.clang;
  lld = pkgs.llvmPackages_latest.lld;

  cargo-auditable = release rec {
    pname = "cargo-auditable";
    version = "0.7.6";
    url = "https://github.com/rust-secure-code/cargo-auditable/releases/download/v${version}/cargo-auditable-${rustTarget}.tar.xz";
    # This subcommand forwards --version to cargo; exercise its embedded metadata instead.
    nativeInstallCheckInputs = [ pkgs.cargo pkgs.rustc pkgs.stdenv.cc pkgs.rust-audit-info ];
    installCheckPhase = ''
      mkdir -p smoke/src
      cd smoke
      printf '[package]\nname = "audit-smoke"\nversion = "0.0.0"\nedition = "2024"\n' > Cargo.toml
      printf 'fn main() {}\n' > src/main.rs
      export CARGO_HOME="$TMPDIR/cargo-home"
      "$out/bin/cargo-auditable" auditable build --offline
      rust-audit-info target/debug/audit-smoke | grep -F '"name":"audit-smoke"'
    '';

    hashes = {
      x86_64-linux = "sha256-QrZshS+7kHSpyjVieakut1P0jd4WAXuMgvSNzQXWyFY=";
      aarch64-linux = "sha256-VyZfvYfpJ3+9hQx0F30X5ab1HxXigDZD22XBh7DU/to=";
      x86_64-darwin = "sha256-1sJt3t0DThSb8s07shHve/BDoYQPWNZMvY6E+xSQNMs=";
      aarch64-darwin = "sha256-v0L7B3OA9Bpyq54hdlG//LbUbmQYUqea0TWLzDCDI/8=";
    };
  };
  ruff = release rec {
    pname = "ruff";
    version = "0.16.9";
    url = "https://github.com/astral-sh/ruff/releases/download/${version}/ruff-${rustTarget}.tar.gz";
    hashes = {
      x86_64-linux = "sha256-alYe1LyGBHL3gz37DwuChdOWn6YnGIxWeOpKwsRiu+4=";
      aarch64-linux = "sha256-QNn+tyEKbZhLuYtfglr9zzYhbuRkqITibBBPlyJsdxM=";
      x86_64-darwin = "sha256-6Y6iWaAhyH06H4vxhjnS4y3NRcsu8a/LQbE+KV3h4rM=";
      aarch64-darwin = "sha256-M9NTlEmQlM9uuQ9zDcgvEcD6sF0XY3iuT2femF7MQUY=";
    };
  };
  pulumi = release rec {
    pname = "pulumi";
    version = "3.265.0";
    url = "https://github.com/pulumi/pulumi/releases/download/v${version}/pulumi-v${version}-${pulumiTarget}.tar.gz";
    installCheckPhase = ''
      "$out/bin/pulumi" version | grep -Fx "v${version}"
    '';
    installPhase = ''
      mkdir -p "$out/bin"
      cp -R . "$out/bin/"
    '';
    hashes = {
      x86_64-linux = "sha256-p+/2O8U55DGait1zg6AeQnrvdKiZS89eap0Sv51nSXk=";
      aarch64-linux = "sha256-VfKss9s2GOx0BcvfK7IVZv30bCgqku9BnZsdcpioPuQ=";
      x86_64-darwin = "sha256-7yMYCWemKu1P6G/WTgooFNkHoRKVLpOvfjVO1/nKP4o=";
      aarch64-darwin = "sha256-ZZ6CViekXngaKQ31P4AfYKIeDCBvNR0I1ovq27H7qv4=";
    };
  };
  kind = pkgs.kind.overrideAttrs (old: {
    version = "0.33.0";
    src = pkgs.fetchFromGitHub {
      owner = "kubernetes-sigs";
      repo = "kind";
      tag = "v0.33.0";
      hash = "sha256-exqO/KERw/SOv3dywcrm/DSHY37dfTOhQezg6Nroew0=";
    };
    # Keep nixpkgs' NixOS module-path fix; upstream includes its load-balancer fix.
    patches = [ (builtins.head old.patches) ];
  });
}
