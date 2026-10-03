//! The OAuth bearer the LINKED libpq signs in with - the workspace's second `unsafe` site.
//!
//! **Why it exists.** libpq 18 compiles its OAUTHBEARER client and `PQsetAuthDataHook`
//! unconditionally, and only its built-in device flow needs libcurl, which the static libpq is
//! built without (`nix/postgres-adbc.nix`). libpq has no connection keyword for a bearer: a token
//! reaches it only as a hook's answer to `PQAUTHDATA_OAUTH_BEARER_TOKEN`. The hook is process-global
//! in libpq and the ADBC driver offers no option for it, so it is installed here, from outside the
//! driver, into the one libpq this artefact links.
//!
//! **Which token it answers with.** The one [`with_bearer`] holds for the calling thread while
//! `dial` runs, and nothing on any other thread or at any other time. The PostgreSQL driver dials
//! synchronously on the caller's thread - once in `DatabaseInit`, once in `ConnectionInit` - so a
//! thread-local read by one global hook answers exactly the dials it wraps. Outside one, the hook
//! hands the request to `PQdefaultAuthDataHook`, which in this libpq has no flow to offer, so libpq
//! refuses the sign-in by name.
//!
//! **Compiled only where the PostgreSQL archive is linked** (`cfg(adbc_postgres_driver_linked)`),
//! for `linked`'s reason, with `linked`'s limit: no `--all-features` gate's clippy run judges it.
//! `cargo xtask check-unsafe` reads it whether or not a compiler does, the `adbc-driver-postgresql`
//! check holds [`BearerRequest`]'s fields to libpq's own header, and the sign-in venue in
//! `nix/shipped.nix` links and RUNS it against a server.

use core::cell::Cell;
use core::ffi::{CStr, c_char, c_int, c_void};
use std::sync::Once;

// `PQAUTHDATA_OAUTH_BEARER_TOKEN`, the second `PGauthData` enumerator (`libpq-fe.h`), so `1`.
const OAUTH_BEARER_TOKEN: c_int = 1;

/// libpq 18's `PGoauthBearerRequest`, field for field: `openid_configuration`, `scope`, `async`,
/// `cleanup`, `token`, `user`. Only `token` is written; the two callbacks stay null, which is
/// libpq's "the token is already here".
#[repr(C)]
struct BearerRequest {
    openid_configuration: *const c_char,
    scope: *const c_char,
    r#async: *const c_void,
    cleanup: *const c_void,
    token: *mut c_char,
    user: *mut c_void,
}

// libpq's hook installer and its default hook, from the archive `../build.rs` links.
//
// WHAT MAKES THE DECLARATIONS SOUND. libpq declares `PQsetAuthDataHook(PQauthDataHook_type)` and
// `int PQdefaultAuthDataHook(PGauthData, PGconn *, void *)`, with `PQauthDataHook_type` that same
// shape. `PGauthData` is a C enum, passed as an `int` on every target this repository builds;
// `PGconn *` is opaque here and only ever handed back. The archive is built from the flake-pinned
// libpq, the same source whose header the `adbc-driver-postgresql` check reads [`BearerRequest`]'s
// fields from, so a libpq bump that reshaped the request fails that check rather than mis-writing.
//
// WHAT WOULD BREAK IT. A libpq that changed the hook's signature without touching the request
// struct: nothing reads the signature line, so that is review's, at a libpq bump.
#[expect(
    unsafe_code,
    reason = "naming libpq's C ABI hook functions is `unsafe extern` on edition 2024"
)]
unsafe extern "C" {
    #[link_name = "PQsetAuthDataHook"]
    fn set_auth_data_hook(hook: extern "C" fn(c_int, *mut c_void, *mut c_void) -> c_int);
    #[link_name = "PQdefaultAuthDataHook"]
    fn default_auth_data_hook(kind: c_int, conn: *mut c_void, data: *mut c_void) -> c_int;
}

thread_local! {
    /// The NUL-terminated bearer the current dial on this thread signs in with, or null.
    static BEARER: Cell<*const c_char> = const { Cell::new(core::ptr::null()) };
}

/// libpq's auth-data hook: the held bearer for a bearer request, libpq's default for anything else.
///
/// `1` is libpq's "handled": it then copies `token` before this thread's dial returns, which is
/// before [`with_bearer`] lets the bytes go.
#[expect(
    unsafe_code,
    reason = "libpq hands this hook a struct of its own to fill, through the C ABI"
)]
extern "C" fn hook(kind: c_int, conn: *mut c_void, data: *mut c_void) -> c_int {
    let bearer = BEARER.with(Cell::get);
    if kind != OAUTH_BEARER_TOKEN || bearer.is_null() || data.is_null() {
        // SAFETY: the arguments are libpq's own, handed back unchanged to libpq's own default.
        return unsafe { default_auth_data_hook(kind, conn, data) };
    }
    // SAFETY: for `PQAUTHDATA_OAUTH_BEARER_TOKEN` libpq's `data` is a live `PGoauthBearerRequest`
    // (checked non-null above) that it owns for this call, laid out as `BearerRequest`; `token` is
    // the only field written, and its bytes outlive the dial (see `with_bearer`).
    unsafe { (*data.cast::<BearerRequest>()).token = bearer.cast_mut() };
    1
}

/// Runs `dial` with `bearer` as the token this thread's libpq signs in with, and only then.
///
/// The hook is installed once per process, on first use. The bearer is cleared when `dial`
/// returns or unwinds, so a later dial on this thread is not signed in with it.
#[expect(unsafe_code, reason = "installing the hook is a call into libpq's C ABI")]
pub(crate) fn with_bearer<T>(bearer: &CStr, dial: impl FnOnce() -> T) -> T {
    struct Cleared;
    impl Drop for Cleared {
        fn drop(&mut self) {
            BEARER.with(|held| held.set(core::ptr::null()));
        }
    }
    static INSTALLED: Once = Once::new();
    // SAFETY: `hook` has the C signature libpq's `PQauthDataHook_type` names, and libpq stores it
    // as a plain function pointer.
    INSTALLED.call_once(|| unsafe { set_auth_data_hook(hook) });
    BEARER.with(|held| held.set(bearer.as_ptr()));
    let _cleared = Cleared;
    dial()
}
