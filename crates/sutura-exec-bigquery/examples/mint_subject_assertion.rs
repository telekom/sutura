//! Mints one Google-issued OIDC ID token from a service-account key, for
//! `crates/sutura-exec-bigquery/tests/exchanged_identity.rs` to present as a subject token.
//!
//! **CI-only tooling, not part of the shipped adapter.** It exists so
//! `SUTURA_BQ_PRINCIPAL_A_ASSERTION_FILE`/`_B_` can be produced at job time from the same
//! per-principal keys `bq-test` already holds (`SVC_SUTURUA_BQ_PRINCIPAL_A`/`_B`) rather than as a
//! new long-lived secret - telekom/sutura#376. It is an `examples/` binary rather than a shipped
//! `[[bin]]` for the same reason `nix/shipped.nix` keeps the `bigquery` feature default-off: nothing
//! published carries this.
//!
//! **Never prints the token.** Three arguments, all paths and public strings, never the credential
//! material: the service-account key file, the target audience, and the file to write the minted ID
//! token to. The caller (a workflow step) is responsible for `umask 077` before invoking this and for
//! never passing the output through `${{ }}` - see `.github/workflows/bigquery-exchanged-identity.yml`.
//!
//! ```text
//! cargo run -p sutura-exec-bigquery --example mint_subject_assertion --features wire --profile ci -- \
//!   "$KEY_FILE" "$TARGET_AUDIENCE" "$OUT_FILE"
//! ```

use std::io::Write as _;
use std::path::PathBuf;

use sutura_exec_bigquery::wire::credential::{Credential, CredentialFile};
use sutura_exec_bigquery::wire::{BytesBilledCeiling, CallDeadline, JobBounds, QueryDeadline, WireAgent};

/// Names what failed and exits - never `.expect()`, which this crate's `clippy.toml` denies
/// outside `#[cfg(test)]` (`allow-expect-in-tests` is scoped there and nowhere else).
///
/// `clippy::exit` only exempts a call inside `fn main` itself; every other caller here reaches
/// this one function instead of repeating an inline `eprintln!` + `exit` pair at each of the six
/// fallible steps below.
#[expect(
    clippy::exit,
    reason = "the one exit point every fallible step in this binary's main funnels through"
)]
fn die(what: &str) -> ! {
    eprintln!("mint_subject_assertion: {what}");
    std::process::exit(1);
}

/// Both places this binary reads the minted token's bytes.
///
/// **The one function that may call `Secret::expose_secret`, and it says why.** `clippy.toml`
/// disallows that call everywhere else in the workspace so a new exposure is a visible diff; this
/// is that visible diff, and both callers below write the bytes straight to the file the caller
/// named, never to a log or a caller's own `Display`.
#[expect(
    clippy::disallowed_methods,
    reason = "the minted ID token has to leave the Secret to be written to the output file; the \
              caller writes only the byte count, never the token, to stdout"
)]
fn write_token(out: &std::path::Path, token: &sutura_domain::identity::Secret) {
    let mut file = std::fs::File::create(out).unwrap_or_else(|cause| {
        die(&format!("{} could not be opened for writing: {cause}", out.display()));
    });
    if let Err(cause) = file.write_all(token.expose_secret().as_bytes()) {
        die(&format!("{} could not be written: {cause}", out.display()));
    }
    // The shape, never the content - the same banner `tests/support/support.rs` prints for the CI
    // credential.
    println!(
        "mint_subject_assertion: {} bytes written to {}",
        token.expose_secret().len(),
        out.display()
    );
}

fn main() {
    let mut args = std::env::args_os().skip(1);
    let (Some(key), Some(target_audience), Some(out)) = (args.next(), args.next(), args.next()) else {
        eprintln!("usage: mint_subject_assertion <key-file> <target-audience> <out-file>");
        std::process::exit(2);
    };
    let target_audience = target_audience.to_string_lossy().into_owned();
    let out = PathBuf::from(out);

    // Thirty seconds and a one-byte ceiling: this call submits no query, so the byte ceiling is
    // never consulted - `WireAgent::pinned` still requires a `JobBounds` because the pinned agent
    // is shared with the crate's job transport, and a second constructor for "an agent with no
    // budget" would be a second way to build the client this crate's own header argues against.
    let deadline =
        QueryDeadline::parse(30).unwrap_or_else(|cause| die(&format!("a thirty-second deadline does not parse: {cause}")));
    let ceiling =
        BytesBilledCeiling::parse(1).unwrap_or_else(|cause| die(&format!("a one-byte ceiling does not parse: {cause}")));
    let bounds = JobBounds::of(deadline, ceiling);
    let agent = WireAgent::pinned(bounds);
    let credential = Credential::read(&CredentialFile::at(key), agent)
        .unwrap_or_else(|cause| die(&format!("the key file could not be read as a credential: {cause}")));

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_else(|cause| die(&format!("this process could not read the time: {cause}")))
        .as_secs();
    let within = CallDeadline::opened(bounds.deadline());

    // `TokenUnavailable`'s `Display` carries only the class the endpoint returned (status and its
    // short `named` code) - never the endpoint's own free text - the same discipline
    // `exchanged_identity.rs`'s `refusal_shape` holds for the exchange leg beside this one.
    let token = credential
        .mint_id_token(&target_audience, now, within)
        .unwrap_or_else(|cause| die(&format!("the ID token could not be minted: {cause}")));

    write_token(&out, &token);
}
