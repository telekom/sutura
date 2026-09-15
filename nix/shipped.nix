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
, craneLib
, commonArgs
, inheritedArtifacts
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
  # **ONE ENTRY, since `github.com/telekom/sutura#685` step 2.** There used to be two: `sutura` and
  # `sutura-serve`, a split `sutura-serve`'s own module header called unprincipled - the release
  # derivations "named the binary that existed before `sutura-serve` did, and kept naming it." The
  # HTTP surface is now `sutura serve`, composed inside `sutura-cli`, so this list is back to the
  # one executable a release has ever actually needed to ship.
  #
  # **FEATURES ARE ABSENT FROM THIS RECORD ON PURPOSE, and that absence IS the decision issue #111
  # asks to be stated rather than discovered.** The published binary is built with cargo's DEFAULT
  # feature set, so it carries the HTTP surface, leg 1 and the rate limiter (unconditional edges),
  # but neither `tls`, `bigquery`, `postgres` nor `datahub`: an operator who wants one of those
  # builds from source with `--features`, and `sutura doctor` says which of them a binary was
  # built with.
  #
  # **That is issue #121's step 3, decided as its own recommendation had it:** the binary ships
  # without the feature, and a deployment that needs it runs a build that carries it. The
  # alternative priced there - a second asset with the feature on, for the two gnu triples only -
  # is more assets to sign, attest and SBOM plus a musl answer stated rather than discovered, and
  # nothing asks for it yet. When a tutorial chapter does, it is a `features` field here and a
  # paragraph beside this one.
  #
  #   * The cost is the four cross builds. `--features tls` and `--features bigquery` each pull an
  #     outbound or inbound rustls closure, `ring` included, which compiles C and assembly; two of
  #     the four release triples are musl. A published binary would pay for that on every target -
  #     measured on 2026-09-02 by compiling `sutura-cli --all-targets` both ways: the default set
  #     touches neither `ring` nor `ureq`, and `--features bigquery` compiles `ring` from C and
  #     assembly.
  #   * **THAT FIRST BULLET IS FALSE ON COST, and it is left standing because the correction is
  #     the useful part.** `craneLib.buildDepsOnly` is called on `args` - deliberately unscoped, so
  #     the checks share one dependency derivation - with `cargoExtraArgs` set on the final attrset
  #     instead, so that derivation resolves the WHOLE workspace at cargo's default set AND builds
  #     its dev-dependencies. `ring`, `rustls` and `ureq` therefore compile inside
  #     `sutura-deps-<triple>` on all four triples WITH THE FEATURE OFF - read out of the `cross`
  #     logs, not predicted - so the musl C and assembly is paid on every pull request either way.
  #   * **AND NOT BECAUSE OF THIS ADAPTER, which is a second correction the first one needed.**
  #     `sutura-exec-bigquery`'s `ureq` is `optional` behind its `wire` feature, so it contributes
  #     nothing at the default set. `cargo tree` on 2026-09-04 gives the real edges: for
  #     `aarch64-unknown-linux-musl` the only one is `sutura-catalog-datahub`'s NON-optional `ureq`
  #     dev-dependency, and `libduckdb-sys` puts a host-side copy there as a build-dependency. So
  #     the cost is paid by a dev-dependency elsewhere in the workspace, and it would come back if
  #     that dependency went - which is the kind of thing a comment asserting the wrong cause hides.
  #   * The failure is loud rather than silent, which is what makes the choice defensible instead
  #     of merely cheap. `security.tls_termination: in-process` on a build without `tls` is a
  #     startup refusal naming the feature, and so is a `kind: bigquery` source on a build without
  #     `bigquery` - `sutura_config` and `sutura-cli` both refuse rather than degrade, each naming
  #     the feature that would link it. An operator who needs either builds from source and knows
  #     it.
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
  # **AND DEFAULT-OFF SURVIVES ON A DIFFERENT REASON THAN THE ONE ABOVE, because running the
  # probe priced it.** `--features bigquery` costs **12 compiled units** on every one of the four
  # published triples - the adapter and its outbound TLS closure, nothing else - and under 2% of a
  # `cross` job, because they finish inside the slack ahead of `datafusion` on the critical path.
  # (CI runs 33808343712 and 33838360913, 2026-09-04. A figure with no run beside it is a figure
  # nobody can re-take, which is why this one carries one.)
  # So default-off buys nothing in BUILD time. What it buys is the artefact: the published binary
  # links no outbound TLS stack, and `checks.shipped-features` asserts that out of its own
  # embedded dependency list rather than out of this file. Keep the decision and cite the
  # artefact for it.
  #
  # `docs/adr/0017` carries the numbers, the derivation A/B they rest on, and what the measurement
  # does NOT cover. One record: a transcript of it here would be a second thing to keep true, and
  # a copy is what rots first.
  binaries = [
    {
      bin = "sutura";
      package = "sutura-cli";
      # The image key, and the empty string is HISTORICAL rather than tidy: `sutura-oci-<triple>`
      # asset names, `<version>-<triple>` leaf tags and the unsuffixed `:latest` are what every
      # release so far published and what `docs/verifying-a-release.md` documents.
      keyPrefix = "";
      entrypoint = "/bin/sutura";
      # A default that does something harmless and provable. The release workflow smoke-tests it.
      # No default argument would ALSO be correct now that `sutura serve` reads its whole
      # configuration from the settings tree - but `--version` is what the release workflow has
      # always smoke-tested, and this entry is not the place to change that.
      cmd = [ "--version" ];
      description = "identity-aware semantic data runtime for AI agents";
      # `docs/getting-started.md` tells a reader to run `cargo build -p sutura-cli
      # --features bigquery`, and before this entry nothing anywhere proved that configuration
      # LINKS on a triple this project publishes. `ureq`, rustls and `ring` are what it adds, and
      # `ring` compiles C and assembly, so the two musl triples are the answer worth having.
      #
      # `postgres` carries the same risk and was added later (`telekom/sutura#124`):
      # `sutura-exec-postgres` is itself pure Rust, but this binary's `postgres` feature makes it a
      # normal dependency and it is not optional there - `tokio-postgres-rustls` and `rustls` are
      # what it adds, `ring` behind them, so the musl link is the same question `bigquery` already
      # answers and had gone unasked for this feature.
      #
      # `tls` and `datahub` are NOT here, and that is unchanged by the fold
      # (`github.com/telekom/sutura#685` step 2): they were `sutura-serve`'s features before the
      # fold and this binary's since, but neither ever had a documented single-feature source
      # build to hold a `<bin>-<feature>-<triple>-ci` probe for - `allFeatures` below is what
      # proves them, together, at fat LTO.
      probeFeatures = [ "bigquery" "postgres" ];
      # THE COMPLETE optional feature list, for `allFeaturesProbes` below - `github.com/telekom/
      # sutura#685` step 1's fat-LTO probe, one build with EVERY feature on rather than one build
      # per feature. **Grew by two at step 2's fold**: `tls` and `datahub` were `sutura-serve`'s
      # own features before `sutura-serve` folded into `sutura serve`, and the shipped artefact now
      # carries all four. Kept as its own field rather than reused for `probeFeatures` so a future
      # feature added to one list without the other is a diff a reviewer sees, not a silent gap.
      allFeatures = [ "bigquery" "postgres" "tls" "datahub" ];
      # This binary legitimately links `polyglot-sql`, for `compile` - `sutura-sql` is a normal
      # dependency of `sutura-cli` and the generator is what renders the statement that
      # subcommand prints. Nothing extra to forbid here beyond the shared list below.
      #
      # **Also unaffected by the fold.** `sutura-serve` used to ban this edge for itself
      # (`alsoForbidden = [ "polyglot-sql" ]`) because it had no legitimate reason to link the SQL
      # generator and `sutura` did; folding the two into one binary makes that ban moot rather than
      # something to carry over - the one binary that remains is the one that was always allowed to
      # link it.
      alsoForbidden = [ ];
    }
  ];

  # The image/asset key for one binary at one target. `sutura` keeps the bare triple; a SECOND
  # shipped binary would be prefixed instead, so `oci-<prefix>-x86_64-unknown-linux-musl` would
  # name a different build than `sutura`'s own `oci-x86_64-unknown-linux-musl`. Dormant since
  # `github.com/telekom/sutura#685` step 2 folded the one binary that used to set `keyPrefix` back
  # into `sutura`, and left generic for the same reason `binaries` above is a list rather than one
  # hand-written pair of derivations: a third executable is an entry, not a rewrite of this
  # function.
  keyFor = b: target: if b.keyPrefix == "" then target else "${b.keyPrefix}-${target}";

  # The `oci` attribute name for a binary's NATIVE image: `oci` for a binary with no prefix, a
  # prefixed name otherwise. Same rule as `keyFor`, applied where there is no target to append -
  # and the same dormant-since-#685-step-2 note.
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
    # `doCheck = false` ON THE DEPS DERIVATION, stated rather than defaulted because
    # `cargo xtask check-warm-start` requires every `buildDepsOnly` to say which it is. crane
    # defaults it to `true`, and that default runs `cargo test --no-run` over the whole closure
    # after `cargo check` and `cargo build` - a second codegen of every dependency, to cache
    # dev-dependency artifacts. NOTHING here consumes them: the consumer below sets
    # `doCheck = false` two dozen lines down, because the tests are their own check in
    # `flake.nix`. Measured on `nix/jscpd.nix`, the same shape: 231 `Compiling` lines to 137.
    #
    # AND THE NOTE BELOW ABOUT STAYING BYTE-IDENTICAL TO WHAT THE CHECKS SHARE IS ALREADY SPENT,
    # which is why this is free rather than a trade. Measured: this file's deps derivation is
    # `sutura-deps-0.1.0.drv` at `h802qsgq…` and `checks.clippy`'s is `lwb1zx31…`. The `ci`
    # profile split them when it landed, so `release` has had a dependency build of its own since
    # then and nothing is being un-shared here.
    craneLib.buildPackage (args // inheritedArtifacts (craneLib.buildDepsOnly (args // { doCheck = false; })) // {
      # NAMED AFTER THE EXECUTABLE, so a build log and a store path say which binary this is -
      # `commonArgs.pname` is `sutura` for the whole workspace, which used to make both shipped
      # binaries' derivations `sutura-0.1.0` before `github.com/telekom/sutura#685` step 2 left
      # one. Kept rather than simplified away: a second shipped binary is one entry in `binaries`
      # above away, and this is what keeps its build log distinguishable when it arrives. On the
      # final attrset and never on `args`, so the deps derivation stays as unscoped as it can be -
      # see the measurement above for what sharing with the checks is still available and what the
      # `ci` profile already ended.
      pname = binary.bin;
      # ONE package. Without this, crane builds the whole workspace and the result held
      # three binaries - `sutura`, `sutura-dev` and `xtask` - which made two stated
      # invariants false: the image is supposed to hold one executable, and xtask's
      # compile-time `CARGO` reference pulled the whole cargo store path into the runtime
      # closure. It also broke reproducibility, because that path differs between builds.
      #
      # On the attrset and not on `args`: a per-package deps build is a deps build per
      # package, which is the duplication this whole file is arranged to avoid.
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
        (p: p.rust-bin.fromRustupToolchainFile ../devco/rust-toolchain-nightly.toml);
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
        # an earlier mimalloc major it switches off `MI_USE_BUILTIN_THREAD_POINTER`
        # (`include/mimalloc/prim.h`), so the thread id comes from the TLS slot instead of
        # `__builtin_thread_pointer`. `CFLAGS_<triple>` is the `cc` crate's per-target
        # hook; it is appended to the flags cc already computed, not a replacement for
        # them, and it beats `TARGET_CFLAGS` / `CFLAGS` in cc's lookup order so nothing
        # in the sandbox can shadow it.
        "CFLAGS_${builtins.replaceStrings [ "-" ] [ "_" ] target}" = "-DMI_LIBC_MUSL=1";
      };
    in
    crossLib.buildPackage (args // inheritedArtifacts (crossLib.buildDepsOnly args) // {
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

  # Every cross-built package, over three dimensions: binary, target, profile variant -
  # `sutura-<triple><suffix>` today, and a second binary's own `<bin>-<triple><suffix>` cannot
  # collide with it because no target triple begins with a binary's name.
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
  # `sutura-performance`.
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
  # `sutura-bigquery-<triple>-ci`. The name is for a human reading `nix build`; nothing parses it,
  # because each manifest row below carries the executable and the feature as their own fields.
  #
  # **`ci` PROFILE AND NOTHING ELSE, which is what keeps this a link check rather than a second
  # release matrix.** Nothing executes the result and nothing publishes it; what is being asked is
  # whether the feature's C and assembly cross-compile and LINK for a musl triple, which is the one
  # question a native `cargo check` cannot answer - it stops at metadata. Deliberately a SEPARATE
  # attrset from `crossPackages`, because `checks.one-binary`, `ociImages` and the release
  # workflow all read that one: a probe cannot reach an image or a published asset even by
  # mistake, the same guarantee the `-ci` variants already rest on.
  #
  # ONE FLAT LIST, and both views below read it, so the packages CI can build and the manifest CI
  # reads to know WHICH to build cannot disagree about which probes exist.
  probes = builtins.concatMap
    (b: builtins.concatMap
      (feature: map
        (t: {
          target = t;
          # The two fields the workflow would otherwise have to recover by splitting the name.
          exe = b.bin;
          inherit feature;
          name = "${b.bin}-${feature}-${t}-ci";
          value =
            if t == hostRustTarget
            then nativeFor { binary = b; profile = "ci"; features = [ feature ]; }
            else crossFor { binary = b; target = t; profile = "ci"; features = [ feature ]; };
        })
        releaseTargets)
      b.probeFeatures)
    binaries;

  # DISJOINT BY ASSERTION, because `//` is right-biased and this is the one place the
  # two-views-of-one-list argument stops - one level ABOVE the list. `flake.nix` merges
  # `crossPackages // ... // featurePackages`, so a probe whose name equalled a shipped package's
  # would SHADOW it: the shipped link step would build a probe while its notice named an
  # artefact. A feature name that happens to equal another binary's `bin` is exactly that
  # collision - reachable again the day a second binary joins `binaries` above. A `throw` rather
  # than a naming rule, so the failure names the collision.
  featurePackages =
    let
      attrs = builtins.listToAttrs (map (p: { inherit (p) name value; }) probes);
      shippedNames = crossPackages // nativeBinaries;
      clashes = builtins.filter (n: builtins.hasAttr n shippedNames) (builtins.attrNames attrs);
    in
    if clashes == [ ] then attrs
    else throw ("nix/shipped.nix: probe package name(s) ${builtins.concatStringsSep ", " clashes} "
      + "collide with a shipped package. `//` is right-biased in flake.nix, so the probe would "
      + "shadow the artefact. Rename the feature, or the probe naming rule.");

  # `github.com/telekom/sutura#685` step 1: whether ALL of a binary's optional features link
  # together at `release-performance` (fat LTO, `codegen-units = 1`, `panic = "abort"`) on the
  # MUSL triples, where `ring`'s C and assembly is the risk `probes` above never takes - that list
  # builds one feature at a time, at the `ci` profile, and never at LTO. Reads `allFeatures`, a
  # field of its own rather than a reuse of `probeFeatures`: the two lists agreed for `sutura`
  # before step 2's fold and must not have to in general - before the fold, `sutura-serve`'s
  # `probeFeatures` was `[ ]` while its `allFeatures` carried all four of `tls`, `bigquery`,
  # `postgres` and `datahub`, because #685 ships one binary with every feature on and proving only
  # the CLI's two features was not proving that artefact. `sutura`'s own `allFeatures` carries the
  # union since the fold. A binary with an empty `allFeatures` yields no probe rather than an
  # empty-features build indistinguishable from `release-performance` itself - not reachable
  # today, since the one remaining binary declares a non-empty list, but the guard costs nothing
  # to keep.
  #
  # MUSL ONLY. The gnu triples are the easy case #685 defers to a later step.
  #
  # A SEPARATE attrset, same reason `featurePackages` is: `checks.one-binary`, `ociImages` and the
  # release workflow read `crossPackages` and `binaries`, none of which this touches, so a probe
  # here cannot reach an image or a published asset even by mistake. The name carries
  # `-performance-probe`, which no `crossPackages` or `featurePackages` key ends in, so the
  # `featurePackages` collision throw above has nothing to guard against here.
  allFeaturesProbes = builtins.listToAttrs (builtins.concatMap
    (b: builtins.concatMap
      (t: pkgs.lib.optional (b.allFeatures != [ ]) {
        name = "${b.bin}-all-features-${t}-performance-probe";
        value = crossFor {
          binary = b;
          target = t;
          profile = "release-performance";
          features = b.allFeatures;
        };
      })
      (builtins.filter (t: pkgs.lib.hasSuffix "-linux-musl" t) crossTargets))
    binaries);

  # WHAT TO BUILD FOR ONE TRIPLE, as a file at a FIXED attribute name: `feature-probes-<triple>`
  # holds one row per probe - the package to build, the executable it installs, and the feature it
  # was built with - and `.github/workflows/cross-link.yml` reads the three fields rather than
  # deriving any of them.
  #
  # **A FILE RATHER THAN A PATTERN IN THE WORKFLOW, and the reason is a dead gate this branch
  # shipped and then measured.** The step used to RECONSTRUCT the set from the package names above
  # - every `-<triple>-ci` attribute minus the shipped ones - so the naming rule was a coupling
  # nothing could check, and reordering the name emptied the set while `probeFeatures` still
  # declared `bigquery`: green run, reassuring sentence, exit ZERO. `docs/adr/0017` has the
  # reproduction.
  #
  # Three properties close it, and each is why this is a derivation and not a convention. A flake
  # that stops producing a manifest fails `nix build` instead of yielding nothing; an empty
  # manifest then means what it says, so the step can refuse rather than guess; and the two fields
  # a printed sentence quotes come from HERE, so renaming a package cannot make that sentence
  # wrong. `file` carries none of it - it exits zero on a path that is not there, measured.
  probeManifests = builtins.listToAttrs (map
    (t: {
      name = "feature-probes-${t}";
      value = pkgs.writeText "feature-probes-${t}"
        (pkgs.lib.concatMapStrings (p: "${p.name} ${p.exe} ${p.feature}\n")
          (builtins.filter (p: p.target == t) probes));
    })
    releaseTargets);

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

  # The two checks that assert what the list above SHIPPED, rather than what it was asked to
  # ship. Here rather than in `flake.nix` because every input they read is this module's -
  # `binaries`, `imageTargets`, `crossPackages`, `nativeBinaries`, and the features paragraph
  # at the head of this file that `shipped-features` is the mechanism for. `flake.nix` also has
  # no room: it sits within a few dozen lines of the 1000-line limit `cargo xtask max-lines`
  # holds, so a check's BODY has to live with the decision it checks. `flake.nix` still
  # declares both under `checks`, which is where `nix flake check` and two xtask gates look.
  artifactChecks = {
    # A shipped package holds one executable, THAT executable is the one it was supposed to
    # build, and no toolchain is in its closure. It held three binaries and a full cargo
    # once, so this is a check rather than a sentence in a comment.
    #
    # EVERY shipped binary at EVERY shipped target, and both dimensions are the point. A
    # cross build has its own dependency derivation and its own `cargoExtraArgs`, so "the
    # native package holds one binary" says nothing about the musl one; and `binaries` is a list
    # rather than a single hand-written pair - it held two entries between #111 and
    # `github.com/telekom/sutura#685` step 2 - so "the package built one thing" says nothing about
    # WHICH thing. The price is that this check pulls every cross build in, which is what it
    # costs for the assertion to be true rather than assumed.
    #
    # **THE NAME ASSERTION IS THE HALF THAT IS NEW, and it is the cheap guard against the
    # defect #111 was.** `cargoExtraArgs` names a cargo PACKAGE while the image names an
    # ENTRYPOINT PATH, and nothing relates the two: a `--package` pointing at the wrong
    # crate builds, ships, and produces an image whose entrypoint does not exist - which
    # fails at `docker run` on somebody else's machine. Two lines here turn that into a
    # red gate.
    one-binary =
      let
        cells = pkgs.lib.concatMap
          (b: map (target: { inherit target; inherit (b) bin; drv = crossPackages."${b.bin}-${target}"; }) imageTargets)
          binaries;
        checkOne = p: ''
          echo "one-binary: ${p.bin} ${p.target}"
          count="$(ls ${p.drv}/bin | wc -l)"
          if [ "$count" != "1" ]; then
            echo "${p.bin} ${p.target}: the shipped package holds $count binaries, expected 1:" >&2
            ls ${p.drv}/bin >&2
            exit 1
          fi
          if [ ! -x "${p.drv}/bin/${p.bin}" ]; then
            echo "${p.bin} ${p.target}: the shipped package holds no executable called '${p.bin}', so the image entrypoint would not exist:" >&2
            ls ${p.drv}/bin >&2
            exit 1
          fi
          # A toolchain in the closure means something baked a build-time path into the
          # binary. That is how cargo got in: `env!("CARGO")` in a workspace member.
          if grep -qE '(cargo|rustc|rust-minimal)-[0-9]' ${pkgs.closureInfo { rootPaths = [ p.drv ]; }}/store-paths; then
            echo "${p.bin} ${p.target}: a Rust toolchain is in the runtime closure:" >&2
            grep -E '(cargo|rustc|rust-minimal)-[0-9]' ${pkgs.closureInfo { rootPaths = [ p.drv ]; }}/store-paths >&2
            exit 1
          fi
        '';
      in
      pkgs.runCommand "sutura-one-binary" { } ''
        set -eu
        ${pkgs.lib.concatMapStrings checkOne cells}
        touch $out
      '';

    # WHICH FEATURES A PUBLISHED BINARY CARRIES, asserted from inside the binary.
    #
    # `nix/shipped.nix` decides that the shipped binary is built with cargo's DEFAULT feature
    # set, and the reason is the four cross builds: `tls`, `bigquery`, `postgres` and `datahub`
    # each pull a rustls closure with `ring` in it, and two of the four release triples are musl.
    # Issue #111 asks for that to be a STATED choice rather than one somebody discovers, and a
    # comment is not a mechanism - so this is the mechanism.
    #
    # READ OUT OF THE ARTIFACT, never out of a manifest. `nix/auditable.nix` builds the shipped
    # binary with `cargo auditable`, which puts the crates the compiler actually linked into one
    # ELF section, and `rust-audit-info` reads them back. A check over `Cargo.toml` would be
    # asserting what somebody wrote down; this asserts what shipped. It is the same section
    # `release.yml`'s SBOM and the pull-request link check already depend on, so a build that
    # stopped embedding it fails here too rather than passing quietly.
    #
    # TWO DIRECTIONS, because only checking the absence would pass on a binary that linked
    # nothing at all: `axum` must be present, and `ring` absent.
    #
    # **What this does NOT claim.** It is a statement about a crate NAME in a list, not
    # about reachable code: a future default feature that pulls TLS under a different crate
    # name is invisible to it, and so is a crate present for a reason other than the feature
    # this row is about. It is also the NATIVE build only - the cross artifacts get the same
    # `cargoExtraArgs` from the same list, so the feature set cannot differ per target
    # without `nix/shipped.nix` saying so, and pulling four cross builds in to re-read the
    # same list would double this check's cost for nothing.
    shipped-features =
      let
        # `axum` for the HTTP surface `sutura serve` runs and `datafusion` for the engine
        # neither `sutura serve` nor any other command can answer a question without.
        #
        # **`axum` moved here from a `sutura-serve` key when `github.com/telekom/sutura#685`
        # step 2 folded that binary into `sutura serve` - it is NOT named in that issue's own
        # body, and dropping the old key without this addition would leave nothing in
        # `required` asserting `axum` is linked at all, which is a silent weakening of this
        # gate rather than a fold.**
        required = { sutura = [ "axum" "datafusion" ]; };
        # `ring` and not `rustls`: `rustls` is a name several crates in the closure carry a
        # variant of, while `ring` is the one that compiles C and assembly and is therefore
        # the one the cross builds actually pay for.
        #
        # **The shipped binary has four features to leave off since issue #121 and #685, and
        # this list is what says it left them off** - an assertion about the ARTIFACT rather
        # than about a manifest, which is the whole reason it reads the embedded dependency
        # list. A `bigquery`, `postgres`, `tls` or `datahub` that stopped being optional fails
        # here.
        #
        # Per-binary rather than only shared: a binary's own `alsoForbidden` (declared beside
        # it above) is appended per binary in `checkOne` below. `sutura`'s is empty - it
        # legitimately links `polyglot-sql` for `compile`, which used to be `sutura-serve`'s
        # own reason to ban it for ITSELF alone; folding the two binaries into one made that
        # ban moot rather than something to carry forward.
        forbidden = [ "ring" "ureq" ];
        quoted = name: "'\"" + name + "\"'";
        wantOne = bin: name: ''
          if ! grep -q ${quoted name} deps-${bin}.json; then
            echo "${bin}: the embedded dependency list does not name ${name}" >&2
            exit 1
          fi
        '';
        banOne = bin: name: ''
          if grep -q ${quoted name} deps-${bin}.json; then
            echo "${bin}: the embedded dependency list names ${name}, so the published binary carries a feature nix/shipped.nix says it does not - see the features paragraph there" >&2
            exit 1
          fi
        '';
        checkOne = b:
          let drv = nativeBinaries.${b.bin}; in ''
          echo "shipped-features: ${b.bin}"
          rust-audit-info ${drv}/bin/${b.bin} > deps-${b.bin}.json
          crates="$(grep -o '"name"' deps-${b.bin}.json | wc -l)"
          # A FLOOR, and the argument is not repeated here. It is what
          # `.github/actions/build-artefacts/action.yml` gives at its own copy of this number:
          # the exact count moves with every dependency bump, and what is checked is the difference
          # between a list of crates and no list at all. `grep -o | wc -l`, never `grep -c`,
          # because the document is one line.
          if [ "''${crates:-0}" -lt 100 ]; then
            echo "${b.bin}: read $crates crate(s) from the binary, expected at least 100 - the cargo auditable section is missing, so nothing below means anything" >&2
            exit 1
          fi
          echo "${b.bin}: $crates crate(s) embedded"
          ${pkgs.lib.concatMapStrings (wantOne b.bin) required.${b.bin}}
          ${pkgs.lib.concatMapStrings (banOne b.bin) (forbidden ++ b.alsoForbidden)}
        '';
      in
      pkgs.runCommand "sutura-shipped-features" { nativeBuildInputs = [ pkgs.rust-audit-info ]; } ''
        set -eu
        ${pkgs.lib.concatMapStrings checkOne binaries}
        touch $out
      '';
  };
in
{
  inherit
    allFeaturesProbes
    artifactChecks
    binaries
    crossPackages
    crossTargets
    featurePackages
    imageTargets
    keyFor
    nativeBinaries
    localImages
    ociImages
    probeManifests
    releaseTargets
    ;
}
