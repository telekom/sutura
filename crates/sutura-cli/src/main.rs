//! The sutura binary.
//!
//! M0 deliberately ships almost nothing: its purpose is to prove the machinery - that the
//! toolchain resolves, the workspace compiles, the gates run, and a release image builds
//! and runs. Behaviour arrives with the milestone that needs it.

use sutura_domain::identity::Secret;

// mimalloc as the global allocator, on Linux only.
//
// WHY REPLACE THE SYSTEM ALLOCATOR. This is a service whose unit of work is a warehouse round
// trip fanned out over threads, and both libcs we ship on allocate badly under exactly that
// shape. musl is the acute case: mallocng serialises the whole process on ONE lock word.
// `src/malloc/mallocng/glue.h` defines `rdlock` and `wrlock` as the same exclusive lock and
// `upgradelock` as a no-op, and `struct malloc_context` is a single global with no arenas and
// no per-thread cache, so there is nothing for a second thread to contend on but that word.
// Its own author's assessment of the rewrite was "I don't think any significant improvement
// was made here".
//
// Measured, one binary with only threading toggled (Tweag, 2023): the single-threaded control
// is a wash at 41.4s either way, and the 48-core run goes 4.45s on glibc to 92.16s on musl -
// 20.7x, and 2.2x slower than musl's OWN single-core run, so more cores made it worse.
// Linking mimalloc in takes the same 48-core run to 3.83s, ahead of glibc. Expect roughly
// 1.0x single-threaded and 4-20x for a threaded application; the microbenchmark spreads are
// larger and are not what an application sees.
//
// WHY `secure` STAYS ON, which is the part that looks like a mistake. It compiles mimalloc at
// MI_SECURE=4: guard pages around each page, encrypted free lists, randomised placement. That
// costs 23-43% against plain mimalloc - upstream's README still says "~10%", which is stale
// twice over - and it is paid anyway, because mimalloc-secure still beats glibc in upstream's
// own scoring (62 vs 39 on AMD, 59 vs 46 on Xeon) and musl's mallocng is SLOWER than glibc.
// The hardening comes out of a margin this binary would not otherwise have had.
//
// WHY NOT `local_dynamic_tls`, which also looks like an omission. That advice
// (microsoft/mimalloc#644, and upstream's CMake putting `-ftls-model=local-dynamic` in
// `mi_cflags_static` whenever `MI_LIBC_MUSL` is set) is for a static archive linked into a
// shared object that is later `dlopen`ed, where an initial-exec TLS block cannot be allocated
// at load time. What we ship is a fully static executable with no dynamic loader in the image
// at all, so initial-exec is both correct and the cheaper access sequence - and it is what
// `libmimalloc-sys`' build script already picks when the feature is off. Turning the feature
// on here would buy a slower fast path to solve a problem this artifact cannot have.
//
// NO `unsafe`, AND NO LINT ESCAPE. `#[global_allocator]` on a static is a safe attribute - the
// `unsafe impl GlobalAlloc` lives inside the mimalloc crate - so this compiles as-is under the
// workspace's `unsafe_code = "forbid"`. There is deliberately no change to the lint table and
// no `#[expect(unsafe_code)]`: narrowing a `forbid` is E0453, so needing one would mean this
// was written wrong.
#[cfg(target_os = "linux")]
#[global_allocator]
static ALLOCATOR: mimalloc::MiMalloc = mimalloc::MiMalloc;

/// Which allocator this binary was linked against, for `doctor`.
///
/// A string rather than a probe: the honest runtime proof is `MIMALLOC_VERBOSE=1`, which makes
/// mimalloc itself announce its version and options on stderr. This line only says what was
/// compiled in.
const ALLOCATOR_NAME: &str = if cfg!(target_os = "linux") {
    "mimalloc 2.x (secure)"
} else {
    "system"
};

fn main() {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("--version" | "-V") => println!("sutura {}", env!("CARGO_PKG_VERSION")),
        Some("doctor") => doctor(),
        Some(other) => {
            eprintln!("sutura: unknown argument `{other}`");
            eprintln!("usage: sutura [--version | doctor]");
            std::process::exit(2);
        }
        None => println!("usage: sutura [--version | doctor]"),
    }
}

/// Reports what the binary can see. Exists so the release artifact has something
/// meaningful to run in CI beyond `--version`.
fn doctor() {
    println!("sutura {}", env!("CARGO_PKG_VERSION"));
    println!(
        "  profile      : {}",
        if cfg!(debug_assertions) { "debug" } else { "release" }
    );
    println!("  target       : {}", std::env::consts::ARCH);
    println!("  allocator    : {ALLOCATOR_NAME}");
    // Proves the redaction invariant holds in the shipped binary, not only under test.
    let probe = Secret::new("must-not-appear");
    println!("  redaction    : {probe:?}");
}
