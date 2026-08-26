//! A panic that reaches the log rather than only the terminal.
//!
//! **A hook, not a `catch_unwind`, and the shipped profiles are why.** `panic = "abort"` is set on
//! every profile this repository ships, so there is no unwinding to catch: the process is going to
//! die. What a hook can still do is run *first*, on the panicking thread, while the payload and the
//! location are in hand - so the last thing in the log says what happened and where, instead of
//! the log simply stopping.
//!
//! That distinction is the whole value. A container that exits with a message on standard error and
//! nothing in the collected log looks, from the outside, exactly like one that was evicted.
//!
//! The previous hook is kept and called afterwards, so the default rendering and any backtrace
//! still appear. Replacing it outright would trade one loss for another.

use std::sync::OnceLock;

/// Installed once. A second call would nest one hook inside another and log each panic twice.
static INSTALLED: OnceLock<()> = OnceLock::new();

/// Makes every subsequent panic emit a `tracing` error before the default hook runs.
///
/// Call it before the first thread is spawned and after the subscriber is installed. Before the
/// subscriber it would still work - `tracing` drops events with no subscriber rather than failing -
/// but the panic it was installed for would be the one that is lost.
///
/// Idempotent: the second and later calls do nothing.
pub fn install_panic_hook() {
    if INSTALLED.set(()).is_err() {
        return;
    }
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // Nothing in here may panic. A panic inside a panic hook aborts immediately with no
        // message at all, which would turn a diagnostic into the very silence it exists to
        // prevent - so this reads the payload defensively and formats the location by hand.
        tracing::error!(
            panic.message = %message_of(info.payload()),
            panic.location = %location_of(info),
            panic.thread = %thread_name(),
            "PANIC - this process is going down"
        );
        previous(info);
    }));
}

/// The panic message, whatever shape the payload arrived in.
///
/// A payload is `&'static str` for `panic!("literal")` and `String` for a formatted one, and it can
/// be any other type entirely - `panic_any` takes anything. The third case gets a placeholder
/// rather than a guess.
fn message_of(payload: &(dyn core::any::Any + Send)) -> String {
    if let Some(text) = payload.downcast_ref::<&'static str>() {
        return String::from(*text);
    }
    if let Some(text) = payload.downcast_ref::<String>() {
        return text.clone();
    }
    String::from("<a panic payload that is not a string>")
}

/// File, line and column, or a note that the location was not recorded.
///
/// `Option` because it genuinely can be absent: a panic raised through `panic_any` from code with
/// no caller location carries none.
fn location_of(info: &std::panic::PanicHookInfo<'_>) -> String {
    info.location().map_or_else(
        || String::from("<no location recorded>"),
        |at| format!("{}:{}:{}", at.file(), at.line(), at.column()),
    )
}

/// The panicking thread's name, or its absence.
///
/// Worth a field of its own: a panic on a worker thread and a panic on the thread accepting
/// connections have different consequences, and the default rendering buries the difference.
fn thread_name() -> String {
    std::thread::current()
        .name()
        .map_or_else(|| String::from("<unnamed>"), String::from)
}

#[cfg(test)]
mod tests {
    use super::{install_panic_hook, message_of};

    #[test]
    fn a_string_literal_payload_is_read_back_verbatim() {
        assert_eq!(message_of(&"boom"), "boom");
    }

    #[test]
    fn a_formatted_payload_is_read_back_verbatim() {
        assert_eq!(message_of(&String::from("boom 42")), "boom 42");
    }

    #[test]
    fn a_payload_that_is_not_a_string_gets_a_placeholder_rather_than_a_guess() {
        // `panic_any` accepts any type. Rendering one with `Debug` is not possible through `Any`,
        // so the honest answer is to say the payload was not text.
        assert!(message_of(&7_u32).starts_with('<'));
    }

    #[test]
    fn installing_twice_is_a_no_op_rather_than_two_nested_hooks() {
        // The reason for the `OnceLock`. Without it a second call wraps the first, and every panic
        // is reported once per installation - which reads, in a log, as several panics.
        install_panic_hook();
        install_panic_hook();
    }

    /// The hook, end to end: it must produce a traced ERROR carrying the message and the location.
    ///
    /// This works because the test profile unwinds. The shipped profiles abort, and the hook still
    /// runs there - it runs before the strategy is applied either way - which is exactly what makes
    /// a hook the right mechanism and `catch_unwind` the wrong one.
    #[test]
    fn a_panic_is_traced_before_the_process_gives_up_on_it() {
        install_panic_hook();

        let recorded = crate::testing::capture(|| {
            drop(std::panic::catch_unwind(|| {
                panic!("a deliberate panic, for the hook");
            }));
        });

        assert!(recorded.contains("a deliberate panic, for the hook"), "{recorded}");
        assert!(
            recorded.contains("panics.rs"),
            "the location should name this file: {recorded}"
        );
        assert!(recorded.contains("PANIC"), "{recorded}");
    }
}
