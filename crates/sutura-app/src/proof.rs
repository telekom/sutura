//! The proof, and the only operation that can mint it.
//!
//! A module of its own and a PRIVATE one, because that is the mechanism: the field of
//! [`crate::Validated`] and its tuple constructor are visible exactly here, so
//! [`verify_and_validate`] is the only safe code anywhere that can produce one. Moving either
//! item out of this module, or adding a second `pub fn` to it that does not call a
//! `Warehouse`, is what a reviewer has to notice - and it is a one-item diff in one place
//! rather than a property of every call site.

use sutura_domain::pinned::{NotValidated, PinnedDefinitions};
use sutura_domain::warehouse::Warehouse;

use crate::warehouses::Warehouses;

/// A `T` that has been shown to hold up.
///
/// **The service accepts only this, so an unvalidated bundle is unrepresentable rather than
/// merely refused.** The field is private to the module this type is declared in, and
/// [`verify_and_validate`] is the only thing in that module which builds one.
///
/// Generic in the type it wraps, but obtainable only for [`PinnedDefinitions`], and that
/// asymmetry is the point: validating means re-running every anchor the bundle declares, so
/// whatever mints this has to be able to enumerate them and to execute them. A blanket
/// constructor for any `T` would be a wrapper that proves nothing, which is worse than no
/// wrapper because it reads like proof.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Validated<T>(T);

impl<T> Validated<T> {
    #[inline]
    pub const fn get(&self) -> &T {
        &self.0
    }

    #[inline]
    pub fn into_inner(self) -> T {
        self.0
    }
}

/// Holds every cardinality declaration and re-runs every anchor, and returns the bundle only if
/// both held.
///
/// **Two checks rather than one, and the second was added against a defect the first cannot
/// see.** An anchor is one range and one number, so a dimension row that duplicates a join key
/// outside that range moves nothing an anchor compares - while it changes a grouped answer, and
/// changes it differently depending on how the plan was shaped. `crate::declared_keys` carries
/// the measurement, the three outcomes that are deliberately not refusals, and what the pair
/// still does not cover.
///
/// The one operation that produces a [`Validated`] bundle. It takes the `Warehouse` and calls
/// it, which is the whole of what the type is now allowed to claim: not "somebody asserted these
/// anchors match", but "these statements were executed against this data system and reproduced
/// the numbers their author certified".
///
/// **What it still does not claim.** `W` is a port, so a caller may pass a fake - and a fake is
/// exactly what the golden suite passes, deliberately, because the alternative is a test suite
/// that needs a database to check a refusal. What the type proves is that a warehouse was
/// called; that the warehouse was the one holding the business's data is a composition-root
/// decision no signature can make. `answer` narrows it a little further by refusing a plan whose
/// source is not the adapter's own.
///
/// The forgery this closes does not compile:
///
/// ```compile_fail
/// use sutura_app::Validated;
/// use sutura_domain::model::MetricName;
/// use sutura_domain::pinned::{AnchorCheck, AnchorReport, PinnedDefinitions};
///
/// // Enumerate the anchors, claim each one matched, hand the claim to the validator.
/// // No data system is opened and no statement is executed.
/// fn _forge(pinned: PinnedDefinitions) -> Validated<PinnedDefinitions> {
///     let names: Vec<MetricName> = pinned.anchored_metrics().map(|(name, _)| name.clone()).collect();
///     let mut report = AnchorReport::new();
///     for name in names {
///         report.record(name, AnchorCheck::Matched);
///     }
///     // Neither the constructor that was here nor the tuple constructor is reachable.
///     Validated::new(pinned, &report).unwrap()
/// }
///
/// fn _wrap(pinned: PinnedDefinitions) -> Validated<PinnedDefinitions> {
///     Validated(pinned)
/// }
/// ```
///
/// The twin of that block, which pins the names so a rename cannot make it pass vacuously:
///
/// ```
/// use sutura_app::{Validated, Warehouses, verify_and_validate};
/// use sutura_domain::pinned::{NotValidated, PinnedDefinitions};
/// use sutura_domain::warehouse::Warehouse;
///
/// fn _served(_bundle: &Validated<PinnedDefinitions>) {}
///
/// fn _mint<W: Warehouse>(
///     pinned: PinnedDefinitions,
///     warehouses: &Warehouses<W>,
/// ) -> Result<Validated<PinnedDefinitions>, NotValidated> {
///     verify_and_validate(pinned, warehouses)
/// }
/// ```
///
/// # It takes the registry, not one warehouse
///
/// Each metric's anchor runs against the data system that metric's own plan names, so a bundle
/// spanning two configured sources verifies both halves. Under one warehouse every anchor on the
/// second source came back as a source mismatch, which is a bundle that cannot be validated for a
/// reason that has nothing to do with its numbers.
///
/// **What it still does not take is an identity**, and that is the honest limit on what an executed
/// anchor proves. The registry says which posture each adapter was handed; it does not hand the
/// adapter a credential to re-run the anchor under, because the port has no parameter for one yet.
/// So the bundle is proven to compute its certified numbers for whatever identity each adapter is
/// configured with - the process, for the file engine that ships - and the composition root refuses
/// a bundle with an anchor on a source that declared no verification identity, which is the half
/// available before the port changes.
///
/// # What is refused before any anchor runs
///
/// A metric whose computation is catalog-authored SQL, unless `W` declares
/// [`Warehouse::EXECUTES_AUTHORED_SQL`]. The fragment is stored as written and nothing published
/// compiles it, so an adapter taking the default cannot execute the metric; refusing the bundle
/// here, naming the metric, is what stands between that and a served bundle with a metric that
/// is silently skipped or a measure quietly substituted. Read off the ONE adapter type
/// `Warehouses<W>` holds, the way `EXECUTES_LEGS` is - so it is a fact about the build, not
/// about the data. No adapter this workspace ships opts in; `docs/adr/0004` is the decision.
///
/// **"Before any anchor runs" is a placement, not an assertion.** It is true because this check
/// sits ahead of `declared_keys::hold` and `verify_anchors` in the body below, and the ordering
/// ahead of `declared_keys::hold` is held INCIDENTALLY, by the `examples/authored-sql` cell: that
/// catalog declares a relationship and attaches no data to it, so a block moved below `hold`
/// fails there first, on `declared_keys::hold`'s own refusal, rather than on this one. Nothing
/// separates the placement from `verify_anchors` alone, and no fixture's fake counts an anchor
/// or a declared key that was never touched.
pub fn verify_and_validate<W>(
    pinned: PinnedDefinitions,
    warehouses: &Warehouses<W>,
) -> Result<Validated<PinnedDefinitions>, NotValidated>
where
    W: Warehouse,
{
    // Before anything is executed: can this build execute every metric the bundle declares?
    if !W::EXECUTES_AUTHORED_SQL {
        let mut authored = pinned.definitions().metrics().values();
        if let Some(metric) = authored.find(|metric| metric.computation().authored_sql().is_some()) {
            return Err(NotValidated::AuthoredSqlNotExecutable {
                metric: metric.name().clone(),
            });
        }
    }
    // Then the cardinality declarations. `declared_keys` carries why that order, and why a
    // violated `many_to_one` is a bundle that cannot be validated rather than a question that
    // cannot be answered: the same question answers differently on one data system and on two,
    // and neither topology refused it.
    super::declared_keys::hold(&pinned, warehouses)?;
    let report = super::verify_anchors(&pinned, warehouses);
    report.verdict(&pinned)?;
    Ok(Validated(pinned))
}
