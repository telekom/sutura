//! The boot-order gate: the pre-flight runs after the credential and before the transport.
//!
//! `github.com/telekom/sutura#120` asked for one order to be pinned - the pre-flight that refuses a
//! bundle naming a table the data system does not hold runs **after** the adapters are opened, which
//! is where a credential is read, and **before** the transport a peer reaches this process through is
//! opened. Both halves are decisions about an operator's morning:
//!
//! * Told about a missing table when the credential file is unreadable, they go and edit the catalog,
//!   which was never wrong.
//! * Told about a missing table after the listener is bound, they are told after a caller has already
//!   been informed it may ask questions - which is the failure the whole check exists to remove,
//!   arriving one round trip later.
//!
//! # Why a gate rather than the comment that used to hold it
//!
//! Both composition roots carried the order as prose. `sutura-serve`'s said in so many words that the
//! order was "a convention this line keeps", after review had disproved TWO earlier claims that a type
//! held it - `Warehouses::of` is `pub`, and a `BigQueryWarehouse` is generic in its transport. What
//! survives of the type argument is one half: the `BigQuerySource` alias names a credential whose only
//! public constructor reads a file, so no arrangement of that root can ask a dataset about a table
//! before a credential was read. **Nothing at all held the second half**, and `AGENTS.md` is
//! unambiguous about a sentence in that position: an invariant is held by a type, a lint, a hook or a
//! gate, never by recall.
//!
//! # What it reads
//!
//! Text, in the order it appears, for `pins.rs`'s reason: a gate has to run on a host with no nix and
//! no resolver. Per root it finds three call sites - the one that opens the adapters, the pre-flight,
//! and the one that opens the transport - and requires them in that order. Comments, MULTI-LINE string
//! interiors and test regions come out first, through the two readers the other Rust-reading gates
//! use, so the prose that NAMES these calls cannot satisfy an anchor the code lost.
//!
//! **A single-line string literal is NOT removed**, and that is a deliberate property of the shared
//! lexer rather than an oversight in it: `serde_parse::scan` keeps a one-line string's content because
//! a `try_from = "String"` is one and the value is the point. The consequence here is that a one-line
//! literal naming one of these three calls is a live anchor to this gate - `eprintln!("open_engine(
//! refused")` above the real `open_engine(` loosens the comparison and reads GREEN, and the same
//! literal naming the transport call reads red. Neither root contains such a literal today, checked
//! over every occurrence of all three needles, so this is a limit and not an open hole.
//!
//! # Three limits, stated next to the claim
//!
//! **It is line position, not execution order.** A pre-flight moved into a helper that runs after the
//! listener is bound, or one put behind a condition the serving path does not take, reads the same to
//! this gate. What it does catch is the edit that is actually plausible here: a line moved while a
//! root is restructured, and a rename that leaves the check called from nowhere.
//!
//! **The agent surface's transport anchor is inside `fn serve`**, the helper *both* of that root's
//! arms tail-call, so what is compared there is where that helper is *defined*: moving it above
//! `fn run` would be a false RED. It cannot anchor on that root's own `serve(` call instead, because
//! the `files` arm makes one *before* the pre-flight and legitimately makes no pre-flight at all - its
//! engine is given its tables. A false red is a person reading a message; a false green is nobody
//! reading anything.
//!
//! **A root that omits the pre-flight ENTIRELY is invisible to both halves.** The scan keys on the
//! pre-flight's own name, so it finds the roots that call it and cannot find one that never did, and
//! [`ROOTS`] then has no entry to miss. Holding that wants a different mechanism - the registry
//! threaded through the pre-flight by value, so a root cannot obtain servable engines without one -
//! and that is an architecture decision rather than a line in this gate.
//!
//! # Fails closed, and one of the three ways it does so was a hole
//!
//! An unreadable file, and a call site this gate cannot find, are each a failure naming what it could
//! not find. The third way is the declaration itself, and it was measured rather than argued: with
//! [`ROOTS`] emptied this gate printed `ok - 0 composition root(s)` and exited zero - a dead gate
//! reading as enforcement, which is the one outcome an order-reading gate must not have. So the
//! declaration is compared against a scan of `crates/`, and an emptied list, an undeclared root and a
//! pre-flight renamed out of existence are each red.
//!
//! The scan costs mostly what the LISTING costs, because `repo::all_files` shells out to git twice
//! rather than walking the directory. Measured on this tree, 20 runs of the debug binary: **136 ms
//! each**, against a `hygiene` sweep of six to ten seconds. It is the listing every other
//! Rust-reading gate uses - tracked plus untracked-but-not-ignored, with a full walk as the sandbox
//! fallback - and a gate that judged only what happened to be committed is the wrong trade at that
//! price.

use crate::Verdict;
use crate::causality::regions::{self, PostImage, TestScope};
use crate::repo;
use crate::serde_parse::scan::code_lines;

/// The pre-flight's spelling, and the one needle both halves of this gate use.
///
/// One constant rather than a field per root, and the honest reason is weaker than "one function":
/// the two roots call the same NAME in two crates. There are two definitions, one in
/// `sutura_serve::boot` and one in `sutura_cli::sources::bigquery`, each with its own body and its
/// own suite, both delegating to `sutura_app::preflight::ask`, which is the actually-shared thing.
/// `sutura_cli::sources` re-exports its OWN one, not the serve crate's.
///
/// So this constant rests on a naming convention rather than on a type, which is a more fragile
/// property than it first reads and is exactly why the spelling limit below is stated: a caller is
/// found by matching text, so a caller that spells the name differently is not found at all.
const PREFLIGHT: &str = "refuse_absent_tables";

/// One composition root, and the call sites whose order it keeps.
///
/// Declared rather than discovered, because "the call that opens the transport" is not a shape a scan
/// can recognise - it is one named function per root, and naming it is what makes a rename a failure
/// somebody has to look at.
struct Root {
    /// The root's file, relative to the repo root.
    path: &'static str,
    /// The call that opens the adapters. A credential is read inside it, which is why the pre-flight
    /// goes after it.
    opens: &'static str,
    /// The call that opens the transport a peer reaches this process through.
    serves: &'static str,
    /// What that transport is, for a message an operator reads once and acts on.
    transport: &'static str,
}

/// Every root that makes a pre-flight. A root added without an entry here is caught by
/// [`every_caller_is_declared`]; one whose call sites are renamed is caught by [`ordered`].
///
/// **The catch is spelling-bound, and one spelling is now refused rather than merely admitted.**
/// [`every_caller_is_declared`] finds a caller by matching [`PREFLIGHT`] as text, so a third root
/// that called the pre-flight under a different spelling would leave `found` equal to the same two
/// files - non-empty, all declared, `ok` - which is the very hole that check exists to close. Two
/// spellings reach it:
///
/// * a **turbofish**, `refuse_absent_tables::<W>(...)`. Now caught: [`PREFLIGHT`] is the bare name,
///   so it no longer depends on the call's next character being `(`.
/// * an **alias**, `use crate::sources::refuse_absent_tables as preflight;`. Text matching cannot
///   follow a rename, so this is refused outright by [`no_caller_hides_behind_an_alias`] instead -
///   and it is one keystroke from an idiom already in this tree, since `sutura_cli::sources`
///   re-exports this function precisely because a root wanted a shorter path.
///
/// What remains uncovered, stated because the rest of this module is about not overstating a gate: a
/// root that calls the pre-flight through a function pointer, a trait method or a macro-generated
/// call. Nothing in this tree does, and no text scan could see it.
const ROOTS: &[Root] = &[
    Root {
        path: "crates/sutura-serve/src/main.rs",
        opens: "open_engine(",
        serves: "serve_until_stopped(",
        transport: "the HTTP listener",
    },
    Root {
        path: "crates/sutura-cli/src/mcp.rs",
        opens: "open_engine(",
        serves: "sutura_mcp::serve_stdio(",
        transport: "the agent surface's pipes",
    },
];

pub(crate) fn run(_args: &[String]) -> Verdict {
    let scanned = match check() {
        Ok(scanned) => scanned,
        Err(why) => {
            eprintln!("xtask check-boot-order: {why}");
            eprintln!();
            eprintln!("An operator told about a missing table while the credential is unreadable edits");
            eprintln!("the catalog, which was never wrong; one told after the transport is open is told");
            eprintln!("after a caller has been invited to ask questions. github.com/telekom/sutura#120.");
            return Verdict::Fail;
        }
    };
    println!(
        "xtask check-boot-order: ok - {} composition root(s) call the pre-flight in {} file(s) under crates/, all declared, each with its call site after the credential's and before the transport's",
        scanned.callers, scanned.read
    );
    Verdict::Pass
}

/// Which roots there are, and whether each one keeps the order. Returns what the SCAN counted - both
/// numbers - which is what puts a scan that read nothing in the verdict line rather than leaving it
/// implied.
///
/// The caller count printed is the scan's and not `ROOTS.len()`. The two are provably equal by the
/// time this returns, since [`ordered`] fails a declared root that does not call the pre-flight and
/// [`every_caller_is_declared`] fails a caller that is not declared - so the old line was honest. It
/// is the scan's number anyway, because this gate's entire history is a verdict line claiming more
/// than was measured, and printing the measurement costs nothing.
///
/// One `Result` rather than a print-and-return block per failure: the task name and the paragraph
/// under it are then written once, which is `api_docs`'s shape and the reason it has it.
fn check() -> Result<Counted, String> {
    let (root, files) = repo::all_files().and_then(|census| census.into_listing(repo::Unmigrated::BootOrder)).map_err(|why| why.describe())?;
    let read = |path: &str| std::fs::read_to_string(root.join(path)).ok();
    let found = scan(&files, &read)?;
    every_caller_is_declared(&declared(), &found.callers)?;
    // After the omission cross-check and not before it: this one exists to keep that check's text
    // matching honest, so it reads as the guard on the line above rather than a rule of its own.
    no_caller_hides_behind_an_alias(&files, &read)?;
    for composition in ROOTS {
        let text = read(composition.path).ok_or_else(|| {
            format!(
                "could not read {}, and it is a declared composition root - so this gate has read NO \
                 order for it. That is a failure on purpose",
                composition.path
            )
        })?;
        ordered(composition, &text, &regions::scope(composition.path, &read))?;
    }
    Ok(Counted {
        callers: found.callers.len(),
        read: found.read,
    })
}

/// The paths [`ROOTS`] declares, for comparison against what the tree actually calls.
fn declared() -> Vec<&'static str> {
    ROOTS.iter().map(|composition| composition.path).collect()
}

/// What the scan found.
///
/// A named pair rather than a tuple, because `type_complexity` is tightened in this workspace and
/// because a reader of the verdict line has to be able to tell the two numbers apart.
struct Scan {
    /// The files whose code calls the pre-flight, repo-relative.
    callers: Vec<String>,
    /// How many Rust files under `crates/` were read to find them.
    read: usize,
}

/// What the scan counted, so the verdict line prints a measurement rather than a declaration.
struct Counted {
    /// Files whose code calls the pre-flight.
    callers: usize,
    /// Rust files under `crates/` that were read to find them.
    read: usize,
}

/// An alias defeats the omission cross-check, so a `use ... as` on the pre-flight is refused.
///
/// [`every_caller_is_declared`] finds a caller by matching [`PREFLIGHT`] as text. A rename at the
/// import - `use crate::sources::refuse_absent_tables as preflight;` - means the call site spells
/// something this gate has never heard of, so a third root written that way leaves `found` equal to
/// the two files already declared: non-empty, all declared, `ok`. That is the omission the scan
/// exists to catch, walking straight past it.
///
/// Text matching cannot follow a rename, so the rename is refused instead of chased. The cost is one
/// forbidden idiom, and the alternative was a limit nobody would read: this tree already re-exports
/// this function once, so the aliasing form is one keystroke away.
///
/// Not a style rule - it is scoped to this one name, and only to a form that renames it.
fn no_caller_hides_behind_an_alias(files: &[String], read: &PostImage<'_>) -> Result<(), String> {
    for rel in files.iter().filter(|rel| in_scope(rel)) {
        let Some(text) = read(rel) else { continue };
        if !text.contains(PREFLIGHT) {
            continue;
        }
        let tests = regions::scope(rel, read);
        for (index, line) in code_lines(&text).iter().enumerate() {
            let number = index.saturating_add(1);
            if tests.covers(number) {
                continue;
            }
            if imports(line) && line.contains(PREFLIGHT) && line.contains(" as ") {
                return Err(format!(
                    "{rel}:{number} imports `{PREFLIGHT}` under another name. This gate finds a root \
                     that omitted the pre-flight by matching that name as text, so a call spelled \
                     differently is invisible to it - import the name as itself, or make the order a \
                     type rather than a citation"
                ));
            }
        }
    }
    Ok(())
}

/// Every file whose CODE calls the pre-flight, and how many files were read.
///
/// [`ROOTS`] is what makes a rename a failure; this is what makes an OMISSION one, and the module
/// header records what the omission looked like while nothing checked it.
///
/// A file that cannot be read is a failure and not a skip, because a scan that quietly shrank is how a
/// third root goes unnoticed. Test code comes out by DECLARATION rather than by a name guess -
/// `regions::scope` resolves `crates/sutura-serve/src/tests.rs` through the `#[cfg(test)] mod tests;`
/// in its parent, which no rule about the file's own name can see.
fn scan(files: &[String], read: &PostImage<'_>) -> Result<Scan, String> {
    let mut callers = Vec::new();
    let mut count = 0_usize;
    for rel in files.iter().filter(|rel| in_scope(rel)) {
        let text = read(rel).ok_or_else(|| {
            format!("could not read {rel}, so the scan that decides WHICH roots this gate checks is incomplete")
        })?;
        count = count.saturating_add(1);
        // Before the lexer, because it only ever REMOVES text: a file whose raw bytes do not carry the
        // call cannot carry it once comments and string interiors are blanked. **Four** files under
        // `crates/` carry the needle and 211 do not, so this skips the lex for all but four.
        //
        // Four and not two, and the difference is instructive: this is a raw `contains`, so it admits
        // the two files whose only occurrences are inside `#[cfg(test)]` regions. That is a
        // POST-LEXER fact and a `contains` cannot know it. An earlier version of this comment read
        // "two", derived from the verdict line's own `2` - but that number counts CALLERS, after the
        // lexer and the region scan, so the derivation was the error rather than the count.
        // `git grep -l "refuse_absent_tables" -- 'crates/**/*.rs'` is the check.
        if !text.contains(PREFLIGHT) {
            continue;
        }
        if call_line(&code_lines(&text), &regions::scope(rel, read), PREFLIGHT).is_some() {
            callers.push(rel.clone());
        }
    }
    Ok(Scan { callers, read: count })
}

/// Is this a Rust file that could be a composition root?
///
/// `crates/` only. A root is a crate, and this gate's own fixture names the pre-flight in `xtask/`.
fn in_scope(rel: &str) -> bool {
    rel.starts_with("crates/")
        && std::path::Path::new(rel)
            .extension()
            .and_then(std::ffi::OsStr::to_str)
            .is_some_and(|ext| ext.eq_ignore_ascii_case("rs"))
}

/// Whether the roots this gate declares are the files that call the pre-flight.
///
/// Two failures, and neither is the one [`ordered`] reports. **An empty scan** means this gate has read
/// no order at all, which is the outcome it must never print `ok` for. **A caller nobody declared** is
/// a serving path whose boot order is held by nothing - the case [`ROOTS`] used to leave to review.
///
/// The other direction - a declared root the tree does not call - is deliberately NOT here: [`ordered`]
/// meets it as a call site it cannot find, and that message already names both files to look at.
fn every_caller_is_declared(declared: &[&str], found: &[String]) -> Result<(), String> {
    if found.is_empty() {
        return Err(format!(
            "no file under crates/ calls `{PREFLIGHT}` outside comments and tests, so this gate has read \
             NO order at all. Either the pre-flight that refuses a bundle naming an absent table is gone \
             - in which case so is the refusal - or it was renamed and `PREFLIGHT` in \
             xtask/src/boot_order.rs has to be renamed with it. A scan that finds nothing does not get to \
             say `ok`"
        ));
    }
    if let Some(undeclared) = found.iter().find(|path| !declared.contains(&path.as_str())) {
        return Err(format!(
            "{undeclared} calls `{PREFLIGHT}` and is not in `ROOTS` in xtask/src/boot_order.rs, so the \
             order that root boots in is held by nothing. Add it, naming the call that opens its adapters \
             and the call that opens its transport"
        ));
    }
    Ok(())
}

/// Whether one root's three call sites appear in the order it has to keep.
///
/// Separated from [`check`] so both verdicts are reachable with no tree to read - which is the half of a
/// gate that is otherwise only ever exercised green.
fn ordered(root: &Root, text: &str, tests: &TestScope) -> Result<(), String> {
    let code = code_lines(text);
    let opens = call_line(&code, tests, root.opens).ok_or_else(|| missing(root, root.opens, "opens the adapters"))?;
    let preflight = call_line(&code, tests, PREFLIGHT).ok_or_else(|| missing(root, PREFLIGHT, "is the pre-flight"))?;
    let serves = call_line(&code, tests, root.serves).ok_or_else(|| missing(root, root.serves, "opens the transport"))?;
    if preflight < opens {
        return Err(format!(
            "{}: the pre-flight is BEFORE the adapters are opened - `{PREFLIGHT}` on line {preflight}, `{}` \
             on line {opens}. The credential is read inside the second, so this order tells an operator \
             about a table when their credential file is what is wrong",
            root.path, root.opens
        ));
    }
    if serves < preflight {
        return Err(format!(
            "{}: the pre-flight is AFTER the transport opens - `{}` ({}) on line {serves}, `{PREFLIGHT}` on \
             line {preflight}. A bundle naming a table the data system does not hold has to stop the \
             process, not one question asked through a transport this process already opened",
            root.path, root.serves, root.transport
        ));
    }
    Ok(())
}

/// What to say about a call site this gate cannot find.
///
/// It names the spelling it looked for and the role it fills, because the fix is either a rename in
/// the root or a rename in [`ROOTS`] and the reader has to be able to tell which.
fn missing(root: &Root, call: &str, role: &str) -> String {
    format!(
        "{}: no `{call}` call site outside comments and tests, and that is the call that {role}. \
         Either this root no longer makes it - in which case the order this gate holds is gone - or it \
         was renamed and the spelling in xtask/src/boot_order.rs has to be renamed with it. A gate that \
         cannot find the lines does not get to say `ok`",
        root.path
    )
}

/// The line one call is first made on, outside comments, string interiors and test code.
///
/// **A definition is not a call**, and that is not a hypothetical distinction: `fn open_engine(` lives
/// in the same file as the call to it in one of the two roots here, and a root reordered so the
/// definition came first would otherwise be read as making the call at the top of the file.
///
/// **The FIRST call site**, so a root that opens its adapters twice is read at the first of them.
fn call_line(code: &[String], tests: &TestScope, call: &str) -> Option<usize> {
    code.iter().enumerate().find_map(|(index, line)| {
        let number = index.saturating_add(1);
        let called = line.match_indices(call).any(|(at, _)| !defines(line, at));
        (called && !tests.covers(number)).then_some(number)
    })
}

/// Whether the occurrence at `at` is a DEFINITION or an IMPORT rather than a call to it.
///
/// Two shapes, and the second arrived with the needle. [`PREFLIGHT`] is the bare name so a turbofish
/// cannot evade the scan, and the cost of dropping the `(` is that `fn refuse_absent_tables<W>` and
/// `use crate::sources::refuse_absent_tables;` now match it too - neither of which calls anything.
/// The definitions live in the two crates that own them and the import sits in a root that is already
/// declared, so admitting either would report a caller that is not one.
///
/// `ends_with("fn")` is checked on the whitespace-trimmed prefix, so `pub(crate) fn` is a definition
/// and a variable ending in the letters `fn` is not - the token is compared, not the suffix.
///
/// **`pub(crate)` because `one_bound` reads for the same two shapes**, and one place knowing what a
/// definition and a `use` item look like to a text scan is this module's own argument about its
/// parser one gate over.
pub(crate) fn defines(line: &str, at: usize) -> bool {
    let before = line.get(..at).unwrap_or_default().trim_end();
    before.split_whitespace().last() == Some("fn") || imports(line)
}

/// Is this line a `use` item?
///
/// The visibility prefix is the part worth spelling out: this tree's re-export is
/// `pub(crate) use bigquery::refuse_absent_tables;`, so a `starts_with("use ")` misses it and reports
/// the re-export as a third caller - which is a red gate on an honest tree. Tokens are compared
/// rather than the string prefixed, so `pub`, `pub(crate)` and `pub(super)` are all one case.
pub(crate) fn imports(line: &str) -> bool {
    let mut tokens = line.split_whitespace();
    match tokens.next() {
        Some("use") => true,
        Some(first) if first.starts_with("pub") => tokens.next() == Some("use"),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        PREFLIGHT, ROOTS, Root, TestScope, call_line, code_lines, declared, every_caller_is_declared,
        no_caller_hides_behind_an_alias, ordered, regions, scan,
    };

    /// The HTTP root's path, which the fixture below stands in for.
    const SERVE: &str = "crates/sutura-serve/src/main.rs";

    /// A composition root in miniature, with every decoy the real ones have: the order stated in
    /// prose, the pre-flight named in a line comment AND in a block comment, a definition above the
    /// call, and a test module underneath that makes the same call for its own reasons.
    ///
    /// A raw string rather than a `concat!`, so the assertions that name a line number can be read
    /// off the gutter. The content starts immediately after the quote: line 1 is the `//!`.
    const ROOT: &str = r#"//! It runs after `open_engine(` and before `serve_until_stopped(`, and this line is prose.
fn open_engine(pinned: &Pinned) -> Result<Opened, String> {
    Err(String::from("a fixture opens nothing"))
}
fn run() -> Result<(), String> {
    let opened = open_engine(&pinned)?;
    /* boot::refuse_absent_tables( in a block comment, which the line-comment rule never saw. */
    // boot::refuse_absent_tables( is named here in prose, and this line is not a call.
    boot::refuse_absent_tables(&pinned, &engines)?;
    runtime.block_on(serve_until_stopped(router, address))
}
#[cfg(test)]
mod tests {
    fn a_fixture() { boot::refuse_absent_tables(&pinned, &engines); }
}
"#;

    /// The fixture's one real call site, line 9. Named once, because a `replace` that misses its
    /// target is a silent no-op.
    const PREFLIGHT_LINE: &str = "    boot::refuse_absent_tables(&pinned, &engines)?;\n";

    /// The call that opens the transport, line 10.
    const SERVES_LINE: &str = "    runtime.block_on(serve_until_stopped(router, address))\n";

    /// The call that opens the adapters, line 6.
    const OPENS_LINE: &str = "    let opened = open_engine(&pinned)?;\n";

    fn serve_root() -> &'static Root {
        ROOTS
            .iter()
            .find(|root| root.path == SERVE)
            .expect("the HTTP root is declared")
    }

    /// The fixture's test regions, resolved the way a real root's are.
    fn tests_in(text: &str) -> TestScope {
        regions::scope(SERVE, &|path| (path == SERVE).then(|| String::from(text)))
    }

    #[test]
    fn every_declared_root_still_reads_as_ordered() {
        // Over the REAL files, for the reason `check-warm-start`'s own suite gives: a reader that
        // matches nothing makes its gate pass vacuously. This asserts each root is still shaped the
        // way the gate reads it AND that the order it keeps is the order issue 120 asked for.
        let root = crate::repo::root().expect("the repo root");
        let read = |path: &str| std::fs::read_to_string(root.join(path)).ok();
        for composition in ROOTS {
            let text = read(composition.path).expect("a declared composition root is readable");
            let tests = regions::scope(composition.path, &read);
            assert_eq!(ordered(composition, &text, &tests), Ok(()), "{}", composition.path);
        }
    }

    /// The needle is the bare name, so the spelling a turbofish produces is still a call.
    ///
    /// Red against the previous needle by construction: it was `refuse_absent_tables(`, and
    /// `refuse_absent_tables::<W>(` does not contain it - so a third root written this way was
    /// invisible to the omission cross-check that exists to find exactly that root.
    #[test]
    fn a_turbofish_call_is_still_a_call() {
        let code = code_lines("    boot::refuse_absent_tables::<W>(&pinned, &engines)?;\n");
        assert_eq!(call_line(&code, &TestScope::Regions(Vec::new()), PREFLIGHT), Some(1));
    }

    /// A definition and an import are not calls, which the bare needle would otherwise admit.
    #[test]
    fn a_definition_and_an_import_are_not_calls() {
        for line in [
            "pub(crate) fn refuse_absent_tables<W>(pinned: &Pinned) -> Result<(), String> {\n",
            "use crate::sources::refuse_absent_tables;\n",
            "    use crate::boot::refuse_absent_tables;\n",
        ] {
            let code = code_lines(line);
            assert_eq!(call_line(&code, &TestScope::Regions(Vec::new()), PREFLIGHT), None, "{line}");
        }
    }

    /// The alias form is refused, because text matching cannot follow a rename.
    #[test]
    fn importing_the_preflight_under_another_name_is_refused() {
        let aliased = "use crate::sources::refuse_absent_tables as preflight;\nfn run() { preflight(&p, &e); }\n";
        let files = vec![String::from("crates/sutura-cli/src/other.rs")];
        let read = |path: &str| (path == "crates/sutura-cli/src/other.rs").then(|| String::from(aliased));

        let refused = no_caller_hides_behind_an_alias(&files, &read);
        let why = refused.expect_err("an aliased import defeats the cross-check and must be refused");
        assert!(why.contains("under another name"), "{why}");
        assert!(why.contains("crates/sutura-cli/src/other.rs:1"), "{why}");
    }

    /// And the honest import is NOT refused - the rule is scoped to a rename, not to importing.
    ///
    /// Without this the check would ban the re-export this tree already has, which is the shape that
    /// turns a targeted refusal into a style rule nobody can satisfy.
    #[test]
    fn importing_the_preflight_as_itself_is_allowed() {
        let plain = "pub(crate) use bigquery::refuse_absent_tables;\n";
        let files = vec![String::from("crates/sutura-cli/src/sources.rs")];
        let read = |path: &str| (path == "crates/sutura-cli/src/sources.rs").then(|| String::from(plain));
        assert_eq!(no_caller_hides_behind_an_alias(&files, &read), Ok(()));
    }

    /// The real tree carries no alias, so the check above is not vacuous on it.
    #[test]
    fn the_tree_itself_holds_no_aliased_import_of_the_preflight() {
        let Ok((root, files)) = crate::repo::all_files()
            .and_then(|census| census.into_listing(crate::repo::Unmigrated::BootOrder))
        else {
            return;
        };
        let read = |path: &str| std::fs::read_to_string(root.join(path)).ok();
        assert_eq!(no_caller_hides_behind_an_alias(&files, &read), Ok(()));
    }

    #[test]
    fn every_file_that_calls_the_preflight_is_a_declared_root() {
        // The OMISSION half, over the real tree: a third root that calls the pre-flight would be a
        // serving path whose order nothing reads. Non-vacuous by construction - the scan has to have
        // read more files than it found roots in, and `every_caller_is_declared` fails on an empty one.
        let (root, files) = crate::repo::all_files().and_then(|census| census.into_listing(crate::repo::Unmigrated::BootOrder)).expect("the repo root");
        let read = |path: &str| std::fs::read_to_string(root.join(path)).ok();
        let found = scan(&files, &read).expect("every Rust file under crates/ is readable");
        assert_eq!(
            every_caller_is_declared(&declared(), &found.callers),
            Ok(()),
            "{:?}",
            found.callers
        );
        assert_eq!(found.callers.len(), ROOTS.len(), "{:?}", found.callers);
        assert!(
            found.read > found.callers.len(),
            "a scan of {} file(s) is not this tree",
            found.read
        );
    }

    #[test]
    fn an_emptied_declaration_is_red_rather_than_ok() {
        // MEASURED, not hypothesised: with `ROOTS` emptied this gate printed
        // `ok - 0 composition root(s)` and exited zero, which is a dead gate reading as enforcement.
        let found = vec![String::from(SERVE)];
        let error = every_caller_is_declared(&[], &found).expect_err("a declaration that names no root checks none");
        assert!(error.contains(SERVE), "{error}");
        assert!(error.contains("held by nothing"), "{error}");
    }

    #[test]
    fn a_tree_that_calls_the_preflight_nowhere_is_red() {
        // The other way to check nothing: the pre-flight renamed, or deleted, so the scan is empty.
        let error = every_caller_is_declared(&[SERVE], &[]).expect_err("a scan that found nothing has read no order");
        assert!(error.contains("read NO order at all"), "{error}");
        assert!(error.contains(PREFLIGHT), "{error}");
        // Both sides empty is the same failure and not a quiet agreement.
        assert!(
            every_caller_is_declared(&[], &[]).is_err(),
            "an empty gate over an empty scan"
        );
    }

    #[test]
    fn a_preflight_after_the_transport_opens_is_red() {
        // THE half nothing held before this gate: the refusal has to stop the process, not one
        // question asked through a transport that is already open.
        let moved = ROOT.replace(PREFLIGHT_LINE, "").replace(
            SERVES_LINE,
            "    runtime.block_on(serve_until_stopped(router, address));\n    boot::refuse_absent_tables(&pinned, &engines)\n",
        );
        let error =
            ordered(serve_root(), &moved, &tests_in(&moved)).expect_err("a pre-flight after the listener opens is not the order");
        assert!(error.contains("AFTER the transport opens"), "{error}");
        assert!(error.contains("the HTTP listener"), "{error}");
    }

    #[test]
    fn a_preflight_before_the_adapters_are_opened_is_red() {
        // The other half, and it is the one review kept mistaking for a property of a type: an
        // operator told about a table when their credential file is unreadable fixes the catalog.
        let early = ROOT
            .replace(PREFLIGHT_LINE, "")
            .replace(OPENS_LINE, &format!("{PREFLIGHT_LINE}{OPENS_LINE}"));
        let error =
            ordered(serve_root(), &early, &tests_in(&early)).expect_err("a pre-flight before the credential is not the order");
        assert!(error.contains("BEFORE the adapters are opened"), "{error}");
    }

    #[test]
    fn a_call_site_this_gate_cannot_find_is_red_rather_than_green() {
        // A rename that leaves the check called from nowhere is the failure this gate is most likely
        // to meet, and the message has to say which of the two files to edit.
        let renamed = ROOT.replace("boot::refuse_absent_tables(", "boot::refuse_tables(");
        let error =
            ordered(serve_root(), &renamed, &tests_in(&renamed)).expect_err("a root that makes no pre-flight is not verified");
        assert!(error.contains("is the pre-flight"), "{error}");
        assert!(error.contains("xtask/src/boot_order.rs"), "{error}");
        let bare = "fn main() {}";
        let no_root =
            ordered(serve_root(), bare, &tests_in(bare)).expect_err("a file with none of the three calls checks nothing");
        assert!(no_root.contains("opens the adapters"), "{no_root}");
    }

    #[test]
    fn neither_prose_nor_a_block_comment_nor_a_test_module_is_a_call_site() {
        // Three decoys, and the order is stated in prose beside every one of these calls - that prose
        // is why the gate exists. Line 9 is the only real call: 7 is a block comment, 8 is a line
        // comment, 14 is under `mod tests`.
        assert_eq!(
            call_line(&code_lines(ROOT), &tests_in(ROOT), PREFLIGHT),
            Some(9),
            "the call on line 9, not the block comment on 7, the prose on 8 or the fixture on 14"
        );
        let decoys_only = ROOT.replace(PREFLIGHT_LINE, "");
        assert_eq!(
            call_line(&code_lines(&decoys_only), &tests_in(&decoys_only), PREFLIGHT),
            None,
            "with the one real call removed, three mentions of it remain and none is a call site"
        );
    }

    #[test]
    fn a_definition_above_the_call_is_not_read_as_the_call() {
        // `fn open_engine(` really does live above its call site in one of the two roots, so reading
        // a signature as a call would report an order that is not the one the process runs in.
        let opens = call_line(&code_lines(ROOT), &tests_in(ROOT), "open_engine(").expect("the call is found");
        assert_eq!(opens, 6, "the CALL on line 6, not the signature on line 2");
    }
}
