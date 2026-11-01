//! A `SuturaWorkerPool` naming a Deployment other than the two the operator started with is refused
//! before either name is ever read: no `scale`, `pods`, `scrape`, `cost` or `resize` call for it.
//! `docs/adr/20261030193029-operator-sizes-the-worker-pool.md`, amended 2026-11-01.

use std::collections::BTreeMap;
use std::time::Instant;

use sutura_config::DeploymentName;

use super::{AllowedDeployments, Cluster, Pods, Pools, Scale};
use crate::{InvalidPool, WorkerPoolSpec, WorkerPoolStatus};

#[derive(Default)]
struct Fake {
    pool: Option<(String, WorkerPoolSpec)>,
    allowed: Option<AllowedDeployments>,
    scaled: Vec<String>,
    pods_called: usize,
    scraped: usize,
    costed: usize,
    resized: Vec<(String, u32)>,
    refused: Vec<(String, String)>,
    reported: Vec<(String, WorkerPoolStatus)>,
}

fn spec(workers: &str, serving: &str) -> WorkerPoolSpec {
    serde_json::from_value(serde_json::json!({
        "workers": workers,
        "serving": serving,
        "min": 0,
        "max": 1,
        "warm": 0,
        "idleAfterSeconds": 0,
    }))
    .expect("a well-formed spec")
}

impl Cluster for Fake {
    type Pod = ();
    type Error = std::convert::Infallible;

    fn pools(&mut self) -> Result<Pools, Self::Error> {
        Ok(self.pool.iter().cloned().collect())
    }

    fn scale(&mut self, name: &str) -> Result<Scale, Self::Error> {
        self.scaled.push(name.to_owned());
        Ok(Scale::new(1, "app=fake".to_owned()).expect("a non-empty selector"))
    }

    fn pods(&mut self, _selector: &str) -> Pods<Self> {
        self.pods_called += 1;
        Ok(Vec::new())
    }

    fn scrape(&mut self, (): &()) -> Result<String, Self::Error> {
        self.scraped += 1;
        Ok(String::new())
    }

    fn cost(&mut self, (): &(), _cost: i32) -> Result<(), Self::Error> {
        self.costed += 1;
        Ok(())
    }

    fn resize(&mut self, name: &str, replicas: u32) -> Result<(), Self::Error> {
        self.resized.push((name.to_owned(), replicas));
        Ok(())
    }

    fn report(&mut self, name: &str, status: &WorkerPoolStatus) -> Result<(), Self::Error> {
        self.reported.push((name.to_owned(), status.clone()));
        Ok(())
    }

    fn allowed(&self) -> Option<&AllowedDeployments> {
        self.allowed.as_ref()
    }

    fn refuse(&mut self, name: &str, why: &str) -> Result<(), Self::Error> {
        self.refused.push((name.to_owned(), why.to_owned()));
        Ok(())
    }
}

fn allowed() -> AllowedDeployments {
    AllowedDeployments::new(
        DeploymentName::parse("workers").expect("a valid name"),
        DeploymentName::parse("serving").expect("a valid name"),
    )
}

/// Runs `tick` over a single pool naming a foreign Deployment and asserts the common shape of a
/// refusal: no Deployment is ever read, scaled, resized or reported, and `refuse` alone is called
/// with `expected`. Shared by both variants below so neither test's body names the other's.
fn refused_before_any_deployment_is_read(spec: WorkerPoolSpec, expected: &InvalidPool) {
    let mut cluster = Fake {
        pool: Some((String::from("pool"), spec)),
        allowed: Some(allowed()),
        ..Fake::default()
    };
    let mut surplus = BTreeMap::new();
    let failed = super::tick(&mut cluster, &mut surplus, Instant::now()).expect("pools are listed");
    assert!(failed.is_empty(), "a refusal is not a cluster failure: {failed:?}");
    assert!(cluster.scaled.is_empty(), "no Deployment's scale is read for a foreign name");
    assert_eq!(cluster.pods_called, 0, "no pod is listed for a refused pool");
    assert_eq!(cluster.scraped, 0, "no pod is scraped for a refused pool");
    assert_eq!(cluster.costed, 0, "no deletion cost is set for a refused pool");
    assert!(cluster.resized.is_empty(), "no Deployment is resized for a refused pool");
    assert!(
        cluster.reported.is_empty(),
        "a refusal reports through `refuse`, not `report`"
    );
    assert_eq!(cluster.refused, vec![(String::from("pool"), expected.to_string())]);
}

#[test]
fn a_pool_naming_a_foreign_workers_deployment_is_refused_before_any_deployment_is_read() {
    refused_before_any_deployment_is_read(
        spec("evil-workers", "serving"),
        &InvalidPool::ForeignWorkers {
            name: String::from("evil-workers"),
        },
    );
}

#[test]
fn a_pool_naming_a_foreign_serving_deployment_is_refused_before_any_deployment_is_read() {
    refused_before_any_deployment_is_read(
        spec("workers", "evil-serving"),
        &InvalidPool::ForeignServing {
            name: String::from("evil-serving"),
        },
    );
}
