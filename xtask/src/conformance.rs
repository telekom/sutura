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
//! # This is also the gate `telekom/sutura#135` needs
//!
//! That issue wants CI's job matrix intersected with path-filter categories, keyed by the registry
//! name, and the packs already give each behaviour a selectable name per adapter -
//! `-E 'test(conformance::duckdb)'` selects one adapter's tier and `-E 'binary(conformance)'`
//! selects every one. **Nothing enforced the two properties those expressions rest on**, and the
//! macro's own documentation said so: the file name, and the wrapper module. Both are checked here,
//! per entry:
//!
//! * the binding is `<the entry's crate>/tests/conformance.rs`, which is what makes
//!   `binary(conformance)` name every adapter's tier and nothing else;
//! * it sits in a module path of exactly `conformance`, so the emitted test is
//!   `conformance::<name>::<behaviour>` and the per-adapter selector is spellable.
//!
//! The selector is COMPUTED from where the invocation sits ([`scan::Invocation::selector`]) and
//! printed on green, so a matrix generated from the registry and a test name emitted by the macro
//! cannot drift apart without this failing.
//!
//! # Fails closed, in eight directions
//!
//! Each is a failure rather than a pass over silence, because a scan that finds nothing is how the
//! property went unheld in the first place:
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
//! * **An exemption that is inert** - naming an entry the registry does not carry, naming the
//!   wrong crate for one it does, or excusing an entry that turns out to be bound. Reported
//!   ALONGSIDE the violations rather than instead of them, which is the defect `max-lines`'
//!   inert-exemption block shipped (`telekom/sutura#311`): a gate that knows two numbers and
//!   prints one costs a round trip.
//!
//! # Measured, by mutation, on 2026-09-06
//!
//! A gate is worth what breaking it proves, so each of these was run and its verdict read:
//!
//! | Mutation | Verdict |
//! | --- | --- |
//! | `git rm` the `duckdb` binding | FAILED, naming it: *`duckdb` is registered as a data system ... and no crate binds it* |
//! | delete this gate's `TASKS` entry | `cargo check -p xtask --all-features` exit 101 - `-D dead-code` over `run`, `judge`, `collect`, `classify`, `decide`, `render`, `explain`, `UNBOUND`, `Held`, `Stray` and the rest |
//! | empty the `data_systems` arm | FAILED: *the `data_systems` arm declares no `$cell!(..)` entry* |
//! | `.take(1)` on the decision loop | FAILED: *3 registered data system(s) were found and 1 judged* |
//! | declare `duckdb` (bound) and `nobody` (unregistered) unbound | FAILED, both named in ONE run |
//! | `git mv tests/conformance.rs tests/packs.rs` | FAILED, naming the path a `binary()` filter needs |
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
//! **A crate outside `crates/` is invisible**, and so is a binding laundered through a macro of
//! somebody else's: the needle is the packs macro's own name, and a wrapper macro expanding to it
//! would satisfy nothing here. Both are the price of reading text rather than a compiled artefact,
//! which is not available - see [`scan`]'s header for why.
//!
//! **The derivation from a type to a crate is `_` to `-` plus a manifest that declares that
//! name.** A crate whose directory disagrees with its package name is found through the manifest;
//! a package name that is not a path segment of the adapter type fails closed with the derivation
//! printed, rather than passing.

pub(crate) mod scan;

use std::collections::BTreeMap;
use std::path::Path;

use crate::{Verdict, repo};

/// Where every crate in this workspace lives.
const CRATES: &str = "crates/";

/// The file name a binding must be written in, so `-E 'binary(conformance)'` names every tier.
const BINDING_FILE: &str = "tests/conformance.rs";

/// The module a binding must sit in, so `-E 'test(conformance::<name>)'` names one tier.
const BINDING_MODULE: &str = "conformance";

/// A registered data system that is deliberately not bound, and why.
///
/// Declared rather than excluded from the comparison, for the reason `check-bounded-wait` declares
/// its one allowance: an exclusion hides an entry and a declaration classifies it - and then
/// [`inert`] makes the classification answer for itself.
#[derive(Debug)]
struct Unbound {
    /// The registry cell's name, matched exactly.
    name: &'static str,
    /// The crate that entry derives. Held as well as the name so that an entry cannot survive the
    /// adapter type moving to another crate, which is exactly when the reason below stops applying.
    crate_name: &'static str,
    /// What is registered and unbound. Printed, so a reader hitting the inert-exemption failure
    /// knows what the entry was about.
    what: &'static str,
    /// Why that is not a hole. Printed, because an exemption whose reason is unstated is
    /// indistinguishable from an oversight.
    why: &'static str,
}

/// Every registered data system this gate accepts as unbound.
///
/// **One entry, and it arrives with the gate.** `docs/adr/0012`'s *one registration, not two* is
/// violated as built, and this is the whole of the violation: the third data system in the matrix
/// is registered and carries no binding on purpose.
///
/// **Adding one is an architecture decision** - the sentence `BLOCKING` in
/// `xtask/src/bounded_wait.rs` carries for the same reason. The diff is where the argument happens,
/// and an entry that stops being true fails the gate rather than quietly widening it.
const UNBOUND: &[Unbound] = &[Unbound {
    name: "postgres",
    crate_name: "sutura-exec-postgres",
    what: "registered in the golden matrix and carrying no `tests/conformance.rs`",
    why: "its cells need a provisioned tier, and the packs have no way to say `no tier is up here` \
          that is not a pass. The golden matrix has one - `DataSystemUnderTest::available`, whose \
          false answer SKIPS a cell and whose skip-or-fail direction a provisioner decides - and \
          the packs carry no equivalent, so a binding written today would be green over a data \
          system that never answered. `telekom/sutura#348` is where the harness grows that, and \
          this entry is deleted by the same change",
}];

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

/// What the tree says about one registry entry. Two states are held and two are the defect.
#[derive(Debug)]
enum Held {
    /// Bound, in the file and module the selectors need.
    Bound(Binding),
    /// Bound, and not where a selector can reach it.
    Misbound(String),
    /// Declared unbound, by this entry.
    Exempt(&'static Unbound),
    /// Neither bound nor declared, which is the hole this gate exists for.
    Unheld,
}

/// A binding that no registry entry claims.
enum Stray {
    /// In the crate that DEFINES the packs: the harness's own fakes, which are not a data system
    /// and must not be. Printed on green rather than hidden, so a fake that moved is visible.
    Fake(Binding),
    /// In a crate the registry names no data system from - so nothing schedules it, and the CI
    /// matrix `docs/adr/0012` emits from the registry cannot see it.
    Unregistered(Binding),
    /// In a registry crate, under a name that crate's entry does not carry - so the selector the
    /// matrix would spell names no test. Carries the name the registry does declare there.
    Misnamed(Binding, String),
}

/// One registry entry and the decision made about it. The unit [`Reconciled`] refuses to be short of.
type Decision = (Entry, Held);

/// Which binding was matched to which registry name.
type Matched = BTreeMap<String, Binding>;

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

/// Every registry entry, paired with what the tree says about it.
///
/// **The pairing is the witness.** A count in a message is not one: this repository has shipped a
/// gate reporting `17 page(s), 16 generated` with 16 of 17 unscanned, and a census comparing two
/// hand-written lists that never read the tests it was about. [`Self::of`] refuses a decision list
/// that is not one decision per entry found, so a scan that judged fewer members than it found
/// cannot reach the line that prints a number.
#[derive(Debug)]
struct Reconciled {
    judged: Vec<Decision>,
}

impl Reconciled {
    /// Pairs the entries with their decisions, refusing anything that is not one-for-one.
    fn of(entries: Vec<Entry>, decided: Vec<Held>) -> Result<Self, String> {
        if entries.is_empty() {
            return Err(String::from(
                "the registry declares no data system, so every comparison below would be about nothing",
            ));
        }
        if entries.len() != decided.len() {
            return Err(format!(
                "{} registered data system(s) were found and {} judged - a verdict over a subset is \
                 not a verdict, so the count is refused rather than printed",
                entries.len(),
                decided.len()
            ));
        }
        Ok(Self {
            judged: entries.into_iter().zip(decided).collect(),
        })
    }

    /// Every way the two declarations disagree, as one message each.
    fn problems(&self, strays: &[Stray]) -> Report {
        let mut problems: Vec<String> = self
            .judged
            .iter()
            .filter_map(|(entry, held)| match held {
                Held::Bound(..) | Held::Exempt(_) => None,
                Held::Misbound(why) => Some(why.clone()),
                Held::Unheld => Some(format!(
                    "`{}` is registered as a data system ({}) and no crate binds it to the \
                     conformance packs - {}{BINDING_FILE} holds no `{}`",
                    entry.name,
                    entry.adapter,
                    entry.crate_dir,
                    scan::BINDING
                )),
            })
            .collect();
        problems.extend(strays.iter().filter_map(Stray::problem));
        problems.extend(inert(&self.judged));
        problems
    }

    /// The entries that are bound, with the selector each one's binding makes spellable.
    fn bound(&self) -> impl Iterator<Item = (&Entry, &Binding)> {
        self.judged.iter().filter_map(|(entry, held)| match held {
            Held::Bound(binding) => Some((entry, binding)),
            _ => None,
        })
    }

    /// The entries declared unbound.
    fn exempt(&self) -> impl Iterator<Item = (&Entry, &'static Unbound)> {
        self.judged.iter().filter_map(|(entry, held)| match held {
            Held::Exempt(allowance) => Some((entry, *allowance)),
            _ => None,
        })
    }
}

impl Stray {
    /// The message, for the two kinds that are a defect. The harness's own fakes are not one.
    fn problem(&self) -> Option<String> {
        match self {
            Self::Fake(_) => None,
            Self::Unregistered(binding) => Some(format!(
                "{}:{} binds `{}` to the packs and the registry names no data system from that \
                 crate - so no CI matrix emitted from the registry schedules it, which is *one \
                 registration, not two* failing in the other direction",
                binding.path, binding.invocation.line, binding.invocation.adapter
            )),
            Self::Misnamed(binding, expected) => Some(format!(
                "{}:{} binds `{}` and the registry calls that crate's data system `{expected}` - \
                 the emitted tests are `{}::<behaviour>` and a matrix keyed on the registry would \
                 select nothing",
                binding.path,
                binding.invocation.line,
                binding.invocation.adapter,
                binding.invocation.selector()
            )),
        }
    }
}

/// Every declared exemption that no longer excuses anything.
///
/// Three ways, and each one widens what is permitted while reading as a considered decision: an
/// entry for a name the registry does not carry, an entry naming the wrong crate for a name it
/// does, and an entry excusing something that is bound after all.
fn inert(judged: &[Decision]) -> Vec<String> {
    let mut problems = Vec::new();
    for allowance in UNBOUND {
        let Some((entry, held)) = judged.iter().find(|(entry, _)| entry.name == allowance.name) else {
            problems.push(format!(
                "`{}` is declared here as deliberately unbound ({}) and the registry carries no data \
                 system of that name - delete the entry rather than leaving it to excuse something \
                 nobody registered",
                allowance.name, allowance.what
            ));
            continue;
        };
        if entry.crate_name != allowance.crate_name {
            problems.push(format!(
                "`{}` is declared here as unbound in `{}` and the registry derives `{}` for it - the \
                 reason an entry gives is about a crate, so an entry that names another one is stale",
                allowance.name, allowance.crate_name, entry.crate_name
            ));
        }
        if let Held::Bound(binding) = held {
            problems.push(format!(
                "`{}` is declared here as deliberately unbound ({}) and {} binds it - delete the \
                 entry rather than leaving it to excuse a binding that exists",
                allowance.name, allowance.what, binding.path
            ));
        }
    }
    problems
}

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

    let (matched, strays) = classify(&entries, &harness, &sources.bindings);
    let decided = entries.iter().map(|entry| decide(entry, &matched)).collect();
    let reconciled = Reconciled::of(entries, decided).map_err(|why| vec![why])?;

    let problems = reconciled.problems(&strays);
    if problems.is_empty() {
        Ok(render(&reconciled, &registry, &harness, &sources, &strays))
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

/// Which binding belongs to which entry, and which belongs to none.
///
/// Keyed on the crate FIRST and the name second, so a binding written in a registry crate under
/// the wrong name is reported as that rather than as two separate absences.
fn classify(entries: &[Entry], harness: &str, bindings: &[Binding]) -> (Matched, Vec<Stray>) {
    let mut matched = Matched::new();
    let mut strays = Vec::new();
    for binding in bindings {
        let dir = owner(&binding.path).unwrap_or_default();
        if dir == harness {
            strays.push(Stray::Fake(binding.clone()));
            continue;
        }
        let here: Vec<&Entry> = entries.iter().filter(|entry| entry.crate_dir == dir).collect();
        if here.is_empty() {
            strays.push(Stray::Unregistered(binding.clone()));
        } else if here.iter().any(|entry| entry.name == binding.invocation.adapter) {
            drop(matched.insert(binding.invocation.adapter.clone(), binding.clone()));
        } else {
            let expected = here.iter().map(|entry| entry.name.as_str()).collect::<Vec<_>>().join("`, `");
            strays.push(Stray::Misnamed(binding.clone(), expected));
        }
    }
    (matched, strays)
}

/// What the tree says about one entry: bound where a selector reaches it, declared, or unheld.
fn decide(entry: &Entry, matched: &Matched) -> Held {
    matched.get(&entry.name).map_or_else(
        || {
            UNBOUND
                .iter()
                .find(|allowance| allowance.name == entry.name)
                .map_or(Held::Unheld, Held::Exempt)
        },
        |binding| misplacement(entry, binding).map_or_else(|| Held::Bound(binding.clone()), Held::Misbound),
    )
}

/// Why this binding is not where the tier selectors can reach it, if it is not.
///
/// The two properties `telekom/sutura#135` rests on, and the macro's own documentation said
/// nothing enforced either of them.
fn misplacement(entry: &Entry, binding: &Binding) -> Option<String> {
    let expected = format!("{}{BINDING_FILE}", entry.crate_dir);
    let (path, invocation) = (&binding.path, &binding.invocation);
    if *path != expected {
        return Some(format!(
            "`{}` is bound at {path}:{} and a tier is selected by binary, so it has to be \
             {expected} for `-E 'binary({BINDING_MODULE})'` to name it",
            entry.name, invocation.line
        ));
    }
    if !matches!(invocation.module.as_slice(), [only] if only == BINDING_MODULE) {
        return Some(format!(
            "`{}` is bound at {path}:{} inside `{}`, so its tests are `{}::<behaviour>` and the \
             per-adapter selector `-E 'test({BINDING_MODULE}::{})'` names nothing - the invocation \
             belongs in `mod {BINDING_MODULE}`",
            entry.name,
            invocation.line,
            if invocation.module.is_empty() {
                String::from("the file's own root")
            } else {
                invocation.module.join("::")
            },
            invocation.selector(),
            entry.name
        ));
    }
    None
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
    eprintln!("  2. declare it unbound in UNBOUND in xtask/src/conformance.rs, with what and why.");
    eprintln!();
    eprintln!("The second is an architecture decision: `docs/adr/0012` says one registration, not");
    eprintln!("two, and an exemption is that consequence going unmet. Today:");
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
    use super::{Entry, Held, Reconciled, Stray, UNBOUND, decide, inert, misplacement, owner, repo, scan};

    fn entry(name: &str, crate_name: &str) -> Entry {
        Entry {
            name: String::from(name),
            adapter: format!("{}::Warehouse", crate_name.replace('-', "_")),
            crate_name: String::from(crate_name),
            crate_dir: format!("crates/{crate_name}/"),
        }
    }

    fn invocation(adapter: &str, module: &[&str]) -> scan::Invocation {
        scan::Invocation {
            line: 42,
            adapter: String::from(adapter),
            module: module.iter().map(|part| String::from(*part)).collect(),
        }
    }

    fn binding(adapter: &str, path: &str, module: &[&str]) -> super::Binding {
        super::Binding {
            path: String::from(path),
            invocation: invocation(adapter, module),
        }
    }

    fn bound_at(adapter: &str, path: &str, module: &[&str]) -> super::Matched {
        let mut matched = super::Matched::new();
        drop(matched.insert(String::from(adapter), binding(adapter, path, module)));
        matched
    }

    /// A registered adapter with no binding and no exemption is a failure that NAMES it.
    #[test]
    fn an_unbound_registered_adapter_is_reported_by_name() {
        let entries = vec![entry("ghost", "sutura-exec-ghost")];
        let decided = vec![Held::Unheld];
        let reconciled = Reconciled::of(entries, decided).expect("one entry, one decision");
        let problems = reconciled.problems(&[]);
        let named = problems
            .iter()
            .find(|why| why.contains("`ghost` is registered"))
            .unwrap_or_else(|| panic!("the unbound entry is named: {problems:?}"));
        assert!(named.contains("crates/sutura-exec-ghost/tests/conformance.rs"), "{named}");
        // The rest of the list is the inert-exemption half firing over this fixture's registry,
        // which is the point of reporting both in one run: a gate that knew two numbers and
        // printed one cost a round trip (`telekom/sutura#311`).
        assert_eq!(problems.len(), 2, "{problems:?}");
    }

    /// **The witness.** A verdict that judged fewer members than it found is refused rather than
    /// printed: this repository has shipped `17 page(s), 16 generated` with 16 of 17 unscanned.
    #[test]
    fn judging_fewer_entries_than_were_found_is_refused() {
        let entries = vec![entry("one", "sutura-exec-one"), entry("two", "sutura-exec-two")];
        let why = Reconciled::of(entries, vec![Held::Unheld]).expect_err("a subset is not a verdict");
        assert!(why.contains("2 registered data system(s) were found and 1 judged"), "{why}");
    }

    /// An empty registry is a failure, not a green run over nothing.
    #[test]
    fn an_empty_registry_is_refused() {
        let why = Reconciled::of(Vec::new(), Vec::new()).expect_err("an empty adapter set is refused");
        assert!(why.contains("declares no data system"), "{why}");
    }

    /// A binding outside the file a `binary()` filter names is reported, with the path it needs.
    #[test]
    fn a_binding_in_another_file_is_not_where_a_tier_selector_reaches_it() {
        let subject = entry("duckdb", "sutura-exec-duckdb");
        let matched = bound_at("duckdb", "crates/sutura-exec-duckdb/tests/packs.rs", &["conformance"]);
        let Held::Misbound(why) = decide(&subject, &matched) else {
            panic!("a binding in the wrong file is misplaced")
        };
        assert!(why.contains("tests/packs.rs"), "{why}");
        assert!(why.contains("crates/sutura-exec-duckdb/tests/conformance.rs"), "{why}");
    }

    /// A binding outside `mod conformance` is reported with the selector it actually produces.
    #[test]
    fn a_binding_outside_the_wrapper_module_is_not_selectable_per_adapter() {
        let subject = entry("duckdb", "sutura-exec-duckdb");
        let matched = bound_at("duckdb", "crates/sutura-exec-duckdb/tests/conformance.rs", &["packs"]);
        let Held::Misbound(why) = decide(&subject, &matched) else {
            panic!("a binding outside the wrapper module is misplaced")
        };
        assert!(why.contains("inside `packs`"), "{why}");
        assert!(why.contains("packs::duckdb"), "{why}");
    }

    /// The bound case, and the selector is DERIVED from where the invocation sits.
    #[test]
    fn a_binding_in_the_right_place_is_held_and_carries_its_selector() {
        let subject = entry("duckdb", "sutura-exec-duckdb");
        let here = binding("duckdb", "crates/sutura-exec-duckdb/tests/conformance.rs", &["conformance"]);
        assert!(misplacement(&subject, &here).is_none());
        let matched = bound_at("duckdb", "crates/sutura-exec-duckdb/tests/conformance.rs", &["conformance"]);
        let Held::Bound(found) = decide(&subject, &matched) else {
            panic!("a binding in the right place is bound")
        };
        assert_eq!(found.invocation.selector(), "conformance::duckdb");
    }

    /// **The inert exemption**, which is the `max-lines` defect this gate must not repeat: an
    /// entry excusing something that is bound after all is a failure, and it is reported in the
    /// same run as everything else the gate knows.
    #[test]
    fn an_exemption_for_something_that_is_bound_is_reported_as_inert() {
        let declared = UNBOUND.first().expect("one declared exemption");
        let judged = vec![(
            entry(declared.name, declared.crate_name),
            Held::Bound(binding(
                declared.name,
                &format!("crates/{}/tests/conformance.rs", declared.crate_name),
                &["conformance"],
            )),
        )];
        let problems = inert(&judged);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(
            problems.first().is_some_and(|why| why.contains("delete the entry")),
            "{problems:?}"
        );
    }

    /// An exemption for something the registry does not carry is inert too, and in the direction
    /// a stale entry actually goes: the registry moved and the exemption stayed.
    #[test]
    fn an_exemption_for_an_unregistered_name_is_reported_as_inert() {
        let problems = inert(&[(entry("somebody-else", "sutura-exec-other"), Held::Unheld)]);
        assert!(
            problems.iter().any(|why| why.contains("carries no data system of that name")),
            "{problems:?}"
        );
    }

    /// An exemption naming the wrong crate for a registered name is inert: the reason an entry
    /// gives is about a crate, so it stops applying when the adapter type moves.
    #[test]
    fn an_exemption_naming_the_wrong_crate_is_reported_as_inert() {
        let declared = UNBOUND.first().expect("one declared exemption");
        let problems = inert(&[(entry(declared.name, "sutura-exec-moved"), Held::Unheld)]);
        assert!(
            problems
                .iter()
                .any(|why| why.contains("the reason an entry gives is about a crate")),
            "{problems:?}"
        );
    }

    /// A binding in a crate the registry names nothing from is the other direction of *one
    /// registration, not two*: coverage no emitted matrix schedules.
    #[test]
    fn a_binding_the_registry_does_not_name_is_reported() {
        let stray = Stray::Unregistered(binding(
            "bigquery",
            "crates/sutura-exec-bigquery/tests/conformance.rs",
            &["conformance"],
        ));
        let why = stray.problem().expect("an unregistered binding is a problem");
        assert!(why.contains("the registry names no data system from that crate"), "{why}");
    }

    /// The harness's own fakes are not a data system and are not reported as one - they are
    /// PRINTED on green instead, so a fake that moved crate is visible rather than silent.
    #[test]
    fn the_packs_crates_own_fake_bindings_are_not_a_violation() {
        let stray = Stray::Fake(binding(
            "a_fake_that_executes_legs",
            "crates/sutura-conformance/tests/bound.rs",
            &["conformance"],
        ));
        assert!(stray.problem().is_none());
    }

    #[test]
    fn every_declared_exemption_carries_its_reason() {
        for allowance in UNBOUND {
            assert!(!allowance.what.is_empty(), "{} has no `what`", allowance.name);
            assert!(!allowance.why.is_empty(), "{} has no `why`", allowance.name);
            assert!(
                allowance.crate_name.starts_with("sutura-"),
                "{} names no crate",
                allowance.name
            );
        }
    }

    /// **Every fixture above is hand-written, and this is what stops them from describing a shape
    /// the tree no longer has.** The failure `check-docs` paid for was a gate re-implementing part
    /// of a tool and being tested against the documentation rather than against the tool; the same
    /// shape here is a needle that stopped matching the real declarations, which would leave every
    /// fixture green. So this reads THIS tree through the same functions the gate uses and asserts
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
