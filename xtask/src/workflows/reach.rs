//! What ordinary CI reaches, and what it may not build once it gets there.
//!
//! **The finding this exists for.** The release-output refusal read exactly one file name -
//! `.github/workflows/ci.yml` - while the set ordinary CI runs is plural in two directions. A
//! reusable workflow `ci.yml` calls runs on the same pull request, and the unexemptable 1000-line
//! cap is what put a job there: `cross-link.yml` was lifted out of `ci.yml` at 999 lines. And
//! `docs.yml` and `security-audit.yml` run on a pull request without being called by anything, so
//! naming `ci.yml` missed them too - reported in review, after the walk had already removed the
//! name from the CALL side and left it on the ROOT side. The root set is derived now
//! ([`super::contexts::OrdinaryCi`]), which is why the verdict prints the set it walked: a green
//! line that names no set cannot be told from one over a smaller set.
//!
//! **The same blind spot reached the other half of this gate.** [`super::contexts`] classified a
//! job by reading the workflows whose own `on:` block gates a merge, and a called workflow's `on:`
//! is `workflow_call` - so every job in `cross-link.yml` was classified by nothing while the gate
//! printed *every gating job classified*. A job nobody classified is the omission that section
//! exists to make impossible.
//!
//! **What a `nix build` line is read for** is in [`literal_release_builds`], and it now skips
//! leading flags: `nix build -L .#sutura-serve` used to walk past the refusal, and that it never
//! bit was a property of this tree's text rather than of the rule.
//!
//! One walk, two readers, for [`super::sources`]' reason: a reference leaves a gate's sight
//! whenever a step moves out of a workflow, a hard line cap is what forces steps out, and when it
//! happens again both halves follow together or neither does.
//!
//! # Three arms, and every count in one is read off a value rather than passed into a sentence
//!
//! * **a call it cannot follow.** A `uses:` this walk cannot open is a step this gate cannot
//!   refuse, so a target that is missing, unreadable, or outside this repository fails closed.
//!   The trigger is the unfollowed set itself. It was once *the set it set out to open is larger
//!   than the set it opened*, which could only SUPPRESS: that comparison unions repo-relative
//!   paths with raw `uses:` values, so a target whose text equalled a path already reached
//!   collided in it and the sentence went unprinted at exit 0.
//! * **the conservation law**, which is that comparison as its own rule rather than as the other's
//!   guard: everything discovered was opened. It can now only fire when the two DISAGREE, which is
//!   a reader bug, and it says so instead of quietly gating a real refusal.
//! * **a shape the key reader stopped recognising.** Every line the pass EXAMINES is also put to a
//!   second, deliberately stupid predicate: does it carry both `uses:` and a `./` path? That
//!   predicate knows nothing about where a key sits, so a line it accepts and the key reader made
//!   no edge of is named, with its file and line. A gate in this repository matched a key at the
//!   start of a trimmed line, could not see `- binaries: ...` inside a `strategy.matrix.include`,
//!   and reported `ok - 4 literal(s)` over a drifted set - this is the arm that would have said so.
//!   **What it holds constant is the block-scalar boundary**: both readers run over the same
//!   examined lines, so a shell body naming a call is a floor for neither, and a boundary error is
//!   silent in both. That is the one shape class where a single mistake blinds both, so it is held
//!   by fixtures - and it needs TWO, one per direction. It had one, over the correctly-bounded
//!   direction, and could not fail on the mis-bounded one: the scalar was recorded at the DASH's
//!   column, so every sibling key of a sequence item whose first key opened a scalar was swallowed
//!   with the body. `- if: >` followed by a sibling `uses:` is idiomatic YAML that GitHub runs, and
//!   its edge disappeared at exit 0 with `just lint-workflows` green over it. **A fixture over one
//!   direction of a two-directional boundary is not the assertion a header can claim for it.**
//!
//! # The YAML shapes this reader does handle
//!
//! Comments, blank lines, **sequence items** (`- uses: ...`, whose dash is not part of the key),
//! **block scalars** (`run: |` and every `|`/`>` indicator - their bodies are shell, and a shell
//! line is not a call), quoted values, and the `steps:` region, which is what tells a job-level
//! `uses:` - a reusable-workflow call - from a step's `uses:`, without assuming a fixed indent.
//!
//! # The shapes it does NOT, stated rather than glossed
//!
//! * **flow mappings.** `{ uses: ./.github/workflows/x.yml }` on one line is legal YAML and this
//!   reader sees the key only at the head of a line or a sequence item. The floor above sees such a
//!   line, so the gate fails rather than passing quietly - which is the point of two readers.
//! * **anchors, aliases and merge keys.** A `uses:` reached through `<<: *ref` is invisible to
//!   both readers.
//! * **the shell.** The closure follows resolvable `uses:` edges, not `bash nix/<script>.sh`. Two
//!   reasons, and the second would still hold with a resolver in hand: a shared script under
//!   `nix/` is reached by a `just` task, by a flake check and by the release path as well, so
//!   *ordinary CI* is not its venue and the refusal below has no meaning there. What is not a limit
//!   is a missing flake output named in one: [`super::sources`] reads `nix/*.sh` unconditionally,
//!   so the REFERENCE half of this gate covers the shell already.
//! * **a remote reusable workflow's body.** It cannot be opened here, so it is refused as a call
//!   this walk cannot follow rather than passed over. There are none; the cost of the rule today is
//!   nothing, and the alternative is a green verdict over a file nobody read.

use std::collections::{BTreeSet, VecDeque};
use std::path::Path;

/// Where a reusable workflow lives, relative to the repository root.
const WORKFLOWS: &str = ".github/workflows/";

/// The two spellings of a composite action's manifest, in the order GitHub resolves them.
const MANIFESTS: [&str; 2] = ["action.yml", "action.yaml"];

/// One file inside the closure: what a failure names it by, and its text.
pub(super) struct Reached {
    /// `ci.yml`, `cross-link.yml`, `actions/reclaim-disk` - what a reader can open. Actions are
    /// labelled by their DIRECTORY, because every one of those files is called `action.yml` and a
    /// failure saying `action.yml:118` names nothing.
    label: String,
    /// Repo-relative path. The identity the walk deduplicates by; a label cannot be, for the
    /// reason above.
    path: String,
    text: String,
    /// A workflow declares jobs and a composite action does not, so only a workflow contributes to
    /// the classification half.
    workflow: bool,
    /// Was this a ROOT of the walk rather than something it reached through a call?
    ///
    /// It decides what a job's context STRING is, which is why it is carried rather than derived: a
    /// root's job reports its own `name:` or id, and a called workflow's reports
    /// `<caller job> / <job> (<matrix value>)`. So a called workflow's job must be classified and
    /// must NOT be allowed to satisfy a required context - see `super::contexts`.
    root: bool,
}

impl Reached {
    /// One root of a walk: a workflow this caller has already read.
    ///
    /// The text comes in rather than being read here, so the caller that already holds it does not
    /// read the same file twice - and so an unreadable ROOT stays that caller's failure to report
    /// rather than becoming a second answer to the same question.
    pub(super) fn workflow(file: &str, text: String) -> Self {
        Self {
            label: String::from(file),
            path: format!("{WORKFLOWS}{file}"),
            text,
            workflow: true,
            root: true,
        }
    }

    pub(super) fn label(&self) -> &str {
        &self.label
    }

    pub(super) fn text(&self) -> &str {
        &self.text
    }

    pub(super) const fn is_workflow(&self) -> bool {
        self.workflow
    }

    pub(super) const fn is_root(&self) -> bool {
        self.root
    }
}

/// What one `uses:` value points at.
#[derive(Clone, Copy)]
enum Edge {
    /// `./.github/workflows/<file>` - a reusable workflow in this repository.
    Workflow,
    /// Any other `./<path>` - a composite action directory in this repository.
    Action,
    /// A job-level `uses:` naming something outside this repository.
    Elsewhere,
}

/// One `uses:` edge, and where it was written.
struct Call {
    /// The label of the file the edge is written in.
    from: String,
    line: usize,
    /// The `uses:` value, verbatim, so a failure quotes what the file says.
    target: String,
    edge: Edge,
}

/// One call the walk could not follow, and the reason in a reader's words.
struct Unfollowed {
    call: Call,
    why: String,
}

/// One examined line that plainly names a local call, wherever its key sits.
struct Sighted {
    from: String,
    line: usize,
}

/// What one pass over a file found: its edges, and every line that plainly named a call.
struct Scan {
    calls: Vec<Call>,
    sighted: Vec<Sighted>,
}

/// The transitive closure of local `uses:` calls from a set of root workflows, and every edge that
/// produced it.
///
/// **Private fields and one constructor**, and that is the whole design: every count a refusal
/// quotes is read off the values this holds, so no sentence can claim a file the walk never
/// opened. A count in a message is a claim; a count read off a witness is the witness.
pub(super) struct Closure {
    /// Every file opened, root and called alike, in the order the walk reached them.
    reached: Vec<Reached>,
    /// Every edge followed. One per `uses:` line, so a target called twice counts twice - this is
    /// the edge count the floor is compared against, not a file count.
    followed: Vec<Call>,
    /// Every edge that could not be followed.
    unfollowed: Vec<Unfollowed>,
    /// Every examined line the position-blind predicate accepted, whether or not the key reader
    /// made an edge of it. The difference between the two is the second arm.
    sighted: Vec<Sighted>,
}

impl Closure {
    /// Walk from these already-read workflows, following every local `uses:` edge transitively.
    pub(super) fn from_roots(root: &Path, roots: Vec<Reached>) -> Self {
        let mut reached: Vec<Reached> = Vec::new();
        let mut seen: BTreeSet<String> = BTreeSet::new();
        let mut followed: Vec<Call> = Vec::new();
        let mut unfollowed: Vec<Unfollowed> = Vec::new();
        let mut sighted: Vec<Sighted> = Vec::new();
        let mut queue: VecDeque<Reached> = roots.into();

        while let Some(file) = queue.pop_front() {
            if !seen.insert(file.path.clone()) {
                continue;
            }
            let scan = scan(&file);
            sighted.extend(scan.sighted);
            for call in scan.calls {
                match open(root, &call) {
                    Ok(next) => {
                        followed.push(call);
                        queue.push_back(next);
                    }
                    Err(why) => unfollowed.push(Unfollowed { call, why }),
                }
            }
            reached.push(file);
        }

        Self {
            reached,
            followed,
            unfollowed,
            sighted,
        }
    }

    /// Every file the walk opened.
    pub(super) fn inspected(&self) -> &[Reached] {
        &self.reached
    }

    /// One sentence per way this walk read less than it discovered - empty when it read everything.
    ///
    /// Each sentence names its arm and carries the counts read off `self`.
    pub(super) fn drift(&self) -> Vec<String> {
        let mut out = Vec::new();

        // ARM ONE, and its trigger is the unfollowed set ITSELF. It used to be gated on
        // `discovered > reached`, which can only ever SUPPRESS: `discovered` unions two
        // namespaces - repo-relative paths out of `reached`, raw `uses:` values out of
        // `unfollowed` - so a target whose text equals a path already reached collided in that set
        // and the sentence went unprinted at exit 0. A non-empty `unfollowed` IS the failure this
        // arm names, so it is what reports; the conservation law below is a second rule over a
        // second property, and neither licenses the other.
        for un in &self.unfollowed {
            out.push(format!(
                "UNFOLLOWED CALL: {}:{} names `{}` and this walk could not open it - {}. {} file(s) inspected of {} discovered, and a call it cannot read is a step it cannot refuse",
                un.call.from,
                un.call.line,
                un.call.target,
                un.why,
                self.reached.len(),
                self.discovered().len()
            ));
        }

        // THE CONSERVATION LAW: everything the walk set out to open, it opened. Arm one reports
        // every reason that can fail, so this firing on its own means the two disagree - a target
        // discovered and neither opened nor refused, which is a reader bug rather than a workflow
        // defect, and it says so.
        let discovered = self.discovered();
        if self.unfollowed.is_empty() && discovered.len() > self.reached.len() {
            out.push(format!(
                "UNRECONCILED WALK: {} file(s) inspected of {} discovered, and nothing was refused - a file was set out for and neither opened nor reported, so this reader is wrong rather than the workflows",
                self.reached.len(),
                discovered.len()
            ));
        }

        let unread = self.unread();
        for at in &unread {
            out.push(format!(
                "UNSIGHTED SHAPE: {}:{} plainly names a local call - it carries both `uses:` and a `./` path - and the key reader made no edge of it. {} of {} such line(s) became edges, so a spelling this reader no longer recognises is a call it silently does not follow; see this module's header for the shapes it handles",
                at.from,
                at.line,
                self.sighted.len().saturating_sub(unread.len()),
                self.sighted.len()
            ));
        }

        out
    }

    /// Every line the position-blind predicate accepted that the key reader turned into no edge.
    fn unread(&self) -> Vec<&Sighted> {
        self.sighted
            .iter()
            .filter(|at| {
                let edged = |call: &Call| call.line == at.line && call.from == at.from;
                !self.followed.iter().any(edged) && !self.unfollowed.iter().any(|un| edged(&un.call))
            })
            .collect()
    }

    /// Every distinct file the walk SET OUT to open: the roots, plus every call target, readable
    /// or not.
    fn discovered(&self) -> BTreeSet<&str> {
        let mut all: BTreeSet<&str> = self.reached.iter().map(|file| file.path.as_str()).collect();
        all.extend(self.unfollowed.iter().map(|un| un.call.target.as_str()));
        all
    }
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

/// What the walk opened, by label, for a verdict to print.
///
/// A verdict that names no set cannot be told from one over a smaller set, which is this gate's own
/// finding turned on its own output: `67 reference(s) in 20 workflow(s)` is the REFERENCE scan's
/// number and says nothing about whether the refusal below walked four files or one.
pub(super) fn walked(closure: &Closure) -> Vec<&str> {
    closure.inspected().iter().map(|file| file.label.as_str()).collect()
}

/// Every literal release output ordinary CI builds, and the file and line that builds it.
///
/// Over the whole closure and not over one file name, which is the widening this module exists
/// for: the refusal has to reach wherever a step can move, and a line cap moves steps.
pub(super) fn release_outputs(closure: &Closure) -> Vec<String> {
    let mut out = Vec::new();
    for file in closure.inspected() {
        for (line, output) in literal_release_builds(&file.text) {
            out.push(format!("{}:{line}  {output}", file.label));
        }
    }
    out
}

/// Every literal `nix build .#<output>` in one file that names a release output.
fn literal_release_builds(text: &str) -> Vec<(usize, String)> {
    let mut found = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if line.trim_start().starts_with('#') {
            continue;
        }
        let Some((_, after)) = line.split_once("nix build ") else {
            continue;
        };
        // SKIP LEADING FLAGS. This used to require the installable to be the very next token, so
        // `nix build -L .#sutura-serve` - an ordinary spelling - walked past the refusal at exit 0.
        // That it did not bite was a property of this tree's text (flags written after the
        // installable everywhere) rather than of the rule.
        let after = after
            .split_whitespace()
            .find(|token| !token.starts_with('-'))
            .unwrap_or_default();
        let after = after.trim_start_matches('"');
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

/// Does this examined line plainly name a local call?
///
/// **The position-blind predicate, and it is not the key reader.** It is the floor that reader is
/// measured against, because a key matched at the head of a trimmed line cannot see a sequence
/// item or a flow mapping - the failure that let a gate here report `ok` over a drifted set. It
/// runs only on lines the pass examined, so the block-scalar boundary is held constant between the
/// two rather than being a floor a shell body can raise.
fn plainly_a_call(trimmed: &str) -> bool {
    trimmed.contains("uses:") && trimmed.contains("./")
}

/// Every `uses:` edge one file declares, and every examined line that plainly named a call.
fn scan(file: &Reached) -> Scan {
    let mut out = Vec::new();
    let mut sighted = Vec::new();
    // The column of the KEY that opened a block scalar - not of the dash in front of it, which is
    // the mistake that swallowed a sibling key. Everything more indented is the scalar's body,
    // which is shell rather than YAML, so a `uses:` written inside a `run:` is not a call; a
    // SIBLING key of a sequence item whose first key opened the scalar sits at exactly the key's
    // column, and `>` rather than `<=` is what keeps it examined.
    let mut scalar: Option<usize> = None;
    // The column of a `steps:` key. Everything more indented is a step, which is what tells a
    // job-level `uses:` from a step's without assuming a fixed indent width.
    let mut steps: Option<usize> = None;

    for (index, raw) in file.text.lines().enumerate() {
        let trimmed = raw.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let column = raw.len().saturating_sub(trimmed.len());
        if scalar.is_some_and(|at| column > at) {
            continue;
        }
        scalar = None;
        if steps.is_some_and(|at| column <= at) {
            steps = None;
        }

        // The floor, over the line this pass is about to read. Recorded BEFORE the key reader gets
        // its hands on it, so a spelling that reader does not recognise still counts.
        if plainly_a_call(trimmed) {
            sighted.push(Sighted {
                from: file.label.clone(),
                line: index.saturating_add(1),
            });
        }

        // A sequence item's dash is not part of its first key, and a key matched at the start of a
        // trimmed line is precisely what cannot see one.
        let item = trimmed.starts_with("- ");
        let key_line = trimmed.strip_prefix("- ").unwrap_or(trimmed);
        // Where the KEY starts, which is the dash's column plus the dash and its space. Every
        // sibling of a sequence item's first key sits here, so this - and not `column` - is what
        // bounds a scalar that key opens.
        let key_column = column.saturating_add(trimmed.len().saturating_sub(key_line.len()));
        let Some((key, value)) = key_line.split_once(':') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim().trim_matches('"').trim_matches('\'');
        if opens_a_scalar(value) {
            scalar = Some(key_column);
        }
        if key == "steps" {
            steps = Some(key_column);
            continue;
        }
        if key != "uses" || value.is_empty() {
            continue;
        }

        // A job's `uses:` is a reusable-workflow call; a step's is an action. Only the first makes
        // a target outside this repository a body nobody read.
        let job_level = file.workflow && steps.is_none() && !item;
        let Some(edge) = classify(value, job_level) else {
            continue;
        };
        out.push(Call {
            from: file.label.clone(),
            line: index.saturating_add(1),
            target: String::from(value),
            edge,
        });
    }
    Scan { calls: out, sighted }
}

/// Which kind of edge a `uses:` value is, or `None` for a step's remote action - which has no body
/// in this repository and is not a call this walk is about.
fn classify(value: &str, job_level: bool) -> Option<Edge> {
    if let Some(path) = value.strip_prefix("./") {
        return Some(if path.starts_with(WORKFLOWS) {
            Edge::Workflow
        } else {
            Edge::Action
        });
    }
    job_level.then_some(Edge::Elsewhere)
}

/// Does this value open a block scalar? `|`, `>` and every indicator YAML allows after them.
fn opens_a_scalar(value: &str) -> bool {
    let mut chars = value.chars();
    matches!(chars.next(), Some('|' | '>')) && chars.all(|c| matches!(c, '-' | '+') || c.is_ascii_digit())
}

/// Open what one call names, or say why it could not be opened.
fn open(root: &Path, call: &Call) -> Result<Reached, String> {
    match call.edge {
        Edge::Elsewhere => Err(String::from(
            "it is a call outside this repository, and its steps are in no tree this gate reads",
        )),
        Edge::Workflow => {
            let path = call.target.trim_start_matches("./");
            let file = path.trim_start_matches(WORKFLOWS);
            read(root, path).map(|text| Reached {
                label: String::from(file),
                path: String::from(path),
                text,
                workflow: true,
                root: false,
            })
        }
        Edge::Action => {
            let dir = call.target.trim_start_matches("./").trim_end_matches('/');
            let named = dir.rsplit('/').next().unwrap_or(dir);
            let mut refusals = Vec::new();
            for manifest in MANIFESTS {
                let candidate = format!("{dir}/{manifest}");
                match read(root, &candidate) {
                    Ok(text) => {
                        return Ok(Reached {
                            label: format!("actions/{named}"),
                            path: candidate,
                            text,
                            workflow: false,
                            root: false,
                        });
                    }
                    Err(why) => refusals.push(why),
                }
            }
            Err(refusals.join("; "))
        }
    }
}

/// One file, read relative to the repository root.
fn read(root: &Path, path: &str) -> Result<String, String> {
    std::fs::read_to_string(root.join(path)).map_err(|error| format!("{path}: {error}"))
}

#[cfg(test)]
mod tests {
    use super::{Closure, Reached};

    /// A workflow that calls a reusable workflow at job level, a composite action from a step, and
    /// names a third call inside a `run:` body - which is shell, not YAML.
    const CALLER: &str = concat!(
        "# A comment naming `uses: ./.github/workflows/decoy.yml`, which is prose.\n",
        "on:\n",
        "  pull_request:\n",
        "jobs:\n",
        "  build:\n",
        "    steps:\n",
        "      - uses: actions/checkout@v4\n",
        "      - uses: ./.github/actions/tidy\n",
        "      - name: A step whose body mentions a call\n",
        "        run: |\n",
        "          uses: ./.github/workflows/never.yml\n",
        "          echo done\n",
        "  called:\n",
        "    uses: ./.github/workflows/leg.yml\n",
    );

    /// One scratch tree per test, so a failure cannot be a leftover from another.
    fn scratch(name: &str) -> std::path::PathBuf {
        let at = std::env::temp_dir().join(format!("sutura-reach-{}-{name}", std::process::id()));
        if at.exists() {
            std::fs::remove_dir_all(&at).expect("a stale scratch tree");
        }
        std::fs::create_dir_all(at.join(".github/workflows")).expect("the workflows directory");
        std::fs::create_dir_all(at.join(".github/actions/tidy")).expect("the action directory");
        std::fs::write(at.join(".github/actions/tidy/action.yml"), "runs:\n  using: composite\n").expect("the action");
        at
    }

    fn walk(at: &std::path::Path, caller: &str) -> Closure {
        std::fs::write(at.join(".github/workflows/ci.yml"), caller).expect("the caller");
        Closure::from_roots(at, vec![Reached::workflow("ci.yml", String::from(caller))])
    }

    #[test]
    fn a_job_level_call_a_step_action_and_neither_a_comment_nor_a_run_body_are_followed() {
        let at = scratch("shapes");
        std::fs::write(
            at.join(".github/workflows/leg.yml"),
            "on:\n  workflow_call:\njobs:\n  link:\n",
        )
        .expect("the leg");
        let closure = walk(&at, CALLER);
        let mut labels: Vec<&str> = closure.inspected().iter().map(Reached::label).collect();
        labels.sort_unstable();
        // `decoy.yml` is in a comment and `never.yml` is inside a `run:` block scalar. Neither
        // exists, so following either would also have made this a failure.
        assert_eq!(labels, vec!["actions/tidy", "ci.yml", "leg.yml"]);
        assert!(closure.drift().is_empty(), "{:?}", closure.drift());
        std::fs::remove_dir_all(&at).expect("the scratch tree");
    }

    #[test]
    fn a_called_workflow_that_is_not_there_is_refused_rather_than_skipped() {
        // The dead-gate half: a call naming a file that does not exist must FAIL, because a call
        // this walk cannot open is a step the release refusal cannot read.
        let at = scratch("missing");
        let closure = walk(&at, CALLER);
        let drift = closure.drift();
        assert_eq!(drift.len(), 1, "{drift:?}");
        let sentence = drift.first().expect("the refusal");
        assert!(sentence.starts_with("UNFOLLOWED CALL:"), "{sentence}");
        assert!(sentence.contains("ci.yml:14"), "{sentence}");
        assert!(sentence.contains("./.github/workflows/leg.yml"), "{sentence}");
        // The count is read off the witness: two files opened, three discovered.
        assert!(sentence.contains("2 file(s) inspected of 3 discovered"), "{sentence}");
        std::fs::remove_dir_all(&at).expect("the scratch tree");
    }

    #[test]
    fn a_call_outside_this_repository_is_a_body_nobody_read() {
        let at = scratch("remote");
        let caller = concat!(
            "on:\n  pull_request:\njobs:\n",
            "  called:\n",
            "    uses: other/repo/.github/workflows/leg.yml@v1\n",
        );
        let closure = walk(&at, caller);
        let drift = closure.drift();
        assert_eq!(drift.len(), 1, "{drift:?}");
        assert!(
            drift.first().is_some_and(|s| s.contains("in no tree this gate reads")),
            "{drift:?}"
        );
        std::fs::remove_dir_all(&at).expect("the scratch tree");
    }

    #[test]
    fn a_step_action_from_a_registry_is_not_a_call_this_walk_refuses() {
        // `actions/checkout@v4` has no body here and never will. Refusing it would make the gate
        // red on every workflow in the repository, so the discrimination is load-bearing.
        let at = scratch("registry");
        let caller = "on:\n  pull_request:\njobs:\n  build:\n    steps:\n      - uses: actions/checkout@v4\n";
        let closure = walk(&at, caller);
        assert!(closure.drift().is_empty(), "{:?}", closure.drift());
        assert_eq!(closure.inspected().len(), 1);
        std::fs::remove_dir_all(&at).expect("the scratch tree");
    }

    #[test]
    fn a_flow_mapping_the_key_reader_cannot_see_is_named_rather_than_passed_over() {
        // THE ARM THAT CATCHES THIS READER GOING BLIND. A one-line flow mapping is legal YAML and
        // the key reader does not see it; the position-blind predicate does, so the gate names the
        // line instead of reporting `ok` over a call nobody followed.
        let at = scratch("flow");
        let caller = "on:\n  pull_request:\njobs:\n  called: { uses: ./.github/workflows/leg.yml }\n";
        let closure = walk(&at, caller);
        let drift = closure.drift();
        assert_eq!(drift.len(), 1, "{drift:?}");
        let sentence = drift.first().expect("the refusal");
        assert!(
            sentence.starts_with("UNSIGHTED SHAPE: ci.yml:4 plainly names a local call"),
            "{sentence}"
        );
        assert!(sentence.contains("0 of 1 such line(s) became edges"), "{sentence}");
        std::fs::remove_dir_all(&at).expect("the scratch tree");
    }

    #[test]
    fn a_sibling_key_of_a_sequence_item_that_opens_a_scalar_is_still_read() {
        // THE OTHER DIRECTION OF THE BOUNDARY, and the one it was wrong in. The scalar used to be
        // recorded at the DASH's column, so every sibling key of a sequence item whose first key
        // opened a scalar was swallowed with the body - and `- if: >` is idiomatic YAML that
        // GitHub runs. Silent in BOTH readers, because the body skip precedes the floor, so the
        // fixture over the correctly-bounded direction could not fail on it.
        let at = scratch("mis-bounded");
        let caller = concat!(
            "on:\n  pull_request:\njobs:\n  build:\n    steps:\n",
            "      - if: >\n",
            "          always()\n",
            "        uses: ./.github/actions/tidy\n",
        );
        let closure = walk(&at, caller);
        assert!(closure.drift().is_empty(), "{:?}", closure.drift());
        let labels: Vec<&str> = closure.inspected().iter().map(Reached::label).collect();
        assert_eq!(labels, vec!["ci.yml", "actions/tidy"], "the sibling `uses:` made no edge");
        std::fs::remove_dir_all(&at).expect("the scratch tree");
    }

    #[test]
    fn a_flag_written_before_the_installable_does_not_walk_past_the_refusal() {
        // `nix build -L .#sutura-serve` is an ordinary spelling. The scan required the installable
        // to be the very next token, so it passed at exit 0 - and that it never bit was a property
        // of this tree's text rather than of the rule.
        let found = super::literal_release_builds(concat!(
            "          nix build -L .#sutura-serve\n",
            "          nix build --no-link \".#oci\"\n",
            "          nix build -L .#checks.x86_64-linux.hygiene\n",
        ));
        assert_eq!(found, vec![(1, String::from("sutura-serve")), (2, String::from("oci"))]);
    }

    #[test]
    fn an_unfollowable_target_naming_a_reached_path_is_still_reported() {
        // The arm's trigger used to be `discovered > reached` over a set unioning repo paths with
        // raw `uses:` values, so a target whose TEXT equalled a path already reached collided in
        // it and the refusal went unprinted at exit 0.
        let at = scratch("collision");
        std::fs::write(
            at.join(".github/workflows/leg.yml"),
            "on:\n  workflow_call:\njobs:\n  link:\n",
        )
        .expect("the leg");
        let caller = concat!(
            "on:\n  pull_request:\njobs:\n",
            "  called:\n    uses: ./.github/workflows/leg.yml\n",
            // The same file, spelled without `./` - so it is `Edge::Elsewhere`, unfollowable, and
            // its target text is exactly the path the job above reached.
            "  alias:\n    uses: .github/workflows/leg.yml\n",
        );
        let closure = walk(&at, caller);
        let drift = closure.drift();
        assert_eq!(drift.len(), 1, "{drift:?}");
        let sentence = drift.first().expect("the refusal");
        assert!(sentence.starts_with("UNFOLLOWED CALL: ci.yml:7"), "{sentence}");
        // AND THE PROOF THE GUARD WAS THE CAUSE: the two counts are EQUAL here, because the
        // unfollowable target's text collides with a path already reached. The old trigger read
        // exactly that comparison and printed nothing.
        assert!(sentence.contains("2 file(s) inspected of 2 discovered"), "{sentence}");
        std::fs::remove_dir_all(&at).expect("the scratch tree");
    }

    #[test]
    fn a_shell_body_naming_a_call_is_a_floor_for_neither_reader() {
        // THE LIMIT OF THE ARM ABOVE, held by a fixture because it cannot be held by that arm: the
        // predicate runs only on examined lines, so a `uses:` inside a `run:` block scalar raises
        // no floor - and the walk does not follow it either. If it did, the two would disagree and
        // this closure would refuse a workflow that is entirely correct.
        let at = scratch("body");
        std::fs::write(
            at.join(".github/workflows/leg.yml"),
            "on:\n  workflow_call:\njobs:\n  link:\n",
        )
        .expect("the leg");
        let closure = walk(&at, CALLER);
        assert!(closure.drift().is_empty(), "{:?}", closure.drift());
        // And the file that shell line names is genuinely absent, so following it would have been
        // an UNFOLLOWED CALL rather than a silent pass.
        assert!(!at.join(".github/workflows/never.yml").exists());
        std::fs::remove_dir_all(&at).expect("the scratch tree");
    }

    #[test]
    fn a_literal_release_build_in_a_called_workflow_is_refused() {
        // #290's own red-before-green fixture: the refusal used to read `ci.yml` alone, and this
        // build is in the file a 1000-line cap moved a job into.
        let at = scratch("release");
        std::fs::write(
            at.join(".github/workflows/leg.yml"),
            "on:\n  workflow_call:\njobs:\n  link:\n    steps:\n      - run: nix build .#sutura-serve -L\n",
        )
        .expect("the leg");
        let closure = walk(&at, CALLER);
        assert!(closure.drift().is_empty(), "{:?}", closure.drift());
        assert_eq!(
            super::release_outputs(&closure),
            vec![String::from("leg.yml:6  sutura-serve")]
        );
        std::fs::remove_dir_all(&at).expect("the scratch tree");
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
    fn the_committed_tree_walks_every_pull_request_workflow_and_past_it() {
        // ANTI-VACUITY, over the real files, through the DERIVED root set the gate uses. Ordinary
        // CI is more than one file in two directions, and a correct tree cannot tell a walk that
        // covered both from one that covered neither by its verdict alone: `ci.yml` CALLS a
        // workflow a line cap moved a job into, and `docs.yml` runs on the same pull request
        // without being called by anything.
        let root = crate::repo::root().expect("the repo root");
        let ci = super::super::contexts::OrdinaryCi::read(&root);
        assert!(ci.unreachable().is_empty(), "{:?}", ci.unreachable());
        let labels = super::walked(ci.closure());
        assert!(
            labels.contains(&"cross-link.yml"),
            "ci.yml no longer reaches the workflow a line cap moved its link matrix into: {labels:?}"
        );
        assert!(
            labels.contains(&"docs.yml"),
            "a pull-request workflow nothing calls is outside the walk again: {labels:?}"
        );
        assert!(labels.len() > 2, "the walk read its roots and nothing they call: {labels:?}");
        assert!(super::release_outputs(ci.closure()).is_empty());
    }
}
