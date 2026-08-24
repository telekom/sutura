//! The sutura binary.
//!
//! M0 deliberately ships almost nothing: its purpose is to prove the machinery - that the
//! toolchain resolves, the workspace compiles, the gates run, and a release image builds
//! and runs. Behaviour arrives with the milestone that needs it.

use sutura_domain::identity::Secret;

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
    // Proves the redaction invariant holds in the shipped binary, not only under test.
    let probe = Secret::new("must-not-appear");
    println!("  redaction    : {probe:?}");
}
