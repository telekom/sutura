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
//! **What is asserted, and what is deliberately not.** Three things, each of them something a
//! reader would copy as fact:
//!
//! * every documented invocation RUNS, and earns the exit status the page's prose claims for it;
//! * every line a page prints that starts with `refused:` or `-- definitions ` is a line the
//!   binary actually printed - those two are the refusal's identity and the provenance stamp;
//! * every definitions digest written in prose is the digest the catalog reports.
//!
//! Not the whole of a captured block. The pages ELIDE on purpose - `docs/getting-started.md` says
//! so where it prints five metrics of eleven, and it wraps a statement that the binary emits on
//! one line - so asserting a block verbatim would fail correct prose. The rows themselves are
//! pinned by `tests/example.rs`'s snapshots, which is the right place for them.
//!
//! Three kinds of documented command are skipped, each with a venue that covers it: a
//! `sutura-serve` invocation (`crates/sutura-serve/tests/served.rs`), `sutura mcp`, which speaks a
//! protocol on its own pipes and would block on this harness's stdin (`tests/mcp.rs`), and a
//! `curl` against a running service (`served.rs` again).

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

    /// The repository root, which is the working directory every documented command assumes: the
    /// paths in them are repo-relative, exactly as a reader at a clone types them.
    fn repo_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    fn page(rel: &str) -> String {
        let path = repo_root().join(rel);
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{} is readable: {e}", path.display()))
    }

    /// Every fenced block in a page, as its lines.
    ///
    /// The info string is dropped: what matters is that a fence is a block, and a `bash` block
    /// holds the commands while a `text` one holds what they printed.
    fn fences(text: &str) -> Vec<Vec<&str>> {
        let mut blocks = Vec::new();
        let mut current: Option<Vec<&str>> = None;
        for line in text.lines() {
            if line.trim_start().starts_with("```") {
                match current.take() {
                    Some(block) => blocks.push(block),
                    None => current = Some(Vec::new()),
                }
                continue;
            }
            if let Some(block) = current.as_mut() {
                block.push(line);
            }
        }
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

    /// The subcommands this file runs, and the exit status the pages claim for each.
    ///
    /// A refusal is a RESULT: `docs/getting-started.md` says so where it prints one, and the
    /// status is `0`. Every other documented invocation answers.
    const RUNNABLE: &[&str] = &["catalog", "describe", "compile", "query"];

    /// One documented invocation: where it was written, and the arguments to pass.
    struct Documented {
        page: String,
        args: Vec<String>,
    }

    /// Every invocation of this binary a page prints, with shell variables it also sets expanded.
    ///
    /// The two documented spellings are `cargo run -p sutura-cli -- <args>`, which is what a
    /// reader at a clone runs, and `sutura <args>`, which is what a reader with the published
    /// binary runs. An assignment inside the same block (`E=examples/single-player`) is expanded,
    /// because one block uses it to keep three commands on one line each.
    fn documented(rel: &str, text: &str) -> Vec<Documented> {
        let mut found = Vec::new();
        for block in fences(text) {
            let mut vars: Vec<(String, String)> = Vec::new();
            for line in logical_lines(&block) {
                if let Some((name, value)) = line.split_once('=')
                    && !name.is_empty()
                    && name.chars().all(|c| c.is_ascii_uppercase() || c == '_')
                    && !value.contains(' ')
                {
                    vars.push((format!("${name}"), String::from(value)));
                    continue;
                }
                let Some(tail) = line
                    .strip_prefix("cargo run -p sutura-cli -- ")
                    .or_else(|| line.strip_prefix("sutura "))
                else {
                    continue;
                };
                let args: Vec<String> = tail
                    .split_whitespace()
                    .map(|word| {
                        vars.iter()
                            .fold(String::from(word), |acc, (name, value)| acc.replace(name, value))
                    })
                    .collect();
                if args.first().is_some_and(|first| RUNNABLE.contains(&first.as_str())) {
                    found.push(Documented {
                        page: String::from(rel),
                        args,
                    });
                }
            }
        }
        found
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

    /// Everything the documented commands printed, and how many there were.
    fn documented_output() -> (String, usize) {
        let mut all = String::new();
        let mut count = 0_usize;
        for rel in PAGES {
            let text = page(rel);
            let commands = documented(rel, &text);
            assert!(
                !commands.is_empty(),
                "no `sutura` invocation read out of {rel} - the extractor is broken, not the page"
            );
            for command in commands {
                let (code, said) = run(&command.args);
                assert_eq!(
                    code,
                    Some(0),
                    "{} prints `sutura {}`, which exited {code:?}:\n{said}",
                    command.page,
                    command.args.join(" ")
                );
                all.push_str(&said);
                count = count.saturating_add(1);
            }
        }
        (all, count)
    }

    #[test]
    fn every_documented_command_runs() {
        let (_, count) = documented_output();
        // Fail closed: a scan that read nothing asserts nothing, and the count is the only thing
        // that tells that apart from a tree whose pages agree.
        assert!(count > 0, "no documented command was run");
    }

    #[test]
    fn every_line_the_documentation_prints_as_output_is_one_the_binary_prints() {
        let (said, _) = documented_output();
        let mut held = 0_usize;
        for rel in PAGES {
            let text = page(rel);
            for block in fences(&text) {
                for line in block {
                    let claim = line.trim();
                    if !claim.starts_with("refused: ") && !claim.starts_with("-- definitions ") {
                        continue;
                    }
                    held = held.saturating_add(1);
                    assert!(
                        said.lines().any(|printed| printed.trim() == claim),
                        "{rel} prints `{claim}` as output, and no documented command printed it:\n{said}"
                    );
                }
            }
        }
        assert!(
            held > 0,
            "no refusal or provenance line read out of {PAGES:?} - the scan is broken"
        );
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
                    held = held.saturating_add(1);
                    assert_eq!(
                        word,
                        digest,
                        "{rel}:{} states a definitions digest the example does not have",
                        number.saturating_add(1)
                    );
                }
            }
        }
        assert!(held > 0, "no digest read out of any page - the scan is broken, not the pages");
    }
}
