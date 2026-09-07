//! Which file holds a mechanism, derived from the tree rather than written down beside the claim.
//!
//! **Why this exists: `github.com/telekom/sutura#297`.** Four comments named `ci.yml` as holding
//! something it never held - a caller of the image smoke test, the command that builds the
//! `shellcheck` list, the reason a floor of 100 is a floor. None of them was falsified by a recent
//! change; they were false when they were written, and `check-guidance` could not see any of them.
//! `claims/remedies.rs` says why in as many words: **a bare filename with no slash is deliberately
//! not path-checked**, because which of eight workflows a sentence meant would be a guess. And a
//! path check would not have helped anyway - **`ci.yml` exists.** A citation that resolves says
//! nothing about the cited file's CONTENTS, which is the whole of what these sentences claimed.
//!
//! **The pattern this extends is `xtask/src/shipped/refusal.rs`.** `github.com/telekom/sutura#289`
//! removed the class from two printed remedies by DERIVING the file: the gate already reads every
//! file under `.github`, so it locates the one holding the refusal and interpolates the path,
//! leaving no claim for a move to falsify. That works where a gate computes the answer. Here the
//! answer is computable too - *which file holds this literal* - so the same move applies to prose,
//! with the comparison the printed case does not need: the tree is asked, the pages are read, and
//! the two have to agree.
//!
//! **The general rule was measured and refused, so it does not need weighing again.** #289 costed
//! *a backticked file citation plus a backticked identifier in one sentence must agree* against its
//! own six false pointers and it caught two. The reachable one was in the miss column for a
//! structural reason: after a step's body moves into a reusable workflow the caller keeps the job
//! NAME, so every token the sentence cites still resolves in the file it names. Loosening the token
//! shape turns the rule into prose parsing, and a citation rule widened past a precision story
//! becomes noise - which is how a gate gets switched off. So this is a REGISTERED attribution,
//! shaped like `claims/counts.rs`: an entry carries an argument, and both sides are read.
//!
//! **What that leaves uncovered, stated because it is most of the class.** A false pointer nobody
//! has registered is invisible here, exactly as a false claim nobody has registered is invisible to
//! `CONTRADICTED`. What the entries below buy is that the four attributions somebody has already
//! got wrong cannot go wrong again silently, and that a pointer written as a full repo path is also
//! path-checked by `dead_paths`. **The remedy for the general case is prose discipline: name the
//! ANCHOR a reader can grep, not the file you believe holds it.**
//!
//! **One more limit, and it decided how a comment in this diff is wrapped.** The attribution is
//! read from the flattened view, so it survives prose WRAPPING - but `claims.rs`'s `flatten` does
//! not strip a comment marker, which it says of itself. So in a `#` comment block a span on one
//! line and the marker on the next flatten with the `#` between them and the attribution is not
//! read at all - silently, since another page stating it keeps the entry non-empty. `nix/shipped.nix`
//! was reflowed to keep the two on one line for exactly that reason, and the mutation naming the
//! wrong file was re-run afterwards to prove the site is still read.

use std::path::Path;

use super::claims::flatten;
use crate::repo::matches_any;

/// A mechanism whose HOME is derived, and the prose that attributes it.
///
/// The third shape of *derive it, then compare it against what is written down* - after
/// `Pin`, which reads a value off a line, and `Counted`, which counts. This one RESOLVES A FILE,
/// which is the shape a workflow refactor breaks and nothing else here can express.
pub(in crate::guidance) struct Hosted {
    /// Human name, for the message.
    name: &'static str,
    /// Files the needles are looked for in. Globs against repo-relative paths.
    over: &'static [&'static str],
    /// Every needle. **ALL of them in ONE file**, for `refusal.rs`'s reason: one literal is
    /// usually a step that reads a thing, and reading it is not the claim.
    holds: &'static [&'static str],
    /// Where the attribution may be stated. **At least one file here has to state it**, or the
    /// entry derives a path and compares it to nothing.
    mentioned_in: &'static [&'static str],
    /// The phrase the attribution is made with. The FILE is the backtick span IMMEDIATELY
    /// BEFORE it.
    ///
    /// Deliberately that narrow, for `Counted::marker`'s reason: a sentence attributing one
    /// mechanism often cites two or three other files, and a check that read every backtick span
    /// on the line would report the correct ones.
    marker: &'static str,
}

/// The attributions this holds, and every one of them was wrong in the tree once.
///
/// **A fifth was folded in while fixing the four.** `.github/actions/embedded-dependency-list`
/// pointed at `release.yml` for the floor's reason, and `release.yml` points onward to the `build`
/// job rather than giving it - so the pointer resolved to a pointer. Same entry now holds both.
pub(in crate::guidance) const HOSTED: &[Hosted] = &[
    Hosted {
        name: "caller of the published image's smoke test",
        // Every workflow and composite action. A `.sh` under `nix/` cannot be a workflow caller.
        over: &[".github/**"],
        // The INVOCATION rather than the path: a mention in prose is followed by a backtick, and
        // this is followed by the argument the script requires. Without that, the script's own
        // header - which names the caller and is therefore in `mentioned_in` - would match its own
        // needle and the entry would resolve to the file making the claim.
        holds: &["serve-smoke.sh \""],
        mentioned_in: &[".github/**", "docs/**", "nix/**", ".agents/skills/**"],
        marker: "invokes the image smoke test",
    },
    Hosted {
        name: "builder of the shellcheck list",
        over: &[".github/**", "nix/**"],
        // `git ls-files` and not `find`, which is the correction that made two of #297's comments
        // false: the `find` form failed on six `.sh` files inside an installed, gitignored pixi
        // environment, so it was replaced - and the sentences kept naming it, in a workflow that
        // never held it either.
        holds: &["git ls-files '*.sh'"],
        mentioned_in: &[".github/**", "docs/**", "nix/**", ".agents/skills/**"],
        marker: "builds from tracked files",
    },
    Hosted {
        name: "reason the embedded-dependency floor is a floor",
        over: &[".github/**"],
        // The ARGUMENT, not the number. `-lt 100` is in four files and the reason is in one, which
        // is exactly why the cross-reference existed and exactly what made it worth deriving.
        holds: &["A FLOOR, not a fixed number"],
        mentioned_in: &[".github/**", "docs/**", "nix/**", ".agents/skills/**"],
        marker: "gives at its own copy of this number",
    },
];

/// What one scan found, and the files it could not read.
///
/// A named struct rather than a tuple because `clippy::type_complexity` refuses the tuple, and it
/// reads better at both ends: the read failures are part of the ANSWER here, not an aside.
struct Scanned<T> {
    /// The rows the scan produced.
    found: Vec<T>,
    /// One entry per in-scope file that could not be read as text.
    unreadable: Vec<String>,
}

/// The backtick span at the end of `head`, if it ends in one.
///
/// Trailing whitespace is allowed between the span and the marker and nothing else is: a possessive
/// or an intervening word means the sentence is attributing something this cannot resolve, and
/// skipping it under-claims rather than guessing which of two cited files was meant.
fn trailing_span(head: &str) -> Option<&str> {
    let trimmed = head.trim_end();
    let inner = trimmed.strip_suffix('`')?;
    let (_, span) = inner.rsplit_once('`')?;
    (!span.is_empty()).then_some(span)
}

/// Which file under `over` holds every needle, as a repo-relative path.
///
/// **ONE FILE OR NONE**, which is `xtask/src/shipped/refusal.rs`'s rule and its reason: two files
/// holding one mechanism are two copies to keep in step, and a sentence that hands a reader a list
/// orients nobody. `None` is a FAILED verdict at the call site either way - the two cases are told
/// apart there, because *it moved* and *there are two of them* want different corrections.
fn hosting(root: &Path, files: &[String], hosted: &Hosted) -> Scanned<String> {
    let mut found = Vec::new();
    let mut unreadable = Vec::new();
    for rel in files {
        if !matches_any(hosted.over, rel) {
            continue;
        }
        match std::fs::read_to_string(root.join(rel)) {
            Ok(text) if hosted.holds.iter().all(|needle| text.contains(needle)) => found.push(rel.clone()),
            Ok(_) => {}
            // FAIL CLOSED ON A READ, not only on a parse. The file this cannot look at may be the
            // one holding the mechanism, and dropping it turns *nobody holds this* into an answer.
            // Measured one gate over during review of `github.com/telekom/sutura#301`: a non-UTF-8
            // page left `check-shipped-binaries` at `ok` and exit 0 because its reader said
            // `continue`.
            Err(why) => unreadable.push(format!("{rel}: cannot be read as UTF-8 text - {why}")),
        }
    }
    Scanned { found, unreadable }
}

/// One place an attribution is written: where, and which file it names.
struct Attributed {
    /// Repo-relative path of the page making the claim.
    file: String,
    /// One-based line, so a message names the line a reader will open.
    line: usize,
    /// The file the claim names.
    named: String,
}

/// Every place this entry's attribution is actually written.
///
/// Read from the FLATTENED view for `Counted`'s reason, which was a defect there before it was a
/// rule: prose wraps, so an attribution whose file span ends one line above its marker is invisible
/// to a per-line search, and every occurrence is read rather than the first on a line.
fn attributions(root: &Path, files: &[String], hosted: &Hosted) -> Scanned<Attributed> {
    let mut found = Vec::new();
    let mut unreadable = Vec::new();
    for rel in files {
        if !matches_any(hosted.mentioned_in, rel) {
            continue;
        }
        // Same rule on this side, and the loss is the mirror image: an unread page may be the one
        // making the false attribution, and another page stating it correctly keeps the entry
        // non-empty - so the miss is silent in exactly the way this whole module exists to stop.
        let text = match std::fs::read_to_string(root.join(rel)) {
            Ok(text) => text,
            Err(why) => {
                unreadable.push(format!("{rel}: cannot be read as UTF-8 text - {why}"));
                continue;
            }
        };
        let (flat, lines) = flatten(&text);
        let mut from = 0_usize;
        while let Some(at) = flat.get(from..).and_then(|rest| rest.find(hosted.marker)) {
            let offset = from.saturating_add(at);
            if let Some(head) = flat.get(..offset)
                && let Some(span) = trailing_span(head)
            {
                found.push(Attributed {
                    file: rel.clone(),
                    line: lines.get(offset).copied().unwrap_or(1),
                    named: String::from(span),
                });
            }
            from = offset.saturating_add(hosted.marker.len().max(1));
        }
    }
    Scanned { found, unreadable }
}

/// A file named as holding a mechanism must be the file that holds it.
pub(in crate::guidance) fn host_mismatches(root: &Path, all: &[String], files: &[String]) -> Vec<String> {
    let mut problems = Vec::new();
    for hosted in HOSTED {
        let Scanned {
            found: holders,
            unreadable,
        } = hosting(root, all, hosted);
        problems.extend(unreadable);
        let derived = match holders.split_first() {
            // The thing attributed is gone. Not the prose being right - a rename, a deletion or a
            // reworded literal all land here, and every one of them leaves the sentences below
            // pointing somewhere by luck.
            None => {
                problems.push(format!(
                    "no file under {:?} holds all of {:?} - the {} moved, so this entry in HOSTED \
                     derives nothing and the prose naming it is unchecked",
                    hosted.over, hosted.holds, hosted.name
                ));
                continue;
            }
            Some((one, [])) => one.clone(),
            // Two homes for one mechanism. `xtask/src/shipped/refusal.rs`'s rule, and the same
            // argument: the correction is to have one, not to make the sentence name a list.
            Some((_, _)) => {
                problems.push(format!(
                    "{} file(s) hold all of {:?} ({}) - the {} has no single home, so no sentence \
                     can name it. One file, or the prose names the anchor instead",
                    holders.len(),
                    hosted.holds,
                    holders.join(", "),
                    hosted.name
                ));
                continue;
            }
        };
        let Scanned {
            found: stated,
            unreadable,
        } = attributions(root, files, hosted);
        problems.extend(unreadable);
        for claim in &stated {
            if claim.named != derived {
                problems.push(format!(
                    "{}:{}: names `{}` as the {} - it is `{derived}`",
                    claim.file, claim.line, claim.named, hosted.name
                ));
            }
        }
        if stated.is_empty() {
            // The way this check goes green over silence, and the way its two siblings did. A
            // derived path nobody writes down is compared to nothing, and a deleted sentence looks
            // exactly like a correct one.
            problems.push(format!(
                "nothing under {:?} attributes the {} - the path is derived and compared to \
                 nothing, so this entry in HOSTED is a gate over silence. State it as a backtick \
                 span immediately before `{}`, or delete the entry",
                hosted.mentioned_in, hosted.name, hosted.marker
            ));
        }
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::Hosted;

    const ENTRY: Hosted = Hosted {
        name: "builder of the shellcheck list",
        over: &["nix/**"],
        holds: &["git ls-files '*.sh'"],
        mentioned_in: &["docs/**"],
        marker: "builds from tracked files",
    };

    /// The span reader, and the shapes it declines rather than guesses at.
    #[test]
    fn the_file_is_the_span_immediately_before_the_marker() {
        assert_eq!(
            super::trailing_span("A `.sh` also lands in the list `nix/lint-workflows.sh` "),
            Some("nix/lint-workflows.sh")
        );
        // A possessive puts a word between the span and the marker, so the sentence is attributing
        // something this cannot resolve. Skipped, not guessed.
        assert_eq!(super::trailing_span("`nix/lint-workflows.sh`'s pass "), None);
        // No span at all, and an unclosed one.
        assert_eq!(super::trailing_span("the shellcheck list "), None);
        assert_eq!(super::trailing_span("`nix/lint-workflows.sh "), None);
    }

    /// A WRAPPED attribution is found, which is what the flattened view buys.
    ///
    /// `claims/counts.rs` records this as a defect its per-line predecessor had: prose wraps, and a
    /// sentence whose file span ends one line above its marker is invisible to a line search.
    #[test]
    fn an_attribution_wrapped_across_two_lines_is_still_read() {
        let dir = std::env::temp_dir().join(format!("sutura-hosts-{}", std::process::id()));
        let docs = dir.join("docs");
        let nix = dir.join("nix");
        let write = |at: &std::path::Path, name: &str, body: &str| {
            std::fs::create_dir_all(at).unwrap_or_else(|e| panic!("{e}"));
            std::fs::write(at.join(name), body).unwrap_or_else(|e| panic!("{e}"));
        };
        write(
            &nix,
            "lint-workflows.sh",
            "mapfile -t scripts < <(git ls-files '*.sh' | sort)\n",
        );
        // Text to begin with, so the read guard below is the only reason it later fails.
        write(&docs, "not-text.md", "nothing to attribute here\n");
        write(
            &docs,
            "page.md",
            "The list is what `nix/lint-workflows.sh`\nbuilds from tracked files, and a `run:` block is not one.\n",
        );
        let all = vec![
            String::from("nix/lint-workflows.sh"),
            String::from("docs/page.md"),
            String::from("docs/not-text.md"),
        ];
        let read = super::attributions(&dir, &all, &ENTRY);
        assert!(read.unreadable.is_empty(), "{:?}", read.unreadable);
        assert_eq!(read.found.len(), 1, "the wrapped attribution was not read");
        assert_eq!(
            read.found.first().map(|one| one.named.as_str()),
            Some("nix/lint-workflows.sh")
        );
        let holders = super::hosting(&dir, &all, &ENTRY);
        assert_eq!(holders.found, vec![String::from("nix/lint-workflows.sh")]);
        assert!(holders.unreadable.is_empty(), "{:?}", holders.unreadable);

        // And the direction the whole entry exists for: a sentence naming the wrong file.
        write(&docs, "page.md", "The list is what `ci.yml` builds from tracked files.\n");
        let wrong = super::attributions(&dir, &all, &ENTRY);
        assert_eq!(wrong.found.first().map(|one| one.named.as_str()), Some("ci.yml"));

        // FAIL CLOSED ON A READ, both sides. A page nobody can read may be the one making the
        // false attribution, and a file nobody can read may be the one holding the mechanism -
        // and another correct page keeps the entry non-empty, so the loss is silent.
        std::fs::write(docs.join("not-text.md"), [0xff_u8, 0xfe, 0x00]).unwrap_or_else(|e| panic!("{e}"));
        let now = super::attributions(&dir, &all, &ENTRY).unreadable;
        assert_eq!(now.len(), 1, "an unreadable page was dropped in silence: {now:?}");
        assert!(now.iter().any(|p| p.contains("docs/not-text.md")), "{now:?}");
        std::fs::remove_dir_all(&dir).unwrap_or_else(|e| panic!("{e}"));
    }

    /// THE REAL TREE, because three of this entry table's four fail-closed directions are
    /// properties of it.
    ///
    /// A fixture cannot establish that each entry resolves to exactly one file and that at least
    /// one page attributes it. Those are what stop the check passing over a moved mechanism or over
    /// silence, so they are asserted here rather than described.
    ///
    /// **The attribution side is given the caller's TEXT set, not the whole tree, and this test
    /// mirrors that deliberately.** The first version passed `all_files()` to both sides and
    /// reported `docs/assets/favicon.png` - a PNG cannot carry an attribution, so a binary inside
    /// the `docs/**` glob is out of SCOPE rather than unreadable. The distinction is what keeps the
    /// read guard from being the thing somebody switches off: it fails on a file this gate is meant
    /// to read and cannot, never on a file it was never going to read.
    #[test]
    fn every_entry_resolves_to_one_file_and_some_page_names_it() {
        let (root, files) = crate::repo::all_files()
            .and_then(|census| census.into_listing(crate::repo::Unmigrated::Guidance))
            .expect("could not locate the repo");
        let text: Vec<String> = files
            .iter()
            .filter(|f| super::super::has_ext(f, &["md", "nix", "yml", "yaml", "toml", "sh"]))
            .cloned()
            .collect();
        for hosted in super::HOSTED {
            let holders = super::hosting(&root, &files, hosted);
            assert!(holders.unreadable.is_empty(), "{:?}", holders.unreadable);
            assert_eq!(holders.found.len(), 1, "{}: {:?}", hosted.name, holders.found);
            let stated = super::attributions(&root, &text, hosted);
            assert!(stated.unreadable.is_empty(), "{:?}", stated.unreadable);
            assert!(!stated.found.is_empty(), "{}: nothing attributes it", hosted.name);
            for claim in &stated.found {
                assert_eq!(
                    Some(claim.named.as_str()),
                    holders.found.first().map(String::as_str),
                    "{}:{} names the wrong file",
                    claim.file,
                    claim.line
                );
            }
        }
        // And the whole check over the real tree, through the entry point the gate calls, so a
        // problem produced by any of the four arms fails this too.
        assert!(super::host_mismatches(&root, &files, &text).is_empty());
    }
}
