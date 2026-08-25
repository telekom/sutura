//! The sutura binary.
//!
//! The composition root, and nothing else. Every command lives in [`commands`], which is the one
//! place an adapter is named; this file holds the allocator, the command table and `doctor`.
//!
//! `Result<_, String>` is used freely below the surface here. The boundary gate exempts a binary
//! on purpose: the audience for these errors is a person reading stderr, not code matching on a
//! variant.

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

mod commands;

use std::process::ExitCode;

use sutura_domain::identity::Secret;

/// A command: the name, the `--help` line, and the code it runs.
///
/// The handler is IN the table, which is the same shape `xtask/src/main.rs` and `dev/src/main.rs`
/// use and for the same reason: dispatch and `--help` are derived from one list, so they cannot
/// describe different sets of commands. A unit test asserts the names are unique and described.
/// What a command does. Named rather than written inline: the complexity threshold in
/// `clippy.toml` catches a bare `fn(&[String]) -> ExitCode` in a struct field, and a name says what
/// it is.
type Action = fn(&[String]) -> ExitCode;

struct Cmd {
    name: &'static str,
    description: &'static str,
    run: Action,
}

const COMMANDS: &[Cmd] = &[
    Cmd {
        name: "doctor",
        description: "what this binary was built with",
        run: |_args| {
            doctor();
            ExitCode::SUCCESS
        },
    },
    Cmd {
        name: "catalog",
        description: "<catalog-dir> - the metrics this catalog defines, with its digest",
        run: commands::catalog,
    },
    Cmd {
        name: "describe",
        description: "<catalog-dir> <metric> - one metric in full, prose included",
        run: commands::describe,
    },
    Cmd {
        name: "compile",
        description: "<catalog-dir> <question.yaml> [dialect] - the statement, no data system",
        run: commands::compile,
    },
    Cmd {
        name: "query",
        description: "<catalog-dir> <question.yaml> <data-dir> - check the anchors, then answer",
        run: commands::query,
    },
];

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rest = args.split_first().map(|(_, rest)| rest).unwrap_or_default();

    match args.first().map(String::as_str) {
        Some("--version" | "-V") => {
            println!("sutura {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Some("--help" | "-h" | "help") => {
            usage();
            ExitCode::SUCCESS
        }
        None => {
            usage();
            ExitCode::from(2)
        }
        Some(requested) => COMMANDS.iter().find(|c| c.name == requested).map_or_else(
            || {
                eprintln!("sutura: unknown command `{requested}`");
                usage();
                ExitCode::from(2)
            },
            |cmd| (cmd.run)(rest),
        ),
    }
}

fn usage() {
    eprintln!("usage: sutura <command> [args]");
    eprintln!("       sutura --version");
    for cmd in COMMANDS {
        eprintln!("  {:<9} {}", cmd.name, cmd.description);
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
    println!(
        "  data systems : {}",
        if cfg!(feature = "exec-duckdb") {
            "duckdb"
        } else {
            "none compiled in"
        }
    );
    // Proves the redaction invariant holds in the shipped binary, not only under test.
    let probe = Secret::new("must-not-appear");
    println!("  redaction    : {probe:?}");
}

#[cfg(test)]
mod tests {
    use super::COMMANDS;

    #[test]
    fn command_names_are_unique_and_described() {
        // The table is the only source for both dispatch and `--help`, so this is what stops a
        // command from being listed twice or listed with no explanation of what it wants.
        let mut names: Vec<&str> = COMMANDS.iter().map(|c| c.name).collect();
        let count = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), count, "two commands share a name");
        for cmd in COMMANDS {
            assert!(!cmd.description.is_empty(), "{} has no help line", cmd.name);
        }
    }

    #[test]
    fn query_is_listed_whether_or_not_its_adapter_is_compiled() {
        // Both builds are real: the cross artifacts ship without a data-system adapter. A command
        // that vanished from `--help` in one of them would read as a packaging mistake, so the
        // absent case is a command that explains itself instead.
        assert!(COMMANDS.iter().any(|c| c.name == "query"));
    }
}
