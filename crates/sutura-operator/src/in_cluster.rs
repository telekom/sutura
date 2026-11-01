//! The [`Cluster`] the operator's own pod runs in: the Kubernetes API, as the pod's service account,
//! and each pod's `/metrics` on its port named `http`, with `security.metrics_token`.
//!
//! A pod's `/metrics` is read over TLS, dialled at the pod's own address and its certificate checked
//! against `server.tls_node_authority` for the name `<deployment>.<namespace>.svc`: the name every
//! pod of a Deployment carries, since a certificate issued to a pod cannot name its address. This
//! check only verifies a server, so the operator holds no node client certificate: its own pod mounts
//! the CA bundle alone.
//!
//! `operator.workers`/`operator.serving` name the only two Deployments this [`Cluster`] ever reads or
//! writes: a `SuturaWorkerPool` naming any other one is refused in `reconcile::size` before its scale
//! is read at all.
//!
//! **Untested here.** The cells in `reconcile` hold a fake; nothing in this crate's tests reaches an
//! API server or a pod.

use std::collections::BTreeMap;
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use k8s_openapi::api::autoscaling::v1::Scale as ScaleObject;
use k8s_openapi::api::core::v1::Pod;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use serde::de::DeserializeOwned;
use sutura_config::{ApiGroup, DeploymentName};
use sutura_domain::identity::Secret;
use sutura_http_client::{Pinned, ReadBounds};

use crate::{AllowedDeployments, Cluster, PLURAL, Pods, Pools, Scale, VERSION, WorkerPoolSpec, WorkerPoolStatus};

/// Where the kubelet mounts the pod's service account token, its namespace and the cluster's CA.
const ACCOUNT: &str = "/var/run/secrets/kubernetes.io/serviceaccount";

/// The cluster, reached from inside one of its pods.
pub struct InCluster<'a> {
    api: Arc<ureq::Agent>,
    pods: Pinned,
    /// Each Deployment read, by its selector.
    deployments: BTreeMap<String, String>,
    base: String,
    namespace: String,
    group: &'a ApiGroup,
    metrics_token: Option<&'a Secret>,
    cap: u64,
    allowed: AllowedDeployments,
}

/// A ready pod's name, where its `/metrics` is served, and the name its certificate carries.
pub struct Target {
    name: String,
    address: SocketAddr,
    server: String,
}

impl<'a> InCluster<'a> {
    /// The cluster this process's pod runs in, trusting the CA its service account mounts for the
    /// API and `node_authority` for each pod, each request bounded by `bounds`.
    pub fn from_pod(
        group: &'a ApiGroup,
        metrics_token: Option<&'a Secret>,
        node_authority: Option<&Path>,
        workers: Option<&DeploymentName>,
        serving: Option<&DeploymentName>,
        bounds: ReadBounds,
    ) -> Result<Self, ClusterError> {
        let variable = |name| std::env::var(name).map_err(|_unset| ClusterError::NotInCluster);
        let host: IpAddr = variable("KUBERNETES_SERVICE_HOST")?
            .parse()
            .map_err(|_not_an_ip| ClusterError::NotInCluster)?;
        let port: u16 = variable("KUBERNETES_SERVICE_PORT")?
            .parse()
            .map_err(|_not_a_port| ClusterError::NotInCluster)?;
        let anchors = sutura_tls::load_anchors(&sutura_tls::Anchors::Bundle(PathBuf::from(format!("{ACCOUNT}/ca.crt"))))
            .map_err(ClusterError::Anchors)?;
        let node_authority = node_authority.ok_or(ClusterError::NoNodeAuthority)?;
        let nodes = sutura_tls::load_anchors(&sutura_tls::Anchors::Bundle(node_authority.to_path_buf()))
            .map_err(ClusterError::NodeAuthority)?;
        let workers = workers.ok_or(ClusterError::NoWorkersName)?.clone();
        let serving = serving.ok_or(ClusterError::NoServingName)?.clone();
        Ok(Self {
            api: sutura_http_client::fixed(bounds, Some(anchors)).current(),
            pods: Pinned::new(bounds, nodes),
            deployments: BTreeMap::new(),
            base: format!("https://{}", SocketAddr::new(host, port)),
            namespace: account("namespace")?,
            group,
            metrics_token,
            cap: bounds.max_response_bytes(),
            allowed: AllowedDeployments::new(workers, serving),
        })
    }

    fn pools_path(&self) -> String {
        format!(
            "/apis/{}/{VERSION}/namespaces/{}/{PLURAL}",
            self.group.as_str(),
            self.namespace
        )
    }

    fn scale_path(&self, name: &str) -> String {
        format!("/apis/apps/v1/namespaces/{}/deployments/{name}/scale", self.namespace)
    }

    fn get<T>(&self, path: &str, selector: Option<&str>) -> Result<T, ClusterError>
    where
        T: DeserializeOwned,
    {
        let mut request = self.api.get(format!("{}{path}", self.base));
        if let Some(selector) = selector {
            request = request.query("labelSelector", selector);
        }
        let response = request.header("authorization", bearer()?).call();
        let body = self.read(path, response)?;
        serde_json::from_str(&body).map_err(|source| ClusterError::Decode {
            path: path.to_owned(),
            source,
        })
    }

    fn patch(&self, path: &str, body: &serde_json::Value) -> Result<(), ClusterError> {
        let response = self
            .api
            .patch(format!("{}{path}", self.base))
            .header("authorization", bearer()?)
            .header("content-type", "application/merge-patch+json")
            .send(body.to_string());
        self.read(path, response).map(drop)
    }

    /// `response`'s body, when it is a success within the response cap.
    fn read(&self, path: &str, response: Result<ureq::http::Response<ureq::Body>, ureq::Error>) -> Result<String, ClusterError> {
        let failed = |source| ClusterError::Request {
            path: path.to_owned(),
            source,
        };
        let mut response = response.map_err(failed)?;
        let status = response.status().as_u16();
        let body = response
            .body_mut()
            .with_config()
            .limit(self.cap)
            .read_to_string()
            .map_err(failed)?;
        if !response.status().is_success() {
            return Err(ClusterError::Status {
                path: path.to_owned(),
                status,
            });
        }
        Ok(body)
    }
}

impl Cluster for InCluster<'_> {
    type Pod = Target;
    type Error = ClusterError;

    fn pools(&mut self) -> Result<Pools, ClusterError> {
        let list: Items<PoolObject> = self.get(&self.pools_path(), None)?;
        Ok(list
            .items
            .into_iter()
            .filter_map(|pool| Some((pool.metadata.name?, pool.spec)))
            .collect())
    }

    fn scale(&mut self, name: &str) -> Result<Scale, ClusterError> {
        let scale: ScaleObject = self.get(&self.scale_path(name), None)?;
        let replicas = scale.spec.and_then(|spec| spec.replicas).unwrap_or(0);
        let selector = scale.status.and_then(|status| status.selector).unwrap_or_default();
        let scale = Scale::new(u32::try_from(replicas).unwrap_or(0), selector)
            .ok_or_else(|| ClusterError::NoSelector { name: name.to_owned() })?;
        self.deployments.insert(scale.selector().to_owned(), name.to_owned());
        Ok(scale)
    }

    fn pods(&mut self, selector: &str) -> Pods<Self> {
        let deployment = self.deployments.get(selector).ok_or_else(|| ClusterError::Unread {
            selector: selector.to_owned(),
        })?;
        let server = format!("{deployment}.{}.svc", self.namespace);
        let path = format!("/api/v1/namespaces/{}/pods", self.namespace);
        let list: Items<Pod> = self.get(&path, Some(selector))?;
        Ok(list.items.iter().filter_map(|pod| target(pod, &server)).collect())
    }

    fn scrape(&mut self, pod: &Target) -> Result<String, ClusterError> {
        let path = format!("{}/metrics", pod.name);
        let agent = self.pods.at(pod.address.ip());
        let mut request = agent.get(format!("https://{}:{}/metrics", pod.server, pod.address.port()));
        if let Some(token) = self.metrics_token {
            request = request.header("authorization", metrics_bearer(token));
        }
        self.read(&path, request.call())
    }

    fn cost(&mut self, pod: &Target, cost: i32) -> Result<(), ClusterError> {
        let path = format!("/api/v1/namespaces/{}/pods/{}", self.namespace, pod.name);
        let annotation = serde_json::json!({
            "metadata": {"annotations": {"controller.kubernetes.io/pod-deletion-cost": cost.to_string()}}
        });
        self.patch(&path, &annotation)
    }

    fn resize(&mut self, name: &str, replicas: u32) -> Result<(), ClusterError> {
        self.patch(&self.scale_path(name), &serde_json::json!({"spec": {"replicas": replicas}}))
    }

    fn report(&mut self, name: &str, status: &WorkerPoolStatus) -> Result<(), ClusterError> {
        let path = format!("{}/{name}/status", self.pools_path());
        self.patch(&path, &serde_json::json!({ "status": status }))
    }

    fn allowed(&self) -> Option<&AllowedDeployments> {
        Some(&self.allowed)
    }

    /// Patches only `status.refusal`, leaving `status.replicas` as whatever the API object already
    /// holds: a merge patch (`application/merge-patch+json`) omits a field rather than clearing it. A
    /// pool refused after it was already sized keeps reporting its last `replicas` alongside the new
    /// `refusal`; a pool refused before its workers were ever read has no `replicas` yet, and the
    /// definition's status schema does not require one, so its status carries only `refusal`.
    fn refuse(&mut self, name: &str, why: &str) -> Result<(), ClusterError> {
        let path = format!("{}/{name}/status", self.pools_path());
        self.patch(&path, &serde_json::json!({ "status": { "refusal": why } }))
    }
}

/// Where `pod`'s `/metrics` is served, when it is ready, has an address and a port named `http`.
fn target(pod: &Pod, server: &str) -> Option<Target> {
    let status = pod.status.as_ref()?;
    let ready = status
        .conditions
        .iter()
        .flatten()
        .any(|condition| condition.type_ == "Ready" && condition.status == "True");
    let ip: IpAddr = status.pod_ip.as_deref()?.parse().ok()?;
    let port = pod
        .spec
        .iter()
        .flat_map(|spec| &spec.containers)
        .flat_map(|container| container.ports.iter().flatten())
        .find(|port| port.name.as_deref() == Some("http"))?;
    let port = u16::try_from(port.container_port).ok()?;
    let name = pod.metadata.name.clone()?;
    ready.then(|| Target {
        name,
        address: SocketAddr::new(ip, port),
        server: server.to_owned(),
    })
}

#[expect(
    clippy::disallowed_methods,
    reason = "a bearer has to reach the wire as text; this builds the one header value a pod's \
              `/metrics` reads, the shape sutura-catalog-datahub's `bearer` holds for its own"
)]
fn metrics_bearer(token: &Secret) -> String {
    format!("Bearer {}", token.expose_secret())
}

/// The service account token, read again for each request because the kubelet rotates it.
fn bearer() -> Result<String, ClusterError> {
    Ok(format!("Bearer {}", account("token")?.trim()))
}

fn account(file: &'static str) -> Result<String, ClusterError> {
    std::fs::read_to_string(format!("{ACCOUNT}/{file}")).map_err(|source| ClusterError::Account { file, source })
}

#[derive(serde::Deserialize)]
struct Items<T> {
    items: Vec<T>,
}

#[derive(serde::Deserialize)]
struct PoolObject {
    metadata: ObjectMeta,
    spec: WorkerPoolSpec,
}

/// Why the operator could not read or write the cluster.
#[derive(Debug, thiserror::Error)]
pub enum ClusterError {
    #[error("`KUBERNETES_SERVICE_HOST` and `KUBERNETES_SERVICE_PORT` are not an address: the operator runs in a pod")]
    NotInCluster,
    #[error("the service account's `{file}` could not be read")]
    Account {
        file: &'static str,
        #[source]
        source: std::io::Error,
    },
    #[error("the service account's `ca.crt` is not a trust anchor bundle")]
    Anchors(#[source] sutura_tls::LoadError),
    #[error("`server.tls_node_authority` is not set: each pod's `/metrics` is read over TLS")]
    NoNodeAuthority,
    #[error("`server.tls_node_authority` is not a trust anchor bundle")]
    NodeAuthority(#[source] sutura_tls::LoadError),
    #[error("`operator.workers` is not set: the operator sizes one named Deployment, never whatever a pool names")]
    NoWorkersName,
    #[error("`operator.serving` is not set: the operator reads demand from one named Deployment, never whatever a pool names")]
    NoServingName,
    #[error("`{path}` was not reached")]
    Request {
        path: String,
        #[source]
        source: ureq::Error,
    },
    #[error("`{path}` answered {status}")]
    Status { path: String, status: u16 },
    #[error("`{path}` answered what the operator cannot read")]
    Decode {
        path: String,
        #[source]
        source: serde_json::Error,
    },
    #[error("the Deployment `{name}` has no selector, and an empty one would select every pod")]
    NoSelector { name: String },
    #[error("no Deployment read has the selector `{selector}`, so its pods' certificate name is not known")]
    Unread { selector: String },
}
