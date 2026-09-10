//! The discriminator that would have caught issue #564, over the ONE shared sampler.
//!
//! `measure-host` used to print a number labelled `mean=…% busy` that was NOT utilisation: its
//! sampler built `total` from every `/proc/stat` field except idle and iowait and then fed it the
//! SYSTEM column as "idle", so the value it computed was the user-mode share of CONSUMED CPU time,
//! which can be high while the machine sits nearly idle. `host-probe` computed true busy. The two
//! actions had independent samplers, the formulas disagreed, and a number drawn from the wrong one
//! was read as utilisation all over #562 and the issue list.
//!
//! The test data IS the issue's table. Two scenarios with IDENTICAL user and system time but true
//! busy of 15% and 3%:
//!
//! | scenario | true busy | broken `measure-host` | fixed (shared sampler) |
//! | --- | --- | --- | --- |
//! | A | 15% | 66 | 15 |
//! | B | 3% | 66 | 3 |
//!
//! Before the fix the sampler returned the SAME number (66) for both A and B; after it returns
//! DIFFERENT numbers (15 vs 3). That is the discriminating pair, and this test holds it against
//! the ACTUAL shared sampler by executing `nix/cpu-busy.sh`.
//!
//! MUTATION PROOF: the shared sampler is where `measure-host` and `host-probe` get their busy
//! now. If anyone regresses `cpu_busy` back to the user-share formula, scenario A and B come out
//! IDENTICAL here and `different_busy_is_seen_as_different` turns red immediately - the same way
//! the defect was first silent. A test that only asserted "a number between 0 and 100" would pass
//! on the broken formula; this one does not.

use std::path::{Path, PathBuf};
use std::process::Command;

/// The repository's one shared busy sampler, relative to this xtask crate.
fn sampler() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("nix/cpu-busy.sh")
}

/// Busy percent (one decimal) the shared sampler reports between `prev` and `cur` cpu lines.
fn busy_between(prev: &str, cur: &str) -> f64 {
    let script = sampler();
    let output = Command::new("bash")
        .arg("-c")
        .arg(r#"source "$1"; cpu_busy "$2" "$3""#)
        .arg("cpu_busy_test")
        .arg(&script)
        .arg(prev)
        .arg(cur)
        .output()
        .expect("bash and the shared sampler are present");
    assert!(
        output.status.success(),
        "shared sampler failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("sampler prints utf-8")
        .trim()
        .parse()
        .expect("sampler prints a busy percent")
}

#[test]
fn different_true_busy_is_reported_as_different() {
    // The issue's exact pair: identical user(66) and system(34), idle differs so true busy is
    // 15% (A) vs 3% (B). Fields are user nice system idle iowait irq softirq steal.
    let prev = "cpu 0 0 0 0 0 0 0 0";
    let scenario_a = "cpu 66 0 34 566 0 0 0 0";
    let scenario_b = "cpu 66 0 34 3233 0 0 0 0";

    let busy_a = busy_between(prev, scenario_a);
    let busy_b = busy_between(prev, scenario_b);

    // AFTER THE FIX the two genuinely-different busy values MUST differ - the mutation proof, and
    // the assertion that was impossible under the broken formula (it returned 66 for both).
    assert!(
        (busy_a - busy_b).abs() > 5.0,
        "true busy differs (15% vs 3%) but the sampler collapsed them: A={busy_a}, B={busy_b} - the user-share formula is back"
    );
    assert!((busy_a - 15.0).abs() < 1.0, "scenario A should read ~15%, got {busy_a}");
    assert!((busy_b - 3.0).abs() < 1.0, "scenario B should read ~3%, got {busy_b}");
}

#[test]
fn the_broken_user_share_formula_was_identical_for_that_pair() {
    // BEFORE the fix the sampler computed `100*(total-system)/total` over a total that EXCLUDED
    // idle and iowait - i.e. user / (user+system), a share of CONSUMED CPU that has no idea how
    // busy the machine is. For both A and B that is 66, which is exactly the collapse the test
    // above now refuses. Held here as the reference for what the fix changed, and as the idiom a
    // line-count check could not have caught.
    let user: f64 = 66.0;
    let system: f64 = 34.0;
    let user_share_a = 100.0 * user / (user + system);
    let user_share_b = 100.0 * user / (user + system);
    assert_eq!(user_share_a, user_share_b);
    assert!((user_share_a - 66.0).abs() < 0.5, "the issue measured 66 for both");
}
