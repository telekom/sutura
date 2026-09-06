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
//! [`TAKING`] argument name - not a line, not a file, and not an occurrence of the word `purge`.
//! Every taking in every `.nix` file in the tree is discovered, each is attributed to whatever
//! RECEIVES it, and [`Swept`] cannot be minted with fewer adjudications than the scan discovered:
//! the count in the verdict comes off the witness rather than out of a `format!`.
//!
//! WHAT PAIRS A TAKING is one of exactly two things, both resolved out of the tree:
//!
//! 1. It is the constructor's own binding, and the constructor's attrset still inlines the sweep.
//!    Since that is the only [`PHASE`] binding permitted anywhere, no consumer can displace it.
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
//! path without naming [`TAKING`] at all. It says nothing about whether the sweep WORKS; that is
//! the script's own subject, and the tests below run it over a real directory rather than reading
//! its source. And it is blind to the profile by design: the sweep derives its profile directory
//! from cargo's own `root-output` record, so it names none and cannot clean the wrong one.

use std::path::{Path, PathBuf};

use crate::{repo, workflows};

/// The sweep, repo-relative. Every taking is paired with THIS file or with nothing.
const SWEEP: &str = "nix/purge-baked-out-dirs.sh";

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

/// One `.nix` file, in both of the views this gate needs.
struct NixFile {
    /// Repo-relative, with `/` separators - what a reader can open.
    rel: String,
    /// The file as written. The shell inside an indented string lives here and nowhere else.
    raw: String,
    /// The nix CODE half, comments and string interiors blanked by [`workflows::nix_code_lines`],
    /// interpolations kept. Lines are joined back up so an offset in it has a line number.
    code: String,
}

/// Which view answers which question, stated once because the split is deliberate.
///
/// A nix BINDING - `cargoArtifacts = ..`, `preBuild = ..` - is read off [`NixFile::code`], because
/// this file's own header comments discuss both names in prose and `nix/mimalloc.nix` writes
/// `runHook preBuild` inside a builder script: a raw scan reports all three. A SHELL line inside
/// an indented string is read off [`NixFile::raw`] through [`super::live_lines`], because the
/// lexer blanks exactly that text - so the warmer's `export` is invisible in the code half, and
/// the rule `live_lines` states (a line that STARTS with `#` runs nothing) is the right one for a
/// string that is shell either way.
impl NixFile {
    fn read(root: &Path, rel: &str) -> Result<Self, String> {
        let raw = std::fs::read_to_string(root.join(rel)).map_err(|error| format!("could not read {rel}: {error}"))?;
        let code = workflows::nix_code_lines(&raw).join("\n");
        Ok(Self {
            rel: String::from(rel),
            raw,
            code,
        })
    }
}

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

/// Every `.nix` file in the tree, or a failure naming what it could not enumerate.
fn nix_files(root: &Path) -> Result<Vec<NixFile>, String> {
    let listing = repo::all_files().ok_or_else(|| String::from("could not enumerate the repo's files"))?;
    let mut files = Vec::new();
    let nix = std::ffi::OsStr::new("nix");
    for rel in listing.files.iter().filter(|rel| Path::new(rel).extension() == Some(nix)) {
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

/// Is this the whole identifier, rather than the tail of a longer one?
fn whole_word(code: &str, start: usize, end: usize) -> bool {
    let word = |c: char| c.is_alphanumeric() || matches!(c, '_' | '-' | '\'');
    let before = code.get(..start).and_then(|head| head.chars().next_back());
    let after = code.get(end..).and_then(|tail| tail.chars().next());
    !before.is_some_and(word) && !after.is_some_and(word)
}

/// Every byte offset in `code` at which `name` is BOUND - the identifier followed by an `=`.
///
/// A binding and a use are different facts and this gate is about bindings: `${cargoArtifacts}`
/// inside the warmer's shell is a use, `{ pkgs, cargoArtifacts, .. }:` is a parameter, and
/// `inheritCargoArtifacts` is crane's own function. None of the three hands artifacts to anything.
fn bound_at(code: &str, name: &str) -> Vec<usize> {
    let mut found = Vec::new();
    let mut from = 0_usize;
    while let Some(offset) = code.get(from..).and_then(|rest| rest.find(name)) {
        let start = from.saturating_add(offset);
        let end = start.saturating_add(name.len());
        from = end;
        if !whole_word(code, start, end) {
            continue;
        }
        let tail = code.get(end..).unwrap_or_default().trim_start();
        if tail.starts_with('=') && !tail.starts_with("==") {
            found.push(start);
        }
    }
    found
}

/// The 1-based line an offset in the code half sits on.
fn line_at(code: &str, offset: usize) -> usize {
    code.get(..offset).unwrap_or_default().matches('\n').count().saturating_add(1)
}

/// The first balanced `{ .. }` at or after `from`, as a byte range.
///
/// An unclosed brace is `None` and therefore an ERROR at the caller, never an answer: this file's
/// neighbours record three gates that counted braces and reported a parse failure as a verdict.
fn attrset_at(code: &str, from: usize) -> Option<(usize, usize)> {
    let open = code.get(from..)?.find('{')?.saturating_add(from);
    let mut depth = 0_usize;
    for (offset, character) in code.get(open..)?.char_indices() {
        match character {
            '{' => depth = depth.saturating_add(1),
            '}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some((open, open.saturating_add(offset).saturating_add(1)));
                }
            }
            _ => {}
        }
    }
    None
}

/// The `{` of the innermost attrset enclosing `offset`.
fn enclosing_attrset(code: &str, offset: usize) -> Option<usize> {
    let mut pending = 0_usize;
    for (at, character) in code.get(..offset)?.char_indices().rev() {
        match character {
            '}' => pending = pending.saturating_add(1),
            '{' if pending == 0 => return Some(at),
            '{' => pending = pending.saturating_sub(1),
            _ => {}
        }
    }
    None
}

/// The path of the module whose argument set opens at `brace`, if that is what this is.
///
/// `inherit (import ./nix/cargo-env.nix {` is the shape in the tree; the leading `(` is trimmed so
/// the two tokens compared are `import` and a relative path.
fn imported_at(code: &str, brace: usize) -> Option<&str> {
    let prefix = code.get(..brace)?.trim_end();
    let mut words = prefix.split_whitespace().rev();
    let path = words.next()?;
    if !path.starts_with("./") {
        return None;
    }
    if words.next()?.trim_start_matches('(') != "import" {
        return None;
    }
    Some(path)
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
        return Err(format!(
            "{}:{line}: `{CONSTRUCTOR}` binds `{PHASE}` to something that is not {SWEEP}, so every taking it pairs \
             now inherits a build root that names nothing: {}",
            file.rel,
            value.trim()
        ));
    }
    Ok(Pairing {
        rel: file.rel.clone(),
        line,
        attrset,
    })
}

/// The sweep is the ONLY [`PHASE`] anywhere, which is what closes the shallow-update hole.
///
/// `args // inheritedArtifacts a // { preBuild = ..; }` keeps the artifacts and loses the sweep,
/// silently, and no count of takings notices - the taking is still paired, by an attrset whose
/// value was replaced afterwards. So the rule is about the PHASE rather than about the update: a
/// `preBuild` this tree does not own is refused wherever it appears, and a `preBuild` needed for
/// another reason composes inside the constructor, where it runs beside the sweep.
fn phase_has_one_owner(files: &[NixFile], owner: &Pairing) -> Result<(), String> {
    let mut elsewhere = Vec::new();
    let mut pairings = 0_usize;
    for file in files {
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
            "`{PHASE}` is bound outside `{CONSTRUCTOR}` at {}. `//` updates one level deep, so a phase bound after the \
             pairing REPLACES the sweep and keeps the artifacts - no error, nothing red, and a build root that names \
             nothing. Compose it in `{CONSTRUCTOR}` ({}:{}) instead",
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
fn module_sweeps(root: &Path, received: &NixFile) -> Result<String, String> {
    let NixFile {
        rel: module, raw: text, ..
    } = received;
    let at = super::live_indexed(text)
        .find(|(_, line)| inlines_sweep(root, module, line))
        .map(|(index, _)| index)
        .ok_or_else(|| {
            format!(
                "{module} receives the artifacts and inlines no `{INLINE} <{SWEEP}>`, so whatever it hands them to \
                 builds against a build root that names nothing. Every consumer of that module inherits the gap"
            )
        })?;

    // A module that exports no target directory is not a shell warmer, so the two rules below -
    // both about a shell's own ordering - have nothing to be about. That is `exported_value`'s
    // answer rather than a list of module names this gate would have to keep current.
    if super::exported_value(text).is_none() {
        return Ok(format!("inlines the sweep at {module}:{}", at.saturating_add(1)));
    }
    let export = super::live_indexed(text)
        .find(|(_, line)| line.trim_start().starts_with(super::EXPORT))
        .map(|(index, _)| index)
        .ok_or_else(|| format!("{module} exports a target directory this gate then could not find the line of"))?;
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

/// The variable [`super::EXPORT`] exports, derived from that one literal rather than spelled again.
fn target_var() -> &'static str {
    super::EXPORT.trim_start_matches("export ").trim_end_matches(['=', '"'])
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
        assert!(why.contains("bound outside `inheritedArtifacts`"), "{why}");
        assert!(why.contains("flake.nix:8"), "{why}");
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

    /// A synthetic unit directory: what cargo leaves behind for one build script.
    ///
    /// `ran_in` is what went into `root-output` - the absolute `$OUT_DIR` the script ran with -
    /// and `baked` is written into `out/` and into `output` separately, because whether the sweep
    /// reads the second one is a STATED LIMIT and a stated limit wants a test.
    fn unit(profile: &Path, crate_name: &str, hash: &str, ran_in: &str, baked_in_out: &str, baked_in_output: &str) {
        let dir = profile.join("build").join(format!("{crate_name}-{hash}"));
        std::fs::create_dir_all(dir.join("out")).expect("the unit directory");
        std::fs::write(dir.join("root-output"), ran_in).expect("the record");
        std::fs::write(dir.join("out/embed.rs"), baked_in_out).expect("the generated file");
        std::fs::write(dir.join("output"), baked_in_output).expect("the directives file");
        std::fs::create_dir_all(profile.join(".fingerprint").join(format!("{crate_name}-{hash}")))
            .expect("the fingerprint directory");
    }

    /// Run the real script over `target`, and hand back its output.
    ///
    /// An `Ok` from a subprocess is not evidence that a side effect happened, so every assertion
    /// below is over the FILESYSTEM and this only supplies the sentence beside it.
    fn sweep(target: &Path) -> String {
        let root = crate::repo::root().expect("the repo root");
        let out = std::process::Command::new("bash")
            .arg(root.join(super::SWEEP))
            .env("CARGO_TARGET_DIR", target)
            .current_dir(&root)
            .output()
            .expect("bash runs the sweep");
        assert!(out.status.success(), "{out:?}");
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    #[test]
    fn the_sweep_removes_what_baked_a_directory_it_no_longer_sits_in() {
        // THE SCRIPT'S OWN BEHAVIOUR, over a real directory, because nothing else in this
        // repository runs it: `just lint-workflows` shellchecks it and every venue that executes
        // it does so for its side effect inside a build. Four units, one per branch of its
        // decision, and each assertion is a `try_exists` rather than a line of its output.
        let target = std::env::temp_dir().join(format!("sutura-sweep-{}", std::process::id()));
        drop(std::fs::remove_dir_all(&target));
        let profile = target.join("ci");
        let elsewhere = "/nix/var/nix/builds/nix-74462-1743377963/source/target/ci/build/moved-aaaa/out";

        // 1. MOVED, and what it generated names the directory it ran in. The one purge.
        unit(
            &profile,
            "moved",
            "aaaa",
            elsewhere,
            &format!("#[folder = \"{elsewhere}\"]"),
            "",
        );
        // 2. MOVED, and nothing it generated names that directory. Relocation alone is true of
        //    every build script in an unpacked closure; purging on it would cost the closure.
        unit(&profile, "relocated", "bbbb", elsewhere, "pub const N: u8 = 1;", "");
        // 3. RAN WHERE IT SITS, which is every build script in an ordinary target directory.
        let own = profile.join("build/local-cccc/out");
        unit(
            &profile,
            "local",
            "cccc",
            &own.to_string_lossy(),
            &format!("#[folder = \"{}\"]", own.display()),
            "",
        );
        // 4. THE STATED LIMIT: the baked path is in `output` - cargo's record of the `cargo::`
        //    directives - which is a SIBLING of `out/` and not inside it, so the search never
        //    reads it. This asserts the limit rather than trusting the paragraph that states it.
        unit(
            &profile,
            "directives",
            "dddd",
            elsewhere,
            "pub const N: u8 = 2;",
            &format!("cargo:rustc-link-search=native={elsewhere}"),
        );

        let said = sweep(&target);

        let gone = |crate_name: &str, hash: &str| {
            let unit = profile.join("build").join(format!("{crate_name}-{hash}"));
            let print = profile.join(".fingerprint").join(format!("{crate_name}-{hash}"));
            (
                unit.try_exists().expect("the unit directory is readable"),
                print.try_exists().expect("the fingerprint is readable"),
            )
        };
        assert_eq!(
            gone("moved", "aaaa"),
            (false, false),
            "the baked unit and its fingerprint both go: {said}"
        );
        assert_eq!(
            gone("relocated", "bbbb"),
            (true, true),
            "relocation alone is not a reason: {said}"
        );
        assert_eq!(
            gone("local", "cccc"),
            (true, true),
            "a script that ran here baked nothing stale: {said}"
        );
        assert_eq!(
            gone("directives", "dddd"),
            (true, true),
            "the `output` file is the STATED LIMIT: {said}"
        );
        assert!(said.contains("1 inherited build script output(s) regenerated here"), "{said}");
        drop(std::fs::remove_dir_all(&target));
    }
}
