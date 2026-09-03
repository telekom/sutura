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
  # asks to be stated rather than discovered - and, since issue #121, the decision for BOTH binaries
  # rather than for the server alone.** Both shipped binaries are built with cargo's DEFAULT feature
  # set, and `sutura-cli` now has a `bigquery` feature of its own to leave off. So no published
  # artefact of either binary can open a dataset: an operator who wants one builds from source with
  # `--features bigquery`, and `sutura doctor` prints which of the two they are holding.
  #
  # **That is issue #121's step 3, decided as its own recommendation had it:** the CLI ships without
  # the feature, and a deployment that needs a dataset runs a build that carries it. The alternative
  # priced there - a second CLI asset with the feature on, for the two gnu triples only - is two more
  # assets to sign, attest and SBOM plus a musl answer stated rather than discovered, and nothing
  # asks for it yet. When a tutorial chapter does, it is a `features` field here and a paragraph
  # beside this one.
  #
  # For `sutura-serve` the default set means the published server carries the HTTP
  # surface, leg 1, the rate limiter and the generated interface description, and carries neither
  # `tls` nor `bigquery`:
  #
  #   * The cost is the four cross builds. `--features tls` and `--features bigquery` each pull an
  #     outbound or inbound rustls closure, `ring` included, which compiles C and assembly; two of
  #     the four release triples are musl. A published server would pay for that on every target,
  #     and so would a published CLI - measured on 2026-09-02 by compiling
  #     `sutura-cli --all-targets` both ways: the default set touches neither `ring` nor `ureq`, and
  #     `--features bigquery` compiles `ring` from C and assembly.
  #   * **THAT FIRST BULLET IS THE ONE THE MEASUREMENT BELOW FALSIFIED, and it is left standing
  #     rather than rewritten because the correction is the useful part.** The cost is NOT the four
  #     cross builds. `craneLib.buildDepsOnly` is called on `args` - deliberately unscoped, so the
  #     checks share one dependency derivation - and `cargoExtraArgs` is set on the final attrset
  #     instead, which means the deps derivation resolves the WHOLE workspace at cargo's default
  #     set. `sutura-exec-bigquery` is a workspace member that takes `ureq` non-optionally, so
  #     `ring`, `rustls`, `rustls-webpki`, `rustls-pki-types`, `webpki-roots`, `ureq` and
  #     `ureq-proto` are compiled inside `sutura-deps-<triple>` on all four triples WITH THE
  #     FEATURE OFF. The `bigquery` feature gates the edge from `sutura-cli` to that crate, not the
  #     closure's arrival. Verified in the four `cross` logs of the run cited below. So the musl C
  #     and assembly this record priced is paid on every pull request either way, and default-off
  #     buys nothing in build time - what it buys is the second bullet and what the artefact links.
  #   * The failure is loud rather than silent, which is what makes the choice defensible instead
  #     of merely cheap. `security.tls_termination: in-process` on a build without `tls` is a
  #     startup refusal naming the feature, and so is a `kind: bigquery` source on a build without
  #     `bigquery` - `sutura_config`, `sutura-serve` and `sutura-cli` all refuse rather than
  #     degrade, each naming the feature that would link it. An operator who needs either builds
  #     from source and knows it.
  #   * A gateway in front is the deployment shape leg 1 already assumes: `security.inbound`
  #     verifies a caller's token behind a component that terminated TLS.
  #
  # There is deliberately no `features` field to set. A field nothing sets is the shape this
  # repository files under *Built And Not Wired*; adding `--features` here when a binary needs
  # them is one line, in front of a reviewer, next to this paragraph.
  #
  # **`probeFeatures` IS NOT THAT FIELD, and the difference is what publishes.** Nothing in
  # `probeFeatures` reaches an artefact: it names the features a SOURCE build may turn on, and
  # `featurePackages` below builds each one at the `ci` profile for every release triple so the
  # four `cross` jobs can answer whether the feature-off decision above was necessary. That is the
  # measurement `github.com/telekom/sutura#121` step 2 owes, and until it existed the only
  # evidence was a native `cargo check` - which stops at metadata and so says nothing about the
  # link that a musl target is the whole risk of. An entry here is a build, not a promise.
  #
  # **AND HERE IS THE NUMBER, taken 2026-09-03 from the four `cross` jobs of run 33781193001** -
  # the first time `--features bigquery` was LINKED for any triple this project publishes. Each
  # pair is the `sutura` crate derivation alone, deps already in the store, off and then on:
  #
  #   | triple                     | OFF   | ON    | delta        |
  #   | -------------------------- | ----- | ----- | ------------ |
  #   | x86_64-unknown-linux-gnu   | 70.5s | 71.0s | +0.5s  +0.7% |
  #   | aarch64-unknown-linux-gnu  | 69.2s | 69.1s | -0.1s  -0.2% |
  #   | x86_64-unknown-linux-musl  | 83.0s | 84.2s | +1.2s  +1.4% |
  #   | aarch64-unknown-linux-musl | 77.3s | 78.6s | +1.3s  +1.7% |
  #
  # All four linked, both musl triples included, with `file` reporting the right architecture on
  # each - static-pie or statically linked on the musl pair, dynamically linked on the gnu pair, as
  # the shipped builds are. The step's own printed wall clock is 73 / 72 / 87 / 82s in that order,
  # which is the table plus one flake evaluation.
  #
  # **SO: DEFAULT-OFF WAS NOT NECESSARY FOR BUILD COST, and this is where that is written down
  # rather than in a commit message.** A delta under two percent is not what a feature gate is for,
  # and the bullet above explains why it could not have been - the closure is in the deps
  # derivation with the feature off. What default-off IS necessary for is the artefact: no
  # published binary of either shipped executable links an outbound TLS stack, which
  # `checks.shipped-features` asserts out of the binary's own embedded dependency list. Keep the
  # decision, drop the build-cost reason for it. **What this measurement does NOT cover:** the
  # probe links and never runs; it says nothing about the artefact's SIZE, which nothing prints;
  # and `sutura-serve`'s `tls` and `bigquery` remain unprobed, so the +1.7% ceiling is the CLI's
  # and is not evidence about the server.
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
      # `docs/getting-started.md` tells a reader to run `cargo build -p sutura-cli
      # --features bigquery`, and before this entry nothing anywhere proved that configuration
      # LINKS on a triple this project publishes. `ureq`, rustls and `ring` are what it adds, and
      # `ring` compiles C and assembly, so the two musl triples are the answer worth having.
      probeFeatures = [ "bigquery" ];
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
      # EMPTY, and deliberately: this binary's `tls` and `bigquery` features are the same shape
      # and the same risk, and probing both would triple a job that already compiles the whole
      # dependency closure per target. The CLI is the one issue #121 owes a measurement for; what
      # this list says is that adding serve's is an entry rather than a design.
      probeFeatures = [ ];
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

  # The `--features` cargo takes, or nothing at all. Empty produces an empty string rather than a
  # bare `--features`, so a build with no features is byte-identical to one that never asked.
  featureArg = features:
    pkgs.lib.optionalString (features != [ ]) " --features ${builtins.concatStringsSep "," features}";

  # A native build of one binary for one profile. For `release` the deps derivation is identical
  # to the flake's `cargoArtifacts`, so Nix dedupes it and the checks' work is reused. A
  # performance build necessarily compiles its own, since the profile is what changed.
  nativeFor = { binary, profile, features ? [ ] }:
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
      cargoExtraArgs = "--package ${binary.package}${featureArg features}";
      # Tests run as their own check in `flake.nix`, sharing the same artifacts.
      doCheck = false;
    } // auditable.toolFor args // {
      cargoBuildCommand = auditable.buildCommand profile;
    });

  # One cross-compiled package per binary per target. `cargoExtraArgs` pins the target and the
  # cross linker comes from pkgsCross, so no developer needs a local cross setup.
  crossFor = { binary, target, profile, features ? [ ] }:
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
      cargoExtraArgs = "--package ${binary.package} --target ${target}${featureArg features}";
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

  # THE FEATURE-ON LINK CHECK, one package per binary per probe feature per release triple:
  # `sutura-bigquery-<triple>-ci`. The binary's name, then the feature, then the triple, then
  # `-ci` - so the last `-`-segment of the prefix IS the feature, and a shell can recover the
  # executable's name from the prefix by dropping it. That is what `ci.yml`'s loop does, and it
  # is why the order is not `<bin>-<triple>-<feature>`.
  #
  # **`ci` PROFILE AND NOTHING ELSE, which is what keeps this a link check rather than a second
  # release matrix.** Nothing executes the result and nothing publishes it; what is being asked is
  # whether the feature's C and assembly cross-compile and LINK for a musl triple, which is the one
  # question a native `cargo check` cannot answer - it stops at metadata. Deliberately a SEPARATE
  # attrset from `crossPackages`, because `checks.one-binary`, `ociImages` and the release
  # workflow all read that one: a probe cannot reach an image or a published asset even by
  # mistake, the same guarantee the `-ci` variants already rest on.
  #
  # Empty when no binary declares a probe feature, and then CI's loop builds nothing - a set with
  # no members rather than a step that reads as having run.
  featurePackages = builtins.listToAttrs (builtins.concatMap
    (b: builtins.concatMap
      (feature: map
        (t: {
          name = "${b.bin}-${feature}-${t}-ci";
          value =
            if t == hostRustTarget
            then nativeFor { binary = b; profile = "ci"; features = [ feature ]; }
            else crossFor { binary = b; target = t; profile = "ci"; features = [ feature ]; };
        })
        releaseTargets)
      b.probeFeatures)
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

  # The local image per binary, and its performance sibling: `nix build .#oci` and
  # `.#oci-serve` are what a developer and the release workflow reach for. Containers are Linux,
  # so Darwin selects the matching musl cross build rather than putting a Mach-O binary in an
  # image labelled Linux. On Linux this remains the host build.
  #
  # streamLayeredImage, not buildLayeredImage: it avoids materialising a multi-hundred-MB
  # tarball in the store just to push it. See `ociFor`.
  localImages = builtins.listToAttrs (builtins.concatMap
    (b:
      let
        imageTarget = {
          "aarch64-darwin" = "aarch64-unknown-linux-musl";
          "x86_64-darwin" = "x86_64-unknown-linux-musl";
        }.${system} or hostRustTarget;
        imageOf = suffix: ociFor {
          package = crossPackages."${b.bin}-${imageTarget}${suffix}";
          architecture = ociArch imageTarget;
          inherit (b) bin entrypoint cmd description;
        };
      in
      [
        { name = ociNameFor b; value = imageOf ""; }
        {
          name = "${ociNameFor b}-performance";
          value = imageOf "-performance";
        }
      ])
    binaries);
in
{
  inherit
    binaries
    crossPackages
    crossTargets
    featurePackages
    imageTargets
    keyFor
    nativeBinaries
    localImages
    ociImages
    releaseTargets
    ;
}
