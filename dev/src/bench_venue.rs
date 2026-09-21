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
    let cores = std::thread::available_parallelism().ok().map(std::num::NonZeroUsize::get);
    let cores_f64 = cores.map(|value| f64::from(u32::try_from(value).unwrap_or(u32::MAX)));
    match (load_average_1m(), cores, cores_f64) {
        (Some(load), Some(cores), Some(cores_f64)) if load > cores_f64 => println!(
            "venue: load average {load:.2} over {cores} core(s) - MORE runnable work than this \
             host has cores. These numbers are not comparable with ones taken on an idle host."
        ),
        (Some(load), Some(cores), _) => println!("venue: load average {load:.2} over {cores} core(s)"),
        (Some(load), None, _) => println!("venue: load average {load:.2}, core count unknown"),
        (None, ..) => {
            println!("venue: load average COULD NOT BE READ - treat every number below as taken under unknown conditions");
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
    use super::first_load_average;

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
}
