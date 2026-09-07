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
//!
//! **A clean bill here is a claim about files this gate READ**, which is why the accounting below
//! exists and why the verdict now carries a count. It said `ok - no CRLF in repo text files` over
//! the whole tree, and it shares `text-hygiene`'s defect and its fix - see `crate::text::Tally`'s
//! argument and `github.com/telekom/sutura#412`: a file that could not be opened answered `false`
//! to `is_text_file` exactly as a PNG does, so the same `continue` took both and the verdict
//! claimed a clean tree over bytes nothing had looked at.

use crate::Verdict;
use crate::repo;

/// What is wrong with one file.
#[derive(Debug, PartialEq, Eq)]
enum Trouble {
    /// How many CRLF line endings it has.
    Crlf(usize),
    /// In scope for this gate and unreadable, with the OS error that said so.
    Unreadable(String),
}

/// What the scan did with every path the listing offered. The denominator, for the reason
/// `crate::text::Tally` gives at length; the buckets differ because this gate has no
/// symlink-pointer skip of its own - a pointer file is text, and text with no CRLF in it passes.
#[derive(Debug, Default, PartialEq, Eq)]
struct Tally {
    /// Read as text and searched for CRLF.
    read: usize,
    /// Read, and not text. Out of scope rather than a failure.
    binary: usize,
    /// Listed and not on disk. Git's business, not this gate's.
    absent: usize,
    /// In scope and unreadable. Each is also a [`Trouble`].
    unreadable: usize,
}

impl Tally {
    /// Every listed path this scan reached a conclusion about.
    const fn accounted(&self) -> usize {
        self.read + self.binary + self.absent + self.unreadable
    }
}

/// One file and what is wrong with it. Named for `clippy::type_complexity`, whose threshold here
/// is 100 rather than the default 250 - `text-hygiene`'s `Offender` exists for the same reason.
type Offender = (String, Trouble);

/// Reads one listing, and says what it did with every entry.
fn scan(root: &std::path::Path, files: &[String]) -> (Vec<Offender>, Tally) {
    let mut offenders = Vec::new();
    let mut tally = Tally::default();

    for path in files {
        let full = root.join(path);
        match repo::probe_text(&full) {
            repo::TextProbe::Binary => tally.binary += 1,
            repo::TextProbe::Unreadable(cause) if cause.kind() == std::io::ErrorKind::NotFound => {
                tally.absent += 1;
            }
            repo::TextProbe::Unreadable(cause) => {
                tally.unreadable += 1;
                offenders.push((path.clone(), Trouble::Unreadable(cause.to_string())));
            }
            repo::TextProbe::Text => match std::fs::read(&full) {
                Err(cause) => {
                    tally.unreadable += 1;
                    offenders.push((path.clone(), Trouble::Unreadable(cause.to_string())));
                }
                Ok(bytes) => {
                    tally.read += 1;
                    let crs = bytes.windows(2).filter(|w| w == b"\r\n").count();
                    if crs > 0 {
                        offenders.push((path.clone(), Trouble::Crlf(crs)));
                    }
                }
            },
        }
    }

    (offenders, tally)
}

/// Turns what the scan found into the exit code, and prints both numbers behind it.
fn report(offered: usize, tally: &Tally, offenders: &[Offender]) -> Verdict {
    let accounted = tally.accounted();
    let mut failed = false;

    if !offenders.is_empty() {
        failed = true;
        eprintln!("xtask line-endings: FAILED");
        for (path, trouble) in offenders {
            match *trouble {
                Trouble::Crlf(n) => eprintln!("  {path}: {n} CRLF line ending(s)"),
                Trouble::Unreadable(ref cause) => {
                    eprintln!("  {path}: this gate is meant to read this file and could not: {cause}");
                }
            }
        }
        // Printed only where it applies. `tr -d` is not a remedy for a file nothing could open,
        // and a remedy the reader cannot act on is `github.com/telekom/sutura#386`'s defect landing
        // inside the change that added the finding.
        if offenders.iter().any(|(_, t)| matches!(*t, Trouble::Crlf(_))) {
            eprintln!("\nFix:  tr -d '\r' < FILE > FILE.lf && mv FILE.lf FILE");
            eprintln!("A carriage return inside a Nix '' '' string becomes part of a shell argument.");
        }
        if offenders.iter().any(|(_, t)| matches!(*t, Trouble::Unreadable(_))) {
            eprintln!("\nA file this gate cannot read needs its permissions or its bytes fixed.");
        }
    }

    // The floor, for the reason `crate::text::Tally` gives at length: every equality above is
    // satisfied by an empty listing, and `ok - no CRLF in 0 text file(s)` is a clean bill over
    // nothing read.
    if offered == 0 {
        failed = true;
        eprintln!(
            "xtask line-endings: FAILED - the listing was empty, so this gate read nothing. \
             Something upstream of here could not enumerate the tree."
        );
    }

    if accounted != offered {
        failed = true;
        eprintln!(
            "xtask line-endings: FAILED - {accounted} of {offered} listed path(s) were accounted \
             for. Every listed path gets one of read, binary, absent or unreadable here, so a \
             shortfall is this gate having stopped reading rather than a smaller tree."
        );
    }

    if failed {
        return Verdict::Fail;
    }

    println!(
        "xtask line-endings: ok - no CRLF in {} text file(s); {accounted} of {offered} listed \
         path(s) accounted for ({} binary, {} absent)",
        tally.read, tally.binary, tally.absent
    );
    Verdict::Pass
}

/// `xtask line-endings` - the registered entry point.
///
/// Everything but the listing is in [`scan`] and [`report`], so a test can drive the verdict over
/// a tree it built. What is left uncovered here is the four lines that find the repo.
pub(crate) fn run(_args: &[String]) -> Verdict {
    let Some(repo::RepoFiles { root, files }) = repo::all_files() else {
        eprintln!("xtask line-endings: could not determine the repo root");
        return Verdict::Fail;
    };

    let (offenders, tally) = scan(&root, &files);
    report(files.len(), &tally, &offenders)
}

#[cfg(test)]
mod tests {
    use super::{Tally, Trouble, report, scan};
    use crate::Verdict;

    /// A scratch tree of this test's own, emptied first so a rerun starts clean. Keyed on the
    /// process id for the reason `crate::falsifier` gives: a pid is reusable.
    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("sutura-crlf-{name}-{}", std::process::id()));
        drop(std::fs::remove_dir_all(&dir));
        std::fs::create_dir_all(&dir).expect("a scratch tree");
        dir
    }

    #[test]
    fn an_unreadable_file_is_reported_rather_than_skipped_into_a_clean_claim() {
        use std::io::Write as _;

        // `ok - no CRLF in repo text files` was said over files this gate never opened: an
        // unreadable one answered `false` to `is_text_file` exactly as a PNG does, and the same
        // `continue` took both. `github.com/telekom/sutura#412`, one gate over.
        let dir = scratch("unreadable");
        std::fs::write(dir.join("lf.md"), "a line\n").expect("an LF file");
        std::fs::write(dir.join("crlf.md"), "a line\r\n").expect("a CRLF file");
        let mut binary = std::fs::File::create(dir.join("favicon.png")).expect("create");
        binary.write_all(b"\x89PNG\r\n\x1a\n\x00\x00").expect("write");
        // A directory rather than a mode-000 file: `EISDIR` is not something a root process is
        // exempt from, where mode bits deny it nothing.
        std::fs::create_dir_all(dir.join("opaque")).expect("an unreadable path");

        let files: Vec<String> = ["lf.md", "crlf.md", "favicon.png", "gone.md", "opaque"]
            .iter()
            .map(|n| String::from(*n))
            .collect();
        let (offenders, tally) = scan(&dir, &files);

        assert!(
            offenders
                .iter()
                .any(|(p, t)| p == "opaque" && matches!(*t, Trouble::Unreadable(_))),
            "an in-scope file that cannot be read is reported: {offenders:?}"
        );
        // And the rule this gate is actually for still fires, so the change did not trade one for
        // the other. The PNG's own `\r\n` is why it must stay out of scope.
        assert!(
            offenders.iter().any(|(p, t)| p == "crlf.md" && *t == Trouble::Crlf(1)),
            "{offenders:?}"
        );
        assert!(!offenders.iter().any(|(p, _)| p == "favicon.png"), "{offenders:?}");
        assert_eq!(
            tally,
            Tally {
                read: 2,
                binary: 1,
                absent: 1,
                unreadable: 1,
            }
        );
        assert_eq!(tally.accounted(), files.len());
        assert_eq!(report(files.len(), &tally, &offenders), Verdict::Fail);

        drop(std::fs::remove_dir_all(&dir));
    }

    #[test]
    fn an_empty_listing_is_refused_rather_than_called_clean() {
        // The same floor `text-hygiene` carries, and for the same recorded reason: every equality
        // in `report` is satisfied by nothing at all, so a gate that enumerated no files would
        // print a clean bill over an empty tree.
        assert_eq!(report(0, &Tally::default(), &[]), Verdict::Fail);
        let one = Tally {
            read: 1,
            ..Tally::default()
        };
        assert_eq!(report(1, &one, &[]), Verdict::Pass);
    }

    #[test]
    fn a_shortfall_between_the_listing_and_the_scan_is_refused() {
        // The offered count is the LENGTH of the listing; the accounted count is summed in the
        // loop. Two numbers from two places, so a loop that stops early cannot shrink both.
        let complete = Tally {
            read: 2,
            ..Tally::default()
        };
        assert_eq!(report(2, &complete, &[]), Verdict::Pass);
        assert_eq!(
            report(3, &complete, &[]),
            Verdict::Fail,
            "a path the scan reached no conclusion about is a gate that stopped reading"
        );
    }
}
