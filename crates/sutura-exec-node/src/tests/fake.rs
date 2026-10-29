//! A loopback node: a real gRPC server over TLS, serving only `DoExchange`, reading each call
//! with the receiver's own framing and answering from a script.

use std::collections::VecDeque;
use std::num::NonZeroUsize;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use arrow_array::RecordBatch;
use arrow_flight::encode::FlightDataEncoderBuilder;
use arrow_flight::error::FlightError;
use arrow_flight::flight_service_server::{FlightService, FlightServiceServer};
use arrow_flight::{
    Action, ActionType, Criteria, Empty, FlightData, FlightDescriptor, FlightInfo, HandshakeRequest, HandshakeResponse, PollInfo,
    PutResult, SchemaResult, Ticket,
};
use futures_util::stream::BoxStream;
use futures_util::{StreamExt as _, stream};
use sutura_domain::plan::remote::LegWire;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio_rustls::TlsAcceptor;
use tokio_rustls::rustls::ServerConfig;
use tokio_rustls::rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use tonic::transport::server::Connected;
use tonic::{Request, Response, Status, Streaming};

/// What the fake node answers one call with.
pub(super) enum Reply {
    Answer(Vec<RecordBatch>),
    Refuse(tonic::Code, &'static str),
}

/// What one call carried, as the receiving node read it.
pub(super) struct Received {
    pub(super) bearer: Option<String>,
    pub(super) timeout: bool,
    pub(super) leg: LegWire,
    pub(super) params: RecordBatch,
}

#[derive(Default)]
struct Script {
    replies: VecDeque<Reply>,
    received: Vec<Received>,
}

/// A node on a loopback port with a certificate for `localhost`, and the runtime it runs on.
pub(super) struct Node {
    pub(super) port: u16,
    pub(super) anchor: CertificateDer<'static>,
    script: Arc<Mutex<Script>>,
    _runtime: tokio::runtime::Runtime,
}

impl Node {
    pub(super) fn answering(replies: impl IntoIterator<Item = Reply>) -> Self {
        let issued = rcgen::generate_simple_self_signed([String::from("localhost")]).expect("a pair generates");
        let anchor = issued.cert.der().clone();
        let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(issued.signing_key.serialize_der()));
        let mut config = ServerConfig::builder_with_provider(Arc::new(tokio_rustls::rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .expect("safe defaults")
            .with_no_client_auth()
            .with_single_cert(vec![anchor.clone()], key)
            .expect("a pair is a certificate");
        config.alpn_protocols = vec![b"h2".to_vec()];
        let acceptor = TlsAcceptor::from(Arc::new(config));
        let script = Arc::new(Mutex::new(Script {
            replies: replies.into_iter().collect(),
            received: Vec::new(),
        }));
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .expect("a runtime");
        let listener = runtime
            .block_on(tokio::net::TcpListener::bind(("127.0.0.1", 0)))
            .expect("a loopback port");
        let port = listener.local_addr().expect("a bound address").port();
        let incoming = stream::unfold((listener, acceptor), |(listener, acceptor)| async move {
            let accepted = match listener.accept().await {
                Ok((tcp, _)) => acceptor.accept(tcp).await.map(Tls),
                Err(cause) => Err(cause),
            };
            Some((accepted, (listener, acceptor)))
        });
        let service = FlightServiceServer::new(Fake(Arc::clone(&script)));
        runtime.spawn(tonic::transport::Server::builder().add_service(service).serve_with_incoming(incoming));
        Self {
            port,
            anchor,
            script,
            _runtime: runtime,
        }
    }

    /// Every call this node has read so far.
    pub(super) fn received(&self) -> Vec<Received> {
        core::mem::take(&mut self.script.lock().expect("an unpoisoned script").received)
    }
}

/// A server TLS stream as the gRPC server takes one.
struct Tls(tokio_rustls::server::TlsStream<tokio::net::TcpStream>);

impl Connected for Tls {
    type ConnectInfo = ();

    fn connect_info(&self) -> Self::ConnectInfo {}
}

impl AsyncRead for Tls {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.0).poll_read(cx, buf)
    }
}

impl AsyncWrite for Tls {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.0).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.0).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.0).poll_shutdown(cx)
    }
}

struct Fake(Arc<Mutex<Script>>);

const UNSERVED: &str = "the fake node serves only do_exchange";

type Answers<T> = BoxStream<'static, Result<T, Status>>;

#[tonic::async_trait]
impl FlightService for Fake {
    type HandshakeStream = Answers<HandshakeResponse>;
    type ListFlightsStream = Answers<FlightInfo>;
    type DoGetStream = Answers<FlightData>;
    type DoPutStream = Answers<PutResult>;
    type DoActionStream = Answers<arrow_flight::Result>;
    type ListActionsStream = Answers<ActionType>;
    type DoExchangeStream = Answers<FlightData>;

    async fn handshake(&self, _: Request<Streaming<HandshakeRequest>>) -> Result<Response<Self::HandshakeStream>, Status> {
        Err(Status::unimplemented(UNSERVED))
    }

    async fn list_flights(&self, _: Request<Criteria>) -> Result<Response<Self::ListFlightsStream>, Status> {
        Err(Status::unimplemented(UNSERVED))
    }

    async fn get_flight_info(&self, _: Request<FlightDescriptor>) -> Result<Response<FlightInfo>, Status> {
        Err(Status::unimplemented(UNSERVED))
    }

    async fn poll_flight_info(&self, _: Request<FlightDescriptor>) -> Result<Response<PollInfo>, Status> {
        Err(Status::unimplemented(UNSERVED))
    }

    async fn get_schema(&self, _: Request<FlightDescriptor>) -> Result<Response<SchemaResult>, Status> {
        Err(Status::unimplemented(UNSERVED))
    }

    async fn do_get(&self, _: Request<Ticket>) -> Result<Response<Self::DoGetStream>, Status> {
        Err(Status::unimplemented(UNSERVED))
    }

    async fn do_put(&self, _: Request<Streaming<FlightData>>) -> Result<Response<Self::DoPutStream>, Status> {
        Err(Status::unimplemented(UNSERVED))
    }

    async fn do_action(&self, _: Request<Action>) -> Result<Response<Self::DoActionStream>, Status> {
        Err(Status::unimplemented(UNSERVED))
    }

    async fn list_actions(&self, _: Request<Empty>) -> Result<Response<Self::ListActionsStream>, Status> {
        Err(Status::unimplemented(UNSERVED))
    }

    async fn do_exchange(&self, request: Request<Streaming<FlightData>>) -> Result<Response<Self::DoExchangeStream>, Status> {
        let bearer = request
            .metadata()
            .get("authorization")
            .and_then(|value| value.to_str().ok())
            .map(String::from);
        let timeout = request.metadata().get("grpc-timeout").is_some();
        let frames = request
            .into_inner()
            .map(|frame| frame.map_err(|status| FlightError::Tonic(Box::new(status))));
        let bound = NonZeroUsize::new(1 << 20).expect("non-zero");
        let (leg, params) = sutura_flight::receive_request(frames, bound, bound)
            .await
            .map_err(|refused| Status::invalid_argument(refused.to_string()))?;
        let reply = {
            let mut script = self.0.lock().expect("an unpoisoned script");
            script.received.push(Received {
                bearer,
                timeout,
                leg,
                params,
            });
            script.replies.pop_front().expect("the script has a reply for every call")
        };
        match reply {
            Reply::Refuse(code, message) => Err(Status::new(code, message)),
            Reply::Answer(batches) => {
                let schema = batches.first().expect("a scripted answer has a batch").schema();
                let encoded = FlightDataEncoderBuilder::new()
                    .with_schema(schema)
                    .build(stream::iter(batches.into_iter().map(Ok)))
                    .map(|frame| frame.map_err(Status::from));
                Ok(Response::new(encoded.boxed()))
            }
        }
    }
}
