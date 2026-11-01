//! One pass over every pool: read its demand from the serving pods, size its workers, report it.

use std::collections::BTreeMap;
use std::time::Instant;

use crate::{Pool, Surplus, WorkerPoolSpec, WorkerPoolStatus, deletion_cost, gauge};

/// The worker slots a serving node wants, summed over the serving pods into a pool's demand.
const SLOTS_WANTED: &str = "sutura_worker_slots_wanted";
/// The node calls a worker has in flight, which set its deletion cost before a pool shrinks.
const IN_FLIGHT: &str = "sutura_worker_requests_in_flight";

/// Each pool by name, as [`Cluster::pools`] lists them.
pub type Pools = Vec<(String, WorkerPoolSpec)>;
/// The ready pods a selector matches, or why `C` could not list them.
pub type Pods<C> = Result<Vec<<C as Cluster>::Pod>, <C as Cluster>::Error>;
/// Each pool `C` failed for, with why; or why `C` could not list the pools at all.
pub type Failed<C> = Result<Vec<(String, <C as Cluster>::Error)>, <C as Cluster>::Error>;

/// What the operator reads from and writes to the cluster it runs in.
pub trait Cluster {
    /// A pod the operator scrapes, and costs when it is a worker.
    type Pod;
    type Error;

    /// The pools in the operator's namespace, by name.
    fn pools(&mut self) -> Result<Pools, Self::Error>;
    /// The Deployment `name`'s replica count and the selector of its pods.
    fn scale(&mut self, name: &str) -> Result<Scale, Self::Error>;
    /// The ready pods `selector` matches.
    fn pods(&mut self, selector: &str) -> Pods<Self>;
    /// `pod`'s Prometheus text exposition.
    fn scrape(&mut self, pod: &Self::Pod) -> Result<String, Self::Error>;
    /// Sets `pod`'s `controller.kubernetes.io/pod-deletion-cost`.
    fn cost(&mut self, pod: &Self::Pod, cost: i32) -> Result<(), Self::Error>;
    /// Sets the Deployment `name`'s replica count.
    fn resize(&mut self, name: &str, replicas: u32) -> Result<(), Self::Error>;
    /// Writes the pool `name`'s status.
    fn report(&mut self, name: &str, status: &WorkerPoolStatus) -> Result<(), Self::Error>;
}

/// A Deployment's `scale` subresource: its replica count and the selector of its pods.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Scale {
    replicas: u32,
    selector: String,
}

impl Scale {
    /// `None` for an empty `selector`, which would select every pod in the namespace.
    pub fn new(replicas: u32, selector: String) -> Option<Self> {
        (!selector.is_empty()).then_some(Self { replicas, selector })
    }

    #[inline]
    pub const fn replicas(&self) -> u32 {
        self.replicas
    }

    #[inline]
    pub fn selector(&self) -> &str {
        &self.selector
    }
}

/// Sizes and reports every pool at `now`.
///
/// Returns each pool the cluster failed for with its error: one pool's failure leaves the others
/// sized. `surplus` carries each pool's [`Surplus`] from one pass to the next and keeps only the
/// pools this pass found.
pub fn tick<C>(cluster: &mut C, surplus: &mut BTreeMap<String, Surplus>, now: Instant) -> Failed<C>
where
    C: Cluster,
{
    let mut failed = Vec::new();
    let mut kept = BTreeMap::new();
    for (name, spec) in cluster.pools()? {
        let mut since = surplus.remove(&name).unwrap_or_default();
        if let Err(why) = size(cluster, &name, &spec, &mut since, now) {
            failed.push((name.clone(), why));
        }
        kept.insert(name, since);
    }
    *surplus = kept;
    Ok(failed)
}

fn size<C>(cluster: &mut C, name: &str, spec: &WorkerPoolSpec, since: &mut Surplus, now: Instant) -> Result<(), C::Error>
where
    C: Cluster,
{
    let workers = cluster.scale(&spec.workers)?;
    let pool = match Pool::parse(spec) {
        Ok(pool) => pool,
        Err(refusal) => {
            let status = WorkerPoolStatus {
                replicas: workers.replicas(),
                refusal: Some(refusal.to_string()),
            };
            return cluster.report(name, &status);
        }
    };
    let serving = cluster.scale(&spec.serving)?;
    let mut demand = 0_u32;
    for pod in cluster.pods(serving.selector())? {
        let wanted = gauge(&cluster.scrape(&pod)?, SLOTS_WANTED).unwrap_or(0);
        demand = demand.saturating_add(u32::try_from(wanted).unwrap_or(u32::MAX));
    }
    let replicas = pool.replicas(workers.replicas(), demand, since, now);
    if replicas < workers.replicas() {
        for pod in cluster.pods(workers.selector())? {
            let in_flight = gauge(&cluster.scrape(&pod)?, IN_FLIGHT).unwrap_or(0);
            cluster.cost(&pod, deletion_cost(in_flight))?;
        }
    }
    if replicas != workers.replicas() {
        cluster.resize(&spec.workers, replicas)?;
    }
    cluster.report(name, &WorkerPoolStatus { replicas, refusal: None })
}

#[cfg(test)]
mod tests;
