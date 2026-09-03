//! The claim `github.com/telekom/sutura#121` is about, asked of the BINARY: a catalog whose models
//! name a data system other than `local` is answered, because a `sources:` entry declared it.
//!
//! **It exists because review found the in-crate suite could not make this claim.** Every case in
//! `crates/sutura-cli/src/sources.rs` builds a `SourceRegistry` through
//! `Sources::defaults(..).with_overlay(..)`, so `environment_from_process`,
//! `config_dir_from_process` and the `<dir>/base.yaml` layering - the whole door this change is
//! about - were exercised by nothing at all, and no test set `SUTURA_CONFIG_DIR`. The one that could
//! is a spawned binary, for the reason `crates/sutura-serve/tests/served.rs` gives at its own head:
//! `std::env::set_var` is `unsafe` in this edition and the workspace forbids it, so what a test can
//! decide is what a CHILD sees.
//!
//! # What it runs, and why the catalog is copied
//!
//! `examples/single-player`'s models say `source: local`, which is the one name the built-in
//! declaration answers to - so running the example unchanged proves the fallback rather than the
//! registry. The harness copies that catalog into `CARGO_TARGET_TMPDIR` and rewrites `source: local`
//! to `source: warehouse` in every document, then declares `sources.warehouse` over the example's
//! own data directory. Same data, same certified numbers, a name no constant in this binary knows.
//! The anchors therefore still have to reproduce, which is what makes the answer worth reading: a
//! bundle whose anchor does not hold never gets a service at all.
//!
//! # No network, no port, no credential
//!
//! So it is a gate rather than a `nix run` app - `checks.nextest` runs it, like `tests/mcp.rs` and
//! `tests/served.rs`. `just declared-source` runs this target alone.

// `cfg(test)` for the reason `crates/sutura-cli/tests/example.rs` gives: clippy honours
// `allow-expect-in-tests` only inside a `#[cfg(test)]` item, and without it every `expect` below is
// a lint error.
#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::process::Command;

    /// The example deployment this suite reads its catalog and data from.
    fn example() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/single-player")
    }

    /// A scratch directory of this test's own, emptied first so a rerun starts clean.
    ///
    /// `CARGO_TARGET_TMPDIR` is defined for an integration target and is inside `target/`, so
    /// nothing here writes into the repository or into a shared system temp directory two checks
    /// could collide in.
    fn scratch(name: &str) -> PathBuf {
        let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
        drop(std::fs::remove_dir_all(&dir));
        std::fs::create_dir_all(&dir).expect("a scratch directory is creatable");
        dir
    }

    /// The example's catalog, copied with every model's source renamed.
    ///
    /// A rewrite of the frontmatter rather than a hand-written catalog: what has to be true is that
    /// a REAL bundle - the one whose anchors this repository certifies - is served under a different
    /// source name, and a fixture written here would be a fixture rather than that bundle.
    fn catalog_naming(source: &str, into: &Path) -> PathBuf {
        let catalog = into.join("catalog");
        // RECURSIVELY, because the example catalog is `models/`, `metrics/`, `relationships/` and
        // `knowledge/` rather than a flat directory - a first version of this helper copied the top
        // level only and the binary answered "this catalog holds no documents", which is the
        // adapter's own refusal doing its job.
        copy_rewriting(&example().join("catalog"), &catalog, source);
        catalog
    }

    /// Copies one directory tree, rewriting every document's declared source on the way.
    fn copy_rewriting(from: &Path, to: &Path, source: &str) {
        std::fs::create_dir_all(to).expect("a catalog directory is creatable");
        let read = std::fs::read_dir(from).expect("the example catalog is readable");
        for entry in read {
            let entry = entry.expect("a directory entry is readable");
            let kind = entry.file_type().expect("a file type is readable");
            if kind.is_dir() {
                copy_rewriting(&entry.path(), &to.join(entry.file_name()), source);
                continue;
            }
            // `is_symlink()` and not `!is_file()`: `clippy::filetype_is_file` is denied here, and its
            // reason is the right one - a negated `is_file` also skips a symlink to a document, and
            // the example catalog is a tree somebody may well symlink into. Directory, symlink,
            // otherwise a file.
            if kind.is_symlink() {
                continue;
            }
            let text = std::fs::read_to_string(entry.path()).expect("an example document is readable");
            std::fs::write(
                to.join(entry.file_name()),
                text.replace("source: local", &format!("source: {source}")),
            )
            .expect("a copied document is writable");
        }
    }

    /// A configuration directory declaring one `files` source over the example's data.
    ///
    /// `security.identity: single-user` with its own reason, which is what lets the entry declare
    /// `shared-service-user` and take the mode's acknowledgement as its witness - the honest shape
    /// for a command a person runs against files they already have.
    fn config_declaring(source: &str, into: &Path) -> PathBuf {
        let dir = into.join("conf");
        std::fs::create_dir_all(&dir).expect("a configuration directory is creatable");
        let data = example()
            .join("data")
            .canonicalize()
            .expect("the example's data directory is there");
        std::fs::write(
            dir.join("base.yaml"),
            format!(
                "security:\n  identity: single-user\n  single_user_because: \"one developer, one \
                 laptop, one set of files\"\nsources:\n  {source}:\n    kind: files\n    data_dir: \
                 \"{}\"\n    posture: shared-service-user\n",
                data.display()
            ),
        )
        .expect("a settings file is writable");
        dir
    }

    /// The command, with this shell's own `SUTURA_*` variables removed.
    ///
    /// **Not cosmetic**, and the same reason `tests/served.rs` gives at its own copy of this: the
    /// settings tree layers one environment variable per key on top of the files, so a developer
    /// with `SUTURA__SERVER__HOST` exported would be running a different deployment from CI and the
    /// failure would name a setting nobody wrote in this test.
    fn command(config_dir: Option<&Path>) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_sutura"));
        for (key, _) in std::env::vars_os() {
            if key.to_string_lossy().starts_with("SUTURA") {
                command.env_remove(key);
            }
        }
        command.env(sutura_config::ENVIRONMENT_VARIABLE, "development");
        match config_dir {
            Some(dir) => command.env(sutura_config::CONFIG_DIR_VARIABLE, dir),
            None => command.env_remove(sutura_config::CONFIG_DIR_VARIABLE),
        };
        command
    }

    /// What one invocation produced: the exit code, and both streams.
    struct Ran {
        code: Option<i32>,
        stdout: String,
        stderr: String,
    }

    impl Ran {
        /// Both streams together, for a failure message that does not hide the half that explains it.
        fn output(&self) -> String {
            format!(
                "code {:?}\n--- stdout ---\n{}\n--- stderr ---\n{}",
                self.code, self.stdout, self.stderr
            )
        }
    }

    /// Runs `sutura` with these arguments and collects everything it said.
    fn run(config_dir: Option<&Path>, args: &[&str]) -> Ran {
        let output = command(config_dir).args(args).output().expect("the composed binary runs");
        Ran {
            code: output.status.code(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        }
    }

    /// The example's own certified question, written where this suite can pass it as a path.
    fn question(into: &Path) -> PathBuf {
        let path = into.join("question.yaml");
        std::fs::write(
            &path,
            "metric: recurring_revenue\ngrain: month\nrange:\n  start: 2026-01-01\n  end: 2026-07-01\n",
        )
        .expect("a question file is writable");
        path
    }

    #[test]
    fn a_catalog_naming_a_source_other_than_local_is_answered_by_the_cli() {
        // **THE claim of issue 121, on the binary a release publishes.** Before it, this catalog was
        // refused outright: the composition root compared the declared source against one constant,
        // so the only catalog the published artifact could answer was one that happened to call its
        // data system `local`. Now the DEPLOYMENT says what `warehouse` is and the answer follows.
        //
        // Every layer this change added is on the path: `environment_from_process`,
        // `config_dir_from_process`, `<dir>/base.yaml`, the `sources:` parse, the kind dispatch, the
        // posture cross-check, the broker built from that entry, and every anchor re-executed
        // against the engine it opened.
        let dir = scratch("declared-source-answered");
        let catalog = catalog_naming("warehouse", &dir);
        let conf = config_declaring("warehouse", &dir);
        let question = question(&dir);

        let ran = run(
            Some(&conf),
            &["query", &catalog.to_string_lossy(), &question.to_string_lossy()],
        );
        assert_eq!(ran.code, Some(0), "{}", ran.output());
        // The certified number, not merely a zero exit: `examples/single-player` documents
        // `recurring_revenue` for June 2026 as 202121 minor units and its anchor pins it.
        assert!(ran.stdout.contains("202121"), "{}", ran.output());
        // And the answer carries the bundle's own provenance line, so what was read is the real
        // catalog rather than something this test assembled.
        assert!(ran.stdout.contains("-- definitions"), "{}", ran.output());
        // NO data directory was passed. That is the other half of what the declaration bought: the
        // entry says where the files are, so the third argument is not required any more.
        assert!(!ran.stderr.contains("no data directory was given"), "{}", ran.output());
    }

    #[test]
    fn the_same_catalog_with_no_declaration_is_refused_and_names_the_entry_to_write() {
        // The control that makes the test above mean something. Same catalog, same data, no
        // configuration directory - so the built-in declaration is all there is, and it answers to
        // `local` only. A refusal here is what proves the answer above came from the REGISTRY rather
        // than from a fallback that would take any name.
        let dir = scratch("declared-source-undeclared");
        let catalog = catalog_naming("warehouse", &dir);
        let question = question(&dir);

        let ran = run(
            None,
            &[
                "query",
                &catalog.to_string_lossy(),
                &question.to_string_lossy(),
                &example().join("data").to_string_lossy(),
            ],
        );
        assert_eq!(ran.code, Some(1), "{}", ran.output());
        assert!(ran.stderr.contains("sources.warehouse"), "{}", ran.output());
        assert!(ran.stderr.contains(sutura_config::CONFIG_DIR_VARIABLE), "{}", ran.output());
    }

    #[test]
    fn the_quickstart_line_still_answers_with_a_configuration_directory_present() {
        // **The regression review reproduced**, as a test on the binary. `sutura query <catalog-dir>
        // <question.yaml> <data-dir>` is the line `README.md` and `docs/getting-started.md` both
        // print, and an operator who also runs `sutura-serve` has `SUTURA_CONFIG_DIR` exported - so
        // refusing the documented command for having a configuration directory broke the quickstart
        // for exactly the people most likely to have one. Two answers that AGREE are one answer.
        let dir = scratch("declared-source-quickstart");
        let conf = config_declaring("local", &dir);
        let question = question(&dir);

        let ran = run(
            Some(&conf),
            &[
                "query",
                &example().join("catalog").to_string_lossy(),
                &question.to_string_lossy(),
                &example().join("data").to_string_lossy(),
            ],
        );
        assert_eq!(ran.code, Some(0), "{}", ran.output());
        assert!(ran.stdout.contains("202121"), "{}", ran.output());
    }

    #[test]
    fn a_directory_that_disagrees_with_the_declared_one_is_refused_and_offers_a_remedy_that_works() {
        // The other side of that: a DIFFERENT directory really is two answers to one question, and
        // the remedy has to be one that works. Review reproduced a closed loop in the old message -
        // "remove the entry", which then got the undeclared-source refusal telling you to put it
        // back - so the only remedy offered now is dropping the argument, and this asserts both that
        // it is named and that the wrong one is gone.
        let dir = scratch("declared-source-disagreement");
        let catalog = catalog_naming("warehouse", &dir);
        let conf = config_declaring("warehouse", &dir);
        let question = question(&dir);

        let ran = run(
            Some(&conf),
            &[
                "query",
                &catalog.to_string_lossy(),
                &question.to_string_lossy(),
                // The catalog directory, which is a real directory and is not the data one.
                &catalog.to_string_lossy(),
            ],
        );
        assert_eq!(ran.code, Some(1), "{}", ran.output());
        assert!(ran.stderr.contains("two answers to one question"), "{}", ran.output());
        assert!(ran.stderr.contains("Drop the argument"), "{}", ran.output());
        assert!(!ran.stderr.contains("remove the"), "{}", ran.output());
    }

    #[test]
    fn prompt_and_query_resolve_the_same_configuration_directory() {
        // **One binary reading the deployment configuration from two places, reproduced by review.**
        // `sutura prompt <catalog>` took its directory from a positional and never looked at the
        // variable, while `sutura query` reads the variable - so on one machine, at one moment,
        // `prompt` printed `configuration from embedded defaults only` and `query` refused on the
        // content of the directory that variable named. Two subcommands resolving different settings
        // is exactly what exporting the variable name from `sutura_config` was meant to stop.
        //
        // `prompt` writes its provenance line to standard ERROR on purpose - its standard output is
        // piped into an agent's configuration - so that is where this reads it.
        let dir = scratch("declared-source-one-door");
        let conf = config_declaring("local", &dir);

        let ran = run(Some(&conf), &["prompt", &example().join("catalog").to_string_lossy()]);
        assert_eq!(ran.code, Some(0), "{}", ran.output());
        assert!(
            ran.stderr.contains(&conf.to_string_lossy().into_owned()),
            "prompt must resolve the directory the variable names: {}",
            ran.output()
        );
        // The positional still wins where one is given, which is the half that must not regress: a
        // caller who names a directory is naming it.
        let other = config_declaring("local", &scratch("declared-source-one-door-positional"));
        let ran = run(
            Some(&conf),
            &[
                "prompt",
                &example().join("catalog").to_string_lossy(),
                &other.to_string_lossy(),
            ],
        );
        assert_eq!(ran.code, Some(0), "{}", ran.output());
        assert!(
            ran.stderr.contains(&other.to_string_lossy().into_owned()),
            "an explicit directory must beat the variable: {}",
            ran.output()
        );
    }

    #[test]
    fn a_serving_refusal_stops_this_command_and_says_whose_refusal_it_is() {
        // **The limit review found stated nowhere, pinned.** `Settings::refusals` is a SERVER's
        // refusal set, and `Settings::load` is the one door - which is deliberate, because a second
        // weaker door could refuse differently from the service's. The consequence is real:
        // `SUTURA_ENVIRONMENT=production` stops `sutura query` on an access token, from a command
        // that binds nothing, and that worked before this door existed.
        //
        // So what is asserted is not the refusal - it is that the message says whose it is and names
        // the two variables that make the command answer again. A refusal an operator cannot act on
        // is the defect, and this is what stops the wording regressing to one.
        let dir = scratch("declared-source-production");
        let conf = config_declaring("local", &dir);
        let question = question(&dir);

        let mut command = command(Some(&conf));
        command.env(sutura_config::ENVIRONMENT_VARIABLE, "production");
        let output = command
            .args([
                "query",
                &example().join("catalog").to_string_lossy(),
                &question.to_string_lossy(),
            ])
            .output()
            .expect("the composed binary runs");
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        assert_eq!(output.status.code(), Some(1), "{stderr}");
        assert!(
            stderr.contains("binds no listener of its own"),
            "the refusal must say whose refusal it is: {stderr}"
        );
        assert!(
            stderr.contains(sutura_config::CONFIG_DIR_VARIABLE) && stderr.contains(sutura_config::ENVIRONMENT_VARIABLE),
            "the refusal must name what to change: {stderr}"
        );
    }
}
