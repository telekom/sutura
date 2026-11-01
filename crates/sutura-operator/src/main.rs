#![forbid(unsafe_code)]
//! `sutura-operator crd` prints the `SuturaWorkerPool` definition for the configured API group, as
//! YAML, for the chart to install.

use std::error::Error;
use std::process::ExitCode;

use sutura_config::{OperatorSettings, Sources, config_dir_from_process, environment_from_process};

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    if !matches!((args.next().as_deref(), args.next()), (Some("crd"), None)) {
        eprintln!("usage: sutura-operator crd");
        return ExitCode::from(2);
    }
    match crd() {
        Ok(yaml) => {
            print!("{yaml}");
            ExitCode::SUCCESS
        }
        Err(why) => {
            eprintln!("sutura-operator: {}", render(why.as_ref()));
            ExitCode::FAILURE
        }
    }
}

/// Whatever stopped the definition rendering: a setting, or the schema.
type Failure = Box<dyn Error>;

fn crd() -> Result<String, Failure> {
    let sources = Sources::from_process_environment(environment_from_process()?, config_dir_from_process());
    let settings = OperatorSettings::load(&sources)?;
    Ok(serde_norway::to_string(&sutura_operator::definition(settings.api_group())?)?)
}

/// `error` and each of its causes, one per line.
fn render(error: &dyn Error) -> String {
    std::iter::successors(Some(error), |&cause| cause.source())
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n  caused by: ")
}
