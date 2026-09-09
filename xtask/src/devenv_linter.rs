//! The EMISSION, read out of the store: is `shellcheck` still what reads a devenv script body?
//!
//! `crate::devenv_shell` holds the structure - every body goes through `linted`, and `linted`
//! hands `writeShellApplication` nothing that could replace its `checkPhase`. That argument is
//! complete over the tree and it is still an argument about TEXT. This is the measurement.
//!
//! # Why it exists
//!
//! `github.com/telekom/sutura#402`'s seventh escape defeated the linter rather than the rule:
//! `checkPhase = "true";` inside `linted` removes `bash -n` AND `shellcheck` from every body, and
//! `just hygiene` stayed at exit 0 because every gate held the spelling `runs`/`sourced` rather
//! than what the wrapper emitted. The pre-#395 state it restored is bit-for-bit readable -
//! `checkPhase: ""` on the derivation - which is what makes reading the derivation the assertion
//! rather than a proxy for it.
//!
//! # What it reads, and why that needs no network
//!
//! One argument: the store path of a body the wrapper produced, interpolated by nix at the call
//! site so it IS this tree's wrapper and not a re-evaluation of it. Then
//! `nix-store --query --deriver` names the derivation that built it - a local lookup, no
//! evaluation and no flake inputs - and `nix derivation show` prints its `checkPhase`. **The exit
//! status of neither command is the evidence**; the text of the phase is, which is why [`holds`]
//! takes a string and is unit-tested over the two shapes that matter.
//!
//! # Limits
//!
//! * **It needs a store the wrapper has been built into**, so it is not a hygiene gate: the sweep
//!   runs inside a nix derivation with no nix. `just devenv-linter` is the venue, and
//!   `just ship-check` runs it when a diff touches a devenv module.
//! * **CI never runs it.** `devenv.nix`'s own header says CI does not use that file, so no pull
//!   request builds a dev shell. The structural half runs on every pull request inside `hygiene`;
//!   this half is a developer-machine and pre-push measurement, and saying otherwise would be the
//!   overstated control this repository treats as the defect.
//! * **It reads ONE body's derivation.** That is enough because there is one wrapper: `linted` is
//!   a single function and `check-devenv-shell` refuses a second application of the builder, so a
//!   `checkPhase` any body got is the `checkPhase` this one got. If that ever stops being true,
//!   the gate that holds it is the one to change.
//! * **It asserts the FLAGS the tool was invoked with, never its findings.** `-x` is required
//!   because that is the flag the 14 tracked `*.sh` files get; `excludeShellChecks` would suppress
//!   findings and is refused at the argument set instead, by `crate::devenv_shell`.
//! * **It does not check WHICH shellcheck, and cannot claim it is the one CI uses.** The two
//!   locks name different nixpkgs: `flake.lock`'s is `NixOS/nixpkgs` `83199d0d`, which is what
//!   `nix run .#shellcheck` resolves through, and `devenv.lock`'s is `cachix/devenv-nixpkgs`
//!   `256551e4`, which is what this wrapper resolves through (measured 2026-09-06, from the two
//!   lock files). So a `checkPhase` here can name a different store path from the app the
//!   `*.sh` glob runs, and #395's commit message claiming *"the same nixpkgs"* was wrong. What
//!   is asserted is that a shellcheck is in the phase at all, by store path.

use std::process::Command;

use crate::Verdict;

/// The tool the phase has to name.
const SHELLCHECK: &str = "/bin/shellcheck";

/// The syntax check nixpkgs runs before it.
const SYNTAX: &str = "bash -n";

/// The flag the tracked `*.sh` files get, and these bodies now get too.
///
/// Asserted here rather than trusted to stay in `devenv.nix`, because it arrives through
/// `extraShellCheckFlags` - a list interpolated into the default phase - and a list is exactly the
/// kind of argument that gets emptied without anything noticing. Read out of the phase, so what is
/// held is the flag the tool was INVOKED with.
const FOLLOW: &str = "-x";

/// What a `checkPhase` has to contain, or why it does not.
///
/// A function over the phase TEXT rather than over a command's status, because a subprocess that
/// exits 0 is evidence about the subprocess. Returns the shellcheck store path so the verdict
/// names what it found - a reader can then compare it against `nix run .#shellcheck` by hand.
fn holds(phase: &str) -> Result<String, String> {
    if phase.trim().is_empty() {
        return Err(String::from(
            "the checkPhase is EMPTY - the body was neither syntax-checked nor linted. That is \
             bit-for-bit the state this repository shipped before the wrapper existed",
        ));
    }
    if !phase.contains(SYNTAX) {
        return Err(format!(
            "the checkPhase does not run `{SYNTAX}`, so a body that does not PARSE would build:\n    {}",
            phase.trim()
        ));
    }
    let found = phase.split_whitespace().find(|token| token.contains(SHELLCHECK));
    let Some(found) = found else {
        return Err(format!(
            "the checkPhase names no `{SHELLCHECK}` - something replaced it, and every script body \
             in the dev shell is then shell nothing reads:\n    {}",
            phase.trim()
        ));
    };
    if !found.starts_with("/nix/store/") {
        return Err(format!(
            "the checkPhase reaches shellcheck as `{found}` rather than by store path, so which \
             shellcheck runs depends on PATH"
        ));
    }
    // The flag, read off the INVOCATION line rather than off the whole phase, so a `-x` inside a
    // comment or a filename is not the evidence.
    let invocation = phase.lines().find(|line| line.contains(found)).unwrap_or_default();
    if !invocation.split_whitespace().any(|token| token == FOLLOW) {
        return Err(format!(
            "the checkPhase runs shellcheck without `{FOLLOW}`, so a body that sources another \
             file is linted without it - the 14 tracked `*.sh` files get that flag. It arrives \
             through `extraShellCheckFlags` in `devenv.nix`, which is a list something emptied:\n\
             \x20   {}",
            invocation.trim()
        ));
    }
    Ok(String::from(found))
}

/// The `checkPhase` of the derivation that built `store_path`.
fn check_phase(store_path: &str) -> Result<String, String> {
    let deriver = run_nix(&["nix-store", "--query", "--deriver", store_path])?;
    let deriver = deriver.trim();
    let is_derivation = std::path::Path::new(deriver)
        .extension()
        .is_some_and(|extension| extension == "drv");
    if !is_derivation {
        return Err(format!(
            "`nix-store --query --deriver` answered `{deriver}` for {store_path} - no derivation, \
             so there is nothing to read. Build the dev shell first: `devenv shell true`"
        ));
    }
    let shown = run_nix(&["nix", "derivation", "show", deriver])?;
    let json: serde_json::Value =
        serde_json::from_str(&shown).map_err(|error| format!("`nix derivation show` was not valid JSON: {error}"))?;
    phase_of(&json, deriver)
}

/// The phase out of either shape `nix derivation show` prints.
///
/// Nix 2.34 wraps the map in `{"version": 4, "derivations": {...}}` and earlier versions print the
/// bare map. Both are read rather than one being assumed, because a gate that stops parsing on a
/// tool upgrade reports a finding about this repository that is not one.
fn phase_of(json: &serde_json::Value, deriver: &str) -> Result<String, String> {
    let map = json.get("derivations").unwrap_or(json);
    // The fallback for a key spelling this version does not use, and it is GUARDED by the map's
    // SIZE. Unguarded, `values().next()` would pick an arbitrary derivation and the verdict would
    // be a phase read off something that is not the wrapper, printed as though it were - the same
    // standard this module applies to a command's exit status, applied to the parse.
    let entries = map.as_object().map_or(0, serde_json::Map::len);
    let sole = if entries == 1 {
        map.as_object().and_then(|object| object.values().next())
    } else {
        None
    };
    let entry = map.get(deriver).or(sole).ok_or_else(|| {
        format!(
            "`nix derivation show` printed {entries} derivation(s) and none keyed by {deriver}, so \
             which one is the wrapper's is not readable here"
        )
    })?;
    entry
        .get("env")
        .and_then(|env| env.get("checkPhase"))
        .and_then(serde_json::Value::as_str)
        .map(String::from)
        .ok_or_else(|| {
            format!(
                "the derivation for {deriver} declares NO checkPhase at all - the wrapper is not \
                 `writeShellApplication` any more, and nothing read the body"
            )
        })
}

/// Run one nix command and return its stdout, or say what went wrong.
fn run_nix(argv: &[&str]) -> Result<String, String> {
    let (program, rest) = argv.split_first().ok_or_else(|| String::from("no command"))?;
    let output = Command::new(program)
        .args(rest)
        .output()
        .map_err(|error| format!("could not run `{}`: {error}", argv.join(" ")))?;
    if !output.status.success() {
        return Err(format!(
            "`{}` failed: {}",
            argv.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

pub(crate) fn run(args: &[String]) -> Verdict {
    let [store_path] = args else {
        eprintln!("usage: cargo xtask check-devenv-linter <store-path-of-a-linted-body>");
        eprintln!();
        eprintln!("The path is interpolated by nix at the call site, so it is THIS tree's wrapper");
        eprintln!("rather than a re-evaluation of it. `just devenv-linter` is how to run this.");
        return Verdict::Usage;
    };

    let phase = match check_phase(store_path) {
        Ok(phase) => phase,
        Err(reason) => return refuse(&reason),
    };
    match holds(&phase) {
        Ok(shellcheck) => {
            println!("xtask check-devenv-linter: ok - the wrapper's checkPhase runs `{SYNTAX}` then {shellcheck}");
            Verdict::Pass
        }
        Err(reason) => refuse(&reason),
    }
}

fn refuse(reason: &str) -> Verdict {
    eprintln!("xtask check-devenv-linter: FAILED");
    eprintln!("  {reason}");
    eprintln!();
    eprintln!("`linted` in devenv.nix is the only thing that reads a devenv script body. Its");
    eprintln!("checkPhase is nixpkgs' own, and `cargo xtask check-devenv-shell` refuses any");
    eprintln!("argument to the builder that could replace it - so a failure here means the");
    eprintln!("wrapper, the builder or nixpkgs changed, and every body is unread until it is fixed.");
    Verdict::Fail
}

#[cfg(test)]
mod tests {
    use super::{holds, phase_of};

    /// The phase measured on this tree's `sutura-ship-check` derivation, 2026-09-07.
    ///
    /// The `-x` between the store path and `"$target"` is `extraShellCheckFlags` expanding. On
    /// 2026-09-06, before that argument was passed, the same position held a DOUBLE SPACE - the
    /// empty list - and that was the evidence, already in this file, that the flag needed no
    /// `checkPhase` override.
    const REAL: &str = "runHook preCheck\n\
        /nix/store/szpwxfkbnw35ayiav64w8vxipsf6viwl-bash-5.3p15/bin/bash -n -O extglob \"$target\"\n\
        # use shellcheck which does not include docs\n\
        /nix/store/1pddyb3y5gkhbwqra3r4fg5xmjg0xxi7-ShellCheck-0.11.0/bin/shellcheck -x \"$target\"\n\
        \nrunHook postCheck\n";

    #[test]
    fn the_measured_phase_names_the_syntax_check_and_a_store_shellcheck() {
        let found = holds(REAL).expect("the real phase holds");
        assert!(found.ends_with("/bin/shellcheck"), "{found}");
        assert!(found.starts_with("/nix/store/"), "{found}");
    }

    #[test]
    fn the_seventh_escape_is_what_this_refuses() {
        // `checkPhase = "true";` in the wrapper, which is what #402 measured green everywhere else.
        let refusal = holds("true").expect_err("a phase that lints nothing refuses");
        assert!(refusal.contains("bash -n"), "{refusal}");
        // And the pre-wrapper state, which is the same defect with no line to point at.
        let empty = holds("").expect_err("an empty phase refuses");
        assert!(empty.contains("EMPTY"), "{empty}");
    }

    #[test]
    fn a_syntax_check_with_no_linter_is_not_enough() {
        let syntax_only = "runHook preCheck\n/nix/store/x-bash/bin/bash -n \"$target\"\nrunHook postCheck\n";
        let refusal = holds(syntax_only).expect_err("bash -n alone is not the linter");
        assert!(refusal.contains("/bin/shellcheck"), "{refusal}");
    }

    #[test]
    fn a_shellcheck_taken_from_path_is_refused() {
        let loose = "runHook preCheck\n/nix/store/x-bash/bin/bash -n \"$target\"\nshellcheck/bin/shellcheck -x \"$target\"\n";
        let refusal = holds(loose).expect_err("a non-store shellcheck is a PATH lookup");
        assert!(refusal.contains("depends on PATH"), "{refusal}");
    }

    #[test]
    fn an_emptied_flag_list_is_refused() {
        // `extraShellCheckFlags` is a LIST interpolated into the default phase, so emptying it
        // leaves a phase that still runs the linter and no longer follows a `source`. That is the
        // 2026-09-06 phase verbatim, double space and all.
        let without = REAL.replace("/bin/shellcheck -x ", "/bin/shellcheck  ");
        let refusal = holds(&without).expect_err("no -x is a refusal");
        assert!(refusal.contains("without `-x`"), "{refusal}");
        // And a `-x` that is not on the invocation line is not the evidence.
        let commented = REAL.replace("# use shellcheck which does not include docs", "# -x");
        let elsewhere = commented.replace("/bin/shellcheck -x ", "/bin/shellcheck  ");
        assert!(holds(&elsewhere).is_err(), "a -x in a comment is not the flag");
    }

    #[test]
    fn a_map_with_two_derivations_is_a_refusal_rather_than_a_guess() {
        // The unguarded fallback would have taken whichever came first and printed its phase as
        // the wrapper's.
        let two = serde_json::json!({
            "/nix/store/a.drv": { "env": { "checkPhase": "phase-a" } },
            "/nix/store/b.drv": { "env": { "checkPhase": "phase-b" } }
        });
        let refusal = phase_of(&two, "/nix/store/c.drv").expect_err("two entries and no key refuses");
        assert!(refusal.contains("2 derivation(s)"), "{refusal}");
    }

    #[test]
    fn both_shapes_of_derivation_show_are_read() {
        let wrapped = serde_json::json!({
            "version": 4,
            "derivations": { "/nix/store/a.drv": { "env": { "checkPhase": "phase-a" } } }
        });
        assert_eq!(phase_of(&wrapped, "/nix/store/a.drv").expect("v4"), "phase-a");
        let bare = serde_json::json!({ "/nix/store/a.drv": { "env": { "checkPhase": "phase-b" } } });
        assert_eq!(phase_of(&bare, "/nix/store/a.drv").expect("bare"), "phase-b");
        // A derivation with no checkPhase at all is the `writeShellScriptBin` case, and it is a
        // refusal rather than an empty string treated as a phase.
        let none = serde_json::json!({ "/nix/store/a.drv": { "env": { "name": "x" } } });
        let refusal = phase_of(&none, "/nix/store/a.drv").expect_err("no phase refuses");
        assert!(refusal.contains("NO checkPhase"), "{refusal}");
    }
}
