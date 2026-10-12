//! A value a source binds into its statement, looked for in every line the process writes at
//! `trace`.
//!
//! The `clickhouse` source sends each bound value as a field of the request URL's query. The cell
//! serves that source against a loopback stand-in for its HTTP interface, declares one more region,
//! a canary, asks for it, and looks for it in every line the process wrote to either stream. An
//! absence proves nothing unless the value was sent and the request was logged, so the cell first
//! shows the question was answered, the stand-in received the canary, and the HTTP client's own
//! request line was captured.

use std::io::{BufRead as _, Read as _, Write as _};

use sutura_domain::warehouse::cardinality::{DISTINCT_LABEL, ROWS_LABEL};

use crate::harness::{LOCAL_SOURCE, LOOPBACK, SINGLE_USER, example_root, settings_over};

/// The subscriber reads `RUST_LOG` before the settings' filter, so this makes every line `trace`.
const TRACE: [(&str, &str); 1] = [("RUST_LOG", "trace")];

/// A `trace` event as the bunyan formatter writes its level.
const TRACE_LEVEL: &str = r#""level":10"#;

/// A region no row holds, distinct enough to find in any text.
const CANARY: &str = "3f9a0c71d2e84b56a1c07e93";

/// The regions `recurring_revenue` declares in the example.
const REGIONS: &str = "values: [central, east, north, south, west]";

/// One request's line and body, read off `stream`.
fn read_request(stream: &std::net::TcpStream) -> std::io::Result<(String, String)> {
    let mut reader = std::io::BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let mut length = 0;
    loop {
        let mut header = String::new();
        reader.read_line(&mut header)?;
        if header.trim_end().is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':')
            && name.eq_ignore_ascii_case("content-length")
        {
            length = value.trim().parse().unwrap_or(0);
        }
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    Ok((line, String::from_utf8_lossy(&body).into_owned()))
}

/// A loopback stand-in for the `clickhouse` HTTP interface: a declared-key probe is answered as a
/// unique key and every other statement with the June row `recurring_revenue`'s anchor certifies.
/// Each request line it read is sent back.
fn stand_in() -> (std::net::SocketAddr, std::sync::mpsc::Receiver<String>) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("a loopback port is free");
    let address = listener.local_addr().expect("a bound listener has a local address");
    let (sender, received) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for mut stream in listener.incoming().map_while(Result::ok) {
            let Ok((line, body)) = read_request(&stream) else {
                continue;
            };
            if sender.send(line).is_err() {
                return;
            }
            let answer = if body.contains(ROWS_LABEL) {
                format!("[\"{ROWS_LABEL}\",\"{DISTINCT_LABEL}\"]\n[\"UInt64\",\"UInt64\"]\n[\"1\",\"1\"]\n")
            } else {
                String::from("[\"month\",\"recurring_revenue\"]\n[\"Date\",\"Int64\"]\n[\"2026-06-01\",202121]\n")
            };
            let reply = format!(
                "HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{answer}",
                answer.len()
            );
            drop(stream.write_all(reply.as_bytes()));
        }
    });
    (address, received)
}

/// `text` without its `anchor:` block: the stand-in answers one anchor and no other.
fn without_the_anchor(text: &str) -> String {
    let mut inside = false;
    let kept: Vec<&str> = text
        .lines()
        .filter(|line| {
            inside = *line == "anchor:" || (inside && line.starts_with(' '));
            !inside
        })
        .collect();
    kept.join("\n")
}

/// The example catalog, copied to `root/catalog` with `recurring_revenue`'s anchor as its only one
/// and [`CANARY`] as one more region.
fn catalog_with_the_canary(root: &std::path::Path) -> std::path::PathBuf {
    let catalog = root.join("catalog");
    let copied = std::process::Command::new("cp")
        .arg("-R")
        .arg(example_root().join("catalog"))
        .arg(&catalog)
        .status()
        .expect("cp runs");
    assert!(copied.success(), "the example catalog did not copy");
    let metrics = std::fs::read_dir(catalog.join("metrics")).expect("the copied metrics directory reads");
    let paths = metrics.map(|entry| entry.expect("a metrics directory entry reads").path());
    for metric in paths.filter(|path| !path.ends_with("recurring_revenue.md")) {
        let text = std::fs::read_to_string(&metric).expect("a copied metric reads");
        std::fs::write(&metric, without_the_anchor(&text)).expect("a derived metric writes");
    }
    let metric = catalog.join("metrics").join("recurring_revenue.md");
    let text = std::fs::read_to_string(&metric).expect("the copied metric reads");
    assert!(text.contains(REGIONS), "{} no longer declares `{REGIONS}`", metric.display());
    let declared = text.replace(REGIONS, &format!("values: [central, east, north, south, west, {CANARY}]"));
    std::fs::write(&metric, declared).expect("the derived metric writes");
    catalog
}

#[test]
fn a_value_a_clickhouse_statement_binds_reaches_no_log_line() {
    let (endpoint, received) = stand_in();
    let case = "bound-value-clickhouse";
    let root = crate::harness::derived_beside(&crate::harness::config_path(case));
    drop(std::fs::remove_dir_all(&root));
    std::fs::create_dir_all(&root).expect("the derived directory is creatable");
    let catalog = catalog_with_the_canary(&root);
    let password_file = root.join("password");
    std::fs::write(&password_file, "unchecked\n").expect("the password file writes");
    let entry = format!(
        "  {LOCAL_SOURCE}:\n    kind: \"clickhouse\"\n    host: \"{}\"\n    port: {}\n    user: \"sutura\"\n    \
         password_file: \"{}\"\n    transport_mode: \"plaintext\"\n    posture: \"shared-service-user\"\n",
        endpoint.ip(),
        endpoint.port(),
        password_file.display()
    );
    let settings = settings_over(&catalog, &example_root().join("data"), LOOPBACK, SINGLE_USER, &entry);
    let question = format!(
        r#"{{"metrics":["recurring_revenue"],"grain":"month","range":{{"start":"2026-06-01","end":"2026-07-01"}},"filters":[{{"op":"eq","dimension":"region","value":"{CANARY}"}}]}}"#
    );

    let mut served = crate::harness::start_configured_with_environment(case, &settings, None, &TRACE, &[]);
    let reply = served.post(
        &crate::harness::v1(sutura_http::constants::base_paths::QUERY),
        None,
        &question,
    );
    served.terminate();
    let lines = served.log();
    let sent: Vec<String> = received.try_iter().collect();

    assert!(
        reply.status == 200,
        "the question filtered on the canary must be answered, or its absence below proves nothing:\n{}",
        reply.body
    );
    assert!(
        sent.iter().any(|line| line.contains(CANARY)),
        "the canary was not sent as a bound value, so its absence below proves nothing:\n{}",
        sent.join("")
    );
    assert!(
        lines.iter().any(|line| line.contains(r#""log.target":"ureq::run""#)),
        "no request line the HTTP client logged was captured, so the capture does not cover the request"
    );
    assert!(
        !lines.iter().any(|line| line.contains(r#""log.target":"ureq_proto"#)),
        "a record from the HTTP client's wire-level target crossed the bridge"
    );
    assert!(
        lines.iter().any(|line| line.contains(TRACE_LEVEL)),
        "no `trace` line was captured, so the capture is not a record of everything logged"
    );
    assert!(
        !lines.iter().any(|line| line.contains(CANARY)),
        "a value the statement bound was logged"
    );
}
