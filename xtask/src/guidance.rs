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
//! than read, and for `versions`, which judges a TOKEN beside a name read out of the manifests.
//!
//! Eleven checks, one theme: a claim in prose is only as good as the thing that verifies it.
//!
//! * `stale` - a forbidden phrase, each with the replacement and the reason
//! * `versions` - a version in a comment, where no mechanism compares it to the pin
//! * `claims` - a statement about what this repo has, checked against what it has
//! * `counts` - a number in prose that counts something, checked against the count
//! * `references` - a gate, task or skill named in prose must exist
//! * `remedies` - the correction a failure prints, held to the standard of the prose it corrects
//! * `advice` - a task a failure prints must exist, over every `.rs` file this repository publishes
//! * `constants` - a doc comment naming a variant of a constant it links must name the one it holds
//! * `absences` - a sentence saying the tree holds no such thing, held against the code that would
//!   refute it
//! * `hosts` - the file a sentence names as holding a mechanism must be the file that holds it
//! * `pages` - a page's amendment ordinals and its table headers, held against the page's own shape
//!
//! `remedies`, `advice`, `constants`, `absences` and `versions` are the five that read Rust rather
//! than prose; `hosts` reads prose and derives its answer from anywhere. `remedies` is here because of
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
//!
//! **Every listed file is read once, by [`inspect_listing`]**, and every check handed its `read`
//! judges those bytes, so a listed file that vanished or cannot be opened refuses the gate rather
//! than dropping out of the scan while the verdict still counts it. What that does NOT cover, next
//! to the claim: `absence_problems` still opens listed files itself, strictly as UTF-8, and pushes
//! a finding for each one it cannot read, so it fails closed outside the census. A file the listing never
//! offered - `remedy_problems`' evidence, `recipe_names`' justfile, `leg_two_citable`'s claims
//! matrix - is read from disk and is not held at all.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::Verdict;
use crate::repo;

// `github.com/telekom/sutura#638`, and a module for the reason every other split below gives:
// `max-lines` caps a file at 1000 and cannot exempt anything under `xtask/`, so the widened
// citation rule got room of its own rather than growing this file past it.
mod citations;
// MECHANICAL SPLIT, and nothing moved across it changed. `max-lines` caps a file at 1000 and
// cannot exempt anything under `xtask/`, and the two tables below grow by ENTRY - one claim is
// about twenty lines - so the file that holds them is the one that has to have room. The
// boundary is the one seam here: `stale` and `references` judge a LINE, while `claims` and
// `counts` judge a claim across lines and need a flattened view to do it.
mod claims;

// One claim whose refutation is a STATE another gate parses rather than a file this one can read:
// *leg 2 is proven*. Its own header carries the measurement - four surfaces said so over deleted
// code while this gate printed `ok`.
mod leg_two;
// The citation half of `github.com/telekom/sutura#243`. A module rather than lines here for the
// reason above: this file has to have room for the tables, and a scan over Rust source shares
// nothing with them but the span walk and the task-name parse below.
mod advice;
// `github.com/telekom/sutura#295`, and its own module for `advice`'s reason. It shares the
// flattened view with `claims` and nothing else: what it resolves is a CONSTANT, out of the tree,
// which none of the tables above can express - a forbidden wording is a ratchet on a sentence
// somebody has already got wrong, and that one was true when it was written.
mod constants;
// `github.com/telekom/sutura#370`, and `constants`' shape one subject over: that one resolves a
// CONSTANT the sentence links, this one resolves an ABSENCE the sentence asserts. Its own module
// for `constants`' reason, and the only check here that matches an authored WORDING inside a `.rs`
// doc comment - which the scope filter below refuses for everything that judges a sentence, so the
// narrowing that makes it safe (only `///` and `//!` are read) lives beside the check rather than
// as an exception here.
mod absences;
// `github.com/telekom/sutura#297`. Prose again, so it could have been lines here - it is a module
// because it grows by ENTRY like the tables above and this file is what has to have room for those.
mod hosts;
// `github.com/telekom/sutura#288`, and the one check here that registers NOTHING: what it reads is
// the page's own structure, so it needs no table and grows by rule rather than by entry. Its
// assertions are in `tests` below rather than beside it, for the reason `mod claims` gives.
mod pages;
// A module for `advice`'s reason, and it reads Rust for `constants`' one layer on: the name it
// keys on comes out of the manifests, and the copy it refuses is as often in a `//` comment as in
// a page. What it does NOT reach, and the measured false positives that decided its axis, are in
// its own header rather than here.
mod versions;

use absences::{Reading, absence_problems};
use citations::dead_paths;
use claims::{CONTRADICTED, COUNTS, contradicted_claims, count_mismatches, remedy_problems};
use constants::constant_problems;
use hosts::{HOSTED, host_mismatches};
use pages::{PageCounts, page_problems};
use versions::{Scan, comment_versions};

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
              `sutura-cli` each declare `tls`, so an entry point missing the flag lints and \
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
        // Here rather than in the `CLAIMS` table, and only ONE of the two reasons this used to
        // give survives. The line-count one does not: the table has since moved to
        // `xtask/src/guidance/claims/contradicted.rs` and has room. The one that decides it is
        // that a `Contradicted` entry retires itself when its evidence goes, which is right for a
        // claim resting on a CODE fact. This one rests on `docs/adr/0016` decision 7, a DECISION, and
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
    // `docs/adr/0016` decision 7's two never-write sentences, one row each. The decision states
    // them as a rule for every narrow connector's guidance, and until these rows nothing measured
    // it: the phrases appeared only in the two pages that QUOTE them in order to forbid them, so
    // the rule was held by recall on whoever wrote the next connector page. That is the case this
    // table exists for, and two rows are the cheapest mechanism it has.
    //
    // LIMIT, because an overstated gate is worse than none: `stale_phrases` matches per LINE, so
    // either sentence wrapped across two lines escapes both rows, and so does any paraphrase -
    // the same ratchet-not-proof limit this module's header already records for every row here.
    Forbidden {
        needle: "Configure your",
        instead: "what a deployment MAY do and what each choice buys it",
        why: "`docs/adr/0016` decision 7: a narrow connector's guidance does not get to tell a \
              deployment how to configure a system it owns. Every choice is an option with a \
              payoff, and none of them is a precondition for the source to load and the prompt to \
              be true",
        only: &[],
        except: &[],
    },
    Forbidden {
        needle: "not usable without",
        instead: "the empty state, named as the supported configuration it is",
        why: "`docs/adr/0016` decision 7, and this half is not a matter of register - it is FALSE. \
              A narrow source loads with zero metrics, zero comments and no raw tool, and pins a \
              bundle whose declaration is exactly that truth. Writing the sentence would make a \
              complete state read as a broken one",
        only: &[],
        // The two pages that QUOTE the sentence in order to forbid it. Without them the detector
        // reports its own reasoning, the way `cargo fmt --all` above needs its two exemptions.
        except: &["docs/adr/0016-what-datahub-can-carry.md", "docs/implementation-plan.md"],
    },
];

// A DEVENV SCRIPT BODY IS NOT HELD HERE ANY MORE, and the deletion is the fix rather than a
// relaxation. Two entries used to forbid the literals `.exec = "` and `.exec = ''` in
// `devenv.nix`. `github.com/telekom/sutura#402` measured six spellings that walked past them with
// a three-finding body in the tree and every gate green: no leading dot, two spaces, a newline
// after the `=`, the `''` form, any attribute that is not `exec` - `enterTest`, and `enterShell`
// itself - and the same literal in a module `imports` reaches, since a slash-less `only:` pattern
// matches a bare basename and nothing else.
//
// `crate::devenv_shell` keys on the `=` and on the attribute NAME instead, so all six are one
// case, and it also holds the wrapper's own argument set - which no phrase rule could. Kept
// alongside, the needles would have enumerated one input of a rule that subsumes them, which is
// the shape `github.com/telekom/sutura#384`'s fix deleted rather than kept.

// A `Pin` table stood here: one entry, the compiler, compared against every page that named
// `rust-toolchain.toml` beside a version. It is gone rather than widened, and the reason is that
// its shape required the thing it was guarding against. It held a version in prose CORRECT, so at
// least one page had to state one - it failed when none did - and a rule that refuses the copy
// outright cannot also require it. `versions` refuses it, so the pin is read from the pin file by
// whoever needs it and written down nowhere. See `xtask/src/guidance/versions.rs`.

/// Case-insensitive extension test. A case-sensitive one is a bug on a case-insensitive
/// filesystem, which is where half of this repo is developed.
///
/// Shared with [`versions`] rather than copied: that check splits a file by KIND to find its
/// comment marker, which is the same question this answers, and `ends_with(".yml")` there was
/// the same bug one module over - clippy's `case_sensitive_file_extension_comparisons` said so.
pub(in crate::guidance) fn has_ext(path: &str, exts: &[&str]) -> bool {
    std::path::Path::new(path)
        .extension()
        .and_then(std::ffi::OsStr::to_str)
        .is_some_and(|e| exts.iter().any(|want| e.eq_ignore_ascii_case(want)))
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
fn bad_task_references(read: &crate::causality::regions::PostImage<'_>, files: &[String]) -> Vec<String> {
    let known = known_tasks();
    let mut problems = Vec::new();
    for rel in files {
        if !has_ext(rel, &["md", "nix", "yaml", "yml"]) {
            continue;
        }
        let Some(text) = read(rel) else {
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

fn stale_phrases(read: &crate::causality::regions::PostImage<'_>, files: &[String]) -> Vec<String> {
    let mut problems = Vec::new();
    for rel in files {
        let Some(text) = read(rel) else {
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
/// What [`tree_problems`] answers: the problems, plus what each walk that carries a floor read.
/// A named type because the tuple grew past what clippy will read as one.
type TreeVerdict = (Vec<String>, PageCounts, Reading, Scan);

fn tree_problems(
    root: &Path,
    read: &crate::causality::regions::PostImage<'_>,
    strict_read: &crate::causality::regions::PostImage<'_>,
    files: &[String],
    text_files: &[String],
) -> TreeVerdict {
    let mut problems = stale_phrases(read, text_files);
    problems.extend(contradicted_claims(root, read, text_files));
    // `text_files`, and the CONDITION comes from another gate's parser rather than from a needle in
    // a page: `crate::venues::leg_two_citable` reads the claims matrix. Its own module header
    // carries why this is not a `CONTRADICTED` row and what the pair does not reach.
    problems.extend(leg_two::problems(root, read, text_files));
    problems.extend(count_mismatches(read, files, text_files));
    problems.extend(bad_task_references(read, text_files));
    // `files` and `text_files`, like `count_mismatches`: the mechanism is derived from ANY file, and
    // the scope limit is about where a claim may be made rather than about what may be read.
    problems.extend(host_mismatches(strict_read, files, text_files));
    problems.extend(dead_paths(root, read, text_files));
    // `files`, and INSIDE this function rather than beside its caller in `run`. It reads a doc
    // comment as PROSE and the code that would refute it, so neither list is `text_files` - but the
    // reason it is here is the one this function exists for: measured in review, replacing
    // `problems.extend(refuted)` in `run` with a discard left 1023 tests green and printed a verdict
    // byte-identical to a clean run over a planted refutation. The CALL was covered by nothing,
    // exactly as `page_problems`' was.
    let (refuted, absence_read) = absence_problems(root, files);
    problems.extend(refuted);
    // The Markdown half, and the only check here that judges a page's SHAPE rather than a sentence
    // in it: `text_files` again, because a page is where an ordinal and a table are written.
    let (page, pages) = page_problems(strict_read, text_files);
    problems.extend(page);
    // `files`, and here rather than beside its caller for the reason above: a version in a `//`
    // comment is the instance this check was widened for, so `text_files` would have put it on
    // the pages and left the source it is copied from alone.
    let (copies, scan) = comment_versions(read, files);
    problems.extend(copies);
    (problems, pages, absence_read, scan)
}

/// The listed paths, their text, and the census's verdict line.
type Listed = Result<(Vec<String>, Texts, String), String>;

/// Keep the original lossy view for checks that already used it; host and page checks use the
/// strict view that their former `read_to_string` calls provided.
#[derive(Default)]
pub(in crate::guidance) struct Texts {
    lossy: BTreeMap<String, String>,
    invalid: BTreeSet<String>,
}

impl Texts {
    pub(in crate::guidance) fn get(&self, rel: &str) -> Option<&String> {
        self.lossy.get(rel)
    }

    pub(in crate::guidance) fn strict(&self, rel: &str) -> Option<String> {
        if self.invalid.contains(rel) {
            None
        } else {
            self.lossy.get(rel).cloned()
        }
    }
}

/// Every listed file, read once through [`repo::Census::inspect`]: the paths, their text, and the
/// census's verdict line.
///
/// A listed file that vanished after discovery is refused here. `inspect` counts it as absent
/// rather than unreachable, because `git ls-files` reads the index and a tracked file deleted with
/// the deletion unstaged is offered and not on disk; this gate refuses it anyway, since the
/// transitional door it replaces let every check skip that file while the verdict still counted it.
/// Paths remain listed when their bytes are not UTF-8. The original lossy text remains available
/// to existing checks, while the strict view returns `None` so host and page checks refuse an
/// unreadable in-scope file. Binary files outside their scope do not cause a refusal - `#412`'s trap.
pub(in crate::guidance) fn inspect_listing(found: Result<repo::Census, repo::Refusal>) -> Listed {
    let mut texts = Texts::default();
    let mut files = Vec::new();
    let inspected = found
        .map_err(|why| why.describe())?
        .inspect(
            &[],
            |_| true,
            |rel, bytes| {
                files.push(String::from(rel));
                let text = if let Ok(valid) = std::str::from_utf8(bytes) {
                    String::from(valid)
                } else {
                    texts.invalid.insert(String::from(rel));
                    String::from_utf8_lossy(bytes).into_owned()
                };
                texts.lossy.insert(String::from(rel), text);
            },
        )
        .map_err(|why| why.describe())?;
    if inspected.absent() != 0 {
        return Err(format!(
            "{} listed file(s) vanished after discovery, so the scan is incomplete",
            inspected.absent()
        ));
    }
    files.sort_unstable();
    Ok((files, texts, inspected.verdict()))
}

pub(crate) fn run(_args: &[String]) -> Verdict {
    let Some(root) = repo::root() else {
        eprintln!("xtask check-guidance: FAILED - could not determine the repo root");
        return Verdict::Fail;
    };
    let (files, texts, witness) = match inspect_listing(repo::all_files()) {
        Ok(triple) => triple,
        Err(why) => {
            eprintln!("xtask check-guidance: FAILED - {why}");
            return Verdict::Fail;
        }
    };
    let text_of = |rel: &str| texts.get(rel).cloned();
    let strict_text_of = |rel: &str| texts.strict(rel);

    // Only text we might make a claim in. `files` is kept whole for `count_mismatches`, which
    // counts things in Rust, in snapshots and in anything else the claim is about - the scope
    // limit below is about where a CLAIM may be made, not about what may be counted.
    let text_files = in_scope(&files);

    let (mut problems, pages, read, scan) = tree_problems(&root, &text_of, &strict_text_of, &files, &text_files);
    // Not over `text_files`: the remedies are in this binary, which the scope above excludes for
    // the reason it states. They are judged against the tree rather than scanned in it.
    let (remedied, live_remedies) = remedy_problems(&root);
    problems.extend(remedied);
    // Also not over `text_files`, and for the same reason one layer out: what a program PRINTS is
    // in `.rs`. `files` and not `text_files` is the whole point of `github.com/telekom/sutura#243`.
    let (advice, cited) = advice::advice_problems(&root, &text_of, &files);
    problems.extend(advice);
    // Also `.rs`, and the same reason a third time: what a page PUBLISHES about a constant is a
    // doc comment, and `docs/api/**` is regenerated from it - so the scope limit above would put
    // this check on the derived copy rather than on the source of the claim.
    let (contradicted_constants, confirmed) = constant_problems(&text_of, &files);
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

    // Byte-identical problems are ONE finding several checks reached, not several findings: every
    // check that walks the tree reads through the census's own held bytes, so one sealed page
    // printed the same `cannot be read` line twenty-five times - measured on `docs/architecture.md`
    // at mode 000. Order-preserving, so the grouping a reader already gets is unchanged, and
    // exact-match only: two checks reporting different things about one file are still two lines.
    let mut seen = BTreeSet::new();
    problems.retain(|problem| seen.insert(problem.clone()));

    if problems.is_empty() {
        // The page numbers are PRINTED, not merely held: a floor nobody can read is a floor
        // nobody checks, and `{read} of {offered}` is what makes a narrowed walk visible in a
        // green run rather than only in a red one.
        println!(
            "xtask check-guidance: ok - {} file(s), {} phrase rule(s), {} pinned name(s) over {} comment line(s), {live_remedies} of {} claim(s), {} count(s), {} derived host(s), {cited} printed citation(s), {confirmed} constant value(s) confirmed, {} absence statement(s) over {} file(s) and {} production line(s), {} of {} page(s) lexed, {} amendment heading(s); {witness}",
            text_files.len(),
            FORBIDDEN.len(),
            scan.names,
            scan.comments,
            CONTRADICTED.len(),
            COUNTS.len(),
            HOSTED.len(),
            // THREE numbers from three places, printed for a reader to compare runs with - and
            // NOT the floor, which is `github.com/telekom/sutura#414`: each is derived by
            // iterating what a walk returned, so a narrowing moves them with itself. Measured,
            // `.take(100)` on the per-file line walk printed 80244 of 294744 here at exit 0.
            // `absences::Sighted::short` is what refuses a short walk now.
            read.stated,
            read.files,
            read.lines,
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
         xtask/src/guidance/claims/contradicted.rs for a claim,\n\
         xtask/src/guidance/claims/counts.rs for a count, or\n\
         xtask/src/guidance/pages.rs for a page's own shape, or\n\
         xtask/src/guidance/versions.rs for a version - with a reason."
    );
    eprintln!();
    eprintln!("A version in a comment is a copy nothing compares, so it rots and then misleads.");
    eprintln!("Name the dependency and not the number - `ureq` at the resolved version and");
    eprintln!("features is already in the graph - and leave the value in the file that pins it.");
    Verdict::Fail
}

#[cfg(test)]
mod tests {
    use super::known_tasks;

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
    fn the_leg_two_rule_is_wired_into_the_walk_and_not_merely_written() {
        // **RED WHEN WRITTEN, and the defect was in the fix rather than in the tree.** Review
        // measured that turning `problems.extend(leg_two::problems(..))` in `tree_problems` into
        // `drop(..)` left 92 tests green, and that with the call gone the pre-round-3 `AGENTS.md`
        // sentence passed `check-guidance` at exit 0. Every cell the rule had drove
        // `leg_two::stated_in` or `venues::leg_two_row::citable_in` directly, so the REGISTRATION
        // was held by nothing - the same defect class the rule itself exists to catch.
        //
        // `api_docs::tests`' unarmed-tree cell is the shape: drive the WALK against a fixture root,
        // so the only way the problem can appear is through the call site under test.
        let root = std::env::temp_dir().join(format!("sutura-guidance-leg-two-{}", std::process::id()));
        drop(std::fs::remove_dir_all(&root));
        std::fs::create_dir_all(root.join("docs")).expect("fixture root");
        // A venue page whose two leg-2 rows carry no citable verdict, which is the condition.
        std::fs::write(
            root.join("docs/where-identity-is-proven.md"),
            "| Claim | Fake at the port |\n| --- | --- |\n\
             | Whether two distinct subjects resolve to two distinct principals | **wired** |\n\
             | A served binary executes as a verified human caller through the declared per-source map | - |\n",
        )
        .expect("fixture venue page");
        // And a page stating the claim anyway.
        std::fs::write(
            root.join("AGENTS.md"),
            "Leg 1 is built. Leg 2 (a source executing AS them) is proven for BigQuery.\n",
        )
        .expect("fixture claimant");

        let subjects = vec![String::from("AGENTS.md")];
        let read = |rel: &str| std::fs::read_to_string(root.join(rel)).ok();
        let (problems, ..) = super::tree_problems(&root, &read, &read, &subjects, &subjects);
        drop(std::fs::remove_dir_all(&root));

        assert!(
            problems.iter().any(|problem| problem.contains("no leg-2 row")),
            "the leg-2 rule is not reached by the walk this gate runs: {problems:?}"
        );
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
        assert!(
            table_problems("a.md", &spaced).is_empty(),
            "a properly spaced table reports no problems"
        );
        // What a rebase did to one record's four-venues table: the pipes joined the paragraph.
        let run_on = page("A paragraph.\n| a | b |\n| --- | --- |\n");
        let problems = table_problems("a.md", &run_on);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].starts_with("a.md:2:"), "{problems:?}");
        // The same shape shown inside a fence is an example, not a table.
        let shown = page("A paragraph.\n\n```text\nA paragraph.\n| a | b |\n```\n");
        assert!(
            table_problems("a.md", &shown).is_empty(),
            "a table shown inside a fence is not a problem"
        );
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
        let (files, texts, _witness) = super::inspect_listing(crate::repo::all_files()).expect("the file census");
        let text_of = |rel: &str| texts.strict(rel);
        let (problems, counts) = super::pages::page_problems(&text_of, &super::in_scope(&files));
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
            "## Amendment, 2026-08-30\n\nPinned at nightly-2020-01-02.\n\n## Fourth amendment\n\nA paragraph.\n| a | b |\n| --- | --- |\n",
        )
        .expect("the fixture page");
        // THE ABSENCE HALF, and it is here because the same discard worked a second time: this
        // fixture carries no file a registered `stated_in` glob matches, so every entry in
        // `ABSENCES` is a gate over silence over it - which `absence_problems` reports and a
        // dropped call does not. Measured before the check moved into `tree_problems`: replacing
        // `problems.extend(refuted)` in `run` left 1023 tests green and the gate at exit 0 over a
        // planted refutation, byte-identical to a clean run.
        // THE ADR HALF (`github.com/telekom/sutura#937`): a record named by a new ordinal. Empty, so
        // it adds a page and nothing else; dropping `adr_number_problems` from `page_problems` left
        // every test green until this.
        let record = String::from("docs/adr/0041-x.md");
        std::fs::create_dir_all(dir.join("docs/adr")).expect("the fixture's docs/adr");
        std::fs::write(dir.join(&record), "").expect("the fixture record");
        let files = vec![rel, record];
        let read = |rel: &str| std::fs::read_to_string(dir.join(rel)).ok();
        let (problems, counts, read, scan) = super::tree_problems(&dir, &read, &read, &files, &files);
        std::fs::remove_dir_all(&dir).unwrap_or_default();
        assert_eq!(counts.read, 2, "the fixture page and record were lexed");
        assert!(
            problems.iter().any(|p| p.contains("0041 is a new ordinal")),
            "the ADR name rule must reach the run: {problems:#?}"
        );
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
        assert!(
            // `ABSENCES` by name rather than the shared phrase, which more than one check emits.
            problems
                .iter()
                .any(|p| p.contains("this entry in ABSENCES is a gate over silence")),
            "the absence check must reach the run: {problems:#?}"
        );
        // THE VERSION HALF. A `nightly-<date>` is the one token this check refuses with no name
        // beside it, which is what lets a fixture with no manifest in it hold the call: the name
        // harvest reads nothing here, so a keyed instance could not fire and a dropped
        // `problems.extend(copies)` would look identical to a clean run.
        assert!(
            problems.iter().any(|p| p.contains("nightly-2020-01-02")),
            "the version check must reach the run: {problems:#?}"
        );
        // And their numbers come back through this function, so a caller that stopped reading
        // them is a compile error rather than a silent zero.
        assert_eq!(read.stated, 0, "the fixture states no registered absence");
        assert_eq!(scan.comments, 9, "every line of the fixture page is prose");
    }
    /// The controls a refusal needs beside it: a subject in scope, one out of scope, an empty one,
    /// one with no trailing newline, and one the listing offers that is not on disk.
    ///
    /// **The refusal, not the predicate.** `github.com/telekom/sutura#619` measured three gates
    /// dropping an unreadable in-scope file; this gate then had the same drop in some of its own
    /// checks and not others, because each check decided again. The census's own read is what they
    /// share now, and the drop it replaced was invisible in a green run for the reason the census
    /// module states: the number this gate prints comes off the LISTING, so a dropped read leaves
    /// the verdict announcing files it never opened rather than moving a denominator.
    ///
    /// Out of scope is the control that matters as much as the refusal - `#412` reddened a correct
    /// tree by refusing a PNG for not being text - so a sealed file the check filters out by path
    /// must stay silent.
    #[test]
    fn an_unreadable_subject_in_scope_is_a_finding_and_one_out_of_scope_is_not() {
        let mut tree = crate::scratch_tree::Tree::of(
            "guidance-unreadable",
            &[
                ("docs/page.md", b"a page with nothing forbidden on it\n"),
                ("docs/empty.md", b""),
                ("docs/unterminated.md", b"no trailing newline"),
                ("docs/logo.png", b"\x89PNG\r\n\x1a\n binary"),
            ],
        );
        let pages: Vec<String> = ["docs/page.md", "docs/empty.md", "docs/unterminated.md", "docs/logo.png"]
            .iter()
            .map(|rel| String::from(*rel))
            .chain(std::iter::once(String::from("docs/deleted-but-listed.md")))
            .collect();
        let root = tree.root().to_path_buf();
        let read = |rel: &str| std::fs::read_to_string(root.join(rel)).ok();

        // CONTROL: readable in scope, empty, unterminated, and a path the listing offers that is
        // not on disk - `git ls-files` reads the index, so that last one is the shape refusing
        // `NotFound` would turn red for anyone mid-edit.
        assert!(
            super::stale_phrases(&read, &pages).is_empty(),
            "a readable page, an empty one, an unterminated one and an absent one are not findings"
        );
        assert!(
            super::dead_paths(&root, &read, &pages).is_empty(),
            "no page cites a dead route"
        );

        // Mode bits are ignored for uid 0 and this suite runs as root in some venues, so a test
        // that asserted the refusal without checking would assert nothing there.
        if !tree.seal("docs/logo.png") {
            return;
        }
        assert!(
            super::dead_paths(&root, &read, &pages).is_empty(),
            "a sealed file this check filters out by path is out of scope, not unreadable - #412"
        );

        if !tree.seal("docs/page.md") {
            return;
        }
        // The checks read the census's bytes now, so the refusal is the census's: a sealed page
        // never reaches them as a skipped `None`, because the read that would hand it over refuses.
        let refused = super::inspect_listing(Ok(crate::repo::collect_files(&root, &root.join("docs"), &["md", "png"])));
        assert!(
            refused.is_err_and(|problem| problem.contains("docs/page.md")),
            "an unreadable page in scope must be named"
        );
    }

    #[test] // A listed file that disappears must invalidate the verdict, not drop out of every check.
    fn a_listed_guidance_file_that_vanishes_refuses_the_scan() {
        let tree = crate::scratch_tree::Tree::of(
            "guidance-vanished",
            &[
                ("docs/kept.md", b"a page that stays\n"),
                ("docs/gone.md", b"a page that is deleted after the listing\n"),
            ],
        );
        let census = crate::repo::collect_files(tree.root(), &tree.root().join("docs"), &["md"]);
        std::fs::remove_file(tree.root().join("docs/gone.md")).expect("a vanished page");
        assert!(
            super::inspect_listing(Ok(census)).is_err_and(|problem| problem.contains("1 listed file(s) vanished")),
            "a listed file that vanished must refuse the scan"
        );
    }

    #[test]
    fn invalid_utf8_stays_offered_without_becoming_readable_text() {
        let tree = crate::scratch_tree::Tree::of("guidance-invalid-text", &[("docs/bad.md", &[0xff_u8])]);
        let census = crate::repo::collect_files(tree.root(), &tree.root().join("docs"), &["md"]);
        let (files, texts, _witness) = super::inspect_listing(Ok(census)).expect("the file census");
        assert_eq!(files, vec![String::from("docs/bad.md")]);
        assert_eq!(
            texts.get("docs/bad.md").map(String::as_str),
            Some("\u{fffd}"),
            "existing scans retain their lossy text"
        );
        assert_eq!(
            texts.strict("docs/bad.md"),
            None,
            "strict scans must reach the unreadable arm"
        );
    }
}
