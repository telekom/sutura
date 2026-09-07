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

pub(crate) fn run(_args: &[String]) -> Verdict {
    let census = match repo::all_files() {
        Ok(census) => census,
        Err(why) => {
            eprintln!("xtask line-endings: FAILED - {}", why.describe());
            return Verdict::Fail;
        }
    };
    let root = census.root().to_path_buf();

    let mut offenders = Vec::new();
    // This gate prints NO count, which is why it is the second one migrated: it demonstrates the
    // `Unreachable` arm standing on its own, with no number to lean on. The refusal is the
    // mechanism and the count is a report - so a gate with no count is not a gate with no floor.
    //
    // READ FIRST, then decide scope. It used to be the other way round, and `is_text_file` answers
    // `false` for a file it cannot open - so an unreadable file was dropped as *not text*, which is
    // a claim about content nothing had read. An unreadable file's textness is unknowable, so it
    // is a refusal; a file that reads and is binary is out of SCOPE, which is the distinction that
    // keeps a read guard from being the thing somebody switches off.
    let anchored = census.inspect(&["flake.nix"], |rel| {
        let path = root.join(rel);
        match std::fs::read(&path) {
            Err(why) => repo::Looked::Unreachable(format!("{rel}: {why}")),
            Ok(bytes) => {
                if !repo::is_text_file(&path) {
                    return repo::Looked::OutOfScope;
                }
                let crs = bytes.windows(2).filter(|w| w == b"\r\n").count();
                if crs > 0 {
                    offenders.push((String::from(rel), crs));
                }
                repo::Looked::Judged
            }
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
