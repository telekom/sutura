//! A `/cubejs-api/v1/load` answer as the [`RowSet`] the plan's labels name.
//!
//! Cube answers each row as an object keyed by member, with a number as a JSON string (`"59"`) and
//! a time bucket as a timestamp at midnight (`"2026-06-01T00:00:00.000"`). The plan decides how each
//! column reads: a key as it came, a bucket as its date, a measure as an integer unless it is a
//! ratio, a mean, or holds a value that is not an integer.

use sutura_domain::warehouse::{Real, RowSet, Value};

use crate::query::{Column, CubeQuery, Read};
use crate::{CubeError, CubeResult};

/// The time of day Cube puts on a bucket of a day or longer.
const MIDNIGHT: &str = "T00:00:00.000";

type Object = serde_json::Map<String, serde_json::Value>;

#[derive(serde::Deserialize)]
struct Answer {
    data: Vec<Object>,
}

#[expect(
    clippy::map_err_ignore,
    reason = "serde's error repeats the answer's own values, and a result value must not reach an error \
              chain that is logged"
)]
fn decode<E>(body: &[u8]) -> CubeResult<Vec<Object>, E>
where
    E: core::error::Error + 'static,
{
    serde_json::from_slice::<Answer>(body)
        .map(|answer| answer.data)
        .map_err(|_| CubeError::MalformedAnswer {
            expected: "load answer with a data array",
        })
}

/// The rows `answers` hold, one answer per query of `query`, in its order.
pub(crate) fn read<E>(answers: &[Vec<u8>], query: &CubeQuery) -> CubeResult<RowSet, E>
where
    E: core::error::Error + 'static,
{
    let malformed = || CubeError::MalformedAnswer {
        expected: "one answer per query",
    };
    let (first, rest) = answers.split_first().ok_or_else(malformed)?;
    let mut data = decode(first)?;
    if query.split() {
        join(&mut data, rest, query)?;
    } else if !rest.is_empty() {
        return Err(malformed());
    }
    let as_real: Vec<bool> = query.columns().iter().map(|column| real(column, &data)).collect();
    let labels = query.columns().iter().map(|column| column.label.clone()).collect();
    let rows = data
        .iter()
        .map(|row| {
            query
                .columns()
                .iter()
                .zip(&as_real)
                .map(|(column, &as_real)| cell(&column.read, as_real, &column.key, row.get(&column.key)))
                .collect::<Result<Vec<_>, _>>()
        })
        .collect::<Result<Vec<_>, _>>()?;
    RowSet::new(labels, rows).map_err(|cause| CubeError::Shape { cause })
}

/// Whether a measure column reads as real numbers: always for a ratio or a mean, and otherwise
/// when any of its values in this answer is not an integer.
///
/// Cube answers a whole real sum as `"0"`, the same text as an integer, and the answer does not say
/// the column's type. ponytail: inferred per answer, so a real column whose every value in one answer
/// is whole reads as integers; a type in Cube's answer, or the catalog's, would settle it.
fn real(column: &Column, data: &[Object]) -> bool {
    let fractional = |value: &serde_json::Value| match *value {
        serde_json::Value::String(ref text) => text.parse::<i64>().is_err(),
        serde_json::Value::Number(ref number) => !number.is_i64(),
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Array(_) | serde_json::Value::Object(_) => {
            false
        }
    };
    match column.read {
        Read::Measure { real: true, .. } => true,
        Read::Measure { real: false, .. } => data.iter().any(|row| row.get(&column.key).is_some_and(fractional)),
        Read::Key | Read::Bucket => false,
    }
}

/// Puts each metric's value from its own answer onto the group row it belongs to.
///
/// A group no row under a metric's guard reached is absent from that metric's answer: it reads as
/// what the plan's SQL answers there, a zero for a count and no value otherwise.
fn join<E>(groups: &mut [Object], measures: &[Vec<u8>], query: &CubeQuery) -> CubeResult<(), E>
where
    E: core::error::Error + 'static,
{
    let group_keys: Vec<&str> = query
        .columns()
        .iter()
        .filter(|column| !matches!(column.read, Read::Measure { .. }))
        .map(|column| column.key.as_str())
        .collect();
    let group_of = |row: &Object| {
        serde_json::Value::Array(
            group_keys
                .iter()
                .map(|key| row.get(*key).cloned().unwrap_or(serde_json::Value::Null))
                .collect(),
        )
        .to_string()
    };
    let metrics: Vec<_> = query
        .columns()
        .iter()
        .filter_map(|column| match column.read {
            Read::Measure { count, .. } => Some((column.key.as_str(), count)),
            Read::Key | Read::Bucket => None,
        })
        .collect();
    if metrics.len() != measures.len() {
        return Err(CubeError::MalformedAnswer {
            expected: "one answer per metric",
        });
    }
    for (&(key, count), body) in metrics.iter().zip(measures) {
        let mut found = std::collections::HashMap::new();
        for row in decode(body)? {
            found.insert(group_of(&row), row.get(key).cloned().unwrap_or(serde_json::Value::Null));
        }
        for row in groups.iter_mut() {
            let empty = if count {
                serde_json::Value::String(String::from("0"))
            } else {
                serde_json::Value::Null
            };
            row.insert(String::from(key), found.remove(&group_of(row)).unwrap_or(empty));
        }
        if !found.is_empty() {
            return Err(CubeError::MalformedAnswer {
                expected: "metric answer within the groups",
            });
        }
    }
    Ok(())
}

fn cell<E>(read: &Read, as_real: bool, member: &str, value: Option<&serde_json::Value>) -> CubeResult<Value, E>
where
    E: core::error::Error + 'static,
{
    let malformed = |expected| CubeError::MalformedAnswer { expected };
    let Some(value) = value else {
        return Err(malformed("member the query asked for"));
    };
    let mistyped = || malformed("member value of the type its column reads");
    match *value {
        serde_json::Value::Null => match *read {
            Read::Measure {
                fails: Some(ref metric), ..
            } => Err(CubeError::ZeroDenominator { metric: metric.clone() }),
            Read::Measure { fails: None, .. } | Read::Key | Read::Bucket => Ok(Value::Null),
        },
        serde_json::Value::String(ref text) => match *read {
            Read::Key => Ok(Value::Text(text.clone())),
            Read::Bucket => text
                .strip_suffix(MIDNIGHT)
                .map(|date| Value::Text(String::from(date)))
                .ok_or_else(|| malformed("time bucket at midnight")),
            Read::Measure { .. } => number_value(text, as_real, member),
        },
        serde_json::Value::Number(ref number) => match *read {
            Read::Key | Read::Measure { .. } => number_value(&number.to_string(), as_real, member),
            Read::Bucket => Err(mistyped()),
        },
        serde_json::Value::Bool(flag) => match *read {
            Read::Key => Ok(Value::Text(flag.to_string())),
            Read::Bucket | Read::Measure { .. } => Err(mistyped()),
        },
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => Err(mistyped()),
    }
}

fn number_value<E>(text: &str, real: bool, member: &str) -> CubeResult<Value, E>
where
    E: core::error::Error + 'static,
{
    if !real && let Ok(integer) = text.parse::<i64>() {
        return Ok(Value::Integer(integer));
    }
    let number = text.parse::<f64>().map_err(|cause| CubeError::NotANumber {
        member: String::from(member),
        cause,
    })?;
    Real::parse(number).map(Value::Real).map_err(|cause| CubeError::NotFinite {
        member: String::from(member),
        cause,
    })
}
