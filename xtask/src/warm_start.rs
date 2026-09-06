//! The warm-start gate: one target directory, one cargo profile, one sweep - where they are used.
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
//! WHAT ELSE IT HOLDS, one level down the same seam. `check-default-feature-tests` derives cargo's
//! profile from [`STAMP`] existing inside `CARGO_TARGET_DIR` rather than from a flag, so two more
//! facts have to stay true of these same two files: the warmer WRITES the stamp into the directory
//! it exports, and `flake.nix` BUILDS the artifacts it unpacks at [`WARM_PROFILE`]. Both were
//! assertions in that gate's own test module, as `contains` over raw text - a commented-out write
//! satisfied the first and a stamp moved out of the exported directory satisfied it too. They are
//! here because this module owns both literals, and a gate in `just hygiene` is a stronger venue
//! than a unit test.
//!
//! AND THE THIRD THING THE SAME SEAM CARRIES, which is #336 and #346: the artifacts are only
//! usable where nothing in them still names the build root they were produced in.
//! `inheritedArtifacts` pairs every `cargoArtifacts` with `nix/purge-baked-out-dirs.sh` and said in
//! its own comment that a consumer therefore *cannot* take one without the other - held, until
//! [`pairing`], by review alone, on both routes. See that module for what a *taking* is and why the
//! count of them is a witness rather than a number in a sentence.
//!
//! FAIL CLOSED, like its neighbours. An unreadable file, an export this gate cannot follow, a
//! binding it cannot find, a taking it cannot attribute or a consumer it did not reach is a
//! FAILURE naming what it could not find. A path-reading gate's worst outcome is to stop finding
//! the path and say `ok`.

use std::path::Path;

use crate::Verdict;
use crate::repo;

/// Nothing takes the inherited artifacts without the regeneration sweep - #336 and #346.
///
/// Its own file because this one is a third of the way to the 1000-line cap already, and its own
/// MODULE because the question is different: this file holds two spellings of one directory
/// against each other, and that one holds every taking of the artifacts against the sweep that
/// makes them usable. What they share is the seam, which is why they share a gate.
mod pairing;

/// The nix module that unpacks the inherited artifacts into the directory.
const WARMER: &str = "nix/cargo-env.nix";

/// The stamp `cargoWarmStart` writes beside the unpacked artifacts, naming the closure it unpacked.
pub(crate) const STAMP: &str = ".sutura-warm-start";

/// The profile the warm artifacts were built at.
///
/// Here rather than beside each reader because this module is already the OWNER of that fact:
/// [`profiled_consumers`] fails the build when any `${cargoWarmStart}` consumer in `flake.nix` does
/// not pass it, including the `--cargo-profile`-versus-`--profile` distinction. A third Rust spelling
/// of `"ci"` next to a gate that already enforces it is a copy that can go stale while the gate
/// stays green.
pub(crate) const WARM_PROFILE: &str = "ci";

/// The gate that builds into it.
const CONSUMER: &str = "xtask/src/causality.rs";

/// The cargo profile to compile at, from the target directory alone.
///
/// HERE because this module owns both literals it reads and both facts that make reading them
/// sound - [`stamped`] holds that the warmer writes the stamp into the directory it exports, and
/// [`built_at_the_warm_profile`] that those artifacts carry this profile. Two gates derive it now
/// (`check-default-features` and `check-default-feature-tests`), and a second copy of the
/// derivation beside one of them is how they come to disagree while both stay green.
///
/// `None` is cargo's default and the developer's answer: `just gates` runs in an ordinary
/// `target/`, where naming a profile would buy a second dependency build for a verdict that does
/// not depend on the profile. Inside the warmed directory the answer is the profile those
/// artifacts carry, because anything else silently reuses none of them.
///
/// The SIGNAL IS THE STAMP AND NOT THE DIRECTORY'S NAME, which is the distinction this module's
/// header draws about its own readers: a name can be renamed while a matcher keeps passing,
/// whereas the stamp exists if and only if the unpack happened.
pub(crate) fn profile_for(target_dir: Option<&Path>) -> Option<&'static str> {
    if target_dir.is_some_and(|dir| dir.join(STAMP).is_file()) {
        return Some(WARM_PROFILE);
    }
    None
}

/// The apps that consume the warmed artifacts.
const APPS: &str = "flake.nix";

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
    let paths = decide(warmed_path, built_path);
    if paths != Verdict::Pass {
        return paths;
    }
    // The stamp's two halves. `check-default-feature-tests` derives cargo's profile from the
    // stamp being present in `CARGO_TARGET_DIR`, so *written into the directory this warms* and
    // *those artifacts built at `WARM_PROFILE`* are properties of these same two files, and they
    // belong to the module that owns both literals rather than to that gate's own test module.
    for (rel, holds) in [
        (WARMER, stamped as fn(&str) -> Result<(), String>),
        (APPS, built_at_the_warm_profile),
    ] {
        if let Err(why) = read(&root, rel).and_then(|text| holds(&text)) {
            eprintln!("xtask check-warm-start: {why}");
            return Verdict::Fail;
        }
    }
    println!("xtask check-warm-start: ok - {WARMER} stamps that directory and {APPS} builds it at profile {WARM_PROFILE}");
    let profiles = read(&root, APPS).and_then(|text| profiled_consumers(&text));
    let consumers = match profiles {
        Ok(consumers) => consumers,
        Err(why) => {
            eprintln!("xtask check-warm-start: {why}");
            return Verdict::Fail;
        }
    };
    println!(
        "xtask check-warm-start: ok - {} app(s) consume the artifacts at profile {WARM_PROFILE}, each in the directory {WARMER} warmed",
        consumers.count()
    );
    // THE PAIRING, which is the other half of what makes an inherited artifact usable: the
    // directory and the profile say WHERE the closure is and HOW it was built, and the sweep says
    // that nothing in it still names a build root it no longer sits in.
    match pairing::holds(&root) {
        Ok(swept) => {
            println!("xtask check-warm-start: ok - {}", swept.verdict());
            Verdict::Pass
        }
        Err(why) => {
            eprintln!("xtask check-warm-start: {why}");
            Verdict::Fail
        }
    }
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

/// The lines of a nix file that could execute anything.
///
/// A `#` line is a comment in nix and a comment in the shell of an indented string alike, so
/// neither can be the export, the assignment or the write - and a comment DISCUSSING one is how
/// each of these readers would otherwise pass over a line that no longer runs. Stated once here
/// because three readers below need the same rule.
///
/// Deliberately NOT [`crate::workflows`]' Nix lexer, which blanks string interiors: every value
/// read here - the export, the assignment, the stamp write, the profile - lives inside one. So the
/// limit is that a needle inside a string on a live line is a live anchor to this scan, comment
/// syntax or not; what it buys is that a commented-OUT line is not one.
fn live_lines(text: &str) -> impl Iterator<Item = &str> {
    live_indexed(text).map(|(_, line)| line)
}

/// The same lines, each with its 0-based index, for the reader that needs an ORDER.
///
/// One rule in one place: [`pairing`] compares where the sweep is inlined against where the target
/// directory is exported, and a second copy of "which lines run" is a second copy to get wrong.
fn live_indexed(text: &str) -> impl Iterator<Item = (usize, &str)> {
    text.lines()
        .enumerate()
        .map(|(index, line)| (index, line.trim_start()))
        .filter(|(_, line)| !line.starts_with('#'))
}

/// The double-quoted value `CARGO_TARGET_DIR` is exported as.
fn exported_value(text: &str) -> Option<String> {
    for trimmed in live_lines(text) {
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
    for trimmed in live_lines(text) {
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

/// [`STAMP`] is written INTO the directory this module warms, not merely somewhere in the file.
///
/// `check-default-feature-tests` derives cargo's profile from the stamp being present in
/// `CARGO_TARGET_DIR`; a write that moved out of the warmed directory would leave that gate
/// compiling at the developer's default profile inside CI's warmed one - a whole dependency build
/// bought back, and nothing red. Followed from the EXPORT for [`warmed`]'s reason: the literal
/// appears in this file's own prose and in `xtask/` besides, so finding it anywhere in the text
/// checks that the string exists rather than that the write does.
fn stamped(text: &str) -> Result<(), String> {
    let exported = exported_value(text).ok_or_else(|| {
        format!("{WARMER} exports no `{EXPORT}..\"`, so this gate cannot tell which directory the stamp belongs in")
    })?;
    let under = format!("{exported}/{STAMP}");
    if live_lines(text).any(|line| writes_to(line, under.as_str())) {
        return Ok(());
    }
    Err(format!(
        "{WARMER} writes no `{under}`: the stamp is not in the directory it exports, so `check-default-feature-tests` would compile at the wrong profile and reuse none of the warmed artifacts"
    ))
}

/// Does this line REDIRECT into `target`, rather than merely mention it?
///
/// Found by mutation, and it is the whole difference between this gate and the `contains` it
/// replaced: commenting the write out left `if [ "$(cat "$warmTarget/.sutura-warm-start" ...` on
/// the line above, which mentions the path, satisfies any scan for it, and READS a stamp nothing
/// writes any more. So the redirect's own target is what is compared.
///
/// The limit, and it is the safe direction: only a `>`/`>>` redirect counts, so a write through
/// `install` or `tee` would fail this gate rather than pass it. `venues::acceptance` asks the
/// neighbouring question - is a redirect's target a file at all - and cannot answer this one.
fn writes_to(line: &str, target: &str) -> bool {
    line.split('>')
        .skip(1)
        .any(|rest| rest.trim_start().trim_start_matches('"').starts_with(target))
}

/// The warmed artifacts are BUILT at [`WARM_PROFILE`], which is what makes deriving it sound.
///
/// The other half of [`stamped`]: the stamp says artifacts are there, this says which profile they
/// carry. What it does NOT reach is which argument set the warm start's `cargoArtifacts` comes
/// from - that binding is nix, and this gate evaluates none.
fn built_at_the_warm_profile(text: &str) -> Result<(), String> {
    let declared = format!("CARGO_PROFILE = \"{WARM_PROFILE}\"");
    if live_lines(text).any(|line| line.contains(declared.as_str())) {
        return Ok(());
    }
    Err(format!(
        "{APPS} declares no `{declared}`, so the artifacts the warm start unpacks are no longer built at profile {WARM_PROFILE}"
    ))
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

/// The expansion that makes an app a consumer of the warmed artifacts.
const WARM_EXPANSION: &str = "${cargoWarmStart}";

/// Every `cargoWarmStart` consumer, each of them INSPECTED.
///
/// The field is private and [`Consumers::over`] is the only constructor, which refuses unless it
/// was handed one inspection per consumer the scan found. So the count in the verdict is the
/// witness's own length: a loop that stopped early cannot print a number as if it had not, which
/// is the defect this repository has recorded in four gates - a report of `17 page(s)` with
/// sixteen unscanned.
#[derive(Debug)]
struct Consumers {
    inspected: Vec<usize>,
}

impl Consumers {
    fn over(found: &[usize], inspected: Vec<usize>) -> Result<Self, String> {
        if found.is_empty() {
            return Err(format!(
                "{APPS} contains no `{WARM_EXPANSION}` consumer; this gate checked nothing"
            ));
        }
        if inspected.len() != found.len() {
            return Err(format!(
                "inspected {} of {} `{WARM_EXPANSION}` consumer(s), so this verdict is about a subset",
                inspected.len(),
                found.len()
            ));
        }
        Ok(Self { inspected })
    }

    const fn count(&self) -> usize {
        self.inspected.len()
    }
}

/// Where every consumer of the warmed artifacts expands the warmer, as 0-based line indices.
fn warm_consumers(lines: &[&str]) -> Vec<usize> {
    lines
        .iter()
        .enumerate()
        .filter(|(_, line)| line.trim() == WARM_EXPANSION)
        .map(|(index, _)| index)
        .collect()
}

/// The lines of one consumer's script, from its expansion to the end of the shell string.
fn consumer_body<'a>(lines: &[&'a str], index: usize) -> Vec<&'a str> {
    lines
        .iter()
        .skip(index.saturating_add(1))
        .map(|line| line.trim())
        .take_while(|line| !line.ends_with("'');"))
        .collect()
}

/// The two things that have to be true of one consumer.
fn inspect_consumer(lines: &[&str], index: usize) -> Result<(), String> {
    let body = consumer_body(lines, index);
    let at = index.saturating_add(1);

    // ONE: THE DIRECTORY THE WARMER LEFT IT IN. Everything the warm start buys is scoped to the
    // directory it exports - the unpacked closure, the stamp `profile_for` derives the profile
    // from, and (since #346) the sweep that regenerates whatever baked a build root. A consumer
    // that points `CARGO_TARGET_DIR` somewhere else afterwards keeps all three and uses none of
    // them: cold build, no stamp, and a sweep that reported about a directory this run does not
    // compile into. Nothing fails from it, which is why it is a gate and not a comment.
    if let Some(own) = body.iter().find(|line| !line.starts_with('#') && line.starts_with(EXPORT)) {
        return Err(format!(
            "{APPS}:{at} expands `{WARM_EXPANSION}` and then exports its own target directory: {own}\n  \
             The unpacked closure, the {STAMP} stamp and the baked-OUT_DIR sweep all belong to the directory \
             {WARMER} exported, so this consumer would compile cold into another one with nothing red."
        ));
    }

    let command = body
        .iter()
        .find(|line| line.starts_with("exec cargo "))
        .ok_or_else(|| format!("{APPS}:{at} warms cargo but executes no cargo command"))?;
    let words: Vec<&str> = command.split_whitespace().collect();
    let flag = if words.starts_with(&["exec", "cargo", "nextest"]) {
        "--cargo-profile"
    } else {
        "--profile"
    };
    // CARGO'S SIDE OF `--` ONLY, and that is a fail-open this gate had rather than a
    // refinement. Everything past `--` goes to the program cargo RUNS, so a `--profile ci`
    // there selects no profile for the build - it reuses none of the 756 MB just unpacked,
    // compiles the closure again, and reaches the same verdict several minutes later with
    // nothing red anywhere. Scanning the whole line accepted exactly that line, which is the
    // shape a flag gets moved into when a gate grows an argument of its own.
    let cargo_side = words.split(|word| *word == "--").next().unwrap_or(&words);
    if !cargo_side.windows(2).any(|pair| pair == [flag, WARM_PROFILE]) {
        return Err(format!(
            "{APPS}:{at} warms profile {WARM_PROFILE} but its cargo command does not pass `{flag} {WARM_PROFILE}` \
             BEFORE `--`; everything after that separator goes to the program cargo runs, so a \
             profile there selects none for the build: {command}"
        ));
    }
    Ok(())
}

/// Check every app that expands `cargoWarmStart`, rather than naming today's five consumers.
///
/// TWO PASSES on purpose. The first says which consumers exist and the second inspects them, so
/// the two numbers are separately obtained and [`Consumers::over`] can compare them. One pass
/// counting as it goes cannot tell a complete inspection from an early return.
fn profiled_consumers(text: &str) -> Result<Consumers, String> {
    let lines: Vec<&str> = text.lines().collect();
    let found = warm_consumers(&lines);
    let mut inspected = Vec::new();
    for index in &found {
        inspect_consumer(&lines, *index)?;
        inspected.push(*index);
    }
    Consumers::over(&found, inspected)
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

    /// The stamp write, as `WARMER` has it: a read guarding the unpack and the write after it.
    const STAMP_WRITE: &str = concat!(
        "    if [ \"$(cat \"$warmTarget/.sutura-warm-start\" 2>/dev/null)\" != \"${cargoArtifacts}\" ]; then\n",
        "      printf '%s' \"${cargoArtifacts}\" > \"$warmTarget/.sutura-warm-start\"\n",
        "    fi\n",
    );

    #[test]
    fn the_stamp_has_to_be_written_into_the_directory_this_warms() {
        let live = format!("{NIX}{STAMP_WRITE}");
        assert_eq!(super::stamped(&live), Ok(()));
        // COMMENTED OUT is the shape a text scan misses, and the one that actually happens: a step
        // gets parked and the line stays in the file. `contains` over the raw text passes here -
        // and so did the first version of this reader, because the `if [ "$(cat ...` line above
        // MENTIONS the stamp while writing nothing. See [`super::writes_to`].
        // Commented AT THE START OF THE LINE, which is the rule `live_lines` states: this
        // fixture first put the `#` mid-line, where it is a shell comment to a reader and not to
        // a line scan, and the test passed for the wrong reason until that was fixed.
        let parked = live.replace("      printf", "      # printf");
        assert!(super::stamped(&parked).is_err(), "a commented-out write is not a write");
        // And the same line with the `#` MID-line, which is a shell comment to a reader and not to
        // a line scan. `live_lines` says a line STARTS with one, so this stays live and the
        // redirect is still a redirect - stated because the fixture above got it wrong first.
        let mid = live.replace("printf '%s'", "true # printf '%s'");
        assert_eq!(super::stamped(&mid), Ok(()));
        // And the stamp moved OUT of the exported directory, which is the other way this gate's
        // reader goes green while `check-default-feature-tests` derives the wrong profile.
        let elsewhere = live.replace("$warmTarget/.sutura-warm-start", "$warmRoot/.sutura-warm-start");
        let moved = super::stamped(&elsewhere).expect_err("a stamp outside the warmed directory is not the stamp");
        assert!(moved.contains("writes no `$warmTarget/.sutura-warm-start`"), "{moved}");
        let unfollowable = super::stamped(STAMP_WRITE).expect_err("no export is no directory to judge");
        assert!(unfollowable.contains("exports no"), "{unfollowable}");
    }

    #[test]
    fn the_warmed_artifacts_have_to_be_built_at_the_profile_this_derives() {
        let live = "        ciArgs = commonArgs // { CARGO_PROFILE = \"ci\"; };\n";
        assert_eq!(super::built_at_the_warm_profile(live), Ok(()));
        let parked = format!("#{live}");
        assert!(
            super::built_at_the_warm_profile(&parked).is_err(),
            "a commented-out declaration builds nothing"
        );
        let released = live.replace("\"ci\"", "\"release\"");
        let wrong = super::built_at_the_warm_profile(&released).expect_err("release artifacts are not ci artifacts");
        assert!(wrong.contains("no longer built at profile ci"), "{wrong}");
    }

    #[test]
    fn both_new_anchors_are_still_live_in_the_real_tree() {
        // The other half of `both_real_files_still_yield_a_path`, and for its reason: a reader that
        // matches nothing on the real tree makes its gate pass over the thing it describes.
        let root = crate::repo::root().expect("the repo root");
        let warmer = super::read(&root, super::WARMER).and_then(|text| super::stamped(&text));
        let apps = super::read(&root, super::APPS).and_then(|text| super::built_at_the_warm_profile(&text));
        assert_eq!(warmer, Ok(()), "{warmer:?}");
        assert_eq!(apps, Ok(()), "{apps:?}");
    }

    #[test]
    fn every_warmed_app_uses_the_profile_that_was_warmed() {
        let apps = concat!(
            "            ${cargoWarmStart}\n",
            "            exec cargo run --profile ci -p xtask\n",
            "          '');\n",
            "            ${cargoWarmStart}\n",
            "            exec cargo nextest run --profile ci -p sutura-exec-bigquery\n",
            "          '');\n",
        );
        let error = super::profiled_consumers(apps).expect_err("nextest's configuration profile is not Cargo's ci profile");
        assert!(error.contains("does not pass `--cargo-profile ci`"), "{error}");
        assert_eq!(
            super::profiled_consumers(&apps.replace("nextest run --profile", "nextest run --cargo-profile"))
                .map(|consumers| consumers.count()),
            Ok(2)
        );
    }

    #[test]
    fn a_consumer_that_repoints_the_target_directory_leaves_everything_the_warmer_did_behind() {
        // Everything `cargoWarmStart` buys is scoped to the directory it exports: the unpacked
        // closure, the stamp `profile_for` reads, and - since #346 - the sweep. A consumer that
        // exports its own afterwards keeps the flag and loses all three, cold-builds, and is
        // green. `--profile ci` on the line below is deliberately correct, so the only thing
        // this test can be reddened by is the rule it is about.
        let apps = concat!(
            "            ${cargoWarmStart}\n",
            "            export CARGO_TARGET_DIR=\"$PWD/target/somewhere-else\"\n",
            "            exec cargo run -q --profile ci -p xtask -- test-causality\n",
            "          '');\n",
        );
        let error = super::profiled_consumers(apps).expect_err("a re-export abandons the warmed directory");
        assert!(error.contains("exports its own target directory"), "{error}");
        // COMMENTED OUT is not exported, which is the rule `live_lines` states one screen up and
        // the shape a raw scan gets wrong in the other direction.
        assert_eq!(
            super::profiled_consumers(&apps.replace("            export CARGO", "            # export CARGO"))
                .map(|consumers| consumers.count()),
            Ok(1)
        );
    }

    #[test]
    fn a_consumer_the_loop_never_reached_cannot_be_counted_as_inspected() {
        // The witness, directly: `Consumers::over` is the only constructor and it compares two
        // separately obtained numbers. A `.take(1)` over the inspection loop is the whole
        // mutation, and it has to be unable to mint a verdict.
        let found = [12, 34, 56];
        let why = super::Consumers::over(&found, vec![12]).expect_err("one of three is not every one");
        assert!(why.contains("inspected 1 of 3"), "{why}");
        let why = super::Consumers::over(&[], Vec::new()).expect_err("no consumer is not a pass");
        assert!(why.contains("checked nothing"), "{why}");
        assert_eq!(
            super::Consumers::over(&found, found.to_vec()).map(|consumers| consumers.count()),
            Ok(3)
        );
    }

    #[test]
    fn a_profile_written_past_the_separator_is_not_cargos_profile() {
        // The fail-open this reader had. `cargo run ... -- <gate> --profile ci` hands the flag to
        // the GATE, so cargo builds at the developer default, reuses none of the 756 MB just
        // unpacked, compiles the closure again and reaches the same verdict several minutes later.
        // Scanning the whole line accepted it, and this is the shape a flag gets moved into when a
        // gate grows a profile argument of its own - which `check-default-features` briefly did,
        // and is the reason it now derives the profile from `profile_for` instead.
        let past = concat!(
            "            ${cargoWarmStart}\n",
            "            exec cargo run -q -p xtask -- check-default-features --profile ci\n",
            "          '');\n",
        );
        let error = super::profiled_consumers(past).expect_err("a profile past `--` selects none for the build");
        assert!(error.contains("BEFORE `--`"), "{error}");
        // Cargo's own side still passes with a tail beside it, so the fix is not a ban on `--`.
        assert_eq!(
            super::profiled_consumers(&past.replace("run -q -p xtask", "run -q --profile ci -p xtask"))
                .map(|consumers| consumers.count()),
            Ok(1)
        );
    }
}
