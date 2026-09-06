//! Every registered data system is held to the conformance packs, or is declared unbound.
//!
//! **The hole this closes, and it was reproducible on `main`:** delete
//! `crates/sutura-exec-duckdb/tests/conformance.rs` and `just validate` is green. The packs are
//! bound where an adapter's own crate binds them - which is the property `telekom/sutura#116`
//! bought - so *which adapters are held* was a reading of which crates happen to carry such a
//! file, and nothing failed when one did not. The only signal was seven fewer tests in a run
//! nobody diffs. `AGENTS.md` calls a golden suite one of this repository's guarantees, and it
//! calls an invariant held by recall a wish.
//!
//! `docs/adr/0012`'s consequence says *one registration, not two*, and as built it is two: a data
//! system is named in the golden matrix's registry and bound again in its own crate, with nothing
//! relating the lists. This gate is what relates them.
//!
//! # The registry side is DERIVED, and why that source is the authority
//!
//! A hand-written list of adapters would be a second thing to keep true, and it rots first - so
//! the set is read out of the tree. Three candidates existed and two are wrong:
//!
//! | Candidate | Why not |
//! | --- | --- |
//! | `Warehouse` implementors | 27 in this workspace, and most are FAKES. A text scan cannot tell `RecordingWarehouse` from a deployable adapter, and the registry's own header says the fakes are deliberately unregistered because *a cell that cannot fail reads as coverage* |
//! | crate membership (`crates/sutura-exec-*`) | Counts a crate that implements the port and is not claimed as a registered data system - `sutura-exec-bigquery` is exactly that today, so this source would demand an exemption for a crate nothing has registered |
//! | **the `data_systems` registry arm** | **Chosen.** It is the declaration of what the golden matrix is a matrix OVER, `docs/adr/0012` says CI's own job matrix is emitted from it, and it is keyed by the same name a pack's emitted test carries |
//!
//! So the authority is the arm, read by [`scan::cells`], and the crate each entry belongs to is
//! derived from the first path segment of the adapter type the entry names - not from a name
//! written here. Both ends of the comparison move with the tree.
//!
//! # The exemption list is EMPTY as of `telekom/sutura#348`
//!
//! This gate shipped with one declared exemption - `postgres`, registered in the golden matrix and
//! carrying no binding because `sutura_conformance` had no way to say *no tier is up here* that was
//! not a pass. It has one now (`sutura_conformance::Fixture`), the adapter binds the packs from its
//! own crate, and the entry is deleted rather than reworded: an exemption for something that is
//! bound after all is itself a failure here, so it could not have been left behind. The mechanism
//! stays with the list empty, because the alternative to a declared exemption is an EXCLUSION,
//! which hides an entry instead of classifying it.
//!
//! # This is also the gate `telekom/sutura#135` needs
//!
//! That issue wants CI's job matrix intersected with path-filter categories, keyed by the registry
//! name, and the packs already give each behaviour a selectable name per adapter -
//! `-E 'test(conformance::duckdb)'` selects one adapter's tier and `-E 'binary(conformance)'`
//! selects every one. **Nothing enforced the two properties those expressions rest on**, and the
//! macro's own documentation said so: the file name, and the wrapper module. Both are checked here,
//! per entry:
//!
//! * the binding is written in `<the entry's crate>/tests/conformance.rs`, which is what makes
//!   `binary(conformance)` name every adapter's tier - and, since a fake in the harness crate may
//!   not take that file name either, nothing else under `crates/`;
//! * it sits in a module path of exactly `conformance`, so the name the expansion emits is
//!   `conformance::<name>::<behaviour>` and the per-adapter selector is spellable.
//!
//! The selector is COMPUTED from where the invocation sits ([`scan::Invocation::selector`]) and
//! printed on green, so the position a matrix generated from the registry keys on is checked
//! rather than assumed. **What is compared is a written INVOCATION and its position, never an
//! emitted test name** - the next section is what that costs and what closes it. Worth recording
//! beside it: `.config/nextest.toml` already spells `binary(conformance) + binary(bound)` and
//! nextest exits 96 (`no binary names matched this`) when a name matches nothing, so the binary's
//! EXISTENCE was already held; what had no mechanism is that every registered adapter's binding is
//! in its own crate's file, in that module.
//!
//! # The evidence is a WRITTEN INVOCATION, and what makes that mean an emitted test
//!
//! A needle proves text. The property is about a test the compiler emits, and three text shapes
//! were measured satisfying the first while producing none of the second - each leaving `duckdb`
//! reported `bound` with its selector printed while its target emitted zero conformance cells,
//! `just hygiene` `ok` and `just lint` exit 0, with `-E 'binary(conformance)'` running 7 tests
//! instead of 14. So the needle is no longer the whole evidence:
//!
//! | Shape | Refused by |
//! | --- | --- |
//! | a one-line string literal spelling the invocation | [`scan::quoted`], which asks `code_lines`' own inverse which literals spell the needle |
//! | the invocation inside an uninvoked `macro_rules!` body | [`scan::Place::template`], recorded by the same brace walk that resolves a module path |
//! | any `cfg` other than `cfg(test)` enclosing it - `cfg(all(test, any()))` strips the module and leaves every needle readable | [`scan::Place::cfg`], from the attribute run above each open block and above the line |
//! | a crate whose manifest turns off test autodiscovery, so the file is no target at all | [`manifest_dir`] |
//!
//! Each is a refusal naming the file and line rather than a binding this gate counts, which is the
//! direction the registry side already failed in: a literal shaped like the registry arm makes
//! [`one`] refuse, and a literal shaped like a binding used to satisfy an entry. **The strong
//! form, that the test EXISTS, still needs a run's machine-readable output** - the same deferred
//! mechanism `telekom/sutura#353`'s half names for per-pack aggregation, and `docs/adr/0012`
//! records it as deferred rather than claimed here.
//!
//! # Fails closed, and in which directions
//!
//! Each is a failure rather than a pass over silence, because a scan that finds nothing is how the
//! property went unheld in the first place. No count is given here on purpose: the four refusals
//! in the section above belong to this list too, and a total in a heading is the second thing to
//! keep true.
//!
//! * **No `.rs` under `crates/` read.** The tree moved out from under the gate.
//! * **A file in scope it cannot read.** Not skipped: a file this gate did not read is a file it
//!   did not judge, and skipping one is how a scan comes out clean over the violation.
//! * **No registry, or two.** The comparison has no left-hand side, or two candidates for it.
//! * **A registry that declares no entry** - an empty adapter set would make every check below
//!   vacuous, which is the shape of *"ok, 0 of 0"*. Refused TWICE, and the honest note is which
//!   one fires: [`scan::cells`]', measured by emptying the arm. [`Reconciled::of`]'s is the
//!   second and is unreachable while the parse refuses first - it is kept because it belongs to
//!   the WITNESS rather than to the parse, so a future caller assembling entries another way
//!   cannot get a verdict either.
//! * **A cell whose shape cannot be read**, and a binding whose `adapter:` cannot be NAMED. A
//!   declaration this gate cannot parse is an error, never an entry it passes over.
//! * **No definition of the packs macro, or two.** The harness crate is discovered by it, and a
//!   discovery that failed would silently reclassify the packs' own fakes.
//! * **Fewer decisions than entries.** [`Reconciled::of`] refuses a verdict that did not judge
//!   every entry it found, so the printed count is a witness rather than a number in a message.
//! * **A binding the split dropped.** One conservation law per level: `matched + strays` is the
//!   number of bindings found, or [`reconcile::Classified::of`] refuses. A second binding for one
//!   registered name used to overwrite the first, and *which* verdict came out was decided by the
//!   file listing's sort order - a silent green from `tests/aaa.rs`, a failure naming the wrong
//!   file from `tests/zz.rs`. Both are now one message naming both files.
//! * **A fake in the harness crate written in the binding FILE.** The one placement rule that
//!   crate shares, because `binary(conformance)` would otherwise select its fake cells alongside
//!   every adapter's tier.
//! * **An exemption that is inert** - naming an entry the registry does not carry, naming the
//!   wrong crate for one it does, or excusing an entry that turns out to be bound. Reported
//!   ALONGSIDE the violations rather than instead of them, which is the defect `max-lines`'
//!   inert-exemption block shipped (`telekom/sutura#311`): a gate that knows two numbers and
//!   prints one costs a round trip.
//!
//! # Measured, by mutation, on 2026-09-06
//!
//! A gate is worth what breaking it proves, so each of these was run and its verdict read. The
//! venue is `just hygiene`, which is this gate's own; the `TASKS` row is `just check-changed`.
//!
//! | Mutation | Verdict |
//! | --- | --- |
//! | delete the `duckdb` binding from the index | FAILED, naming it: *`duckdb` is registered as a data system ... and no crate binds it* |
//! | delete this gate's `TASKS` entry | exit 101, **61** errors under `-D dead-code` - `CRATES`, `BINDING_FILE`, `Entry`, `Binding`, `Site`, `Report`, `run`, `judge`, `collect`, `render`, `explain` and the rest, in both modules |
//! | empty the `data_systems` arm | FAILED: *the `data_systems` arm declares no `$cell!(..)` entry* |
//! | `.take(1)` on the decision loop | FAILED: *3 registered data system(s) were found and 1 judged* |
//! | declare `duckdb` (bound) and `nobody` (unregistered) unbound | FAILED, both named in ONE run |
//! | move `tests/conformance.rs` to `tests/packs.rs` | FAILED, naming the path a `binary()` filter needs |
//!
//! **The second round, and it is the one that mattered.** Every mutation below was GREEN before
//! this change. The first three were green everywhere - `just hygiene: ok`, `just lint` exit 0,
//! with the duckdb tier simply absent from the run - which is why the section above exists:
//!
//! | Mutation | Verdict |
//! | --- | --- |
//! | `#[cfg(all(test, any()))]` on `mod conformance` | FAILED: *sits under `cfg(all(test,any()))`, and only `cfg(test)` - or no `cfg` at all - is accepted here* |
//! | the invocation replaced by a **used** one-line string literal spelling it | FAILED: *a string literal spells `execute_packs!` ... a quoted declaration is not a declaration* |
//! | the invocation moved inside an uninvoked `macro_rules!`, same file and module | FAILED: *written inside a `macro_rules!` body, which is a TEMPLATE* |
//! | `autotests = false` in the adapter's manifest | FAILED: *turns off test autodiscovery, so a `tests/conformance.rs` in that crate is not a target Cargo builds* |
//! | a second binding at `tests/aaa.rs`, which sorts FIRST | FAILED, two problems in one run: `aaa.rs` is not where a `binary()` filter reaches it, and *`duckdb` is bound twice - ...aaa.rs:47 and ...conformance.rs:47* |
//! | the same file at `tests/zz.rs`, which sorts LAST | FAILED, naming BOTH files - the earlier version named `zz.rs` as the sole binding, and the `aaa.rs` direction was a silent green whose only tell was the file count moving 239 to 240 |
//! | a third fake at `crates/sutura-conformance/tests/conformance.rs` | FAILED: *a fake in the crate that DEFINES the packs, written in tests/conformance.rs* |
//!
//! # The limits, next to the claim
//!
//! **It holds that a registered data system HAS a binding, not that the binding covers anything.**
//! What each pack asserts is `sutura-conformance`'s own business, and
//! `crates/sutura-conformance/src/lib.rs` carries the four mechanisms that keep a behaviour from
//! being a silent pass - including the one this gate cannot reach: a pack whose body returns `Ok`
//! unconditionally passes every census and every binding.
//!
//! **It reads the `data_systems` arm alone.** The registry's `catalogs` arm has no pack to be
//! bound to - compile packs are `docs/adr/0012`'s next piece and are not built - so nothing is
//! compared for a catalog adapter, and a metadata adapter is under no obligation this gate can
//! see. It grows a second arm the day those packs exist.
//!
//! **What is invisible is the STRAY direction, not the registered one.** A registered adapter
//! whose type resolves outside `crates/` fails CLOSED at [`resolve`], with the derivation printed -
//! measured with `crate::adapters::DuckDbWarehouse` as the cell's type. What nothing here sees is a
//! *binding* written in a crate outside `crates/`: it is compared to nothing and reported as
//! nothing.
//!
//! **A wrapper macro is two shapes, and this gate used to hold only one of them.** Defined in
//! another file and invoked in the binding file, it fails closed and names the adapter - the needle
//! is the packs macro's own name and the invocation does not spell it. Defined INSIDE the binding
//! file and never invoked, it satisfied the needle, and the mechanism that caught that shape was
//! `unused_macros` under `-D warnings` rather than anything here. It is now refused by
//! [`scan::Place::template`], which is the first time this gate can be credited with it.
//!
//! Both directions are the price of reading text rather than a compiled artefact, which is not
//! available - see [`scan`]'s header for why.
//!
//! **The derivation from a type to a crate is `_` to `-` plus a manifest that declares that
//! name.** A crate whose directory disagrees with its package name is found through the manifest;
//! a package name that is not a path segment of the adapter type fails closed with the derivation
//! printed, rather than passing.

mod reconcile;
pub(crate) mod scan;

use std::collections::BTreeMap;
use std::path::Path;

use crate::{Verdict, repo};
use reconcile::{Classified, Reconciled, Stray, UNBOUND, decide};

/// Where every crate in this workspace lives.
const CRATES: &str = "crates/";

/// The file name a binding must be written in, so `-E 'binary(conformance)'` names every tier.
const BINDING_FILE: &str = "tests/conformance.rs";

/// The module a binding must sit in, so `-E 'test(conformance::<name>)'` names one tier.
const BINDING_MODULE: &str = "conformance";

/// One registry entry, with the crate it belongs to derived from the adapter type it names.
#[derive(Debug)]
struct Entry {
    /// The cell's name, which is the key everything else is joined on.
    name: String,
    /// The adapter type, as the registry writes it.
    adapter: String,
    /// The package name derived from that type's first path segment.
    crate_name: String,
    /// That package's directory, repo-relative with a trailing `/`.
    crate_dir: String,
}

/// One invocation of the packs macro, and the file it was found in.
#[derive(Debug, Clone)]
struct Binding {
    /// Repo-relative path.
    path: String,
    /// What the scan read out of it.
    invocation: scan::Invocation,
}

/// A file holding one of the two declarations, and the dense lines it was recognised in.
///
/// The lines are CARRIED rather than re-read, so the text a cell is parsed out of is the text the
/// discovery matched. A second `read_to_string` would be a second answer about one file, which is
/// the shape `check-docs` paid for - the file a gate reads is not always the file it judged.
struct Site {
    path: String,
    dense: Vec<String>,
}

/// A verdict, one line at a time. Lines rather than one string because `clippy::format_push_string`
/// refuses the obvious accumulation and a `Vec` is what a test can read a row out of.
type Report = Vec<String>;

/// The registry's own path, and the entries it declares.
type Registry = (String, Vec<Entry>);

/// A package name and the directory declaring it.
type Declared = (String, String);

/// What one pass over the Rust under `crates/` found.
struct Sources {
    /// Files read. Zero is a failure: a gate that checked nothing is the failure mode a gate exists
    /// to prevent.
    read: usize,
    /// Every file whose code holds the registry arm, with the lines it was found in.
    registries: Vec<Site>,
    /// Every file whose code defines the packs macro.
    definitions: Vec<String>,
    /// Every invocation found under some crate's `tests/`, with the file holding it.
    bindings: Vec<Binding>,
    /// Package name to directory, from every manifest under `crates/`.
    dirs: BTreeMap<String, String>,
}

pub(crate) fn run(_args: &[String]) -> Verdict {
    let Some(repo::RepoFiles { root, files }) = repo::all_files() else {
        eprintln!("xtask check-conformance-bindings: could not determine the repo root");
        return Verdict::Fail;
    };
    match judge(&root, &files) {
        Ok(report) => {
            for line in report {
                println!("{line}");
            }
            Verdict::Pass
        }
        Err(problems) => {
            eprintln!("xtask check-conformance-bindings: FAILED - the registry and the packs disagree:");
            for problem in &problems {
                eprintln!("  {problem}");
            }
            eprintln!();
            explain();
            Verdict::Fail
        }
    }
}

/// The whole rule: read both declarations, reconcile them, and render one verdict.
fn judge(root: &Path, files: &[String]) -> Result<Report, Report> {
    let sources = collect(root, files).map_err(|why| vec![why])?;
    let (registry, entries) = read_registry(&sources).map_err(|why| vec![why])?;
    let harness = one(&sources.definitions, scan::PACKS_MACRO)
        .and_then(|path| owner(&path).ok_or_else(|| format!("{path} is not inside a crate")))
        .map_err(|why| vec![why])?;

    let classified = Classified::of(&entries, &harness, &sources.bindings).map_err(|why| vec![why])?;
    let decided = entries.iter().map(|entry| decide(entry, classified.matched())).collect();
    let reconciled = Reconciled::of(entries, decided).map_err(|why| vec![why])?;

    let problems = reconciled.problems(classified.strays());
    if problems.is_empty() {
        Ok(render(&reconciled, &registry, &harness, &sources, classified.strays()))
    } else {
        Err(problems)
    }
}

/// One pass over every `.rs` file under `crates/`, plus every manifest there.
fn collect(root: &Path, files: &[String]) -> Result<Sources, String> {
    let mut found = Sources {
        read: 0,
        registries: Vec::new(),
        definitions: Vec::new(),
        bindings: Vec::new(),
        dirs: BTreeMap::new(),
    };
    for rel in files {
        if let Some(dir) = manifest_dir(root, rel)? {
            drop(found.dirs.insert(dir.0, dir.1));
            continue;
        }
        if !is_crate_rust(rel) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(root.join(rel)) else {
            // Not skipped. A file in scope this gate cannot read is a file it did not judge.
            return Err(format!("could not read {rel}, which is in scope"));
        };
        found.read = found.read.saturating_add(1);
        inspect(rel, &text, &mut found)?;
    }
    if found.read == 0 {
        return Err(format!(
            "no Rust under {CRATES} was read, so this gate checked nothing - either the workspace \
             moved or the listing it reads is empty"
        ));
    }
    if found.dirs.is_empty() {
        return Err(format!("no manifest under {CRATES} declares a package name"));
    }
    Ok(found)
}

/// One file's contribution to the scan.
fn inspect(rel: &str, text: &str, found: &mut Sources) -> Result<(), String> {
    let (code, dense) = scan::lexed(text);
    if !scan::sites(&dense, scan::REGISTRY_ARM).is_empty() {
        found.registries.push(Site {
            path: rel.to_owned(),
            dense: dense.clone(),
        });
    }
    if !scan::sites(&dense, scan::PACKS_MACRO).is_empty() {
        found.definitions.push(rel.to_owned());
        // The definition's own recursive arms name the macro; nothing there is a binding.
        return Ok(());
    }
    let sites = scan::sites(&dense, scan::BINDING);
    if sites.is_empty() || !is_test_target(rel) {
        return Ok(());
    }
    // A needle inside a one-line string literal is live in the dense form and satisfies nothing
    // the compiler emits, so it is refused before anything is parsed rather than counted as a
    // binding - the fail-open half of the limit `scan`'s header declares.
    if let Some(line) = scan::quoted(text, scan::BINDING).first() {
        return Err(format!(
            "{rel}: line {line}: a string literal spells `{}`, and a needle is the whole evidence \
             this gate has - a quoted declaration is not a declaration",
            scan::BINDING
        ));
    }
    let paths = scan::module_paths(&code).map_err(|why| format!("{rel}: {why}"))?;
    for line in sites {
        let invocation = scan::invocation(&dense, &paths, line).map_err(|why| format!("{rel}: {why}"))?;
        found.bindings.push(Binding {
            path: rel.to_owned(),
            invocation,
        });
    }
    Ok(())
}

/// The registry file, and the entries it declares with each one's crate resolved.
fn read_registry(sources: &Sources) -> Result<Registry, String> {
    let paths: Vec<String> = sources.registries.iter().map(|site| site.path.clone()).collect();
    let registry = one(&paths, scan::REGISTRY_ARM)?;
    let site = sources
        .registries
        .iter()
        .find(|site| site.path == registry)
        .ok_or_else(|| format!("{registry} was found and then lost"))?;
    let line = *scan::sites(&site.dense, scan::REGISTRY_ARM)
        .first()
        .ok_or_else(|| format!("{registry} no longer holds the registry arm"))?;
    let cells = scan::cells(&site.dense, line).map_err(|why| format!("{registry}: {why}"))?;
    let entries = cells
        .into_iter()
        .map(|cell| resolve(&cell, &sources.dirs))
        .collect::<Result<Vec<Entry>, String>>()?;
    Ok((registry, entries))
}

/// One registry cell, with its crate derived from the adapter type it names.
fn resolve(cell: &scan::Cell, dirs: &BTreeMap<String, String>) -> Result<Entry, String> {
    let segment = cell.adapter.split("::").next().unwrap_or_default();
    let crate_name = segment.replace('_', "-");
    let crate_dir = dirs.get(&crate_name).ok_or_else(|| {
        format!(
            "the registry names `{}` for `{}`, whose first path segment derives the package \
             `{crate_name}`, and no manifest under {CRATES} declares it",
            cell.adapter, cell.name
        )
    })?;
    Ok(Entry {
        name: cell.name.clone(),
        adapter: cell.adapter.clone(),
        crate_name,
        crate_dir: crate_dir.clone(),
    })
}

/// The green verdict: the counts, and every decision that produced one.
///
/// Every number here is read off [`Reconciled`], whose construction refused anything but one
/// decision per entry - so `judged` cannot exceed what was found and `bound + exempt` cannot
/// exceed `judged`.
fn render(reconciled: &Reconciled, registry: &str, harness: &str, sources: &Sources, strays: &[Stray]) -> Report {
    let bound: Vec<_> = reconciled.bound().collect();
    let exempt: Vec<_> = reconciled.exempt().collect();
    let fakes = strays.iter().filter(|stray| matches!(stray, Stray::Fake(_))).count();
    let mut out = vec![format!(
        "xtask check-conformance-bindings: ok - {} registered data system(s) in {registry}, {} judged: \
         {} bound, {} declared unbound ({} file(s) of Rust under {CRATES} read; the packs macro is \
         defined in {harness}, whose own {fakes} binding(s) are fakes)",
        reconciled.judged.len(),
        reconciled.judged.len(),
        bound.len(),
        exempt.len(),
        sources.read,
    )];
    for (entry, binding) in bound {
        out.push(format!(
            "  bound    {} - {}:{}, selected by `-E 'test({})'`",
            entry.name,
            binding.path,
            binding.invocation.line,
            binding.invocation.selector()
        ));
    }
    // Named on GREEN, the convention `check-boundaries` and `check-bounded-wait` state for the same
    // reason: an allowance a green run never mentions is one nobody re-reads.
    for (entry, allowance) in exempt {
        out.push(format!(
            "  unbound  {} - {}: {}",
            entry.name, allowance.crate_name, allowance.what
        ));
    }
    // The harness's own bindings, PRINTED for the reason the exemptions are: this gate treats them
    // as fakes rather than as data systems, and a classification a green run never mentions is one
    // nobody re-reads. A fake that moved to another crate becomes an `Unregistered` violation, and
    // this is the line that says where they were when the run was green.
    for stray in strays {
        if let Stray::Fake(binding) = stray {
            out.push(format!(
                "  fake     {} - {}:{}",
                binding.invocation.adapter, binding.path, binding.invocation.line
            ));
        }
    }
    out
}

/// Printed on failure. A gate that only says no gets worked around.
fn explain() {
    eprintln!("A pack is bound where an adapter's own crate binds it, which is what lets a data");
    eprintln!("system prove itself by registering rather than by anybody editing a test. The cost of");
    eprintln!("that shape is that WHICH adapters are held used to be a reading of which crates carry");
    eprintln!("a tests/conformance.rs - so deleting one left `just validate` green, with seven fewer");
    eprintln!("tests in a run nobody diffs as the only signal.");
    eprintln!();
    eprintln!("So the fix is one of exactly two things, and both are visible in a diff:");
    eprintln!("  1. bind the adapter from its own crate - `sutura_conformance::execute_packs!` in");
    eprintln!("     <the crate>/{BINDING_FILE}, wrapped in `mod {BINDING_MODULE}`; or");
    eprintln!("  2. declare it unbound in UNBOUND in xtask/src/conformance/reconcile.rs, with what");
    eprintln!("     and why.");
    eprintln!();
    eprintln!("The second is an architecture decision: `docs/adr/0012` says one registration, not");
    eprintln!("two, and an exemption is that consequence going unmet. Today:");
    if UNBOUND.is_empty() {
        // Said out loud rather than left as an empty list, because *nothing is exempt* is the
        // state this gate was built to reach and a reader arriving at a failure needs to know
        // that the first option above is the only one anybody has taken.
        eprintln!("  nothing is declared unbound, so option 1 is what every registered adapter did.");
    }
    for allowance in UNBOUND {
        eprintln!("  {} in {} - {}", allowance.name, allowance.crate_name, allowance.what);
        eprintln!("    Why: {}", allowance.why);
    }
}

/// Exactly one candidate, or a message saying which way the discovery failed.
fn one(found: &[String], needle: &str) -> Result<String, String> {
    match found {
        [only] => Ok(only.clone()),
        [] => Err(format!(
            "no file under {CRATES} holds `{needle}` - the declaration this gate reads has moved or \
             changed shape, and a scan that finds neither side compares nothing"
        )),
        many => Err(format!(
            "{} files under {CRATES} hold `{needle}`, and one declaration is the point: {}",
            many.len(),
            many.join(", ")
        )),
    }
}

/// The package name and directory a `crates/<dir>/Cargo.toml` declares, if this path is one.
fn manifest_dir(root: &Path, rel: &str) -> Result<Option<Declared>, String> {
    let Some(dir) = rel.strip_prefix(CRATES).and_then(|rest| rest.strip_suffix("/Cargo.toml")) else {
        return Ok(None);
    };
    if dir.contains('/') {
        return Ok(None);
    }
    let text = std::fs::read_to_string(root.join(rel)).map_err(|e| format!("could not read {rel}: {e}"))?;
    // The manifest half of *is this test EMITTED*. Autodiscovery is what makes a file under
    // `tests/` a target at all, so a crate that turns it off leaves every binding this gate reads
    // there in a file nothing compiles - the same class as the three text shapes `scan` refuses,
    // one level further out. No manifest in this workspace sets it, so this arm starts green, and
    // a crate that needs it argues in the diff the way a declared exemption does.
    if text
        .lines()
        .map(str::trim)
        .any(|line| !line.starts_with('#') && scan::dense(line) == "autotests=false")
    {
        return Err(format!(
            "{rel} turns off test autodiscovery, so a `{BINDING_FILE}` in that crate is not a \
             target Cargo builds - a binding this gate could read and nothing would run"
        ));
    }
    let name = text
        .lines()
        .map(str::trim)
        .find_map(|line| line.strip_prefix("name"))
        .and_then(|rest| rest.trim_start().strip_prefix('='))
        .map(|rest| rest.trim().trim_matches('"').to_owned())
        .ok_or_else(|| format!("{rel} declares no package name"))?;
    Ok(Some((name, format!("{CRATES}{dir}/"))))
}

/// The crate directory a repo-relative path is inside, with its trailing `/`.
fn owner(rel: &str) -> Option<String> {
    let rest = rel.strip_prefix(CRATES)?;
    let dir = rest.split('/').next()?;
    Some(format!("{CRATES}{dir}/"))
}

/// Is this a Rust file inside a crate?
fn is_crate_rust(rel: &str) -> bool {
    rel.starts_with(CRATES)
        && Path::new(rel)
            .extension()
            .and_then(std::ffi::OsStr::to_str)
            .is_some_and(|ext| ext.eq_ignore_ascii_case("rs"))
}

/// Is this file an integration test target of its crate?
///
/// Bindings are looked for here and nowhere else, which is what keeps the packs crate's own
/// recursive arms and every doc example out of the binding set.
fn is_test_target(rel: &str) -> bool {
    owner(rel).is_some_and(|dir| rel.starts_with(&format!("{dir}tests/")))
}

#[cfg(test)]
mod tests {
    use super::{owner, repo};

    /// **Every fixture in `reconcile` and `scan` is hand-written, and this is what stops them
    /// describing a shape the tree no longer has.** The failure `check-docs` paid for was a gate
    /// re-implementing part of a tool and being tested against the documentation rather than
    /// against the tool; the same shape here is a needle that stopped matching the real
    /// declarations, which would leave every fixture green. So this reads THIS tree through the
    /// same functions the gate uses and asserts
    /// both declarations were found: one registry, one packs macro, and bindings under `tests/`.
    #[test]
    fn the_declarations_in_this_tree_are_found_by_the_needles_this_gate_keys_on() {
        let repo::RepoFiles { root, files } = repo::all_files().expect("the tests run inside the repo");
        let sources = super::collect(&root, &files).expect("this tree is in scope");
        let found: Vec<&str> = sources.registries.iter().map(|site| site.path.as_str()).collect();
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(sources.definitions.len(), 1, "{:?}", sources.definitions);
        assert!(!sources.bindings.is_empty(), "no binding found in this tree");
        // And the registry's own entries resolve to crates that exist, which is the half a
        // fixture cannot establish: the derivation from an adapter TYPE to a package name.
        let (_, entries) = super::read_registry(&sources).expect("the registry resolves");
        assert!(entries.len() >= 2, "{entries:?}");
        for entry in &entries {
            assert!(
                root.join(&entry.crate_dir).join("Cargo.toml").is_file(),
                "{} derives {}, which is not a crate",
                entry.name,
                entry.crate_dir
            );
        }
    }

    #[test]
    fn a_crate_owns_the_paths_beneath_it() {
        assert_eq!(
            owner("crates/sutura-exec-duckdb/tests/conformance.rs").as_deref(),
            Some("crates/sutura-exec-duckdb/")
        );
        assert_eq!(owner("xtask/src/conformance.rs"), None);
    }
}
