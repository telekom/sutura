#![forbid(unsafe_code)]
//! The bounded, TLS-rotating `ureq` client `sutura-catalog-datahub`'s and
//! `sutura-catalog-openmetadata`'s real HTTP readers both build on.
//!
//! **Extracted, not designed up front** - `github.com/telekom/sutura#970`'s review found
//! `sutura-catalog-openmetadata/src/http.rs` and `sutura-catalog-datahub/src/http.rs` sharing an
//! `Endpoint` parser, a `ReadBounds`/`Budget` pair and a `rotating_agent` constructor byte-for-byte,
//! and the same shape again in each crate's own `tls_roots.rs` and `test_support.rs`.
//! `cargo xtask check-jscpd`'s allowlist (`devco/dup-ignore`) explicitly refuses an exemption for
//! anything under `crates/`, so the fix has to be structural: this crate is what
//! `.agents/skills/sutura/crate-map/SKILL.md` already argues `sutura-tls` is one layer down for - "a
//! small read or computation two same-class adapters both need... joins no existing prefix's rules
//! and starts in no forbidden class by construction". `sutura-catalog-*` (the "metadata providers"
//! class `xtask/src/boundaries/adapters.rs` names) may not reach another member of its own class,
//! but nothing forbids two of them reaching a THIRD, unprefixed crate - the same argument that
//! already lets both depend on `sutura-tls`.
//!
//! # What is generic, and what stays in each reader
//!
//! Everything here is protocol-agnostic ureq/TLS plumbing: it never names an entity kind, a wire
//! field, or a mapping. [`Endpoint::parse`]'s grammar, [`ReadBounds`]'s two settings, the shared
//! [`Budget`] a reader's own `read()` opens once, [`rotating_agent`]/[`fixed`]'s TLS wiring and the
//! anchor fold in `tls` are the same read for `DataHub`'s `OpenAPI` v3 surface and `OpenMetadata`'s
//! REST API alike. What stays in each reader crate: the entity-shaped `HttpReaderError` variants
//! (their `Display` text names the platform), the paged `fetch`/`entities` helpers built over
//! [`Budget`], and every `harvest_*` mapping function - `docs/what-openmetadata-can-carry.md` and
//! `sutura-catalog-datahub`'s own module header are explicit that those mappings are a first-party
//! claim about each platform's wire shape, which this crate must never blur by generalizing over.
//!
//! # TLS
//!
//! [`fixed`] and [`rotating_agent`] are the two ways a reader gets an outbound `ureq::Agent`: fixed
//! at construction over an already-loaded anchor bundle, or rebuilt on every
//! [`sutura_tls::POLL_INTERVAL`] poll from a `security.outbound` declaration
//! (`github.com/telekom/sutura#125`). Both fold through `tls::config`, the one place a loaded
//! [`sutura_tls::LoadedAnchors`]/[`sutura_tls::LoadedIdentity`] becomes `ureq::tls::TlsConfig` -
//! `sutura-tls` itself may never depend on `ureq` (`xtask/src/boundaries/edges.rs`'s
//! `sutura-tls -> ring` forbidden edge is what that would reintroduce), so the fold lives here
//! instead, the same reasoning `sutura-catalog-datahub`'s own `tls_roots.rs` module header gave
//! before this crate existed.

mod agent;
mod bounds;
mod budget;
mod endpoint;
mod message;
mod tls;

#[cfg(feature = "test")]
pub mod test_support;

#[cfg(feature = "tls-test")]
pub mod tls_test_support;

pub use agent::{OutboundAgent, fixed, rotating_agent};
pub use bounds::{DEFAULT_MAX_RESPONSE_BYTES, DEFAULT_TIMEOUT_SECONDS, InvalidReadBounds, ReadBounds};
pub use budget::Budget;
pub use endpoint::{Endpoint, InvalidEndpoint};
pub use message::EndpointMessage;
