//! Every tracked text file must use LF, not CRLF.
//!
//! Why this is a gate and not a note: `devenv.nix` and `flake.nix` were once written with
//! CRLF on a Windows checkout. Nix preserves a carriage return inside an `''…''` string,
//! so every line of a shell script in there gained a trailing `\r` — which reached cargo
//! as part of the argument and produced errors whose "did you mean" suggestion was
//! byte-indistinguishable from what was typed (`unknown lint: 'warnings\r'`, tip:
//! `warnings`). It cost a red CI run and a confusing half hour.
//!
//! `.gitattributes` normalises what is COMMITTED. It does not normalise a working-tree
//! file that a tool just wrote, and Nix reads the working tree — hence this check.

use std::path::Path;
use std::process::ExitCode;

/// Extensions that are text and therefore must be LF. Anything not listed is ignored, so
/// a new binary format does not need an exemption.
const TEXT_EXT: &[&str] = &[
    "rs",
    "toml",
    "nix",
    "yaml",
    "yml",
    "md",
    "sh",
    "json",
    "lock",
    "gitignore",
    "gitattributes",
];

pub(crate) fn run() -> ExitCode {
    let out = match std::process::Command::new("git").args(["ls-files", "-z"]).output() {
        Ok(o) if o.status.success() => o.stdout,
        _ => {
            eprintln!("xtask line-endings: could not list tracked files via git");
            return ExitCode::FAILURE;
        }
    };

    let mut offenders = Vec::new();
    for raw in out.split(|b| *b == 0) {
        if raw.is_empty() {
            continue;
        }
        let path = String::from_utf8_lossy(raw).to_string();
        let p = Path::new(&path);
        let is_text = p.extension().and_then(|e| e.to_str()).map_or_else(
            || p.file_name().and_then(|f| f.to_str()).is_some_and(|f| f.starts_with('.')),
            |e| TEXT_EXT.contains(&e),
        );
        if !is_text {
            continue;
        }
        if let Ok(bytes) = std::fs::read(&path) {
            let crs = bytes.windows(2).filter(|w| w == b"\r\n").count();
            if crs > 0 {
                offenders.push((path, crs));
            }
        }
    }

    if offenders.is_empty() {
        println!("xtask line-endings: ok — no CRLF in tracked text files");
        return ExitCode::SUCCESS;
    }

    eprintln!("xtask line-endings: FAILED — CRLF found in tracked text files");
    for (p, n) in &offenders {
        eprintln!("  {p}: {n} CRLF line ending(s)");
    }
    eprintln!("\nFix:  tr -d '\r' < FILE > FILE.lf && mv FILE.lf FILE");
    eprintln!("A carriage return inside a Nix '' '' string becomes part of a shell argument.");
    ExitCode::FAILURE
}
