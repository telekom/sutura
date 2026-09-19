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
    let text = String::from_utf8(output.stdout).ok()?;
    let (_, after) = text.rsplit_once("load average")?;
    let first = after
        .trim_start_matches(|c: char| !c.is_ascii_digit())
        .split([',', ' '])
        .next()?;
    first.parse().ok()
}
