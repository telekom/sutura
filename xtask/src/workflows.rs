//! Do workflows reference flake outputs that exist, and does ordinary CI avoid release outputs?
//!
//! CI reaches every tool through `nix run .#name` or `nix build .#checks.<system>.name`. A
//! renamed or deleted output is not a build error - it is a workflow that fails at the moment
//! that step runs, minutes into a run, on a push that already happened.
//!
//! That is exactly what happened: the docs toolchain moved from a nix Python environment to
//! pixi, `apps.mkdocs` and `apps.mike` were deleted, and `docs.yml` still called them. Nothing
//! local could have noticed - clippy does not read YAML and zizmor does not read flake.nix -
//! so the first report was a red run on the pull request.
//!
//! THREE PLACES, not one: `.github/workflows`, `.github/actions` and the shared shell under `nix/`.
//! The two additions were holes rather than widenings, and they are the same hole. `nix run .#cosign`
//! has lived in a local composite action since that sequence was split out of `release.yml`, so the
//! one reference that publishes a release was the one reference nothing checked; and
//! `nix/run-gate.sh` names `.#deny`, `.#betterleaks` and `.#crap`, which decide whether three gates
//! run at all. **A reference leaves this gate's sight whenever a step moves out of a workflow, and a
//! hard line cap is what forces steps out** - so the scan follows the shell.
//!
//! Text scanning on both sides, because this has to run where there is no nix. It cannot know
//! whether an output BUILDS; it knows whether it is declared, which is the failure that recurs.
//!
//! **`nix eval .#checks.<system> --apply builtins.attrNames` IS the authority and cannot be the
//! mechanism.** It is the obvious answer to "which checks exist" and it was weighed: the gate's own
//! venue rules it out. `check-workflows` runs inside `checks.hygiene`, a nix derivation with no nix
//! and no network, and evaluating that attrset needs the flake's inputs - crane and nixpkgs - which
//! means either fetching them or a second, differently-shaped gate outside the sandbox. So the
//! authority is unreachable exactly where the check runs, and what replaced the brace counting is a
//! Nix *lexer* rather than a Nix *evaluator*: see [`code_lines`] for the three shapes that fooled
//! the counting, and note that the resulting parse is now cross-checked against the tree by a unit
//! test rather than trusted.
//!
//! Literal package builds and the two release-profile assertions are also refused in ordinary CI -
//! `ci.yml` AND every local workflow or composite action it calls, transitively, which is
//! [`reach`]'s walk and not one file name. Pull requests use interpolated `-ci` packages for their
//! link matrix; the tag-triggered release workflow owns everything that is published.

use crate::Verdict;
use crate::repo;
use std::collections::BTreeSet;

// The three places CI invokes something from. Shared with `crate::venues`, which asks a different
// question of the same files - see that module's header for why one walk rather than two.
pub(crate) mod sources;
// One job of a workflow, and one step inside it. Its own module because what it reads is a
// workflow's STRUCTURE rather than the flake references this file scans for, and two gates need
// the same reader: `venues::acceptance` for the acceptance job's properties, and
// `default_feature_tests` for whether a step still invokes it.
pub(crate) mod step;

// Which jobs GATE a merge, and which only look as though they do. Its own file for the reason
// `shipped::refusal` is: this one is against the unexemptable 1000-line cap. It reads a different
// authority - `devco/required-contexts`, a record of an API answer - and its fixtures come with it.
mod contexts;

// WHICH FILES ORDINARY CI ACTUALLY RUNS. One walk of the local `uses:` call graph, read by both
// halves of this gate: the release-output refusal below, which used to read one file name while a
// line cap moved a job into a second, and `contexts`, which classified the jobs of the workflows
// whose own `on:` block gates and therefore classified none of a CALLED workflow's.
mod reach;

// A BADGE IS A PUBLIC CLAIM, held against the mechanism it claims. Its own file because it reads
// two more authorities - `README.md` and `devco/scorecard-publication` - and because a new entry
// in `main.rs`'s task table is not available: that file stands at 999 lines against a cap
// `crates/` and `xtask/` cannot be exempted from. It belongs here regardless: *what may a workflow
// do* is this gate's question, and it already walks every place CI invokes something from.
mod scorecard;

// CAN THE PUBLICATION THE BADGE IS SERVED FROM LAND? `scorecard` holds badge <-> publication
// DECLARED; this holds the workflow against the shape `api.scorecard.dev` will actually accept,
// because three runs reported success while the API refused every one of them.
mod publication;

// AN ACCEPTED SCORE IS A CLAIM ABOUT ITS STAND-IN, and `docs/adr/0025` accepts Scorecard's SAST
// zero on the strength of two mechanisms inside the required context. Its own file for
// `scorecard`'s reasons plus one: that module stands at 912 lines against the same cap, so the
// rule could not join it. See `sast`'s header for each refusal and what it does not hold.
mod sast;

// CAN A `pull_request` PATH WRITE THE ACTIONS CACHE? Its own file for `sast`'s reason - this one
// is against the unexemptable 1000-line cap - and the seam is the question: every other rule here
// asks whether a reference RESOLVES, this one asks what a step is ALLOWED to do. See its header for
// the name list it is limited to and why a job-level condition is deliberately not accepted.
mod cache_scope;

// READING A NAMED BLOCK OUT OF `flake.nix`, in its own file because this one reached the
// 1000-line cap the moment two changes registered a rule module in the same window. The seam is
// the question, not the line count: that module answers *which attributes does this block
// declare, and did it close*, for four callers. See its header.
mod nix_block;
pub(crate) use nix_block::declared_block;
// `block_source` is read only by `crate::compose::file`, which is a `#[cfg(test)] mod` - so in the
// binary it is dead code the compiler is right to refuse, and the gate on the re-export has to
// match the gate on the item.
#[cfg(test)]
pub(crate) use nix_block::block_source;

/// Which output namespace a reference points into.
///
/// `Runnable` and not `App`: `nix run .#name` resolves an app OR a package with a matching main
/// program, and this repo relies on that - `nix run .#xtask` runs `packages.xtask`, which has no
/// `apps.xtask`. Checking only `apps` reported every such call as missing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Runnable,
    Check,
}

impl Kind {
    const fn label(self) -> &'static str {
        match self {
            Self::Runnable => "apps or packages",
            Self::Check => "checks",
        }
    }
}

/// One reference, and where it was written.
struct Reference {
    workflow: String,
    line: usize,
    kind: Kind,
    name: String,
}

pub(crate) fn run(_args: &[String]) -> Verdict {
    let Some(root) = repo::root() else {
        eprintln!("xtask check-workflows: could not locate the repo root");
        return Verdict::Fail;
    };

    // ORDINARY CI IS PLURAL, and NAMING one file is how the refusal below stopped covering it:
    // `cross-link.yml` was lifted out of `ci.yml` at 999 lines against the 1000-line cap, and a
    // literal release build written there was refused by nothing. The root set is DERIVED - every
    // workflow whose `on:` block names a gating event - because rooting the walk at `ci.yml` left
    // `docs.yml` and `security-audit.yml` outside it, which is the same defect one file over.
    let ordinary = contexts::OrdinaryCi::read(&root);
    let unreachable = ordinary.unreachable();
    if !unreachable.is_empty() {
        eprintln!("xtask check-workflows: FAILED - ordinary CI is not fully readable from here\n");
        for problem in &unreachable {
            eprintln!("  {problem}");
        }
        eprintln!();
        eprintln!("A call this gate cannot open is a step it cannot refuse, so it fails closed. The");
        eprintln!("release-output refusal is over the files the walk opened, and that has to be all");
        eprintln!("of them - see the header of xtask/src/workflows/reach.rs for its arms.");
        return Verdict::Fail;
    }
    let walked = reach::walked(ordinary.closure());
    if walked.is_empty() {
        eprintln!("xtask check-workflows: no workflow runs on a pull request - the scan is broken");
        eprintln!("  rather than the workflows, and every rule below would pass over nothing");
        return Verdict::Fail;
    }
    let release_builds = reach::release_outputs(ordinary.closure());
    if !release_builds.is_empty() {
        eprintln!("xtask check-workflows: ordinary CI builds release outputs");
        for found in &release_builds {
            eprintln!("  {found}");
        }
        eprintln!("Release outputs belong to the tag-triggered release workflow, not ordinary CI -");
        eprintln!("and ordinary CI is every pull-request workflow plus everything they call:");
        eprintln!("  {}", walked.join(", "));
        return Verdict::Fail;
    }

    let flake = match std::fs::read_to_string(root.join("flake.nix")) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("xtask check-workflows: could not read flake.nix: {error}");
            return Verdict::Fail;
        }
    };
    // Apps and packages together, because `nix run` accepts either.
    let mut runnable = declared_apps(&flake);
    let (Some(packages), Some(checks)) = (declared_block(&flake, "packages = "), declared_block(&flake, "checks = {")) else {
        eprintln!("xtask check-workflows: a flake output block does not close in flake.nix");
        eprintln!("  the scan is broken, not the workflows - it would otherwise report names");
        eprintln!("  from whatever follows the block, and lose the ones inside it");
        return Verdict::Fail;
    };
    runnable.extend(packages);

    // An empty side would make this gate pass by finding nothing - the failure mode a
    // text-scanning check is most prone to.
    if runnable.is_empty() || checks.is_empty() {
        eprintln!("xtask check-workflows: parsed no runnables or no checks out of flake.nix");
        eprintln!("  the scan is broken, not the workflows");
        return Verdict::Fail;
    }

    let Some(Scan { references, files }) = gather(&root) else {
        return Verdict::Fail;
    };

    // WHICH JOBS GATE A MERGE. Nothing in this repository could say so before: the required set
    // lived only in GitHub's API, so *the four cross link legs block a merge* was believed by
    // readers and checked by nothing - and it was false.
    let unclassified = contexts::problems(&root, &ordinary);
    if !unclassified.is_empty() {
        eprintln!(
            "xtask check-workflows: FAILED - {} job(s) or context(s) are not accounted for\n",
            unclassified.len()
        );
        for problem in &unclassified {
            eprintln!("  {problem}");
        }
        eprintln!();
        eprintln!("A job nobody requires gates nothing, and a required context nothing reports is a");
        eprintln!("permanently pending merge. Which of the two a job is belongs in the record, not in");
        eprintln!("a reader's assumption - see the header of devco/required-contexts for what that");
        eprintln!("record can and cannot hold.");
        return Verdict::Fail;
    }

    // WHAT A BADGE CLAIMS, AGAINST WHAT HOLDS IT. Beside the classification above because both
    // are rules about what a workflow is ALLOWED to assert rather than about whether it resolves -
    // and a badge is the one thing in this tree that asserts a control to somebody who cannot read
    // the tree. See `scorecard`'s header for each rule and the limit beside it.
    let mut badges = scorecard::problems(&root);
    badges.extend(publication::problems(&root));
    if !badges.is_empty() {
        eprintln!(
            "xtask check-workflows: FAILED - {} badge/publication rule(s) broken\n",
            badges.len()
        );
        for problem in &badges {
            eprintln!("  {problem}");
        }
        eprintln!();
        eprintln!("A badge is a public claim, and an overstated control is itself the defect here.");
        eprintln!("docs/adr/0024 is the decision; devco/scorecard-publication is what publishing");
        eprintln!("sends and why it is on.");
        return Verdict::Fail;
    }

    // AN ACCEPTED ZERO IS A CLAIM TOO, and it is the same class of defect one row over: a badge
    // asserts a control to somebody who cannot read the tree, and so does a published score whose
    // low row this repository has argued is held by other means. `docs/adr/0025` makes that
    // argument about clippy and zizmor; this reads whether they are still there.
    let stand_ins = sast::problems(&root, &flake, &references);
    if !stand_ins.is_empty() {
        eprintln!(
            "xtask check-workflows: FAILED - {} SAST stand-in rule(s) broken\n",
            stand_ins.len()
        );
        for problem in &stand_ins {
            eprintln!("  {problem}");
        }
        eprintln!();
        eprintln!("docs/adr/0025 accepts Scorecard's SAST zero because clippy under -D warnings and");
        eprintln!("zizmor run inside the one required context. A record that outlives its stand-in is");
        eprintln!("an overstated control, which AGENTS.md calls the defect itself.");
        return Verdict::Fail;
    }

    // WHO MAY WRITE THE ACTIONS CACHE. Third rule in a row about what a workflow is ALLOWED to do
    // rather than whether it resolves, and the newest: an entry a pull request writes is readable
    // by exactly one pull request, so ordinary CI restores on every event and saves only from a
    // push to `main`. Nothing held that before - `check-workflows` read flake references and
    // `zizmor` reads security shapes, and neither can tell a cache action that saves from one that
    // does not.
    let writes = cache_scope::problems(ordinary.closure());
    if !writes.is_empty() {
        eprintln!(
            "xtask check-workflows: FAILED - {} Actions-cache write rule(s) broken\n",
            writes.len()
        );
        for problem in &writes {
            eprintln!("  {problem}");
        }
        eprintln!();
        eprintln!("A run restores caches from its own ref or the default branch, so an entry a pull");
        eprintln!("request writes is readable by exactly one pull request and is then pruned. Restore");
        eprintln!("everywhere, save only from a push to main - see .github/actions/nix-store-cache.");
        return Verdict::Fail;
    }

    let missing: Vec<&Reference> = references
        .iter()
        .filter(|r| {
            let known = match r.kind {
                Kind::Runnable => &runnable,
                Kind::Check => &checks,
            };
            !known.contains(&r.name)
        })
        .collect();

    if missing.is_empty() {
        // TWO NUMBERS, because they are two scans. `files` is the REFERENCE scan's - every file
        // under the three places CI invokes from - and nothing in it distinguished a release
        // refusal that walked four files from one that walked one. So the walked set is printed
        // too, which is the property `the_committed_tree_reaches_past_ci_yml` asserts.
        println!(
            "xtask check-workflows: ok - {} reference(s) in {files} workflow(s), action(s) and script(s), all declared, every gating job classified, every badge held by what it claims and its publication shaped as the API will accept, the Actions cache written only from a push to main, no release output in the {} file(s) ordinary CI runs: {}",
            references.len(),
            walked.len(),
            walked.join(", ")
        );
        return Verdict::Pass;
    }

    eprintln!("xtask check-workflows: these name flake outputs that do not exist\n");
    for r in &missing {
        eprintln!("  {}:{}  {}.{}", r.workflow, r.line, r.kind.label(), r.name);
    }
    eprintln!();
    eprintln!("Runnable:  {}", joined(&runnable));
    eprintln!("Checks:    {}", joined(&checks));
    eprintln!();
    eprintln!("A deleted output is a workflow that fails minutes into a run, after the push.");
    Verdict::Fail
}

fn joined(names: &BTreeSet<String>) -> String {
    names.iter().cloned().collect::<Vec<_>>().join(", ")
}

/// Every `apps.<name>` declaration. Read off the CODE half of the file, so a comment or a
/// string naming an output in prose is not a declaration.
fn declared_apps(text: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for line in code_lines(text) {
        if let Some(rest) = line.code.trim_start().strip_prefix("apps.")
            && let Some(name) = rest.split([' ', '=', '.']).next()
            && !name.is_empty()
        {
            names.insert(String::from(name));
        }
    }
    names
}

/// One line of a Nix file with everything that is not code blanked out, and the `let` depth it
/// starts at.
///
/// `pub(crate)` rather than private, for [`declared_block`]'s reason one layer down:
/// `crate::devenv_shell` asks a different question of the same projection - which attributes does
/// a devenv module ASSIGN, and did the value go through a wrapper - and a second Nix reader would
/// be a second thing to keep in step with the three shapes [`code_lines`] records. **The blanking
/// is what that gate keys on**: a value whose code projection is empty was a string literal, which
/// is how it tells a wrapped body from a bare one without a second parse.
pub(crate) struct CodeLine {
    /// The line, with every character inside a comment or a string literal replaced by a space.
    /// Interpolations are kept, because `${...}` is code and its braces balance.
    pub(crate) code: String,
    /// How many `let`s are open at the START of this line. A binding inside `let ... in` is not
    /// an attribute of the enclosing set, and at brace depth alone the two are indistinguishable.
    pub(crate) lets: u32,
    /// Does this line START inside a `''...''` literal?
    ///
    /// The blanking above is what makes a literal invisible, and for one caller that is the wrong
    /// answer: `crate::devenv_shell` has to find a MULTI-LINE indented literal - a shell body's
    /// shape - wherever it appears, including as an argument to something that is not a wrapper.
    /// `pkgs.writeShellScriptBin "x" ''...''` projects to an application, not to a literal, so the
    /// code alone cannot see the body in it. True on the body lines only: a literal that opens and
    /// closes on one line sets this nowhere, which is exactly the one-liner/block discriminator.
    pub(crate) in_indented: bool,
}

/// Which construct the scanner is inside.
enum Frame {
    /// Nix code. `braces` counts `{` this frame has open; `interpolation` marks a frame opened by
    /// `${`, whose own unmatched `}` ends it; `lets` counts open `let`s.
    Code { braces: u32, lets: u32, interpolation: bool },
    /// `"..."`, where `\` escapes and `${` opens code.
    Quoted,
    /// `''...''`, where `''$`, `'''` and `''\` escape and `${` opens code.
    Indented,
    /// `#` to end of line.
    Line,
    /// `/* ... */`.
    Block,
}

/// Split a Nix file into its code half, line by line.
///
/// **A BRACE-COUNTING SCAN OVER RAW TEXT CANNOT READ NIX, and it failed by INVENTING names as
/// well as by losing them - the worse of the two failure modes for a gate whose whole job is to
/// say which outputs exist.** Three shapes broke it, all of them in the tree at once:
///
/// - **A brace inside a `#` comment.** Comments were skipped when reading a NAME and counted when
///   counting DEPTH, so one sentence quoting `` `checks = {` `` in prose shifted every line after
///   it one level deeper. Six real checks became invisible - `hygiene`, `fmt`, `doctest`, `crap`,
///   `api-docs` and `reuse`, each of them referenced by a workflow - and `check-workflows` failed
///   on the references rather than on the parse.
/// - **A brace inside a string.** `checks.keycloak-tier`'s body is an inline shell script in a
///   `''...''` literal, so shell text sat at the block's own depth: `tree=`, `endpoints=`, `port=`
///   and `realm=` were reported as declared checks. Any shell brace - a `case`, an `awk` program -
///   would have shifted the depth on top of that.
/// - **A `let` binding inside a check's value.** `let` opens no brace, so `cells`, `checkOne`,
///   `quoted`, `required`, `forbidden`, `wantOne` and `banOne` all read as declared outputs on
///   `main` for as long as the release checks were written inline. Harmless only because no
///   workflow happens to name them.
///
/// So this is a small lexer instead: comments, both string forms with their escapes, `${...}`
/// interpolation as nested code, and `let ... in` as a scope. Nothing else about Nix is modelled,
/// and nothing else is needed to answer "which attributes does this block declare".
pub(crate) fn code_lines(text: &str) -> Vec<CodeLine> {
    let chars: Vec<char> = text.chars().collect();
    let at = |index: usize| chars.get(index).copied().unwrap_or('\0');
    let word = |c: char| c.is_alphanumeric() || matches!(c, '_' | '-' | '\'');
    let mut stack = vec![Frame::Code {
        braces: 0,
        lets: 0,
        interpolation: false,
    }];
    let mut lines = Vec::new();
    let mut code = String::new();
    // Only a line that STARTS outside every `let` can declare an attribute. Inside a string
    // literal the answer is "not a declaration", which is what a non-zero depth says.
    let mut lets = 0_u32;
    // Whether the line being built starts inside a `''...''` literal. See `CodeLine::in_indented`.
    let mut in_indented = false;
    let mut index = 0_usize;
    while index < chars.len() {
        let current = at(index);
        if current == '\n' {
            lines.push(CodeLine {
                code: std::mem::take(&mut code),
                lets,
                in_indented,
            });
            if matches!(stack.last(), Some(Frame::Line)) {
                stack.pop();
            }
            lets = match stack.last() {
                Some(&Frame::Code { lets: open, .. }) => open,
                _ => 1,
            };
            // The frame the NEXT line starts in. A `${...}` inside the literal pushes a code
            // frame, so the test is whether an `Indented` frame is open anywhere below the top -
            // otherwise an interpolation spanning a newline would read as ordinary code.
            in_indented = stack.iter().any(|frame| matches!(frame, Frame::Indented));
            index = index.saturating_add(1);
            continue;
        }
        let next = at(index.saturating_add(1));
        let after = at(index.saturating_add(2));
        let previous = index.checked_sub(1).map_or(' ', &at);
        let keyword = |word_chars: &[char]| {
            !word(previous)
                && chars.get(index..).is_some_and(|rest| rest.starts_with(word_chars))
                && !word(at(index.saturating_add(word_chars.len())))
        };
        match stack.last_mut() {
            Some(Frame::Line) => {
                code.push(' ');
                index = index.saturating_add(1);
            }
            Some(Frame::Block) => {
                if current == '*' && next == '/' {
                    stack.pop();
                    code.push_str("  ");
                    index = index.saturating_add(2);
                } else {
                    code.push(' ');
                    index = index.saturating_add(1);
                }
            }
            Some(Frame::Quoted) => {
                if current == '\\' {
                    code.push_str("  ");
                    index = index.saturating_add(2);
                } else if current == '"' {
                    stack.pop();
                    code.push(' ');
                    index = index.saturating_add(1);
                } else if current == '$' && next == '{' {
                    stack.push(Frame::Code {
                        braces: 0,
                        lets: 0,
                        interpolation: true,
                    });
                    code.push_str("${");
                    index = index.saturating_add(2);
                } else {
                    code.push(' ');
                    index = index.saturating_add(1);
                }
            }
            Some(Frame::Indented) => {
                if current == '\'' && next == '\'' {
                    if matches!(after, '$' | '\'' | '\\') {
                        code.push_str("   ");
                        index = index.saturating_add(3);
                    } else {
                        stack.pop();
                        code.push_str("  ");
                        index = index.saturating_add(2);
                    }
                } else if current == '$' && next == '{' {
                    stack.push(Frame::Code {
                        braces: 0,
                        lets: 0,
                        interpolation: true,
                    });
                    code.push_str("${");
                    index = index.saturating_add(2);
                } else {
                    code.push(' ');
                    index = index.saturating_add(1);
                }
            }
            Some(Frame::Code {
                braces,
                lets: open,
                interpolation,
            }) => {
                if current == '#' {
                    stack.push(Frame::Line);
                    code.push(' ');
                    index = index.saturating_add(1);
                } else if current == '/' && next == '*' {
                    stack.push(Frame::Block);
                    code.push_str("  ");
                    index = index.saturating_add(2);
                } else if current == '\'' && next == '\'' {
                    stack.push(Frame::Indented);
                    code.push_str("  ");
                    index = index.saturating_add(2);
                } else if current == '"' {
                    stack.push(Frame::Quoted);
                    code.push(' ');
                    index = index.saturating_add(1);
                } else if current == '{' {
                    *braces = braces.saturating_add(1);
                    code.push('{');
                    index = index.saturating_add(1);
                } else if current == '}' {
                    // An unmatched `}` in an interpolation frame is the `}` of its own `${`.
                    // Both are emitted, so the block scan sees a balanced pair either way.
                    if *braces > 0 {
                        *braces = braces.saturating_sub(1);
                    } else if *interpolation {
                        stack.pop();
                    }
                    code.push('}');
                    index = index.saturating_add(1);
                } else if keyword(&['l', 'e', 't']) {
                    *open = open.saturating_add(1);
                    code.push_str("let");
                    index = index.saturating_add(3);
                } else if keyword(&['i', 'n']) {
                    *open = open.saturating_sub(1);
                    code.push_str("in");
                    index = index.saturating_add(2);
                } else {
                    code.push(current);
                    index = index.saturating_add(1);
                }
            }
            None => break,
        }
    }
    lines.push(CodeLine { code, lets, in_indented });
    lines
}

/// The code half of a Nix file, one `String` per line, comments and string interiors blanked.
///
/// `pub(crate)` for the reason [`block_attributes`] gives one screen down: `crate::warm_start`
/// asks a different question of the same files - which of them bind `cargoArtifacts`, and which
/// bind `preBuild` - and a second Nix reader for it would be a second reader to get wrong, three
/// times over, since this one's own doc comment lists the three shapes that fooled the brace
/// count it replaced. The `lets` depth stays private: it answers *is this line an attribute of
/// the enclosing set*, which nothing outside this module asks.
pub(crate) fn nix_code_lines(text: &str) -> Vec<String> {
    code_lines(text).into_iter().map(|line| line.code).collect()
}

/// What one scan of `.github` found: the references, and how many files were read.
///
/// A named struct rather than a tuple, because `clippy::type_complexity` refuses the tuple - and it
/// is right to: `usize` beside a `Vec` says nothing about which count it is.
struct Scan {
    references: Vec<Reference>,
    files: usize,
}

/// Every `nix run .#` / `nix build .#` reference CI can reach, and how many files were read.
///
/// A function rather than the body of `run`, so a test can assert WHERE the references came from.
/// The composite-action and shell halves are only observable that way: a gate that walked one
/// directory and a gate that walks three return the same verdict on a correct tree, which is
/// exactly how both of those holes went unnoticed.
///
/// **The walk itself is [`sources`]' and not this gate's**, because `check-venues` now asks the
/// same question of the same files - *does CI invoke this* - and two walks would be two answers.
/// That module's header carries which three places, and why a missing one is a failure in one case
/// and legitimate in the other two.
fn gather(root: &std::path::Path) -> Option<Scan> {
    let read = sources::ci_sources(root)?;
    let mut references = Vec::new();
    for source in &read {
        collect(&source.text, &source.label, &mut references);
    }
    Some(Scan {
        references,
        files: read.len(),
    })
}

/// Find every `nix run .#...` and `nix build .#...` in one workflow.
fn collect(text: &str, workflow: &str, out: &mut Vec<Reference>) {
    for (index, line) in text.lines().enumerate() {
        if line.trim_start().starts_with('#') {
            continue;
        }
        for (needle, kind) in [("nix run .#", Kind::Runnable), ("nix build .#", Kind::Check)] {
            let mut rest = line;
            while let Some(at) = rest.find(needle) {
                let after = rest.get(at.saturating_add(needle.len())..).unwrap_or_default();
                let token: String = after
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '.' || *c == '-' || *c == '_')
                    .collect();
                if let Some(name) = attribute(&token, kind) {
                    out.push(Reference {
                        workflow: String::from(workflow),
                        line: index.saturating_add(1),
                        kind,
                        name,
                    });
                }
                rest = after;
            }
        }
    }
}

/// The attribute a token refers to.
///
/// An app is `.#name`. A check is `.#checks.<system>.name`. `nix build .#sutura` names a
/// PACKAGE, which this gate does not track, so it is ignored rather than reported missing.
fn attribute(token: &str, kind: Kind) -> Option<String> {
    let parts: Vec<&str> = token.split('.').collect();
    match kind {
        Kind::Runnable => parts.first().filter(|p| !p.is_empty()).map(|p| String::from(*p)),
        Kind::Check => {
            if parts.first().copied() == Some("checks") {
                parts.get(2).filter(|p| !p.is_empty()).map(|p| String::from(*p))
            } else {
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Kind;

    #[test]
    fn an_app_and_a_check_are_told_apart() {
        let mut found = Vec::new();
        super::collect(
            "        run: nix run .#zizmor -- .github/workflows\n        run: nix build .#checks.x86_64-linux.hygiene -L\n",
            "ci.yml",
            &mut found,
        );
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].name, "zizmor");
        assert_eq!(found[0].kind, Kind::Runnable);
        assert_eq!(found[1].name, "hygiene");
        assert_eq!(found[1].kind, Kind::Check);
    }

    #[test]
    fn a_package_build_is_not_a_check() {
        // Reading `.#sutura` as a check would report every release build as missing.
        let mut found = Vec::new();
        super::collect("          nix build .#sutura -L\n", "release.yml", &mut found);
        assert!(found.is_empty());
    }

    #[test]
    fn the_shared_nix_shell_scripts_are_scanned_too() {
        // RED against the previous behaviour for the same reason as the actions test: a verdict
        // cannot tell the two scans apart on a correct tree. This one is not hypothetical - moving
        // `ci.yml`'s workflow-analysis body into `nix/lint-workflows.sh` dropped this gate's count
        // from 60 references to 55 and nothing failed.
        let Some(root) = crate::repo::root() else {
            return;
        };
        let Some(scan) = super::gather(&root) else {
            panic!("the scan could not read .github/workflows");
        };
        let from_nix: Vec<&str> = scan
            .references
            .iter()
            .map(|r| r.workflow.as_str())
            .filter(|w| w.starts_with("nix/"))
            .collect();
        assert!(
            !from_nix.is_empty(),
            "no flake reference was collected from nix/*.sh, so a step moved out of a workflow has \
             left this gate's sight"
        );
        assert!(
            scan.references
                .iter()
                .any(|r| r.workflow == "nix/run-gate.sh" && r.name == "deny"),
            "run-gate.sh's `nix run .#deny` was not seen: collected {from_nix:?}"
        );
    }

    #[test]
    fn the_local_composite_actions_are_scanned_too() {
        // RED against the previous behaviour, which read `.github/workflows` alone: this asserts
        // where a reference came FROM, because a verdict cannot tell the two scans apart on a
        // correct tree. `attest-and-sign` reaches `cosign` and is the reference that publishes a
        // release, so it is the one worth naming rather than a synthetic fixture.
        let Some(root) = crate::repo::root() else {
            return;
        };
        let Some(scan) = super::gather(&root) else {
            panic!("the scan could not read .github/workflows");
        };
        let references = scan.references;
        let from_actions: Vec<&str> = references
            .iter()
            .map(|r| r.workflow.as_str())
            .filter(|w| w.starts_with("actions/"))
            .collect();
        assert!(
            !from_actions.is_empty(),
            "no flake reference was collected from .github/actions, so a step split out of a \
             workflow has left this gate's sight"
        );
        assert!(
            references
                .iter()
                .any(|r| r.workflow == "actions/attest-and-sign" && r.name == "cosign"),
            "attest-and-sign's `nix run .#cosign` was not seen: collected {from_actions:?}"
        );
    }

    #[test]
    fn a_comment_is_not_a_reference() {
        let mut found = Vec::new();
        super::collect("      # was: nix run .#mkdocs -- build\n", "docs.yml", &mut found);
        assert!(found.is_empty());
    }

    #[test]
    fn apps_come_from_declarations_not_prose() {
        let flake = concat!(
            "        # CI used to reach these through apps.mkdocs, in prose.\n",
            "        apps.zizmor = {\n",
            "        apps.pixi = {\n",
        );
        let apps = super::declared_apps(flake);
        assert_eq!(apps.len(), 2, "a comment must not contribute a name");
        assert!(apps.contains("zizmor"));
    }

    #[test]
    fn a_deleted_app_is_what_this_catches() {
        // The regression that motivated the gate, as a unit test: docs.yml called an app that
        // flake.nix no longer declares.
        let flake = "        apps.pixi = {\n";
        let apps = super::declared_apps(flake);
        let mut found = Vec::new();
        super::collect("        run: nix run .#mkdocs -- build --strict\n", "docs.yml", &mut found);
        assert_eq!(found.len(), 1);
        assert!(!apps.contains(&found[0].name), "mkdocs must read as missing");
    }
}
