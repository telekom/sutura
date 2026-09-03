//! The sutura binary.
//!
//! The composition root, and nothing else. Every command lives in [`commands`] and every adapter is
//! named in [`sources`]; this file holds the allocator, the command table and `doctor`.
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
mod mcp;
mod sources;

use std::process::ExitCode;

use sutura_domain::identity::Secret;

/// A command: the name, the arguments it takes, what it is for, and the code it runs.
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
    /// The arguments, in the spelling `--help` prints them: `<required>` and `[optional]`, one
    /// whitespace-separated token each, empty for a command that takes none.
    ///
    /// **Split out of `description` so the arity is derived rather than declared twice.**
    /// [`Cmd::max_args`] counts the tokens here, so a command that grows an argument widens what
    /// [`vet`] accepts in the same edit that documents it - there is no second number to forget.
    args: &'static str,
    /// What the command is for.
    description: &'static str,
    run: Action,
}

impl Cmd {
    /// How many arguments this command accepts, counted from [`Cmd::args`].
    ///
    /// Required and optional alike: the command's own handler decides which of them it cannot do
    /// without, and says so by name. This is only the ceiling.
    fn max_args(&self) -> usize {
        self.args.split_whitespace().count()
    }

    /// `usage: sutura <name> <args>`, and no trailing space for a command that takes none.
    fn usage_line(&self) -> String {
        if self.args.is_empty() {
            format!("usage: sutura {}", self.name)
        } else {
            format!("usage: sutura {} {}", self.name, self.args)
        }
    }

    /// The line the top-level listing prints: the argument spec, then what the command is for.
    ///
    /// Reassembled from the two fields, so the listing reads exactly as it did when they were one.
    fn help_line(&self) -> String {
        if self.args.is_empty() {
            String::from(self.description)
        } else {
            format!("{} - {}", self.args, self.description)
        }
    }
}

const COMMANDS: &[Cmd] = &[
    Cmd {
        name: "doctor",
        args: "",
        description: "what this binary was built with",
        run: |_args| {
            doctor();
            ExitCode::SUCCESS
        },
    },
    Cmd {
        name: "catalog",
        args: "<catalog-dir>",
        description: "the metrics this catalog defines, with its digest",
        run: commands::catalog,
    },
    Cmd {
        name: "describe",
        args: "<catalog-dir> <metric>",
        description: "one metric in full, prose included",
        run: commands::describe,
    },
    Cmd {
        name: "prompt",
        args: "<catalog-dir> [config-dir]",
        description: "the system prompt to give an agent",
        run: commands::prompt,
    },
    Cmd {
        name: "compile",
        args: "<catalog-dir> <question.yaml> [dialect]",
        description: "the statement, no data system",
        run: commands::compile,
    },
    Cmd {
        name: "query",
        args: "<catalog-dir> <question.yaml> [data-dir]",
        description: "check the anchors, then answer",
        run: commands::query,
    },
    Cmd {
        name: "mcp",
        args: "<catalog-dir> [data-dir]",
        description: "serve the agent surface over stdin/stdout",
        run: mcp::mcp,
    },
];

/// What an argument vector asked for.
///
/// The point of the type is that [`requested`] is **pure**: every decision about argv returns a value
/// rather than printing one, so a table-driven test asserts the exit code *and* the message with no
/// process to spawn and no stream to capture. [`dispatch`] is the only place that prints, and it is
/// three lines.
enum Requested<'a> {
    /// The caller asked for this. Standard output, exit `0`.
    Print(String),
    /// The argument vector is not a request this binary can serve. Standard error, exit `2`.
    ///
    /// `2` rather than `1`, and the distinction is the reason this variant exists separately: `1` is
    /// what a command returns when it ran and could not do the job, so a caller - a shell script, a
    /// CI step - can tell "you typed it wrong" from "it did not work".
    Usage(String),
    /// Run this command with these arguments.
    Run(&'a Cmd, &'a [String]),
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    dispatch(&args)
}

/// Reads an argument vector and does what it asked for.
///
/// Extracted from `main` so the argv layer is reachable from a test: `main` now reads the process
/// arguments and nothing else, which is the whole of what a test cannot call.
fn dispatch(args: &[String]) -> ExitCode {
    match requested(args) {
        Requested::Print(text) => {
            println!("{text}");
            ExitCode::SUCCESS
        }
        Requested::Usage(text) => {
            eprintln!("{text}");
            ExitCode::from(2)
        }
        Requested::Run(cmd, rest) => (cmd.run)(rest),
    }
}

/// What this argument vector asks for. Pure.
fn requested(args: &[String]) -> Requested<'_> {
    let rest = args.split_first().map(|(_, rest)| rest).unwrap_or_default();

    match args.first().map(String::as_str) {
        Some("--version" | "-V") => Requested::Print(format!("sutura {}", env!("CARGO_PKG_VERSION"))),
        Some("--help" | "-h" | "help") => Requested::Print(usage()),
        None => Requested::Usage(usage()),
        Some(name) => COMMANDS.iter().find(|c| c.name == name).map_or_else(
            || Requested::Usage(format!("sutura: unknown command `{name}`\n{}", usage())),
            |cmd| vet(cmd, rest),
        ),
    }
}

/// One command's arguments, checked before the command sees them.
///
/// Two things happen here and neither was any command's business.
///
/// **`--help` is answered rather than passed down.** `sutura compile --help` used to reach
/// `commands::compile`, which took `--help` as the catalog directory and then failed on the argument
/// after it - so a request that succeeded printed `sutura: missing <question.yaml>` and exited `1`.
/// First position only, because that is the only position in which it is unambiguous: everything else
/// is a value a command was given.
///
/// **An argument the command does not declare is refused rather than dropped.** `sutura catalog <dir>
/// bogus extra` exited `0`, printed the listing, and said nothing about either extra word. That is the
/// worse of the two bugs: silently ignoring a trailing argument is how a caller comes to believe a
/// flag took effect, and the belief survives because the command succeeded. The message names the
/// first unexpected argument rather than counting them, because the name is what the caller has to
/// look at.
fn vet<'a>(cmd: &'a Cmd, args: &'a [String]) -> Requested<'a> {
    if matches!(args.first().map(String::as_str), Some("--help" | "-h")) {
        return Requested::Print(format!("{}\n  {}", cmd.usage_line(), cmd.description));
    }
    args.get(cmd.max_args()).map_or_else(
        || Requested::Run(cmd, args),
        |extra| {
            Requested::Usage(format!(
                "sutura: `{extra}` is not an argument of `{}`\n{}",
                cmd.name,
                cmd.usage_line()
            ))
        },
    )
}

/// The top-level listing, as text.
///
/// A `String` rather than a run of `eprintln!` calls, because [`requested`] has to be able to hand it
/// back for a test to read - and because a `--help` that succeeds belongs on standard output while the
/// same text for a bad invocation belongs on standard error. Which stream is [`dispatch`]'s decision.
fn usage() -> String {
    let listed: Vec<String> = COMMANDS
        .iter()
        .map(|cmd| format!("  {:<9} {}", cmd.name, cmd.help_line()))
        .collect();
    format!(
        "usage: sutura <command> [args]\n       sutura --version\n{}",
        listed.join("\n")
    )
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
    println!("  engine       : datafusion (arrow, in process)");
    println!("  data systems : none - this build reads files, and pushes down to nothing");
    // Proves the redaction invariant holds in the shipped binary, not only under test.
    let probe = Secret::new("must-not-appear");
    println!("  redaction    : {probe:?}");
}

#[cfg(test)]
mod tests {
    use super::{COMMANDS, Requested, requested};

    /// One argument vector's outcome: the exit code it earns, and what the caller is told.
    ///
    /// `None` for a command that runs, because its exit code is the command's and not argv's; the
    /// text is then the routing decision - the command's name and the arguments it was handed - so a
    /// row can assert an argument reached the right place instead of being swallowed.
    fn outcome(argv: &[&str]) -> (Option<u8>, String) {
        let owned: Vec<String> = argv.iter().copied().map(String::from).collect();
        match requested(&owned) {
            Requested::Print(text) => (Some(0), text),
            Requested::Usage(text) => (Some(2), text),
            Requested::Run(cmd, rest) => (None, format!("{} {}", cmd.name, rest.join(" "))),
        }
    }

    #[test]
    fn argv_decides_one_way_and_says_which() {
        // There was NOTHING testing the argument layer, which is how both of the bugs below survived
        // in a shipped binary. A table rather than a test per case, because what is being asserted is
        // one property over the whole vector space: every argument vector reaches exactly one of
        // "print this", "you typed it wrong", or "run this command with these arguments", and says so.
        struct Case {
            argv: &'static [&'static str],
            /// `Some(code)` for a vector argv answers itself; `None` for one it routes.
            code: Option<u8>,
            /// A fragment of what the caller is told - or, for a routed vector, the routing.
            says: &'static str,
        }

        const CASES: &[Case] = &[
            // THE FIRST BUG. `--help` was passed straight to the handler, which took it as the
            // catalog directory and failed on the argument AFTER it: `sutura: missing
            // <question.yaml>`, exit 1, for a request that succeeded. `prompt --help` was worse - it
            // reached the settings loader and failed with `the catalog root --help is not a directory`.
            Case {
                argv: &["compile", "--help"],
                code: Some(0),
                says: "usage: sutura compile <catalog-dir> <question.yaml> [dialect]",
            },
            Case {
                argv: &["prompt", "-h"],
                code: Some(0),
                says: "usage: sutura prompt <catalog-dir> [config-dir]",
            },
            // A command that takes no arguments still explains itself, and still says so on stdout
            // with a zero exit.
            Case {
                argv: &["doctor", "--help"],
                code: Some(0),
                says: "usage: sutura doctor",
            },
            // THE SECOND BUG, and the one that matters more. This exited 0, printed the catalog
            // listing, and said nothing about either extra word - which is how a caller comes to
            // believe a flag took effect.
            Case {
                argv: &["catalog", "examples/single-player/catalog", "bogus", "extra"],
                code: Some(2),
                says: "`bogus` is not an argument of `catalog`",
            },
            // The same for a command that declares none at all, where anything is one too many.
            Case {
                argv: &["doctor", "--json"],
                code: Some(2),
                says: "`--json` is not an argument of `doctor`",
            },
            // The optional argument is not the ceiling: `prompt` takes two, so the third is the
            // first unexpected one. A gate that counted only the required arguments would refuse
            // the config directory this command exists to read.
            Case {
                argv: &["prompt", "cat", "conf"],
                code: None,
                says: "prompt cat conf",
            },
            Case {
                argv: &["prompt", "cat", "conf", "spare"],
                code: Some(2),
                says: "`spare` is not an argument of `prompt`",
            },
            // What already worked, so the two checks above are not asserted only by their failures:
            // a well formed vector routes to its command with its arguments intact.
            Case {
                argv: &["query", "cat", "q.yaml", "data"],
                code: None,
                says: "query cat q.yaml data",
            },
            // The data directory is OPTIONAL since the source registry landed - a deployment that
            // declares its data system has already said where the data is - so a vector without one
            // has to ROUTE rather than be refused here. `commands::query` is what then says which of
            // the two declarations was missing; argv's job is only to stop counting it as required.
            Case {
                argv: &["query", "cat", "q.yaml"],
                code: None,
                says: "query cat q.yaml",
            },
            Case {
                argv: &["mcp", "cat"],
                code: None,
                says: "mcp cat",
            },
            // `--help` in a later position is a value, not a request. There is no position but the
            // first in which it is unambiguous, and a command that wanted a file called `--help`
            // deserves the refusal it gets from its own handler rather than a usage message here.
            Case {
                argv: &["describe", "cat", "--help"],
                code: None,
                says: "describe cat --help",
            },
            // The top-level cases, unchanged in behaviour and now covered: nothing at all is a usage
            // error, and an unknown command names itself before the listing.
            Case {
                argv: &[],
                code: Some(2),
                says: "usage: sutura <command> [args]",
            },
            Case {
                argv: &["frobnicate"],
                code: Some(2),
                says: "unknown command `frobnicate`",
            },
            Case {
                argv: &["--version"],
                code: Some(0),
                says: "sutura ",
            },
        ];

        for case in CASES {
            let (code, says) = outcome(case.argv);
            assert_eq!(code, case.code, "{:?} earned the wrong exit code: {says}", case.argv);
            assert!(
                says.contains(case.says),
                "{:?} was told {says:?}, which does not carry {:?}",
                case.argv,
                case.says
            );
        }
    }

    #[test]
    fn the_argument_ceiling_is_read_off_the_line_that_documents_it() {
        // Why `args` is a field and not a number: the arity a command enforces and the arguments its
        // `--help` advertises are the same string, so they cannot drift. This fails if somebody adds
        // an argument to a handler and to the help line without the two agreeing - which is the shape
        // the old single `description` field made impossible to check at all.
        for cmd in COMMANDS {
            let spelt = cmd.args.split_whitespace().count();
            assert_eq!(cmd.max_args(), spelt, "{} counts its own arguments wrongly", cmd.name);
            for token in cmd.args.split_whitespace() {
                assert!(
                    (token.starts_with('<') && token.ends_with('>')) || (token.starts_with('[') && token.ends_with(']')),
                    "{} spells an argument as {token}, which the count cannot read",
                    cmd.name
                );
            }
        }
    }

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
    fn query_is_listed_identically_whichever_adapters_were_compiled() {
        // Both builds are real: a published artifact ships without a networked adapter. A command
        // that vanished from `--help` in one of them would read as a packaging mistake, so the
        // absent case is a command that explains itself instead.
        //
        // **It asserts the ARGUMENT SPEC as well as the name, which is a review correction:** the
        // old body checked only that `COMMANDS` contains `"query"`, so it would have passed
        // identically if no feature existed at all and it said nothing about what the command takes.
        // The spec is what `vet` derives its arity from and what `--help` prints, and it must not
        // vary by build - a binary that accepted a different number of arguments depending on which
        // adapters were linked is a worse surprise than a missing command.
        let query = COMMANDS
            .iter()
            .find(|c| c.name == "query")
            .expect("query is listed in every build");
        assert_eq!(
            query.args, "<catalog-dir> <question.yaml> [data-dir]",
            "the argument spec is the same on every build, and it is what `vet` counts"
        );
        let mcp = COMMANDS
            .iter()
            .find(|c| c.name == "mcp")
            .expect("mcp is listed in every build");
        assert_eq!(mcp.args, "<catalog-dir> [data-dir]");
    }
}
