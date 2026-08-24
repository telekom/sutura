//! The hexagon's interior: the types the business rules are written in, and - when the first
//! adapter needs one - the port traits it names its dependencies by.
//!
//! Nothing here may depend on a framework: no async runtime, no web server, no query engine.
//! `cargo xtask check-boundaries` enforces it over the whole transitive tree, because the rule
//! is worth more as a check than as a sentence in a design document.
//!
//! **There are no port traits yet, and that is deliberate.** A port exists to invert a
//! dependency on something outside the hexagon, and no adapter exists to invert - the catalog,
//! warehouse and credential adapters are all still planned. A trait with no implementor and no
//! caller is a guess at a signature that only the first real adapter can settle, and in a
//! library crate `pub` hides it from `dead_code`, which is exactly how an unused item survives
//! review. The modules below are grouped by concept so that a port lands next to the types it
//! speaks in when it arrives, rather than in a module named after the trait.

pub mod definitions;
pub mod identity;
