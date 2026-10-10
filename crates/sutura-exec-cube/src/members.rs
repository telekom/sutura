//! The members a Cube deployment serves, read off its `/cubejs-api/v1/meta` answer.

use std::collections::BTreeSet;

use crate::{CubeError, CubeResult};

#[derive(serde::Deserialize)]
struct Meta {
    cubes: Vec<Cube>,
}

#[derive(serde::Deserialize)]
struct Cube {
    #[serde(default)]
    measures: Vec<Named>,
    #[serde(default)]
    dimensions: Vec<Named>,
    #[serde(default)]
    segments: Vec<Named>,
}

#[derive(serde::Deserialize)]
struct Named {
    name: String,
}

/// Refuses the first of `asked` that the metadata answer `served` does not name.
#[expect(
    clippy::map_err_ignore,
    reason = "serde's error repeats the answer's own text, which stays off an error chain that is logged"
)]
pub(crate) fn refuse_absent<E>(served: &[u8], asked: &[String]) -> CubeResult<(), E>
where
    E: core::error::Error + 'static,
{
    let meta: Meta = serde_json::from_slice(served).map_err(|_| CubeError::MalformedAnswer {
        expected: "metadata answer",
    })?;
    let known: BTreeSet<&str> = meta
        .cubes
        .iter()
        .flat_map(|cube| cube.measures.iter().chain(&cube.dimensions).chain(&cube.segments))
        .map(|named| named.name.as_str())
        .collect();
    asked
        .iter()
        .find(|member| !known.contains(member.as_str()))
        .map_or(Ok(()), |member| Err(CubeError::AbsentMember { member: member.clone() }))
}
