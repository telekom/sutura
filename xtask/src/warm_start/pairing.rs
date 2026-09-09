//! Nothing takes the inherited artifacts without the regeneration sweep - as a gate, not a shape.
//!
//! `flake.nix`'s `inheritedArtifacts` returns `cargoArtifacts` AND a `preBuild` sweep as one
//! attrset, and its own comment says why: *so a consumer cannot take the artifacts without the
//! regeneration.* That was true of every derivation the day it was written and was held by nothing
//! - `git grep "preBuild\|inheritedArtifacts\|purge" -- xtask/src` answered with no lines at all,
//! which is #336. Two ways it comes apart, neither of them loud:
//!
//! - **`//` updates one level deep.** A consumer that binds `preBuild` after the pairing keeps the
//!   artifacts and loses the sweep, with no error anywhere. The pairing is a convention at that
//!   point, and a convention is what this repository deletes rows over.
//! - **A route that never calls the constructor.** The warm start is exactly that: `flake.nix`
//!   hands `ciArtifacts` to `nix/cargo-env.nix` under crane's own argument name, and a bare cargo
//!   unpacks them outside any derivation, where a `preBuild` means nothing.
//!
//! WHAT IT COUNTS, and this is the half worth reading. The claim is *no consumer takes the
//! artifacts without the sweep*, so the unit is a **taking** - one binding of crane's
//! `TAKING` argument name - not a line, not a file, and not an occurrence of the word `purge`.
//! Every taking in every `.nix` file in the tree is discovered, each is attributed to whatever
//! RECEIVES it, and [`crate::warm_start::pairing::Swept`] cannot be minted with fewer adjudications than the scan discovered:
//! the count in the verdict comes off the witness rather than out of a `format!`.
//!
//! WHAT PAIRS A TAKING is one of exactly two things, both resolved out of the tree:
//!
//! 1. It is the constructor's own binding, and the constructor's attrset still inlines the sweep.
//!    Since that is the only `PHASE` binding permitted in the artifact flow - see
//!    `in_the_flow` for why the rule is scoped and derived - no consumer can displace it.
//! 2. It is an argument to an `import`ed module, and that module inlines the sweep itself. For a
//!    module that also exports the target directory - a shell warmer - two more facts are read:
//!    the sweep comes AFTER that export, and the variable the sweep resolves is the variable the
//!    export names. Being in the right place buys nothing if the two names have drifted, and
//!    being spelled right buys nothing above the export.
//!
//! Anything else is unattributed and is a REFUSAL, not a pass: a taking this gate cannot follow is
//! the shape a new route arrives in.
//!
//! WHAT IT DOES NOT REACH. It reads text, for [`crate::pins`]' reason - the sandbox it runs in has
//! no nix - so a taking assembled by evaluation (a taking behind a `let` alias, an attrset built
//! by a function this gate does not follow) is invisible, and so is anything that unpacks a store
//! path without naming `TAKING` at all. `preBuild` is the only phase read, so a consumer that
//! re-places artifacts in a later phase is outside it. And it is blind to the profile by design:
//! the sweep derives its profile directory from cargo's own `root-output` record, so it names none
//! and cannot clean the wrong one - which is the trap `cargo clean` fell into in
//! [`crate::causality`].
//!
//! IT ANSWERS *WHERE THE SWEEP'S TEXT IS INLINED*, NEVER *THAT THE TEXT RUNS*, and the two come
//! apart. Measured: comment out the script's own trailing `suturaPurgeBakedOutDirs` invocation and
//! the script still exits 0, `just lint-workflows` shellchecks 14 scripts clean, and the whole of
//! `just hygiene` reports `ok - 32 gate(s)` - over a tree that purges nothing. That half belongs to
//! `xtask/src/warm_start/sweep.rs`, which runs the real script over a real directory and asserts `try_exists` on
//! the unit and its fingerprint, and it reddens on exactly that mutation (`left: (true, true)`).
//! *An `Ok` from a subprocess is not evidence the side effect happened*, and neither is a text
//! scan; the pairing is text and the effect is a filesystem, so the two are held in two venues.
//! Both sit inside `just validate`, and there is no diff for which one runs without the other:
//! `.github/workflows/ci.yml` classifies `nix/purge-baked-out-dirs.sh` and [`crate::warm_start::WARMER`] under
//! no area, which fails open to `run_all`, and `flake.nix`'s area lists a `rust` consumer - so the
//! sufficiency is CI's classifier, not a coincidence. The limit is second-order: putting `nix/**`
//! into `DOCS_ONLY`, or into an area with no `rust` consumer, would let this route go green.
//!
//! **Reachability of the inline SITE is held by neither**: the sweep placed after an `exit`, or
//! inside a shell conditional, in [`crate::warm_start::WARMER`]'s exported string satisfies this gate's line
//! rules and `xtask/src/warm_start/sweep.rs`'s standalone run alike. What IS held is that the inline sits in the
//! string a `${..}` expands rather than anywhere in the file - `module_sweeps` carries the
//! measurement, because file-wide was a printed pass over a tree where nothing swept.

use std::path::{Path, PathBuf};

use crate::repo;

/// Reading a nix file: the binding scan, the brace walks and the `inherit` reader.
///
/// Its own file because the claim and the reading are two tasks, and because this one was at
/// the 1000-line cap. The primitives take text and return offsets, so they are exercised
/// directly there rather than only through this module's verdict.
mod reading;

use reading::{NixFile, attrset_at, bound_at, enclosing_attrset, imported_at, inherited_at, line_at, whole_word};

/// The sweep, repo-relative. Every taking is paired with THIS file or with nothing.
pub(super) const SWEEP: &str = "nix/purge-baked-out-dirs.sh";

/// crane's argument name for artifacts built in another derivation.
///
/// A BINDING of it is the unit this gate counts, because every route into this workspace's builds
/// receives the artifacts under this name - the derivations through the constructor, the warm
/// start through a module argument. Counting lines or files instead would answer a question
/// nobody asked: two takings fit on one line, and one file holds several.
const TAKING: &str = "cargoArtifacts";

/// The phase the sweep is inlined into for a derivation, and the only one this tree may bind.
const PHASE: &str = "preBuild";

/// The binding that pairs a taking with the sweep.
const CONSTRUCTOR: &str = "inheritedArtifacts";

/// How the sweep's text gets into a nix expression.
const INLINE: &str = "builtins.readFile";

/// The variable the sweep script resolves its target directory out of.
const SWEEP_TARGET: &str = "targetDir";

/// The constructor, and the range of the attrset that has to hold the pairing.
struct Pairing {
    /// The file the constructor is declared in.
    rel: String,
    /// The line it is declared on, for a message a reader can act on.
    line: usize,
    /// Byte range of its attrset within that file's code half.
    attrset: (usize, usize),
}

/// One place the artifacts are handed to something that builds with them.
struct Taking {
    rel: String,
    line: usize,
    receiver: Receiver,
}

/// What receives a taking, resolved out of the tree rather than listed here.
enum Receiver {
    /// The constructor's own binding.
    Constructor,
    /// An imported module's argument set, as a repo-relative path.
    Module(String),
    /// Nothing this gate can follow.
    Unattributed,
}

/// Every taking the scan found. The field is private and [`discover`] is the only constructor, so
/// a denominator cannot be conjured by a caller that reached fewer of them.
struct Discovered(Vec<Taking>);

/// A verdict that adjudicated EVERY taking the scan discovered.
///
/// [`Swept::over`] is the only constructor and it refuses two ways: fewer adjudications than
/// takings, and no takings at all. So the sentence [`Swept::verdict`] prints cannot state a number
/// the scan did not reach - the count is the witness's own length - and a gate that discovered
/// nothing is a failure rather than an `ok` over silence.
///
/// **Which of the two arms is reachable through [`holds`], stated because the other reads stronger
/// than it is.** The subset arm is the live one: `.take(1)` over the adjudication loop reddens the
/// gate. The empty arm is belt-and-braces, because [`pairing`] already refuses a constructor that
/// binds no [`TAKING`] and that binding is itself a taking - so an empty scan is caught one step
/// earlier, by that floor or by [`nix_files`] not finding `flake.nix`. It stays because the floor
/// above it could be relaxed by someone who did not read this far.
#[derive(Debug)]
pub(super) struct Swept {
    paired: Vec<String>,
}

impl Swept {
    fn over(discovered: &Discovered, paired: Vec<String>) -> Result<Self, String> {
        if discovered.0.is_empty() {
            return Err(format!(
                "no nix file in this tree binds `{TAKING}`, so nothing takes the inherited artifacts and this gate checked NOTHING. \
                 That is a failure rather than a pass: the pairing it holds is about takings, and a scan that finds none is broken \
                 rather than satisfied"
            ));
        }
        if paired.len() != discovered.0.len() {
            return Err(format!(
                "inspected {} of {} taking(s) of the inherited artifacts - the rest were never adjudicated, so this verdict is about a subset",
                paired.len(),
                discovered.0.len()
            ));
        }
        Ok(Self { paired })
    }

    pub(super) fn verdict(&self) -> String {
        format!(
            "{} taking(s) of the inherited artifacts, each paired with {SWEEP}: {}",
            self.paired.len(),
            self.paired.join("; ")
        )
    }
}

/// One nix file as a SIBLING claim needs it: where it lives, and what the evaluator sees.
///
/// A named pair rather than a tuple, for the reason [`Scan`]'s neighbours give: `(String, String)`
/// says nothing about which string is the path. Not [`NixFile`] either - that type carries `raw`
/// as well, and a claim that only reads code should not be handed the view where a comment still
/// counts as text.
pub(super) struct NixCode {
    pub(super) rel: String,
    pub(super) code: String,
}

/// The CODE view of every `.nix` file in the tree, for a sibling claim over the same scan.
///
/// Here and not in [`super::deps_targets`] because [`nix_files`] is the only enumeration this
/// gate owns and `repo::all_files` has an exact-count door on its callers: one scan, two claims,
/// no second caller.
pub(super) fn code_of_every_nix_file(root: &Path) -> Result<Vec<NixCode>, String> {
    Ok(nix_files(root)?
        .into_iter()
        .map(|file| NixCode {
            rel: file.rel,
            code: file.code,
        })
        .collect())
}

/// Every `.nix` file in the tree, or a failure naming what it could not enumerate.
fn nix_files(root: &Path) -> Result<Vec<NixFile>, String> {
    let (_root, listing) = repo::all_files()
        .and_then(|census| census.into_listing(repo::Unmigrated::WarmStart))
        .map_err(|why| why.describe())?;
    let mut files = Vec::new();
    let nix = std::ffi::OsStr::new("nix");
    for rel in listing.iter().filter(|rel| Path::new(rel).extension() == Some(nix)) {
        files.push(NixFile::read(root, rel)?);
    }
    // FAIL CLOSED, and not on emptiness alone: this gate's whole subject is declared in
    // `flake.nix`, so a listing that reached every other nix file and not that one is a broken
    // scan wearing a plausible file count.
    if !files.iter().any(|file| file.rel == "flake.nix") {
        return Err(format!(
            "read {} nix file(s) and flake.nix was not among them, so the takings this gate is about were never scanned",
            files.len()
        ));
    }
    Ok(files)
}

/// Can this file be in the artifact flow at all?
///
/// A whole-word mention of the constructor or the taking, in either direction: a file that
/// APPLIES the constructor, one that RECEIVES it as an argument (`nix/shipped.nix`), and one that
/// receives the artifacts under crane's own name (`nix/cargo-env.nix`). Anything else has no
/// artifacts to lose, so [`PHASE`]'s rule has nothing to say about it.
fn in_the_flow(file: &NixFile) -> bool {
    [CONSTRUCTOR, TAKING].iter().any(|name| {
        let mut from = 0_usize;
        while let Some(offset) = file.code.get(from..).and_then(|rest| rest.find(name)) {
            let start = from.saturating_add(offset);
            let end = start.saturating_add(name.len());
            from = end;
            if whole_word(&file.code, start, end) {
                return true;
            }
        }
        false
    })
}

/// A nix path literal in `rel`'s directory, as a repo-relative path that EXISTS.
///
/// Resolved rather than compared as text, which is the difference between reading a path and
/// reading a spelling: `./purge-baked-out-dirs.sh` in `nix/cargo-env.nix` and
/// `./nix/purge-baked-out-dirs.sh` in `flake.nix` are the same file and neither string says so.
fn resolve(root: &Path, rel: &str, literal: &str) -> Option<String> {
    let parent = Path::new(rel).parent().unwrap_or_else(|| Path::new(""));
    let joined: PathBuf = root.join(parent).join(literal.trim_start_matches("./"));
    let canonical = joined.canonicalize().ok()?;
    repo::relative(&root.canonicalize().ok()?, &canonical)
}

/// Does this line inline the sweep - resolving the path, not matching the spelling?
fn inlines_sweep(root: &Path, rel: &str, line: &str) -> bool {
    let Some((_, after)) = line.split_once(INLINE) else {
        return false;
    };
    let literal = after
        .trim_start()
        .split(|c: char| c.is_whitespace() || matches!(c, ';' | '}' | ')'))
        .next()
        .unwrap_or_default();
    if literal.is_empty() {
        return false;
    }
    resolve(root, rel, literal).is_some_and(|resolved| resolved == SWEEP)
}

/// The constructor, and the assertion that it still pairs.
///
/// EXACTLY ONE declaration, because two would be two things to keep in step and a consumer would
/// pick one; zero is a broken scan rather than a tree with no pairing, since the takings it
/// attributes to it are still there.
fn pairing(root: &Path, files: &[NixFile]) -> Result<Pairing, String> {
    let mut declarations = Vec::new();
    for file in files {
        for offset in bound_at(&file.code, CONSTRUCTOR) {
            declarations.push((file, offset));
        }
    }
    let [(file, offset)] = declarations.as_slice() else {
        return Err(format!(
            "expected exactly one `{CONSTRUCTOR} =` declaration in this tree and found {}; \
             the pairing has one owner or it has none",
            declarations.len()
        ));
    };
    let line = line_at(&file.code, *offset);
    let attrset = attrset_at(&file.code, *offset).ok_or_else(|| {
        format!(
            "{}:{line}: `{CONSTRUCTOR}` opens no attrset that closes, so this gate cannot read the pairing",
            file.rel
        )
    })?;
    let body = file.code.get(attrset.0..attrset.1).unwrap_or_default();

    if bound_at(body, TAKING).is_empty() {
        return Err(format!(
            "{}:{line}: `{CONSTRUCTOR}` binds no `{TAKING}`, so it pairs nothing with {SWEEP}",
            file.rel
        ));
    }
    let phases = bound_at(body, PHASE);
    let [phase] = phases.as_slice() else {
        return Err(format!(
            "{}:{line}: `{CONSTRUCTOR}` binds `{PHASE}` {} times; the sweep is paired with the artifacts once or the pairing is a guess",
            file.rel,
            phases.len()
        ));
    };
    // The BINDING'S OWN VALUE, up to the `;`, and not the whole attrset: a `preBuild` bound to
    // something else beside a comment naming the sweep is the shape a scan over the attrset
    // passes. This is the mutation that breaks the pairing and it has to be the loud one.
    let value = body
        .get(phase.saturating_add(PHASE.len())..)
        .unwrap_or_default()
        .split(';')
        .next()
        .unwrap_or_default();
    if !inlines_sweep(root, &file.rel, value) {
        // THE PHASE'S OWN LINE AND THE RAW TEXT OF IT, because the value read above is LEXED: a
        // `preBuild` bound to a string has its interior blanked, so printing what was compared
        // prints `=` and names nothing an author can act on. Measured on the real tree - a
        // refusal that cannot be acted on is half a gate.
        let at = line_at(&file.code, attrset.0.saturating_add(*phase));
        let wrote = file.raw.lines().nth(at.saturating_sub(1)).unwrap_or_default().trim();
        return Err(format!(
            "{}:{at}: `{CONSTRUCTOR}` binds `{PHASE}` to something that is not {SWEEP}, so every taking it pairs \
             now inherits a build root that names nothing: {wrote}",
            file.rel
        ));
    }
    Ok(Pairing {
        rel: file.rel.clone(),
        line,
        attrset,
    })
}

/// The sweep is the ONLY [`PHASE`] in the artifact flow, which closes the shallow-update hole.
///
/// `args // inheritedArtifacts a // { preBuild = ..; }` keeps the artifacts and loses the sweep,
/// silently, and no count of takings notices - the taking is still paired, by an attrset whose
/// value was replaced afterwards. So the rule is about the PHASE rather than about the update: a
/// `preBuild` this flow does not own is refused, and a `preBuild` needed for another reason
/// composes inside the constructor, where it runs beside the sweep.
///
/// SCOPED, and DERIVED rather than listed. Tree-wide would redden correct work - an unrelated
/// derivation in some other `nix/` module may want a `preBuild` and has no artifacts to lose - and
/// a gate that reddens correct work gets disabled. The scope is [`in_the_flow`]: a file that names
/// the constructor or the taking, which today selects `flake.nix`, `nix/shipped.nix` and
/// `nix/cargo-env.nix` and would select a new module the moment it received either.
fn phase_has_one_owner(files: &[NixFile], owner: &Pairing) -> Result<(), String> {
    let mut elsewhere = Vec::new();
    let mut pairings = 0_usize;
    for file in files.iter().filter(|file| in_the_flow(file)) {
        for offset in bound_at(&file.code, PHASE) {
            let inside = file.rel == owner.rel && offset >= owner.attrset.0 && offset < owner.attrset.1;
            if inside {
                pairings = pairings.saturating_add(1);
            } else {
                elsewhere.push(format!("{}:{}", file.rel, line_at(&file.code, offset)));
            }
        }
    }
    if pairings == 0 {
        return Err(format!(
            "no `{PHASE}` is bound inside `{CONSTRUCTOR}`, so this gate read a pairing that is not there"
        ));
    }
    if !elsewhere.is_empty() {
        return Err(format!(
            "`{PHASE}` is bound in the artifact flow but outside `{CONSTRUCTOR}` at {}. `//` updates one level deep, \
             so a phase bound after the pairing REPLACES the sweep and keeps the artifacts - no error, nothing red, \
             and a build root that names nothing. Compose it in `{CONSTRUCTOR}` ({}:{}) instead",
            elsewhere.join(", "),
            owner.rel,
            owner.line
        ));
    }
    Ok(())
}

/// Every taking in the tree, each attributed to whatever receives it.
fn discover(files: &[NixFile], owner: &Pairing) -> Discovered {
    let mut takings = Vec::new();
    for file in files {
        for offset in bound_at(&file.code, TAKING) {
            let receiver = if file.rel == owner.rel && offset >= owner.attrset.0 && offset < owner.attrset.1 {
                Receiver::Constructor
            } else {
                enclosing_attrset(&file.code, offset)
                    .and_then(|brace| imported_at(&file.code, brace))
                    .map_or(Receiver::Unattributed, |literal| Receiver::Module(String::from(literal)))
            };
            takings.push(Taking {
                rel: file.rel.clone(),
                line: line_at(&file.code, offset),
                receiver,
            });
        }
        for offset in inherited_at(&file.code, TAKING) {
            takings.push(Taking {
                rel: file.rel.clone(),
                line: line_at(&file.code, offset),
                receiver: Receiver::Unattributed,
            });
        }
    }
    Discovered(takings)
}

/// What pairs this one taking, or why nothing does.
fn adjudicate(root: &Path, files: &[NixFile], taking: &Taking, owner: &Pairing) -> Result<String, String> {
    let Taking { rel, line, receiver } = taking;
    match receiver {
        Receiver::Constructor => Ok(format!("{rel}:{line} is `{CONSTRUCTOR}`'s own")),
        Receiver::Module(literal) => {
            let module = resolve(root, rel, literal).ok_or_else(|| {
                format!("{rel}:{line} hands the artifacts to `import {literal}`, which resolves to no file in this repo")
            })?;
            let received = files.iter().find(|file| file.rel == module).ok_or_else(|| {
                format!("{rel}:{line} hands the artifacts to {module}, which is not among the nix files this gate read")
            })?;
            let how = module_sweeps(root, received)?;
            Ok(format!("{rel}:{line} → {module} ({how})"))
        }
        Receiver::Unattributed => Err(format!(
            "{rel}:{line} binds `{TAKING}` and this gate cannot attribute it. A taking is paired by `{CONSTRUCTOR}` \
             ({}:{}) or by an `import`ed module that inlines {SWEEP} itself, and this is neither - so the artifacts \
             arrive with whatever absolute build directory a build script baked into what it generated, and the \
             failure lands in whatever compiles them",
            owner.rel, owner.line
        )),
    }
}

/// A module that receives the artifacts runs the sweep itself, in the right place, on the right
/// directory.
///
/// Three facts and each one is load-bearing on its own. The sweep is INLINED there - resolved, so
/// a rename is caught. It is inlined AFTER the export, because the script resolves its target
/// directory out of that variable and above the export it walks the developer's `target/` and
/// prints `0 ... regenerated here` about a directory nobody asked about. And the variable it
/// resolves is the variable the export names, because position buys nothing once the names differ.
///
/// The last two apply to a shell warmer and are skipped for a module that exports no target
/// directory, which is [`super::exported_value`]'s answer rather than a list of module names.
///
/// AND FOR A SHELL WARMER THE SCOPE IS THE STRING, NOT THE FILE, which is the review finding this
/// paragraph exists for. The search used to be file-wide: move the inline out of the binding its
/// consumers expand into a `let` nothing expands, still below the export, and the gate printed
/// `inlines the sweep at nix/cargo-env.nix:129, after the CARGO_TARGET_DIR export it resolves` over
/// a tree where **none of the five consumers swept** - #346's defect restored and reported as a
/// pass, with the sweep's filesystem test green beside it because the script itself was untouched.
/// The bound is [`enclosing_indented_string`] around the export line, so the text this rule reads
/// is the text a `${..}` expansion actually inserts.
fn module_sweeps(root: &Path, received: &NixFile) -> Result<String, String> {
    let NixFile {
        rel: module, raw: text, ..
    } = received;
    let inline_at = |range: std::ops::RangeInclusive<usize>| {
        super::live_indexed(text)
            .find(|(index, line)| range.contains(index) && inlines_sweep(root, module, line))
            .map(|(index, _)| index)
    };

    // A module that exports no target directory is not a shell warmer, so the rules below - all
    // three about one shell's own text and ordering - have nothing to be about, and the inline may
    // be anywhere in the file. That is `exported_value`'s answer rather than a list of module
    // names this gate would have to keep current.
    if super::exported_value(text).is_none() {
        let at = inline_at(0..=usize::MAX).ok_or_else(|| {
            format!(
                "{module} receives the artifacts and inlines no `{INLINE} <{SWEEP}>`, so whatever it hands them to \
                 builds against a build root that names nothing. Every consumer of that module inherits the gap"
            )
        })?;
        return Ok(format!("inlines the sweep at {module}:{}", at.saturating_add(1)));
    }
    let export = super::live_indexed(text)
        .find(|(_, line)| line.starts_with(super::export_assignment()))
        .map(|(index, _)| index)
        .ok_or_else(|| format!("{module} exports a target directory this gate then could not find the line of"))?;
    let (open, close) = enclosing_indented_string(text, export).ok_or_else(|| {
        format!(
            "{module} exports {} at line {} and this gate cannot find the `''..''` string that line sits in, so it \
             cannot tell whether the sweep is in the text a consumer expands",
            target_var(),
            export.saturating_add(1)
        )
    })?;
    let at = inline_at(open..=close).ok_or_else(|| {
        format!(
            "{module} receives the artifacts and inlines no `{INLINE} <{SWEEP}>` INSIDE the `''..''` string it \
             exports {} in (lines {}-{}), so whatever expands that string builds against a build root that names \
             nothing. An inline elsewhere in the file is text no `${{..}}` inserts. Every consumer inherits the gap",
            target_var(),
            open.saturating_add(1),
            close.saturating_add(1)
        )
    })?;
    if at < export {
        return Err(format!(
            "{module} inlines the sweep at line {} and exports the target directory at line {}. The sweep resolves \
             {} and would sweep the developer's `target/` from up there, printing `0 ... regenerated here` about a \
             directory nobody asked about. It goes AFTER the export",
            at.saturating_add(1),
            export.saturating_add(1),
            target_var()
        ));
    }

    let sweep = std::fs::read_to_string(root.join(SWEEP)).map_err(|error| format!("could not read {SWEEP}: {error}"))?;
    let assigned = super::assigned(&sweep, SWEEP_TARGET).ok_or_else(|| {
        format!("{SWEEP} assigns no `{SWEEP_TARGET}=\"..\"`, so this gate cannot tell which directory it sweeps")
    })?;
    let resolves = shell_variable(&assigned).ok_or_else(|| {
        format!(
            "{SWEEP} sets {SWEEP_TARGET} to {assigned:?}, which names no shell variable this gate can compare against the export"
        )
    })?;
    if resolves != target_var() {
        return Err(format!(
            "{module} exports {} and {SWEEP} sweeps whatever ${resolves} holds. Being after the export buys nothing \
             once the two names differ: the sweep would report about one directory while the build uses another",
            target_var()
        ));
    }
    Ok(format!(
        "inlines the sweep at {module}:{}, after the {} export it resolves",
        at.saturating_add(1),
        target_var()
    ))
}

/// The variable the warmer exports, derived from [`super::export_assignment`] rather than spelled
/// again - so the quote `super::EXPORT` carries for [`super::exported_value`]'s sake cannot leak
/// into a name.
fn target_var() -> &'static str {
    super::export_assignment().trim_start_matches("export ").trim_end_matches('=')
}

/// Does this line carry an indented-string delimiter, rather than only nix's escapes for one?
///
/// Inside a `''..''` string `''$` writes a literal `${`, `'''` writes `''` and `''\` starts an
/// escape - none of the three ends the string. `nix/cargo-env.nix` writes `''${LD_LIBRARY_PATH:+..}`
/// in the binding above the warm start, so a `contains("''")` would read that as a delimiter and
/// bound [`module_sweeps`]' search to the wrong text.
fn delimits(line: &str) -> bool {
    let mut from = 0_usize;
    while let Some(found) = line.get(from..).and_then(|rest| rest.find("''")) {
        let start = from.saturating_add(found);
        let end = start.saturating_add(2);
        from = end;
        if !line.as_bytes().get(end).is_some_and(|c| matches!(*c, b'$' | b'\'' | b'\\')) {
            return true;
        }
    }
    false
}

/// The `(open, close)` line indices of the indented string the line at `at` sits inside.
///
/// Line-based, and that is the limit: it finds the nearest delimiter each way rather than tracking
/// nesting, so a `''..''` opened and closed on one line between the export and its own delimiter
/// would mislead it. Nothing in this tree writes that, and the direction it fails in is a range
/// that is too SMALL - which refuses rather than passes.
fn enclosing_indented_string(text: &str, at: usize) -> Option<(usize, usize)> {
    let lines: Vec<&str> = text.lines().collect();
    let open = lines.get(..at)?.iter().rposition(|line| delimits(line))?;
    let after = at.saturating_add(1);
    let close = after.saturating_add(lines.get(after..)?.iter().position(|line| delimits(line))?);
    Some((open, close))
}

/// The first `${NAME...}` or `$NAME` a shell value names.
fn shell_variable(value: &str) -> Option<&str> {
    let after = value.split_once('$')?.1.trim_start_matches('{');
    let end = after.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))?;
    let name = after.get(..end)?;
    if name.is_empty() { None } else { Some(name) }
}

/// The pairing, over the whole tree.
pub(super) fn holds(root: &Path) -> Result<Swept, String> {
    let files = nix_files(root)?;
    let owner = pairing(root, &files)?;
    phase_has_one_owner(&files, &owner)?;
    let discovered = discover(&files, &owner);
    let mut paired = Vec::new();
    for taking in &discovered.0 {
        paired.push(adjudicate(root, &files, taking, &owner)?);
    }
    Swept::over(&discovered, paired)
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    /// The sibling accessor reaches the same tree this gate's own scan does.
    ///
    /// Here rather than in [`super::super::deps_targets`], and the reason is the harness rather
    /// than tidiness: `test-causality` keeps a file that adds a `#[test]` and reverts one that
    /// does not, so the accessor and the claim built on it have to be red-able from the same
    /// side of that line - otherwise reverting THIS file leaves a kept test calling a function
    /// that no longer exists, and a build failure is not the same evidence as a red test.
    #[test]
    fn the_shared_scan_reaches_the_flake() {
        let root = crate::repo::root().expect("the repo root");
        let files = super::code_of_every_nix_file(&root).expect("every nix file's code");
        assert!(
            files.iter().any(|file| file.rel == "flake.nix"),
            "the scan reached {} file(s) and flake.nix was not among them",
            files.len()
        );
        // And the code view carries CODE, not an empty string: the sibling's whole judgement is
        // made out of this field, so a scan that reached the file and handed over nothing would
        // pass it silently.
        let flake = files.iter().find(|file| file.rel == "flake.nix").expect("flake.nix");
        assert!(flake.code.contains("buildDepsOnly"), "flake.nix's code view is empty");
    }

    /// The one line of the sweep script this gate reads, for whichever variable it resolves.
    ///
    /// A `format!` rather than a literal, and not only for clippy's sake: building it makes the
    /// drifted-variable case below a DIFFERENT SCRIPT rather than a text rewrite of the right one.
    fn sweep_script(variable: &str) -> String {
        format!("suturaPurgeBakedOutDirs() (\n  targetDir=\"${{{variable}:-target}}\"\n)\n")
    }

    /// A tree with a constructor, a module taking, and a module that sweeps after its export.
    fn fixture(root: &Path, sweep_variable: &str) -> std::io::Result<()> {
        std::fs::create_dir_all(root.join("nix"))?;
        std::fs::write(root.join(super::SWEEP), sweep_script(sweep_variable))?;
        std::fs::write(
            root.join("nix/cargo-env.nix"),
            concat!(
                "{ cargoArtifacts }:\n",
                "{\n",
                "  cargoWarmStart = ''\n",
                "    export CARGO_TARGET_DIR=\"$warmTarget\"\n",
                "    ${builtins.readFile ./purge-baked-out-dirs.sh}\n",
                "  '';\n",
                "}\n",
            ),
        )?;
        std::fs::write(
            root.join("flake.nix"),
            concat!(
                "{\n",
                "  # A comment discussing preBuild and cargoArtifacts, which no scan may count.\n",
                "  inheritedArtifacts = artifacts: {\n",
                "    cargoArtifacts = artifacts;\n",
                "    preBuild = builtins.readFile ./nix/purge-baked-out-dirs.sh;\n",
                "  };\n",
                "  inherit (import ./nix/cargo-env.nix {\n",
                "    cargoArtifacts = ciArtifacts;\n",
                "  }) cargoWarmStart;\n",
                "}\n",
            ),
        )?;
        std::fs::write(
            root.join("nix/mimalloc.nix"),
            "{\n  build = ''\n    runHook preBuild\n  '';\n}\n",
        )
    }

    fn tree(case: &str) -> PathBuf {
        tree_sweeping(case, "CARGO_TARGET_DIR")
    }

    fn tree_sweeping(case: &str, sweep_variable: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("sutura-pairing-{case}-{}", std::process::id()));
        drop(std::fs::remove_dir_all(&root));
        fixture(&root, sweep_variable).expect("the fixture tree");
        root
    }

    /// The gate over a fixture tree, without `repo::all_files`, which answers about THIS repo.
    fn over(root: &Path) -> Result<super::Swept, String> {
        let mut files = Vec::new();
        for rel in ["flake.nix", "nix/cargo-env.nix", "nix/mimalloc.nix"] {
            files.push(super::NixFile::read(root, rel)?);
        }
        let owner = super::pairing(root, &files)?;
        super::phase_has_one_owner(&files, &owner)?;
        let discovered = super::discover(&files, &owner);
        let mut paired = Vec::new();
        for taking in &discovered.0 {
            paired.push(super::adjudicate(root, &files, taking, &owner)?);
        }
        super::Swept::over(&discovered, paired)
    }

    fn rewrite(root: &Path, rel: &str, from: &str, to: &str) {
        let text = std::fs::read_to_string(root.join(rel)).expect("the fixture file");
        assert!(text.contains(from), "the fixture no longer contains {from:?}");
        std::fs::write(root.join(rel), text.replace(from, to)).expect("the rewrite");
    }

    #[test]
    fn a_paired_tree_passes_and_counts_takings_rather_than_lines() {
        let root = tree("paired");
        let swept = over(&root).expect("both takings are paired");
        // TWO, and they are the two ROUTES: the constructor's own binding covers every derivation
        // that calls it, and the module argument is the warm start. The decoys in the fixture -
        // a comment naming both identifiers, `runHook preBuild` inside a builder string - are
        // exactly what a raw scan counts and this must not.
        assert_eq!(swept.paired.len(), 2, "{:?}", swept.paired);
        assert!(swept.verdict().contains("2 taking(s)"), "{}", swept.verdict());
    }

    #[test]
    fn taking_the_artifacts_without_the_sweep_reddens_and_names_what_it_found() {
        // MUTATION ONE, the whole subject: a consumer takes the artifacts and pairs them with
        // nothing. This is the shape `flake.nix:346` had for the warm start before #346.
        let root = tree("unpaired");
        rewrite(
            &root,
            "flake.nix",
            "  inherit (import ./nix/cargo-env.nix {\n    cargoArtifacts = ciArtifacts;\n  }) cargoWarmStart;\n",
            "  nextest = craneLib.cargoNextest (ciArgs // {\n    cargoArtifacts = ciArtifacts;\n  });\n",
        );
        let why = over(&root).expect_err("an unattributed taking is not paired");
        assert!(why.contains("flake.nix:8"), "the refusal has to name the taking: {why}");
        assert!(why.contains("cannot attribute it"), "{why}");
    }

    #[test]
    fn a_constructor_that_stopped_pairing_is_the_loud_failure() {
        // MUTATION TWO: delete the sweep from the constructor. Every taking it covers - seven
        // checks, the xtask package and both shipped builds on the real tree - loses the
        // regeneration at once, and nothing else in the file changes.
        let root = tree("unpaired-constructor");
        rewrite(
            &root,
            "flake.nix",
            "    preBuild = builtins.readFile ./nix/purge-baked-out-dirs.sh;\n",
            "",
        );
        let why = over(&root).expect_err("a constructor with no sweep pairs nothing");
        assert!(why.contains("binds `preBuild` 0 times"), "{why}");
        // And bound to something ELSE, which is the version a comment beside it would hide.
        let root = tree("wrong-constructor");
        rewrite(
            &root,
            "flake.nix",
            "builtins.readFile ./nix/purge-baked-out-dirs.sh;",
            "builtins.readFile ./nix/mimalloc.nix; # purge-baked-out-dirs.sh",
        );
        let why = over(&root).expect_err("a phase bound to another file is not the sweep");
        assert!(why.contains("is not nix/purge-baked-out-dirs.sh"), "{why}");
        // And bound to a STRING that spells the path, which is the shape the refusal could not
        // name: the lexer blanks a string's interior, so the value COMPARED is empty and a message
        // printing it says `=`. The line is the phase's own (5) rather than the constructor's (3),
        // and the text is the raw one, because those two together are what an author acts on.
        let root = tree("stringly-constructor");
        rewrite(
            &root,
            "flake.nix",
            "    preBuild = builtins.readFile ./nix/purge-baked-out-dirs.sh;\n",
            "    preBuild = \"echo ./nix/purge-baked-out-dirs.sh\";\n",
        );
        let why = over(&root).expect_err("a phase bound to a string reads no file");
        assert!(why.contains("flake.nix:5:"), "the PHASE's line, not the constructor's: {why}");
        assert!(
            why.contains("echo ./nix/purge-baked-out-dirs.sh"),
            "the raw line is the actionable half: {why}"
        );
    }

    #[test]
    fn a_second_phase_anywhere_is_refused_because_it_would_displace_the_sweep() {
        // MUTATION THREE: the shallow-update hole. The taking is still paired and the sweep is
        // still in the constructor; the consumer just binds the phase again afterwards.
        let root = tree("displaced");
        rewrite(
            &root,
            "flake.nix",
            "  inherit (import ./nix/cargo-env.nix {",
            "  clippy = craneLib.cargoClippy (ciArgs // inheritedArtifacts ciArtifacts // {\n    preBuild = \"true\";\n  });\n  inherit (import ./nix/cargo-env.nix {",
        );
        let why = over(&root).expect_err("a second preBuild replaces the sweep");
        assert!(why.contains("outside `inheritedArtifacts`"), "{why}");
        assert!(why.contains("flake.nix:8"), "{why}");
    }

    #[test]
    fn an_inherited_taking_is_discovered_and_refused_rather_than_missed() {
        // `inherit cargoArtifacts;` has no `=` in it, so the binding scan passes straight over it.
        // Discovered as a taking and never attributed, because an `inherit` names something in an
        // enclosing scope and this gate follows none.
        let root = tree("inherited");
        rewrite(
            &root,
            "flake.nix",
            "  inherit (import ./nix/cargo-env.nix {",
            "  hygiene = craneLib.mkCargoDerivation (ciArgs // { inherit cargoArtifacts; });\n  inherit (import ./nix/cargo-env.nix {",
        );
        let why = over(&root).expect_err("an inherited taking is not an attributed one");
        assert!(why.contains("cannot attribute it"), "{why}");
        assert!(why.contains("flake.nix:7"), "{why}");
        // And the `inherit (expr) names;` form is NOT a false positive, which is why this reader
        // skips a parenthesised source: the real tree writes exactly that, with a taking inside
        // the parentheses that the binding scan already attributes to the module.
        let root = tree("inherit-from-an-expression");
        let swept = over(&root).expect("the tree writes `inherit (import ..) names;` and passes");
        assert_eq!(swept.paired.len(), 2, "{:?}", swept.paired);
    }

    #[test]
    fn a_module_that_receives_the_artifacts_has_to_sweep_them_itself() {
        let root = tree("silent-module");
        rewrite(
            &root,
            "nix/cargo-env.nix",
            "    ${builtins.readFile ./purge-baked-out-dirs.sh}\n",
            "",
        );
        let why = over(&root).expect_err("a module with no sweep leaves every consumer of it exposed");
        assert!(why.contains("inlines no"), "{why}");
        // A renamed script is the same failure and a text scan for the basename would miss it.
        let root = tree("renamed-script");
        rewrite(
            &root,
            "nix/cargo-env.nix",
            "./purge-baked-out-dirs.sh",
            "./purge-baked-out-dirs.sh.bak",
        );
        let why = over(&root).expect_err("a path that resolves to no file is not the sweep");
        assert!(why.contains("inlines no"), "{why}");
    }

    #[test]
    fn a_sweep_outside_the_string_the_consumers_expand_is_not_the_warmers_sweep() {
        // THE BLOCKING FINDING. The inline moves out of the binding `${cargoWarmStart}` expands
        // into one nothing expands - still in the same file, still below the export - and the
        // file-wide search called that paired: `inlines the sweep at .. after the CARGO_TARGET_DIR
        // export it resolves`, over a tree where no consumer sweeps. #346's defect restored and
        // printed as a pass, and the sweep's filesystem test green beside it because the script is
        // untouched. The scope is the string now, so this is red.
        let root = tree("outside-the-string");
        rewrite(
            &root,
            "nix/cargo-env.nix",
            concat!(
                "    export CARGO_TARGET_DIR=\"$warmTarget\"\n",
                "    ${builtins.readFile ./purge-baked-out-dirs.sh}\n",
                "  '';\n",
            ),
            concat!(
                "    export CARGO_TARGET_DIR=\"$warmTarget\"\n",
                "  '';\n",
                "  unexpanded = ''\n",
                "    ${builtins.readFile ./purge-baked-out-dirs.sh}\n",
                "  '';\n",
            ),
        );
        let why = over(&root).expect_err("an inline nothing expands is not the warmer's sweep");
        assert!(why.contains("INSIDE the `''..''` string"), "{why}");
        assert!(why.contains("no `${..}` inserts"), "{why}");
        // And nix's own escapes for a delimiter are not delimiters: `''${..}` above the export is
        // what `nix/cargo-env.nix` writes one binding up, and reading it as one would bound the
        // search to the wrong text and redden the true tree.
        assert!(!super::delimits("    export LD_LIBRARY_PATH=\"$x''${LD_LIBRARY_PATH:+:$y}\""));
        assert!(!super::delimits("    printf '''"));
        assert!(super::delimits("  cargoWarmStart = ''"));
        assert!(super::delimits("  '';"));
    }

    #[test]
    fn the_sweep_above_the_export_is_a_sweep_of_the_wrong_directory() {
        let root = tree("above-the-export");
        rewrite(
            &root,
            "nix/cargo-env.nix",
            "    export CARGO_TARGET_DIR=\"$warmTarget\"\n    ${builtins.readFile ./purge-baked-out-dirs.sh}\n",
            "    ${builtins.readFile ./purge-baked-out-dirs.sh}\n    export CARGO_TARGET_DIR=\"$warmTarget\"\n",
        );
        let why = over(&root).expect_err("above the export it sweeps the developer's target");
        assert!(why.contains("goes AFTER the export"), "{why}");
    }

    #[test]
    fn a_sweep_reading_another_variable_is_not_saved_by_its_position() {
        let root = tree_sweeping("drifted-variable", "SUTURA_TARGET_DIR");
        let why = over(&root).expect_err("the sweep and the export have to name one variable");
        assert!(why.contains("$SUTURA_TARGET_DIR"), "{why}");
    }

    #[test]
    fn an_empty_taking_set_fails_rather_than_passing_over_silence() {
        // The empty-scan defect, in both of the shapes this gate can reach it in.
        let root = tree("no-takings");
        rewrite(&root, "flake.nix", "    cargoArtifacts = artifacts;\n", "");
        let why = over(&root).expect_err("a constructor that binds no artifacts pairs nothing");
        assert!(why.contains("binds no `cargoArtifacts`"), "{why}");

        let discovered = super::Discovered(Vec::new());
        let why = super::Swept::over(&discovered, Vec::new()).expect_err("no takings is not a pass");
        assert!(why.contains("checked NOTHING"), "{why}");
    }

    #[test]
    fn inspecting_fewer_takings_than_were_discovered_cannot_be_minted() {
        // MUTATION FOUR: the count in the verdict has to be the witness. `.take(1)` over the
        // adjudication loop is the whole mutation, and it must not be able to print `ok`.
        let root = tree("subset");
        let mut files = Vec::new();
        for rel in ["flake.nix", "nix/cargo-env.nix", "nix/mimalloc.nix"] {
            files.push(super::NixFile::read(&root, rel).expect("the fixture"));
        }
        let owner = super::pairing(&root, &files).expect("the constructor");
        let discovered = super::discover(&files, &owner);
        assert_eq!(discovered.0.len(), 2);
        let subset = discovered
            .0
            .iter()
            .take(1)
            .map(|taking| super::adjudicate(&root, &files, taking, &owner).expect("the first taking"))
            .collect();
        let why = super::Swept::over(&discovered, subset).expect_err("one of two is not every one");
        assert!(why.contains("inspected 1 of 2"), "{why}");
    }

    #[test]
    fn the_real_tree_is_still_shaped_the_way_this_gate_reads_it() {
        // The floor every reader here needs: a scan that matches nothing on the real tree makes
        // its gate pass over the thing it describes. Whether the tree PAIRS is the verdict; this
        // asserts the anchors are live, and that the file set includes the one that declares them.
        let root = crate::repo::root().expect("the repo root");
        let files = super::nix_files(&root).expect("the tree's nix files");
        assert!(files.len() >= 2, "{} nix file(s)", files.len());
        let owner = super::pairing(&root, &files).expect("the constructor still pairs");
        assert_eq!(owner.rel, "flake.nix");
        let discovered = super::discover(&files, &owner);
        assert!(!discovered.0.is_empty(), "no taking found in the real tree");
        for taking in &discovered.0 {
            let how = super::adjudicate(&root, &files, taking, &owner);
            assert!(how.is_ok(), "{how:?}");
        }
    }
}
