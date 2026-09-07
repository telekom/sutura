//! Which `SUTURA__*` variables this PROCESS has set, for a refusal that has to name them.
//!
//! `github.com/telekom/sutura#386`. Every settings key is reachable as one variable under this
//! prefix, so any of them can be what stopped a command - and the remedy printed under a refusal
//! named [`super::CONFIG_DIR_VARIABLE`] and [`super::ENVIRONMENT_VARIABLE`] and nothing else. For a
//! reader whose exported `SUTURA__SERVER__HOST` caused it, that points at two variables that are
//! neither set nor able to fix it. The process can see which ones it has, so it says so.
//!
//! A file of its own for the reason [`super::posture`] and [`super::inbound`] are: `settings.rs`
//! fails `cargo xtask max-lines` at a thousand lines, and this is separable along the same seam -
//! nothing here parses a value or refuses a deployment.

use std::ffi::OsString;

use super::{VARIABLE_PREFIX, VARIABLE_SEPARATOR};

/// The NAMES of the `SUTURA__*` variables this process has set, sorted.
///
/// **Names only, never values, and that is the security half rather than brevity.**
/// `SUTURA__SECURITY__ACCESS_TOKEN` is one of these, so printing the environment as pairs would put
/// a deployment credential into the text of a refusal - which goes to stderr, into a log, and into
/// whatever collects one.
///
/// **The limit, next to the claim: this is what is SET, not what was USED.** A name here is a
/// candidate for the refusal above it, not a diagnosis - it may be setting a key the refusal is not
/// about, and the refusal may be about a key that came from a file.
///
/// That is a choice rather than a wall: the pinned `config` records the origin of every value and
/// `crate::settings::read` discards it, which `github.com/telekom/sutura#440` measures and costs.
#[must_use]
pub fn configuration_variables_from_process() -> Vec<String> {
    prefixed(std::env::vars_os().map(|(name, _)| name))
}

/// The sorted, deduplicated `SUTURA__*` names out of an arbitrary listing.
///
/// Separated from the process read for the reason [`super::Sources`] itself is a value:
/// `std::env::set_var` is `unsafe` in this edition and this workspace forbids it, so a test cannot
/// arrange a process environment. Taking the names as an iterator makes the filtering a pure
/// function a test can drive, and leaves exactly one caller reading the real environment.
fn prefixed(names: impl Iterator<Item = OsString>) -> Vec<String> {
    // The full `SUTURA__` and not the bare prefix: `SUTURA_CONFIG_DIR` and `SUTURA_ENVIRONMENT`
    // start with `SUTURA` too and are named on their own in the same message, so matching the
    // prefix alone would list them twice under a heading that does not describe them.
    let prefix = format!("{VARIABLE_PREFIX}{VARIABLE_SEPARATOR}");
    // LOSSY rather than `into_string().ok()`, which would DROP a name that is not valid Unicode -
    // and dropping something in silence is the class of defect this change is one half of. A
    // mangled name a reader can still recognise beats a variable nothing mentions.
    let mut found: Vec<String> = names
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| name.starts_with(&prefix))
        .collect();
    found.sort_unstable();
    found.dedup();
    found
}

#[cfg(test)]
mod tests {
    use super::prefixed;

    /// The listing a process read would produce, without a process environment this edition will
    /// not let a test arrange.
    fn names(raw: &[&str]) -> Vec<std::ffi::OsString> {
        raw.iter().map(|n| std::ffi::OsString::from(*n)).collect()
    }

    #[test]
    fn the_overlay_names_are_the_prefixed_ones_and_not_the_two_that_look_like_them() {
        // The refusal a stray variable causes has to be able to NAME it. `SUTURA_CONFIG_DIR` and
        // `SUTURA_ENVIRONMENT` start with the same word and are already named on their own line of
        // that message, so matching the bare prefix would list them a second time under a heading
        // that does not describe them - they set no settings key.
        let found = prefixed(
            names(&[
                "SUTURA__SERVER__HOST",
                "SUTURA_CONFIG_DIR",
                "SUTURA_ENVIRONMENT",
                "PATH",
                "SUTURA__SECURITY__ACCESS_TOKEN",
                "SUTURAX",
            ])
            .into_iter(),
        );
        assert_eq!(found, vec!["SUTURA__SECURITY__ACCESS_TOKEN", "SUTURA__SERVER__HOST"]);

        // Sorted and deduplicated, so the message reads the same twice and a repeated name is one.
        let twice = prefixed(names(&["SUTURA__SERVER__PORT", "SUTURA__SERVER__HOST", "SUTURA__SERVER__PORT"]).into_iter());
        assert_eq!(twice, vec!["SUTURA__SERVER__HOST", "SUTURA__SERVER__PORT"]);

        // And nothing at all is an empty list rather than a guess: the empty answer is what rules
        // the overlay out for a reader, so it has to be distinguishable rather than absent.
        assert_eq!(prefixed(names(&["PATH", "HOME"]).into_iter()), Vec::<String>::new());
    }
}
