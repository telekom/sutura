//! Every package that SHIPS compiles and lints at the feature set it ships with.
//!
//! **The lane no other gate can see, and it was found by shipping a red branch through it.** Every
//! compiling gate in this repository passes `--all-features`: `just lint`, `just test`,
//! `just check-changed` and the `clippy` and `nextest` nix checks alike. `nix/shipped.nix`, though,
//! builds each published binary with cargo's DEFAULT features - `--package` and `--target`, no
//! `--features` - because a non-optional networked adapter would cross-compile `ureq`, rustls and
//! `ring` for two musl triples for a binary that links none of them. So a `#[cfg(feature = "...")]`
//! that a developer only ever compiled with the feature ON can be a hard error in the exact
//! configuration a release publishes, and every gate a developer runs before pushing is green.
//!
//! That happened: `sutura-serve`'s boot pre-flight landed with three items reachable only from a
//! `bigquery` arm, `dead_code = "deny"` made all three errors with the feature off, and it reached
//! review as a green branch. The four `cross` link checks WOULD have caught it - they build
//! `.#sutura-serve-<triple>-ci` at the default set on every pull request - but they are
//! `needs: [ci]`, and `ci` had failed on something else, so they never ran. That is a sequencing
//! fact rather than coverage, and a gate a developer can run is the fix for it. The same review then
//! measured the LINT half of the same lane still red on a pre-existing `doc_markdown` in a
//! `#[cfg(not(feature = "bigquery"))]` doc comment, which is why this runs clippy as well as check:
//! the two commands see different code, and neither is `just lint`'s.
//!
//! **NOT a hygiene gate, for `check-attribution-current`'s reason.** It invokes cargo, so it needs a
//! resolvable registry and a target directory; the nix sandbox `hygiene` runs in has neither. So it
//! lives in `just gates`, which is where every gate that shells out to cargo lives.
//!
//! **CI runs it as `nix run .#default-features`, inside the one required job.** Before that step
//! existed, what CI had for this lane was the four `cross` link builds for the COMPILE half - and
//! they are `needs: [ci]`, so a `ci` failure skips them, which is exactly how the branch above
//! reached review - and nothing at all for the LINT half. An app rather than a check for the reason
//! the paragraph above gives. [`tests::both_lanes_still_invoke_this_gate`] is what holds the
//! wiring, in both venues, by reading the step rather than the file.
//!
//! **The profile is DERIVED from the target directory and not passed as a flag** -
//! [`crate::warm_start::profile_for`], shared with this lane's other half. Cargo keys artifacts per
//! profile, so compiling inside the warmed directory at anything but the profile those artifacts
//! carry reuses none of them: it rebuilds the closure, passes, and nobody attributes the minutes to
//! it. An earlier revision threaded `--profile <name>` through instead and put the flag where cargo
//! cannot read it - past the `--` on the app's own line, which selects a profile for the xtask
//! binary and none for the build - while `check-warm-start` still printed `ok`. A derivation has no
//! wrong side of a separator to be written on. `just gates` derives `None`, which is the
//! developer's default profile and no second dependency build.
//!
//! **THE REUSE IS PARTIAL, and read that before costing this step.** The closure holds dependency
//! units at the workspace-wide feature union; this gate deliberately asks the narrow question
//! instead - one shipped package, no feature flags - so the v2 resolver gives much of the graph a
//! narrower feature set, a fresh `-C metadata`, and a recompile. MEASURED in this gate's own CI job,
//! 2026-09-03: the `cargo check` pass compiled 104 units in 38.27 s for `sutura-cli` and 89 in
//! 41.00 s for `sutura-serve`, against `cargo tree --edges normal,build` graphs of 261 and 301
//! packages - about a third of each, and the EXPENSIVE third, because the whole
//! arrow/parquet/datafusion stack misses: 27 s of that first 38 s. The two clippy passes were
//! 3.96 s and 4.46 s only because cargo runs clippy-driver on the primary package alone, so they
//! consume what the check pass beside them just produced. Matching the closure would mean asking
//! about the whole workspace at once, which is the cross-member feature unification this gate
//! exists to see past - so the recompile is the price of the question and not a defect in it.
//!
//! **That cost is SHARED now, so the figure above will not reproduce alone.** It was taken before
//! `check-default-feature-tests` existed, and that gate compiles the same narrow configuration -
//! whichever of the two `ci` steps runs first pays the recompile and the second reuses it. They are
//! adjacent in `ci.yml` for that reason. Cost the pair, never this step by itself.
//!
//! **The package list is DERIVED and not written here**, which is the single-owner rule: it is every
//! `package = "..."` inside `nix/shipped.nix`'s `binaries` list, the same declaration
//! `check-shipped-binaries` compares the release literals against. A binary added there is covered by
//! this gate without anybody remembering to add it, and a package renamed in one place fails rather
//! than silently dropping out. FAIL CLOSED on parsing none, for that gate's own stated reason: a
//! parser that silently sees half a file is worse than no parser.
//!
//! **What it does NOT do**, stated because a green run invites the wider reading: it compiles the
//! default set only. A feature declared and never compiled by anything is still uncovered here - the
//! `--all-features` gates are what reach those - and this says nothing about a package that does not
//! ship. Nor does it link: `cargo check` and `cargo clippy` both stop at metadata, which is what
//! keeps it affordable and is also why the `cross` builds stay the authority on a musl link.
//!
//! **And it RUNS nothing, which for a whole category of test meant nobody did.** Stopping at
//! metadata compiles a `#[cfg(not(feature = "..."))]` test and never executes it, while every venue
//! that does run a test passes `--all-features`, where that cfg is false. `check-default-feature-tests`
//! is this lane's other half and its module header carries the measurement.

use std::path::Path;

use crate::Verdict;
use crate::repo;
use crate::warm_start::profile_for;

/// The declaration the package list is read out of.
pub(crate) const SOURCE: &str = "nix/shipped.nix";

/// Every `package = "..."` inside `nix/shipped.nix`'s `binaries = [ ... ]`, in declaration order.
///
/// Scoped to that list rather than grepping the file, and found anywhere on the line rather than at
/// its start - both for the reasons `shipped::declared` states at length beside its own parser: the
/// key appears elsewhere in that file, and `{ bin = "sutura"; package = "sutura-cli"; }` is one legal
/// record on one line. Duplicates are dropped, keeping first appearance, so two binaries out of one
/// package are one compile rather than two.
///
/// `pub(crate)` for exactly one other reader: `check-default-feature-tests` runs the same packages'
/// tests at the same feature set, and a second parser over one declaration is how two gates come to
/// disagree about which packages ship.
pub(crate) fn shipped_packages(text: &str) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    let mut indent: Option<usize> = None;
    for line in text.lines() {
        let trimmed = line.trim();
        let Some(open) = indent else {
            if trimmed.starts_with("binaries = [") {
                indent = Some(line.len().saturating_sub(line.trim_start().len()));
            }
            continue;
        };
        if trimmed == "];" && line.len().saturating_sub(line.trim_start().len()) == open {
            break;
        }
        for found in packages_in(line) {
            let owned = String::from(found);
            if !names.contains(&owned) {
                names.push(owned);
            }
        }
    }
    names
}

/// Every `package = "..."` value on one line, left to right.
///
/// The character before the key must not be part of a name, so a hypothetical `subPackage = "x"` is
/// not read as one. An unterminated quote yields nothing rather than the rest of the file.
fn packages_in(line: &str) -> impl Iterator<Item = &str> {
    const KEY: &str = "package = \"";
    let mut rest = line;
    core::iter::from_fn(move || {
        loop {
            let at = rest.find(KEY)?;
            let is_key = rest
                .get(..at)
                .and_then(|s| s.chars().next_back())
                .is_none_or(|c| !c.is_alphanumeric() && c != '_' && c != '-');
            let tail = rest.get(at.saturating_add(KEY.len())..)?;
            let end = tail.find('"')?;
            let value = tail.get(..end)?;
            rest = tail.get(end.saturating_add(1)..)?;
            if is_key && !value.is_empty() {
                return Some(value);
            }
        }
    })
}

/// One cargo invocation, named for the question it answers.
struct Pass {
    /// What this pass is called in the output.
    what: &'static str,
    /// The cargo subcommand and its arguments, before `--package`.
    lead: &'static [&'static str],
    /// Arguments after `--package`, which is where a lint level goes.
    tail: &'static [&'static str],
}

/// The two passes, and they are two because they see different code.
///
/// `check` answers *does the shipped configuration compile*; clippy answers *is it clean under this
/// workspace's lint set*, which includes the whole `restriction` category and `-D warnings`. A
/// `doc_markdown` on a `#[cfg(not(feature = ...))]` item is invisible to the first and to every
/// `--all-features` run, which is the measured case that put both here.
const PASSES: &[Pass] = &[
    Pass {
        what: "check",
        lead: &["check", "--all-targets"],
        tail: &[],
    },
    Pass {
        what: "clippy",
        lead: &["clippy", "--all-targets"],
        tail: &["--", "-D", "warnings"],
    },
];

/// The words one pass hands cargo, for one package, at one profile.
///
/// The profile travels with the SUBCOMMAND and never in [`Pass::tail`]: clippy's tail opens `--`,
/// and everything past that separator belongs to the lint driver rather than to cargo - so a
/// `--profile` appended there would select no profile, warm nothing, and not fail either. That is
/// the same confusion `check-warm-start` reads out of the flake app's own line, one level up, and
/// the reason this gate takes no flag at all.
fn invocation<'a>(pass: &Pass, package: &'a str, profile: Option<&'a str>) -> Vec<&'a str> {
    let mut words: Vec<&str> = pass.lead.to_vec();
    if let Some(name) = profile {
        words.extend(["--profile", name]);
    }
    words.extend(["--package", package]);
    words.extend(pass.tail.iter().copied());
    words
}

/// What a gate needs before it can compile anything the declaration names.
///
/// A struct rather than a tuple, and clippy asked for it: the two fields are a path and a list of
/// strings, which is exactly the pair a positional return gets wrong silently.
pub(crate) struct Shipped {
    /// The repo root, so a `Command` can set the working directory cargo resolves paths against.
    pub(crate) root: std::path::PathBuf,
    /// The packages `nix/shipped.nix` publishes, in declaration order.
    pub(crate) packages: Vec<String>,
}

/// The shipped package list, or the verdict to return instead.
///
/// **One owner for the fail-closed policy**, and that is the whole reason this is a function. The
/// rule - *a list this gate reads as empty checks nothing and passes, which is the one failure it
/// must not have* - was stated twice in two paraphrases once a second gate read the same declaration,
/// and a policy stated twice is a policy that drifts. The per-gate prologue is a house pattern here
/// (`shipped.rs` has a third instance against the same file), so what is shared is the part with no
/// precedent for duplication: this one, whose two readers also share the PARSER.
///
/// `gate` names the caller in every message, because a reader of a failure needs to know which gate
/// could not read the declaration.
pub(crate) fn shipped_or_fail(gate: &str) -> Result<Shipped, Verdict> {
    let Some(root) = repo::root() else {
        eprintln!("xtask {gate}: could not determine the repo root");
        return Err(Verdict::Fail);
    };
    let path = root.join(SOURCE);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("xtask {gate}: could not read {}: {error}", path.display());
            return Err(Verdict::Fail);
        }
    };
    let packages = shipped_packages(&text);
    if packages.is_empty() {
        eprintln!("xtask {gate}: FAILED - parsed no package out of {SOURCE}");
        eprintln!("  A list this gate reads as empty checks nothing and passes, which is the one");
        eprintln!("  failure it must not have. `binaries = [` and `package = \"...\";` are the two");
        eprintln!("  shapes it looks for.");
        return Err(Verdict::Fail);
    }
    Ok(Shipped { root, packages })
}

/// `cargo xtask check-default-features` - the shipped feature set compiles and lints.
pub(crate) fn run(args: &[String]) -> Verdict {
    if !args.is_empty() {
        eprintln!("usage: check-default-features - it takes no arguments");
        eprintln!("  The cargo profile is derived from the target directory rather than passed in.");
        return Verdict::Usage;
    }
    let Shipped { root, packages } = match shipped_or_fail("check-default-features") {
        Ok(read) => read,
        Err(verdict) => return verdict,
    };
    let target_dir = std::env::var_os("CARGO_TARGET_DIR");
    let profile = profile_for(target_dir.as_deref().map(Path::new));
    println!(
        "xtask check-default-features: {} shipped package(s) from {SOURCE}: {}",
        packages.len(),
        packages.join(", ")
    );
    println!("  cargo's DEFAULT feature set - the one `nix/shipped.nix` publishes and no other gate compiles.");
    if let Some(name) = profile {
        println!(
            "  profile `{name}` - the warmed artifacts' own, so the units that match this narrow feature set are reused. Not most of them: this module's header has the measurement."
        );
    }
    let mut failed: Vec<String> = Vec::new();
    for package in &packages {
        for pass in PASSES {
            println!("\n=== {} {package} ===", pass.what);
            let mut command = std::process::Command::new("cargo");
            command.current_dir(&root).args(invocation(pass, package, profile));
            match command.status() {
                Ok(status) if status.success() => {}
                Ok(_) => failed.push(format!("{} {package}", pass.what)),
                Err(error) => {
                    eprintln!("xtask check-default-features: could not run cargo: {error}");
                    return Verdict::Fail;
                }
            }
        }
    }
    if failed.is_empty() {
        println!(
            "\nxtask check-default-features: ok - {} package(s) compile and lint at their default features",
            packages.len()
        );
        return Verdict::Pass;
    }
    eprintln!(
        "\nxtask check-default-features: FAILED - {}: {}",
        failed.len(),
        failed.join(", ")
    );
    eprintln!("  `just lint` and `just test` pass --all-features and cannot see this, and neither can");
    eprintln!("  `just check-changed`. What ships is what this compiled.");
    Verdict::Fail
}

#[cfg(test)]
mod tests {
    use super::{PASSES, invocation, shipped_packages};

    /// The flake output CI reaches this gate through.
    const APP: &str = "default-features";

    /// The name `main.rs` registers this gate under.
    const TASK: &str = "check-default-features";

    /// The recipe that is the developer's lane.
    const RECIPE: &str = "gates";

    /// The workflow the CI lane lives in.
    const WORKFLOW: &str = ".github/workflows/ci.yml";

    /// The job it has to be in, which is the one required context.
    const JOB: &str = "ci";

    /// The condition every rust step in that job is gated on.
    const CLASSIFIED: &str = "steps.classify.outputs.rust == 'true'";

    #[test]
    fn every_package_in_the_binaries_list_is_read_in_declaration_order() {
        let nix = concat!(
            "  binaries = [\n",
            "    {\n",
            "      bin = \"sutura\";\n",
            "      package = \"sutura-cli\";\n",
            "    }\n",
            "    {\n",
            "      bin = \"sutura-serve\";\n",
            "      package = \"sutura-serve\";\n",
            "    }\n",
            "  ];\n",
        );
        assert_eq!(shipped_packages(nix), vec!["sutura-cli", "sutura-serve"]);
    }

    #[test]
    fn a_record_written_on_one_line_is_still_a_record() {
        // The failure `shipped::declared`'s own tests forced: `nixpkgs-fmt`'s shape is not the only
        // legal one, and a parser that reads fewer packages than are declared passes by checking
        // less - which is the one failure mode this gate may not have.
        let nix = "  binaries = [\n    { bin = \"sutura\"; package = \"sutura-cli\"; }\n  ];\n";
        assert_eq!(shipped_packages(nix), vec!["sutura-cli"]);
    }

    #[test]
    fn a_longer_key_ending_in_package_is_not_a_package() {
        let nix = "  binaries = [\n    { subPackage = \"decoy\"; package = \"sutura-cli\"; }\n  ];\n";
        assert_eq!(shipped_packages(nix), vec!["sutura-cli"]);
    }

    #[test]
    fn a_package_outside_the_list_is_not_a_declaration() {
        // The scoping reason: this key appears elsewhere in that file, and a whole-file grep would
        // compile packages nothing publishes.
        let nix = concat!(
            "  someOther = { package = \"not-shipped\"; };\n",
            "  binaries = [\n",
            "    { package = \"sutura-cli\"; }\n",
            "  ];\n",
            "  after = { package = \"also-not-shipped\"; };\n",
        );
        assert_eq!(shipped_packages(nix), vec!["sutura-cli"]);
    }

    #[test]
    fn two_binaries_out_of_one_package_are_one_compile() {
        let nix = concat!(
            "  binaries = [\n",
            "    { bin = \"one\"; package = \"sutura-cli\"; }\n",
            "    { bin = \"two\"; package = \"sutura-cli\"; }\n",
            "  ];\n",
        );
        assert_eq!(shipped_packages(nix), vec!["sutura-cli"]);
    }

    #[test]
    fn every_pass_puts_the_profile_where_cargo_reads_it_and_not_past_the_lint_separator() {
        // The failure this exists for: clippy's tail opens `--`, so a profile appended to the end
        // of the line goes to the lint driver instead of to cargo. Nothing errors - the run just
        // compiles at the default profile, reuses none of the warmed artifacts and passes, so the
        // only symptom is a CI step quietly paying for a whole dependency build.
        for pass in PASSES {
            let words = invocation(pass, "sutura-serve", Some("ci"));
            let at = words
                .windows(2)
                .position(|pair| pair == ["--profile", "ci"])
                .unwrap_or_else(|| panic!("{} must pass `--profile ci`, got {words:?}", pass.what));
            assert!(at > 0, "{} must keep the subcommand first, got {words:?}", pass.what);
            if let Some(separator) = words.iter().position(|word| *word == "--") {
                assert!(
                    at < separator,
                    "{} puts the profile past `--`, where cargo never sees it: {words:?}",
                    pass.what
                );
            }
        }
    }

    #[test]
    fn the_lint_level_is_still_the_last_thing_clippy_is_told() {
        // The other half of that seam: threading a profile in must not reorder `-D warnings` out of
        // the driver's arguments, which would turn the lint half into a warning-only run.
        let clippy = PASSES
            .iter()
            .find(|pass| pass.what == "clippy")
            .expect("a clippy pass is declared");
        let words = invocation(clippy, "sutura-cli", Some("ci"));
        assert_eq!(
            words.get(words.len().saturating_sub(3)..),
            Some(&["--", "-D", "warnings"][..])
        );
    }

    #[test]
    fn no_profile_argument_leaves_cargo_on_the_developers_default() {
        // `just gates` passes nothing and must not be pushed into a second profile: locally that is
        // a second dependency build in the same target directory for an identical verdict.
        for pass in PASSES {
            let words = invocation(pass, "sutura-cli", None);
            assert!(!words.contains(&"--profile"), "{} named a profile: {words:?}", pass.what);
            assert!(words.windows(2).any(|pair| pair == ["--package", "sutura-cli"]));
        }
    }

    #[test]
    fn both_lanes_still_invoke_this_gate() {
        // A gate reachable from neither lane is a module, and this lane's whole history is a check
        // that existed while nothing ran it. Two readers, therefore: the developer's `just gates`
        // and the required CI job. `check-workflows` holds the other direction, that the app this
        // names is declared in flake.nix.
        //
        // NEITHER READER IS `contains` OVER RAW TEXT, which is the point of this test rather than
        // a detail of it. `#     cargo run -q -p xtask -- check-default-features` in the recipe and
        // `# run: nix run .#default-features` in the workflow each satisfy a substring while no
        // lane invokes anything - the dead-check shape, in the test that says the check is wired.
        // So a comment line of the recipe body is dropped, and the workflow is read through
        // `workflows::step`, whose reader skips a `#` line.
        let root = crate::repo::root().expect("the repo root");
        assert!(
            crate::TASKS.iter().any(|task| task.name == TASK),
            "{TASK} is not a registered task"
        );
        let body = crate::tasks::recipe_body(&root, RECIPE).expect("a `gates` recipe in the justfile");
        assert!(
            body.iter()
                .any(|line| !line.trim_start().starts_with('#') && line.contains(TASK)),
            "`just {RECIPE}` no longer runs {TASK} on a line that is not a comment"
        );
        let workflow = std::fs::read_to_string(root.join(WORKFLOW)).expect("ci.yml");
        let step = crate::workflows::step::app_step(&workflow, JOB, APP)
            .unwrap_or_else(|| panic!("a live `nix run .#{APP}` step inside {WORKFLOW}'s `{JOB}` job"));
        let declared = step.join("\n");
        // The two ways the step stays in the file and stops being a gate. A job under a NEW name
        // would be the third and is not reachable from here: which contexts are required is a
        // branch-ruleset setting no file in this tree states, which is why the step is in `{JOB}`.
        assert!(
            !declared.contains("continue-on-error"),
            "the {APP} step tolerates its own failure, so the CI half reports rather than gates:\n{declared}"
        );
        assert!(
            declared.contains(CLASSIFIED),
            "the {APP} step is not gated on `{CLASSIFIED}`, which is the condition the rust steps around it use:\n{declared}"
        );
    }

    #[test]
    fn a_list_this_parser_cannot_find_reads_as_empty_so_the_gate_can_fail_closed() {
        // `run` turns this into a FAILURE rather than a pass, which is the whole of why the parser
        // is allowed to answer nothing.
        assert!(shipped_packages("nothing that looks like a binaries list").is_empty());
    }
}
