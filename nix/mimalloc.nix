# The allocator, as a derivation per target and optimisation level.
#
# ITS OWN FILE for the reason `nix/api-docs.nix` gives: flake.nix sat at exactly the 1000-line
# limit `cargo xtask max-lines` enforces. This is the cleanest of the seams - nothing here reads
# crane, the flake inputs or the source filter, so the interface is one `pkgs` and two functions
# out.
#
# `optLevelFor` travels WITH it and not with the profiles it names, because the number it returns
# has to be the number cc-rs computes for that profile - the two are one decision, and splitting
# them is how the allocator would silently decouple from the code it is linked into.
{ pkgs }:

let
  # The allocator's C, compiled in its own derivation rather than by the build script.
  #
  # WHY A DERIVATION. `vendor/mimalloc_rust` is a PATH dependency, so crane's shared
  # dependency build does not shield it the way it shields a registry crate: without
  # this, every edit to our own Rust recompiled mimalloc's C, once per target. Here it
  # is hash-addressed by version, target and optimisation level, so it is built once per
  # combination and then reused from the store and from CI's cache. Our source changes
  # cannot invalidate it. The first build per target is still from source, because
  # nothing upstream caches a musl cross of mimalloc.
  #
  # WHY NOT CMAKE, which would have been the obvious way to build a C library. Upstream's
  # `CMakeLists.txt` decides three things behind our back. `MI_OVERRIDE` defaults ON, which
  # compiles `alloc-override.c` and exports `malloc`, `free` and `operator new` - a
  # semantic change, and the thing issue #5 turns on. `MI_OPT_ARCH` defaults ON for arm64
  # and raises the architecture floor implicitly, which is what Debian, Fedora and nixpkgs
  # all patch out; we DO raise that floor below, but as a stated decision rather than a
  # default nobody chose. And `MI_LIBC_MUSL=ON` appends `-ftls-model=local-dynamic`,
  # against the reasoning in `crates/sutura-cli/src/main.rs`. Compiling `src/static.c` -
  # the single translation unit upstream maintains for exactly this purpose, and the one
  # the build script itself compiles - means none of those defaults exist to override.
  #
  # THE FLAGS ARE A MEASUREMENT, not a design: they are what cc-rs passes today, captured
  # with `CC_ENABLE_DEBUG_OUTPUT=1`. The one addition is `-DMI_PADDING_CHECK_BYTES=1`,
  # because 3.5.0 redefined `MI_SECURE=4` to mean level 3 and moved byte-precise
  # buffer-overflow checking to level 5; without it a `secure level: 4` line would be
  # quietly weaker than the one it replaces. See issue #5.
  mimallocVersion = "3.5.0";
  mimallocFor = { targetPkgs, optLevel, isMusl }:
    let
      # ARMv8.3 FLOOR for the aarch64 targets, deliberately. mimalloc 3.5.0 gains from
      # `LDAPR` (FEAT_LRCPC, v8.3) for its C11 acquire loads, and the level also brings
      # `FEAT_LSE` (v8.1), so atomics become `cas`/`ldadd` rather than `ldxr`/`stxr`
      # retry loops. Measured on the real translation unit: 58 acquire loads move from
      # `ldar` to `ldapr`, with the object file the same size. The aarch64 artifacts
      # therefore REQUIRE ARMv8.3-A or later, and `.cargo/config.toml` sets matching
      # Rust features so the C and the Rust agree on that floor.
      isAarch64 = targetPkgs.stdenv.hostPlatform.isAarch64;
    in
    targetPkgs.stdenv.mkDerivation {
      pname = "mimalloc-static";
      version = mimallocVersion;
      # `fetchurl` on the release tarball, not `fetchFromGitHub`: this way the recorded
      # hash is the hash of the artifact upstream published, which anyone can check with
      # `curl` and `sha256sum`. `fetchFromGitHub` would record a NAR hash of the unpacked
      # tree instead, which is checkable only by nix.
      src = pkgs.fetchurl {
        name = "mimalloc-${mimallocVersion}.tar.gz";
        url = "https://codeload.github.com/microsoft/mimalloc/tar.gz/refs/tags/v${mimallocVersion}";
        sha256 = "1e432f0559a4ab512143b9bff7a700541a2c8d4712b26a72de3e0222790da305";
      };
      dontConfigure = true;
      # Matches cc-rs, which sets it for the same reason: a timestamp in the archive
      # would make the output differ between builds.
      env.ZERO_AR_DATE = "1";
      buildPhase = ''
        runHook preBuild
        $CC -O${optLevel} -ffunction-sections -fdata-sections -fPIC \
          -I include -I src \
          -Wall -Wextra -Wno-error=date-time \
          -ftls-model=initial-exec \
          -DMI_SECURE=4 -DMI_PADDING_CHECK_BYTES=1 \
          -DMI_DEBUG=0 -DMI_BUILD_RELEASE -DNDEBUG \
          ${pkgs.lib.optionalString isMusl "-DMI_LIBC_MUSL=1"} \
          ${pkgs.lib.optionalString isAarch64 "-march=armv8.3-a"} \
          -c src/static.c -o static.o
        $AR cqD libmimalloc.a static.o
        runHook postBuild
      '';
      installPhase = ''
        runHook preInstall
        mkdir -p $out/lib
        cp libmimalloc.a $out/lib/
        runHook postInstall
      '';
    };

  # The C tracks the cargo profile, so the derivation has to as well. Measured rather
  # than assumed: `release` compiles the allocator at `-O1` and `release-performance` at
  # `-O3`, because cc-rs reads cargo's `OPT_LEVEL`. Freezing one number here would
  # silently decouple the allocator from the profile, so this is a pure caching change
  # and not a performance one.
  #
  # `dev` lands on `-O3` and that is NOT an oversight: the allocator is a DEPENDENCY, and
  # `[profile.dev.package."*"] opt-level = 3` in Cargo.toml is what cc-rs sees for it - the
  # `opt-level = 0` on `[profile.dev]` applies to our own crates, not to this. Reading the
  # wrong one of those two keys is the easy mistake here. It also means `dev` reuses the
  # `release-performance` archive rather than adding a third C build to the cache.
  # EXPLICIT per profile, and an unknown one is an error rather than a default. It was
  # `if profile == "release" then "1" else "3"` while there were two profiles, and when a
  # third arrived it inherited `3` by falling through the `else` - silently, and nobody
  # chose it. `throw` is the whole point: a fourth profile has to state its own number
  # here, because the value has to match what cc-rs computes for that profile or the
  # allocator decouples from the code it is linked into.
  optLevelFor = profile:
    {
      # cc-rs reads cargo's OPT_LEVEL, and for a DEPENDENCY that is
      # `[profile.<p>.package."*"]` rather than the profile's own `opt-level`.
      release = "1";
      release-performance = "3";
      # `[profile.ci.package."*"] opt-level = 0` - the point of that profile is compile
      # speed, so its allocator is compiled to match rather than shared with a shipped one.
      ci = "0";
    }.${profile} or (throw "optLevelFor: no opt level declared for profile '${profile}'");
in
{
  inherit mimallocFor optLevelFor;
}
