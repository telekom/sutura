//! Guidance rot: documentation and comments that describe a repo we no longer have.
//!
//! Every rule here exists because the mistake was already made in this repo. A doc claiming
//! CI enters the devenv shell, a comment citing a gate that was deleted, an ADR quoting a
//! compiler version two releases old - each read as current, and each cost someone the time
//! to find out otherwise.
//!
//! Scope: documentation and configuration (`.md`, `.nix`, `.yml`, `.yaml`, `.toml`, `.sh`) for
//! everything that judges a SENTENCE - Rust source is deliberately out of that, see the filter in
//! `run` - plus Rust source for the two checks that judge a CITATION, which is resolvable rather
//! than read.
//!
//! Ten checks, one theme: a claim in prose is only as good as the thing that verifies it.
//!
//! * `stale` - a forbidden phrase, each with the replacement and the reason
//! * `versions` - a version written anywhere must match the pin it describes
//! * `claims` - a statement about what this repo has, checked against what it has
//! * `counts` - a number in prose that counts something, checked against the count
//! * `references` - a gate, task or skill named in prose must exist
//! * `remedies` - the correction a failure prints, held to the standard of the prose it corrects
//! * `advice` - a task a failure prints must exist, over every `.rs` file this repository publishes
//! * `constants` - a doc comment naming a variant of a constant it links must name the one it holds
//! * `hosts` - the file a sentence names as holding a mechanism must be the file that holds it
//! * `pages` - a page's amendment ordinals and its table headers, held against the page's own shape
//!
//! `remedies`, `advice` and `constants` are the three that read Rust rather than prose;
//! `hosts` reads prose and derives its answer from anywhere. `remedies` is here because of
//! `github.com/telekom/sutura#241`: a remedy in `claims` said a transport surface was absent for as
//! long as it took a person to read it, because the prose scope below never reached this binary's
//! own source. `advice` is here because of `github.com/telekom/sutura#243`, which is the same hole
//! one layer out - it closed for the remedy table and stayed open for every other string a program
//! prints, so `AGENTS.md`'s "cite a `just` task, never a raw command line" held where a reader is
//! documented to and nowhere a reader is told. Its scope, and what it declines to catch, are in
//! [`advice`]'s own header; the short version is that it resolves a citation and does not read a
//! sentence, which is why widening to `.rs` costs it no false positives.
//!
//! `claims` and `counts` exist because of one review, and because of one CAUSE rather than
//! nineteen mistakes: nineteen false sentences across `README.md`, `docs/` and `AGENTS.md`, four of
//! them saying there is no HTTP surface while two crates and a published page ship one, and in
//! almost every case the corrected sentence already existed in a sibling document. The correction
//! had landed in one file and had not been carried to the others. So what is checked here is the
//! CLAIM rather than the file: one `Contradicted` entry carries every wording of one claim, which
//! is what makes a sibling that was missed a failure rather than a survivor.
//!
//! **What these two cannot do, stated before a reader trusts them.** They match a literal, so a
//! paraphrase escapes - the same limit `AGENTS.md` records for the leak guard. They are a ratchet
//! on a sentence somebody has already written once, not a reader.

use std::collections::BTreeSet;
use std::path::Path;

use crate::Verdict;
use crate::repo;

// MECHANICAL SPLIT, and nothing moved across it changed. `max-lines` caps a file at 1000 and
// cannot exempt anything under `xtask/`, and the two tables below grow by ENTRY - one claim is
// about twenty lines - so the file that holds them is the one that has to have room. The
// boundary is the one seam here: `stale`, `versions` and `references` judge a LINE, while
// `claims` and `counts` judge a claim across lines and need a flattened view to do it.
mod claims;
// The citation half of `github.com/telekom/sutura#243`. A module rather than lines here for the
// reason above: this file has to have room for the tables, and a scan over Rust source shares
// nothing with them but the span walk and the task-name parse below.
mod advice;
// `github.com/telekom/sutura#295`, and its own module for `advice`'s reason. It shares the
// flattened view with `claims` and nothing else: what it resolves is a CONSTANT, out of the tree,
// which none of the tables above can express - a forbidden wording is a ratchet on a sentence
// somebody has already got wrong, and that one was true when it was written.
mod constants;
// `github.com/telekom/sutura#297`. Prose again, so it could have been lines here - it is a module
// because it grows by ENTRY like the tables above and this file is what has to have room for those.
mod hosts;
// `github.com/telekom/sutura#288`, and the one check here that registers NOTHING: what it reads is
// the page's own structure, so it needs no table and grows by rule rather than by entry. Its
// assertions are in `tests` below rather than beside it, for the reason `mod claims` gives.
mod pages;

use claims::{CONTRADICTED, COUNTS, contradicted_claims, count_mismatches, remedy_problems};
use constants::constant_problems;
use hosts::{HOSTED, host_mismatches};
use pages::{PageCounts, page_problems};

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
        why: "the flag went into every entry point while it was still a no-op, which is the \
              cheapest time to do it. It is load-bearing now: `sutura-config`, `sutura-http` and \
              `sutura-serve` each declare `tls`, so an entry point missing the flag lints and \
              tests nothing behind it",
        only: &[],
        except: &[".agents/skills/engineering/rust/SKILL.md"],
    },
    Forbidden {
        needle: "cargo fmt --all",
        instead: "cargo run -q -p xtask -- fmt (add --check to verify)",
        why: "`--all` formats every package cargo metadata reports, INCLUDING the path \
              dependencies `[workspace] exclude` keeps out of the member list - so it wanted to \
              rewrite the VENDORED mimalloc source, which is the one thing vendoring must not \
              do. xtask derives the member list instead. The justfile and the commit hook were \
              fixed for this and two devenv scripts were missed for weeks, which is why it is a \
              gate now rather than three comments",
        only: &[],
        // TWO exemptions, and deliberately not three. Both of these quote the form in order to
        // forbid it, so a detector with no exemption here would report its own reasoning - the
        // failure mode this repo deleted a whole gate over. `devenv.nix` is NOT exempt: it is the
        // file that actually carried the bug, so its comment is worded to avoid the literal
        // rather than exempted, and the gate therefore still guards it. Rust source is out of
        // scope for this whole module (see the filter in `run`), so `fmt.rs` needs no entry.
        except: &["justfile", ".pre-commit-config.yaml"],
    },
    Forbidden {
        needle: "cargo nextest run --workspace\"",
        instead: "cargo nextest run --workspace --all-features",
        why: "same reason as clippy: adopted while it was a no-op, load-bearing since `tls` \
              landed on three crates",
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
    Forbidden {
        // Here rather than in the `CLAIMS` table for two reasons. `xtask/src/guidance/claims.rs`
        // is at 985 of an unexemptable 1000 lines, and - the one that decides it - a
        // `Contradicted` entry retires itself when its evidence goes, which is right for a claim
        // resting on a CODE fact. This one rests on `docs/adr/0016` decision 7, a DECISION, and
        // reversing a decision is the case where the entry gets deleted rather than retired.
        needle: "property named `sutura`",
        instead: "one string-valued structured property under a name of the DEPLOYMENT's choosing; \
                  `sutura` is the field `document::MetricAspect` carries the scalar under on the \
                  adapter's own canonical shape, not a urn this repository dictates",
        why: "`docs/adr/0016`'s addendum decided the name is the deployment's, and the same change \
              wrote the opposite into five other places - a record, the plan, an example README \
              and two crate doc comments. That is exactly the one-cause-many-files shape this \
              module exists for. Rust source is out of scope for the filter in `run`, and it does \
              not have to be in it: a doc comment reaches `docs/api/**` through `just api`, which \
              `check-api-docs` forces, so the generated page is where a comment gets caught",
        only: &[],
        except: &[],
    },
    Forbidden {
        // TWO NEEDLES FOR ONE RULE, and the pair is the rule: a devenv script body assigned as a
        // literal - a `"` string here, a `''` block below - is shell that went nowhere near
        // `devenv.nix`'s `linted` wrapper, so ShellCheck never read it. The wrapper is what makes a
        // body unrunnable until it passes; nothing made a NEW body go through the wrapper, which is
        // `github.com/telekom/sutura#320`'s own fail-closed requirement and the half a reviewer
        // measured as still open: a plain-string body added beside the wrapped ones built and ran
        // with two findings in it and said nothing.
        //
        // A needle rather than a parser, because the failure direction is inverted from
        // `xtask/src/action_shell.rs`: that one has to EXTRACT and so fails open on a bad read,
        // while this one fails on what it FINDS. What it gives up is stated below.
        needle: ".exec = \"",
        instead: "onStable \"<name>\" \"<body>\" for a gate, or runs \"<name>\" \"<body>\" - both \
                  go through `linted`, whose checkPhase is `bash -n` plus ShellCheck",
        why: "a literal body is shell nothing reads: `nix/lint-workflows.sh` globs tracked `*.sh` \
              files, `nix/lint-action-shell.sh` reads composite actions, and the `shellcheck` hook \
              is `files: \\.sh$` - a Nix string is none of those. Measured: the derivation devenv \
              built for `ship-check` carried an EMPTY checkPhase, so those fifty lines had neither \
              ShellCheck nor a syntax check, and the first ShellCheck run over them found a live \
              command substitution inside an error message",
        only: &["devenv.nix"],
        except: &[],
    },
    Forbidden {
        // The block half of the pair above. Its own entry rather than a cleverer single needle,
        // because a substring rule is the whole mechanism here and two literals are cheaper to
        // read - and to keep true - than one pattern that has to mean both.
        needle: ".exec = ''",
        instead: "onStable \"<name>\" ''<body>'' for a gate, or runs \"<name>\" ''<body>''",
        why: "same as the entry above, and this is the shape that carried the defect: `ship-check` \
              is fifty lines of bash in a `''` block",
        only: &["devenv.nix"],
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
    /// Where the value may also appear, and must match if it does. **At least one page here has
    /// to state it**, or the pin is read and compared to nothing - see [`version_mismatches`].
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

/// The versions this line claims the pin IS, which is empty unless it names the pin file.
///
/// Deliberately narrow: only lines that also name the pin file are judged, because those are
/// the ones asserting what the pin is. A line mentioning some other version is not this
/// check's business, and a line naming the pin file with no version is a legitimate sentence
/// about where the pin lives.
///
/// Tokens rather than a verdict, because one walk answers both questions the gate has: *does this
/// line contradict the pin*, and *does any page state it at all*. The second is what keeps the
/// first from running over an empty set - see [`version_mismatches`].
fn stated_versions(line: &str, pin: &Pin) -> Vec<String> {
    if !line.contains(pin.marker) {
        return Vec::new();
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
        .map(String::from)
        .collect()
}

/// Does this line look like it states a version, other than the pinned one?
fn contradicts(line: &str, pin: &Pin, value: &str) -> bool {
    stated_versions(line, pin).iter().any(|t| t != value)
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

/// Every CLOSED backtick span in `text`, in order.
///
/// Fields 1, 3, 5 ... of a backtick split are the spans, and `take` before `skip` is what drops an
/// unterminated trailing backtick: the last field follows no closing one, so it is not a span.
///
/// One walk, here rather than in each of its callers, because the remedy check under `claims` and
/// [`advice`] read a citation the same way, and a second copy of "what is a backtick span" would be
/// a second thing to keep true - the class of drift this whole module is about.
pub(in crate::guidance) fn spans(text: &str) -> Vec<&str> {
    let parts: Vec<&str> = text.split('`').collect();
    let closed = parts.len().saturating_sub(1);
    parts.into_iter().take(closed).skip(1).step_by(2).collect()
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
            if !rule.only.is_empty() && !repo::matches_any(rule.only, rel) {
                continue;
            }
            if repo::matches_any(rule.except, rel) {
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
        let mut stated = 0_usize;
        for rel in files {
            if !repo::matches_any(pin.mentioned_in, rel) {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(root.join(rel)) else {
                continue;
            };
            for (i, line) in text.lines().enumerate() {
                if !stated_versions(line, pin).is_empty() {
                    stated = stated.saturating_add(1);
                }
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
        if stated == 0 {
            // The same failure `count_mismatches` fails for, on the shape whose documentation
            // calls itself "the third shape of the same idea" - and it was LIVE, not latent, when
            // this check was written: six pages named `rust-toolchain.toml` and not one carried a
            // version, so the comparison ran over an empty set every run while the success line
            // said `1 pin(s)`. A control over nothing reads as a control on the compiler version.
            problems.push(format!(
                "nothing under {:?} states the {} version - the pin is read and compared to \
                 nothing, so this entry in PINS is a gate over silence. State it on a line naming \
                 `{}`, or delete the entry",
                pin.mentioned_in, pin.name, pin.marker
            ));
        }
    }
    problems
}

/// Where a claim may be MADE: documentation and configuration, not Rust source.
///
/// Two reasons, and the second is the one that matters: a rule table written in Rust contains the
/// very phrases it forbids, so scanning `.rs` makes this gate report itself - and the only fix
/// would be an exclusion list, which is a hole anything can be added to. The narrower scope is
/// honest instead. A stale comment beside Rust code is reviewed with that code.
///
/// A function rather than a filter inline in [`run`] so the tree-wide assertion in `tests` asks
/// the same question this gate asks, off one definition of the scope rather than a second copy.
/// Filtered BEFORE cloning, which `iter_overeager_cloned` requires and is also the cheaper order:
/// the strings that do not survive the filter are never copied.
fn in_scope(files: &[String]) -> Vec<String> {
    files
        .iter()
        .filter(|f| has_ext(f, &["md", "nix", "yml", "yaml", "toml", "sh"]))
        // Mirrored upstream material makes claims about ITS repo, not ours.
        .filter(|f| !f.starts_with(".agents/skill-library/"))
        .filter(|f| !f.starts_with(".agents/skills/engineering/ms-rust/0"))
        .filter(|f| !f.starts_with(".agents/skills/engineering/ms-rust/1"))
        .cloned()
        .collect()
}

/// Every check that judges the tree against the pages, gathered where a test can call it.
///
/// **A function rather than a run of `extend` lines inside [`run`], and the reason is a measured
/// hole.** Replacing one of those lines with a discard left 871 tests green and the gate reporting
/// `ok` over a planted defect: each check is covered by its own unit tests, and the CALL was
/// covered by nothing. `tests::a_check_dropped_from_the_run_is_caught` holds this composition over
/// a fixture tree, so a check that stops being wired is red rather than silent.
fn tree_problems(root: &Path, files: &[String], text_files: &[String]) -> (Vec<String>, PageCounts) {
    let mut problems = stale_phrases(root, text_files);
    problems.extend(version_mismatches(root, text_files));
    problems.extend(contradicted_claims(root, text_files));
    problems.extend(count_mismatches(root, files, text_files));
    problems.extend(bad_task_references(root, text_files));
    // `files` and `text_files`, like `count_mismatches`: the mechanism is derived from ANY file, and
    // the scope limit is about where a claim may be made rather than about what may be read.
    problems.extend(host_mismatches(root, files, text_files));
    problems.extend(dead_paths(root, text_files));
    // The Markdown half, and the only check here that judges a page's SHAPE rather than a sentence
    // in it: `text_files` again, because a page is where an ordinal and a table are written.
    let (page, pages) = page_problems(root, text_files);
    problems.extend(page);
    (problems, pages)
}

pub(crate) fn run(_args: &[String]) -> Verdict {
    let Some(repo::RepoFiles { root, files }) = repo::all_files() else {
        eprintln!("xtask check-guidance: could not determine the repo root");
        return Verdict::Fail;
    };

    // Only text we might make a claim in. `files` is kept whole for `count_mismatches`, which
    // counts things in Rust, in snapshots and in anything else the claim is about - the scope
    // limit below is about where a CLAIM may be made, not about what may be counted.
    let text_files = in_scope(&files);

    let (mut problems, pages) = tree_problems(&root, &files, &text_files);
    // Not over `text_files`: the remedies are in this binary, which the scope above excludes for
    // the reason it states. They are judged against the tree rather than scanned in it.
    problems.extend(remedy_problems(&root));
    // Also not over `text_files`, and for the same reason one layer out: what a program PRINTS is
    // in `.rs`. `files` and not `text_files` is the whole point of `github.com/telekom/sutura#243`.
    let (advice, cited) = advice::advice_problems(&root, &files);
    problems.extend(advice);
    // Also `.rs`, and the same reason a third time: what a page PUBLISHES about a constant is a
    // doc comment, and `docs/api/**` is regenerated from it - so the scope limit above would put
    // this check on the derived copy rather than on the source of the claim.
    let (contradicted_constants, confirmed) = constant_problems(&root, &files);
    problems.extend(contradicted_constants);
    // FAIL CLOSED. A citation walk that reads nothing passes everything, which is the failure mode
    // `check-scope` and the remedy scan each guard separately. This tree prints dozens, so zero
    // means the span reader stopped reading rather than the advice being clean.
    if cited == 0 {
        problems.push(String::from(
            "read no `just` or `cargo xtask` citation out of any printed line in the workspace - \
             the scan is broken, not the source",
        ));
    }

    if problems.is_empty() {
        // The page numbers are PRINTED, not merely held: a floor nobody can read is a floor
        // nobody checks, and `{read} of {offered}` is what makes a narrowed walk visible in a
        // green run rather than only in a red one.
        println!(
            "xtask check-guidance: ok - {} file(s), {} phrase rule(s), {} pin(s), {} claim(s), {} count(s), {} derived host(s), {cited} printed citation(s), {confirmed} constant value(s) confirmed, {} of {} page(s) lexed, {} amendment heading(s)",
            text_files.len(),
            FORBIDDEN.len(),
            PINS.len(),
            CONTRADICTED.len(),
            COUNTS.len(),
            HOSTED.len(),
            pages.read,
            pages.offered,
            pages.headings
        );
        return Verdict::Pass;
    }

    eprintln!("xtask check-guidance: FAILED");
    for p in &problems {
        eprintln!("  {p}");
    }
    eprintln!();
    eprintln!("Guidance that no longer matches the repo is read as current. Fix the text, or");
    eprintln!(
        "if the rule itself is wrong, change it in xtask/src/guidance.rs - or in\n\
         xtask/src/guidance/claims.rs for a claim or a count, or\n\
         xtask/src/guidance/pages.rs for a page's own shape - with a reason."
    );
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
    fn a_line_naming_the_pin_file_with_no_version_states_nothing_either() {
        use super::stated_versions;
        // The distinction the vacuity check rests on: *fine* and *a statement* are not the same
        // verdict. Every marker line in this repo used to be the first kind, which is how a
        // working comparison ended up with nothing to compare.
        assert!(stated_versions("The compiler pin lives in rust-toolchain.toml and nowhere else.", &PIN).is_empty());
        assert_eq!(
            stated_versions("Pinned in rust-toolchain.toml at 1.98.0.", &PIN),
            vec![String::from("1.98.0")]
        );
        assert!(stated_versions("DataFusion 53.0.0 is the upstream version.", &PIN).is_empty());
    }

    #[test]
    fn a_pin_entry_is_compared_against_a_page_that_states_it() {
        // The mirror of `claims::tests::a_count_entry_is_compared_against_a_page_that_states_it`,
        // for the shape `Counted`'s own documentation calls the third of the same idea. This was
        // RED when it was written: the pin was `1.98.0`, six pages named the pin file, and none of
        // them said what the pin was.
        let root = crate::repo::root().expect("the repo root");
        let crate::repo::RepoFiles { files, .. } = crate::repo::all_files().expect("could not list the repo");
        for pin in super::PINS {
            let value = super::pinned_value(&root, pin).expect("the pin value");
            let stated = files
                .iter()
                .filter(|rel| crate::repo::matches_any(pin.mentioned_in, rel))
                .filter_map(|rel| std::fs::read_to_string(root.join(rel)).ok())
                .flat_map(|text| text.lines().map(|line| super::stated_versions(line, pin)).collect::<Vec<_>>())
                .any(|versions| !versions.is_empty());
            assert!(
                stated,
                "no page under {:?} states the {} version ({value}) beside `{}`",
                pin.mentioned_in, pin.name, pin.marker
            );
        }
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

    /// A page's prose lines, the way the gate reads them.
    fn page(text: &str) -> Vec<String> {
        crate::markdown::prose(text).unwrap_or_else(|why| panic!("{why}"))
    }

    #[test]
    fn an_amendment_list_is_consecutive_and_has_no_duplicate_ordinals() {
        use super::pages::sequence_problems;
        let straight = page("## Amendment, 2026-08-30\n## Second amendment\n## Third amendment\n");
        let (problems, walked) = sequence_problems("a.md", &straight);
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(walked, 3, "the walk reports what it iterated, and it is compared per page");

        // The shape issue 288 reported: a second Fifth, after the Sixth.
        let drifted = page(concat!(
            "## Amendment, 2026-08-30\n",
            "## Second amendment\n",
            "## Third amendment\n",
            "## Fourth amendment\n",
            "## Fifth amendment: where each identity claim is proven\n",
            "## Sixth amendment\n",
            "## Fifth amendment: the command-line tool opens a dataset too\n",
        ));
        let (problems, walked) = sequence_problems("a.md", &drifted);
        assert_eq!(walked, 7, "every heading is walked, not only the ones reported");
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].starts_with("a.md:7:"), "{problems:?}");
        assert!(
            problems[0].contains("fifth") && problems[0].contains("seventh"),
            "{problems:?}"
        );
    }

    #[test]
    fn only_the_first_amendment_may_be_unnumbered() {
        use super::pages::sequence_problems;
        // Unnumbered at the top predates the sequence, and records here open that way.
        // Unnumbered in the middle is the other half of 288, and a reader cannot cite it.
        let mid = page("## Amendment, 2026-08-30\n## Second amendment\n## Amendment, 2026-09-03\n");
        let (problems, _) = sequence_problems("a.md", &mid);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].starts_with("a.md:3:"), "{problems:?}");
        assert!(problems[0].contains("third"), "{problems:?}");
    }

    #[test]
    fn a_subsection_or_a_heading_about_something_else_is_not_in_the_sequence() {
        use super::pages::{Heading, amendment};
        assert_eq!(
            amendment("## Amendment, 2026-08-30: the run happened"),
            Some(Heading::Unnumbered)
        );
        assert_eq!(
            amendment("## Second amendment, 2026-08-30: registered"),
            Some(Heading::Numbered(2))
        );
        // A level-three heading is how this tree writes an addendum TO an amendment.
        assert_eq!(amendment("### Addendum to the amendment"), None);
        assert_eq!(amendment("### Second amendment"), None);
        assert_eq!(amendment("## The decision"), None);
        // An ordinal on its own is not a claim about the sequence.
        assert_eq!(amendment("## Second thoughts on the corpus"), None);
    }

    #[test]
    fn a_markdown_table_header_is_preceded_by_a_blank_line() {
        use super::pages::table_problems;
        let spaced = page("A paragraph.\n\n| a | b |\n| --- | --- |\n| 1 | 2 |\n");
        assert!(table_problems("a.md", &spaced).is_empty());
        // What a rebase did to one record's four-venues table: the pipes joined the paragraph.
        let run_on = page("A paragraph.\n| a | b |\n| --- | --- |\n");
        let problems = table_problems("a.md", &run_on);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].starts_with("a.md:2:"), "{problems:?}");
        // The same shape shown inside a fence is an example, not a table.
        let shown = page("A paragraph.\n\n```text\nA paragraph.\n| a | b |\n```\n");
        assert!(table_problems("a.md", &shown).is_empty());
    }

    #[test]
    fn a_shape_the_renderer_still_tables_is_not_reported() {
        use super::pages::table_problems;
        // MEASURED against this repository's own renderer - `.pixi/envs/docs` python-markdown with
        // `mkdocs.yml`'s extension list - and the first version of this rule reddened all four
        // while printing that they render as a paragraph. They do not: they render as tables.
        for above in [
            "### Heading",
            "# Heading",
            "---",
            "***",
            "___",
            "!!! note \"T\"",
            "??? note \"T\"",
        ] {
            let lines = page(&format!("{above}\n| a | b |\n| --- | --- |\n"));
            let problems = table_problems("a.md", &lines);
            assert!(
                problems.is_empty(),
                "{above:?} still tables, so it must not be reported: {problems:?}"
            );
        }
        // And the shapes the SAME renderer says swallow the pipes stay reportable, so the fix is a
        // narrowing rather than a hole: each of these renders no table at all.
        for above in ["- an item", "* an item", "1. an item", "> quoted", "A paragraph."] {
            let lines = page(&format!("{above}\n| a | b |\n| --- | --- |\n"));
            let problems = table_problems("a.md", &lines);
            assert_eq!(
                problems.len(),
                1,
                "{above:?} renders no table, so it must be reported: {problems:?}"
            );
        }
    }

    #[test]
    fn every_page_in_scope_numbers_its_amendments_and_spaces_its_tables() {
        // The tree-wide half, and why the rules above are not a ratchet on nothing: this was RED
        // when it landed, on the record 288 names and on two others that never numbered a second
        // amendment at all.
        let Some(crate::repo::RepoFiles { root, files }) = crate::repo::all_files() else {
            panic!("could not determine the repo root");
        };
        let (problems, counts) = super::pages::page_problems(&root, &super::in_scope(&files));
        assert!(problems.is_empty(), "{problems:#?}");
        // The floors, asserted rather than only printed: a sweep that read nothing satisfies an
        // empty problem list, and these are the two numbers that say it did not.
        assert_eq!(counts.read, counts.offered, "every page offered was lexed");
        assert!(counts.headings > 0, "this tree writes amendment headings");
    }

    #[test]
    fn a_check_dropped_from_the_run_is_caught() {
        // MEASURED HOLE this closes: replacing `problems.extend(page_problems(..))` in the run with
        // a discard left 871 tests green and the gate reporting `ok` over a planted defect. Every
        // check here has unit tests; the CALL had none. This holds the composition itself.
        let dir = std::env::temp_dir().join(format!("sutura-guidance-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).unwrap_or_default();
        std::fs::create_dir_all(&dir).expect("a fixture directory");
        let rel = String::from("record.md");
        // Both defects at once, so the page half is red for the sequence rule AND the table rule.
        // The amendment headings also keep the `headings == 0` floor satisfied, which is the point
        // of writing a record rather than a bare table.
        std::fs::write(
            dir.join(&rel),
            "## Amendment, 2026-08-30\n\ntext\n\n## Fourth amendment\n\nA paragraph.\n| a | b |\n| --- | --- |\n",
        )
        .expect("the fixture page");
        let files = vec![rel];
        let (problems, counts) = super::tree_problems(&dir, &files, &files);
        std::fs::remove_dir_all(&dir).unwrap_or_default();
        assert_eq!(counts.read, 1, "the fixture page was lexed");
        assert!(
            problems.iter().any(|p| p.contains("reads as the fourth amendment")),
            "the sequence rule must reach the run: {problems:#?}"
        );
        assert!(
            problems
                .iter()
                .any(|p| p.contains("a table starts against the paragraph above it")),
            "the table rule must reach the run: {problems:#?}"
        );
    }
}
