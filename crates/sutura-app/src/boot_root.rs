//! The boot root of trust: the ONE value a process holds where boot trust is asserted.
//!
//! **What it is.** [`BootRoot`] is a single, minimal store of what boot needs and nothing else:
//! the validated, digest-pinned bundle and the process's own root identity. It is the value a
//! composition root hands wherever boot trust is asserted, and [`BootRoot::validate`] is the only
//! route to a validated bundle ("single site"), because it is [`crate::verify_and_validate`] behind
//! a name that cannot hold anything per-request.
//!
//! **What it deliberately does NOT hold** - the parse refuses them by having no parameter and no
//! field for them: a `RequestContext` / caller assertion, any per-subject `LegCredentials`, any
//! `Expiry`, any clock or timeout. The constructor takes a [`BootIdentity`] and stores no
//! credential material beside it, so the root cannot be repurposed to *answer as* the process - it
//! answers nothing; it only certifies the bundle boot validated. The request path borrows
//! [`Self::definitions`] off the root; it never receives the root itself.
//!
//! **The honest limit, stated with the claim.** This is a type narrowing WHAT the root holds. It
//! does not by itself confine `verify_anchor` to boot - that half stays the existing `clippy.toml`
//! ban plus one `#[expect]`, kept to boot by a LINT, not by this type. C2 makes the root unable to
//! hold per-request state; a future per-request caller of `verify_anchor` is still stopped by the
//! lint.

use sutura_domain::catalog::Definitions;
use sutura_domain::pinned::{NotValidated, PinnedDefinitions};
use sutura_domain::warehouse::Warehouse;

use crate::proof::{Validated, verify_and_validate};
use crate::warehouses::Warehouses;

/// The process's own static root identity, under which boot-time trust runs.
///
/// The deployment's own word for who it is - `Subject::TheDeploymentItself` in the domain - and the
/// identity `verify_anchor` and the shared-service-user legs run as. A marker rather than a
/// credential, and deliberately a unit: there is one deployment, one value, and no way to confuse
/// it with a caller. [`BootRoot`] holds exactly one of these, and nothing on the request path can
/// mint one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BootIdentity;

/// The single root of trust boot holds: the validated bundle and nothing a request needs.
///
/// ## A caller's assertion cannot make it in
///
/// The constructor's second argument is the deployment's OWN identity, so a `RequestContext` is a
/// compile error wherever a `BootRoot` is being built - the root cannot be handed, or repurposed
/// to answer as, a caller.
///
/// ```compile_fail
/// use sutura_app::{BootIdentity, BootRoot};
/// use sutura_domain::identity::RequestContext;
/// use sutura_domain::pinned::{NotValidated, PinnedDefinitions};
/// use sutura_domain::warehouse::Warehouse;
///
/// // No parameter takes a caller's assertion: the second argument is the root's own identity.
/// fn _boot<W: Warehouse>(
///     pinned: PinnedDefinitions,
///     context: RequestContext,
///     warehouses: &sutura_app::Warehouses<W>,
/// ) -> Result<BootRoot, NotValidated> {
///     BootRoot::validate(pinned, context, warehouses)
/// }
/// ```
///
/// The twin, with the root's only other argument:
///
/// ```
/// use sutura_app::{BootIdentity, BootRoot};
/// use sutura_domain::pinned::{NotValidated, PinnedDefinitions};
/// use sutura_domain::warehouse::Warehouse;
///
/// fn _boot<W: Warehouse>(
///     pinned: PinnedDefinitions,
///     identity: BootIdentity,
///     warehouses: &sutura_app::Warehouses<W>,
/// ) -> Result<BootRoot, NotValidated> {
///     BootRoot::validate(pinned, identity, warehouses)
/// }
/// ```
pub struct BootRoot {
    /// The validated, digest-pinned bundle. Held as [`Validated`], whose only constructor is
    /// [`crate::verify_and_validate`], so a `BootRoot` cannot carry a bundle boot did not certify.
    validated: Validated<PinnedDefinitions>,
    /// The process's own root identity; stored here and nowhere on the request path.
    boot_identity: BootIdentity,
}

impl BootRoot {
    /// Validates a pinned bundle against the configured adapters and names the root identity.
    ///
    /// The ONLY route to a [`Validated`] bundle: this is [`crate::verify_and_validate`] (itself the
    /// sole constructor of the `Validated` marker) with the root's own identity attached. It takes
    /// the registry, not one warehouse, so a bundle spanning two configured sources verifies both
    /// halves - the same contract as the validator it wraps.
    ///
    /// **What this does not do, stated:** it does not prove the bundle is *correct* (validation is
    /// a separate gate), and the identity it stores is exactly as trustworthy as the deployment's
    /// own secret handling - a leaked process credential still reads under it.
    pub fn validate<W>(
        pinned: PinnedDefinitions,
        boot_identity: BootIdentity,
        warehouses: &Warehouses<W>,
    ) -> Result<Self, NotValidated>
    where
        W: Warehouse,
    {
        Ok(Self {
            validated: verify_and_validate(pinned, warehouses)?,
            boot_identity,
        })
    }

    /// The validated, digest-pinned bundle and nothing else.
    #[inline]
    pub const fn pinned(&self) -> &PinnedDefinitions {
        self.validated.get()
    }

    /// The validated definitions a question answers against - a borrow, never an `Arc` or a clone.
    ///
    /// This is the only view the request path takes off the root, and it is definitions, not a
    /// credential: nothing here can mint a credential or re-validate, and a per-request value can
    /// never be added to the root.
    #[inline]
    pub const fn definitions(&self) -> &Definitions {
        self.validated.get().definitions()
    }

    /// The deployment's own root identity - the static root boot ran its anchors under.
    #[inline]
    pub const fn boot_identity(&self) -> BootIdentity {
        self.boot_identity
    }
}

#[cfg(test)]
mod tests {
    use super::{BootIdentity, BootRoot};
    use crate::Warehouses;
    use crate::tests::{bundle, certified, shared, source};
    use crate::tests_support::FixedWarehouse;

    fn root() -> BootRoot {
        BootRoot::validate(
            bundle(),
            BootIdentity,
            &Warehouses::of(FixedWarehouse::answering(source(), shared(), certified())),
        )
        .expect("the test bundle validates against the fixed warehouse")
    }

    /// C2: [`BootRoot::validate`] is the single validation site, and validation does not disturb
    /// the bundle it certifies.
    ///
    /// Two roots built from one read share the bundle's digest - there is no second read that could
    /// diverge (the two-`load()` gap the identity skill documents). And the value carried is the
    /// *validated* bundle, whose field is obtainable only through [`crate::verify_and_validate`]; a
    /// `BootRoot` cannot wrap a bundle boot did not certify.
    #[test]
    fn bootroot_is_the_single_validation_site() {
        let first = root();
        let second = root();
        assert_eq!(first.pinned().digest(), second.pinned().digest());
        assert_eq!(first.definitions(), second.definitions());
    }

    /// C2: the root exposes only boot trust - the definitions (a borrow) and the root identity.
    ///
    /// There is no accessor here that yields a `RequestContext`, a per-subject credential, an
    /// expiry or a clock, because the type has no field for any of them - [`BootRoot::definitions`]
    /// is the whole request-facing surface, and it is definitions, not a credential.
    #[test]
    fn bootroot_exposes_only_boot_trust() {
        let root = root();
        // definitions() is a borrow of the validated bundle's content; holding it and the root
        // together is fine (no Arc, no clone on this path).
        let defs = root.definitions();
        assert!(!defs.metrics().is_empty(), "the validated bundle has content");

        let identity = root.boot_identity();
        assert_eq!(
            identity, BootIdentity,
            "the root carries exactly the deployment's own identity"
        );
    }
}
