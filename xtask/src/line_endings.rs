//! Every tracked text file must use LF, not CRLF.
//!
//! Why this is a gate and not a note: `devenv.nix` and `flake.nix` were once written with
//! CRLF on a Windows checkout. Nix preserves a carriage return inside an `''…''` string,
//! so every line of a shell script in there gained a trailing `\r` - which reached cargo
//! as part of the argument and produced errors whose "did you mean" suggestion was
//! byte-indistinguishable from what was typed (`unknown lint: 'warnings\r'`, tip:
//! `warnings`). It cost a red CI run and a confusing half hour.
//!
//! `.gitattributes` normalises what is COMMITTED. It does not normalise a working-tree
//! file that a tool just wrote, and Nix reads the working tree - hence this check.

use crate::Verdict;
use crate::repo;

/// Every tracked file is a candidate, and BINARY is decided from the bytes rather than from a
/// second `open`.
///
/// A [`repo::Scope`], so it is a bare `fn` with nothing captured: it cannot count subjects and it
/// cannot see content, which is what stops a scope decision standing in for a failed read.
const fn every_text_candidate(_rel: &str) -> bool {
    true
}

pub(crate) fn run(_args: &[String]) -> Verdict {
    let census = match repo::all_files() {
        Ok(census) => census,
        Err(why) => {
            eprintln!("xtask line-endings: FAILED - {}", why.describe());
            return Verdict::Fail;
        }
    };
    let mut offenders = Vec::new();
    // This gate prints NO count, which is why it is the second one migrated: it demonstrates the
    // `Unreachable` arm standing on its own, with no number to lean on. The refusal is the
    // mechanism and the count is a report - so a gate with no count is not a gate with no floor.
    //
    // **The read is the census's now, and this gate is where that matters most.** It used to
    // decide scope from `is_text_file`, which answers `false` for a file it cannot open - a claim
    // about content nothing had read - and then classify the outcome itself. It cannot classify
    // anything now: a subject in scope is one `Census::inspect` opened, so an unreadable file is a
    // refusal it has no arm to downgrade, and textness is decided from the bytes already in hand
    // rather than by re-opening the file.
    let scope: repo::Scope = every_text_candidate;
    let anchored = census.inspect(&["flake.nix"], scope, |rel, bytes| {
        if !repo::looks_like_text(bytes) {
            return;
        }
        let crs = bytes.windows(2).filter(|w| w == b"\r\n").count();
        if crs > 0 {
            offenders.push((String::from(rel), crs));
        }
    });
    if let Err(why) = anchored {
        eprintln!("xtask line-endings: FAILED - {}", why.describe());
        return Verdict::Fail;
    }

    if offenders.is_empty() {
        println!("xtask line-endings: ok - no CRLF in repo text files");
        return Verdict::Pass;
    }

    eprintln!("xtask line-endings: FAILED - CRLF found in repo text files");
    for (p, n) in &offenders {
        eprintln!("  {p}: {n} CRLF line ending(s)");
    }
    eprintln!("\nFix:  tr -d '\r' < FILE > FILE.lf && mv FILE.lf FILE");
    eprintln!("A carriage return inside a Nix '' '' string becomes part of a shell argument.");
    Verdict::Fail
}
