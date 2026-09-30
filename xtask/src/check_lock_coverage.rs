//! Every `Cargo.lock` the repo walk finds sits in a directory the cargo entry of
//! `.github/dependabot.yml` lists, so no lock is left for a bump to miss.
//!
//! The subjects come from [`repo::all_files`]: `git ls-files` (tracked plus untracked-not-ignored)
//! in a checkout, the tree walk in the Nix sandbox, with `repo`'s own skip list either way - so a
//! nested agent worktree's locks are not judged as this repo's. The config is read line by line, not
//! as YAML: only a `directories: [..]` or `directory:` line inside the cargo entry counts, and a
//! shape it cannot read covers nothing, so it fails closed.

use std::collections::BTreeSet;

use crate::Verdict;
use crate::repo;

const CONFIG: &str = ".github/dependabot.yml";
const LOCK: &str = "Cargo.lock";

/// A [`repo::Scope`]: every lock, and the config that must cover them.
fn lock_or_config(rel: &str) -> bool {
    rel == CONFIG || rel == LOCK || rel.ends_with("/Cargo.lock")
}

pub(crate) fn run(_args: &[String]) -> Verdict {
    let census = match repo::all_files() {
        Ok(census) => census,
        Err(why) => {
            eprintln!("check-lock-coverage: FAILED - {}", why.describe());
            return Verdict::Fail;
        }
    };
    let mut locks = BTreeSet::new();
    let mut config = String::new();
    let inspected = census.inspect(&[LOCK, CONFIG], lock_or_config, |rel, bytes| {
        if rel == CONFIG {
            config = String::from_utf8_lossy(bytes).into_owned();
        } else {
            locks.insert(lock_dir(rel));
        }
    });
    if let Err(why) = inspected {
        eprintln!("check-lock-coverage: FAILED - {}", why.describe());
        return Verdict::Fail;
    }
    let covered = cargo_directories(&config);
    let uncovered: Vec<&String> = locks.iter().filter(|dir| !covered.contains(*dir)).collect();
    if uncovered.is_empty() {
        println!("check-lock-coverage: ok - {} lock(s)", locks.len());
        return Verdict::Pass;
    }
    eprintln!("check-lock-coverage: {} uncovered by {CONFIG}:", uncovered.len());
    for dir in uncovered {
        eprintln!("  {dir}");
    }
    Verdict::Fail
}

/// `fuzz/Cargo.lock` -> `/fuzz`, `Cargo.lock` -> `/`: the spelling Dependabot's `directories` uses.
fn lock_dir(rel: &str) -> String {
    let dir = rel.strip_suffix(LOCK).unwrap_or(rel).trim_end_matches('/');
    format!("/{dir}")
}

/// The directories the cargo entry lists, from `directories: ["/", ..]` or `directory: "/"`.
fn cargo_directories(yaml: &str) -> BTreeSet<String> {
    let unquote = |item: &str| item.trim().trim_matches(['"', '\'']).to_owned();
    let mut dirs = BTreeSet::new();
    let mut in_cargo = false;
    for line in yaml.lines().map(str::trim_start) {
        if let Some(ecosystem) = line.strip_prefix("- package-ecosystem:") {
            in_cargo = ecosystem.trim() == "cargo";
            continue;
        }
        if !in_cargo {
            continue;
        }
        if let Some(list) = line.strip_prefix("directories:") {
            let inner = list.trim().strip_prefix('[').and_then(|rest| rest.strip_suffix(']'));
            dirs.extend(inner.into_iter().flat_map(|inner| inner.split(',')).map(unquote));
        } else if let Some(one) = line.strip_prefix("directory:") {
            dirs.insert(unquote(one));
        }
    }
    dirs
}
