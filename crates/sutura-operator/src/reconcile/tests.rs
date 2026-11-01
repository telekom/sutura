use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use super::{Cluster, Pods, Pools, Scale, tick};
use crate::{Surplus, WorkerPoolSpec, WorkerPoolStatus};

#[derive(Debug, PartialEq, Eq)]
enum Write {
    Cost(&'static str, i32),
    Resize(String, u32),
    Report(String, WorkerPoolStatus),
}

/// What the fake has no entry for.
#[derive(Debug, PartialEq, Eq)]
struct Absent(String);

/// Deployments' replicas by name, pods by selector, expositions by pod, and every write in order.
#[derive(Default)]
struct Fake {
    pools: Vec<(String, WorkerPoolSpec)>,
    deployments: BTreeMap<String, u32>,
    pods: BTreeMap<String, Vec<&'static str>>,
    expositions: BTreeMap<&'static str, String>,
    writes: Vec<Write>,
}

impl Fake {
    fn deployment(mut self, name: &str, replicas: u32, pods: &[(&'static str, &str)]) -> Self {
        self.pods
            .insert(format!("app={name}"), pods.iter().map(|(pod, _)| *pod).collect());
        for (pod, exposition) in pods {
            self.expositions.insert(pod, (*exposition).to_owned());
        }
        self.deployments.insert(name.to_owned(), replicas);
        self
    }

    fn pool(mut self, name: &str, workers: &str, serving: &str, bounds: [u32; 4]) -> Self {
        let [min, max, warm, idle_after_seconds] = bounds;
        let spec = WorkerPoolSpec {
            workers: workers.to_owned(),
            serving: serving.to_owned(),
            min,
            max,
            warm,
            idle_after_seconds,
        };
        self.pools.push((name.to_owned(), spec));
        self
    }
}

impl Cluster for Fake {
    type Pod = &'static str;
    type Error = Absent;

    fn pools(&mut self) -> Result<Pools, Absent> {
        Ok(self.pools.clone())
    }

    fn scale(&mut self, name: &str) -> Result<Scale, Absent> {
        let replicas = *self.deployments.get(name).ok_or_else(|| Absent(name.to_owned()))?;
        Ok(Scale::new(replicas, format!("app={name}")).expect("the selector is not empty"))
    }

    fn pods(&mut self, selector: &str) -> Pods<Self> {
        Ok(self.pods.get(selector).cloned().unwrap_or_default())
    }

    fn scrape(&mut self, pod: &&'static str) -> Result<String, Absent> {
        self.expositions.get(pod).cloned().ok_or_else(|| Absent((*pod).to_owned()))
    }

    fn cost(&mut self, pod: &&'static str, cost: i32) -> Result<(), Absent> {
        self.writes.push(Write::Cost(pod, cost));
        Ok(())
    }

    fn resize(&mut self, name: &str, replicas: u32) -> Result<(), Absent> {
        self.writes.push(Write::Resize(name.to_owned(), replicas));
        self.deployments.insert(name.to_owned(), replicas);
        Ok(())
    }

    fn report(&mut self, name: &str, status: &WorkerPoolStatus) -> Result<(), Absent> {
        self.writes.push(Write::Report(name.to_owned(), status.clone()));
        Ok(())
    }
}

fn sized(replicas: u32) -> WorkerPoolStatus {
    WorkerPoolStatus { replicas, refusal: None }
}

#[test]
fn a_pool_grows_at_once_to_the_slots_its_serving_pods_want_plus_warm() {
    let mut cluster = Fake::default()
        .deployment("workers", 1, &[("w0", "sutura_worker_requests_in_flight 0\n")])
        .deployment(
            "serving",
            2,
            &[
                ("s0", "sutura_worker_slots_wanted 2\n"),
                ("s1", "# no federation, so no series\n"),
            ],
        )
        .pool("pool", "workers", "serving", [0, 10, 1, 300]);
    let failed = tick(&mut cluster, &mut BTreeMap::new(), Instant::now()).expect("the pools are listed");
    assert!(failed.is_empty(), "{failed:?}");
    assert_eq!(
        cluster.writes,
        [
            Write::Resize(String::from("workers"), 3),
            Write::Report(String::from("pool"), sized(3))
        ]
    );
}

#[test]
fn a_pool_shrinks_after_idle_after_and_costs_each_worker_by_its_calls_in_flight_first() {
    let mut cluster = Fake::default()
        .deployment(
            "workers",
            3,
            &[
                ("w0", "sutura_worker_requests_in_flight 4\n"),
                ("w1", "sutura_worker_requests_in_flight 0\n"),
                ("w2", "sutura_worker_requests_in_flight 1\n"),
            ],
        )
        .deployment("serving", 1, &[("s0", "sutura_worker_slots_wanted 1\n")])
        .pool("pool", "workers", "serving", [0, 10, 0, 300]);
    let mut surplus = BTreeMap::new();
    let start = Instant::now();

    tick(&mut cluster, &mut surplus, start).expect("the pools are listed");
    assert_eq!(
        cluster.writes,
        [Write::Report(String::from("pool"), sized(3))],
        "a new surplus stays"
    );

    cluster.writes.clear();
    tick(&mut cluster, &mut surplus, start + Duration::from_secs(300)).expect("the pools are listed");
    assert_eq!(
        cluster.writes,
        [
            Write::Cost("w0", 4),
            Write::Cost("w1", 0),
            Write::Cost("w2", 1),
            Write::Resize(String::from("workers"), 1),
            Write::Report(String::from("pool"), sized(1)),
        ]
    );
    assert_eq!(
        surplus.get("pool"),
        Some(&Surplus::default()),
        "the surplus ended with the shrink"
    );
}

#[test]
fn a_refused_pool_is_reported_and_its_workers_are_left_alone() {
    let mut cluster = Fake::default()
        .deployment("workers", 2, &[])
        .deployment("serving", 1, &[("s0", "sutura_worker_slots_wanted 9\n")])
        .pool("pool", "workers", "serving", [3, 1, 0, 300]);
    tick(&mut cluster, &mut BTreeMap::new(), Instant::now()).expect("the pools are listed");
    let [Write::Report(name, status)] = cluster.writes.as_slice() else {
        panic!("one report and nothing else, got {:?}", cluster.writes);
    };
    assert_eq!((name.as_str(), status.replicas), ("pool", 2));
    assert!(
        status.refusal.as_deref().is_some_and(|why| why.contains("`min` is 3")),
        "{status:?}"
    );
}

#[test]
fn a_pool_the_cluster_fails_for_is_returned_and_the_next_is_still_sized() {
    let mut cluster = Fake::default()
        .deployment("workers", 0, &[])
        .deployment("serving", 1, &[("s0", "sutura_worker_slots_wanted 1\n")])
        .pool("broken", "absent", "serving", [0, 10, 0, 300])
        .pool("pool", "workers", "serving", [0, 10, 0, 300]);
    let mut surplus = BTreeMap::from([(String::from("deleted"), Surplus::default())]);
    let failed = tick(&mut cluster, &mut surplus, Instant::now()).expect("the pools are listed");
    assert_eq!(failed, [(String::from("broken"), Absent(String::from("absent")))]);
    assert_eq!(
        cluster.writes,
        [
            Write::Resize(String::from("workers"), 1),
            Write::Report(String::from("pool"), sized(1))
        ]
    );
    assert_eq!(
        surplus.keys().map(String::as_str).collect::<Vec<_>>(),
        ["broken", "pool"],
        "a pool no longer listed is forgotten"
    );
}
