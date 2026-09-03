//! The venue map keeps its own rule: an identity acceptance task is named by the page that maps it.
//!
//! `docs/where-identity-is-proven.md` closes with a rule about itself:
//!
//! > A new venue arrives as a row in the table above **with its exclusions written**, in the same change.
//!
//! Nothing read that. `AGENTS.md` calls a rule with no mechanism a wish, and this is the shape of
//! wish that fails quietly: a venue reached by its own task, cited in a review as though the page had
//! always mapped it, and the page silent about what the venue cannot answer. The cost is not a red
//! run - it is a green one read as proving more than it did, which is the failure the page opens by
//! naming.
//!
//! HOW IT READS THEM. Text, not evaluation, for the reason `pins.rs` and `warm_start.rs` give: this
//! runs on a host with no nix and no `just`. The task names come from the justfile, because a task is
//! how a venue is reached by a person and by CI alike, and the page is matched on the backticked
//! `` `just <name>` `` form its own *Reached by* column uses.
//!
//! WHAT IT DOES NOT REACH, because an overstated gate spends trust a reviewer needed elsewhere:
//!
//! * **Only a venue whose door is an `*-acceptance` recipe.** Three of the venues on that page are
//!   reached by `just test` instead - the fake at the port, the mock issuer, and the real provider in
//!   the sandbox - and this gate cannot see any of them. It would not have caught the provider tier,
//!   which arrived with its row because its author wrote one.
//! * **It checks the page NAMES the task, not that a well-formed row states its exclusions.** The
//!   match is `contains`, so a task named in a sentence satisfies it. Reading a table cell would be a
//!   parser over prose, and judging whether an exclusion is honest is a review, not a gate. What is
//!   held is the half a scan can hold: the page cannot go silent about a venue that exists.
//!
//! FAIL CLOSED, like its neighbours. An unreadable file is a failure, and so is finding **no**
//! acceptance recipe at all: a name-reading gate whose reader has stopped matching reports every
//! venue registered and checks nothing, which is worse than no gate.

use crate::Verdict;
use crate::repo;

/// Where the task names come from - the spelling git tracks, which is lower-case.
const JUSTFILE: &str = "justfile";

/// The page that maps a claim to the venue that can honestly answer it.
const VENUE_MAP: &str = "docs/where-identity-is-proven.md";

/// The one `-acceptance` task that is **not** an identity venue, and so has no row on that page.
///
/// It reaches a metadata-catalog platform; the page maps identity claims. Named as a constant rather
/// than inferred, because a gate reading "every `-acceptance` task" would force a non-identity venue
/// into an identity map, and this is where a reader finds out that the exception was a decision.
const NON_IDENTITY_ACCEPTANCE: &[&str] = &["datahub-acceptance"];

pub(crate) fn run(_args: &[String]) -> Verdict {
    let Some(root) = repo::root() else {
        eprintln!("xtask check-venues: could not locate the repo root");
        return Verdict::Fail;
    };
    let justfile = match std::fs::read_to_string(root.join(JUSTFILE)) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("xtask check-venues: could not read {JUSTFILE}: {error}");
            return Verdict::Fail;
        }
    };
    let map = match std::fs::read_to_string(root.join(VENUE_MAP)) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("xtask check-venues: could not read {VENUE_MAP}: {error}");
            return Verdict::Fail;
        }
    };
    decide(&justfile, &map)
}

/// The verdict over the two files' text.
///
/// Separated from [`run`] so both answers can be tested without a tree to read - the half of a gate
/// that is otherwise only ever exercised green.
fn decide(justfile: &str, map: &str) -> Verdict {
    let split = match split(justfile, map) {
        Ok(split) => split,
        Err(why) => {
            eprintln!("xtask check-venues: {why}");
            eprintln!();
            eprintln!("This gate reads task names out of {JUSTFILE} and found none, so it has checked");
            eprintln!("NOTHING. That is a failure rather than a pass on purpose: a reader that has");
            eprintln!("stopped matching reports every venue registered.");
            return Verdict::Fail;
        }
    };

    if !split.missing.is_empty() {
        eprintln!(
            "xtask check-venues: identity acceptance task(s) with no mention in {VENUE_MAP}: {}",
            split.missing.join(", ")
        );
        eprintln!();
        eprintln!("A venue arrives as a row in that page's table WITH ITS EXCLUSIONS WRITTEN, naming");
        eprintln!("`just <task>` in its `Reached by` column - see the rule under \"Keeping this page");
        eprintln!("honest\". A venue nobody can look up is one a green run gets cited for.");
        eprintln!();
        eprintln!("If the task reaches no identity claim at all, add it to NON_IDENTITY_ACCEPTANCE in");
        eprintln!("xtask/src/venues.rs with the reason, rather than writing it an identity row.");
        return Verdict::Fail;
    }

    println!(
        "xtask check-venues: ok - {} identity venue(s) named by {VENUE_MAP}: {}",
        split.registered.len(),
        split.registered.join(", ")
    );
    Verdict::Pass
}

/// The acceptance tasks, split by whether [`VENUE_MAP`] names them.
#[derive(Debug)]
struct Split<'a> {
    registered: Vec<&'a str>,
    missing: Vec<&'a str>,
}

fn split<'a>(justfile: &'a str, map: &str) -> Result<Split<'a>, String> {
    let tasks = acceptance_tasks(justfile);
    if tasks.is_empty() {
        return Err(format!("{JUSTFILE} declares no `*-acceptance` recipe this gate can read"));
    }
    let mut split = Split {
        registered: Vec::new(),
        missing: Vec::new(),
    };
    for task in tasks {
        if NON_IDENTITY_ACCEPTANCE.contains(&task) {
            continue;
        }
        if map.contains(&format!("`just {task}`")) {
            split.registered.push(task);
        } else {
            split.missing.push(task);
        }
    }
    Ok(split)
}

/// Every `*-acceptance` recipe the justfile declares, sorted and deduplicated.
fn acceptance_tasks(justfile: &str) -> Vec<&str> {
    let mut tasks: Vec<&str> = justfile
        .lines()
        .filter_map(recipe_name)
        .filter(|name| name.ends_with("-acceptance"))
        .collect();
    tasks.sort_unstable();
    tasks.dedup();
    tasks
}

/// The recipe a justfile line declares, if it declares one.
///
/// **Not `strip_suffix(':')`**, which the first version of this gate used. That spelling misses a
/// recipe taking a parameter (`some-acceptance *args:`) and reads the name as `some-acceptance *args`
/// where it does not - either way the task is silently absent from a gate whose whole job is noticing
/// an absence. A recipe is unindented, so a body line and a shebang inside one are excluded by
/// column; the name is the first word before the `:`; and `x := y`, `set a := b` and an `alias` are
/// assignments rather than recipes, which is what the `:=` arm is for.
fn recipe_name(line: &str) -> Option<&str> {
    if line.is_empty() || line.starts_with([' ', '\t', '#']) {
        return None;
    }
    let (head, rest) = line.split_once(':')?;
    if rest.starts_with('=') {
        return None;
    }
    let name = head.split_whitespace().next()?;
    name.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        .then_some(name)
}

#[cfg(test)]
mod tests {
    use crate::Verdict;

    /// A justfile's shapes, including the ones the first version of this gate got wrong: a recipe
    /// with a parameter, an assignment, a dependency, and a body line quoting a task name.
    const JUSTFILE: &str = concat!(
        "set dotenv-load := true\n",
        "target := \"aarch64-apple-darwin\"\n",
        "\n",
        "# The comment above a recipe, mentioning keycloak-acceptance: in prose.\n",
        "bigquery-acceptance:\n",
        "    #!/usr/bin/env bash\n",
        "    echo \"bigquery-acceptance: not a gate\"\n",
        "    nix run .#bigquery-acceptance -- --no-capture\n",
        "\n",
        "datahub-acceptance: dev-up\n",
        "    cargo test -p sutura-catalog-datahub\n",
        "\n",
        "exchange-acceptance *args:\n",
        "    cargo test {{args}}\n",
    );

    /// The page, naming one of the three in the form its `Reached by` column uses.
    const MAP: &str = concat!(
        "| **A real dataset under a shared key** | on demand | a key | `just bigquery-acceptance` |\n",
        "| **A real token exchange** | nowhere yet | a pool | not built |\n",
    );

    #[test]
    fn a_recipe_is_read_by_its_name_and_a_body_line_is_not_a_recipe() {
        // The parse is the whole gate: a name it cannot read is a venue it cannot miss. Each arm
        // here is a line shape the real justfile has.
        assert_eq!(super::recipe_name("bigquery-acceptance:"), Some("bigquery-acceptance"));
        // A parameter, which `strip_suffix(':')` could not read at all.
        assert_eq!(super::recipe_name("clean-branches *args:"), Some("clean-branches"));
        // A dependency after the colon is not part of the name.
        assert_eq!(super::recipe_name("datahub-acceptance: dev-up"), Some("datahub-acceptance"));
        // A body line is indented, and a shebang inside a recipe is a body line.
        assert!(super::recipe_name("    nix run .#keycloak-acceptance -- --no-capture").is_none());
        assert!(super::recipe_name("    #!/usr/bin/env bash").is_none());
        // Neither an assignment nor a `set` nor a comment declares a recipe.
        assert!(super::recipe_name("target := \"aarch64-apple-darwin\"").is_none());
        assert!(super::recipe_name("set dotenv-load := true").is_none());
        assert!(super::recipe_name("# bigquery-acceptance: in prose").is_none());
    }

    #[test]
    fn the_tasks_are_the_recipes_and_not_every_line_naming_one() {
        // A comment and a recipe body both spell `keycloak-acceptance` in the fixture above; neither
        // declares it, so a gate that grepped for the suffix would demand a row for a venue that
        // does not exist.
        assert_eq!(
            super::acceptance_tasks(JUSTFILE),
            vec!["bigquery-acceptance", "datahub-acceptance", "exchange-acceptance"]
        );
    }

    #[test]
    fn a_venue_the_page_does_not_name_is_red_and_the_excepted_one_is_not_asked() {
        // `exchange-acceptance` is an identity venue the fixture's page never names: the case this
        // gate exists for. `datahub-acceptance` is the declared non-identity exception and must not
        // be reported, or the gate forces a metadata platform into an identity map.
        let split = super::split(JUSTFILE, MAP).expect("the fixture declares three recipes");
        assert_eq!(split.missing, vec!["exchange-acceptance"]);
        assert_eq!(split.registered, vec!["bigquery-acceptance"]);
        assert_eq!(super::decide(JUSTFILE, MAP), Verdict::Fail);

        // And with a row for it, the same tree passes - so the failure above is about the row and
        // not about the reader.
        let named = format!("{MAP}| **An exchange** | on demand | a pool | `just exchange-acceptance` |\n");
        assert_eq!(super::decide(JUSTFILE, &named), Verdict::Pass);
    }

    #[test]
    fn a_page_that_names_a_task_in_the_wrong_form_does_not_satisfy_the_gate() {
        // The `Reached by` column's own spelling is the contract, because that column is what a
        // reader follows. A bare mention is not it.
        let prose = MAP.replace("`just bigquery-acceptance`", "run bigquery-acceptance on demand");
        let split = super::split(JUSTFILE, &prose).expect("the fixture declares three recipes");
        assert!(split.missing.contains(&"bigquery-acceptance"), "{:?}", split.missing);
    }

    #[test]
    fn a_reader_that_matches_nothing_is_a_failure_rather_than_every_venue_registered() {
        // The fail-open shape this gate would otherwise have: no recipe read means an empty
        // `missing`, which is indistinguishable from a fully-registered page. `warm_start.rs` states
        // the same rule for the same reason - a path-reading gate that says `ok` having found no
        // path is the failure mode.
        let unreadable = "    bigquery-acceptance:\n    datahub-acceptance:\n";
        assert!(super::acceptance_tasks(unreadable).is_empty());
        let why = super::split(unreadable, MAP).expect_err("a gate that read no task checked nothing");
        assert!(why.contains("no `*-acceptance` recipe"), "{why}");
        assert_eq!(super::decide(unreadable, MAP), Verdict::Fail);
    }

    #[test]
    fn the_real_justfile_still_declares_tasks_this_gate_can_read() {
        // Caught here and not only on a branch: a reader that matches nothing makes its gate pass
        // vacuously, and the justfile's shape is not this gate's to control. Whether every venue is
        // REGISTERED is the gate's verdict; that the reader still works is this assertion.
        let root = crate::repo::root().expect("the repo root");
        let justfile = std::fs::read_to_string(root.join(super::JUSTFILE)).expect("the justfile is tracked");
        let tasks = super::acceptance_tasks(&justfile);
        assert!(
            tasks.len() >= 2,
            "this gate read {tasks:?} out of {}, and would pass anything",
            super::JUSTFILE
        );
        // The declared exception has to still BE one, or the constant is documenting a task that no
        // longer exists and the next non-identity venue inherits a silent pass.
        for excepted in super::NON_IDENTITY_ACCEPTANCE {
            assert!(
                tasks.contains(excepted),
                "`{excepted}` is excepted from the venue map and is not a task any more - drop it \
                 from NON_IDENTITY_ACCEPTANCE rather than leaving an exception nothing uses"
            );
        }
    }
}
