#![forbid(unsafe_code)]
//! The worker-pool operator's core (`docs/adr/20261030193029-operator-sizes-the-worker-pool.md`): a
//! pool's bounds, the size they call for, and the `SuturaWorkerPool` definition the chart installs.
//!
//! [`tick`] sizes every pool through a [`Cluster`]; [`InCluster`] is the one that talks to a cluster.

mod in_cluster;
mod reconcile;

pub use in_cluster::{ClusterError, InCluster, Target};
pub use reconcile::{AllowedDeployments, Cluster, Failed, Pods, Pools, Scale, tick};

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use k8s_openapi::apiextensions_apiserver::pkg::apis::apiextensions::v1::{
    CustomResourceDefinition, CustomResourceDefinitionNames, CustomResourceDefinitionSpec, CustomResourceDefinitionVersion,
    CustomResourceSubresourceStatus, CustomResourceSubresources, CustomResourceValidation, JSONSchemaProps,
};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use schemars::generate::SchemaSettings;
use schemars::transform::RecursiveTransform;
use schemars::{JsonSchema, Schema};
use sutura_config::ApiGroup;

/// The resource's kind.
pub const KIND: &str = "SuturaWorkerPool";
/// The resource's one version.
pub const VERSION: &str = "v1alpha1";
const PLURAL: &str = "suturaworkerpools";

/// What a Deployment's name is: a lowercase RFC 1123 subdomain, so a name is one path segment.
const DEPLOYMENT_NAME: &str = r"^[a-z0-9]([-a-z0-9]*[a-z0-9])?(\.[a-z0-9]([-a-z0-9]*[a-z0-9])?)*$";

/// What a `SuturaWorkerPool` asks for: the bounds the operator sizes the pool within. Its fields are
/// the schema's; nothing outside this crate builds one until the operator reads it from a cluster.
#[derive(Clone, Debug, PartialEq, Eq, JsonSchema, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkerPoolSpec {
    /// The Deployment of workers the operator sizes, in the pool's namespace.
    #[schemars(length(max = 253), regex(pattern = DEPLOYMENT_NAME))]
    workers: String,
    /// The Deployment of serving nodes whose wanted worker slots are the pool's demand.
    #[schemars(length(max = 253), regex(pattern = DEPLOYMENT_NAME))]
    serving: String,
    /// The fewest workers the pool keeps, with no demand at all.
    min: u32,
    /// The most workers the pool grows to, whatever the demand.
    max: u32,
    /// Idle workers kept beyond demand, so a leg does not wait for a worker to start.
    warm: u32,
    /// How long a worker above the size demand calls for stays before it is removed, in seconds.
    idle_after_seconds: u32,
}

/// What the operator reports it did.
#[derive(Clone, Debug, PartialEq, Eq, JsonSchema, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkerPoolStatus {
    /// The workers the operator last sized the pool to.
    replicas: u32,
    /// Why the operator refused the spec, when it did.
    refusal: Option<String>,
}

/// A pool's bounds, its floor at most its ceiling, and how long a surplus stays.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pool {
    min: u32,
    max: u32,
    warm: u32,
    idle_after: Duration,
}

impl Pool {
    /// The bounds `spec` asks for.
    pub const fn parse(spec: &WorkerPoolSpec) -> Result<Self, InvalidPool> {
        if spec.min > spec.max {
            return Err(InvalidPool::MinAboveMax {
                min: spec.min,
                max: spec.max,
            });
        }
        Ok(Self {
            min: spec.min,
            max: spec.max,
            warm: spec.warm,
            idle_after: Duration::from_secs(spec.idle_after_seconds as u64),
        })
    }

    /// The workers `demand` busy ones call for: `demand + warm`, clamped to `[min, max]`.
    pub fn size(self, demand: u32) -> u32 {
        demand.saturating_add(self.warm).clamp(self.min, self.max)
    }

    /// The workers to run at `now` with `running` up: [`Self::size`] at once when that is at least
    /// `running`, and only once the pool has stayed above it for `idleAfter` when it is less.
    pub fn replicas(self, running: u32, demand: u32, surplus: &mut Surplus, now: Instant) -> u32 {
        let size = self.size(demand);
        if size >= running {
            *surplus = Surplus::default();
            return size;
        }
        let since = *surplus.0.get_or_insert(now);
        if now.saturating_duration_since(since) < self.idle_after {
            return running;
        }
        *surplus = Surplus::default();
        size
    }
}

/// Since when a pool has run more workers than its demand calls for.
///
/// Held in the operator's memory, not in the resource: an operator that restarts waits `idleAfter`
/// again before it shrinks, which keeps workers longer and never removes one sooner.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Surplus(Option<Instant>);

/// The value of the unlabelled series `name` in a Prometheus text exposition, where it has one.
pub fn gauge(exposition: &str, name: &str) -> Option<u64> {
    exposition
        .lines()
        .find_map(|line| line.strip_prefix(name)?.strip_prefix(' ')?.parse().ok())
}

/// The `controller.kubernetes.io/pod-deletion-cost` for a worker with `in_flight` calls: the fewer,
/// the sooner the Deployment controller removes it.
pub fn deletion_cost(in_flight: u64) -> i32 {
    i32::try_from(in_flight).unwrap_or(i32::MAX)
}

/// Why a spec's bounds are not a [`Pool`].
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum InvalidPool {
    #[error("`min` is {min} and `max` is {max}: a pool's floor cannot be above its ceiling")]
    MinAboveMax { min: u32, max: u32 },
    #[error("`{name}` is not the Deployment this operator sizes")]
    ForeignWorkers { name: String },
    #[error("`{name}` is not the Deployment this operator reads demand from")]
    ForeignServing { name: String },
}

/// The `SuturaWorkerPool` definition under `group`: namespaced, one version, a status subresource.
///
/// The schema is derived from [`WorkerPoolSpec`] and [`WorkerPoolStatus`]. An integer `format` the
/// API server does not know, as `uint32` is, is stripped there with a warning; it is removed here, and
/// a `uint32`'s ceiling is written as its `maximum` so the API server refuses what the type cannot hold.
pub fn definition(group: &ApiGroup) -> Result<CustomResourceDefinition, serde_json::Error> {
    let mut generator = SchemaSettings::openapi3()
        .with(|settings| {
            settings.inline_subschemas = true;
            settings.meta_schema = None;
        })
        .with_transform(RecursiveTransform(|schema: &mut Schema| {
            if schema.remove("format").is_some_and(|format| format == "uint32") {
                schema.insert(String::from("maximum"), u32::MAX.into());
            }
        }))
        .into_generator();
    let props = |schema: Schema| serde_json::from_value::<JSONSchemaProps>(schema.to_value());
    // `replicas` is not `required`: a pool refused before it is ever sized (`InvalidPool::Foreign*`)
    // has a status with only `refusal`, and the API server would reject that merge patch otherwise.
    // No `default` is added either - the server would apply it and invent a size that was never read.
    let mut status = props(generator.root_schema_for::<WorkerPoolStatus>())?;
    if let Some(required) = status.required.as_mut() {
        required.retain(|field| field != "replicas");
    }
    status.required = status.required.take().filter(|required| !required.is_empty());
    let root = JSONSchemaProps {
        type_: Some(String::from("object")),
        properties: Some(BTreeMap::from([
            (String::from("spec"), props(generator.root_schema_for::<WorkerPoolSpec>())?),
            (String::from("status"), status),
        ])),
        required: Some(vec![String::from("spec")]),
        ..JSONSchemaProps::default()
    };
    Ok(CustomResourceDefinition {
        metadata: ObjectMeta {
            name: Some(format!("{PLURAL}.{}", group.as_str())),
            ..ObjectMeta::default()
        },
        spec: CustomResourceDefinitionSpec {
            group: group.as_str().to_owned(),
            names: CustomResourceDefinitionNames {
                kind: KIND.to_owned(),
                plural: PLURAL.to_owned(),
                ..CustomResourceDefinitionNames::default()
            },
            scope: String::from("Namespaced"),
            versions: vec![CustomResourceDefinitionVersion {
                name: VERSION.to_owned(),
                served: true,
                storage: true,
                schema: Some(CustomResourceValidation {
                    open_api_v3_schema: Some(root),
                }),
                subresources: Some(CustomResourceSubresources {
                    status: Some(CustomResourceSubresourceStatus(serde_json::json!({}))),
                    scale: None,
                }),
                ..CustomResourceDefinitionVersion::default()
            }],
            ..CustomResourceDefinitionSpec::default()
        },
        status: None,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use sutura_config::ApiGroup;

    use std::time::{Duration, Instant};

    use serde::Deserialize as _;

    use super::{InvalidPool, Pool, Surplus, WorkerPoolSpec, definition, deletion_cost, gauge};

    fn spec(min: u32, max: u32, warm: u32) -> WorkerPoolSpec {
        WorkerPoolSpec {
            workers: String::from("workers"),
            serving: String::from("serving"),
            min,
            max,
            warm,
            idle_after_seconds: 300,
        }
    }

    #[test]
    fn a_pool_whose_min_is_above_its_max_is_refused() {
        assert_eq!(Pool::parse(&spec(3, 2, 0)), Err(InvalidPool::MinAboveMax { min: 3, max: 2 }));
    }

    #[test]
    fn a_pool_is_its_demand_plus_warm_within_its_bounds() {
        let pool = Pool::parse(&spec(1, 10, 2)).expect("1 is at most 10");
        let sized: Vec<u32> = [0, 3, 8, 9, u32::MAX].into_iter().map(|demand| pool.size(demand)).collect();
        assert_eq!(sized, [2, 5, 10, 10, 10]);
        let floor = Pool::parse(&spec(4, 10, 0)).expect("4 is at most 10");
        assert_eq!(floor.size(1), 4, "below its floor a pool is its floor");
    }

    #[test]
    fn a_pool_grows_at_once_and_shrinks_only_once_it_has_stayed_above_its_size_for_idle_after() {
        let pool = Pool::parse(&spec(0, 10, 0)).expect("0 is at most 10");
        let start = Instant::now();
        let at = |seconds: u64| start + Duration::from_secs(seconds);
        let mut surplus = Surplus::default();

        assert_eq!(
            pool.replicas(2, 6, &mut surplus, at(0)),
            6,
            "a pool grows as soon as demand asks"
        );
        assert_eq!(pool.replicas(6, 1, &mut surplus, at(0)), 6, "a surplus stays while it is new");
        assert_eq!(pool.replicas(6, 2, &mut surplus, at(299)), 6, "a surplus stays for idleAfter");
        assert_eq!(
            pool.replicas(6, 2, &mut surplus, at(300)),
            2,
            "a surplus idle for idleAfter goes"
        );

        assert_eq!(pool.replicas(2, 1, &mut surplus, at(400)), 2);
        assert_eq!(
            pool.replicas(2, 3, &mut surplus, at(500)),
            3,
            "demand above the pool ends its surplus"
        );
        assert_eq!(
            pool.replicas(3, 1, &mut surplus, at(799)),
            3,
            "a surplus that ended starts its wait again"
        );
    }

    #[test]
    fn a_gauge_is_read_from_its_own_unlabelled_line_and_its_cost_saturates() {
        let exposition = "# TYPE wanted_total counter\nwanted_total 9\n# TYPE wanted gauge\nwanted 3\n";
        assert_eq!(gauge(exposition, "wanted"), Some(3));
        assert_eq!(gauge(exposition, "absent"), None, "a series not exported is no value");
        assert_eq!(gauge("wanted many\n", "wanted"), None, "a value that is no count is no value");
        assert_eq!(
            [0, 7, u64::MAX].map(deletion_cost),
            [0, 7, i32::MAX],
            "a worker busier than a cost holds is the costliest"
        );
    }

    #[test]
    fn the_definition_is_namespaced_under_the_configured_group_with_u32_bounds() {
        let group = ApiGroup::parse("pools.example.com").expect("a group");
        let definition = definition(&group).expect("the derived schema is a definition's schema");
        assert_eq!(
            definition.metadata.name.as_deref(),
            Some("suturaworkerpools.pools.example.com")
        );
        assert_eq!(
            (definition.spec.group.as_str(), definition.spec.scope.as_str()),
            ("pools.example.com", "Namespaced")
        );
        let [version] = definition.spec.versions.as_slice() else {
            panic!("one version, got {:?}", definition.spec.versions);
        };
        assert_eq!(
            (version.name.as_str(), version.served, version.storage),
            ("v1alpha1", true, true)
        );
        assert!(version.subresources.as_ref().and_then(|each| each.status.as_ref()).is_some());
        let root = version
            .schema
            .as_ref()
            .and_then(|each| each.open_api_v3_schema.as_ref())
            .expect("a schema");
        let spec = &root.properties.as_ref().expect("properties")["spec"];
        assert_eq!(
            spec.required.iter().flatten().map(String::as_str).collect::<BTreeSet<_>>(),
            BTreeSet::from(["idleAfterSeconds", "max", "min", "serving", "warm", "workers"])
        );
        let workers = &spec.properties.as_ref().expect("properties")["workers"];
        assert_eq!(
            (workers.max_length, workers.pattern.as_deref()),
            (Some(253), Some(super::DEPLOYMENT_NAME))
        );
        let bound = &spec.properties.as_ref().expect("properties")["idleAfterSeconds"];
        assert_eq!(
            (bound.format.as_deref(), bound.minimum, bound.maximum),
            (None, Some(0.0), Some(f64::from(u32::MAX)))
        );
    }

    /// The chart renders the definition rather than running `sutura-operator crd`, so its copy is
    /// held equal to this one.
    #[test]
    fn the_charts_definition_is_the_operators() {
        let golden =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../charts/sutura/testdata/golden/pool/worker-pool.yaml");
        let text = std::fs::read_to_string(&golden).unwrap_or_else(|error| panic!("{} is readable: {error}", golden.display()));
        let rendered = serde_norway::Deserializer::from_str(&text)
            .map(|doc| serde_json::Value::deserialize(doc).expect("a YAML document"))
            .find(|doc| doc["kind"] == "CustomResourceDefinition")
            .expect("the chart renders the definition");
        let group = ApiGroup::parse("sutura.telekom.github.io").expect("the chart's default group");
        let generated = definition(&group).expect("the derived schema is a definition's schema");
        assert_eq!(rendered["metadata"]["name"].as_str(), generated.metadata.name.as_deref());
        assert_eq!(
            rendered["spec"],
            serde_json::to_value(&generated.spec).expect("a definition is JSON")
        );
    }
}
