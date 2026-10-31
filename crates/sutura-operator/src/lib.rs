#![forbid(unsafe_code)]
//! The worker-pool operator's core (`docs/adr/20261030193029-operator-sizes-the-worker-pool.md`): a
//! pool's bounds, the size they call for, and the `SuturaWorkerPool` definition the chart installs.
//!
//! Nothing here talks to a cluster.

use std::collections::BTreeMap;

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

/// What a `SuturaWorkerPool` asks for: the bounds the operator sizes the pool within. Its fields are
/// the schema's; nothing outside this crate builds one until the operator reads it from a cluster.
#[derive(Clone, Debug, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct WorkerPoolSpec {
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
#[derive(Clone, Debug, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct WorkerPoolStatus {
    /// The workers the operator last sized the pool to.
    replicas: u32,
    /// Why the operator refused the spec, when it did.
    refusal: Option<String>,
}

/// A pool's bounds, its floor at most its ceiling.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pool {
    min: u32,
    max: u32,
    warm: u32,
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
        })
    }

    /// The workers `demand` busy ones call for: `demand + warm`, clamped to `[min, max]`.
    pub fn size(self, demand: u32) -> u32 {
        demand.saturating_add(self.warm).clamp(self.min, self.max)
    }
}

/// Why a spec's bounds are not a [`Pool`].
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum InvalidPool {
    #[error("`min` is {min} and `max` is {max}: a pool's floor cannot be above its ceiling")]
    MinAboveMax { min: u32, max: u32 },
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
    let root = JSONSchemaProps {
        type_: Some(String::from("object")),
        properties: Some(BTreeMap::from([
            (String::from("spec"), props(generator.root_schema_for::<WorkerPoolSpec>())?),
            (
                String::from("status"),
                props(generator.root_schema_for::<WorkerPoolStatus>())?,
            ),
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

    use super::{InvalidPool, Pool, WorkerPoolSpec, definition};

    fn spec(min: u32, max: u32, warm: u32) -> WorkerPoolSpec {
        WorkerPoolSpec {
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
            BTreeSet::from(["idleAfterSeconds", "max", "min", "warm"])
        );
        let bound = &spec.properties.as_ref().expect("properties")["idleAfterSeconds"];
        assert_eq!(
            (bound.format.as_deref(), bound.minimum, bound.maximum),
            (None, Some(0.0), Some(f64::from(u32::MAX)))
        );
    }
}
