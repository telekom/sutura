//! Guidance rot: documentation and comments that describe a repo we no longer have.
//!
//! Every rule here exists because the mistake was already made in this repo. A doc claiming
//! CI enters the devenv shell, a comment citing a gate that was deleted, an ADR quoting a
//! compiler version two releases old - each read as current, and each cost someone the time
//! to find out otherwise.
//!
//! Scope: documentation and configuration (`.md`, `.nix`, `.yml`, `.yaml`, `.toml`, `.sh`).
//! Rust source is deliberately out of scope - see the filter in `run`.
//!
//! Three checks, one theme: a claim in prose is only as good as the thing that verifies it.
//!
//! * `stale` - a forbidden phrase, each with the replacement and the reason
//! * `versions` - a version written anywhere must match the pin it describes
//! * `references` - a gate, task or skill named in prose must exist

use std::collections::BTreeSet;
use std::path::Path;

use crate::Verdict;
use crate::repo;

/// A phrase that should not appear, and what to write instead.
struct Forbidden {
    /// Literal to look for. Substring match, case-sensitive.
    needle: &'static str,
    /// What to write instead.
    instead: &'static str,
    /// Why it is wrong. Printed, because a rule whose reason is unstated gets reverted.
    why: &'static str,
    /// Paths this applies to. Empty means everywhere.
    only: &'static [&'static str],
    /// Paths exempt from it - typically the file that documents the rule itself.
    except: &'static [&'static str],
}

const FORBIDDEN: &[Forbidden] = &[
    Forbidden {
        needle: "devenv shell",
        instead: "a flake output: nix build .#checks.<system>.<name>, or nix run .#<app>",
        why: "CI does not use devenv. It needs nix alone, and `devenv shell <x>` in a workflow \
              reports a devenv error instead of the gate's own output",
        only: &[".github/**"],
        except: &[],
    },
    Forbidden {
        needle: "--all-targets -- -D warnings",
        instead: "--all-targets --all-features -- -D warnings",
        why: "no crate here declares a feature today, so this is a no-op - and that is the \
              point: the flag is in every entry point already, so the day an adapter goes \
              behind one, coverage does not silently drop to nothing",
        only: &[],
        except: &[".agents/skills/engineering/rust/SKILL.md"],
    },
    Forbidden {
        needle: "cargo nextest run --workspace\"",
        instead: "cargo nextest run --workspace --all-features",
        why: "same reason as clippy: a no-op today, in place so it stays correct later",
        only: &[],
        except: &[],
    },
    Forbidden {
        needle: "pixi run zizmor",
        instead: "pixi run --frozen zizmor",
        why: "without --frozen, pixi may resolve and rewrite pixi.lock during a validation run, \
              so the check no longer describes the locked environment",
        only: &[".github/**", ".pre-commit-config.yaml"],
        except: &[],
    },
    Forbidden {
        // Assembled would be cleaner, but this needle is not a credential and the rule table
        // is not scanned (see the filter in `run`), so a literal is fine here.
        needle: "nix run nixpkgs#",
        instead: "a flake app: nix run .#<tool>, defined in flake.nix from the locked nixpkgs",
        why: "the registry form resolves to whatever nixpkgs-unstable points at when the job runs: an unreviewed mutable input, in a job holding a write token",
        only: &[".github/**"],
        except: &[],
    },
    Forbidden {
        needle: "accept-flake-config",
        instead: "nothing - put what CI needs in extra_nix_config, where the diff shows it",
        why: "it honours settings from flake.nix itself, so a pull request could add a substituter and a trusted key and have CI fetch attacker-built store paths whose signature verifies",
        only: &[".github/**"],
        except: &[],
    },
    Forbidden {
        needle: "no-cranelift",
        instead: "nothing - the gate was deleted; the rule holds by construction",
        why: "the detector matched its own source and needed a self-exclusion to work at all",
        only: &[],
        except: &[],
    },
];

/// A version that must agree wherever it is written.
struct Pin {
    /// Human name, for the message.
    name: &'static str,
    /// File holding the authoritative value.
    source: &'static str,
    /// Line prefix in that file; the value is the rest, unquoted.
    key: &'static str,
    /// Where the value may also appear, and must match if it does.
    mentioned_in: &'static [&'static str],
    /// Regex-free detector: a line containing this marker must contain the value.
    marker: &'static str,
}

const PINS: &[Pin] = &[Pin {
    name: "Rust toolchain",
    source: "rust-toolchain.toml",
    key: "channel = ",
    mentioned_in: &["docs/**", "AGENTS.md", ".agents/skills/**", "README.md"],
    // A line that names the pin file and a version is claiming what the pin is.
    marker: "rust-toolchain.toml",
}];

/// Case-insensitive extension test. A case-sensitive one is a bug on a case-insensitive
/// filesystem, which is where half of this repo is developed.
fn has_ext(path: &str, exts: &[&str]) -> bool {
    std::path::Path::new(path)
        .extension()
        .and_then(std::ffi::OsStr::to_str)
        .is_some_and(|e| exts.iter().any(|want| e.eq_ignore_ascii_case(want)))
}

fn matches_any(patterns: &[&str], path: &str) -> bool {
    patterns.iter().any(|p| repo::matches(p, path))
}

/// The value of `key` in `source`, with quotes and whitespace stripped.
fn pinned_value(root: &Path, pin: &Pin) -> Option<String> {
    let text = std::fs::read_to_string(root.join(pin.source)).ok()?;
    for line in text.lines() {
        if let Some(rest) = line.trim().strip_prefix(pin.key) {
            return Some(String::from(rest.trim().trim_matches('"')));
        }
    }
    None
}

/// Does this line look like it states a version, other than the pinned one?
///
/// Deliberately narrow: only lines that also name the pin file are judged, because those are
/// the ones asserting what the pin is. A line mentioning some other version is not this
/// check's business.
fn contradicts(line: &str, pin: &Pin, value: &str) -> bool {
    if !line.contains(pin.marker) {
        return false;
    }
    let digits: String = line
        .chars()
        .map(|c| if c.is_ascii_digit() || c == '.' { c } else { ' ' })
        .collect();
    digits
        .split_whitespace()
        // Trim the sentence's own punctuation first: "1.98.0." is the same version as
        // "1.98.0", and treating them as different is how a correct doc gets flagged.
        .map(|t| t.trim_matches('.'))
        .filter(|t| t.contains('.') && t.starts_with(|c: char| c.is_ascii_digit()))
        .any(|t| t != value)
}

/// Task names this binary actually dispatches, so prose cannot cite a deleted gate.
fn known_tasks() -> BTreeSet<&'static str> {
    crate::task_names().collect()
}

/// The task name at the start of `tail`, or `None` if there is not one there.
///
/// `None` for a flag: `cargo xtask --help` is documented and correct, and reading `--help` as
/// a deleted gate made this gate fail on the page that describes the gates. `None` also for a
/// placeholder like `<task>`, which is how the usage line is written.
fn task_name_at(tail: &str) -> Option<&str> {
    let end = tail
        .find(|c: char| !c.is_ascii_alphanumeric() && c != '-')
        .unwrap_or(tail.len());
    let name = tail.get(..end)?;
    if name.is_empty() || name.starts_with('-') {
        return None;
    }
    Some(name)
}

/// `cargo xtask <name>` mentioned anywhere must be a task that exists.
fn bad_task_references(root: &Path, files: &[String]) -> Vec<String> {
    let known = known_tasks();
    let mut problems = Vec::new();
    for rel in files {
        if !has_ext(rel, &["md", "nix", "yaml", "yml"]) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(root.join(rel)) else {
            continue;
        };
        for (i, line) in text.lines().enumerate() {
            for marker in ["cargo xtask ", "-p xtask -- "] {
                let mut rest = line;
                while let Some(at) = rest.find(marker) {
                    let tail = rest.get(at + marker.len()..).unwrap_or("");
                    if let Some(name) = task_name_at(tail)
                        && !known.contains(name)
                    {
                        problems.push(format!(
                            "{rel}:{}: `{name}` is not an xtask task - it was renamed or deleted",
                            i + 1
                        ));
                    }
                    rest = tail;
                }
            }
        }
    }
    problems
}

/// A path in backticks that looks like a repo path must exist.
///
/// Only `.agents/...` paths, because those are the ones a router or a doc sends an agent to,
/// and a dead route is the failure this catches. Broadening it to every backticked path would
/// flag illustrative examples.
fn dead_paths(root: &Path, files: &[String]) -> Vec<String> {
    let mut problems = Vec::new();
    for rel in files {
        if !has_ext(rel, &["md"]) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(root.join(rel)) else {
            continue;
        };
        for (i, line) in text.lines().enumerate() {
            for piece in line.split('`').skip(1).step_by(2) {
                let candidate = piece.trim();
                if !candidate.starts_with(".agents/") || candidate.contains(['*', ' ', '<']) {
                    continue;
                }
                if !root.join(candidate).exists() {
                    problems.push(format!("{rel}:{}: `{candidate}` does not exist", i + 1));
                }
            }
        }
    }
    problems
}

fn stale_phrases(root: &Path, files: &[String]) -> Vec<String> {
    let mut problems = Vec::new();
    for rel in files {
        let Ok(text) = std::fs::read_to_string(root.join(rel)) else {
            continue;
        };
        for rule in FORBIDDEN {
            if !rule.only.is_empty() && !matches_any(rule.only, rel) {
                continue;
            }
            if matches_any(rule.except, rel) {
                continue;
            }
            for (i, line) in text.lines().enumerate() {
                if line.contains(rule.needle) {
                    problems.push(format!(
                        "{rel}:{}: `{}`\n      write instead: {}\n      why: {}",
                        i + 1,
                        rule.needle,
                        rule.instead,
                        rule.why
                    ));
                }
            }
        }
    }
    problems
}

fn version_mismatches(root: &Path, files: &[String]) -> Vec<String> {
    let mut problems = Vec::new();
    for pin in PINS {
        let Some(value) = pinned_value(root, pin) else {
            problems.push(format!(
                "could not read the {} pin from {} (key `{}`)",
                pin.name, pin.source, pin.key
            ));
            continue;
        };
        for rel in files {
            if !matches_any(pin.mentioned_in, rel) {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(root.join(rel)) else {
                continue;
            };
            for (i, line) in text.lines().enumerate() {
                if contradicts(line, pin, &value) {
                    problems.push(format!(
                        "{rel}:{}: names {} but the pin in {} is {value}\n      {}",
                        i + 1,
                        pin.name,
                        pin.source,
                        line.trim()
                    ));
                }
            }
        }
    }
    problems
}

pub(crate) fn run(_args: &[String]) -> Verdict {
    let Some(repo::RepoFiles { root, files }) = repo::all_files() else {
        eprintln!("xtask check-guidance: could not determine the repo root");
        return Verdict::Fail;
    };

    // Only text we might make a claim in.
    let text_files: Vec<String> = files
        .into_iter()
        // Documentation and configuration, NOT Rust source. Two reasons, and the second is
        // the one that matters: a rule table written in Rust contains the very phrases it
        // forbids, so scanning `.rs` makes this gate report itself - and the only fix would
        // be an exclusion list, which is a hole anything can be added to. The narrower scope
        // is honest instead. A stale comment beside Rust code is reviewed with that code.
        .filter(|f| has_ext(f, &["md", "nix", "yml", "yaml", "toml", "sh"]))
        // Mirrored upstream material makes claims about ITS repo, not ours.
        .filter(|f| !f.starts_with(".agents/skill-library/"))
        .filter(|f| !f.starts_with(".agents/skills/engineering/ms-rust/0"))
        .filter(|f| !f.starts_with(".agents/skills/engineering/ms-rust/1"))
        .collect();

    let mut problems = stale_phrases(&root, &text_files);
    problems.extend(version_mismatches(&root, &text_files));
    problems.extend(bad_task_references(&root, &text_files));
    problems.extend(dead_paths(&root, &text_files));

    if problems.is_empty() {
        println!(
            "xtask check-guidance: ok - {} file(s), {} phrase rule(s), {} pin(s)",
            text_files.len(),
            FORBIDDEN.len(),
            PINS.len()
        );
        return Verdict::Pass;
    }

    eprintln!("xtask check-guidance: FAILED");
    for p in &problems {
        eprintln!("  {p}");
    }
    eprintln!();
    eprintln!("Guidance that no longer matches the repo is read as current. Fix the text, or");
    eprintln!("if the rule itself is wrong, change it in xtask/src/guidance.rs with a reason.");
    Verdict::Fail
}

#[cfg(test)]
mod tests {
    use super::{Pin, contradicts, known_tasks};

    const PIN: Pin = Pin {
        name: "Rust toolchain",
        source: "rust-toolchain.toml",
        key: "channel = ",
        mentioned_in: &[],
        marker: "rust-toolchain.toml",
    };

    #[test]
    fn a_line_naming_the_pin_file_and_a_stale_version_contradicts() {
        assert!(contradicts(
            "The compiler pin is rust-toolchain.toml, currently 1.93.0.",
            &PIN,
            "1.98.0"
        ));
        assert!(!contradicts(
            "The compiler pin is rust-toolchain.toml, currently 1.98.0.",
            &PIN,
            "1.98.0"
        ));
    }

    #[test]
    fn a_line_not_naming_the_pin_file_is_none_of_its_business() {
        // Some other version in prose is not a claim about our pin.
        assert!(!contradicts("DataFusion 53 is the upstream version.", &PIN, "1.98.0"));
    }

    #[test]
    fn trailing_punctuation_is_not_a_different_version() {
        // The sentence's full stop is not part of the version.
        assert!(!contradicts("Pinned in rust-toolchain.toml at 1.98.0.", &PIN, "1.98.0"));
        assert!(contradicts("Pinned in rust-toolchain.toml at 1.93.0.", &PIN, "1.98.0"));
    }

    #[test]
    fn a_line_naming_the_pin_file_with_no_version_is_fine() {
        assert!(!contradicts(
            "The compiler pin lives in rust-toolchain.toml and nowhere else.",
            &PIN,
            "1.98.0"
        ));
    }

    #[test]
    fn a_flag_or_placeholder_is_not_a_task_name() {
        use super::task_name_at;
        // The real case: the page documenting the gates cites `cargo xtask --help`.
        assert_eq!(task_name_at("--help` prints the list"), None);
        assert_eq!(task_name_at("<task>"), None);
        assert_eq!(task_name_at(""), None);
        // And a real task name still parses, stopping at the backtick or space.
        assert_eq!(task_name_at("check-guidance` runs"), Some("check-guidance"));
        assert_eq!(task_name_at("max-lines --json"), Some("max-lines"));
    }

    #[test]
    fn the_task_list_is_the_dispatch_table() {
        let known = known_tasks();
        // If this fails, the table and the dispatcher have diverged, which is the whole
        // reason prose is checked against the table rather than against a second list.
        assert!(known.contains("max-lines"));
        assert!(known.contains("check-guidance"));
        assert!(!known.contains("no-cranelift"), "deleted gate must not be dispatchable");
    }
}
