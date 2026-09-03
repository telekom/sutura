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
//! # What it reads, and what that is worth
//!
//! Text, in the order it appears, for `pins.rs`'s reason: a gate has to run on a host with no nix and
//! no resolver. Per root it finds three call sites - the one that opens the adapters, the pre-flight,
//! and the one that opens the transport - and requires them in that order.
//!
//! **The limit, stated next to the claim.** This is the order of three lines in one file, not
//! execution order. A pre-flight moved into a helper that runs after the listener is bound, or one put
//! behind a condition the serving path does not take, reads the same to this gate. What it does catch
//! is the edit that is actually plausible here: a line moved while a root is restructured, and a
//! rename that leaves the check called from nowhere - the second being the not-found half, which is a
//! failure rather than a pass.
//!
//! **Fails closed, like its neighbours.** An unreadable root, or a call site this gate cannot find, is
//! a FAILURE naming what it could not find. An order-reading gate's worst outcome is to stop finding
//! the lines and say `ok`.

use crate::Verdict;
use crate::repo;

/// One composition root, and the three call sites whose order it keeps.
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
    /// The pre-flight call this root makes.
    preflight: &'static str,
    /// The call that opens the transport a peer reaches this process through.
    serves: &'static str,
    /// What that transport is, for a message an operator reads once and acts on.
    transport: &'static str,
}

/// Every root that makes a pre-flight. A root added without an entry here is caught by review; one
/// whose call sites are renamed is caught by this gate.
const ROOTS: &[Root] = &[
    Root {
        path: "crates/sutura-serve/src/main.rs",
        opens: "open_engine(",
        preflight: "boot::refuse_absent_tables(",
        serves: "serve_until_stopped(",
        transport: "the HTTP listener",
    },
    Root {
        path: "crates/sutura-cli/src/mcp.rs",
        opens: "open_engine(",
        preflight: "refuse_absent_tables(",
        serves: "sutura_mcp::serve_stdio(",
        transport: "the agent surface's pipes",
    },
];

pub(crate) fn run(_args: &[String]) -> Verdict {
    let Some(root) = repo::root() else {
        eprintln!("xtask check-boot-order: could not locate the repo root");
        return Verdict::Fail;
    };
    let mut checked = 0_usize;
    for composition in ROOTS {
        let text = match std::fs::read_to_string(root.join(composition.path)) {
            Ok(text) => text,
            Err(error) => {
                eprintln!("xtask check-boot-order: could not read {}: {error}", composition.path);
                eprintln!();
                eprintln!("This gate reads the order of three calls in a composition root and could not");
                eprintln!("read one of them, so it has checked NOTHING. That is a failure on purpose.");
                return Verdict::Fail;
            }
        };
        if let Err(why) = ordered(composition, &text) {
            eprintln!("xtask check-boot-order: {why}");
            eprintln!();
            eprintln!("An operator told about a missing table while the credential is unreadable edits");
            eprintln!("the catalog, which was never wrong; one told after the transport is open is told");
            eprintln!("after a caller has been invited to ask questions. github.com/telekom/sutura#120.");
            return Verdict::Fail;
        }
        checked = checked.saturating_add(1);
    }
    println!(
        "xtask check-boot-order: ok - {checked} composition root(s), the pre-flight after the credential and before the transport"
    );
    Verdict::Pass
}

/// Whether one root's three call sites appear in the order it has to keep.
///
/// Separated from [`run`] so both verdicts are reachable with no tree to read - which is the half of a
/// gate that is otherwise only ever exercised green.
fn ordered(root: &Root, text: &str) -> Result<(), String> {
    let code = code_lines(text);
    let opens = call_line(&code, root.opens).ok_or_else(|| missing(root, root.opens, "opens the adapters"))?;
    let preflight = call_line(&code, root.preflight).ok_or_else(|| missing(root, root.preflight, "is the pre-flight"))?;
    let serves = call_line(&code, root.serves).ok_or_else(|| missing(root, root.serves, "opens the transport"))?;
    if preflight < opens {
        return Err(format!(
            "{}: the pre-flight is BEFORE the adapters are opened - `{}` on line {preflight}, `{}` on line {opens}. \
             The credential is read inside the second, so this order tells an operator about a table when \
             their credential file is what is wrong",
            root.path, root.preflight, root.opens
        ));
    }
    if serves < preflight {
        return Err(format!(
            "{}: the pre-flight is AFTER {} opens - `{}` on line {serves}, `{}` on line {preflight}. \
             A bundle naming a table the data system does not hold has to stop the process, not one question \
             asked through a transport this process already opened",
            root.path, root.transport, root.serves, root.preflight
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
         was renamed and `ROOTS` in xtask/src/boot_order.rs has to be renamed with it. A gate that \
         cannot find the lines does not get to say `ok`",
        root.path
    )
}

/// The lines of a composition root that are code, numbered from one.
///
/// **A comment line is dropped and a trailing comment is cut**, because every call site this gate
/// reads is also NAMED in the prose beside it - that prose is the reason the gate exists - and a
/// paragraph must not be able to satisfy an anchor the code lost.
///
/// **Everything from `mod tests` on is dropped**, which is the same rule pointed the other way: a
/// fixture in a test module must not stand in for the serving path. It is `mod tests` rather than
/// `#[cfg(test)]` deliberately - `sutura-serve`'s root has a `#[cfg(test)] const` two hundred lines
/// above `run`, so cutting at the attribute would cut the whole file and the gate would find nothing.
fn code_lines(text: &str) -> Vec<(usize, &str)> {
    let mut lines = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("mod tests") {
            break;
        }
        if trimmed.starts_with("//") {
            continue;
        }
        let code = line.find("// ").map_or(line, |at| line.get(..at).unwrap_or_default());
        lines.push((index.saturating_add(1), code));
    }
    lines
}

/// The line one call is first made on.
///
/// **A definition is not a call**, and that is not a hypothetical distinction: `fn open_engine(` lives
/// in the same file as the call to it in one of the two roots here, and a root reordered so the
/// definition came first would otherwise be read as making the call at the top of the file.
fn call_line(code: &[(usize, &str)], call: &str) -> Option<usize> {
    code.iter()
        .find(|(_, line)| line.match_indices(call).any(|(at, _)| !defines(line, at)))
        .map(|(number, _)| *number)
}

/// Whether the occurrence at `at` is this call's own signature rather than a call to it.
fn defines(line: &str, at: usize) -> bool {
    line.get(..at).unwrap_or_default().trim_end().ends_with("fn")
}

#[cfg(test)]
mod tests {
    use super::{ROOTS, Root, code_lines, ordered};

    /// A composition root in miniature, with the decoys the real ones have: the order stated in
    /// prose, the pre-flight named in a comment, a definition above the call, and a test module
    /// underneath that makes the same call for its own reasons.
    const ROOT: &str = concat!(
        "//! It runs after `open_engine(` and before `serve_until_stopped(`, and this line is prose.\n",
        "fn open_engine(pinned: &Pinned) -> Result<Opened, String> {\n",
        "    Err(String::from(\"a fixture opens nothing\"))\n",
        "}\n",
        "fn run() -> Result<(), String> {\n",
        "    let opened = open_engine(&pinned)?;\n",
        "    // boot::refuse_absent_tables( is named here in prose, and this line is not a call.\n",
        "    boot::refuse_absent_tables(&pinned, &engines)?;\n",
        "    runtime.block_on(serve_until_stopped(router, address))\n",
        "}\n",
        "#[cfg(test)]\n",
        "mod tests {\n",
        "    fn a_fixture() { boot::refuse_absent_tables(&pinned, &engines); }\n",
        "}\n",
    );

    fn serve_root() -> &'static Root {
        ROOTS
            .iter()
            .find(|root| root.path == "crates/sutura-serve/src/main.rs")
            .expect("the HTTP root is declared")
    }

    #[test]
    fn every_declared_root_still_reads_as_ordered() {
        // Over the REAL files, for the reason `check-warm-start`'s own suite gives: a reader that
        // matches nothing makes its gate pass vacuously. This asserts each root is still shaped the
        // way the gate reads it AND that the order it keeps is the order issue 120 asked for.
        let root = crate::repo::root().expect("the repo root");
        for composition in ROOTS {
            let text = std::fs::read_to_string(root.join(composition.path)).expect("a declared composition root is readable");
            assert_eq!(ordered(composition, &text), Ok(()), "{}", composition.path);
        }
    }

    #[test]
    fn a_preflight_after_the_transport_opens_is_red() {
        // THE half nothing held before this gate: the refusal has to stop the process, not one
        // question asked through a transport that is already open.
        let moved = ROOT
            .replace("    boot::refuse_absent_tables(&pinned, &engines)?;\n", "")
            .replace(
                "    runtime.block_on(serve_until_stopped(router, address))\n",
                "    runtime.block_on(serve_until_stopped(router, address));\n    boot::refuse_absent_tables(&pinned, &engines)\n",
            );
        let error = ordered(serve_root(), &moved).expect_err("a pre-flight after the listener opens is not the order");
        assert!(error.contains("AFTER the HTTP listener opens"), "{error}");
    }

    #[test]
    fn a_preflight_before_the_adapters_are_opened_is_red() {
        // The other half, and it is the one review kept mistaking for a property of a type: an
        // operator told about a table when their credential file is unreadable fixes the catalog.
        let early = ROOT
            .replace("    boot::refuse_absent_tables(&pinned, &engines)?;\n", "")
            .replace(
                "    let opened = open_engine(&pinned)?;\n",
                "    boot::refuse_absent_tables(&pinned, &engines)?;\n    let opened = open_engine(&pinned)?;\n",
            );
        let error = ordered(serve_root(), &early).expect_err("a pre-flight before the credential is not the order");
        assert!(error.contains("BEFORE the adapters are opened"), "{error}");
    }

    #[test]
    fn a_call_site_this_gate_cannot_find_is_red_rather_than_green() {
        // A rename that leaves the check called from nowhere is the failure this gate is most likely
        // to meet, and the message has to say which of the two files to edit.
        let renamed = ROOT.replace("boot::refuse_absent_tables(", "boot::refuse_tables(");
        let error = ordered(serve_root(), &renamed).expect_err("a root that makes no pre-flight is not verified");
        assert!(error.contains("is the pre-flight"), "{error}");
        assert!(error.contains("xtask/src/boot_order.rs"), "{error}");
        let no_root = ordered(serve_root(), "fn main() {}").expect_err("a file with none of the three calls checks nothing");
        assert!(no_root.contains("opens the adapters"), "{no_root}");
    }

    #[test]
    fn neither_prose_nor_a_test_module_can_stand_in_for_the_serving_path() {
        // Both directions of `code_lines`, and both matter: the order is stated in prose next to
        // every one of these calls, and every one of them is also made by a fixture underneath.
        let only_prose = ROOT.replace(
            "    boot::refuse_absent_tables(&pinned, &engines)?;\n",
            "    // boot::refuse_absent_tables(&pinned, &engines)? used to be here.\n",
        );
        let error = ordered(serve_root(), &only_prose).expect_err("a call named in a comment is not a call");
        assert!(error.contains("is the pre-flight"), "{error}");
        let numbered = code_lines(ROOT);
        assert_eq!(
            numbered
                .iter()
                .filter(|(_, line)| line.contains("boot::refuse_absent_tables("))
                .map(|(number, _)| *number)
                .collect::<Vec<usize>>(),
            vec![8_usize],
            "one call site: line 7 is prose and line 13 is the fixture's own, under `mod tests`"
        );
    }

    #[test]
    fn a_definition_above_the_call_is_not_read_as_the_call() {
        // `fn open_engine(` really does live above its call site in one of the two roots, so reading
        // a signature as a call would report an order that is not the one the process runs in.
        let opens = super::call_line(&code_lines(ROOT), "open_engine(").expect("the call is found");
        assert_eq!(opens, 6, "the CALL on line 6, not the signature on line 2");
    }
}
