//! The warm-start gate: one target directory, spelled in two files, checked to be one directory.
//!
//! `nix/cargo-env.nix` unpacks the dependency closure the checks already built into a directory
//! under `target/`, and `xtask/src/causality.rs` points `CARGO_TARGET_DIR` at a directory it
//! computes for its own two runs. Those are the same directory, and until this gate the only thing
//! saying so was a comment in the nix module - which named the seam, said to change one and change
//! the other, and closed by admitting that nothing checked it.
//!
//! WHY IT NEEDS A GATE RATHER THAN THE COMMENT. Drift here does not break anything. The unpack
//! still succeeds, cargo still builds, `test-causality` still reaches the same verdict - it just
//! reaches it having compiled the whole closure a second time, which measured 9m48s of a 12m16s
//! CI step on the run that prompted the warm start. A gate that goes green while the thing it
//! guards has stopped working is exactly the failure this repo keeps deleting rows over, and a
//! wasted quarter-hour nobody attributes to a rename is the version of it that survives longest.
//!
//! HOW IT READS THEM. Text, not evaluation, for the reason `pins.rs` gives: this has to run on a
//! host with no nix. Neither side is found by searching for the literal `causality-target`, which
//! would be a gate that passes as long as the string exists somewhere in each file. The nix side is
//! read through the MECHANISM instead - what `CARGO_TARGET_DIR` is actually exported as, followed
//! to a variable's assignment when that is what it names - so an export rewired to some other
//! variable is caught as well as a renamed directory. The Rust side is the `join` chain of the
//! binding whose value every `cargo_test` call receives as `CARGO_TARGET_DIR`.
//!
//! FAIL CLOSED, like its neighbours. An unreadable file, an export this gate cannot follow, or a
//! binding it cannot find is a FAILURE naming what it could not find. A path-reading gate's worst
//! outcome is to stop finding the path and say `ok`.

use std::path::Path;

use crate::Verdict;
use crate::repo;

/// The nix module that unpacks the inherited artifacts into the directory.
const WARMER: &str = "nix/cargo-env.nix";

/// The gate that builds into it.
const CONSUMER: &str = "xtask/src/causality.rs";

/// What [`WARMER`] must export, up to the value.
const EXPORT: &str = "export CARGO_TARGET_DIR=\"";

/// The binding in [`CONSUMER`] whose `join` chain is the path. Its value is handed to every
/// `cargo_test` call as `CARGO_TARGET_DIR`, which is what makes it the other half of this pair.
const BINDING: &str = "let shared_target =";

/// The call this gate reads a path component out of.
const JOIN: &str = "join(\"";

pub(crate) fn run(_args: &[String]) -> Verdict {
    let Some(root) = repo::root() else {
        eprintln!("xtask check-warm-start: could not locate the repo root");
        return Verdict::Fail;
    };
    let warmed_path = read(&root, WARMER).and_then(|text| warmed(&text));
    let built_path = read(&root, CONSUMER).and_then(|text| built(&text));
    decide(warmed_path, built_path)
}

/// What to say about the two paths.
///
/// Separated from [`run`] so the comparison has no tree to read and can be tested on both
/// answers - which is the half of a gate that is otherwise only ever exercised green.
fn decide(warmed_path: Result<String, String>, built_path: Result<String, String>) -> Verdict {
    let (left, right) = match (warmed_path, built_path) {
        (Err(why), _) | (Ok(_), Err(why)) => {
            eprintln!("xtask check-warm-start: {why}");
            eprintln!();
            eprintln!("This gate reads a path out of two files and could not read one of them, so");
            eprintln!("it has checked NOTHING. That is a failure rather than a pass on purpose.");
            return Verdict::Fail;
        }
        (Ok(left), Ok(right)) => (left, right),
    };

    if left == right {
        println!("xtask check-warm-start: ok - {WARMER} and {CONSUMER} both name {left}");
        return Verdict::Pass;
    }

    eprintln!("xtask check-warm-start: the warm start and the causality gate name DIFFERENT directories\n");
    eprintln!("  {WARMER}   unpacks into  {left}");
    eprintln!("  {CONSUMER}  builds into   {right}");
    eprintln!();
    eprintln!("Nothing fails from this, which is the problem: `test-causality` would compile the");
    eprintln!("whole dependency closure again rather than reuse what the checks already built,");
    eprintln!("and still reach the same verdict several minutes later. Change one, change both.");
    Verdict::Fail
}

fn read(root: &Path, rel: &str) -> Result<String, String> {
    std::fs::read_to_string(root.join(rel)).map_err(|error| format!("could not read {rel}: {error}"))
}

/// The repo-relative directory [`WARMER`] warms, followed from the export rather than matched.
///
/// TWO SPELLINGS, and the first draft of this gate handled only one. An export whose value is a
/// bare `$variable` is followed to that variable's assignment; an export naming the path inline is
/// read where it stands. Getting that wrong is not a false pass but it is close enough: the first
/// version chased the leading variable of an inline path and reported `dev/null || pwd)` as the
/// warmed directory, which is a red gate for the wrong reason and no easier to read than a green
/// one for the wrong reason. Found by the test below, which is why the case is in it.
fn warmed(text: &str) -> Result<String, String> {
    let exported = exported_value(text).ok_or_else(|| {
        format!("{WARMER} exports no `{EXPORT}..\"`, so this gate cannot tell which directory the warm start fills")
    })?;
    let raw = match bare_variable(&exported) {
        Some(name) => assigned(text, name)
            .ok_or_else(|| format!("{WARMER} exports CARGO_TARGET_DIR from ${name} and assigns {name} nowhere"))?,
        None => exported,
    };
    beneath_the_root(&raw).ok_or_else(|| format!("{WARMER} warms {raw:?}, which this gate cannot reduce to a path in the repo"))
}

/// The double-quoted value `CARGO_TARGET_DIR` is exported as.
fn exported_value(text: &str) -> Option<String> {
    for line in text.lines() {
        let trimmed = line.trim_start();
        // A comment discussing the export is not the export.
        if trimmed.starts_with('#') {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix(EXPORT) {
            let value: String = rest.chars().take_while(|c| *c != '"').collect();
            if !value.is_empty() {
                return Some(value);
            }
        }
    }
    None
}

/// The variable name, if this value is nothing but a reference to one.
fn bare_variable(value: &str) -> Option<&str> {
    let name = value.strip_prefix('$')?.trim_start_matches('{').trim_end_matches('}');
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return None;
    }
    Some(name)
}

/// The double-quoted value assigned to a shell variable.
fn assigned(text: &str, variable: &str) -> Option<String> {
    let prefix = format!("{variable}=\"");
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('#') {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix(prefix.as_str()) {
            let value: String = rest.chars().take_while(|c| *c != '"').collect();
            if !value.is_empty() {
                return Some(value);
            }
        }
    }
    None
}

/// A shell path, as a path relative to the repo root.
///
/// `$warmRoot/target/x` is the root plus a relative path, so the leading variable goes. A value
/// still holding a `$` after that is an interpolation this gate cannot resolve, and is refused
/// rather than compared as text - comparing it would be a verdict about a string nobody wrote.
fn beneath_the_root(raw: &str) -> Option<String> {
    let path = match raw.strip_prefix('$') {
        Some(rest) => rest.split_once('/').map(|(_, tail)| tail)?,
        None => raw,
    };
    if path.is_empty() || path.contains('$') {
        return None;
    }
    Some(String::from(path))
}

/// The repo-relative directory [`CONSUMER`] builds into, read off its `join` chain.
fn built(text: &str) -> Result<String, String> {
    let at = text
        .find(BINDING)
        .ok_or_else(|| format!("{CONSUMER} no longer binds `{BINDING}`, which is where this gate reads the path"))?;
    let tail = text.get(at..).unwrap_or_default();
    let end = tail.find(';').unwrap_or(tail.len());
    let components = joined(tail.get(..end).unwrap_or_default());
    if components.is_empty() {
        return Err(format!(
            "{CONSUMER} binds `{BINDING}` with no `{JOIN}..\")` in it, so this gate cannot read a path from it"
        ));
    }
    Ok(components.join("/"))
}

/// Every string literal passed to a `join` in one expression, in order.
fn joined(expression: &str) -> Vec<String> {
    let mut components = Vec::new();
    let mut rest = expression;
    while let Some(at) = rest.find(JOIN) {
        let tail = rest.get(at.saturating_add(JOIN.len())..).unwrap_or_default();
        let end = tail.find('"').unwrap_or(tail.len());
        if let Some(literal) = tail.get(..end)
            && !literal.is_empty()
        {
            components.push(String::from(literal));
        }
        rest = tail;
    }
    components
}

#[cfg(test)]
mod tests {
    use crate::Verdict;

    /// The nix side, with the two decoys a real file has: a comment that names the path in prose,
    /// and a second variable assigned beside the one that matters.
    const NIX: &str = concat!(
        "  # THE TARGET DIRECTORY IS NAMED IN TWO PLACES. `causality.rs` computes\n",
        "  # `<root>/target/causality-target` and reads no environment variable for it.\n",
        "  cargoWarmStart = ''\n",
        "    warmRoot=\"$(git rev-parse --show-toplevel 2>/dev/null || pwd)\"\n",
        "    warmTarget=\"$warmRoot/target/causality-target\"\n",
        "    export CARGO_HOME=\"$warmRoot/target/causality-cargo-home\"\n",
        "    # export CARGO_TARGET_DIR=\"$somethingElse\"\n",
        "    export CARGO_TARGET_DIR=\"$warmTarget\"\n",
        "  '';\n",
    );

    /// The Rust side, with a `join` chain before the binding that must not be read as the path.
    const RUST: &str = concat!(
        "    let wt = root.join(\"target\").join(\"causality-worktree\");\n",
        "    let shared_target = root.join(\"target\").join(\"causality-target\");\n",
        "    let (head_ok, head_out) = cargo_test(root, &shared_target);\n",
    );

    #[test]
    fn the_nix_path_is_the_one_the_export_actually_points_at() {
        // Read through the export rather than by matching the literal, so a comment quoting the
        // path contributes nothing and `CARGO_HOME` next to it is not mistaken for the answer.
        assert_eq!(super::warmed(NIX).as_deref(), Ok("target/causality-target"));
        // A variable rename that carries the export with it is not drift, and must not be reported
        // as any: the directory is still the same one.
        let renamed = NIX
            .replace("warmTarget=\"", "warmDir=\"")
            .replace("\"$warmTarget\"", "\"$warmDir\"");
        assert_eq!(super::warmed(&renamed).as_deref(), Ok("target/causality-target"));
        // And an export that names the path inline instead of through a variable is read where it
        // stands. The first version of this chased `warmRoot` here and answered `dev/null || pwd)`.
        let inline = NIX.replace("\"$warmTarget\"", "\"$warmRoot/target/somewhere-else\"");
        assert_eq!(super::warmed(&inline).as_deref(), Ok("target/somewhere-else"));
    }

    #[test]
    fn the_rust_path_is_the_whole_join_chain_of_the_right_binding() {
        // The whole chain: reading only the first component would compare `target` against
        // `target/causality-target` and be permanently red, and reading the wrong binding would
        // compare the WORKTREE directory - a wrong answer that looks like a real one.
        assert_eq!(super::built(RUST).as_deref(), Ok("target/causality-target"));
    }

    #[test]
    fn agreeing_spellings_pass_and_disagreeing_ones_fail() {
        let path = || Ok(String::from("target/causality-target"));
        assert_eq!(super::decide(path(), path()), Verdict::Pass);
        assert_eq!(
            super::decide(path(), Ok(String::from("target/causality"))),
            Verdict::Fail,
            "a directory the warm start does not fill must not pass"
        );
    }

    #[test]
    fn a_path_this_gate_cannot_find_is_red_rather_than_green() {
        // Each of the three ways the read can come up empty, and then the verdict for it. A
        // path-reading gate that says `ok` having found no path is the failure mode here, and the
        // message has to name what was not found or the failure is unactionable.
        let no_export = super::warmed("cargoWarmStart = ''\n  warmTarget=\"$warmRoot/target/x\"\n''").unwrap_err();
        assert!(no_export.contains("exports no"), "{no_export}");
        let unassigned = super::warmed("export CARGO_TARGET_DIR=\"$nothingAssignsThis\"").unwrap_err();
        assert!(unassigned.contains("nothingAssignsThis nowhere"), "{unassigned}");
        let no_binding = super::built("fn prove(root: &Path) -> Verdict { Verdict::Pass }").unwrap_err();
        assert!(no_binding.contains("let shared_target ="), "{no_binding}");
        assert_eq!(
            super::decide(Err(String::from("could not read it")), Ok(String::from("target/x"))),
            Verdict::Fail
        );
        assert_eq!(
            super::decide(Ok(String::from("target/x")), Err(String::from("could not read it"))),
            Verdict::Fail
        );
    }

    #[test]
    fn both_real_files_still_yield_a_path() {
        // Caught here and not only on a branch, for the reason the COUNTS table's own test gives:
        // a reader that matches nothing makes its gate pass vacuously. This asserts the two files
        // are still SHAPED the way the gate reads them; whether they AGREE is the gate's verdict.
        let root = crate::repo::root().expect("the repo root");
        let warmed_path = super::read(&root, super::WARMER).and_then(|text| super::warmed(&text));
        let built_path = super::read(&root, super::CONSUMER).and_then(|text| super::built(&text));
        assert!(warmed_path.is_ok(), "{warmed_path:?}");
        assert!(built_path.is_ok(), "{built_path:?}");
    }
}
