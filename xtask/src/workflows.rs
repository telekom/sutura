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
//! Literal package builds and the two release-profile assertions are also refused in `ci.yml`.
//! Pull requests use interpolated `-ci` packages for their link matrix; the tag-triggered release
//! workflow owns everything that is published.

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

    let ci = match std::fs::read_to_string(root.join(".github/workflows/ci.yml")) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("xtask check-workflows: could not read ci.yml: {error}");
            return Verdict::Fail;
        }
    };
    let release_builds = literal_release_builds(&ci);
    if !release_builds.is_empty() {
        eprintln!("xtask check-workflows: ci.yml builds release outputs");
        for (line, output) in release_builds {
            eprintln!("  ci.yml:{line}  {output}");
        }
        eprintln!("Release outputs belong to the tag-triggered release workflow, not ordinary CI.");
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
    let unclassified = contexts::problems(&root);
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
        println!(
            "xtask check-workflows: ok - {} reference(s) in {files} workflow(s), action(s) and script(s), all declared, every gating job classified",
            references.len()
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

/// The one literal `nix build` prefix ordinary CI may name, and why it is safe to name.
///
/// A `feature-probes-<triple>` output is a `writeText` listing which feature-on link probes exist
/// for that triple - `nix/shipped.nix`'s `probeManifests`. It installs no `bin/`, so it cannot be
/// a published asset, and the `cross` jobs read it to learn which probes to build. It has to be
/// LITERAL for the same reason it exists: the step used to reconstruct that set from a naming
/// pattern, which went silently empty when the pattern changed, so a fixed name is what makes a
/// missing manifest a failed `nix build` rather than a green run over nothing.
const PROBE_MANIFEST: &str = "feature-probes-";

fn literal_release_builds(text: &str) -> Vec<(usize, String)> {
    let mut found = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if line.trim_start().starts_with('#') {
            continue;
        }
        let Some((_, after)) = line.split_once("nix build ") else {
            continue;
        };
        let after = after.trim_start().trim_start_matches('"');
        let Some(after) = after.strip_prefix(".#") else {
            continue;
        };
        let output: String = after
            .chars()
            .take_while(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.'))
            .collect();
        if output.is_empty() || output.starts_with(PROBE_MANIFEST) {
            continue;
        }
        // Anything not a `checks.` output is a release PACKAGE; a `checks.` one is ordinary
        // CI's to build, except the two the release path owns.
        let release_check = output.ends_with(".one-binary") || output.ends_with(".shipped-features");
        if !output.starts_with("checks.") || release_check {
            found.push((index.saturating_add(1), output));
        }
    }
    found
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
    let mut index = 0_usize;
    while index < chars.len() {
        let current = at(index);
        if current == '\n' {
            lines.push(CodeLine {
                code: std::mem::take(&mut code),
                lets,
            });
            if matches!(stack.last(), Some(Frame::Line)) {
                stack.pop();
            }
            lets = match stack.last() {
                Some(&Frame::Code { lets: open, .. }) => open,
                _ => 1,
            };
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
    lines.push(CodeLine { code, lets });
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

/// Every attribute at the top level of an output block.
///
/// Depth-tracked rather than stopping at the first `};`. The first version broke out there and
/// so missed everything after the first NESTED close - which meant `checks.hygiene`, declared
/// well below `clippy`, read as undeclared while CI built it happily every run. A parser that
/// silently sees half a file is worse than no parser.
///
/// THE BLOCK'S OWN OPENING BRACE IS COUNTED rather than assumed to be on the header line, and
/// that is a second version of the same bug. `depth` used to be set to 1 the moment the header
/// matched, which is right only while the `{` is on that line: written as
///
/// ```text
/// packages = crossPackages // ociImages
///   // nativeImages // {
/// ```
///
/// the brace on the continuation line read as a NESTED attrset, so depth became 2 and every name
/// in the block was invisible - `packages.xtask` among them, which `ci.yml` runs three times.
/// Measured, on the change that split that line. Counting the header's braces like any other
/// line's makes both shapes the same case, and `opened` is what keeps the `depth <= 0` break from
/// firing before the block has started.
/// `pub(crate)` rather than private: `crate::compose::file` asks the same question of the same
/// block - which checks does `flake.nix` declare - and a second parser for it would be a second
/// thing to keep in step with the shapes this doc comment records.
///
/// `None` where the header matched and the block never closed. That case USED TO BE SILENT, and
/// silence is what made the comment-brace defect expensive: the scan ran off the end of the block
/// into the rest of `outputs`, so it reported `formatter` as a check, lost six real ones, and the
/// failure surfaced as eighteen workflow references that "do not exist". A gate whose parse has
/// desynchronised must say the parse is broken - never answer the question with a guess.
pub(crate) fn declared_block(text: &str, header: &str) -> Option<BTreeSet<String>> {
    scan_block(text, header).map(|(names, _)| names)
}

/// The RAW source of one output block, header line to closing line.
///
/// Raw and not the code projection, because the question its caller asks - does this block name
/// `sutura-<service>-tier` - is about a store path inside a string literal, which the projection
/// blanks out. `#[cfg(test)]` because `crate::compose::file`, the gate that asks, is a unit test:
/// a field nothing reads in the binary is dead code the compiler is right to refuse.
#[cfg(test)]
pub(crate) fn block_source(text: &str, header: &str) -> Option<String> {
    scan_block(text, header).map(|(_, source)| source)
}

/// One parsed output block: the attributes it declares, and its raw source.
///
/// An alias and not a struct, and that is the compiler choosing between two lints rather than a
/// style preference. `clippy::type_complexity` refuses the tuple written out; a struct puts the
/// source in a named field, whose only reader is `crate::compose::file` - a `#[cfg(test)] mod` -
/// so `dead_code` refuses that in the binary. The alias satisfies both without an `allow`.
type Block = (BTreeSet<String>, String);

/// One scan, shared by both faces above, because a second one would be a second thing to keep in
/// step with the shapes recorded here.
fn scan_block(text: &str, header: &str) -> Option<Block> {
    let mut names = BTreeSet::new();
    let mut depth = 0_i32;
    let mut inside = false;
    let mut opened = false;
    let mut block_closed = false;
    let mut source = String::new();
    let raw: Vec<&str> = text.lines().collect();
    for (number, line) in code_lines(text).into_iter().enumerate() {
        let trimmed = line.code.trim();
        let header_line = !inside && trimmed.starts_with(header);
        if header_line {
            inside = true;
        } else if !inside {
            continue;
        }
        if let Some(original) = raw.get(number) {
            source.push_str(original);
            source.push('\n');
        }

        // Only the outermost level of the block declares an output; everything deeper belongs
        // to one. Counted after the name check so the closing line of a nested attrset does
        // not look like a declaration, and never on the header line, which declares the block
        // rather than a member of it.
        let opens = i32::try_from(trimmed.matches('{').count()).unwrap_or(0);
        let closes = i32::try_from(trimmed.matches('}').count()).unwrap_or(0);

        if !header_line
            && opened
            && depth == 1
            && line.lets == 0
            && let Some((key, _)) = trimmed.split_once('=')
        {
            let key = key.trim();
            let plain = !key.is_empty()
                && !key.contains(' ')
                && !key.contains('.')
                && key.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_');
            if plain {
                names.insert(String::from(key));
            }
        }

        depth = depth.saturating_add(opens).saturating_sub(closes);
        if depth > 0 {
            opened = true;
        }
        if opened && depth <= 0 {
            block_closed = true;
            break;
        }
    }
    block_closed.then_some((names, source))
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
    fn literal_release_outputs_are_kept_out_of_ordinary_ci() {
        let found = super::literal_release_builds(concat!(
            "          nix build .#checks.x86_64-linux.hygiene -L\n",
            "          nix build .#checks.x86_64-linux.one-binary -L\n",
            "          nix build .#sutura-serve -L\n",
            "          nix build \".#oci\" -L\n",
            "          nix build \".#${bin}-${TARGET}-ci\" -L\n",
        ));
        assert_eq!(
            found,
            vec![
                (2, String::from("checks.x86_64-linux.one-binary")),
                (3, String::from("sutura-serve")),
                (4, String::from("oci")),
            ]
        );
    }

    #[test]
    fn the_probe_manifest_is_the_one_literal_ordinary_ci_may_build() {
        // The `cross` jobs must name it literally - that is what makes a missing manifest a failed
        // build instead of a green run over an empty set - and it installs no `bin/`, so it cannot
        // become a published asset. Everything else keeps failing, including a literal that merely
        // starts the same way.
        let found = super::literal_release_builds(concat!(
            "          nix build \".#feature-probes-${TARGET}\" --no-link\n",
            "          nix build .#feature-probes-x86_64-unknown-linux-musl\n",
            "          nix build .#feature-probesque -L\n",
        ));
        assert_eq!(found, vec![(3, String::from("feature-probesque"))]);
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

    #[test]
    fn a_block_whose_opening_brace_is_on_a_continuation_line_still_declares_its_members() {
        // The `packages = ` line in flake.nix grew past one line when a second shipped binary
        // was added, and the brace moved with it. Depth was pinned to 1 at the header, so the
        // brace on the second line read as a NESTED attrset and every member of the block became
        // invisible - including `xtask`, which `ci.yml` runs three times. This is that shape.
        let flake = concat!(
            "        packages = crossPackages // ociImages\n",
            "          // nativeImages // {\n",
            "          default = sutura;\n",
            "          xtask = craneLib.buildPackage (ciArgs // {\n",
            "            pname = \"xtask\";\n",
            "          });\n",
            "        };\n",
        );
        let names = super::declared_block(flake, "packages = ").expect("the block closes");
        assert!(names.contains("xtask"), "xtask must be declared, got {names:?}");
        assert!(names.contains("default"), "default must be declared, got {names:?}");
        assert!(!names.contains("pname"), "a nested attribute is not a declaration");
    }

    #[test]
    fn a_block_whose_opening_brace_is_on_the_header_line_is_unchanged() {
        // The shape every other block in flake.nix has, asserted beside the one above so a fix
        // for one cannot quietly become a regression in the other.
        let flake = concat!(
            "        checks = {\n",
            "          clippy = craneLib.cargoClippy (ciArgs // {\n",
            "            cargoArtifacts = ciArtifacts;\n",
            "          });\n",
            "          hygiene = pkgs.runCommand \"h\" { } \"\";\n",
            "        };\n",
        );
        let names = super::declared_block(flake, "checks = {").expect("the block closes");
        assert!(names.contains("clippy"), "got {names:?}");
        assert!(names.contains("hygiene"), "declared below a nested close, got {names:?}");
        assert!(!names.contains("cargoArtifacts"), "a nested attribute is not a declaration");
    }

    #[test]
    fn a_brace_inside_a_comment_does_not_shift_the_block() {
        // THE DEFECT THAT SHIPPED. A sentence in `flake.nix` quoting the block's own header in
        // prose - `checks = {` inside a `#` comment - was skipped when reading a name and counted
        // when counting depth, so everything below it sat one level too deep. Six checks that CI
        // builds every run became undeclared, and the gate reported eighteen workflow references
        // as pointing at outputs that do not exist.
        let flake = concat!(
            "        checks = {\n",
            "          clippy = craneLib.cargoClippy { };\n",
            "          # `checks = {` is the block two xtask gates read, so it stays here.\n",
            "          hygiene = pkgs.runCommand \"h\" { } \"\";\n",
            "        };\n",
            "        formatter = pkgs.nixfmt;\n",
        );
        let names = super::declared_block(flake, "checks = {").expect("the block closes");
        assert!(names.contains("hygiene"), "a comment's brace must not hide it, got {names:?}");
        assert!(!names.contains("formatter"), "the scan ran past the block, got {names:?}");
        assert_eq!(names.len(), 2, "got {names:?}");
    }

    #[test]
    fn shell_inside_an_indented_string_declares_nothing() {
        // `checks.keycloak-tier`'s body is an inline shell script. At raw-text depth its
        // assignments sat at the block's own level, so `port` and `realm` were reported as
        // declared checks - a gate INVENTING outputs, which is worse than losing them because a
        // caller cannot tell the difference. The `${...}` is code and its braces still balance.
        let flake = concat!(
            "        checks = {\n",
            "          keycloak-tier = pkgs.runCommand \"k\" { } ''\n",
            "            port=\"$(jq -r '.port' \"$f\")\"\n",
            "            realm=.sutura-dev/keycloak-realm.json\n",
            "            case \"$x\" in *a*) echo ${tier.realm} ;; esac\n",
            // A brace shell leaves unbalanced, which is the half a `matches('{').count()` cannot
            // survive at all: one of these shifts every line below it.
            "            sed -n 's|.*}||p' \"$log\"\n",
            "          '';\n",
            "          fmt = craneLib.cargoFmt { };\n",
            "        };\n",
        );
        let names = super::declared_block(flake, "checks = {").expect("the block closes");
        assert_eq!(names.len(), 2, "shell text is not a declaration, got {names:?}");
        assert!(names.contains("keycloak-tier"), "got {names:?}");
        assert!(
            names.contains("fmt"),
            "an unbalanced shell brace must not hide it, got {names:?}"
        );
    }

    #[test]
    fn a_let_binding_inside_a_check_is_not_a_check() {
        // `let` opens no brace, so a binding in a check's own value is at the same brace depth as
        // the check. Seven of them read as declared outputs on `main` for as long as the release
        // checks were written inline in `flake.nix`.
        let flake = concat!(
            "        checks = {\n",
            "          one-binary =\n",
            "            let\n",
            "              cells = map f binaries;\n",
            "              checkOne = p: \"x\";\n",
            "            in\n",
            "            pkgs.runCommand \"one\" { } \"\";\n",
            "          reuse = pkgs.runCommand \"r\" { } \"\";\n",
            "        };\n",
        );
        let names = super::declared_block(flake, "checks = {").expect("the block closes");
        assert_eq!(names.len(), 2, "a let binding is not an output, got {names:?}");
        assert!(names.contains("one-binary"), "got {names:?}");
        assert!(names.contains("reuse"), "got {names:?}");
    }

    #[test]
    fn a_block_that_never_closes_is_an_error_and_not_an_answer() {
        // The direction this gate has to fail in. Answering with the names it happened to collect
        // is how a desynchronised parse became eighteen confusing reference failures instead of
        // one clear "the scan is broken".
        let flake = concat!(
            "        checks = {\n",
            "          clippy = craneLib.cargoClippy { };\n",
            "          hygiene = pkgs.runCommand \"h\" { } \"\";\n",
        );
        assert!(
            super::declared_block(flake, "checks = {").is_none(),
            "an unclosed block must not answer the question"
        );
    }

    #[test]
    fn an_interpolation_holding_an_attrset_keeps_the_scan_aligned() {
        // `${pkgs.closureInfo { rootPaths = [ drv ]; }}` - a `${` whose code contains its own
        // braces. Popping the interpolation on the FIRST `}` swallows the second, and the block
        // then never closes.
        let flake = concat!(
            "        checks = {\n",
            "          one-binary = pkgs.runCommand \"o\" { } ''\n",
            "            grep -q x ${pkgs.closureInfo { rootPaths = [ drv ]; }}/store-paths\n",
            "          '';\n",
            "          reuse = pkgs.runCommand \"r\" { } \"\";\n",
            "        };\n",
        );
        let names = super::declared_block(flake, "checks = {").expect("the block closes");
        assert_eq!(names.len(), 2, "got {names:?}");
        assert!(names.contains("reuse"), "the scan lost alignment, got {names:?}");
    }

    #[test]
    fn the_real_flake_declares_the_checks_ci_builds() {
        // The unit fixtures above are shapes; this is the file. Anchored on names `ci.yml` and
        // `justfile` both build, so a parse that regresses on the real tree fails here rather
        // than in a nix step minutes into a run.
        let Some(root) = crate::repo::root() else { return };
        let Ok(flake) = std::fs::read_to_string(root.join("flake.nix")) else {
            return;
        };
        let names = super::declared_block(&flake, "checks = {").expect("flake.nix's `checks = {` block must close");
        for required in ["clippy", "nextest", "hygiene", "fmt", "doctest", "crap", "api-docs"] {
            assert!(names.contains(required), "`checks.{required}` is not declared, got {names:?}");
        }
        assert!(
            !names.contains("formatter"),
            "`formatter` is a sibling of `checks`, so the scan ran past the block: {names:?}"
        );
    }
}
