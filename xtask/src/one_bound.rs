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
//! **A root is now a FILE, not only a crate - since `github.com/telekom/sutura#685` folded
//! `sutura-serve` into `sutura-cli` as a subcommand.** One crate, `sutura-cli`, has two files that
//! each independently compose a transport (`src/mcp.rs`, `src/serve.rs`) and each build their own
//! bound - which used to be two crates, each a separate PROCESS, and is now two ARGV DISPATCH
//! TARGETS in one binary, mutually exclusive at runtime because `main.rs`'s own `dispatch` calls at
//! most one root's `run` per invocation - structural control flow, not a test. **That mutual
//! exclusivity is not something this text scan can see or prove, and it is a NARROWER claim than
//! the code once made for it.** `main.rs`'s own `#[cfg(test)] mod tests` never calls `dispatch` -
//! only `requested`, the routing decision - so it holds nothing about `dispatch`'s execution arm.
//! What reacts, measurably, is the `sutura-cli::mcp` integration suite (spawns the compiled binary,
//! speaks the wire protocol): injecting an extra call to the OTHER root inside `dispatch`'s `Run`
//! arm turns 11 of its tests red, but only for the SAME-root-invoked-twice shape and only because
//! the extra call happens to receive valid arguments - it says nothing about a cross-root double
//! dispatch. What this gate still holds, at the FILE granularity a fold like that needs: a file
//! that composes a transport builds exactly one bound of its own, and a bound built anywhere else
//! in the crate - a helper module, a shared type - is refused the same as before. `roots` derives
//! which files count, the same way it always derived which crates did.
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
//! **Every in-scope file is read once, by [`inspect_scan`]**, and every check reads those bytes, so a
//! listed file that vanished is a refusal rather than a file the count lost. The census holds that a
//! file was OPENED, not what the scan did with it; `inspect_scan`'s own count reconciles the two.
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
//! **A root is one FILE, and a crate's one binary may hold several.** A member declaring two
//! `[[bin]]` targets would still fail the second rule if either binary itself built two bounds.
//! `is_bin` treats `src/bin/*.rs` (and `src/bin/*/main.rs`) as a root unconditionally, same as
//! `src/main.rs` - **this was open until `github.com/telekom/sutura#784`'s review**: the fallback
//! (any file in the crate that composes a transport roots itself) used to be the ONLY way such a
//! file was seen, and it is keyed off `src/main.rs` alone, so a crate with no `src/main.rs` at
//! all - `sutura-mcp`, which is a library - never entered `binary_crates` and a `[[bin]]` grown
//! under it composed a transport with zero bound completely invisibly. `sutura-cli`'s `mcp.rs` and
//! `serve.rs` are the measured case for the fallback: two root files, one crate, one binary, never
//! both live in the same process because argv dispatch picks one. `sutura-mcp`'s `serve_stdio` and
//! `service` are the measured case for staying EXCUSED: both take an `Admission` as a parameter and
//! call `AgentSurface::new(` on a crate with no `src/main.rs` and no `src/bin/`, so neither reads as
//! a root - and adding a real `src/bin/*.rs` there does not change that, because `is_bin` never
//! widens `binary_crates`.
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
         none in a transport, over {} Rust file(s) under crates/ and {} root file(s) (a crate's \
         src/main.rs, or a file that composes a transport itself); {}",
        counted.sites, counted.read, counted.roots, counted.witness
    );
    Verdict::Pass
}

/// What the scan counted, so the verdict line prints a measurement rather than a declaration.
struct Counted {
    /// Construction sites found outside comments and tests.
    sites: usize,
    /// Rust files under `crates/` that were read to find them.
    read: usize,
    /// Composition-root files under `crates/` - a `src/main.rs`, or a file that itself composes a
    /// transport.
    roots: usize,
    /// The census's own verdict line, so the file count is a witness rather than a declaration.
    witness: String,
}

/// One `Result` rather than a print-and-return block per failure, so the task name and the
/// paragraph under it are written once - `boot_order`'s shape, for the reason it gives.
fn check() -> Result<Counted, String> {
    let (scanned, files, texts, witness) = inspect_scan(repo::all_files().map_err(|why| why.describe())?)?;
    let read = |path: &str| texts.get(path).cloned();
    door_is_still_defined(&read)?;
    no_bound_hides_behind_an_alias(&files, &read)?;
    let roots = roots(&files, &scanned);
    at_least_one_bound_is_built(&scanned)?;
    every_taker_is_still_called(&scanned)?;
    every_site_is_in_a_composition_root(&scanned, &roots)?;
    at_most_one_bound_per_crate(&scanned)?;
    every_serving_root_builds_one(&scanned, &roots)?;
    Ok(Counted {
        sites: scanned.sites.values().map(Vec::len).sum(),
        read: scanned.read,
        roots: roots.len(),
        witness,
    })
}

/// The scan, the in-scope paths, their text, and the census's verdict line.
type Inspected = Result<(Scan, Vec<String>, BTreeMap<String, String>, String), String>;

/// Every in-scope file read once, by [`repo::Census::inspect`], and [`scan`] run over those bytes.
/// `inspect` counts a listed file that vanished as absent, not unreachable; a gate whose subject is
/// a count refuses it, and refuses a `scan` that read fewer files than the census opened.
fn inspect_scan(census: repo::Census) -> Inspected {
    let mut texts = BTreeMap::new();
    // Lossy, as `check-guidance` reads: a byte that is not UTF-8 never makes an opened file unread.
    let inspected = census
        .inspect(&[], in_scope, |rel, bytes| {
            texts.insert(String::from(rel), String::from_utf8_lossy(bytes).into_owned());
        })
        .map_err(|why| why.describe())?;
    if inspected.absent() != 0 {
        return Err(format!(
            "{} in-scope file(s) vanished after discovery, so the count is incomplete",
            inspected.absent()
        ));
    }
    let files: Vec<String> = texts.keys().cloned().collect();
    let scanned = scan(&files, &|path| texts.get(path).cloned())?;
    if scanned.read != inspected.judged() {
        return Err(format!(
            "scanned {} of {} in-scope file(s) - the walk stopped early",
            scanned.read,
            inspected.judged()
        ));
    }
    Ok((scanned, files, texts, inspected.verdict()))
}

/// What the scan found.
///
/// Keyed by ROOT: a file that composes a transport (or is a crate's `src/main.rs`) is its own key,
/// its own file path, because that is now the finest unit this gate can hold to *one bound* -
/// `github.com/telekom/sutura#685` gave one crate two such files. A site outside every such file
/// falls back to the CRATE name, which is what keeps it caught rather than silently rooted by its
/// own presence. The values keep `path:line` so a failure names the line to open rather than the
/// key to search.
#[derive(Default)]
struct Scan {
    /// Root (or, failing that, crate name) to the construction sites in its non-test code.
    sites: BTreeMap<String, Vec<String>>,
    /// Root file to the files whose non-test code composes a transport - itself, always, since a
    /// file becomes this key BY composing one.
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
    // A file that calls a taker is only its OWN root inside a crate that is a binary at all -
    // `crates/sutura-mcp` calls `AgentSurface::new(` from its own library code (the type's home
    // crate, constructing what it defines) and has no `src/main.rs`; without this restriction that
    // library file would read as a composition root that never builds a bound. Binary status is
    // per CRATE, same as `is_main` reads it, so this is one pass over the listing rather than a
    // second file read.
    let binary_crates: BTreeSet<String> = files.iter().filter(|rel| is_main(rel)).map(|rel| crate_of(rel)).collect();
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
        // A FILE IS ITS OWN ROOT the moment it composes a transport itself, in a crate that is a
        // binary at all - or is a crate's `src/main.rs` outright. Computed once, over the WHOLE
        // file, before either needle's per-line pass, so a door call and a taker call in the same
        // file agree on whose bound it is regardless of which comes first on the page. This is the
        // `github.com/telekom/sutura#685` fix: it used to be enough to ask which CRATE a line was
        // in, because a crate had one such file. `roots` below reads this same signal
        // independently, over the FULL LISTING, which is what keeps a random helper file from
        // rooting itself merely by building a bound - see its own doc. The `binary_crates` guard is
        // what keeps a LIBRARY's own taker call - `sutura-mcp` constructs `AgentSurface::new(` in
        // its own code, and has no `src/main.rs` - from reading as a root that never builds one.
        let is_root_file = is_main(rel)
            || is_bin(rel)
            || (binary_crates.contains(&crate_of(rel))
                && code.iter().enumerate().any(|(index, line)| {
                    !tests.covers(index.saturating_add(1)) && TAKERS.iter().any(|needle| call_count(line, needle) > 0)
                }));
        let owner = if is_root_file { rel.clone() } else { crate_of(rel) };
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

/// Is `rel` a crate's `src/main.rs`?
///
/// Split out of `roots` because [`scan`] needs the same question per file, before it knows
/// whether that file also composes a transport - a `src/main.rs` is a root even if it never calls
/// [`TAKERS`] itself, the same as before this gate went per-file.
fn is_main(rel: &str) -> bool {
    let name = crate_of(rel);
    !name.is_empty() && rel == format!("crates/{name}/src/main.rs")
}

/// Is `rel` one of a crate's OTHER binary targets - `src/bin/*.rs` or `src/bin/*/main.rs` - the
/// shape cargo auto-discovers a `[[bin]]` from with no manifest entry needed?
///
/// **Unconditionally a root, the same as [`is_main`] and for the same reason: each is its own
/// process regardless of what it composes.** Deliberately NOT folded into `binary_crates`: that set
/// exists so a fold like `github.com/telekom/sutura#685`'s can let several files inside ONE binary
/// (`sutura-cli`'s `mcp.rs`, `serve.rs`) each root themselves, because every file under such a
/// crate's `src/` compiles into that SAME single process. A `src/bin/*.rs` file is the opposite
/// shape: a second, SEPARATE binary artifact that a crate's library files (`lib.rs`, `http.rs`) are
/// merely linked BY, never compiled INTO. Adding its crate to `binary_crates` would make every
/// library file that calls a taker - `sutura-mcp`'s `serve_stdio`/`service`, which take an
/// `Admission` as a parameter and are the excused case [`every_serving_root_builds_one`]'s own test
/// names - misread as an unbounded root the day some OTHER file in the same crate grew a `[[bin]]`.
fn is_bin(rel: &str) -> bool {
    let name = crate_of(rel);
    !name.is_empty() && rel.starts_with(&format!("crates/{name}/src/bin/")) && in_scope(rel)
}

/// Which files under `crates/` are composition roots.
///
/// **Derived, and that is the difference from `check-boot-order`'s declared list**: a crate with a
/// `src/main.rs` is a binary and a binary is a process, so a fourth root is covered by this gate the
/// day somebody writes it rather than the day somebody remembers to declare it.
///
/// **A FILE now, not only a crate name - since `github.com/telekom/sutura#685` step 2 folded two
/// binaries' composition roots into one crate.** `found.takers`' own keys are either a FILE that
/// composes a transport in a crate that is itself a binary, or a bare CRATE name - `scan`'s own
/// fallback for a library's taker call, `sutura-mcp` constructing the type it defines being the
/// measured case. Only the file-shaped keys are roots; `contains('/')` is what tells them apart,
/// because a bare crate name under `crates/<name>/...` never holds one. Unioning the file-shaped
/// half with every `src/main.rs` is the same derivation `check-boot-order` always made, one level
/// finer. This is NOT circular: a file that builds a bound but calls no taker itself is
/// attributed to its CRATE name in `found.sites`, never to its own path, so it cannot make itself
/// a root merely by building one - [`every_site_is_in_a_composition_root`] still catches it.
fn roots(files: &[String], found: &Scan) -> BTreeSet<String> {
    files
        .iter()
        .filter(|rel| is_main(rel) || is_bin(rel))
        .cloned()
        .chain(found.takers.keys().filter(|owner| owner.contains('/')).cloned())
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
            "`{owner}` builds an execution bound at {where_} and it is not a composition root - it is \
             neither a crate's `src/main.rs` nor a file that itself composes a transport, so it is a \
             library that some process links. A bound built there is a second permit set in every \
             process that links it beside another one, each reporting a limit the other can exceed. \
             Take an `Admission` as an argument and let the file that serves decide there is one of it"
        ));
    }
    Ok(())
}

/// One root, one bound.
///
/// Per root file rather than per crate line - `github.com/telekom/sutura#685` gave one crate two
/// such files, mutually exclusive at runtime through argv dispatch rather than through being two
/// binaries. What is still true regardless: two sites attributed to the SAME root are two permit
/// sets that root's own process would build, whatever numbers they were built from.
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
             root - either a crate's `src/main.rs`, or a file that composes a transport itself - so it is \
             the one place that can decide there is one bound for this process. Call `{DOOR}` once and \
             hand the value to whatever serves"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod fixtures;

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::fixtures::{GOOD_ROOT, Tree, named, scanned, tree};
    use super::{
        BOUND, DOOR, DOOR_DEFINED_IN, DOOR_SIGNATURE, Scan, TAKERS, at_least_one_bound_is_built, at_most_one_bound_per_crate,
        call_count, crate_of, door_is_still_defined, every_serving_root_builds_one, every_site_is_in_a_composition_root,
        every_taker_is_still_called, inspect_scan, is_bin, located, no_bound_hides_behind_an_alias, roots,
    };

    #[test]
    fn the_tree_itself_passes_and_the_scan_is_not_empty() {
        // Over the REAL files, for the reason `check-boot-order`'s own suite gives: a reader that
        // matches nothing makes its gate pass vacuously. Non-vacuous by construction - more files
        // read than sites found, and every rule asserted rather than the summary.
        let Ok(census) = crate::repo::all_files() else {
            return;
        };
        let (found, files, texts, _) = inspect_scan(census).expect("every Rust file under crates/ is readable");
        let read = |path: &str| texts.get(path).cloned();
        assert_eq!(door_is_still_defined(&read), Ok(()));
        let roots = roots(&files, &found);
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
        let error = every_site_is_in_a_composition_root(&found, &named(&["crates/sutura-serve/src/main.rs"]))
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
        //
        // The crate's own `src/main.rs` has to be IN the listing too, empty is enough: it is what
        // tells `scan` this crate is a binary at all, which is what lets `mcp.rs`'s own taker call
        // root ITSELF rather than falling back to the crate name - the same distinction
        // `crates/sutura-mcp`'s library code needs on the other side of it.
        let found = scanned(&[
            ("crates/sutura-cli/src/main.rs", "fn main() {}\n"),
            (
                "crates/sutura-cli/src/mcp.rs",
                "fn serve() {\n    block_on(sutura_mcp::serve_stdio(service, permitted, prose))\n}\n",
            ),
        ]);
        let error = every_serving_root_builds_one(&found, &named(&["crates/sutura-cli/src/mcp.rs"]))
            .expect_err("a root that serves a transport bounds it");
        assert!(error.contains("composes a transport"), "{error}");
        assert!(error.contains("crates/sutura-cli/src/mcp.rs"), "{error}");
        // And a file that is NOT a root is left alone: `sutura-mcp` itself constructs the handler.
        assert_eq!(every_serving_root_builds_one(&found, &BTreeSet::new()), Ok(()));
    }

    #[test]
    fn a_bin_target_that_builds_no_bound_is_red() {
        // `github.com/telekom/sutura#784`'s review, finding 2: a crate with no `src/main.rs` at all
        // - so `binary_crates` never held it before this test's own fix - grows a REAL second
        // `[[bin]]` (cargo auto-discovers `src/bin/*.rs`, no manifest entry needed) that composes a
        // transport and builds no bound. Before `is_bin` existed this was invisible: `roots` only
        // ever added a bare crate name here, and `every_serving_root_builds_one` skips anything
        // `roots` does not contain.
        assert!(is_bin("crates/sutura-mcp/src/bin/probe_agent_server.rs"));
        assert!(!is_bin("crates/sutura-mcp/src/lib.rs"));
        assert!(
            !is_bin("crates/sutura-mcp/src/main.rs"),
            "src/main.rs is is_main's, not is_bin's"
        );
        let files = [String::from("crates/sutura-mcp/src/bin/probe_agent_server.rs")];
        let found = scanned(&[(
            "crates/sutura-mcp/src/bin/probe_agent_server.rs",
            "fn main() {\n    let a = AgentSurface::new(service, permitted, prose);\n}\n",
        )]);
        let roots = roots(&files, &found);
        assert_eq!(
            roots,
            named(&["crates/sutura-mcp/src/bin/probe_agent_server.rs"]),
            "{roots:?}"
        );
        let error = every_serving_root_builds_one(&found, &roots).expect_err("a second bin target that serves bounds it too");
        assert!(error.contains("composes a transport"), "{error}");
        assert!(error.contains("probe_agent_server.rs"), "{error}");
    }

    #[test]
    fn a_bin_target_does_not_widen_which_library_files_root_themselves() {
        // The guard [`is_bin`]'s own doc names: `sutura-mcp`'s `serve_stdio`/`service` take an
        // `Admission` as a parameter and call `AgentSurface::new(` in library code that is not
        // `src/main.rs` and not `src/bin/*.rs`. That stays excused EVEN WHILE a sibling `[[bin]]`
        // exists in the same crate - `is_bin` never adds the crate to `binary_crates`, unlike
        // `is_main`, so `lib.rs` never falls back into rooting itself just because some other file
        // in the crate happens to be a second binary now.
        let files = [
            String::from("crates/sutura-mcp/src/lib.rs"),
            String::from("crates/sutura-mcp/src/bin/probe_agent_server.rs"),
        ];
        let found = scanned(&[
            (
                "crates/sutura-mcp/src/lib.rs",
                "pub fn serve_stdio(admission: Admission) {\n    let a = AgentSurface::new(service, permitted, prose, admission, reply);\n}\n",
            ),
            (
                "crates/sutura-mcp/src/bin/probe_agent_server.rs",
                "fn main() {\n    let a = AgentSurface::new(service, permitted, prose);\n}\n",
            ),
        ]);
        let roots = roots(&files, &found);
        assert!(
            !roots.contains("crates/sutura-mcp/src/lib.rs"),
            "the excused library shape must stay excused: {roots:?}"
        );
        assert!(roots.contains("crates/sutura-mcp/src/bin/probe_agent_server.rs"), "{roots:?}");
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
            found.sites.get("crates/sutura-serve/src/main.rs").map(Vec::as_slice),
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
        // No taker-callers in this fixture, so `roots` is exactly the two `src/main.rs` paths - the
        // `github.com/telekom/sutura#685` fold added the OTHER half, covered by
        // `a_serving_root_that_builds_no_bound_is_red` and the real-tree test above instead.
        assert_eq!(
            roots(&paths, &Scan::default()),
            named(&["crates/sutura-cli/src/main.rs", "crates/sutura-serve/src/main.rs"])
        );
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

    #[test] // A listed source that disappears must invalidate the verdict, not shrink the count.
    fn a_listed_source_that_vanishes_refuses_the_one_bound_scan() {
        let fixture: &[u8] = b"fn plain() {}\n";
        let tree = crate::scratch_tree::Tree::of(
            "one-bound-vanished",
            &[("crates/a/src/lib.rs", fixture), ("crates/b/src/lib.rs", fixture)],
        );
        let census = crate::repo::collect_files(tree.root(), &tree.root().join("crates"), &["rs"]);
        std::fs::remove_file(tree.root().join("crates/b/src/lib.rs")).expect("a vanished source");
        let refused = inspect_scan(census).is_err_and(|problem| problem.contains("1 in-scope file(s) vanished"));
        assert!(refused, "a listed file that vanished must refuse the scan");
    }
}
