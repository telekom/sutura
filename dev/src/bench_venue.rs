//! Whether the host was too busy for a benchmark's number to mean anything.
//!
//! A fourth half of this crate, unrelated to the other three: `benches/*.rs` under `crates/` is a
//! venue `cargo xtask check-jscpd` refuses to exempt a clone in, so the load-average probe two
//! independent bench binaries both print before measuring anything lives here once instead of
//! twice - `github.com/telekom/sutura#915`. A measurement taken under this repository's usual
//! multi-lane load is not comparable with one taken idle, and the point of [`print()`] is that a
//! reader of the numbers below it never has to take that on faith.

/// Prints the host's own 1-minute load average beside its core count, before anything is
/// measured, and says so loudly when there is more runnable work than this host has cores.
pub fn print() {
    println!(
        "{}",
        line(
            load_average_1m(),
            std::thread::available_parallelism().ok().map(std::num::NonZeroUsize::get)
        )
    );
}

/// The line [`print()`] prints, as a value so the two arms a reader acts on are held by cells
/// rather than by whoever last read the output: the contention threshold that decides whether a
/// number is quotable, and the fail-closed message an unreadable load produces - a reading that
/// cannot be taken must not present itself as a plausible small number, which is the same defect
/// class the decimal-comma parse fixes.
fn line(load: Option<f64>, cores: Option<usize>) -> String {
    let cores_f64 = cores.map(|value| f64::from(u32::try_from(value).unwrap_or(u32::MAX)));
    match (load, cores, cores_f64) {
        (Some(load), Some(cores), Some(cores_f64)) if load > cores_f64 => format!(
            "venue: load average {load:.2} over {cores} core(s) - MORE runnable work than this \
             host has cores. These numbers are not comparable with ones taken on an idle host."
        ),
        (Some(load), Some(cores), _) => format!("venue: load average {load:.2} over {cores} core(s)"),
        (Some(load), None, _) => format!("venue: load average {load:.2}, core count unknown"),
        (None, ..) => {
            String::from("venue: load average COULD NOT BE READ - treat every number below as taken under unknown conditions")
        }
    }
}

/// The 1-minute load average, read the two ways this workspace's build platforms expose it:
/// Linux (and the nix sandbox) publishes it as the first field of `/proc/loadavg`; a host with no
/// such file - every developer's own macOS machine - has it as the tail of `uptime`'s own
/// sentence instead.
fn load_average_1m() -> Option<f64> {
    if let Ok(text) = std::fs::read_to_string("/proc/loadavg") {
        return text.split_whitespace().next()?.parse().ok();
    }
    let output = std::process::Command::new("uptime").output().ok()?;
    first_load_average(&String::from_utf8(output.stdout).ok()?)
}

/// `uptime`'s first average, under either decimal separator.
///
/// **Measured at cost, `github.com/telekom/sutura#140`.** `uptime` formats the three averages in
/// the invoking shell's locale, and a locale that writes a decimal comma - the one this was found
/// on - prints `load average: 23,66, 30,98, 26,01`, where the field separator and the decimal
/// point are the same character. Splitting on the comma therefore read `23` and silently dropped
/// the fraction, so a host at 5,90 announced itself as `5.00`: the probe understated exactly the
/// number that decides whether a measurement is quotable, in the direction that makes a busy host
/// look quiet.
///
/// The field is cut at the first `", "` instead, which is the separator in both formats, and any
/// comma left inside it is then the decimal point. **The limit:** a locale that writes a decimal
/// comma AND no space after the separator leaves this unable to parse, and [`print()`] then says the
/// average could not be read - fail-closed to the honest message rather than to a low number.
fn first_load_average(uptime: &str) -> Option<f64> {
    let (_, after) = uptime.rsplit_once("load average")?;
    let averages = after.trim_start_matches(|c: char| !c.is_ascii_digit());
    let first = averages.split_once(", ").map_or(averages, |(first, _)| first);
    first.trim().replace(',', ".").parse().ok()
}

#[cfg(test)]
mod tests {
    use super::{first_load_average, line};

    /// The defect this parse exists for: the same host, printed by the same `uptime`, under the
    /// two locales this repository's developers and its Linux runners actually run in.
    #[test]
    fn a_decimal_comma_keeps_its_fraction_and_a_decimal_point_is_unchanged() {
        for line in [
            " 9:50  up 21 days, 9 users, load average: 5,90, 30,98, 26,01\n",
            " 9:50  up 21 days, 9 users, load average: 5.90, 30.98, 26.01\n",
        ] {
            assert_eq!(first_load_average(line), Some(5.90), "{line}");
        }
    }

    /// A single average with nothing after it - no separator to cut at - and text with no average
    /// at all, which has to be `None` rather than a low number a reader would take for quiet.
    #[test]
    fn a_lone_average_is_read_and_an_absent_one_is_not_invented() {
        assert_eq!(first_load_average("load average: 0,25"), Some(0.25));
        assert_eq!(first_load_average("load average: 0.25\n"), Some(0.25));
        assert_eq!(first_load_average(" 9:50  up 21 days, 9 users"), None);
    }

    /// The contention threshold, pinned at both sides of it. This is the comparison the PR's own
    /// quiet-host evidence rests on - "the harness's own contention warning did not fire" - and it
    /// was held by nothing: widening the threshold to ten times the core count left the whole
    /// suite green, and `-D dead-code` cannot stand in because the field is still read. A load
    /// clearly above the core count must warn; one clearly below must not.
    #[test]
    fn a_load_over_the_core_count_warns_and_one_under_it_does_not() {
        let over = line(Some(16.0), Some(4));
        assert!(
            over.contains("MORE runnable work"),
            "a load clearly above the core count must warn: {over}"
        );
        assert!(over.contains("16.00"), "{over}");
        let under = line(Some(2.0), Some(4));
        assert!(
            !under.contains("MORE runnable work") && under.contains("venue: load average 2.00 over 4 core(s)"),
            "a load clearly below the core count must not warn: {under}"
        );
        // The boundary itself is the threshold, stated: not ten times it, which was the mutation
        // that went green. Both sides asserted from the same predicate so a `<`-for-`>` swap or a
        // `>=`-for-`>` swap cannot satisfy one arm and slip past the other.
        let at = line(Some(4.0), Some(4));
        assert!(
            !at.contains("MORE runnable work"),
            "the threshold is strictly greater-than, not greater-or-equal: {at}"
        );
    }

    /// The fail-closed arm's CONTENT, not just its `None`-ness: the message that an unreadable
    /// load prints is what a reader of the numbers below it acts on, and it must refuse the
    /// reading rather than present one. The mutation this cell exists for replaced the whole
    /// message with a plausible low reading over an unknown core count and stayed green.
    #[test]
    fn an_unreadable_load_refuses_rather_than_inventing_a_reading() {
        let line = line(None, Some(4));
        assert!(
            line.contains("COULD NOT BE READ"),
            "the refusal must say the load could not be read, not present a reading: {line}"
        );
        // No number a reader would take for quiet may appear beside an unreadable load.
        assert!(!line.contains("0.00"), "{line}");
    }
}
