//! A reading worker's own certification of a leg before it reads the partition a grant names -
//! `docs/adr/20261102082308-workers-read-sources.md` Decision 5's last paragraph: "before it reads,
//! each worker re-certifies the leg against its own pinned catalog. That is the check a node runs
//! on a remote leg: Decision 3 of `docs/adr/20261017032054-leg-transport-between-sutura-nodes.md`
//! and its per-metric digest amendment."
//!
//! **This runs the same check [`crate::remote_leg::run_remote_leg`] runs for a leg crossing from
//! another node, over the same [`LegWire::parse`], and shares its one closed refusal
//! ([`crate::remote_leg::leg_not_certified`]) rather than a second copy of it.** A leg its pinned
//! catalog does not certify, or certifies differently, and a source this worker did not open are
//! both refused as [`RefusalReason::RemoteLegNotCertified`], naming only the source - a hostile
//! grant learns nothing about which case applies, the same limit `remote_leg`'s own documentation
//! states.
//!
//! **A reading worker's partition reader calls [`read_recertified`] before every read.**
//! `crates/sutura-cli/src/serve/worker.rs` opens the worker's catalog and declared sources at boot
//! and keeps them for that reader, which reads only what this check returns. **The limit:** a
//! planning node routes a split leaf to a worker only for a lookup leg its source granted, which only
//! a `sutura serve` deployment's PostgreSQL source or `BigQuery` pool source does.

use sutura_domain::model::SourceName;
use sutura_domain::pinned::PinnedDefinitions;
use sutura_domain::plan::LegPlan;
use sutura_domain::plan::remote::{LegWire, PartitionLeg};
use sutura_domain::query::RefusalReason;
use sutura_domain::warehouse::{ParamValue, Warehouse};

use crate::Warehouses;

/// Re-certifies a leg's wire document against this worker's own pinned catalog and the sources it
/// opened at boot.
///
/// **Two refusal paths, one refusal.** A source `opened` holds nothing for is refused before
/// [`LegWire::parse`] is even tried - the same order `run_remote_leg` certifies a remote leg in,
/// and for the same reason: there is nothing to certify the leg's statement against without a
/// source. A source `opened` does hold is then certified structurally, through the parse this
/// node's own planner certifies every remote leg with - a digest mismatch and any other certification
/// cause both collapse to the same refusal there, and this function inherits that collapse rather
/// than re-deciding it.
///
/// `params` are the leg's own bound parameter values, in the placeholder order `LegWire::parse`
/// takes them - the partition's read is not scoped by this function; only the leg it reads is
/// certified here.
pub fn recertify_leg<W>(
    pinned: &PinnedDefinitions,
    opened: &Warehouses<W>,
    leg: LegWire,
    params: Vec<ParamValue>,
) -> Result<LegPlan, RefusalReason>
where
    W: Warehouse,
{
    let source = leg.source().clone();
    if opened.get(&source).is_none() {
        return Err(crate::remote_leg::leg_not_certified(source));
    }
    leg.parse(pinned, params)
        .map_err(|_cause| crate::remote_leg::leg_not_certified(source))
}

/// `read` over the leg `wire` holds, once [`recertify_leg`] certified it, and the source `opened`
/// holds for it - and `read` is never called otherwise.
///
/// `wire` is a [`PartitionLeg`], the document the task's reading grant digests; `source` is the
/// source the task names beside it. Bytes that are not one document, and a leg naming another
/// source, are refused as an uncertified leg is, naming `source`.
pub fn read_recertified<W, T>(
    pinned: &PinnedDefinitions,
    opened: &Warehouses<W>,
    source: &SourceName,
    wire: &[u8],
    read: impl FnOnce(&W, LegPlan) -> T,
) -> Result<T, RefusalReason>
where
    W: Warehouse,
{
    let refused = || crate::remote_leg::leg_not_certified(source.clone());
    let (leg, params) = PartitionLeg::parse(wire).ok_or_else(refused)?.into_parts();
    if leg.source() != source {
        return Err(refused());
    }
    let plan = recertify_leg(pinned, opened, leg, params)?;
    let warehouse = opened.get(source).ok_or_else(refused)?;
    Ok(read(warehouse, plan))
}

#[cfg(test)]
mod tests;
