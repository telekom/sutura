#![forbid(unsafe_code)]
//! `sutura-operator crd` prints the `SuturaWorkerPool` definition for the configured API group, as
//! YAML, for the chart to install. `sutura-operator run` sizes every pool in its namespace, every
//! [`POLL`], until it is stopped. `sutura-operator --version` prints its version.

use std::collections::BTreeMap;
use std::convert::Infallible;
use std::error::Error;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use sutura_config::{OperatorSettings, Sources, config_dir_from_process, environment_from_process};
use sutura_http_client::{DEFAULT_MAX_RESPONSE_BYTES, DEFAULT_TIMEOUT_SECONDS, ReadBounds};
use sutura_operator::InCluster;

/// How often every pool is sized.
const POLL: Duration = Duration::from_secs(15);

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let outcome = match (args.next().as_deref(), args.next()) {
        (Some("crd"), None) => crd().map(|yaml| print!("{yaml}")),
        (Some("run"), None) => run().map(|never| match never {}),
        (Some("--version"), None) => {
            println!("sutura-operator {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        _ => {
            eprintln!("usage: sutura-operator crd | run | --version");
            return ExitCode::from(2);
        }
    };
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(why) => {
            eprintln!("sutura-operator: {}", render(why.as_ref()));
            ExitCode::FAILURE
        }
    }
}

/// Whatever stopped the operator: a setting, the schema, or the cluster's pools not being listed.
type Failure = Box<dyn Error>;

fn settings() -> Result<OperatorSettings, Failure> {
    let sources = Sources::from_process_environment(environment_from_process()?, config_dir_from_process());
    Ok(OperatorSettings::load(&sources)?)
}

fn crd() -> Result<String, Failure> {
    let settings = settings()?;
    Ok(serde_norway::to_string(&sutura_operator::definition(settings.api_group())?)?)
}

/// Sizes every pool each [`POLL`]. A pool the cluster fails for is logged and tried again next
/// pass; the pools not being listed at all stops the operator, so its pod restarts.
fn run() -> Result<Infallible, Failure> {
    let settings = settings()?;
    let bounds = ReadBounds::parse(DEFAULT_TIMEOUT_SECONDS, DEFAULT_MAX_RESPONSE_BYTES)?;
    let mut cluster = InCluster::from_pod(
        settings.api_group(),
        settings.metrics_token(),
        settings.node_authority(),
        settings.workers(),
        settings.serving(),
        bounds,
    )?;
    let mut surplus = BTreeMap::new();
    loop {
        for (pool, why) in sutura_operator::tick(&mut cluster, &mut surplus, Instant::now())? {
            eprintln!("sutura-operator: pool `{pool}`: {}", render(&why));
        }
        std::thread::sleep(POLL);
    }
}

/// `error` and each of its causes, one per line.
fn render(error: &dyn Error) -> String {
    std::iter::successors(Some(error), |&cause| cause.source())
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n  caused by: ")
}
