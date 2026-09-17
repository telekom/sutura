//! The two-source fixture the federated capability gate needs, split out of `testing.rs` by that
//! file's own `max-lines` reason - it sits at the cap on its own.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use sutura_domain::model::SourceName;
use sutura_domain::warehouse::{RowSet, Value};

use super::FakeWarehouse;

/// Both of `two_source_bundle`'s sources open, each a [`FakeWarehouse`] taking the port's
/// defaulted `executes_legs` - unlike `fake_warehouse` alone: the per-leg gate looks a source up
/// before asking its capability, so one source open would refuse `SourceUnavailable` for the
/// other before either leg's capability is asked.
pub(crate) fn two_source_fake_warehouse() -> sutura_app::Warehouses<FakeWarehouse> {
    let result = RowSet::new(vec![String::from("revenue")], vec![vec![Value::Integer(197_122)]])
        .expect("a one-cell result is a result set");
    let local = FakeWarehouse {
        source: super::source(),
        posture: super::shared_posture(),
        result: result.clone(),
        held: Arc::new(AtomicBool::new(false)),
    };
    let elsewhere = FakeWarehouse {
        source: SourceName::parse("elsewhere").expect("a test source is a source"),
        posture: super::shared_posture(),
        result,
        held: Arc::new(AtomicBool::new(false)),
    };
    sutura_app::Warehouses::of(local)
        .and(elsewhere)
        .expect("two distinct sources, one registry")
}
