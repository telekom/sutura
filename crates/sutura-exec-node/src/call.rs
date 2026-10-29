//! The one `DoExchange` per leg, over server-authenticated TLS, on this adapter's own runtime.

use std::sync::Arc;
use std::time::Instant;

use arrow_array::RecordBatch;
use arrow_flight::decode::FlightRecordBatchStream;
use arrow_flight::error::FlightError;
use arrow_flight::flight_service_client::FlightServiceClient;
use futures_util::{StreamExt as _, TryStreamExt as _, stream};
use hyper_util::rt::TokioIo;
use sutura_domain::plan::remote::LegWire;
use sutura_domain::warehouse::ResultBatches;
use sutura_domain::warehouse::arrow::{Accumulating, ResultBudget};
use sutura_domain::warehouse::deadline::Deadline;
use tokio_rustls::TlsConnector;
use tokio_rustls::rustls::pki_types::{CertificateDer, ServerName};
use tokio_rustls::rustls::{ClientConfig, RootCertStore};
use tonic::metadata::{Ascii, MetadataValue};
use tonic::transport::Channel;

use crate::endpoint::NodeEndpoint;
use crate::error::{NodeError, NotBuilt};

/// The channel to one node and the runtime that drives it.
pub(crate) struct Call {
    /// An `Option` for `sutura-exec-datafusion`'s reason: a runtime dropped inside an async
    /// context panics, so [`Drop`] shuts it down in the background. Construction sets it.
    runtime: Option<tokio::runtime::Runtime>,
    channel: Channel,
}

impl Drop for Call {
    fn drop(&mut self) {
        if let Some(runtime) = self.runtime.take() {
            runtime.shutdown_background();
        }
    }
}

impl Call {
    /// A lazily connected channel to `endpoint`, trusting `anchors` and nothing else. Nothing is
    /// dialled: the first leg is the first connection.
    pub(crate) fn open(
        endpoint: &NodeEndpoint,
        anchors: impl IntoIterator<Item = CertificateDer<'static>>,
    ) -> Result<Self, NotBuilt> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .thread_name("sutura-node")
            .enable_all()
            .build()
            .map_err(|cause| NotBuilt::Runtime { cause })?;
        let connector = TlsConnector::from(Arc::new(client_config(anchors)?));
        let name = ServerName::try_from(String::from(endpoint.host())).map_err(|_cause| NotBuilt::ServerName)?;
        let (host, port) = (String::from(endpoint.host()), endpoint.port());
        let dial = tower::service_fn(move |_uri| {
            let (connector, name, host) = (connector.clone(), name.clone(), host.clone());
            async move {
                let tcp = tokio::net::TcpStream::connect((host.as_str(), port)).await?;
                let tls = connector.connect(name, tcp).await?;
                Ok::<_, std::io::Error>(TokioIo::new(tls))
            }
        });
        let channel = {
            let _entered = runtime.enter();
            tonic::transport::Endpoint::from(endpoint.uri().clone()).connect_with_connector_lazy(dial)
        };
        Ok(Self {
            runtime: Some(runtime),
            channel,
        })
    }

    /// Sends `leg` with its one parameter batch and `bearer`, and reads the reply into `budget`.
    ///
    /// The whole exchange - dial, send and every reply batch - is bounded by what `deadline` has
    /// left, which also travels as `grpc-timeout` so the node opens no longer a deadline of its own.
    pub(crate) fn exchange(
        &self,
        leg: &LegWire,
        params: RecordBatch,
        bearer: MetadataValue<Ascii>,
        (deadline, budget): (Deadline, ResultBudget),
    ) -> Result<ResultBatches, NodeError> {
        let remaining = deadline.remaining_at(Instant::now()).ok_or(NodeError::DeadlineExceeded)?;
        let runtime = self.runtime.as_ref().ok_or(NodeError::Unreachable {
            code: tonic::Code::Unavailable,
        })?;
        let request = sutura_flight::request_stream(leg, stream::iter([Ok(params)])).map_err(|_cause| NodeError::Framing)?;
        let mut client = FlightServiceClient::new(self.channel.clone());
        runtime.block_on(async move {
            let exchanged = async {
                // One parameter batch, so the frames are few and are encoded before the call opens.
                let frames: Vec<_> = request.try_collect().await.map_err(|_cause| NodeError::Framing)?;
                let mut call = tonic::Request::new(stream::iter(frames));
                call.metadata_mut().insert("authorization", bearer);
                call.set_timeout(remaining);
                let reply = client.do_exchange(call).await.map_err(|status| NodeError::of_status(&status))?;
                read(reply.into_inner(), budget).await
            };
            tokio::time::timeout(remaining, exchanged)
                .await
                .map_err(|_elapsed| NodeError::DeadlineExceeded)?
        })
    }
}

/// The reply's batches, each checked against the first one's schema and charged as it arrives.
async fn read(reply: tonic::Streaming<arrow_flight::FlightData>, budget: ResultBudget) -> Result<ResultBatches, NodeError> {
    let mut decoded =
        FlightRecordBatchStream::new_from_flight_data(reply.map(|frame| frame.map_err(|status| FlightError::Tonic(Box::new(status)))));
    let mut accumulating: Option<Accumulating> = None;
    while let Some(batch) = decoded.try_next().await.map_err(|cause| match cause {
        FlightError::Tonic(status) => NodeError::of_status(&status),
        _ => NodeError::Answer,
    })? {
        accumulating
            .get_or_insert_with(|| Accumulating::announcing(batch.schema(), usize::MAX, budget))
            .push(batch)
            .map_err(|cause| NodeError::Result { cause })?;
    }
    match accumulating {
        Some(accumulated) => Ok(accumulated.finish()),
        None => decoded
            .schema()
            .map(|schema| ResultBatches::none_under(Arc::clone(schema)))
            .ok_or(NodeError::Answer),
    }
}

fn client_config(anchors: impl IntoIterator<Item = CertificateDer<'static>>) -> Result<ClientConfig, NotBuilt> {
    let mut roots = RootCertStore::empty();
    for anchor in anchors {
        roots.add(anchor).map_err(|cause| NotBuilt::Anchors { cause })?;
    }
    let mut config = ClientConfig::builder_with_provider(Arc::new(tokio_rustls::rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()
        .map_err(|cause| NotBuilt::Anchors { cause })?
        .with_root_certificates(roots)
        .with_no_client_auth();
    config.alpn_protocols = vec![b"h2".to_vec()];
    Ok(config)
}
