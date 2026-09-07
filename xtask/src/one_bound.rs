//! One process builds one execution bound, and a transport builds none.
//!
//! `github.com/telekom/sutura#340`. `sutura_runtime::admission`'s own module documentation states
//! the invariant - *two independently sized semaphores would be two controls each reporting a limit
//! that the other can exceed, so the composition root builds one* - and **nothing held the word
//! *one***. Worse, the two transports did not agree on who builds it: `sutura_http`'s request state
//! DERIVED an `Admission` from the settings it was handed, so a second state was a second permit
//! set, while the agent surface took one from a root. Both read the same two keys, so the NUMBER
//! agreed; what was ungated was the count of permit sets.
//!
//! # What the type holds, and what is left for this gate
//!
//! The type does the first half and this gate does not repeat it: `Admission::new` is `pub(crate)`
//! and both transports' constructors *take* the value, so a bound cannot be built by a transport
//! at all and cannot be assembled from two numbers a caller chose. What a type cannot say is **how
//! many times a composition root calls the one public door**, because that is a property of a
//! program rather than of a signature - so it is counted here.
//!
//! # What it reads
//!
//! Text, in the order it appears, for `pins.rs`'s reason: a gate has to run on a host with no nix
//! and no resolver. Comments, MULTI-LINE string interiors and test regions come out first, through
//! the same two readers the other Rust-reading gates use - so the paragraphs that name the door
//! (this file included, if it lived under `crates/`) cannot satisfy it, and a test that builds its
//! own bound is not a composition root.
//!
//! **A composition root is DERIVED and not declared**: a crate under `crates/` with a `src/main.rs`
//! is a binary, and a binary is a process. So a third root is covered the day it is written, and
//! `check-boot-order`'s own limit - a declared list with no entry for a new root - does not apply.
//!
//! # The five rules
//!
//! | Rule | What it catches |
//! | --- | --- |
//! | Every construction is in a crate that has a `src/main.rs` | the defect itself: a transport, an application or a port deriving its own bound |
//! | At most one construction per crate | two bounds in one binary, which is two limits each reporting one the other can exceed |
//! | Every root that composes a transport has a site | a serving process whose bound came from somewhere this gate cannot see |
//! | Every declared taker is still CALLED somewhere | the rule above going vacuous on a rename - a gate that cannot notice its own subject disappearing |
//! | The door is still defined where this gate reads it | the same, one level up: a renamed constructor leaves the scan matching nothing |
//!
//! # Fails closed, five ways
//!
//! An unreadable in-scope file, a tree with no construction at all, a door that is no longer defined
//! where this reads it, a taker needle nothing calls, and a RENAME of the type - each is a failure
//! naming what could not be found. The last is a refusal rather than a check: text matching cannot
//! follow a rename, and a root written that way would leave the count at zero for its crate and read
//! as *this root builds none* while building two. `check-boot-order` refuses the same shape for the
//! same reason.
//!
//! **The refusal covers two spellings and it covered one**, which is worth stating precisely rather
//! than as *an aliased import*: `use sutura_runtime::Admission as Bound;` and
//! `type Bound = Admission;`. The second was open, was measured green over a tree that built three,
//! and survives at module scope - an in-body `type` is refused by `clippy::items_after_statements`
//! under `-D warnings`. What it still cannot follow: a `type` item split across two lines, and a
//! re-export of the type through a third crate.
//!
//! # Four limits, stated next to the claim
//!
//! **It counts CONSTRUCTIONS, and a construction is a spelling of the door.** One call inside a loop
//! is one construction and as many bounds as iterations. A bound reached through a function pointer,
//! a trait method, a macro-generated call, a re-export through a third crate, or a qualified
//! `<Admission as Trait>::from_settings` is invisible to any text scan. Two on ONE LINE used to be
//! invisible too - see [`call_count`], which is where the count stopped being per line.
//!
//! **A crate is one process here because a crate has one binary here.** A member declaring two
//! `[[bin]]` targets would legitimately want two sites and would fail the second rule; none does,
//! and the honest fix then is a per-target scan rather than a raised number. A `[[bin]]` whose
//! `path` is not `src/main.rs` is not seen as a root at all - it would fail the FIRST rule, which
//! is the safe direction.
//!
//! **`crates/` only**, so a composition root written outside it - `dev/`, `xtask/` - is outside
//! every rule. Both of those are tools, neither serves a transport, and this file's own fixtures
//! name the door in `xtask/`, which is why the scope is not the whole tree.
//!
//! **It says nothing about the NUMBER.** That the two keys are read from one place is
//! `Admission::from_settings`' own signature, and that the number reaches a transport is each
//! root's own test.

use std::collections::{BTreeMap, BTreeSet};

use crate::Verdict;
use crate::boot_order::{defines, imports};
use crate::causality::regions::{self, PostImage};
use crate::repo;
use crate::serde_parse::scan::code_lines;

/// The one public door to a permit set.
///
/// The qualified spelling and not the bare method name, which is what keeps `fn from_settings` -
/// the definition itself - from reading as a call to it. An alias at the import defeats that and is
/// refused by [`no_bound_hides_behind_an_alias`] rather than chased.
const DOOR: &str = "Admission::from_settings";

/// The type whose renaming at an import would defeat the scan.
const BOUND: &str = "Admission";

/// Where the door has to still be defined, and the signature this gate is reading for.
///
/// The fifth way this fails closed. With the constructor renamed - or made private, or moved to
/// another crate - every scan below matches nothing, every rule passes over an empty set, and a
/// gate that read no program prints `ok`. `check-boot-order` measured exactly that on its own
/// declaration and it is the outcome an order-reading or count-reading gate must never have.
const DOOR_DEFINED_IN: &str = "crates/sutura-runtime/src/admission.rs";

/// The door's signature, as the crate that owns it spells it.
const DOOR_SIGNATURE: &str = "pub fn from_settings";

/// The transport compositions a bound is handed TO.
///
/// Each is a constructor that cannot be called without a bound, which is what makes *this crate
/// composes a transport* a property a text scan may read: it is not a guess about a role, it is a
/// call to a signature that requires the value. `serve_stdio` is here as well as `AgentSurface::new`
/// because a root reaches the agent surface through it and never names the handler type.
///
/// **Every one of these must be called somewhere**, or the rule they serve is vacuous - see
/// [`every_taker_is_still_called`].
const TAKERS: &[&str] = &["ServiceState::new(", "AgentSurface::new(", "serve_stdio("];

pub(crate) fn run(_args: &[String]) -> Verdict {
    let counted = match check() {
        Ok(counted) => counted,
        Err(why) => {
            eprintln!("xtask check-one-bound: {why}");
            eprintln!();
            eprintln!("Two independently sized bounds over one blocking pool are two controls each");
            eprintln!("reporting a limit the other can exceed, which is not a bound. One composition");
            eprintln!("root builds one and hands it down. github.com/telekom/sutura#340.");
            return Verdict::Fail;
        }
    };
    println!(
        "xtask check-one-bound: ok - {} execution bound(s) built, one per serving composition root, \
         none in a transport, over {} Rust file(s) under crates/ and {} root(s) with a src/main.rs",
        counted.sites, counted.read, counted.roots
    );
    Verdict::Pass
}

/// What the scan counted, so the verdict line prints a measurement rather than a declaration.
struct Counted {
    /// Construction sites found outside comments and tests.
    sites: usize,
    /// Rust files under `crates/` that were read to find them.
    read: usize,
    /// Crates under `crates/` with a `src/main.rs`.
    roots: usize,
}

/// One `Result` rather than a print-and-return block per failure, so the task name and the
/// paragraph under it are written once - `boot_order`'s shape, for the reason it gives.
fn check() -> Result<Counted, String> {
    let (root, files) = repo::all_files().and_then(|census| census.into_listing(repo::Unmigrated::OneBound)).map_err(|why| why.describe())?;
    let read = |path: &str| std::fs::read_to_string(root.join(path)).ok();
    door_is_still_defined(&read)?;
    let scanned = scan(&files, &read)?;
    no_bound_hides_behind_an_alias(&files, &read)?;
    let roots = roots(&files);
    at_least_one_bound_is_built(&scanned)?;
    every_taker_is_still_called(&scanned)?;
    every_site_is_in_a_composition_root(&scanned, &roots)?;
    at_most_one_bound_per_crate(&scanned)?;
    every_serving_root_builds_one(&scanned, &roots)?;
    Ok(Counted {
        sites: scanned.sites.values().map(Vec::len).sum(),
        read: scanned.read,
        roots: roots.len(),
    })
}

/// What the scan found.
///
/// Keyed by CRATE, because the rule about how many bounds there may be is a rule about a process
/// and a process here is a binary crate. The values keep `path:line` so a failure names the line to
/// open rather than the crate to search.
#[derive(Default)]
struct Scan {
    /// Crate name to the construction sites in its non-test code.
    sites: BTreeMap<String, Vec<String>>,
    /// Crate name to the files whose non-test code composes a transport.
    takers: BTreeMap<String, Vec<String>>,
    /// Taker needle to how many files call it anywhere under `crates/`.
    called: BTreeMap<&'static str, usize>,
    /// Rust files under `crates/` that were read.
    read: usize,
}

/// Every construction site and every transport composition, outside comments and test code.
///
/// A file that cannot be read is a failure and not a skip: a scan that quietly shrank is how a
/// second bound goes unnoticed, and this gate's whole subject is a count.
fn scan(files: &[String], read: &PostImage<'_>) -> Result<Scan, String> {
    // Seeded with every needle at zero, so a taker nothing calls is a MISSING count rather than a
    // missing key - which is what [`every_taker_is_still_called`] reads.
    let mut found = Scan {
        called: TAKERS.iter().map(|&needle| (needle, 0)).collect(),
        ..Scan::default()
    };
    for rel in files.iter().filter(|rel| in_scope(rel)) {
        let text = read(rel).ok_or_else(|| {
            format!("could not read {rel}, so the count this gate exists to take is over fewer files than the tree has")
        })?;
        found.read = found.read.saturating_add(1);
        // Before the lexer, because it only ever REMOVES text: a file whose raw bytes carry neither
        // needle cannot carry one once comments and string interiors are blanked. This skips the lex
        // for all but a handful of files under `crates/`.
        if !text.contains(DOOR) && !TAKERS.iter().any(|needle| text.contains(needle)) {
            continue;
        }
        let code = code_lines(&text);
        let tests = regions::scope(rel, read);
        let owner = crate_of(rel);
        for (index, line) in code.iter().enumerate() {
            let number = index.saturating_add(1);
            if tests.covers(number) {
                continue;
            }
            // One entry PER CONSTRUCTION and not per line - see [`call_count`] for the hole that
            // was. Two on one line is two entries with the same location, and [`located`] is what
            // makes the failure message say so rather than printing one place twice.
            let built = call_count(line, DOOR);
            if built > 0 {
                let sites = found.sites.entry(owner.clone()).or_default();
                sites.extend(std::iter::repeat_n(format!("{rel}:{number}"), built));
            }
            for needle in TAKERS {
                let called = call_count(line, needle);
                if called > 0 {
                    *found.called.entry(needle).or_default() += called;
                    let composing = found.takers.entry(owner.clone()).or_default();
                    if !composing.contains(rel) {
                        composing.push(rel.clone());
                    }
                }
            }
        }
    }
    Ok(found)
}

/// Is this a Rust file that could build or take a bound?
fn in_scope(rel: &str) -> bool {
    rel.starts_with("crates/")
        && std::path::Path::new(rel)
            .extension()
            .and_then(std::ffi::OsStr::to_str)
            .is_some_and(|ext| ext.eq_ignore_ascii_case("rs"))
}

/// The crate a `crates/<name>/...` path belongs to.
///
/// The path shape rather than a manifest read, for this gate's stated reason: no resolver. A path
/// under `crates/` with no second segment cannot exist, and an empty name would only ever group
/// sites this gate then reports together.
fn crate_of(rel: &str) -> String {
    rel.split('/').nth(1).unwrap_or_default().to_owned()
}

/// Which crates under `crates/` are composition roots.
///
/// **Derived, and that is the difference from `check-boot-order`'s declared list**: a crate with a
/// `src/main.rs` is a binary and a binary is a process, so a fourth root is covered by this gate the
/// day somebody writes it rather than the day somebody remembers to declare it.
fn roots(files: &[String]) -> BTreeSet<String> {
    files
        .iter()
        .filter_map(|rel| {
            let name = crate_of(rel);
            (!name.is_empty() && rel == &format!("crates/{name}/src/main.rs")).then_some(name)
        })
        .collect()
}

/// How many times `line` CALLS `needle`, rather than defining or importing it.
///
/// **A COUNT and not a predicate, and that distinction was a live hole rather than a refinement.**
/// This answered a bool per line and the scan pushed one site per line, so
/// `let (a, b) = (Admission::from_settings(r), Admission::from_settings(r));` - one line, inside
/// `max_width = 130`, so rustfmt keeps it - read as ONE bound. `cargo fmt --check`, `just lint` and
/// `just hygiene` were all green over a tree that built three, with this gate printing
/// `2 execution bound(s) built, one per serving composition root`. The same two calls on two lines
/// was red, so the whole difference was line granularity - and this file's own two-bound fixture
/// used two lines, which is why nothing noticed. **A gate has to ask the question its claim is
/// about**, and the claim here is about how many bounds get BUILT.
///
/// [`crate::boot_order`]'s two readers rather than a second copy of them, which is the argument that
/// module's own header makes about its recipe parser: one place knows what a Rust definition and a
/// `use` item look like to a text scan.
fn call_count(line: &str, needle: &str) -> usize {
    if imports(line) {
        return 0;
    }
    line.match_indices(needle).filter(|&(at, _)| !defines(line, at)).count()
}

/// The locations of a set of construction sites, with a repeat collapsed into a count.
///
/// Two constructions on ONE line are two sites at one location, so a plain join printed the same
/// place twice and left a reader counting commas. This is what makes the count and the message agree.
fn located(sites: &[String]) -> String {
    let mut counted: Vec<(&str, usize)> = Vec::new();
    for site in sites {
        match counted.last_mut() {
            Some(&mut (place, ref mut times)) if place == site.as_str() => *times += 1,
            _ => counted.push((site.as_str(), 1)),
        }
    }
    counted
        .into_iter()
        .map(|(place, times)| {
            if times > 1 {
                format!("{place} ({times} on that line)")
            } else {
                String::from(place)
            }
        })
        .collect::<Vec<String>>()
        .join(", ")
}

/// The door is still defined where this gate reads for calls to it.
///
/// Not a style rule and not a claim about the crate's API: it is the guard that stops every rule
/// below from passing over an empty set. A constructor renamed, made private or moved is a real
/// change and it has to be a red gate rather than a quiet one.
fn door_is_still_defined(read: &PostImage<'_>) -> Result<(), String> {
    let text = read(DOOR_DEFINED_IN).ok_or_else(|| {
        format!("could not read {DOOR_DEFINED_IN}, which is where the one door to an execution bound is defined - so this gate has no needle it can trust")
    })?;
    let tests = regions::scope(DOOR_DEFINED_IN, read);
    let defined = code_lines(&text).iter().enumerate().any(|(index, line)| {
        let number = index.saturating_add(1);
        !tests.covers(number) && line.contains(DOOR_SIGNATURE)
    });
    if defined {
        return Ok(());
    }
    Err(format!(
        "{DOOR_DEFINED_IN} no longer defines `{DOOR_SIGNATURE}` outside comments and tests. Every rule \
         in this gate matches `{DOOR}` as text, so a renamed, narrowed or moved constructor leaves it \
         counting nothing and printing `ok`. Rename the constant in xtask/src/one_bound.rs with it, or \
         - if the door is gone - the bound this gate counts is gone too"
    ))
}

/// An alias defeats the scan, so BOTH spellings of a rename on the bound's type are refused.
///
/// `use sutura_runtime::Admission as Bound;` makes every call site spell something this gate has
/// never heard of, so a root written that way reads as building none while building two. Text
/// matching cannot follow a rename, so the rename is refused instead of chased - one forbidden
/// idiom, scoped to one name and only to the forms that rename it.
///
/// **`type Bound = Admission;` is the second spelling and it was open**, because this read only
/// [`imports`]: a `type` item's first token is neither `use` nor a call, so the line was invisible
/// and `Bound::from_settings(` matched no needle. Measured at MODULE scope, where it survives - an
/// in-body alias is refused by `clippy::items_after_statements` under `-D warnings`, so the module
/// is the shape that mattered. One honest construction beside one aliased one was two permit sets in
/// one binary reading as `ok`.
///
/// **The `type` rule is narrow on purpose, because a false red here is a real idiom.**
/// `crates/sutura-cli/src/mcp.rs` declares `type Served<W> = (Composed<W>, CatalogProse, Admission,
/// RequestTimeout);` - an alias that NAMES the bound inside a tuple and is not another name FOR it.
/// So the right-hand side has to BE the bound: equal to it, or a path ending in it.
fn no_bound_hides_behind_an_alias(files: &[String], read: &PostImage<'_>) -> Result<(), String> {
    for rel in files.iter().filter(|rel| in_scope(rel)) {
        let Some(text) = read(rel) else { continue };
        if !text.contains(BOUND) {
            continue;
        }
        let tests = regions::scope(rel, read);
        for (index, line) in code_lines(&text).iter().enumerate() {
            let number = index.saturating_add(1);
            if tests.covers(number) {
                continue;
            }
            if aliases(line) {
                return Err(format!(
                    "{rel}:{number} gives `{BOUND}` another name. This gate counts the places a process \
                     builds one by matching that name as text, so a construction spelled differently is \
                     invisible to it - name the type as itself, whether that is an import or a `type` item"
                ));
            }
        }
    }
    Ok(())
}

/// Whether `line` renames the bound's type, in either of the two spellings that would do it.
///
/// `use ... as ...` and a `type` item whose whole right-hand side IS the bound. The second is
/// compared after the `=` and against the WHOLE remainder, so an alias that merely mentions the
/// bound inside a bigger type - a tuple this tree really declares - is not a rename and is left
/// alone. `ends_with` covers a qualified path (`sutura_runtime::Admission`) without admitting a
/// longer name that happens to end in those letters, because the segment separator is part of the
/// comparison.
fn aliases(line: &str) -> bool {
    if imports(line) && line.contains(BOUND) && line.contains(" as ") {
        return true;
    }
    let mut tokens = line.split_whitespace().skip_while(|token| token.starts_with("pub"));
    if tokens.next() != Some("type") {
        return false;
    }
    let Some((_, right)) = line.split_once('=') else {
        return false;
    };
    let named = right.trim().trim_end_matches(';').trim();
    named == BOUND || named.ends_with(&format!("::{BOUND}"))
}

/// Something in this tree builds a bound.
///
/// The empty-scan arm. A workspace where nothing constructs an `Admission` is one where nothing is
/// bounded, or one where this gate's needle has stopped matching - and both are red rather than a
/// gate reporting `ok` over a count of zero.
fn at_least_one_bound_is_built(found: &Scan) -> Result<(), String> {
    if found.sites.values().any(|sites| !sites.is_empty()) {
        return Ok(());
    }
    Err(format!(
        "no file under crates/ calls `{DOOR}` outside comments and tests, so nothing in this workspace \
         builds an execution bound - or the door was renamed and this gate is counting a spelling that \
         no longer exists. A scan that finds nothing does not get to say `ok`"
    ))
}

/// Each transport composition this gate keys on is still called by something.
///
/// **The rule that keeps the next one honest.** *Every serving root builds a bound* is decided by
/// finding the roots that compose a transport, and they are found by matching [`TAKERS`] as text -
/// so a renamed constructor would leave that rule with no root to check and this gate green over a
/// serving process it can no longer see. A control that cannot notice its own subject disappearing
/// is the failure mode this whole gate exists for.
fn every_taker_is_still_called(found: &Scan) -> Result<(), String> {
    if let Some((needle, _)) = found.called.iter().find(|&(_, &count)| count == 0) {
        return Err(format!(
            "nothing under crates/ calls `{needle}` outside comments and tests. That is one of the \
             transport compositions this gate uses to find a root that SERVES, so with it unmatched the \
             rule `every serving root builds a bound` has no root left to check. Either the transport is \
             gone, or it was renamed and `TAKERS` in xtask/src/one_bound.rs has to be renamed with it"
        ));
    }
    Ok(())
}

/// Only a composition root may build a bound.
///
/// **The defect itself.** `sutura_http::state::ServiceState::new` derived one from the settings it
/// was handed, which made a second state a second permit set - and the argument in its own comment
/// was that a bound a caller supplies is a bound a caller can forget. It cannot be forgotten: the
/// parameter has no default.
fn every_site_is_in_a_composition_root(found: &Scan, roots: &BTreeSet<String>) -> Result<(), String> {
    for (owner, sites) in &found.sites {
        if roots.contains(owner) {
            continue;
        }
        let where_ = located(sites);
        return Err(format!(
            "`{owner}` builds an execution bound at {where_} and it is not a composition root - it has no \
             crates/{owner}/src/main.rs, so it is a library that some process links. A bound built there is \
             a second permit set in every process that links it beside another one, each reporting a limit \
             the other can exceed. Take an `Admission` as an argument and let the root that has a main() \
             decide there is one of it"
        ));
    }
    Ok(())
}

/// One process, one bound.
///
/// Per crate rather than per file, because the thing being counted is a process: two sites in two
/// modules of one binary are still two permit sets.
fn at_most_one_bound_per_crate(found: &Scan) -> Result<(), String> {
    for (owner, sites) in &found.sites {
        if sites.len() <= 1 {
            continue;
        }
        let where_ = located(sites);
        return Err(format!(
            "`{owner}` builds {} execution bounds, at {where_}. A process gets one: two semaphores over one \
             blocking pool are two controls each reporting a limit the other can exceed, whatever numbers \
             they were built from. Build one and hand it to whatever serves - every clone of an `Admission` \
             shares its permit set",
            sites.len()
        ));
    }
    Ok(())
}

/// A root that composes a transport builds the bound that transport takes.
///
/// The other direction from the two rules above, and the one that would catch a serving root whose
/// bound came from somewhere no scan here can see - a re-export, a helper crate, a clone handed
/// across a boundary. It is also the rule that makes the first two non-trivial: without it a tree
/// with zero bounds and two serving roots satisfies *at most one per crate* perfectly.
fn every_serving_root_builds_one(found: &Scan, roots: &BTreeSet<String>) -> Result<(), String> {
    for (owner, composing) in &found.takers {
        if !roots.contains(owner) || found.sites.get(owner).is_some_and(|sites| !sites.is_empty()) {
            continue;
        }
        let where_ = composing.join(", ");
        return Err(format!(
            "`{owner}` composes a transport at {where_} and builds no execution bound. It is a composition \
             root - crates/{owner}/src/main.rs exists - so it is the one place that can decide there is one \
             bound for this process. Call `{DOOR}` once and hand the value to whatever serves"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use super::{
        BOUND, DOOR, DOOR_DEFINED_IN, DOOR_SIGNATURE, Scan, TAKERS, at_least_one_bound_is_built, at_most_one_bound_per_crate,
        call_count, crate_of, door_is_still_defined, every_serving_root_builds_one, every_site_is_in_a_composition_root,
        every_taker_is_still_called, located, no_bound_hides_behind_an_alias, roots, scan,
    };

    /// A fixture tree: the paths the gate lists, and what each one holds.
    ///
    /// Named rather than a tuple, because `type_complexity` is tightened in this workspace.
    struct Tree {
        paths: Vec<String>,
        contents: BTreeMap<String, String>,
    }

    /// A tree of paths to contents, read the way the gate reads the working tree.
    fn tree(files: &[(&str, &str)]) -> Tree {
        Tree {
            paths: files.iter().map(|&(path, _)| String::from(path)).collect(),
            contents: files
                .iter()
                .map(|&(path, text)| (String::from(path), String::from(text)))
                .collect(),
        }
    }

    /// The scan over such a tree.
    fn scanned(files: &[(&str, &str)]) -> Scan {
        let Tree { paths, contents } = tree(files);
        scan(&paths, &|path| contents.get(path).cloned()).expect("a fixture tree is readable")
    }

    fn named(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|&name| String::from(name)).collect()
    }

    /// A root that builds one bound and hands it to every taker this gate keys on.
    ///
    /// All three, which no real root does - `sutura-serve` composes the HTTP state and `sutura-cli`
    /// the agent surface. It has to be all three here so that renaming ONE of them below leaves the
    /// other two matched, and the failure therefore names the needle under test rather than
    /// whichever happens to sort first.
    const GOOD_ROOT: &str = "\
fn run() -> Result<(), String> {
    let admission = Admission::from_settings(settings.runtime());
    let state = ServiceState::new(service, Arc::new(settings), admission.clone());
    let agent = AgentSurface::new(service, permitted, prose, admission.clone(), reply);
    block_on(sutura_mcp::serve_stdio(service, permitted, prose, admission, reply))
}
";

    #[test]
    fn the_tree_itself_passes_and_the_scan_is_not_empty() {
        // Over the REAL files, for the reason `check-boot-order`'s own suite gives: a reader that
        // matches nothing makes its gate pass vacuously. Non-vacuous by construction - more files
        // read than sites found, and every rule asserted rather than the summary.
        let Ok((root, files)) = crate::repo::all_files()
            .and_then(|census| census.into_listing(crate::repo::Unmigrated::OneBound))
        else {
            return;
        };
        let read = |path: &str| std::fs::read_to_string(root.join(path)).ok();
        assert_eq!(door_is_still_defined(&read), Ok(()));
        let found = scan(&files, &read).expect("every Rust file under crates/ is readable");
        let roots = roots(&files);
        assert_eq!(at_least_one_bound_is_built(&found), Ok(()));
        assert_eq!(every_taker_is_still_called(&found), Ok(()));
        assert_eq!(
            every_site_is_in_a_composition_root(&found, &roots),
            Ok(()),
            "{:?}",
            found.sites
        );
        assert_eq!(at_most_one_bound_per_crate(&found), Ok(()), "{:?}", found.sites);
        assert_eq!(every_serving_root_builds_one(&found, &roots), Ok(()), "{:?}", found.takers);
        assert_eq!(no_bound_hides_behind_an_alias(&files, &read), Ok(()));
        // The measurement, so this test fails if the scan stops reading the tree rather than only if
        // a rule stops holding.
        let sites: usize = found.sites.values().map(Vec::len).sum();
        assert!(sites >= 2, "one bound per serving root, and there are two roots: {sites}");
        assert!(found.read > sites, "a scan of {} file(s) is not this tree", found.read);
        assert!(roots.len() >= 2, "{roots:?}");
    }

    #[test]
    fn a_transport_that_derives_its_own_bound_is_red() {
        // THE defect, replayed: this is the line `sutura_http::state` carried until #340, and the
        // crate it is in has no `src/main.rs`.
        let found = scanned(&[
            (
                "crates/sutura-http/src/state.rs",
                "    let admission = Admission::from_settings(settings.runtime());\n",
            ),
            ("crates/sutura-serve/src/main.rs", GOOD_ROOT),
        ]);
        let error = every_site_is_in_a_composition_root(&found, &named(&["sutura-serve"]))
            .expect_err("a transport may not build a permit set");
        assert!(error.contains("crates/sutura-http/src/state.rs:1"), "{error}");
        assert!(error.contains("second permit set"), "{error}");
    }

    #[test]
    fn two_bounds_in_one_root_are_red() {
        // The count, which is the half no signature can hold: both of these compile.
        let found = scanned(&[(
            "crates/sutura-serve/src/main.rs",
            "fn run() {\n    let a = Admission::from_settings(settings.runtime());\n    let b = Admission::from_settings(other.runtime());\n    let state = ServiceState::new(service, settings, a);\n}\n",
        )]);
        let error = at_most_one_bound_per_crate(&found).expect_err("a process gets one bound");
        assert!(error.contains("builds 2 execution bounds"), "{error}");
        assert!(error.contains("crates/sutura-serve/src/main.rs:2"), "{error}");
        assert!(error.contains("crates/sutura-serve/src/main.rs:3"), "{error}");
    }

    #[test]
    fn two_bounds_on_one_line_are_red_too() {
        // **The hole the fixture above used to leave.** It puts the two constructions on two LINES,
        // and the scan counted lines - so a destructuring pair inside `max_width = 130`, which
        // rustfmt keeps as one line, read as ONE bound. Measured green over a tree that built three,
        // with the verdict printing `2 ... one per serving composition root`.
        let found = scanned(&[(
            "crates/sutura-serve/src/main.rs",
            "fn run() {\n    let (admission, spare) = (Admission::from_settings(r), Admission::from_settings(r));\n    let state = ServiceState::new(service, settings, admission);\n}\n",
        )]);
        let error = at_most_one_bound_per_crate(&found).expect_err("two constructions on one line are two bounds");
        assert!(error.contains("builds 2 execution bounds"), "{error}");
        // And the message says they share a line rather than printing one place twice.
        assert!(
            error.contains("crates/sutura-serve/src/main.rs:2 (2 on that line)"),
            "{error}"
        );
        // The count is what the verdict line prints, so the honest number reaches a reader.
        let sites: usize = found.sites.values().map(Vec::len).sum();
        assert_eq!(sites, 2, "{:?}", found.sites);
    }

    #[test]
    fn a_repeated_location_is_reported_as_a_count_and_a_single_one_is_not() {
        // The formatting half of the rule above, on its own, because a message that printed one
        // place twice is how a reader gets talked out of a correct count.
        let twice = [String::from("a.rs:7"), String::from("a.rs:7")];
        assert_eq!(located(&twice), "a.rs:7 (2 on that line)");
        let apart = [String::from("a.rs:7"), String::from("a.rs:9")];
        assert_eq!(located(&apart), "a.rs:7, a.rs:9");
    }

    #[test]
    fn a_serving_root_that_builds_no_bound_is_red() {
        // The direction that makes the other two non-trivial: a tree with no bounds at all satisfies
        // `at most one per crate` perfectly.
        let found = scanned(&[(
            "crates/sutura-cli/src/mcp.rs",
            "fn serve() {\n    block_on(sutura_mcp::serve_stdio(service, permitted, prose))\n}\n",
        )]);
        let error =
            every_serving_root_builds_one(&found, &named(&["sutura-cli"])).expect_err("a root that serves a transport bounds it");
        assert!(error.contains("composes a transport"), "{error}");
        assert!(error.contains("crates/sutura-cli/src/mcp.rs"), "{error}");
        // And a crate that is NOT a root is left alone: `sutura-mcp` itself constructs the handler.
        assert_eq!(every_serving_root_builds_one(&found, &BTreeSet::new()), Ok(()));
    }

    #[test]
    fn a_tree_that_builds_no_bound_anywhere_is_red() {
        // The empty scan. Both spellings of empty: no entry at all, and an entry with no sites.
        let found = scanned(&[("crates/sutura-http/src/state.rs", "fn new() {}\n")]);
        let error = at_least_one_bound_is_built(&found).expect_err("a tree with no bound has none to count");
        assert!(error.contains(DOOR), "{error}");
        assert!(error.contains("does not get to say `ok`"), "{error}");
        let mut hollow = Scan::default();
        drop(hollow.sites.insert(String::from("sutura-serve"), Vec::new()));
        assert!(
            at_least_one_bound_is_built(&hollow).is_err(),
            "an entry with no sites is no site"
        );
    }

    #[test]
    fn a_taker_nothing_calls_is_red_rather_than_vacuous() {
        // The mutation a reviewer reaches for: rename the transport constructor and the rule about a
        // serving root has nothing left to find. Every needle is asserted, not just the missing one.
        for renamed in TAKERS {
            let found = scanned(&[(
                "crates/sutura-serve/src/main.rs",
                &GOOD_ROOT.replace(renamed, "SomethingElse::new("),
            )]);
            let error = every_taker_is_still_called(&found).expect_err("a needle nothing calls checks nothing");
            assert!(error.contains(renamed), "{error}");
            assert!(error.contains("has no root left to check"), "{error}");
        }
    }

    #[test]
    fn a_door_that_moved_is_red() {
        // The needle's own definition, which is the last thing standing between this gate and a
        // count of zero read as compliance.
        let read =
            |path: &str| (path == DOOR_DEFINED_IN).then(|| String::from("    fn built_from(runtime: RuntimeSettings) {}\n"));
        let error = door_is_still_defined(&read).expect_err("a door that moved leaves the scan matching nothing");
        assert!(error.contains(DOOR_SIGNATURE), "{error}");
        assert!(error.contains("printing `ok`"), "{error}");
        // And a file this gate cannot read at all is the same failure rather than a skip.
        assert!(door_is_still_defined(&|_| None).is_err());
    }

    #[test]
    fn either_spelling_of_a_rename_of_the_bound_is_refused() {
        // **BOTH spellings, and the second was open.** The refusal read only `use ... as`, so
        // `type Bound = Admission;` at module scope left one honest construction beside one aliased
        // one - two permit sets in one binary - reading as `ok`.
        for renamed in [
            "use sutura_runtime::Admission as Bound;\nfn run() { let a = Bound::from_settings(r); }\n",
            "type Bound = Admission;\nfn run() { let a = Bound::from_settings(r); }\n",
            "pub type Bound = sutura_runtime::Admission;\nfn run() { let a = Bound::from_settings(r); }\n",
        ] {
            let Tree { paths, contents } = tree(&[("crates/sutura-serve/src/main.rs", renamed)]);
            let error = no_bound_hides_behind_an_alias(&paths, &|path| contents.get(path).cloned())
                .expect_err("a rename makes every construction invisible to a text scan");
            assert!(error.contains("another name"), "{error}");
            assert!(error.contains("crates/sutura-serve/src/main.rs:1"), "{renamed} -> {error}");
        }
    }

    #[test]
    fn naming_the_bound_without_renaming_it_is_allowed() {
        // The other side, and it is not hypothetical: a false red here would ban an idiom this tree
        // already has. `sutura-cli` declares an alias for what its `mcp` composition RESOLVES, and
        // the bound is one member of that tuple rather than the thing being renamed.
        for honest in [
            "use sutura_runtime::Admission;\n",
            "type Served<W> = (Composed<W>, CatalogProse, Admission, RequestTimeout);\n",
            "pub type Bounds = (Admission, RequestTimeout);\n",
        ] {
            let Tree { paths, contents } = tree(&[("crates/sutura-cli/src/mcp.rs", honest)]);
            assert_eq!(
                no_bound_hides_behind_an_alias(&paths, &|path| contents.get(path).cloned()),
                Ok(()),
                "{honest}"
            );
        }
    }

    #[test]
    fn prose_a_definition_and_test_code_are_not_construction_sites() {
        // Three decoys, and each is really in this tree: the door is named in prose in four modules,
        // defined once, and called by tests that are another composition root.
        let found = scanned(&[(
            "crates/sutura-serve/src/main.rs",
            "//! It calls Admission::from_settings once, and this line is prose.\n/* Admission::from_settings( in a block comment. */\nfn run() {\n    let a = Admission::from_settings(settings.runtime());\n    let s = ServiceState::new(x, y, a);\n}\n#[cfg(test)]\nmod tests {\n    fn fixture() { let a = Admission::from_settings(other.runtime()); }\n}\n",
        )]);
        assert_eq!(
            found.sites.get("sutura-serve").map(Vec::as_slice),
            Some(["crates/sutura-serve/src/main.rs:4".to_owned()].as_slice()),
            "{:?}",
            found.sites
        );
    }

    #[test]
    fn a_root_is_a_crate_with_a_main() {
        // Derived rather than declared, which is what covers a third root the day it is written.
        let paths: Vec<String> = [
            "crates/sutura-serve/src/main.rs",
            "crates/sutura-cli/src/main.rs",
            "crates/sutura-http/src/lib.rs",
            "crates/sutura-mcp/src/server/main.rs",
            "xtask/src/main.rs",
        ]
        .iter()
        .map(|&path| String::from(path))
        .collect();
        assert_eq!(roots(&paths), named(&["sutura-cli", "sutura-serve"]));
        assert_eq!(crate_of("crates/sutura-http/src/state.rs"), "sutura-http");
    }

    #[test]
    fn a_definition_and_an_import_are_not_calls_and_two_on_a_line_are_two() {
        // The bare `serve_stdio(` needle would otherwise read the transport's own signature as a
        // call to it, and `use` lines are how the rename refusal above stays targeted.
        assert_eq!(call_count("    let a = Admission::from_settings(runtime);", DOOR), 1);
        assert_eq!(call_count("use sutura_runtime::Admission::from_settings;", DOOR), 0);
        assert_eq!(
            call_count("    block_on(sutura_mcp::serve_stdio(service))", "serve_stdio("),
            1
        );
        assert_eq!(
            call_count(
                "pub async fn serve_stdio(service: Arc<S>) -> Result<(), NotServed> {",
                "serve_stdio("
            ),
            0
        );
        assert_eq!(call_count(&format!("pub fn {BOUND}"), BOUND), 0);
        // THE count that used to be a bool: one line, two constructions.
        assert_eq!(
            call_count(
                "    let (admission, spare) = (Admission::from_settings(r), Admission::from_settings(r));",
                DOOR
            ),
            2
        );
    }
}
