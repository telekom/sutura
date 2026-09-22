//! The driver LINKED INTO this artefact, and the one `unsafe` declaration in this workspace.
//!
//! **Why it exists: the static musl artefacts.** A statically linked binary has no dynamic
//! loader, so `ManagedDriver::load_dynamic_from_filename` can never succeed on the two musl
//! triples `nix/shipped.nix` publishes - a fact this repository used to record as a limit and
//! assert in `nix/bigquery-driver-check.sh` in the negative direction. `adbc_driver_manager`
//! offers the other constructor for exactly this case, and it needs the driver's `AdbcDriverInit`
//! resolved at LINK time rather than found on disk. That is what `../../build.rs` arranges from
//! `nix/bigquery-adbc.nix`'s `c-archive` output.
//!
//! **Compiled only where the archive is linked** (`cfg(adbc_driver_linked)`), which is the limit
//! worth stating first: `just lint` and every `--all-features` cargo gate in this workspace take
//! the source build, so no gate's clippy run judges the lines below. What holds them instead is
//! `cargo xtask check-unsafe`, a text gate that reads this file whether or not a compiler does,
//! plus the `cross` release builds and `just bigquery-driver-check`, which link and RUN it.
//!
//! By hand, a compiler CAN be pointed at this file without a release build: exporting
//! `SUTURA_ADBC_BIGQUERY_ARCHIVE_DIR` at any driver derivation's `lib/` turns the `cfg` on for a
//! plain `cargo check`/`clippy`, measured on a darwin host against the x86_64-musl archive - the
//! metadata pass needs no linkable object. That is a route and not a mechanism; nothing runs it.

use adbc_core::error::{AdbcStatusCode, Error as CoreError};
use adbc_core::options::AdbcVersion;
use adbc_driver_manager::ManagedDriver;
use adbc_ffi::{FFI_AdbcDriverInitFunc, FFI_AdbcError};
use core::ffi::{c_int, c_void};

// The ADBC v1 C ABI entrypoint the Go `c-archive` exports.
//
// `//` and not `///`, because rustdoc generates nothing for an extern block and says so
// (`unused_doc_comments`, which `-D warnings` makes an error).
//
// WHAT MAKES THE DECLARATION SOUND. The signature is not written out here by hand and then hoped
// to match: it is `FFI_AdbcDriverInitFunc` applied to this item at `INIT`, so a driver-manager
// release that changed the ABI would not compile rather than mis-call. The symbol itself comes from
// `libadbc_driver_bigquery.a`, built from the flake-pinned driver source by the ONE derivation that
// also builds the `.so` this repository has been loading dynamically since `telekom/sutura#913` -
// the same entrypoint, the same revision, one build mode apart.
//
// WHAT WOULD BREAK IT. A second producer of this symbol name: the declaration resolves to whatever
// the linker found, so linking a different `adbc` driver archive into the same artefact would
// silently make this the wrong driver, and the link is one `cargo::rustc-link-lib` from one nix
// derivation with nothing in the type system to say so. And this is a raw `extern` declaration, so
// a build that compiled it against a target whose `c_int` differs from the archive's would be
// undefined behaviour - both are produced for the same triple by the same flake, which is the
// argument and not a check.
#[expect(unsafe_code, reason = "naming a C ABI symbol is `unsafe extern` on edition 2024")]
unsafe extern "C" {
    #[link_name = "AdbcDriverInit"]
    fn driver_init(version: c_int, driver: *mut c_void, error: *mut FFI_AdbcError) -> AdbcStatusCode;
}

/// The entrypoint as the driver manager's own function-pointer type.
///
/// A `const` rather than a cast at the call site, so the ABI check above happens once and
/// [`driver`] hands out a reference to something with a lifetime rather than to a temporary.
const INIT: FFI_AdbcDriverInitFunc = driver_init;

/// The driver this artefact carries, initialised through its linked-in entrypoint.
///
/// # Errors
///
/// Whatever the driver's own initialisation reported - the same [`CoreError`] the dynamic route
/// returns, so a caller cannot tell the two apart and does not have to.
pub(super) fn driver() -> Result<ManagedDriver, CoreError> {
    ManagedDriver::load_static(&INIT, AdbcVersion::default())
}
