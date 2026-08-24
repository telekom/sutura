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
    let Some(repo::RepoFiles { root, files }) = repo::all_files() else {
        eprintln!("xtask line-endings: could not determine the repo root");
        return Verdict::Fail;
    };

    let mut offenders = Vec::new();
    for path in files {
        if !repo::is_text_file(&root.join(&path)) {
            continue;
        }
        if let Ok(bytes) = std::fs::read(root.join(&path)) {
            let crs = bytes.windows(2).filter(|w| w == b"\r\n").count();
            if crs > 0 {
                offenders.push((path, crs));
            }
        }
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
