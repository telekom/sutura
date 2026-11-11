use std::time::{Duration, Instant};

#[derive(Debug, thiserror::Error)]
enum ProbeError {
    #[error("fixture input is missing")]
    Environment(#[from] std::env::VarError),
    #[error("fixture authority cannot be read")]
    File(#[from] std::io::Error),
    #[error("driver setup failed")]
    Driver(#[from] oracledb::Error),
    #[error("fixture port is invalid")]
    Port(#[from] std::num::ParseIntError),
}

fn verdict(error: &oracledb::Error) -> String {
    match error.kind() {
        oracledb::ErrorKind::CallTimeoutExceeded => String::from("call_timeout"),
        oracledb::ErrorKind::UnableToRecover => String::from("unable_to_recover"),
        oracledb::ErrorKind::DbError(error) => format!("ora_{:05}", error.code()),
        _ => String::from("other_driver_error"),
    }
}

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), ProbeError> {
    let mode = std::env::var("SUTURA_ORACLE_RAW_MODE")?;
    let port: u16 = std::env::var("SUTURA_ORACLE_RAW_PORT")?.parse()?;
    let service = std::env::var("SUTURA_ORACLE_RAW_SERVICE")?;
    let password = std::env::var("SUTURA_ORACLE_RAW_PASSWORD")?;
    let address = if mode == "native" {
        format!("127.0.0.1:{port}/{service}")
    } else {
        format!("tcps://127.0.0.1:{port}/{service}")
    };
    let mut config = oracledb::Config::default()
        .set_connect_string(&address)?
        .set_transport_connect_timeout(Some(Duration::from_secs(5)))
        .set_credentials("RAWDEADLINE", &password);
    if mode == "native" {
        config = config.set_native_encryption(true);
    } else {
        let anchors = std::fs::read_to_string(std::env::var("SUTURA_ORACLE_RAW_CA")?)?;
        config = config.set_trust_anchors_pem(&anchors)?;
    }
    println!("phase=connect");
    let connected = Instant::now();
    let connection = oracledb::connect(config)?;
    println!("connect_ms={}", connected.elapsed().as_millis());
    connection.set_module("sutura.raw-deadline-probe")?;
    connection.set_call_timeout(Some(Duration::from_secs(3)))?;
    connection.execute("SET TRANSACTION READ ONLY", &[])?;
    connection.set_action("long-query")?;
    println!("phase=query");
    let started = Instant::now();
    let result = connection.query_row(
        "SELECT /* sutura_raw_deadline_probe */ SUM(ORA_HASH(LEVEL)) FROM dual CONNECT BY LEVEL <= 1000000000",
        &[],
    );
    println!("query_ms={}", started.elapsed().as_millis());
    println!("query_result={}", result.as_ref().map_or_else(verdict, |_| String::from("completed")));
    println!("phase=rollback");
    let rollback = Instant::now();
    let result = connection.rollback();
    println!("rollback_ms={}", rollback.elapsed().as_millis());
    println!("rollback_result={}", result.as_ref().map_or_else(verdict, |()| String::from("ok")));
    println!("phase=drop");
    let cleanup = Instant::now();
    drop(connection);
    println!("cleanup_ms={}", cleanup.elapsed().as_millis());
    println!("client_finished=true");
    Ok(())
}
