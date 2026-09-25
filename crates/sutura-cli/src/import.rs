//! `sutura import <kind> <dir> <out>`: converts a foreign catalog project into markdown catalog
//! documents for review, plus a refusal report naming everything that did not convert.
//!
//! **One kind today - `wren`.** A word this match does not recognise is a usage refusal rather than
//! a silent no-op, so a second importer earns its own arm here instead of a dispatch nobody reads
//! to find out what is supported.
//!
//! `<out>` becomes a directory of `kind:`-tagged markdown documents, loadable by
//! [`sutura_catalog_local::LocalCatalog`] exactly as a hand-written catalog is - this command
//! writes the same document shape a person would type, not a second format `sutura` reads
//! differently. **Nothing here is certified by running it.** The output is authored text for a
//! person to read and commit; `sutura catalog <out>` (or a plain read of the files) is how a
//! reviewer decides whether it says what the source system says.

mod wren;

use std::path::Path;
use std::process::ExitCode;

use crate::commands::{arg, report};

pub(crate) fn import(args: &[String]) -> ExitCode {
    report(run(args))
}

/// The command's own logic, apart from turning its result into an [`ExitCode`] - so a refusal can
/// be asserted on directly rather than through the exit code [`report`] erases it into.
fn run(args: &[String]) -> Result<(), String> {
    let usage = "import <kind> <catalog-dir> <out-dir>";
    let kind = arg(args, 0, "kind", usage)?;
    let source = arg(args, 1, "catalog-dir", usage)?;
    let destination = arg(args, 2, "out-dir", usage)?;
    match kind.as_str() {
        "wren" => {
            let summary = wren::import(Path::new(&source), Path::new(&destination))?;
            println!(
                "mapped {} models, {} relationships, {} metrics into {destination}",
                summary.models, summary.relationships, summary.metrics
            );
            println!("refused {} items - see {destination}/report.txt", summary.refusals);
            Ok(())
        }
        other => Err(format!(
            "no importer for {other:?} - only \"wren\" is supported\nusage: sutura {usage}"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::run;

    #[test]
    fn an_unrecognised_kind_is_refused_by_name() {
        let args: Vec<String> = ["okf", "a", "b"].into_iter().map(String::from).collect();
        let error = run(&args).expect_err("an unknown importer kind must be refused");
        assert!(error.contains("okf"), "{error}");
    }
}
