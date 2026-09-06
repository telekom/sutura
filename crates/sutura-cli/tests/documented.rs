//! Every `sutura` command the documentation prints, run - and every line it prints as fact.
//!
//! **Why this exists.** `tests/example.rs` proves the example's CATALOG still answers, by driving
//! the libraries. Nothing proved the pages: the argument vector a reader copies, and the output
//! block underneath it, were prose. They drifted, and the drift is the reason this file is here -
//! `docs/getting-started.md` and `examples/single-player/README.md` both printed
//!
//! ```text
//! refused: DimensionValueNotAllowed { metric: MetricName("recurring_revenue"), dimension: DimensionName("region") }
//! ```
//!
//! which is the `Debug` dump `render_refusal` REPLACED. The binary has printed a variant, a
//! meaning, its fields and a remedy since; the first page a reader is sent to showed output no
//! build had produced for as long as that renderer has existed.
//!
//! **What is asserted.** Three things, each of them something a reader would copy as fact:
//!
//! * every invocation of this binary either page prints is RUN from a clone's working directory and
//!   exits `0` - or its subcommand is in `SKIPPED`, which names the venue that covers it. A line
//!   that reads as an invocation and is neither is a FAILURE rather than a skip;
//! * every refusal block and every answer a page prints - in both cases the whole contiguous run
//!   from its `refused:` or `-- definitions ` line to the end of its fence - is held against the
//!   output of the command in the fence ABOVE it;
//! * every definitions digest written in prose is the digest the catalog reports.
//!
//! **Two of those three are shaped the way they are because the first version of this file was
//! wrong about them, and both mistakes were silent.** A candidate spelled one flag differently -
//! `cargo run -q -p sutura-cli --` - was dropped by a bare `continue`, so a first page printing a
//! command that cannot run left this suite green; the per-page floor could not see it, because the
//! page's other commands satisfied it. And a `refused: ` PREFIX holds one line of the five
//! `render_refusal` emits: the meaning, each field and the remedy are indented under it, so the
//! block this file exists for was the one part of it nothing held. Both are why an unrecognised
//! candidate is now loud and why a claim is a contiguous run rather than a line.
//!
//! **What is deliberately not asserted.** Not the whole of a captured block. The pages ELIDE on
//! purpose - `docs/getting-started.md` says so where it prints five metrics of eleven, and it wraps
//! a statement that the binary emits on one line - so asserting a block verbatim would fail correct
//! prose. What that leaves unheld is a printed line that is none of the three shapes: a listing
//! this binary pads with SPACES, a compiled statement, a rendered prompt. `tests/example.rs`'s
//! snapshots pin the numbers, which is the right place for them.
//!
//! **A `query`'s ROWS were in that unheld list and are not any more, at a measured cost.** A
//! `query` prints its header and every cell joined with `\t`; a change to `docs/getting-started.md`
//! rewrote seven of those rows to space padding while this suite stayed 66/66, and
//! `examples/single-player/README.md` had been carrying the same space padding for longer than
//! that. So the `-- definitions ` stamp is now the head of a block rather than a line, exactly as
//! `refused:` is - see [`claims`], including why a tab-carrying LINE was tried first and does not
//! hold.
//!
//! **The other limits, next to the claim.** `SKIPPED` is `sutura mcp`, which speaks a protocol on
//! its own pipes and would block on this harness's stdin; a `sutura-serve` invocation and a `curl`
//! are not invocations of this binary and are not read here at all -
//! `crates/sutura-serve/tests/served.rs` is their venue. An invocation that SETS an environment
//! variable is refused rather than run, because `run` strips every `SUTURA*` variable and adds
//! none, so this harness cannot be the deployment such a line describes. And the subcommand names
//! are DERIVED from the listing the binary prints with no arguments, so a rename is red here rather
//! than a fence that silently stops being read.

// `cfg(test)` for the reason `tests/example.rs` gives: clippy honours `allow-expect-in-tests` only
// inside a `#[cfg(test)]` item, and without it every `expect` below is a lint error.
#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::process::Command;

    /// The pages whose commands are run and whose output lines are held.
    ///
    /// `docs/getting-started.md` is the path the site sends a reader down;
    /// `examples/single-player/README.md` is the corpus's own page. Both print an argument vector
    /// followed by what it produced.
    const PAGES: &[&str] = &["docs/getting-started.md", "examples/single-player/README.md"];

    /// Pages that state a definitions digest without printing a command that produces one.
    ///
    /// `docs/serving.md` carries it inside a JSON response body, which is the same fact reached
    /// through the other surface.
    const DIGEST_PAGES: &[&str] = &["docs/serving.md"];

    /// The page `docs/index.md` sends a stranger down, whose guided half must run on an artefact.
    const GUIDED: &str = "docs/getting-started.md";

    /// The heading that opens the contributor's route, and therefore closes the guided path.
    ///
    /// The split point is a HEADING rather than a line number because a page grows: what is being
    /// held is that a toolchain appears below it and never above, and a page that lost the heading
    /// is a failure rather than a walk with nothing to compare against.
    const CONTRIBUTOR: &str = "## Building from source";

    /// The workflow that publishes the release, whose `TARGETS` is the set of triples it produces.
    const RELEASE: &str = ".github/workflows/release.yml";

    /// The routes to a tree that [`GUIDED`] may not print anywhere: it is a download, not a clone.
    ///
    /// **Two needles and matched with `contains`, both because review defeated one of each.**
    /// `cd /tmp && git clone …` on one line got past a `starts_with` on the trimmed line, and
    /// `gh repo clone telekom/sutura` is the same act spelled without the word `git` at all.
    const CLONE: &[&str] = &["git clone", "repo clone"];

    /// The heading over the fence that puts a verified binary on a reader's `PATH`.
    const INSTALL: &str = "## Install it";

    /// The shipped binary this page installs and then runs, as a release asset names it.
    ///
    /// Named because a release publishes `sutura-serve` at the same four triples, so the triple
    /// alone does not say which binary a reader downloaded.
    const CLI: &str = "sutura";

    /// Every step the install fence has to carry, and what its absence would cost a reader.
    ///
    /// **A list because deleting the whole fence was green.** Asserting that some fence parses
    /// under the heading proves nothing about what is in it - the mutation review ran removed the
    /// download, both checks, the extraction and the `PATH` install in one edit, and the suite
    /// stayed 66/66 over a page whose prose still explained what the two verifications establish.
    ///
    /// Each entry is matched as a substring of one logical line, so a flag added to a step does not
    /// break it while a step going missing does. What this cannot say is that a step WORKS - each
    /// one was executed against the live release by hand, and no test can fetch an artefact.
    const STEPS: &[(&str, &str)] = &[
        (
            "gh release download",
            "there is nothing for the rest of the fence to check or unpack",
        ),
        (
            "sha256sum -c",
            "a truncated or corrupted download is unpacked and run without a word",
        ),
        (
            "gh attestation verify",
            "a substituted file is unpacked and run, and the page's `download, verify, run` is two thirds true",
        ),
        ("tar -xzf", "the archive is never opened, so no binary exists to install"),
        (
            "install -m755",
            "nothing reaches a `PATH`, and every bare `sutura` command below fails",
        ),
    ];

    /// The repository root, which is the working directory every documented command assumes: the
    /// paths in them are repo-relative, exactly as a reader at a clone types them.
    fn repo_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    fn page(rel: &str) -> String {
        let path = repo_root().join(rel);
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{} is readable: {e}", path.display()))
    }

    /// One fenced block: the line its opening delimiter is on, and the lines inside it.
    struct Fence<'a> {
        opened_at: usize,
        lines: Vec<&'a str>,
    }

    /// A fence still being read: the delimiter that opened it, its length, and the block so far.
    struct Open<'a> {
        mark: char,
        len: usize,
        fence: Fence<'a>,
    }

    /// A fence delimiter: which character opened it, and how many of them there were.
    ///
    /// Both halves are needed. A tracker that toggles on any three-or-more run inverts the parse
    /// for the whole of a page below a nested block, and then every command and output line after
    /// the inversion is read as the wrong kind of thing. Neither page in `PAGES` nests a fence
    /// today, so this is what stops that being a property of the pages rather than of the parser -
    /// the same defect `xtask/src/docs/links.rs` carried over the whole docs tree.
    ///
    /// **NOT shared with the lexer that fixed it there, and the reason is structural.**
    /// `xtask/src/markdown.rs` is `pub(crate)` in a binary-only package with no library target, so
    /// nothing under `crates/` can reach it, and a dev-dependency pointing that way would invert
    /// the layering the crate map exists to hold. It also answers the opposite question: `prose`
    /// BLANKS every fenced block so a link scan cannot read code, where this file wants the fenced
    /// blocks and nothing else. Two small lexers, one rule - record the delimiter and its length,
    /// and refuse to answer over an unclosed block.
    fn delimiter(line: &str) -> Option<(char, usize)> {
        let trimmed = line.trim_start();
        for mark in ['`', '~'] {
            let run = trimmed.chars().take_while(|c| *c == mark).count();
            if run >= 3 {
                return Some((mark, run));
            }
        }
        None
    }

    /// Every fenced block in a page, as its lines.
    ///
    /// The info string is dropped: what matters is that a fence is a block, and a `bash` block
    /// holds the commands while a `text` one holds what they printed. A block still open at the end
    /// of the page is an ERROR rather than an answer, because the alternative is a scan that
    /// quietly read half a page.
    fn fences<'a>(rel: &str, text: &'a str) -> Vec<Fence<'a>> {
        let mut blocks = Vec::new();
        let mut open: Option<Open<'a>> = None;
        for (index, line) in text.lines().enumerate() {
            match open {
                Some(ref mut block) => {
                    let closes = delimiter(line).is_some_and(|(mark, run)| {
                        mark == block.mark && run >= block.len && line.trim().trim_start_matches(mark).is_empty()
                    });
                    if closes {
                        let block = open.take().expect("the fence being closed is the open one");
                        blocks.push(block.fence);
                    } else {
                        block.fence.lines.push(line);
                    }
                }
                None => {
                    if let Some((mark, len)) = delimiter(line) {
                        open = Some(Open {
                            mark,
                            len,
                            fence: Fence {
                                opened_at: index + 1,
                                lines: Vec::new(),
                            },
                        });
                    }
                }
            }
        }
        assert!(
            open.is_none(),
            "{rel} opens a fence it never closes, so every line below it was read as the wrong kind"
        );
        blocks
    }

    /// One logical shell line per entry, with a trailing `\` continuation joined up.
    fn logical_lines(block: &[&str]) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let mut pending = String::new();
        for line in block {
            let trimmed = line.trim();
            if let Some(head) = trimmed.strip_suffix('\\') {
                pending.push_str(head.trim_end());
                pending.push(' ');
                continue;
            }
            pending.push_str(trimmed);
            out.push(std::mem::take(&mut pending));
        }
        if !pending.trim().is_empty() {
            out.push(pending);
        }
        out
    }

    /// The subcommands this file runs when a page prints them.
    ///
    /// All five are required to exit `0`, the refusal among them: a refusal is a RESULT, and
    /// `docs/getting-started.md` says so in the sentence above the one it prints. So there is no
    /// per-subcommand status table to keep - there is one rule, and a command that started
    /// erroring would break it.
    ///
    /// `doctor` is here because the guided path prints it: it is the first thing a reader runs on a
    /// binary they just downloaded, and it takes no catalog and reads no settings. **What running
    /// it here does NOT hold is its `data systems` line**, and the reason is this suite's own
    /// build: `just documented` compiles the binary with `--all-features`, so the `bigquery` arm of
    /// that line is the one that prints here while a published artefact prints the other. `just
    /// shipped` is the venue for that half, out of the released binary's embedded dependency list.
    ///
    /// **This is not an allowlist that may quietly grow a gap.** A documented subcommand that is
    /// neither here nor in `SKIPPED` fails the walk, and every name in both is checked against the
    /// listing the binary itself prints - so adding a subcommand, renaming one, or documenting one
    /// is a decision somebody writes down rather than a fence that stops being read.
    const RUNNABLE: &[&str] = &["doctor", "catalog", "describe", "compile", "query"];

    /// The subcommands a page may print that this file does not run, each with the venue that does.
    ///
    /// One entry. `sutura mcp` speaks the Model Context Protocol on the process's own pipes and
    /// would block on this harness's stdin, so it is driven by a client that answers -
    /// `just mcp-e2e` runs that one.
    const SKIPPED: &[(&str, &str)] = &[("mcp", "crates/sutura-cli/tests/mcp.rs")];

    /// The separator in `cargo run -p sutura-cli -- <args>`, which is one of the two spellings a
    /// page prints: that one is what a reader at a clone runs, and `sutura <args>` is what a reader
    /// with the published binary runs.
    ///
    /// The separator rather than the whole prefix is what is matched, so `-q`, `--release` and any
    /// other cargo flag land on the same path instead of being dropped as an unrecognised line.
    const SEPARATOR: &str = " -p sutura-cli -- ";

    /// This package, as a `cargo run` names it. A run of it with no `--` is a candidate whose
    /// argument vector cannot be built, which is a failure rather than a skip.
    const PACKAGE: &str = "-p sutura-cli";

    /// Shell syntax this harness cannot honour: it spawns the binary directly, with no shell.
    const SHELL: &[char] = &['$', '"', '\'', '|', '<', '>', '&', ';', '`', '(', ')'];

    /// A `NAME=value` word, which is an assignment rather than the start of a command.
    fn is_assignment(word: &str) -> bool {
        word.split_once('=').is_some_and(|(name, _)| {
            !name.is_empty()
                && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                && name.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
        })
    }

    /// What one line of a fence is, as far as this binary is concerned.
    enum Candidate {
        /// Not an invocation of this binary at all - a `curl`, a `sutura-serve`, a `cargo build`,
        /// or output.
        NotOurs,
        /// An invocation, and the argument vector to pass.
        Argv(Vec<String>),
        /// A line that reads as an invocation of this binary and cannot be run as one, and why.
        Unrunnable(String),
    }

    /// The argument vector a page's line asks this binary for, or why it cannot be one.
    ///
    /// Only `NotOurs` is silent, and the other two are the finding this file was reviewed for: the
    /// first version recognised two literal prefixes and dropped everything else with a bare
    /// `continue`, which is how a page came to print a command that could not run while this suite
    /// stayed green.
    fn argv(line: &str, vars: &[(String, String)]) -> Candidate {
        let line = line.trim();
        let bare = line.strip_prefix("$ ").unwrap_or(line).trim_start();
        let mut rest = bare;
        let mut assigned: Vec<&str> = Vec::new();
        while let Some((word, tail)) = rest.split_once(' ') {
            if !is_assignment(word) {
                break;
            }
            assigned.push(word);
            rest = tail.trim_start();
        }
        let tail = if rest.starts_with("cargo run") {
            match rest.split_once(SEPARATOR) {
                Some((_, tail)) => tail,
                None if rest.contains(PACKAGE) => {
                    return Candidate::Unrunnable(String::from(
                        "it is a `cargo run` of this package with no `--` separator, so there is no argument vector to pass",
                    ));
                }
                None => return Candidate::NotOurs,
            }
        } else if let Some(tail) = rest.strip_prefix("sutura ") {
            tail
        } else {
            return Candidate::NotOurs;
        };
        if let Some(name) = assigned.first() {
            return Candidate::Unrunnable(format!(
                "it sets `{name}` in front of the command, and this harness runs the binary with every `SUTURA*` variable REMOVED and none added - so it cannot be the deployment that line describes"
            ));
        }
        let args: Vec<String> = tail
            .split_whitespace()
            .map(|word| {
                vars.iter()
                    .fold(String::from(word), |acc, (name, value)| acc.replace(name, value))
            })
            .collect();
        if let Some(word) = args.iter().find(|word| word.contains(SHELL)) {
            return Candidate::Unrunnable(format!(
                "`{word}` carries shell syntax, and this harness spawns the binary with no shell"
            ));
        }
        Candidate::Argv(args)
    }

    /// One invocation a page prints, once its subcommand has been classified.
    enum Invocation {
        /// A subcommand this file runs, with the arguments to pass.
        Run(Vec<String>),
        /// A subcommand covered somewhere else, and where.
        Elsewhere(&'static str),
    }

    /// Which of the two an argument vector is, or why it is neither.
    fn classify(args: Vec<String>, known: &[String]) -> Result<Invocation, String> {
        let Some(first) = args.first().cloned() else {
            return Err(String::from("no subcommand follows it"));
        };
        if RUNNABLE.contains(&first.as_str()) {
            return Ok(Invocation::Run(args));
        }
        for &(name, venue) in SKIPPED {
            if name == first.as_str() {
                return Ok(Invocation::Elsewhere(venue));
            }
        }
        if known.contains(&first) {
            return Err(format!(
                "`{first}` is a subcommand this binary offers that this harness neither runs nor names as skipped - put it in `RUNNABLE`, or in `SKIPPED` with the venue that covers it"
            ));
        }
        Err(format!("this binary lists no subcommand `{first}`: it offers {known:?}"))
    }

    /// Every invocation of this binary one fence prints.
    ///
    /// An assignment on its own line (`E=examples/single-player`) is expanded into the ones after
    /// it, because one block uses it to keep three commands on one line each.
    fn invocations(rel: &str, fence: &Fence<'_>, known: &[String]) -> Vec<Invocation> {
        let mut vars: Vec<(String, String)> = Vec::new();
        let mut found = Vec::new();
        for line in logical_lines(&fence.lines) {
            if let Some((name, value)) = line.split_once('=')
                && !name.is_empty()
                && name.chars().all(|c| c.is_ascii_uppercase() || c == '_')
                && !value.contains(' ')
            {
                vars.push((format!("${name}"), String::from(value)));
                continue;
            }
            let outcome = match argv(&line, &vars) {
                Candidate::NotOurs => continue,
                Candidate::Unrunnable(why) => Err(why),
                Candidate::Argv(args) => classify(args, known),
            };
            let at = fence.opened_at;
            found.push(outcome.unwrap_or_else(|why| panic!("{rel}:{at} prints `{line}` as a command, and {why}")));
        }
        found
    }

    /// One thing a page prints as output: a `-- definitions ` stamp, or a whole refusal block.
    struct Claim {
        at: usize,
        lines: Vec<String>,
    }

    /// What a fence claims the binary printed.
    ///
    /// A refusal is a BLOCK and holding its first line holds a fifth of it: `render_refusal` emits
    /// the variant, then the meaning, each field and the remedy, each indented two spaces under it.
    /// So the run reaches the end of the fence - which is what the binary emits as one block, and
    /// what both pages had wrong in a way a `refused: ` prefix could not see.
    ///
    /// **A `-- definitions ` STAMP IS NOW A BLOCK TOO, for the same reason and after two attempts.**
    /// `query` prints the stamp and then the answer, joined with `\t`, and holding the stamp alone
    /// left every row prose: the change that added this file's guided path rewrote seven real tabs
    /// to space padding and stayed 66/66, and `examples/single-player/README.md` had been carrying
    /// the same space padding since before it.
    ///
    /// **A tab-carrying line was tried first as its own shape and it does not work.** A page's
    /// claim would then be opt-in from the page: de-tabbing a row removes the tab, so it removes
    /// the claim, and the mutation stays green - measured. Structure is what closes it. Everything
    /// after the stamp in a `query`'s fence IS its answer, so the run reaches the end of the fence
    /// and a row cannot leave the claim by being edited.
    ///
    /// **The trade, stated: a page may no longer ELIDE a `query`'s rows.** That is the right
    /// direction here and not a general rule - the six rows on both pages sum to the figure the
    /// prose then reasons about, so a hidden row is a wrong total rather than a shortened example.
    /// A listing this binary pads with SPACES is still prose: `catalog`'s metric block is elided
    /// on purpose, and `compile`'s statement is re-wrapped to fit the page.
    fn claims(fence: &Fence<'_>) -> Vec<Claim> {
        let mut out = Vec::new();
        for (offset, line) in fence.lines.iter().enumerate() {
            let claim = line.trim();
            let at = fence.opened_at + offset + 1;
            if claim.starts_with("refused: ") {
                out.push(Claim {
                    at,
                    lines: block(fence, offset),
                });
                break;
            }
            if claim.starts_with("-- definitions ") {
                out.push(Claim {
                    at,
                    lines: block(fence, offset),
                });
                break;
            }
        }
        out
    }

    /// One fence's lines from `offset` to its last non-empty one, trimmed.
    ///
    /// The two block shapes share it because they are the same fact: what the binary emitted as one
    /// run, held as one run. [`printed`] then requires the lines CONTIGUOUS and IN ORDER, which a
    /// per-line claim does not.
    fn block(fence: &Fence<'_>, offset: usize) -> Vec<String> {
        let mut lines: Vec<String> = fence
            .lines
            .iter()
            .skip(offset)
            .map(|held| String::from(held.trim()))
            .collect();
        let keep = lines.iter().rposition(|held| !held.is_empty()).map_or(0, |last| last + 1);
        lines.truncate(keep);
        lines
    }

    /// Whether one command's output carries a claim's lines as a contiguous run.
    fn printed(said: &str, claim: &[String]) -> bool {
        if claim.is_empty() {
            return false;
        }
        let lines: Vec<&str> = said.lines().map(str::trim).collect();
        lines
            .windows(claim.len())
            .any(|window| window.iter().zip(claim).all(|(shown, held)| *shown == held.as_str()))
    }

    /// Runs one documented invocation from the repository root, as a reader would.
    ///
    /// This shell's own `SUTURA_*` variables are removed for the reason
    /// `tests/declared_source.rs` gives: one environment variable per settings key layers over the
    /// files, so a developer with one exported would be running a different deployment from CI.
    fn run(args: &[String]) -> (Option<i32>, String) {
        let mut command = Command::new(env!("CARGO_BIN_EXE_sutura"));
        for (key, _) in std::env::vars_os() {
            if key.to_string_lossy().starts_with("SUTURA") {
                command.env_remove(key);
            }
        }
        let output = command
            .current_dir(repo_root())
            .args(args)
            .output()
            .expect("the composed binary runs");
        let mut said = String::from_utf8_lossy(&output.stdout).into_owned();
        said.push_str(&String::from_utf8_lossy(&output.stderr));
        (output.status.code(), said)
    }

    /// The subcommands this binary offers, read out of the listing it prints with no arguments.
    ///
    /// DERIVED rather than written down a second time. A renamed or removed subcommand is red in
    /// `every_subcommand_this_harness_classifies_is_one_the_binary_offers` instead of turning every
    /// fence that prints it into a silent skip, and a documented name this binary does not have is
    /// a failure naming what it does have. The parse takes a name at exactly two spaces of indent,
    /// which is the listing's shape and not a contract - so its caller requires every name in
    /// `RUNNABLE` and `SKIPPED` to come back, and a reshaped listing is a red test rather than an
    /// empty set that classifies nothing.
    fn subcommands() -> Vec<String> {
        let (_, said) = run(&[]);
        said.lines()
            .filter_map(|line| {
                let name = line.strip_prefix("  ")?;
                if name.starts_with(' ') {
                    return None;
                }
                name.split_whitespace().next().map(String::from)
            })
            .collect()
    }

    /// One command block and what it printed, which is what the fence below it is held against.
    struct Above {
        at: usize,
        said: String,
        runs: usize,
        /// The venue covering each command in it this harness did not run, so a fence attributed
        /// to one says where to look rather than only that nothing ran.
        elsewhere: Vec<&'static str>,
    }

    /// Runs one page's commands and holds every line it prints as output against the command it was
    /// printed UNDER. Returns how many ran, and how many claims were held.
    ///
    /// Attribution is the point. Pooling every command's output into one string and asking whether
    /// ANY of them printed a line lets a page put one command's output under another and stay
    /// green, which is the same class of drift this file was written to end: on
    /// `docs/getting-started.md` the refusal command's question file could be changed to a
    /// different refusal and the block below it left alone, because the other page printed that
    /// block from its own command and satisfied the assertion out of the pool.
    fn hold_page(rel: &str, text: &str, known: &[String]) -> (usize, usize) {
        let mut ran = 0_usize;
        let mut held = 0_usize;
        let mut above: Option<Above> = None;
        for fence in fences(rel, text) {
            let found = invocations(rel, &fence, known);
            if found.is_empty() {
                held += hold_output(rel, &fence, above.as_ref());
                continue;
            }
            let mut said = String::new();
            let mut runs = 0_usize;
            let mut elsewhere = Vec::new();
            for invocation in found {
                match invocation {
                    Invocation::Elsewhere(venue) => elsewhere.push(venue),
                    Invocation::Run(args) => {
                        let (code, shown) = run(&args);
                        assert_eq!(
                            code,
                            Some(0),
                            "{rel}:{} prints `sutura {}`, which exited {code:?}:\n{shown}",
                            fence.opened_at,
                            args.join(" ")
                        );
                        said.push_str(&shown);
                        runs += 1;
                        ran += 1;
                    }
                }
            }
            above = Some(Above {
                at: fence.opened_at,
                said,
                runs,
                elsewhere,
            });
        }
        (ran, held)
    }

    /// Holds one output fence against the command fence above it, and says how many claims it made.
    fn hold_output(rel: &str, fence: &Fence<'_>, above: Option<&Above>) -> usize {
        let mut held = 0_usize;
        for claim in claims(fence) {
            let Some(command) = above else {
                panic!("{rel}:{} prints output above every command on the page", claim.at);
            };
            assert!(
                command.runs > 0,
                "{rel}:{} prints output under the fence at {rel}:{}, whose commands this harness does not run - {:?} covers them",
                claim.at,
                command.at,
                command.elsewhere
            );
            assert!(
                printed(&command.said, &claim.lines),
                "{rel}:{} prints this as output:\n{}\nand the command in the fence at {rel}:{} printed:\n{}",
                claim.at,
                claim.lines.join("\n"),
                command.at,
                command.said
            );
            held += 1;
        }
        held
    }

    #[test]
    fn every_subcommand_this_harness_classifies_is_one_the_binary_offers() {
        let known = subcommands();
        // Fail closed: a listing that parsed as nothing, or as fewer names than are classified
        // here, classifies nothing - and would turn every unrecognised command into a skip.
        assert!(
            known.len() > RUNNABLE.len(),
            "the usage listing parsed as {known:?}, which is not this binary's subcommands - its shape changed and this parse did not"
        );
        for name in RUNNABLE {
            assert!(
                known.iter().any(|offered| offered == name),
                "`RUNNABLE` names `{name}`, which this binary does not offer: {known:?}"
            );
        }
        for &(name, venue) in SKIPPED {
            assert!(
                known.iter().any(|offered| offered == name),
                "`SKIPPED` names `{name}`, covered by {venue}, which this binary does not offer: {known:?}"
            );
        }
    }

    #[test]
    fn every_command_the_pages_print_runs_and_every_line_of_output_is_one_it_printed() {
        let known = subcommands();
        for rel in PAGES {
            let (ran, held) = hold_page(rel, &page(rel), &known);
            // Both floors are PER PAGE. A tree-wide one is satisfied by the other page, which is
            // how a page that lost every command, or every output line, would stay green.
            assert!(
                ran > 0,
                "no `sutura` invocation ran out of {rel} - the extractor is broken, not the page"
            );
            assert!(
                held > 0,
                "no refusal or provenance line read out of {rel} - the scan is broken, not the page"
            );
        }
    }

    #[test]
    fn every_definitions_digest_in_prose_is_the_one_the_catalog_reports() {
        let (code, said) = run(&[String::from("catalog"), String::from("examples/single-player/catalog")]);
        assert_eq!(code, Some(0), "the example catalog lists:\n{said}");
        let digest = said
            .lines()
            .find_map(|line| line.strip_prefix("digest "))
            .expect("the listing stamps a digest")
            .trim();
        assert_eq!(digest.len(), 64, "a digest is 64 hex characters: {digest}");

        let mut held = 0_usize;
        for rel in PAGES.iter().chain(DIGEST_PAGES) {
            for (number, line) in page(rel).lines().enumerate() {
                for word in line.split(|c: char| !c.is_ascii_hexdigit()) {
                    if word.len() != 64 {
                        continue;
                    }
                    held += 1;
                    assert_eq!(
                        word,
                        digest,
                        "{rel}:{} states a definitions digest the example does not have",
                        number + 1
                    );
                }
            }
        }
        assert!(held > 0, "no digest read out of any page - the scan is broken, not the pages");
    }

    /// The triples a release publishes, read out of [`RELEASE`]'s `TARGETS`.
    ///
    /// DERIVED rather than written down here, for the reason [`subcommands`] is derived: a second
    /// copy of the set is a second thing to keep true, and it would rot in the direction that
    /// matters - a page naming an asset no release produces is a reader whose first command 404s.
    /// That literal is the authority a test can reach without evaluating a flake, and it is
    /// already reconciled inside its own file: `publish` asserts that exactly these four arrived,
    /// and `strategy.matrix.target` is held against it there.
    ///
    /// Read as a YAML block scalar - the more-indented single-word lines under the key, stopping
    /// at the first line that is neither. **A reshaped literal yields fewer than four**, which its
    /// caller turns into a red test rather than an empty set that would accept any asset name at
    /// all.
    fn published_triples() -> Vec<String> {
        let text = page(RELEASE);
        let mut lines = text.lines();
        let key = lines
            .by_ref()
            .find(|line| line.trim_start().starts_with("TARGETS:"))
            .unwrap_or_else(|| panic!("{RELEASE} declares no TARGETS: the published set moved"));
        let indent = key.len() - key.trim_start().len();
        lines
            .take_while(|line| {
                let trimmed = line.trim_start();
                line.len() - trimmed.len() > indent && trimmed.split_whitespace().count() == 1
            })
            .map(|line| String::from(line.trim()))
            .collect()
    }

    /// Every binary tarball a page names, as the BINARY and the triple in its name.
    ///
    /// A `.sha256` or `.sigstore.json` sidecar is the same asset, so all three shapes reduce to
    /// one pair, and anything else ending `.tar.gz` reduces to a name no release publishes.
    ///
    /// **Both halves, and the binary is why.** This returned the triple alone first, folding
    /// `sutura-serve-<triple>` in with `sutura-<triple>` - and review defeated it in one edit:
    /// swap every asset name on the page to `sutura-serve-*` and the suite stayed green over a
    /// page that installs the HTTP SERVER and then runs `sutura catalog` on it. A release
    /// publishes both binaries at all four triples, so the triple was never the half that says
    /// which one a reader downloads.
    ///
    /// The split is on the characters a file name cannot hold, so a name inside a table cell, a
    /// backtick span or a shell quote is read the same way.
    fn named_assets(text: &str) -> Vec<(String, String)> {
        text.split(|c: char| c.is_whitespace() || "`|()'\",".contains(c))
            .filter_map(|word| {
                let asset = word
                    .strip_suffix(".sha256")
                    .or_else(|| word.strip_suffix(".sigstore.json"))
                    .unwrap_or(word);
                let name = asset.strip_prefix("sutura")?.strip_suffix(".tar.gz")?;
                let (binary, triple) = match name.strip_prefix("-serve-") {
                    Some(triple) => ("sutura-serve", triple),
                    None => ("sutura", name.strip_prefix('-')?),
                };
                Some((String::from(binary), String::from(triple)))
            })
            .collect()
    }

    /// The logical lines of the first fence a heading opens, or a panic naming which half is gone.
    ///
    /// **Both absences are failures and they read differently on purpose.** A heading nobody wrote
    /// and a heading with no fence under it are two different edits, and a helper that answered
    /// "nothing" for either would turn its caller's every step assertion vacuous - which is exactly
    /// the state deleting the install fence used to leave this suite in.
    fn fence_under(text: &str, heading: &str) -> Vec<String> {
        let at = text
            .lines()
            .position(|line| line.trim_end() == heading)
            .unwrap_or_else(|| panic!("{GUIDED} carries no `{heading}` heading"))
            + 1;
        let block = fences(GUIDED, text)
            .into_iter()
            .find(|fence| fence.opened_at > at)
            .unwrap_or_else(|| panic!("{GUIDED} opens no fence under `{heading}`"));
        logical_lines(&block.lines)
    }

    /// Every fenced line on a page, with the line its fence opened at.
    fn fenced(rel: &str, text: &str) -> Vec<(usize, String)> {
        fences(rel, text)
            .into_iter()
            .flat_map(|fence| {
                let at = fence.opened_at;
                logical_lines(&fence.lines).into_iter().map(move |line| (at, line))
            })
            .collect()
    }

    /// The page a stranger is sent to leads with a published artefact, and the toolchain is the
    /// contributor's route below it.
    ///
    /// **The defect this is the mechanism for.** `docs/index.md` offered *"Install it and ask a
    /// question"* and sent a reader here, and every runnable command on the page was a
    /// `cargo run -p sutura-cli --`: the page promising an install delivered a source build, named
    /// no release asset, and said nothing about which platforms have one. The release-download
    /// instructions were in `docs/serving.md`, a later chapter about a different surface, and its
    /// recipe's second command was `cd examples/single-player` - a directory no release asset
    /// carries.
    ///
    /// **What is held here, and what is held elsewhere.** This asserts the SHAPE of the page: the
    /// guided half names assets a release actually publishes **of the binary it goes on to run**,
    /// carries an install fence whose steps are each named, spells the commands the way somebody
    /// holding one of them types them, mentions no `cargo` at all, and reaches the corpus without a
    /// clone. That the commands then WORK is
    /// [`every_command_the_pages_print_runs_and_every_line_of_output_is_one_it_printed`], which
    /// runs each one - though over a locally built binary rather than the downloaded one, because a
    /// test cannot fetch a release. The artefact itself is `just shipped` and the release
    /// workflow's own smoke tests.
    ///
    /// **Three mutations left the first version of this green, and each one is now a named
    /// assertion rather than a consequence of one.** The whole `## Install it` fence deleted -
    /// download, both verifications, the extraction, the `PATH` install - and the suite stayed
    /// 66/66, because nothing asked what was in a fence. Every asset name swapped to
    /// `sutura-serve-*`, green, because the parse folded the server in with the tool. And
    /// `cd /tmp && git clone …` on one line, green, because the clone check read a prefix. See
    /// [`STEPS`], [`named_assets`] and [`CLONE`].
    ///
    /// **The fourth mutation was not one this test can hold**, and it is why [`claims`] grew a
    /// third shape instead: this change rewrote seven real tabs under a `query` to space padding,
    /// and the suite stayed 66/66 because a listing's rows were outside every claim. A tab-carrying
    /// line is held now. A listing the binary pads with SPACES still is not.
    #[test]
    fn the_guided_path_installs_a_published_binary_rather_than_a_toolchain() {
        let text = page(GUIDED);

        // The page names an artefact OF THE BINARY IT GOES ON TO RUN. First, because it is the
        // whole defect: without one, the only way to run anything the page prints is to build the
        // tree. Scoped to `sutura` because a release publishes `sutura-serve` at the same four
        // triples, and a page that downloads the server and then runs `sutura catalog` on it is
        // the mutation review defeated the first version of this test with.
        let named = named_assets(&text);
        let ours: Vec<String> = named
            .iter()
            .filter(|(binary, _)| binary == CLI)
            .map(|(_, triple)| triple.clone())
            .collect();
        assert!(
            !ours.is_empty(),
            "{GUIDED} names no `{CLI}-<triple>.tar.gz` release asset - it names {named:?}, so \
             nothing on it can be run without building the tree, which is a Rust toolchain a \
             reader was never told they needed"
        );

        // And the assets it names are the ones a release produces - every one of them, in both
        // directions, so the platform table cannot be a subset a reader takes for the whole.
        let published = published_triples();
        // FAIL CLOSED on the parse, and NON-EMPTY rather than a count: a triple added to or
        // dropped from a release is a legitimate change, and gating on today's number would report
        // a correct tree under a diagnosis about this parser - whose cheapest fix is editing the
        // expectation. The guard survives without the number because `ours` is non-empty by here,
        // so a parse that yielded nothing fails the second containment loop below instead. What a
        // NEW triple has to do is reach the page, which is the first one.
        assert!(
            !published.is_empty(),
            "{RELEASE} declares a TARGETS this parse read as no triples at all - its shape changed \
             and this parse did not"
        );
        for triple in &published {
            // The platform claim, measured rather than written on the page: every published
            // artefact is Linux. A darwin or windows triple here is a red test until the page
            // stops saying otherwise.
            assert!(
                triple.contains("-linux-"),
                "{RELEASE} publishes `{triple}`, so {GUIDED} may no longer say every asset is Linux"
            );
            assert!(
                ours.contains(triple),
                "{RELEASE} publishes `{CLI}-{triple}.tar.gz` and {GUIDED} names {ours:?} - a \
                 reader on that platform is left to find out by failing"
            );
        }
        for (binary, triple) in &named {
            assert!(
                published.contains(triple),
                "{GUIDED} names `{binary}-{triple}.tar.gz`, which no release publishes: {published:?}"
            );
        }

        // The install fence, and the steps a reader copies out of it. **Deleting the whole fence
        // left the first version of this test green** - download, checksum, attestation, extract
        // and install all gone, and the page's honesty about verification held by nothing. Which is
        // the shape of a gate that reports `ok` over a subject that emitted nothing, so the fence
        // is located by its heading and its steps are named one at a time.
        let install = fence_under(&text, INSTALL);
        for &(step, why) in STEPS {
            assert!(
                install.iter().any(|line| line.contains(step)),
                "{GUIDED}'s `{INSTALL}` fence has no `{step}` step, so {why}. It prints {install:?}"
            );
        }
        let asset = |line: &String| {
            published
                .iter()
                .any(|triple| line.contains(&format!("{CLI}-{triple}.tar.gz")))
        };
        assert!(
            install.iter().any(asset),
            "{GUIDED}'s `{INSTALL}` fence names no published `{CLI}` asset, so its steps run on \
             nothing: {install:?}"
        );

        // Where the guided path ends. Exactly one heading, so the walk below cannot be vacuous.
        let heading: Vec<usize> = text
            .lines()
            .enumerate()
            .filter(|(_, line)| line.trim_end() == CONTRIBUTOR)
            .map(|(index, _)| index + 1)
            .collect();
        assert_eq!(
            heading.len(),
            1,
            "{GUIDED} carries {} `{CONTRIBUTOR}` headings - the source build is the contributor's \
             route and belongs under exactly one",
            heading.len()
        );
        let opens_at = heading[0];

        let mut guided = 0_usize;
        let mut source = 0_usize;
        for (at, line) in fenced(GUIDED, &text) {
            for needle in CLONE {
                assert!(
                    !line.contains(needle),
                    "{GUIDED}:{at} prints `{line}`, which carries `{needle}` - the corpus is taken \
                     as a tarball so that the guided path needs no git and no toolchain"
                );
            }
            if at < opens_at {
                assert!(
                    !line.contains("cargo"),
                    "{GUIDED}:{at} prints `{line}` above `{CONTRIBUTOR}`, and everything above it \
                     must run on a downloaded artefact"
                );
                if line.trim_start().starts_with("sutura ") {
                    guided = guided.saturating_add(1);
                }
            } else if line.contains(SEPARATOR) {
                source = source.saturating_add(1);
            }
        }
        assert!(
            guided > 0,
            "{GUIDED} prints no `sutura <args>` command above `{CONTRIBUTOR}` - that spelling is \
             what a reader holding the published binary types"
        );
        assert!(
            source > 0,
            "{GUIDED} prints no `cargo run{SEPARATOR}` command under `{CONTRIBUTOR}` - the source \
             build is moved and relabelled there, not deleted"
        );

        // Download, VERIFY, run. The page does not repeat what a signature does and does not say,
        // and the page that does is the one it has to send a reader to.
        assert!(
            text.contains("verifying-a-release.md"),
            "{GUIDED} tells a reader to download an artefact and links no verification page"
        );
    }
}
