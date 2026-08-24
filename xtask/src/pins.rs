//! The double-pin gate.
//!
//! nix pins every tool this repo runs. pixi pins a few as well, because nix does not run on
//! every host we develop on and because a git hook runs outside the dev shell, where a
//! nix-provided binary is not on PATH.
//!
//! That overlap is where a second source of truth comes from. The first attempt at fixing it
//! GENERATED pixi's dependency table out of `flake.nix` and failed a check when the committed
//! file drifted. It worked, and it was still a synchroniser standing between two pins - one
//! more thing to keep correct, and it broke twice while being written (`==3.13.*` is not a
//! valid conda specifier; a package directory is not an executable).
//!
//! So the rule is a SPLIT rather than a sync: a tool whose version changes what a gate
//! reports is pinned by nix and by nothing else. A tool that merely runs or interprets other
//! things - the hook runner, the interpreter for a maintenance script - is pixi's alone,
//! where a range is honest because no verdict depends on it.
//!
//! This gate is what makes that a rule instead of a comment: it fails if any tool is named
//! both as a flake app and as a pixi dependency. Text scanning rather than evaluation,
//! because it has to run on the hosts that have no nix.

use crate::Verdict;
use crate::repo;
use std::collections::BTreeSet;

/// Names that legitimately appear in both and are not tools whose version decides anything.
///
/// `pixi` itself is the obvious case: the flake exposes it so CI can run it at all, and
/// pixi cannot bootstrap its own interpreter. Kept as an explicit list so an addition is a
/// visible decision rather than a silent hole.
const EXEMPT: &[&str] = &["pixi"];

pub(crate) fn run(_args: &[String]) -> Verdict {
    let Some(root) = repo::root() else {
        eprintln!("xtask check-pins: could not locate the repo root");
        return Verdict::Fail;
    };

    let flake = match std::fs::read_to_string(root.join("flake.nix")) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("xtask check-pins: could not read flake.nix: {error}");
            return Verdict::Fail;
        }
    };
    let pixi = match std::fs::read_to_string(root.join("pixi.toml")) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("xtask check-pins: could not read pixi.toml: {error}");
            return Verdict::Fail;
        }
    };

    let apps = flake_apps(&flake);
    let deps = pixi_dependencies(&pixi);

    // An empty side would make this gate pass by finding nothing, which is the failure mode a
    // text-scanning check is most prone to. Both sides are non-empty in any real checkout.
    if apps.is_empty() {
        eprintln!("xtask check-pins: found no `apps.<name>` in flake.nix - the scan is broken");
        return Verdict::Fail;
    }
    if deps.is_empty() {
        eprintln!("xtask check-pins: found no [dependencies] in pixi.toml - the scan is broken");
        return Verdict::Fail;
    }

    let overlap: Vec<&String> = deps
        .iter()
        .filter(|name| apps.contains(*name) && !EXEMPT.contains(&name.as_str()))
        .collect();

    if overlap.is_empty() {
        println!(
            "xtask check-pins: ok - {} flake app(s), {} pixi dependency(ies), no overlap",
            apps.len(),
            deps.len()
        );
        return Verdict::Pass;
    }

    eprintln!("xtask check-pins: these tools are pinned TWICE\n");
    for name in &overlap {
        eprintln!("  {name}  - `apps.{name}` in flake.nix and [dependencies] in pixi.toml");
    }
    eprintln!();
    eprintln!("nix is the only pin for anything whose version changes what it reports.");
    eprintln!("Either drop it from pixi.toml, or - if its version genuinely cannot change a");
    eprintln!("gate's verdict - add it to EXEMPT in xtask/src/pins.rs with the reason.");
    Verdict::Fail
}

/// Every `apps.<name>` defined in flake.nix.
fn flake_apps(text: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for line in text.lines() {
        let trimmed = line.trim_start();
        // Comments mention app names in prose; only a definition counts.
        if trimmed.starts_with('#') {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("apps.")
            && let Some(name) = rest.split([' ', '=', '.']).next()
            && !name.is_empty()
        {
            names.insert(String::from(name));
        }
    }
    names
}

/// Every key in pixi.toml's `[dependencies]` table.
fn pixi_dependencies(text: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    let mut inside = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            inside = trimmed == "[dependencies]";
            continue;
        }
        if !inside || trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some((key, _)) = trimmed.split_once('=') {
            let key = key.trim();
            if !key.is_empty() {
                names.insert(String::from(key));
            }
        }
    }
    names
}

#[cfg(test)]
mod tests {
    #[test]
    fn apps_are_read_from_definitions_not_prose() {
        let flake = concat!(
            "        # CI reaches these through apps.zizmor, which is prose.\n",
            "        apps.zizmor = {\n",
            "          type = \"app\";\n",
            "        };\n",
            "        apps.cargo = {\n",
        );
        let apps = super::flake_apps(flake);
        assert!(apps.contains("zizmor"));
        assert!(apps.contains("cargo"));
        assert_eq!(apps.len(), 2, "a comment must not contribute a name");
    }

    #[test]
    fn dependencies_stop_at_the_next_table() {
        let pixi = concat!(
            "[workspace]\n",
            "name = \"x\"\n",
            "[dependencies]\n",
            "# a comment\n",
            "prek = \">=0.4\"\n",
            "python = \"3.13.*\"\n",
            "\n",
            "[tasks]\n",
            "zizmor = \"zizmor .github/workflows\"\n",
        );
        let deps = super::pixi_dependencies(pixi);
        // `name` is in [workspace] and `zizmor` is a TASK, not a dependency. Reading either as
        // a pin would make this gate fire on something that is not a pin at all.
        assert_eq!(deps, ["prek", "python"].iter().map(|s| String::from(*s)).collect());
    }
}
