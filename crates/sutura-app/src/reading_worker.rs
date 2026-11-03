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
//! **Nothing calls this from a worker's read path yet.** `crates/sutura-cli/src/serve/worker.rs`
//! opens a reading worker's catalog and its declared sources at boot; this function is the check
//! the slice that reads a granted partition will run before it does, over the leg the grant names
//! and the partition's own bound parameters. No `PartitionReader` port exists yet - that arrives
//! with its first implementor, a later slice.

use sutura_domain::pinned::PinnedDefinitions;
use sutura_domain::plan::LegPlan;
use sutura_domain::plan::remote::LegWire;
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

#[cfg(test)]
mod tests;
