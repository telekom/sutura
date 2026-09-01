# WHAT A RELEASE PUBLISHES: the shipped binaries, one derivation shape, and the images over them.
#
# ITS OWN FILE for the reason `nix/oci.nix`, `nix/mimalloc.nix`, `nix/auditable.nix` and
# `nix/api-docs.nix` each give: `flake.nix` is under the same 1000-line cap
# `cargo xtask max-lines` enforces on everything else, and it stood at 986 with this inline -
# fourteen lines of room for a change that adds a second shipped binary. Nothing a text-scanning
# gate reads moves: `apps.<name>`, the `packages = ` block and the `checks = {` block all stay in
# `flake.nix`, and `xtask/src/workflows.rs` still finds them there. What moves is the functions
# those call and the lists they read.
#
# **THE PARAMETER THAT MATTERS IS `binaries`, AND IT IS DECLARED HERE RATHER THAN PASSED IN.**
# Before this file existed there was one shipped executable and `--package sutura-cli` was written
# into two derivations by hand. That is how `github.com/telekom/sutura#111` happened: the HTTP
# surface, leg 1, the rate limiter and the generated interface description shipped in no artefact
# on any platform, because nothing ever decided that they should - the release derivations named
# the binary that existed before `sutura-serve` did, and kept naming it. A list is the fix, not a
# second hand-written `--package`: the packages, the images, the `one-binary` check and the
# release workflow all read it, so a third binary is an entry rather than four edits that have to
# agree.
{ pkgs
, nixpkgs
, system
, crane
, rust-overlay
, rustToolchainFile
, craneLib
, commonArgs
, auditable
, mimallocFor
, optLevelFor
, version
}:

let
  # Targets we CROSS-build. Deliberately excludes the host architecture: on an x86_64 builder
  # `sutura` already IS the x86_64-linux binary, and building a separate "cross" x86_64
  # derivation would compile the whole tree a second time for a byte-identical result.
  # `packages.sutura-x86_64-unknown-linux-gnu` is an alias to the native build instead - see
  # `crossPackages` below.
  crossTargets = [
    "aarch64-unknown-linux-gnu"
    # The statically linked pair, and `x86_64-unknown-linux-musl` counts as a CROSS target
    # even on an x86_64 builder: the CPU is the same but the libc is not, so it is a real
    # second compile rather than the byte-identical one the comment above rules out.
    #
    # Why ship them at all: a musl artifact has no dynamic loader and no libc version
    # floor, so it runs on a host older than the builder and in a `FROM scratch` image.
    # What it costs is the allocator - see `crates/sutura-cli/src/main.rs`, because musl's
    # own is why mimalloc is not optional here.
    "x86_64-unknown-linux-musl"
    "aarch64-unknown-linux-musl"
  ];

  # THE SHIPPED SET. One entry per executable a release publishes, and every consumer reads this
  # list rather than naming a package itself.
  #
  # **FEATURES ARE ABSENT FROM THIS RECORD ON PURPOSE, and that absence IS the decision issue #111
  # asks to be stated rather than discovered.** Both shipped binaries are built with cargo's
  # DEFAULT feature set. For `sutura-serve` that means the published server carries the HTTP
  # surface, leg 1, the rate limiter and the generated interface description, and carries neither
  # `tls` nor `bigquery`:
  #
  #   * The cost is the four cross builds. `--features tls` and `--features bigquery` each pull an
  #     outbound or inbound rustls closure, `ring` included, which compiles C and assembly; two of
  #     the four release triples are musl. A published server would pay for that on every target.
  #   * The failure is loud rather than silent, which is what makes the choice defensible instead
  #     of merely cheap. `security.tls_termination: in-process` on a build without `tls` is a
  #     startup refusal naming the feature, and so is a `kind: bigquery` source on a build without
  #     `bigquery` - `sutura_config` and `sutura-serve` both refuse rather than degrade. An
  #     operator who needs either builds from source and knows it.
  #   * A gateway in front is the deployment shape leg 1 already assumes: `security.inbound`
  #     verifies a caller's token behind a component that terminated TLS.
  #
  # There is deliberately no `features` field to set. A field nothing sets is the shape this
  # repository files under *Built And Not Wired*; adding `--features` here when a binary needs
  # them is one line, in front of a reviewer, next to this paragraph.
  binaries = [
    {
      bin = "sutura";
      package = "sutura-cli";
      # The image key, and the empty string is HISTORICAL rather than tidy: `sutura-oci-<triple>`
      # asset names, `<version>-<triple>` leaf tags and the unsuffixed `:latest` are what every
      # release so far published and what `docs/verifying-a-release.md` documents. Renaming them
      # to make a second binary's names symmetrical would break every consumer's script to buy
      # nothing.
      keyPrefix = "";
      entrypoint = "/bin/sutura";
      # A default that does something harmless and provable. The release workflow smoke-tests it.
      cmd = [ "--version" ];
      description = "identity-aware semantic data runtime for AI agents";
    }
    {
      bin = "sutura-serve";
      package = "sutura-serve";
      keyPrefix = "serve";
      entrypoint = "/bin/sutura-serve";
      # NO default argument, and that is not an omission. This binary takes its whole
      # configuration from the settings tree - defaults, then files, then one variable per key -
      # so there is no flag whose absence needs a stand-in. Running the image with no arguments
      # starts the server, which is what a platform scheduling it will do.
      cmd = [ ];
      description = "identity-aware semantic data runtime for AI agents: the HTTP surface";
    }
  ];

  # The image/asset key for one binary at one target. `sutura` keeps the bare triple; every other
  # binary is prefixed, so `oci-serve-x86_64-unknown-linux-musl` and
  # `sutura-oci-serve-x86_64-unknown-linux-musl.cdx.json` name the same build as
  # `sutura-serve-x86_64-unknown-linux-musl.tar.gz`.
  keyFor = b: target: if b.keyPrefix == "" then target else "${b.keyPrefix}-${target}";

  # The `oci` attribute name for a binary's NATIVE image: `oci` for the CLI, `oci-serve` for the
  # server. Same rule as `keyFor`, applied where there is no target to append.
  ociNameFor = b: if b.keyPrefix == "" then "oci" else "oci-${b.keyPrefix}";

  # A native build of one binary for one profile. For `release` the deps derivation is identical
  # to the flake's `cargoArtifacts`, so Nix dedupes it and the checks' work is reused. A
  # performance build necessarily compiles its own, since the profile is what changed.
  nativeFor = { binary, profile }:
    let
      args = commonArgs // {
        CARGO_PROFILE = profile;
        # The prebuilt archive, so the build script links it instead of compiling the C.
        SUTURA_MIMALLOC_LIB_DIR = "${mimallocFor { targetPkgs = pkgs; optLevel = optLevelFor profile; isMusl = false; }}/lib";
      };
    in
    craneLib.buildPackage (args // {
      cargoArtifacts = craneLib.buildDepsOnly args;
      # NAMED AFTER THE EXECUTABLE, so a build log and a store path say which of the two shipped
      # binaries this is. `commonArgs.pname` is `sutura` for the workspace, and with two shipped
      # binaries that made both derivations `sutura-0.1.0`. On the final attrset and never on
      # `args`: `buildDepsOnly` above must stay byte-identical to what the checks share.
      pname = binary.bin;
      # ONE package. Without this, crane builds the whole workspace and the result held
      # three binaries - `sutura`, `sutura-dev` and `xtask` - which made two stated
      # invariants false: the image is supposed to hold one executable, and xtask's
      # compile-time `CARGO` reference pulled the whole cargo store path into the runtime
      # closure. It also broke reproducibility, because that path differs between builds.
      #
      # On the attrset and not on `args`: `buildDepsOnly` above must stay unscoped, or
      # the shared dependency build stops being shared with the checks.
      cargoExtraArgs = "--package ${binary.package}";
      # Tests run as their own check in `flake.nix`, sharing the same artifacts.
      doCheck = false;
    } // auditable.toolFor args // {
      cargoBuildCommand = auditable.buildCommand profile;
    });

  # One cross-compiled package per binary per target. `cargoExtraArgs` pins the target and the
  # cross linker comes from pkgsCross, so no developer needs a local cross setup.
  crossFor = { binary, target, profile }:
    let
      isMusl = pkgs.lib.hasSuffix "-linux-musl" target;
      crossPkgs = import nixpkgs {
        inherit system;
        overlays = [ (import rust-overlay) ];
        crossSystem = { config = target; };
      };
      crossLib = (crane.mkLib crossPkgs).overrideToolchain
        (p: p.rust-bin.fromRustupToolchainFile rustToolchainFile);
      args = commonArgs // {
        CARGO_BUILD_TARGET = target;
        CARGO_PROFILE = profile;
        # The prebuilt archive for THIS target. See `mimallocFor`.
        SUTURA_MIMALLOC_LIB_DIR = "${mimallocFor { targetPkgs = crossPkgs; optLevel = optLevelFor profile; inherit isMusl; }}/lib";
        # Tests cannot run for a foreign architecture on this host; the native build
        # and CI's gates job cover correctness.
        doCheck = false;
        strictDeps = true;
      } // pkgs.lib.optionalAttrs isMusl {
        # THE C COMPILER. mimalloc is C, so a musl target needs a compiler that knows
        # MUSL's headers rather than the host's - compile against glibc headers and link
        # against musl and mimalloc takes its `__GLIBC__` code paths. Nothing is added to
        # `nativeBuildInputs` for it: `crossPkgs.stdenv` already carries the musl cross
        # cc, and crane's `mkCrossToolchainEnv` exports it to the `cc` crate as
        # `CC_<triple>` / `CXX_<triple>` / `AR_<triple>` plus the `TARGET_*` aliases, and
        # to cargo as `CARGO_TARGET_<TRIPLE>_LINKER`. The static-linking rustflags are in
        # `.cargo/config.toml`, which explains why they cannot live here.
        #
        # `-DMI_LIBC_MUSL=1` is upstream's musl switch, and it has to be a COMPILE DEFINE
        # rather than the environment variable of the same name: that name is a CMake
        # option, and `libmimalloc-sys` builds with the `cc` crate and never reads it.
        # nixpkgs' own mimalloc derivation sets it whenever the host libc is musl. In
        # mimalloc v2 it switches off `MI_USE_BUILTIN_THREAD_POINTER`
        # (`include/mimalloc/prim.h`), so the thread id comes from the TLS slot instead of
        # `__builtin_thread_pointer`. `CFLAGS_<triple>` is the `cc` crate's per-target
        # hook; it is appended to the flags cc already computed, not a replacement for
        # them, and it beats `TARGET_CFLAGS` / `CFLAGS` in cc's lookup order so nothing
        # in the sandbox can shadow it.
        "CFLAGS_${builtins.replaceStrings [ "-" ] [ "_" ] target}" = "-DMI_LIBC_MUSL=1";
      };
    in
    crossLib.buildPackage (args // {
      cargoArtifacts = crossLib.buildDepsOnly args;
      # Same reasoning as `nativeFor`; crane appends the target itself.
      pname = binary.bin;
      # One package, and the target. Same reasoning as `nativeFor`.
      cargoExtraArgs = "--package ${binary.package} --target ${target}";
      # The embedded dependency list, per target. Same reasoning as `nativeFor`, and
      # `cargo-auditable` comes from `pkgs` rather than `crossPkgs` because it is a tool
      # that RUNS during the build - `strictDeps = true` above makes that distinction
      # load-bearing rather than stylistic.
    } // auditable.toolFor args // {
      cargoBuildCommand = auditable.buildCommand profile;
    });

  # Nix system -> Rust target triple. Needed because the alias below must be named
  # after the RUST target CI asks for, not after the Nix system.
  hostRustTarget = {
    "x86_64-linux" = "x86_64-unknown-linux-gnu";
    "aarch64-linux" = "aarch64-unknown-linux-gnu";
    "x86_64-darwin" = "x86_64-apple-darwin";
    "aarch64-darwin" = "aarch64-apple-darwin";
  }.${system} or null;

  # `sutura` and `sutura-<triple>` build the default profile; each has a
  # `-performance` sibling. Two NAMES rather than one name plus a flag, so a published
  # asset cannot be ambiguous about which profile produced it, and so a workflow can
  # select one without the build definition growing a mode.
  variants = [
    { suffix = ""; profile = "release"; }
    { suffix = "-performance"; profile = "release-performance"; }
    # The link-check sibling, for pull requests: a branch needs to know that every target
    # still COMPILES AND LINKS - the allocator C included, per target, which is where
    # cross breakage actually lives - and it does not need that answer at LTO prices.
    #
    # `ci` and not `dev`. `dev` optimises every dependency and keeps full debuginfo,
    # which is the right bargain in an incremental shell and the wrong one here: the
    # sandbox starts cold and nothing executes the result, so that was optimisation and
    # debuginfo bought and never used, cached at four targets' worth of size. See the
    # profile in Cargo.toml.
    #
    # Not a shipped artifact and never published. `releaseTargets`, `imageTargets` and the
    # `one-binary` check all key off the unsuffixed name, so nothing here can reach a
    # release asset by accident - `nix eval` shows no `oci-*-ci` attribute exists.
    { suffix = "-ci"; profile = "ci"; }
  ];

  # Every cross-built package, over three dimensions now rather than two: binary, target,
  # profile variant. `sutura-<triple><suffix>` and `sutura-serve-<triple><suffix>`, and the two
  # sets cannot collide because no target triple begins with a binary's name.
  crossPackages = builtins.listToAttrs (builtins.concatMap
    (b: builtins.concatMap
      (v:
        (map
          (t: {
            name = "${b.bin}-${t}${v.suffix}";
            value = crossFor { binary = b; target = t; profile = v.profile; };
          })
          # Never cross-build the host triple: it would compile the whole tree a second
          # time for a byte-identical result.
          (builtins.filter (t: t != hostRustTarget) crossTargets))
        ++ (if hostRustTarget == null then [ ]
        else [{
          name = "${b.bin}-${hostRustTarget}${v.suffix}";
          value = nativeFor { binary = b; profile = v.profile; };
        }]))
      variants)
    binaries);

  # The unsuffixed native build of each binary, plus its performance sibling: `sutura`,
  # `sutura-performance`, `sutura-serve`, `sutura-serve-performance`.
  nativeBinaries = builtins.listToAttrs (builtins.concatMap
    (b: [
      { name = b.bin; value = nativeFor { binary = b; profile = "release"; }; }
      {
        name = "${b.bin}-performance";
        value = nativeFor { binary = b; profile = "release-performance"; };
      }
    ])
    binaries);

  # Every target that becomes a published artifact: the cross list plus the host triple,
  # which is built natively rather than cross-built. One list, so the packages, the
  # images and the one-binary check cannot disagree about what "shipped" means.
  releaseTargets = pkgs.lib.unique (crossTargets
    ++ (if hostRustTarget == null then [ ] else [ hostRustTarget ]));

  # The subset that can become a container image. A darwin host triple lands in
  # `releaseTargets` and must not.
  imageTargets = builtins.filter (t: pkgs.lib.hasInfix "-linux-" t) releaseTargets;

  # A shipped binary becomes a container image.
  #
  # **ONE IMAGE PER BINARY, not one image with two entrypoints**, and the deciding argument is a
  # check rather than a preference: `checks.one-binary` reads each shipped package's `bin/` and
  # its runtime closure, and that assertion only means something while a shipped package holds
  # exactly one executable. An image carrying both would have kept `:latest` unmoved and cost
  # that check its subject.
  inherit (import ./oci.nix { inherit pkgs version; }) ociArch ociFor;

  # One image per shipped binary per shipped target, named after the RUST triple like the
  # binaries are, so a published image and a published tarball can be traced back to the same
  # build. `oci-<triple>` for the CLI, `oci-serve-<triple>` for the server.
  ociImages = builtins.listToAttrs (builtins.concatMap
    (b: map
      (t: {
        name = "oci-${keyFor b t}";
        value = ociFor {
          package = crossPackages."${b.bin}-${t}";
          architecture = ociArch t;
          inherit (b) bin entrypoint cmd description;
        };
      })
      imageTargets)
    binaries);

  # The NATIVE image per binary, and its performance sibling: `nix build .#oci` and
  # `.#oci-serve` are what a developer and the release workflow reach for, and on an x86_64
  # builder each is the same derivation as its `oci-<host triple>` entry.
  #
  # streamLayeredImage, not buildLayeredImage: it avoids materialising a multi-hundred-MB
  # tarball in the store just to push it. See `ociFor`.
  nativeImages = builtins.listToAttrs (builtins.concatMap
    (b:
      let
        architecture = if pkgs.stdenv.hostPlatform.isAarch64 then "arm64" else "amd64";
        imageOf = drv: ociFor {
          package = drv;
          inherit architecture;
          inherit (b) bin entrypoint cmd description;
        };
      in
      [
        { name = ociNameFor b; value = imageOf nativeBinaries.${b.bin}; }
        {
          name = "${ociNameFor b}-performance";
          value = imageOf nativeBinaries."${b.bin}-performance";
        }
      ])
    binaries);
in
{
  inherit
    binaries
    crossPackages
    crossTargets
    imageTargets
    keyFor
    nativeBinaries
    nativeImages
    ociImages
    releaseTargets
    ;
}
