//! A one-connection HTTP server for the timeout tests: it answers like ClickHouse's `ArrowStream`
//! output for as long as its script goes, then stalls without closing the connection.

use arrow_array::{Int64Array, RecordBatch};
use arrow_ipc::writer::StreamWriter;
use arrow_schema::{DataType, Field, Schema};
use clickhouse::{Client, Compression};
use std::io::{self, BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

/// How long a stalled connection is held if the test does not drop its [`FakeServer`] first.
const HOLD: Duration = Duration::from_secs(5);

pub(crate) enum Step {
    /// One HTTP chunk.
    Send(Vec<u8>),
    Wait(Duration),
    /// The end of the HTTP body.
    Finish,
}

pub(crate) struct FakeServer {
    pub(crate) url: String,
    /// Dropping this ends the stall.
    _release: mpsc::Sender<()>,
}

impl FakeServer {
    /// Accepts the request and never answers.
    pub(crate) fn silent() -> Self {
        Self::serve(None)
    }

    /// Answers `200` and plays `script`, then stalls.
    pub(crate) fn streaming(script: Vec<Step>) -> Self {
        Self::serve(Some(script))
    }

    fn serve(script: Option<Vec<Step>>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let (release, held) = mpsc::channel::<()>();

        thread::spawn(move || {
            let (mut connection, _) = listener.accept().unwrap();

            let mut head = BufReader::new(connection.try_clone().unwrap());
            loop {
                let mut line = String::new();
                if head.read_line(&mut line).unwrap() == 0 || line == "\r\n" {
                    break;
                }
            }

            if let Some(script) = script {
                // The client may hang up first; that ends the script, not the test.
                let _ = play(&mut connection, script);
            }

            let _ = held.recv_timeout(HOLD);
        });

        Self {
            url,
            _release: release,
        }
    }

    /// A client for this server whose responses are not LZ4-framed, which this server does not
    /// produce.
    pub(crate) fn client(&self) -> Client {
        Client::default()
            .with_url(&self.url)
            .with_compression(Compression::None)
    }
}

fn play(connection: &mut TcpStream, script: Vec<Step>) -> io::Result<()> {
    connection.write_all(b"HTTP/1.1 200 OK\r\ntransfer-encoding: chunked\r\n\r\n")?;

    for step in script {
        match step {
            Step::Send(bytes) => {
                write!(connection, "{:x}\r\n", bytes.len())?;
                connection.write_all(&bytes)?;
                connection.write_all(b"\r\n")?;
            }
            Step::Wait(duration) => thread::sleep(duration),
            Step::Finish => connection.write_all(b"0\r\n\r\n")?,
        }
    }

    Ok(())
}

/// An Arrow IPC stream of `batches` one-row batches, cut into one piece for each batch: the first
/// piece also carries the schema, and the last the end-of-stream marker.
pub(crate) fn ipc_pieces(batches: usize) -> Vec<Vec<u8>> {
    let schema = Arc::new(Schema::new(vec![Field::new("n", DataType::Int64, false)]));
    let mut writer = StreamWriter::try_new(Vec::new(), &schema).unwrap();
    let mut pieces = Vec::new();

    for n in 0..batches {
        let column = Int64Array::from(vec![n as i64]);
        let batch = RecordBatch::try_new(schema.clone(), vec![Arc::new(column)]).unwrap();
        writer.write(&batch).unwrap();
        pieces.push(std::mem::take(writer.get_mut()));
    }

    writer.finish().unwrap();
    pieces
        .last_mut()
        .unwrap()
        .extend(std::mem::take(writer.get_mut()));
    pieces
}
